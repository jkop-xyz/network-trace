// The app's version, as released (e.g. "0.1.0-beta.5").
import { getVersion } from "@tauri-apps/api/app";

let cached: Promise<string> | null = null;

export function appVersion(): Promise<string> {
  cached ??= getVersion().catch(() => "");
  return cached;
}
