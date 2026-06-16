use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::app_state::{AppConfig, AppState};
use crate::error::{AppError, AppResult};
use crate::liftoff::{assets as liftoff_assets, geometry as liftoff_geometry};
use crate::processing::collisions::CollisionEvent;
use crate::processing::job::{JobStatus, ProcessingProgress};
use crate::processing::pipeline::{self, PipelineSummary};
use crate::processing::profile::{DEFAULT_PROFILE_ID, PROCESSOR_VERSION};
use crate::processing::segmentation::{self, SegmentationConfig};
use crate::processing::timing::{self, TimingRows};
use crate::storage::db::DbPool;
use crate::storage::repositories::{
    self, ProcessedDatasetRow, ProcessingJobRow, ProcessingProfileRow, RaceSessionRow,
    SessionTimingDetail,
};
use crate::telemetry::liftoff_schema::LiftoffSchema;
use crate::telemetry::sample::TelemetrySample;

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessCaptureRequest {
    pub capture_id: String,
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessCaptureResponse {
    pub job: ProcessingJobRow,
    pub dataset: ProcessedDatasetRow,
    pub summary: PipelineSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct DatasetDetail {
    pub dataset: ProcessedDatasetRow,
    pub summary: PipelineSummary,
    pub samples: Vec<SamplePoint>,
    pub collision_events: Vec<CollisionEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplePoint {
    pub capture_time_seconds: f64,
    pub speed: f32,
    pub throttle: Option<f32>,
    /// World position [x, y, z] in meters (Liftoff/Unity, Y-up). `None` for
    /// samples without position, or sample files written before this field
    /// existed (see `#[serde(default)]`).
    #[serde(default)]
    pub pos: Option<[f32; 3]>,
}

#[tauri::command]
pub async fn list_processing_profiles(
    state: State<'_, AppState>,
) -> AppResult<Vec<ProcessingProfileRow>> {
    let conn = state.db.get()?;
    repositories::list_profiles(&conn)
}

#[tauri::command]
pub async fn process_capture(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: ProcessCaptureRequest,
) -> AppResult<ProcessCaptureResponse> {
    run_processing(
        app,
        state.db.clone(),
        state.app_config.clone(),
        req.capture_id,
        req.profile_id,
    )
    .await
}

/// Core processing pipeline shared by the `process_capture` command and the
/// automatic post-capture trigger in `stop_capture`. Takes owned, cloneable
/// handles (rather than borrowing `State`) so it can run inside a spawned
/// background task. Emits the same `processing_*` events as the command.
pub async fn run_processing(
    app: tauri::AppHandle,
    db: DbPool,
    app_config: Arc<RwLock<AppConfig>>,
    capture_id: String,
    profile_id: Option<String>,
) -> AppResult<ProcessCaptureResponse> {
    let profile_id = profile_id.unwrap_or_else(|| DEFAULT_PROFILE_ID.to_string());

    let (capture_row, profile_row) = {
        let conn = db.get()?;
        let cap = repositories::get_capture(&conn, &capture_id)?
            .ok_or_else(|| AppError::NotFound(capture_id.clone()))?;
        let profile = repositories::get_profile(&conn, &profile_id)?
            .ok_or_else(|| AppError::NotFound(profile_id.clone()))?;
        (cap, profile)
    };

    let job_id = format!("job_{}", uuid::Uuid::new_v4().simple());
    let dataset_id = format!("ds_{}", uuid::Uuid::new_v4().simple());
    let started_at = Utc::now();

    let mut job = ProcessingJobRow {
        id: job_id.clone(),
        capture_id: capture_row.id.clone(),
        profile_id: profile_row.id.clone(),
        status: JobStatus::Running.as_str().into(),
        started_at: Some(started_at),
        completed_at: None,
        processor_version: PROCESSOR_VERSION.into(),
        input_capture_hash: capture_row.capture_hash.clone(),
        output_dataset_id: None,
        error_message: None,
    };

    {
        let conn = db.get()?;
        repositories::insert_job(&conn, &job)?;
    }
    let _ = app.emit("processing_started", &job);

    let raw_path = PathBuf::from(&capture_row.raw_file_path);
    let expected_hash = capture_row.capture_hash.clone();
    let endpoint = {
        let cfg = app_config
            .read()
            .map_err(|_| AppError::InvalidState("app config mutex poisoned".into()))?;
        format!("{}:{}", cfg.udp_bind_addr, cfg.udp_port)
    };
    let schema = LiftoffSchema::canonical(&endpoint);

    let app_for_progress = app.clone();
    let job_for_progress = job_id.clone();
    let pipeline_result = tokio::task::spawn_blocking(move || {
        pipeline::run_pipeline(
            &raw_path,
            expected_hash.as_deref(),
            &schema,
            move |processed| {
                let _ = app_for_progress.emit(
                    "processing_progress",
                    ProcessingProgress {
                        job_id: job_for_progress.clone(),
                        processed_packets: processed,
                        total_packets: 0,
                        warnings_count: 0,
                    },
                );
            },
        )
    })
    .await?;

    let mut pipeline_output = match pipeline_result {
        Ok(output) => output,
        Err(e) => {
            job.status = JobStatus::Failed.as_str().into();
            job.completed_at = Some(Utc::now());
            job.error_message = Some(e.to_string());
            {
                let conn = db.get()?;
                repositories::update_job(&conn, &job)?;
            }
            let _ = app.emit("processing_failed", &job);
            return Err(e);
        }
    };

    {
        let mut conn = db.get()?;
        finalize_sessions_and_collisions(&mut conn, &capture_row.id, &mut pipeline_output)?;
    }

    let summary_json = serde_json::to_string(&pipeline_output.summary)?;

    let dataset = ProcessedDatasetRow {
        id: dataset_id.clone(),
        capture_id: capture_row.id.clone(),
        job_id: job_id.clone(),
        profile_id: profile_row.id.clone(),
        created_at: Utc::now(),
        dataset_version: PROCESSOR_VERSION.into(),
        summary_json: Some(summary_json),
    };

    {
        let mut conn = db.get()?;
        repositories::insert_dataset(&conn, &dataset)?;
        finalize_session_timing(
            &mut conn,
            &dataset.id,
            &capture_row.id,
            &pipeline_output.samples,
            pipeline_output.summary.start_monotonic_ns as f64 / 1_000_000_000.0,
            pipeline_output
                .samples
                .last()
                .map(|s| {
                    s.capture_time_seconds
                        + pipeline_output.summary.start_monotonic_ns as f64 / 1_000_000_000.0
                })
                .unwrap_or(0.0),
        )?;
        job.status = JobStatus::Completed.as_str().into();
        job.completed_at = Some(Utc::now());
        job.output_dataset_id = Some(dataset.id.clone());
        repositories::update_job(&conn, &job)?;
    }

    if let Some(parent) = PathBuf::from(&capture_row.raw_file_path).parent() {
        let samples_path = parent.join(format!("dataset_{}.samples.json", dataset.id));
        let collisions_path = parent.join(format!("dataset_{}.collisions.json", dataset.id));
        let downsampled = downsample(&pipeline_output.samples, 8000);
        let _ = std::fs::write(&samples_path, serde_json::to_vec(&downsampled)?);
        let _ = std::fs::write(
            &collisions_path,
            serde_json::to_vec(&pipeline_output.collision_events)?,
        );
    }

    let _ = app.emit("processing_completed", &job);

    Ok(ProcessCaptureResponse {
        job,
        dataset,
        summary: pipeline_output.summary,
    })
}

#[tauri::command]
pub async fn get_processing_job(
    state: State<'_, AppState>,
    job_id: String,
) -> AppResult<ProcessingJobRow> {
    let conn = state.db.get()?;
    repositories::get_job(&conn, &job_id)?.ok_or_else(|| AppError::NotFound(job_id))
}

#[tauri::command]
pub async fn list_processed_datasets(
    state: State<'_, AppState>,
    capture_id: String,
) -> AppResult<Vec<ProcessedDatasetRow>> {
    let conn = state.db.get()?;
    repositories::list_datasets_for_capture(&conn, &capture_id)
}

#[tauri::command]
pub async fn get_dataset_detail(
    state: State<'_, AppState>,
    dataset_id: String,
) -> AppResult<DatasetDetail> {
    let dataset = {
        let conn = state.db.get()?;
        repositories::get_dataset(&conn, &dataset_id)?
            .ok_or_else(|| AppError::NotFound(dataset_id.clone()))?
    };
    let summary: PipelineSummary = match &dataset.summary_json {
        Some(s) => serde_json::from_str(s)?,
        None => return Err(AppError::InvalidState("dataset has no summary".into())),
    };

    let capture = {
        let conn = state.db.get()?;
        repositories::get_capture(&conn, &dataset.capture_id)?
            .ok_or_else(|| AppError::NotFound(dataset.capture_id.clone()))?
    };
    let samples_path = PathBuf::from(&capture.raw_file_path)
        .parent()
        .map(|p| p.join(format!("dataset_{}.samples.json", dataset.id)));
    let samples = match samples_path {
        Some(p) if p.exists() => {
            let bytes = std::fs::read(&p)?;
            serde_json::from_slice(&bytes).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let collision_events_path = PathBuf::from(&capture.raw_file_path)
        .parent()
        .map(|p| p.join(format!("dataset_{}.collisions.json", dataset.id)));
    let collision_events = match collision_events_path {
        Some(p) if p.exists() => {
            let bytes = std::fs::read(&p)?;
            serde_json::from_slice(&bytes).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    Ok(DatasetDetail {
        dataset,
        summary,
        samples,
        collision_events,
    })
}

#[tauri::command]
pub async fn get_session_timing_detail(
    state: State<'_, AppState>,
    dataset_id: String,
    session_id: String,
) -> AppResult<SessionTimingDetail> {
    let conn = state.db.get()?;
    repositories::get_session_timing_detail(&conn, &dataset_id, &session_id)
}

fn finalize_sessions_and_collisions(
    conn: &mut rusqlite::Connection,
    capture_id: &str,
    pipeline_output: &mut pipeline::PipelineOutput,
) -> AppResult<()> {
    // Refine the provisional gamelog race sessions using telemetry signals
    // (drone-reset sim_time resets, movement/idle), or derive telemetry-only
    // sessions when there was no game log.
    let gamelog_sessions = repositories::list_race_sessions(conn, capture_id)?;
    let offset_seconds = pipeline_output.summary.start_monotonic_ns as f64 / 1_000_000_000.0;
    let total_seconds = pipeline_output
        .samples
        .last()
        .map(|s| s.capture_time_seconds + offset_seconds)
        .unwrap_or(0.0);
    let mut final_sessions = gamelog_sessions.clone();
    // Only refine the gamelog-provisional set; don't clobber an already-fused result.
    let provisional = gamelog_sessions
        .iter()
        .all(|s| s.segmentation_method == "gamelog");
    if provisional {
        let cfg = SegmentationConfig::default();
        let signals = segmentation::extract_telemetry_signals(&pipeline_output.samples, &cfg);
        let refined = segmentation::fuse_sessions(
            capture_id,
            &gamelog_sessions,
            &signals,
            offset_seconds,
            total_seconds,
            &cfg,
        );
        if !refined.is_empty() {
            final_sessions = refined;
        }
    }

    pipeline_output.collision_events = confirm_events_for_sessions(
        conn,
        &final_sessions,
        &pipeline_output.samples,
        &pipeline_output.collision_events,
        offset_seconds,
        total_seconds,
    )?;
    update_collision_summary(
        &mut pipeline_output.summary,
        &pipeline_output.collision_events,
    );

    apply_collision_metrics(
        &mut final_sessions,
        &pipeline_output.collision_events,
        offset_seconds,
        total_seconds,
    );
    if !final_sessions.is_empty() {
        repositories::replace_race_sessions(conn, capture_id, &final_sessions)?;
    }
    Ok(())
}

fn finalize_session_timing(
    conn: &mut rusqlite::Connection,
    dataset_id: &str,
    capture_id: &str,
    samples: &[TelemetrySample],
    offset_seconds: f64,
    total_seconds: f64,
) -> AppResult<()> {
    let sessions = repositories::list_race_sessions(conn, capture_id)?;
    let mut rows = TimingRows::default();

    for session in sessions {
        let course = match liftoff_assets::resolve_session_course(conn, &session) {
            Ok(Some(course)) => course,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(
                    session_id = %session.id,
                    "skipping lap timing because course resolution failed: {error}"
                );
                continue;
            }
        };

        let session_rows = timing::derive_session_timing(
            dataset_id,
            capture_id,
            &session,
            &course,
            samples,
            offset_seconds,
            total_seconds,
        );
        rows.laps.extend(session_rows.laps);
        rows.gate_splits.extend(session_rows.gate_splits);
        rows.passage_events.extend(session_rows.passage_events);
    }

    repositories::replace_dataset_timing(
        conn,
        dataset_id,
        &rows.laps,
        &rows.gate_splits,
        &rows.passage_events,
    )
}

fn confirm_events_for_sessions(
    conn: &rusqlite::Connection,
    sessions: &[RaceSessionRow],
    samples: &[TelemetrySample],
    events: &[CollisionEvent],
    offset_seconds: f64,
    total_seconds: f64,
) -> AppResult<Vec<CollisionEvent>> {
    if sessions.is_empty() {
        return Ok(liftoff_geometry::confirm_collision_events(
            events, samples, None,
        ));
    }

    let mut confirmed = Vec::new();
    for (index, session) in sessions.iter().enumerate() {
        let start = session.start_seconds;
        let end = session.end_seconds.unwrap_or(total_seconds);
        let is_last = index + 1 == sessions.len();
        let session_events = events
            .iter()
            .filter(|event| {
                let event_time = event.capture_time_seconds + offset_seconds;
                event_time >= start && (event_time < end || (is_last && event_time <= end))
            })
            .cloned()
            .collect::<Vec<_>>();
        if session_events.is_empty() {
            continue;
        }

        let course = liftoff_assets::resolve_session_course(conn, session)?;
        let geometry = match course {
            Some(course) => liftoff_geometry::load_course_geometry(conn, &course).ok(),
            None => None,
        };
        confirmed.extend(liftoff_geometry::confirm_collision_events(
            &session_events,
            samples,
            geometry.as_ref(),
        ));
    }
    confirmed.sort_by(|a, b| {
        a.capture_time_seconds
            .partial_cmp(&b.capture_time_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(confirmed)
}

fn update_collision_summary(summary: &mut PipelineSummary, events: &[CollisionEvent]) {
    summary.collision_count = events.len() as u64;
    summary.collision_max_severity = events.iter().map(|event| event.severity).max().unwrap_or(0);
    summary.collision_avg_severity = if events.is_empty() {
        None
    } else {
        Some(
            events
                .iter()
                .map(|event| event.severity as f32)
                .sum::<f32>()
                / events.len() as f32,
        )
    };
}

fn apply_collision_metrics(
    sessions: &mut [RaceSessionRow],
    events: &[CollisionEvent],
    offset_seconds: f64,
    total_seconds: f64,
) {
    let session_count = sessions.len();
    for (index, session) in sessions.iter_mut().enumerate() {
        let start = session.start_seconds;
        let end = session.end_seconds.unwrap_or(total_seconds);
        let is_last = index + 1 == session_count;
        let mut count = 0i64;
        let mut max_severity = 0i64;
        let mut severity_sum = 0i64;

        for event in events {
            let event_time = event.capture_time_seconds + offset_seconds;
            if event_time >= start && (event_time < end || (is_last && event_time <= end)) {
                let severity = event.severity as i64;
                count += 1;
                severity_sum += severity;
                max_severity = max_severity.max(severity);
            }
        }

        session.collision_count = count;
        session.collision_max_severity = max_severity;
        session.collision_avg_severity = if count > 0 {
            Some(severity_sum as f64 / count as f64)
        } else {
            None
        };
    }
}

fn downsample(samples: &[TelemetrySample], max_points: usize) -> Vec<SamplePoint> {
    if samples.is_empty() {
        return Vec::new();
    }
    let step = (samples.len() / max_points).max(1);
    samples
        .iter()
        .step_by(step)
        .map(|s| SamplePoint {
            capture_time_seconds: s.capture_time_seconds,
            speed: s.velocity.map(|v| v.magnitude()).unwrap_or(0.0),
            throttle: s.input.map(|i| i.throttle),
            pos: s.position.map(|p| [p.x, p.y, p.z]),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(start: f64, end: f64) -> RaceSessionRow {
        RaceSessionRow {
            id: format!("rs_{start}"),
            capture_id: "cap".into(),
            session_index: 0,
            start_monotonic_ns: (start * 1_000_000_000.0) as i64,
            end_monotonic_ns: Some((end * 1_000_000_000.0) as i64),
            start_seconds: start,
            end_seconds: Some(end),
            duration_seconds: Some(end - start),
            level: None,
            race: None,
            track: None,
            game_mode: None,
            drone: None,
            race_guid: None,
            title: None,
            segmentation_method: "telemetry".into(),
            confidence: Some(0.5),
            collision_count: 0,
            collision_max_severity: 0,
            collision_avg_severity: None,
        }
    }

    fn collision(sample_time: f64, severity: u8) -> CollisionEvent {
        CollisionEvent {
            sample_index: 0,
            capture_time_seconds: sample_time,
            severity,
            confidence: 1.0,
            speed_before: 8.0,
            speed_after: 0.0,
            speed_delta: 8.0,
            decel_mps2: 80.0,
            pos: None,
            geometry_confirmed: false,
            geometry_status: Some("not_checked".into()),
            hit_source: None,
            hit_label: None,
            hit_shape: None,
            hit_distance: None,
        }
    }

    #[test]
    fn assigns_collision_metrics_to_session_windows() {
        let mut sessions = vec![session(5.0, 10.0), session(10.0, 15.0)];
        let events = vec![collision(1.0, 3), collision(6.0, 8), collision(8.0, 4)];

        apply_collision_metrics(&mut sessions, &events, 4.0, 15.0);

        assert_eq!(sessions[0].collision_count, 1);
        assert_eq!(sessions[0].collision_max_severity, 3);
        assert_eq!(sessions[0].collision_avg_severity, Some(3.0));
        assert_eq!(sessions[1].collision_count, 2);
        assert_eq!(sessions[1].collision_max_severity, 8);
        assert_eq!(sessions[1].collision_avg_severity, Some(6.0));
    }
}
