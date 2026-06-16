import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AutoCaptureState,
  CaptureRow,
  CaptureStats,
  CaptureMarkerRow,
  GameContextEvent,
  AssetRefreshProgress,
  GameAssetSourceStatus,
  ProcessingJobRow,
  ProcessingProgress,
} from "./types";

export type AppEvents = {
  auto_capture_state: AutoCaptureState;
  capture_started: CaptureRow;
  capture_stats_updated: CaptureStats;
  capture_stopped: CaptureRow;
  capture_failed: CaptureRow;
  /** Auto capture stopped but was junk (too few packets) and was deleted. */
  capture_discarded: CaptureRow;
  capture_deleted: string;
  marker_added: CaptureMarkerRow;
  game_context_detected: GameContextEvent;
  processing_started: ProcessingJobRow;
  processing_progress: ProcessingProgress;
  processing_completed: ProcessingJobRow;
  processing_failed: ProcessingJobRow;
  asset_refresh_started: AssetRefreshProgress;
  asset_refresh_progress: AssetRefreshProgress;
  asset_refresh_completed: GameAssetSourceStatus[];
  asset_refresh_failed: AssetRefreshProgress;
};

export function subscribe<K extends keyof AppEvents>(
  event: K,
  handler: (payload: AppEvents[K]) => void,
): Promise<UnlistenFn> {
  return listen<AppEvents[K]>(event, (msg) => handler(msg.payload));
}
