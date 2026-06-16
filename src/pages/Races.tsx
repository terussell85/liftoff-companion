import { useCallback, useEffect, useMemo, useState } from "react";
import { CircleAlert, Inbox, Loader2 } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Combobox } from "@/components/Combobox";
import { ConfirmButton } from "@/components/ConfirmButton";
import { Page } from "@/components/Page";
import { PageHeader } from "@/components/PageHeader";
import { Panel } from "@/components/Panel";
import { SegBadge } from "@/components/SegBadge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { api } from "@/lib/api";
import type { RaceSessionWithCapture } from "@/lib/types";
import type { View } from "@/App";

type Props = {
  onNavigate: (view: View) => void;
};

type DateFilter = "all" | "today" | "7d" | "30d";

const DATE_LABELS: Record<DateFilter, string> = {
  all: "All time",
  today: "Today",
  "7d": "Last 7 days",
  "30d": "Last 30 days",
};

// Display name for a run, falling back through the fields the game log fills.
function raceName(r: RaceSessionWithCapture): string {
  return r.race ?? r.level ?? r.track ?? "Unknown run";
}

export function RacesView({ onNavigate }: Props) {
  const [rows, setRows] = useState<RaceSessionWithCapture[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [raceFilter, setRaceFilter] = useState("");
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const captures = await api.listCaptures();
      const flat: RaceSessionWithCapture[] = [];
      for (const c of captures) {
        if (c.status !== "completed") continue;
        try {
          const sessions = await api.listRaceSessions(c.id);
          for (const s of sessions) {
            flat.push({
              ...s,
              capture_created_at: c.created_at,
              capture_status: c.status,
            });
          }
        } catch {
          /* skip captures whose sessions fail to load */
        }
      }
      flat.sort((a, b) => {
        const byDate =
          new Date(b.capture_created_at).getTime() -
          new Date(a.capture_created_at).getTime();
        return byDate !== 0 ? byDate : a.session_index - b.session_index;
      });
      setRows(flat);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Distinct race names seen, for the filter suggestions.
  const raceOptions = useMemo(() => {
    const s = new Set<string>();
    for (const r of rows) if (r.race) s.add(r.race);
    return [...s].sort();
  }, [rows]);

  const filtered = useMemo(() => {
    const q = raceFilter.trim().toLowerCase();
    let threshold = -Infinity;
    if (dateFilter === "today") {
      const d = new Date();
      d.setHours(0, 0, 0, 0);
      threshold = d.getTime();
    } else if (dateFilter === "7d") {
      threshold = Date.now() - 7 * 86_400_000;
    } else if (dateFilter === "30d") {
      threshold = Date.now() - 30 * 86_400_000;
    }
    return rows.filter((r) => {
      if (new Date(r.capture_created_at).getTime() < threshold) return false;
      if (q && !raceName(r).toLowerCase().includes(q)) return false;
      return true;
    });
  }, [rows, raceFilter, dateFilter]);

  const clearFilters = () => {
    setRaceFilter("");
    setDateFilter("all");
  };

  const openRace = useCallback(
    (r: RaceSessionWithCapture) => {
      onNavigate({
        kind: "race-detail",
        captureId: r.capture_id,
        sessionId: r.id,
      });
    },
    [onNavigate],
  );

  return (
    <Page
      header={
        <PageHeader
          eyebrow="01 · Race Log"
          title="Races"
          subtitle="Every race run across all captures. Filter by which track you flew and when."
          actions={
            rows.length > 0 ? (
              <span className="text-muted-foreground font-mono text-[11px] tracking-wide">
                {filtered.length} runs
              </span>
            ) : undefined
          }
        />
      }
    >
      {error && (
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Couldn&apos;t load races</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {loading && (
        <div className="text-muted-foreground flex items-center gap-2 font-mono text-sm">
          <Loader2 className="size-4 animate-spin" />
          loading…
        </div>
      )}

      {!loading && rows.length === 0 && (
        <Panel>
          <div className="flex flex-col items-center gap-3 py-12 text-center">
            <div className="border-border bg-muted/40 flex size-12 items-center justify-center rounded-full border">
              <Inbox className="text-muted-foreground size-5" />
            </div>
            <p className="font-medium">No races recorded yet</p>
            <p className="text-muted-foreground text-sm">
              Fly in Liftoff while ARMED is on; finished runs land here
              on their own.
            </p>
          </div>
        </Panel>
      )}

      {!loading && rows.length > 0 && (
        <>
          <div className="flex flex-wrap items-end gap-3">
            <div className="flex flex-col gap-1.5">
              <span className="text-muted-foreground font-mono text-[10px] tracking-wider uppercase">
                Race
              </span>
              <div className="w-56">
                <Combobox
                  id="race-filter"
                  value={raceFilter}
                  onChange={setRaceFilter}
                  options={raceOptions}
                  placeholder="All races"
                  className="font-mono"
                />
              </div>
            </div>
            <div className="flex flex-col gap-1.5">
              <span className="text-muted-foreground font-mono text-[10px] tracking-wider uppercase">
                Date
              </span>
              <Select
                value={dateFilter}
                onValueChange={(v) => setDateFilter(v as DateFilter)}
              >
                <SelectTrigger className="w-[150px] font-mono text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(Object.keys(DATE_LABELS) as DateFilter[]).map((k) => (
                    <SelectItem key={k} value={k} className="font-mono text-xs">
                      {DATE_LABELS[k]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {(raceFilter || dateFilter !== "all") && (
              <Button variant="ghost" size="sm" onClick={clearFilters}>
                Clear
              </Button>
            )}
          </div>

          {filtered.length === 0 ? (
            <Panel>
              <div className="flex flex-col items-center gap-3 py-10 text-center">
                <p className="font-medium">No races match these filters</p>
                <Button variant="outline" size="sm" onClick={clearFilters}>
                  Clear filters
                </Button>
              </div>
            </Panel>
          ) : (
            <Panel bodyClassName="p-0">
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead className="pl-4 font-mono text-[10px] tracking-wider uppercase">
                      Date
                    </TableHead>
                    <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                      Race
                    </TableHead>
                    <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                      Level
                    </TableHead>
                    <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                      Drone
                    </TableHead>
                    <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                      Mode
                    </TableHead>
                    <TableHead className="text-right font-mono text-[10px] tracking-wider uppercase">
                      Duration
                    </TableHead>
                    <TableHead className="font-mono text-[10px] tracking-wider uppercase">
                      Method
                    </TableHead>
                    <TableHead className="pr-4 text-right" />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filtered.map((r) => (
                    <TableRow
                      key={r.id}
                      tabIndex={0}
                      aria-label={`Open ${raceName(r)}`}
                      className="focus-visible:ring-signal/60 cursor-pointer focus-visible:ring-2 focus-visible:ring-inset focus-visible:outline-none"
                      onClick={() => openRace(r)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          openRace(r);
                        }
                      }}
                    >
                      <TableCell className="text-muted-foreground pl-4 font-mono text-xs">
                        {new Date(r.capture_created_at).toLocaleString()}
                      </TableCell>
                      <TableCell className="text-sm font-medium">
                        {raceName(r)}
                      </TableCell>
                      <TableCell className="text-muted-foreground font-mono text-xs">
                        {r.level ?? "—"}
                      </TableCell>
                      <TableCell className="text-muted-foreground font-mono text-xs">
                        {r.drone ?? "—"}
                      </TableCell>
                      <TableCell className="text-muted-foreground font-mono text-xs">
                        {r.game_mode ?? "—"}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs tabular-nums">
                        {r.duration_seconds != null
                          ? `${r.duration_seconds.toFixed(1)}s`
                          : "—"}
                      </TableCell>
                      <TableCell>
                        <SegBadge method={r.segmentation_method} />
                      </TableCell>
                      <TableCell
                        className="pr-4 text-right"
                        onClick={(event) => event.stopPropagation()}
                        onKeyDown={(event) => event.stopPropagation()}
                      >
                        <div className="flex items-center justify-end">
                          <ConfirmButton
                            iconOnly
                            label="Delete race"
                            onConfirm={async () => {
                              await api.deleteRaceSession(r.id);
                              setRows((prev) =>
                                prev.filter((row) => row.id !== r.id),
                              );
                            }}
                          />
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </Panel>
          )}
        </>
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
