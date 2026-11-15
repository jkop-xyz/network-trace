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

export interface Gap {
  start: Millis;
  end: Millis;
}

export interface RouteChange {
  ts: Millis;
  changes: { hop: number; from: string | null; to: string | null }[];
}

export interface SessionView {
  meta: SessionMeta;
  running: boolean;
  sizeBytes: number;
  firstSample: Millis | null;
  lastSample: Millis | null;
  hops: HopStats[];
  gaps: Gap[];
  routeChanges: RouteChange[];
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
  sessionRestart: (id: string) => invoke<{ id: string; alreadyRunning: boolean }>("session_restart", { id }),
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
};
