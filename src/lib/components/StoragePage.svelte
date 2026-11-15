<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type SessionSummary } from "$lib/api";
  import { bytes, duration, shortStamp } from "$lib/format";

  interface Props {
    sessions: SessionSummary[];
    onChanged: () => void;
    onOpen: (id: string) => void;
    onBack: () => void;
  }
  let { sessions, onChanged, onOpen, onBack }: Props = $props();

  let selected: string[] = $state([]);
  let olderThanDays = $state(30);
  let message = $state("");

  const bySize = $derived([...sessions].sort((a, b) => b.sizeBytes - a.sizeBytes));
  const total = $derived(sessions.reduce((n, s) => n + s.sizeBytes, 0));
  const selectedBytes = $derived(sessions.filter((s) => selected.includes(s.id)).reduce((n, s) => n + s.sizeBytes, 0));

  function toggle(id: string) {
    selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
  }

  async function remove(ids: string[], what: string) {
    if (!ids.length) return;
    if (!(await ask(`Delete ${what}? This cannot be undone.`, { title: "Delete Sessions", kind: "warning" }))) return;
    try {
      await api.sessionsDelete(ids);
      message = `Deleted ${ids.length} Session${ids.length === 1 ? "" : "s"}.`;
      selected = [];
    } catch (e) {
      message = String(e);
    }
    onChanged();
  }

  async function deleteOld() {
    const cutoff = Date.now() - olderThanDays * 86_400_000;
    const ids = sessions.filter((s) => !s.running && (s.stoppedAt ?? s.startedAt) < cutoff).map((s) => s.id);
    if (!ids.length) {
      message = `No stopped Sessions older than ${olderThanDays} days.`;
      return;
    }
    remove(ids, `${ids.length} Session${ids.length === 1 ? "" : "s"} older than ${olderThanDays} days`);
  }
</script>

<div class="page">
  <header>
    <button onclick={onBack}>← Back</button>
    <h1>Storage</h1>
  </header>
  <p>
    <strong>{bytes(total)}</strong> used by {sessions.length} Session{sessions.length === 1 ? "" : "s"}.
    <button class="link" onclick={() => api.openDataDir()}>Open folder</button>
  </p>

  <div class="tools">
    <button class="danger" disabled={!selected.length} onclick={() => remove(selected, `${selected.length} selected Session${selected.length === 1 ? "" : "s"} (${bytes(selectedBytes)})`)}>
      Delete selected{selected.length ? ` (${bytes(selectedBytes)})` : ""}
    </button>
    <span class="muted">or delete stopped Sessions older than</span>
    <select bind:value={olderThanDays}>
      {#each [7, 14, 30, 90, 180, 365] as d (d)}<option value={d}>{d} days</option>{/each}
    </select>
    <button class="danger" onclick={deleteOld}>Delete</button>
  </div>
  {#if message}<p class="muted">{message}</p>{/if}

  <table>
    <thead>
      <tr><th></th><th>Target</th><th>Started</th><th>Length</th><th class="num">Size</th><th></th></tr>
    </thead>
    <tbody>
      {#each bySize as s (s.id)}
        <tr>
          <td>
            <input type="checkbox" checked={selected.includes(s.id)} disabled={s.running} onchange={() => toggle(s.id)} aria-label="Select {s.target}" />
          </td>
          <td><button class="link strong" onclick={() => onOpen(s.id)}>{s.target}</button></td>
          <td>{shortStamp(s.startedAt)}</td>
          <td>{s.running ? "running" : duration((s.stoppedAt ?? s.startedAt) - s.startedAt)}</td>
          <td class="num">{bytes(s.sizeBytes)}</td>
          <td class="muted small">{s.running ? "Stop it to delete" : ""}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .page {
    padding: 16px 20px;
    max-width: 1100px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  h1 {
    font-size: 1.55rem;
    margin: 0;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 12px 0;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  th,
  td {
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    text-align: left;
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 0.9rem;
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
  }
  .strong {
    font-weight: 600;
  }
</style>
