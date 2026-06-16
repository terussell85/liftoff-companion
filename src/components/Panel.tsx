import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Instrument panel: hairline-bordered surface with a mono uppercase title bar.
 * Denser than a default card — reads like a console module, not a web card.
 */
export function Panel({
  title,
  action,
  children,
  className,
  bodyClassName,
}: {
  title?: string;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section
      data-slot="card"
      className={cn(
        "bg-card border-border overflow-hidden rounded-lg border",
        className,
      )}
    >
      {(title || action) && (
        <header className="border-border/70 flex min-h-[40px] items-center justify-between gap-3 border-b px-4 py-2">
          {title && (
            <h2 className="text-muted-foreground font-mono text-[11px] font-medium tracking-[0.16em] uppercase">
              {title}
            </h2>
          )}
          {action}
        </header>
      )}
      <div className={cn("p-4", bodyClassName)}>{children}</div>
    </section>
  );
}
