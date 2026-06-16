import { useCallback, useEffect, useState } from "react";
import { CircleAlert, Loader2, Orbit } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { ConfirmButton } from "@/components/ConfirmButton";
import { Page } from "@/components/Page";
import { PageHeader } from "@/components/PageHeader";
import { Panel } from "@/components/Panel";
import { Stat } from "@/components/Stat";
import { FlightRecorderChart } from "@/components/FlightRecorderChart";
import { SessionCourseMap } from "@/components/SessionCourseMap";
import type { LapPath } from "@/components/SessionCourseMap";
import {
  collisionHitDetail,
  collisionHitLabel,
  isDisplayCollision,
  severityVar,
} from "@/lib/collisions";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";
import type {
  CaptureRow,
  CollisionEvent,
  DatasetDetail,
  RaceGateSplitRow,
  RaceLapRow,
  RaceSessionRow,
  ReplayCheckpoint,
  ReplayCourseData,
  SessionTimingDetail,
} from "@/lib/types";
import type { View } from "@/App";

type Props = {
  captureId: string;
  sessionId: string;
  onNavigate: (view: View) => void;
};

export function RaceDetailView({ captureId, sessionId, onNavigate }: Props) {
  const [capture, setCapture] = useState<CaptureRow | null>(null);
  const [session, setSession] = useState<RaceSessionRow | null>(null);
  const [dataset, setDataset] = useState<DatasetDetail | null>(null);
  const [timing, setTiming] = useState<SessionTimingDetail | null>(null);
  const [course, setCourse] = useState<ReplayCourseData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    setCourse(null);
    try {
      const detail = await api.getCapture(captureId);
      setCapture(detail.capture);
      setSession(detail.race_sessions.find((s) => s.id === sessionId) ?? null);
      // Pull the most recent dataset (if processed) for this run's telemetry.
      const datasets = await api.listProcessedDatasets(captureId);
      if (datasets.length > 0) {
        const datasetId = datasets[0].id;
        const [datasetDetail, timingDetail] = await Promise.all([
          api.getDatasetDetail(datasetId),
          api.getSessionTimingDetail(datasetId, sessionId),
        ]);
        setDataset(datasetDetail);
        setTiming(timingDetail);
        // Course geometry (predicted line + gates) loads in the background; the
        // minimap draws the actual laps without it and gains the reference line
        // and gate ticks once it resolves.
        void api
          .resolveSessionCourse(captureId, sessionId)
          .then((res) => setCourse(res.course))
          .catch(() => setCourse(null));
      } else {
        setDataset(null);
        setTiming(null);
      }
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoading(false);
    }
  }, [captureId, sessionId]);

  useEffect(() => {
    load();
  }, [load]);

  if (error) {
    return (
      <Page>
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Couldn&apos;t load race</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      </Page>
    );
  }
  if (loading) {
    return (
      <Page>
        <div className="text-muted-foreground flex items-center gap-2 font-mono text-sm">
          <Loader2 className="size-4 animate-spin" />
          loading…
        </div>
      </Page>
    );
  }
  if (!session || !capture) {
    return (
      <Page
        header={
          <PageHeader
            eyebrow="Race Log · Race"
            title="Race not found"
            actions={
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onNavigate({ kind: "races" })}
              >
                Back
              </Button>
            }
          />
        }
      >
        <Panel>
          <p className="text-muted-foreground text-sm">
            This race session is no longer available.
          </p>
        </Panel>
      </Page>
    );
  }

  const name = session.race ?? session.level ?? session.track ?? "Unknown run";

  return (
    <Page
      header={
        <PageHeader
          eyebrow="Race Log · Race"
          title={name}
          subtitle={
            <span className="font-mono text-xs">
              {new Date(capture.created_at).toLocaleString()} · {capture.id}
            </span>
          }
          actions={
            <div className="flex gap-2">
              {dataset && (
                <Button
                  size="sm"
                  onClick={() =>
                    onNavigate({
                      kind: "flight",
                      datasetId: dataset.dataset.id,
                      sessionId: session.id,
                    })
                  }
                >
                  <Orbit className="size-4" />
                  Visualize
                </Button>
              )}
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onNavigate({ kind: "capture-detail", captureId })}
              >
                View capture
              </Button>
              <ConfirmButton
                label="Delete"
                onConfirm={async () => {
                  await api.deleteRaceSession(session.id);
                  onNavigate({ kind: "races" });
                }}
              />
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onNavigate({ kind: "races" })}
              >
                Back
              </Button>
            </div>
          }
        />
      }
    >
      {dataset ? (
        <RaceBriefing
          session={session}
          dataset={dataset}
          timing={timing}
          course={course}
        />
      ) : (
        <Panel
          title={`Session ${pad2(session.session_index + 1)}`}
          action={
            <span className="text-muted-foreground font-mono text-[11px]">
              not processed
            </span>
          }
        >
          <div className="flex flex-col gap-4">
            <SessionMetaGrid capture={capture} session={session} />
            <p className="text-muted-foreground/60 font-mono text-xs">
              process this capture to see the telemetry, course map, and splits
              for this session
            </p>
          </div>
        </Panel>
      )}
    </Page>
  );
}

