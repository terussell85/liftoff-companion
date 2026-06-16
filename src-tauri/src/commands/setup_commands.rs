use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tokio::net::UdpSocket;
use tokio::time::Instant;

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::liftoff::assets::{
    self, GameAssetCatalog, GameAssetSourceStatus, RefreshRaceTrackCacheRequest,
    ResolveSessionCourseRequest, ResolveSessionCourseResponse,
};
use crate::liftoff::geometry::{self as liftoff_geometry, CollisionShape};
use crate::liftoff::paths::{
    candidate_dirs, candidate_player_logs, LiftoffDirCandidate, PlayerLogCandidate,
};
use crate::liftoff::telemetry_config_writer::{
    apply_canonical, disable, read_status, ApplyConfigOutcome, DisableOutcome,
    TelemetryConfigStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupSnapshot {
    pub udp_bind_addr: String,
    pub udp_port: u16,
    pub dirs: Vec<LiftoffDirCandidate>,
    pub config_status: Option<TelemetryConfigStatus>,
    pub player_logs: Vec<PlayerLogCandidate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplyTelemetryConfigRequest {
    pub path: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DisableTelemetryConfigRequest {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateNetworkConfigRequest {
    pub udp_bind_addr: Option<String>,
    pub udp_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestListenerResult {
    pub bind_addr: String,
    pub duration_seconds: f64,
    pub packet_count: u64,
    pub packet_rate_hz: f32,
    pub last_source_addr: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveSessionCollisionGeometryResponse {
    pub shapes: Vec<CollisionShape>,
    pub warnings: Vec<String>,
    pub unavailable: bool,
    pub status: String,
    pub message: Option<String>,
}

#[tauri::command]
pub async fn find_liftoff_dirs() -> AppResult<Vec<LiftoffDirCandidate>> {
    Ok(candidate_dirs())
}

#[tauri::command]
pub async fn get_setup_snapshot(state: State<'_, AppState>) -> AppResult<SetupSnapshot> {
    let (bind_addr, port) = {
        let cfg = state
            .app_config
            .read()
            .map_err(|_| AppError::InvalidState("app config mutex poisoned".into()))?;
        (cfg.udp_bind_addr.clone(), cfg.udp_port)
    };
    let endpoint = format!("{}:{}", bind_addr, port);
    let mut dirs = candidate_dirs();
    // Resolve per-install tracking state: for each candidate that already has a
    // config on disk, record whether it matches the canonical schema for the
    // current endpoint. This drives each install card's Track switch (tracked vs.
    // tracked-but-differs) without an extra round trip per dir.
    for dir in dirs.iter_mut() {
        if dir.config_exists {
            dir.matches_canonical =
                Some(read_status(&dir.config_path, &endpoint)?.matches_canonical);
        }
    }
    let first_existing = dirs.iter().find(|d| d.exists).cloned();
    let config_status = match first_existing {
        Some(d) => Some(read_status(&d.config_path, &endpoint)?),
        None => None,
    };
    Ok(SetupSnapshot {
        udp_bind_addr: bind_addr,
        udp_port: port,
        dirs,
        config_status,
        player_logs: candidate_player_logs(),
    })
}

#[tauri::command]
pub async fn read_telemetry_config(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<TelemetryConfigStatus> {
    let endpoint = {
        let cfg = state
            .app_config
            .read()
            .map_err(|_| AppError::InvalidState("app config mutex poisoned".into()))?;
        format!("{}:{}", cfg.udp_bind_addr, cfg.udp_port)
    };
    read_status(&PathBuf::from(path), &endpoint)
}

#[tauri::command]
pub async fn apply_recommended_telemetry_config(
    state: State<'_, AppState>,
    req: ApplyTelemetryConfigRequest,
) -> AppResult<ApplyConfigOutcome> {
    let endpoint = req.endpoint.unwrap_or_else(|| {
        let cfg = state.app_config.read().unwrap();
        format!("{}:{}", cfg.udp_bind_addr, cfg.udp_port)
    });
    let path = match req.path {
        Some(p) => PathBuf::from(p),
        None => {
            let dirs = candidate_dirs();
            let chosen = dirs.into_iter().find(|d| d.exists).ok_or_else(|| {
                AppError::LiftoffConfig(
                    "no Liftoff data directory found; specify path manually".into(),
                )
            })?;
            chosen.config_path
        }
    };
    apply_canonical(&path, &endpoint)
}

#[tauri::command]
pub async fn disable_telemetry_config(
    req: DisableTelemetryConfigRequest,
) -> AppResult<DisableOutcome> {
    disable(&PathBuf::from(req.path))
}

#[tauri::command]
pub async fn update_network_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: UpdateNetworkConfigRequest,
) -> AppResult<SetupSnapshot> {
    // Bounce the auto-capture supervisor across the change so it rebinds the
    // (possibly new) endpoint instead of holding the old socket.
    let supervisor_was_running = crate::capture::auto::stop_if_running(&app).await;
    {
        let mut cfg = state
            .app_config
            .write()
            .map_err(|_| AppError::InvalidState("app config mutex poisoned".into()))?;
        if let Some(addr) = req.udp_bind_addr {
            cfg.udp_bind_addr = addr;
        }
        if let Some(port) = req.udp_port {
            cfg.udp_port = port;
        }
    }
    if supervisor_was_running {
        crate::capture::auto::ensure_started(&app);
    }
    get_setup_snapshot(state).await
}

#[tauri::command]
pub async fn list_game_asset_sources(
    state: State<'_, AppState>,
) -> AppResult<Vec<GameAssetSourceStatus>> {
    let conn = state.db.get()?;
    assets::list_sources_with_cache(&conn)
}

#[tauri::command]
pub async fn list_game_asset_catalog(
    state: State<'_, AppState>,
) -> AppResult<Vec<GameAssetCatalog>> {
    let conn = state.db.get()?;
    assets::list_asset_catalog(&conn)
}

#[tauri::command]
pub async fn refresh_race_track_cache(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: RefreshRaceTrackCacheRequest,
) -> AppResult<Vec<GameAssetSourceStatus>> {
    let db = state.db.clone();
    let lock = state.asset_refresh_lock.clone();
    let app_for_progress = app.clone();
    let force = req.force.unwrap_or(false);
    let data_root = req.data_root.clone();
    let mut started =
        assets::AssetRefreshProgress::new("queued", "Waiting for the asset refresh worker.");
    started.data_root = data_root.clone();
    let _ = app.emit("asset_refresh_started", started);
    let result = tokio::task::spawn_blocking(move || {
        let _guard = lock
            .lock()
            .map_err(|_| AppError::InvalidState("asset refresh mutex poisoned".into()))?;
        let mut conn = db.get()?;
        assets::refresh_sources(&mut conn, force, data_root.as_deref(), move |progress| {
            let _ = app_for_progress.emit("asset_refresh_progress", &progress);
        })
    })
    .await?;

    match &result {
        Ok(statuses) => {
            let _ = app.emit("asset_refresh_completed", statuses);
        }
        Err(error) => {
            let _ = app.emit(
                "asset_refresh_failed",
                assets::AssetRefreshProgress::new(
                    "failed",
                    format!("Race/track data refresh failed: {error}"),
                ),
            );
        }
    }
    result
}

#[tauri::command]
pub async fn resolve_session_course(
    state: State<'_, AppState>,
    req: ResolveSessionCourseRequest,
) -> AppResult<ResolveSessionCourseResponse> {
    let db = state.db.clone();
    let lock = state.asset_refresh_lock.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = lock
            .lock()
            .map_err(|_| AppError::InvalidState("asset refresh mutex poisoned".into()))?;
        let mut conn = db.get()?;
        let session = crate::storage::repositories::list_race_sessions(&conn, &req.capture_id)?
            .into_iter()
            .find(|session| session.id == req.session_id)
            .ok_or_else(|| AppError::NotFound(req.session_id.clone()))?;

        let mut refreshed = false;
        if assets::any_source_needs_refresh(&conn)? {
            let _ = assets::refresh_sources(&mut conn, false, None, |_| {})?;
            refreshed = true;
        }

        let mut course = assets::resolve_session_course(&conn, &session)?;
        if course.is_none() && !refreshed {
            let _ = assets::refresh_sources(&mut conn, false, None, |_| {})?;
            refreshed = true;
            course = assets::resolve_session_course(&conn, &session)?;
        }

        let (status, message) = if course.is_some() {
            ("ok".into(), None)
        } else {
            (
                "missing".into(),
                Some("No matching cached race/track data was found for this session.".into()),
            )
        };

        Ok(ResolveSessionCourseResponse {
            course,
            refreshed,
            status,
            message,
        })
    })
    .await?
}

#[tauri::command]
pub async fn resolve_session_collision_geometry(
    state: State<'_, AppState>,
    req: ResolveSessionCourseRequest,
) -> AppResult<ResolveSessionCollisionGeometryResponse> {
    let db = state.db.clone();
    let lock = state.asset_refresh_lock.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = lock
            .lock()
            .map_err(|_| AppError::InvalidState("asset refresh mutex poisoned".into()))?;
        let mut conn = db.get()?;
        let session = crate::storage::repositories::list_race_sessions(&conn, &req.capture_id)?
            .into_iter()
            .find(|session| session.id == req.session_id)
            .ok_or_else(|| AppError::NotFound(req.session_id.clone()))?;

        if assets::any_source_needs_refresh(&conn)? {
            let _ = assets::refresh_sources(&mut conn, false, None, |_| {})?;
        }

        let Some(course) = assets::resolve_session_course(&conn, &session)? else {
            return Ok(ResolveSessionCollisionGeometryResponse {
                shapes: Vec::new(),
                warnings: Vec::new(),
                unavailable: true,
                status: "missing".into(),
                message: Some(
                    "No matching cached race/track data was found for this session.".into(),
                ),
            });
        };

        let geometry = liftoff_geometry::load_course_geometry(&conn, &course)?;
        let status = if geometry.shapes.is_empty() {
            "missing"
        } else if geometry.unavailable {
            "partial"
        } else {
            "ok"
        };
        let message = match status {
            "missing" => Some("Collision geometry is unavailable for this course.".into()),
            "partial" => Some("Some collision geometry is unavailable for this course.".into()),
            _ => None,
        };

        Ok(ResolveSessionCollisionGeometryResponse {
            shapes: geometry.shapes,
            warnings: geometry.warnings,
            unavailable: geometry.unavailable,
            status: status.into(),
            message,
        })
    })
    .await?
}

#[tauri::command]
pub async fn run_test_listener(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    duration_seconds: Option<f64>,
) -> AppResult<TestListenerResult> {
    // The armed auto-capture supervisor holds the endpoint; release it for the
    // probe and re-arm afterwards regardless of the probe's outcome.
    let supervisor_was_running = crate::capture::auto::stop_if_running(&app).await;
    let result = run_test_listener_inner(&state, duration_seconds).await;
    if supervisor_was_running {
        crate::capture::auto::ensure_started(&app);
    }
    result
}

async fn run_test_listener_inner(
    state: &State<'_, AppState>,
    duration_seconds: Option<f64>,
) -> AppResult<TestListenerResult> {
    let _udp_guard = state.reserve_udp_endpoint()?;

    if state
        .recorder
        .lock()
        .map_err(|_| AppError::InvalidState("recorder mutex poisoned".into()))?
        .is_some()
    {
        return Err(AppError::InvalidState(
            "cannot run test listener while a capture is in progress".into(),
        ));
    }

    let (bind_addr_str, port) = {
        let cfg = state
            .app_config
            .read()
            .map_err(|_| AppError::InvalidState("app config mutex poisoned".into()))?;
        (cfg.udp_bind_addr.clone(), cfg.udp_port)
    };
    let bind: std::net::SocketAddr = format!("{}:{}", bind_addr_str, port).parse()?;
    let socket = UdpSocket::bind(bind)
        .await
        .map_err(|err| AppError::udp_bind(bind, err))?;
    let bound = socket.local_addr()?.to_string();

    let dur = Duration::from_secs_f64(duration_seconds.unwrap_or(3.0).clamp(0.5, 15.0));
    let start = Instant::now();
    let deadline = start + dur;

    let mut buf = vec![0u8; 65_535];
    let mut packet_count = 0u64;
    let mut last_source: Option<String> = None;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let res = tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await;
        match res {
            Ok(Ok((_n, addr))) => {
                packet_count += 1;
                last_source = Some(addr.to_string());
            }
            Ok(Err(e)) => {
                tracing::warn!("test listener recv error: {}", e);
                break;
            }
            Err(_) => break,
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 {
        packet_count as f32 / elapsed as f32
    } else {
        0.0
    };
    Ok(TestListenerResult {
        bind_addr: bound,
        duration_seconds: elapsed,
        packet_count,
        packet_rate_hz: rate,
        last_source_addr: last_source,
    })
}
