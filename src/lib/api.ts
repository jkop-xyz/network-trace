// Typed wrappers around the Rust commands. Field names mirror the Rust
// structs (serde camelCase).
import { invoke } from "@tauri-apps/api/core";

export type Millis = number;

export interface SessionMeta {
  id: string;
  target: string;
  address: string;
  startedAt: Millis;
  stoppedAt: Millis | null;
  intervalMs: number;
  timeoutMs: number;
  note: string;
}

export type SessionStatus = "stopped" | "waiting" | "ok" | "loss";

export interface SessionSummary extends SessionMeta {
  sizeBytes: number;
  firstSample: Millis | null;
  lastSample: Millis | null;
  running: boolean;
  status: SessionStatus;
  /** Why the Session stopped on its own, if it failed. */
  error: string | null;
}

export interface AddressSeen {
  address: string;
  count: number;
  first: Millis;
  last: Millis;
}

export interface HopStats {
  hop: number;
  address: string | null;
  addresses: AddressSeen[];
  sent: number;
  lost: number;
  lossPct: number;
  minMs: number | null;
  avgMs: number | null;
  maxMs: number | null;
  curMs: number | null;
  jitterMs: number | null;
  name: string | null;
}

/** The Target's own figures, from Samples where its position was known. */
export interface TargetStats {
  sent: number;
  lost: number;
  lossPct: number;
  minMs: number | null;
  avgMs: number | null;
  maxMs: number | null;
  jitterMs: number | null;
  hop: number;
}

export interface IpOwner {
  address: string;
  network: string | null;
  organisation: string | null;
  country: string | null;
  range: string | null;
  source: string;
}

export interface Gap {
  start: Millis;
  end: Millis;
}

export interface RouteChange {
  ts: Millis;
  changes: { hop: number; from: string | null; to: string | null }[];
}

/** Samples in a row in which the Target didn't answer. */
export interface Outage {
  start: Millis;
  /** First Sample answered again, or the last unanswered one if not `recovered`. */
  end: Millis;
  recovered: boolean;
  samples: number;
  /** Furthest Hop that kept answering, if any. */
  lastHop: number | null;
  lastAddress: string | null;
  /** A router that answered "destination unreachable": it had no route onward. */
  unreachableFrom: string | null;
  unreachableHop: number | null;
}

export interface SessionView {
  meta: SessionMeta;
  running: boolean;
  sizeBytes: number;
  firstSample: Millis | null;
  lastSample: Millis | null;
  hops: HopStats[];
  target: TargetStats | null;
  gaps: Gap[];
  routeChanges: RouteChange[];
  outages: Outage[];
  names: Record<string, string>;
}

export interface Bucket {
  start: Millis;
  end: Millis;
  sent: number;
  lost: number;
  minMs: number | null;
  avgMs: number | null;
  maxMs: number | null;
}

export type TimelineHop = { kind: "final" } | { kind: "number"; hop: number };

export interface Settings {
  timeoutMs: number;
  defaultIntervalMs: number;
  publicIpInReport: boolean;
}

export interface ReportContext {
  os: string;
  appVersion: string;
  publicIp: string | null;
  publicIpError: string | null;
}

export const api = {
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (settings: Settings) => invoke<Settings>("settings_set", { settings }),
  sessionsList: () => invoke<SessionSummary[]>("sessions_list"),
  sessionStart: (target: string) => invoke<{ id: string; alreadyRunning: boolean }>("session_start", { target }),
  sessionStop: (id: string) => invoke<void>("session_stop", { id }),
  sessionResume: (id: string) => invoke<{ id: string; alreadyRunning: boolean }>("session_resume", { id }),
  sessionSetInterval: (id: string, intervalMs: number) => invoke<number>("session_set_interval", { id, intervalMs }),
  sessionSetNote: (id: string, note: string) => invoke<void>("session_set_note", { id, note }),
  sessionsDelete: (ids: string[]) => invoke<void>("sessions_delete", { ids }),
  sessionView: (id: string, from: Millis, to: Millis) => invoke<SessionView>("session_view", { id, from, to }),
  sessionTimeline: (id: string, hop: TimelineHop, from: Millis, to: Millis, buckets: number) =>
    invoke<Bucket[]>("session_timeline", { id, hop, from, to, buckets }),
  exportCsv: (id: string, from: Millis, to: Millis, path: string) => invoke<number>("export_csv", { id, from, to, path }),
  reportContext: () => invoke<ReportContext>("report_context"),
  saveText: (path: string, contents: string) => invoke<void>("save_text", { path, contents }),
  printReport: (html: string) => invoke<void>("print_report", { html }),
  openDataDir: () => invoke<void>("open_data_dir"),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  ipOwner: (address: string) => invoke<IpOwner>("ip_owner", { address }),
};
