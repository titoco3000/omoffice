//! Where the map moves this tab's avatar when a meeting is joined or left
//! by an explicit click rather than by walking (see `useMeetingSession`'s
//! `onIntent`): next to whoever was joined, or to the nearest spot clear of
//! everyone after leaving. Pure functions over cells, so the choice is the
//! same whichever layout the click came from.

import { OFFICE_SIZE, PROXIMITY_RANGE } from "./api";

// How far (in cells, each way) `clearCell` looks before giving up.
const CLEAR_SEARCH_RADIUS = 20;

const inBounds = ({ x, y }) =>
  x >= 0 && x < OFFICE_SIZE && y >= 0 && y < OFFICE_SIZE;
const gridDistance = (a, b) => Math.abs(a.x - b.x) + Math.abs(a.y - b.y);
const ringDistance = (a, b) =>
  Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y));

// Of `cells`, the one closest to `from` by walking distance -- ties broken
// by row, then column, so the answer never depends on iteration order.
function closest(cells, from) {
  let best = null;
  for (const cell of cells) {
    const order = best
      ? gridDistance(cell, from) - gridDistance(best, from) ||
        cell.y - best.y ||
        cell.x - best.x
      : -1;
    if (order < 0) best = cell;
  }
  return best;
}

function square(center, radius) {
  const cells = [];
  for (let dy = -radius; dy <= radius; dy++)
    for (let dx = -radius; dx <= radius; dx++)
      cells.push({ x: center.x + dx, y: center.y + dy });
  return cells.filter(inBounds);
}

// Every cell-picking function below takes `blocked(x, y)` -- walls, which
// nobody can be moved onto.
//
// A cell within `PROXIMITY_RANGE` of `target` -- so the map agrees this tab
// is with them -- that nobody else stands on, and, if at all possible, out
// of range of everyone in `strangers` (anyone not already in the meeting
// being joined): they'd be newcomers on the very next step, and pull this
// tab's meeting into theirs. The innermost ring around `target` with such
// a cell, and within it the shortest walk from `from`. Failing that, any
// free cell in range; failing that, `target` itself (stacked, same as
// walking onto someone). `others` is everyone else's cell.
function cellNextTo(target, from, others, strangers, blocked) {
  const taken = new Set(others.map(({ x, y }) => `${x},${y}`));
  const free = (ring) =>
    square(target, ring).filter(
      (cell) =>
        ringDistance(cell, target) === ring &&
        !taken.has(`${cell.x},${cell.y}`) &&
        !blocked(cell.x, cell.y),
    );
  for (let ring = 1; ring <= PROXIMITY_RANGE; ring++) {
    const safe = free(ring).filter((cell) =>
      strangers.every((other) => ringDistance(cell, other) > PROXIMITY_RANGE),
    );
    if (safe.length > 0) return closest(safe, from);
  }
  for (let ring = 1; ring <= PROXIMITY_RANGE; ring++) {
    const cells = free(ring);
    if (cells.length > 0) return closest(cells, from);
  }
  return target;
}

// The shortest walk from `from` to a cell out of `PROXIMITY_RANGE` of
// everyone in `others` -- not just of meetings: anyone still in range
// would count as a newcomer on the very next step, and pull this tab
// straight back into a meeting. `null` if there's no such cell within
// `CLEAR_SEARCH_RADIUS`.
function clearCell(from, others, blocked) {
  const clear = square(from, CLEAR_SEARCH_RADIUS).filter(
    (cell) =>
      !blocked(cell.x, cell.y) &&
      others.every((other) => ringDistance(cell, other) > PROXIMITY_RANGE),
  );
  return closest(clear, from);
}

// Where to move for `intent` (see `useMeetingSession`'s `onIntent`), from
// cell `from`: `positions` is user_id -> cell, `members` who's online,
// `meetings` the org's meeting list. `null` to stay put.
export function relocationCell(
  intent,
  { from, myId, positions, members, meetings, blocked = () => false },
) {
  const here = { x: Math.round(from.x), y: Math.round(from.y) };
  const others = members.filter(
    (member) => member.id !== myId && positions[member.id],
  );
  const cells = others.map((member) => positions[member.id]);
  if (intent.type === "leave") return clearCell(here, cells, blocked);
  if (intent.type !== "join") return null;
  const target = positions[intent.userId];
  if (!target) return null;
  const group = new Set(
    meetings.find((meeting) => meeting.attendees?.includes(intent.userId))
      ?.attendees ?? [intent.userId],
  );
  const strangers = others
    .filter((member) => !group.has(member.id))
    .map((member) => positions[member.id]);
  return cellNextTo(target, here, cells, strangers, blocked);
}
