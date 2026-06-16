import { useCallback, useEffect, useState } from "react";
import { CircleAlert, Inbox, Loader2 } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmButton } from "@/components/ConfirmButton";
import { Page } from "@/components/Page";
import { PageHeader } from "@/components/PageHeader";
import { Panel } from "@/components/Panel";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { api } from "@/lib/api";
import type { CaptureRow, ProcessedDatasetRow } from "@/lib/types";
import type { View } from "@/App";

type Props = {
  onNavigate: (view: View) => void;
};

export function SessionsView({ onNavigate }: Props) {
  const [captures, setCaptures] = useState<CaptureRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [datasetsByCapture, setDatasetsByCapture] = useState<
    Record<string, ProcessedDatasetRow[]>
  >({});

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await api.listCaptures();
      setCaptures(rows);
      const map: Record<string, ProcessedDatasetRow[]> = {};
      for (const c of rows) {
        try {
          map[c.id] = await api.listProcessedDatasets(c.id);
        } catch {
          map[c.id] = [];
        }
      }
      setDatasetsByCapture(map);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const totalPackets = captures.reduce((a, c) => a + c.packet_count, 0);

  return (
    <Page
      header={
        <PageHeader
          eyebrow="03 · Flight Log"
          title="Sessions"
          subtitle="Every recorded session. Raw captures are immutable and hash-verified."
          actions={
            captures.length > 0 ? (
              <span className="text-muted-foreground font-mono text-[11px] tracking-wide">
                {captures.length} sessions · {totalPackets.toLocaleString()} pkts
              </span>
            ) : undefined
          }
        />
      }
    >
      {error && (
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Couldn&apos;t load captures</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {loading && (
        <div className="text-muted-foreground flex items-center gap-2 font-mono text-sm">
          <Loader2 className="size-4 animate-spin" />
          loading…
        </div>
      )}

      {!loading && captures.length === 0 && (
        <Panel>
          <div className="flex flex-col items-center gap-3 py-12 text-center">
            <div className="border-border bg-muted/40 flex size-12 items-center justify-center rounded-full border">
              <Inbox className="text-muted-foreground size-5" />
            </div>
            <p className="font-medium">No captures yet</p>
            <p className="text-muted-foreground text-sm">
              Armed capture starts automatically on your first flight.
            </p>
          </div>
        </Panel>
      )}

      {!loading && captures.length > 0 && (
        <Panel bodyClassName="p-0">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="pl-4 font-mono text-[10px] tracking-wider uppercase">
                  Date
                </TableHead>
                <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                  Capture
                </TableHead>
                <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                  Status
                </TableHead>
                <TableHead className="text-right font-mono text-[10px] tracking-wider uppercase">
                  Duration
                </TableHead>
                <TableHead className="text-right font-mono text-[10px] tracking-wider uppercase">
                  Packets
                </TableHead>
                <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                  Processed
                </TableHead>
                <TableHead className="pr-4 text-right" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {captures.map((c) => {
                const datasets = datasetsByCapture[c.id] ?? [];
                return (
                  <TableRow key={c.id}>
                    <TableCell className="text-muted-foreground pl-4 font-mono text-xs">
                      {new Date(c.created_at).toLocaleString()}
                    </TableCell>
                    <TableCell className="font-mono text-xs">{c.id}</TableCell>
                    <TableCell>
                      <Badge
                        variant={
                          c.status === "completed"
                            ? "secondary"
                            : c.status === "recording"
                              ? "default"
                              : "outline"
                        }
                        className="font-mono text-[10px] tracking-wide uppercase"
                      >
                        {c.status}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs tabular-nums">
                      {c.duration_seconds != null
                        ? `${c.duration_seconds.toFixed(1)}s`
                        : "—"}
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs tabular-nums">
                      {c.packet_count.toLocaleString()}
                    </TableCell>
                    <TableCell>
                      {datasets.length > 0 ? (
                        <Badge
                          variant="success"
                          className="font-mono text-[10px]"
                        >
                          {datasets.length}×
                        </Badge>
                      ) : (
                        <span className="text-muted-foreground/50 font-mono text-xs">
                          —
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="pr-4 text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={c.status !== "completed"}
                          onClick={() =>
                            onNavigate({
                              kind: "capture-detail",
                              captureId: c.id,
                            })
                          }
                        >
                          Open
                        </Button>
                        <ConfirmButton
                          iconOnly
                          label="Delete capture"
                          disabled={c.status === "recording"}
                          onConfirm={async () => {
                            await api.deleteCapture(c.id);
                            await refresh();
                          }}
                        />
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
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
