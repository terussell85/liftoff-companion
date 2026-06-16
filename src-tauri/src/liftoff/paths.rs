use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiftoffDirCandidate {
    pub path: PathBuf,
    pub exists: bool,
    pub config_path: PathBuf,
    pub config_exists: bool,
    pub label: String,
    /// Whether the existing config matches the canonical schema for the current
    /// endpoint. `None` until resolved against an endpoint (see
    /// `get_setup_snapshot`); only meaningful when `config_exists` is true.
    pub matches_canonical: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerLogCandidate {
    /// Game title, e.g. `Liftoff` or `Liftoff Micro Drones`.
    pub title: String,
    pub path: PathBuf,
    pub exists: bool,
    /// Last-modified time as a Unix epoch millisecond count (None if unknown).
    pub modified_ms: Option<i64>,
}

/// Platform-specific candidate `Player.log` paths for both Liftoff titles.
/// Unity writes these to the OS log directory, separate from the game data dir.
pub fn candidate_player_logs() -> Vec<PlayerLogCandidate> {
    raw_log_candidates()
        .into_iter()
        .map(|(title, path)| {
            let exists = path.exists();
            let modified_ms = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64);
            PlayerLogCandidate {
                title,
                path,
                exists,
                modified_ms,
            }
        })
        .collect()
}

fn raw_log_candidates() -> Vec<(String, PathBuf)> {
    let titles = ["Liftoff", "Liftoff Micro Drones"];
    let mut out = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let base = home.join("Library/Logs/LuGus Studios");
            for t in titles {
                out.push((t.to_string(), base.join(t).join("Player.log")));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(profile) = dirs::data_local_dir() {
            // Unity logs to %USERPROFILE%\AppData\LocalLow\<company>\<product>\Player.log
            if let Some(profile_root) = profile.parent().and_then(|p| p.parent()) {
                let low = profile_root.join("LocalLow").join("LuGus Studios");
                for t in titles {
                    out.push((t.to_string(), low.join(t).join("Player.log")));
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            let base = home.join(".config/unity3d/LuGus Studios");
            for t in titles {
                out.push((t.to_string(), base.join(t).join("Player.log")));
            }
            out.push((
                "Liftoff (Flatpak)".to_string(),
                home.join(".var/app/com.valvesoftware.Steam/.config/unity3d/LuGus Studios/Liftoff/Player.log"),
            ));
        }
    }

    out
}

/// Returns the platform-specific candidate Liftoff user data directories.
/// Each candidate includes whether the directory and its `TelemetryConfiguration.json` exist.
pub fn candidate_dirs() -> Vec<LiftoffDirCandidate> {
    let candidates = raw_candidates();
    candidates
        .into_iter()
        .map(|(label, path)| {
            let exists = path.exists();
            let config_path = path.join("TelemetryConfiguration.json");
            let config_exists = config_path.exists();
            LiftoffDirCandidate {
                path,
                exists,
                config_path,
                config_exists,
                label,
                matches_canonical: None,
            }
        })
        .collect()
}

fn raw_candidates() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(profile) = dirs::data_local_dir() {
            // dirs::data_local_dir() returns %LOCALAPPDATA% but Liftoff uses LocalLow.
            // Walk up to %USERPROFILE% and then down into AppData\LocalLow.
            if let Some(profile_root) = profile.parent().and_then(|p| p.parent()) {
                let low = profile_root.join("LocalLow").join("LuGus Studios");
                for sub in ["Liftoff", "Liftoff Micro Drones", "Liftoff® Micro Drones"] {
                    out.push((format!("Windows LocalLow / {}", sub), low.join(sub)));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let base = home.join("Library/Application Support/LuGus Studios");
            for sub in ["Liftoff", "Liftoff Micro Drones"] {
                out.push((format!("macOS / {}", sub), base.join(sub)));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            let base = home.join(".config/unity3d/LuGus Studios");
            for sub in ["Liftoff", "Liftoff Micro Drones"] {
                out.push((format!("Linux / {}", sub), base.join(sub)));
            }
            let flatpak =
                home.join(".var/app/com.valvesoftware.Steam/.config/unity3d/LuGus Studios/Liftoff");
            out.push(("Linux Flatpak Steam".to_string(), flatpak));
        }
    }

    out
}
