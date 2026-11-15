<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Settings } from "$lib/api";
  import { interval } from "$lib/format";

  let { onBack }: { onBack: () => void } = $props();

  let settings = $state<Settings | null>(null);
  let saved = $state("");

  onMount(async () => {
    settings = await api.settingsGet();
  });

  async function persist() {
    if (!settings) return;
    settings = await api.settingsSet(settings);
    saved = "Saved. Changes apply to Sessions started from now on.";
  }
</script>

<div class="page">
  <header>
    <button onclick={onBack}>← Back</button>
    <h1>Settings</h1>
  </header>
  {#if settings}
    <label>
      <span>Sample interval for new Sessions</span>
      <select bind:value={settings.defaultIntervalMs} onchange={persist}>
        {#each [1000, 2500, 5000, 10000, 15000, 30000, 60000] as i (i)}<option value={i}>{interval(i)}</option>{/each}
      </select>
    </label>
    <label>
      <span>A Probe counts as lost after</span>
      <select bind:value={settings.timeoutMs} onchange={persist}>
        {#each [1000, 2000, 3000, 5000, 10000] as t (t)}<option value={t}>{interval(t)}</option>{/each}
      </select>
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={settings.publicIpInReport} onchange={persist} />
      <span>Include my public IP address in Reports <span class="muted small">(looked up from api.ipify.org only when a Report is made)</span></span>
    </label>
    {#if saved}<p class="muted small">{saved}</p>{/if}
  {/if}
</div>

<style>
  .page {
    padding: 16px 20px;
    max-width: 640px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }
  h1 {
    font-size: 20px;
    margin: 0;
  }
  label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
  }
  label.check {
    justify-content: flex-start;
  }
</style>
