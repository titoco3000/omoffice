//! Realtime regions: the office grid cut into `REGION_SIZE`-cell squares,
//! which moves are published and subscribed to by (see `realtime.js`).
//! Mirrors `features::map` in backend/lib/features/map/mod.rs.

import { OFFICE_SIZE } from "./api";

export const REGION_SIZE = 10;
export const REGIONS_PER_SIDE = OFFICE_SIZE / REGION_SIZE;

// A viewport touching more regions than this follows the coarse channel
// instead of subscribing region by region.
export const COARSE_ABOVE_REGIONS = 16;

export const regionOf = (x, y) => [
  Math.floor(x / REGION_SIZE),
  Math.floor(y / REGION_SIZE),
];

export const regionKey = (rx, ry) => `${rx}-${ry}`;

const clampRegion = (r) => Math.max(0, Math.min(REGIONS_PER_SIDE - 1, r));

// Keys of the regions a rect of cells touches, clamped to the office.
export function regionsInRect({ minX, maxX, minY, maxY }) {
  const [x0] = regionOf(minX, 0);
  const [x1] = regionOf(maxX, 0);
  const [, y0] = regionOf(0, minY);
  const [, y1] = regionOf(0, maxY);
  const keys = [];
  for (let ry = clampRegion(y0); ry <= clampRegion(y1); ry++)
    for (let rx = clampRegion(x0); rx <= clampRegion(x1); rx++)
      keys.push(regionKey(rx, ry));
  return keys;
}
