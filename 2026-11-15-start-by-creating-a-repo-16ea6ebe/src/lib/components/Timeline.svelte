<script lang="ts">
  import { api, type Gap, type TimelineHop } from "$lib/api";
  import { bucketsFor, timelineSvg } from "$lib/chart";

  interface Props {
    sessionId: string;
    hop: TimelineHop;
    from: number;
    to: number;
    gaps: Gap[];
    intervalMs: number;
    /** Changes whenever the data should be re-read (new Samples). */
    tick: number;
  }
  let { sessionId, hop, from, to, gaps, intervalMs, tick }: Props = $props();

  let width = $state(0);
  let svg = $state("");

  $effect(() => {
    const w = width;
    const args = [sessionId, hop, from, to, gaps, tick] as const;
    if (w < 50) return;
    let cancelled = false;
    api
      .sessionTimeline(args[0], args[1], args[2], args[3], bucketsFor(w, args[3] - args[2], intervalMs))
      .then((buckets) => {
        if (!cancelled) svg = timelineSvg(buckets, { width: w, from: args[2], to: args[3], gaps: args[4] });
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
    min-height: 206px;
  }
</style>
