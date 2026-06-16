import type { SamplePoint } from "@/lib/types";

/**
 * SVG line chart of speed over a race session's time window. Samples are
 * filtered to [startSeconds, endSeconds] (both in capture-time seconds).
 */
export function SessionSpeedChart({
  samples,
  startSeconds,
  endSeconds,
}: {
  samples: SamplePoint[];
  startSeconds: number;
  endSeconds: number;
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
  const width = 800;
  const height = 180;
  const pad = { top: 12, right: 12, bottom: 22, left: 40 };
  const xs = points.map((p) => p.capture_time_seconds);
  const ys = points.map((p) => p.speed);
  const xMin = Math.min(...xs);
  const xMax = Math.max(...xs);
  const yMax = Math.max(...ys, 1);
  const sx = (x: number) =>
    pad.left +
    ((x - xMin) / Math.max(1e-6, xMax - xMin)) *
      (width - pad.left - pad.right);
  const sy = (y: number) =>
    height - pad.bottom - (y / yMax) * (height - pad.top - pad.bottom);
  const line = points
    .map(
      (p, i) =>
        `${i === 0 ? "M" : "L"}${sx(p.capture_time_seconds).toFixed(1)},${sy(p.speed).toFixed(1)}`,
    )
    .join(" ");

  return (
    <div className="border-border/60 bg-background/40 overflow-hidden rounded-md border">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        className="h-44 w-full"
      >
        <line
          x1={pad.left}
          x2={width - pad.right}
          y1={sy(0)}
          y2={sy(0)}
          stroke="var(--border)"
        />
        <path
          d={line}
          fill="none"
          stroke="var(--signal)"
          strokeWidth={1.5}
          strokeLinejoin="round"
        />
        <text
          x={pad.left}
          y={pad.top + 2}
          className="fill-muted-foreground font-mono text-[10px]"
        >
          {yMax.toFixed(1)} m/s
        </text>
        <text
          x={width - pad.right}
          y={height - 6}
          textAnchor="end"
          className="fill-muted-foreground font-mono text-[10px]"
        >
          {(xMax - xMin).toFixed(1)}s
        </text>
      </svg>
    </div>
  );
}
