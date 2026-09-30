//! `map.png` (three 100x100 levels side by side) as raw pixels
//! (`{width, height, data}`, see `loadPixels`) and decoded into per-cell
//! grids (`tileMapFromPixels`), shared by everything that needs to know
//! what's on a cell: `MapCanvas` to draw it, `useMapSession` and
//! `MapRelocationProvider` to keep avatars off walls. The pixels are also
//! what the map editor edits (`paintCell`) and saves -- an offscreen canvas
//! is the only way to read pixel colors back out of an `<img>`.

import mapUrl from "./assets/map.png";
import { TILES, TILE_PX, TILE_WATER, paintColor, tileForColor } from "./tiles";

// Levels left to right, each `LEVEL_PX` wide: `base` is the ground, `mid`
// holds walls, tables and the like (blocking or not), `top` holds
// decorations drawn over everything and never blocking.
export const LEVELS = ["base", "mid", "top"];
export const LEVEL_PX = 100;
export const MAP_WIDTH_PX = LEVEL_PX * LEVELS.length;

export function loadPixels(url) {
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

/// The map bundled with the app, used by any org that hasn't saved its own.
export const loadDefaultPixels = () => loadPixels(mapUrl);

/// `blob` (an image) as pixels, e.g. an org's saved map.
export async function pixelsFromBlob(blob) {
  const url = URL.createObjectURL(blob);
  try {
    return await loadPixels(url);
  } finally {
    URL.revokeObjectURL(url);
  }
}

/// `pixels` as a PNG blob, what the editor uploads.
export function pixelsToBlob({ width, height, data }) {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  canvas
    .getContext("2d")
    .putImageData(
      new ImageData(new Uint8ClampedArray(data), width, height),
      0,
      0,
    );
  return new Promise((resolve, reject) =>
    canvas.toBlob(
      (blob) =>
        blob ? resolve(blob) : reject(new Error("could not encode map")),
      "image/png",
    ),
  );
}

export const clonePixels = ({ width, height, data }) => ({
  width,
  height,
  data: new Uint8ClampedArray(data),
});

export function samePixels(a, b) {
  if (a.data.length !== b.data.length) return false;
  for (let i = 0; i < a.data.length; i++) {
    if (a.data[i] !== b.data[i]) return false;
  }
  return true;
}

/// Byte offset in `map.data` of cell (col, row) of level `index`.
function pixelOffset(map, index, col, row) {
  return (row * map.width + index * LEVEL_PX + col) * 4;
}

/// Writes `tile` (a `TILES` key) into cell (col, row) of `level`, or clears
/// it when `tile` is `null` -- transparent on `mid`/`top`, and on `base`
/// (which has no "empty") the water code. Returns whether anything changed.
export function paintCell(map, level, col, row, tile) {
  if (col < 0 || row < 0 || col >= LEVEL_PX || row >= LEVEL_PX) return false;
  const p = pixelOffset(map, LEVELS.indexOf(level), col, row);
  let rgba;
  if (tile) rgba = [...paintColor(tile), 255];
  else rgba = level === "base" ? [0, 0, 0, 255] : [0, 0, 0, 0];
  let changed = false;
  for (let i = 0; i < 4; i++) {
    if (map.data[p + i] !== rgba[i]) {
      map.data[p + i] = rgba[i];
      changed = true;
    }
  }
  return changed;
}

// Pixels of every sprite image, filled by `preloadSprites` -- decoding a map
// is synchronous, so what it needs must already be here.
const spritePixels = new Map();

/// Loads every sprite image `tileMapFromPixels` may need. Call (and await)
/// before it.
export async function preloadSprites() {
  const urls = Object.values(TILES)
    .map((tile) => tile.sprite)
    .filter((url) => url && !spritePixels.has(url));
  await Promise.all(
    urls.map(async (url) => spritePixels.set(url, await loadPixels(url))),
  );
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
function decodeLayer(map, index) {
  const count = LEVEL_PX * LEVEL_PX;
  const cells = new Array(count).fill(null);
  for (let i = 0; i < count; i++) {
    const p = pixelOffset(map, index, i % LEVEL_PX, Math.floor(i / LEVEL_PX));
    if (map.data[p + 3] > 0) {
      cells[i] = tileForColor(map.data[p], map.data[p + 1], map.data[p + 2]);
    }
  }
  const sprites = new Array(count).fill(null);
  spriteCells(sprites, cells, LEVEL_PX, LEVEL_PX, spritePixels);
  return { cells, sprites };
}

/// `{width, height, tiles, mid, top}` -- one entry per cell, row by row.
/// `tiles` holds a `TILES` key from the base level; `mid` and `top` are
/// `{cells, sprites}` (see `decodeLayer`). Needs `preloadSprites` done.
export function tileMapFromPixels(map) {
  if (map.width !== MAP_WIDTH_PX || map.height !== LEVEL_PX) {
    throw new Error(`map must be ${MAP_WIDTH_PX}x${LEVEL_PX}`);
  }
  const tiles = new Array(LEVEL_PX * LEVEL_PX);
  for (let i = 0; i < tiles.length; i++) {
    const p = pixelOffset(map, 0, i % LEVEL_PX, Math.floor(i / LEVEL_PX));
    tiles[i] = tileForColor(map.data[p], map.data[p + 1], map.data[p + 2]);
  }
  return {
    width: LEVEL_PX,
    height: LEVEL_PX,
    tiles,
    mid: decodeLayer(map, 1),
    top: decodeLayer(map, 2),
  };
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
