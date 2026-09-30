//! Captures `console.*` output and uncaught errors into a ring buffer so
//! the mobile debug panel (`ui/DebugPanel.jsx`) can show them -- a phone has
//! no devtools. Imported first thing by `main.jsx` so nothing logged during
//! startup is missed; the original console methods still run. Whether the
//! panel is *shown* is a separate per-browser setting kept in `localStorage`.

import { useSyncExternalStore } from "react";

const MAX_ENTRIES = 300;
const STORAGE_KEY = "omoffice_debug_panel";
const LEVELS = ["log", "info", "warn", "error", "debug"];

let entries = [];
const listeners = new Set();
const notify = () => listeners.forEach((listener) => listener());

function format(arg) {
  if (typeof arg === "string") return arg;
  if (arg instanceof Error) return arg.stack || arg.message;
  try {
    return JSON.stringify(arg) ?? String(arg);
  } catch {
    return String(arg);
  }
}

function push(level, args) {
  // A new array each time: `useSyncExternalStore` needs a fresh snapshot.
  entries = [
    ...entries.slice(-(MAX_ENTRIES - 1)),
    {
      id: (entries.at(-1)?.id ?? 0) + 1,
      level,
      text: args.map(format).join(" "),
    },
  ];
  notify();
}

for (const level of LEVELS) {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    original(...args);
    push(level, args);
  };
}

window.addEventListener("error", (event) =>
  push("error", [event.error ?? event.message]),
);
window.addEventListener("unhandledrejection", (event) =>
  push("error", ["Unhandled rejection:", event.reason]),
);

export function clearDebugLog() {
  entries = [];
  notify();
}

function subscribe(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useDebugLog() {
  return useSyncExternalStore(subscribe, () => entries);
}

// The show/hide setting, same tiny-store shape.
function readShown() {
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

let shown = readShown();

export function setDebugPanelShown(next) {
  shown = next;
  try {
    localStorage.setItem(STORAGE_KEY, next ? "1" : "0");
  } catch {
    // Best-effort -- the choice still applies for the rest of this tab.
  }
  notify();
}

export function useDebugPanelShown() {
  return useSyncExternalStore(subscribe, () => shown);
}
