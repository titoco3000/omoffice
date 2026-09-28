//! Three-state theme (system/light/dark), applied as a `.dark` class on
//! `<html>` for `index.css`'s `@custom-variant dark` to key off. "system"
//! tracks `prefers-color-scheme` live rather than resolving it once, so a
//! stored preference of "system" keeps following the OS after the fact.

import { createContext, useContext, useEffect, useState } from "react";

const STORAGE_KEY = "omoffice_theme";
const THEMES = ["system", "light", "dark"];
const ThemeContext = createContext(null);

function readStoredTheme() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return THEMES.includes(stored) ? stored : "system";
  } catch {
    return "system";
  }
}

function prefersDark() {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function ThemeProvider({ children }) {
  const [theme, setTheme] = useState(readStoredTheme);
  const [systemDark, setSystemDark] = useState(prefersDark);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (event) => setSystemDark(event.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const dark = theme === "dark" || (theme === "system" && systemDark);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);

  const changeTheme = (next) => {
    setTheme(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Best-effort -- the choice still applies for the rest of this tab.
    }
  };

  return (
    <ThemeContext.Provider value={{ theme, dark, setTheme: changeTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}
