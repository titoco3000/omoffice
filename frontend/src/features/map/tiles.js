import grassUrl from "./assets/floor/grass.png";
import sandUrl from "./assets/floor/sand.png";
import waterUrl from "./assets/floor/water.png";
import plankLightUrl from "./assets/floor/plank-light.png";
import plankMediumUrl from "./assets/floor/plank-medium.png";
import purpleFlowerUrl from "./assets/decorations/purple-flower.png";
import tableUrl from "./assets/decorations/table-2x2.png";
import wallBlueTopUrl from "./assets/decorations/wall/blue/top.png";
import wallBlueMidUrl from "./assets/decorations/wall/blue/mid.png";
import wallBlueBottomUrl from "./assets/decorations/wall/blue/bottom.png";
import wallCreamTopUrl from "./assets/decorations/wall/cream/top.png";
import wallCreamMidUrl from "./assets/decorations/wall/cream/mid.png";
import wallCreamBottomUrl from "./assets/decorations/wall/cream/bottom.png";
import topUrl from "./assets/transitions/top.png";
import topLeftUrl from "./assets/transitions/top-left.png";
import outerTopLeftUrl from "./assets/transitions/outer-top-left.png";

// One pixel of `floor.png` (and of `decorations.png`, where opaque) per cell --
// its color picks the tile drawn in that cell. Anything not listed (including black) is water, so an unassigned
// color reads as water rather than crashing.
//
// `organic` tiles blend into one another: the tile with the higher `rank`
// bleeds over the lower one's edge through the masks in `assets/transitions`.
// Non-organic ("artificial") tiles have no transition.
export const TILE_WATER = "water";
export const TILES = {
  water: { url: waterUrl, organic: true, rank: 0 },
  sand: { url: sandUrl, organic: true, rank: 1, color: [255, 255, 0] },
  grass: { url: grassUrl, organic: true, rank: 2, color: [0, 255, 0] },
  // The plank images carry a 1px darker column either side of the 16px
  // body, so only that body is drawn (`src`: x, y, w, h).
  "plank-light": {
    url: plankLightUrl,
    organic: false,
    color: [255, 128, 0],
    src: [1, 0, 16, 16],
  },
  "plank-medium": {
    url: plankMediumUrl,
    organic: false,
    color: [128, 64, 0],
    src: [1, 0, 16, 16],
  },
  // Sprite decorations: one map pixel marks the sprite's top-left cell and
  // the image spans as many cells as it needs (16px each, so it may be
  // larger than 1x1 or an odd shape). See `spriteCells` in `tileMap.js`.
  "purple-flower": {
    organic: false,
    color: [128, 0, 255],
    sprite: purpleFlowerUrl,
  },
  "table-2x2": {
    organic: false,
    blocked: true,
    color: [192, 128, 64],
    sprite: tableUrl,
  },
  // Wall variants: one map color each, drawn as `top`/`mid`/`bottom` by
  // what sits below the cell (see `wallPart`).
  "wall-blue": {
    organic: false,
    blocked: true,
    color: [0, 0, 255],
    wall: {
      top: wallBlueTopUrl,
      mid: wallBlueMidUrl,
      bottom: wallBlueBottomUrl,
    },
  },
  "wall-cream": {
    organic: false,
    blocked: true,
    color: [255, 255, 128],
    wall: {
      top: wallCreamTopUrl,
      mid: wallCreamMidUrl,
      bottom: wallCreamBottomUrl,
    },
  },
};

export function tileForColor(r, g, b) {
  for (const [name, { color }] of Object.entries(TILES)) {
    if (color && color[0] === r && color[1] === g && color[2] === b) {
      return name;
    }
  }
  return TILE_WATER;
}

// Drawn under the grid and avatars, on top of every base tile.
const ORGANIC_BY_RANK = Object.entries(TILES)
  .filter(([, tile]) => tile.organic)
  .sort((a, b) => a[1].rank - b[1].rank)
  .map(([name]) => name);

export const TILE_PX = 16; // one map cell in source images
const MASK_PX = TILE_PX; // transitions and organic tiles are 16x16

// Neighbor slots, clockwise from north: N, NE, E, SE, S, SW, W, NW.
const NEIGHBOR_OFFSETS = [
  [0, -1],
  [1, -1],
  [1, 0],
  [1, 1],
  [0, 1],
  [-1, 1],
  [-1, 0],
  [-1, -1],
];

// Draws `img` rotated `quarter` turns clockwise about the mask's center,
// offset by (dx, dy) in the image's own (unrotated) frame.
function drawRotated(ctx, img, quarter, dx, dy) {
  ctx.save();
  ctx.translate(MASK_PX / 2, MASK_PX / 2);
  ctx.rotate((quarter * Math.PI) / 2);
  ctx.translate(-MASK_PX / 2, -MASK_PX / 2);
  ctx.drawImage(img, dx, dy);
  ctx.restore();
}

