// The timeline graph, rendered to an SVG string. The same code draws the
// graphs on screen and in Reports, so the two always match.
//
// Colours come from CSS custom properties with print-friendly fallbacks, so
// the SVG works inside the app (themed) and in a standalone Report.

import type { Bucket, Gap } from "./api";
import { date, shortStamp, time, utcOffset } from "./format";

export interface TimelineOptions {
  width: number;
  /** Height of the latency area; the loss strip, axis and legend are added below. */
  height?: number;
  from: number;
  to: number;
  gaps?: Gap[];
  /** Times of Route Changes, marked at the top of the plot. */
  routeChanges?: number[];
  /** Fixed top of the latency scale, so several graphs can share one. */
  yMax?: number;
  fontSize?: number;
}

const LEFT = 50;
const RIGHT = 8;
const TOP = 10;
const LOSS_H = 30;

const STEPS = [
  60e3, 2 * 60e3, 5 * 60e3, 10 * 60e3, 15 * 60e3, 30 * 60e3, 3600e3, 2 * 3600e3, 3 * 3600e3, 6 * 3600e3,
  12 * 3600e3, 24 * 3600e3, 2 * 24 * 3600e3, 7 * 24 * 3600e3,
];

const C = {
  bg: "var(--chart-bg,#fbfbfd)",
  grid: "var(--chart-grid,#d6d9e0)",
  text: "var(--chart-text,#5b6170)",
  avg: "var(--chart-avg,#1a56c4)",
  range: "var(--chart-range,#9db8e8)",
  loss: "var(--chart-loss,#d93025)",
  gap: "var(--chart-gap,#e4e6eb)",
  route: "var(--chart-route,#b26a00)",
};

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

/** The latency scale for one or more graphs: room above the average, with
 * rare spikes allowed to clip. Pass several bucket lists to share a scale. */
