import { useEffect, useState } from "react";
import { Check, Loader2, Settings, TriangleAlert } from "lucide-react";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";
import { subscribe } from "@/lib/events";
import {
  useProcessingStatus,
  type ProcessingStatus,
} from "@/lib/useProcessingStatus";
import type { AutoCaptureState, CaptureStats } from "@/lib/types";

const APP_VERSION = "0.1.0";

export function StatusBar({
  stats,
  recording,
  refreshKey,
  settingsActive,
  onOpenSetup,
}: {
  stats: CaptureStats | null;
  recording: boolean;
  refreshKey: string;
  settingsActive: boolean;
  onOpenSetup: () => void;
}) {
  const [endpoint, setEndpoint] = useState<string>("—");
  const [configured, setConfigured] = useState<boolean | null>(null);
  const [auto, setAuto] = useState<AutoCaptureState | null>(null);
  const [autoBusy, setAutoBusy] = useState(false);
  const processing = useProcessingStatus();

  useEffect(() => {
    api
      .getSetupSnapshot()
      .then((s) => {
        setEndpoint(`${s.udp_bind_addr}:${s.udp_port}`);
        setConfigured(
          !!(s.config_status?.exists && s.config_status.matches_canonical),
        );
      })
      .catch(() => {});
  }, [refreshKey]);

  useEffect(() => {
    const unlisten = subscribe("auto_capture_state", setAuto);
    api.getAutoCapture().then(setAuto).catch(() => {});
    return () => {
      unlisten.then((u) => u()).catch(() => {});
    };
  }, []);

  const armed = !recording && auto?.enabled && auto.phase === "armed";
  const autoEnabled = auto?.enabled ?? false;

  const onToggleAuto = async () => {
    if (recording || autoBusy) return;
    setAutoBusy(true);
    try {
      setAuto(await api.setAutoCapture(!autoEnabled));
    } catch {
      // Keep the status pill usable even if the backend rejects a transient toggle.
    } finally {
      setAutoBusy(false);
    }
  };

  return (
    <footer className="border-border/80 bg-card/60 text-muted-foreground relative z-10 flex h-7 shrink-0 items-center gap-3 border-t px-3 font-mono text-[11px] backdrop-blur">
      <button
        type="button"
        onClick={onToggleAuto}
        disabled={recording || autoBusy}
        className={cn(
          "flex items-center gap-1.5 tracking-wide transition-colors",
          recording || autoBusy
            ? "cursor-default"
            : "hover:text-foreground cursor-pointer",
        )}
        title={
          recording
            ? "Recording is active"
            : autoEnabled
              ? "Disarm capture"
              : "Arm capture"
        }
        aria-label={autoEnabled ? "Disarm capture" : "Arm capture"}
      >
        <span className="relative inline-flex size-1.5 items-center justify-center">
          {(recording || armed) && (
            <span
              className={cn(
                "absolute inline-flex size-1.5 animate-ping rounded-full opacity-75",
                recording ? "bg-destructive" : "bg-signal",
              )}
              style={armed ? { animationDuration: "2.4s" } : undefined}
            />
          )}
          <span
            className={cn(
              "relative inline-flex size-1.5 rounded-full",
              recording
                ? "bg-destructive"
                : armed
                  ? "bg-signal"
                  : "bg-muted-foreground/50",
            )}
          />
        </span>
        <span
          className={cn(
            "tracking-wide",
            recording
              ? "text-destructive font-medium"
              : armed
                ? "text-signal font-medium"
                : autoBusy
                  ? "text-foreground/80"
                : undefined,
          )}
        >
          {recording ? "REC" : armed ? "ARMED" : autoBusy ? "..." : "IDLE"}
        </span>
        {recording && stats && (
          <span className="text-foreground/80 tabular-nums">
            {stats.duration_seconds.toFixed(1)}s
          </span>
        )}
      </button>

      {processing.phase !== "idle" && (
        <>
          <Divider />
          <ProcessingChip status={processing} />
        </>
      )}

      <Divider />
      <button
        type="button"
        onClick={onOpenSetup}
        className="hover:text-foreground flex items-center gap-1.5 tracking-wide transition-colors"
        title="Telemetry configuration"
      >
        <span
          className={
            "size-1.5 rounded-full " +
            (configured === null
              ? "bg-muted-foreground/40"
              : configured
                ? "bg-signal"
                : "bg-warn")
          }
        />
        {configured === null ? "LINK" : configured ? "LINKED" : "UNLINKED"}
      </button>

      <Divider />
      <span className="tabular-nums">{endpoint}</span>

      {stats && (
        <>
          <Divider />
          <span className="tabular-nums">
            {stats.packet_count.toLocaleString()} pkts
          </span>
          <Divider />
          <span className="tabular-nums">
            {stats.packet_rate_hz.toFixed(1)} Hz
          </span>
        </>
      )}

      <span className="ml-auto flex items-center gap-2.5">
        <span className="text-signal/70">●</span>
        <span className="tracking-wide">LIFTOFF COMPANION v{APP_VERSION}</span>
        <Divider />
        <button
          type="button"
          onClick={onOpenSetup}
          title="Setup"
          aria-label="Setup"
          className={cn(
            "inline-flex size-4 items-center justify-center transition-colors",
            settingsActive
              ? "text-signal"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          <Settings className="size-3.5" />
        </button>
      </span>
    </footer>
  );
}

function ProcessingChip({ status }: { status: ProcessingStatus }) {
  if (status.phase === "processing") {
    return (
      <span className="text-foreground/80 flex items-center gap-1.5 tracking-wide">
        <Loader2 className="size-3 animate-spin" />
        PROCESSING
        {status.processed > 0 && (
          <span className="tabular-nums">
            {status.processed.toLocaleString()}
          </span>
        )}
      </span>
    );
  }
  if (status.phase === "done") {
    return (
      <span className="text-signal flex items-center gap-1.5 font-medium tracking-wide">
        <Check className="size-3" />
        PROCESSED
      </span>
    );
  }
  return (
    <span className="text-destructive flex items-center gap-1.5 font-medium tracking-wide">
      <TriangleAlert className="size-3" />
      PROCESS FAILED
    </span>
  );
}

function Divider() {
  return <span className="bg-border h-3 w-px" aria-hidden />;
}
