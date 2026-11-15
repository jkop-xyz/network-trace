<script lang="ts">
  import type { HopStats } from "$lib/api";
  import { ms, pct } from "$lib/format";

  interface Props {
    hops: HopStats[];
    names: Record<string, string>;
    graphed: number[];
    onToggle: (hop: number) => void;
  }
  let { hops, names, graphed, onToggle }: Props = $props();

  // Scale for the inline latency bars: the slowest max in view.
  const scale = $derived(Math.max(1, ...hops.map((h) => h.maxMs ?? 0)));
  const finalHop = $derived(hops[hops.length - 1]?.hop);
</script>

<table>
  <thead>
    <tr>
      <th class="num">Hop</th>
      <th>Address</th>
      <th>Name</th>
      <th class="num">Sent</th>
      <th class="num">Loss %</th>
      <th class="num">Min</th>
      <th class="num">Avg</th>
      <th class="num">Max</th>
      <th class="num">Jitter</th>
      <th class="num">Cur</th>
      <th class="bar-col">Latency (ms)</th>
    </tr>
  </thead>
  <tbody>
    {#each hops as h (h.hop)}
      <tr
        class:lossy={h.lossPct >= 1}
        class:graphed={graphed.includes(h.hop)}
        onclick={() => h.hop !== finalHop && onToggle(h.hop)}
        title={h.hop === finalHop ? "The Target, always graphed below" : "Click to show this Hop's graph"}
      >
        <td class="num">{h.hop}</td>
        <td>
          {h.address ?? "no reply"}
          {#if h.addresses.length > 1}
            <span
              class="multi small"
              title={"Answered at this Hop:\n" + h.addresses.map((a) => `${a.address} (${a.count}×)`).join("\n")}
              >+{h.addresses.length - 1}</span
            >
          {/if}
        </td>
        <td class="name muted">{h.address ? (names[h.address] ?? "") : ""}</td>
        <td class="num">{h.sent}</td>
        <td class="num loss">{pct(h.lossPct)}</td>
        <td class="num">{ms(h.minMs)}</td>
        <td class="num">{ms(h.avgMs)}</td>
        <td class="num">{ms(h.maxMs)}</td>
        <td class="num">{ms(h.jitterMs)}</td>
        <td class="num">{ms(h.curMs)}</td>
        <td class="bar-col">
          {#if h.minMs != null && h.maxMs != null && h.avgMs != null}
            <div class="bar">
              <span class="range" style:left="{(h.minMs / scale) * 100}%" style:width="{((h.maxMs - h.minMs) / scale) * 100}%"></span>
              <span class="avg" style:left="{(h.avgMs / scale) * 100}%"></span>
            </div>
          {/if}
        </td>
      </tr>
    {/each}
  </tbody>
</table>

<style>
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  th {
    text-align: left;
    font-weight: 600;
    color: var(--muted);
    font-size: 12px;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover {
    background: var(--row-hover);
  }
  tr.lossy td.loss {
    color: var(--loss);
    font-weight: 600;
  }
  tr.lossy {
    background: var(--row-lossy);
  }
  tr.graphed {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .name {
    max-width: 280px;
    overflow: hidden;
    text-overflow: ellipsis;
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
</style>
