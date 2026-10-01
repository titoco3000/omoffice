//! Ranked composition breakdown of several values.
//!
//! `items` is `[{label, composition: {part: number}}]`: one stacked bar per
//! item, one segment per part. The parts are listed on the left, ranked by
//! their average across items (a part an item lacks counts as 0), and the
//! stacks use the same top-to-bottom order. An orthogonal connector (only
//! horizontal and vertical runs) leaves each part's label for its segment in
//! the first bar that has one, then continues segment to segment through the
//! following bars. Each bar's total is printed centered on it. Hovering (or
//! focusing) a part's label bolds it, dims everything else and prints that
//! part's value on each bar in place of the total.
//!
//! `colors` is an optional `{part: cssColor}`. A part without one takes the
//! next categorical slot (`--om-cat-N`) in order of first appearance, never
//! its rank, so a colour follows its part when the ranking changes; past the
//! eight slots a part is neutral gray.
import { useMemo, useState } from "react";

const SLOTS = 8;
const OVERFLOW_COLOR = "var(--om-ink-faint)";
const ROW_H = 24;
const BAR_W = 56;
const CHANNEL_W = 8;
const SEG_GAP = 2;
const TOP = 32;
const MIN_PLOT_H = 220;
const PAD = 16;
const CORNER = 4;
// The graph scales down with its container, but not below this (text stays
// legible); a narrower container scrolls instead.
const MIN_WIDTH = 480;

const defaultFormat = (v) => String(Math.round(v * 100) / 100);

// Segment with its top corners rounded, bottom square.
function roundedTop(x, y, w, h, r) {
  const k = Math.min(r, h / 2);
  return `M${x} ${y + h}V${y + k}Q${x} ${y} ${x + k} ${y}H${x + w - k}Q${x + w} ${y} ${x + w} ${y + k}V${y + h}Z`;
}

function layout(items, colors) {
  const keys = [];
  for (const item of items) {
    for (const [key, value] of Object.entries(item.composition)) {
      if (value > 0 && !keys.includes(key)) keys.push(key);
    }
  }
  const average = (key) =>
    items.reduce((sum, it) => sum + Math.max(0, it.composition[key] ?? 0), 0) /
    (items.length || 1);
  const ranked = [...keys].sort((a, b) => average(b) - average(a));
  const colorOf = Object.fromEntries(
    keys.map((key, i) => [
      key,
      colors?.[key] ?? (i < SLOTS ? `var(--om-cat-${i + 1})` : OVERFLOW_COLOR),
    ]),
  );

  const labelW = Math.min(
    220,
    Math.max(80, Math.max(0, ...keys.map((k) => k.length)) * 7 + 28),
  );
  const plotH = Math.max(MIN_PLOT_H, ranked.length * ROW_H);
  const bottom = TOP + plotH;
  const totals = items.map((it) =>
    ranked.reduce((sum, key) => sum + Math.max(0, it.composition[key] ?? 0), 0),
  );
  const scale = plotH / (Math.max(0, ...totals) || 1);

  // Segments, stacked in rank order from the top of each bar.
  const bars = items.map((item, b) => {
    let y = bottom - totals[b] * scale;
    const segments = {};
    for (const key of ranked) {
      const value = item.composition[key] ?? 0;
      if (!(value > 0)) continue;
      const h = value * scale;
      segments[key] = { value, y, h, mid: y + h / 2 };
      y += h;
    }
    return {
      label: item.label,
      segments,
      total: totals[b],
      mid: bottom - (totals[b] * scale) / 2,
    };
  });

  // Connectors: label -> first bar with the part, then bar to next bar with it.
  const rowY = Object.fromEntries(
    ranked.map((key, i) => [key, TOP + i * ROW_H + ROW_H / 2]),
  );
  const arrivals = bars.map(() => []);
  ranked.forEach((key, rank) => {
    let from = { y: rowY[key] };
    bars.forEach((bar, b) => {
      const seg = bar.segments[key];
      if (!seg) return;
      arrivals[b].push({ key, rank, from, ty: seg.mid });
      from = { bar: b, y: seg.mid };
    });
  });

  // One vertical channel per arriving connector in the gap before its bar.
  // Ordered so that connectors going the same way never cross: when going
  // down, higher-ranked parts turn closer to the bar; when going up, the
  // reverse.
  let cursor = labelW + PAD;
  const gapX = [];
  const barX = [];
  const channels = arrivals.map((list) => {
    const down = list
      .filter((a) => a.ty >= a.from.y)
      .sort((a, b) => b.rank - a.rank);
    const up = list
      .filter((a) => a.ty < a.from.y)
      .sort((a, b) => a.rank - b.rank);
    return [...down, ...up];
  });
  channels.forEach((list) => {
    gapX.push(cursor);
    cursor += Math.max(32, list.length * CHANNEL_W + 24);
    barX.push(cursor);
    cursor += BAR_W;
  });

  const connectors = [];
  channels.forEach((list, b) => {
    list.forEach((arrival, c) => {
      const sx =
        arrival.from.bar === undefined
          ? labelW
          : barX[arrival.from.bar] + BAR_W;
      const cx = gapX[b] + 12 + c * CHANNEL_W;
      connectors.push({
        key: arrival.key,
        d: `M${sx} ${arrival.from.y}H${cx}V${arrival.ty}H${barX[b]}`,
      });
    });
  });

  return {
    ranked,
    colorOf,
    rowY,
    labelW,
    bars,
    barX,
    connectors,
    width: cursor + PAD,
    height: bottom + PAD / 2,
  };
}

