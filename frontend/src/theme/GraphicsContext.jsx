//! Graphics mode for `/in`: "map" (a canvas fills the page, roster moves to
//! the sidebar/toggle) or "no-graphics" (today's plain layout). Same
//! localStorage-backed pattern as `ThemeContext`.

import { createContext, useContext, useState } from "react";

const STORAGE_KEY = "omoffice_graphics_mode";
const MODES = ["map", "no-graphics"];
const GraphicsContext = createContext(null);

function readStoredMode() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return MODES.includes(stored) ? stored : MODES[0];
  } catch {
    return MODES[0];
  }
}

export function GraphicsProvider({ children }) {
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
    <GraphicsContext.Provider value={{ mode, setMode }}>
      {children}
    </GraphicsContext.Provider>
  );
}

export function useGraphics() {
  return useContext(GraphicsContext);
}
