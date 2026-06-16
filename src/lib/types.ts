export type CaptureRow = {
  id: string;
  created_at: string;
  stopped_at: string | null;
  status: string;
  source_type: string;
  source_config_json: string | null;
  raw_file_path: string;
  metadata_file_path: string | null;
  context_json: string | null;
  packet_count: number;
  byte_count: number;
  duration_seconds: number | null;
  app_version: string | null;
  telemetry_config_hash: string | null;
  capture_hash: string | null;
};

export type CaptureMarkerRow = {
  id: string;
  capture_id: string;
  created_at: string;
  monotonic_ns: number | null;
  marker_type: string;
  note: string | null;
};

export type RaceSessionRow = {
  id: string;
  capture_id: string;
  session_index: number;
  start_monotonic_ns: number;
  end_monotonic_ns: number | null;
  start_seconds: number;
  end_seconds: number | null;
  duration_seconds: number | null;
  level: string | null;
  race: string | null;
  track: string | null;
  game_mode: string | null;
  drone: string | null;
  race_guid: string | null;
  title: string | null;
  segmentation_method: string;
  confidence: number | null;
  collision_count: number;
  collision_max_severity: number;
  collision_avg_severity: number | null;
};

export type CaptureDetail = {
  capture: CaptureRow;
  markers: CaptureMarkerRow[];
  race_sessions: RaceSessionRow[];
};

/**
 * A race session flattened with its parent capture's wall-clock time and
 * status. Built client-side in the Races view; not a wire type.
 */
export type RaceSessionWithCapture = RaceSessionRow & {
  capture_created_at: string;
  capture_status: string;
};

export type GameContextEvent = {
  capture_id: string;
  title: string | null;
  level: string;
  environment_raw: string;
  game_mode: string | null;
  drone: string | null;
  track: string | null;
  race: string | null;
  race_guid: string | null;
};

export type PlayerLogCandidate = {
  title: string;
  path: string;
  exists: boolean;
  modified_ms: number | null;
};

export type CaptureStats = {
  capture_id: string;
  packet_count: number;
  byte_count: number;
  bytes_written: number;
  packet_rate_hz: number;
  duration_seconds: number;
  last_packet_at_utc: string | null;
  last_source_addr: string | null;
  raw_file_path: string;
  status: string;
};

/** Snapshot of the blackbox auto-capture supervisor. */
export type AutoCaptureState = {
  enabled: boolean;
  phase: "disabled" | "waiting" | "armed" | "recording" | string;
  bind_addr: string | null;
  message: string | null;
};

export type StartCaptureRequest = {
  bind_addr?: string;
  port?: number;
  context?: Record<string, unknown>;
  telemetry_config_hash?: string;
};

export type StartCaptureResponse = {
  capture: CaptureRow;
  bind_addr: string;
  raw_file_path: string;
  markers_file_path: string;
};

export type AddMarkerRequest = {
  capture_id: string;
  marker_type?: string;
  note?: string;
};

export type ProcessingProfileRow = {
  id: string;
  name: string;
  created_at: string;
  config_json: string;
  is_default: boolean;
};

export type ProcessingJobRow = {
  id: string;
  capture_id: string;
  profile_id: string;
  status: "pending" | "running" | "completed" | "failed" | string;
  started_at: string | null;
  completed_at: string | null;
  processor_version: string;
  input_capture_hash: string | null;
  output_dataset_id: string | null;
  error_message: string | null;
};

export type ProcessedDatasetRow = {
  id: string;
  capture_id: string;
  job_id: string;
  profile_id: string;
  created_at: string;
  dataset_version: string;
  summary_json: string | null;
};

export type PipelineSummary = {
  packet_count: number;
  sample_count: number;
  warning_count: number;
  warnings_by_kind: Record<string, number>;
  schema_endpoint: string;
  schema_field_count: number;
  schema_config_hash: string;
  mean_speed: number;
  max_speed: number;
  min_speed: number;
  collision_count: number;
  collision_max_severity: number;
  collision_avg_severity: number | null;
  start_monotonic_ns: number;
};

