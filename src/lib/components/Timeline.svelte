<script lang="ts">
  import { api, type Gap, type TimelineHop } from "$lib/api";
  import { bucketsFor, timelineSvg } from "$lib/chart";

  interface Props {
    sessionId: string;
    hop: TimelineHop;
    from: number;
    to: number;
    gaps: Gap[];
    routeChanges?: number[];
    intervalMs: number;
    /** Fixed graph height; by default it grows with the width. */
    height?: number;
    /** Changes whenever the data should be re-read (new Samples). */
    tick: number;
  }
  let { sessionId, hop, from, to, gaps, routeChanges = [], intervalMs, height, tick }: Props = $props();

  let width = $state(0);
  let svg = $state("");

  $effect(() => {
    const w = width;
    const args = { sessionId, hop, from, to, gaps, routeChanges, intervalMs, height };
    void tick;
    if (w < 50) return;
    let cancelled = false;
    const rootFont = parseFloat(getComputedStyle(document.documentElement).fontSize) || 13;
    api
      .sessionTimeline(args.sessionId, args.hop, args.from, args.to, bucketsFor(w, args.to - args.from, args.intervalMs))
      .then((buckets) => {
        if (cancelled) return;
        svg = timelineSvg(buckets, {
          width: w,
          height: args.height ?? Math.round(Math.min(340, Math.max(140, w * 0.14))),
          from: args.from,
          to: args.to,
          gaps: args.gaps,
          routeChanges: args.routeChanges,
          fontSize: Math.round(rootFont * 0.8),
        });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  });
</script>

<div class="timeline" bind:clientWidth={width}>{@html svg}</div>

<style>
  .timeline {
    width: 100%;
    min-height: 120px;
  }
</style>
