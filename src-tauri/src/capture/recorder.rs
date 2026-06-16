use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tokio::net::UdpSocket;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::interval;

use crate::capture::rawcap::{FileHeader, RawCapWriter};
use crate::capture::stats::{CaptureStats, StatsAccumulator};
use crate::error::{AppError, AppResult};

pub struct RecorderHandle {
    pub capture_id: String,
    pub raw_file_path: PathBuf,
    pub markers_file_path: PathBuf,
    pub bind_addr: SocketAddr,
    pub start_utc: DateTime<Utc>,
    pub start_instant: Instant,
    pub stats_rx: watch::Receiver<CaptureStats>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<AppResult<RecorderResult>>>,
}

#[derive(Debug, Clone)]
pub struct RecorderResult {
    pub packet_count: u64,
    pub byte_count: u64,
    pub bytes_written: u64,
    pub duration_seconds: f64,
    pub final_stats: CaptureStats,
}

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub capture_id: String,
    pub raw_file_path: PathBuf,
    pub markers_file_path: PathBuf,
    pub bind_addr: SocketAddr,
    pub app_version: String,
    pub telemetry_config_hash: Option<String>,
    pub stats_interval: Duration,
}

/// A socket the auto-capture supervisor already bound and (optionally) received
/// the first telemetry packet on. Handing both to the recorder means no rebind
/// window and no lost packets between detection and recording.
pub struct PreboundSocket {
    pub socket: UdpSocket,
    pub first_packet: Option<(Vec<u8>, SocketAddr)>,
}

pub async fn start(config: RecorderConfig) -> AppResult<RecorderHandle> {
    start_with(config, None).await
}

pub async fn start_with(
    config: RecorderConfig,
    prebound: Option<PreboundSocket>,
) -> AppResult<RecorderHandle> {
    let (socket, first_packet) = match prebound {
        Some(p) => (p.socket, p.first_packet),
        None => {
            let socket = UdpSocket::bind(config.bind_addr)
                .await
                .map_err(|err| AppError::udp_bind(config.bind_addr, err))?;
            (socket, None)
        }
    };
    let bind_addr = socket.local_addr()?;
    let start_utc = Utc::now();
    let start_instant = Instant::now();

    let header = FileHeader {
        format_version: crate::capture::rawcap::FORMAT_VERSION,
        capture_id: config.capture_id.clone(),
        created_at: start_utc.to_rfc3339(),
        app_version: config.app_version.clone(),
        telemetry_config_hash: config.telemetry_config_hash.clone(),
    };
    let writer = RawCapWriter::create(&config.raw_file_path, &header)?;

    let initial_stats = CaptureStats::new(
        config.capture_id.clone(),
        config.raw_file_path.to_string_lossy().to_string(),
    );
    let (stats_tx, stats_rx) = watch::channel(initial_stats);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let task_config = config.clone();
    let join = tokio::spawn(run_recorder(
        socket,
        first_packet,
        writer,
        task_config,
        start_instant,
        stats_tx,
        shutdown_rx,
    ));

    Ok(RecorderHandle {
        capture_id: config.capture_id,
        raw_file_path: config.raw_file_path,
        markers_file_path: config.markers_file_path,
        bind_addr,
        start_utc,
        start_instant,
        stats_rx,
        shutdown_tx: Some(shutdown_tx),
        join: Some(join),
    })
}

impl RecorderHandle {
    pub async fn stop(mut self) -> AppResult<RecorderResult> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        match self.join.take() {
            Some(join) => join.await?,
            None => Err(AppError::InvalidState("recorder already stopped".into())),
        }
    }
}

async fn run_recorder(
    socket: UdpSocket,
    first_packet: Option<(Vec<u8>, SocketAddr)>,
    mut writer: RawCapWriter,
    config: RecorderConfig,
    start_instant: Instant,
    stats_tx: watch::Sender<CaptureStats>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> AppResult<RecorderResult> {
    let mut buf = vec![0u8; 65_535];
    let mut accumulator = StatsAccumulator::new();
    let mut stats_timer = interval(config.stats_interval);
    stats_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Packet that triggered an auto-start: write it as the capture's first record.
    if let Some((data, addr)) = first_packet {
        let monotonic_ns = start_instant.elapsed().as_nanos() as u64;
        let utc_ns = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let bytes_written = writer.write_packet(monotonic_ns, utc_ns, addr, &data)?;
        accumulator.record_packet(data.len(), addr.to_string(), bytes_written);
    }

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => break,
            res = socket.recv_from(&mut buf) => {
                match res {
                    Ok((n, addr)) => {
                        let monotonic_ns = start_instant.elapsed().as_nanos() as u64;
                        let utc_ns = Utc::now().timestamp_nanos_opt().unwrap_or(0);
                        let bytes_written = writer.write_packet(monotonic_ns, utc_ns, addr, &buf[..n])?;
                        accumulator.record_packet(n, addr.to_string(), bytes_written);
                    }
                    Err(e) => {
                        tracing::warn!("udp recv error: {}", e);
                    }
                }
            }
            _ = stats_timer.tick() => {
                let snapshot = accumulator.snapshot(
                    &config.capture_id,
                    &config.raw_file_path.to_string_lossy(),
                    "recording",
                );
                let _ = stats_tx.send(snapshot);
            }
        }
    }

    let bytes_written = writer.finalize()?;
    let final_stats = accumulator.snapshot(
        &config.capture_id,
        &config.raw_file_path.to_string_lossy(),
        "completed",
    );
    let result = RecorderResult {
        packet_count: final_stats.packet_count,
        byte_count: final_stats.byte_count,
        bytes_written,
        duration_seconds: final_stats.duration_seconds,
        final_stats: final_stats.clone(),
    };
    let _ = stats_tx.send(final_stats);
    Ok(result)
}
