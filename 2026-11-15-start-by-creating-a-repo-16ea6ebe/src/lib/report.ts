// The Report: a self-contained HTML page made for sending to an ISP.
//
// It is assembled from independent sections (REPORT_SECTIONS). To change the
// Report, add, remove or reorder sections there; each one only reads
// ReportData and returns HTML.

import type { Bucket, HopStats, ReportContext, SessionView } from "./api";
import { api } from "./api";
import { bucketsFor, timelineSvg } from "./chart";
import { dateTime, duration, escapeHtml as esc, interval, ms, pct } from "./format";
import type { ResolvedWindow } from "./timeWindow";

export const REPORT_WIDTH = 960;

export interface ReportData {
  view: SessionView;
  window: ResolvedWindow;
  finalTimeline: Bucket[];
  /** Graphs for Hops with notable loss, worst first. */
  hopTimelines: { hop: HopStats; buckets: Bucket[] }[];
  context: ReportContext;
  generatedAt: number;
}

export type ReportSection = (d: ReportData) => string;

const finalHop = (d: ReportData) => d.view.hops[d.view.hops.length - 1];
const hopLabel = (h: HopStats, d: ReportData) => {
  const name = h.address ? d.view.names[h.address] : undefined;
  return h.address ? `${esc(h.address)}${name ? ` <span class="muted">(${esc(name)})</span>` : ""}` : "<span class=\"muted\">no reply</span>";
};

/** Hops (other than the Target) whose loss is worth showing as its own graph. */
export function notableHops(hops: HopStats[]): HopStats[] {
  const final = hops[hops.length - 1];
  return hops
    .filter((h) => h !== final && h.sent >= 5 && h.lossPct >= 1)
    .sort((a, b) => b.lossPct - a.lossPct)
    .slice(0, 5)
    .sort((a, b) => a.hop - b.hop);
}

const header: ReportSection = (d) => `
  <header>
    <h1>Network trace report: ${esc(d.view.meta.target)}</h1>
    <p class="muted">Generated ${esc(dateTime(d.generatedAt))} by network-trace ${esc(d.context.appVersion)}</p>
  </header>`;

const details: ReportSection = (d) => {
  const m = d.view.meta;
  const rows: [string, string][] = [
    ["Target", `${esc(m.target)}${m.target !== m.address ? ` (${esc(m.address)})` : ""}`],
    ["Time window", `${esc(d.window.label)}: ${esc(dateTime(d.window.from))} to ${esc(dateTime(d.window.to))} (${duration(d.window.to - d.window.from)})`],
    ["Session", `started ${esc(dateTime(m.startedAt))}${m.stoppedAt ? `, stopped ${esc(dateTime(m.stoppedAt))}` : ", still running"}`],
    ["Sample interval", `${interval(m.intervalMs)}, one ICMP echo per Hop per Sample; lost after ${interval(m.timeoutMs)} without reply`],
    ["Measured from", esc(d.context.os)],
  ];
  if (d.context.publicIp) rows.push(["Public IP", esc(d.context.publicIp)]);
  return `<section><table class="kv">${rows.map(([k, v]) => `<tr><th>${k}</th><td>${v}</td></tr>`).join("")}</table></section>`;
};

const note: ReportSection = (d) =>
  d.view.meta.note.trim()
    ? `<section><h2>Problem description</h2><p class="note">${esc(d.view.meta.note.trim()).replace(/\n/g, "<br>")}</p></section>`
    : "";

const atAGlance: ReportSection = (d) => {
  const f = finalHop(d);
  if (!f) return `<section><p>No Samples in this time window.</p></section>`;
  const items = [
    ["Loss to target", `${pct(f.lossPct)}%`, `${f.lost} of ${f.sent} probes`],
    ["Average latency", `${ms(f.avgMs)} ms`, `min ${ms(f.minMs)} / max ${ms(f.maxMs)} ms`],
    ["Jitter", `${ms(f.jitterMs)} ms`, "mean change between samples"],
    ["Route changes", String(d.view.routeChanges.length), `${d.view.gaps.length} gap${d.view.gaps.length === 1 ? "" : "s"} in data`],
  ];
  return `<section class="glance">${items
    .map(([label, value, sub]) => `<div><div class="label">${label}</div><div class="value">${value}</div><div class="muted">${sub}</div></div>`)
    .join("")}</section>`;
};

