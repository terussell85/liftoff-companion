//! Integration test stitching the pieces the Tauri capture/processing commands
//! rely on (which can't be called directly without a Tauri runtime): DB pool +
//! migrations → insert capture → gamelog provisional sessions → telemetry fusion
//! → replace + list.

use std::path::PathBuf;

use chrono::Utc;
use liftoff_companion_lib::gamelog::segment::build_gamelog_sessions;
use liftoff_companion_lib::gamelog::tailer::SegmentBoundary;
use liftoff_companion_lib::liftoff::player_log::DetectedContext;
use liftoff_companion_lib::processing::segmentation::{
    extract_telemetry_signals, fuse_sessions, SegmentationConfig,
};
use liftoff_companion_lib::storage::db::open_pool;
use liftoff_companion_lib::storage::repositories::{self, NewCapture};
use liftoff_companion_lib::telemetry::sample::{TelemetrySample, Vec3};

fn tempdir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("whoop_seg_it_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn ctx(level: &str, race: &str) -> DetectedContext {
    DetectedContext {
        environment_raw: level.into(),
        level: level.into(),
        game_mode: Some("Race".into()),
        drone: Some("Air75".into()),
        track: Some(race.into()),
        race: Some(race.into()),
        race_guid: Some("75f61b19".into()),
        title: Some("Liftoff Micro Drones".into()),
    }
}

#[test]
fn provisional_then_fused_sessions_persist() {
    let dir = tempdir();
    let pool = open_pool(&dir.join("whoop.db")).expect("pool + migrations");

    // Insert a capture row.
    let capture_id = "cap_it".to_string();
    {
        let conn = pool.get().unwrap();
        repositories::insert_capture(
            &conn,
            &NewCapture {
                id: capture_id.clone(),
                created_at: Utc::now(),
                status: "recording".into(),
                source_type: "udp".into(),
                source_config_json: None,
                raw_file_path: dir.join("packets.rawcap").to_string_lossy().into(),
                context_json: None,
                app_version: Some("test".into()),
                telemetry_config_hash: None,
            },
        )
        .unwrap();
    }

    // Provisional sessions from a single gamelog track boundary (2s..stop).
    let secs = |s: i64| s * 1_000_000_000;
    let boundaries = vec![SegmentBoundary {
        monotonic_ns: secs(2),
        context: Some(ctx("Azure District", "01 - Garage Galore")),
        is_menu: false,
        from_backfill: false,
    }];
    let provisional = build_gamelog_sessions(&capture_id, &boundaries, secs(40));
    assert_eq!(provisional.len(), 1);
    {
        let conn = pool.get().unwrap();
        for s in &provisional {
            repositories::insert_race_session(&conn, s).unwrap();
        }
        let listed = repositories::list_race_sessions(&conn, &capture_id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].segmentation_method, "gamelog");
        assert_eq!(listed[0].level.as_deref(), Some("Azure District"));
    }

    // Build telemetry: idle 0..5s, flying 5..35s, idle 35..40s.
    let mut samples = Vec::new();
    for i in 0..400 {
        let t = i as f64 * 0.1;
        let speed = if (5.0..35.0).contains(&t) { 8.0 } else { 0.0 };
        samples.push(TelemetrySample {
            capture_time_seconds: t,
            sim_time: Some(t as f32),
            velocity: Some(Vec3 {
                x: speed,
                y: 0.0,
                z: 0.0,
            }),
            ..Default::default()
        });
    }

    // Fuse and replace.
    let cfg = SegmentationConfig::default();
    let signals = extract_telemetry_signals(&samples, &cfg);
    let gamelog_sessions = {
        let conn = pool.get().unwrap();
        repositories::list_race_sessions(&conn, &capture_id).unwrap()
    };
    let refined = fuse_sessions(&capture_id, &gamelog_sessions, &signals, 0.0, 40.0, &cfg);
    assert_eq!(refined.len(), 1);
    {
        let mut conn = pool.get().unwrap();
        repositories::replace_race_sessions(&mut conn, &capture_id, &refined).unwrap();
        let listed = repositories::list_race_sessions(&conn, &capture_id).unwrap();
        assert_eq!(listed.len(), 1, "replaced, not duplicated");
        let s = &listed[0];
        assert_eq!(s.segmentation_method, "gamelog+telemetry");
        // Identity preserved from the gamelog, edges trimmed to the flying span.
        assert_eq!(s.level.as_deref(), Some("Azure District"));
        assert_eq!(s.race.as_deref(), Some("01 - Garage Galore"));
        assert!(
            s.start_seconds >= 4.5 && s.start_seconds <= 6.0,
            "start {}",
            s.start_seconds
        );
        assert!(s.end_seconds.unwrap() <= 36.0, "end {:?}", s.end_seconds);
    }

    let _ = std::fs::remove_dir_all(&dir);
}
