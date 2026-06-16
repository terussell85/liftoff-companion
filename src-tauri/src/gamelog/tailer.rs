use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::interval;

use crate::error::AppResult;
use crate::liftoff::paths::PlayerLogCandidate;
use crate::liftoff::player_log::{find_last_context, DetectedContext, LineParser, LogEvent};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A pre-start log is trusted for backfill only if modified within this window
/// of capture start. Generous because steady flight may not flush for a while;
/// truncation invalidation is the precise backstop for mid-capture relaunch.
const BACKFILL_FRESHNESS_MS: i64 = 120_000; // 2 minutes

/// Payload for the `game_context_detected` Tauri event.
#[derive(Debug, Clone, Serialize)]
pub struct GameContextEvent {
    pub capture_id: String,
    pub title: Option<String>,
    pub level: String,
    pub environment_raw: String,
    pub game_mode: Option<String>,
    pub drone: Option<String>,
    pub track: Option<String>,
    pub race: Option<String>,
    pub race_guid: Option<String>,
}

impl GameContextEvent {
    fn from_context(capture_id: &str, c: &DetectedContext) -> Self {
        Self {
            capture_id: capture_id.to_string(),
            title: c.title.clone(),
            level: c.level.clone(),
            environment_raw: c.environment_raw.clone(),
            game_mode: c.game_mode.clone(),
            drone: c.drone.clone(),
            track: c.track.clone(),
            race: c.race.clone(),
            race_guid: c.race_guid.clone(),
        }
    }
}

/// A segment edge derived from the log. Track boundaries carry context; menu
/// returns (and the implicit capture-stop edge) end the current segment.
#[derive(Debug, Clone)]
pub struct SegmentBoundary {
    pub monotonic_ns: i64,
    pub context: Option<DetectedContext>,
    pub is_menu: bool,
    /// Seeded from a pre-start log by `backfill`. Such a boundary may reflect a
    /// prior session and is dropped if the game relaunches mid-capture (the log
    /// truncates). Live boundaries set this `false`.
    pub from_backfill: bool,
}

#[derive(Debug, Default)]
pub struct LogTailerResult {
    pub boundaries: Vec<SegmentBoundary>,
    pub last_context: Option<DetectedContext>,
    pub lines_written: u64,
}

/// Side-effect sink. The real impl (DB + Tauri) lives in `sink.rs`; tests use a
/// recording stub. `monotonic_ns` is measured against the capture start.
pub trait TailerSink: Send + Sync + 'static {
    fn on_context(&self, event: &GameContextEvent);
    fn on_marker(&self, marker_type: &str, note: Option<String>, monotonic_ns: i64);
}

pub struct TailerParams {
    pub capture_id: String,
    pub start_instant: Instant,
    pub gamelog_file_path: PathBuf,
    pub logs: Vec<PlayerLogCandidate>,
}

pub struct LogTailerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<AppResult<LogTailerResult>>>,
}

impl LogTailerHandle {
    pub async fn stop(mut self) -> AppResult<LogTailerResult> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        match self.join.take() {
            Some(join) => join.await?,
            None => Ok(LogTailerResult::default()),
        }
    }
}

pub fn start(params: TailerParams, sink: Arc<dyn TailerSink>) -> LogTailerHandle {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let join = tokio::spawn(run(params, sink, shutdown_rx));
    LogTailerHandle {
        shutdown_tx: Some(shutdown_tx),
        join: Some(join),
    }
}

struct FileState {
    title: Option<String>,
    path: PathBuf,
    file: Option<File>,
    pos: u64,
    partial: String,
    parser: LineParser,
}

impl FileState {
    fn open_at_end(candidate: &PlayerLogCandidate) -> Self {
        let (file, pos) = match File::open(&candidate.path) {
            Ok(mut f) => {
                let end = f.seek(SeekFrom::End(0)).unwrap_or(0);
                (Some(f), end)
            }
            Err(_) => (None, 0),
        };
        Self {
            title: Some(candidate.title.clone()),
            path: candidate.path.clone(),
            file,
            pos,
            partial: String::new(),
            parser: LineParser::new(Some(candidate.title.clone())),
        }
    }

