//! `map.png` decoded into a per-cell terrain grid, shared by everything that
//! needs to know what's on a cell: `MapCanvas` to draw it, `useMapSession`
//! and `MapRelocationProvider` to keep avatars off walls. Decoded once per
//! page load -- an offscreen canvas is the only way to read pixel colors
//! back out of an `<img>`.

import { useEffect, useState } from "react";
import mapUrl from "./assets/map.png";
import { TILES, TILE_WATER, tileForColor } from "./tiles";

let loading = null;

/// `{width, height, tiles}` -- `tiles` holds one `TILES` key per cell, row
/// by row. Rejects if the image fails to load; a later call tries again.
export function loadTileMap() {
  loading ??= new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const off = document.createElement("canvas");
      off.width = img.width;
      off.height = img.height;
      const octx = off.getContext("2d");
      octx.drawImage(img, 0, 0);
      const { data } = octx.getImageData(0, 0, off.width, off.height);
      const tiles = new Array(off.width * off.height);
      for (let i = 0; i < tiles.length; i++) {
        tiles[i] = tileForColor(data[i * 4], data[i * 4 + 1], data[i * 4 + 2]);
      }
      resolve({ width: off.width, height: off.height, tiles });
    };
    img.onerror = () => {
      loading = null;
      reject(new Error("map.png failed to load"));
    };
    img.src = mapUrl;
  });
  return loading;
}

/// The decoded map, `null` until it has loaded.
export function useTileMap() {
  const [tileMap, setTileMap] = useState(null);
  useEffect(() => {
    let cancelled = false;
    loadTileMap()
      .then((loaded) => {
        if (!cancelled) setTileMap(loaded);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);
  return tileMap;
}

/// Whether nobody can stand on cell (x, y): a wall. Off the map
/// counts as open (positions are clamped to the grid separately), and so
/// does everything while `tileMap` is still `null`.
export function isBlocked(tileMap, x, y) {
  if (!tileMap) return false;
  if (x < 0 || y < 0 || x >= tileMap.width || y >= tileMap.height) return false;
  return Boolean(TILES[tileMap.tiles[y * tileMap.width + x]]?.blocked);
}

/// Whether cell (x, y) is water: walkable, but shallow -- avatars standing
/// in it are drawn with their feet cut off (see `MapCanvas`).
export function isWater(tileMap, x, y) {
  if (!tileMap) return false;
  if (x < 0 || y < 0 || x >= tileMap.width || y >= tileMap.height) return false;
  return tileMap.tiles[y * tileMap.width + x] === TILE_WATER;
}

/// The walkable cell closest to `from` (straight-line distance, ties by
/// row then column so every tab agrees), or `null` if the whole map is
/// blocked.
export function nearestOpenCell(tileMap, from) {
  let best = null;
  let bestDistance = Infinity;
  for (let y = 0; y < tileMap.height; y++) {
    for (let x = 0; x < tileMap.width; x++) {
      if (isBlocked(tileMap, x, y)) continue;
      const distance = (x - from.x) ** 2 + (y - from.y) ** 2;
      if (distance < bestDistance) {
        best = { x, y };
        bestDistance = distance;
      }
    }
  }
  return best;
}
