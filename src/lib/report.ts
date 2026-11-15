// The Report: a self-contained HTML page made for sending to an ISP.
//
// It is assembled from independent sections (REPORT_SECTIONS). To change the
// Report, add, remove or reorder sections there; each one only reads
// ReportData and returns HTML.

import type { Bucket, HopStats, ReportContext, SessionView } from "./api";
import { api } from "./api";
import { analyse, verdict, worstStretch, type Analysis } from "./analysis";
import { bucketsFor, timelineSvg, yScaleFor } from "./chart";
import { dateTime, duration, escapeHtml as esc, interval, ms, pct, shortStamp, time } from "./format";
import type { ResolvedWindow } from "./timeWindow";

/** Fits an A4/Letter page at print size, so graph text stays readable. */
export const REPORT_WIDTH = 700;

export interface ReportGraph {
  hop: HopStats;
  buckets: Bucket[];
  caption: string;
}

export interface ReportData {
  view: SessionView;
  window: ResolvedWindow;
  analysis: Analysis;
  /** The Target's timeline. */
  finalTimeline: Bucket[];
  /** Graphs of the Hops around where loss starts, and other lossy Hops. */
  hopGraphs: ReportGraph[];
  /** One latency scale shared by every graph, so they compare fairly. */
  yMax: number;
  context: ReportContext;
  generatedAt: number;
}

export type ReportSection = (d: ReportData) => string;

const hopLabel = (h: HopStats, d: ReportData) => {
  const name = h.address ? d.view.names[h.address] : undefined;
  return h.address ? `${esc(h.address)}${name ? ` <span class="muted">(${esc(name)})</span>` : ""}` : `<span class="muted">no reply</span>`;
};

/** Which Hops get their own graph: where loss starts and the clean Hop
 * before it, then (captioned as such) the worst loss that isn't carried. */
export function graphedHops(view: SessionView, a: Analysis): { hop: HopStats; caption: string }[] {
  const out: { hop: HopStats; caption: string }[] = [];
  const isTarget = (h: HopStats) => h.hop === a.targetHop;
  if (a.beforeLoss && !isTarget(a.beforeLoss)) out.push({ hop: a.beforeLoss, caption: "last Hop before the loss starts" });
  if (a.lossStart && !isTarget(a.lossStart)) out.push({ hop: a.lossStart, caption: "loss starts here and continues to the target" });
  const notCarried = view.hops
    .filter((h) => a.kinds.get(h.hop) === "not-carried" && h.sent >= 5)
    .sort((x, y) => y.lossPct - x.lossPct)
    .slice(0, 2);
  for (const h of notCarried) out.push({ hop: h, caption: "loss here does not continue to the target" });
  return out.sort((x, y) => x.hop.hop - y.hop.hop);
}

const banner: ReportSection = () => `
  <div class="banner">To save this Report as a PDF, choose <b>Save as PDF</b> (Windows) or <b>Print to File</b>, format PDF (Linux) in the print dialog. This line is not printed.</div>`;

const header: ReportSection = (d) => `
  <header>
    <h1>Network trace report: ${esc(d.view.meta.target)}</h1>
    <p class="muted">Generated ${esc(dateTime(d.generatedAt))} by Network Trace ${esc(d.context.appVersion)}</p>
  </header>`;

const summary: ReportSection = (d) => `
  <section class="verdict"><h2>Summary</h2><p>${esc(verdict(d.analysis, d.view.hops, d.view.target, d.view.names))}</p></section>`;

const sessionRows = (d: ReportData): [string, string][] => {
  const m = d.view.meta;
  return [
    ["Target", `${esc(m.target)}${m.target !== m.address ? ` (${esc(m.address)})` : ""}`],
    ["Time Window", `${esc(d.window.label)}: ${esc(dateTime(d.window.from))} to ${esc(dateTime(d.window.to))} (${duration(d.window.to - d.window.from)})`],
    ["Session", `started ${esc(dateTime(m.startedAt))}${m.stoppedAt ? `, stopped ${esc(dateTime(m.stoppedAt))}` : ", still running"}`],
  ];
};

const sharedRows = (d: ReportData): [string, string][] => {
  const m = d.view.meta;
  const rows: [string, string][] = [
    ["Method", `ICMP echo to every Hop once per Sample, every ${interval(m.intervalMs)}; a Probe counts as lost after ${interval(m.timeoutMs)} without reply`],
    ["Measured from", esc(d.context.os)],
  ];
  if (d.context.publicIp) rows.push(["Public IP", esc(d.context.publicIp)]);
  return rows;
};

