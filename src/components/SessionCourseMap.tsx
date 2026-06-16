import { useMemo, useState } from "react";
import { cn } from "@/lib/utils";
import type { CollisionEvent, ReplayCheckpoint } from "@/lib/types";

/** One lap's actual flown path (world X/Z + speed per sample). */
export type LapPath = {
  lapIndex: number;
  points: { x: number; z: number; speed: number }[];
};

type Metric = "consistency" | "velocity" | "variance";

const METRICS: { key: Metric; label: string; left: string; right: string }[] = [
  { key: "consistency", label: "Consistency", left: "consistent", right: "varies" },
  { key: "velocity", label: "Velocity", left: "faster", right: "slower" },
  { key: "variance", label: "vs Ideal", left: "on line", right: "off line" },
];

/**
 * Top-down session minimap. The bold line is the *predicted* path through the
 * gates (course guide path, or gates joined in order); it is coloured by a
 * selectable metric — cross-lap consistency, velocity, or deviation from the
 * ideal line. The actual per-lap paths are drawn underneath as subtle ghosts,
 * and contacts are marked where they happened. Static — no zoom/pan.
 */
export function SessionCourseMap({
  laps,
  guidePath,
  checkpoints,
  collisions,
  className,
}: {
  laps: LapPath[];
  guidePath?: { x: number; z: number }[];
  checkpoints?: ReplayCheckpoint[];
  collisions: CollisionEvent[];
  className?: string;
}) {
  const [metric, setMetric] = useState<Metric>("consistency");
  const model = useMemo(
    () => buildMap(laps, guidePath ?? [], checkpoints ?? [], collisions),
    [laps, guidePath, checkpoints, collisions],
  );

  if (!model) {
    return (
      <div
        className={cn(
          "text-muted-foreground/60 flex items-center justify-center p-6 text-center font-mono text-xs",
          className,
        )}
      >
        no position data for this session — re-process the capture to draw the
        course
      </div>
    );
  }

  const active = model.available[metric] ? metric : "velocity";
  const t = model.tByMetric[active];
  const meta = METRICS.find((m) => m.key === active)!;

  return (
    <div className={cn("relative flex min-h-0 flex-col", className)}>
      {/* metric selector */}
      <div className="border-border/70 bg-card/80 absolute top-2 right-2 z-10 flex rounded-md border p-0.5 backdrop-blur">
        {METRICS.map((m) => {
          const enabled = model.available[m.key];
          return (
            <button
              key={m.key}
              type="button"
              disabled={!enabled}
              onClick={() => setMetric(m.key)}
              className={cn(
                "rounded px-2 py-0.5 font-mono text-[10px] tracking-[0.04em] transition-colors",
                active === m.key
                  ? "bg-signal/15 text-signal"
                  : enabled
                    ? "text-muted-foreground hover:text-foreground"
                    : "text-muted-foreground/30 cursor-not-allowed",
              )}
              title={enabled ? undefined : "needs more laps for this metric"}
            >
              {m.label}
            </button>
          );
        })}
      </div>

      <div className="min-h-0 flex-1">
        <svg
          viewBox={`0 0 ${model.width} ${model.height}`}
          preserveAspectRatio="xMidYMid meet"
          className="h-full w-full"
        >
          {/* subtle actual lap paths */}
          {model.ghosts.map((d, i) => (
            <polyline
              key={i}
              points={d}
              fill="none"
              stroke="oklch(0.6 0.03 256)"
              strokeOpacity={0.16}
              strokeWidth={model.unit * 1.1}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          ))}

          {/* predicted line, coloured by the active metric */}
          {model.refSeg.map((s, i) => (
            <line
              key={i}
              x1={s.x1}
              y1={s.y1}
              x2={s.x2}
              y2={s.y2}
              stroke={metricColor(t[i])}
              strokeWidth={model.unit * 3}
              strokeLinecap="round"
            />
          ))}

          {/* gates */}
          {model.gates.map((g) => (
            <g key={g.key}>
              <circle
                cx={g.x}
                cy={g.y}
                r={model.gateR}
                fill="var(--background)"
                stroke="oklch(0.78 0.03 256)"
                strokeWidth={model.gateR * 0.45}
              />
              <text
                x={g.x}
                y={g.y - model.gateR - 3}
                textAnchor="middle"
                className="fill-muted-foreground font-mono"
                style={{ fontSize: model.labelSize }}
              >
                {g.label}
              </text>
            </g>
          ))}

          {/* contacts */}
          {model.hits.map((h, i) => (
            <g key={i}>
              <circle cx={h.x} cy={h.y} r={h.r + model.unit * 5} fill={h.color} fillOpacity={0.16} />
              <circle cx={h.x} cy={h.y} r={h.r} fill="none" stroke={h.color} strokeWidth={model.unit * 1.6} />
              <path
                d={`M${h.x},${h.y - h.r * 0.55} l${h.r * 0.5},${h.r * 0.85} h${-h.r} Z`}
                fill={h.color}
              />
            </g>
          ))}

          {/* start / finish */}
          <circle cx={model.start.x} cy={model.start.y} r={model.unit * 5} fill="var(--signal)" />
          <text
            x={model.start.x + model.unit * 8}
            y={model.start.y + model.unit * 4}
            className="fill-signal font-mono font-semibold"
            style={{ fontSize: model.labelSize * 1.05 }}
          >
            S/F
          </text>
        </svg>
      </div>

      {/* legend — colour scale adapts to the active metric */}
      <div className="border-border text-muted-foreground flex flex-wrap items-center gap-x-3 gap-y-1 border-t px-3 py-2 font-mono text-[10px]">
        <span>{meta.left}</span>
        <span
          className="h-2 w-20 rounded-full"
          style={{
            background:
              "linear-gradient(90deg, oklch(0.8 0.17 145), oklch(0.8 0.17 85), oklch(0.8 0.17 25))",
          }}
        />
        <span>{meta.right}</span>
        <span className="ml-auto flex items-center gap-3">
          <span className="flex items-center gap-1.5">
            <i className="inline-block size-2 rounded-full border" style={{ borderColor: "oklch(0.78 0.03 256)" }} />
            gate
          </span>
          <span className="flex items-center gap-1.5">
            <i className="bg-warn inline-block size-2 rounded-full" />
            contact
          </span>
        </span>
      </div>
    </div>
  );
}

