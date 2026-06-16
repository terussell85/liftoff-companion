CREATE TABLE IF NOT EXISTS race_sessions (
  id TEXT PRIMARY KEY,
  capture_id TEXT NOT NULL,
  session_index INTEGER NOT NULL,
  start_monotonic_ns INTEGER NOT NULL,
  end_monotonic_ns INTEGER,
  start_seconds REAL NOT NULL,
  end_seconds REAL,
  duration_seconds REAL,
  level TEXT,
  race TEXT,
  track TEXT,
  game_mode TEXT,
  drone TEXT,
  race_guid TEXT,
  title TEXT,
  segmentation_method TEXT NOT NULL,
  confidence REAL,
  FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_race_sessions_capture ON race_sessions(capture_id);
