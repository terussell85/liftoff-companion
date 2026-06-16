use std::path::Path;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

use crate::error::AppResult;
use crate::storage::migrations;

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn open_pool(db_path: &Path) -> AppResult<DbPool> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Per-connection pragmas only. journal_mode is a *persistent* property of the
    // database file, so it must NOT go here — r2d2 opens all pool connections at
    // once and a concurrent WAL switch races and fails with "database is locked".
    // busy_timeout makes concurrent writers (capture commands, the game-log
    // tailer's auto-markers, processing) wait for the WAL writer lock instead.
    let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )
    });
    let pool = Pool::builder().max_size(8).build(manager)?;

    {
        let conn = pool.get()?;
        // Set WAL once; it persists in the file for all future connections.
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        migrations::run(&conn)?;
        seed_defaults(&conn)?;
    }

    Ok(pool)
}

fn seed_defaults(conn: &rusqlite::Connection) -> AppResult<()> {
    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM processing_profiles", [], |r| r.get(0))?;
    if count == 0 {
        let now = chrono::Utc::now().to_rfc3339();
        let config = serde_json::json!({
            "profile_id": "default-v1",
            "parser_version": "liftoff-udp-v1",
            "description": "Default telemetry processing profile with race segmentation and telemetry-only collision detection."
        });
        conn.execute(
            "INSERT INTO processing_profiles (id, name, created_at, config_json, is_default) \
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![
                "default-v1",
                "Default v1",
                now,
                serde_json::to_string(&config)?
            ],
        )?;
    }
    Ok(())
}
