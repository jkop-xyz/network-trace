<script lang="ts">
  import { api, type SessionSummary } from "$lib/api";
  import { bytes, shortStamp } from "$lib/format";

  interface Props {
    sessions: SessionSummary[];
    selectedId: string | null;
    onSelect: (id: string) => void;
    onStarted: (id: string) => void;
    onStorage: () => void;
    onSettings: () => void;
  }
  let { sessions, selectedId, onSelect, onStarted, onStorage, onSettings }: Props = $props();

  let target = $state("");
  let starting = $state(false);
  let message = $state("");

  const ordered = $derived([...sessions].sort((a, b) => Number(b.running) - Number(a.running) || b.startedAt - a.startedAt));
  const totalBytes = $derived(sessions.reduce((n, s) => n + s.sizeBytes, 0));

  async function start(e: Event) {
    e.preventDefault();
    if (!target.trim() || starting) return;
    starting = true;
    message = "";
    try {
      const res = await api.sessionStart(target);
      if (res.alreadyRunning) message = `Already monitoring ${target.trim()}: showing that Session.`;
      target = "";
      onStarted(res.id);
    } catch (err) {
      message = String(err);
    } finally {
      starting = false;
    }
  }

  const statusLabel = { ok: "Target answering", loss: "Target not answering", waiting: "Starting", stopped: "Stopped" };
</script>

<aside>
  <form onsubmit={start}>
    <input placeholder="Hostname or IP address" bind:value={target} disabled={starting} aria-label="Target" />
    <button class="primary" disabled={starting || !target.trim()}>{starting ? "Starting…" : "Start"}</button>
  </form>
  {#if message}<p class="message small">{message}</p>{/if}

  <nav aria-label="Sessions">
    {#each ordered as s (s.id)}
      <button class="session" class:selected={s.id === selectedId} onclick={() => onSelect(s.id)}>
        <span class="dot {s.status}" title={statusLabel[s.status]}></span>
        <span class="text">
          <span class="target">{s.target}</span>
          <span class="muted small">{shortStamp(s.startedAt)}{s.running ? " · running" : ""}</span>
        </span>
      </button>
    {/each}
  </nav>

  <footer>
    <button class="link" onclick={onStorage} title="Manage stored Sessions">Storage: {bytes(totalBytes)}</button>
    <button class="link" onclick={onSettings}>Settings</button>
  </footer>
</aside>

<style>
  aside {
    background: var(--sidebar);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  form {
    display: flex;
    gap: 6px;
    padding: 12px;
  }
  form input {
    flex: 1;
    min-width: 0;
  }
  .message {
    margin: -4px 12px 8px;
    color: var(--warn);
  }
  nav {
    flex: 1;
    overflow: auto;
    padding: 0 6px;
  }
  .session {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 7px 8px;
    border-radius: 6px;
  }
  .session:hover {
    background: var(--row-hover);
  }
  .session.selected {
    background: var(--row-selected);
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .target {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
    background: var(--border);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.loss {
    background: var(--loss);
  }
  .dot.waiting {
    background: var(--warn);
  }
  footer {
    display: flex;
    justify-content: space-between;
    padding: 8px 12px;
    border-top: 1px solid var(--border);
  }
  .link {
    border: 0;
    background: none;
    padding: 2px 0;
    color: var(--muted);
  }
  .link:hover {
    color: var(--accent);
  }
</style>