export default function ValueCompositionGraph({
  items,
  colors,
  formatValue = defaultFormat,
  className = "",
}) {
  const [active, setActive] = useState(null);
  const g = useMemo(() => layout(items, colors), [items, colors]);
  if (g.ranked.length === 0) return null;

  const dim = (key) => (active && active !== key ? 0.2 : 1);

  return (
    <div className={`overflow-x-auto ${className}`}>
      <svg
        viewBox={`0 0 ${g.width} ${g.height}`}
        style={{
          width: "100%",
          height: "auto",
          maxWidth: g.width,
          minWidth: Math.min(g.width, MIN_WIDTH),
        }}
        role="img"
        aria-label="Composition of each value"
      >
        <g fill="none" strokeLinejoin="round">
          {g.connectors.map((c, i) => (
            <path
              key={i}
              d={c.d}
              stroke={g.colorOf[c.key]}
              strokeWidth={active === c.key ? 2.5 : 1.5}
              opacity={active && active !== c.key ? 0.15 : 0.9}
            />
          ))}
        </g>

        {g.bars.map((bar, b) => (
          <g key={b}>
            <text
              x={g.barX[b] + BAR_W / 2}
              y={TOP - 10}
              textAnchor="middle"
              className="fill-ink text-xs"
            >
              {bar.label}
            </text>
            {g.ranked.map((key, i) => {
              const seg = bar.segments[key];
              if (!seg) return null;
              const first = g.ranked.slice(0, i).every((k) => !bar.segments[k]);
              const y = seg.y + SEG_GAP / 2;
              const h = Math.max(1, seg.h - SEG_GAP);
              return first ? (
                <path
                  key={key}
                  d={roundedTop(g.barX[b], y, BAR_W, h, CORNER)}
                  fill={g.colorOf[key]}
                  opacity={dim(key)}
                />
              ) : (
                <rect
                  key={key}
                  x={g.barX[b]}
                  y={y}
                  width={BAR_W}
                  height={h}
                  fill={g.colorOf[key]}
                  opacity={dim(key)}
                />
              );
            })}
            {!active && bar.total > 0 && (
              <text
                x={g.barX[b] + BAR_W / 2}
                y={bar.mid}
                textAnchor="middle"
                dominantBaseline="central"
                strokeWidth={3}
                style={{ paintOrder: "stroke" }}
                className="fill-ink stroke-surface text-xs font-medium"
              >
                {formatValue(bar.total)}
              </text>
            )}
            {active && bar.segments[active] && (
              <text
                x={g.barX[b] + BAR_W / 2}
                y={bar.segments[active].mid}
                textAnchor="middle"
                dominantBaseline="central"
                strokeWidth={3}
                style={{ paintOrder: "stroke" }}
                className="fill-ink stroke-surface text-xs font-medium"
              >
                {formatValue(bar.segments[active].value)}
              </text>
            )}
          </g>
        ))}

        {g.ranked.map((key) => (
          <g
            key={key}
            tabIndex={0}
            onMouseEnter={() => setActive(key)}
            onMouseLeave={() => setActive(null)}
            onFocus={() => setActive(key)}
            onBlur={() => setActive(null)}
            className="cursor-default outline-none"
          >
            <rect
              x={0}
              y={g.rowY[key] - ROW_H / 2}
              width={g.labelW}
              height={ROW_H}
              fill="transparent"
            />
            <circle cx={6} cy={g.rowY[key]} r={4} fill={g.colorOf[key]} />
            <text
              x={16}
              y={g.rowY[key]}
              dominantBaseline="central"
              fontWeight={active === key ? 700 : 400}
              className="fill-ink text-xs"
            >
              {key}
            </text>
          </g>
        ))}
      </svg>
    </div>
  );
}
