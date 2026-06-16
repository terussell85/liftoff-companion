import type { GameAssetSourceStatus } from "./types";

/** Human label for an asset-refresh progress phase string. */
export function assetPhaseLabel(phase: string): string {
  const labels: Record<string, string> = {
    queued: "Queued",
    discovering_sources: "Discovering installs",
    sources_discovered: "Installs found",
    source_started: "Checking install",
    source_skipped: "Cache current",
    fingerprinting: "Fingerprinting",
    xml_extracting: "Extracting XML",
    courses_building: "Building courses",
    geometry_preparing: "Preparing geometry",
    geometry_started: "Extracting geometry",
    geometry_scope_started: "Scanning level",
    geometry_bundle_started: "Reading scene data",
    geometry_bundle_completed: "Scene data read",
    geometry_item_group_started: "Resolving shared geometry",
    geometry_item_group_bundle_started: "Reading shared geometry",
    geometry_item_group_bundle_completed: "Shared geometry read",
    geometry_scope_completed: "Level complete",
    geometry_completed: "Geometry complete",
    storing_cache: "Saving cache",
    source_completed: "Install complete",
    refresh_completed: "Refresh complete",
    source_failed: "Install failed",
    failed: "Refresh failed",
  };
  return labels[phase] ?? phase.replace(/_/g, " ");
}

/** Short freshness label for a cached source, e.g. "fresh Jun 2". */
export function assetStatusLabel(source: GameAssetSourceStatus): string {
  if (source.cache_status === "fresh") {
    return source.extracted_at
      ? `fresh ${formatAssetDate(source.extracted_at)}`
      : "fresh";
  }
  if (source.cache_status === "stale") return "stale";
  if (source.cache_status === "missing") return "not loaded";
  if (source.cache_status === "error") return "error";
  return source.cache_status;
}

export function formatAssetDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

export function progressPercent(value: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, (value / total) * 100));
}
