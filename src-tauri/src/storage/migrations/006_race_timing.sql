CREATE TABLE IF NOT EXISTS race_laps (
  id TEXT PRIMARY KEY,
  dataset_id TEXT NOT NULL,
  capture_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  lap_index INTEGER NOT NULL,
  start_seconds REAL NOT NULL,
  end_seconds REAL NOT NULL,
  duration_seconds REAL NOT NULL,
  start_sample_index INTEGER,
  end_sample_index INTEGER,
  status TEXT NOT NULL,
  confidence REAL NOT NULL,
  FOREIGN KEY (dataset_id) REFERENCES processed_datasets(id) ON DELETE CASCADE,
  FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
  FOREIGN KEY (session_id) REFERENCES race_sessions(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_race_laps_dataset_session_lap
  ON race_laps(dataset_id, session_id, lap_index);

CREATE INDEX IF NOT EXISTS idx_race_laps_session
  ON race_laps(session_id, lap_index);

CREATE TABLE IF NOT EXISTS race_gate_splits (
  id TEXT PRIMARY KEY,
  dataset_id TEXT NOT NULL,
  capture_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  lap_index INTEGER NOT NULL,
  section_index INTEGER NOT NULL,
  section_kind TEXT NOT NULL,
  from_checkpoint_id INTEGER,
  from_checkpoint_sequence INTEGER,
  from_passage_type TEXT,
  to_checkpoint_id INTEGER,
  to_checkpoint_sequence INTEGER,
  to_passage_type TEXT,
  start_seconds REAL NOT NULL,
  end_seconds REAL NOT NULL,
  duration_seconds REAL NOT NULL,
  start_sample_index INTEGER,
  end_sample_index INTEGER,
  confidence REAL NOT NULL,
  FOREIGN KEY (dataset_id) REFERENCES processed_datasets(id) ON DELETE CASCADE,
  FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
  FOREIGN KEY (session_id) REFERENCES race_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_race_gate_splits_session
  ON race_gate_splits(session_id, lap_index, section_index);

CREATE TABLE IF NOT EXISTS race_passage_events (
  id TEXT PRIMARY KEY,
  dataset_id TEXT NOT NULL,
  capture_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  lap_index INTEGER NOT NULL,
  checkpoint_id INTEGER NOT NULL,
  checkpoint_sequence INTEGER NOT NULL,
  passage_type TEXT NOT NULL,
  directionality TEXT NOT NULL,
  event_seconds REAL NOT NULL,
  sample_index INTEGER,
  confidence REAL NOT NULL,
  FOREIGN KEY (dataset_id) REFERENCES processed_datasets(id) ON DELETE CASCADE,
  FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
  FOREIGN KEY (session_id) REFERENCES race_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_race_passage_events_session
  ON race_passage_events(session_id, event_seconds);
