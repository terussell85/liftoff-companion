use std::path::PathBuf;
use std::time::Duration;

use liftoff_companion_lib::capture::integrity::compute_file_hash;
use liftoff_companion_lib::capture::rawcap::RawCapReader;
use liftoff_companion_lib::capture::recorder::{self, RecorderConfig};
use liftoff_companion_lib::error::AppError;
use liftoff_companion_lib::telemetry::liftoff_schema::{build_canonical_packet, LiftoffSchema};
use liftoff_companion_lib::telemetry::parser::parse_packet;
use tokio::net::UdpSocket;
use tokio::time::sleep;

fn tempdir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("whoop_it_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[tokio::test]
async fn recorder_captures_canonical_packets() {
    let dir = tempdir();
    let raw_file_path = dir.join("packets.rawcap");
    let markers_file_path = dir.join("markers.jsonl");
    let cfg = RecorderConfig {
        capture_id: "cap_test".to_string(),
        raw_file_path: raw_file_path.clone(),
        markers_file_path,
        // Bind to ephemeral port so the OS picks an available one.
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        app_version: "test/0.0".to_string(),
        telemetry_config_hash: Some("deadbeef".to_string()),
        stats_interval: Duration::from_millis(50),
    };

    let handle = recorder::start(cfg).await.expect("recorder start");
    let target = handle.bind_addr;

    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sender.connect(target).await.unwrap();

    let total_packets = 250u64;
    for seq in 0..total_packets {
        let t = seq as f32 * 0.01;
        let packet = build_canonical_packet(
            t,
            (seq as f32 * 0.1, 1.0, 0.0),
            (0.0, 0.0, 0.0, 1.0),
            (1.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            (0.5, 0.0, 0.0, 0.0),
            (12.0, 90.0),
            &[1000.0, 1010.0, 990.0, 1005.0],
        );
        sender.send(&packet).await.unwrap();
        // Pace the sender so the recv loop has a chance to read each packet.
        if seq.is_multiple_of(20) {
            sleep(Duration::from_millis(1)).await;
        }
    }

    // Give the recorder a moment to drain remaining packets.
    sleep(Duration::from_millis(100)).await;

    let result = handle.stop().await.expect("recorder stop");
    assert!(
        result.packet_count >= total_packets - 5,
        "expected near {} packets, got {}",
        total_packets,
        result.packet_count
    );

    // Replay to confirm parity with the in-memory counter.
    let replay = RawCapReader::open(&raw_file_path).unwrap();
    let replay_count = replay.count_packets().unwrap();
    assert_eq!(replay_count, result.packet_count);

    // Hash the file and confirm it's deterministic.
    let h1 = compute_file_hash(&raw_file_path).unwrap();
    let h2 = compute_file_hash(&raw_file_path).unwrap();
    assert_eq!(h1, h2);

    // Parse the first packet and verify field values.
    let schema = LiftoffSchema::canonical("127.0.0.1:9001");
    let mut reader = RawCapReader::open(&raw_file_path).unwrap();
    let first = reader.next_record().unwrap().unwrap();
    let (sample, warnings) = parse_packet(&first.payload, &schema, first.sequence_number, 0.0);
    assert!(
        warnings.is_empty(),
        "unexpected parser warnings: {:?}",
        warnings
    );
    let pos = sample.position.expect("position");
    assert!((pos.y - 1.0).abs() < 1e-5);
    let battery = sample.battery.expect("battery");
    assert!((battery.voltage - 12.0).abs() < 1e-5);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn recorder_reports_udp_endpoint_conflicts() {
    let occupied = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let bind_addr = occupied.local_addr().unwrap();

    let dir = tempdir();
    let cfg = RecorderConfig {
        capture_id: "cap_conflict".to_string(),
        raw_file_path: dir.join("packets.rawcap"),
        markers_file_path: dir.join("markers.jsonl"),
        bind_addr,
        app_version: "test/0.0".to_string(),
        telemetry_config_hash: None,
        stats_interval: Duration::from_millis(50),
    };

    let err = match recorder::start(cfg).await {
        Ok(handle) => {
            let _ = handle.stop().await;
            panic!("recorder unexpectedly started on occupied UDP endpoint");
        }
        Err(err) => err,
    };

    match err {
        AppError::UdpEndpointInUse { endpoint } => {
            assert_eq!(endpoint, bind_addr.to_string());
        }
        other => panic!("unexpected error: {other}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}
