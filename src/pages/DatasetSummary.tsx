import { useEffect, useState } from "react";
import { CircleAlert, Loader2 } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Page } from "@/components/Page";
import { PageHeader } from "@/components/PageHeader";
import { Panel } from "@/components/Panel";
import { Stat } from "@/components/Stat";
import { api } from "@/lib/api";
import type { DatasetDetail } from "@/lib/types";
import type { View } from "@/App";

type Props = {
  datasetId: string;
  onNavigate: (view: View) => void;
};

export function DatasetSummaryView({ datasetId, onNavigate }: Props) {
  const [detail, setDetail] = useState<DatasetDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getDatasetDetail(datasetId)
      .then(setDetail)
      .catch((e) => setError(formatError(e)));
  }, [datasetId]);

  if (error) {
    return (
      <Page>
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Couldn&apos;t load dataset</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      </Page>
    );
  }
  if (!detail) {
    return (
      <Page>
        <div className="text-muted-foreground flex items-center gap-2 font-mono text-sm">
          <Loader2 className="size-4 animate-spin" />
          loading…
        </div>
      </Page>
    );
  }

  const { summary, samples, dataset } = detail;
  return (
    <Page
      header={
        <PageHeader
          eyebrow="05 · Dataset"
          title="Processed dataset"
          subtitle={<span className="font-mono text-xs">{dataset.id}</span>}
          actions={
            <Button
              variant="ghost"
              onClick={() => onNavigate({ kind: "sessions" })}
            >
              Back to sessions
            </Button>
          }
        />
      }
    >
      <Panel title="Summary">
        <div className="grid grid-cols-2 gap-x-5 gap-y-4 sm:grid-cols-3">
          <Stat
            label="Samples"
            value={summary.sample_count.toLocaleString()}
            accent
          />
          <Stat
            label="Packets"
            value={summary.packet_count.toLocaleString()}
          />
          <Stat
            label="Warnings"
            value={summary.warning_count.toLocaleString()}
          />
          <Stat
            label="Mean speed"
            value={`${summary.mean_speed.toFixed(2)} m/s`}
          />
          <Stat
            label="Max speed"
            value={`${summary.max_speed.toFixed(2)} m/s`}
          />
          <Stat
            label="Collisions"
            value={summary.collision_count.toLocaleString()}
            accent={summary.collision_count > 0}
          />
          <Stat
            label="Worst impact"
            value={`${summary.collision_max_severity}/10`}
            accent={summary.collision_max_severity > 0}
          />
          <Stat label="Profile" value={dataset.profile_id} />
        </div>
      </Panel>

      <Panel title="Speed · m/s">
        {samples.length === 0 ? (
          <p className="text-muted-foreground/60 font-mono text-xs">
            no samples persisted for this dataset
          </p>
        ) : (
          <SpeedChart points={samples} />
        )}
      </Panel>
    </Page>
  );
}

function SpeedChart({
  points,
}: {
  points: { capture_time_seconds: number; speed: number }[];
}) {
  if (points.length < 2)
    return (
      <p className="text-muted-foreground/60 font-mono text-xs">
        not enough samples to render
      </p>
    );
  const width = 800;
  const height = 220;
  const padding = { top: 14, right: 14, bottom: 26, left: 44 };
  const xs = points.map((p) => p.capture_time_seconds);
  const ys = points.map((p) => p.speed);
  const xMin = Math.min(...xs);
  const xMax = Math.max(...xs);
  const yMin = Math.min(0, ...ys);
  const yMax = Math.max(...ys, 1);
  const sx = (x: number) =>
    padding.left +
    ((x - xMin) / Math.max(1e-6, xMax - xMin)) *
      (width - padding.left - padding.right);
  const sy = (y: number) =>
    height -
    padding.bottom -
    ((y - yMin) / Math.max(1e-6, yMax - yMin)) *
      (height - padding.top - padding.bottom);
  const line = points
    .map(
      (p, i) =>
        `${i === 0 ? "M" : "L"}${sx(p.capture_time_seconds).toFixed(1)},${sy(p.speed).toFixed(1)}`,
    )
    .join(" ");
  const area =
    `M${sx(points[0].capture_time_seconds).toFixed(1)},${sy(0).toFixed(1)} ` +
    points
      .map(
        (p) =>
          `L${sx(p.capture_time_seconds).toFixed(1)},${sy(p.speed).toFixed(1)}`,
      )
      .join(" ") +
    ` L${sx(points[points.length - 1].capture_time_seconds).toFixed(1)},${sy(0).toFixed(1)} Z`;

  const yTicks = [0, yMax / 2, yMax];

  return (
    <div className="border-border/60 bg-background/40 overflow-hidden rounded-md border">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        className="h-60 w-full"
      >
        <defs>
          <linearGradient id="speed-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--signal)" stopOpacity="0.28" />
            <stop offset="100%" stopColor="var(--signal)" stopOpacity="0" />
          </linearGradient>
        </defs>

        {yTicks.map((t, i) => (
          <g key={i}>
            <line
              x1={padding.left}
              x2={width - padding.right}
              y1={sy(t)}
              y2={sy(t)}
              stroke="var(--border)"
              strokeWidth={1}
              strokeDasharray={i === 0 ? undefined : "2 4"}
            />
            <text
              x={padding.left - 8}
              y={sy(t) + 3}
              textAnchor="end"
              className="fill-muted-foreground font-mono text-[10px]"
            >
              {t.toFixed(0)}
            </text>
          </g>
        ))}

        <path d={area} fill="url(#speed-fill)" />
        <path
          d={line}
          fill="none"
          stroke="var(--signal)"
          strokeWidth={1.5}
          strokeLinejoin="round"
        />

        <text
          x={width - padding.right}
          y={height - 8}
          textAnchor="end"
          className="fill-muted-foreground font-mono text-[10px]"
        >
          {(xMax - xMin).toFixed(1)}s
        </text>
        <text
          x={padding.left}
          y={height - 8}
          className="fill-muted-foreground font-mono text-[10px]"
        >
          0s
        </text>
      </svg>
    </div>
  );
}

function formatError(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message?: unknown }).message ?? e);
  }
  return String(e);
}
