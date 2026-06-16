import type {
  ColliderRenderDecision,
  ColliderRenderPolicy,
  ColliderRenderPolicyContext,
  ColliderRenderStyle,
} from "./types";

export const FIELD_DAY_COLLIDER_POLICY_KEYS = [
  "fdca6e12-4dff-438d-91d8-bbabe74ae426",
  "StrawBaleRace01_0001",
  "01 - Field Day",
];

export const fieldDayColliderPolicy: ColliderRenderPolicy = (
  context,
): ColliderRenderDecision => {
  if (isHayBaleShape(context)) {
    return {
      action: "default",
      style: HAY_BALE_COLLIDER_RENDER_STYLE,
    };
  }

  return { action: "default" };
};

const HAY_BALE_COLLIDER_RENDER_STYLE: ColliderRenderStyle = {
  lineColor: "#facc15",
  lineOpacity: 0.96,
  lineRenderOrder: 3,
  fillColor: "#facc15",
  fillOpacity: 0.16,
  fillRenderOrder: 2,
};

function isHayBaleShape(context: ColliderRenderPolicyContext): boolean {
  const key = [context.labelKey, context.objectPathKey].join(" ");
  return key.includes("haybale") || key.includes("strawbale");
}
