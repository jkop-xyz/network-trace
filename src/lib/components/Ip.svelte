<script lang="ts">
  import { openAddressMenu } from "$lib/ui.svelte";

  let { address }: { address: string } = $props();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="ip" title="Right-click for options" oncontextmenu={(e) => openAddressMenu(e, address)}>{address}</span>

<style>
  .ip {
    cursor: context-menu;
    border-bottom: 1px dotted transparent;
  }
  .ip:hover {
    border-bottom-color: var(--muted);
  }
</style>
