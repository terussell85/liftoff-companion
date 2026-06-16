import {
  FIELD_DAY_COLLIDER_POLICY_KEYS,
  fieldDayColliderPolicy,
} from "./fieldDay";
import {
  GYM_CLASS_COLLIDER_POLICY_KEYS,
  gymClassColliderPolicy,
} from "./gymClass";
import type { ReplayCourseData } from "@/lib/types";
import type {
  ColliderRenderDecision,
  ColliderRenderPolicy,
  ColliderRenderPolicyContext,
} from "./types";

export type {
  ColliderRenderDecision,
  ColliderRenderGeometry,
  ColliderRenderMode,
  ColliderRenderPolicy,
  ColliderRenderPolicyContext,
  ColliderRenderStyle,
} from "./types";

const DEFAULT_COLLIDER_RENDER_DECISION: ColliderRenderDecision = {
  action: "default",
};

// Keys are normalized by normalizeColliderPolicyKey. Prefer race GUIDs or race
// asset keys; use track/environment maps only for genuinely shared rules.
const RACE_COLLIDER_POLICIES: Record<string, ColliderRenderPolicy> =
  Object.fromEntries(
    [
      ...policyEntries(GYM_CLASS_COLLIDER_POLICY_KEYS, gymClassColliderPolicy),
      ...policyEntries(FIELD_DAY_COLLIDER_POLICY_KEYS, fieldDayColliderPolicy),
    ],
  );

const TRACK_COLLIDER_POLICIES: Record<string, ColliderRenderPolicy> = {};

const ENVIRONMENT_COLLIDER_POLICIES: Record<string, ColliderRenderPolicy> = {};

export function resolveCourseColliderRenderDecision(
  context: ColliderRenderPolicyContext,
): ColliderRenderDecision {
  const policy = findCourseColliderPolicy(context.course);
  return policy?.(context) ?? DEFAULT_COLLIDER_RENDER_DECISION;
}

export function normalizeColliderPolicyKey(
  value: string | null | undefined,
): string {
  return (value ?? "").toLowerCase().replace(/[^a-z0-9]/g, "");
}

function findCourseColliderPolicy(
  course: ReplayCourseData,
): ColliderRenderPolicy | null {
  return (
    findColliderPolicy(RACE_COLLIDER_POLICIES, [
      course.race_guid,
      course.race_asset_key,
      course.race_name,
    ]) ??
    findColliderPolicy(TRACK_COLLIDER_POLICIES, [
      course.track_guid,
      course.track_asset_key,
    ]) ??
    findColliderPolicy(ENVIRONMENT_COLLIDER_POLICIES, [course.environment_id])
  );
}

function findColliderPolicy(
  registry: Record<string, ColliderRenderPolicy>,
  keys: (string | null | undefined)[],
): ColliderRenderPolicy | null {
  for (const key of keys) {
    const policy = registry[normalizeColliderPolicyKey(key)];
    if (policy) return policy;
  }
  return null;
}

function policyEntries(
  keys: readonly string[],
  policy: ColliderRenderPolicy,
): [string, ColliderRenderPolicy][] {
  return keys.map((key) => [normalizeColliderPolicyKey(key), policy]);
}
