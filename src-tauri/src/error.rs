use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("addr parse error: {0}")]
    AddrParse(#[from] std::net::AddrParseError),

    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("uuid error: {0}")]
    Uuid(#[from] uuid::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid state: {0}")]
    InvalidState(String),

    #[error("capture format error: {0}")]
    CaptureFormat(String),

    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("liftoff config error: {0}")]
    LiftoffConfig(String),

    #[error("UDP endpoint {endpoint} is already in use. Stop the other listener or choose a different port in Setup.")]
    UdpEndpointInUse { endpoint: String },

    #[error("other: {0}")]
    Other(String),
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Other(err.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "kind".to_string(),
            serde_json::Value::String(self.kind().to_string()),
        );
        payload.insert(
            "message".to_string(),
            serde_json::Value::String(self.to_string()),
        );
        serde_json::Value::Object(payload).serialize(serializer)
    }
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Io(_) => "io",
            AppError::Db(_) => "db",
            AppError::Pool(_) => "pool",
            AppError::Json(_) => "json",
            AppError::Tauri(_) => "tauri",
            AppError::AddrParse(_) => "addr_parse",
            AppError::Join(_) => "join",
            AppError::Uuid(_) => "uuid",
            AppError::NotFound(_) => "not_found",
            AppError::InvalidState(_) => "invalid_state",
            AppError::CaptureFormat(_) => "capture_format",
            AppError::HashMismatch { .. } => "hash_mismatch",
            AppError::LiftoffConfig(_) => "liftoff_config",
            AppError::UdpEndpointInUse { .. } => "udp_endpoint_in_use",
            AppError::Other(_) => "other",
        }
    }

    pub fn udp_bind(endpoint: impl std::fmt::Display, err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::AddrInUse {
            AppError::UdpEndpointInUse {
                endpoint: endpoint.to_string(),
            }
        } else {
            AppError::Io(err)
        }
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;
