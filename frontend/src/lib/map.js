//! Calls into `/map/*`, the contract `features::map::routes` mounts
//! (backend/lib/features/map.rs). A feature in its own right, not part of
//! `/rbac/*` -- initial positions come from its own `GET .../positions`
//! rather than riding along on the roster.

import { apiFetch } from "./api";

// Mirrors `features::map`: cells per side of the (square) office grid,
// `MAP_MOVE_CHANNEL` for one live cell change (`{org_id, user_id, x, y}`),
// `MAP_ACTIVE_CHANNEL` sent (not received) to report the canvas
// mounting/unmounting, same shape/reasoning as `rbac.js`'s
// `ORG_ACTIVE_CHANNEL` but for "is this tab actually watching the map"
// rather than "online".
export const OFFICE_SIZE = 100;
export const MAP_MOVE_CHANNEL = "map_move";
export const MAP_ACTIVE_CHANNEL = "map_active";

// Every member's current cell -- {positions: [{user_id, x, y, map_active}, ...]}.
export const listMapPositions = (token, orgId) =>
  apiFetch(`/map/org/${orgId}/positions`, { token });

export const moveInMap = (token, orgId, x, y) =>
  apiFetch(`/map/org/${orgId}/move`, {
    method: "POST",
    token,
    body: { x, y },
  });