/** Per-lap rollup the ladder + verdict read from (duration, contacts, best). */
type LapMeta = {
  lapIndex: number;
  duration: number;
  hits: number;
  best: boolean;
};

/**
 * The Briefing layout: a result hero (verdict + ledger | course map), the
 * flight recorder, derived timing (waterfall + lap consistency), and an
 * incident log. Everything reads from the already-loaded dataset/timing; the
 * splits waterfall itself is unchanged.
 */
function RaceBriefing({
  session,
  dataset,
  timing,
  course,
}: {
  session: RaceSessionRow;
  dataset: DatasetDetail;
  timing: SessionTimingDetail | null;
  course: ReplayCourseData | null;
}) {
  // Samples and collisions are dataset-relative; the session window is in
  // capture seconds, so shift it by the dataset's monotonic start.
  const offset = dataset.summary.start_monotonic_ns / 1e9;
  const winStart = session.start_seconds - offset;
  const winEnd = (session.end_seconds ?? Infinity) - offset;

  const windowedSamples = dataset.samples.filter(
    (s) =>
      s.capture_time_seconds >= winStart && s.capture_time_seconds <= winEnd,
  );
  const collisions = dataset.collision_events
    .filter(
      (c) =>
        c.capture_time_seconds >= winStart && c.capture_time_seconds <= winEnd,
    )
    .filter(isDisplayCollision)
    .sort((a, b) => a.capture_time_seconds - b.capture_time_seconds);

  const laps = timing?.laps ?? [];
  const lapSplits = (timing?.gate_splits ?? []).filter(
    (s) => s.section_kind === "lap_section",
  );
  const bestLap = laps.reduce<number | null>(
    (b, l) => (b == null ? l.duration_seconds : Math.min(b, l.duration_seconds)),
    null,
  );
  const bestLapRow =
    bestLap == null
      ? null
      : (laps.find((l) => Math.abs(l.duration_seconds - bestLap) < 1e-9) ??
        null);
  const theoretical = laps.length >= 2 ? computeTheoretical(lapSplits) : null;

  const lapOf = (t: number) =>
    laps.find(
      (l) => t >= l.start_seconds - offset && t <= l.end_seconds - offset,
    )?.lap_index ?? null;
  const lapMeta: LapMeta[] = laps
    .map((l) => ({
      lapIndex: l.lap_index,
      duration: l.duration_seconds,
      hits: collisions.filter((c) => lapOf(c.capture_time_seconds) === l.lap_index)
        .length,
      best: bestLap != null && Math.abs(l.duration_seconds - bestLap) < 1e-9,
    }))
    .sort((a, b) => a.lapIndex - b.lapIndex);
  const cleanLaps = lapMeta.filter((m) => m.hits === 0).length;
  const bestLapClean = bestLapRow
    ? !collisions.some(
        (c) => lapOf(c.capture_time_seconds) === bestLapRow.lap_index,
      )
    : false;

  // Actual flown path per lap (world X/Z + speed) for the minimap ghosts and
  // its colour metrics; falls back to one path spanning the window if laps
  // weren't derived. The predicted reference line comes from the course.
  const toXZ = (s: DatasetDetail["samples"][number]) => ({
    x: s.pos![0],
    z: s.pos![2],
    speed: s.speed,
  });
  const lapPaths: LapPath[] =
    laps.length > 0
      ? laps
          .map((l) => ({
            lapIndex: l.lap_index,
            points: windowedSamples
              .filter(
                (s) =>
                  s.pos != null &&
                  s.capture_time_seconds >= l.start_seconds - offset &&
                  s.capture_time_seconds <= l.end_seconds - offset,
              )
              .map(toXZ),
          }))
          .filter((l) => l.points.length >= 2)
      : windowedSamples.some((s) => s.pos != null)
        ? [
            {
              lapIndex: 1,
              points: windowedSamples.filter((s) => s.pos != null).map(toXZ),
            },
          ]
        : [];
  const guidePath =
    course?.guide_path?.segments.flatMap((seg) =>
      seg.points.map((p) => ({ x: p[0], z: p[2] })),
    ) ?? [];
  const checkpoints = course?.checkpoints ?? [];

  return (
    <>
      <Panel
        title={`Race result · Session ${pad2(session.session_index + 1)}`}
        action={
          <span className="text-muted-foreground font-mono text-[11px]">
            {[session.track, session.race].filter(Boolean).join(" · ") || "—"}
          </span>
        }
      >
        <BriefingHero
          dataset={dataset}
          lapPaths={lapPaths}
          guidePath={guidePath}
          collisions={collisions}
          checkpoints={checkpoints}
          bestLap={bestLap}
          bestLapRow={bestLapRow}
          bestLapClean={bestLapClean}
          theoretical={theoretical}
          cleanLaps={cleanLaps}
          lapCount={laps.length}
        />
        <Loadout session={session} />
      </Panel>

      <Panel
        title="Flight recorder"
        action={
          <span className="text-muted-foreground font-mono text-[11px] tabular-nums">
            {laps.length > 0 ? `${laps.length} laps · ` : ""}
            {(winEnd - winStart).toFixed(1)}s
          </span>
        }
      >
        {windowedSamples.length >= 2 ? (
          <FlightRecorderChart
            samples={dataset.samples}
            startSeconds={winStart}
            endSeconds={winEnd}
            laps={laps.map((l) => ({
              lap: l.lap_index,
              start: l.start_seconds - offset,
            }))}
            contacts={collisions.map((c) => ({
              t: c.capture_time_seconds,
              severity: c.severity,
            }))}
          />
        ) : (
          <p className="text-muted-foreground/60 font-mono text-xs">
            no telemetry samples in this race window
          </p>
        )}
      </Panel>

      <DerivedTiming timing={timing} bestLap={bestLap} lapMeta={lapMeta} />

      <Panel
        title="Incidents"
        action={
          <span className="text-muted-foreground font-mono text-[11px]">
            {collisions.length === 0
              ? "clean"
              : `${collisions.length} contact${collisions.length === 1 ? "" : "s"} · worst ${Math.max(
                  ...collisions.map((c) => c.severity),
                )}/10`}
          </span>
        }
      >
        <IncidentLog collisions={collisions} lapOf={lapOf} winStart={winStart} />
      </Panel>
    </>
  );
}

