//! Moves this tab's avatar when a meeting is joined or left by an explicit
//! click (the roster's Join, the dock's Leave -- `useMeetingSession`'s
//! `onIntent`), whichever layout the click came from. `relocation.js`
//! decides where.
//!
//! With the map open, `useMapSession` claims these
//! (`useMapRelocationClaim`) and walks there itself, on the live positions
//! it already has. Without one, this provider handles them on its own,
//! without following anyone's live position: it fetches the stored
//! positions once, picks the cell, stores it, and publishes that single
//! move (`publishMapMove`) so any open maps see it -- no socket, no
//! subscription, and no proximity check, so the move can't pull this tab
//! into a meeting either. Nested inside `MeetingSessionProvider` (see
//! `app/App.jsx`).

import { createContext, useContext, useEffect, useRef } from "react";
import { useAuth } from "../../core/auth/AuthContext";
import { useOrgSession } from "../org";
import { useMeetingSessionContext } from "../meetings";
import { getMapRealtime, listMapPositions, moveInMap } from "./api";
import { publishMapMove } from "./realtime";
import { facingBetween } from "./navigation";
import { relocationCell } from "./relocation";
import { isBlocked } from "./tileMap";
import { useTileMap } from "./MapEditorContext";

const MapRelocationContext = createContext(null);

export function MapRelocationProvider({ children }) {
  const { token } = useAuth();
  const { myId, currentOrg, roster } = useOrgSession();
  const session = useMeetingSessionContext();
  const tileMap = useTileMap();
  const claimRef = useRef(null); // the open map's handler, if any

  const latestRef = useRef(null);
  useEffect(() => {
    latestRef.current = { token, myId, currentOrg, roster, session, tileMap };
  });

  const { onIntent } = session;
  useEffect(
    () =>
      onIntent(async (intent) => {
        if (claimRef.current) return claimRef.current(intent);
        const { token, myId, currentOrg, roster, session, tileMap } =
          latestRef.current;
        const orgId = currentOrg?.id;
        if (!orgId) return;
        try {
          const data = await listMapPositions(token, orgId);
          const positions = {};
          for (const position of data.positions ?? [])
            positions[position.user_id] = { x: position.x, y: position.y };
          const from = positions[myId] ?? { x: 0, y: 0 };
          const cell = relocationCell(intent, {
            from,
            myId,
            positions,
            members: roster.members.filter((member) => member.online),
            meetings: session.meetings,
            blocked: (x, y) => isBlocked(tileMap, x, y),
          });
          if (!cell || (cell.x === from.x && cell.y === from.y)) return;
          // Stored either way; without AppSync this same request is also
          // what broadcasts it (see `moveInMap`).
          const facing = facingBetween(from, cell);
          await moveInMap(token, orgId, cell.x, cell.y, true, facing);
          const realtime = await getMapRealtime(token, orgId);
          if (realtime.endpoint)
            await publishMapMove({
              endpoint: realtime.endpoint,
              token: realtime.token,
              orgId,
              userId: myId,
              x: cell.x,
              y: cell.y,
              facing,
            });
        } catch {
          // Best-effort: the meeting itself is unaffected, and the avatar
          // just stays where it was.
        }
      }),
    [onIntent],
  );

  return (
    <MapRelocationContext.Provider value={claimRef}>
      {children}
    </MapRelocationContext.Provider>
  );
}

// Take over relocations while mounted -- see the module doc. `handler`
// may change every render; the latest one is always the one called.
export function useMapRelocationClaim(handler) {
  const claimRef = useContext(MapRelocationContext);
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  });
  useEffect(() => {
    if (!claimRef) return;
    const claim = (intent) => handlerRef.current(intent);
    claimRef.current = claim;
    return () => {
      if (claimRef.current === claim) claimRef.current = null;
    };
  }, [claimRef]);
}
