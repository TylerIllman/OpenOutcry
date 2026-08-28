import type { Trade } from "../protocol";
import { formatPrice } from "../protocol";

interface Props {
  trades: Trade[];
  /** Drawn as a dashed line, so you can see where the market was wrong. */
  trueValue: number | null;
  tickSize: number | null;
}

const W = 720;
const H = 260;
const PAD = { top: 18, right: 56, bottom: 26, left: 8 };

/**
 * Every trade in the round, in order, with the settlement value across it.
 *
 * The x axis is trade number rather than wall-clock time: rounds are short and
 * bursty, and spacing by time leaves most of the chart empty with a scribble at
 * one end.
 */
export function PriceChart({ trades, trueValue, tickSize }: Props) {
  if (trades.length === 0) {
    return <p className="tape__empty">Nothing traded, so there is nothing to plot.</p>;
  }

  const prices = trades.map((t) => t.price);
  const candidates = trueValue === null ? prices : [...prices, trueValue];
  const rawMin = Math.min(...candidates);
  const rawMax = Math.max(...candidates);
  // A flat market would otherwise divide by zero and collapse to a single line
  // pinned to the top of the box.
  const pad = (rawMax - rawMin || Math.abs(rawMax) || 1) * 0.15;
  const min = rawMin - pad;
  const max = rawMax + pad;

  const plotW = W - PAD.left - PAD.right;
  const plotH = H - PAD.top - PAD.bottom;

  const x = (i: number) =>
    PAD.left + (trades.length === 1 ? plotW / 2 : (i / (trades.length - 1)) * plotW);
  const y = (p: number) => PAD.top + plotH - ((p - min) / (max - min)) * plotH;

  const path = trades.map((t, i) => `${i === 0 ? "M" : "L"}${x(i)},${y(t.price)}`).join(" ");
  const trueY = trueValue === null ? null : y(trueValue);

  return (
    <figure className="chart">
      <svg viewBox={`0 0 ${W} ${H}`} className="chart__svg" role="img"
           aria-label="Traded price over the course of the round">
        {/* Settlement value */}
        {trueY !== null && (
          <>
            <line x1={PAD.left} x2={W - PAD.right} y1={trueY} y2={trueY}
                  className="chart__true" strokeDasharray="6 5" />
            <text x={W - PAD.right + 8} y={trueY + 4} className="chart__true-label">
              {formatPrice(trueValue as number, tickSize)}
            </text>
          </>
        )}

        <path d={path} className="chart__line" fill="none" />

        {trades.map((t, i) => (
          <circle
            key={t.id}
            cx={x(i)}
            cy={y(t.price)}
            r={trades.length > 60 ? 2.5 : 4}
            className={t.aggressor === "buy" ? "chart__dot chart__dot--buy" : "chart__dot chart__dot--sell"}
          />
        ))}
      </svg>

      <figcaption className="chart__legend">
        <span><i className="swatch swatch--buy" /> bought</span>
        <span><i className="swatch swatch--sell" /> sold</span>
        {trueValue !== null && <span><i className="swatch swatch--true" /> answer</span>}
      </figcaption>
    </figure>
  );
}
