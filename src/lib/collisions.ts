import type { CollisionEvent } from "./types";

/**
 * Collision display + labelling helpers, mirroring the conventions the 3D
 * FlightPath view uses, so the race-detail incident log stays consistent with
 * what's drawn in the visualizer (helper/out-of-scope colliders are hidden).
 */

const HELPER_LABEL_PARTS = [
  "levelwall",
  "lightprobe",
  "navmesh",
  "occlusion",
  "postprocess",
  "reflectionprobe",
];
const SCOPED_ENVIRONMENT_LABEL_PARTS = [{ label: "kowloon", source: "kowloon" }];

function normalizeKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function isExplicitColliderShape(shape: string): boolean {
  return shape.endsWith("_collider") || shape === "procedural_ribbon_segment";
}

function isHelperLabel(label: string): boolean {
  const value = normalizeKey(label);
  return HELPER_LABEL_PARTS.some((part) => value.includes(part));
}

function isOutOfScopeEnvironmentLabel(
  label: string,
  source: string | undefined,
): boolean {
  const labelKey = normalizeKey(label);
  const sourceKey = normalizeKey(source ?? "");
  return SCOPED_ENVIRONMENT_LABEL_PARTS.some(
    (scope) => labelKey.includes(scope.label) && !sourceKey.includes(scope.source),
  );
}

/** Whether a collision is a real, in-scope impact worth surfacing. */
export function isDisplayCollision(event: CollisionEvent): boolean {
  if (event.hit_shape && !isExplicitColliderShape(event.hit_shape)) return false;
  if (
    event.hit_source?.toLowerCase().startsWith("environment:") &&
    isOutOfScopeEnvironmentLabel(event.hit_label ?? "", event.hit_source)
  ) {
    return false;
  }
  return !isHelperLabel(event.hit_label ?? "");
}

function humanizeIdentifier(value: string): string {
  return value
    .replace(/01$/, "")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/(\d+)([A-Z])/g, "$1 $2")
    .trim();
}

function formatHitLabel(value: string): string {
  const segments = value
    .split(/[\\/:]/)
    .map((part) => part.trim())
    .filter(Boolean);
  const leaf = segments.length > 0 ? segments[segments.length - 1] : value;
  const instance = segments
    .slice(0, -1)
    .reverse()
    .map((part) => part.match(/\((\d+)\)/)?.[1])
    .find(Boolean);
  const label = humanizeIdentifier(leaf.replace(/\s*\(\d+\)\s*$/, ""));
  return instance ? `${label} #${instance}` : label;
}

function formatHitShape(shape: string | null | undefined): string | null {
  if (!shape) return null;
  switch (shape) {
    case "mesh_collider": return "mesh collider";
    case "box_collider": return "box collider";
    case "sphere_collider": return "sphere collider";
    case "capsule_collider": return "capsule collider";
    case "procedural_ribbon_segment": return "ribbon segment";
    default: return humanizeIdentifier(shape);
  }
}

function formatHitDistance(distance: number | null | undefined): string | null {
  if (distance == null || !Number.isFinite(distance)) return null;
  if (distance < 0.005) return "touching";
  return `${distance.toFixed(distance < 0.1 ? 2 : 1)} m`;
}

/** Human label for what was hit, or null when it can't be named confidently. */
export function collisionHitLabel(event: CollisionEvent): string | null {
  if (!event.geometry_confirmed) return null;
  const label = event.hit_label?.trim() || event.hit_source?.trim();
  return label ? formatHitLabel(label) : null;
}

/** Secondary detail line for an impact (shape · distance). */
export function collisionHitDetail(event: CollisionEvent): string | null {
  if (!event.geometry_confirmed) return null;
  const parts = [formatHitShape(event.hit_shape), formatHitDistance(event.hit_distance)].filter(
    Boolean,
  );
  return parts.length > 0 ? parts.join(" · ") : null;
}

/** CSS variable for a severity, matching the map/recorder palette. */
export function severityVar(severity: number): string {
  if (severity >= 8) return "var(--destructive)";
  if (severity >= 4) return "var(--warn)";
  return "var(--chart-3)";
}
