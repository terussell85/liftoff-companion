import { useEffect, useRef, useState } from "react";
import { subscribe } from "./events";

export type ProcessingPhase = "idle" | "processing" | "done" | "failed";

export type ProcessingStatus = {
  phase: ProcessingPhase;
  processed: number;
};

/**
 * Tracks background processing jobs (manual or the auto-run on capture stop) by
 * subscribing to the `processing_*` events. The terminal "done"/"failed" states
 * auto-clear back to "idle" after a short delay so the indicator is transient.
 */
export function useProcessingStatus(): ProcessingStatus {
  const [status, setStatus] = useState<ProcessingStatus>({
    phase: "idle",
    processed: 0,
  });
  const clearTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const cancelTimer = () => {
      if (clearTimer.current) {
        clearTimeout(clearTimer.current);
        clearTimer.current = null;
      }
    };
    const armClear = (ms: number) => {
      cancelTimer();
      clearTimer.current = setTimeout(
        () => setStatus({ phase: "idle", processed: 0 }),
        ms,
      );
    };

    const ps: Promise<() => void>[] = [
      subscribe("processing_started", () => {
        cancelTimer();
        setStatus({ phase: "processing", processed: 0 });
      }),
      subscribe("processing_progress", (p) =>
        setStatus((s) =>
          s.phase === "processing"
            ? { phase: "processing", processed: p.processed_packets }
            : s,
        ),
      ),
      subscribe("processing_completed", () => {
        setStatus((s) => ({ phase: "done", processed: s.processed }));
        armClear(4000);
      }),
      subscribe("processing_failed", () => {
        setStatus({ phase: "failed", processed: 0 });
        armClear(6000);
      }),
    ];

    return () => {
      cancelTimer();
      for (const p of ps) p.then((u) => u()).catch(() => {});
    };
  }, []);

  return status;
}
