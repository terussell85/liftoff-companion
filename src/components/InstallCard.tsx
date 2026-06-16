import {
  CheckCircle2,
  CircleAlert,
  Loader2,
  RefreshCw,
  RotateCcw,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Stat } from "@/components/Stat";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { assetPhaseLabel, formatAssetDate, progressPercent } from "@/lib/assetStatus";
import type { AssetRefreshProgress } from "@/lib/types";

/** One Liftoff install, joined across config dir + asset source + player log. */
export type InstallView = {
  /** Stable bucket key (also used to match refresh progress to this card). */
  key: string;
  title: string;
  sourceBadge: string | null;
  /** Install present on disk (a data dir exists or a valid asset source). */
  detected: boolean;
  // Telemetry config
  configPath: string | null;
  configExists: boolean;
  matchesCanonical: boolean | null;
  // Race data
  dataRoot: string | null;
  cacheStatus: string | null;
  raceCount: number;
  trackCount: number;
  extractedAt: string | null;
  errorMessage: string | null;
  // Game log
  logFound: boolean;
  logPath: string | null;
};

export type InstallNotice = { kind: "enabled" | "stopped" } | null;

type Props = {
  install: InstallView;
  /** Live refresh progress when this install is the one being synced. */
  progress?: AssetRefreshProgress | null;
  /** Transient confirmation shown after toggling. */
  notice?: InstallNotice;
  busy?: boolean;
  onToggle: (install: InstallView, next: boolean) => void;
  onReapply: (install: InstallView) => void;
  onResync: (install: InstallView) => void;
};

export function InstallCard({
  install,
  progress,
  notice,
  busy,
  onToggle,
  onReapply,
  onResync,
}: Props) {
  const tracked = install.configExists;
  const differs = tracked && install.matchesCanonical === false;
  const errored = install.cacheStatus === "error";
  const syncing = progress != null;
  const resyncTitle = errored
    ? "Retry extraction"
    : install.cacheStatus === "fresh" || install.cacheStatus === "stale"
      ? "Re-sync race data"
      : "Sync race data";

  const accent = !install.detected
    ? "muted"
    : errored
      ? "destructive"
      : differs
        ? "warn"
        : tracked
          ? "signal"
          : "muted";

  const dotClass =
    accent === "signal"
      ? "bg-signal"
      : accent === "warn"
        ? "bg-warn"
        : accent === "destructive"
          ? "bg-destructive"
          : "bg-muted-foreground/40";

  const hairline =
    accent === "signal"
      ? "from-signal"
      : accent === "warn"
        ? "from-warn"
        : accent === "destructive"
          ? "from-destructive"
          : "from-transparent";

  return (
    <section
      data-slot="card"
      className={cn(
        "bg-card border-border relative overflow-hidden rounded-lg border",
        !install.detected && "opacity-60",
      )}
    >
      <span
        className={cn(
          "absolute inset-x-0 top-0 h-0.5 bg-gradient-to-r to-transparent",
          hairline,
        )}
      />

      {/* header */}
      <div className="flex items-start gap-3 px-4 pt-3.5 pb-3">
        <span className={cn("mt-1.5 size-1.5 shrink-0 rounded-full", dotClass)} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 font-mono text-[13px]">
            <span className="truncate">{install.title}</span>
            {install.sourceBadge && (
              <span className="border-border text-muted-foreground shrink-0 rounded border px-1.5 py-0.5 font-mono text-[9px] tracking-[0.14em] uppercase">
                {install.sourceBadge}
              </span>
            )}
          </div>
          <div className="text-muted-foreground truncate font-mono text-[11px]">
            {install.dataRoot ?? install.configPath ?? "not detected on this machine"}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-3">
          <label className="flex cursor-pointer items-center gap-2">
            <span
              className={cn(
                "font-mono text-[10px] tracking-[0.12em] uppercase",
                tracked ? "text-signal" : "text-muted-foreground",
              )}
            >
              {tracked ? "Tracked" : "Off"}
            </span>
            <Switch
              checked={tracked}
              disabled={busy || !install.detected || !install.configPath}
              onCheckedChange={(next) => onToggle(install, next)}
            />
          </label>
        </div>
      </div>

      {/* drift / notice strip */}
      {differs && !notice && (
        <div className="border-border text-warn flex items-center gap-2 border-t bg-[oklch(0.8_0.15_78_/_0.06)] px-4 py-2 font-mono text-[11px]">
          <CircleAlert className="size-3.5 shrink-0" />
          config differs from canonical
          <Button
            size="sm"
            variant="outline"
            className="ml-auto h-6 px-2 text-[11px]"
            disabled={busy}
            onClick={() => onReapply(install)}
          >
            Re-apply
          </Button>
        </div>
      )}
      {notice?.kind === "enabled" && (
        <div className="border-border text-signal flex items-center gap-2 border-t bg-[oklch(0.86_0.19_128_/_0.06)] px-4 py-2 font-mono text-[11px]">
          <CheckCircle2 className="size-3.5 shrink-0" />
          config written · reset your drone in Liftoff to re-read it
        </div>
      )}
      {notice?.kind === "stopped" && (
        <div className="border-border text-muted-foreground flex items-center gap-2 border-t px-4 py-2 font-mono text-[11px]">
          <RotateCcw className="size-3.5 shrink-0" />
          tracking stopped · previous config restored
        </div>
      )}

      {/* race data */}
      {install.detected ? (
        <>
        <div className="border-border border-t px-4 py-3">
          <div className="mb-2.5 flex items-center justify-between gap-2">
            <span className="text-muted-foreground font-mono text-[10px] font-medium tracking-[0.14em] uppercase">
              Race data
            </span>
            <div className="flex items-center gap-2">
              {syncing ? (
                <span className="text-signal flex items-center gap-1.5 font-mono text-[11px]">
                  <Loader2 className="size-3 animate-spin" />
                  {assetPhaseLabel(progress!.phase)}
                </span>
              ) : install.cacheStatus === "fresh" ? (
                <span className="text-muted-foreground font-mono text-[11px]">
                  {install.extractedAt
                    ? `fresh · ${formatAssetDate(install.extractedAt)}`
                    : "fresh"}
                </span>
              ) : install.cacheStatus === "stale" ? (
                <span className="text-warn flex items-center gap-1.5 font-mono text-[11px]">
                  <span className="bg-warn size-1.5 rounded-full" />
                  stale
                </span>
              ) : null}
              {!syncing && install.dataRoot && (
                <Button
                  size="icon"
                  variant="ghost"
                  className="text-muted-foreground hover:text-foreground -mr-1 size-6"
                  title={resyncTitle}
                  disabled={busy}
                  onClick={() => onResync(install)}
                >
                  <RefreshCw className="size-3.5" />
                </Button>
              )}
            </div>
          </div>

          {syncing ? (
            <SyncBody progress={progress!} />
          ) : errored ? (
            <ErrorBody message={install.errorMessage} />
          ) : install.cacheStatus === "fresh" || install.cacheStatus === "stale" ? (
            <div className="flex gap-6">
              <Stat label="Races" value={install.raceCount.toLocaleString()} accent />
              <Stat label="Tracks" value={install.trackCount.toLocaleString()} />
            </div>
          ) : (
            <span className="text-muted-foreground/70 font-mono text-xs">
              not loaded
            </span>
          )}
        </div>
        <GameLogRow found={install.logFound} path={install.logPath} />
        </>
      ) : (
        <div className="border-border text-muted-foreground/80 border-t px-4 py-3 font-mono text-[11px] leading-relaxed">
          Install not found. Launch it once so it writes its data folder, or it
          isn't installed.
        </div>
      )}
    </section>
  );
}

