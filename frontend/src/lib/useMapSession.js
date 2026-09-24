//! Live office-map state for one org: everyone's cell (fetched once from
//! `GET /map/org/{org_id}/positions` as a base, overridden by whatever
//! `MAP_MOVE_CHANNEL` or this tab's own moves have reported since) plus
//! this tab's own movement.
//!
//! Movement is one cell per `1 / USER_SPEED` seconds, gated by a single
//! cooldown shared by both input methods:
//! - **Arrow keys** step in whichever direction is currently held, checked
//!   again each time the cooldown clears -- so holding one down repeats it
//!   for as long as it stays held, same as a classic tile-grid walk.
//! - **A click** queues its cell as a one-shot absolute target, applied
//!   (not walked toward -- a direct move to that cell) the moment the
//!   cooldown allows; a second click before that happens replaces the
//!   queued one rather than stacking.
//! A click takes priority over a held key if both are pending when the
//! cooldown clears. A steady metronome (every `1 / USER_SPEED` seconds)
//! drives that check, rather than each move rescheduling the next one
//! itself, so held-key repeats don't depend on a self-referencing timer.
//!
//! The backend only clamps and stores whatever cell it's told (see
//! `features::map::set_position`, backend/lib/features/map.rs) -- this hook
//! is what keeps the cadence honest, applied optimistically before the
//! request even lands, same as `useMeetingSession`'s `create`/`join`.
//!
//! Every move is broadcast, but only some are stored (`persist`): the first
//! move after `PERSIST_MS` without one, the resting cell once movement
//! stops (a cooldown passes with nothing pending), and whatever cell is
//! unsaved when the canvas unmounts -- so a held key's every step costs a
//! push, not a database write.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { decodeJwt } from "./jwt";
import { messenger } from "./messenger";
import {
  MAP_ACTIVE_CHANNEL,
  MAP_MOVE_CHANNEL,
  OFFICE_SIZE,
  listMapPositions,
  moveInMap,
} from "./map";
import { useMapProximity } from "./useMapProximity";
import { useOrgSession } from "../org/OrgSessionContext";

// Cells per second -- governs both the move cooldown (one cell every
// `1 / USER_SPEED` seconds) and, so the two stay in sync, how fast
// `MapCanvas` animates a translation to a new cell.
export const USER_SPEED = 4;
const STEP_MS = 1000 / USER_SPEED;
// How often the cooldown is polled -- finer than `STEP_MS` so a move fires
// right when the cooldown clears rather than up to one metronome period
// late; the cooldown itself is still enforced by `lastMoveAtRef`, not by
// this interval's period.
const POLL_MS = 30;
// At most one stored move per this long while moving -- see the module doc.
const PERSIST_MS = 1000;

const ARROW_DELTA = {
  ArrowUp: { dx: 0, dy: -1 },
  ArrowDown: { dx: 0, dy: 1 },
  ArrowLeft: { dx: -1, dy: 0 },
  ArrowRight: { dx: 1, dy: 0 },
};

// `Math.min`/`max` pass a NaN straight through -- guard against ever
// serializing one into `{"x":null,...}` on the wire (JSON has no NaN) by
// falling back to 0, same as an out-of-range value clamps rather than errors.
const clamp = (v) =>
  Number.isFinite(v) ? Math.max(0, Math.min(OFFICE_SIZE - 1, v)) : 0;

