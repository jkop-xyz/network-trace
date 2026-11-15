<script lang="ts">
  import { lookUpOwner, ui } from "$lib/ui.svelte";

  let copied = $state(false);

  async function copy() {
    if (!ui.menu) return;
    try {
      await navigator.clipboard.writeText(ui.menu.address);
      copied = true;
      setTimeout(() => {
        copied = false;
        ui.menu = null;
      }, 600);
    } catch {
      ui.menu = null;
    }
  }

  function close() {
    ui.menu = null;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      ui.menu = null;
      ui.owner = null;
    }
  }

  // Keep the menu inside the window.
  const pos = $derived(
    ui.menu
      ? { x: Math.min(ui.menu.x, window.innerWidth - 230), y: Math.min(ui.menu.y, window.innerHeight - 90) }
      : { x: 0, y: 0 },
  );
</script>

<svelte:window onkeydown={onKey} onresize={close} />

{#if ui.menu}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={close} oncontextmenu={(e) => { e.preventDefault(); close(); }}></div>
  <div class="menu" role="menu" style:left="{pos.x}px" style:top="{pos.y}px">
    <div class="head muted">{ui.menu.address}</div>
    <button role="menuitem" onclick={copy}>{copied ? "Copied" : "Copy address"}</button>
    <button role="menuitem" onclick={() => ui.menu && lookUpOwner(ui.menu.address)}>Who owns this address?</button>
  </div>
{/if}

{#if ui.owner}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop dim" onclick={() => (ui.owner = null)}></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Address owner">
    <h2>{ui.owner.address}</h2>
    {#if ui.owner.loading}
      <p class="muted">Looking up the registry…</p>
    {:else if ui.owner.error}
      <p class="error">Lookup failed: {ui.owner.error}</p>
    {:else if ui.owner.result}
      {@const r = ui.owner.result}
      <dl>
        {#if r.organisation}<dt>Organisation</dt><dd>{r.organisation}</dd>{/if}
        {#if r.network}<dt>Network</dt><dd>{r.network}</dd>{/if}
        {#if r.range}<dt>Address block</dt><dd>{r.range}</dd>{/if}
        {#if r.country}<dt>Country</dt><dd>{r.country}</dd>{/if}
      </dl>
      {#if r.source}<p class="muted small">From the public RDAP registry via {r.source}</p>{/if}
    {/if}
    <div class="actions"><button onclick={() => (ui.owner = null)}>Close</button></div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
  }
  .backdrop.dim {
    background: rgb(0 0 0 / 0.3);
  }
  .menu {
    position: fixed;
    z-index: 51;
    min-width: 220px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.25);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .head {
    font-size: 0.85rem;
    padding: 4px 10px;
  }
  .menu button {
    text-align: left;
    border: 0;
    background: none;
    border-radius: 5px;
    padding: 6px 10px;
  }
  .menu button:hover {
    background: var(--row-hover);
  }
  .dialog {
    position: fixed;
    z-index: 51;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(480px, calc(100vw - 32px));
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 16px 20px;
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.3);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 1.15rem;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 14px;
    margin: 0 0 10px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
  }
  .error {
    color: var(--loss);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
