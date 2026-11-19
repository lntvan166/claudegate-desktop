<script lang="ts">
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
        onclick={() => ontab(t.id)}
      >
        {t.label}
        <span class="count num" class:zero={counts[t.id] === 0}>{counts[t.id]}</span>
      </button>
    {/each}
  </div>

  <div class="mode" role="group" aria-label="Layout">
    <button class:on={mode === "tree"} aria-pressed={mode === "tree"} aria-label="Show as a folder tree" title="Folder tree" onclick={() => onmode("tree")}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M3 3h4M6 8h7M6 13h7M4 4v8.5c0 .3.2.5.5.5H6M4 8h2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
    </button>
    <button class:on={mode === "list"} aria-pressed={mode === "list"} aria-label="Show as a flat list" title="Flat list" onclick={() => onmode("list")}>
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
  <p class="filterline num" title={label}>{label}</p>
{/if}

<style>
  .bar {
    display: flex;
    align-items: stretch;
    justify-content: space-between;
    height: 40px;
    padding: 0 12px 0 10px;
    border-bottom: 1px solid var(--rule);
    flex: none;
  }

  .tabs {
    display: flex;
  }

  .tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 9px;
    font-size: var(--fs-body);
    font-weight: 500;
    color: var(--muted);
    transition: color 120ms var(--ease);
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
    left: 9px;
    right: 9px;
    bottom: -1px;
    height: 2px;
    background: transparent;
  }
  .tab.active::after {
    background: var(--text);
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

  .mode {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .mode button {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-control);
    color: var(--faint);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .mode button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .mode button.on {
    color: var(--text);
    background: var(--active);
  }
</style>
