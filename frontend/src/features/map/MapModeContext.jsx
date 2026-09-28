//! Map mode: which layout `/in` uses -- "map" (a canvas fills the page,
//! roster moves to the sidebar/toggle) or "no-graphics" (the plain roster
//! layout). Unrelated to theme; it only borrows `ThemeContext`'s
//! localStorage-backed pattern. The storage key and mode values predate the
//! rename and are kept as-is so a saved choice survives.

import { createContext, useContext, useState } from "react";

const STORAGE_KEY = "omoffice_graphics_mode";
const MODES = ["map", "no-graphics"];
const MapModeContext = createContext(null);

function readStoredMode() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return MODES.includes(stored) ? stored : MODES[0];
  } catch {
    return MODES[0];
  }
}

export function MapModeProvider({ children }) {
  const [mode, setModeState] = useState(readStoredMode);

  const setMode = (next) => {
    setModeState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Best-effort -- the choice still applies for the rest of this tab.
    }
  };

  return (
    <MapModeContext.Provider value={{ mode, setMode }}>
      {children}
    </MapModeContext.Provider>
  );
}

export function useMapMode() {
  return useContext(MapModeContext);
}
