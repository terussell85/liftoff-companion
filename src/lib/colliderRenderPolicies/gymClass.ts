import type { CollisionGeometryShape } from "@/lib/types";
import type {
  ColliderRenderDecision,
  ColliderRenderGeometry,
  ColliderRenderPolicy,
  ColliderRenderPolicyContext,
} from "./types";

export const GYM_CLASS_COLLIDER_POLICY_KEYS = [
  "ef572c3d-8b5b-4e67-affb-6e98ea08c1ab",
  "HovertonHighRace01_0001",
  "01 - Gym Class",
];

export const gymClassColliderPolicy: ColliderRenderPolicy = (
  context,
): ColliderRenderDecision => {
  if (isGymClassWallShape(context)) {
    return { action: "hide" };
  }

  const hoopTop =
    gymClassHoopTopCache.get(context.allShapes) ??
    computeGymClassHoopTopMeters(context.allShapes);
  gymClassHoopTopCache.set(context.allShapes, hoopTop);

  const cutoff = hoopTop + GYM_CLASS_HOOP_CLEARANCE_METERS;
  const bottom = shapeBottomMeters(context.shape);
  const top = shapeTopMeters(context.shape);
  if (bottom >= cutoff) {
    return { action: "hide" };
  }
  if (top > cutoff) {
    return {
      action: "default",
      geometry: clipShapeTopMeters(context.shape, cutoff),
    };
  }

  return { action: "default" };
};

const GYM_CLASS_HOOP_CLEARANCE_METERS = 0;
const GYM_CLASS_FALLBACK_HOOP_TOP_METERS = 4.05;
const GYM_CLASS_WALL_LABEL_PARTS = [
  "levelwall",
  "wallsmain",
  "courtlargewall",
  "courtsidewall",
  "courtsidewalllower",
];
const gymClassHoopTopCache = new WeakMap<
  readonly CollisionGeometryShape[],
  number
>();

function computeGymClassHoopTopMeters(
  shapes: readonly CollisionGeometryShape[],
): number {
  const hoopTops = shapes
    .filter(isGymClassHoopReferenceShape)
    .map(shapeTopMeters)
    .filter((top) => Number.isFinite(top));

  return hoopTops.length > 0
    ? Math.max(...hoopTops)
    : GYM_CLASS_FALLBACK_HOOP_TOP_METERS;
}

function isGymClassHoopReferenceShape(shape: CollisionGeometryShape): boolean {
  const key = normalizeCollisionKey(
    [shape.label, shape.object_path ?? ""].join(" "),
  );
  return key.includes("basketballhoopbackboard");
}

function isGymClassWallShape(context: ColliderRenderPolicyContext): boolean {
  const key = [context.labelKey, context.objectPathKey].join(" ");
  return GYM_CLASS_WALL_LABEL_PARTS.some((part) => key.includes(part));
}

function shapeTopMeters(shape: CollisionGeometryShape): number {
  return shape.center[1] + Math.abs(shape.half_extents[1]);
}

function shapeBottomMeters(shape: CollisionGeometryShape): number {
  return shape.center[1] - Math.abs(shape.half_extents[1]);
}

function clipShapeTopMeters(
  shape: CollisionGeometryShape,
  top: number,
): ColliderRenderGeometry {
  const bottom = shapeBottomMeters(shape);
  const clippedHalfY = Math.max((top - bottom) / 2, 0.001);
  return {
    center: [shape.center[0], bottom + clippedHalfY, shape.center[2]],
    half_extents: [
      shape.half_extents[0],
      clippedHalfY,
      shape.half_extents[2],
    ],
  };
}

function normalizeCollisionKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}