    /// Read bytes appended since the last poll, returning `(truncated, lines)`.
    /// Detects truncation/rotation (file shrank, or a previously-absent file
    /// reappeared) and reopens from the start. `truncated` is true when that
    /// reopen happens after we already held content — i.e. the game relaunched
    /// mid-capture — so callers can invalidate stale pre-start state.
    fn read_new_lines(&mut self) -> (bool, Vec<String>) {
        let len = match std::fs::metadata(&self.path) {
            Ok(m) => m.len(),
            Err(_) => return (false, Vec::new()),
        };
        let mut truncated = false;
        if len < self.pos || self.file.is_none() {
            // truncated/rotated/reappeared — reopen and reparse from the top.
            // `pos > 0` means we'd already advanced into the old file, so the
            // shrink is a genuine mid-capture relaunch (the game recreated the
            // log) rather than the harmless first read.
            truncated = self.pos > 0;
            self.file = File::open(&self.path).ok();
            self.pos = 0;
            self.partial.clear();
            self.parser = LineParser::new(self.title.clone());
        }
        let file = match self.file.as_mut() {
            Some(f) => f,
            None => return (truncated, Vec::new()),
        };
        if file.seek(SeekFrom::Start(self.pos)).is_err() {
            return (truncated, Vec::new());
        }
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return (truncated, Vec::new());
        }
        self.pos += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));

        let mut lines = Vec::new();
        while let Some(idx) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=idx).collect();
            lines.push(line);
        }
        (truncated, lines)
    }
}

async fn run(
    params: TailerParams,
    sink: Arc<dyn TailerSink>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> AppResult<LogTailerResult> {
    let mut result = LogTailerResult::default();

    if let Some(parent) = params.gamelog_file_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut gamelog = File::create(&params.gamelog_file_path)
        .ok()
        .map(BufWriter::new);

    let mut files: Vec<FileState> = params
        .logs
        .iter()
        .filter(|c| c.exists)
        .map(FileState::open_at_end)
        .collect();

    backfill(&params, &sink, &mut result);

    let mut ticker = interval(POLL_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                poll(&params, &sink, &mut files, &mut gamelog, &mut result);
                break;
            }
            _ = ticker.tick() => {
                poll(&params, &sink, &mut files, &mut gamelog, &mut result);
            }
        }
    }

    if let Some(g) = gamelog.as_mut() {
        let _ = g.flush();
    }
    Ok(result)
}

/// Read the most recent `Level setup:` from the freshest existing log so a
/// capture started mid-flight already knows the current level.
fn backfill(params: &TailerParams, sink: &Arc<dyn TailerSink>, result: &mut LogTailerResult) {
    let now_ms = Utc::now().timestamp_millis();
    let mut best: Option<(i64, DetectedContext)> = None;
    for cand in params.logs.iter().filter(|c| c.exists) {
        // Freshness gate: a log not written recently is almost certainly left
        // over from a prior session, so its last `Level setup:` is stale. Skip
        // it (unknown mtime is treated as stale).
        let fresh = cand
            .modified_ms
            .is_some_and(|m| now_ms.saturating_sub(m) <= BACKFILL_FRESHNESS_MS);
        if !fresh {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&cand.path) {
            if let Some(ctx) = find_last_context(&text, Some(cand.title.clone())) {
                let score = cand.modified_ms.unwrap_or(0);
                if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                    best = Some((score, ctx));
                }
            }
        }
    }
    if let Some((_, ctx)) = best {
        let mono = params.start_instant.elapsed().as_nanos() as i64;
        sink.on_context(&GameContextEvent::from_context(&params.capture_id, &ctx));
        result.boundaries.push(SegmentBoundary {
            monotonic_ns: mono,
            context: Some(ctx.clone()),
            is_menu: false,
            from_backfill: true,
        });
        result.last_context = Some(ctx);
    }
}

