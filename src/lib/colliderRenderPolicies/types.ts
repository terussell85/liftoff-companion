import type { CollisionGeometryShape, ReplayCourseData } from "@/lib/types";

export type ColliderRenderMode = "overview" | "nearby";

export type ColliderRenderStyle = {
  lineColor?: string;
  lineOpacity?: number;
  lineRenderOrder?: number;
  fillColor?: string | null;
  fillOpacity?: number;
  fillRenderOrder?: number;
};

export type ColliderRenderGeometry = {
  center?: [number, number, number];
  half_extents?: [number, number, number];
};

export type ColliderRenderDecision =
  | {
      action: "default";
      style?: ColliderRenderStyle;
      geometry?: ColliderRenderGeometry;
    }
  | { action: "hide" }
  | {
      action: "show";
      style?: ColliderRenderStyle;
      geometry?: ColliderRenderGeometry;
    };

export type ColliderRenderPolicyContext = {
  course: ReplayCourseData;
  shape: CollisionGeometryShape;
  allShapes: readonly CollisionGeometryShape[];
  renderMode: ColliderRenderMode;
  maxSpan: number;
  isConfirmedHit: boolean;
  isNearPath: () => boolean;
  worldCenter: [number, number, number];
  sceneCenter: [number, number, number];
  pathHeight: number;
  labelKey: string;
  objectPathKey: string;
  sourceKey: string;
  sourceAssetKey: string;
};

export type ColliderRenderPolicy = (
  context: ColliderRenderPolicyContext,
) => ColliderRenderDecision | null | undefined;
