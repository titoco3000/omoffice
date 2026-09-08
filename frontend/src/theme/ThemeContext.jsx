import { createContext, useContext, useEffect, useMemo, useState } from "react";

const STORAGE_KEY = "omoffice_theme";

/// The three states a person can pick. `system` is the default and follows
/// the OS; the other two pin it.
export const THEME_MODES = ["system", "light", "dark"];

const ThemeContext = createContext(null);

function loadStoredMode() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return THEME_MODES.includes(stored) ? stored : "system";
  } catch {
    return "system"; // private browsing, storage disabled, etc.
  }
}

function systemPrefersDark() {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

/**
 * Owns which palette `index.css` serves, by putting (or not putting) the
 * `dark` class on `<html>`. Nothing else in the app reads a color scheme.
 */
export function ThemeProvider({ children }) {
  const [mode, setMode] = useState(loadStoredMode);
  const [prefersDark, setPrefersDark] = useState(systemPrefersDark);

  // Only matters while the mode is `system`, but the listener is cheap and
  // unconditional here rather than resubscribed on every mode change.
  useEffect(() => {
    const query = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!query) return undefined;
    const update = (event) => setPrefersDark(event.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  const dark = mode === "dark" || (mode === "system" && prefersDark);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, mode);
    } catch {
      // The choice just won't survive a refresh.
    }
  }, [mode]);

  const value = useMemo(() => ({ mode, setMode, dark }), [mode, dark]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("useTheme must be used inside a ThemeProvider");
  return context;
}
