use std::path::PathBuf;

use tauri::{AppHandle, Emitter};

use crate::commands::capture_commands::record_marker;
use crate::gamelog::tailer::{GameContextEvent, TailerSink};
use crate::storage::db::DbPool;

/// Production [`TailerSink`]: emits the `game_context_detected` event and writes
/// auto-markers through the same path as manual markers.
pub struct AppSink {
    pub app: AppHandle,
    pub db: DbPool,
    pub markers_file_path: PathBuf,
    pub capture_id: String,
}

impl TailerSink for AppSink {
    fn on_context(&self, event: &GameContextEvent) {
        let _ = self.app.emit("game_context_detected", event);
    }

    fn on_marker(&self, marker_type: &str, note: Option<String>, monotonic_ns: i64) {
        let _ = record_marker(
            &self.db,
            &self.app,
            &self.markers_file_path,
            &self.capture_id,
            monotonic_ns,
            marker_type,
            note,
        );
    }
}
