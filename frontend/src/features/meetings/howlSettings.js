//! In-browser settings for the mic howl guard, kept in `localStorage` like
//! the theme (`core/theme/ThemeContext.jsx`) -- per browser, no backend.
//! A tiny external store rather than a context, because `howlGuard.js`
//! reads it from a timer outside React.

import { useSyncExternalStore } from "react";

const STORAGE_KEY = "omoffice_howl_guard";
const DEFAULTS = { enabled: true };

function read() {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(STORAGE_KEY)) };
  } catch {
    return DEFAULTS;
  }
}

let settings = read();
const listeners = new Set();

export function getHowlSettings() {
  return settings;
}

export function setHowlSettings(patch) {
  settings = { ...settings, ...patch };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Best-effort -- the choice still applies for the rest of this tab.
  }
  listeners.forEach((listener) => listener());
}

function subscribe(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useHowlSettings() {
  return useSyncExternalStore(subscribe, getHowlSettings);
}
