import { useCallback, useEffect, useState } from "react";
import { CircleAlert, Loader2, Play } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Page } from "@/components/Page";
import { PageHeader } from "@/components/PageHeader";
import { Panel } from "@/components/Panel";
import { Stat } from "@/components/Stat";
import { api } from "@/lib/api";
import { subscribe } from "@/lib/events";
import type {
  CaptureDetail,
  PipelineSummary,
  ProcessingProfileRow,
  ProcessingProgress,
} from "@/lib/types";
import type { View } from "@/App";

type Props = {
  captureId: string;
  onNavigate: (view: View) => void;
};

export function ProcessingView({ captureId, onNavigate }: Props) {
  const [capture, setCapture] = useState<CaptureDetail | null>(null);
  const [profiles, setProfiles] = useState<ProcessingProfileRow[]>([]);
  const [selectedProfile, setSelectedProfile] = useState<string>("");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ProcessingProgress | null>(null);
  const [summary, setSummary] = useState<PipelineSummary | null>(null);
  const [datasetId, setDatasetId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([api.getCapture(captureId), api.listProcessingProfiles()])
      .then(([cap, profs]) => {
        setCapture(cap);
        setProfiles(profs);
        const def = profs.find((p) => p.is_default) ?? profs[0];
        if (def) setSelectedProfile(def.id);
      })
      .catch((e) => setError(formatError(e)));
  }, [captureId]);

  useEffect(() => {
    const ps: Promise<() => void>[] = [];
    ps.push(subscribe("processing_progress", (p) => setProgress(p)));
    return () => {
      for (const p of ps) {
        p.then((u) => u()).catch(() => {});
      }
    };
  }, []);

  const run = useCallback(async () => {
    setRunning(true);
    setError(null);
    setProgress(null);
    setSummary(null);
    setDatasetId(null);
    try {
      const res = await api.processCapture(
        captureId,
        selectedProfile || undefined,
      );
      setSummary(res.summary);
      setDatasetId(res.dataset.id);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setRunning(false);
    }
  }, [captureId, selectedProfile]);

  if (!capture) {
    return (
      <Page>
        <div className="text-muted-foreground flex items-center gap-2 font-mono text-sm">
          <Loader2 className="size-4 animate-spin" />
          loading…
        </div>
      </Page>
    );
  }

  return (
    <Page
      header={
        <PageHeader
          eyebrow="04 · Pipeline"
          title="Process capture"
          subtitle={
            <span className="font-mono text-xs">{captureId}</span>
          }
          actions={
            <Button disabled={running} onClick={run}>
              {running ? (
                <>
                  <Loader2 className="size-4 animate-spin" />
                  Processing
                </>
              ) : (
                <>
                  <Play className="size-4" />
                  Run pipeline
                </>
              )}
            </Button>
          }
        />
      }
    >
      {error && (
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Processing error</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <Panel title="Input capture">
        <div className="grid grid-cols-2 gap-x-5 gap-y-4 sm:grid-cols-4">
          <Stat
            label="Packets"
            value={capture.capture.packet_count.toLocaleString()}
          />
          <Stat
            label="Duration"
            value={
              capture.capture.duration_seconds != null
                ? `${capture.capture.duration_seconds.toFixed(1)}s`
                : "—"
            }
          />
          <Stat label="Markers" value={String(capture.markers.length)} />
          <Stat label="Status" value={capture.capture.status} />
        </div>
      </Panel>

      <Panel title="Profile">
        <div className="flex flex-col gap-4">
          <p className="text-muted-foreground text-sm leading-relaxed">
            Processing is deterministic given the same capture, profile, and
            processor version — re-run any time analytics improve.
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <Select
              value={selectedProfile}
              onValueChange={setSelectedProfile}
              disabled={running}
            >
              <SelectTrigger className="w-64 font-mono">
                <SelectValue placeholder="Select a profile" />
              </SelectTrigger>
              <SelectContent>
                {profiles.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.name}
                    {p.is_default ? " · default" : ""}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {progress && (
            <p className="text-signal font-mono text-xs tabular-nums">
              › processed {progress.processed_packets.toLocaleString()} packets…
            </p>
          )}
        </div>
      </Panel>

      {summary && (
        <Panel
          title="Result"
          action={
            datasetId && (
              <Button
                size="sm"
                onClick={() => onNavigate({ kind: "dataset", datasetId })}
              >
                Open dataset
              </Button>
            )
          }
        >
          <div className="flex flex-col gap-5">
            <div className="grid grid-cols-2 gap-x-5 gap-y-4 sm:grid-cols-3">
              <Stat
                label="Samples"
                value={summary.sample_count.toLocaleString()}
                accent
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
              <Stat label="Endpoint" value={summary.schema_endpoint} />
              <Stat label="Fields" value={String(summary.schema_field_count)} />
            </div>
            {Object.keys(summary.warnings_by_kind).length > 0 && (
              <ul className="border-border/60 bg-background/40 divide-border/50 divide-y rounded-md border font-mono text-xs">
                {Object.entries(summary.warnings_by_kind).map(([k, n]) => (
                  <li
                    key={k}
                    className="flex items-center justify-between px-3 py-1.5"
                  >
                    <span className="text-muted-foreground">{k}</span>
                    <span className="text-warn font-medium tabular-nums">
                      {n}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <div className="border-border/60 flex border-t pt-4">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onNavigate({ kind: "sessions" })}
              >
                Back to sessions
              </Button>
            </div>
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
