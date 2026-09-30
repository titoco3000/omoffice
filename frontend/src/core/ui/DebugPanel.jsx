//! Console output on screen, for phones. Never blocks the app: the log is
//! `pointer-events-none`, so taps go through to whatever is underneath. Shows only on mobile and only while
//! the "Debug panel" setting is on (see `core/debugLog.js`). Mounted once by
//! `App`.

import { useEffect, useRef, useState } from "react";
import {
  clearDebugLog,
  setDebugPanelShown,
  useDebugLog,
  useDebugPanelShown,
} from "../debugLog";
import { useIsMobile } from "../hooks/useViewport";

const LEVEL_CLASS = {
  error: "text-danger",
  warn: "text-amber-500",
};

export default function DebugPanel() {
  const isMobile = useIsMobile();
  const shown = useDebugPanelShown();
  const entries = useDebugLog();
  const listRef = useRef(null);
  const [collapsed, setCollapsed] = useState(false);
  const visible = isMobile && shown;

  useEffect(() => {
    const list = listRef.current;
    if (list) list.scrollTop = list.scrollHeight;
  }, [entries, visible]);

  if (!visible) return null;
  // The log itself is see-through to touches (`pointer-events-none`), so
  // everything under it stays usable; only the small button chip takes taps.
  return (
    <div className="pointer-events-none fixed inset-x-0 top-0 z-[80]">
      {!collapsed && (
        <ul
          ref={listRef}
          className="h-1/4 overflow-hidden bg-surface/75 p-1 font-mono text-[10px] leading-tight text-ink"
        >
          {entries.slice(-40).map((entry) => (
            <li
              key={entry.id}
              className={`whitespace-pre-wrap break-all ${LEVEL_CLASS[entry.level] ?? ""}`}
            >
              {entry.text}
            </li>
          ))}
        </ul>
      )}
      <div className="pointer-events-auto absolute right-1 top-1 flex gap-3 rounded-card border bg-surface px-2 py-1 text-xs text-ink-muted shadow">
        <button onClick={() => setCollapsed(!collapsed)}>
          {collapsed ? `Show (${entries.length})` : "Hide"}
        </button>
        {!collapsed && <button onClick={clearDebugLog}>Clear</button>}
        <button onClick={() => setDebugPanelShown(false)}>Close</button>
      </div>
    </div>
  );
}
