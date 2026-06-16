CREATE TABLE IF NOT EXISTS collision_geometry_cache (
  cache_id TEXT NOT NULL,
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  geometry_json TEXT NOT NULL,
  status TEXT NOT NULL,
  source_bundle TEXT,
  source_hash TEXT,
  warning_count INTEGER NOT NULL DEFAULT 0,
  error_message TEXT,
  extracted_at TEXT,
  PRIMARY KEY (cache_id, scope_kind, scope_id),
  FOREIGN KEY (cache_id) REFERENCES game_asset_caches(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_collision_geometry_cache_scope
  ON collision_geometry_cache(scope_kind, scope_id);