export type ProcessCaptureResponse = {
  job: ProcessingJobRow;
  dataset: ProcessedDatasetRow;
  summary: PipelineSummary;
};

export type SamplePoint = {
  capture_time_seconds: number;
  speed: number;
  throttle: number | null;
  /** World position [x, y, z] in meters (Liftoff/Unity, Y-up); null when unavailable. */
  pos?: [number, number, number] | null;
};

export type CollisionEvent = {
  sample_index: number;
  capture_time_seconds: number;
  severity: number;
  confidence: number;
  speed_before: number;
  speed_after: number;
  speed_delta: number;
  decel_mps2: number;
  pos?: [number, number, number] | null;
  geometry_confirmed?: boolean;
  geometry_status?: string | null;
  hit_source?: string | null;
  hit_label?: string | null;
  hit_shape?: string | null;
  hit_distance?: number | null;
};

export type RaceLapRow = {
  id: string;
  dataset_id: string;
  capture_id: string;
  session_id: string;
  lap_index: number;
  start_seconds: number;
  end_seconds: number;
  duration_seconds: number;
  start_sample_index: number | null;
  end_sample_index: number | null;
  status: string;
  confidence: number;
};

export type RaceGateSplitRow = {
  id: string;
  dataset_id: string;
  capture_id: string;
  session_id: string;
  lap_index: number;
  section_index: number;
  section_kind: string;
  from_checkpoint_id: number | null;
  from_checkpoint_sequence: number | null;
  from_passage_type: string | null;
  to_checkpoint_id: number | null;
  to_checkpoint_sequence: number | null;
  to_passage_type: string | null;
  start_seconds: number;
  end_seconds: number;
  duration_seconds: number;
  start_sample_index: number | null;
  end_sample_index: number | null;
  confidence: number;
};

export type RacePassageEventRow = {
  id: string;
  dataset_id: string;
  capture_id: string;
  session_id: string;
  lap_index: number;
  checkpoint_id: number;
  checkpoint_sequence: number;
  passage_type: string;
  directionality: string;
  event_seconds: number;
  sample_index: number | null;
  confidence: number;
};

export type SessionTimingDetail = {
  laps: RaceLapRow[];
  gate_splits: RaceGateSplitRow[];
  passage_events: RacePassageEventRow[];
};

export type DatasetDetail = {
  dataset: ProcessedDatasetRow;
  summary: PipelineSummary;
  samples: SamplePoint[];
  collision_events: CollisionEvent[];
};

export type LiftoffDirCandidate = {
  path: string;
  exists: boolean;
  config_path: string;
  config_exists: boolean;
  label: string;
  /** Whether the on-disk config matches the canonical schema for the current
   * endpoint. null until resolved; only meaningful when config_exists. */
  matches_canonical: boolean | null;
};

export type TelemetryConfigStatus = {
  path: string;
  exists: boolean;
  endpoint: string | null;
  stream_format: string[];
  raw_json: string | null;
  config_hash: string | null;
  matches_canonical: boolean;
};

export type ApplyConfigOutcome = {
  path: string;
  backup_path: string | null;
  previous_hash: string | null;
  new_hash: string;
  previous_existed: boolean;
};

export type DisableOutcome = {
  path: string;
  /** True when a `.json.bak` backup was restored in place; false when the
   * config was simply removed (or there was nothing to remove). */
  restored: boolean;
  restored_from: string | null;
};

export type SetupSnapshot = {
  udp_bind_addr: string;
  udp_port: number;
  dirs: LiftoffDirCandidate[];
  config_status: TelemetryConfigStatus | null;
  player_logs: PlayerLogCandidate[];
};