function BriefingHero({
  dataset,
  lapPaths,
  guidePath,
  collisions,
  checkpoints,
  bestLap,
  bestLapRow,
  bestLapClean,
  theoretical,
  cleanLaps,
  lapCount,
}: {
  dataset: DatasetDetail;
  lapPaths: LapPath[];
  guidePath: { x: number; z: number }[];
  collisions: CollisionEvent[];
  checkpoints: ReplayCheckpoint[];
  bestLap: number | null;
  bestLapRow: RaceLapRow | null;
  bestLapClean: boolean;
  theoretical: number | null;
  cleanLaps: number;
  lapCount: number;
}) {
  const onTable =
    bestLap != null && theoretical != null ? theoretical - bestLap : null;
  const lapTag = bestLapRow ? `Lap ${pad2(bestLapRow.lap_index)}` : null;
  const runKind = bestLapClean ? "Clean run" : "Best run";

  const verdict =
    bestLap == null ? (
      <>
        No complete lap was timed for this run — the gate splits below show the
        sections that were captured.
      </>
    ) : onTable != null && onTable > 0.005 && theoretical != null ? (
      <>
        {runKind} on {lapTag?.toLowerCase()}. Stitching your fastest gate-to-gate
        sections leaves{" "}
        <b className="text-signal font-mono font-semibold">
          {formatSigned(onTable)}s
        </b>{" "}
        on the table — your theoretical best is{" "}
        <b className="text-signal font-mono font-semibold">
          {theoretical.toFixed(2)}s
        </b>
        .
      </>
    ) : (
      <>
        {runKind} on {lapTag?.toLowerCase()} — about as tight as your sections
        went this session.
      </>
    );

  const ledger: [string, string, boolean][] = [
    ["Top speed", `${dataset.summary.max_speed.toFixed(1)} m/s`, false],
    ["Avg speed", `${dataset.summary.mean_speed.toFixed(1)} m/s`, false],
    ["Laps", lapCount.toLocaleString(), false],
    ["Clean laps", `${cleanLaps} / ${lapCount}`, false],
    ["Contacts", collisions.length.toLocaleString(), collisions.length > 0],
  ];

  return (
    <div className="grid items-stretch gap-0 lg:grid-cols-[1fr_1.06fr]">
      {/* verdict + ledger */}
      <div className="border-border flex flex-col lg:border-r lg:pr-6">
        <div>
          <div className="text-muted-foreground font-mono text-[10px] font-medium tracking-[0.18em] uppercase">
            {lapTag ? `Best lap · ${lapTag}` : "Best lap"}
          </div>
          <div className="text-signal mt-1.5 font-mono text-5xl leading-none font-semibold tracking-tight tabular-nums [text-shadow:0_0_30px_oklch(0.86_0.19_128/0.3)]">
            {bestLap == null ? (
              "—"
            ) : (
              <>
                {bestLap.toFixed(2)}
                <span className="text-muted-foreground text-2xl font-medium">
                  s
                </span>
              </>
            )}
          </div>
          <p className="text-foreground mt-3.5 max-w-[42ch] text-sm leading-relaxed">
            {verdict}
          </p>
        </div>
        <div className="border-border mt-4 flex flex-col border-t pt-3.5">
          {ledger.map(([k, v, warn]) => (
            <div
              key={k}
              className="border-border flex items-baseline justify-between gap-3 border-b py-1.5 last:border-b-0"
            >
              <span className="text-muted-foreground font-mono text-[10px] tracking-[0.14em] uppercase">
                {k}
              </span>
              <span
                className={cn(
                  "font-mono text-[17px] font-semibold tabular-nums",
                  warn && "text-warn",
                )}
              >
                {v}
              </span>
            </div>
          ))}
        </div>
      </div>

      {/* course minimap, filling the hero's full height */}
      <div className="mt-5 flex flex-col lg:mt-0 lg:pl-5">
        <SessionCourseMap
          className="border-border min-h-[280px] flex-1 overflow-hidden rounded-lg border bg-[oklch(0.14_0.006_256)]"
          laps={lapPaths}
          guidePath={guidePath}
          checkpoints={checkpoints}
          collisions={collisions}
        />
      </div>
    </div>
  );
}

