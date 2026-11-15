<script lang="ts">
  import type { Analysis } from "$lib/analysis";
  import type { HopStats } from "$lib/api";
  import { ms, pct } from "$lib/format";
  import Ip from "./Ip.svelte";

  interface Props {
    hops: HopStats[];
    names: Record<string, string>;
    analysis: Analysis;
    graphed: number[];
    onToggle: (hop: number) => void;
  }
  let { hops, names, analysis, graphed, onToggle }: Props = $props();

  // Scale for the inline latency bars: the slowest max in view.
  const scale = $derived(Math.max(1, ...hops.map((h) => h.maxMs ?? 0)));

  const lossTitle = {
    carried: "Loss that continues all the way to the Target",
    "not-carried": "Loss that doesn't continue to later Hops: usually this router limiting how often it answers, not a fault",
    "no-reply": "This router never answers probes, which is normal for some routers",
    none: "",
  };
</script>

<div class="wrap">
<table>
  <thead>
    <tr>
      <th class="num" title="Position along the route; click a number to graph that Hop">Hop</th>
      <th>Address <span class="muted small">(right-click for owner)</span></th>
      <th class="num opt-2" title="Probes sent to this Hop in the Time Window">Sent</th>
      <th class="num" title="Share of Probes that got no reply">Loss %</th>
      <th class="num" title="Fastest round trip (ms)">Min</th>
      <th class="num" title="Average round trip (ms)">Avg</th>
      <th class="num" title="Slowest round trip (ms)">Max</th>
      <th class="num opt-2" title="Average change in round trip between Samples (ms)">Jitter</th>
      <th class="num opt-1" title="Round trip in the latest Sample (ms)">Cur</th>
      <th class="bar-col">Latency <span class="muted small">0–{Math.round(scale)} ms</span></th>
    </tr>
  </thead>
  <tbody>
    {#each hops as h (h.hop)}
      {@const kind = analysis.kinds.get(h.hop) ?? "none"}
      {@const isTarget = h.hop === analysis.targetHop}
      <tr class={kind} class:graphed={graphed.includes(h.hop)}>
        <td class="num">
          {#if isTarget}
            <span class="hopnum" title="The Target, always graphed below">{h.hop}</span>
          {:else}
            <button class="hopnum" aria-pressed={graphed.includes(h.hop)} title="Show or hide this Hop's graph" onclick={() => onToggle(h.hop)}>{h.hop}</button>
          {/if}
        </td>
        <td class="addr">
          {#if h.address}
            <Ip address={h.address} />
          {:else}
            <span class="muted" title={lossTitle["no-reply"]}>no reply</span>
          {/if}
          {#if isTarget}<span class="badge">Target</span>{/if}
          {#if h.addresses.length > 1}
            <span class="multi small" title={"Answered at this Hop:\n" + h.addresses.map((a) => `${a.address} (${a.count}×)`).join("\n")}
              >+{h.addresses.length - 1}</span
            >
          {/if}
          {#if h.address && names[h.address]}<div class="name muted small">{names[h.address]}</div>{/if}
        </td>
        <td class="num opt-2">{h.sent}</td>
        <td class="num loss" title={lossTitle[kind]}>
          {#if kind === "no-reply"}<span class="muted">–</span>{:else}{pct(h.lossPct)}{/if}
        </td>
        <td class="num">{ms(h.minMs)}</td>
        <td class="num">{ms(h.avgMs)}</td>
        <td class="num">{ms(h.maxMs)}</td>
        <td class="num opt-2">{ms(h.jitterMs)}</td>
        <td class="num opt-1">{ms(h.curMs)}</td>
        <td class="bar-col">
          {#if h.minMs != null && h.maxMs != null && h.avgMs != null}
            <div class="bar" aria-hidden="true">
              <span class="range" style:left="{(h.minMs / scale) * 100}%" style:width="{((h.maxMs - h.minMs) / scale) * 100}%"></span>
              <span class="avg" style:left="{(h.avgMs / scale) * 100}%"></span>
            </div>
          {/if}
        </td>
      </tr>
    {/each}
  </tbody>
</table>
</div>
<p class="legend muted small">
  <span><span class="sw carried"></span>loss that continues to the Target</span>
  <span><span class="sw not-carried"></span>loss that doesn't continue (usually rate limiting)</span>
</p>

<style>
  /* Less important columns give way as the table's space shrinks. */
  .wrap {
    container-type: inline-size;
    overflow-x: auto;
    /* Room for a horizontal scrollbar, so it never covers the last row. */
    padding-bottom: 12px;
  }
  @container (max-width: 1000px) {
    .bar-col {
      display: none;
    }
  }
  @container (max-width: 780px) {
    .opt-1 {
      display: none;
    }
  }
  @container (max-width: 640px) {
    .opt-2 {
      display: none;
    }
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    padding: 0.35rem 0.6rem;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  th {
    text-align: left;
    font-weight: 600;
    color: var(--muted);
    font-size: 0.9rem;
  }
  tbody tr:hover {
    background: var(--row-hover);
  }
  tr.carried {
    background: var(--row-lossy);
  }
  tr.carried td.loss {
    color: var(--loss);
    font-weight: 700;
  }
  tr.not-carried td.loss {
    color: var(--warn);
  }
  tr.no-reply td {
    color: var(--muted);
  }
  tr.graphed {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .hopnum {
    display: inline-block;
    min-width: 2em;
    text-align: center;
    padding: 1px 6px;
  }
  button.hopnum[aria-pressed="true"] {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .addr {
    white-space: normal;
    min-width: 12em;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 32em;
  }
  .badge {
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    background: var(--row-selected);
    color: var(--accent);
    border-radius: 4px;
    padding: 0 5px;
    margin-left: 6px;
  }
  .multi {
    background: var(--row-selected);
    border-radius: 8px;
    padding: 0 5px;
    margin-left: 4px;
    cursor: help;
  }
  .bar-col {
    width: 22%;
    min-width: 120px;
  }
  .bar {
    position: relative;
    height: 10px;
    background: var(--chart-bg);
    border: 1px solid var(--chart-grid);
    border-radius: 2px;
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--chart-range);
  }
  .avg {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: 2px;
    background: var(--chart-avg);
  }
  .legend {
    margin: 6px 0 0;
    display: flex;
    gap: 6px 14px;
    align-items: center;
    flex-wrap: wrap;
  }
  .sw {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 2px;
    margin-right: 5px;
    vertical-align: -1px;
  }
  .sw.carried {
    background: var(--loss);
  }
  .sw.not-carried {
    background: var(--warn);
  }
</style>
