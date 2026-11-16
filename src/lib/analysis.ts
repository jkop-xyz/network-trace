// Reading the hop table the way a network engineer would: loss only matters
// if it carries through to the Target. Shared by the screen and the Report.

import type { Bucket, HopStats, Outage, TargetStats } from "./api";
import { dateTime, duration, pct } from "./format";

/** Loss at or above this (percent) counts as loss. */
export const LOSS_THRESHOLD = 1;

/**
 * - `carried`: lossy, and every later Hop including the Target is lossy too,
 *   which points at this Hop or the link before it.
 * - `not-carried`: lossy here but not further along: usually the router
 *   limiting how often it answers, not a fault.
 * - `no-reply`: never answered at all; many routers are set up that way.
 */
export type HopLoss = "none" | "carried" | "not-carried" | "no-reply";

export interface Analysis {
  kinds: Map<number, HopLoss>;
  /** First Hop of carried loss, if the Target lost packets. */
  lossStart: HopStats | null;
  /** The last answering Hop before `lossStart`. */
  beforeLoss: HopStats | null;
  /** `beforeLoss` answered "destination unreachable" during an Outage. */
  beforeLossSaidNoRoute: boolean;
  /** Hop number of the Target, if it answered in the window. */
  targetHop: number | null;
}

const isNoReply = (h: HopStats) => h.address == null && h.addresses.length === 0;
const lossy = (h: HopStats) => h.lossPct >= LOSS_THRESHOLD;

/**
 * `outages` (optional) refines where loss starts: a router that answered
 * "destination unreachable" during an Outage was up, so the loss can't start
 * at its Hop, even if that Hop shows loss (routers limit how often they send
 * such answers).
 */
export function analyse(hops: HopStats[], target: TargetStats | null, outages: Outage[] = []): Analysis {
  const kinds = new Map<number, HopLoss>();
  const reportedNoRoute = new Set(outages.flatMap((o) => (o.unreachableHop != null ? [o.unreachableHop] : [])));
  const targetHop = target?.hop ?? null;
  for (const h of hops) kinds.set(h.hop, isNoReply(h) ? "no-reply" : lossy(h) ? "not-carried" : "none");

  let lossStart: HopStats | null = null;
  if (target && target.lossPct >= LOSS_THRESHOLD) {
    // Walk back from the Target while every answering Hop is lossy.
    const answering = hops.filter((h) => !isNoReply(h) && h.hop <= target.hop);
    for (let i = answering.length - 1; i >= 0 && lossy(answering[i]) && !reportedNoRoute.has(answering[i].hop); i--) lossStart = answering[i];
    // The Target lost packets even if its row (which can mix in Samples from
    // before a route change) is just under the threshold.
    if (!lossStart && answering.length) lossStart = answering[answering.length - 1];
    if (lossStart) for (const h of answering) if (h.hop >= lossStart.hop) kinds.set(h.hop, "carried");
  }
  const beforeLoss = lossStart ? ([...hops].reverse().find((h) => h.hop < lossStart!.hop && !isNoReply(h)) ?? null) : null;
  const beforeLossSaidNoRoute = beforeLoss != null && reportedNoRoute.has(beforeLoss.hop);
  return { kinds, lossStart, beforeLoss, beforeLossSaidNoRoute, targetHop };
}

