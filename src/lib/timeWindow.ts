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