const hopTable: ReportSection = (d) => `
  <section>
    <h2>Route and per-hop statistics</h2>
    <table class="hops">
      <thead><tr><th>Hop</th><th>Address</th><th>Sent</th><th>Loss %</th><th>Min</th><th>Avg</th><th>Max</th><th>Jitter</th><th>Cur</th></tr></thead>
      <tbody>${d.view.hops
        .map(
          (h) => `<tr class="${h.lossPct >= 1 ? "lossy" : ""}">
            <td>${h.hop}</td>
            <td>${hopLabel(h, d)}${h.addresses.length > 1 ? `<div class="muted small">also: ${h.addresses.slice(1).map((a) => esc(a.address)).join(", ")}</div>` : ""}</td>
            <td>${h.sent}</td><td>${pct(h.lossPct)}</td><td>${ms(h.minMs)}</td><td>${ms(h.avgMs)}</td><td>${ms(h.maxMs)}</td><td>${ms(h.jitterMs)}</td><td>${ms(h.curMs)}</td>
          </tr>`,
        )
        .join("")}</tbody>
    </table>
    <p class="muted small">Latency in milliseconds. The last hop is the target.</p>
  </section>`;

const graph = (title: string, buckets: Bucket[], d: ReportData) => `
  <figure>
    <figcaption>${title}</figcaption>
    ${timelineSvg(buckets, { width: REPORT_WIDTH, height: 160, from: d.window.from, to: d.window.to, gaps: d.view.gaps })}
  </figure>`;

const finalGraph: ReportSection = (d) => {
  const f = finalHop(d);
  if (!f) return "";
  return `<section><h2>Latency and loss over time</h2>${graph(`Hop ${f.hop}: target ${hopLabel(f, d)}`, d.finalTimeline, d)}
    <p class="muted small">Blue line: average latency; light band: min to max; red bars: packet loss (full height = 100%); grey: no data.</p></section>`;
};

const hopGraphs: ReportSection = (d) =>
  d.hopTimelines.length
    ? `<section><h2>Hops with packet loss</h2>${d.hopTimelines
        .map(({ hop, buckets }) => graph(`Hop ${hop.hop}: ${hopLabel(hop, d)}, ${pct(hop.lossPct)}% loss`, buckets, d))
        .join("")}</section>`
    : "";

const routeChanges: ReportSection = (d) =>
  d.view.routeChanges.length
    ? `<section><h2>Route changes</h2><table class="hops"><thead><tr><th>Time</th><th>Change</th></tr></thead><tbody>${d.view.routeChanges
        .slice(0, 200)
        .map(
          (rc) =>
            `<tr><td>${esc(dateTime(rc.ts))}</td><td>${rc.changes
              .map((c) => `hop ${c.hop}: ${esc(c.from ?? "none")} → ${esc(c.to ?? "none")}`)
              .join("<br>")}</td></tr>`,
        )
        .join("")}</tbody></table>${d.view.routeChanges.length > 200 ? `<p class="muted small">First 200 of ${d.view.routeChanges.length} shown.</p>` : ""}</section>`
    : "";

const gaps: ReportSection = (d) =>
  d.view.gaps.length
    ? `<section><h2>Gaps in data</h2><p class="muted small">No probes were sent during these periods (for example, the computer was asleep). They are not counted as loss.</p><ul>${d.view.gaps
        .map((g) => `<li>${esc(dateTime(g.start))} to ${esc(dateTime(g.end))} (${duration(g.end - g.start)})</li>`)
        .join("")}</ul></section>`
    : "";