/** Compact mono strip recovering the run's loadout metadata under the hero. */
function Loadout({ session }: { session: RaceSessionRow }) {
  const chips: [string, string | null][] = [
    ["Level", session.level],
    ["Track", session.track],
    ["Drone", session.drone],
    ["Mode", session.game_mode],
    [
      "Conf",
      session.confidence != null
        ? `${(session.confidence * 100).toFixed(0)}%`
        : null,
    ],
    ["Method", session.segmentation_method],
  ].filter(([, v]) => v != null) as [string, string][];

  return (
    <div className="border-border text-muted-foreground mt-4 flex flex-wrap items-center border-t pt-3 font-mono text-[11px]">
      {chips.map(([k, v]) => (
        <span
          key={k}
          className="border-border border-r px-3 first:pl-0 last:border-r-0"
        >
          <span className="block text-[9px] tracking-[0.16em] uppercase opacity-70">
            {k}
          </span>
          <b className="text-foreground font-semibold">{v}</b>
        </span>
      ))}
    </div>
  );
}

function DerivedTiming({
  timing,
  bestLap,
  lapMeta,
}: {
  timing: SessionTimingDetail | null;
  bestLap: number | null;
  lapMeta: LapMeta[];
}) {
  const laps = timing?.laps ?? [];
  const lapSplits = (timing?.gate_splits ?? []).filter(
    (s) => s.section_kind === "lap_section",
  );

  return (
    <Panel
      title="Derived timing"
      action={
        bestLap != null && (
          <span className="text-muted-foreground font-mono text-[11px] tabular-nums">
            best <b className="text-signal font-semibold">{formatDuration(bestLap)}</b>
          </span>
        )
      }
    >
      {laps.length === 0 && lapSplits.length === 0 ? (
        <p className="text-muted-foreground/60 font-mono text-xs">
          no complete lap or gate-section timing was derived for this processed
          dataset
        </p>
      ) : (
        <div className="flex flex-col gap-6">
          {lapSplits.length > 0 && (
            <SplitsWaterfall splits={lapSplits} laps={laps} bestLap={bestLap} />
          )}
          {lapMeta.length > 0 && bestLap != null && (
            <div className="border-border flex flex-col gap-2.5 border-t pt-5">
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-sm font-semibold tracking-tight">
                  Lap consistency
                </h3>
                <span className="text-muted-foreground font-mono text-[10px] tracking-[0.16em] uppercase">
                  duration per lap
                </span>
              </div>
              <LapLadder lapMeta={lapMeta} bestLap={bestLap} />
            </div>
          )}
        </div>
      )}
    </Panel>
  );
}

function LapLadder({
  lapMeta,
  bestLap,
}: {
  lapMeta: LapMeta[];
  bestLap: number;
}) {
  const totals = lapMeta.map((m) => m.duration);
  const min = Math.min(...totals);
  const max = Math.max(...totals);
  const range = max - min || Math.max(min * 0.04, 0.5);
  const lo = min - range * 0.4;
  const hi = max + range * 0.15;
  const pct = (t: number) => ((t - lo) / (hi - lo)) * 100;
  const spread = max - min;
  const cleanCount = lapMeta.filter((m) => m.hits === 0).length;
  const bestIdx = lapMeta.find((m) => m.best)?.lapIndex ?? lapMeta[0].lapIndex;

  return (
    <div className="flex flex-col gap-0.5">
      {lapMeta.map((m) => {
        const w = pct(m.duration);
        return (
          <div
            key={m.lapIndex}
            className="hover:bg-muted/40 flex items-center gap-3 rounded-sm px-1 py-1 transition-colors"
          >
            <span className="text-muted-foreground w-14 shrink-0 font-mono text-[11px] font-semibold tracking-[0.12em] uppercase">
              Lap {pad2(m.lapIndex)}
            </span>
            <div className="relative h-5 min-w-[160px] flex-1">
              <div
                className={cn(
                  "absolute top-0.5 left-0 h-4 rounded-[3px] border",
                  m.best
                    ? "bg-signal/20 border-signal/55 shadow-[0_0_14px_oklch(0.86_0.19_128/0.18)]"
                    : m.hits > 0
                      ? "bg-warn/[0.14] border-warn/40"
                      : "bg-muted border-border",
                )}
                style={{ width: `${Math.max(w, 2)}%` }}
              >
                {m.hits > 0 && (
                  <span
                    className="bg-warn absolute top-[-1px] h-[18px] w-0.5 rounded shadow-[0_0_8px_oklch(0.8_0.15_78/0.6)]"
                    style={{ left: `${Math.min(85, w * 0.62)}%` }}
                  />
                )}
              </div>
            </div>
            <span
              className={cn(
                "w-16 shrink-0 text-right font-mono text-sm font-semibold tabular-nums",
                m.best && "text-signal",
              )}
            >
              {m.duration.toFixed(2)}s
            </span>
            <span
              className={cn(
                "text-muted-foreground w-14 shrink-0 text-right font-mono text-xs tabular-nums",
                !m.best && "text-warn",
              )}
            >
              {m.best ? "best" : formatSigned(m.duration - bestLap)}
            </span>
            <span
              className={cn(
                "w-16 shrink-0 rounded-[3px] border py-0.5 text-center font-mono text-[9px] tracking-[0.14em] uppercase",
                m.hits > 0
                  ? "border-warn/35 text-warn"
                  : "border-signal/30 text-signal",
              )}
            >
              {m.hits > 0 ? `${m.hits} hit` : "clean"}
            </span>
          </div>
        );
      })}
      <div className="border-border text-muted-foreground mt-3 flex flex-wrap gap-x-5 gap-y-1 border-t pt-2.5 font-mono text-[11px]">
        <span>
          spread <b className="text-foreground font-semibold">{spread.toFixed(2)}s</b>
        </span>
        <span>
          best{" "}
          <b className="text-signal font-semibold">
            Lap {pad2(bestIdx)} · {bestLap.toFixed(2)}s
          </b>
        </span>
        <span>
          clean{" "}
          <b className="text-foreground font-semibold">
            {cleanCount}/{lapMeta.length}
          </b>
        </span>
      </div>
    </div>
  );
}

