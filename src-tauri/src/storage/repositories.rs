use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRow {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub status: String,
    pub source_type: String,
    pub source_config_json: Option<String>,
    pub raw_file_path: String,
    pub metadata_file_path: Option<String>,
    pub context_json: Option<String>,
    pub packet_count: i64,
    pub byte_count: i64,
    pub duration_seconds: Option<f64>,
    pub app_version: Option<String>,
    pub telemetry_config_hash: Option<String>,
    pub capture_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCapture {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub status: String,
    pub source_type: String,
    pub source_config_json: Option<String>,
    pub raw_file_path: String,
    pub context_json: Option<String>,
    pub app_version: Option<String>,
    pub telemetry_config_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureMarkerRow {
    pub id: String,
    pub capture_id: String,
    pub created_at: DateTime<Utc>,
    pub monotonic_ns: Option<i64>,
    pub marker_type: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingProfileRow {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub config_json: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingJobRow {
    pub id: String,
    pub capture_id: String,
    pub profile_id: String,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub processor_version: String,
    pub input_capture_hash: Option<String>,
    pub output_dataset_id: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedDatasetRow {
    pub id: String,
    pub capture_id: String,
    pub job_id: String,
    pub profile_id: String,
    pub created_at: DateTime<Utc>,
    pub dataset_version: String,
    pub summary_json: Option<String>,
}

pub fn insert_capture(conn: &Connection, c: &NewCapture) -> AppResult<()> {
    conn.execute(
        "INSERT INTO captures (id, created_at, status, source_type, source_config_json, \
         raw_file_path, context_json, app_version, telemetry_config_hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            c.id,
            c.created_at.to_rfc3339(),
            c.status,
            c.source_type,
            c.source_config_json,
            c.raw_file_path,
            c.context_json,
            c.app_version,
            c.telemetry_config_hash,
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn finalize_capture(
    conn: &Connection,
    id: &str,
    stopped_at: DateTime<Utc>,
    status: &str,
    metadata_file_path: &str,
    packet_count: i64,
    byte_count: i64,
    duration_seconds: f64,
    capture_hash: &str,
) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE captures SET stopped_at = ?1, status = ?2, metadata_file_path = ?3, \
         packet_count = ?4, byte_count = ?5, duration_seconds = ?6, capture_hash = ?7 \
         WHERE id = ?8",
        params![
            stopped_at.to_rfc3339(),
            status,
            metadata_file_path,
            packet_count,
            byte_count,
            duration_seconds,
            capture_hash,
            id,
        ],
    )?;
    if updated == 0 {
        return Err(AppError::NotFound(format!("capture {}", id)));
    }
    Ok(())
}

pub fn update_capture_context(conn: &Connection, id: &str, context_json: &str) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE captures SET context_json = ?1 WHERE id = ?2",
        params![context_json, id],
    )?;
    if updated == 0 {
        return Err(AppError::NotFound(format!("capture {}", id)));
    }
    Ok(())
}

/// Delete a capture row. Markers, race sessions, processing jobs, and
/// processed datasets are removed by the schema's ON DELETE CASCADE.
pub fn delete_capture(conn: &Connection, id: &str) -> AppResult<()> {
    let deleted = conn.execute("DELETE FROM captures WHERE id = ?1", params![id])?;
    if deleted == 0 {
        return Err(AppError::NotFound(format!("capture {}", id)));
    }
    Ok(())
}

pub fn list_captures(conn: &Connection) -> AppResult<Vec<CaptureRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, stopped_at, status, source_type, source_config_json, \
         raw_file_path, metadata_file_path, context_json, packet_count, byte_count, \
         duration_seconds, app_version, telemetry_config_hash, capture_hash \
         FROM captures ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let created_at_str: String = r.get(1)?;
            let stopped_at_str: Option<String> = r.get(2)?;
            Ok(CaptureRow {
                id: r.get(0)?,
                created_at: parse_dt(&created_at_str).unwrap_or_else(|_| Utc::now()),
                stopped_at: stopped_at_str.and_then(|s| parse_dt(&s).ok()),
                status: r.get(3)?,
                source_type: r.get(4)?,
                source_config_json: r.get(5)?,
                raw_file_path: r.get(6)?,
                metadata_file_path: r.get(7)?,
                context_json: r.get(8)?,
                packet_count: r.get(9)?,
                byte_count: r.get(10)?,
                duration_seconds: r.get(11)?,
                app_version: r.get(12)?,
                telemetry_config_hash: r.get(13)?,
                capture_hash: r.get(14)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_capture(conn: &Connection, id: &str) -> AppResult<Option<CaptureRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, stopped_at, status, source_type, source_config_json, \
         raw_file_path, metadata_file_path, context_json, packet_count, byte_count, \
         duration_seconds, app_version, telemetry_config_hash, capture_hash \
         FROM captures WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            let created_at_str: String = r.get(1)?;
            let stopped_at_str: Option<String> = r.get(2)?;
            Ok(CaptureRow {
                id: r.get(0)?,
                created_at: parse_dt(&created_at_str).unwrap_or_else(|_| Utc::now()),
                stopped_at: stopped_at_str.and_then(|s| parse_dt(&s).ok()),
                status: r.get(3)?,
                source_type: r.get(4)?,
                source_config_json: r.get(5)?,
                raw_file_path: r.get(6)?,
                metadata_file_path: r.get(7)?,
                context_json: r.get(8)?,
                packet_count: r.get(9)?,
                byte_count: r.get(10)?,
                duration_seconds: r.get(11)?,
                app_version: r.get(12)?,
                telemetry_config_hash: r.get(13)?,
                capture_hash: r.get(14)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn insert_marker(conn: &Connection, m: &CaptureMarkerRow) -> AppResult<()> {
    conn.execute(
        "INSERT INTO capture_markers (id, capture_id, created_at, monotonic_ns, marker_type, note) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            m.id,
            m.capture_id,
            m.created_at.to_rfc3339(),
            m.monotonic_ns,
            m.marker_type,
            m.note,
        ],
    )?;
    Ok(())
}

pub fn list_markers(conn: &Connection, capture_id: &str) -> AppResult<Vec<CaptureMarkerRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, capture_id, created_at, monotonic_ns, marker_type, note \
         FROM capture_markers WHERE capture_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt
        .query_map(params![capture_id], |r| {
            let created_at_str: String = r.get(2)?;
            Ok(CaptureMarkerRow {
                id: r.get(0)?,
                capture_id: r.get(1)?,
                created_at: parse_dt(&created_at_str).unwrap_or_else(|_| Utc::now()),
                monotonic_ns: r.get(3)?,
                marker_type: r.get(4)?,
                note: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_profiles(conn: &Connection) -> AppResult<Vec<ProcessingProfileRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, config_json, is_default \
         FROM processing_profiles ORDER BY is_default DESC, name ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let created_at_str: String = r.get(2)?;
            Ok(ProcessingProfileRow {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: parse_dt(&created_at_str).unwrap_or_else(|_| Utc::now()),
                config_json: r.get(3)?,
                is_default: r.get::<_, i32>(4)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_profile(conn: &Connection, id: &str) -> AppResult<Option<ProcessingProfileRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, config_json, is_default FROM processing_profiles WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            let created_at_str: String = r.get(2)?;
            Ok(ProcessingProfileRow {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: parse_dt(&created_at_str).unwrap_or_else(|_| Utc::now()),
                config_json: r.get(3)?,
                is_default: r.get::<_, i32>(4)? != 0,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn insert_job(conn: &Connection, job: &ProcessingJobRow) -> AppResult<()> {
    conn.execute(
        "INSERT INTO processing_jobs (id, capture_id, profile_id, status, started_at, \
         completed_at, processor_version, input_capture_hash, output_dataset_id, error_message) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            job.id,
            job.capture_id,
            job.profile_id,
            job.status,
            job.started_at.map(|d| d.to_rfc3339()),
            job.completed_at.map(|d| d.to_rfc3339()),
            job.processor_version,
            job.input_capture_hash,
            job.output_dataset_id,
            job.error_message,
        ],
    )?;
    Ok(())
}

pub fn update_job(conn: &Connection, job: &ProcessingJobRow) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE processing_jobs SET status = ?1, started_at = ?2, completed_at = ?3, \
         input_capture_hash = ?4, output_dataset_id = ?5, error_message = ?6 \
         WHERE id = ?7",
        params![
            job.status,
            job.started_at.map(|d| d.to_rfc3339()),
            job.completed_at.map(|d| d.to_rfc3339()),
            job.input_capture_hash,
            job.output_dataset_id,
            job.error_message,
            job.id,
        ],
    )?;
    if updated == 0 {
        return Err(AppError::NotFound(format!("job {}", job.id)));
    }
    Ok(())
}

pub fn get_job(conn: &Connection, id: &str) -> AppResult<Option<ProcessingJobRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, capture_id, profile_id, status, started_at, completed_at, \
         processor_version, input_capture_hash, output_dataset_id, error_message \
         FROM processing_jobs WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            let started_at_str: Option<String> = r.get(4)?;
            let completed_at_str: Option<String> = r.get(5)?;
            Ok(ProcessingJobRow {
                id: r.get(0)?,
                capture_id: r.get(1)?,
                profile_id: r.get(2)?,
                status: r.get(3)?,
                started_at: started_at_str.and_then(|s| parse_dt(&s).ok()),
                completed_at: completed_at_str.and_then(|s| parse_dt(&s).ok()),
                processor_version: r.get(6)?,
                input_capture_hash: r.get(7)?,
                output_dataset_id: r.get(8)?,
                error_message: r.get(9)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn insert_dataset(conn: &Connection, ds: &ProcessedDatasetRow) -> AppResult<()> {
    conn.execute(
        "INSERT INTO processed_datasets (id, capture_id, job_id, profile_id, created_at, \
         dataset_version, summary_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            ds.id,
            ds.capture_id,
            ds.job_id,
            ds.profile_id,
            ds.created_at.to_rfc3339(),
            ds.dataset_version,
            ds.summary_json,
        ],
    )?;
    Ok(())
}

pub fn list_datasets_for_capture(
    conn: &Connection,
    capture_id: &str,
) -> AppResult<Vec<ProcessedDatasetRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, capture_id, job_id, profile_id, created_at, dataset_version, summary_json \
         FROM processed_datasets WHERE capture_id = ?1 ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map(params![capture_id], |r| {
            let created_at_str: String = r.get(4)?;
            Ok(ProcessedDatasetRow {
                id: r.get(0)?,
                capture_id: r.get(1)?,
                job_id: r.get(2)?,
                profile_id: r.get(3)?,
                created_at: parse_dt(&created_at_str).unwrap_or_else(|_| Utc::now()),
                dataset_version: r.get(5)?,
                summary_json: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_dataset(conn: &Connection, id: &str) -> AppResult<Option<ProcessedDatasetRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, capture_id, job_id, profile_id, created_at, dataset_version, summary_json \
         FROM processed_datasets WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            let created_at_str: String = r.get(4)?;
            Ok(ProcessedDatasetRow {
                id: r.get(0)?,
                capture_id: r.get(1)?,
                job_id: r.get(2)?,
                profile_id: r.get(3)?,
                created_at: parse_dt(&created_at_str).unwrap_or_else(|_| Utc::now()),
                dataset_version: r.get(5)?,
                summary_json: r.get(6)?,
            })
        })
        .optional()?;
    Ok(row)
}

fn parse_dt(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::Other(format!("bad datetime: {}", e)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceSessionRow {
    pub id: String,
    pub capture_id: String,
    pub session_index: i64,
    pub start_monotonic_ns: i64,
    pub end_monotonic_ns: Option<i64>,
    pub start_seconds: f64,
    pub end_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub level: Option<String>,
    pub race: Option<String>,
    pub track: Option<String>,
    pub game_mode: Option<String>,
    pub drone: Option<String>,
    pub race_guid: Option<String>,
    pub title: Option<String>,
    pub segmentation_method: String,
    pub confidence: Option<f64>,
    pub collision_count: i64,
    pub collision_max_severity: i64,
    pub collision_avg_severity: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameAssetCacheRow {
    pub id: String,
    pub game_title: String,
    pub data_root: String,
    pub extractor_version: String,
    pub source_fingerprint_hash: String,
    pub source_fingerprint_json: String,
    pub status: String,
    pub error_message: Option<String>,
    pub extracted_at: Option<DateTime<Utc>>,
    pub race_count: i64,
    pub track_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceCourseCacheRow {
    pub cache_id: String,
    pub race_guid: String,
    pub race_name: String,
    pub track_guid: Option<String>,
    pub track_name: Option<String>,
    pub environment_id: Option<String>,
    pub required_laps: Option<i64>,
    pub course_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionGeometryCacheRow {
    pub cache_id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub geometry_json: String,
    pub status: String,
    pub source_bundle: Option<String>,
    pub source_hash: Option<String>,
    pub warning_count: i64,
    pub error_message: Option<String>,
    pub extracted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceLapRow {
    pub id: String,
    pub dataset_id: String,
    pub capture_id: String,
    pub session_id: String,
    pub lap_index: i64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub duration_seconds: f64,
    pub start_sample_index: Option<i64>,
    pub end_sample_index: Option<i64>,
    pub status: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceGateSplitRow {
    pub id: String,
    pub dataset_id: String,
    pub capture_id: String,
    pub session_id: String,
    pub lap_index: i64,
    pub section_index: i64,
    pub section_kind: String,
    pub from_checkpoint_id: Option<i64>,
    pub from_checkpoint_sequence: Option<i64>,
    pub from_passage_type: Option<String>,
    pub to_checkpoint_id: Option<i64>,
    pub to_checkpoint_sequence: Option<i64>,
    pub to_passage_type: Option<String>,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub duration_seconds: f64,
    pub start_sample_index: Option<i64>,
    pub end_sample_index: Option<i64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RacePassageEventRow {
    pub id: String,
    pub dataset_id: String,
    pub capture_id: String,
    pub session_id: String,
    pub lap_index: i64,
    pub checkpoint_id: i64,
    pub checkpoint_sequence: i64,
    pub passage_type: String,
    pub directionality: String,
    pub event_seconds: f64,
    pub sample_index: Option<i64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTimingDetail {
    pub laps: Vec<RaceLapRow>,
    pub gate_splits: Vec<RaceGateSplitRow>,
    pub passage_events: Vec<RacePassageEventRow>,
}

pub fn insert_race_session(conn: &Connection, s: &RaceSessionRow) -> AppResult<()> {
    conn.execute(
        "INSERT INTO race_sessions (id, capture_id, session_index, start_monotonic_ns, \
         end_monotonic_ns, start_seconds, end_seconds, duration_seconds, level, race, track, \
         game_mode, drone, race_guid, title, segmentation_method, confidence, \
         collision_count, collision_max_severity, collision_avg_severity) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            s.id,
            s.capture_id,
            s.session_index,
            s.start_monotonic_ns,
            s.end_monotonic_ns,
            s.start_seconds,
            s.end_seconds,
            s.duration_seconds,
            s.level,
            s.race,
            s.track,
            s.game_mode,
            s.drone,
            s.race_guid,
            s.title,
            s.segmentation_method,
            s.confidence,
            s.collision_count,
            s.collision_max_severity,
            s.collision_avg_severity,
        ],
    )?;
    Ok(())
}

/// Replace all race sessions for a capture (delete-then-insert) so processing
/// can upgrade the provisional gamelog sessions with fused ones.
pub fn replace_race_sessions(
    conn: &mut Connection,
    capture_id: &str,
    rows: &[RaceSessionRow],
) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM race_sessions WHERE capture_id = ?1",
        params![capture_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO race_sessions (id, capture_id, session_index, start_monotonic_ns, \
             end_monotonic_ns, start_seconds, end_seconds, duration_seconds, level, race, track, \
             game_mode, drone, race_guid, title, segmentation_method, confidence, \
             collision_count, collision_max_severity, collision_avg_severity) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )?;
        for s in rows {
            stmt.execute(params![
                s.id,
                s.capture_id,
                s.session_index,
                s.start_monotonic_ns,
                s.end_monotonic_ns,
                s.start_seconds,
                s.end_seconds,
                s.duration_seconds,
                s.level,
                s.race,
                s.track,
                s.game_mode,
                s.drone,
                s.race_guid,
                s.title,
                s.segmentation_method,
                s.confidence,
                s.collision_count,
                s.collision_max_severity,
                s.collision_avg_severity,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn list_race_sessions(conn: &Connection, capture_id: &str) -> AppResult<Vec<RaceSessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, capture_id, session_index, start_monotonic_ns, end_monotonic_ns, \
         start_seconds, end_seconds, duration_seconds, level, race, track, game_mode, drone, \
         race_guid, title, segmentation_method, confidence, collision_count, \
         collision_max_severity, collision_avg_severity \
         FROM race_sessions WHERE capture_id = ?1 ORDER BY session_index ASC",
    )?;
    let rows = stmt
        .query_map(params![capture_id], |r| {
            Ok(RaceSessionRow {
                id: r.get(0)?,
                capture_id: r.get(1)?,
                session_index: r.get(2)?,
                start_monotonic_ns: r.get(3)?,
                end_monotonic_ns: r.get(4)?,
                start_seconds: r.get(5)?,
                end_seconds: r.get(6)?,
                duration_seconds: r.get(7)?,
                level: r.get(8)?,
                race: r.get(9)?,
                track: r.get(10)?,
                game_mode: r.get(11)?,
                drone: r.get(12)?,
                race_guid: r.get(13)?,
                title: r.get(14)?,
                segmentation_method: r.get(15)?,
                confidence: r.get(16)?,
                collision_count: r.get(17)?,
                collision_max_severity: r.get(18)?,
                collision_avg_severity: r.get(19)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn replace_dataset_timing(
    conn: &mut Connection,
    dataset_id: &str,
    laps: &[RaceLapRow],
    gate_splits: &[RaceGateSplitRow],
    passage_events: &[RacePassageEventRow],
) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM race_passage_events WHERE dataset_id = ?1",
        params![dataset_id],
    )?;
    tx.execute(
        "DELETE FROM race_gate_splits WHERE dataset_id = ?1",
        params![dataset_id],
    )?;
    tx.execute(
        "DELETE FROM race_laps WHERE dataset_id = ?1",
        params![dataset_id],
    )?;

    {
        let mut stmt = tx.prepare(
            "INSERT INTO race_laps (id, dataset_id, capture_id, session_id, lap_index, \
             start_seconds, end_seconds, duration_seconds, start_sample_index, end_sample_index, \
             status, confidence) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        for row in laps {
            stmt.execute(params![
                row.id,
                row.dataset_id,
                row.capture_id,
                row.session_id,
                row.lap_index,
                row.start_seconds,
                row.end_seconds,
                row.duration_seconds,
                row.start_sample_index,
                row.end_sample_index,
                row.status,
                row.confidence,
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare(
            "INSERT INTO race_gate_splits (id, dataset_id, capture_id, session_id, lap_index, \
             section_index, section_kind, from_checkpoint_id, from_checkpoint_sequence, \
             from_passage_type, to_checkpoint_id, to_checkpoint_sequence, to_passage_type, \
             start_seconds, end_seconds, duration_seconds, start_sample_index, end_sample_index, \
             confidence) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        )?;
        for row in gate_splits {
            stmt.execute(params![
                row.id,
                row.dataset_id,
                row.capture_id,
                row.session_id,
                row.lap_index,
                row.section_index,
                row.section_kind,
                row.from_checkpoint_id,
                row.from_checkpoint_sequence,
                row.from_passage_type,
                row.to_checkpoint_id,
                row.to_checkpoint_sequence,
                row.to_passage_type,
                row.start_seconds,
                row.end_seconds,
                row.duration_seconds,
                row.start_sample_index,
                row.end_sample_index,
                row.confidence,
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare(
            "INSERT INTO race_passage_events (id, dataset_id, capture_id, session_id, lap_index, \
             checkpoint_id, checkpoint_sequence, passage_type, directionality, event_seconds, \
             sample_index, confidence) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        for row in passage_events {
            stmt.execute(params![
                row.id,
                row.dataset_id,
                row.capture_id,
                row.session_id,
                row.lap_index,
                row.checkpoint_id,
                row.checkpoint_sequence,
                row.passage_type,
                row.directionality,
                row.event_seconds,
                row.sample_index,
                row.confidence,
            ])?;
        }
    }

    tx.commit()?;
    Ok(())
}

pub fn get_session_timing_detail(
    conn: &Connection,
    dataset_id: &str,
    session_id: &str,
) -> AppResult<SessionTimingDetail> {
    let laps = {
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, capture_id, session_id, lap_index, start_seconds, \
             end_seconds, duration_seconds, start_sample_index, end_sample_index, status, confidence \
             FROM race_laps WHERE dataset_id = ?1 AND session_id = ?2 ORDER BY lap_index ASC",
        )?;
        let rows = stmt.query_map(params![dataset_id, session_id], |r| {
            Ok(RaceLapRow {
                id: r.get(0)?,
                dataset_id: r.get(1)?,
                capture_id: r.get(2)?,
                session_id: r.get(3)?,
                lap_index: r.get(4)?,
                start_seconds: r.get(5)?,
                end_seconds: r.get(6)?,
                duration_seconds: r.get(7)?,
                start_sample_index: r.get(8)?,
                end_sample_index: r.get(9)?,
                status: r.get(10)?,
                confidence: r.get(11)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let gate_splits = {
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, capture_id, session_id, lap_index, section_index, section_kind, \
             from_checkpoint_id, from_checkpoint_sequence, from_passage_type, to_checkpoint_id, \
             to_checkpoint_sequence, to_passage_type, start_seconds, end_seconds, duration_seconds, \
             start_sample_index, end_sample_index, confidence \
             FROM race_gate_splits WHERE dataset_id = ?1 AND session_id = ?2 \
             ORDER BY lap_index ASC, section_index ASC, start_seconds ASC",
        )?;
        let rows = stmt.query_map(params![dataset_id, session_id], |r| {
            Ok(RaceGateSplitRow {
                id: r.get(0)?,
                dataset_id: r.get(1)?,
                capture_id: r.get(2)?,
                session_id: r.get(3)?,
                lap_index: r.get(4)?,
                section_index: r.get(5)?,
                section_kind: r.get(6)?,
                from_checkpoint_id: r.get(7)?,
                from_checkpoint_sequence: r.get(8)?,
                from_passage_type: r.get(9)?,
                to_checkpoint_id: r.get(10)?,
                to_checkpoint_sequence: r.get(11)?,
                to_passage_type: r.get(12)?,
                start_seconds: r.get(13)?,
                end_seconds: r.get(14)?,
                duration_seconds: r.get(15)?,
                start_sample_index: r.get(16)?,
                end_sample_index: r.get(17)?,
                confidence: r.get(18)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let passage_events = {
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, capture_id, session_id, lap_index, checkpoint_id, \
             checkpoint_sequence, passage_type, directionality, event_seconds, sample_index, \
             confidence \
             FROM race_passage_events WHERE dataset_id = ?1 AND session_id = ?2 \
             ORDER BY event_seconds ASC",
        )?;
        let rows = stmt.query_map(params![dataset_id, session_id], |r| {
            Ok(RacePassageEventRow {
                id: r.get(0)?,
                dataset_id: r.get(1)?,
                capture_id: r.get(2)?,
                session_id: r.get(3)?,
                lap_index: r.get(4)?,
                checkpoint_id: r.get(5)?,
                checkpoint_sequence: r.get(6)?,
                passage_type: r.get(7)?,
                directionality: r.get(8)?,
                event_seconds: r.get(9)?,
                sample_index: r.get(10)?,
                confidence: r.get(11)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    Ok(SessionTimingDetail {
        laps,
        gate_splits,
        passage_events,
    })
}

/// Delete a single race session. Note: re-processing the parent capture
/// rebuilds sessions from telemetry, so a deleted session can reappear after a
/// manual re-process.
pub fn delete_race_session(conn: &Connection, id: &str) -> AppResult<()> {
    let deleted = conn.execute("DELETE FROM race_sessions WHERE id = ?1", params![id])?;
    if deleted == 0 {
        return Err(AppError::NotFound(format!("race session {}", id)));
    }
    Ok(())
}

pub fn list_game_asset_caches(conn: &Connection) -> AppResult<Vec<GameAssetCacheRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, game_title, data_root, extractor_version, source_fingerprint_hash, \
         source_fingerprint_json, status, error_message, extracted_at, race_count, track_count \
         FROM game_asset_caches ORDER BY game_title ASC, data_root ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let extracted_at_str: Option<String> = r.get(8)?;
            Ok(GameAssetCacheRow {
                id: r.get(0)?,
                game_title: r.get(1)?,
                data_root: r.get(2)?,
                extractor_version: r.get(3)?,
                source_fingerprint_hash: r.get(4)?,
                source_fingerprint_json: r.get(5)?,
                status: r.get(6)?,
                error_message: r.get(7)?,
                extracted_at: extracted_at_str.and_then(|s| parse_dt(&s).ok()),
                race_count: r.get(9)?,
                track_count: r.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_game_asset_cache_by_root(
    conn: &Connection,
    data_root: &str,
) -> AppResult<Option<GameAssetCacheRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, game_title, data_root, extractor_version, source_fingerprint_hash, \
         source_fingerprint_json, status, error_message, extracted_at, race_count, track_count \
         FROM game_asset_caches WHERE data_root = ?1",
    )?;
    let row = stmt
        .query_row(params![data_root], |r| {
            let extracted_at_str: Option<String> = r.get(8)?;
            Ok(GameAssetCacheRow {
                id: r.get(0)?,
                game_title: r.get(1)?,
                data_root: r.get(2)?,
                extractor_version: r.get(3)?,
                source_fingerprint_hash: r.get(4)?,
                source_fingerprint_json: r.get(5)?,
                status: r.get(6)?,
                error_message: r.get(7)?,
                extracted_at: extracted_at_str.and_then(|s| parse_dt(&s).ok()),
                race_count: r.get(9)?,
                track_count: r.get(10)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn replace_game_asset_cache(
    conn: &mut Connection,
    cache: &GameAssetCacheRow,
    courses: &[RaceCourseCacheRow],
) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO game_asset_caches (id, game_title, data_root, extractor_version, \
         source_fingerprint_hash, source_fingerprint_json, status, error_message, extracted_at, \
         race_count, track_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT(data_root) DO UPDATE SET id = excluded.id, game_title = excluded.game_title, \
         extractor_version = excluded.extractor_version, \
         source_fingerprint_hash = excluded.source_fingerprint_hash, \
         source_fingerprint_json = excluded.source_fingerprint_json, status = excluded.status, \
         error_message = excluded.error_message, extracted_at = excluded.extracted_at, \
         race_count = excluded.race_count, track_count = excluded.track_count",
        params![
            cache.id,
            cache.game_title,
            cache.data_root,
            cache.extractor_version,
            cache.source_fingerprint_hash,
            cache.source_fingerprint_json,
            cache.status,
            cache.error_message,
            cache.extracted_at.map(|d| d.to_rfc3339()),
            cache.race_count,
            cache.track_count,
        ],
    )?;
    tx.execute(
        "DELETE FROM race_course_cache WHERE cache_id = ?1",
        params![cache.id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO race_course_cache (cache_id, race_guid, race_name, track_guid, \
             track_name, environment_id, required_laps, course_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for course in courses {
            stmt.execute(params![
                course.cache_id,
                course.race_guid,
                course.race_name,
                course.track_guid,
                course.track_name,
                course.environment_id,
                course.required_laps,
                course.course_json,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn replace_collision_geometry_cache(
    conn: &mut Connection,
    cache_id: &str,
    rows: &[CollisionGeometryCacheRow],
) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM collision_geometry_cache WHERE cache_id = ?1",
        params![cache_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO collision_geometry_cache (cache_id, scope_kind, scope_id, \
             geometry_json, status, source_bundle, source_hash, warning_count, error_message, \
             extracted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for row in rows {
            stmt.execute(params![
                row.cache_id,
                row.scope_kind,
                row.scope_id,
                row.geometry_json,
                row.status,
                row.source_bundle,
                row.source_hash,
                row.warning_count,
                row.error_message,
                row.extracted_at.map(|d| d.to_rfc3339()),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn list_collision_geometry_for_cache(
    conn: &Connection,
    cache_id: &str,
) -> AppResult<Vec<CollisionGeometryCacheRow>> {
    let mut stmt = conn.prepare(
        "SELECT cache_id, scope_kind, scope_id, geometry_json, status, source_bundle, \
         source_hash, warning_count, error_message, extracted_at \
         FROM collision_geometry_cache WHERE cache_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![cache_id], |r| {
            let extracted_at_str: Option<String> = r.get(9)?;
            Ok(CollisionGeometryCacheRow {
                cache_id: r.get(0)?,
                scope_kind: r.get(1)?,
                scope_id: r.get(2)?,
                geometry_json: r.get(3)?,
                status: r.get(4)?,
                source_bundle: r.get(5)?,
                source_hash: r.get(6)?,
                warning_count: r.get(7)?,
                error_message: r.get(8)?,
                extracted_at: extracted_at_str.and_then(|s| parse_dt(&s).ok()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_collision_geometry_scope(
    conn: &Connection,
    cache_id: &str,
    scope_kind: &str,
    scope_id: &str,
) -> AppResult<Option<CollisionGeometryCacheRow>> {
    let mut stmt = conn.prepare(
        "SELECT cache_id, scope_kind, scope_id, geometry_json, status, source_bundle, \
         source_hash, warning_count, error_message, extracted_at \
         FROM collision_geometry_cache \
         WHERE cache_id = ?1 AND scope_kind = ?2 AND scope_id = ?3",
    )?;
    let row = stmt
        .query_row(params![cache_id, scope_kind, scope_id], |r| {
            let extracted_at_str: Option<String> = r.get(9)?;
            Ok(CollisionGeometryCacheRow {
                cache_id: r.get(0)?,
                scope_kind: r.get(1)?,
                scope_id: r.get(2)?,
                geometry_json: r.get(3)?,
                status: r.get(4)?,
                source_bundle: r.get(5)?,
                source_hash: r.get(6)?,
                warning_count: r.get(7)?,
                error_message: r.get(8)?,
                extracted_at: extracted_at_str.and_then(|s| parse_dt(&s).ok()),
            })
        })
        .optional()?;
    Ok(row)
}

pub fn list_race_course_caches(
    conn: &Connection,
) -> AppResult<Vec<(GameAssetCacheRow, RaceCourseCacheRow)>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.game_title, c.data_root, c.extractor_version, \
         c.source_fingerprint_hash, c.source_fingerprint_json, c.status, c.error_message, \
         c.extracted_at, c.race_count, c.track_count, r.cache_id, r.race_guid, r.race_name, \
         r.track_guid, r.track_name, r.environment_id, r.required_laps, r.course_json \
         FROM race_course_cache r JOIN game_asset_caches c ON c.id = r.cache_id \
         ORDER BY c.game_title ASC, r.race_name ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let extracted_at_str: Option<String> = r.get(8)?;
            Ok((
                GameAssetCacheRow {
                    id: r.get(0)?,
                    game_title: r.get(1)?,
                    data_root: r.get(2)?,
                    extractor_version: r.get(3)?,
                    source_fingerprint_hash: r.get(4)?,
                    source_fingerprint_json: r.get(5)?,
                    status: r.get(6)?,
                    error_message: r.get(7)?,
                    extracted_at: extracted_at_str.and_then(|s| parse_dt(&s).ok()),
                    race_count: r.get(9)?,
                    track_count: r.get(10)?,
                },
                RaceCourseCacheRow {
                    cache_id: r.get(11)?,
                    race_guid: r.get(12)?,
                    race_name: r.get(13)?,
                    track_guid: r.get(14)?,
                    track_name: r.get(15)?,
                    environment_id: r.get(16)?,
                    required_laps: r.get(17)?,
                    course_json: r.get(18)?,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma");
        migrations::run(&conn).expect("migrations");
        conn
    }

    fn seed_capture(conn: &Connection, id: &str) {
        insert_capture(
            conn,
            &NewCapture {
                id: id.into(),
                created_at: Utc::now(),
                status: "completed".into(),
                source_type: "udp".into(),
                source_config_json: None,
                raw_file_path: format!("/tmp/{}/packets.rawcap", id),
                context_json: None,
                app_version: None,
                telemetry_config_hash: None,
            },
        )
        .expect("insert capture");
    }

    fn seed_race_session(conn: &Connection, id: &str, capture_id: &str) {
        insert_race_session(
            conn,
            &RaceSessionRow {
                id: id.into(),
                capture_id: capture_id.into(),
                session_index: 0,
                start_monotonic_ns: 0,
                end_monotonic_ns: Some(1_000_000_000),
                start_seconds: 0.0,
                end_seconds: Some(1.0),
                duration_seconds: Some(1.0),
                level: None,
                race: Some("Test Race".into()),
                track: None,
                game_mode: None,
                drone: None,
                race_guid: None,
                title: None,
                segmentation_method: "gamelog".into(),
                confidence: Some(1.0),
                collision_count: 0,
                collision_max_severity: 0,
                collision_avg_severity: None,
            },
        )
        .expect("insert race session");
    }

    fn seed_dataset(conn: &Connection, capture_id: &str, dataset_id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO processing_profiles \
             (id, name, created_at, config_json, is_default) VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["default", "Default", Utc::now().to_rfc3339(), "{}", 1],
        )
        .expect("insert profile");
        insert_job(
            conn,
            &ProcessingJobRow {
                id: format!("job_{dataset_id}"),
                capture_id: capture_id.into(),
                profile_id: "default".into(),
                status: "completed".into(),
                started_at: Some(Utc::now()),
                completed_at: Some(Utc::now()),
                processor_version: "test".into(),
                input_capture_hash: None,
                output_dataset_id: Some(dataset_id.into()),
                error_message: None,
            },
        )
        .expect("insert job");
        insert_dataset(
            conn,
            &ProcessedDatasetRow {
                id: dataset_id.into(),
                capture_id: capture_id.into(),
                job_id: format!("job_{dataset_id}"),
                profile_id: "default".into(),
                created_at: Utc::now(),
                dataset_version: "test".into(),
                summary_json: None,
            },
        )
        .expect("insert dataset");
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect("count")
    }

    #[test]
    fn delete_capture_cascades_to_children() {
        let conn = test_conn();
        seed_capture(&conn, "cap_a");
        seed_capture(&conn, "cap_b");
        seed_race_session(&conn, "rs_a", "cap_a");
        seed_race_session(&conn, "rs_b", "cap_b");
        insert_marker(
            &conn,
            &CaptureMarkerRow {
                id: "m_a".into(),
                capture_id: "cap_a".into(),
                created_at: Utc::now(),
                monotonic_ns: Some(1),
                marker_type: "generic".into(),
                note: None,
            },
        )
        .expect("insert marker");

        delete_capture(&conn, "cap_a").expect("delete capture");

        assert_eq!(count(&conn, "SELECT COUNT(*) FROM captures"), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM capture_markers"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM race_sessions"), 1);
        // The unrelated capture's session survives.
        assert!(
            list_race_sessions(&conn, "cap_b")
                .expect("list sessions")
                .iter()
                .any(|s| s.id == "rs_b")
        );
    }

    #[test]
    fn replaces_and_lists_session_timing() {
        let mut conn = test_conn();
        seed_capture(&conn, "cap_a");
        seed_race_session(&conn, "rs_a", "cap_a");
        seed_dataset(&conn, "cap_a", "ds_a");

        let lap = RaceLapRow {
            id: "lap_a".into(),
            dataset_id: "ds_a".into(),
            capture_id: "cap_a".into(),
            session_id: "rs_a".into(),
            lap_index: 1,
            start_seconds: 10.0,
            end_seconds: 25.0,
            duration_seconds: 15.0,
            start_sample_index: Some(10),
            end_sample_index: Some(25),
            status: "completed".into(),
            confidence: 0.95,
        };
        let split = RaceGateSplitRow {
            id: "split_a".into(),
            dataset_id: "ds_a".into(),
            capture_id: "cap_a".into(),
            session_id: "rs_a".into(),
            lap_index: 1,
            section_index: 0,
            section_kind: "lap_section".into(),
            from_checkpoint_id: Some(1),
            from_checkpoint_sequence: Some(0),
            from_passage_type: Some("Start".into()),
            to_checkpoint_id: Some(2),
            to_checkpoint_sequence: Some(1),
            to_passage_type: Some("Pass".into()),
            start_seconds: 10.0,
            end_seconds: 15.0,
            duration_seconds: 5.0,
            start_sample_index: Some(10),
            end_sample_index: Some(15),
            confidence: 0.9,
        };
        let event = RacePassageEventRow {
            id: "pass_a".into(),
            dataset_id: "ds_a".into(),
            capture_id: "cap_a".into(),
            session_id: "rs_a".into(),
            lap_index: 1,
            checkpoint_id: 1,
            checkpoint_sequence: 0,
            passage_type: "Start".into(),
            directionality: "Any".into(),
            event_seconds: 10.0,
            sample_index: Some(10),
            confidence: 0.9,
        };

        replace_dataset_timing(&mut conn, "ds_a", &[lap], &[split], &[event])
            .expect("replace timing");

        let detail = get_session_timing_detail(&conn, "ds_a", "rs_a").expect("timing detail");
        assert_eq!(detail.laps.len(), 1);
        assert_eq!(detail.gate_splits.len(), 1);
        assert_eq!(detail.passage_events.len(), 1);
        assert_eq!(detail.laps[0].duration_seconds, 15.0);

        replace_dataset_timing(&mut conn, "ds_a", &[], &[], &[]).expect("clear timing");
        let detail = get_session_timing_detail(&conn, "ds_a", "rs_a").expect("timing detail");
        assert!(detail.laps.is_empty());
        assert!(detail.gate_splits.is_empty());
        assert!(detail.passage_events.is_empty());
    }

    #[test]
    fn delete_capture_missing_is_not_found() {
        let conn = test_conn();
        match delete_capture(&conn, "cap_missing") {
            Err(AppError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn delete_race_session_only_removes_target() {
        let conn = test_conn();
        seed_capture(&conn, "cap_a");
        seed_race_session(&conn, "rs_a", "cap_a");
        seed_race_session(&conn, "rs_b", "cap_a");

        delete_race_session(&conn, "rs_a").expect("delete session");

        let remaining = list_race_sessions(&conn, "cap_a").expect("list sessions");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "rs_b");

        match delete_race_session(&conn, "rs_a") {
            Err(AppError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {:?}", other.map(|_| ())),
        }
    }
}
