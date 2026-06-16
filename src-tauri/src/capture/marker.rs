use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerRecord {
    pub id: String,
    pub capture_id: String,
    pub created_at: DateTime<Utc>,
    pub monotonic_ns: Option<i64>,
    #[serde(rename = "type")]
    pub marker_type: String,
    pub note: Option<String>,
}

pub fn append_marker(path: &Path, marker: &MarkerRecord) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(marker)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn read_markers(path: &Path) -> AppResult<Vec<MarkerRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let marker: MarkerRecord = serde_json::from_str(&line)?;
        out.push(marker);
    }
    Ok(out)
}
