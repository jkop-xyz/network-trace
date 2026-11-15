// The timeline graph, rendered to an SVG string. The same code draws the
// graphs on screen and in Reports, so the two always match.
//
// Colours come from CSS custom properties with print-friendly fallbacks, so
// the SVG works inside the app (themed) and in a standalone Report.

import type { Bucket, Gap } from "./api";
import { shortStamp, time } from "./format";

export interface TimelineOptions {
  width: number;
  /** Height of the latency area; the loss strip and axis are added below. */
  height?: number;
  from: number;
  to: number;
  gaps?: Gap[];
}

const LEFT = 46;
const RIGHT = 8;
const TOP = 8;
const LOSS_H = 26;
const AXIS_H = 18;

const STEPS = [
  60e3, 2 * 60e3, 5 * 60e3, 10 * 60e3, 15 * 60e3, 30 * 60e3, 3600e3, 2 * 3600e3, 3 * 3600e3, 6 * 3600e3,
  12 * 3600e3, 24 * 3600e3, 2 * 24 * 3600e3, 7 * 24 * 3600e3,
];

function niceCeil(v: number): number {
  if (v <= 0) return 10;
  const p = 10 ** Math.floor(Math.log10(v));
  for (const m of [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) if (m * p >= v) return m * p;
  return 10 * p;
}

function percentile(values: number[], p: number): number {
  if (!values.length) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
}

/** How many buckets to request for a graph of this pixel width: at least
 * 2 px each, and no more than one per Sample so short windows aren't sparse. */
export function bucketsFor(width: number, spanMs: number, intervalMs: number): number {
  const byWidth = Math.floor((width - LEFT - RIGHT) / 2);
  const bySamples = Math.ceil(spanMs / Math.max(1, intervalMs));
  return Math.max(1, Math.min(1500, byWidth, bySamples));
}

export function timelineSvg(buckets: Bucket[], opts: TimelineOptions): string {
  const W = Math.max(200, Math.round(opts.width));
  const H = opts.height ?? 150;
  const plotW = W - LEFT - RIGHT;
  const totalH = TOP + H + 4 + LOSS_H + AXIS_H;
  const span = Math.max(1, opts.to - opts.from);
  const x = (t: number) => LEFT + ((t - opts.from) / span) * plotW;
  const bw = plotW / Math.max(1, buckets.length);

  const avgs = buckets.flatMap((b) => (b.avgMs == null ? [] : [b.avgMs]));
  const maxes = buckets.flatMap((b) => (b.maxMs == null ? [] : [b.maxMs]));
  const yMax = niceCeil(Math.max(Math.max(0, ...avgs) * 1.25, percentile(maxes, 0.9), 1));
  const y = (v: number) => TOP + H - (Math.min(v, yMax) / yMax) * H;
  const lossTop = TOP + H + 4;

  const parts: string[] = [];
  parts.push(
    `<svg xmlns="http://www.w3.org/2000/svg" class="timeline" width="${W}" height="${totalH}" viewBox="0 0 ${W} ${totalH}" font-family="system-ui, sans-serif" font-size="10">`,
  );
  parts.push(
    `<rect x="${LEFT}" y="${TOP}" width="${plotW}" height="${H}" style="fill:var(--chart-bg,#fbfbfd);stroke:var(--chart-grid,#d6d9e0)"/>`,
  );
  parts.push(
    `<rect x="${LEFT}" y="${lossTop}" width="${plotW}" height="${LOSS_H}" style="fill:var(--chart-bg,#fbfbfd);stroke:var(--chart-grid,#d6d9e0)"/>`,
  );

  // Y grid and labels.
  for (const v of [0, yMax / 2, yMax]) {
    const yy = y(v);
    parts.push(`<line x1="${LEFT}" x2="${LEFT + plotW}" y1="${yy}" y2="${yy}" style="stroke:var(--chart-grid,#d6d9e0)" stroke-dasharray="2 3"/>`);
    parts.push(`<text x="${LEFT - 5}" y="${yy + 3}" text-anchor="end" style="fill:var(--chart-text,#5b6170)">${Math.round(v)} ms</text>`);
  }
  parts.push(`<text x="${LEFT - 5}" y="${lossTop + LOSS_H / 2 + 3}" text-anchor="end" style="fill:var(--chart-text,#5b6170)">loss</text>`);

  // Gaps: no Samples were sent (machine asleep, app closed).
  for (const g of opts.gaps ?? []) {
    const x1 = Math.max(LEFT, x(g.start));
    const x2 = Math.min(LEFT + plotW, x(g.end));
    if (x2 > x1) {
      parts.push(`<rect x="${x1}" y="${TOP}" width="${x2 - x1}" height="${H + 4 + LOSS_H}" style="fill:var(--chart-gap,#e4e6eb)"><title>Gap: no Samples sent</title></rect>`);
    }
  }

  // Per-bucket min–max range, full-outage shading and loss bars. The average
  // line only breaks at Gaps, not at buckets that happen to hold no Sample.
  const inGap = (b: Bucket) => (opts.gaps ?? []).some((g) => b.start < g.end && b.end > g.start);
  let avgPath = "";
  let penDown = false;
  const dots: string[] = [];
  buckets.forEach((b, i) => {
    const bx = LEFT + i * bw;
    if (b.sent === 0) {
      if (inGap(b)) penDown = false;
      return;
    }
    const cx = bx + bw / 2;
    if (b.lost === b.sent) {
      parts.push(`<rect x="${bx}" y="${TOP}" width="${Math.max(bw, 1)}" height="${H}" style="fill:var(--chart-loss,#d93025)" opacity="0.18"/>`);
    }
    if (b.minMs != null && b.maxMs != null) {
      parts.push(
        `<line x1="${cx}" x2="${cx}" y1="${y(b.minMs)}" y2="${y(b.maxMs)}" style="stroke:var(--chart-range,#9db8e8)" stroke-width="${Math.max(1, bw - 0.5)}"/>`,
      );
    }
    if (b.avgMs != null) {
      avgPath += `${penDown ? "L" : "M"}${cx.toFixed(1)} ${y(b.avgMs).toFixed(1)}`;
      penDown = true;
      if (bw >= 6) dots.push(`<circle cx="${cx.toFixed(1)}" cy="${y(b.avgMs).toFixed(1)}" r="2" style="fill:var(--chart-avg,#1a56c4)"/>`);
    }
    if (b.lost > 0) {
      const h = (b.lost / b.sent) * LOSS_H;
      parts.push(`<rect x="${bx}" y="${lossTop + LOSS_H - h}" width="${Math.max(bw, 1)}" height="${h}" style="fill:var(--chart-loss,#d93025)"/>`);
    }
  });
  if (avgPath) {
    parts.push(`<path d="${avgPath}" fill="none" style="stroke:var(--chart-avg,#1a56c4)" stroke-width="1.5" stroke-linejoin="round"/>`);
  }
  parts.push(...dots);

  // Time axis.
  const step = STEPS.find((s) => span / s <= 7) ?? STEPS[STEPS.length - 1];
  const multiDay = span > 20 * 3600e3;
  const tzOffsetMs = new Date(opts.from).getTimezoneOffset() * 60e3;
  // Align ticks to local wall-clock multiples of the step.
  let t = Math.ceil((opts.from - tzOffsetMs) / step) * step + tzOffsetMs;
  const axisY = lossTop + LOSS_H + 12;
  for (; t <= opts.to; t += step) {
    const xx = x(t);
    parts.push(`<line x1="${xx}" x2="${xx}" y1="${TOP}" y2="${lossTop + LOSS_H}" style="stroke:var(--chart-grid,#d6d9e0)" opacity="0.6"/>`);
    const label = multiDay || step >= 24 * 3600e3 ? shortStamp(t) : time(t, false);
    parts.push(`<text x="${xx}" y="${axisY}" text-anchor="middle" style="fill:var(--chart-text,#5b6170)">${label}</text>`);
  }

  parts.push(`</svg>`);
  return parts.join("");
}
