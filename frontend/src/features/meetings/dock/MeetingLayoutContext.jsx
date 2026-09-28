//! Whether the mobile meeting dock is at its half-screen size or expanded
//! to fullscreen. Shared state -- the toggle button lives in `NavContainer`'s
//! bottom bar, the dock it controls is a sibling of it under `AppShell`.

import { createContext, useContext, useState } from "react";

const MeetingLayoutContext = createContext(null);

export function MeetingLayoutProvider({ children }) {
  const [mobileExpanded, setMobileExpanded] = useState(false);
  const toggleMobileExpanded = () => setMobileExpanded((v) => !v);

  return (
    <MeetingLayoutContext.Provider
      value={{ mobileExpanded, toggleMobileExpanded }}
    >
      {children}
    </MeetingLayoutContext.Provider>
  );
}

export function useMeetingLayout() {
  return useContext(MeetingLayoutContext);
}
