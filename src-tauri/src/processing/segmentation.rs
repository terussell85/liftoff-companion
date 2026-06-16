//! Telemetry-fusion race-session segmentation.
//!
//! The gamelog gives authoritative track identity + coarse windows
//! (`gamelog/segment.rs`). Here we refine those windows using telemetry signals
//! — drone-reset `sim_time` resets, movement start, and idle gaps — and, when no
//! gamelog is available, derive sessions from telemetry alone. All times here
//! are in the **sample** base (`capture_time_seconds`, relative to the first
//! packet); we convert to/from the capture-start base via `offset_seconds`.

use crate::storage::repositories::RaceSessionRow;
use crate::telemetry::sample::TelemetrySample;

const NS_PER_SEC: f64 = 1_000_000_000.0;

/// Minimum length (s) of a run produced by splitting on a reset. Cuts that would
/// leave a shorter sliver are skipped (the reset is absorbed into the adjacent
/// run), guarding against noisy/duplicate `sim_time` drops.
const MIN_SUBSESSION_SECONDS: f64 = 1.0;

#[derive(Debug, Clone)]
pub struct SegmentationConfig {
    /// Speed (m/s) above which the drone counts as actively flying.
    pub min_speed: f32,
    /// Idle gap (s) that closes an active span.
    pub idle_seconds: f64,
    /// A `sim_time` reset is detected when it drops from above this…
    pub reset_min_prev: f32,
    /// …to below this.
    pub reset_max_cur: f32,
    /// Snap a refined start to a reset within this many seconds.
    pub snap_tolerance_seconds: f64,
}

