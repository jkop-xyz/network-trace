<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api, type SessionSummary } from "$lib/api";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import SessionPanel from "$lib/components/SessionPanel.svelte";
  import StoragePage from "$lib/components/StoragePage.svelte";
  import SettingsPage from "$lib/components/SettingsPage.svelte";

  let sessions: SessionSummary[] = $state([]);
  let selectedId = $state<string | null>(null);
  let page: "session" | "storage" | "settings" = $state("session");
  let error = $state("");

  const selected = $derived(sessions.find((s) => s.id === selectedId) ?? null);

  async function refresh() {
    try {
      sessions = await api.sessionsList();
      if (selectedId && !sessions.some((s) => s.id === selectedId)) selectedId = null;
      if (!selectedId && sessions.length) selectedId = sessions.find((s) => s.running)?.id ?? sessions[0].id;
    } catch (e) {
      error = String(e);
    }
  }

  // Sample events arrive every interval per running Session; refresh the
  // list (status dots, sizes) at most every two seconds.
  let pending = false;
  function refreshSoon() {
    if (pending) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      refresh();
    }, 2000);
  }

  onMount(() => {
    refresh();
    const unlisten = [listen("session-sample", refreshSoon), listen("session-stopped", refresh)];
    return () => unlisten.forEach((p) => p.then((f) => f()));
  });

  function open(id: string) {
    selectedId = id;
    page = "session";
    refresh();
  }
</script>

<div class="app">
  <Sidebar
    {sessions}
    {selectedId}
    onSelect={(id) => open(id)}
    onStarted={(id) => open(id)}
    onStorage={() => (page = "storage")}
    onSettings={() => (page = "settings")}
  />
  <main>
    {#if error}
      <div class="error" role="alert">{error} <button onclick={() => (error = "")}>Dismiss</button></div>
    {/if}
    {#if page === "storage"}
      <StoragePage {sessions} onChanged={refresh} onOpen={open} onBack={() => (page = "session")} />
    {:else if page === "settings"}
      <SettingsPage onBack={() => (page = "session")} />
    {:else if selected}
      {#key selected.id}
        <SessionPanel summary={selected} onChanged={refresh} onOpen={open} />
      {/key}
    {:else}
      <div class="empty">
        <h2>No Sessions yet</h2>
        <p class="muted">Enter a hostname or IP address on the left and press Start to begin monitoring the route to it.</p>
        <p class="muted">Good first targets: your router, a server you have problems reaching, or <code>1.1.1.1</code>.</p>
      </div>
    {/if}
  </main>
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: 270px 1fr;
    height: 100vh;
  }
  main {
    overflow: auto;
    min-width: 0;
  }
  .empty {
    max-width: 520px;
    margin: 80px auto;
    text-align: center;
  }
  .error {
    margin: 12px 16px 0;
    padding: 8px 12px;
    border-radius: 6px;
    background: var(--row-lossy);
    color: var(--loss);
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
</style>
