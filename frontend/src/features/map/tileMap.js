//! `map.png` (three 100x100 levels side by side) decoded into per-cell grids, shared by
//! everything that needs to know what's on a cell: `MapCanvas` to draw it,
//! `useMapSession` and `MapRelocationProvider` to keep avatars off walls.
//! Decoded once per page load -- an offscreen canvas is the only way to read
//! pixel colors back out of an `<img>`.

import { useEffect, useState } from "react";
import mapUrl from "./assets/map.png";
import { TILES, TILE_PX, TILE_WATER, tileForColor } from "./tiles";

// Levels left to right, each `LEVEL_PX` wide: `base` is the ground, `mid`
// holds walls, tables and the like (blocking or not), `top` holds
// decorations drawn over everything and never blocking.
const LEVEL_PX = 100;

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

/// Each anchored sprite's footprint: for every cell of its image holding an
/// opaque pixel (so odd shapes cover only what they touch), `{tile, dx, dy}`
/// -- which sprite, and which 16px slice of it -- written into `sprites`.
function spriteCells(sprites, decorationTiles, width, height, images) {
  for (let i = 0; i < decorationTiles.length; i++) {
    const tile = decorationTiles[i];
    const image = tile && images.get(TILES[tile].sprite);
    if (!image) continue;
    const col = i % width;
    const row = Math.floor(i / width);
    for (let dy = 0; dy * TILE_PX < image.height; dy++) {
      for (let dx = 0; dx * TILE_PX < image.width; dx++) {
        if (col + dx >= width || row + dy >= height) continue;
        let opaque = false;
        for (
          let y = dy * TILE_PX;
          y < Math.min(image.height, (dy + 1) * TILE_PX);
          y++
        ) {
          for (
            let x = dx * TILE_PX;
            x < Math.min(image.width, (dx + 1) * TILE_PX);
            x++
          ) {
            if (image.data[(y * image.width + x) * 4 + 3] > 0) opaque = true;
          }
        }
        if (opaque) sprites[(row + dy) * width + col + dx] = { tile, dx, dy };
      }
    }
  }
}

/// One of `mid`/`top`, read from the `index`th `LEVEL_PX` strip of `map`:
/// `{cells, sprites}` -- `cells` holds a `TILES` key per cell, or `null`
/// where that pixel is transparent (a sprite's anchor cell, or a wall);
/// `sprites` holds the slice of a sprite covering that cell, or `null`.
async function decodeLayer(map, index) {
  const count = LEVEL_PX * LEVEL_PX;
  const cells = new Array(count).fill(null);
  for (let i = 0; i < count; i++) {
    const p = pixelIndex(map, index, i);
    if (map.data[p + 3] > 0) {
      cells[i] = tileForColor(map.data[p], map.data[p + 1], map.data[p + 2]);
    }
  }
  const urls = new Set(cells.map((d) => d && TILES[d].sprite).filter(Boolean));
  const images = new Map(
    await Promise.all(
      [...urls].map(async (url) => [url, await loadPixels(url)]),
    ),
  );
  const sprites = new Array(count).fill(null);
  spriteCells(sprites, cells, LEVEL_PX, LEVEL_PX, images);
  return { cells, sprites };
}

/// Byte offset in `map.data` of cell `i` (row by row) of level `index`.
function pixelIndex(map, index, i) {
  const x = index * LEVEL_PX + (i % LEVEL_PX);
  const y = Math.floor(i / LEVEL_PX);
  return (y * map.width + x) * 4;
}

/// `{width, height, tiles, mid, top}` -- one entry per cell, row by row.
/// `tiles` holds a `TILES` key from the base level; `mid` and `top` are
/// `{cells, sprites}` (see `decodeLayer`). Rejects if the image fails to load;
/// a later call tries again.
export function loadTileMap() {
  loading ??= loadPixels(mapUrl)
    .then(async (map) => {
      if (map.width !== LEVEL_PX * 3 || map.height !== LEVEL_PX) {
        throw new Error(`map.png must be ${LEVEL_PX * 3}x${LEVEL_PX}`);
      }
      const tiles = new Array(LEVEL_PX * LEVEL_PX);
      for (let i = 0; i < tiles.length; i++) {
        const p = pixelIndex(map, 0, i);
        tiles[i] = tileForColor(map.data[p], map.data[p + 1], map.data[p + 2]);
      }
      return {
        width: LEVEL_PX,
        height: LEVEL_PX,
        tiles,
        mid: await decodeLayer(map, 1),
        top: await decodeLayer(map, 2),
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
    TILES[tileMap.mid.cells[i]]?.blocked ||
    TILES[tileMap.mid.sprites[i]?.tile]?.blocked ||
    TILES[tileMap.tiles[i]]?.blocked,
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