function IncidentLog({
  collisions,
  lapOf,
  winStart,
}: {
  collisions: CollisionEvent[];
  lapOf: (t: number) => number | null;
  winStart: number;
}) {
  if (collisions.length === 0) {
    return (
      <p className="text-muted-foreground/60 font-mono text-xs">
        clean session — no contacts recorded in this window
      </p>
    );
  }
  const cols =
    "grid grid-cols-[44px_64px_minmax(0,1fr)_132px_136px_72px] gap-3 items-center";

  return (
    <div className="flex flex-col">
      <div
        className={cn(
          cols,
          "text-muted-foreground border-border border-b px-1.5 pb-2 font-mono text-[9px] font-medium tracking-[0.16em] uppercase",
        )}
      >
        <span>Lap</span>
        <span>Time</span>
        <span>What you hit</span>
        <span>Severity</span>
        <span>Speed lost</span>
        <span className="text-right">Decel</span>
      </div>
      {collisions.map((c, i) => {
        const lap = lapOf(c.capture_time_seconds);
        const label = collisionHitLabel(c) ?? "Unknown object";
        const detail = collisionHitDetail(c);
        const lost = Math.abs(c.speed_delta);
        return (
          <div
            key={`${c.sample_index}-${i}`}
            className={cn(cols, "border-border/60 border-b px-1.5 py-2.5")}
          >
            <span className="text-muted-foreground font-mono text-[11px] font-semibold tracking-[0.1em] uppercase">
              {lap != null ? `L${lap}` : "—"}
            </span>
            <span className="text-foreground font-mono text-[13px] tabular-nums">
              {formatClock(c.capture_time_seconds - winStart)}
            </span>
            <span className="min-w-0">
              <span className="block truncate text-[13.5px] font-medium">
                {label}
              </span>
              {detail && (
                <span className="text-muted-foreground block truncate font-mono text-[10px]">
                  {detail}
                </span>
              )}
            </span>
            <span className="flex items-center gap-2">
              <span className="flex gap-0.5">
                {Array.from({ length: 10 }).map((_, j) => (
                  <span
                    key={j}
                    className="h-3 w-1 rounded-[1px]"
                    style={{
                      background:
                        j < c.severity ? severityVar(c.severity) : "oklch(1 0 0 / 0.1)",
                    }}
                  />
                ))}
              </span>
              <span className="text-muted-foreground font-mono text-xs tabular-nums">
                {c.severity}/10
              </span>
            </span>
            <span className="font-mono text-[12.5px] tabular-nums">
              <span className="text-muted-foreground/70">
                {c.speed_before.toFixed(1)}
              </span>{" "}
              → <span className="text-warn font-semibold">{c.speed_after.toFixed(1)}</span>{" "}
              <span className="text-muted-foreground">(−{lost.toFixed(1)})</span>
            </span>
            <span className="text-muted-foreground text-right font-mono text-[12.5px] tabular-nums">
              {c.decel_mps2.toFixed(0)} m/s²
            </span>
          </div>
        );
      })}
    </div>
  );
}

