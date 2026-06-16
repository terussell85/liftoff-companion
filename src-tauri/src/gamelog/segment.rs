//! Provisional race-session segmentation from game-log boundaries.
//!
//! Each `Level setup:` boundary starts a session (one per track run); it ends at
//! the next boundary, a menu return, or capture stop. This is the fast,
//! authoritative-identity pass run at `stop_capture`; the processing layer later
//! refines the *edges* using telemetry (`processing/segmentation.rs`).

use crate::gamelog::tailer::SegmentBoundary;
use crate::storage::repositories::RaceSessionRow;

const NS_PER_SEC: f64 = 1_000_000_000.0;

/// Build sessions from ordered boundaries. `capture_end_monotonic_ns` closes the
/// final open session. Menu boundaries only close the current session (no new one).
pub fn build_gamelog_sessions(
    capture_id: &str,
    boundaries: &[SegmentBoundary],
    capture_end_monotonic_ns: i64,
) -> Vec<RaceSessionRow> {
    let mut sorted: Vec<&SegmentBoundary> = boundaries.iter().collect();
    sorted.sort_by_key(|b| b.monotonic_ns);

    let mut sessions = Vec::new();
    let mut index: i64 = 0;
    let mut open: Option<(&SegmentBoundary, i64)> = None; // (track boundary, start ns)

    let close = |open: &mut Option<(&SegmentBoundary, i64)>,
                 end_ns: i64,
                 idx: &mut i64,
                 out: &mut Vec<RaceSessionRow>| {
        if let Some((b, start)) = open.take() {
            if end_ns <= start {
                return;
            }
            out.push(row_from(capture_id, *idx, b, start, end_ns));
            *idx += 1;
        }
    };

    for b in sorted {
        if b.is_menu {
            close(&mut open, b.monotonic_ns, &mut index, &mut sessions);
        } else {
            // a new track boundary closes the previous session and opens a new one
            close(&mut open, b.monotonic_ns, &mut index, &mut sessions);
            open = Some((b, b.monotonic_ns));
        }
    }
    close(
        &mut open,
        capture_end_monotonic_ns,
        &mut index,
        &mut sessions,
    );

    sessions
}

fn row_from(
    capture_id: &str,
    index: i64,
    b: &SegmentBoundary,
    start_ns: i64,
    end_ns: i64,
) -> RaceSessionRow {
    let ctx = b.context.as_ref();
    let start_seconds = start_ns as f64 / NS_PER_SEC;
    let end_seconds = end_ns as f64 / NS_PER_SEC;
    RaceSessionRow {
        id: format!("rs_{}", uuid::Uuid::new_v4().simple()),
        capture_id: capture_id.to_string(),
        session_index: index,
        start_monotonic_ns: start_ns,
        end_monotonic_ns: Some(end_ns),
        start_seconds,
        end_seconds: Some(end_seconds),
        duration_seconds: Some(end_seconds - start_seconds),
        level: ctx.map(|c| c.level.clone()),
        race: ctx.and_then(|c| c.race.clone()),
        track: ctx.and_then(|c| c.track.clone()),
        game_mode: ctx.and_then(|c| c.game_mode.clone()),
        drone: ctx.and_then(|c| c.drone.clone()),
        race_guid: ctx.and_then(|c| c.race_guid.clone()),
        title: ctx.and_then(|c| c.title.clone()),
        segmentation_method: "gamelog".to_string(),
        confidence: Some(0.9),
        collision_count: 0,
        collision_max_severity: 0,
        collision_avg_severity: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liftoff::player_log::DetectedContext;

    fn ctx(level: &str, race: &str) -> DetectedContext {
        DetectedContext {
            environment_raw: level.into(),
            level: level.into(),
            game_mode: Some("Race".into()),
            drone: Some("Air75".into()),
            track: Some(race.into()),
            race: Some(race.into()),
            race_guid: Some("guid".into()),
            title: Some("Liftoff Micro Drones".into()),
        }
    }

    fn track_boundary(ns: i64, level: &str, race: &str) -> SegmentBoundary {
        SegmentBoundary {
            monotonic_ns: ns,
            context: Some(ctx(level, race)),
            is_menu: false,
            from_backfill: false,
        }
    }

    #[test]
    fn two_tracks_with_menu_gap() {
        let secs = |s: i64| s * 1_000_000_000;
        let boundaries = vec![
            track_boundary(secs(2), "Azure District", "01 - Garage Galore"),
            SegmentBoundary {
                monotonic_ns: secs(40),
                context: None,
                is_menu: true,
                from_backfill: false,
            },
            track_boundary(secs(55), "In Transit", "01 - Order Picking"),
        ];
        let sessions = build_gamelog_sessions("cap", &boundaries, secs(90));
        assert_eq!(sessions.len(), 2);

        assert_eq!(sessions[0].session_index, 0);
        assert_eq!(sessions[0].level.as_deref(), Some("Azure District"));
        assert_eq!(sessions[0].race.as_deref(), Some("01 - Garage Galore"));
        assert!((sessions[0].start_seconds - 2.0).abs() < 1e-6);
        assert!((sessions[0].end_seconds.unwrap() - 40.0).abs() < 1e-6); // closed by menu
        assert_eq!(sessions[0].segmentation_method, "gamelog");

        assert_eq!(sessions[1].level.as_deref(), Some("In Transit"));
        assert!((sessions[1].start_seconds - 55.0).abs() < 1e-6);
        assert!((sessions[1].end_seconds.unwrap() - 90.0).abs() < 1e-6); // closed by capture end
    }

    #[test]
    fn back_to_back_tracks_without_menu() {
        let secs = |s: i64| s * 1_000_000_000;
        let boundaries = vec![
            track_boundary(secs(1), "Azure District", "A"),
            track_boundary(secs(30), "Azure District", "B"),
        ];
        let sessions = build_gamelog_sessions("cap", &boundaries, secs(60));
        assert_eq!(sessions.len(), 2);
        assert!((sessions[0].end_seconds.unwrap() - 30.0).abs() < 1e-6);
        assert_eq!(sessions[1].race.as_deref(), Some("B"));
    }
}
