//! The current org's meeting session -- `useMeetingSession` run once and
//! shared, so the dock, the roster's join/invite actions, the notifications
//! overlay and the map's proximity joins all see the same state. Nested
//! inside `OrgSessionProvider` (see `app/App.jsx`), whose current org it
//! follows; org itself knows nothing about meetings.

import { createContext, useContext } from "react";
import { useAuth } from "../../core/auth/AuthContext";
import { useOrgSession } from "../org";
import { useMeetingSession } from "./useMeetingSession";

const MeetingSessionContext = createContext(null);

export function MeetingSessionProvider({ children }) {
  const { token } = useAuth();
  const { currentOrg } = useOrgSession();
  const session = useMeetingSession(token, currentOrg?.id ?? null);

  return (
    <MeetingSessionContext.Provider value={session}>
      {children}
    </MeetingSessionContext.Provider>
  );
}

export function useMeetingSessionContext() {
  return useContext(MeetingSessionContext);
}
