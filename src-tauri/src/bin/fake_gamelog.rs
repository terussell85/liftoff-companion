//! Dev helper: append synthetic Liftoff `Player.log` lines to a target file so
//! the game-log tailer (and race-session segmentation) can be exercised without
//! launching the game. Pair with `fake_liftoff` (UDP telemetry) for a full loop.
//!
//! Example:
//!   cargo run --bin fake_gamelog -- --path /tmp/Player.log --tracks 2 --fly-secs 8

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

struct Args {
    path: String,
    tracks: usize,
    fly_secs: u64,
}

fn parse_args() -> Result<Args> {
    let mut path = None;
    let mut tracks = 2usize;
    let mut fly_secs = 8u64;
    let mut iter = env::args().skip(1);
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--path" => path = iter.next(),
            "--tracks" => {
                tracks = iter
                    .next()
                    .ok_or_else(|| anyhow!("--tracks needs value"))?
                    .parse()
                    .context("tracks")?
            }
            "--fly-secs" => {
                fly_secs = iter
                    .next()
                    .ok_or_else(|| anyhow!("--fly-secs needs value"))?
                    .parse()
                    .context("fly-secs")?
            }
            "-h" | "--help" => {
                println!(
                    "fake_gamelog --path <Player.log> [--tracks N] [--fly-secs S]\n\
                     Appends N synthetic 'Level setup:' blocks, each followed by a flight\n\
                     and a return to the menu."
                );
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown flag: {}", other)),
        }
    }
    Ok(Args {
        path: path.ok_or_else(|| anyhow!("--path is required"))?,
        tracks,
        fly_secs,
    })
}

const ENVS: &[(&str, &str)] = &[
    ("SilverScreen", "01 - Garage Galore"),
    ("InTransit", "01 - Order Picking"),
    ("HovertonHigh", "02 - Hall Pass"),
];

fn append(path: &str, text: &str) -> Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(text.as_bytes())?;
    f.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let args = parse_args()?;
    println!(
        "appending {} synthetic track(s) to {} ({}s flight each)",
        args.tracks, args.path, args.fly_secs
    );

    for i in 0..args.tracks {
        let (env_name, race) = ENVS[i % ENVS.len()];
        let block = format!(
            "Level setup:\n\
             Flags: Race\n\
             Environment: {env_name}\n\
             Type: DRONE\n\
             Name: [Copy] Air75\n\
             Status: Player-created\n\
             Local ID: 1c3e3d0d-515d-46a3-a2fa-23bb97e7e744\n\
             Type: TRACK\n\
             Name: {race}\n\
             Status: Internal\n\
             Local ID: b7830037-571b-4f04-a3b1-3eb5b3850ad9\n\
             Type: RACE\n\
             Name: {race}\n\
             Status: Internal\n\
             Local ID: 75f61b19-504d-49c0-8f88-3a791b6e8441\n\
             Disabling all controller mappings.\n\
             Enabling controller mapping: Flight.\n"
        );
        append(&args.path, &block)?;
        println!("  track {}: {env_name} / {race} — flying…", i + 1);
        sleep(Duration::from_secs(args.fly_secs));

        // Pause back to the menu (ends the segment).
        append(
            &args.path,
            "Drone reset locked by \"x\".\n\
             Enabling controller mapping: Menu.\n\
             ================================= SCENE LOAD START: XSMainMenu ===================\n",
        )?;
        sleep(Duration::from_secs(1));
    }

    println!("done.");
    Ok(())
}
