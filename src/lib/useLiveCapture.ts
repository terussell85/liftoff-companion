import { useEffect, useState } from "react";
import { api } from "./api";
import { subscribe } from "./events";
import type { CaptureStats } from "./types";

/**
 * Tracks active capture stats by subscribing to capture events.
 * Multiple listeners on the same Tauri event are fine, so the Capture page
 * and the global status bar can both use this independently.
 */
export function useLiveCapture() {
  const [stats, setStats] = useState<CaptureStats | null>(null);

  useEffect(() => {
    const ps: Promise<() => void>[] = [];
    ps.push(subscribe("capture_stats_updated", (s) => setStats(s)));
    ps.push(
      subscribe("capture_stopped", () =>
        setStats((s) => (s ? { ...s, status: "completed" } : s)),
      ),
    );
    api.currentCapture().then((s) => {
      if (s) setStats(s);
    });
    return () => {
      for (const p of ps) p.then((u) => u()).catch(() => {});
    };
  }, []);

  const recording = stats?.status === "recording";
  return { stats, recording };
}