/** The flat session-metadata grid, kept for the unprocessed fallback. */
function SessionMetaGrid({
  capture,
  session,
}: {
  capture: CaptureRow;
  session: RaceSessionRow;
}) {
  return (
    <div className="grid grid-cols-2 gap-x-5 gap-y-4 sm:grid-cols-4">
      <Stat
        label="Date"
        value={new Date(capture.created_at).toLocaleDateString()}
      />
      <Stat label="Level" value={session.level ?? "—"} />
      <Stat label="Race" value={session.race ?? "—"} />
      <Stat label="Mode" value={session.game_mode ?? "—"} />
      <Stat label="Drone" value={session.drone ?? "—"} />
      <Stat
        label="Window"
        value={`${session.start_seconds.toFixed(1)}–${
          session.end_seconds?.toFixed(1) ?? "?"
        }s`}
      />
      <Stat label="Method" value={session.segmentation_method} />
      <Stat
        label="Confidence"
        value={
          session.confidence != null
            ? `${(session.confidence * 100).toFixed(0)}%`
            : "—"
        }
      />
    </div>
  );
}

/** Theoretical best lap = fastest run of each gate-pair, stitched in course
 * order. Mirrors the waterfall's ideal lap; null with <2 laps or no repeats. */
function computeTheoretical(lapSplits: RaceGateSplitRow[]): number | null {
  const finite = lapSplits.filter((s) => Number.isFinite(s.duration_seconds));
  if (finite.length === 0) return null;
  const best = extremaByGate(finite, "min");
  const counts = countByGate(finite);
  if (![...counts.values()].some((c) => c > 1)) return null;

  const byLap = new Map<number, RaceGateSplitRow[]>();
  for (const s of finite) {
    const arr = byLap.get(s.lap_index) ?? [];
    arr.push(s);
    byLap.set(s.lap_index, arr);
  }
  let template: RaceGateSplitRow[] = [];
  for (const arr of byLap.values()) {
    if (arr.length > template.length) template = arr;
  }
  return template
    .slice()
    .sort((a, b) => a.section_index - b.section_index)
    .reduce((sum, s) => sum + (best.get(sectionKey(s)) ?? s.duration_seconds), 0);
}

function formatClock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const sec = seconds - m * 60;
  return `${m}:${sec.toFixed(1).padStart(4, "0")}`;
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/**
 * A split = one gate-to-gate segment within a lap, prepared for the waterfall:
 * positioned on a shared lap-time scale, ranked against the same gate-pair in
 * other laps, and tagged best/worst the way a motorsport timing tower colours
 * sectors (lime = session-best run of that gate-pair, amber = slowest).
 */
type SplitView = {
  id: string;
  gateKey: string;
  label: string;
  /** Lap-elapsed seconds at this split's entry/exit gate. */
  startInLap: number;
  endInLap: number;
  duration: number;
  /** Δ vs the fastest run of this gate-pair; null when it never repeats. */
  deltaToBest: number | null;
  rank: "best" | "worst" | "mid" | "solo";
};

type LapGroup = {
  lapIndex: number;
  total: number;
  isBestLap: boolean;
  splits: SplitView[];
};

