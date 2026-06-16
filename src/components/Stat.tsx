import { cn } from "@/lib/utils";

/**
 * Instrument-cluster readout: a hairline top rule, a tiny mono uppercase
 * label, and a large mono tabular value. Lay these out in a grid for a
 * gauge-panel look.
 */
export function Stat({
  label,
  value,
  accent,
  className,
}: {
  label: string;
  value: React.ReactNode;
  accent?: boolean;
  className?: string;
}) {
  return (
    <div className={cn("border-border/70 flex flex-col gap-0.5 border-t pt-2", className)}>
      <div className="text-muted-foreground font-mono text-[10px] font-medium tracking-[0.14em] uppercase">
        {label}
      </div>
      <div
        className={cn(
          "truncate font-mono text-lg leading-tight font-medium tabular-nums",
          accent ? "text-signal" : "text-foreground",
        )}
      >
        {value}
      </div>
    </div>
  );
}
