// Time Windows: the part of a Session being looked at or exported.

export type PresetId = "10m" | "30m" | "1h" | "6h" | "24h" | "48h" | "full" | "custom";

export const PRESETS: { id: PresetId; label: string; minutes?: number }[] = [
  { id: "10m", label: "Last 10 min", minutes: 10 },
  { id: "30m", label: "Last 30 min", minutes: 30 },
  { id: "1h", label: "Last hour", minutes: 60 },
  { id: "6h", label: "Last 6 hours", minutes: 360 },
  { id: "24h", label: "Last 24 hours", minutes: 1440 },
  { id: "48h", label: "Last 48 hours", minutes: 2880 },
  { id: "full", label: "Full Session" },
  { id: "custom", label: "Custom…" },
];

export interface TimeWindow {
  preset: PresetId;
  /** Only used by "custom". */
  from?: number;
  to?: number;
}

export interface ResolvedWindow {
  from: number;
  to: number;
  label: string;
}

interface SessionTimes {
  startedAt: number;
  firstSample: number | null;
  stoppedAt: number | null;
  lastSample: number | null;
  running: boolean;
}

const STORAGE_KEY = "timeWindows";

function readAll(): Record<string, TimeWindow> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") ?? {};
  } catch {
    return {};
  }
}

const GLOBAL_KEY = "timeWindow.all";
const valid = (w: TimeWindow | null | undefined): w is TimeWindow => !!w && PRESETS.some((p) => p.id === w.preset);

/** The Time Window set for all Sessions, if any. */
export function globalWindow(): TimeWindow | null {
  try {
    const w = JSON.parse(localStorage.getItem(GLOBAL_KEY) ?? "null");
    return valid(w) ? w : null;
  } catch {
    return null;
  }
}

/** Uses one Time Window for every Session, replacing choices made for
 * individual Sessions. */
export function setGlobalWindow(w: TimeWindow) {
  try {
    localStorage.setItem(GLOBAL_KEY, JSON.stringify(w));
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* not remembered */
  }
}

/** The Time Window to show for a Session: the one chosen for it, else the
 * one set for all Sessions. Kept on this computer only; storage may be
 * unavailable, in which case nothing is remembered. */
export function rememberedWindow(sessionId: string): TimeWindow | null {
  const own = readAll()[sessionId];
  return valid(own) ? own : globalWindow();
}

export function rememberWindow(sessionId: string, w: TimeWindow) {
  try {
    const all = readAll();
    all[sessionId] = w;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch {
    /* not remembered */
  }
}

export function forgetWindows(sessionIds: string[]) {
  try {
    const all = readAll();
    for (const id of sessionIds) delete all[id];
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch {
    /* nothing to forget */
  }
}

/** The default Time Window: recent activity for a running Session, everything for a stopped one. */
export function defaultWindow(running: boolean): TimeWindow {
  return { preset: running ? "10m" : "full" };
}

/** Turns a Time Window into concrete bounds. "Last …" counts back from now
 * for a running Session and from its end for a stopped one. */
export function resolveWindow(w: TimeWindow, s: SessionTimes, now = Date.now()): ResolvedWindow {
  const start = Math.min(s.startedAt, s.firstSample ?? s.startedAt);
  const end = s.running ? now : Math.max(s.lastSample ?? 0, s.stoppedAt ?? 0, start);
  const preset = PRESETS.find((p) => p.id === w.preset) ?? PRESETS[0];
  if (w.preset === "custom" && w.from != null && w.to != null) {
    return { from: Math.max(w.from, start), to: Math.max(w.to, w.from), label: "Custom range" };
  }
  if (preset.minutes) {
    return { from: Math.max(start, end - preset.minutes * 60_000), to: end, label: preset.label };
  }
  return { from: start, to: end, label: "Full Session" };
}
