import { createContext, useCallback, useContext, useMemo, useState } from "react";

const MeetingContext = createContext(null);

/**
 * The one meeting a session is in, held above the router so navigating away
 * from `/meeting/{code}` does not leave it.
 *
 * The session itself lives in `MeetingDock`, mounted once at the app root;
 * this only holds which code it is on and where on screen it should sit.
 * `slot` is the element `MeetingPage` renders as its stage: while one is
 * registered the dock covers it, and with none it shrinks to a corner.
 */
export function MeetingProvider({ children }) {
  const [code, setCode] = useState("");
  const [slot, setSlot] = useState(null);

  const join = useCallback((wanted) => setCode(wanted ?? ""), []);
  const leave = useCallback(() => setCode(""), []);

  const value = useMemo(
    () => ({ code, join, leave, slot, setSlot }),
    [code, join, leave, slot],
  );

  return <MeetingContext.Provider value={value}>{children}</MeetingContext.Provider>;
}

export function useActiveMeeting() {
  const context = useContext(MeetingContext);
  if (!context) {
    throw new Error("useActiveMeeting must be used inside a MeetingProvider");
  }
  return context;
}