const howToRead: ReportSection = () => `
  <section class="howto">
    <h2>How to read this report</h2>
    <ul>
      <li>Each hop is a router between this computer and the target, numbered by distance. Hop 1 is usually the local router.</li>
      <li>Loss or high latency that starts at one hop <em>and continues through every later hop to the target</em> points at that hop or the link just before it.</li>
      <li>Loss at a middle hop that does not carry through to the target is usually that router limiting how often it answers probes, not a fault.</li>
      <li>All times include their UTC offset.</li>
    </ul>
  </section>`;

/** The Report, top to bottom. Edit this list to change what Reports contain. */
export const REPORT_SECTIONS: ReportSection[] = [header, details, note, atAGlance, hopTable, finalGraph, hopGraphs, routeChanges, gaps, howToRead];

const STYLE = `
  * { box-sizing: border-box; }
  body { font: 13px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; color: #1d2230; background: #fff; margin: 0; }
  main { max-width: ${REPORT_WIDTH + 48}px; margin: 0 auto; padding: 24px; }
  h1 { font-size: 20px; margin: 0 0 4px; }
  h2 { font-size: 15px; margin: 24px 0 8px; padding-bottom: 4px; border-bottom: 1px solid #e3e5ea; }
  .muted { color: #5b6170; }
  .small { font-size: 11px; }
  table { border-collapse: collapse; width: 100%; }
  .kv th { text-align: left; width: 150px; font-weight: 600; vertical-align: top; padding: 3px 8px 3px 0; }
  .kv td { padding: 3px 0; }
  .hops th, .hops td { text-align: right; padding: 4px 8px; border-bottom: 1px solid #eceef2; font-variant-numeric: tabular-nums; }
  .hops th:nth-child(2), .hops td:nth-child(2), .hops td:first-child + td { text-align: left; }
  .hops th { background: #f4f5f8; font-weight: 600; }
  .hops tr.lossy td { background: #fdf0ef; }
  .glance { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; margin-top: 16px; }
  .glance > div { border: 1px solid #e3e5ea; border-radius: 6px; padding: 10px; }
  .glance .label { font-size: 11px; text-transform: uppercase; letter-spacing: .04em; color: #5b6170; }
  .glance .value { font-size: 20px; font-weight: 650; }
  .note { white-space: normal; background: #f7f8fa; border-left: 3px solid #1a56c4; padding: 8px 12px; margin: 0; }
  figure { margin: 8px 0 16px; break-inside: avoid; }
  figcaption { font-weight: 600; margin-bottom: 4px; }
  svg { max-width: 100%; height: auto; }
  section { break-inside: auto; }
  .howto { break-inside: avoid; }
  @media print { main { padding: 0; } @page { margin: 14mm; } }
`;

export function buildReportHtml(d: ReportData): string {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Network trace report: ${esc(d.view.meta.target)}</title><style>${STYLE}</style></head>
<body><main>${REPORT_SECTIONS.map((s) => s(d)).join("\n")}</main></body></html>`;
}

/** Gathers everything a Report needs for one Session and Time Window. */
export async function collectReport(view: SessionView, window: ResolvedWindow): Promise<ReportData> {
  const buckets = bucketsFor(REPORT_WIDTH, window.to - window.from, view.meta.intervalMs);
  const { id } = view.meta;
  const [finalTimeline, context, hopTimelines] = await Promise.all([
    api.sessionTimeline(id, { kind: "final" }, window.from, window.to, buckets),
    api.reportContext(),
    Promise.all(
      notableHops(view.hops).map(async (hop) => ({
        hop,
        buckets: await api.sessionTimeline(id, { kind: "number", hop: hop.hop }, window.from, window.to, buckets),
      })),
    ),
  ]);
  return { view, window, finalTimeline, hopTimelines, context, generatedAt: Date.now() };
}
