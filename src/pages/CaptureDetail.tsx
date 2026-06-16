import { useCallback, useEffect, useState } from "react";
import { ChevronRight, CircleAlert, Loader2, Orbit } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { ConfirmButton } from "@/components/ConfirmButton";
import { Page } from "@/components/Page";
import { PageHeader } from "@/components/PageHeader";
import { Panel } from "@/components/Panel";
import { Stat } from "@/components/Stat";
import { SegBadge } from "@/components/SegBadge";
import { SessionSpeedChart } from "@/components/SessionSpeedChart";
import { cn } from "@/lib/utils";
import { api } from "@/lib/api";
import type { CaptureDetail, DatasetDetail, RaceSessionRow } from "@/lib/types";
import type { View } from "@/App";

type Props = {
  captureId: string;
  onNavigate: (view: View) => void;
};

export function CaptureDetailView({ captureId, onNavigate }: Props) {
  const [detail, setDetail] = useState<CaptureDetail | null>(null);
  const [dataset, setDataset] = useState<DatasetDetail | null>(null);
  const [selected, setSelected] = useState<RaceSessionRow | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const cap = await api.getCapture(captureId);
      setDetail(cap);
      setSelected(cap.race_sessions[0] ?? null);
      // Pull the most recent dataset (if processed) for per-session telemetry.
      const datasets = await api.listProcessedDatasets(captureId);
      if (datasets.length > 0) {
        setDataset(await api.getDatasetDetail(datasets[0].id));
      } else {
        setDataset(null);
      }
    } catch (e) {
      setError(formatError(e));
    }
  }, [captureId]);

  useEffect(() => {
    load();
  }, [load]);

  if (error) {
    return (
      <Page>
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Couldn&apos;t load capture</AlertTitle>
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

  const { capture, race_sessions } = detail;
  const offset = dataset ? dataset.summary.start_monotonic_ns / 1e9 : 0;

  return (
    <Page
      header={
        <PageHeader
          eyebrow="Flight Log · Capture"
          title="Race sessions"
          subtitle={<span className="font-mono text-xs">{capture.id}</span>}
          actions={
            <div className="flex gap-2">
              {capture.status === "completed" && (
                <Button
                  size="sm"
                  onClick={() =>
                    onNavigate({ kind: "process", captureId: capture.id })
                  }
                >
                  {dataset ? "Re-process" : "Process"}
                </Button>
              )}
              <ConfirmButton
                label="Delete"
                confirmLabel="Delete capture + files"
                disabled={capture.status === "recording"}
                onConfirm={async () => {
                  await api.deleteCapture(capture.id);
                  onNavigate({ kind: "sessions" });
                }}
              />
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onNavigate({ kind: "sessions" })}
              >
                Back
              </Button>
            </div>
          }
        />
      }
    >
      <div className="grid grid-cols-2 gap-x-5 gap-y-4 sm:grid-cols-4">
        <Stat label="Sessions" value={race_sessions.length.toString()} accent />
        <Stat
          label="Duration"
          value={
            capture.duration_seconds != null
              ? `${capture.duration_seconds.toFixed(1)}s`
              : "—"
          }
        />
        <Stat label="Packets" value={capture.packet_count.toLocaleString()} />
        <Stat label="Processed" value={dataset ? "yes" : "no"} />
      </div>

      {race_sessions.length === 0 ? (
        <Panel>
          <p className="text-muted-foreground text-sm">
            No race sessions detected. Either no Liftoff game log was found
            during capture, or the capture hasn&apos;t been processed yet.
          </p>
        </Panel>
      ) : (
        <Panel title="Sessions" bodyClassName="p-0">
          <ul className="divide-border/60 divide-y">
            {race_sessions.map((s) => (
              <li key={s.id}>
                <button
                  type="button"
                  onClick={() => setSelected(s)}
                  className={cn(
                    "flex w-full items-center gap-3 px-4 py-3 text-left transition-colors",
                    selected?.id === s.id
                      ? "bg-accent/60"
                      : "hover:bg-accent/30",
                  )}
                >
                  <span className="text-muted-foreground w-6 shrink-0 font-mono text-xs tabular-nums">
                    {String(s.session_index + 1).padStart(2, "0")}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">
                      {s.race ?? s.level ?? "Unknown run"}
                    </div>
                    <div className="text-muted-foreground truncate font-mono text-[11px]">
                      {[s.level, s.game_mode, s.drone]
                        .filter(Boolean)
                        .join(" · ") || "telemetry-only"}
                    </div>
                  </div>
                  <span className="text-muted-foreground shrink-0 font-mono text-xs tabular-nums">
                    {s.duration_seconds != null
                      ? `${s.duration_seconds.toFixed(1)}s`
                      : "—"}
                  </span>
                  <SegBadge method={s.segmentation_method} />
                  <ChevronRight className="text-muted-foreground size-4 shrink-0" />
                </button>
              </li>
            ))}
          </ul>
        </Panel>
      )}

      {selected && (
        <Panel
          title={`Session ${String(selected.session_index + 1).padStart(2, "0")}`}
          action={
            dataset ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  onNavigate({
                    kind: "flight",
                    datasetId: dataset.dataset.id,
                    sessionId: selected.id,
                  })
                }
              >
                <Orbit />
                Visualize
              </Button>
            ) : undefined
          }
        >
          <div className="flex flex-col gap-4">
            <div className="grid grid-cols-2 gap-x-5 gap-y-4 sm:grid-cols-4">
              <Stat label="Level" value={selected.level ?? "—"} />
              <Stat label="Race" value={selected.race ?? "—"} />
              <Stat label="Mode" value={selected.game_mode ?? "—"} />
              <Stat label="Drone" value={selected.drone ?? "—"} />
              <Stat
                label="Window"
                value={`${selected.start_seconds.toFixed(1)}–${
                  selected.end_seconds?.toFixed(1) ?? "?"
                }s`}
              />
              <Stat
                label="Method"
                value={selected.segmentation_method}
              />
              <Stat
                label="Confidence"
                value={
                  selected.confidence != null
                    ? `${(selected.confidence * 100).toFixed(0)}%`
                    : "—"
                }
              />
              <Stat
                label="Collisions"
                value={selected.collision_count.toLocaleString()}
                accent={selected.collision_count > 0}
              />
              <Stat
                label="Worst impact"
                value={`${selected.collision_max_severity}/10`}
                accent={selected.collision_max_severity > 0}
              />
              {selected.race_guid && (
                <Stat label="Race GUID" value={selected.race_guid.slice(0, 8)} />
              )}
            </div>

            {dataset ? (
              <SessionSpeedChart
                samples={dataset.samples}
                startSeconds={selected.start_seconds - offset}
                endSeconds={(selected.end_seconds ?? 0) - offset}
              />
            ) : (
              <p className="text-muted-foreground/60 font-mono text-xs">
                process this capture to see the telemetry for this session
              </p>
            )}
          </div>
        </Panel>
      )}
    </Page>
  );
}

function formatError(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message?: unknown }).message ?? e);
  }
  return String(e);
}