/** Green→red by t (0 = good/green, 1 = bad/red). */
function metricColor(t: number): string {
  const h = 145 - 120 * Math.max(0, Math.min(1, t));
  return `oklch(0.8 0.17 ${h.toFixed(0)})`;
}

type MapModel = {
  width: number;
  height: number;
  unit: number;
  gateR: number;
  labelSize: number;
  ghosts: string[];
  refSeg: { x1: number; y1: number; x2: number; y2: number }[];
  tByMetric: Record<Metric, number[]>;
  available: Record<Metric, boolean>;
  gates: { key: string; label: string; x: number; y: number }[];
  hits: { x: number; y: number; r: number; color: string }[];
  start: { x: number; y: number };
};

type Pt = { x: number; z: number; speed: number };

const SAMPLES = 120; // resample resolution along the course

function buildMap(
  laps: LapPath[],
  guidePath: { x: number; z: number }[],
  checkpoints: ReplayCheckpoint[],
  collisions: CollisionEvent[],
): MapModel | null {
  const lapPaths = laps.filter((l) => l.points.length >= 2);
  if (lapPaths.length === 0) return null;

  // Center X/Z over the run; negate Z to match the 3D view's chirality.
  let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
  for (const l of lapPaths) {
    for (const p of l.points) {
      minX = Math.min(minX, p.x); maxX = Math.max(maxX, p.x);
      minZ = Math.min(minZ, p.z); maxZ = Math.max(maxZ, p.z);
    }
  }
  const cx = (minX + maxX) / 2;
  const cz = (minZ + maxZ) / 2;
  const wx = (x: number) => x - cx;
  const wy = (z: number) => -(z - cz);

  // Resample every lap to a shared progress parameterisation (by arc length).
  const lapsR = lapPaths.map((l) =>
    resample(l.points.map((p) => ({ x: wx(p.x), z: wy(p.z), speed: p.speed })), SAMPLES),
  );

  // Reference (predicted) line: guide path, else gates in order, else mean lap.
  const hasRealRef = guidePath.length >= 2 || checkpoints.length >= 2;
  let refR: Pt[];
  if (guidePath.length >= 2) {
    refR = resample(guidePath.map((p) => ({ x: wx(p.x), z: wy(p.z), speed: 0 })), SAMPLES);
  } else if (checkpoints.length >= 2) {
    const ordered = [...checkpoints].sort((a, b) => a.sequence_index - b.sequence_index);
    const pts = ordered.map((c) => ({ x: wx(c.position[0]), z: wy(c.position[2]), speed: 0 }));
    pts.push(pts[0]); // close the loop
    refR = resample(pts, SAMPLES);
  } else {
    refR = Array.from({ length: SAMPLES }, (_, i) => meanPoint(lapsR.map((lap) => lap[i])));
  }

  // Per-progress metrics.
  const velocity: number[] = [];
  const consistency: number[] = [];
  const variance: number[] = [];
  for (let i = 0; i < SAMPLES; i++) {
    const col = lapsR.map((lap) => lap[i]);
    velocity[i] = mean(col.map((p) => p.speed));
    const centroid = meanPoint(col);
    consistency[i] = mean(col.map((p) => dist(p, centroid)));
    // Deviation from the ideal line: at each reference point, how close did each
    // lap's flown path actually come? Distance to the *nearest* point on the lap
    // polyline — not the same-index sample — so this is invariant to phase,
    // direction, and arc-length pacing differences between the reference line
    // and the laps (they don't share a start point or traversal speed).
    variance[i] = mean(lapsR.map((lap) => distToPath(refR[i], lap)));
  }
  // Normalise to t∈[0,1] where 0 = good (green): fast / consistent / on-line.
  const tVel = invert(normalize(velocity));
  const tCon = normalize(consistency);
  const tVar = normalize(variance);

  const available: Record<Metric, boolean> = {
    velocity: true,
    consistency: lapPaths.length >= 2,
    variance: hasRealRef ? lapPaths.length >= 1 : lapPaths.length >= 2,
  };

  // Fit + scale into a normalised box so marker sizes are scale-independent.
  const gatesW = checkpoints.map((c) => ({
    key: `${c.sequence_index}-${c.checkpoint_id}`,
    label: `G${c.sequence_index + 1}`,
    x: wx(c.position[0]),
    y: wy(c.position[2]),
  }));
  const hitsW = collisions
    .map((c) => (c.pos ? { x: wx(c.pos[0]), y: wy(c.pos[2]), sev: c.severity } : null))
    .filter((h): h is { x: number; y: number; sev: number } => h != null);

  const all = [
    ...lapsR.flat().map((p) => ({ x: p.x, y: p.z })),
    ...refR.map((p) => ({ x: p.x, y: p.z })),
    ...gatesW,
    ...hitsW,
  ];
  let bx0 = Infinity, bx1 = -Infinity, by0 = Infinity, by1 = -Infinity;
  for (const p of all) {
    bx0 = Math.min(bx0, p.x); bx1 = Math.max(bx1, p.x);
    by0 = Math.min(by0, p.y); by1 = Math.max(by1, p.y);
  }
  const spanX = Math.max(bx1 - bx0, 1e-6);
  const spanY = Math.max(by1 - by0, 1e-6);
  const TARGET = 1000;
  const scale = TARGET / spanX;
  const pad = TARGET * 0.06;
  const width = TARGET + pad * 2;
  const height = spanY * scale + pad * 2;
  const sx = (x: number) => (x - bx0) * scale + pad;
  const sy = (y: number) => (y - by0) * scale + pad;
  const unit = width / 420;

  const refScreen = refR.map((p) => ({ x: sx(p.x), y: sy(p.z) }));
  const refSeg = [];
  for (let i = 1; i < refScreen.length; i++) {
    refSeg.push({
      x1: refScreen[i - 1].x, y1: refScreen[i - 1].y,
      x2: refScreen[i].x, y2: refScreen[i].y,
    });
  }

  return {
    width,
    height,
    unit,
    gateR: unit * 3.2,
    labelSize: unit * 8,
    ghosts: lapsR.map((lap) =>
      lap.map((p) => `${sx(p.x).toFixed(1)},${sy(p.z).toFixed(1)}`).join(" "),
    ),
    refSeg,
    // t arrays are per-point; segment i uses the t at its start point.
    tByMetric: { consistency: tCon, velocity: tVel, variance: tVar },
    available,
    gates: gatesW.map((g) => ({ ...g, x: sx(g.x), y: sy(g.y) })),
    hits: hitsW.map((h) => ({
      x: sx(h.x),
      y: sy(h.y),
      r: unit * (4 + h.sev * 0.9),
      color: h.sev >= 8 ? "var(--destructive)" : h.sev >= 4 ? "var(--warn)" : "var(--chart-3)",
    })),
    start: refScreen[0] ?? { x: sx(lapsR[0][0].x), y: sy(lapsR[0][0].z) },
  };
}