function SplitsWaterfall({
  splits,
  laps,
  bestLap,
}: {
  splits: RaceGateSplitRow[];
  laps: RaceLapRow[];
  bestLap: number | null;
}) {
  const finite = splits.filter((s) => Number.isFinite(s.duration_seconds));
  const best = extremaByGate(finite, "min");
  const worst = extremaByGate(finite, "max");
  const counts = countByGate(finite);

  const lapStartFor = (lapIndex: number) => {
    const lap = laps.find((l) => l.lap_index === lapIndex);
    if (lap) return lap.start_seconds;
    const starts = finite
      .filter((s) => s.lap_index === lapIndex)
      .map((s) => s.start_seconds);
    return starts.length ? Math.min(...starts) : 0;
  };

  // Group splits by lap, in course order, with lap-elapsed positions.
  const lapIndices = [...new Set(finite.map((s) => s.lap_index))].sort(
    (a, b) => a - b,
  );
  const groups: LapGroup[] = lapIndices.map((lapIndex) => {
    const lapStart = lapStartFor(lapIndex);
    const rows = finite
      .filter((s) => s.lap_index === lapIndex)
      .sort((a, b) => a.section_index - b.section_index)
      .map<SplitView>((s) => {
        const key = sectionKey(s);
        const repeats = (counts.get(key) ?? 0) > 1;
        const bestForGate = best.get(key);
        const isBest =
          repeats &&
          bestForGate != null &&
          Math.abs(s.duration_seconds - bestForGate) < 1e-9;
        const isWorst =
          repeats &&
          Math.abs(s.duration_seconds - (worst.get(key) ?? 0)) < 1e-9;
        return {
          id: s.id,
          gateKey: key,
          label: gateLabel(s),
          startInLap: s.start_seconds - lapStart,
          endInLap: s.end_seconds - lapStart,
          duration: s.duration_seconds,
          deltaToBest:
            repeats && bestForGate != null
              ? s.duration_seconds - bestForGate
              : null,
          rank: !repeats ? "solo" : isBest ? "best" : isWorst ? "worst" : "mid",
        };
      });
    const lap = laps.find((l) => l.lap_index === lapIndex);
    const total = lap?.duration_seconds ?? rows[rows.length - 1]?.endInLap ?? 0;
    return {
      lapIndex,
      total,
      isBestLap: bestLap != null && Math.abs(total - bestLap) < 1e-9,
      splits: rows,
    };
  });

  const multiLap = groups.length > 1;

  // Shared horizontal scale: the longest lap fills the track.
  const scaleMax = Math.max(
    ...groups.map((g) => g.splits[g.splits.length - 1]?.endInLap ?? 0),
    1e-6,
  );

  // Ideal lap: fastest run of each gate-pair, stitched contiguously in course
  // order. Only meaningful once a gate-pair has been flown more than once.
  const ideal = buildIdealLap(groups, best);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="text-sm font-semibold tracking-tight">Splits</h3>
        <span className="text-muted-foreground font-mono text-[10px] tracking-[0.16em] uppercase">
          gate → gate
        </span>
      </div>

      {/* column header */}
      <div className="text-muted-foreground flex items-center gap-3 font-mono text-[9.5px] font-medium tracking-[0.16em] uppercase">
        <span className="w-16 shrink-0">Split</span>
        <span className="w-12 shrink-0 text-right">Δ</span>
        <span className="flex-1">Lap timeline →</span>
        <span className="w-16 shrink-0 text-right">Total</span>
      </div>

      <div className="border-border/70 flex flex-col gap-1 border-t pt-2">
        {groups.map((group) => (
          <div key={group.lapIndex} className="flex flex-col">
            <LapHeader
              name={`Lap ${group.lapIndex.toString().padStart(2, "0")}`}
              total={group.total}
              fast={group.isBestLap && multiLap}
            />
            {group.splits.map((split) => (
              <SplitRow key={split.id} split={split} scaleMax={scaleMax} />
            ))}
          </div>
        ))}

        {ideal && (
          <div className="mt-1 flex flex-col">
            <LapHeader name="◆ Ideal lap" total={ideal.total} ideal />
            {ideal.splits.map((split) => (
              <SplitRow
                key={`ideal-${split.id}`}
                split={split}
                scaleMax={scaleMax}
                forceBest
              />
            ))}
          </div>
        )}

        <TimelineAxis scaleMax={scaleMax} />
      </div>

      {ideal && bestLap != null && (
        <div className="border-signal/30 bg-signal/[0.07] flex flex-wrap items-center gap-x-4 gap-y-1 rounded-md border px-4 py-3">
          <span className="text-signal font-mono text-[10px] font-semibold tracking-[0.18em] uppercase">
            Theoretical best lap
          </span>
          <span className="text-signal font-mono text-2xl leading-none font-semibold tabular-nums">
            {formatDuration(ideal.total)}
          </span>
          <span className="text-muted-foreground ml-auto font-mono text-xs tabular-nums">
            vs best lap {formatDuration(bestLap)} ·{" "}
            <b className="text-foreground font-semibold">
              {formatSigned(ideal.total - bestLap)}
            </b>{" "}
            on the table
          </span>
        </div>
      )}

      <div className="text-muted-foreground flex flex-wrap gap-x-5 gap-y-1 font-mono text-[11px]">
        <span className="flex items-center gap-1.5">
          <i className="bg-signal inline-block size-2.5 rounded-[2px]" /> session-best
          split
        </span>
        <span className="flex items-center gap-1.5">
          <i className="bg-warn inline-block size-2.5 rounded-[2px]" /> your slowest
        </span>
        <span>bar position = window · width = split time</span>
      </div>
    </div>
  );
}

function LapHeader({
  name,
  total,
  fast,
  ideal,
}: {
  name: string;
  total: number;
  fast?: boolean;
  ideal?: boolean;
}) {
  return (
    <div className="flex items-baseline gap-2.5 pt-3 pb-1">
      <span
        className={cn(
          "font-mono text-xs font-semibold tracking-[0.14em] uppercase",
          ideal && "text-signal",
        )}
      >
        {name}
      </span>
      {fast && (
        <span className="text-background bg-signal rounded-[3px] px-1.5 py-0.5 font-mono text-[9px] font-semibold tracking-[0.18em] uppercase shadow-[0_0_12px_oklch(0.86_0.19_128/0.45)]">
          ◀ Fast lap
        </span>
      )}
      <span
        className={cn(
          "border-border/60 mt-0.5 flex-1 border-b border-dotted",
          ideal && "border-signal/35",
        )}
      />
      <span className="text-muted-foreground font-mono text-xs tabular-nums">
        total{" "}
        <b
          className={cn(
            "text-foreground text-sm font-semibold",
            ideal && "text-signal",
          )}
        >
          {formatDuration(total)}
        </b>
      </span>
    </div>
  );
}

