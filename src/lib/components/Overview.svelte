<script lang="ts">
  // Several Sessions side by side in a grid (1×1 up to 3×3), each showing its
  // Target's figures and graph for a shared Time Window.
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api, type Gap, type SessionSummary, type TargetStats } from "$lib/api";
  import { ms, pct } from "$lib/format";
  import { PRESETS, globalWindow, resolveWindow, setGlobalWindow, type TimeWindow } from "$lib/timeWindow";
  import CombinedExport from "./CombinedExport.svelte";
  import Timeline from "./Timeline.svelte";

  interface Props {
    sessions: SessionSummary[];
    onOpen: (id: string) => void;
  }
  let { sessions, onOpen }: Props = $props();

  const LAYOUTS = [
    { id: "1x1", cols: 1, rows: 1 },
    { id: "2x1", cols: 2, rows: 1 },
    { id: "2x2", cols: 2, rows: 2 },
    { id: "3x2", cols: 3, rows: 2 },
    { id: "3x3", cols: 3, rows: 3 },
  ];
  const WINDOW_PRESETS = PRESETS.filter((p) => p.id !== "custom");

  // Layout, window and which Session sits in which cell are remembered per
  // computer; storage can be unavailable, so every access is guarded.
  function load<T>(key: string, fallback: T): T {
    try {
      const v = localStorage.getItem(key);
      return v ? (JSON.parse(v) as T) : fallback;
    } catch {
      return fallback;
    }
  }
  function store(key: string, value: unknown) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* not persisted */
    }
  }

  let layoutId = $state(load("overview.layout", "2x2"));
  // The Overview's Time Window is the one set for all Sessions.
  let tw = $state<TimeWindow>(globalWindow() ?? { preset: "30m" });
  let exporting = $state(false);
  function chooseWindow(id: string) {
    tw = { preset: id as TimeWindow["preset"] };
    setGlobalWindow(tw);
  }
  let assigned = $state<(string | null)[]>(load("overview.cells", []));
  let tick = $state(0);
  let cellHeights = $state<number[]>([]);
  // Graph height that fills the cell under its header and figures.
  const graphHeight = (i: number) => Math.max(60, (cellHeights[i] ?? 0) - 215);
  let stats = $state<Record<string, TargetStats | null>>({});
  let gapsBy = $state<Record<string, Gap[]>>({});

  const layout = $derived(LAYOUTS.find((l) => l.id === layoutId) ?? LAYOUTS[2]);
  const cellCount = $derived(layout.cols * layout.rows);

  // Fill empty cells with running Sessions first, then the most recent ones.
  const cells = $derived.by(() => {
    const byId = new Map(sessions.map((s) => [s.id, s]));
    const chosen = assigned.slice(0, cellCount).map((id) => (id && byId.has(id) ? id : null));
    while (chosen.length < cellCount) chosen.push(null);
    const used = new Set(chosen.filter(Boolean));
    const candidates = [...sessions]
      .sort((a, b) => Number(b.running) - Number(a.running) || b.startedAt - a.startedAt)
      .filter((s) => !used.has(s.id));
    return chosen.map((id) => (id ? byId.get(id)! : (candidates.shift() ?? null)));
  });

  $effect(() => store("overview.layout", layoutId));

  function assign(index: number, id: string) {
    const next = cells.map((c) => c?.id ?? null);
    next[index] = id || null;
    assigned = next;
    store("overview.cells", next);
  }

  function windowFor(s: SessionSummary) {
    return resolveWindow(tw, s);
  }

  async function refreshStats() {
    const out: Record<string, TargetStats | null> = {};
    const gaps: Record<string, Gap[]> = {};
    await Promise.all(
      cells.map(async (s) => {
        if (!s) return;
        const w = windowFor(s);
        try {
          const v = await api.sessionView(s.id, w.from, w.to);
          out[s.id] = v.target;
          gaps[s.id] = v.gaps;
        } catch {
          out[s.id] = null;
        }
      }),
    );
    stats = out;
    gapsBy = gaps;
    tick++;
  }

  $effect(() => {
    void cells;
    void tw;
    refreshStats();
  });

  let pending = false;
  onMount(() => {
    const un = listen("session-sample", () => {
      if (pending) return;
      pending = true;
      setTimeout(() => {
        pending = false;
        refreshStats();
      }, 3000);
    });
    return () => un.then((f) => f());
  });
