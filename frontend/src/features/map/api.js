//! Calls into `/map/*`, the contract `features::map::routes` mounts
//! (backend/lib/features/map.rs). A feature in its own right, not part of
//! `/rbac/*` -- initial positions come from its own `GET .../positions`
//! rather than riding along on the roster.

import { apiFetch } from "../../core/api";

// Mirrors `features::map`: cells per side of the (square) office grid,
// `MAP_MOVE_CHANNEL` for one live cell change (`{org_id, user_id, x, y, facing}`),
// `MAP_ACTIVE_CHANNEL` sent (not received) to report the canvas
// mounting/unmounting, same shape/reasoning as `rbac.js`'s
// `ORG_ACTIVE_CHANNEL` but for "is this tab actually watching the map"
// rather than "online".
export const OFFICE_SIZE = 100;
export const MAP_MOVE_CHANNEL = "map_move";
export const MAP_ACTIVE_CHANNEL = "map_active";
// Cells in each direction someone counts as nearby -- see
// `features::map::proximity::PROXIMITY_RANGE`.
export const PROXIMITY_RANGE = 2;
// `{org_id, meeting_id}`: someone walking up is about to pull this user
// into `meeting_id` -- see `features::map::proximity::PENDING_CHANNEL`.
export const MAP_MEETING_PENDING_CHANNEL = "map_meeting_pending";

// Every member's current cell -- {positions: [{user_id, x, y, map_active}, ...]}.
export const listMapPositions = (token, orgId) =>
  apiFetch(`/map/org/${orgId}/positions`, { token });

// The caller just moved within `PROXIMITY_RANGE` of `nearby` (user ids):
// the backend starts, joins or merges meetings accordingly -- see
// `features::map::proximity`. `meetingId` is the id to use should that
// mean a new meeting. Answers with a join response, or
// `{meeting_id, unchanged: true}` when the caller stays where they are.
export const gatherNearby = (token, orgId, nearby, meetingId) =>
  apiFetch(`/map/org/${orgId}/proximity`, {
    method: "POST",
    token,
    body: { nearby, meeting_id: meetingId },
  });

// `{endpoint, token}` for the map's AppSync Events API (see `mapEvents.js`),
// both null when the backend has none -- live moves then ride
// `MAP_MOVE_CHANNEL`, broadcast by `moveInMap` itself. The token lasts 15
// minutes, see `features::map::realtime::route`.
export const getMapRealtime = (token, orgId) =>
  apiFetch(`/map/org/${orgId}/realtime`, { token });

// Only a move with `persist` set is stored; without AppSync, every move is
// also broadcast -- see `features::map::MoveBody`. `facing` ("up" | "down" |
// "left" | "right") is never stored, only broadcast along with the cell.
export const moveInMap = (token, orgId, x, y, persist = false, facing) =>
  apiFetch(`/map/org/${orgId}/move`, {
    method: "POST",
    token,
    body: { x, y, persist, facing },
  });
