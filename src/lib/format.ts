// Formatting shared by the screen and Reports.

const pad = (n: number, w = 2) => String(n).padStart(w, "0");

/** "+01:00" style offset of the local time zone at `ts`. */
export function utcOffset(ts: number): string {
  const mins = -new Date(ts).getTimezoneOffset();
  const sign = mins >= 0 ? "+" : "-";
  const abs = Math.abs(mins);
  return `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`;
}

export function date(ts: number): string {
  const d = new Date(ts);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function time(ts: number, seconds = true): string {
  const d = new Date(ts);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}${seconds ? ":" + pad(d.getSeconds()) : ""}`;
}

/** "2026-11-15 20:14:02 UTC+01:00": unambiguous for readers in other time zones. */
export function dateTime(ts: number): string {
  return `${date(ts)} ${time(ts)} UTC${utcOffset(ts)}`;
}

/** Short local stamp for lists, e.g. "15 Nov 20:14". */
export function shortStamp(ts: number): string {
  const d = new Date(ts);
  const month = d.toLocaleString("en", { month: "short" });
  return `${d.getDate()} ${month} ${time(ts, false)}`;
}

export function duration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h} h ${m % 60} min`;
  return `${Math.floor(h / 24)} d ${h % 24} h`;
}

export function ms(v: number | null | undefined): string {
  if (v == null) return "–";
  return v < 10 ? v.toFixed(1) : Math.round(v).toString();
}

export function pct(v: number | null | undefined): string {
  if (v == null) return "–";
  if (v === 0) return "0";
  return v < 10 ? v.toFixed(1) : Math.round(v).toString();
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export function interval(msValue: number): string {
  return msValue % 1000 === 0 ? `${msValue / 1000} s` : `${(msValue / 1000).toFixed(1)} s`;
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

/** A filesystem-safe stem for exported files. */
export function fileStem(target: string, from: number): string {
  const d = new Date(from);
  const stamp = `${date(from)}_${pad(d.getHours())}${pad(d.getMinutes())}`;
  return `network-trace_${target.replace(/[^a-zA-Z0-9.-]+/g, "_")}_${stamp}`;
}