export type GameAssetSourceStatus = {
  game_title: string;
  label: string;
  data_root: string;
  valid: boolean;
  cache_status: "fresh" | "missing" | "stale" | "error" | string;
  cache_id: string | null;
  extracted_at: string | null;
  race_count: number;
  track_count: number;
  error_message: string | null;
};

export type GameAssetCatalog = {
  cache_id: string;
  game_title: string;
  data_root: string;
  levels: GameAssetLevelCatalog[];
};

export type GameAssetLevelCatalog = {
  environment_id: string | null;
  name: string;
  races: GameAssetRaceCatalog[];
};

export type GameAssetRaceCatalog = {
  race_guid: string;
  race_name: string;
  race_asset_key: string | null;
  track_guid: string | null;
  track_name: string | null;
  track_asset_key: string | null;
  required_laps: number | null;
  checkpoint_count: number;
  prop_count: number;
  collision_prop_count: number;
};

export type AssetRefreshProgress = {
  phase: string;
  message: string;
  game_title: string | null;
  data_root: string | null;
  sources_done: number;
  sources_total: number;
  scopes_done: number;
  scopes_total: number;
  levels_done: number;
  levels_total: number;
  bundles_done: number;
  bundles_total: number;
  current_scope: string | null;
  current_level: string | null;
  current_bundle: string | null;
  races_found: number;
  tracks_found: number;
  geometry_ready: number;
  geometry_partial: number;
  geometry_missing: number;
  geometry_shapes: number;
};

export type ReplayCourseProp = {
  instance_id: number;
  item_id: string;
  kind: string;
  position: [number, number, number];
  rotation: [number, number, number];
  dimensions: [number, number, number] | null;
  attach_points: [number, number, number][];
  procedural_geometry: boolean;
  name: string | null;
};

export type ReplayGuidePathSegment = {
  from_passage_id: string | null;
  to_passage_id: string | null;
  from_checkpoint_id: number | null;
  to_checkpoint_id: number | null;
  points: [number, number, number][];
};

export type ReplayGuidePath = {
  algorithm: string;
  accuracy: string;
  segments: ReplayGuidePathSegment[];
};

export type ReplayCheckpoint = {
  sequence_index: number;
  checkpoint_id: number;
  passage_type: string;
  directionality: string;
  item_id: string;
  position: [number, number, number];
  rotation: [number, number, number];
  dimensions: [number, number, number];
};

export type ReplayCourseData = {
  cache_id: string;
  game_title: string;
  data_root: string;
  race_guid: string;
  race_name: string;
  race_asset_key?: string | null;
  track_guid: string | null;
  track_name: string | null;
  track_asset_key?: string | null;
  environment_id: string | null;
  required_laps: number | null;
  checkpoints: ReplayCheckpoint[];
  spawnpoint: ReplayCourseProp | null;
  props: ReplayCourseProp[];
  collision_props?: ReplayCourseProp[];
  guide_path?: ReplayGuidePath | null;
};

export type ResolveSessionCourseResponse = {
  course: ReplayCourseData | null;
  refreshed: boolean;
  status: string;
  message: string | null;
};

export type CollisionGeometryShape = {
  id: string;
  source_kind: string;
  source_id: string;
  label: string;
  provenance?: string | null;
  source_asset?: string | null;
  object_path?: string | null;
  shape: string;
  center: [number, number, number];
  half_extents: [number, number, number];
  rotation?: [number, number, number, number];
  confidence: number;
};

export type ResolveSessionCollisionGeometryResponse = {
  shapes: CollisionGeometryShape[];
  warnings: string[];
  unavailable: boolean;
  status: string;
  message: string | null;
};

export type TestListenerResult = {
  bind_addr: string;
  duration_seconds: number;
  packet_count: number;
  packet_rate_hz: number;
  last_source_addr: string | null;
};

export type ProcessingProgress = {
  job_id: string;
  processed_packets: number;
  total_packets: number;
  warnings_count: number;
};

export type AppErrorPayload = {
  kind: string;
  message: string;
};
