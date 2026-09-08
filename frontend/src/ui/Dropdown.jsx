import { useEffect, useRef, useState } from "react";

/**
 * A popover anchored to its own trigger. `trigger` is rendered with
 * `{open, toggle}` and `children` with `close`, so a menu item can act and
 * dismiss without the caller tracking open state itself.
 */
export default function Dropdown({ trigger, align = "left", width = "w-56", children }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef(null);

  useEffect(() => {
    if (!open) return undefined;

    function onPointerDown(event) {
      if (!rootRef.current?.contains(event.target)) setOpen(false);
    }
    function onKeyDown(event) {
      if (event.key === "Escape") setOpen(false);
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      {trigger({ open, toggle: () => setOpen((current) => !current) })}
      {open && (
        <div
          className={`absolute z-30 mt-1 ${width} rounded-card border border-line bg-surface p-1 shadow-lg ${
            align === "right" ? "right-0" : "left-0"
          }`}
        >
          {children(() => setOpen(false))}
        </div>
      )}
    </div>
  );
}

/// One row in a dropdown. `tone="danger"` for the destructive one.
export function MenuItem({ tone = "default", className = "", ...rest }) {
  return (
    <button
      type="button"
      className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm transition-colors disabled:pointer-events-none disabled:opacity-50 ${
        tone === "danger"
          ? "text-danger hover:bg-danger/10"
          : "text-ink hover:bg-muted"
      } ${className}`}
      {...rest}
    />
  );
}