const kvTable = (rows: [string, string][]) =>
  `<section><table class="kv">${rows.map(([k, v]) => `<tr><th>${k}</th><td>${v}</td></tr>`).join("")}</table></section>`;

const details: ReportSection = (d) => kvTable([...sessionRows(d), ...sharedRows(d)]);

const note: ReportSection = (d) =>
  d.view.meta.note.trim()
    ? `<section><h2>Problem description</h2><p class="note">${esc(d.view.meta.note.trim()).replace(/\n/g, "<br>")}</p></section>`
    : "";

const atAGlance: ReportSection = (d) => {
  const t = d.view.target;
  if (!t) return "";
  const worst = worstStretch(d.finalTimeline, 5);
  const items: [string, string, string][] = [
    ["Loss to target", `${pct(t.lossPct)}%`, `${t.lost} of ${t.sent} Probes`],
    worst
      ? ["Worst 5 minutes", `${pct(worst.lossPct)}%`, `loss, ${d.window.to - d.window.from > 20 * 3600e3 ? shortStamp(worst.from) : time(worst.from, false)}–${time(worst.to, false)}`]
      : ["Jitter", `${ms(t.jitterMs)} ms`, "average change between Samples"],
    ["Average latency", `${ms(t.avgMs)} ms`, `min ${ms(t.minMs)} / max ${ms(t.maxMs)} ms`],
    ["Route changes", String(d.view.routeChanges.length), `${d.view.gaps.length} gap${d.view.gaps.length === 1 ? "" : "s"} with no data`],
  ];
  return `<section class="glance">${items
    .map(([label, value, sub]) => `<div><div class="label">${label}</div><div class="value">${value}</div><div class="muted">${sub}</div></div>`)
    .join("")}</section>`;
};

const hopTable: ReportSection = (d) => {
  const row = (h: HopStats) => {
    const kind = d.analysis.kinds.get(h.hop);
    const isTarget = h.hop === d.analysis.targetHop;
    const lossCell =
      kind === "no-reply"
        ? `<span class="muted">no reply</span>`
        : kind === "carried"
          ? `<b class="loss">${pct(h.lossPct)}</b>`
          : kind === "not-carried"
            ? `<span class="minor">${pct(h.lossPct)}*</span>`
            : pct(h.lossPct);
    return `<tr class="${kind === "carried" ? "carried" : ""}">
      <td>${h.hop}</td>
      <td>${hopLabel(h, d)}${isTarget ? ` <span class="badge">target</span>` : ""}${h.addresses.length > 1 ? `<div class="muted small">also: ${h.addresses.slice(1).map((a) => esc(a.address)).join(", ")}</div>` : ""}</td>
      <td>${h.sent}</td><td>${lossCell}</td><td>${ms(h.minMs)}</td><td>${ms(h.avgMs)}</td><td>${ms(h.maxMs)}</td><td>${ms(h.jitterMs)}</td>
    </tr>`;
  };
  const anyMinor = d.view.hops.some((h) => d.analysis.kinds.get(h.hop) === "not-carried");
  return `
  <section>
    <h2>Route and per-Hop statistics</h2>
    <table class="hops">
      <thead><tr><th>Hop</th><th>Address</th><th>Sent</th><th>Loss %</th><th>Min ms</th><th>Avg ms</th><th>Max ms</th><th>Jitter ms</th></tr></thead>
      <tbody>${d.view.hops.map(row).join("")}</tbody>
    </table>
    <p class="muted small">Red rows: loss that continues all the way to the target.${anyMinor ? " * Loss that does not continue to later Hops (usually the router rate-limiting its replies)." : ""} "No reply": the router never answers probes, which is normal for some routers.</p>
  </section>`;
};

const graph = (title: string, buckets: Bucket[], d: ReportData) => `
  <figure>
    <figcaption>${title}</figcaption>
    ${timelineSvg(buckets, {
      width: REPORT_WIDTH,
      height: 150,
      from: d.window.from,
      to: d.window.to,
      gaps: d.view.gaps,
      routeChanges: d.view.routeChanges.map((r) => r.ts),
      yMax: d.yMax,
      fontSize: 11,
    })}
  </figure>`;

const targetGraph: ReportSection = (d) => {
  const t = d.view.target;
  if (!t) return "";
  const row = d.view.hops.find((h) => h.hop === t.hop);
  return `<section><h2>Latency and loss over time</h2>${graph(`Target: ${row ? hopLabel(row, d) : esc(d.view.meta.address)}`, d.finalTimeline, d)}</section>`;
};

