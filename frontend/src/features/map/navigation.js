//! Walking around walls: pure functions over cells, so every tab -- the
//! mover's, and everyone watching -- plans the same route from the same
//! two cells and the walk reads identically without the path itself ever
//! going over the wire (only the destination does, see `useMapSession`).

import { OFFICE_SIZE } from "./api";
import { isBlocked } from "./tileMap";

// Costs are integers so a tie is a tie on every machine. A turn costs a
// sliver of a step: among equally short routes, the one with the fewest
// turns wins -- an L rather than a staircase.
const STEP_COST = 1000;
const TURN_COST = 1;

// Neighbor order fixes which of two equal routes is found first.
const DIRECTIONS = [
  { dx: 1, dy: 0 },
  { dx: 0, dy: 1 },
  { dx: -1, dy: 0 },
  { dx: 0, dy: -1 },
];

const inBounds = (x, y) =>
  x >= 0 && y >= 0 && x < OFFICE_SIZE && y < OFFICE_SIZE;

/// Which way one step from `from` to `to` faces: "up" | "down" | "left" |
/// "right", by whichever axis moved more, or `fallback` if neither did.
export function facingBetween(from, to, fallback = "down") {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  if (!dx && !dy) return fallback;
  if (Math.abs(dx) > Math.abs(dy)) return dx > 0 ? "right" : "left";
  return dy > 0 ? "down" : "up";
}

// A binary min-heap of [priority, order, value]; `order` (insertion
// count) settles equal priorities so the pop order is deterministic.
class Heap {
  constructor() {
    this.items = [];
    this.count = 0;
  }
  push(priority, value) {
    const items = this.items;
    items.push([priority, this.count++, value]);
    let i = items.length - 1;
    while (i > 0) {
      const parent = (i - 1) >> 1;
      if (this.less(items[parent], items[i])) break;
      [items[parent], items[i]] = [items[i], items[parent]];
      i = parent;
    }
  }
  pop() {
    const items = this.items;
    const top = items[0];
    const last = items.pop();
    if (items.length > 0) {
      items[0] = last;
      let i = 0;
      for (;;) {
        const l = 2 * i + 1;
        const r = l + 1;
        let min = i;
        if (l < items.length && this.less(items[l], items[min])) min = l;
        if (r < items.length && this.less(items[r], items[min])) min = r;
        if (min === i) break;
        [items[min], items[i]] = [items[i], items[min]];
        i = min;
      }
    }
    return top[2];
  }
  less(a, b) {
    return a[0] < b[0] || (a[0] === b[0] && a[1] < b[1]);
  }
  get size() {
    return this.items.length;
  }
}

// A* over (cell, heading) states, four-way, never through a wall. Returns
// the cells from `from` to `to` inclusive, or `null` if `to` can't be
// reached (or is itself a wall).
function search(tileMap, from, to) {
  if (isBlocked(tileMap, to.x, to.y) || isBlocked(tileMap, from.x, from.y))
    return null;
  if (from.x === to.x && from.y === to.y) return [{ x: from.x, y: from.y }];

  const key = (x, y, heading) => (y * OFFICE_SIZE + x) * 5 + heading;
  const heuristic = (x, y) =>
    (Math.abs(x - to.x) + Math.abs(y - to.y)) * STEP_COST;
  const best = new Map([[key(from.x, from.y, 4), 0]]); // heading 4: none yet
  const parent = new Map();
  const open = new Heap();
  open.push(heuristic(from.x, from.y), { x: from.x, y: from.y, heading: 4 });

  while (open.size > 0) {
    const { x, y, heading } = open.pop();
    const cost = best.get(key(x, y, heading));
    if (x === to.x && y === to.y) {
      const cells = [];
      let at = key(x, y, heading);
      let cell = { x, y };
      for (;;) {
        cells.push(cell);
        const prev = parent.get(at);
        if (!prev) break;
        at = prev.key;
        cell = { x: prev.x, y: prev.y };
      }
      return cells.reverse();
    }
    for (let h = 0; h < DIRECTIONS.length; h++) {
      const nx = x + DIRECTIONS[h].dx;
      const ny = y + DIRECTIONS[h].dy;
      if (!inBounds(nx, ny) || isBlocked(tileMap, nx, ny)) continue;
      const next =
        cost + STEP_COST + (heading !== 4 && heading !== h ? TURN_COST : 0);
      const nextKey = key(nx, ny, h);
      if (next >= (best.get(nextKey) ?? Infinity)) continue;
      best.set(nextKey, next);
      parent.set(nextKey, { key: key(x, y, heading), x, y });
      open.push(next + heuristic(nx, ny), { x: nx, y: ny, heading: h });
    }
  }
  return null;
}

/// The route from `from` (a cell, or a point between two cells on one axis
/// -- an avatar mid-step) to the cell `to`: the points to walk through, in
/// order, starting with `from` itself; `null` if `to` is a wall or can't
/// be reached, which the caller answers with a teleport. Mid-step, the
/// nearer way on is chosen by whichever of the two cells beside `from`
/// gives the shorter walk, so nobody slides sideways through a wall.
/// `tileMap` may be `null` (not loaded yet): everything is open then.
export function findRoute(tileMap, from, to) {
  const fx = Number.isInteger(from.x);
  const fy = Number.isInteger(from.y);
  const anchors = [];
  if (fx && fy) anchors.push({ x: from.x, y: from.y });
  else if (!fx) {
    anchors.push({ x: Math.floor(from.x), y: from.y });
    anchors.push({ x: Math.ceil(from.x), y: from.y });
  } else {
    anchors.push({ x: from.x, y: Math.floor(from.y) });
    anchors.push({ x: from.x, y: Math.ceil(from.y) });
  }
  let bestRoute = null;
  let bestLength = Infinity;
  for (const anchor of anchors) {
    const cells = search(tileMap, anchor, to);
    if (!cells) continue;
    const length =
      Math.abs(anchor.x - from.x) +
      Math.abs(anchor.y - from.y) +
      cells.length -
      1;
    if (length < bestLength) {
      bestLength = length;
      bestRoute = cells;
    }
  }
  if (!bestRoute) return null;
  const [first] = bestRoute;
  return first.x === from.x && first.y === from.y
    ? bestRoute
    : [{ x: from.x, y: from.y }, ...bestRoute];
}

/// Cells of walking along `route` (see `findRoute`).
export function routeLength(route) {
  let length = 0;
  for (let i = 1; i < route.length; i++)
    length +=
      Math.abs(route[i].x - route[i - 1].x) +
      Math.abs(route[i].y - route[i - 1].y);
  return length;
}

/// Where you are after walking `distance` cells along `route`; the end of
/// it once you've gone the whole way.
export function routePosition(route, distance) {
  let left = distance;
  for (let i = 1; i < route.length; i++) {
    const leg =
      Math.abs(route[i].x - route[i - 1].x) +
      Math.abs(route[i].y - route[i - 1].y);
    if (left <= leg) {
      const t = leg ? left / leg : 0;
      return {
        x: route[i - 1].x + (route[i].x - route[i - 1].x) * t,
        y: route[i - 1].y + (route[i].y - route[i - 1].y) * t,
      };
    }
    left -= leg;
  }
  return { ...route[route.length - 1] };
}
