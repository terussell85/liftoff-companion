import type { SamplePoint } from "@/lib/types";

type Contact = { t: number; severity: number };
type LapMark = { lap: number; start: number };

/**
 * Speed trace over a race window with an optional throttle band, lap-boundary
 * dividers + labels, and contact markers. All times are dataset-relative
 * seconds (the frame of `SamplePoint.capture_time_seconds`).
 */
export function FlightRecorderChart({
  samples,
  startSeconds,
  endSeconds,
  laps = [],
  contacts = [],
}: {
  samples: SamplePoint[];
  startSeconds: number;
  endSeconds: number;
  laps?: LapMark[];
  contacts?: Contact[];
}) {
  const points = samples.filter(
    (s) =>
      s.capture_time_seconds >= startSeconds &&
      s.capture_time_seconds <= endSeconds,
  );
  if (points.length < 2) {
    return (
      <p className="text-muted-foreground/60 font-mono text-xs">
        not enough samples in this window
      </p>
    );
  }

  const hasThrottle = points.some((p) => p.throttle != null);
  const width = 1000;
  const height = 200;
  const pad = { top: 16, right: 14, bottom: 22, left: 42 };
  const x0 = points[0].capture_time_seconds;
  const x1 = points[points.length - 1].capture_time_seconds;
  const span = Math.max(1e-6, x1 - x0);
  const maxSpeed = Math.max(...points.map((p) => p.speed), 1);
  // Headroom so the peak doesn't hug the ceiling (the mockup did this with a
  // fixed 33 m/s scale above a ~31 m/s peak).
  const yMax = maxSpeed * 1.08;

  const sx = (t: number) =>
    pad.left + ((t - x0) / span) * (width - pad.left - pad.right);
  const sy = (s: number) =>
    height - pad.bottom - (s / yMax) * (height - pad.top - pad.bottom);
  // Throttle lives in the lower half of the plot so it never fights the trace.
  const ty = (thr: number) =>
    height - pad.bottom - thr * (height - pad.top - pad.bottom) * 0.5;

  const speedLine = points
    .map(
      (p, i) =>
        `${i === 0 ? "M" : "L"}${sx(p.capture_time_seconds).toFixed(1)},${sy(p.speed).toFixed(1)}`,
    )
    .join(" ");

  // Throttle arrives as the raw controller axis, which may be 0..1, 0..100, or a
  // signed range (-1..1 / -100..100). Normalize to 0 (idle) … 1 (full) so it
  // maps onto the band instead of running off-scale. Rendered as a soft fill
  // only — a stroked trace draws resting throttle as a hard flat line.
  const baseY = height - pad.bottom;
  const normThrottle = makeThrottleNorm(points);
  const throttlePts = hasThrottle
    ? points
        .map(
          (p) =>
            `${sx(p.capture_time_seconds).toFixed(1)},${ty(normThrottle(p.throttle)).toFixed(1)}`,
        )
        .join(" L")
    : null;
  // Fill (closed, no stroke) + the trace as an open stroked polyline — the open
  // line has no baseline edge, so there's no flat line across the chart.
  const throttleFill = throttlePts
    ? `M${sx(x0).toFixed(1)},${baseY.toFixed(1)} L${throttlePts} L${sx(x1).toFixed(1)},${baseY.toFixed(1)} Z`
    : null;
  const throttleLine = throttlePts ? `M${throttlePts}` : null;

  const nearestSpeed = (t: number) =>
    points.reduce((a, b) =>
      Math.abs(b.capture_time_seconds - t) < Math.abs(a.capture_time_seconds - t)
        ? b
        : a,
    ).speed;

  // Lap labels/dividers: clamp into the window; the first lap sits at the left.
  const lapMarks = laps
    .filter((l) => l.start <= x1)
    .map((l) => ({
      lap: l.lap,
      x: Math.max(sx(Math.max(l.start, x0)), pad.left + 1),
      divider: l.start > x0 + 1e-3 && l.start < x1,
    }));

  return (
    <div className="border-border/60 bg-background/40 overflow-hidden rounded-md border">
      <div className="border-border/70 text-muted-foreground flex items-center justify-between border-b px-3 py-1.5 font-mono text-[10.5px]">
        <div className="flex flex-wrap gap-x-3.5 gap-y-1">
          <span className="flex items-center gap-1.5">
            <i className="bg-signal inline-block h-[2.5px] w-3.5 rounded" />
            speed
          </span>
          {hasThrottle && (
            <span className="flex items-center gap-1.5">
              <i
                className="inline-block size-2 rounded-[2px]"
                style={{ background: "var(--chart-2)", opacity: 0.6 }}
              />
              throttle
            </span>
          )}
          {contacts.length > 0 && (
            <span className="flex items-center gap-1.5">
              <i className="bg-warn inline-block size-2 rounded-full" />
              contact
            </span>
          )}
        </div>
        <span className="tracking-[0.16em] uppercase">
          m/s{contacts.length > 0 ? ` · ${contacts.length} contacts` : ""}
        </span>
      </div>

      <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="h-52 w-full">
        <line x1={pad.left} x2={width - pad.right} y1={sy(0)} y2={sy(0)} stroke="var(--border)" />

        {throttleFill && (
          <path d={throttleFill} fill="var(--chart-2)" fillOpacity={0.12} stroke="none" />
        )}
        {throttleLine && (
          <path d={throttleLine} fill="none" stroke="var(--chart-2)" strokeOpacity={0.55} strokeWidth={1} strokeLinejoin="round" />
        )}

        {/* lap dividers + labels */}
        {lapMarks.map((m, i) => (
          <g key={i}>
            {m.divider && (
              <line x1={m.x} x2={m.x} y1={pad.top} y2={height - pad.bottom} stroke="var(--border)" strokeDasharray="3 4" />
            )}
            <text x={m.x + 5} y={pad.top + 9} className="fill-muted-foreground font-mono text-[9px] tracking-[0.12em]">
              LAP {m.lap}
            </text>
          </g>
        ))}

        <path d={speedLine} fill="none" stroke="var(--signal)" strokeWidth={1.6} strokeLinejoin="round" />

        {/* contact markers */}
        {contacts.map((c, i) => {
          const x = sx(c.t);
          const y = sy(nearestSpeed(c.t));
          const hot = c.severity >= 8;
          const col = hot ? "var(--destructive)" : c.severity >= 4 ? "var(--warn)" : "var(--chart-3)";
          const r = c.severity >= 4 ? 5 : 4;
          return (
            <g key={i}>
              <line x1={x} x2={x} y1={pad.top} y2={height - pad.bottom} stroke={col} strokeOpacity={0.35} />
              <circle cx={x} cy={y} r={r} fill={col} />
              <circle cx={x} cy={y} r={r + 4} fill="none" stroke={col} strokeOpacity={0.4} />
            </g>
          );
        })}

        <text x={pad.left} y={pad.top + 2} className="fill-muted-foreground font-mono text-[10px]">
          {Math.round(yMax)} m/s
        </text>
        <text x={width - pad.right} y={height - 6} textAnchor="end" className="fill-muted-foreground font-mono text-[10px]">
          {span.toFixed(1)}s
        </text>
      </svg>
    </div>
  );
}

/**
 * Build a throttle normalizer to 0 (idle) … 1 (full). The controller axis can
 * be 0..1, 0..100, or signed (-1..1 / -100..100); detect which from the data:
 * magnitude ≤ 1.5 ⇒ unit scale, else hundreds; negatives ⇒ signed range.
 */
function makeThrottleNorm(
  points: SamplePoint[],
): (thr: number | null | undefined) => number {
  const vals = points
    .map((p) => p.throttle)
    .filter((v): v is number => v != null && Number.isFinite(v));
  if (vals.length === 0) return () => 0;
  const mn = Math.min(...vals);
  const mx = Math.max(...vals);
  const unit = Math.max(Math.abs(mn), Math.abs(mx)) <= 1.5 ? 1 : 100;
  const lo = mn < -0.001 ? -unit : 0;
  const span = unit - lo || 1;
  return (thr) =>
    thr == null || !Number.isFinite(thr)
      ? 0
      : Math.max(0, Math.min(1, (thr - lo) / span));
}
