<script lang="ts">
  import { onMount, tick } from "svelte";

  let {
    x,
    y,
    label,
    hint = "",
    onpick,
    onclose,
  }: {
    x: number;
    y: number;
    label: string;
    /** The shortcut, shown at the right edge. */
    hint?: string;
    onpick: () => void;
    onclose: () => void;
  } = $props();

  let menu: HTMLDivElement | undefined = $state();
  let item: HTMLButtonElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);

  onMount(() => {
    left = x;
    top = y;
    // Keep the menu inside the window.
    void tick().then(() => {
      const r = menu?.getBoundingClientRect();
      if (r) {
        left = Math.max(4, Math.min(x, window.innerWidth - r.width - 4));
        top = Math.max(4, Math.min(y, window.innerHeight - r.height - 4));
      }
      item?.focus();
    });
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
  function outside(e: Event) {
    if (!menu?.contains(e.target as Node)) onclose();
  }
</script>

<svelte:window onpointerdown={outside} onblur={onclose} onresize={onclose} />

<div class="menu" role="menu" aria-label="Actions" tabindex="-1" bind:this={menu} style:left="{left}px" style:top="{top}px" {onkeydown}>
  <button role="menuitem" bind:this={item} onclick={onpick}>
    <span>{label}</span>
    {#if hint}<kbd>{hint}</kbd>{/if}
  </button>
</div>

<style>
  .menu {
    position: fixed;
    z-index: 45;
    min-width: 220px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--menu-shadow);
    animation: pop 100ms var(--ease);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
  }
  button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    width: 100%;
    height: 30px;
    padding: 0 10px 0 8px;
    border-radius: 5px;
    font-size: var(--fs-body);
    text-align: left;
    color: var(--text);
  }
  button:hover,
  button:focus-visible {
    background: var(--hover);
  }
  button:focus-visible {
    outline: 1px solid var(--focus-ring);
    outline-offset: -1px;
  }
  kbd {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--faint);
  }
</style>
