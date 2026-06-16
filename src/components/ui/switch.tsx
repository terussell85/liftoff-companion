import * as React from "react";
import * as SwitchPrimitive from "@radix-ui/react-switch";

import { cn } from "@/lib/utils";

/**
 * Avionics toggle: a phosphor-lime track when on, neutral cockpit grey when off.
 * Used for per-install telemetry tracking on the Setup page.
 */
function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        "peer inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border transition-colors outline-none",
        "focus-visible:ring-ring/50 focus-visible:ring-[3px]",
        "disabled:cursor-not-allowed disabled:opacity-50",
        "data-[state=checked]:border-transparent data-[state=checked]:bg-signal",
        "data-[state=unchecked]:border-border data-[state=unchecked]:bg-muted",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className={cn(
          "pointer-events-none block size-4 rounded-full shadow-sm ring-0 transition-transform",
          "data-[state=checked]:translate-x-4 data-[state=checked]:bg-primary-foreground",
          "data-[state=unchecked]:translate-x-0.5 data-[state=unchecked]:bg-muted-foreground",
        )}
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
