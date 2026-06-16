ALTER TABLE race_sessions
  ADD COLUMN collision_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE race_sessions
  ADD COLUMN collision_max_severity INTEGER NOT NULL DEFAULT 0;

ALTER TABLE race_sessions
  ADD COLUMN collision_avg_severity REAL;