impl Default for SegmentationConfig {
    fn default() -> Self {
        Self {
            min_speed: 0.5,
            idle_seconds: 3.0,
            reset_min_prev: 1.0,
            reset_max_cur: 0.5,
            snap_tolerance_seconds: 2.0,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TelemetrySignals {
    /// Times (sample base) where `sim_time` reset toward zero (drone reset).
    pub resets: Vec<f64>,
    /// Contiguous active-flying spans (sample base), merged across short idle.
    pub active_spans: Vec<(f64, f64)>,
}

fn speed_of(s: &TelemetrySample) -> f32 {
    s.velocity.map(|v| v.magnitude()).unwrap_or(0.0)
}

pub fn extract_telemetry_signals(
    samples: &[TelemetrySample],
    cfg: &SegmentationConfig,
) -> TelemetrySignals {
    let mut resets = Vec::new();
    let mut prev_sim: Option<f32> = None;
    for s in samples {
        if let (Some(prev), Some(cur)) = (prev_sim, s.sim_time) {
            if prev > cfg.reset_min_prev && cur < cfg.reset_max_cur && cur < prev {
                resets.push(s.capture_time_seconds);
            }
        }
        if s.sim_time.is_some() {
            prev_sim = s.sim_time;
        }
    }

    // Active spans: open on movement, close after idle longer than idle_seconds.
    let mut spans = Vec::new();
    let mut start: Option<f64> = None;
    let mut last_move = 0.0f64;
    for s in samples {
        let t = s.capture_time_seconds;
        if speed_of(s) > cfg.min_speed {
            if start.is_none() {
                start = Some(t);
            }
            last_move = t;
        } else if let Some(st) = start {
            if t - last_move > cfg.idle_seconds {
                spans.push((st, last_move));
                start = None;
            }
        }
    }
    if let Some(st) = start {
        spans.push((st, last_move));
    }

    TelemetrySignals {
        resets,
        active_spans: spans,
    }
}

/// Refine gamelog sessions with telemetry, or derive telemetry-only sessions.
/// `offset_seconds` is the first packet's capture-start-relative time, so
/// `sample_time + offset_seconds = capture-start time`.
pub fn fuse_sessions(
    capture_id: &str,
    gamelog_sessions: &[RaceSessionRow],
    signals: &TelemetrySignals,
    offset_seconds: f64,
    total_seconds: f64,
    cfg: &SegmentationConfig,
) -> Vec<RaceSessionRow> {
    if gamelog_sessions.is_empty() {
        return telemetry_only_sessions(capture_id, signals, offset_seconds);
    }

    let mut out = Vec::new();
    // Running index so that splitting one gamelog session into several runs still
    // yields globally-unique, contiguous session indices.
    let mut index: i64 = 0;
    for g in gamelog_sessions {
        // gamelog window in sample base
        let gs = (g.start_seconds - offset_seconds).max(0.0);
        let ge = g.end_seconds.unwrap_or(total_seconds) - offset_seconds;

        match active_overlap(&signals.active_spans, gs, ge) {
            Some((mut rs, re)) => {
                // Snap start to a nearby reset (more precise run start). This is
                // also the first split point; later interior resets each start a
                // new run with the same track identity.
                if let Some(reset) = nearest_reset(&signals.resets, rs, cfg.snap_tolerance_seconds)
                {
                    rs = reset.max(gs);
                }
                for (wrs, wre) in
                    split_window_at_resets(&signals.resets, rs, re, MIN_SUBSESSION_SECONDS)
                {
                    out.push(refined_row(
                        capture_id,
                        index,
                        g,
                        wrs,
                        wre,
                        offset_seconds,
                        "gamelog+telemetry",
                        0.95,
                    ));
                    index += 1;
                }
            }
            // No telemetry support inside the window → keep gamelog window.
            None => {
                out.push(refined_row(
                    capture_id,
                    index,
                    g,
                    gs,
                    ge,
                    offset_seconds,
                    "gamelog",
                    0.9,
                ));
                index += 1;
            }
        }
    }
    out
}

fn telemetry_only_sessions(
    capture_id: &str,
    signals: &TelemetrySignals,
    offset_seconds: f64,
) -> Vec<RaceSessionRow> {
    let mut out = Vec::new();
    let mut index: i64 = 0;
    for &(s, e) in &signals.active_spans {
        for (ws, we) in split_window_at_resets(&signals.resets, s, e, MIN_SUBSESSION_SECONDS) {
            let start_seconds = ws + offset_seconds;
            let end_seconds = we + offset_seconds;
            out.push(RaceSessionRow {
                id: format!("rs_{}", uuid::Uuid::new_v4().simple()),
                capture_id: capture_id.to_string(),
                session_index: index,
                start_monotonic_ns: (start_seconds * NS_PER_SEC) as i64,
                end_monotonic_ns: Some((end_seconds * NS_PER_SEC) as i64),
                start_seconds,
                end_seconds: Some(end_seconds),
                duration_seconds: Some(end_seconds - start_seconds),
                level: None,
                race: None,
                track: None,
                game_mode: None,
                drone: None,
                race_guid: None,
                title: None,
                segmentation_method: "telemetry".to_string(),
                confidence: Some(0.5),
                collision_count: 0,
                collision_max_severity: 0,
                collision_avg_severity: None,
            });
            index += 1;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn refined_row(
    capture_id: &str,
    index: i64,
    g: &RaceSessionRow,
    rs_sample: f64,
    re_sample: f64,
    offset_seconds: f64,
    method: &str,
    confidence: f64,
) -> RaceSessionRow {
    let start_seconds = rs_sample + offset_seconds;
    let end_seconds = re_sample + offset_seconds;
    RaceSessionRow {
        id: format!("rs_{}", uuid::Uuid::new_v4().simple()),
        capture_id: capture_id.to_string(),
        session_index: index,
        start_monotonic_ns: (start_seconds * NS_PER_SEC) as i64,
        end_monotonic_ns: Some((end_seconds * NS_PER_SEC) as i64),
        start_seconds,
        end_seconds: Some(end_seconds),
        duration_seconds: Some(end_seconds - start_seconds),
        level: g.level.clone(),
        race: g.race.clone(),
        track: g.track.clone(),
        game_mode: g.game_mode.clone(),
        drone: g.drone.clone(),
        race_guid: g.race_guid.clone(),
        title: g.title.clone(),
        segmentation_method: method.to_string(),
        confidence: Some(confidence),
        collision_count: 0,
        collision_max_severity: 0,
        collision_avg_severity: None,
    }
}

/// Intersection of active spans with [gs, ge] → (earliest start, latest end).
fn active_overlap(spans: &[(f64, f64)], gs: f64, ge: f64) -> Option<(f64, f64)> {
    let mut rs: Option<f64> = None;
    let mut re: Option<f64> = None;
    for &(a, b) in spans {
        let s = a.max(gs);
        let e = b.min(ge);
        if s < e {
            rs = Some(rs.map_or(s, |x| x.min(s)));
            re = Some(re.map_or(e, |x| x.max(e)));
        }
    }
    match (rs, re) {
        (Some(s), Some(e)) => Some((s, e)),
        _ => None,
    }
}

/// Split an active window `[rs, re]` (sample base) at each drone-reset that
/// falls strictly inside it, into ordered, contiguous sub-windows — one per run
/// attempt. Always returns at least one window. Cuts within `min_len` of the
/// current segment's start or of `re` are skipped so no sliver is emitted.
fn split_window_at_resets(resets: &[f64], rs: f64, re: f64, min_len: f64) -> Vec<(f64, f64)> {
    let mut cuts: Vec<f64> = resets
        .iter()
        .copied()
        .filter(|&r| r > rs && r < re)
        .collect();
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut windows = Vec::new();
    let mut seg_start = rs;
    for c in cuts {
        // Skip cuts that would leave too little on either side.
        if c - seg_start < min_len || re - c < min_len {
            continue;
        }
        windows.push((seg_start, c));
        seg_start = c;
    }
    windows.push((seg_start, re));
    windows
}

fn nearest_reset(resets: &[f64], near: f64, tol: f64) -> Option<f64> {
    resets
        .iter()
        .copied()
        .filter(|r| (r - near).abs() <= tol)
        .min_by(|a, b| {
            (a - near)
                .abs()
                .partial_cmp(&(b - near).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::sample::{TelemetrySample, Vec3};

    fn sample(t: f64, speed: f32, sim: f32) -> TelemetrySample {
        TelemetrySample {
            capture_time_seconds: t,
            sim_time: Some(sim),
            velocity: Some(Vec3 {
                x: speed,
                y: 0.0,
                z: 0.0,
            }),
            ..Default::default()
        }
    }

    fn gamelog_session(start: f64, end: f64) -> RaceSessionRow {
        RaceSessionRow {
            id: "g".into(),
            capture_id: "cap".into(),
            session_index: 0,
            start_monotonic_ns: (start * NS_PER_SEC) as i64,
            end_monotonic_ns: Some((end * NS_PER_SEC) as i64),
            start_seconds: start,
            end_seconds: Some(end),
            duration_seconds: Some(end - start),
            level: Some("Azure District".into()),
            race: Some("01 - Garage Galore".into()),
            track: Some("01 - Garage Galore".into()),
            game_mode: Some("Race".into()),
            drone: Some("Air75".into()),
            race_guid: Some("guid".into()),
            title: Some("Liftoff Micro Drones".into()),
            segmentation_method: "gamelog".into(),
            confidence: Some(0.9),
            collision_count: 0,
            collision_max_severity: 0,
            collision_avg_severity: None,
        }
    }

    /// Build samples: idle, then flying with a sim_time reset partway through.
    fn flying_samples() -> Vec<TelemetrySample> {
        let mut v = Vec::new();
        // 0..5s idle
        for i in 0..50 {
            v.push(sample(i as f64 * 0.1, 0.0, (i as f64 * 0.1) as f32));
        }
        // 5..35s flying; reset at 20s (sim_time drops to ~0)
        for i in 50..350 {
            let t = i as f64 * 0.1;
            let sim = if t < 20.0 {
                (t - 5.0) as f32
            } else {
                (t - 20.0) as f32
            };
            v.push(sample(t, 8.0, sim.max(0.0)));
        }
        // 35..40s idle
        for i in 350..400 {
            v.push(sample(i as f64 * 0.1, 0.0, 0.0));
        }
        v
    }

    #[test]
    fn extracts_active_span_and_reset() {
        let cfg = SegmentationConfig::default();
        let sig = extract_telemetry_signals(&flying_samples(), &cfg);
        assert_eq!(sig.active_spans.len(), 1, "one merged active span");
        let (s, e) = sig.active_spans[0];
        assert!((s - 5.0).abs() < 0.2, "fly-start ~5s, got {s}");
        assert!((e - 34.9).abs() < 0.3, "fly-end ~35s, got {e}");
        assert!(
            sig.resets.iter().any(|&r| (r - 20.0).abs() < 0.2),
            "reset near 20s, got {:?}",
            sig.resets
        );
    }

    #[test]
    fn fuses_gamelog_window_with_telemetry() {
        let cfg = SegmentationConfig::default();
        let sig = extract_telemetry_signals(&flying_samples(), &cfg);
        // gamelog says the track spanned 2..40s (with menu/idle padding).
        let gl = vec![gamelog_session(2.0, 40.0)];
        let fused = fuse_sessions("cap", &gl, &sig, 0.0, 40.0, &cfg);
        // The reset at ~20s splits the single run into two sub-sessions.
        assert_eq!(fused.len(), 2, "reset at ~20s splits the run");
        for (i, f) in fused.iter().enumerate() {
            assert_eq!(f.segmentation_method, "gamelog+telemetry");
            assert_eq!(f.level.as_deref(), Some("Azure District")); // identity preserved
            assert_eq!(f.race.as_deref(), Some("01 - Garage Galore"));
            assert_eq!(f.session_index, i as i64);
            assert!(f.confidence.unwrap() > 0.9);
        }
        // First run: fly-start ~5s to the reset ~20s.
        assert!(
            fused[0].start_seconds >= 4.5 && fused[0].start_seconds <= 6.0,
            "start {}",
            fused[0].start_seconds
        );
        assert!(
            (fused[0].end_seconds.unwrap() - 20.0).abs() < 0.3,
            "split {:?}",
            fused[0].end_seconds
        );
        // Second run: reset ~20s to fly-end ~35s.
        assert!(
            (fused[1].start_seconds - 20.0).abs() < 0.3,
            "start {}",
            fused[1].start_seconds
        );
        assert!(
            fused[1].end_seconds.unwrap() <= 36.0,
            "end {:?}",
            fused[1].end_seconds
        );
    }

    #[test]
    fn telemetry_only_when_no_gamelog() {
        let cfg = SegmentationConfig::default();
        let sig = extract_telemetry_signals(&flying_samples(), &cfg);
        let fused = fuse_sessions("cap", &[], &sig, 0.0, 40.0, &cfg);
        // The reset at ~20s also splits the telemetry-only active span.
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].session_index, 0);
        assert_eq!(fused[1].session_index, 1);
        for f in &fused {
            assert_eq!(f.segmentation_method, "telemetry");
            assert_eq!(f.level, None);
        }
    }

    #[test]
    fn split_window_at_resets_basic() {
        // Two interior resets → three contiguous runs.
        assert_eq!(
            split_window_at_resets(&[15.0, 25.0], 5.0, 35.0, 1.0),
            vec![(5.0, 15.0), (15.0, 25.0), (25.0, 35.0)]
        );
        // Resets outside the window are ignored.
        assert_eq!(
            split_window_at_resets(&[2.0, 40.0], 5.0, 35.0, 1.0),
            vec![(5.0, 35.0)]
        );
        // Cuts that would leave a sub-1s sliver at either edge are dropped.
        assert_eq!(
            split_window_at_resets(&[5.3, 34.8], 5.0, 35.0, 1.0),
            vec![(5.0, 35.0)]
        );
        // Closely-spaced resets coalesce to one boundary.
        assert_eq!(
            split_window_at_resets(&[15.0, 15.5], 5.0, 35.0, 1.0),
            vec![(5.0, 15.0), (15.0, 35.0)]
        );
    }

    #[test]
    fn splits_gamelog_session_on_two_resets() {
        let cfg = SegmentationConfig::default();
        let signals = TelemetrySignals {
            resets: vec![15.0, 25.0],
            active_spans: vec![(5.0, 35.0)],
        };
        let gl = vec![gamelog_session(2.0, 40.0)];
        let fused = fuse_sessions("cap", &gl, &signals, 0.0, 40.0, &cfg);
        assert_eq!(fused.len(), 3, "initial + two restarts");
        for (i, f) in fused.iter().enumerate() {
            assert_eq!(f.session_index, i as i64);
            assert_eq!(f.level.as_deref(), Some("Azure District"));
            assert_eq!(f.race.as_deref(), Some("01 - Garage Galore"));
            assert_eq!(f.segmentation_method, "gamelog+telemetry");
        }
        // Contiguous, split at the reset times.
        assert!((fused[0].end_seconds.unwrap() - 15.0).abs() < 1e-6);
        assert!((fused[1].start_seconds - 15.0).abs() < 1e-6);
        assert!((fused[1].end_seconds.unwrap() - 25.0).abs() < 1e-6);
        assert!((fused[2].start_seconds - 25.0).abs() < 1e-6);
    }

    #[test]
    fn global_session_index_across_two_gamelog_sessions() {
        let cfg = SegmentationConfig::default();
        // First gamelog session has an interior reset (splits into two runs);
        // the second has none. Indices must stay globally contiguous: 0,1,2.
        let signals = TelemetrySignals {
            resets: vec![15.0],
            active_spans: vec![(5.0, 28.0), (32.0, 58.0)],
        };
        let gl = vec![gamelog_session(2.0, 30.0), gamelog_session(30.0, 60.0)];
        let fused = fuse_sessions("cap", &gl, &signals, 0.0, 60.0, &cfg);
        assert_eq!(fused.len(), 3);
        assert_eq!(
            fused.iter().map(|s| s.session_index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }
}