/** Resample a polyline to `n` points evenly spaced by arc length. */
function resample(pts: Pt[], n: number): Pt[] {
  if (pts.length === 0) return [];
  if (pts.length === 1) return Array.from({ length: n }, () => pts[0]);
  const cum = [0];
  for (let i = 1; i < pts.length; i++) cum[i] = cum[i - 1] + dist(pts[i - 1], pts[i]);
  const total = cum[cum.length - 1];
  if (total <= 0) return Array.from({ length: n }, () => pts[0]);

  const out: Pt[] = [];
  let seg = 1;
  for (let k = 0; k < n; k++) {
    const target = (total * k) / (n - 1);
    while (seg < pts.length - 1 && cum[seg] < target) seg++;
    const a = pts[seg - 1];
    const b = pts[seg];
    const span = cum[seg] - cum[seg - 1] || 1e-6;
    const f = Math.max(0, Math.min(1, (target - cum[seg - 1]) / span));
    out.push({
      x: a.x + (b.x - a.x) * f,
      z: a.z + (b.z - a.z) * f,
      speed: a.speed + (b.speed - a.speed) * f,
    });
  }
  return out;
}

function dist(a: { x: number; z: number }, b: { x: number; z: number }): number {
  return Math.hypot(a.x - b.x, a.z - b.z);
}
/** Shortest distance from point `p` to a polyline (its nearest segment). */
function distToPath(p: { x: number; z: number }, path: Pt[]): number {
  if (path.length === 1) return dist(p, path[0]);
  let best = Infinity;
  for (let i = 1; i < path.length; i++) {
    const d = distToSegment(p, path[i - 1], path[i]);
    if (d < best) best = d;
  }
  return best;
}
/** Distance from point `p` to the segment a–b. */
function distToSegment(
  p: { x: number; z: number },
  a: { x: number; z: number },
  b: { x: number; z: number },
): number {
  const dx = b.x - a.x;
  const dz = b.z - a.z;
  const len2 = dx * dx + dz * dz;
  if (len2 <= 1e-12) return dist(p, a);
  let t = ((p.x - a.x) * dx + (p.z - a.z) * dz) / len2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p.x - (a.x + dx * t), p.z - (a.z + dz * t));
}
function mean(xs: number[]): number {
  return xs.length ? xs.reduce((s, x) => s + x, 0) / xs.length : 0;
}
function meanPoint(pts: Pt[]): Pt {
  return {
    x: mean(pts.map((p) => p.x)),
    z: mean(pts.map((p) => p.z)),
    speed: mean(pts.map((p) => p.speed)),
  };
}
function normalize(xs: number[]): number[] {
  const lo = Math.min(...xs);
  const hi = Math.max(...xs);
  const span = hi - lo || 1;
  return xs.map((x) => (x - lo) / span);
}
function invert(xs: number[]): number[] {
  return xs.map((x) => 1 - x);
}