export function useMapSession(token, orgId, members) {
  const myId = decodeJwt(token)?.sub;
  // Fetched once per org -- a member with nothing here yet (the fetch
  // hasn't landed, or they've simply never moved) just defaults to (0, 0),
  // same as the backend's own default for a member with no row.
  const [basePositions, setBasePositions] = useState({}); // user_id -> {x, y}
  const [overrides, setOverrides] = useState({}); // user_id -> {x, y}
  // Whether `basePositions` has landed -- until then everyone sits at the
  // (0, 0) default, which says nothing about who's actually near whom.
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (!orgId) return;
    let cancelled = false;
    listMapPositions(token, orgId)
      .then((data) => {
        if (cancelled) return;
        const next = {};
        for (const position of data.positions ?? [])
          next[position.user_id] = { x: position.x, y: position.y };
        setBasePositions(next);
        setLoaded(true);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      setBasePositions({});
      setLoaded(false);
    };
  }, [token, orgId]);

  const positions = useMemo(() => {
    const result = {};
    for (const member of members)
      result[member.id] = overrides[member.id] ??
        basePositions[member.id] ?? { x: 0, y: 0 };
    return result;
  }, [members, basePositions, overrides]);

  useEffect(() => {
    if (!orgId) return;
    return messenger.on(MAP_MOVE_CHANNEL, (message) => {
      if (message.org_id !== orgId) return;
      // Self already applied this optimistically in `applyMove`, straight
      // off what was actually sent -- not off this broadcast, which can
      // arrive out of order (each held-key step fires its own HTTP
      // request, with no guarantee an earlier one's response/broadcast
      // lands first). Applying it here too let a late, stale broadcast
      // stomp a newer optimistic position, which read as a one-cell
      // rollback mid-walk.
      if (message.user_id === myId) return;
      setOverrides((prev) => ({
        ...prev,
        [message.user_id]: { x: message.x, y: message.y },
      }));
    });
  }, [orgId, myId]);

  // Report this tab's canvas as actually mounted -- narrows who a move
  // reaches to tabs that would actually render it, see `features::map`'s
  // module doc. Cleared on unmount, same reasoning as `ORG_ACTIVE_CHANNEL`.
  useEffect(() => {
    if (!orgId) return;
    messenger.send(MAP_ACTIVE_CHANNEL, { org_id: orgId, active: true });
    return () =>
      messenger.send(MAP_ACTIVE_CHANNEL, { org_id: orgId, active: false });
  }, [orgId]);

  const myPosition = positions[myId] ?? { x: 0, y: 0 };
  // Read inside the metronome/input handlers below without pulling
  // `positions` into their dependencies -- kept current by its own effect
  // rather than written during render. Keyed on the primitive x/y, not
  // `myPosition` itself, which is a fresh object every render.
  const myPositionRef = useRef(myPosition);
  useEffect(() => {
    myPositionRef.current = { x: myPosition.x, y: myPosition.y };
  }, [myPosition.x, myPosition.y]);

  const heldKeysRef = useRef([]); // most-recently-pressed direction last
  const queuedClickRef = useRef(null);
  const lastMoveAtRef = useRef(0);
  const lastPersistAtRef = useRef(0);
  // This tab has moved since its last stored move.
  const dirtyRef = useRef(false);

  const applyMove = useCallback(
    (x, y) => {
      const target = { x: clamp(x), y: clamp(y) };
      const persist = Date.now() - lastPersistAtRef.current >= PERSIST_MS;
      if (persist) lastPersistAtRef.current = Date.now();
      dirtyRef.current = !persist;
      setOverrides((prev) => ({ ...prev, [myId]: target }));
      moveInMap(token, orgId, target.x, target.y, persist).catch(() => {});
    },
    [token, orgId, myId],
  );

  // Store the current cell if any move since the last stored one wasn't --
  // movement stopping, or the canvas going away.
  const flush = useCallback(() => {
    if (!dirtyRef.current) return;
    dirtyRef.current = false;
    lastPersistAtRef.current = Date.now();
    const { x, y } = myPositionRef.current;
    moveInMap(token, orgId, x, y, true).catch(() => {});
  }, [token, orgId]);

  useEffect(() => flush, [flush]);

  // Apply whichever of a queued click or a held key is pending, preferring
  // the click -- but only once the cooldown since the last actual move has
  // elapsed. Safe to call as often as convenient (a poll, a keydown, a
  // click): it's a no-op both when nothing is pending and when the
  // cooldown hasn't cleared yet.
  const attempt = useCallback(() => {
    if (Date.now() - lastMoveAtRef.current < STEP_MS) return;

    let target = queuedClickRef.current;
    queuedClickRef.current = null;
    if (!target) {
      const key = heldKeysRef.current[heldKeysRef.current.length - 1];
      const delta = key && ARROW_DELTA[key];
      if (delta) {
        const from = myPositionRef.current;
        target = { x: from.x + delta.dx, y: from.y + delta.dy };
      }
    }
    // Nothing pending a full cooldown after the last move: movement has
    // stopped, so store where it stopped.
    if (!target) return flush();

    applyMove(target.x, target.y);
    lastMoveAtRef.current = Date.now();
  }, [applyMove, flush]);

  // Polls `attempt` so a held key keeps repeating and a click queued during
  // the cooldown still gets applied once it clears, for as long as this
  // org's map is open.
  useEffect(() => {
    if (!orgId) return;
    const interval = setInterval(attempt, POLL_MS);
    return () => clearInterval(interval);
  }, [orgId, attempt]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if (!ARROW_DELTA[event.key]) return;
      if (["INPUT", "TEXTAREA"].includes(event.target.tagName)) return;
      event.preventDefault();
      if (!heldKeysRef.current.includes(event.key))
        heldKeysRef.current.push(event.key);
      attempt();
    };
    const onKeyUp = (event) => {
      heldKeysRef.current = heldKeysRef.current.filter(
        (key) => key !== event.key,
      );
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [attempt]);

  const moveTo = useCallback(
    (x, y) => {
      queuedClickRef.current = { x, y };
      attempt();
    },
    [attempt],
  );

  // Walking up to someone starts or joins a meeting with them -- see
  // `useMapProximity`.
  const { session } = useOrgSession();
  useMapProximity(token, orgId, positions, loaded, members, myId, session);

  return { positions, myPosition, moveTo };
}
