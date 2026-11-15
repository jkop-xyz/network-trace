<script lang="ts">
  import { api, type SessionSummary } from "$lib/api";
  import { bytes, shortStamp } from "$lib/format";
  import { appVersion } from "$lib/version";

  let version = $state("");
  appVersion().then((v) => (version = v));

  interface Props {
    sessions: SessionSummary[];
    selectedId: string | null;
    page: string;
    onSelect: (id: string) => void;
    onStarted: (id: string, note?: string) => void;
    onError: (message: string) => void;
    onOverview: () => void;
    onStorage: () => void;
    onSettings: () => void;
  }
  let { sessions, selectedId, page, onSelect, onStarted, onError, onOverview, onStorage, onSettings }: Props = $props();

  let target = $state("");
  let starting = $state(false);

  const ordered = $derived([...sessions].sort((a, b) => Number(b.running) - Number(a.running) || b.startedAt - a.startedAt));
  const totalBytes = $derived(sessions.reduce((n, s) => n + s.sizeBytes, 0));
  const runningCount = $derived(sessions.filter((s) => s.running).length);

  async function start(e: Event) {
    e.preventDefault();
    if (!target.trim() || starting) return;
    starting = true;
    try {
      const res = await api.sessionStart(target);
      const typed = target.trim();
      target = "";
      onStarted(res.id, res.alreadyRunning ? `Already monitoring ${typed}: showing that Session.` : undefined);
    } catch (err) {
      onError(String(err));
    } finally {
      starting = false;
    }
  }

  const statusText = { ok: "answering", loss: "not answering", waiting: "starting", stopped: "stopped" };
</script>

<aside>
  <form onsubmit={start}>
    <label for="target-input" class="muted small">Monitor a host</label>
    <div class="row">
      <input id="target-input" size="1" placeholder="Hostname or IP address" bind:value={target} disabled={starting} autocomplete="off" spellcheck="false" />
      <button class="primary" disabled={starting || !target.trim()}>{starting ? "…" : "Start"}</button>
    </div>
  </form>

  <button class="nav" class:selected={page === "overview"} onclick={onOverview} disabled={!sessions.length}>
    Overview <span class="muted small">{runningCount} running</span>
  </button>

  <nav aria-label="Sessions">
    {#each ordered as s (s.id)}
      <button class="session" class:selected={page === "session" && s.id === selectedId} onclick={() => onSelect(s.id)}>
        <span class="dot {s.status}" aria-hidden="true"></span>
        <span class="text">
          <span class="target">{s.target}</span>
          <span class="muted small"
            >{shortStamp(s.startedAt)} · <span class="state {s.status}">{s.error ? "failed" : statusText[s.status]}</span></span
          >
        </span>
      </button>
    {/each}
  </nav>

  <footer>
    <button class="link" class:active={page === "storage"} onclick={onStorage} title="Manage stored Sessions">Storage: {bytes(totalBytes)}</button>
    <button class="link" class:active={page === "settings"} onclick={onSettings}>Settings</button>
  </footer>
  {#if version}<div class="version muted" title="Network Trace version">v{version}</div>{/if}
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
    padding: 14px 12px 10px;
  }
  form label {
    display: block;
    margin-bottom: 4px;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .row input {
    flex: 1;
    min-width: 0;
    font-size: 1.05rem;
    padding: 8px 10px;
  }
  .row button {
    padding: 8px 14px;
  }
  .nav {
    margin: 2px 6px 6px;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 7px 10px;
    border-radius: 6px;
    font-weight: 600;
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .nav:hover:not(:disabled),
  .session:hover {
    background: var(--row-hover);
  }
  .nav.selected,
  .session.selected {
    background: var(--row-selected);
  }
  nav {
    flex: 1;
    overflow: auto;
    padding: 0 6px;
    border-top: 1px solid var(--border);
    padding-top: 6px;
  }
  .session {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 7px 10px;
    border-radius: 6px;
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
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: none;
    border: 2px solid var(--muted);
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
  .state.loss {
    color: var(--loss);
  }
  .version {
    font-size: 0.75rem;
    padding: 0 12px 6px;
    margin-top: -4px;
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
  .link:hover,
  .link.active {
    color: var(--accent);
  }
</style>