/// A mid-capture truncation means the game (re)launched, so any context seeded
/// from the pre-start log is from a prior run — drop it. Clears `last_context`
/// only if no live (non-menu) boundary has superseded it yet.
fn invalidate_backfill(result: &mut LogTailerResult) {
    if !result.boundaries.iter().any(|b| b.from_backfill) {
        return;
    }
    result.boundaries.retain(|b| !b.from_backfill);
    if !result
        .boundaries
        .iter()
        .any(|b| !b.is_menu && b.context.is_some())
    {
        result.last_context = None;
    }
}

#[derive(Serialize)]
struct GamelogRecord<'a> {
    monotonic_ns: i64,
    utc_ns: i64,
    title: Option<&'a str>,
    event: &'a LogEvent,
    raw: &'a str,
}

fn poll(
    params: &TailerParams,
    sink: &Arc<dyn TailerSink>,
    files: &mut [FileState],
    gamelog: &mut Option<BufWriter<File>>,
    result: &mut LogTailerResult,
) {
    for fs in files.iter_mut() {
        let title = fs.title.clone();
        let (truncated, lines) = fs.read_new_lines();
        if truncated {
            invalidate_backfill(result);
        }
        for line in lines {
            let raw = line.trim_end_matches(['\r', '\n']);
            for event in fs.parser.push_line(&line) {
                let mono = params.start_instant.elapsed().as_nanos() as i64;
                let utc_ns = Utc::now().timestamp_nanos_opt().unwrap_or(0);
                write_gamelog(gamelog, &event, title.as_deref(), mono, utc_ns, raw, result);
                handle_event(params, sink, result, event, mono);
            }
        }
    }
}

fn write_gamelog(
    gamelog: &mut Option<BufWriter<File>>,
    event: &LogEvent,
    title: Option<&str>,
    monotonic_ns: i64,
    utc_ns: i64,
    raw: &str,
    result: &mut LogTailerResult,
) {
    if let Some(g) = gamelog.as_mut() {
        let rec = GamelogRecord {
            monotonic_ns,
            utc_ns,
            title,
            event,
            raw,
        };
        if let Ok(json) = serde_json::to_string(&rec) {
            if g.write_all(json.as_bytes()).is_ok() && g.write_all(b"\n").is_ok() {
                result.lines_written += 1;
            }
        }
    }
}

