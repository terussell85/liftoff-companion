import { useState } from "react";
import { Activity, Flag, FolderClock, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useLiveCapture } from "@/lib/useLiveCapture";
import { StatusBar } from "@/components/StatusBar";
import { SetupView } from "./pages/Setup";
import { RacesView } from "./pages/Races";
import { RaceDetailView } from "./pages/RaceDetail";
import { SessionsView } from "./pages/Sessions";
import { CaptureDetailView } from "./pages/CaptureDetail";
import { ProcessingView } from "./pages/Processing";
import { DatasetSummaryView } from "./pages/DatasetSummary";
import { FlightPathView } from "./pages/FlightPath";

export type View =
  | { kind: "setup" }
  | { kind: "races" }
  | { kind: "race-detail"; captureId: string; sessionId: string }
  | { kind: "sessions" }
  | { kind: "capture-detail"; captureId: string }
  | { kind: "process"; captureId: string }
  | { kind: "dataset"; datasetId: string }
  | { kind: "flight"; datasetId: string; sessionId: string };

type NavItem = {
  label: string;
  icon: LucideIcon;
  view: View;
};

// Races are the primary entity: the log you land on, everything else supports it.
const PRIMARY_NAV_ITEMS: NavItem[] = [
  {
    label: "Race Log",
    icon: Flag,
    view: { kind: "races" },
  },
];

const SECONDARY_NAV_ITEMS: NavItem[] = [
  {
    label: "Capture Log",
    icon: FolderClock,
    view: { kind: "sessions" },
  },
];

function App() {
  const [view, setView] = useState<View>({ kind: "races" });
  const { stats, recording } = useLiveCapture();

  return (
    <div className="relative z-10 flex h-screen flex-col overflow-hidden">
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <Sidebar view={view} onNavigate={setView} recording={recording} />
        <main className="min-h-0 min-w-0 flex-1 overflow-hidden">
          {view.kind === "flight" ? (
            <FlightPathView
              datasetId={view.datasetId}
              sessionId={view.sessionId}
              onNavigate={setView}
            />
          ) : (
            <>
              {view.kind === "setup" && <SetupView />}
              {view.kind === "races" && <RacesView onNavigate={setView} />}
              {view.kind === "race-detail" && (
                <RaceDetailView
                  captureId={view.captureId}
                  sessionId={view.sessionId}
                  onNavigate={setView}
                />
              )}
              {view.kind === "sessions" && (
                <SessionsView onNavigate={setView} />
              )}
              {view.kind === "capture-detail" && (
                <CaptureDetailView
                  captureId={view.captureId}
                  onNavigate={setView}
                />
              )}
              {view.kind === "process" && (
                <ProcessingView
                  captureId={view.captureId}
                  onNavigate={setView}
                />
              )}
              {view.kind === "dataset" && (
                <DatasetSummaryView
                  datasetId={view.datasetId}
                  onNavigate={setView}
                />
              )}
            </>
          )}
        </main>
      </div>
      <StatusBar
        stats={stats}
        recording={recording}
        refreshKey={view.kind}
        settingsActive={view.kind === "setup"}
        onOpenSetup={() => setView({ kind: "setup" })}
      />
    </div>
  );
}

/** Which nav section a view belongs to, so detail pages keep their parent lit. */
function navKindFor(view: View): View["kind"] {
  switch (view.kind) {
    case "race-detail":
    case "flight":
      return "races";
    case "capture-detail":
    case "process":
    case "dataset":
      return "sessions";
    default:
      return view.kind;
  }
}

function Sidebar({
  view,
  onNavigate,
  recording,
}: {
  view: View;
  onNavigate: (v: View) => void;
  recording: boolean;
}) {
  const renderNavButton = (item: NavItem, secondary = false) => {
    const Icon = item.icon;
    const active = navKindFor(view) === item.view.kind;

    return (
      <button
        key={item.label}
        type="button"
        onClick={() => onNavigate(item.view)}
        className={cn(
          "group relative flex items-center gap-2.5 rounded-md text-left transition-colors",
          secondary ? "px-2.5 py-1.5 text-xs" : "px-2.5 py-2 text-sm",
          active
            ? secondary
              ? "bg-accent/65 text-accent-foreground"
              : "bg-accent text-accent-foreground"
            : secondary
              ? "text-muted-foreground hover:bg-accent/45 hover:text-foreground"
              : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
        )}
      >
        {active && (
          <span
            className={cn(
              "absolute left-0 top-1/2 w-0.5 -translate-y-1/2 rounded-r",
              secondary ? "bg-signal/70 h-3" : "bg-signal h-4",
            )}
          />
        )}
        <Icon
          className={cn(
            "shrink-0",
            secondary ? "size-3.5" : "size-4",
            active
              ? secondary
                ? "text-signal/80"
                : "text-signal"
              : secondary
                ? "text-muted-foreground group-hover:text-foreground"
                : "text-muted-foreground group-hover:text-foreground",
          )}
        />
        <span className="flex flex-col leading-none">
          <span className="font-medium">
            {item.label}
          </span>
        </span>
      </button>
    );
  };

  return (
    <aside className="border-border/80 bg-card/40 flex w-52 shrink-0 flex-col border-r">
      <div className="border-border/70 flex h-14 items-center gap-2.5 border-b px-4">
        <div className="border-signal/40 bg-signal/10 text-signal relative flex size-8 items-center justify-center rounded-md border">
          <Activity className="size-4" />
          {recording && (
            <span className="border-card bg-destructive absolute -right-1 -top-1 size-2.5 rounded-full border" />
          )}
        </div>
        <div className="flex flex-col leading-none">
          <span className="text-sm font-semibold tracking-tight">Liftoff</span>
          <span className="text-muted-foreground font-mono text-[10px] tracking-[0.18em] uppercase">
            Companion
          </span>
        </div>
      </div>

      <nav className="flex flex-1 flex-col p-2">
        <div className="flex flex-col gap-0.5">
          <div className="text-muted-foreground/70 px-2 pb-1 pt-2 font-mono text-[10px] tracking-[0.2em] uppercase">
            Workspace
          </div>
          {PRIMARY_NAV_ITEMS.map((item) => renderNavButton(item))}
        </div>
        <div className="border-border/50 mt-auto flex flex-col gap-0.5 border-t pt-2">
          {SECONDARY_NAV_ITEMS.map((item) => renderNavButton(item, true))}
        </div>
      </nav>
    </aside>
  );
}

export default App;
