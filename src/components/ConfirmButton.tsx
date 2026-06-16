import { useEffect, useRef, useState } from "react";
import { Loader2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/**
 * Two-step destructive action: first click arms (label flips to a confirm
 * prompt), second click fires. Disarms itself after a beat so a stray click
 * never deletes anything. Keeps destructive flows inline — no modal.
 */
export function ConfirmButton({
  label = "Delete",
  confirmLabel = "Confirm delete",
  iconOnly = false,
  disabled = false,
  className,
  onConfirm,
}: {
  label?: string;
  confirmLabel?: string;
  iconOnly?: boolean;
  disabled?: boolean;
  className?: string;
  onConfirm: () => void | Promise<void>;
}) {
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const disarmTimer = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (disarmTimer.current != null) window.clearTimeout(disarmTimer.current);
    };
  }, []);

  const arm = () => {
    setArmed(true);
    if (disarmTimer.current != null) window.clearTimeout(disarmTimer.current);
    disarmTimer.current = window.setTimeout(() => setArmed(false), 3000);
  };

  const fire = async () => {
    if (disarmTimer.current != null) window.clearTimeout(disarmTimer.current);
    setBusy(true);
    try {
      await onConfirm();
    } finally {
      setBusy(false);
      setArmed(false);
    }
  };

  if (!armed) {
    return (
      <Button
        size={iconOnly ? "icon" : "sm"}
        variant="ghost"
        disabled={disabled || busy}
        onClick={arm}
        title={label}
        aria-label={label}
        className={cn(
          "text-muted-foreground hover:text-destructive",
          iconOnly && "size-8",
          className,
        )}
      >
        <Trash2 className="size-3.5" />
        {!iconOnly && label}
      </Button>
    );
  }

  return (
    <Button
      size="sm"
      variant="destructive"
      disabled={busy}
      onClick={fire}
      onBlur={() => !busy && setArmed(false)}
      className={cn("font-mono text-[11px] tracking-wide uppercase", className)}
    >
      {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Trash2 className="size-3.5" />}
      {confirmLabel}
    </Button>
  );
}
