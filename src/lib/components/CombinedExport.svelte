<script lang="ts">
  // Pick Sessions and export them as one Report for the shared Time Window.
  import { save } from "@tauri-apps/plugin-dialog";
  import { api, type SessionSummary } from "$lib/api";
  import { date, shortStamp } from "$lib/format";
  import { buildCombinedReportHtml, collectReport } from "$lib/report";
  import { PRESETS, resolveWindow, type TimeWindow } from "$lib/timeWindow";

  interface Props {
    sessions: SessionSummary[];
    window: TimeWindow;
    onClose: () => void;
  }
  let { sessions, window: tw, onClose }: Props = $props();

  const ordered = $derived([...sessions].sort((a, b) => Number(b.running) - Number(a.running) || b.startedAt - a.startedAt));
  // Running Sessions are the "current" ones; preselect them.
  // svelte-ignore state_referenced_locally
  let picked = $state<string[]>(sessions.filter((s) => s.running).map((s) => s.id));
  let busy = $state("");
  let message = $state("");
  let savedPath = $state<string | null>(null);

  const label = $derived(PRESETS.find((p) => p.id === tw.preset)?.label ?? "Custom range");

  function toggle(id: string) {
    picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
  }

  async function build(): Promise<string> {
    const chosen = ordered.filter((s) => picked.includes(s.id));
    const context = await api.reportContext();
    const all = [];
    for (const [i, s] of chosen.entries()) {
      busy = `Collecting ${s.target} (${i + 1} of ${chosen.length})…`;
      const w = resolveWindow(tw, s);
      const view = await api.sessionView(s.id, w.from, w.to);
      all.push(await collectReport(view, w, context));
    }
    return buildCombinedReportHtml(all);
  }

  async function exportAs(kind: "pdf" | "html") {
    if (!picked.length) return;
    message = "";
    savedPath = null;
    let path: string | null = null;
    if (kind === "html") {
      const stamp = date(Date.now());
      path = await save({ defaultPath: `network-trace_${picked.length}-targets_${stamp}.html`, filters: [{ name: "HTML report", extensions: ["html"] }] });
      if (!path) return;
    }
    try {
      const html = await build();
      busy = "";
      if (path) {
        await api.saveText(path, html);
        message = `Report saved to ${path}`;
        savedPath = path;
      } else {
        await api.printReport(html);
        message = "In the print dialog, choose “Save as PDF” (Windows) or “Print to File” → PDF (Linux).";
      }
    } catch (e) {
      message = String(e);
    } finally {
      busy = "";
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !busy && onClose()} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={() => !busy && onClose()}></div>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="combined-title">
  <h2 id="combined-title">Combined Report</h2>
  <p class="muted">
    One Report with a section per Session, for the Time Window <b>{label}</b> (set in the Overview; it applies to all Sessions).
  </p>

  <div class="list">
    {#each ordered as s (s.id)}
      <label class="row">
        <input type="checkbox" checked={picked.includes(s.id)} onchange={() => toggle(s.id)} disabled={!!busy} />
        <span class="target">{s.target}</span>
        <span class="muted small">{shortStamp(s.startedAt)} · {s.running ? "running" : "stopped"}</span>
      </label>
    {/each}
  </div>

  {#if busy || message}
    <p class="status small">
      {busy || message}
      {#if savedPath && !busy}<button class="link" onclick={() => savedPath && api.revealFile(savedPath)}>Show in folder</button>{/if}
    </p>
  {/if}

  <div class="actions">
    <span class="muted small">{picked.length} selected</span>
    <span class="spacer"></span>
    <button onclick={onClose} disabled={!!busy}>Close</button>
    <button onclick={() => exportAs("html")} disabled={!!busy || !picked.length}>HTML</button>
    <button class="primary" onclick={() => exportAs("pdf")} disabled={!!busy || !picked.length}>Export Report (PDF)</button>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 0.35);
  }
  .dialog {
    position: fixed;
    z-index: 51;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 16px 20px;
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.3);
  }
  h2 {
    margin: 0 0 6px;
    font-size: 1.2rem;
  }
  p {
    margin: 0 0 10px;
  }
  .list {
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px;
    min-height: 60px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: 5px;
    cursor: pointer;
  }
  .row:hover {
    background: var(--row-hover);
  }
  .target {
    font-weight: 600;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    margin: 10px 0 0;
    color: var(--muted);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }
  .spacer {
    flex: 1;
  }
  .link {
    border: 0;
    background: none;
    padding: 0 4px;
    color: var(--accent);
    text-decoration: underline;
  }
</style>
