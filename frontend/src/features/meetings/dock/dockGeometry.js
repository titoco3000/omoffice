//! Desktop-only: the eight places a dragged dock may rest are the 3x3 grid
//! from dev-docs/phase/6.md with its center square dropped (that's where the
//! page content lives).

export const SNAP_MARGIN = 16; // px between the panel and the viewport edge when docked

// Given the panel's own size, where a named slot puts its top-left corner.
// Kept in plain top/left pixels (never right/bottom, never a translate) so
// every slot animates through the same two CSS properties.
export function slotPosition(slot, size) {
  const [row, col] = slot.split("-");
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const left =
    col === "left"
      ? SNAP_MARGIN
      : col === "right"
        ? vw - size.width - SNAP_MARGIN
        : (vw - size.width) / 2;
  const top =
    row === "top"
      ? SNAP_MARGIN
      : row === "bot"
        ? vh - size.height - SNAP_MARGIN
        : (vh - size.height) / 2;
  return { top, left };
}

// Which slot a dropped panel center is closest to. The viewport is divided
// into thirds each way; "mid-center" isn't a real slot, so a drop there
// falls back to whichever side of the screen it's nearer to.
export function nearestSlot(centerX, centerY) {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const col =
    centerX < vw / 3 ? "left" : centerX < (2 * vw) / 3 ? "center" : "right";
  const row = centerY < vh / 3 ? "top" : centerY < (2 * vh) / 3 ? "mid" : "bot";
  if (row === "mid" && col === "center") {
    return centerX < vw / 2 ? "mid-left" : "mid-right";
  }
  return `${row}-${col}`;
}