const hopGraphs: ReportSection = (d) =>
  d.hopGraphs.length
    ? `<section><h2>Hops around the problem</h2>${d.hopGraphs
        .map(({ hop, buckets, caption }) => graph(`Hop ${hop.hop}: ${hopLabel(hop, d)}, ${pct(hop.lossPct)}% loss <span class="muted">(${esc(caption)})</span>`, buckets, d))
        .join("")}<p class="muted small">All graphs share the same latency scale.</p></section>`
    : "";

const routeChanges: ReportSection = (d) =>
  d.view.routeChanges.length
    ? `<section><h2>Route changes</h2><table class="hops"><thead><tr><th>Time</th><th>Change</th></tr></thead><tbody>${d.view.routeChanges
        .slice(0, 200)
        .map(
          (rc) =>
            `<tr><td>${esc(dateTime(rc.ts))}</td><td>${rc.changes
              .map((c) => `Hop ${c.hop}: ${esc(c.from ?? "none")} → ${esc(c.to ?? "none")}`)
              .join("<br>")}</td></tr>`,
        )
        .join("")}</tbody></table>${d.view.routeChanges.length > 200 ? `<p class="muted small">First 200 of ${d.view.routeChanges.length} shown.</p>` : ""}</section>`
    : "";

const gaps: ReportSection = (d) =>
  d.view.gaps.length
    ? `<section><h2>Gaps in data</h2><p class="muted small">No Probes were sent during these periods (for example, the computer was asleep). They are not counted as loss.</p><ul>${d.view.gaps
        .map((g) => `<li>${esc(dateTime(g.start))} to ${esc(dateTime(g.end))} (${duration(g.end - g.start)})</li>`)
        .join("")}</ul></section>`
    : "";

const howToRead: ReportSection = () => `
  <section class="howto">
    <h2>How to read this report</h2>
    <ul>
      <li>Each Hop is a router between this computer and the target, numbered by distance. Hop 1 is usually the local router.</li>
      <li>Loss or high latency that starts at one Hop <em>and continues through every later Hop to the target</em> points at that Hop or the link just before it.</li>
      <li>Loss at a middle Hop that does not carry through to the target is usually that router limiting how often it answers probes, not a fault.</li>
      <li>All times, including graph axes, are local time with the UTC offset shown.</li>
    </ul>
  </section>`;

/** The Report, top to bottom. Edit this list to change what Reports contain. */
export const REPORT_SECTIONS: ReportSection[] = [
  banner,
  header,
  summary,
  details,
  note,
  atAGlance,
  targetGraph,
  hopTable,
  hopGraphs,
  routeChanges,
  gaps,
  howToRead,
];

const STYLE = `
  * { box-sizing: border-box; }
  html { color-scheme: light; }
  body { font: 13px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; color: #1d2230; background: #fff; margin: 0;
         -webkit-print-color-adjust: exact; print-color-adjust: exact; }
  main { max-width: ${REPORT_WIDTH + 48}px; margin: 0 auto; padding: 24px; }
  h1 { font-size: 20px; margin: 0 0 4px; }
  h2 { font-size: 15px; margin: 22px 0 8px; padding-bottom: 4px; border-bottom: 1px solid #e3e5ea; }
  h2, figcaption { break-after: avoid; page-break-after: avoid; }
  .muted { color: #5b6170; }
  .small { font-size: 11px; }
  .banner { background: #fff6dd; border: 1px solid #f0d58a; border-radius: 6px; padding: 8px 12px; margin-bottom: 16px; }
  .verdict p { font-size: 14px; margin: 0; background: #f4f7fd; border-left: 3px solid #1a56c4; padding: 10px 12px; }
  table { border-collapse: collapse; width: 100%; }
  .kv th { text-align: left; width: 130px; font-weight: 600; vertical-align: top; padding: 3px 8px 3px 0; }
  .kv td { padding: 3px 0; }
  .hops th, .hops td { text-align: right; padding: 4px 6px; border-bottom: 1px solid #eceef2; font-variant-numeric: tabular-nums; }
  .hops th:nth-child(2), .hops td:nth-child(2) { text-align: left; }
  .hops th { background: #f4f5f8; font-weight: 600; }
  .hops tr.carried td { background: #fdecea; }
  .loss { color: #c5221f; }
  .minor { color: #8a5a00; }
  .badge { font-size: 10px; text-transform: uppercase; letter-spacing: .04em; background: #e3ebfb; color: #1a56c4; border-radius: 3px; padding: 0 4px; }
  .glance { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; margin-top: 16px; }
  .glance > div { border: 1px solid #e3e5ea; border-radius: 6px; padding: 8px 10px; }
  .glance .label { font-size: 10px; text-transform: uppercase; letter-spacing: .04em; color: #5b6170; }
  .glance .value { font-size: 19px; font-weight: 650; }
  .note { background: #f7f8fa; border-left: 3px solid #5b6170; padding: 8px 12px; margin: 0; }
  figure { margin: 8px 0 14px; break-inside: avoid; page-break-inside: avoid; }
  figcaption { font-weight: 600; margin-bottom: 4px; }
  svg { max-width: 100%; height: auto; }
  .howto, .glance, .verdict { break-inside: avoid; }
  .verdict p + p { margin-top: 6px; }
  .session { margin-top: 28px; padding-top: 4px; border-top: 3px solid #1a56c4; }
  .session-title { font-size: 19px; margin: 8px 0 6px; }
  @media print { .session { break-before: page; border-top: 0; margin-top: 0; } }
  @media print { main { padding: 0; } .banner { display: none; } @page { margin: 14mm; } }
`;

