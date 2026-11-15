<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { save } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { api, type SessionSummary, type SessionView } from "$lib/api";
  import { date, dateTime, duration, fileStem, interval, shortStamp, time } from "$lib/format";
  import { buildReportHtml, collectReport } from "$lib/report";
  import { PRESETS, defaultWindow, resolveWindow, type ResolvedWindow, type TimeWindow } from "$lib/timeWindow";
  import HopTable from "./HopTable.svelte";
  import Timeline from "./Timeline.svelte";

  interface Props {
    summary: SessionSummary;
    onChanged: () => void;
    onOpen: (id: string) => void;
  }
  let { summary, onChanged, onOpen }: Props = $props();

  const INTERVALS = [1000, 2500, 5000, 10000, 15000, 30000, 60000];

  // svelte-ignore state_referenced_locally
  let tw: TimeWindow = $state(defaultWindow(summary.running));
  let win = $state<ResolvedWindow | null>(null);
  let view = $state<SessionView | null>(null);
  let tick = $state(0);
  let graphed: number[] = $state([]);
  let note = $state("");
  let noteLoaded = false;
  let busy = $state("");
  let message = $state("");

  async function load() {
    const w = resolveWindow(tw, summary);
    try {
      const v = await api.sessionView(summary.id, w.from, w.to);
      view = v;
      win = w;
      tick++;
      if (!noteLoaded) {
        note = v.meta.note;
        noteLoaded = true;
      }
    } catch (e) {
      message = String(e);
    }
  }

  // Reload whenever the Time Window or the Session's running state changes.
  $effect(() => {
    void tw;
    void summary.running;
    void summary.lastSample;
    load();
  });

  let pending = false;
  function loadSoon() {
    if (pending) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      load();
    }, 1000);
  }

  onMount(() => {
    const unlisten = [
      listen<{ sessionId: string }>("session-sample", (e) => {
        if (e.payload.sessionId === summary.id) loadSoon();
      }),
      listen("names-updated", loadSoon),
    ];
    return () => unlisten.forEach((p) => p.then((f) => f()));
  });

  function choosePreset(id: string) {
    if (id === "custom") {
      const w = win ?? resolveWindow(tw, summary);
      tw = { preset: "custom", from: w.from, to: w.to };
    } else {
      tw = { preset: id as TimeWindow["preset"] };
    }
  }

  const toLocalInput = (ts: number) => `${date(ts)}T${time(ts, false)}`;
  function setCustom(which: "from" | "to", value: string) {
    const ts = new Date(value).getTime();
    if (Number.isNaN(ts)) return;
    tw = { ...tw, preset: "custom", [which]: ts };
  }

  function toggleGraph(hop: number) {
    graphed = graphed.includes(hop) ? graphed.filter((h) => h !== hop) : [...graphed, hop].sort((a, b) => a - b);
  }

  async function stop() {
    busy = "Stopping…";
    await api.sessionStop(summary.id);
    busy = "";
    onChanged();
  }

  async function startAgain() {
    try {
      const res = await api.sessionRestart(summary.id);
      onOpen(res.id);
    } catch (e) {
      message = String(e);
    }
  }

  async function changeInterval(value: string) {
    await api.sessionSetInterval(summary.id, Number(value));
    onChanged();
  }

  async function saveNote() {
    if (view && note !== view.meta.note) {
      await api.sessionSetNote(summary.id, note);
      view.meta.note = note;
    }
  }

  async function exportReport(kind: "html" | "pdf") {
    if (!view || !win) return;
    await saveNote();
    const stem = fileStem(summary.target, win.from);
    let path: string | null = null;
    if (kind === "html") {
      path = await save({ defaultPath: `${stem}.html`, filters: [{ name: "HTML report", extensions: ["html"] }] });
      if (!path) return;
    }
    busy = "Building Report…";
    message = "";
    try {
      const html = buildReportHtml(await collectReport(view, win));
      if (path) {
        await api.saveText(path, html);
        message = `Report saved to ${path}`;
      } else {
        await api.printReport(html);
        message = "Choose “Save as PDF” (or “Print to File”) in the print dialog.";
      }
    } catch (e) {
      message = String(e);
    } finally {
      busy = "";
    }
  }

  async function exportCsv() {
    if (!win) return;
    const path = await save({
      defaultPath: `${fileStem(summary.target, win.from)}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!path) return;
    busy = "Writing CSV…";
    try {
      const rows = await api.exportCsv(summary.id, win.from, win.to, path);
      message = `${rows.toLocaleString()} Probes saved to ${path}`;
    } catch (e) {
      message = String(e);
    } finally {
      busy = "";
    }
  }

  const finalHop = $derived(view?.hops[view.hops.length - 1]);
</script>

<div class="panel">
  <header>
    <div class="title">
      <h1>{summary.target}</h1>
      <span class="muted">
        {#if summary.target !== summary.address}{summary.address} ·
        {/if}started {dateTime(summary.startedAt)}
        {#if summary.stoppedAt}· ran {duration(summary.stoppedAt - summary.startedAt)}{/if}
      </span>
    </div>
    <div class="actions">
      {#if summary.running}
        <label class="muted"
          >Every
          <select value={summary.intervalMs} onchange={(e) => changeInterval(e.currentTarget.value)}>
            {#each INTERVALS as i (i)}<option value={i}>{interval(i)}</option>{/each}
          </select>
        </label>
        <button onclick={stop} disabled={!!busy}>Stop</button>
      {:else}
        <span class="muted">Stopped</span>
        <button onclick={startAgain}>Start again</button>
      {/if}
    </div>
  </header>

  <div class="toolbar">
    <label>
      <span class="muted">Time window</span>
      <select value={tw.preset} onchange={(e) => choosePreset(e.currentTarget.value)}>
        {#each PRESETS as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
      </select>
    </label>
    {#if tw.preset === "custom" && tw.from != null && tw.to != null}
      <input type="datetime-local" value={toLocalInput(tw.from)} onchange={(e) => setCustom("from", e.currentTarget.value)} />
      <span class="muted">to</span>
      <input type="datetime-local" value={toLocalInput(tw.to)} onchange={(e) => setCustom("to", e.currentTarget.value)} />
    {:else if win}
      <span class="muted small">{shortStamp(win.from)} – {shortStamp(win.to)}</span>
    {/if}
    <span class="spacer"></span>
    <button onclick={() => exportReport("html")} disabled={!!busy || !view?.hops.length}>Export HTML</button>
    <button onclick={() => exportReport("pdf")} disabled={!!busy || !view?.hops.length}>Export PDF</button>
    <button onclick={exportCsv} disabled={!!busy || !view?.hops.length}>Export CSV</button>
  </div>

  {#if busy || message}
    <p class="status small">{busy || message}</p>
  {/if}

  {#if view && win}
    {#if view.hops.length === 0}
      <p class="muted waiting">{summary.running ? "Waiting for the first Sample…" : "No Samples in this time window."}</p>
    {:else}
      <section class="card">
        <HopTable hops={view.hops} names={view.names} {graphed} onToggle={toggleGraph} />
      </section>

      <section class="card">
        <h2>
          Hop {finalHop?.hop}: {summary.target}
          <span class="muted">{finalHop?.address !== summary.target ? finalHop?.address : ""}</span>
        </h2>
        <Timeline sessionId={summary.id} hop={{ kind: "final" }} from={win.from} to={win.to} gaps={view.gaps} intervalMs={summary.intervalMs} {tick} />
      </section>

      {#each graphed as hop (hop)}
        {@const h = view.hops.find((x) => x.hop === hop)}
        {#if h}
          <section class="card">
            <h2>
              Hop {hop}: {h.address ?? "no reply"}
              <span class="muted">{h.address ? (view.names[h.address] ?? "") : ""}</span>
              <button class="close" onclick={() => toggleGraph(hop)} aria-label="Hide graph">×</button>
            </h2>
            <Timeline sessionId={summary.id} hop={{ kind: "number", hop }} from={win.from} to={win.to} gaps={view.gaps} intervalMs={summary.intervalMs} {tick} />
          </section>
        {/if}
      {/each}

      {#if view.routeChanges.length}
        <section class="card">
          <h2>Route changes <span class="muted">({view.routeChanges.length})</span></h2>
          <ul class="changes">
            {#each view.routeChanges.slice(-50).reverse() as rc (rc.ts)}
              <li>
                <span class="muted">{dateTime(rc.ts)}</span>
                {#each rc.changes as c (c.hop)}
                  <span class="change">hop {c.hop}: {c.from ?? "none"} → {c.to ?? "none"}</span>
                {/each}
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}

    <section class="card">
      <h2>Note <span class="muted small">printed at the top of the Report</span></h2>
      <textarea
        rows="3"
        placeholder="Describe the problem for your ISP, e.g. “Lag in games every evening since 10 Nov; router already restarted.”"
        bind:value={note}
        onblur={saveNote}
      ></textarea>
    </section>
  {/if}
</div>

<style>
  .panel {
    padding: 16px 20px 32px;
    max-width: 1400px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
  }
  h1 {
    font-size: 20px;
    margin: 0;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 14px 0 10px;
  }
  .toolbar label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .spacer {
    flex: 1;
  }
  .status {
    margin: 0 0 10px;
    color: var(--muted);
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 12px;
    margin-bottom: 12px;
    overflow-x: auto;
  }
  h2 {
    font-size: 13px;
    margin: 0 0 6px;
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .close {
    margin-left: auto;
    border: 0;
    padding: 0 6px;
    font-size: 16px;
    line-height: 1;
  }
  .waiting {
    padding: 40px 0;
    text-align: center;
  }
  .changes {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 220px;
    overflow: auto;
  }
  .changes li {
    padding: 3px 0;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
  }
  textarea {
    width: 100%;
    resize: vertical;
  }
</style>
