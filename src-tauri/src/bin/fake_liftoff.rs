use std::env;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::net::UdpSocket;
use tokio::time::sleep;

use liftoff_companion_lib::telemetry::liftoff_schema::build_canonical_packet;

struct Args {
    addr: SocketAddr,
    rate_hz: f64,
    duration_s: Option<f64>,
}

fn parse_args() -> Result<Args> {
    let mut addr = "127.0.0.1:9001".to_string();
    let mut rate_hz = 100.0f64;
    let mut duration_s = None;

    let mut iter = env::args().skip(1);
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--addr" => addr = iter.next().ok_or_else(|| anyhow!("--addr needs value"))?,
            "--rate-hz" => {
                rate_hz = iter
                    .next()
                    .ok_or_else(|| anyhow!("--rate-hz needs value"))?
                    .parse()
                    .context("rate-hz")?;
            }
            "--duration-s" => {
                duration_s = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("--duration-s needs value"))?
                        .parse()
                        .context("duration-s")?,
                );
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown flag: {}", other)),
        }
    }
    Ok(Args {
        addr: addr.parse().context("addr")?,
        rate_hz,
        duration_s,
    })
}

fn print_help() {
    println!(
        "fake_liftoff — synthetic Liftoff telemetry UDP emitter\n\
         \n\
         Flags:\n  --addr 127.0.0.1:9001   target endpoint\n  \
         --rate-hz 100           packets per second\n  \
         --duration-s 60         stop after N seconds (otherwise runs forever)\n  \
         -h, --help              show this help"
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    println!(
        "emitting canonical Liftoff packets to {} at {:.1} Hz{}",
        args.addr,
        args.rate_hz,
        match args.duration_s {
            Some(d) => format!(" for {:.1}s", d),
            None => " (Ctrl+C to stop)".to_string(),
        }
    );

    let period = Duration::from_secs_f64(1.0 / args.rate_hz.max(0.001));
    let start = Instant::now();
    let deadline = args.duration_s.map(|d| start + Duration::from_secs_f64(d));
    let mut sequence: u64 = 0;
    let mut next_send = Instant::now();

    loop {
        if let Some(d) = deadline {
            if Instant::now() >= d {
                break;
            }
        }
        let t = start.elapsed().as_secs_f32();
        let packet = synthetic_packet(t, sequence);
        // UDP is fire-and-forget; ignore send errors that come from prior ICMP unreachable.
        let _ = socket.send_to(&packet, args.addr).await;
        sequence = sequence.wrapping_add(1);

        next_send += period;
        let now = Instant::now();
        if next_send > now {
            sleep(next_send - now).await;
        } else {
            next_send = now;
        }
    }
    println!("done. emitted {} packets", sequence);
    Ok(())
}

fn synthetic_packet(t: f32, seq: u64) -> Vec<u8> {
    // Slow figure-eight in xz plane, gentle altitude wave on y.
    let speed = 5.0; // m/s
    let radius = 4.0;
    let pos_x = (t * 0.5).sin() * radius;
    let pos_z = (t * 0.5 * 2.0).sin() * radius;
    let pos_y = 1.5 + (t * 0.3).sin() * 0.5;

    let vel_x = (t * 0.5).cos() * radius * 0.5 * speed * 0.2;
    let vel_z = (t * 0.5 * 2.0).cos() * radius * 1.0 * speed * 0.2;
    let vel_y = (t * 0.3).cos() * 0.5 * 0.3;

    // Identity quaternion slowly rotating around y.
    let angle = (t * 0.4).rem_euclid(std::f32::consts::TAU);
    let attitude = (0.0, (angle * 0.5).sin(), 0.0, (angle * 0.5).cos());

    let gyro = (
        (t * 1.5).sin() * 30.0,
        (t * 1.3).cos() * 20.0,
        (t * 0.9).sin() * 10.0,
    );

    let throttle = 0.5 + (t * 0.7).sin() * 0.2;
    let input = (
        throttle.clamp(0.0, 1.0),
        0.0,
        (t * 0.4).sin() * 0.3,
        (t * 0.5).cos() * 0.3,
    );

    // Battery slowly drains over 5 min.
    let drain_per_sec = 100.0 / (5.0 * 60.0);
    let pct = (100.0 - t * drain_per_sec).clamp(0.0, 100.0);
    let voltage = 12.6 - (1.0 - pct / 100.0) * 1.8;
    let battery = (voltage, pct);

    let rpm = 1000.0 + throttle * 8000.0 + (seq as f32 * 0.1).sin() * 50.0;
    let motors = [rpm, rpm * 1.01, rpm * 0.99, rpm * 1.005];

    build_canonical_packet(
        t,
        (pos_x, pos_y, pos_z),
        attitude,
        (vel_x, vel_y, vel_z),
        gyro,
        input,
        battery,
        &motors,
    )
}
