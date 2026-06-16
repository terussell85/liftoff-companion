//! Live tailing of Liftoff's `Player.log` during a capture.
//!
//! Side effects (DB writes, Tauri events) are isolated behind [`tailer::TailerSink`]
//! so the file-reading + parsing loop is testable without a Tauri app. The pure
//! log grammar lives in `crate::liftoff::player_log`.
pub mod segment;
pub mod sink;
pub mod tailer;
