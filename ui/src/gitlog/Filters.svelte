<script lang="ts">
  import { RANGES, addAuthor, type FilterState } from "./log";

  // The filter bar above the commit list: search, authors (chips and Me), path, branch and the
  // date range. It edits `filter` in place; the list watches it and debounces the query.
  let {
    filter = $bindable(),
    loading,
    onrefresh,
    onexit,
  }: {
    filter: FilterState;
    loading: boolean;
    onrefresh: () => void;
    /** Esc or ↓ in the search box: hand the keyboard to the list. */
    onexit: () => void;
  } = $props();

  let searchEl: HTMLInputElement | undefined = $state();
  let authorDraft = $state("");

  /** `/` lands here. */
  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  function searchKey(e: KeyboardEvent) {
    if (e.key === "Escape" || e.key === "ArrowDown" || e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      onexit();
    }
  }

  function commitAuthor() {
    const next = addAuthor(filter.authors, authorDraft);
    if (next !== filter.authors) filter.authors = next;
    authorDraft = "";
  }
  function authorKey(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      commitAuthor();
    } else if (e.key === "Backspace" && authorDraft === "" && filter.authors.length) {
      filter.authors = filter.authors.slice(0, -1);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (authorDraft) authorDraft = "";
      else onexit();
    }
  }
  function removeAuthor(a: string) {
    filter.authors = filter.authors.filter((x) => x !== a);
  }
  function fieldKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onexit();
    }
  }
</script>

<div class="filters" role="search" aria-label="Filter commits">
  <div class="field search">
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" stroke-width="1.3" /><path d="M10.2 10.2 13 13" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /></svg>
    <input bind:this={searchEl} bind:value={filter.search} placeholder="Search messages" aria-label="Search commit messages" aria-keyshortcuts="/" onkeydown={searchKey} spellcheck="false" />
  </div>

  <div class="field authors">
    <button class="me" type="button" aria-pressed={filter.mine} onclick={() => (filter.mine = !filter.mine)} title="Only your commits, by each repository's git user.email">Me</button>
    {#each filter.authors as a (a)}
      <span class="achip">{a}<button type="button" aria-label="Remove author {a}" onclick={() => removeAuthor(a)}>×</button></span>
    {/each}
    <input bind:value={authorDraft} placeholder={filter.authors.length || filter.mine ? "Add author" : "Author"} aria-label="Author" onkeydown={authorKey} onblur={commitAuthor} spellcheck="false" />
  </div>

  <div class="field path">
    <input bind:value={filter.path} placeholder="Path" aria-label="Only commits touching this path" onkeydown={fieldKey} spellcheck="false" />
  </div>
  <div class="field branch">
    <input bind:value={filter.branch} placeholder="Branch" aria-label="Branch to show" onkeydown={fieldKey} spellcheck="false" />
  </div>

  <div class="range" role="group" aria-label="Date range">
    {#each RANGES as r (r.id)}
      <button type="button" aria-pressed={filter.range === r.id} class:on={filter.range === r.id} onclick={() => (filter.range = r.id)}>{r.label}</button>
    {/each}
  </div>
  {#if filter.range === "custom"}
    <div class="dates">
      <input type="date" bind:value={filter.from} aria-label="From date" max={filter.to || undefined} />
      <span aria-hidden="true">–</span>
      <input type="date" bind:value={filter.to} aria-label="To date" min={filter.from || undefined} />
    </div>
  {/if}

  <button class="refresh" type="button" onclick={onrefresh} aria-label="Refresh the log" title="Refresh">
    <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true" class:spin={loading}>
      <path d="M13 8a5 5 0 1 1-1.6-3.7M13 2.5V5h-2.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  </button>
</div>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    min-height: 44px;
    padding: 8px var(--pad);
    border-bottom: 1px solid var(--rule);
  }
  .field {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    height: 26px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--muted);
  }
  .field:focus-within {
    outline: 2px solid var(--focus-ring);
    outline-offset: -1px;
  }
  .field input {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 8px;
    border: 0;
    outline: none;
    background: transparent;
    font: inherit;
    font-size: var(--fs-small);
    color: var(--text);
  }
  .search {
    flex: 2 1 180px;
    padding-left: 8px;
  }
  .search input {
    padding-left: 2px;
  }
  .authors {
    flex: 2 1 190px;
    padding-left: 3px;
    overflow: hidden;
  }
  .authors input {
    min-width: 70px;
  }
  .path,
  .branch {
    flex: 1 1 110px;
  }
  .me {
    flex: none;
    height: 20px;
    padding: 0 7px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--muted);
  }
  .me:hover {
    background: var(--hover);
  }
  .me[aria-pressed="true"] {
    color: var(--on-primary);
    background: var(--primary);
  }
  .achip {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 2px;
    max-width: 120px;
    height: 20px;
    padding: 0 2px 0 7px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--text);
    background: var(--active);
    white-space: nowrap;
  }
  .achip button {
    width: 16px;
    height: 16px;
    border-radius: 3px;
    line-height: 1;
    color: var(--muted);
  }
  .achip button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .range {
    display: inline-flex;
    flex: none;
    height: 26px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .range button {
    padding: 0 9px;
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .range button + button {
    border-left: 1px solid var(--control-border);
  }
  .range button:hover {
    background: var(--hover);
  }
  .range button.on {
    color: var(--text);
    font-weight: 600;
    background: var(--active);
  }
  .dates {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
  }
  .dates input {
    height: 26px;
    padding: 0 6px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font: inherit;
    font-size: var(--fs-small);
    color: var(--text);
    background: var(--surface);
    color-scheme: light dark;
  }
  .refresh {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-left: auto;
    border-radius: var(--radius-control);
    color: var(--muted);
  }
  .refresh:hover {
    background: var(--hover);
    color: var(--text);
  }
  .spin {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  button:focus-visible,
  .dates input:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -1px;
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
</style>
