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
//!   (and broadcast) as a single move the moment the cooldown allows -- the
//!   cells in between are never reported, so walking past someone on the
//!   way doesn't count as meeting them, and clicking on someone meets them
//!   straight away. `MapCanvas` is what shows the walk there, along the
//!   shortest route around the walls (`navigation.js`) at `USER_SPEED`.
//!   A click on a wall is ignored; a cell no route reaches (walled in) is
//!   teleported to instead of walked to. A second click before the
//!   cooldown clears replaces the queued one rather than stacking.
//! - **Walls** also stop arrow keys: a step into one moves nothing, it only
//!   turns the avatar to face it.
//! Every move carries which way the avatar now faces (`facing`), so turning
//! in place reaches everyone else too. It is only ever broadcast, never
//! stored: whoever loads the map afresh sees everyone facing down.
//!
//! Mid-walk, this hook tracks roughly where the avatar has visibly got to
//! (`walkRef`), and both inputs start from there rather than from the
//! logical cell already reached: another click re-targets from that spot,
//! and an arrow key cancels the walk, stepping from the nearest cell to it
//! -- the same as clicking the cell next to that one.
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
//!
//! Where a move is broadcast depends on the backend: with an AppSync Events
//! API (`GET .../realtime` answers with an endpoint), every move is
//! published there directly (`mapEvents.js`) and received from there, and
//! only the stored ones are posted to `POST .../move` at all. Without one
//! (a local run), every move is posted, and the backend broadcasts it over
//! `MAP_MOVE_CHANNEL` -- which is also the path used until `realtime` has
//! answered.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { decodeJwt } from "../../core/jwt";
import { messenger } from "../../core/messenger";
import {
  MAP_ACTIVE_CHANNEL,
  MAP_MOVE_CHANNEL,
  OFFICE_SIZE,
  getMapRealtime,
  listMapPositions,
  moveInMap,
} from "./api";
import { openMapEvents } from "./realtime";
import { useMapProximity } from "./useMapProximity";
import { relocationCell } from "./relocation";
import { useMapRelocationClaim } from "./MapRelocationContext";
import { useMeetingSessionContext } from "../meetings";
import {
  facingBetween,
  findRoute,
  routeLength,
  routePosition,
} from "./navigation";
import { isBlocked } from "./tileMap";

// Cells per second -- governs both the move cooldown (one cell every
// `1 / USER_SPEED` seconds) and, so the two stay in sync, how fast
// `MapCanvas` animates a translation to a new cell.
export const USER_SPEED = 5;
const STEP_MS = 1000 / USER_SPEED;
// How often the cooldown is polled -- finer than `STEP_MS` so a move fires
// right when the cooldown clears rather than up to one metronome period
// late; the cooldown itself is still enforced by `lastMoveAtRef`, not by
// this interval's period.
const POLL_MS = 30;
// At most one stored move per this long while moving -- see the module doc.
const PERSIST_MS = 1000;
// Fetch a fresh map token this long before the current one expires, and
// reconnect with it.
const TOKEN_REFRESH_MARGIN_MS = 60 * 1000;
// Wait before retrying a failed `realtime` fetch or a dropped socket.
const REALTIME_RETRY_MS = 3000;

const ARROW_DELTA = {
  ArrowUp: { dx: 0, dy: -1, facing: "up" },
  ArrowDown: { dx: 0, dy: 1, facing: "down" },
  ArrowLeft: { dx: -1, dy: 0, facing: "left" },
  ArrowRight: { dx: 1, dy: 0, facing: "right" },
};

// `Math.min`/`max` pass a NaN straight through -- guard against ever
// serializing one into `{"x":null,...}` on the wire (JSON has no NaN) by
// falling back to 0, same as an out-of-range value clamps rather than errors.
const clamp = (v) =>
  Number.isFinite(v) ? Math.max(0, Math.min(OFFICE_SIZE - 1, v)) : 0;

const gridDistance = (a, b) => Math.abs(a.x - b.x) + Math.abs(a.y - b.y);

