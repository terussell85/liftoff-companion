CREATE TABLE IF NOT EXISTS game_asset_caches (
  id TEXT PRIMARY KEY,
  game_title TEXT NOT NULL,
  data_root TEXT NOT NULL UNIQUE,
  extractor_version TEXT NOT NULL,
  source_fingerprint_hash TEXT NOT NULL,
  source_fingerprint_json TEXT NOT NULL,
  status TEXT NOT NULL,
  error_message TEXT,
  extracted_at TEXT,
  race_count INTEGER NOT NULL DEFAULT 0,
  track_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_game_asset_caches_title ON game_asset_caches(game_title);
CREATE INDEX IF NOT EXISTS idx_game_asset_caches_status ON game_asset_caches(status);

CREATE TABLE IF NOT EXISTS race_course_cache (
  cache_id TEXT NOT NULL,
  race_guid TEXT NOT NULL,
  race_name TEXT NOT NULL,
  track_guid TEXT,
  track_name TEXT,
  environment_id TEXT,
  required_laps INTEGER,
  course_json TEXT NOT NULL,
  PRIMARY KEY (cache_id, race_guid),
  FOREIGN KEY (cache_id) REFERENCES game_asset_caches(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_race_course_cache_race_guid ON race_course_cache(race_guid);
CREATE INDEX IF NOT EXISTS idx_race_course_cache_names ON race_course_cache(race_name, track_name);
