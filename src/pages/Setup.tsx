import { useCallback, useEffect, useState } from "react";
import {
  CircleAlert,
  Database,
  Loader2,
  Pencil,
  Radio,
  RefreshCw,
} from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Page } from "@/components/Page";
import { PageHeader } from "@/components/PageHeader";
import { Panel } from "@/components/Panel";
import { Stat } from "@/components/Stat";
import { InstallCard } from "@/components/InstallCard";
import type { InstallNotice, InstallView } from "@/components/InstallCard";
import { api } from "@/lib/api";
import { subscribe } from "@/lib/events";
import type {
  AssetRefreshProgress,
  GameAssetSourceStatus,
  SetupSnapshot,
  TestListenerResult,
} from "@/lib/types";

export function SetupView() {
  const [snapshot, setSnapshot] = useState<SetupSnapshot | null>(null);
  const [assetSources, setAssetSources] = useState<GameAssetSourceStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [bindAddr, setBindAddr] = useState("");
  const [port, setPort] = useState<number>(9001);
  const [editingEndpoint, setEditingEndpoint] = useState(false);
  const [savingEndpoint, setSavingEndpoint] = useState(false);
  const [testResult, setTestResult] = useState<TestListenerResult | null>(null);
  const [testing, setTesting] = useState(false);

  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [notices, setNotices] = useState<Record<string, InstallNotice>>({});
  const [assetProgress, setAssetProgress] =
    useState<AssetRefreshProgress | null>(null);

  const refreshSnapshot = useCallback(async () => {
    const snap = await api.getSetupSnapshot();
    setSnapshot(snap);
    if (!editingEndpoint) {
      setBindAddr(snap.udp_bind_addr);
      setPort(snap.udp_port);
    }
    return snap;
  }, [editingEndpoint]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [snap, sources] = await Promise.all([
        api.getSetupSnapshot(),
        api.listGameAssetSources(),
      ]);
      setSnapshot(snap);
      setAssetSources(sources);
      setBindAddr(snap.udp_bind_addr);
      setPort(snap.udp_port);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const subscriptions: Promise<() => void>[] = [
      subscribe("asset_refresh_started", (p) => setAssetProgress(p)),
      subscribe("asset_refresh_progress", (p) => setAssetProgress(p)),
      subscribe("asset_refresh_completed", (sources) => {
        setAssetSources(sources);
        setAssetProgress(null);
      }),
      subscribe("asset_refresh_failed", () => setAssetProgress(null)),
    ];
    return () => {
      for (const subscription of subscriptions) {
        subscription.then((unsubscribe) => unsubscribe()).catch(() => {});
      }
    };
  }, []);

  const showNotice = useCallback((key: string, notice: InstallNotice) => {
    setNotices((prev) => ({ ...prev, [key]: notice }));
    setTimeout(() => {
      setNotices((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    }, 6000);
  }, []);

  const saveNetwork = useCallback(async () => {
    setSavingEndpoint(true);
    setError(null);
    try {
      const snap = await api.updateNetworkConfig(bindAddr, port);
      setSnapshot(snap);
      setBindAddr(snap.udp_bind_addr);
      setPort(snap.udp_port);
      setEditingEndpoint(false);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setSavingEndpoint(false);
    }
  }, [bindAddr, port]);

  const runTest = useCallback(async () => {
    setTesting(true);
    setError(null);
    setTestResult(null);
    try {
      setTestResult(await api.runTestListener(5));
    } catch (e) {
      setError(formatError(e));
    } finally {
      setTesting(false);
    }
  }, []);

  const setTracking = useCallback(
    async (install: InstallView, next: boolean) => {
      if (!install.configPath) return;
      setBusyKey(install.key);
      setError(null);
      try {
        if (next) {
          await api.applyRecommendedTelemetryConfig(install.configPath);
          showNotice(install.key, { kind: "enabled" });
        } else {
          await api.disableTelemetryConfig(install.configPath);
          showNotice(install.key, { kind: "stopped" });
        }
        await refreshSnapshot();
      } catch (e) {
        setError(formatError(e));
      } finally {
        setBusyKey(null);
      }
    },
    [refreshSnapshot, showNotice],
  );

  const reapply = useCallback(
    async (install: InstallView) => {
      if (!install.configPath) return;
      setBusyKey(install.key);
      setError(null);
      try {
        await api.applyRecommendedTelemetryConfig(install.configPath);
        showNotice(install.key, { kind: "enabled" });
        await refreshSnapshot();
      } catch (e) {
        setError(formatError(e));
      } finally {
        setBusyKey(null);
      }
    },
    [refreshSnapshot, showNotice],
  );

  const resync = useCallback(async (install: InstallView) => {
    if (!install.dataRoot) return;
    setError(null);
    setAssetProgress(queuedProgress(install.dataRoot));
    try {
      const sources = await api.refreshRaceTrackCache(true, install.dataRoot);
      setAssetSources(sources);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setAssetProgress(null);
    }
  }, []);

  const resyncAll = useCallback(async () => {
    setError(null);
    setAssetProgress(queuedProgress(null));
    try {
      const sources = await api.refreshRaceTrackCache(true);
      setAssetSources(sources);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setAssetProgress(null);
    }
  }, []);

  const installs = snapshot ? buildInstalls(snapshot, assetSources) : [];
  const linked = snapshot?.dirs.some((d) => d.matches_canonical === true) ?? false;
  const refreshing = assetProgress !== null;
  const progressKey =
    assetProgress && (assetProgress.game_title || assetProgress.data_root)
      ? bucketOf(assetProgress.game_title ?? "", assetProgress.data_root ?? "")
      : null;

  return (
    <Page
      header={
        <PageHeader
          eyebrow="Settings"
          title="Telemetry & game data"
          subtitle="Listen on one endpoint, choose which installs stream to it, and keep their race data in sync."
          actions={
            <span
              className={
                "inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 font-mono text-[11px] tracking-wide " +
                (linked
                  ? "border-signal/40 bg-signal/10 text-signal"
                  : "border-border bg-muted/40 text-muted-foreground")
              }
            >
              <span
                className={
                  "size-1.5 rounded-full " +
                  (linked ? "bg-signal" : "bg-muted-foreground/50")
                }
              />
              {linked ? "LINKED" : "NOT LINKED"}
            </span>
          }
        />
      }
    >
      {error && (
        <Alert variant="destructive">
          <CircleAlert />
          <AlertTitle>Setup error</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {/* one-time telemetry endpoint */}
      <Panel
        title="Telemetry endpoint"
        action={
          <div className="flex items-center gap-2">
            {!editingEndpoint && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setEditingEndpoint(true)}
              >
                <Pencil className="size-3.5" />
                Edit endpoint
              </Button>
            )}
            <Button size="sm" variant="outline" disabled={testing} onClick={runTest}>
              {testing ? (
                <>
                  <Loader2 className="size-3.5 animate-spin" />
                  Listening
                </>
              ) : (
                <>
                  <Radio className="size-3.5" />
                  Probe 5s
                </>
              )}
            </Button>
          </div>
        }
      >
        {editingEndpoint ? (
          <div className="flex flex-wrap items-end gap-3">
            <div className="flex flex-col gap-1.5">
              <Label
                htmlFor="bind-addr"
                className="font-mono text-[11px] tracking-wide uppercase"
              >
                Bind addr
              </Label>
              <Input
                id="bind-addr"
                value={bindAddr}
                onChange={(e) => setBindAddr(e.target.value)}
                placeholder="127.0.0.1"
                className="w-36 font-mono"
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label
                htmlFor="port"
                className="font-mono text-[11px] tracking-wide uppercase"
              >
                Port
              </Label>
              <Input
                id="port"
                type="number"
                value={port}
                min={1}
                max={65535}
                onChange={(e) => setPort(parseInt(e.target.value, 10) || 9001)}
                className="w-24 font-mono"
              />
            </div>
            <Button disabled={savingEndpoint} onClick={saveNetwork}>
              {savingEndpoint && <Loader2 className="size-3.5 animate-spin" />}
              Save & probe
            </Button>
            <Button
              variant="ghost"
              disabled={savingEndpoint}
              onClick={() => {
                setEditingEndpoint(false);
                if (snapshot) {
                  setBindAddr(snapshot.udp_bind_addr);
                  setPort(snapshot.udp_port);
                }
              }}
            >
              Cancel
            </Button>
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
            <div className="flex items-center gap-2.5">
              <span className="text-muted-foreground font-mono text-[10px] tracking-[0.16em] uppercase">
                Listening
              </span>
              <span className="font-mono text-base font-semibold">
                {bindAddr || "127.0.0.1"}
                <span className="text-muted-foreground">:</span>
                {port}
              </span>
            </div>
            {testResult && (
              <>
                <span className="bg-border h-6 w-px" />
                <Stat
                  label="Packets"
                  value={testResult.packet_count.toLocaleString()}
                  accent={testResult.packet_count > 0}
                  className="border-t-0 pt-0"
                />
                <Stat
                  label="Rate"
                  value={`${testResult.packet_rate_hz.toFixed(1)} Hz`}
                  className="border-t-0 pt-0"
                />
                <Stat
                  label="Source"
                  value={testResult.last_source_addr ?? "—"}
                  className="border-t-0 pt-0"
                />
              </>
            )}
            {!testResult && (
              <span className="text-muted-foreground/60 font-mono text-xs">
                probe to confirm packets are arriving
              </span>
            )}
          </div>
        )}
      </Panel>

      {/* game installs */}
      <Panel
        title="Game installs"
        action={
          <Button
            size="sm"
            variant="outline"
            disabled={refreshing}
            onClick={resyncAll}
          >
            {refreshing ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <RefreshCw className="size-3.5" />
            )}
            Re-sync all
          </Button>
        }
      >
        <div className="flex flex-col gap-4">
          <p className="text-muted-foreground text-sm leading-relaxed">
            Toggle an install to stream its telemetry to this app. Race data
            powers the 3D replay (gates, collisions) and is read from your local
            install — re-sync after a Liftoff or Companion update.
          </p>

          {loading ? (
            <div className="text-muted-foreground flex items-center gap-2 py-2 font-mono text-sm">
              <Loader2 className="size-4 animate-spin" />
              scanning for liftoff…
            </div>
          ) : installs.length > 0 ? (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(340px,1fr))] gap-3">
              {installs.map((install) => (
                <InstallCard
                  key={install.key}
                  install={install}
                  progress={progressKey === install.key ? assetProgress : null}
                  notice={notices[install.key] ?? null}
                  busy={busyKey === install.key}
                  onToggle={setTracking}
                  onReapply={reapply}
                  onResync={resync}
                />
              ))}
            </div>
          ) : (
            <div className="text-muted-foreground/60 flex items-center gap-2 font-mono text-xs">
              <Database className="size-3.5" />
              no Liftoff installs detected — launch Liftoff once
            </div>
          )}
        </div>
      </Panel>
    </Page>
  );
}