// Builds a `MASK_PX` square canvas holding `tile`'s texture, visible only
// where the neighbor pattern `same` (which of the 8 neighbors are `tile`)
// calls for a transition. Null while any image is still loading.
function buildBlend(tile, same, getImage) {
  const top = getImage(topUrl);
  const corner = getImage(topLeftUrl);
  const outer = getImage(outerTopLeftUrl);
  const texture = getImage(TILES[tile].url);
  if (!top || !corner || !outer || !texture) return null;

  const mask = document.createElement("canvas");
  mask.width = mask.height = MASK_PX;
  const mctx = mask.getContext("2d");
  mctx.imageSmoothingEnabled = false;

  for (let quarter = 0; quarter < 4; quarter++) {
    // Rotating the top-left corner clockwise walks it round to top-right,
    // bottom-right, bottom-left: the sides it joins are `quarter` and the
    // one before it, the diagonal is the matching corner slot.
    const sideA = quarter * 2;
    const sideB = (quarter * 2 + 6) % 8;
    const diagonal = (quarter * 2 + 7) % 8;
    if (same[sideA]) drawRotated(mctx, top, quarter, 0, 0);
    if (same[sideA] && same[sideB]) {
      drawRotated(mctx, outer, quarter, 0, 0);
    } else if (same[diagonal] && !same[sideA] && !same[sideB]) {
      drawRotated(mctx, corner, quarter, 0, 0);
    }
  }

  // Overlapping masks would compound their 50% alpha; flatten to one level.
  const pixels = mctx.getImageData(0, 0, MASK_PX, MASK_PX);
  let level = 0;
  for (let i = 3; i < pixels.data.length; i += 4) {
    level = Math.max(level, pixels.data[i]);
  }
  for (let i = 3; i < pixels.data.length; i += 4) {
    pixels.data[i] = pixels.data[i] > 0 ? level : 0;
  }
  mctx.putImageData(pixels, 0, 0);

  const out = document.createElement("canvas");
  out.width = out.height = MASK_PX;
  const octx = out.getContext("2d");
  octx.imageSmoothingEnabled = false;
  octx.drawImage(texture, 0, 0, MASK_PX, MASK_PX);
  octx.globalCompositeOperation = "destination-in";
  octx.drawImage(mask, 0, 0);
  return out;
}

// Returns `{ drawBase, drawTransitions, drawDecorations }`, all taking `(ctx, tileMap, col,
// row, x0, y0, x1, y1)` for one cell's on-screen rect. Call `drawBase` for
// every visible cell first, then `drawTransitions` for every one, so a
// transition is never covered by a later cell's base tile; `drawDecorations`
// goes last, over both.
export function createTileRenderer(getImage) {
  const blends = new Map(); // `${tile}|${same bitmask}` -> canvas

  const tileAt = (tileMap, col, row) =>
    col < 0 || row < 0 || col >= tileMap.width || row >= tileMap.height
      ? null
      : tileMap.tiles[row * tileMap.width + col];

  const decorationAt = (tileMap, col, row) =>
    col < 0 || row < 0 || col >= tileMap.width || row >= tileMap.height
      ? null
      : tileMap.decorations[row * tileMap.width + col];

  // Off-map neighbors count as the cell's own tile: no edge to draw.
  const neighbors = (tileMap, col, row, own) =>
    NEIGHBOR_OFFSETS.map(
      ([dx, dy]) => tileAt(tileMap, col + dx, row + dy) ?? own,
    );

  // A wall column reads bottom-up: the cell with no same wall below is the
  // bottom, the one above a bottom is the mid, every cell above that a top.
  const wallPart = (tileMap, col, row, tile) => {
    if (decorationAt(tileMap, col, row + 1) !== tile) return "bottom";
    return decorationAt(tileMap, col, row + 2) !== tile ? "mid" : "top";
  };

  const drawBase = (ctx, tileMap, col, row, x0, y0, x1, y1) => {
    const tile = tileAt(tileMap, col, row);
    const image = getImage(TILES[tile].url);
    if (!image) return;
    const src = TILES[tile].src;
    if (src) {
      ctx.drawImage(image, ...src, x0, y0, x1 - x0, y1 - y0);
    } else {
      ctx.drawImage(image, x0, y0, x1 - x0, y1 - y0);
    }
  };

  const drawDecorations = (ctx, tileMap, col, row, x0, y0, x1, y1) => {
    const piece = tileMap.sprites[row * tileMap.width + col];
    if (piece) {
      // This cell's slice of a sprite anchored elsewhere (or here); a slice
      // hanging past the image's edge is drawn proportionally short.
      const image = getImage(TILES[piece.tile].sprite);
      if (!image) return;
      const sx = piece.dx * TILE_PX;
      const sy = piece.dy * TILE_PX;
      const sw = Math.min(TILE_PX, image.naturalWidth - sx);
      const sh = Math.min(TILE_PX, image.naturalHeight - sy);
      ctx.drawImage(
        image,
        sx,
        sy,
        sw,
        sh,
        x0,
        y0,
        ((x1 - x0) * sw) / TILE_PX,
        ((y1 - y0) * sh) / TILE_PX,
      );
    }
    const tile = decorationAt(tileMap, col, row);
    const wall = tile && TILES[tile].wall;
    if (!wall) return;
    const image = getImage(wall[wallPart(tileMap, col, row, tile)]);
    if (!image) return;
    ctx.drawImage(image, x0, y0, x1 - x0, y1 - y0);
  };

  const drawTransitions = (ctx, tileMap, col, row, x0, y0, x1, y1) => {
    const tile = tileAt(tileMap, col, row);
    if (!TILES[tile].organic) return;
    const around = neighbors(tileMap, col, row, tile);

    for (const other of ORGANIC_BY_RANK) {
      if (TILES[other].rank <= TILES[tile].rank) continue;
      const same = around.map((n) => n === other);
      if (!same.includes(true)) continue;
      const key = `${other}|${same.map(Number).join("")}`;
      let blend = blends.get(key);
      if (!blend) {
        blend = buildBlend(other, same, getImage);
        if (!blend) continue;
        blends.set(key, blend);
      }
      ctx.drawImage(blend, x0, y0, x1 - x0, y1 - y0);
    }
  };

  return { drawBase, drawTransitions, drawDecorations };
}
