use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::capture::auto::{AutoCaptureHandle, AutoCaptureState};
use crate::capture::recorder::RecorderHandle;
use crate::error::{AppError, AppResult};
use crate::gamelog::tailer::LogTailerHandle;
use crate::storage::db::DbPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub udp_bind_addr: String,
    pub udp_port: u16,
    pub liftoff_config_path_override: Option<PathBuf>,
    pub gamelog_enabled: bool,
    pub auto_capture_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            udp_bind_addr: "127.0.0.1".to_string(),
            udp_port: 9001,
            liftoff_config_path_override: None,
            gamelog_enabled: true,
            auto_capture_enabled: true,
        }
    }
}

pub struct AppState {
    pub db: DbPool,
    pub recorder: Arc<Mutex<Option<RecorderHandle>>>,
    pub log_tailer: Arc<Mutex<Option<LogTailerHandle>>>,
    pub asset_refresh_lock: Arc<Mutex<()>>,
    udp_endpoint_busy: Arc<Mutex<bool>>,
    pub data_dir: PathBuf,
    pub captures_dir: PathBuf,
    pub app_config: Arc<RwLock<AppConfig>>,
    pub app_version: String,
    pub auto_capture: Arc<Mutex<Option<AutoCaptureHandle>>>,
    pub auto_capture_state: Arc<RwLock<AutoCaptureState>>,
}

impl AppState {
    pub fn new(db: DbPool, data_dir: PathBuf, captures_dir: PathBuf) -> Self {
        Self {
            db,
            recorder: Arc::new(Mutex::new(None)),
            log_tailer: Arc::new(Mutex::new(None)),
            asset_refresh_lock: Arc::new(Mutex::new(())),
            udp_endpoint_busy: Arc::new(Mutex::new(false)),
            data_dir,
            captures_dir,
            app_config: Arc::new(RwLock::new(AppConfig::default())),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            auto_capture: Arc::new(Mutex::new(None)),
            auto_capture_state: Arc::new(RwLock::new(AutoCaptureState::disabled())),
        }
    }

    pub fn reserve_udp_endpoint(&self) -> AppResult<UdpEndpointGuard> {
        let mut busy = self
            .udp_endpoint_busy
            .lock()
            .map_err(|_| AppError::InvalidState("udp endpoint mutex poisoned".into()))?;
        if *busy {
            return Err(AppError::InvalidState(
                "the UDP endpoint is already starting, stopping, or being tested".into(),
            ));
        }

        *busy = true;
        Ok(UdpEndpointGuard {
            busy: Arc::clone(&self.udp_endpoint_busy),
            active: true,
        })
    }
}

pub struct UdpEndpointGuard {
    busy: Arc<Mutex<bool>>,
    active: bool,
}

impl UdpEndpointGuard {
    pub fn release(mut self) {
        self.clear();
    }

    fn clear(&mut self) {
        if self.active {
            if let Ok(mut busy) = self.busy.lock() {
                *busy = false;
            }
            self.active = false;
        }
    }
}

impl Drop for UdpEndpointGuard {
    fn drop(&mut self) {
        self.clear();
    }
}
