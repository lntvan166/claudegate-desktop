<script lang="ts">
  import { tick } from "svelte";
  import { editorLabel, menuOptions, type EditorConfig } from "../lib/editors";

  let {
    config,
    disabled = false,
    onopen,
    onpick,
  }: {
    config: EditorConfig;
    disabled?: boolean;
    /** Open in the remembered editor. */
    onopen: () => void;
    /** Open with `template` and remember it. */
    onpick: (template: string) => void;
  } = $props();

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  let toggle: HTMLButtonElement | undefined = $state();
  let menu: HTMLDivElement | undefined = $state();

  const options = $derived(menuOptions(config));
  const label = $derived(editorLabel(config.editor));

  const items = () => [...(menu?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') ?? [])];

  async function show(focus: "current" | "first" | "last" = "current") {
    open = true;
    await tick();
    const all = items();
    const i =
      focus === "first" ? 0 : focus === "last" ? all.length - 1 : Math.max(0, options.findIndex((o) => o.current));
    all[i]?.focus();
  }

  function hide(refocus: boolean) {
    open = false;
    if (refocus) toggle?.focus();
  }

  function pick(template: string) {
    hide(true);
    onpick(template);
  }

  function onToggleKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      void show(e.key === "ArrowDown" ? "current" : "last");
    }
  }

  function onMenuKey(e: KeyboardEvent) {
    const all = items();
    const i = all.indexOf(document.activeElement as HTMLButtonElement);
    let next = -1;
    if (e.key === "ArrowDown") next = (i + 1) % all.length;
    else if (e.key === "ArrowUp") next = (i - 1 + all.length) % all.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = all.length - 1;
    else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hide(true);
      return;
    } else if (e.key === "Tab") {
      hide(false);
      return;
    } else return;
    e.preventDefault();
    all[next]?.focus();
  }

  // Close on a press anywhere outside the control while the menu is open.
  $effect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (root && !root.contains(e.target as Node)) hide(false);
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  });
</script>

<div class="split" class:open bind:this={root}>
  <button class="main" {disabled} title="Open in {label} at the first change (O)" onclick={onopen}>
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path
        d="M9.5 2.5h4v4M13.5 2.5 7.5 8.5M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3"
        fill="none"
        stroke="currentColor"
        stroke-width="1.3"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
    <span>Open in {label}</span>
  </button>
  <button
    class="more"
    bind:this={toggle}
    aria-label="Open with"
    aria-haspopup="menu"
    aria-expanded={open}
    title="Open with"
    onclick={() => (open ? hide(false) : show())}
    onkeydown={onToggleKey}
  >
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path d="M4.5 6.5 8 10l3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  </button>

  {#if open}
    <div class="menu" role="menu" aria-label="Open with" tabindex="-1" bind:this={menu} onkeydown={onMenuKey}>
      <p class="menu-title" role="presentation">Open with</p>
      {#each options as o (o.label)}
        <button role="menuitemradio" aria-checked={o.current} tabindex="-1" onclick={() => pick(o.template)}>
          <svg class="check" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            {#if o.current}
              <path d="M3.5 8.5 6.5 11.5 12.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
            {/if}
          </svg>
          <span class="label">{o.label}</span>
          <code class="cmd">{o.template}</code>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .split {
    position: relative;
    display: inline-flex;
    flex: none;
    height: 28px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }

  .split > button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--text);
    transition: background 120ms var(--ease);
  }
  .main {
    padding: 0 10px 0 9px;
    border-radius: calc(var(--radius-control) - 1px) 0 0 calc(var(--radius-control) - 1px);
  }
  .main svg {
    color: var(--muted);
  }
  .more {
    justify-content: center;
    width: 26px;
    border-left: 1px solid var(--control-border);
    border-radius: 0 calc(var(--radius-control) - 1px) calc(var(--radius-control) - 1px) 0;
    color: var(--muted);
  }
  .split > button:hover:not(:disabled) {
    background: var(--hover);
  }
  .split > button:active:not(:disabled) {
    background: var(--active);
  }
  .split.open .more {
    background: var(--active);
    color: var(--text);
  }
  .main:disabled {
    color: var(--faint);
  }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: -1px;
    z-index: 20;
    min-width: 280px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--menu-shadow);
    animation: drop 120ms var(--ease);
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }

  .menu-title {
    padding: 6px 8px 4px;
    font-size: var(--fs-meta);
    color: var(--muted);
  }

  .menu button {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 30px;
    padding: 0 10px 0 6px;
    border-radius: 5px;
    font-size: var(--fs-body);
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--hover);
  }
  .menu button:focus-visible {
    outline: 1px solid var(--focus-ring);
    outline-offset: -1px;
  }
  .check {
    flex: none;
    color: var(--text);
  }
  .label {
    flex: none;
  }
  .cmd {
    margin-left: auto;
    padding-left: 16px;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--faint);
    white-space: nowrap;
  }
</style>
