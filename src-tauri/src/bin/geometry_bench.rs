use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use liftoff_companion_lib::liftoff::assets::{self, AssetRefreshProgress};
use liftoff_companion_lib::storage::{db, repositories};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct NormalizedGeometryRow {
    scope_kind: String,
    scope_id: String,
    status: String,
    source_bundle: Option<String>,
    source_hash: Option<String>,
    warning_count: i64,
    geometry_json: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct NormalizedRun {
    cache_id: String,
    rows: Vec<NormalizedGeometryRow>,
}

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let data_root = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: geometry_bench <Unity Data root> [output.json]"))?;
    let output = args.next().map(PathBuf::from);

    let db_path = env::temp_dir().join(format!(
        "liftoff-geometry-bench-{}.sqlite3",
        std::process::id()
    ));
    let pool = db::open_pool(&db_path)?;
    let mut conn = pool.get()?;

    let started = Instant::now();
    let mut last_phase = String::new();
    let mut last_event = Instant::now();
    let mut geometry_started = None;
    let mut geometry_events = 0u64;
    let mut max_bundles_total = 0u64;

    let root_string = data_root.to_string_lossy().to_string();
    let statuses = assets::refresh_sources(&mut conn, true, Some(&root_string), |progress| {
        print_progress(
            &progress,
            started,
            &mut last_phase,
            &mut last_event,
            &mut geometry_started,
            &mut geometry_events,
            &mut max_bundles_total,
        );
    })?;

    let elapsed = started.elapsed();
    let cache_id = statuses
        .iter()
        .find_map(|status| status.cache_id.clone())
        .ok_or_else(|| anyhow::anyhow!("refresh did not produce a cache id"))?;
    let rows = repositories::list_collision_geometry_for_cache(&conn, &cache_id)?;
    let mut ready = 0usize;
    let mut partial = 0usize;
    let mut missing = 0usize;
    let mut shapes = 0usize;
    let mut normalized_rows = Vec::new();

    for row in rows {
        match row.status.as_str() {
            "ready" => ready += 1,
            "partial" => partial += 1,
            "missing" => missing += 1,
            _ => {}
        }
        let geometry_json: serde_json::Value = serde_json::from_str(&row.geometry_json)?;
        shapes += geometry_json
            .get("shapes")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        normalized_rows.push(NormalizedGeometryRow {
            scope_kind: row.scope_kind,
            scope_id: row.scope_id,
            status: row.status,
            source_bundle: row.source_bundle,
            source_hash: row.source_hash,
            warning_count: row.warning_count,
            geometry_json,
        });
    }
    normalized_rows.sort_by(|a, b| {
        a.scope_kind
            .cmp(&b.scope_kind)
            .then_with(|| a.scope_id.cmp(&b.scope_id))
    });

    eprintln!(
        "summary elapsed={:.3}s rows={} ready={} partial={} missing={} shapes={} geometry_events={} max_bundles_total={}",
        elapsed.as_secs_f64(),
        normalized_rows.len(),
        ready,
        partial,
        missing,
        shapes,
        geometry_events,
        max_bundles_total
    );

    if let Some(output) = output {
        let run = NormalizedRun {
            cache_id,
            rows: normalized_rows,
        };
        fs::write(output, serde_json::to_vec_pretty(&run)?)?;
    }

    let _ = fs::remove_file(&db_path);
    let _ = fs::remove_file(db_path.with_extension("sqlite3-shm"));
    let _ = fs::remove_file(db_path.with_extension("sqlite3-wal"));
    Ok(())
}

fn print_progress(
    progress: &AssetRefreshProgress,
    started: Instant,
    last_phase: &mut String,
    last_event: &mut Instant,
    geometry_started: &mut Option<Instant>,
    geometry_events: &mut u64,
    max_bundles_total: &mut u64,
) {
    if progress.phase.starts_with("geometry_") {
        *geometry_events += 1;
        *max_bundles_total = (*max_bundles_total).max(progress.bundles_total);
        if progress.phase == "geometry_started" && geometry_started.is_none() {
            *geometry_started = Some(Instant::now());
        }
    }

    let now = Instant::now();
    let bundle_phase = matches!(
        progress.phase.as_str(),
        "geometry_bundle_started" | "geometry_bundle_completed"
    );
    let should_print = (!bundle_phase && progress.phase != *last_phase)
        || now.duration_since(*last_event).as_secs_f64() >= 5.0
        || progress.phase == "geometry_completed";
    if !should_print {
        return;
    }
    *last_phase = progress.phase.clone();
    *last_event = now;
    let geometry_elapsed = geometry_started
        .map(|start| now.duration_since(start).as_secs_f64())
        .unwrap_or(0.0);
    eprintln!(
        "progress elapsed={:.3}s geometry={:.3}s phase={} scopes={}/{} bundles={}/{} current_scope={} current_bundle={} races={} tracks={}",
        now.duration_since(started).as_secs_f64(),
        geometry_elapsed,
        progress.phase,
        progress.scopes_done,
        progress.scopes_total,
        progress.bundles_done,
        progress.bundles_total,
        progress.current_scope.as_deref().unwrap_or("-"),
        progress.current_bundle.as_deref().unwrap_or("-"),
        progress.races_found,
        progress.tracks_found
    );
}