fn handle_event(
    params: &TailerParams,
    sink: &Arc<dyn TailerSink>,
    result: &mut LogTailerResult,
    event: LogEvent,
    mono: i64,
) {
    match event {
        LogEvent::LevelSetup(ctx) => {
            sink.on_context(&GameContextEvent::from_context(&params.capture_id, &ctx));
            let note = match (&ctx.race, &ctx.game_mode) {
                (Some(r), Some(m)) => Some(format!("{r} · {m}")),
                (Some(r), None) => Some(r.clone()),
                (None, _) => Some(ctx.level.clone()),
            };
            sink.on_marker("track_loaded", note, mono);
            result.boundaries.push(SegmentBoundary {
                monotonic_ns: mono,
                context: Some(ctx.clone()),
                is_menu: false,
                from_backfill: false,
            });
            result.last_context = Some(ctx);
        }
        LogEvent::SceneLoad { is_menu: true, .. } => {
            result.boundaries.push(SegmentBoundary {
                monotonic_ns: mono,
                context: None,
                is_menu: true,
                from_backfill: false,
            });
        }
        LogEvent::FlightActive => sink.on_marker("flight_active", None, mono),
        LogEvent::Paused => sink.on_marker("paused", None, mono),
        // Scene loads into environments and reset lock/unlock are recorded in
        // gamelog.jsonl only — too noisy for markers.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration as StdDuration;

    #[derive(Default)]
    struct RecordingSink {
        contexts: Mutex<Vec<GameContextEvent>>,
        markers: Mutex<Vec<(String, Option<String>)>>,
    }
    impl TailerSink for RecordingSink {
        fn on_context(&self, event: &GameContextEvent) {
            self.contexts.lock().unwrap().push(event.clone());
        }
        fn on_marker(&self, marker_type: &str, note: Option<String>, _mono: i64) {
            self.markers
                .lock()
                .unwrap()
                .push((marker_type.to_string(), note));
        }
    }

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("whoop_gamelog_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    const BLOCK: &str = "\
Level setup:
Flags: Race
Environment: SilverScreen
Type: DRONE
Name: Air75
Status: Player-created
Local ID: aaa
Type: RACE
Name: 01 - Garage Galore
Status: Internal
Local ID: 75f61b19-504d-49c0-8f88-3a791b6e8441
Disabling all controller mappings.
Enabling controller mapping: Flight.
";

    #[tokio::test]
    async fn tails_appended_block_and_records_marker_and_gamelog() {
        let dir = tempdir();
        let log_path = dir.join("Player.log");
        std::fs::write(&log_path, "boot line\n").unwrap();
        let gamelog_path = dir.join("gamelog.jsonl");

        let sink = Arc::new(RecordingSink::default());
        let params = TailerParams {
            capture_id: "cap_test".into(),
            start_instant: Instant::now(),
            gamelog_file_path: gamelog_path.clone(),
            logs: vec![PlayerLogCandidate {
                title: "Liftoff Micro Drones".into(),
                path: log_path.clone(),
                exists: true,
                modified_ms: Some(1),
            }],
        };
        let handle = start(params, sink.clone());

        // Give the tailer a moment to seek to EOF, then append a Level setup block.
        tokio::time::sleep(StdDuration::from_millis(150)).await;
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_path)
                .unwrap();
            f.write_all(BLOCK.as_bytes()).unwrap();
            f.flush().unwrap();
        }
        tokio::time::sleep(StdDuration::from_millis(700)).await;

        let result = handle.stop().await.unwrap();

        let contexts = sink.contexts.lock().unwrap();
        assert!(
            contexts
                .iter()
                .any(|c| c.level == "Azure District"
                    && c.race.as_deref() == Some("01 - Garage Galore")),
            "expected detected context, got {:?}",
            *contexts
        );
        let markers = sink.markers.lock().unwrap();
        assert!(markers.iter().any(|(t, _)| t == "track_loaded"));
        assert!(markers.iter().any(|(t, _)| t == "flight_active"));

        assert!(result.lines_written >= 2, "gamelog records written");
        assert!(result.last_context.is_some());
        let gl = std::fs::read_to_string(&gamelog_path).unwrap();
        assert!(gl.contains("\"level_setup\""));
        assert!(gl.contains("Azure District") || gl.contains("SilverScreen"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn backfills_existing_block_on_start() {
        let dir = tempdir();
        let log_path = dir.join("Player.log");
        std::fs::write(&log_path, BLOCK).unwrap();

        let sink = Arc::new(RecordingSink::default());
        let params = TailerParams {
            capture_id: "cap_bf".into(),
            start_instant: Instant::now(),
            gamelog_file_path: dir.join("gamelog.jsonl"),
            logs: vec![PlayerLogCandidate {
                title: "Liftoff Micro Drones".into(),
                path: log_path.clone(),
                exists: true,
                // Fresh mtime so the freshness gate trusts this mid-flight log.
                modified_ms: Some(Utc::now().timestamp_millis()),
            }],
        };
        let handle = start(params, sink.clone());
        tokio::time::sleep(StdDuration::from_millis(150)).await;
        let result = handle.stop().await.unwrap();

        // Pre-existing block is surfaced via backfill, not the live tail.
        assert_eq!(
            result.last_context.as_ref().map(|c| c.level.as_str()),
            Some("Azure District")
        );
        assert!(sink
            .contexts
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.level == "Azure District"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A log left over from a prior session (old mtime) must NOT be backfilled —
    /// otherwise the first race inherits a stale level (the 0–18s bug).
    #[tokio::test]
    async fn stale_backfill_skipped_when_log_is_old() {
        let dir = tempdir();
        let log_path = dir.join("Player.log");
        std::fs::write(&log_path, BLOCK).unwrap();

        let sink = Arc::new(RecordingSink::default());
        let old_ms = Utc::now().timestamp_millis() - 10 * 60 * 1000; // 10 min ago
        let params = TailerParams {
            capture_id: "cap_stale".into(),
            start_instant: Instant::now(),
            gamelog_file_path: dir.join("gamelog.jsonl"),
            logs: vec![PlayerLogCandidate {
                title: "Liftoff Micro Drones".into(),
                path: log_path.clone(),
                exists: true,
                modified_ms: Some(old_ms),
            }],
        };
        let handle = start(params, sink.clone());
        tokio::time::sleep(StdDuration::from_millis(150)).await;
        let result = handle.stop().await.unwrap();

        assert!(result.last_context.is_none(), "stale log must not backfill");
        assert!(result.boundaries.is_empty(), "no backfill boundary");
        assert!(sink.contexts.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Capture starts, a fresh-but-prior block is backfilled, then the game
    /// relaunches (log truncated) and a *different* level loads. The backfilled
    /// context must be dropped and replaced by the live one.
    #[tokio::test]
    async fn truncation_after_start_invalidates_backfill() {
        let dir = tempdir();
        let log_path = dir.join("Player.log");
        // Pre-start content with Azure District, fresh mtime so backfill seeds it.
        std::fs::write(&log_path, BLOCK).unwrap();

        let sink = Arc::new(RecordingSink::default());
        let params = TailerParams {
            capture_id: "cap_trunc".into(),
            start_instant: Instant::now(),
            gamelog_file_path: dir.join("gamelog.jsonl"),
            logs: vec![PlayerLogCandidate {
                title: "Liftoff Micro Drones".into(),
                path: log_path.clone(),
                exists: true,
                modified_ms: Some(Utc::now().timestamp_millis()),
            }],
        };
        let handle = start(params, sink.clone());
        // Let the tailer seek to EOF and run the backfill.
        tokio::time::sleep(StdDuration::from_millis(150)).await;

        // Game relaunches: truncate the log and write a different level. Keep it
        // shorter than the prior content so the tailer sees the file shrink
        // (`len < pos`) and treats it as a relaunch.
        let new_block = "\
Level setup:
Flags: Race
Environment: InTransit
Disabling all controller mappings.
";
        std::fs::write(&log_path, new_block).unwrap(); // truncate + rewrite
        tokio::time::sleep(StdDuration::from_millis(700)).await;

        let result = handle.stop().await.unwrap();

        assert_eq!(
            result.last_context.as_ref().map(|c| c.level.as_str()),
            Some("In Transit"),
            "live level should replace the backfilled one"
        );
        assert!(
            !result.boundaries.iter().any(|b| b.from_backfill),
            "stale backfill boundary must be dropped on truncation"
        );
        assert!(
            result
                .boundaries
                .iter()
                .any(|b| b.context.as_ref().map(|c| c.level.as_str()) == Some("In Transit")),
            "live In Transit boundary present"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// If the freshest log ends in a menu scene-load after its last `Level
    /// setup:`, the player is back in a menu — there is no active track to seed.
    #[tokio::test]
    async fn menu_after_last_level_setup_skips_backfill() {
        let dir = tempdir();
        let log_path = dir.join("Player.log");
        let text = format!(
            "{BLOCK}Enabling controller mapping: Menu.\n\
             ================================= SCENE LOAD START: XSMainMenu ===================\n"
        );
        std::fs::write(&log_path, text).unwrap();

        let sink = Arc::new(RecordingSink::default());
        let params = TailerParams {
            capture_id: "cap_menu".into(),
            start_instant: Instant::now(),
            gamelog_file_path: dir.join("gamelog.jsonl"),
            logs: vec![PlayerLogCandidate {
                title: "Liftoff Micro Drones".into(),
                path: log_path.clone(),
                exists: true,
                modified_ms: Some(Utc::now().timestamp_millis()),
            }],
        };
        let handle = start(params, sink.clone());
        tokio::time::sleep(StdDuration::from_millis(150)).await;
        let result = handle.stop().await.unwrap();

        assert!(
            result.last_context.is_none(),
            "log ending in a menu must not backfill a track"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