function GameLogRow({ found, path }: { found: boolean; path: string | null }) {
  return (
    <div
      className="border-border flex items-center gap-2.5 border-t px-4 py-2"
      title={path ?? undefined}
    >
      <span
        className={cn(
          "size-1.5 shrink-0 rounded-full",
          found ? "bg-signal" : "bg-muted-foreground/40",
        )}
      />
      <span className="text-muted-foreground font-mono text-[10px] font-medium tracking-[0.14em] uppercase">
        Game log
      </span>
      <span
        className={cn(
          "ml-auto font-mono text-[11px]",
          found ? "text-muted-foreground" : "text-muted-foreground/60",
        )}
      >
        {found ? "tailing" : "not found"}
      </span>
    </div>
  );
}

function SyncBody({ progress }: { progress: AssetRefreshProgress }) {
  const levelsTotal = progress.levels_total ?? 0;
  const levelsDone = progress.levels_done ?? 0;
  const pct =
    levelsTotal > 0
      ? progressPercent(levelsDone, levelsTotal)
      : progressPercent(progress.sources_done, progress.sources_total);
  return (
    <div className="flex flex-col gap-2">
      <div className="bg-muted/70 h-1.5 overflow-hidden rounded-full">
        <div
          className="bg-signal h-full rounded-full transition-[width] duration-300"
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="text-muted-foreground truncate font-mono text-[11px]">
        {progress.current_level
          ? `${progress.current_level} · `
          : ""}
        {progress.races_found > 0
          ? `${progress.races_found.toLocaleString()} races found`
          : progress.message}
      </div>
    </div>
  );
}

function ErrorBody({ message }: { message: string | null }) {
  return (
    <div className="text-destructive flex items-start gap-2 rounded-md border border-[oklch(0.64_0.21_22_/_0.2)] bg-[oklch(0.64_0.21_22_/_0.08)] px-2.5 py-2 font-mono text-[11px] leading-snug">
      <CircleAlert className="mt-0.5 size-3.5 shrink-0" />
      <span className="min-w-0 break-words">
        {message ?? "extraction failed"}
      </span>
    </div>
  );
}