// Roughly where `MapCanvas` has drawn this tab mid-walk: `walk.route` at
// `USER_SPEED`. Used to time a re-targeted walk and to pick the cell an
// arrow key steps from, so it needs the distance left right, not the exact
// corner taken.
const walkPosition = (walk, now) =>
  routePosition(walk.route, (now - walk.startedAt) / STEP_MS);

function bumpPlacements(prev, ids) {
  const next = { ...prev };
  for (const id of ids) next[id] = (prev[id] ?? 0) + 1;
  return next;
}

// `tileMap` (see `tileMap.js`) is `null` until `map.png` has loaded, and
// everything is walkable until then.
export function useMapSession(token, orgId, members, tileMap) {
  const myId = decodeJwt(token)?.sub;
  // Fetched once per org -- a member with nothing here yet (the fetch
  // hasn't landed, or they've simply never moved) just defaults to (0, 0),
  // same as the backend's own default for a member with no row.
  const [basePositions, setBasePositions] = useState({}); // user_id -> {x, y}
  const [overrides, setOverrides] = useState({}); // user_id -> {x, y}
  // Whether `basePositions` has landed -- until then everyone sits at the
  // (0, 0) default, which says nothing about who's actually near whom.
  const [loaded, setLoaded] = useState(false);
  // user_id -> a counter bumped whenever a fetch (re)places someone, so
  // `MapCanvas` draws them straight there instead of easing over from
  // wherever they were last drawn: everyone on the initial load, and anyone
  // who comes online later. `null` while a newcomer's fetch is in flight --
  // not drawn yet, rather than drawn at a position that may be stale.
  const [placements, setPlacements] = useState({});

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
        setPlacements((prev) => bumpPlacements(prev, Object.keys(next)));
        setLoaded(true);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      setBasePositions({});
      setLoaded(false);
    };
  }, [token, orgId]);

  // Someone who just came online: this tab's copy of their cell dates from
  // whenever it was fetched or last pushed, and they may have moved since
  // (in another session, or before going offline). Fetch it fresh, and
  // have them placed there rather than seen walking over from the old one.
  // Any live move of theirs that lands before the fetch does still wins --
  // it's an override, newer than anything stored.
  const memberKey = members
    .map((member) => member.id)
    .sort()
    .join(",");
  const seenMembersRef = useRef(null);
  const orgIdRef = useRef(orgId);
  useEffect(() => {
    orgIdRef.current = orgId;
  }, [orgId]);
  useEffect(() => {
    if (!loaded) {
      seenMembersRef.current = null;
      return;
    }
    const ids = memberKey ? memberKey.split(",") : [];
    const seen = seenMembersRef.current;
    seenMembersRef.current = new Set(ids);
    // The first roster after a load is exactly what that load placed.
    if (!seen) return;
    const arrived = ids.filter((id) => !seen.has(id) && id !== myId);
    if (arrived.length === 0) return;

    setOverrides((prev) => {
      const next = { ...prev };
      for (const id of arrived) delete next[id];
      return next;
    });
    setPlacements((prev) => {
      const next = { ...prev };
      for (const id of arrived) next[id] = null;
      return next;
    });
    const fetchedFor = orgId;
    const place = (fresh) => {
      if (orgIdRef.current !== fetchedFor) return;
      setBasePositions((prev) => ({ ...prev, ...fresh }));
      setPlacements((prev) => bumpPlacements(prev, arrived));
    };
    listMapPositions(token, orgId)
      .then((data) => {
        const fresh = {};
        for (const position of data.positions ?? [])
          if (arrived.includes(position.user_id))
            fresh[position.user_id] = { x: position.x, y: position.y };
        place(fresh);
      })
      // Placed where this tab last knew them, rather than never drawn.
      .catch(() => place({}));
  }, [loaded, memberKey, myId, token, orgId]);

  const positions = useMemo(() => {
    const result = {};
    for (const member of members)
      result[member.id] = overrides[member.id] ??
        basePositions[member.id] ?? { x: 0, y: 0 };
    return result;
  }, [members, basePositions, overrides]);

  // Someone else's live move, from either transport. Self already applied
  // its own optimistically in `applyMove`, straight off what was actually
  // sent -- not off the echo, which can arrive out of order (with no
  // guarantee an earlier step's echo lands first). Applying it here too
  // let a late, stale echo stomp a newer optimistic position, which read
  // as a one-cell rollback mid-walk.
  const receiveMove = useCallback(
    (userId, x, y, facing) => {
      if (userId === myId) return;
      setOverrides((prev) => ({
        ...prev,
        [userId]: { x, y, facing: facing ?? prev[userId]?.facing },
      }));
    },
    [myId],
  );

  useEffect(() => {
    if (!orgId) return;
    return messenger.on(MAP_MOVE_CHANNEL, (message) => {
      if (message.org_id !== orgId) return;
      receiveMove(message.user_id, message.x, message.y, message.facing);
    });
  }, [orgId, receiveMove]);

  // The AppSync connection moves are published to, once `realtime` says
  // there is one -- null until then, and for good without one (see the
  // module doc). A connection is replaced with a fresh token shortly before
  // the current token expires: the new one opens alongside, and the old one
  // is closed once the new one is subscribed, so no move goes unheard in
  // between.
  const mapEventsRef = useRef(null);
  useEffect(() => {
    if (!orgId || !myId) return;
    let cancelled = false;
    let timer = null;
    let current = null;
    // Connections a newer one is replacing, closed once it's subscribed.
    let stale = [];
    const retry = () => {
      clearTimeout(timer);
      timer = setTimeout(connect, REALTIME_RETRY_MS);
    };
    async function connect() {
      let realtime;
      try {
        realtime = await getMapRealtime(token, orgId);
      } catch {
        if (!cancelled) retry();
        return;
      }
      if (cancelled || !realtime.endpoint) return;
      if (current) stale.push(current);
      const next = openMapEvents({
        endpoint: realtime.endpoint,
        token: realtime.token,
        orgId,
        userId: myId,
        onMove: receiveMove,
        onReady: () => {
          stale.forEach((old) => old.close());
          stale = [];
        },
        onClose: () => {
          if (cancelled || current !== next) return;
          retry();
        },
      });
      current = next;
      mapEventsRef.current = next;
      const expiresAt = (decodeJwt(realtime.token)?.exp ?? 0) * 1000;
      clearTimeout(timer);
      timer = setTimeout(
        connect,
        Math.max(
          REALTIME_RETRY_MS,
          expiresAt - Date.now() - TOKEN_REFRESH_MARGIN_MS,
        ),
      );
    }
    connect();
    return () => {
      cancelled = true;
      clearTimeout(timer);
      current?.close();
      stale.forEach((old) => old.close());
      mapEventsRef.current = null;
    };
  }, [token, orgId, myId, receiveMove]);

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

  const tileMapRef = useRef(tileMap);
  useEffect(() => {
    tileMapRef.current = tileMap;
  }, [tileMap]);
  const myFacingRef = useRef("down");

  const heldKeysRef = useRef([]); // most-recently-pressed direction last
  const queuedClickRef = useRef(null);
  // The walk `MapCanvas` is drawing toward the last move's cell:
  // {route, startedAt, endsAt} -- see the module doc.
  const walkRef = useRef(null);
  const lastMoveAtRef = useRef(0);
  const lastPersistAtRef = useRef(0);
  // This tab has moved since its last stored move.
  const dirtyRef = useRef(false);

  const applyMove = useCallback(
    (x, y, facing) => {
      const target = { x: clamp(x), y: clamp(y) };
      myFacingRef.current = facing;
      const persist = Date.now() - lastPersistAtRef.current >= PERSIST_MS;
      if (persist) lastPersistAtRef.current = Date.now();
      dirtyRef.current = !persist;
      setOverrides((prev) => ({ ...prev, [myId]: { ...target, facing } }));
      const mapEvents = mapEventsRef.current;
      mapEvents?.publish(target.x, target.y, facing);
      if (persist || !mapEvents)
        moveInMap(token, orgId, target.x, target.y, persist, facing).catch(
          () => {},
        );
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
    moveInMap(token, orgId, x, y, true, myFacingRef.current).catch(() => {});
  }, [token, orgId]);

  useEffect(() => flush, [flush]);

  // Apply whichever of a queued click or a held key is pending, preferring
  // the click -- but only once the cooldown since the last actual move has
  // elapsed. Safe to call
  // as often as convenient (a poll, a keydown, a click): it's a no-op both
  // when nothing is pending and when the cooldown hasn't cleared yet.
  const attempt = useCallback(() => {
    const now = Date.now();
    if (now - lastMoveAtRef.current < STEP_MS) return;

    const walk = walkRef.current;
    const from =
      walk && now < walk.endsAt
        ? walkPosition(walk, now)
        : myPositionRef.current;
    let click = queuedClickRef.current;
    queuedClickRef.current = null;
    const map = tileMapRef.current;
    if (click && isBlocked(map, click.x, click.y)) click = null;

    if (click) {
      const route = findRoute(map, from, click);
      let facing = myFacingRef.current;
      if (route) {
        walkRef.current = {
          route,
          startedAt: now,
          endsAt: now + routeLength(route) * STEP_MS,
        };
        if (route.length >= 2)
          facing = facingBetween(route[route.length - 2], click, facing);
      } else {
        // Walled in: nothing to walk, so the avatar just appears there.
        walkRef.current = null;
      }
      applyMove(click.x, click.y, facing);
      lastMoveAtRef.current = now;
      return;
    }

    const key = heldKeysRef.current[heldKeysRef.current.length - 1];
    const delta = key && ARROW_DELTA[key];
    // Nothing pending a full cooldown after the last move: movement has
    // stopped, so store where it stopped.
    if (!delta) return flush();

    const cell = { x: Math.round(from.x), y: Math.round(from.y) };
    const step = { x: clamp(cell.x + delta.dx), y: clamp(cell.y + delta.dy) };
    if (
      isBlocked(map, step.x, step.y) ||
      (step.x === cell.x && step.y === cell.y)
    ) {
      // A wall (or the edge): turn to face it, don't move. Already facing
      // it means there's nothing to report.
      if (delta.facing === myFacingRef.current) return flush();
      walkRef.current = null;
      applyMove(cell.x, cell.y, delta.facing);
      lastMoveAtRef.current = now;
      return;
    }
    walkRef.current = {
      route: [from, step],
      startedAt: now,
      endsAt: now + gridDistance(from, step) * STEP_MS,
    };
    applyMove(step.x, step.y, delta.facing);
    lastMoveAtRef.current = now;
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
      // Clamped here, not just in `applyMove`, so the walk is timed to
      // the cell actually moved to.
      queuedClickRef.current = { x: clamp(x), y: clamp(y) };
      attempt();
    },
    [attempt],
  );

  // Walking up to someone starts or joins a meeting with them -- see
  // `useMapProximity`. `silentCellRef` is the cell a relocation below is
  // headed for, which proximity doesn't treat as walking up to anyone.
  const session = useMeetingSessionContext();
  const silentCellRef = useRef(null);
  useMapProximity(
    token,
    orgId,
    positions,
    loaded,
    members,
    myId,
    session,
    silentCellRef,
  );

  // A meeting joined or left by an explicit click (roster, dock) while the
  // map is open: move there as a click on the map would -- walked, one move
  // over the wire -- but silently, since the meeting side is already
  // handled. Claimed from `MapRelocationProvider`, which covers the same
  // clicks while no map is open. See `relocation.js` for where.
  const relocationRef = useRef(null);
  useEffect(() => {
    relocationRef.current = { positions, members, myId, loaded, session };
  });
  useMapRelocationClaim((intent) => {
    const { positions, members, myId, loaded, session } = relocationRef.current;
    if (!loaded) return;
    const walk = walkRef.current;
    const now = Date.now();
    const cell = relocationCell(intent, {
      from:
        walk && now < walk.endsAt
          ? walkPosition(walk, now)
          : myPositionRef.current,
      myId,
      positions,
      members,
      meetings: session.meetings,
      blocked: (x, y) => isBlocked(tileMapRef.current, x, y),
    });
    if (!cell) return;
    silentCellRef.current = cell;
    moveTo(cell.x, cell.y);
  });

  return { positions, myPosition, moveTo, loaded, placements };
}
