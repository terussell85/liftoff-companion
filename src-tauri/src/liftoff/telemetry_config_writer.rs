use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::capture::integrity::hash_bytes;
use crate::error::{AppError, AppResult};
use crate::telemetry::liftoff_schema::LiftoffSchema;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfigStatus {
    pub path: PathBuf,
    pub exists: bool,
    pub endpoint: Option<String>,
    pub stream_format: Vec<String>,
    pub raw_json: Option<String>,
    pub config_hash: Option<String>,
    pub matches_canonical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyConfigOutcome {
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub previous_hash: Option<String>,
    pub new_hash: String,
    pub previous_existed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisableOutcome {
    pub path: PathBuf,
    /// True when a `.json.bak` backup was found and restored in place; false
    /// when the config was simply removed (or there was nothing to remove).
    pub restored: bool,
    pub restored_from: Option<PathBuf>,
}

pub fn read_status(path: &Path, canonical_endpoint: &str) -> AppResult<TelemetryConfigStatus> {
    if !path.exists() {
        return Ok(TelemetryConfigStatus {
            path: path.to_path_buf(),
            exists: false,
            endpoint: None,
            stream_format: Vec::new(),
            raw_json: None,
            config_hash: None,
            matches_canonical: false,
        });
    }
    let bytes = fs::read(path)?;
    let raw_json = String::from_utf8_lossy(&bytes).to_string();
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::LiftoffConfig(format!("invalid JSON: {}", e)))?;
    let schema = LiftoffSchema::from_config_json(&value)?;
    let canonical = LiftoffSchema::canonical(canonical_endpoint);
    let endpoint = value
        .get("EndPoint")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let stream_format = value
        .get("StreamFormat")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(TelemetryConfigStatus {
        path: path.to_path_buf(),
        exists: true,
        endpoint,
        stream_format,
        raw_json: Some(raw_json),
        config_hash: Some(schema.config_hash.clone()),
        matches_canonical: schema.config_hash == canonical.config_hash,
    })
}

pub fn apply_canonical(path: &Path, endpoint: &str) -> AppResult<ApplyConfigOutcome> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let canonical = LiftoffSchema::canonical(endpoint);
    let new_json = serde_json::to_vec_pretty(&canonical.to_config_json())?;

    let previous_existed = path.exists();
    let mut backup_path: Option<PathBuf> = None;
    let mut previous_hash: Option<String> = None;
    if previous_existed {
        let existing = fs::read(path)?;
        previous_hash = Some(hash_bytes(&existing));
        let bak = path.with_extension("json.bak");
        fs::write(&bak, &existing)?;
        backup_path = Some(bak);
    }

    fs::write(path, &new_json)?;
    Ok(ApplyConfigOutcome {
        path: path.to_path_buf(),
        backup_path,
        previous_hash,
        new_hash: canonical.config_hash.clone(),
        previous_existed,
    })
}

pub fn write_custom(path: &Path, json: &serde_json::Value) -> AppResult<String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(json)?;
    fs::write(path, &bytes)?;
    Ok(hash_bytes(&bytes))
}

/// Stop tracking an install: undo what `apply_canonical` did. If a `.json.bak`
/// backup exists (the user's pre-existing config), restore it in place and drop
/// the backup; otherwise remove the config file we wrote. A no-op if neither
/// the config nor a backup is present.
pub fn disable(path: &Path) -> AppResult<DisableOutcome> {
    let backup = path.with_extension("json.bak");
    if backup.exists() {
        let existing = fs::read(&backup)?;
        fs::write(path, &existing)?;
        fs::remove_file(&backup)?;
        return Ok(DisableOutcome {
            path: path.to_path_buf(),
            restored: true,
            restored_from: Some(backup),
        });
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(DisableOutcome {
        path: path.to_path_buf(),
        restored: false,
        restored_from: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "127.0.0.1:9001";

    fn temp_config_path(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        // Unique-enough per run without pulling in rand: pid + a static counter.
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.push(format!(
            "liftoff-companion-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.push("TelemetryConfiguration.json");
        dir
    }

    #[test]
    fn disable_restores_backup_when_prior_config_existed() {
        let path = temp_config_path("restore");
        // A pre-existing, user-authored config (non-canonical).
        let prior = serde_json::json!({ "EndPoint": "10.0.0.5:5000" });
        fs::write(&path, serde_json::to_vec_pretty(&prior).unwrap()).unwrap();

        // Enabling backs the prior up to `.json.bak` and writes canonical.
        let applied = apply_canonical(&path, ENDPOINT).unwrap();
        assert!(applied.previous_existed);
        assert!(applied.backup_path.as_ref().unwrap().exists());
        assert!(read_status(&path, ENDPOINT).unwrap().matches_canonical);

        // Disabling restores the prior config and removes the backup.
        let outcome = disable(&path).unwrap();
        assert!(outcome.restored);
        assert!(!path.with_extension("json.bak").exists());
        let restored: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored["EndPoint"], "10.0.0.5:5000");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn disable_removes_config_when_no_prior_existed() {
        let path = temp_config_path("remove");
        // No prior config: enabling writes canonical with no backup.
        let applied = apply_canonical(&path, ENDPOINT).unwrap();
        assert!(!applied.previous_existed);
        assert!(applied.backup_path.is_none());
        assert!(path.exists());

        // Disabling removes the file we wrote.
        let outcome = disable(&path).unwrap();
        assert!(!outcome.restored);
        assert!(!path.exists());

        // Idempotent: disabling again is a clean no-op.
        let again = disable(&path).unwrap();
        assert!(!again.restored);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