// ---- install join -------------------------------------------------------

const TITLES: Record<"base" | "micro", string> = {
  base: "Liftoff",
  micro: "Liftoff Micro Drones",
};

/** Mirrors the backend `normalize_key`: ascii-alphanumeric, lowercased. */
function normalizeKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

/** Bucket any source's identifying strings into one of the two canonical
 * installs, matching the backend's "contains microdrones" rule. */
function bucketOf(...parts: (string | null | undefined)[]): "base" | "micro" {
  const hay = normalizeKey(parts.filter(Boolean).join(" "));
  return hay.includes("microdrones") ? "micro" : "base";
}

function badgeFor(path: string | null | undefined): string | null {
  if (!path) return null;
  if (path.endsWith(".app")) return ".app";
  if (/steamapps|\/steam\//i.test(path)) return "Steam";
  return null;
}

function buildInstalls(
  snapshot: SetupSnapshot,
  sources: GameAssetSourceStatus[],
): InstallView[] {
  const dirRank = (d: SetupSnapshot["dirs"][number]) =>
    (d.config_exists ? 2 : 0) + (d.exists ? 1 : 0);

  const dirByBucket = new Map<string, SetupSnapshot["dirs"][number]>();
  for (const d of snapshot.dirs) {
    const k = bucketOf(d.label, d.path);
    const cur = dirByBucket.get(k);
    if (!cur || dirRank(d) > dirRank(cur)) dirByBucket.set(k, d);
  }

  const srcByBucket = new Map<string, GameAssetSourceStatus>();
  for (const s of sources) {
    const k = bucketOf(s.game_title, s.label, s.data_root);
    const cur = srcByBucket.get(k);
    if (!cur || (s.valid && !cur.valid)) srcByBucket.set(k, s);
  }

  const logByBucket = new Map<string, SetupSnapshot["player_logs"][number]>();
  for (const l of snapshot.player_logs) {
    const k = bucketOf(l.title, l.path);
    if (l.exists || !logByBucket.has(k)) logByBucket.set(k, l);
  }

  const order: ("base" | "micro")[] = ["base", "micro"];
  return order
    .filter(
      (k) => (dirByBucket.get(k)?.exists ?? false) || srcByBucket.has(k),
    )
    .map((k) => {
      const d = dirByBucket.get(k);
      const s = srcByBucket.get(k);
      const l = logByBucket.get(k);
      return {
        key: k,
        title: TITLES[k],
        sourceBadge: badgeFor(s?.data_root ?? d?.path),
        detected: (d?.exists ?? false) || (s?.valid ?? false),
        configPath: d?.config_path ?? null,
        configExists: d?.config_exists ?? false,
        matchesCanonical: d?.matches_canonical ?? null,
        dataRoot: s?.data_root ?? null,
        cacheStatus: s?.cache_status ?? null,
        raceCount: s?.race_count ?? 0,
        trackCount: s?.track_count ?? 0,
        extractedAt: s?.extracted_at ?? null,
        errorMessage: s?.error_message ?? null,
        logFound: l?.exists ?? false,
        logPath: l?.path ?? null,
      };
    });
}

function queuedProgress(dataRoot: string | null): AssetRefreshProgress {
  return {
    phase: "queued",
    message: "Waiting for the asset refresh worker.",
    game_title: null,
    data_root: dataRoot,
    sources_done: 0,
    sources_total: 0,
    scopes_done: 0,
    scopes_total: 0,
    levels_done: 0,
    levels_total: 0,
    bundles_done: 0,
    bundles_total: 0,
    current_scope: null,
    current_level: null,
    current_bundle: null,
    races_found: 0,
    tracks_found: 0,
    geometry_ready: 0,
    geometry_partial: 0,
    geometry_missing: 0,
    geometry_shapes: 0,
  };
}

function formatError(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message?: unknown }).message ?? e);
  }
  return String(e);
}
