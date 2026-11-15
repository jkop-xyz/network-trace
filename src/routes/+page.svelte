<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api, type SessionSummary } from "$lib/api";
  import AddressMenu from "$lib/components/AddressMenu.svelte";
  import Overview from "$lib/components/Overview.svelte";
  import SessionPanel from "$lib/components/SessionPanel.svelte";
  import SettingsPage from "$lib/components/SettingsPage.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import StoragePage from "$lib/components/StoragePage.svelte";

  let sessions: SessionSummary[] = $state([]);
  let selectedId = $state<string | null>(null);
  let page: "session" | "overview" | "storage" | "settings" = $state("session");
  let error = $state("");
  let notice = $state("");

  const selected = $derived(sessions.find((s) => s.id === selectedId) ?? null);
  const sysctlFix = 'sudo sysctl -w net.ipv4.ping_group_range="0 2147483647"';

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
    const unlisten = [
      listen("session-sample", refreshSoon),
      listen("session-stopped", refresh),
      listen("sessions-changed", refresh),
    ];
    // No browser context menu (back/reload…) except where text is edited.
    const noMenu = (e: MouseEvent) => {
      if (!(e.target as HTMLElement | null)?.closest("input, textarea")) e.preventDefault();
    };
    document.addEventListener("contextmenu", noMenu);
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
      document.removeEventListener("contextmenu", noMenu);
    };
  });

  // In narrow windows the sidebar becomes a drawer behind a "Sessions" button.
  let drawerOpen = $state(false);

  function go(p: typeof page) {
    page = p;
    drawerOpen = false;
  }

  function open(id: string, message?: string) {
    drawerOpen = false;
    selectedId = id;
    page = "session";
    error = "";
    notice = message ?? "";
    refresh();
  }
</script>

<div class="app" class:drawer-open={drawerOpen}>
  <div class="sidebar-slot">
    <Sidebar
      {sessions}
      {selectedId}
      {page}
      onSelect={(id) => open(id)}
      onStarted={(id, message) => open(id, message)}
      onError={(m) => {
        error = m;
        drawerOpen = false;
      }}
      onOverview={() => go("overview")}
      onStorage={() => go("storage")}
      onSettings={() => go("settings")}
    />
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={() => (drawerOpen = false)}></div>
  <main>
    <div class="topbar">
      <button onclick={() => (drawerOpen = true)} aria-label="Show Sessions">☰ Sessions</button>
      <span class="muted">{page === "session" ? (selected?.target ?? "") : page[0].toUpperCase() + page.slice(1)}</span>
    </div>
    {#if error}
      <div class="banner error" role="alert">
        <div>
          {error}
          {#if error.includes("ping_group_range")}
            <div class="fix">
              Run this once in a terminal, then try again:
              <code>{sysctlFix}</code>
              <button onclick={() => navigator.clipboard.writeText(sysctlFix)}>Copy</button>
            </div>
          {/if}
        </div>
        <button onclick={() => (error = "")}>Dismiss</button>
      </div>
    {/if}
    {#if notice}
      <div class="banner info"><span>{notice}</span><button onclick={() => (notice = "")}>Dismiss</button></div>
    {/if}
    {#if page === "overview"}
      <Overview {sessions} onOpen={(id) => open(id)} />
    {:else if page === "storage"}
      <StoragePage {sessions} onChanged={refresh} onOpen={(id) => open(id)} onBack={() => (page = "session")} />
    {:else if page === "settings"}
      <SettingsPage onBack={() => (page = "session")} />
    {:else if selected}
      {#key selected.id}
        <SessionPanel summary={selected} onChanged={refresh} onOpen={(id, message) => open(id, message)} />
      {/key}
    {:else}
      <div class="empty">
        <h2>No Sessions yet</h2>
        <p class="muted">Enter a hostname or IP address on the left and press Start to begin monitoring the route to it.</p>
        <p class="muted">Good first targets: your router, a server you have problems reaching, or <code>1.1.1.1</code>.</p>
        <p class="muted">Monitoring keeps running in the tray when you close this window.</p>
      </div>
    {/if}
  </main>
</div>
<AddressMenu />

<style>
  .app {
    display: grid;
    grid-template-columns: clamp(260px, 20vw, 360px) 1fr;
    height: 100vh;
  }
  main {
    overflow: auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  main > :global(*) {
    flex: none;
  }
  .sidebar-slot {
    display: flex;
    min-height: 0;
  }
  .sidebar-slot > :global(aside) {
    flex: 1;
  }
  .topbar,
  .scrim {
    display: none;
  }
  @media (max-width: 820px) {
    .app {
      grid-template-columns: 1fr;
    }
    .sidebar-slot {
      position: fixed;
      inset: 0 auto 0 0;
      width: min(300px, 85vw);
      z-index: 40;
      transform: translateX(-100%);
      transition: transform 0.15s ease-out;
      box-shadow: 0 0 24px rgb(0 0 0 / 0.35);
    }
    .drawer-open .sidebar-slot {
      transform: none;
    }
    .drawer-open .scrim {
      display: block;
      position: fixed;
      inset: 0;
      z-index: 39;
      background: rgb(0 0 0 / 0.3);
    }
    .topbar {
      display: flex;
      align-items: center;
      gap: 10px;
      padding: 8px 1rem;
      border-bottom: 1px solid var(--border);
      background: var(--sidebar);
      position: sticky;
      top: 0;
      z-index: 10;
    }
  }
  .empty {
    max-width: 560px;
    margin: 80px auto;
    text-align: center;
  }
  .banner {
    margin: 12px 1.5rem 0;
    padding: 8px 12px;
    border-radius: 6px;
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }
  .banner.error {
    background: var(--row-lossy);
    color: var(--loss);
  }
  .banner.info {
    background: var(--row-selected);
  }
  .fix {
    margin-top: 6px;
    color: var(--text);
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  code {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 6px;
  }
</style>