</script>

<div class="overview">
  <header>
    <h1>Overview</h1>
    <div class="controls">
      <div class="layouts" role="group" aria-label="Grid layout">
        {#each LAYOUTS as l (l.id)}
          <button class:active={l.id === layoutId} aria-pressed={l.id === layoutId} onclick={() => (layoutId = l.id)}>{l.cols}×{l.rows}</button>
        {/each}
      </div>
      <label title="Applies to every Session">
        <span class="muted">Time Window (all Sessions)</span>
        <select value={tw.preset} onchange={(e) => chooseWindow(e.currentTarget.value)}>
          {#each WINDOW_PRESETS as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
          {#if tw.preset === "custom"}<option value="custom" disabled>Custom range</option>{/if}
        </select>
      </label>
      <button class="primary" onclick={() => (exporting = true)} disabled={!sessions.length}>Export combined Report</button>
    </div>
  </header>

  {#if exporting}
    <CombinedExport {sessions} window={tw} onClose={() => (exporting = false)} />
  {/if}

  <div class="grid" style:grid-template-columns="repeat({layout.cols}, minmax(0, 1fr))" style:grid-template-rows="repeat({layout.rows}, minmax(0, 1fr))">
    {#each cells as s, i (i)}
      <section class="cell" bind:clientHeight={cellHeights[i]}>
        <div class="cell-head">
          <select value={s?.id ?? ""} onchange={(e) => assign(i, e.currentTarget.value)} aria-label="Session in this cell">
            <option value="">(empty)</option>
            {#each sessions as o (o.id)}<option value={o.id}>{o.target}{o.running ? "" : " (stopped)"}</option>{/each}
          </select>
          {#if s}
            <span class="dot {s.status}" aria-hidden="true"></span>
            <button class="link" onclick={() => onOpen(s.id)}>Open</button>
          {/if}
        </div>
        {#if s}
          {@const t = stats[s.id]}
          {@const w = windowFor(s)}
          <div class="figures">
            <span class:bad={(t?.lossPct ?? 0) >= 1}><b>{t ? pct(t.lossPct) : "–"}%</b> loss</span>
            <span><b>{ms(t?.avgMs)}</b> ms avg</span>
            <span><b>{ms(t?.jitterMs)}</b> ms jitter</span>
            {#if !t}<span class="muted">Target not answering</span>{/if}
          </div>
          <Timeline sessionId={s.id} hop={{ kind: "final" }} from={w.from} to={w.to} gaps={gapsBy[s.id] ?? []} intervalMs={s.intervalMs} height={graphHeight(i)} {tick} />
        {:else}
          <p class="muted empty">Pick a Session for this cell.</p>
        {/if}
      </section>
    {/each}
  </div>
</div>

<style>
  .overview {
    padding: 1rem 1.5rem;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 12px;
  }
  h1 {
    font-size: 1.55rem;
    margin: 0;
  }
  .controls {
    display: flex;
    gap: 14px;
    align-items: center;
    flex-wrap: wrap;
  }
  .controls label {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .layouts {
    display: flex;
  }
  .layouts button {
    border-radius: 0;
    margin-left: -1px;
  }
  .layouts button:first-child {
    border-radius: 6px 0 0 6px;
  }
  .layouts button:last-child {
    border-radius: 0 6px 6px 0;
  }
  .layouts button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .grid {
    flex: 1;
    display: grid;
    gap: 12px;
    min-height: 0;
  }
  .cell {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 12px;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .cell-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .cell-head select {
    flex: 1;
    min-width: 0;
    font-weight: 600;
  }
  .figures {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    margin: 8px 0 4px;
  }
  .figures b {
    font-size: 1.15rem;
  }
  .bad {
    color: var(--loss);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 2px solid var(--muted);
    flex: none;
  }
  .dot.ok {
    background: var(--ok);
    border-color: var(--ok);
  }
  .dot.loss {
    background: var(--loss);
    border-color: var(--loss);
  }
  .dot.waiting {
    background: var(--warn);
    border-color: var(--warn);
  }
  .link {
    border: 0;
    background: none;
    color: var(--accent);
    padding: 0 4px;
  }
  .empty {
    margin: auto;
  }
</style>
