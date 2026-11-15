<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { ask, save } from "@tauri-apps/plugin-dialog";
  import { onMount, tick as svelteTick } from "svelte";
  import { analyse } from "$lib/analysis";
  import { api, type SessionSummary, type SessionView } from "$lib/api";
  import { date, dateTime, duration, fileStem, interval, shortStamp, time } from "$lib/format";
  import { buildReportHtml, collectReport } from "$lib/report";
  import {
    PRESETS,
    defaultWindow,
    forgetWindows,
    rememberWindow,
    rememberedWindow,
    resolveWindow,
    type ResolvedWindow,
    type TimeWindow,
  } from "$lib/timeWindow";
  import HopTable from "./HopTable.svelte";
  import Ip from "./Ip.svelte";
  import Timeline from "./Timeline.svelte";

  interface Props {
    summary: SessionSummary;
    onChanged: () => void;
    onOpen: (id: string, message?: string) => void;
  }
  let { summary, onChanged, onOpen }: Props = $props();

  const INTERVALS = [1000, 2500, 5000, 10000, 15000, 30000, 60000];

  // The Time Window chosen for this Session is remembered; until one is
  // chosen, the default depends on whether the Session is running.
  // svelte-ignore state_referenced_locally
  let tw: TimeWindow = $state(rememberedWindow(summary.id) ?? defaultWindow(summary.running));
  function setWindow(w: TimeWindow) {
    tw = w;
    rememberWindow(summary.id, w);
  }
  let win = $state<ResolvedWindow | null>(null);
  let view = $state<SessionView | null>(null);
  let tick = $state(0);
  let graphed: number[] = $state([]);
  let note = $state("");
  let noteLoaded = false;
  let busy = $state("");
  let message = $state("");
  let savedPath = $state<string | null>(null);

  const analysis = $derived(view ? analyse(view.hops, view.target) : null);
  const sessionSpan = $derived(
    Math.max(summary.lastSample ?? 0, summary.stoppedAt ?? 0, summary.running ? Date.now() : 0) - summary.startedAt,
  );

  let loading = false;
  let reloadQueued = false;
  async function load() {
    // One load at a time; a request during a load runs once it finishes.
    if (loading) {
      reloadQueued = true;
      return;
    }
    loading = true;
    try {
      const w = resolveWindow(tw, summary);
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
    } finally {
      loading = false;
      if (reloadQueued) {
        reloadQueued = false;
        load();
      }
    }
  }

  // Reload whenever the Time Window or the Session's running state changes.
  $effect(() => {
    void tw;
    void summary.running;
    load();
  });

  // Live updates: frequent for short windows, rarer for long ones, whose
  // figures barely move with each Sample but are costly to recompute.
  let pending = false;
  function loadSoon() {
    if (pending) return;
    pending = true;
    const span = win ? win.to - win.from : 0;
    const delay = span > 6 * 3600e3 ? 15000 : span > 3600e3 ? 5000 : 1000;
    setTimeout(() => {
      pending = false;
      load();
    }, delay);
  }

  onMount(() => {
    const unlisten = [
      listen<{ sessionId: string }>("session-sample", (e) => {
        if (e.payload.sessionId === summary.id) loadSoon();
      }),
      listen("names-updated", loadSoon),
    ];
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
      flushNote();
    };
  });

  function choosePreset(id: string) {
    if (id === "custom") {
      const w = win ?? resolveWindow(tw, summary);
      setWindow({ preset: "custom", from: w.from, to: w.to });
    } else {
      setWindow({ preset: id as TimeWindow["preset"] });
    }
  }

  const toLocalInput = (ts: number) => `${date(ts)}T${time(ts, false)}`;
  function setCustom(which: "from" | "to", value: string) {
    const ts = new Date(value).getTime();
    if (Number.isNaN(ts)) return;
    setWindow({ ...tw, preset: "custom", [which]: ts });
  }

  async function toggleGraph(hop: number) {
    const adding = !graphed.includes(hop);
    graphed = adding ? [...graphed, hop].sort((a, b) => a - b) : graphed.filter((h) => h !== hop);
    if (adding) {
      await svelteTick();
      // Wait for the graph to draw so the whole card, not just its top
      // edge, is scrolled into view.
      setTimeout(() => document.getElementById(`hop-graph-${hop}`)?.scrollIntoView({ behavior: "smooth", block: "nearest" }), 400);
    }
  }

  async function stop() {
    busy = "Stopping…";
    await api.sessionStop(summary.id);
    busy = "";
    onChanged();
  }

  async function resume() {
    try {
      const res = await api.sessionResume(summary.id);
      if (res.id === summary.id) onChanged();
      else onOpen(res.id, `${summary.target} is already being monitored in another Session: showing that one.`);
    } catch (e) {
      message = String(e);
    }
  }

  async function remove() {
    const ok = await ask(`Delete the Session for ${summary.target} started ${dateTime(summary.startedAt)}? Its data is removed and cannot be recovered.`, {
      title: "Delete Session",
      kind: "warning",
    });
    if (!ok) return;
    try {
      await api.sessionsDelete([summary.id]);
      forgetWindows([summary.id]);
      onChanged();
    } catch (e) {
      message = String(e);
    }
  }

  async function changeInterval(value: string) {
    await api.sessionSetInterval(summary.id, Number(value));
    onChanged();
  }

  // The Note saves shortly after typing stops, and when leaving the Session.
  let noteTimer: ReturnType<typeof setTimeout> | undefined;
  function noteChanged() {
    clearTimeout(noteTimer);
    noteTimer = setTimeout(flushNote, 800);
  }
  async function flushNote() {
    clearTimeout(noteTimer);
    if (view && note !== view.meta.note) {
      const text = note;
      await api.sessionSetNote(summary.id, text);
      if (view) view.meta.note = text;
    }
  }

  async function exportReport(kind: "html" | "pdf") {
    if (!view || !win) return;
    await flushNote();
    const stem = fileStem(summary.target, win.from);
    let path: string | null = null;
    if (kind === "html") {
      path = await save({ defaultPath: `${stem}.html`, filters: [{ name: "HTML report", extensions: ["html"] }] });
      if (!path) return;
    }
    busy = "Building Report…";
    message = "";
    savedPath = null;
    try {
      const html = buildReportHtml(await collectReport(view, win));
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

  async function exportCsv() {
    if (!win) return;
    const path = await save({
      defaultPath: `${fileStem(summary.target, win.from)}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!path) return;
    busy = "Writing CSV…";
    savedPath = null;
    try {
      const rows = await api.exportCsv(summary.id, win.from, win.to, path);
      message = `${rows.toLocaleString()} Probes saved to ${path}`;
      savedPath = path;
    } catch (e) {
      message = String(e);
    } finally {
      busy = "";
    }
  }

  const targetRow = $derived(view?.target ? view.hops.find((h) => h.hop === view!.target!.hop) : undefined);
  const windowIsPartial = $derived(win != null && tw.preset !== "full" && win.to - win.from < sessionSpan - 60_000);
</script>

<div class="panel">
  <header>
    <div class="title">
      <h1>{summary.target}</h1>
      <span class="muted">
        {#if summary.target !== summary.address}<Ip address={summary.address} /> ·
        {/if}started {dateTime(summary.startedAt)}
        {#if summary.stoppedAt}· ran {duration(summary.stoppedAt - summary.startedAt)}{/if}
      </span>
    </div>
    <div class="actions">
      {#if summary.running}
        <label class="muted"
          >Sample every
          <select value={summary.intervalMs} onchange={(e) => changeInterval(e.currentTarget.value)}>
            {#each INTERVALS as i (i)}<option value={i}>{interval(i)}</option>{/each}
          </select>
        </label>
        <button onclick={stop} disabled={!!busy}>Stop</button>
      {:else}
        <span class="muted">Stopped</span>
        <button onclick={resume} title="Continue monitoring in this Session; the stopped time shows as a gap">Resume</button>
        <button class="danger" onclick={remove} title="Delete this Session and its data">Delete</button>
      {/if}
    </div>
  </header>

  {#if summary.error}
    <p class="error-banner" role="alert">This Session stopped on its own: {summary.error}</p>
  {/if}

  <section class="card note-card">
    <label for="note-{summary.id}">Note <span class="muted small">— describe the problem for your ISP; printed at the top of the Report</span></label>
    <textarea
      id="note-{summary.id}"
      rows="2"
      placeholder="e.g. “Lag in games every evening since 10 Nov; router already restarted.”"
      bind:value={note}
      oninput={noteChanged}
      onblur={flushNote}
    ></textarea>
  </section>

  <div class="toolbar">
    <label>
      <span class="muted">Time Window</span>
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
    <div class="exports">
      <button class="primary" onclick={() => exportReport("pdf")} disabled={!!busy || !view?.hops.length}>Export Report (PDF)</button>
      <button onclick={() => exportReport("html")} disabled={!!busy || !view?.hops.length}>HTML</button>
      <button onclick={exportCsv} disabled={!!busy || !view?.hops.length} title="Every Probe as CSV">Raw Export (CSV)</button>
    </div>
  </div>
  {#if win}
    <p class="covers small muted">
      Exports cover: {win.label} ({shortStamp(win.from)} – {shortStamp(win.to)}).
      {#if windowIsPartial}<button class="link" onclick={() => setWindow({ preset: "full" })}>Choose Full Session</button> to include everything.{/if}
    </p>
  {/if}

  {#if busy || message}
    <p class="status small">
      {busy || message}
      {#if savedPath && !busy}<button class="link" onclick={() => savedPath && api.revealFile(savedPath)}>Show in folder</button>{/if}
    </p>
  {/if}

  {#if view && win && analysis}
    {#if view.hops.length === 0}
      <p class="muted waiting">{summary.running ? "Waiting for the first Sample…" : "No Samples in this Time Window."}</p>
    {:else}
      <section class="card">
        <HopTable hops={view.hops} names={view.names} {analysis} {graphed} onToggle={toggleGraph} />
      </section>

      <section class="card">
        <h2>
          Target: {summary.target}
          {#if targetRow?.address && targetRow.address !== summary.target}<span class="muted">{targetRow.address}</span>{/if}
          {#if !view.target}<span class="warn">has not answered in this Time Window</span>{/if}
        </h2>
        <Timeline
          sessionId={summary.id}
          hop={{ kind: "final" }}
          from={win.from}
          to={win.to}
          gaps={view.gaps}
          routeChanges={view.routeChanges.map((r) => r.ts)}
          intervalMs={summary.intervalMs}
          {tick}
        />
      </section>

      {#each graphed as hop (hop)}
        {@const h = view.hops.find((x) => x.hop === hop)}
        {#if h}
          <section class="card" id="hop-graph-{hop}">
            <h2>
              Hop {hop}: {h.address ?? "no reply"}
              <span class="muted">{h.address ? (view.names[h.address] ?? "") : ""}</span>
              <button class="close" onclick={() => toggleGraph(hop)} aria-label="Hide graph">×</button>
            </h2>
            <Timeline
              sessionId={summary.id}
              hop={{ kind: "number", hop }}
              from={win.from}
              to={win.to}
              gaps={view.gaps}
              routeChanges={view.routeChanges.map((r) => r.ts)}
              intervalMs={summary.intervalMs}
              {tick}
            />
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
                  <span class="change">Hop {c.hop}: {c.from ?? "none"} → {c.to ?? "none"}</span>
                {/each}
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .panel {
    padding: 1rem 1.5rem 2rem;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    flex-wrap: wrap;
  }
  h1 {
    font-size: 1.55rem;
    margin: 0;
  }
  .actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .error-banner {
    margin: 10px 0 0;
    padding: 8px 12px;
    border-radius: 6px;
    background: var(--row-lossy);
    color: var(--loss);
  }
  .note-card {
    margin-top: 12px;
  }
  .note-card label {
    display: block;
    font-weight: 600;
    margin-bottom: 4px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 4px 0 2px;
  }
  .toolbar label,
  .actions label {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .exports {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .spacer {
    flex: 1;
  }
  .covers {
    margin: 4px 0 10px;
    text-align: right;
  }
  .status {
    margin: 0 0 10px;
    color: var(--muted);
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    text-decoration: underline;
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
    font-size: 1rem;
    margin: 0 0 6px;
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .warn {
    color: var(--warn);
    font-weight: normal;
  }
  .close {
    margin-left: auto;
    border: 0;
    padding: 0 6px;
    font-size: 1.2rem;
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
