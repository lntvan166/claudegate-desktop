<script lang="ts">
  import { tick } from "svelte";
  import type { DiffKind } from "../lib/api";
  import { filterLabel, normalizeFilter } from "../lib/filter";

  let {
    tab,
    counts,
    mode,
    filter = $bindable(""),
    shown = 0,
    total = 0,
    ontab,
    onmode,
  }: {
    tab: DiffKind;
    counts: Record<DiffKind, number>;
    mode: "tree" | "list";
    /** The Pending filter text, raw; the box is on the Pending tab only. */
    filter?: string;
    /** Pending rows the filter shows, and all of them, for the count label. */
    shown?: number;
    total?: number;
    ontab: (t: DiffKind) => void;
    onmode: (m: "tree" | "list") => void;
  } = $props();

  const TABS: { id: DiffKind; label: string }[] = [
    { id: "pending", label: "Pending" },
    { id: "accepted", label: "Accepted" },
    { id: "rejected", label: "Rejected" },
  ];

  let input: HTMLInputElement | undefined = $state();
  let modeGroup: HTMLDivElement | undefined = $state();

  /** In a narrow column only the button for the other layout shows (the way VS Code's own
   *  tree/list action works), so the one just pressed hides: keep focus in the group. */
  async function pickMode(m: "tree" | "list") {
    const hadFocus = !!modeGroup?.contains(document.activeElement);
    onmode(m);
    await tick();
    if (!hadFocus || !modeGroup) return;
    const active = document.activeElement as HTMLElement | null;
    if (active && modeGroup.contains(active) && active.offsetParent !== null) return;
    [...modeGroup.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.offsetParent !== null)?.focus();
  }
  const label = $derived(tab === "pending" ? filterLabel(normalizeFilter(filter), shown, total) : undefined);

  /** `/` from anywhere in the window. */
  export function focusFilter() {
    input?.focus();
    input?.select();
  }

  /** Esc clears a set filter; with nothing to clear it gives focus back to the list. Enter does too. */
  function onfilterkey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (filter !== "") filter = "";
      else input?.blur();
    } else if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      input?.blur();
    }
  }

  function onkey(e: KeyboardEvent) {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const i = TABS.findIndex((t) => t.id === tab);
    const next = TABS[(i + (e.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length];
    ontab(next.id);
    e.preventDefault();
  }
</script>

<div class="bar">
  <div class="tabs" role="tablist" aria-label="Review state" tabindex="-1" onkeydown={onkey}>
    {#each TABS as t (t.id)}
      <button
        role="tab"
        class="tab {t.id}"
        class:active={tab === t.id}
        aria-selected={tab === t.id}
        tabindex={tab === t.id ? 0 : -1}
        title={t.label}
        onclick={() => ontab(t.id)}
      >
        <!-- A narrow column keeps the open tab's name and shows the others as a mark and a count. -->
        <svg class="glyph" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          {#if t.id === "pending"}
            <circle cx="8" cy="8" r="3.2" fill="currentColor" />
          {:else if t.id === "accepted"}
            <path d="M3.5 8.5 6.5 11.5 12.5 4.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
          {:else}
            <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
          {/if}
        </svg>
        <span class="label">{t.label}</span>
        <span class="count num" class:zero={counts[t.id] === 0}>{counts[t.id]}</span>
      </button>
    {/each}
  </div>

  <div class="mode" role="group" aria-label="Layout" bind:this={modeGroup}>
    <button class:on={mode === "tree"} aria-pressed={mode === "tree"} aria-label="Show as a folder tree" title="Folder tree" onclick={() => void pickMode("tree")}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M3 3h4M6 8h7M6 13h7M4 4v8.5c0 .3.2.5.5.5H6M4 8h2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
    </button>
    <button class:on={mode === "list"} aria-pressed={mode === "list"} aria-label="Show as a flat list" title="Flat list" onclick={() => void pickMode("list")}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M3 4h10M3 8h10M3 12h10" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</div>
{#if tab === "pending"}
  <div class="find" class:set={filter !== ""}>
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" stroke-width="1.3" />
      <path d="M10.2 10.2 13.5 13.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
    </svg>
    <input
      bind:this={input}
      bind:value={filter}
      type="text"
      spellcheck="false"
      autocomplete="off"
      placeholder="Filter"
      aria-label="Filter pending files"
      aria-keyshortcuts="/"
      onkeydown={onfilterkey}
    />
    {#if filter === ""}
      <kbd aria-hidden="true">/</kbd>
    {:else}
      <button class="clear" aria-label="Clear the filter" title="Clear the filter (Esc)" onclick={() => { filter = ""; input?.focus(); }}>
        <svg viewBox="0 0 16 16" width="10" height="10" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
      </button>
    {/if}
  </div>
{/if}
{#if label}
  <p class="filterline tnum" title={label}>{label}</p>
{/if}

<style>
  /* One row at every width: the tabs keep their full names while the column has room, then
     step down (below) rather than pushing the layout toggle off the edge. */
  .bar {
    display: flex;
    align-items: stretch;
    gap: 8px;
    height: 40px;
    /* The first tab's name lines up with the project title above it. */
    padding: 0 10px 0 var(--pad);
    border-bottom: 1px solid var(--rule);
    flex: none;
  }

  .tabs {
    display: flex;
    flex: 1 1 auto;
    min-width: 0;
    gap: 2px;
    /* Never wider than its share: with very long counts a tab is cut, not the toggle. */
    overflow: hidden;
  }

  .tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    padding: 0 8px;
    font-size: var(--fs-body);
    font-weight: 500;
    white-space: nowrap;
    color: var(--muted);
    transition: color 120ms var(--ease);
  }
  .tab:first-child {
    margin-left: -8px;
  }
  .tab:hover,
  .tab.active {
    color: var(--text);
  }
  .tab.active {
    font-weight: 600;
  }
  .tab::after {
    content: "";
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 0;
    height: 2px;
    border-radius: 2px 2px 0 0;
    background: transparent;
  }
  .tab.active::after {
    background: var(--text);
  }
  .tab:not(.active):hover::after {
    background: var(--rule);
  }

  .glyph {
    display: none;
    flex: none;
  }
  .tab.pending .glyph {
    color: var(--pending);
  }

  .count {
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .tab.pending .count:not(.zero) {
    color: var(--pending);
  }
  .count.zero {
    color: var(--faint);
    font-weight: 400;
  }

  .find {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    margin: 8px 12px 0;
    padding: 0 6px 0 8px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    color: var(--faint);
    background: var(--surface);
  }
  .find:focus-within {
    border-color: var(--focus-ring);
    color: var(--muted);
  }
  .find.set {
    color: var(--muted);
  }
  .find input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    outline: 0;
    font-size: var(--fs-small);
    color: var(--text);
    background: transparent;
  }
  .find input::placeholder {
    color: var(--faint);
  }
  .find kbd {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--faint);
  }
  .find .clear {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border-radius: 3px;
    color: var(--muted);
  }
  .find .clear:hover {
    color: var(--text);
    background: var(--hover);
  }
  .filterline {
    flex: none;
    padding: 5px 12px;
    border-bottom: 1px solid var(--rule);
    font-size: var(--fs-small);
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Tree or list: a two-button segmented control while there is room for both. */
  .mode {
    display: flex;
    align-items: center;
    flex: none;
  }
  .mode button {
    display: grid;
    place-items: center;
    width: 26px;
    height: 24px;
    border: 1px solid var(--control-border);
    color: var(--faint);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .mode button:first-child {
    border-radius: var(--radius-control) 0 0 var(--radius-control);
  }
  .mode button:last-child {
    margin-left: -1px;
    border-radius: 0 var(--radius-control) var(--radius-control) 0;
  }
  .mode button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .mode button.on {
    color: var(--text);
    background: var(--active);
  }

  /* Under 340px the toggle becomes one button, for the layout you are not in. */
  @container review (max-width: 339px) {
    .mode button.on {
      display: none;
    }
    .mode button,
    .mode button:first-child,
    .mode button:last-child {
      width: 26px;
      height: 26px;
      margin: 0;
      border-color: transparent;
      border-radius: var(--radius-control);
    }
  }

  /* Under 310px the tabs you are not on show a mark and a count; the name stays for screen
     readers and in the tooltip. */
  @container review (max-width: 309px) {
    .bar {
      padding-left: 14px;
    }
    .tab {
      gap: 5px;
      padding: 0 6px;
    }
    .tab:first-child {
      margin-left: -6px;
    }
    .tab::after {
      left: 6px;
      right: 6px;
    }
    .tab:not(.active) .glyph {
      display: block;
    }
    .tab:not(.active) .label {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }
  }
</style>