function page(title: string, body: string): string {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light">
<title>${esc(title)}</title><style>${STYLE}</style></head>
<body><main>${body}</main></body></html>`;
}

export function buildReportHtml(d: ReportData): string {
  return page(`Network trace report: ${d.view.meta.target}`, REPORT_SECTIONS.map((s) => s(d)).join("\n"));
}

/** What each Session contributes to a combined Report. */
const SESSION_SECTIONS: ReportSection[] = [
  (d) => kvTable(sessionRows(d)),
  note,
  atAGlance,
  targetGraph,
  hopTable,
  hopGraphs,
  routeChanges,
  gaps,
];

/** One Report covering several Sessions: a verdict per Target up front,
 * then a section per Session, each starting on a new printed page. */
export function buildCombinedReportHtml(all: ReportData[]): string {
  const first = all[0];
  const targets = all.map((d) => d.view.meta.target);
  const head = `
    <header>
      <h1>Network trace report: ${all.length} targets</h1>
      <p class="muted">${targets.map(esc).join(", ")}</p>
      <p class="muted">Generated ${esc(dateTime(first.generatedAt))} by Network Trace ${esc(first.context.appVersion)}</p>
    </header>`;
  const summaries = `
    <section class="verdict"><h2>Summary</h2>${all
      .map((d) => `<p><b>${esc(d.view.meta.target)}:</b> ${esc(verdict(d.analysis, d.view.hops, d.view.target, d.view.names))}</p>`)
      .join("")}</section>`;
  const shared = kvTable([["Time Window", esc(first.window.label)], ...sharedRows(first)]);
  const sessions = all
    .map(
      (d) => `<section class="session"><h1 class="session-title">${esc(d.view.meta.target)}</h1>${SESSION_SECTIONS.map((s) => s(d)).join("\n")}</section>`,
    )
    .join("\n");
  return page(`Network trace report: ${targets.join(", ")}`, [banner(first), head, summaries, shared, sessions, howToRead(first)].join("\n"));
}

/** Gathers everything a Report needs for one Session and Time Window.
 * Pass `context` when building several Reports so it's fetched once. */
export async function collectReport(view: SessionView, window: ResolvedWindow, context?: ReportContext): Promise<ReportData> {
  const buckets = bucketsFor(REPORT_WIDTH, window.to - window.from, view.meta.intervalMs);
  const { id } = view.meta;
  const analysis = analyse(view.hops, view.target);
  const [finalTimeline, ctx, hopGraphs] = await Promise.all([
    api.sessionTimeline(id, { kind: "final" }, window.from, window.to, buckets),
    context ?? api.reportContext(),
    Promise.all(
      graphedHops(view, analysis).map(async ({ hop, caption }) => ({
        hop,
        caption,
        buckets: await api.sessionTimeline(id, { kind: "number", hop: hop.hop }, window.from, window.to, buckets),
      })),
    ),
  ]);
  const yMax = yScaleFor(finalTimeline, ...hopGraphs.map((g) => g.buckets));
  return { view, window, analysis, finalTimeline, hopGraphs, yMax, context: ctx, generatedAt: Date.now() };
}
