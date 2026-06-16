import { invoke } from "@tauri-apps/api/core";
import type {
  AddMarkerRequest,
  ApplyConfigOutcome,
  AutoCaptureState,
  DisableOutcome,
  CaptureDetail,
  CaptureMarkerRow,
  CaptureRow,
  CaptureStats,
  DatasetDetail,
  GameAssetCatalog,
  GameAssetSourceStatus,
  PipelineSummary,
  ProcessCaptureResponse,
  ProcessedDatasetRow,
  ProcessingJobRow,
  ProcessingProfileRow,
  RaceSessionRow,
  ResolveSessionCollisionGeometryResponse,
  ResolveSessionCourseResponse,
  SessionTimingDetail,
  SetupSnapshot,
  StartCaptureRequest,
  StartCaptureResponse,
  TelemetryConfigStatus,
  TestListenerResult,
} from "./types";

export const api = {
  // Capture lifecycle
  startCapture(req: StartCaptureRequest): Promise<StartCaptureResponse> {
    return invoke("start_capture", { req });
  },
  stopCapture(captureId: string): Promise<CaptureRow> {
    return invoke("stop_capture", { captureId });
  },
  addCaptureMarker(req: AddMarkerRequest): Promise<CaptureMarkerRow> {
    return invoke("add_capture_marker", { req });
  },
  listCaptures(): Promise<CaptureRow[]> {
    return invoke("list_captures");
  },
  getCapture(captureId: string): Promise<CaptureDetail> {
    return invoke("get_capture", { captureId });
  },
  listRaceSessions(captureId: string): Promise<RaceSessionRow[]> {
    return invoke("list_race_sessions", { captureId });
  },
  updateCaptureContext(
    captureId: string,
    context: Record<string, unknown>,
  ): Promise<CaptureRow> {
    return invoke("update_capture_context", {
      req: { capture_id: captureId, context },
    });
  },
  currentCapture(): Promise<CaptureStats | null> {
    return invoke("current_capture");
  },
  deleteCapture(captureId: string): Promise<void> {
    return invoke("delete_capture", { captureId });
  },
  deleteRaceSession(sessionId: string): Promise<void> {
    return invoke("delete_race_session", { sessionId });
  },
  getAutoCapture(): Promise<AutoCaptureState> {
    return invoke("get_auto_capture");
  },
  setAutoCapture(enabled: boolean): Promise<AutoCaptureState> {
    return invoke("set_auto_capture", { enabled });
  },

  // Processing
  listProcessingProfiles(): Promise<ProcessingProfileRow[]> {
    return invoke("list_processing_profiles");
  },
  processCapture(
    captureId: string,
    profileId?: string,
  ): Promise<ProcessCaptureResponse> {
    return invoke("process_capture", {
      req: { capture_id: captureId, profile_id: profileId },
    });
  },
  getProcessingJob(jobId: string): Promise<ProcessingJobRow> {
    return invoke("get_processing_job", { jobId });
  },
  listProcessedDatasets(captureId: string): Promise<ProcessedDatasetRow[]> {
    return invoke("list_processed_datasets", { captureId });
  },
  getDatasetDetail(datasetId: string): Promise<DatasetDetail> {
    return invoke("get_dataset_detail", { datasetId });
  },
  getSessionTimingDetail(
    datasetId: string,
    sessionId: string,
  ): Promise<SessionTimingDetail> {
    return invoke("get_session_timing_detail", { datasetId, sessionId });
  },

  // Setup
  getSetupSnapshot(): Promise<SetupSnapshot> {
    return invoke("get_setup_snapshot");
  },
  readTelemetryConfig(path: string): Promise<TelemetryConfigStatus> {
    return invoke("read_telemetry_config", { path });
  },
  applyRecommendedTelemetryConfig(
    path?: string,
    endpoint?: string,
  ): Promise<ApplyConfigOutcome> {
    return invoke("apply_recommended_telemetry_config", {
      req: { path, endpoint },
    });
  },
  disableTelemetryConfig(path: string): Promise<DisableOutcome> {
    return invoke("disable_telemetry_config", { req: { path } });
  },
  updateNetworkConfig(
    udpBindAddr?: string,
    udpPort?: number,
  ): Promise<SetupSnapshot> {
    return invoke("update_network_config", {
      req: { udp_bind_addr: udpBindAddr, udp_port: udpPort },
    });
  },
  runTestListener(durationSeconds?: number): Promise<TestListenerResult> {
    return invoke("run_test_listener", { durationSeconds });
  },
  listGameAssetSources(): Promise<GameAssetSourceStatus[]> {
    return invoke("list_game_asset_sources");
  },
  listGameAssetCatalog(): Promise<GameAssetCatalog[]> {
    return invoke("list_game_asset_catalog");
  },
  refreshRaceTrackCache(
    force?: boolean,
    dataRoot?: string,
  ): Promise<GameAssetSourceStatus[]> {
    return invoke("refresh_race_track_cache", {
      req: { force, data_root: dataRoot },
    });
  },
  resolveSessionCourse(
    captureId: string,
    sessionId: string,
  ): Promise<ResolveSessionCourseResponse> {
    return invoke("resolve_session_course", {
      req: { capture_id: captureId, session_id: sessionId },
    });
  },
  resolveSessionCollisionGeometry(
    captureId: string,
    sessionId: string,
  ): Promise<ResolveSessionCollisionGeometryResponse> {
    return invoke("resolve_session_collision_geometry", {
      req: { capture_id: captureId, session_id: sessionId },
    });
  },
};

export type { PipelineSummary };
