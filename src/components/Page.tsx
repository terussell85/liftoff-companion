import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Page scaffold: a fixed header region above the divider that stays put while
 * the body below it scrolls. The header and body share the same horizontal
 * padding so the divider lines up with the content. Content spans the full
 * width of the work area — no max-width gutter — to read like a native app.
 *
 * The `header` is optional — transient states (loading, error) can render
 * their content with no fixed header at all.
 */
export function Page({
  header,
  children,
  bodyClassName,
}: {
  header?: ReactNode;
  children: ReactNode;
  bodyClassName?: string;
}) {
  return (
    <div className="flex h-full flex-col">
      {header && (
        <div className="border-border/70 shrink-0 border-b px-8 pb-3 pt-5">
          {header}
        </div>
      )}
      <div
        className={cn(
          "flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-8 pb-7 [&>*]:shrink-0",
          header ? "pt-6" : "pt-7",
          bodyClassName,
        )}
      >
        {children}
      </div>
    </div>
  );
}
