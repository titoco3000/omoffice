//! `floor.png` and `decorations.png` decoded into per-cell grids, shared by
//! everything that needs to know what's on a cell: `MapCanvas` to draw it,
//! `useMapSession` and `MapRelocationProvider` to keep avatars off walls.
//! Decoded once per page load -- an offscreen canvas is the only way to read
//! pixel colors back out of an `<img>`.

import { useEffect, useState } from "react";
import floorUrl from "./assets/floor.png";
import decorationsUrl from "./assets/decorations.png";
import { TILES, TILE_WATER, tileForColor } from "./tiles";

let loading = null;

function loadPixels(url) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const off = document.createElement("canvas");
      off.width = img.width;
      off.height = img.height;
      const octx = off.getContext("2d");
      octx.drawImage(img, 0, 0);
      resolve({
        width: off.width,
        height: off.height,
        data: octx.getImageData(0, 0, off.width, off.height).data,
      });
    };
    img.onerror = () => reject(new Error(`${url} failed to load`));
    img.src = url;
  });
}

/// `{width, height, tiles, decorations}` -- one entry per cell, row by row.
/// `tiles` holds a `TILES` key from `floor.png`; `decorations` holds a
/// `TILES` key from `decorations.png`, or `null` where that pixel is
/// transparent. Rejects if an image fails to load; a later call tries again.
export function loadTileMap() {
  loading ??= Promise.all([loadPixels(floorUrl), loadPixels(decorationsUrl)])
    .then(([floor, decorations]) => {
      const count = floor.width * floor.height;
      const tiles = new Array(count);
      const decorationTiles = new Array(count).fill(null);
      for (let i = 0; i < count; i++) {
        const d = floor.data;
        tiles[i] = tileForColor(d[i * 4], d[i * 4 + 1], d[i * 4 + 2]);
        const e = decorations.data;
        if (e[i * 4 + 3] > 0) {
          decorationTiles[i] = tileForColor(
            e[i * 4],
            e[i * 4 + 1],
            e[i * 4 + 2],
          );
        }
      }
      return {
        width: floor.width,
        height: floor.height,
        tiles,
        decorations: decorationTiles,
      };
    })
    .catch((error) => {
      loading = null;
      throw error;
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
  const i = y * tileMap.width + x;
  return Boolean(
    TILES[tileMap.decorations[i]]?.blocked || TILES[tileMap.tiles[i]]?.blocked,
  );
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
