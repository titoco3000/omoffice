import grassUrl from "./assets/tiles/grass.png";
import sandUrl from "./assets/tiles/sand.png";
import waterUrl from "./assets/tiles/water.png";
import plankLightUrl from "./assets/tiles/plank-light.png";
import plankMediumUrl from "./assets/tiles/plank-medium.png";
import topUrl from "./assets/transitions/top.png";
import topLeftUrl from "./assets/transitions/top-left.png";
import outerTopLeftUrl from "./assets/transitions/outer-top-left.png";

// One pixel of `map.png` per cell -- its color picks the tile drawn in that
// cell. Anything not listed (including black) is water, so an unassigned
// color reads as water rather than crashing.
//
// `organic` tiles blend into one another: the tile with the higher `rank`
// bleeds over the lower one's edge through the masks in `assets/transitions`.
// Non-organic ("artificial") tiles instead get a gray + black border line
// wherever they meet a different tile.
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

const MASK_PX = 16; // transitions and organic tiles are 16x16
const EDGE_LINE_GRAY = "#8a8a8a";
const EDGE_LINE_BLACK = "#000000";

// Neighbor slots, clockwise from north.
const N = 0;
const NE = 1;
const E = 2;
const SE = 3;
const S = 4;
const SW = 5;
const W = 6;
const NW = 7;
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

// Returns `{ drawBase, drawTransitions }`, both taking `(ctx, tileMap, col,
// row, x0, y0, x1, y1)` for one cell's on-screen rect. Call `drawBase` for
// every visible cell first, then `drawTransitions` for every one, so a
// transition is never covered by a later cell's base tile.
export function createTileRenderer(getImage) {
  const blends = new Map(); // `${tile}|${same bitmask}` -> canvas

  const tileAt = (tileMap, col, row) =>
    col < 0 || row < 0 || col >= tileMap.width || row >= tileMap.height
      ? null
      : tileMap.tiles[row * tileMap.width + col];

  // Off-map neighbors count as the cell's own tile: no edge to draw.
  const neighbors = (tileMap, col, row, own) =>
    NEIGHBOR_OFFSETS.map(
      ([dx, dy]) => tileAt(tileMap, col + dx, row + dy) ?? own,
    );

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

  const drawTransitions = (ctx, tileMap, col, row, x0, y0, x1, y1) => {
    const tile = tileAt(tileMap, col, row);
    const around = neighbors(tileMap, col, row, tile);

    if (TILES[tile].organic) {
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
      return;
    }

    // Artificial tile: a gray line inside a black one along every edge that
    // meets a different tile. Gray goes down first so the black wins where
    // two edges cross at a corner. Between two artificial tiles the lines
    // are split across the boundary as gray - black - gray: the cell
    // on the north/west side draws gray then black at its S/E edge, and the
    // cell beyond adds one gray line at its own N/W edge.
    const px = Math.max(1, Math.round((x1 - x0) / MASK_PX));
    const open = (slot) => around[slot] !== tile;
    const artificial = (slot) => !TILES[around[slot]].organic;
    const rect = (slot, width, inset, color) => {
      if (!open(slot)) return;
      ctx.fillStyle = color;
      switch (slot) {
        case N:
          ctx.fillRect(x0, y0 + inset, x1 - x0, width);
          break;
        case S:
          ctx.fillRect(x0, y1 - inset - width, x1 - x0, width);
          break;
        case W:
          ctx.fillRect(x0 + inset, y0, width, y1 - y0);
          break;
        default:
          ctx.fillRect(x1 - inset - width, y0, width, y1 - y0);
      }
    };
    const ownsBlack = (slot) => slot === S || slot === E;
    for (const slot of [N, E, S, W]) {
      if (!artificial(slot) || ownsBlack(slot)) {
        rect(slot, px, px, EDGE_LINE_GRAY);
      }
    }
    for (const slot of [N, E, S, W]) {
      if (!artificial(slot)) rect(slot, px, 0, EDGE_LINE_BLACK);
      else if (ownsBlack(slot)) rect(slot, px, 0, EDGE_LINE_BLACK);
      else rect(slot, px, 0, EDGE_LINE_GRAY);
    }

    // Inside corner: both sides continue but the diagonal doesn't. Black
    // dot in the corner, gray around it, to close the line's turn.
    const corners = [
      [NW, N, W, x0, y0, 1, 1],
      [NE, N, E, x1, y0, -1, 1],
      [SE, S, E, x1, y1, -1, -1],
      [SW, S, W, x0, y1, 1, -1],
    ];
    for (const [diag, a, b, cx, cy, sx, sy] of corners) {
      if (!open(diag) || open(a) || open(b)) continue;
      const cell = (i, j, color) => {
        ctx.fillStyle = color;
        const ox = sx > 0 ? cx + i * px : cx - (i + 1) * px;
        const oy = sy > 0 ? cy + j * px : cy - (j + 1) * px;
        ctx.fillRect(ox, oy, px, px);
      };
      // The diagonal tile's own S/E black already sits next to this corner
      // when it is another artificial tile, leaving just the gray that
      // closes the gray - black - gray run.
      if (diag === NW && artificial(diag)) {
        cell(0, 0, EDGE_LINE_GRAY);
        continue;
      }
      cell(1, 0, EDGE_LINE_GRAY);
      cell(0, 1, EDGE_LINE_GRAY);
      cell(1, 1, EDGE_LINE_GRAY);
      cell(0, 0, EDGE_LINE_BLACK);
    }
  };

  return { drawBase, drawTransitions };
}
