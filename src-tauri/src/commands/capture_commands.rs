use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use crate::app_state::AppState;
use crate::capture::auto::{self, AutoCaptureState};
use crate::capture::integrity::compute_file_hash;
use crate::capture::marker::{append_marker, MarkerRecord};
use crate::capture::recorder::{self, PreboundSocket, RecorderConfig};
use crate::capture::stats::CaptureStats;
use crate::error::{AppError, AppResult};
use crate::gamelog::sink::AppSink;
use crate::gamelog::tailer::{self, TailerParams};
use crate::liftoff::paths::candidate_player_logs;
use crate::storage::repositories::{
    self, CaptureMarkerRow, CaptureRow, NewCapture, RaceSessionRow,
};

#[derive(Debug, Clone, Deserialize)]
pub struct StartCaptureRequest {
    pub bind_addr: Option<String>,
    pub port: Option<u16>,
    pub context: Option<serde_json::Value>,
    pub telemetry_config_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartCaptureResponse {
    pub capture: CaptureRow,
    pub bind_addr: String,
    pub raw_file_path: String,
    pub markers_file_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddMarkerRequest {
    pub capture_id: String,
    pub marker_type: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateContextRequest {
    pub capture_id: String,
    pub context: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureDetail {
    pub capture: CaptureRow,
    pub markers: Vec<CaptureMarkerRow>,
    pub race_sessions: Vec<RaceSessionRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CaptureMetadataSidecar {
    capture_id: String,
    schema_version: u32,
    created_at: String,
    stopped_at: Option<String>,
    source: serde_json::Value,
    game: serde_json::Value,
    context: Option<serde_json::Value>,
    stats: serde_json::Value,
}

#[tauri::command]
pub async fn start_capture(
    app: tauri::AppHandle,
    req: StartCaptureRequest,
) -> AppResult<StartCaptureResponse> {
    start_capture_inner(&app, req, None).await
}

/// Shared start path for the manual command and the auto-capture supervisor.
/// `prebound` carries the supervisor's already-bound socket (and the packet
/// that triggered the start) so no telemetry is lost to a rebind.
pub(crate) async fn start_capture_inner(
    app: &tauri::AppHandle,
    req: StartCaptureRequest,
    prebound: Option<PreboundSocket>,
) -> AppResult<StartCaptureResponse> {
    let state = app.state::<AppState>();
    let udp_guard = state.reserve_udp_endpoint()?;

    {
        let guard = state.recorder.lock().map_err(poisoned)?;
        if guard.is_some() {
            return Err(AppError::InvalidState(
                "a capture is already in progress".into(),
            ));
        }
    }

    let (bind_addr_str, port) = {
        let config = state.app_config.read().map_err(poisoned)?;
        (
            req.bind_addr
                .unwrap_or_else(|| config.udp_bind_addr.clone()),
            req.port.unwrap_or(config.udp_port),
        )
    };
    let bind_addr = format!("{}:{}", bind_addr_str, port).parse()?;

    let capture_id = format!("cap_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now();
    let year = now.format("%Y").to_string();
    let month = now.format("%m").to_string();
    let dir = state.captures_dir.join(year).join(month).join(&capture_id);
    std::fs::create_dir_all(&dir)?;
    let raw_file_path = dir.join("packets.rawcap");
    let markers_file_path = dir.join("markers.jsonl");

    let context_json = match &req.context {
        Some(v) => Some(serde_json::to_string(v)?),
        None => None,
    };

    let cfg = RecorderConfig {
        capture_id: capture_id.clone(),
        raw_file_path: raw_file_path.clone(),
        markers_file_path: markers_file_path.clone(),
        bind_addr,
        app_version: state.app_version.clone(),
        telemetry_config_hash: req.telemetry_config_hash.clone(),
        stats_interval: Duration::from_millis(250),
    };

    let handle = recorder::start_with(cfg, prebound).await?;
    let insert_result = (|| -> AppResult<()> {
        let conn = state.db.get()?;
        repositories::insert_capture(
            &conn,
            &NewCapture {
                id: capture_id.clone(),
                created_at: now,
                status: "recording".into(),
                source_type: "udp".into(),
                source_config_json: Some(
                    serde_json::json!({
                        "bind_addr": bind_addr_str,
                        "port": port,
                    })
                    .to_string(),
                ),
                raw_file_path: raw_file_path.to_string_lossy().into_owned(),
                context_json,
                app_version: Some(state.app_version.clone()),
                telemetry_config_hash: req.telemetry_config_hash.clone(),
            },
        )
    })();
    if let Err(err) = insert_result {
        let _ = handle.stop().await;
        return Err(err);
    }

    let mut stats_rx = handle.stats_rx.clone();
    let emit_app = app.clone();
    tokio::spawn(async move {
        loop {
            let stats: CaptureStats = stats_rx.borrow().clone();
            let terminal = stats.status != "recording";
            let _ = emit_app.emit("capture_stats_updated", stats);
            if terminal {
                break;
            }
            if stats_rx.changed().await.is_err() {
                break;
            }
        }
    });

    // Best-effort game-log tailer: auto-detect Level/Race/mode + auto-markers.
    // Shares the recorder's start_instant so log events align with telemetry time.
    let gamelog_enabled = state.app_config.read().map_err(poisoned)?.gamelog_enabled;
    if gamelog_enabled {
        let logs: Vec<_> = candidate_player_logs()
            .into_iter()
            .filter(|c| c.exists)
            .collect();
        if !logs.is_empty() {
            let sink = std::sync::Arc::new(AppSink {
                app: app.clone(),
                db: state.db.clone(),
                markers_file_path: markers_file_path.clone(),
                capture_id: capture_id.clone(),
            });
            let params = TailerParams {
                capture_id: capture_id.clone(),
                start_instant: handle.start_instant,
                gamelog_file_path: dir.join("gamelog.jsonl"),
                logs,
            };
            let tailer = tailer::start(params, sink);
            let mut guard = state.log_tailer.lock().map_err(poisoned)?;
            *guard = Some(tailer);
        }
    }

    let bind_addr_actual = handle.bind_addr.to_string();
    {
        let mut guard = state.recorder.lock().map_err(poisoned)?;
        *guard = Some(handle);
    }
    udp_guard.release();

    let capture_row = {
        let conn = state.db.get()?;
        repositories::get_capture(&conn, &capture_id)?
            .ok_or_else(|| AppError::NotFound(capture_id.clone()))?
    };

    let _ = app.emit("capture_started", &capture_row);

    Ok(StartCaptureResponse {
        capture: capture_row,
        bind_addr: bind_addr_actual,
        raw_file_path: raw_file_path.to_string_lossy().into_owned(),
        markers_file_path: markers_file_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn stop_capture(app: tauri::AppHandle, capture_id: String) -> AppResult<CaptureRow> {
    stop_capture_inner(&app, capture_id, true).await
}

/// Shared stop path for the manual command and the auto-capture supervisor.
/// `auto_process` controls whether processing is kicked off in the background;
/// the supervisor passes false so junk captures can be discarded instead.
pub(crate) async fn stop_capture_inner(
    app: &tauri::AppHandle,
    capture_id: String,
    auto_process: bool,
) -> AppResult<CaptureRow> {
    let state = app.state::<AppState>();
    let _udp_guard = state.reserve_udp_endpoint()?;

    let handle = {
        let mut guard = state.recorder.lock().map_err(poisoned)?;
        let h = guard
            .take()
            .ok_or_else(|| AppError::InvalidState("no capture in progress".into()))?;
        if h.capture_id != capture_id {
            *guard = Some(h);
            return Err(AppError::InvalidState(format!(
                "capture id mismatch: {}",
                capture_id
            )));
        }
        h
    };

    let raw_file_path = handle.raw_file_path.clone();
    let markers_file_path = handle.markers_file_path.clone();
    let start_utc = handle.start_utc;
    let bind_addr = handle.bind_addr.to_string();
    let telemetry_config_hash = {
        let conn = state.db.get()?;
        repositories::get_capture(&conn, &capture_id)?.and_then(|c| c.telemetry_config_hash)
    };

    let result = handle.stop().await?;
    let stopped_at = Utc::now();
    let capture_hash = tokio::task::spawn_blocking({
        let path = raw_file_path.clone();
        move || compute_file_hash(&path)
    })
    .await??;

    // Stop the game-log tailer and derive provisional race sessions from its
    // boundaries. Refined later by the telemetry-fusion segmenter at processing.
    let tailer_result = {
        let taken = state.log_tailer.lock().map_err(poisoned)?.take();
        match taken {
            Some(t) => Some(t.stop().await?),
            None => None,
        }
    };
    if let Some(tr) = &tailer_result {
        let capture_end_ns = (result.duration_seconds * 1_000_000_000.0) as i64;
        let sessions = crate::gamelog::segment::build_gamelog_sessions(
            &capture_id,
            &tr.boundaries,
            capture_end_ns,
        );
        let conn = state.db.get()?;
        for s in &sessions {
            repositories::insert_race_session(&conn, s)?;
        }
        // Merge the detected game context into the capture's context_json so the
        // sidecar (written below) and library include it.
        if let Some(ctx) = &tr.last_context {
            let mut obj = repositories::get_capture(&conn, &capture_id)?
                .and_then(|c| c.context_json)
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            obj.insert("detected".into(), serde_json::to_value(ctx)?);
            repositories::update_capture_context(
                &conn,
                &capture_id,
                &serde_json::Value::Object(obj).to_string(),
            )?;
        }
    }

    let metadata_path = raw_file_path
        .parent()
        .map(|p| p.join("capture.meta.json"))
        .unwrap_or_else(|| PathBuf::from("capture.meta.json"));

    let (context_value, source_config_value) = {
        let conn = state.db.get()?;
        let row = repositories::get_capture(&conn, &capture_id)?
            .ok_or_else(|| AppError::NotFound(capture_id.clone()))?;
        let ctx = row
            .context_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let src = row
            .source_config_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);
        (ctx, src)
    };

    let sidecar = CaptureMetadataSidecar {
        capture_id: capture_id.clone(),
        schema_version: 1,
        created_at: start_utc.to_rfc3339(),
        stopped_at: Some(stopped_at.to_rfc3339()),
        source: serde_json::json!({
            "type": "udp",
            "bind_addr": bind_addr,
            "config": source_config_value,
        }),
        game: serde_json::json!({
            "name": "Liftoff: Micro Drones",
            "telemetry_config_hash": telemetry_config_hash,
        }),
        context: context_value,
        stats: serde_json::json!({
            "packet_count": result.packet_count,
            "byte_count": result.byte_count,
            "bytes_written": result.bytes_written,
            "duration_seconds": result.duration_seconds,
            "capture_hash": capture_hash,
        }),
    };
    std::fs::write(&metadata_path, serde_json::to_vec_pretty(&sidecar)?)?;

    {
        let conn = state.db.get()?;
        repositories::finalize_capture(
            &conn,
            &capture_id,
            stopped_at,
            "completed",
            &metadata_path.to_string_lossy(),
            result.packet_count as i64,
            result.byte_count as i64,
            result.duration_seconds,
            &capture_hash,
        )?;
    }

    let _ = markers_file_path; // currently unused; markers already on disk if any

    let row = {
        let conn = state.db.get()?;
        repositories::get_capture(&conn, &capture_id)?
            .ok_or_else(|| AppError::NotFound(capture_id.clone()))?
    };

    let _ = app.emit("capture_stopped", &row);

    // Automatically process the freshly stopped capture in the background so
    // race sessions get a telemetry dataset (and the 3D flight path) without a
    // manual step. The auto-capture supervisor passes auto_process=false and
    // decides itself after the keep/discard check.
    if auto_process {
        spawn_auto_processing(app, capture_id.clone());
    }

    Ok(row)
}

/// Kick off background processing for a completed capture. The pipeline emits
/// the usual `processing_*` events; failures surface via `processing_failed`.
pub(crate) fn spawn_auto_processing(app: &tauri::AppHandle, capture_id: String) {
    let state = app.state::<AppState>();
    let app_bg = app.clone();
    let db = state.db.clone();
    let cfg = state.app_config.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) =
            crate::commands::processing_commands::run_processing(app_bg, db, cfg, capture_id, None)
                .await
        {
            tracing::warn!("auto-processing failed for stopped capture: {e}");
        }
    });
}

#[tauri::command]
pub async fn add_capture_marker(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: AddMarkerRequest,
) -> AppResult<CaptureMarkerRow> {
    let (markers_path, monotonic_ns) = {
        let guard = state.recorder.lock().map_err(poisoned)?;
        let handle = guard
            .as_ref()
            .ok_or_else(|| AppError::InvalidState("no capture in progress".into()))?;
        if handle.capture_id != req.capture_id {
            return Err(AppError::InvalidState(format!(
                "capture id mismatch: {}",
                req.capture_id
            )));
        }
        let elapsed = handle.start_instant.elapsed().as_nanos() as i64;
        (handle.markers_file_path.clone(), elapsed)
    };

    record_marker(
        &state.db,
        &app,
        &markers_path,
        &req.capture_id,
        monotonic_ns,
        &req.marker_type.clone().unwrap_or_else(|| "generic".into()),
        req.note.clone(),
    )
}

/// Shared marker-write path: append to `markers.jsonl`, insert into the DB, and
/// emit `marker_added`. Used by the manual `add_capture_marker` command and by
/// the game-log tailer for auto-markers. `monotonic_ns` must already be measured
/// against the capture's `start_instant`.
pub(crate) fn record_marker(
    db: &crate::storage::db::DbPool,
    app: &tauri::AppHandle,
    markers_path: &std::path::Path,
    capture_id: &str,
    monotonic_ns: i64,
    marker_type: &str,
    note: Option<String>,
) -> AppResult<CaptureMarkerRow> {
    let marker = MarkerRecord {
        id: format!("m_{}", uuid::Uuid::new_v4().simple()),
        capture_id: capture_id.to_string(),
        created_at: Utc::now(),
        monotonic_ns: Some(monotonic_ns),
        marker_type: marker_type.to_string(),
        note,
    };

    append_marker(markers_path, &marker)?;

    let row = CaptureMarkerRow {
        id: marker.id.clone(),
        capture_id: marker.capture_id.clone(),
        created_at: marker.created_at,
        monotonic_ns: marker.monotonic_ns,
        marker_type: marker.marker_type.clone(),
        note: marker.note.clone(),
    };

    {
        let conn = db.get()?;
        repositories::insert_marker(&conn, &row)?;
    }

    let _ = app.emit("marker_added", &row);
    Ok(row)
}

#[tauri::command]
pub async fn list_captures(state: State<'_, AppState>) -> AppResult<Vec<CaptureRow>> {
    let conn = state.db.get()?;
    repositories::list_captures(&conn)
}

#[tauri::command]
pub async fn get_capture(
    state: State<'_, AppState>,
    capture_id: String,
) -> AppResult<CaptureDetail> {
    let conn = state.db.get()?;
    let capture = repositories::get_capture(&conn, &capture_id)?
        .ok_or_else(|| AppError::NotFound(capture_id.clone()))?;
    let markers = repositories::list_markers(&conn, &capture_id)?;
    let race_sessions = repositories::list_race_sessions(&conn, &capture_id)?;
    Ok(CaptureDetail {
        capture,
        markers,
        race_sessions,
    })
}

#[tauri::command]
pub async fn list_race_sessions(
    state: State<'_, AppState>,
    capture_id: String,
) -> AppResult<Vec<RaceSessionRow>> {
    let conn = state.db.get()?;
    repositories::list_race_sessions(&conn, &capture_id)
}

#[tauri::command]
pub async fn update_capture_context(
    state: State<'_, AppState>,
    req: UpdateContextRequest,
) -> AppResult<CaptureRow> {
    let context_str = serde_json::to_string(&req.context)?;
    let conn = state.db.get()?;
    repositories::update_capture_context(&conn, &req.capture_id, &context_str)?;
    repositories::get_capture(&conn, &req.capture_id)?
        .ok_or_else(|| AppError::NotFound(req.capture_id))
}

#[tauri::command]
pub async fn current_capture(state: State<'_, AppState>) -> AppResult<Option<CaptureStats>> {
    let guard = state.recorder.lock().map_err(poisoned)?;
    Ok(guard.as_ref().map(|h| h.stats_rx.borrow().clone()))
}

#[tauri::command]
pub async fn delete_capture(app: tauri::AppHandle, capture_id: String) -> AppResult<()> {
    delete_capture_inner(&app, &capture_id).await
}

/// Remove a capture's DB row (children cascade) and its on-disk directory.
/// Shared by the command and the auto-capture supervisor's junk filter.
pub(crate) async fn delete_capture_inner(
    app: &tauri::AppHandle,
    capture_id: &str,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    {
        let guard = state.recorder.lock().map_err(poisoned)?;
        if let Some(h) = guard.as_ref() {
            if h.capture_id == capture_id {
                return Err(AppError::InvalidState(
                    "cannot delete a capture that is still recording".into(),
                ));
            }
        }
    }

    let raw_file_path = {
        let conn = state.db.get()?;
        let row = repositories::get_capture(&conn, capture_id)?
            .ok_or_else(|| AppError::NotFound(capture_id.to_string()))?;
        repositories::delete_capture(&conn, capture_id)?;
        row.raw_file_path
    };

    // Best-effort disk cleanup. Every capture's files (raw packets, sidecar,
    // markers, dataset caches) live in a directory named after the capture id;
    // only remove it when the layout matches that expectation.
    let dir = PathBuf::from(&raw_file_path)
        .parent()
        .filter(|d| d.file_name().map(|n| n == capture_id).unwrap_or(false))
        .map(|d| d.to_path_buf());
    match dir {
        Some(dir) => {
            let result = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&dir)).await?;
            if let Err(e) = result {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("couldn't remove capture dir for {}: {}", capture_id, e);
                }
            }
        }
        None => tracing::warn!(
            "capture {} files not in a per-capture dir; leaving {} in place",
            capture_id,
            raw_file_path
        ),
    }

    let _ = app.emit("capture_deleted", capture_id.to_string());
    Ok(())
}

#[tauri::command]
pub async fn delete_race_session(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<()> {
    let conn = state.db.get()?;
    repositories::delete_race_session(&conn, &session_id)
}

#[tauri::command]
pub async fn get_auto_capture(state: State<'_, AppState>) -> AppResult<AutoCaptureState> {
    state
        .auto_capture_state
        .read()
        .map(|s| s.clone())
        .map_err(|_| AppError::InvalidState("auto capture state lock poisoned".into()))
}

#[tauri::command]
pub async fn set_auto_capture(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<AutoCaptureState> {
    {
        let mut cfg = state.app_config.write().map_err(poisoned)?;
        cfg.auto_capture_enabled = enabled;
    }
    if enabled {
        auto::ensure_started(&app);
    } else {
        auto::stop_if_running(&app).await;
    }
    get_auto_capture(state).await
}

#[allow(dead_code)]
pub fn ensure_app_data_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(format!("app_data_dir: {}", e)))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> AppError {
    AppError::InvalidState("recorder mutex poisoned".into())
}
