CREATE TABLE IF NOT EXISTS captures (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  stopped_at TEXT,
  status TEXT NOT NULL,
  source_type TEXT NOT NULL,
  source_config_json TEXT,
  raw_file_path TEXT NOT NULL,
  metadata_file_path TEXT,
  context_json TEXT,
  packet_count INTEGER NOT NULL DEFAULT 0,
  byte_count INTEGER NOT NULL DEFAULT 0,
  duration_seconds REAL,
  app_version TEXT,
  telemetry_config_hash TEXT,
  capture_hash TEXT
);

CREATE INDEX IF NOT EXISTS idx_captures_created_at ON captures(created_at);
CREATE INDEX IF NOT EXISTS idx_captures_status ON captures(status);

CREATE TABLE IF NOT EXISTS capture_markers (
  id TEXT PRIMARY KEY,
  capture_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  monotonic_ns INTEGER,
  marker_type TEXT NOT NULL,
  note TEXT,
  FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_capture_markers_capture_id ON capture_markers(capture_id);

CREATE TABLE IF NOT EXISTS processing_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  config_json TEXT NOT NULL,
  is_default INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS processing_jobs (
  id TEXT PRIMARY KEY,
  capture_id TEXT NOT NULL,
  profile_id TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  processor_version TEXT NOT NULL,
  input_capture_hash TEXT,
  output_dataset_id TEXT,
  error_message TEXT,
  FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
  FOREIGN KEY (profile_id) REFERENCES processing_profiles(id)
);

CREATE INDEX IF NOT EXISTS idx_processing_jobs_capture_id ON processing_jobs(capture_id);
CREATE INDEX IF NOT EXISTS idx_processing_jobs_status ON processing_jobs(status);

CREATE TABLE IF NOT EXISTS processed_datasets (
  id TEXT PRIMARY KEY,
  capture_id TEXT NOT NULL,
  job_id TEXT NOT NULL,
  profile_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  dataset_version TEXT NOT NULL,
  summary_json TEXT,
  FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
  FOREIGN KEY (job_id) REFERENCES processing_jobs(id) ON DELETE CASCADE,
  FOREIGN KEY (profile_id) REFERENCES processing_profiles(id)
);

CREATE INDEX IF NOT EXISTS idx_processed_datasets_capture_id ON processed_datasets(capture_id);
