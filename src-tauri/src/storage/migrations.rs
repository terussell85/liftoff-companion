use rusqlite::Connection;

use crate::error::AppResult;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("migrations/001_initial.sql")),
    (2, include_str!("migrations/002_race_sessions.sql")),
    (3, include_str!("migrations/003_game_asset_cache.sql")),
    (4, include_str!("migrations/004_collision_metrics.sql")),
    (
        5,
        include_str!("migrations/005_collision_geometry_cache.sql"),
    ),
    (6, include_str!("migrations/006_race_timing.sql")),
];

pub fn run(conn: &Connection) -> AppResult<()> {
    let current: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (version, sql) in MIGRATIONS {
        if *version > current {
            conn.execute_batch(sql)?;
            conn.execute_batch(&format!("PRAGMA user_version = {};", version))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    #[test]
    fn fresh_db_applies_all_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        assert!(table_exists(&conn, "captures"));
        assert!(table_exists(&conn, "race_sessions"));
        assert!(table_exists(&conn, "game_asset_caches"));
        assert!(table_exists(&conn, "race_course_cache"));
        assert!(table_exists(&conn, "collision_geometry_cache"));
        assert!(table_exists(&conn, "race_laps"));
        assert!(table_exists(&conn, "race_gate_splits"));
        assert!(table_exists(&conn, "race_passage_events"));
        let v: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 6);
    }

    #[test]
    fn upgrades_v1_database_to_v2() {
        let conn = Connection::open_in_memory().unwrap();
        // Simulate an existing v1 DB: apply only migration 001 and stamp version 1.
        conn.execute_batch(MIGRATIONS[0].1).unwrap();
        conn.execute_batch("PRAGMA user_version = 1;").unwrap();
        assert!(!table_exists(&conn, "race_sessions"));

        run(&conn).unwrap();
        assert!(table_exists(&conn, "race_sessions"));

        // Idempotent: running again is a no-op.
        run(&conn).unwrap();
        let v: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 6);
    }
}