export function yScaleFor(...lists: Bucket[][]): number {
  const all = lists.flat();
  const avgs = all.flatMap((b) => (b.avgMs == null ? [] : [b.avgMs]));
  const maxes = all.flatMap((b) => (b.maxMs == null ? [] : [b.maxMs]));
  return niceCeil(Math.max(Math.max(0, ...avgs) * 1.25, percentile(maxes, 0.9), 1));
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
  const fs = opts.fontSize ?? 10;
  const plotW = W - LEFT - RIGHT;
  const lossTop = TOP + H + 12;
  const axisY = lossTop + LOSS_H + fs + 3;
  const legendY = axisY + fs + 8;
  const totalH = legendY + 4;
  const span = Math.max(1, opts.to - opts.from);
  const x = (t: number) => LEFT + ((t - opts.from) / span) * plotW;
  const bw = plotW / Math.max(1, buckets.length);

  const yMax = opts.yMax ?? yScaleFor(buckets);
  const y = (v: number) => TOP + H - (Math.min(v, yMax) / yMax) * H;
  const text = (tx: number, ty: number, s: string, anchor = "end", extra = "") =>
    `<text x="${tx}" y="${ty}" text-anchor="${anchor}" style="fill:${C.text}"${extra}>${s}</text>`;

  const parts: string[] = [];
  parts.push(
    `<svg xmlns="http://www.w3.org/2000/svg" class="timeline" width="${W}" height="${totalH}" viewBox="0 0 ${W} ${totalH}" font-family="system-ui, sans-serif" font-size="${fs}">`,
  );
  parts.push(`<rect x="${LEFT}" y="${TOP}" width="${plotW}" height="${H}" style="fill:${C.bg};stroke:${C.grid}"/>`);
  parts.push(`<rect x="${LEFT}" y="${lossTop}" width="${plotW}" height="${LOSS_H}" style="fill:${C.bg};stroke:${C.grid}"/>`);

  // Y grid and labels.
  for (const v of [0, yMax / 2, yMax]) {
    const yy = y(v);
    parts.push(`<line x1="${LEFT}" x2="${LEFT + plotW}" y1="${yy}" y2="${yy}" style="stroke:${C.grid}" stroke-dasharray="2 3"/>`);
    parts.push(text(LEFT - 5, yy + fs * 0.3, `${Math.round(v)} ms`));
  }
  parts.push(text(LEFT - 5, lossTop + fs * 0.8, "100%"));
  parts.push(text(LEFT - 5, lossTop + LOSS_H, "loss"));

  // Gaps: no Samples were sent (machine asleep, app closed).
  for (const g of opts.gaps ?? []) {
    const x1 = Math.max(LEFT, x(g.start));
    const x2 = Math.min(LEFT + plotW, x(g.end));
    if (x2 > x1) {
      parts.push(`<rect x="${x1}" y="${TOP}" width="${x2 - x1}" height="${lossTop - TOP + LOSS_H}" style="fill:${C.gap}"><title>Gap: no Samples sent</title></rect>`);
      if (x2 - x1 >= 40) parts.push(text((x1 + x2) / 2, TOP + H / 2, "no data", "middle"));
    }
  }

  // Per-bucket min–max range, full-outage shading, clipped spikes and loss
  // bars. The average line only breaks at Gaps, not at empty buckets.
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
      parts.push(`<rect x="${bx}" y="${TOP}" width="${Math.max(bw, 1)}" height="${H}" style="fill:${C.loss}" opacity="0.18"/>`);
    }
    if (b.minMs != null && b.maxMs != null) {
      parts.push(`<line x1="${cx}" x2="${cx}" y1="${y(b.minMs)}" y2="${y(b.maxMs)}" style="stroke:${C.range}" stroke-width="${Math.max(1, bw - 0.5)}"/>`);
      if (b.maxMs > yMax) {
        // Spike above the scale: mark the clip at the top edge.
        parts.push(`<rect x="${bx}" y="${TOP}" width="${Math.max(bw, 1.5)}" height="2.5" style="fill:${C.avg}"><title>Up to ${Math.round(b.maxMs)} ms</title></rect>`);
      }
    }
    if (b.avgMs != null) {
      avgPath += `${penDown ? "L" : "M"}${cx.toFixed(1)} ${y(b.avgMs).toFixed(1)}`;
      penDown = true;
      if (bw >= 6) dots.push(`<circle cx="${cx.toFixed(1)}" cy="${y(b.avgMs).toFixed(1)}" r="2" style="fill:${C.avg}"/>`);
    }
    if (b.lost > 0) {
      const h = (b.lost / b.sent) * LOSS_H;
      parts.push(`<rect x="${bx}" y="${lossTop + LOSS_H - h}" width="${Math.max(bw, 1)}" height="${h}" style="fill:${C.loss}"/>`);
    }
  });
  if (avgPath) {
    parts.push(`<path d="${avgPath}" fill="none" style="stroke:${C.avg}" stroke-width="1.5" stroke-linejoin="round"/>`);
  }
  parts.push(...dots);

  // Route Changes: small markers hanging from the top edge.
  for (const ts of opts.routeChanges ?? []) {
    if (ts < opts.from || ts > opts.to) continue;
    const xx = x(ts);
    parts.push(`<path d="M${xx - 4} ${TOP} L${xx + 4} ${TOP} L${xx} ${TOP + 6} Z" style="fill:${C.route}"><title>Route Change</title></path>`);
  }

  // Time axis. A tick shows its date when it's the first one or a new day.
  const step = STEPS.find((s) => span / s <= 7) ?? STEPS[STEPS.length - 1];
  const tzOffsetMs = new Date(opts.from).getTimezoneOffset() * 60e3;
  let t = Math.ceil((opts.from - tzOffsetMs) / step) * step + tzOffsetMs;
  let lastDate = "";
  let ticks = 0;
  for (; t <= opts.to; t += step) {
    ticks++;
    const xx = x(t);
    parts.push(`<line x1="${xx}" x2="${xx}" y1="${TOP}" y2="${lossTop + LOSS_H}" style="stroke:${C.grid}" opacity="0.6"/>`);
    const d = date(t);
    const label = d !== lastDate || step >= 24 * 3600e3 ? shortStamp(t) : time(t, false);
    lastDate = d;
    parts.push(text(xx, axisY, label, "middle"));
  }
  if (ticks === 0) {
    // Window shorter than one tick step: label both ends instead.
    parts.push(text(LEFT, axisY, time(opts.from), "start"));
    parts.push(text(LEFT + plotW, axisY, time(opts.to)));
  }

  // Legend and time zone, so a printed graph explains itself.
  const items: [string, string][] = [
    [`<line x1="0" x2="14" y1="-3" y2="-3" style="stroke:${C.avg}" stroke-width="2"/>`, "average"],
    [`<rect x="4" y="-8" width="6" height="9" style="fill:${C.range}"/>`, "min–max"],
    [`<rect x="4" y="-8" width="6" height="9" style="fill:${C.loss}"/>`, "packet loss"],
    [`<rect x="2" y="-8" width="10" height="9" style="fill:${C.gap};stroke:${C.grid}"/>`, "no data"],
    [`<path d="M3 -8 L11 -8 L7 -2 Z" style="fill:${C.route}"/>`, "route change"],
  ];
  let lx = LEFT;
  // Narrow graphs (e.g. in the Overview) keep only the time zone.
  const legendWidth = items.reduce((n, [, l]) => n + 32 + l.length * fs * 0.55, 0) + 140;
  for (const [icon, label] of legendWidth <= plotW ? items : []) {
    parts.push(`<g transform="translate(${lx},${legendY})">${icon}</g>`);
    parts.push(text(lx + 18, legendY, label, "start"));
    lx += 18 + label.length * fs * 0.55 + 14;
  }
  parts.push(text(LEFT + plotW, legendY, `Times are UTC${utcOffset(opts.from)}`));

  parts.push(`</svg>`);
  return parts.join("");
}
