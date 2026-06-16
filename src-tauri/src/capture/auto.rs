//! Auto-capture supervisor: the "blackbox" capture model.
//!
//! While enabled, the supervisor owns the telemetry UDP socket whenever no
//! capture is running. The first packet that arrives starts a capture (the
//! socket and that packet are handed to the recorder, so nothing is lost), and
//! sustained packet silence stops it again. Captures with too few packets to
//! be a real flight are discarded. This mirrors how FPV blackboxes and goggle
//! DVRs scope a session to an arm/disarm cycle: telemetry only flows while
//! flying, so packets ARE the session boundary.

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::UdpSocket;
use tokio::sync::watch;

use crate::app_state::AppState;
use crate::capture::recorder::PreboundSocket;
use crate::commands::capture_commands::{self, StartCaptureRequest};

/// How long telemetry must stay silent before an auto capture is stopped.
const SILENCE_TIMEOUT: Duration = Duration::from_secs(10);
/// Liftoff streams ~90–100 Hz; captures below this packet count never left the
/// menus (or were a sub-second blip) and are discarded after stopping.
const MIN_KEEP_PACKETS: i64 = 300;
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const RETRY_DELAY: Duration = Duration::from_secs(3);

/// Snapshot of the supervisor for the frontend. `phase` is one of
/// `disabled | waiting | armed | recording`.
#[derive(Debug, Clone, Serialize)]
pub struct AutoCaptureState {
    pub enabled: bool,
    pub phase: String,
    pub bind_addr: Option<String>,
    pub message: Option<String>,
}

impl AutoCaptureState {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            phase: "disabled".into(),
            bind_addr: None,
            message: None,
        }
    }
}

pub struct AutoCaptureHandle {
    shutdown_tx: watch::Sender<bool>,
    join: tauri::async_runtime::JoinHandle<()>,
}

/// Start the supervisor when the config enables it and none is running.
pub fn ensure_started(app: &AppHandle) {
    let state = app.state::<AppState>();
    let enabled = state
        .app_config
        .read()
        .map(|c| c.auto_capture_enabled)
        .unwrap_or(false);
    if !enabled {
        return;
    }
    let Ok(mut guard) = state.auto_capture.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let task_app = app.clone();
    let join = tauri::async_runtime::spawn(async move {
        supervise(task_app, shutdown_rx).await;
    });
    *guard = Some(AutoCaptureHandle { shutdown_tx, join });
}

/// Stop the supervisor if it is running; returns whether one was running.
/// Callers that only need the socket released temporarily (endpoint probe,
/// endpoint change) should pair this with `ensure_started` afterwards.
pub async fn stop_if_running(app: &AppHandle) -> bool {
    let handle = {
        let state = app.state::<AppState>();
        let Ok(mut guard) = state.auto_capture.lock() else {
            return false;
        };
        guard.take()
    };
    match handle {
        Some(h) => {
            let _ = h.shutdown_tx.send(true);
            let _ = h.join.await;
            true
        }
        None => false,
    }
}

async fn supervise(app: AppHandle, mut shutdown_rx: watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        // A capture is running (auto or manual): watch it for packet silence.
        if recorder_active(&app) {
            match monitor_active_capture(&app, &mut shutdown_rx).await {
                ControlFlow::Break(()) => break,
                ControlFlow::Continue(()) => continue,
            }
        }

        // Idle: hold the endpoint and wait for the first telemetry packet.
        let bind_addr = match configured_bind_addr(&app) {
            Ok(addr) => addr,
            Err(e) => {
                publish(&app, "waiting", None, Some(format!("bad endpoint: {e}")));
                if sleep_or_shutdown(&mut shutdown_rx, RETRY_DELAY).await {
                    break;
                }
                continue;
            }
        };
        let socket = match UdpSocket::bind(bind_addr).await {
            Ok(socket) => socket,
            Err(e) => {
                publish(
                    &app,
                    "waiting",
                    Some(bind_addr.to_string()),
                    Some(format!("endpoint busy: {e}")),
                );
                if sleep_or_shutdown(&mut shutdown_rx, RETRY_DELAY).await {
                    break;
                }
                continue;
            }
        };

        publish(&app, "armed", Some(bind_addr.to_string()), None);
        let mut buf = vec![0u8; 65_535];
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            res = socket.recv_from(&mut buf) => match res {
                Ok((n, src)) => {
                    let req = StartCaptureRequest {
                        bind_addr: None,
                        port: None,
                        context: Some(serde_json::json!({ "mode": "sim", "trigger": "auto" })),
                        telemetry_config_hash: None,
                    };
                    let prebound = PreboundSocket {
                        socket,
                        first_packet: Some((buf[..n].to_vec(), src)),
                    };
                    if let Err(e) =
                        capture_commands::start_capture_inner(&app, req, Some(prebound)).await
                    {
                        tracing::warn!("auto-capture start failed: {e}");
                        publish(&app, "waiting", None, Some(format!("auto-start failed: {e}")));
                        if sleep_or_shutdown(&mut shutdown_rx, RETRY_DELAY).await {
                            break;
                        }
                    }
                    // Next iteration enters monitor_active_capture.
                }
                Err(e) => {
                    tracing::warn!("auto-capture recv error: {e}");
                    if sleep_or_shutdown(&mut shutdown_rx, RETRY_DELAY).await {
                        break;
                    }
                }
            },
        }
    }
    publish(&app, "disabled", None, None);
}