function SplitRow({
  split,
  scaleMax,
  forceBest,
}: {
  split: SplitView;
  scaleMax: number;
  forceBest?: boolean;
}) {
  const rank = forceBest ? "best" : split.rank;
  const left = (split.startInLap / scaleMax) * 100;
  const width = (split.duration / scaleMax) * 100;

  const barTone =
    rank === "best"
      ? "bg-signal/20 border-signal/55"
      : rank === "worst"
        ? "bg-warn/[0.14] border-warn/45"
        : "bg-muted border-border";
  const timeTone =
    rank === "best"
      ? "text-signal"
      : rank === "worst"
        ? "text-warn"
        : "text-foreground";

  return (
    <div
      className="hover:bg-muted/40 flex items-center gap-3 rounded-sm py-1 transition-colors"
      title={`${split.label} · ${formatDuration(split.duration)} · ${split.startInLap.toFixed(2)}–${split.endInLap.toFixed(2)}s into lap`}
    >
      <span className="text-muted-foreground w-16 shrink-0 font-mono text-[13px] whitespace-nowrap">
        {split.label}
      </span>
      <span
        className={cn(
          "w-12 shrink-0 text-right font-mono text-[12px] tabular-nums",
          rank === "best"
            ? "text-signal/70"
            : split.deltaToBest != null
              ? "text-warn"
              : "text-muted-foreground/60",
        )}
      >
        {rank === "best" || split.deltaToBest == null
          ? "—"
          : formatSigned(split.deltaToBest)}
      </span>
      <div className="relative h-[22px] min-w-[200px] flex-1">
        <div className="pointer-events-none absolute inset-0">
          {[25, 50, 75].map((p) => (
            <span
              key={p}
              className="bg-foreground/[0.05] absolute -top-px -bottom-px w-px"
              style={{ left: `${p}%` }}
            />
          ))}
        </div>
        <div
          className={cn(
            "absolute top-[3px] flex h-4 items-center overflow-hidden rounded-[3px] border px-1.5",
            barTone,
          )}
          style={{ left: `${left}%`, width: `${Math.max(width, 0)}%` }}
        >
          <span
            className={cn(
              "font-mono text-[12px] font-semibold tabular-nums whitespace-nowrap",
              timeTone,
            )}
          >
            {split.duration.toFixed(2)}
          </span>
        </div>
      </div>
      <span className="text-foreground w-16 shrink-0 text-right font-mono text-[13px] tabular-nums">
        {split.endInLap.toFixed(2)}
      </span>
    </div>
  );
}

function TimelineAxis({ scaleMax }: { scaleMax: number }) {
  const ticks = [0, 0.25, 0.5, 0.75, 1];
  return (
    <div className="flex items-center gap-3 pt-1">
      <span className="w-16 shrink-0" />
      <span className="w-12 shrink-0" />
      <div className="relative h-3.5 flex-1">
        {ticks.map((t) => (
          <span
            key={t}
            className="text-muted-foreground absolute top-0 -translate-x-1/2 font-mono text-[9px] tabular-nums"
            style={{ left: `${t * 100}%` }}
          >
            {(scaleMax * t).toFixed(1)}
            {t === 1 ? "s" : ""}
          </span>
        ))}
      </div>
      <span className="w-16 shrink-0" />
    </div>
  );
}

function buildIdealLap(
  groups: LapGroup[],
  best: Map<string, number>,
): { total: number; splits: SplitView[] } | null {
  if (groups.length < 2) return null;
  // Use the lap with the most splits as the canonical gate order.
  const template = groups.reduce((a, b) =>
    b.splits.length > a.splits.length ? b : a,
  );
  if (template.splits.length === 0) return null;

  let cursor = 0;
  const splits = template.splits.map<SplitView>((s, i) => {
    const duration = best.get(s.gateKey) ?? s.duration;
    const startInLap = cursor;
    cursor += duration;
    return {
      id: `${i}`,
      gateKey: s.gateKey,
      label: s.label,
      startInLap,
      endInLap: cursor,
      duration,
      deltaToBest: 0,
      rank: "best",
    };
  });
  return { total: cursor, splits };
}

function extremaByGate(
  splits: RaceGateSplitRow[],
  kind: "min" | "max",
): Map<string, number> {
  const out = new Map<string, number>();
  for (const split of splits) {
    const key = sectionKey(split);
    const current = out.get(key);
    if (
      current == null ||
      (kind === "min"
        ? split.duration_seconds < current
        : split.duration_seconds > current)
    ) {
      out.set(key, split.duration_seconds);
    }
  }
  return out;
}

function countByGate(splits: RaceGateSplitRow[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const split of splits) {
    const key = sectionKey(split);
    out.set(key, (out.get(key) ?? 0) + 1);
  }
  return out;
}

function sectionKey(split: RaceGateSplitRow): string {
  return [
    split.from_checkpoint_id ?? split.from_checkpoint_sequence ?? "from",
    split.to_checkpoint_id ?? split.to_checkpoint_sequence ?? "to",
  ].join("->");
}

/** Compact gate-pair label, e.g. "G1 → G2"; nulls render as a dot. */
function gateLabel(split: RaceGateSplitRow): string {
  return `${gateTick(split.from_checkpoint_sequence)} → ${gateTick(
    split.to_checkpoint_sequence,
  )}`;
}

function gateTick(sequence: number | null): string {
  return sequence == null ? "•" : `G${sequence + 1}`;
}

function formatDuration(seconds: number): string {
  return `${seconds.toFixed(2)}s`;
}

function formatSigned(seconds: number): string {
  const sign = seconds > 0 ? "+" : seconds < 0 ? "−" : "";
  return `${sign}${Math.abs(seconds).toFixed(2)}`;
}

function formatError(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message?: unknown }).message ?? e);
  }
  return String(e);
}
