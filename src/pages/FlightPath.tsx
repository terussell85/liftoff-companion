import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { Billboard, Grid, Line, OrbitControls, Text } from "@react-three/drei";
import * as THREE from "three";
import { CircleAlert, Loader2, Orbit, Pause, Play, RotateCcw } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api } from "@/lib/api";
import {
  normalizeColliderPolicyKey,
  resolveCourseColliderRenderDecision,
} from "@/lib/colliderRenderPolicies";
import type {
  ColliderRenderGeometry,
  ColliderRenderMode,
  ColliderRenderStyle,
} from "@/lib/colliderRenderPolicies";
import type {
  CollisionGeometryShape,
  CollisionEvent,
  RaceLapRow,
  RaceSessionRow,
  ReplayCheckpoint,
  ReplayCourseData,
  ReplayCourseProp,
  ReplayGuidePath,
  ReplayGuidePathSegment,
  SamplePoint,
} from "@/lib/types";
import type { View } from "@/App";

// Concrete colors (three.js can't parse the app's oklch CSS variables).
const SIGNAL = "#9ae600";
const GRID_CELL = "#2c313b";
const GRID_SECTION = "#4a5160";
const COURSE_WARN = "#facc15";
const COURSE_FINISH = "#38bdf8";
const GUIDE_PATH = "#f59e0b";
const COLLISION_LOW = "#facc15";
const COLLISION_MID = "#fb923c";
const COLLISION_HIGH = "#ef4444";
const COLLIDER_DEFAULT_LINE = "#ffffff";
const UNCLASSIFIED_PATH = "#64748b";
const LAP_COLORS = [
  "#38bdf8",
  "#f59e0b",
  "#c084fc",
  "#22c55e",
  "#fb7185",
  "#a3e635",
] as const;
const COLLIDER_EXPLICIT_MIN_RENDER_CONFIDENCE = 0.75;
const COLLIDER_ENVIRONMENT_MAX_RENDER_SPAN_METERS = 12;
const COLLIDER_PATH_RELEVANCE_RADIUS_METERS = 3.5;
const COLLIDER_PATH_RELEVANCE_MAX_ANCHORS = 256;
const COLLIDER_VERTICAL_RELEVANCE_MARGIN_METERS = 3;
const COLLIDER_HELPER_LABEL_PARTS = [
  "levelwall",
  "lightprobe",
  "navmesh",
  "occlusion",
  "postprocess",
  "reflectionprobe",
];
const COLLIDER_SCOPED_ENVIRONMENT_LABEL_PARTS = [
  { label: "kowloon", source: "kowloon" },
];
const DEFAULT_COLLIDER_RENDER_STYLE: ResolvedColliderRenderStyle = {
  lineColor: COLLIDER_DEFAULT_LINE,
  lineOpacity: 0.72,
  lineRenderOrder: 1,
  fillColor: null,
  fillOpacity: 0,
  fillRenderOrder: 0,
};
const GUIDE_ARROW_UP = new THREE.Vector3(0, 1, 0);
const GUIDE_TRAIL_PHASES = [0, 1 / 3, 2 / 3] as const;
const GUIDE_ARROW_SPEED = 1.2;

const SPEEDS = [0.5, 1, 1.25, 1.5, 2] as const;

type Props = {
  datasetId: string;
  sessionId: string;
  onNavigate: (view: View) => void;
};

type PreparedPath = {
  points: THREE.Vector3[];
  pathSegments: PreparedPathSegment[];
  collisionEvents: PreparedCollisionEvent[];
  /** seconds since the first windowed sample (monotonic, ascending) */
  times: number[];
  duration: number;
  radius: number;
  height: number;
  centerX: number;
  minY: number;
  centerZ: number;
  captureId: string;
  sessionTitle: string;
};

type PreparedPathSegment = {
  id: string;
  label: string;
  lapIndex: number | null;
  color: string;
  points: THREE.Vector3[];
  times: number[];
  positions: Float32Array;
};

type PreparedCollisionEvent = CollisionEvent & {
  point: THREE.Vector3;
  replayTime: number;
};

type LapWindow = {
  lapIndex: number;
  startSeconds: number;
  endSeconds: number;
};

type CourseStatus = "idle" | "loading" | "ready" | "missing" | "error";
type CollisionGeometryStatus =
  | "idle"
  | "loading"
  | "ready"
  | "partial"
  | "missing"
  | "error";
type GuideRenderMode = "animated" | "solid";
type CollisionGeometryRenderMode = ColliderRenderMode;

type GuideTrace = {
  points: THREE.Vector3[];
  distances: number[];
  length: number;
};

type CollisionRelevanceContext = {
  anchors: THREE.Vector3[];
  hitLabels: Set<string>;
};

type ResolvedColliderRenderStyle = {
  lineColor: string;
  lineOpacity: number;
  lineRenderOrder: number;
  fillColor: string | null;
  fillOpacity: number;
  fillRenderOrder: number;
};

type CollisionGeometryRenderItem = {
  shape: CollisionGeometryShape;
  style: ResolvedColliderRenderStyle;
};

