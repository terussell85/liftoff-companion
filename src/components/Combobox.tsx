import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

/**
 * Type-to-search input with a suggestion dropdown. Built by hand rather than
 * with a native <datalist> because WKWebView (Tauri's macOS webview) doesn't
 * render datalist popups. The list is portalled to <body> so the parent
 * Panel's overflow:hidden can't clip it. Free text is always allowed.
 */
export function Combobox({
  id,
  value,
  onChange,
  options,
  placeholder,
  className,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
  options: string[];
  placeholder?: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [rect, setRect] = useState<{
    left: number;
    top: number;
    width: number;
  } | null>(null);
  const wrapRef = useRef<HTMLDivElement>(null);

  const q = value.trim().toLowerCase();
  const filtered = q
    ? options.filter((o) => o.toLowerCase().includes(q))
    : options;
  // Don't bother showing a single option that exactly equals the input.
  const meaningful =
    filtered.length > 0 &&
    !(filtered.length === 1 && filtered[0].toLowerCase() === q);
  const show = open && meaningful;

  const place = () => {
    const el = wrapRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setRect({ left: r.left, top: r.bottom + 4, width: r.width });
  };

  useLayoutEffect(() => {
    if (open) place();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onMove = () => place();
    window.addEventListener("scroll", onMove, true);
    window.addEventListener("resize", onMove);
    return () => {
      window.removeEventListener("scroll", onMove, true);
      window.removeEventListener("resize", onMove);
    };
  }, [open]);

  useEffect(() => {
    setActive(0);
  }, [value, open]);

  const select = (opt: string) => {
    onChange(opt);
    setOpen(false);
  };

  return (
    <div ref={wrapRef}>
      <Input
        id={id}
        value={value}
        placeholder={placeholder}
        autoComplete="off"
        className={className}
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            if (!open) setOpen(true);
            else setActive((a) => Math.min(a + 1, filtered.length - 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setActive((a) => Math.max(a - 1, 0));
          } else if (e.key === "Enter") {
            if (show && filtered[active]) {
              e.preventDefault();
              select(filtered[active]);
            }
          } else if (e.key === "Escape") {
            setOpen(false);
          }
        }}
      />
      {show &&
        rect &&
        createPortal(
          <ul
            role="listbox"
            className="bg-popover border-border z-50 max-h-56 overflow-y-auto rounded-md border p-1 font-mono text-sm shadow-lg"
            style={{
              position: "fixed",
              left: rect.left,
              top: rect.top,
              width: rect.width,
            }}
          >
            {filtered.map((opt, i) => (
              <li
                key={opt}
                role="option"
                aria-selected={i === active}
                // preventDefault keeps focus on the input so onBlur doesn't
                // close the list before the click registers.
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setActive(i)}
                onClick={() => select(opt)}
                className={cn(
                  "cursor-pointer rounded-sm px-2 py-1.5",
                  i === active
                    ? "bg-accent text-accent-foreground"
                    : "text-foreground",
                )}
              >
                {opt}
              </li>
            ))}
          </ul>,
          document.body,
        )}
    </div>
  );
}