/// Poll the live recorder; stop it once packets go silent for SILENCE_TIMEOUT.
/// Break = supervisor shutdown, Continue = capture ended (by us or manually).
async fn monitor_active_capture(
    app: &AppHandle,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> ControlFlow<()> {
    publish(app, "recording", None, None);
    let mut last_count: Option<u64> = None;
    let mut last_change = Instant::now();

    loop {
        if sleep_or_shutdown(shutdown_rx, POLL_INTERVAL).await {
            return ControlFlow::Break(());
        }
        let Some((capture_id, packet_count)) = active_capture_stats(app) else {
            // Stopped manually; go back to arming.
            return ControlFlow::Continue(());
        };
        if last_count != Some(packet_count) {
            last_count = Some(packet_count);
            last_change = Instant::now();
            continue;
        }
        if last_change.elapsed() < SILENCE_TIMEOUT {
            continue;
        }

        publish(app, "waiting", None, None);
        match capture_commands::stop_capture_inner(app, capture_id, false).await {
            Ok(row) => {
                if row.packet_count >= MIN_KEEP_PACKETS {
                    capture_commands::spawn_auto_processing(app, row.id.clone());
                } else {
                    match capture_commands::delete_capture_inner(app, &row.id).await {
                        Ok(()) => {
                            let _ = app.emit("capture_discarded", &row);
                        }
                        Err(e) => {
                            tracing::warn!("couldn't discard junk capture {}: {e}", row.id);
                        }
                    }
                }
            }
            Err(e) => tracing::warn!("auto-capture stop failed: {e}"),
        }
        return ControlFlow::Continue(());
    }
}

fn recorder_active(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state
        .recorder
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

fn active_capture_stats(app: &AppHandle) -> Option<(String, u64)> {
    let state = app.state::<AppState>();
    let guard = state.recorder.lock().ok()?;
    let handle = guard.as_ref()?;
    let packet_count = handle.stats_rx.borrow().packet_count;
    Some((handle.capture_id.clone(), packet_count))
}

fn configured_bind_addr(app: &AppHandle) -> Result<std::net::SocketAddr, String> {
    let state = app.state::<AppState>();
    let cfg = state
        .app_config
        .read()
        .map_err(|_| "app config lock poisoned".to_string())?;
    format!("{}:{}", cfg.udp_bind_addr, cfg.udp_port)
        .parse()
        .map_err(|e| format!("{e}"))
}

/// Sleep for `dur`, returning true when shutdown fires first.
async fn sleep_or_shutdown(shutdown_rx: &mut watch::Receiver<bool>, dur: Duration) -> bool {
    tokio::select! {
        _ = shutdown_rx.changed() => true,
        _ = tokio::time::sleep(dur) => false,
    }
}

/// Record the snapshot in app state (for `get_auto_capture`) and emit it.
fn publish(app: &AppHandle, phase: &str, bind_addr: Option<String>, message: Option<String>) {
    let snapshot = AutoCaptureState {
        enabled: phase != "disabled",
        phase: phase.into(),
        bind_addr,
        message,
    };
    let state = app.state::<AppState>();
    if let Ok(mut current) = state.auto_capture_state.write() {
        *current = snapshot.clone();
    }
    let _ = app.emit("auto_capture_state", snapshot);
}