export function FlightPathView({ datasetId, sessionId, onNavigate }: Props) {
  const [path, setPath] = useState<PreparedPath | null>(null);
  const [captureId, setCaptureId] = useState<string | null>(null);
  const [course, setCourse] = useState<ReplayCourseData | null>(null);
  const [courseStatus, setCourseStatus] = useState<CourseStatus>("idle");
  const [courseMessage, setCourseMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [empty, setEmpty] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setPath(null);
    setCourse(null);
    setCourseStatus("idle");
    setCourseMessage(null);
    setError(null);
    setEmpty(false);
    (async () => {
      try {
        const [detail, timing] = await Promise.all([
          api.getDatasetDetail(datasetId),
          api
            .getSessionTimingDetail(datasetId, sessionId)
            .catch(() => null),
        ]);
        if (cancelled) return;
        const capId = detail.dataset.capture_id;
        setCaptureId(capId);

        const sessions = await api.listRaceSessions(capId);
        if (cancelled) return;
        const session = sessions.find((s) => s.id === sessionId) ?? null;

        const offset = detail.summary.start_monotonic_ns / 1e9;
        const start = (session?.start_seconds ?? -Infinity) - offset;
        const end = (session?.end_seconds ?? Infinity) - offset;
        const windowed = detail.samples.filter(
          (s) =>
            s.capture_time_seconds >= start && s.capture_time_seconds <= end,
        );
        const collisionEvents = detail.collision_events.filter(
          (event) =>
            event.capture_time_seconds >= start &&
            event.capture_time_seconds <= end,
        ).filter(shouldDisplayCollisionEvent);
        const lapWindows = buildLapWindows(timing?.laps ?? [], offset);

        const prepared = preparePath(
          windowed,
          collisionEvents,
          lapWindows,
          capId,
          sessionTitle(session, sessions),
        );
        if (cancelled) return;
        if (!prepared) {
          setEmpty(true);
          return;
        }
        setPath(prepared);
        if (session) {
          setCourseStatus("loading");
          void api
            .resolveSessionCourse(capId, session.id)
            .then((result) => {
              if (cancelled) return;
              setCourse(result.course);
              setCourseStatus(result.course ? "ready" : "missing");
              setCourseMessage(result.message);
            })
            .catch((e) => {
              if (cancelled) return;
              setCourse(null);
              setCourseStatus("error");
              setCourseMessage(formatError(e));
            });
        }
      } catch (e) {
        if (!cancelled) setError(formatError(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [datasetId, sessionId]);

  if (error) {
    return (
      <CenteredOverlay
        onNavigate={onNavigate}
        captureId={captureId}
        sessionId={sessionId}
      >
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Couldn&apos;t load flight path</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      </CenteredOverlay>
    );
  }

  if (empty) {
    return (
      <CenteredOverlay
        onNavigate={onNavigate}
        captureId={captureId}
        sessionId={sessionId}
      >
        <div className="flex max-w-md flex-col items-center gap-4 text-center">
          <Orbit className="text-muted-foreground size-8" />
          <div className="flex flex-col gap-1">
            <p className="text-sm font-medium">No 3D positions for this session</p>
            <p className="text-muted-foreground text-sm">
              This dataset was processed before position data was captured to
              the samples cache. Re-process the capture to enable the 3D path.
            </p>
          </div>
          {captureId && (
            <Button
              size="sm"
              onClick={() =>
                onNavigate({ kind: "process", captureId })
              }
            >
              Re-process capture
            </Button>
          )}
        </div>
      </CenteredOverlay>
    );
  }

  if (!path) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-muted-foreground flex items-center gap-2 font-mono text-sm">
          <Loader2 className="size-4 animate-spin" />
          loading…
        </div>
      </div>
    );
  }

  return (
    <FlightPathViewer
      path={path}
      course={course}
      courseStatus={courseStatus}
      courseMessage={courseMessage}
      captureId={path.captureId}
      sessionId={sessionId}
      onNavigate={onNavigate}
    />
  );
}

function FlightPathViewer({
  path,
  course,
  courseStatus,
  courseMessage,
  captureId,
  sessionId,
  onNavigate,
}: {
  path: PreparedPath;
  course: ReplayCourseData | null;
  courseStatus: CourseStatus;
  courseMessage: string | null;
  captureId: string;
  sessionId: string;
  onNavigate: (view: View) => void;
}) {
  const [playing, setPlaying] = useState(false);
  const [replaying, setReplaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [displayTime, setDisplayTime] = useState(0);
  const [showGuidePath, setShowGuidePath] = useState(true);
  const [guideRenderMode, setGuideRenderMode] =
    useState<GuideRenderMode>("animated");
  const [showCourseProps, setShowCourseProps] = useState(false);
  const [showPropLabels, setShowPropLabels] = useState(true);
  const [showCollisions, setShowCollisions] = useState(false);
  const [showCollisionGeometry, setShowCollisionGeometry] = useState(true);
  const [collisionGeometryRenderMode, setCollisionGeometryRenderMode] =
    useState<CollisionGeometryRenderMode>("overview");
  const [collisionGeometry, setCollisionGeometry] = useState<
    CollisionGeometryShape[]
  >([]);
  const [collisionGeometryStatus, setCollisionGeometryStatus] =
    useState<CollisionGeometryStatus>("idle");
  const [collisionGeometryMessage, setCollisionGeometryMessage] = useState<
    string | null
  >(null);

  const playheadRef = useRef(0);
  const playingRef = useRef(false);
  const replayingRef = useRef(false);
  const speedRef = useRef(1);
  const collisionGeometryRequestRef = useRef(0);

  useEffect(() => {
    playingRef.current = playing;
  }, [playing]);
  useEffect(() => {
    replayingRef.current = replaying;
  }, [replaying]);
  useEffect(() => {
    speedRef.current = speed;
  }, [speed]);

  useEffect(() => {
    collisionGeometryRequestRef.current += 1;
    setShowCollisions(false);
    setShowCollisionGeometry(true);
    setCollisionGeometry([]);
    setCollisionGeometryStatus("idle");
    setCollisionGeometryMessage(null);
  }, [captureId, sessionId]);

  const loadCollisionGeometry = useCallback(() => {
    const requestId = collisionGeometryRequestRef.current + 1;
    collisionGeometryRequestRef.current = requestId;
    setCollisionGeometryStatus("loading");
    setCollisionGeometryMessage(null);

    void api
      .resolveSessionCollisionGeometry(captureId, sessionId)
      .then((result) => {
        if (collisionGeometryRequestRef.current !== requestId) return;
        const status: CollisionGeometryStatus =
          result.shapes.length === 0
            ? "missing"
            : result.status === "partial"
              ? "partial"
              : "ready";
        setCollisionGeometry(result.shapes);
        setCollisionGeometryStatus(status);
        setCollisionGeometryMessage(
          result.message ?? result.warnings[0] ?? null,
        );
      })
      .catch((e) => {
        if (collisionGeometryRequestRef.current !== requestId) return;
        setCollisionGeometry([]);
        setCollisionGeometryStatus("error");
        setCollisionGeometryMessage(formatError(e));
      });
  }, [captureId, sessionId]);

  // Keep the scrubber/readout in sync while playing (throttled — the 3D
  // update itself runs every frame off the refs, not React state).
  useEffect(() => {
    if (!playing) return;
    const id = setInterval(() => setDisplayTime(playheadRef.current), 80);
    return () => clearInterval(id);
  }, [playing]);

  const onEnd = useCallback(() => {
    playingRef.current = false;
    setPlaying(false);
    setDisplayTime(path.duration);
  }, [path.duration]);

  const pathLines = useMemo(
    () =>
      path.pathSegments.map((segment) => {
        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute(
          "position",
          new THREE.BufferAttribute(segment.positions, 3),
        );
        const material = new THREE.LineBasicMaterial({ color: segment.color });
        return new THREE.Line(geometry, material);
      }),
    [path.pathSegments],
  );

  useEffect(() => {
    return () => {
      pathLines.forEach((line) => {
        line.geometry.dispose();
        (line.material as THREE.Material).dispose();
      });
    };
  }, [pathLines]);

  const play = () => {
    if (playheadRef.current >= path.duration - 1e-6) playheadRef.current = 0;
    setReplaying(true);
    setPlaying(true);
  };
  const pause = () => setPlaying(false);
  const reset = () => {
    playheadRef.current = 0;
    setDisplayTime(0);
    setPlaying(false);
    setReplaying(false);
  };
  const scrub = (v: number) => {
    playheadRef.current = v;
    setDisplayTime(v);
    setReplaying(true);
    setPlaying(false);
  };
  const toggleCollisionGeometry = (checked: boolean) => {
    setShowCollisionGeometry(checked);
    if (!checked || courseStatus !== "ready") {
      return;
    }
    if (
      collisionGeometryStatus === "idle" ||
      collisionGeometryStatus === "missing" ||
      collisionGeometryStatus === "error"
    ) {
      loadCollisionGeometry();
    }
  };

  useEffect(() => {
    if (
      showCollisionGeometry &&
      courseStatus === "ready" &&
      collisionGeometryStatus === "idle"
    ) {
      loadCollisionGeometry();
    }
  }, [
    collisionGeometryStatus,
    courseStatus,
    loadCollisionGeometry,
    showCollisionGeometry,
  ]);

  const camPos: [number, number, number] = [
    path.radius * 1.6,
    path.height + path.radius * 1.1,
    path.radius * 1.6,
  ];
  const coursePropCount = course?.props.filter(shouldRenderCourseProp).length ?? 0;
  const guideSegmentCount = course?.guide_path?.segments.length ?? 0;
  const collisionCount = path.collisionEvents.length;
  const collisionRelevance = useMemo(
    () => buildCollisionRelevanceContext(path),
    [path],
  );
  const visibleCollisionGeometry = useMemo(
    () => {
      if (!course) return [];
      return collisionGeometry
        .map((shape) =>
          buildCollisionGeometryRenderItem(
            shape,
            collisionGeometry,
            course,
            path,
            collisionRelevance,
            collisionGeometryRenderMode,
          ),
        )
        .filter((item): item is CollisionGeometryRenderItem => item != null);
    },
    [
      collisionGeometry,
      collisionGeometryRenderMode,
      collisionRelevance,
      course,
      path,
    ],
  );
  const collisionGeometryCount = visibleCollisionGeometry.length;
  const lapLegend = useMemo(() => buildLapLegend(path), [path]);

  return (
    <div className="relative h-full w-full">
      <Canvas
        className="absolute inset-0"
        camera={{
          position: camPos,
          fov: 50,
          near: 0.1,
          far: Math.max(2000, path.radius * 40),
        }}
        gl={{ antialias: true, alpha: true }}
      >
        <Grid
          cellSize={1}
          cellThickness={0.6}
          cellColor={GRID_CELL}
          sectionSize={10}
          sectionThickness={1}
          sectionColor={GRID_SECTION}
          infiniteGrid
          fadeDistance={path.radius * 8}
          fadeStrength={1.5}
        />
        {course && (
          <CourseScene
            course={course}
            path={path}
            showGuidePath={showGuidePath}
            guideRenderMode={guideRenderMode}
            showProps={showCourseProps}
            showPropLabels={showPropLabels}
          />
        )}
        {showCollisionGeometry && visibleCollisionGeometry.length > 0 && (
          <CollisionGeometryScene items={visibleCollisionGeometry} path={path} />
        )}
        <PathScene
          path={path}
          lines={pathLines}
          playingRef={playingRef}
          replayingRef={replayingRef}
          speedRef={speedRef}
          playheadRef={playheadRef}
          onEnd={onEnd}
        />
        {showCollisions && (
          <CollisionMarkers
            path={path}
            replayingRef={replayingRef}
            playheadRef={playheadRef}
          />
        )}
        <OrbitControls
          makeDefault
          enableDamping
          target={[0, path.height / 2, 0]}
        />
      </Canvas>

      {/* Header overlay */}
      <div className="pointer-events-none absolute inset-x-0 top-0 flex items-start justify-between gap-3 p-5">
        <div className="pointer-events-auto">
          <div className="eyebrow">Flight Path</div>
          <div className="font-mono text-sm">{path.sessionTitle}</div>
        </div>
        <Button
          variant="ghost"
          size="sm"
          className="pointer-events-auto"
          onClick={() =>
            onNavigate({ kind: "race-detail", captureId, sessionId })
          }
        >
          Back to race
        </Button>
      </div>

      {(courseStatus !== "idle" || collisionCount > 0) && (
        <div className="pointer-events-none absolute left-5 top-16 flex max-w-[min(28rem,calc(100vw-2.5rem))] flex-col items-start gap-2">
          <div className="border-border/70 bg-card/75 pointer-events-auto flex items-center gap-3 rounded-md border px-2.5 py-1.5 font-mono text-[11px] text-muted-foreground backdrop-blur">
            {courseStatus !== "idle" ? (
              <span>
                {courseStatus === "loading" && (
                  <span className="inline-flex items-center gap-1.5">
                    <Loader2 className="size-3 animate-spin" />
                    loading course data…
                  </span>
                )}
                {courseStatus === "ready" &&
                  `${course?.checkpoints.length ?? 0} gates · ${course?.game_title ?? ""}`}
                {courseStatus === "missing" &&
                  (courseMessage ?? "course data unavailable")}
                {courseStatus === "error" &&
                  `course data error: ${courseMessage ?? "unknown"}`}
              </span>
            ) : (
              <span>
                {collisionCount} impact{collisionCount === 1 ? "" : "s"}
              </span>
            )}
          </div>
          {(collisionCount > 0 || courseStatus === "ready") && (
            <div className="border-border/70 bg-card/75 pointer-events-auto rounded-md border px-2.5 py-2 font-mono text-[11px] text-muted-foreground backdrop-blur">
              <div className="mb-1.5 text-[10px] uppercase tracking-[0.16em] text-muted-foreground/80">
                Render Options
              </div>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
                {collisionCount > 0 && (
                  <label className="text-foreground/80 flex cursor-pointer items-center gap-1.5">
                    <input
                      type="checkbox"
                      checked={showCollisions}
                      onChange={(e) =>
                        setShowCollisions(e.currentTarget.checked)
                      }
                      className="size-3 accent-[var(--signal)]"
                    />
                    Impacts
                  </label>
                )}
                {courseStatus === "ready" && (
                  <label className="text-foreground/80 flex cursor-pointer items-center gap-1.5">
                    <input
                      type="checkbox"
                      checked={showCollisionGeometry}
                      disabled={collisionGeometryStatus === "loading"}
                      onChange={(e) =>
                        toggleCollisionGeometry(e.currentTarget.checked)
                      }
                      className="size-3 accent-[var(--signal)] disabled:cursor-wait"
                    />
                    Colliders
                    {collisionGeometryStatus === "loading" && (
                      <Loader2 className="size-3 animate-spin text-muted-foreground" />
                    )}
                    {collisionGeometryCount > 0 && (
                      <span className="text-muted-foreground/80">
                        {collisionGeometryCount.toLocaleString()}
                      </span>
                    )}
                  </label>
                )}
                {guideSegmentCount > 0 && (
                  <>
                    <label className="text-foreground/80 flex cursor-pointer items-center gap-1.5">
                      <input
                        type="checkbox"
                        checked={showGuidePath}
                        onChange={(e) =>
                          setShowGuidePath(e.currentTarget.checked)
                        }
                        className="size-3 accent-[var(--signal)]"
                      />
                      Guide
                    </label>
                    {showGuidePath && (
                      <div className="border-border/70 bg-background/30 flex rounded border p-0.5">
                        {(["animated", "solid"] as const).map((mode) => (
                          <button
                            key={mode}
                            type="button"
                            onClick={() => setGuideRenderMode(mode)}
                            className={cn(
                              "rounded px-2 py-0.5 capitalize transition-colors",
                              guideRenderMode === mode
                                ? "bg-signal/15 text-signal"
                                : "text-muted-foreground hover:text-foreground",
                            )}
                          >
                            {mode}
                          </button>
                        ))}
                      </div>
                    )}
                  </>
                )}
                {showCollisionGeometry && collisionGeometry.length > 0 && (
                  <div className="border-border/70 bg-background/30 flex rounded border p-0.5">
                    {(["overview", "nearby"] as const).map((mode) => (
                      <button
                        key={mode}
                        type="button"
                        onClick={() => setCollisionGeometryRenderMode(mode)}
                        className={cn(
                          "rounded px-2 py-0.5 capitalize transition-colors",
                          collisionGeometryRenderMode === mode
                            ? "bg-signal/15 text-signal"
                            : "text-muted-foreground hover:text-foreground",
                        )}
                      >
                        {mode}
                      </button>
                    ))}
                  </div>
                )}
                {coursePropCount > 0 && (
                  <label className="text-foreground/80 flex cursor-pointer items-center gap-1.5">
                    <input
                      type="checkbox"
                      checked={showCourseProps}
                      onChange={(e) =>
                        setShowCourseProps(e.currentTarget.checked)
                      }
                      className="size-3 accent-[var(--signal)]"
                    />
                    Props
                  </label>
                )}
                {coursePropCount > 0 && showCourseProps && (
                  <label className="text-foreground/80 flex cursor-pointer items-center gap-1.5">
                    <input
                      type="checkbox"
                      checked={showPropLabels}
                      onChange={(e) =>
                        setShowPropLabels(e.currentTarget.checked)
                      }
                      className="size-3 accent-[var(--signal)]"
                    />
                    Labels
                  </label>
                )}
              </div>
              {lapLegend.length > 1 && (
                <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-muted-foreground">
                  {lapLegend.map((lap) => (
                    <span
                      key={lap.lapIndex}
                      className="inline-flex items-center gap-1.5"
                    >
                      <span
                        className="size-2 rounded-full"
                        style={{ backgroundColor: lap.color }}
                      />
                      Lap {lap.lapIndex}
                    </span>
                  ))}
                </div>
              )}
              {showCollisionGeometry &&
                collisionGeometryMessage &&
                (collisionGeometryStatus === "missing" ||
                  collisionGeometryStatus === "partial" ||
                  collisionGeometryStatus === "error") && (
                  <div className="mt-1.5 max-w-80 text-[10px] leading-snug text-muted-foreground">
                    {collisionGeometryMessage}
                  </div>
                )}
            </div>
          )}
        </div>
      )}

      {/* Playback controls */}
      <div className="border-border/80 bg-card/80 absolute inset-x-0 bottom-0 flex items-center gap-4 border-t px-4 py-2.5 backdrop-blur">
        <div className="flex items-center gap-1">
          <Button
            size="icon"
            variant="ghost"
            onClick={playing ? pause : play}
            aria-label={playing ? "Pause" : "Play"}
          >
            {playing ? <Pause /> : <Play />}
          </Button>
          <Button
            size="icon"
            variant="ghost"
            onClick={reset}
            disabled={!replaying}
            aria-label="Reset"
          >
            <RotateCcw />
          </Button>
        </div>

        <input
          type="range"
          min={0}
          max={path.duration}
          step={0.01}
          value={replaying ? displayTime : 0}
          onChange={(e) => scrub(parseFloat(e.target.value))}
          className="h-1 flex-1 cursor-pointer accent-[var(--signal)]"
          aria-label="Timeline"
        />

        <div className="text-muted-foreground w-28 text-right font-mono text-xs tabular-nums">
          {replaying ? displayTime.toFixed(1) : "full"} /{" "}
          {path.duration.toFixed(1)}s
        </div>

        <div className="flex items-center gap-0.5">
          {SPEEDS.map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => setSpeed(s)}
              className={cn(
                "rounded px-2 py-1 font-mono text-[11px] tabular-nums transition-colors",
                speed === s
                  ? "bg-signal/15 text-signal"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {s}×
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function PathScene({
  path,
  lines,
  playingRef,
  replayingRef,
  speedRef,
  playheadRef,
  onEnd,
}: {
  path: PreparedPath;
  lines: THREE.Line[];
  playingRef: React.RefObject<boolean>;
  replayingRef: React.RefObject<boolean>;
  speedRef: React.RefObject<number>;
  playheadRef: React.RefObject<number>;
  onEnd: () => void;
}) {
  const headRef = useRef<THREE.Mesh>(null);

  useFrame((_, delta) => {
    const head = headRef.current;

    // Idle: show the entire traced path, hide the moving head.
    if (!replayingRef.current) {
      for (let index = 0; index < lines.length; index++) {
        lines[index].geometry.setDrawRange(
          0,
          path.pathSegments[index]?.points.length ?? 0,
        );
      }
      if (head) head.visible = false;
      return;
    }

    if (playingRef.current) {
      playheadRef.current += delta * speedRef.current;
      if (playheadRef.current >= path.duration) {
        playheadRef.current = path.duration;
        onEnd();
      }
    }

    for (let index = 0; index < lines.length; index++) {
      const segment = path.pathSegments[index];
      const count = segment
        ? Math.min(
            countUpTo(segment.times, playheadRef.current),
            segment.points.length,
          )
        : 0;
      lines[index].geometry.setDrawRange(0, count);
    }

    const count = Math.min(
      Math.max(countUpTo(path.times, playheadRef.current), 1),
      path.points.length,
    );
    if (head) {
      head.visible = true;
      head.position.copy(path.points[count - 1]);
    }
  });

  const headRadius = Math.max(path.radius * 0.018, 0.15);

  return (
    <>
      {lines.map((line, index) => (
        <primitive key={path.pathSegments[index]?.id ?? index} object={line} />
      ))}
      <mesh ref={headRef} visible={false}>
        <sphereGeometry args={[headRadius, 16, 16]} />
        <meshBasicMaterial color={SIGNAL} />
      </mesh>
    </>
  );
}

function CollisionMarkers({
  path,
  replayingRef,
  playheadRef,
}: {
  path: PreparedPath;
  replayingRef: React.RefObject<boolean>;
  playheadRef: React.RefObject<number>;
}) {
  if (path.collisionEvents.length === 0) return null;

  const refs = useRef<(THREE.Group | null)[]>([]);
  const baseRadius = Math.max(path.radius * 0.018, 0.16);

  useFrame(() => {
    const showAll = !replayingRef.current;
    const playhead = playheadRef.current;
    for (let i = 0; i < refs.current.length; i++) {
      const marker = refs.current[i];
      if (marker) {
        marker.visible = showAll || path.collisionEvents[i].replayTime <= playhead;
      }
    }
  });

  return (
    <group>
      {path.collisionEvents.map((event, index) => {
        const markerRadius = baseRadius * (0.85 + event.severity / 10);
        const severityFontSize = clamp(path.radius * 0.018, 0.32, 0.75);
        const hitLabel = event.geometry_confirmed
          ? collisionHitLabel(event)
          : null;
        const hitDetail = event.geometry_confirmed
          ? collisionHitDetail(event)
          : null;
        const hitText = [hitLabel, hitDetail].filter(Boolean).join("\n");
        const hitFontSize = clamp(path.radius * 0.011, 0.18, 0.38);
        const hitMaxWidth = clamp(path.radius * 0.16, 2.8, 7.5);
        return (
          <group
            key={`${event.sample_index}-${event.capture_time_seconds}-${index}`}
            ref={(node) => {
              refs.current[index] = node;
            }}
            position={event.point}
          >
            <mesh>
              <sphereGeometry args={[markerRadius, 18, 18]} />
              <meshBasicMaterial
                color={collisionColor(event.severity)}
                transparent
                opacity={0.9}
              />
            </mesh>
            {event.geometry_confirmed && (
              <mesh rotation={[Math.PI / 2, 0, 0]}>
                <torusGeometry args={[markerRadius * 1.45, markerRadius * 0.08, 8, 28]} />
                <meshBasicMaterial
                  color="#ffffff"
                  transparent
                  opacity={0.78}
                />
              </mesh>
            )}
            <Billboard position={[0, markerRadius * 2.4, 0]}>
              <Text
                color="#ffffff"
                fontSize={severityFontSize}
                anchorX="center"
                anchorY="middle"
                outlineColor="#020617"
                outlineWidth={0.04}
              >
                {event.severity}/10
              </Text>
            </Billboard>
            {hitText && (
              <Billboard position={[0, markerRadius * 3.75, 0]}>
                <Text
                  color="#e2e8f0"
                  fontSize={hitFontSize}
                  maxWidth={hitMaxWidth}
                  textAlign="center"
                  lineHeight={1.1}
                  anchorX="center"
                  anchorY="middle"
                  outlineColor="#020617"
                  outlineWidth={0.035}
                >
                  {hitText}
                </Text>
              </Billboard>
            )}
          </group>
        );
      })}
    </group>
  );
}

function CourseScene({
  course,
  path,
  showGuidePath,
  guideRenderMode,
  showProps,
  showPropLabels,
}: {
  course: ReplayCourseData;
  path: PreparedPath;
  showGuidePath: boolean;
  guideRenderMode: GuideRenderMode;
  showProps: boolean;
  showPropLabels: boolean;
}) {
  const spawn = course.spawnpoint;

  return (
    <group>
      {showGuidePath && course.guide_path && (
        <CourseGuidePath
          guidePath={course.guide_path}
          path={path}
          mode={guideRenderMode}
        />
      )}
      {course.checkpoints.map((checkpoint) => (
        <GateMesh
          key={`${checkpoint.sequence_index}-${checkpoint.checkpoint_id}-${checkpoint.passage_type}`}
          checkpoint={checkpoint}
          path={path}
          gameTitle={course.game_title}
        />
      ))}
      {showProps &&
        course.props.filter(shouldRenderCourseProp).map((prop) => (
          <CoursePropMesh
            key={`${prop.instance_id}-${prop.item_id}`}
            prop={prop}
            path={path}
            showLabel={showPropLabels}
          />
        ))}
      {spawn && (
        <mesh
          position={toScenePosition(spawn.position, path)}
          quaternion={toSceneQuaternion(spawn.rotation)}
        >
          <sphereGeometry args={[Math.max(path.radius * 0.012, 0.12), 12, 12]} />
          <meshBasicMaterial color={COURSE_WARN} transparent opacity={0.75} />
        </mesh>
      )}
    </group>
  );
}

function CollisionGeometryScene({
  items,
  path,
}: {
  items: CollisionGeometryRenderItem[];
  path: PreparedPath;
}) {
  const renderObjects = useMemo(() => {
    const lineGroups = new Map<
      string,
      { style: ResolvedColliderRenderStyle; shapes: CollisionGeometryShape[] }
    >();
    const fillGroups = new Map<
      string,
      { style: ResolvedColliderRenderStyle; shapes: CollisionGeometryShape[] }
    >();

    for (const item of items) {
      if (item.style.lineOpacity > 0) {
        const key = colliderLineStyleKey(item.style);
        const group = lineGroups.get(key) ?? {
          style: item.style,
          shapes: [],
        };
        group.shapes.push(item.shape);
        lineGroups.set(key, group);
      }
      if (item.style.fillColor && item.style.fillOpacity > 0) {
        const key = colliderFillStyleKey(item.style);
        const group = fillGroups.get(key) ?? {
          style: item.style,
          shapes: [],
        };
        group.shapes.push(item.shape);
        fillGroups.set(key, group);
      }
    }

    const objects: THREE.Object3D[] = [];
    for (const group of [...fillGroups.values()].sort(
      (a, b) => a.style.fillRenderOrder - b.style.fillRenderOrder,
    )) {
      const object = buildCollisionBoxFills(
        group.shapes,
        path,
        group.style.fillColor ?? "#ffffff",
        group.style.fillOpacity,
        group.style.fillRenderOrder,
      );
      if (object) objects.push(object);
    }
    for (const group of [...lineGroups.values()].sort(
      (a, b) => a.style.lineRenderOrder - b.style.lineRenderOrder,
    )) {
      const object = buildCollisionLineSegments(
        group.shapes,
        path,
        group.style.lineColor,
        group.style.lineOpacity,
        group.style.lineRenderOrder,
      );
      if (object) objects.push(object);
    }
    return objects;
  }, [items, path.centerX, path.centerZ, path.minY]);

  useEffect(() => {
    return () => {
      renderObjects.forEach(disposeCollisionRenderObject);
    };
  }, [renderObjects]);

  if (items.length === 0) return null;

  return (
    <group>
      {renderObjects.map((object, index) => (
        <primitive key={index} object={object} />
      ))}
    </group>
  );
}

function buildCollisionGeometryRenderItem(
  shape: CollisionGeometryShape,
  allShapes: readonly CollisionGeometryShape[],
  course: ReplayCourseData,
  path: PreparedPath,
  relevance: CollisionRelevanceContext,
  mode: CollisionGeometryRenderMode,
): CollisionGeometryRenderItem | null {
  const maxSpan = collisionShapeMaxSpan(shape);
  const sceneCenter = toScenePosition(shape.center, path);
  const decision = resolveCourseColliderRenderDecision({
    course,
    shape,
    allShapes,
    renderMode: mode,
    maxSpan,
    isConfirmedHit: relevance.hitLabels.has(shape.label.trim()),
    isNearPath: () => isCollisionShapeNearPath(shape, path, relevance),
    worldCenter: shape.center,
    sceneCenter,
    pathHeight: path.height,
    labelKey: normalizeColliderPolicyKey(shape.label),
    objectPathKey: normalizeColliderPolicyKey(shape.object_path),
    sourceKey: normalizeColliderPolicyKey(shape.source_id),
    sourceAssetKey: normalizeColliderPolicyKey(shape.source_asset),
  });

  if (decision.action === "hide") return null;

  const visible =
    decision.action === "show" ||
    shouldRenderCollisionGeometry(shape, path, relevance, mode);
  if (!visible) return null;

  return {
    shape: applyColliderRenderGeometry(shape, decision.geometry),
    style: resolveCollisionRenderStyle(decision.style),
  };
}

function applyColliderRenderGeometry(
  shape: CollisionGeometryShape,
  geometry: ColliderRenderGeometry | undefined,
): CollisionGeometryShape {
  if (!geometry) return shape;
  return {
    ...shape,
    center: geometry.center ?? shape.center,
    half_extents: geometry.half_extents ?? shape.half_extents,
  };
}

function resolveCollisionRenderStyle(
  override: ColliderRenderStyle | undefined,
): ResolvedColliderRenderStyle {
  const base = DEFAULT_COLLIDER_RENDER_STYLE;
  if (!override) return base;

  return {
    lineColor: override.lineColor ?? base.lineColor,
    lineOpacity: clamp(override.lineOpacity ?? base.lineOpacity, 0, 1),
    lineRenderOrder: override.lineRenderOrder ?? base.lineRenderOrder,
    fillColor:
      override.fillColor === undefined ? base.fillColor : override.fillColor,
    fillOpacity: clamp(override.fillOpacity ?? base.fillOpacity, 0, 1),
    fillRenderOrder: override.fillRenderOrder ?? base.fillRenderOrder,
  };
}

function colliderLineStyleKey(style: ResolvedColliderRenderStyle): string {
  return [
    style.lineColor,
    style.lineOpacity,
    style.lineRenderOrder,
  ].join("|");
}

function colliderFillStyleKey(style: ResolvedColliderRenderStyle): string {
  return [
    style.fillColor ?? "",
    style.fillOpacity,
    style.fillRenderOrder,
  ].join("|");
}

function buildCollisionLineSegments(
  shapes: CollisionGeometryShape[],
  path: PreparedPath,
  color: string,
  opacity: number,
  renderOrder: number,
): THREE.LineSegments | null {
  if (shapes.length === 0) return null;

  const positions = new Float32Array(shapes.length * 24 * 3);
  const center = new THREE.Vector3();
  const rotation = new THREE.Quaternion();
  const local = new THREE.Vector3();
  const corners = Array.from({ length: 8 }, () => new THREE.Vector3());
  const edgeIndices = [
    0, 1, 1, 2, 2, 3, 3, 0,
    4, 5, 5, 6, 6, 7, 7, 4,
    0, 4, 1, 5, 2, 6, 3, 7,
  ];
  let offset = 0;

  for (const shape of shapes) {
    center.set(...toScenePosition(shape.center, path));
    const x = Math.max(Math.abs(shape.half_extents[0]), 0.015);
    const y = Math.max(Math.abs(shape.half_extents[1]), 0.015);
    const z = Math.max(Math.abs(shape.half_extents[2]), 0.015);
    setSceneQuaternionFromUnity(rotation, shape.rotation);
    setBoxCorner(corners[0], local, center, rotation, -x, -y, -z);
    setBoxCorner(corners[1], local, center, rotation, x, -y, -z);
    setBoxCorner(corners[2], local, center, rotation, x, y, -z);
    setBoxCorner(corners[3], local, center, rotation, -x, y, -z);
    setBoxCorner(corners[4], local, center, rotation, -x, -y, z);
    setBoxCorner(corners[5], local, center, rotation, x, -y, z);
    setBoxCorner(corners[6], local, center, rotation, x, y, z);
    setBoxCorner(corners[7], local, center, rotation, -x, y, z);

    for (const cornerIndex of edgeIndices) {
      const corner = corners[cornerIndex];
      positions[offset++] = corner.x;
      positions[offset++] = corner.y;
      positions[offset++] = corner.z;
    }
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  const material = new THREE.LineBasicMaterial({
    color,
    depthWrite: false,
    opacity,
    transparent: true,
  });
  const lineSegments = new THREE.LineSegments(geometry, material);
  lineSegments.frustumCulled = false;
  lineSegments.renderOrder = renderOrder;
  return lineSegments;
}

function buildCollisionBoxFills(
  shapes: CollisionGeometryShape[],
  path: PreparedPath,
  color: string,
  opacity: number,
  renderOrder: number,
): THREE.InstancedMesh | null {
  if (shapes.length === 0) return null;

  const geometry = new THREE.BoxGeometry(1, 1, 1);
  const material = new THREE.MeshBasicMaterial({
    color,
    depthWrite: false,
    opacity,
    transparent: true,
  });
  const mesh = new THREE.InstancedMesh(geometry, material, shapes.length);
  const center = new THREE.Vector3();
  const rotation = new THREE.Quaternion();
  const scale = new THREE.Vector3();
  const matrix = new THREE.Matrix4();

  shapes.forEach((shape, index) => {
    center.set(...toScenePosition(shape.center, path));
    setSceneQuaternionFromUnity(rotation, shape.rotation);
    scale.set(
      Math.max(Math.abs(shape.half_extents[0]) * 2, 0.03),
      Math.max(Math.abs(shape.half_extents[1]) * 2, 0.03),
      Math.max(Math.abs(shape.half_extents[2]) * 2, 0.03),
    );
    matrix.compose(center, rotation, scale);
    mesh.setMatrixAt(index, matrix);
  });
  mesh.instanceMatrix.needsUpdate = true;
  mesh.frustumCulled = false;
  mesh.renderOrder = renderOrder;
  return mesh;
}

function disposeCollisionRenderObject(object: THREE.Object3D) {
  const renderable = object as THREE.Object3D & {
    geometry?: THREE.BufferGeometry;
    material?: THREE.Material | THREE.Material[];
  };
  renderable.geometry?.dispose();
  if (Array.isArray(renderable.material)) {
    renderable.material.forEach((material) => material.dispose());
  } else {
    renderable.material?.dispose();
  }
}

function CourseGuidePath({
  guidePath,
  path,
  mode,
}: {
  guidePath: ReplayGuidePath;
  path: PreparedPath;
  mode: GuideRenderMode;
}) {
  const traces = useMemo(
    () => buildGuideTraces(guidePath, path),
    [guidePath, path.centerX, path.centerZ, path.minY],
  );

  return (
    <group>
      {traces.map((trace, traceIndex) =>
        mode === "solid" ? (
          <SolidGuideTrace key={traceIndex} trace={trace} />
        ) : (
          GUIDE_TRAIL_PHASES.map((trailPhase, trailIndex) => (
            <AnimatedGuideTrace
              key={`${traceIndex}-${trailIndex}`}
              trace={trace}
              radius={path.radius}
              phase={(traceIndex * 0.07 + trailPhase) % 1}
            />
          ))
        ),
      )}
    </group>
  );
}

function SolidGuideTrace({ trace }: { trace: GuideTrace }) {
  return (
    <Line
      points={trace.points}
      color={GUIDE_PATH}
      lineWidth={1.5}
      transparent
      opacity={0.72}
    />
  );
}

function AnimatedGuideTrace({
  trace,
  radius,
  phase,
}: {
  trace: GuideTrace;
  radius: number;
  phase: number;
}) {
  const arrowRefs = useRef<Array<THREE.Group | null>>([]);
  const arrowCount = Math.min(14, Math.max(7, Math.round(trace.length / 7)));
  const arrowLength = Math.min(Math.max(radius * 0.014, 0.22), 0.75);
  const arrowRadius = arrowLength * 0.34;
  const tailLength = arrowLength * 0.9;
  const tailRadius = arrowLength * 0.07;
  const windowLength = Math.min(
    Math.max(trace.length * 0.075, arrowLength * arrowCount * 1.2),
    trace.length,
  );
  const spacing = windowLength / Math.max(arrowCount - 1, 1);
  const cycleDuration = Math.max(8, Math.min(26, trace.length / 9));

  useFrame(({ clock }) => {
    const headProgress =
      (((clock.elapsedTime * GUIDE_ARROW_SPEED) / cycleDuration + phase) % 1 +
        1) %
      1;
    const headDistance = headProgress * trace.length;

    for (let index = 0; index < arrowRefs.current.length; index++) {
      const arrow = arrowRefs.current[index];
      if (!arrow) continue;

      const sample = sampleGuideTrace(trace, headDistance - index * spacing);
      if (!sample) {
        arrow.visible = false;
        continue;
      }

      arrow.visible = true;
      arrow.position.copy(sample.position);
      arrow.quaternion.setFromUnitVectors(GUIDE_ARROW_UP, sample.tangent);
    }
  });

  return (
    <group>
      {Array.from({ length: arrowCount }).map((_, index) => {
        const opacity = THREE.MathUtils.lerp(
          0.28,
          0.9,
          1 - index / Math.max(arrowCount - 1, 1),
        );
        const scale = THREE.MathUtils.lerp(
          0.74,
          1,
          1 - index / Math.max(arrowCount - 1, 1),
        );

        return (
          <group
            key={index}
            ref={(node) => {
              arrowRefs.current[index] = node;
            }}
            scale={[scale, scale, scale]}
          >
            <mesh position={[0, arrowLength * 0.28, 0]}>
              <coneGeometry args={[arrowRadius, arrowLength, 14]} />
              <meshBasicMaterial
                color={GUIDE_PATH}
                transparent
                opacity={opacity}
                depthWrite={false}
              />
            </mesh>
            <mesh position={[0, -tailLength * 0.36, 0]}>
              <cylinderGeometry
                args={[tailRadius, tailRadius, tailLength, 8]}
              />
              <meshBasicMaterial
                color={GUIDE_PATH}
                transparent
                opacity={opacity * 0.72}
                depthWrite={false}
              />
            </mesh>
          </group>
        );
      })}
    </group>
  );
}

function buildGuideTraces(
  guidePath: ReplayGuidePath,
  path: PreparedPath,
): GuideTrace[] {
  const traces: THREE.Vector3[][] = [];
  let current: THREE.Vector3[] = [];
  const continuityTolerance = 1.25;

  for (const segment of guidePath.segments) {
    const points = guideSegmentScenePoints(segment, path);
    if (points.length < 2) continue;

    if (
      current.length > 0 &&
      current[current.length - 1].distanceTo(points[0]) > continuityTolerance
    ) {
      traces.push(current);
      current = [];
    }

    if (current.length === 0) {
      current.push(...points);
    } else {
      current.push(...points.slice(1));
    }
  }

  if (current.length > 1) {
    traces.push(current);
  }

  return traces
    .map((points) => buildGuideTrace(points))
    .filter((trace): trace is GuideTrace => trace != null);
}

function guideSegmentScenePoints(
  segment: ReplayGuidePathSegment,
  path: PreparedPath,
): THREE.Vector3[] {
  return segment.points.map(
    (point) => new THREE.Vector3(...toScenePosition(point, path)),
  );
}

function buildGuideTrace(points: THREE.Vector3[]): GuideTrace | null {
  const distances = new Array(points.length).fill(0) as number[];
  let length = 0;
  for (let index = 1; index < points.length; index++) {
    length += points[index - 1].distanceTo(points[index]);
    distances[index] = length;
  }

  if (!Number.isFinite(length) || length <= 0) {
    return null;
  }

  return { points, distances, length };
}

function sampleGuideTrace(
  trace: GuideTrace,
  distance: number,
): { position: THREE.Vector3; tangent: THREE.Vector3 } | null {
  const target = ((distance % trace.length) + trace.length) % trace.length;
  let lo = 0;
  let hi = trace.distances.length - 1;

  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (trace.distances[mid] < target) lo = mid + 1;
    else hi = mid;
  }

  const next = Math.max(1, lo);
  const prev = next - 1;
  const startDistance = trace.distances[prev];
  const endDistance = trace.distances[next];
  const span = Math.max(endDistance - startDistance, 0.0001);
  const t = THREE.MathUtils.clamp((target - startDistance) / span, 0, 1);
  const position = trace.points[prev].clone().lerp(trace.points[next], t);
  const tangent = trace.points[next].clone().sub(trace.points[prev]).normalize();

  if (tangent.lengthSq() <= 0) return null;
  return { position, tangent };
}

function CoursePropMesh({
  prop,
  path,
  showLabel,
}: {
  prop: ReplayCourseProp;
  path: PreparedPath;
  showLabel: boolean;
}) {
  const dims = (prop.dimensions ?? [0.5, 0.5, 0.5]).map((value) =>
    Math.max(Math.abs(value), 0.08),
  ) as [number, number, number];
  const position = toScenePosition(prop.position, path);

  return (
    <group position={position}>
      <mesh quaternion={toSceneQuaternion(prop.rotation)}>
        <boxGeometry args={dims} />
        <meshBasicMaterial
          color="#94a3b8"
          transparent
          opacity={0.28}
          wireframe
        />
      </mesh>
      {showLabel && (
        <Billboard position={[0, dims[1] * 0.65 + 0.35, 0]}>
          <Text
            color="#e5e7eb"
            fontSize={0.55}
            anchorX="center"
            anchorY="middle"
            outlineColor="#020617"
            outlineWidth={0.035}
            maxWidth={10}
          >
            {propLabel(prop)}
          </Text>
        </Billboard>
      )}
    </group>
  );
}

function GateMesh({
  checkpoint,
  path,
  gameTitle,
}: {
  checkpoint: ReplayCheckpoint;
  path: PreparedPath;
  gameTitle: string;
}) {
  const dims = toGateBoxDimensions(checkpoint.dimensions, gameTitle);
  const color =
    checkpoint.passage_type.toLowerCase() === "finish"
      ? COURSE_FINISH
      : checkpoint.passage_type.toLowerCase() === "start"
        ? COURSE_WARN
        : SIGNAL;

  return (
    <mesh
      position={toScenePosition(checkpoint.position, path)}
      quaternion={toSceneQuaternion(checkpoint.rotation)}
    >
      <boxGeometry args={dims} />
      <meshBasicMaterial
        color={color}
        transparent
        opacity={0.62}
        wireframe
      />
    </mesh>
  );
}

function toGateBoxDimensions(
  [x, y, z]: [number, number, number],
  gameTitle: string,
): [number, number, number] {
  if (isMicroDrones(gameTitle)) {
    return [
      Math.max(Math.abs(x), 0.08),
      Math.max(Math.abs(y), 0.08),
      Math.max(Math.abs(z), 0.08),
    ];
  }

  return [
    Math.max(Math.abs(z), 0.08),
    Math.max(Math.abs(y), 0.08),
    Math.max(Math.abs(x), 0.08),
  ];
}

function isMicroDrones(gameTitle: string): boolean {
  return gameTitle.toLowerCase().replace(/[^a-z0-9]/g, "").includes("microdrones");
}

function shouldRenderCourseProp(prop: ReplayCourseProp): boolean {
  if (prop.procedural_geometry) return false;
  const itemId = prop.item_id.toLowerCase();
  return !(
    itemId.includes("checkpoint") ||
    itemId.includes("gate") ||
    itemId.includes("finish")
  );
}

function shouldRenderCollisionGeometry(
  shape: CollisionGeometryShape,
  path: PreparedPath,
  relevance: CollisionRelevanceContext,
  mode: CollisionGeometryRenderMode,
): boolean {
  const maxSpan = collisionShapeMaxSpan(shape);
  const sourceKind = shape.source_kind.toLowerCase();
  const isConfirmedHit = relevance.hitLabels.has(shape.label.trim());
  const hasRenderableConfidence =
    shape.confidence >= COLLIDER_EXPLICIT_MIN_RENDER_CONFIDENCE;

  if (
    !isExplicitColliderShape(shape.shape) ||
    isHelperCollisionLabel(shape.label) ||
    (sourceKind === "environment" &&
      isOutOfScopeEnvironmentCollisionLabel(shape.label, shape.source_id))
  ) {
    return false;
  }

  if (mode === "overview") {
    return hasRenderableConfidence || isConfirmedHit;
  }

  if (sourceKind === "environment") {
    if (
      !isConfirmedHit &&
      (maxSpan > COLLIDER_ENVIRONMENT_MAX_RENDER_SPAN_METERS ||
        !hasRenderableConfidence)
    ) {
      return false;
    }
    return (
      isConfirmedHit ||
      isCollisionShapeNearPath(shape, path, relevance)
    );
  }

  return hasRenderableConfidence;
}

function shouldDisplayCollisionEvent(event: CollisionEvent): boolean {
  if (event.hit_shape && !isExplicitColliderShape(event.hit_shape)) {
    return false;
  }
  if (
    event.hit_source?.toLowerCase().startsWith("environment:") &&
    isOutOfScopeEnvironmentCollisionLabel(
      event.hit_label ?? "",
      event.hit_source,
    )
  ) {
    return false;
  }
  return !isHelperCollisionLabel(event.hit_label ?? "");
}

function buildCollisionRelevanceContext(
  path: PreparedPath,
): CollisionRelevanceContext {
  const anchors: THREE.Vector3[] = [];
  const step = Math.max(
    1,
    Math.ceil(path.points.length / COLLIDER_PATH_RELEVANCE_MAX_ANCHORS),
  );

  for (let idx = 0; idx < path.points.length; idx += step) {
    anchors.push(path.points[idx]);
  }
  const last = path.points[path.points.length - 1];
  if (last && anchors[anchors.length - 1] !== last) {
    anchors.push(last);
  }
  for (const event of path.collisionEvents) {
    anchors.push(event.point);
  }

  return {
    anchors,
    hitLabels: new Set(
      path.collisionEvents
        .map((event) => event.hit_label?.trim())
        .filter((label): label is string => Boolean(label)),
    ),
  };
}

function isExplicitColliderShape(shape: string): boolean {
  return shape.endsWith("_collider") || shape === "procedural_ribbon_segment";
}

function isHelperCollisionLabel(label: string): boolean {
  const value = normalizeCollisionKey(label);
  return COLLIDER_HELPER_LABEL_PARTS.some((part) => value.includes(part));
}

function isOutOfScopeEnvironmentCollisionLabel(
  label: string,
  source: string | undefined,
): boolean {
  const labelKey = normalizeCollisionKey(label);
  const sourceKey = normalizeCollisionKey(source ?? "");
  return COLLIDER_SCOPED_ENVIRONMENT_LABEL_PARTS.some(
    (scope) =>
      labelKey.includes(scope.label) && !sourceKey.includes(scope.source),
  );
}

function normalizeCollisionKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function isCollisionShapeNearPath(
  shape: CollisionGeometryShape,
  path: PreparedPath,
  relevance: CollisionRelevanceContext,
): boolean {
  if (relevance.anchors.length === 0) return false;

  const [x, y, z] = toScenePosition(shape.center, path);
  const verticalRadius = Math.abs(shape.half_extents[1]);
  if (
    y + verticalRadius < -COLLIDER_VERTICAL_RELEVANCE_MARGIN_METERS ||
    y - verticalRadius > path.height + COLLIDER_VERTICAL_RELEVANCE_MARGIN_METERS
  ) {
    return false;
  }

  const horizontalRadius = Math.hypot(
    shape.half_extents[0],
    shape.half_extents[2],
  );
  const maxDistance =
    horizontalRadius + COLLIDER_PATH_RELEVANCE_RADIUS_METERS;
  const maxDistanceSq = maxDistance * maxDistance;
  const maxVerticalDistance =
    verticalRadius + COLLIDER_VERTICAL_RELEVANCE_MARGIN_METERS;

  for (const anchor of relevance.anchors) {
    if (Math.abs(anchor.y - y) > maxVerticalDistance) continue;
    const dx = anchor.x - x;
    const dz = anchor.z - z;
    if (dx * dx + dz * dz <= maxDistanceSq) return true;
  }

  return false;
}

function collisionShapeMaxSpan(shape: CollisionGeometryShape): number {
  return Math.max(
    Math.abs(shape.half_extents[0]) * 2,
    Math.abs(shape.half_extents[1]) * 2,
    Math.abs(shape.half_extents[2]) * 2,
  );
}

function collisionColor(severity: number): string {
  if (severity >= 8) return COLLISION_HIGH;
  if (severity >= 4) return COLLISION_MID;
  return COLLISION_LOW;
}

function collisionHitLabel(event: CollisionEvent): string | null {
  const label = event.hit_label?.trim() || event.hit_source?.trim();
  if (!label) return null;
  return formatHitLabel(label);
}

function collisionHitDetail(event: CollisionEvent): string | null {
  const parts = [
    formatHitShape(event.hit_shape),
    formatHitDistance(event.hit_distance),
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : null;
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
    case "mesh_collider":
      return "mesh collider";
    case "box_collider":
      return "box collider";
    case "sphere_collider":
      return "sphere collider";
    case "capsule_collider":
      return "capsule collider";
    case "procedural_ribbon_segment":
      return "ribbon segment";
    default:
      return humanizeIdentifier(shape);
  }
}

function formatHitDistance(distance: number | null | undefined): string | null {
  if (distance == null || !Number.isFinite(distance)) return null;
  if (distance < 0.005) return "touching";
  return `${distance.toFixed(distance < 0.1 ? 2 : 1)} m`;
}

function propLabel(prop: ReplayCourseProp): string {
  return `${shortPropName(prop.item_id)} #${prop.instance_id}`;
}

function shortPropName(itemId: string): string {
  return humanizeIdentifier(itemId);
}

function humanizeIdentifier(value: string): string {
  return value
    .replace(/01$/, "")
    .replace(/White$/, "White")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/(\d+)([A-Z])/g, "$1 $2")
    .trim();
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function CenteredOverlay({
  children,
  onNavigate,
  captureId,
  sessionId,
}: {
  children: React.ReactNode;
  onNavigate: (view: View) => void;
  captureId: string | null;
  sessionId: string;
}) {
  return (
    <div className="relative flex h-full items-center justify-center p-8">
      <div className="absolute right-5 top-5">
        <Button
          variant="ghost"
          size="sm"
          onClick={() =>
            captureId
              ? onNavigate({ kind: "race-detail", captureId, sessionId })
              : onNavigate({ kind: "races" })
          }
        >
          {captureId ? "Back to race" : "Back to races"}
        </Button>
      </div>
      {children}
    </div>
  );
}

/** Count of ascending entries with value <= t (binary search). */
function countUpTo(times: number[], t: number): number {
  let lo = 0;
  let hi = times.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] <= t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function preparePath(
  windowed: SamplePoint[],
  collisionEvents: CollisionEvent[],
  lapWindows: LapWindow[],
  captureId: string,
  title: string,
): PreparedPath | null {
  const raw = windowed.filter(
    (s): s is SamplePoint & { pos: [number, number, number] } =>
      s.pos != null,
  );
  if (raw.length < 2) return null;

  let minX = Infinity,
    minY = Infinity,
    minZ = Infinity,
    maxX = -Infinity,
    maxY = -Infinity,
    maxZ = -Infinity;
  for (const s of raw) {
    const [x, y, z] = s.pos;
    if (x < minX) minX = x;
    if (x > maxX) maxX = x;
    if (y < minY) minY = y;
    if (y > maxY) maxY = y;
    if (z < minZ) minZ = z;
    if (z > maxZ) maxZ = z;
  }

  // Center X/Z over the origin and drop the lowest point onto the grid (y=0),
  // so the path sits above the floor regardless of world origin.
  // Liftoff/Unity is left-handed (Y-up); three.js is right-handed. Copying
  // coordinates straight across mirrors the path (left turns read as right), so
  // negate Z — the standard Unity→three.js conversion — to fix the chirality.
  const cx = (minX + maxX) / 2;
  const cz = (minZ + maxZ) / 2;
  const t0 = raw[0].capture_time_seconds;

  const points: THREE.Vector3[] = new Array(raw.length);
  const times: number[] = new Array(raw.length);
  for (let i = 0; i < raw.length; i++) {
    const [x, y, z] = raw[i].pos;
    const px = x - cx;
    const py = y - minY;
    const pz = -(z - cz);
    points[i] = new THREE.Vector3(px, py, pz);
    times[i] = raw[i].capture_time_seconds - t0;
  }
  const pathSegments = buildPathSegments(raw, points, times, lapWindows);

  const height = maxY - minY;
  const dx = maxX - minX;
  const dz = maxZ - minZ;
  const radius = 0.5 * Math.sqrt(dx * dx + height * height + dz * dz) || 1;
  const duration = times[times.length - 1];
  const preparedCollisions = collisionEvents
    .map((event) => {
      const pos =
        event.pos ?? nearestSamplePosition(raw, event.capture_time_seconds);
      if (!pos) return null;
      const [x, y, z] = pos;
      return {
        ...event,
        point: new THREE.Vector3(x - cx, y - minY, -(z - cz)),
        replayTime: event.capture_time_seconds - t0,
      };
    })
    .filter((event): event is PreparedCollisionEvent => event != null);

  return {
    points,
    pathSegments,
    collisionEvents: preparedCollisions,
    times,
    duration,
    radius,
    height,
    centerX: cx,
    minY,
    centerZ: cz,
    captureId,
    sessionTitle: title,
  };
}

function buildLapWindows(laps: RaceLapRow[], offset: number): LapWindow[] {
  return laps
    .map((lap) => ({
      lapIndex: lap.lap_index,
      startSeconds: lap.start_seconds - offset,
      endSeconds: lap.end_seconds - offset,
    }))
    .filter(
      (lap) =>
        Number.isFinite(lap.startSeconds) &&
        Number.isFinite(lap.endSeconds) &&
        lap.endSeconds > lap.startSeconds,
    )
    .sort((a, b) => a.startSeconds - b.startSeconds);
}

function buildPathSegments(
  samples: (SamplePoint & { pos: [number, number, number] })[],
  points: THREE.Vector3[],
  replayTimes: number[],
  lapWindows: LapWindow[],
): PreparedPathSegment[] {
  if (lapWindows.length === 0) {
    return [
      buildPathSegment(
        "path",
        "Path",
        null,
        SIGNAL,
        points,
        replayTimes,
      ),
    ];
  }

  const segments: PreparedPathSegment[] = [];
  const firstTime = samples[0].capture_time_seconds;
  const lastTime = samples[samples.length - 1].capture_time_seconds;
  let cursor = firstTime;

  for (const lap of lapWindows) {
    const start = clamp(lap.startSeconds, firstTime, lastTime);
    const end = clamp(lap.endSeconds, firstTime, lastTime);
    if (start > cursor + 0.05) {
      pushPathSegment(
        segments,
        samples,
        points,
        firstTime,
        cursor,
        start,
        `unclassified-before-lap-${lap.lapIndex}`,
        "Between laps",
        null,
        UNCLASSIFIED_PATH,
      );
    }
    if (end > start + 0.05) {
      pushPathSegment(
        segments,
        samples,
        points,
        firstTime,
        start,
        end,
        `lap-${lap.lapIndex}`,
        `Lap ${lap.lapIndex}`,
        lap.lapIndex,
        lapColor(lap.lapIndex),
      );
    }
    cursor = Math.max(cursor, end);
  }

  if (lastTime > cursor + 0.05) {
    pushPathSegment(
      segments,
      samples,
      points,
      firstTime,
      cursor,
      lastTime,
      "unclassified-end",
      "Between laps",
      null,
      UNCLASSIFIED_PATH,
    );
  }

  return segments.length > 0
    ? segments
    : [
        buildPathSegment(
          "path",
          "Path",
          null,
          SIGNAL,
          points,
          replayTimes,
        ),
      ];
}

function pushPathSegment(
  segments: PreparedPathSegment[],
  samples: (SamplePoint & { pos: [number, number, number] })[],
  scenePoints: THREE.Vector3[],
  originTime: number,
  startTime: number,
  endTime: number,
  id: string,
  label: string,
  lapIndex: number | null,
  color: string,
) {
  const points = [sampleScenePointAt(samples, scenePoints, startTime)];
  const times = [startTime - originTime];

  for (let index = 0; index < samples.length; index++) {
    const sampleTime = samples[index].capture_time_seconds;
    if (sampleTime <= startTime || sampleTime >= endTime) {
      continue;
    }
    points.push(scenePoints[index].clone());
    times.push(sampleTime - originTime);
  }

  points.push(sampleScenePointAt(samples, scenePoints, endTime));
  times.push(endTime - originTime);

  if (points.length >= 2) {
    segments.push(buildPathSegment(id, label, lapIndex, color, points, times));
  }
}

function buildPathSegment(
  id: string,
  label: string,
  lapIndex: number | null,
  color: string,
  points: THREE.Vector3[],
  times: number[],
): PreparedPathSegment {
  const positions = new Float32Array(points.length * 3);
  for (let index = 0; index < points.length; index++) {
    positions[index * 3] = points[index].x;
    positions[index * 3 + 1] = points[index].y;
    positions[index * 3 + 2] = points[index].z;
  }
  return { id, label, lapIndex, color, points, times, positions };
}

function sampleScenePointAt(
  samples: (SamplePoint & { pos: [number, number, number] })[],
  scenePoints: THREE.Vector3[],
  targetTime: number,
): THREE.Vector3 {
  if (targetTime <= samples[0].capture_time_seconds) {
    return scenePoints[0].clone();
  }
  const lastIndex = samples.length - 1;
  if (targetTime >= samples[lastIndex].capture_time_seconds) {
    return scenePoints[lastIndex].clone();
  }

  let lo = 0;
  let hi = lastIndex;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (samples[mid].capture_time_seconds < targetTime) lo = mid + 1;
    else hi = mid;
  }

  const next = Math.max(1, lo);
  const previous = next - 1;
  const startTime = samples[previous].capture_time_seconds;
  const endTime = samples[next].capture_time_seconds;
  const span = Math.max(endTime - startTime, 0.0001);
  const t = clamp((targetTime - startTime) / span, 0, 1);
  return scenePoints[previous].clone().lerp(scenePoints[next], t);
}

function lapColor(lapIndex: number): string {
  return LAP_COLORS[(Math.max(lapIndex, 1) - 1) % LAP_COLORS.length];
}

function buildLapLegend(
  path: PreparedPath,
): { lapIndex: number; color: string }[] {
  const seen = new Set<number>();
  const legend: { lapIndex: number; color: string }[] = [];
  for (const segment of path.pathSegments) {
    if (segment.lapIndex == null || seen.has(segment.lapIndex)) {
      continue;
    }
    seen.add(segment.lapIndex);
    legend.push({ lapIndex: segment.lapIndex, color: segment.color });
  }
  return legend.sort((a, b) => a.lapIndex - b.lapIndex);
}

function nearestSamplePosition(
  samples: (SamplePoint & { pos: [number, number, number] })[],
  time: number,
): [number, number, number] | null {
  let best: (SamplePoint & { pos: [number, number, number] }) | null = null;
  let bestDt = Infinity;
  for (const sample of samples) {
    const dt = Math.abs(sample.capture_time_seconds - time);
    if (dt < bestDt) {
      best = sample;
      bestDt = dt;
    }
  }
  return best?.pos ?? null;
}

function toScenePosition(
  [x, y, z]: [number, number, number],
  path: PreparedPath,
): [number, number, number] {
  return [x - path.centerX, y - path.minY, -(z - path.centerZ)];
}

function toSceneQuaternion(
  [x, y, z]: [number, number, number],
): THREE.Quaternion {
  // Liftoff applies Unity Euler degrees as Z, then X, then Y. Build that as the
  // same Y * X * Z quaternion product used by the Rust extractor, then mirror Z
  // to match the position conversion above.
  const qz = new THREE.Quaternion().setFromAxisAngle(
    new THREE.Vector3(0, 0, 1),
    THREE.MathUtils.degToRad(z),
  );
  const qx = new THREE.Quaternion().setFromAxisAngle(
    new THREE.Vector3(1, 0, 0),
    THREE.MathUtils.degToRad(x),
  );
  const qy = new THREE.Quaternion().setFromAxisAngle(
    new THREE.Vector3(0, 1, 0),
    THREE.MathUtils.degToRad(y),
  );
  const unityRotation = qy.multiply(qx).multiply(qz).normalize();
  return new THREE.Quaternion(
    -unityRotation.x,
    -unityRotation.y,
    unityRotation.z,
    unityRotation.w,
  );
}

function setSceneQuaternionFromUnity(
  out: THREE.Quaternion,
  rotation: [number, number, number, number] | undefined,
) {
  if (!rotation) {
    out.identity();
    return;
  }
  out.set(-rotation[0], -rotation[1], rotation[2], rotation[3]).normalize();
}

function setBoxCorner(
  out: THREE.Vector3,
  local: THREE.Vector3,
  center: THREE.Vector3,
  rotation: THREE.Quaternion,
  x: number,
  y: number,
  z: number,
) {
  out.copy(local.set(x, y, z).applyQuaternion(rotation).add(center));
}

function sessionTitle(
  session: RaceSessionRow | null,
  sessions: RaceSessionRow[],
): string {
  if (!session) return "Session";
  const idx = String(session.session_index + 1).padStart(2, "0");
  const label =
    session.race ?? session.track ?? session.level ?? `Session ${idx}`;
  return sessions.length > 1 ? `${idx} · ${label}` : label;
}

function formatError(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message?: unknown }).message ?? e);
  }
  return String(e);
}
