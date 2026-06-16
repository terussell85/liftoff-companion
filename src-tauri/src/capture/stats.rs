use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureStats {
    pub capture_id: String,
    pub packet_count: u64,
    pub byte_count: u64,
    pub bytes_written: u64,
    pub packet_rate_hz: f32,
    pub duration_seconds: f64,
    pub last_packet_at_utc: Option<DateTime<Utc>>,
    pub last_source_addr: Option<String>,
    pub raw_file_path: String,
    pub status: String,
}

impl CaptureStats {
    pub fn new(capture_id: String, raw_file_path: String) -> Self {
        Self {
            capture_id,
            packet_count: 0,
            byte_count: 0,
            bytes_written: 0,
            packet_rate_hz: 0.0,
            duration_seconds: 0.0,
            last_packet_at_utc: None,
            last_source_addr: None,
            raw_file_path,
            status: "recording".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatsAccumulator {
    started_at: Instant,
    packet_count: u64,
    byte_count: u64,
    bytes_written: u64,
    last_window_count: u64,
    last_window_at: Instant,
    last_packet_at_utc: Option<DateTime<Utc>>,
    last_source_addr: Option<String>,
}

impl StatsAccumulator {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            started_at: now,
            packet_count: 0,
            byte_count: 0,
            bytes_written: 0,
            last_window_count: 0,
            last_window_at: now,
            last_packet_at_utc: None,
            last_source_addr: None,
        }
    }

    pub fn record_packet(&mut self, payload_len: usize, source_addr: String, written_bytes: u64) {
        self.packet_count = self.packet_count.saturating_add(1);
        self.byte_count = self.byte_count.saturating_add(payload_len as u64);
        self.bytes_written = written_bytes;
        self.last_packet_at_utc = Some(Utc::now());
        self.last_source_addr = Some(source_addr);
    }

    pub fn snapshot(
        &mut self,
        capture_id: &str,
        raw_file_path: &str,
        status: &str,
    ) -> CaptureStats {
        let now = Instant::now();
        let window = now.duration_since(self.last_window_at).as_secs_f32();
        let packet_rate_hz = if window > 0.0 {
            let delta = (self.packet_count - self.last_window_count) as f32;
            delta / window
        } else {
            0.0
        };
        self.last_window_at = now;
        self.last_window_count = self.packet_count;

        CaptureStats {
            capture_id: capture_id.to_string(),
            packet_count: self.packet_count,
            byte_count: self.byte_count,
            bytes_written: self.bytes_written,
            packet_rate_hz,
            duration_seconds: now.duration_since(self.started_at).as_secs_f64(),
            last_packet_at_utc: self.last_packet_at_utc,
            last_source_addr: self.last_source_addr.clone(),
            raw_file_path: raw_file_path.to_string(),
            status: status.to_string(),
        }
    }

    pub fn total_duration_seconds(&self) -> f64 {
        Instant::now().duration_since(self.started_at).as_secs_f64()
    }

    pub fn packet_count(&self) -> u64 {
        self.packet_count
    }

    pub fn byte_count(&self) -> u64 {
        self.byte_count
    }
}

impl Default for StatsAccumulator {
    fn default() -> Self {
        Self::new()
    }
}