/** Plain-language summary of where loss starts, for the top of a Report. */
export function verdict(a: Analysis, hops: HopStats[], target: TargetStats | null, names: Record<string, string>): string {
  const label = (h: HopStats) => `Hop ${h.hop}${h.address ? ` (${h.address}${names[h.address] ? `, ${names[h.address]}` : ""})` : ""}`;
  const notCarried = hops.filter((h) => a.kinds.get(h.hop) === "not-carried");
  const notCarriedNote = notCarried.length
    ? ` Loss seen at ${notCarried.map((h) => `Hop ${h.hop}`).join(", ")} does not continue to later Hops, so it is most likely ${notCarried.length === 1 ? "that router" : "those routers"} limiting how often ${notCarried.length === 1 ? "it answers" : "they answer"} probes, not a fault.`
    : "";

  if (!target) {
    return "The target did not answer any probes in this time window, so the route to it could not be measured. It may block ping; try monitoring a different address on the same network.";
  }
  const lossText = `${pct(target.lossPct)}% of probes to the target were lost`;
  if (!a.lossStart) {
    return `${capitalise(lossText)} (${target.lost} of ${target.sent}), below the ${LOSS_THRESHOLD}% considered a problem.${notCarriedNote}`;
  }
  if (a.lossStart.hop === 1) {
    return `${capitalise(lossText)}. The loss already starts at ${label(a.lossStart)}, the first device on the route (usually your own router), and continues to the target. This points at the local network or the connection to that device.${notCarriedNote}`;
  }
  const before = !a.beforeLoss
    ? ""
    : a.beforeLossSaidNoRoute
      ? ` ${label(a.beforeLoss)}, just before it, was up: during the Outages it answered "destination unreachable", meaning it had no route onward.`
      : lossy(a.beforeLoss)
        ? ""
        : ` ${label(a.beforeLoss)}, just before it, is clean.`;
  return `${capitalise(lossText)}. Packet loss first appears at ${label(a.lossStart)} and continues through every later Hop to the target.${before} This points at ${label(a.lossStart)} or the link leading to it.${notCarriedNote}`;
}

/** Samples in a row that make an Outage (matches `OUTAGE_MIN_SAMPLES` in trace-core). */
export const OUTAGE_MIN_SAMPLES = 3;

/** How a router is named in plain-language text. */
export function routerLabel(address: string, hop: number | null, names: Record<string, string>): string {
  const name = names[address] ? `, ${names[address]}` : "";
  return hop != null ? `Hop ${hop} (${address}${name})` : `${address}${name ? ` (${names[address]})` : ""}`;
}

/** Plain-language summary of the Outages, for under the verdict. Empty if none. */
export function outageSummary(outages: Outage[], names: Record<string, string>): string {
  if (!outages.length) return "";
  const n = outages.length;
  const longest = outages.reduce((a, b) => (b.end - b.start > a.end - a.start ? b : a));
  const parts = [
    `The target stopped answering completely ${n === 1 ? "once" : `${n} times`} (an Outage: ${OUTAGE_MIN_SAMPLES} or more Samples in a row without reply)${n === 1 ? ", lasting" : "; the longest lasted"} ${longest.recovered ? "" : "at least "}${duration(longest.end - longest.start)}, from ${dateTime(longest.start)}.`,
  ];
  const byRouter = new Map<string, Outage[]>();
  for (const o of outages) if (o.unreachableFrom) byRouter.set(o.unreachableFrom, [...(byRouter.get(o.unreachableFrom) ?? []), o]);
  for (const [address, list] of byRouter) {
    const hop = list.find((o) => o.unreachableHop != null)?.unreachableHop ?? null;
    const during = n === 1 ? "During it" : list.length === n ? "During all of them" : `During ${list.length} of them`;
    const local = hop === 1 ? ", the first device on the route (usually your own router)," : "";
    parts.push(
      `${during}, ${routerLabel(address, hop, names)}${local} answered "destination unreachable": it was reachable but had no route onward, so the connection beyond it was down.`,
    );
  }
  return parts.join(" ");
}

/** The worst stretch of about `minutes` in a timeline, by loss. */
export function worstStretch(buckets: Bucket[], minutes = 5): { lossPct: number; from: number; to: number } | null {
  if (!buckets.length) return null;
  const bucketMs = Math.max(1, buckets[0].end - buckets[0].start + 1);
  const width = Math.max(1, Math.round((minutes * 60_000) / bucketMs));
  if (buckets.length < width) return null;
  let best: { lossPct: number; from: number; to: number } | null = null;
  let sent = 0;
  let lost = 0;
  for (let i = 0; i < buckets.length; i++) {
    sent += buckets[i].sent;
    lost += buckets[i].lost;
    if (i >= width) {
      sent -= buckets[i - width].sent;
      lost -= buckets[i - width].lost;
    }
    if (i >= width - 1 && sent > 0) {
      const pct = (lost / sent) * 100;
      if (!best || pct > best.lossPct) best = { lossPct: pct, from: buckets[i - width + 1].start, to: buckets[i].end };
    }
  }
  return best;
}

function capitalise(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
