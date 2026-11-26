<script lang="ts">
  import { tick } from "svelte";
  import {
    UNCERTAIN_TITLE,
    itemTime,
    missedHeader,
    relPath,
    rove,
    scopeTag,
    type AuditItem,
    type AuditView,
  } from "../lib/audit";

  let {
    audit,
    projectRoot,
    scopeRoots,
    canWrite = true,
    onopen,
    ondismiss,
  }: {
    audit: AuditView;
    projectRoot: string;
    scopeRoots: string[];
    /** Dismiss writes the app's config; false while read-only mode blocks it. */
    canWrite?: boolean;
    onopen: (item: AuditItem) => void;
    /** Hide these paths now; the owner restores them and says why if the backend refuses. */
    ondismiss: (paths: string[]) => void;
  } = $props();

  let open = $state(false);
  let uncertainOpen = $state(false);
  /** The row that holds the roving tab stop. */
  let cur = $state<string | null>(null);
  let panel: HTMLDivElement | undefined = $state();

  const header = $derived(missedHeader(audit.missed.length));
  const rows = $derived([...audit.missed, ...(uncertainOpen ? audit.uncertain : [])]);
  const stop = $derived(rows.some((r) => r.path === cur) ? cur : (rows[0]?.path ?? null));

  function els(): HTMLElement[] {
    return panel ? [...panel.querySelectorAll<HTMLElement>("[data-audit-row]")] : [];
  }

  async function focusPath(path: string | undefined) {
    if (!path) return;
    cur = path;
    await tick();
    els().find((e) => e.dataset.auditRow === path)?.focus();
  }

  function dismiss(paths: string[]) {
    if (!canWrite || paths.length === 0) return;
    // Keep the keyboard in the list: the row after the first dismissed one, else the one before.
    const i = rows.findIndex((r) => paths.includes(r.path));
    const next = rows.slice(i + 1).find((r) => !paths.includes(r.path)) ?? rows.slice(0, i).reverse().find((r) => !paths.includes(r.path));
    ondismiss(paths);
    if (next) void focusPath(next.path);
  }

  function key(e: KeyboardEvent, it: AuditItem) {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter") {
      e.preventDefault();
      onopen(it);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      dismiss([it.path]);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Home" || e.key === "End") {
      e.preventDefault();
      const i = rows.findIndex((r) => r.path === it.path);
      void focusPath(rows[rove(i, e.key, rows.length)]?.path);
    }
  }
</script>

{#if header}
  <div class="nc">
    <button class="head" aria-expanded={open} aria-controls="nc-panel" onclick={() => (open = !open)}>
      <svg class="chev" class:open viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
        <path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{header}</span>
    </button>

    {#if open}
      <div class="panel" id="nc-panel" bind:this={panel}>
        <p class="why">
          Nothing was recorded before these changed, so there is no Reject. Open one to check it.
        </p>

        {#snippet list(items: AuditItem[], label: string)}
          <ul class="list" aria-label={label}>
            {#each items as it (it.path)}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
              <li
                class="row"
                data-audit-row={it.path}
                tabindex={stop === it.path ? 0 : -1}
                onkeydown={(e) => key(e, it)}
                onfocus={() => (cur = it.path)}
                ondblclick={() => onopen(it)}
              >
                <span class="path" title={it.path}>
                  {#if scopeTag(it, projectRoot, scopeRoots)}<span class="tag">{scopeTag(it, projectRoot, scopeRoots)}</span>{/if}{relPath(it, scopeRoots)}
                </span>
                <span class="meta">{itemTime(it)}{it.commandHead ? " · " : ""}<code>{it.commandHead}</code></span>
                <span class="acts">
                  <button tabindex="-1" aria-label={`Open ${relPath(it, scopeRoots)} in editor`} onclick={() => onopen(it)}>Open in editor</button>
                  <button
                    tabindex="-1"
                    aria-label={`Dismiss ${relPath(it, scopeRoots)}`}
                    disabled={!canWrite}
                    title={canWrite ? "Hide this file from the list (Delete)" : "Read-only mode is on"}
                    onclick={() => dismiss([it.path])}>Dismiss</button
                  >
                </span>
              </li>
            {/each}
          </ul>
        {/snippet}

        <div class="sec-head">
          <span class="sec-title">Not captured</span>
          <button class="link" disabled={!canWrite} onclick={() => dismiss(audit.missed.map((i) => i.path))}>Dismiss all</button>
        </div>
        {@render list(audit.missed, "Files Claude changed that weren't captured")}

        {#if audit.uncertain.length > 0}
          <div class="sec-head">
            <button class="disclose" aria-expanded={uncertainOpen} aria-controls="nc-uncertain" onclick={() => (uncertainOpen = !uncertainOpen)}>
              <svg class="chev" class:open={uncertainOpen} viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                <path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
              <span>{UNCERTAIN_TITLE} · {audit.uncertain.length}</span>
            </button>
            {#if uncertainOpen}
              <button class="link" disabled={!canWrite} onclick={() => dismiss(audit.uncertain.map((i) => i.path))}>Dismiss all</button>
            {/if}
          </div>
          <div id="nc-uncertain">
            {#if uncertainOpen}{@render list(audit.uncertain, UNCERTAIN_TITLE)}{/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .nc {
    flex: none;
    padding: 0 var(--pad) 8px;
    font-size: var(--fs-small);
  }
  .head,
  .disclose {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 22px;
    color: var(--muted);
    font-size: var(--fs-small);
    text-align: left;
  }
  .head:hover,
  .disclose:hover {
    color: var(--text);
  }
  .chev {
    flex: none;
    transition: transform 120ms var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .panel {
    margin-top: 6px;
    max-height: min(40vh, 320px);
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--raised);
    padding: 4px 0 6px;
  }
  .why {
    padding: 4px 12px 6px;
    color: var(--muted);
    text-wrap: pretty;
  }
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 12px 2px;
  }
  .sec-title {
    font-weight: 600;
    color: var(--text);
  }
  .link {
    color: var(--muted);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .link:hover:not(:disabled) {
    color: var(--text);
  }
  .link:disabled,
  .acts button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-areas: "path acts" "meta acts";
    column-gap: 10px;
    padding: 4px 12px;
    cursor: default;
  }
  .row:hover,
  .row:focus-within {
    background: var(--hover);
  }
  .path {
    grid-area: path;
    font-family: var(--font-mono);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    margin-right: 6px;
    padding: 0 5px;
    border-radius: var(--radius-chip);
    background: var(--active);
    color: var(--muted);
    font-family: var(--font);
    font-size: var(--fs-meta);
  }
  .meta {
    grid-area: meta;
    color: var(--faint);
    font-size: var(--fs-meta);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta code {
    font-family: var(--font-mono);
  }
  .acts {
    grid-area: acts;
    display: flex;
    align-items: center;
    gap: 4px;
    opacity: 0;
  }
  .row:hover .acts,
  .row:focus-within .acts {
    opacity: 1;
  }
  @media (hover: none) {
    .acts {
      opacity: 1;
    }
  }
  .acts button {
    height: 22px;
    padding: 0 8px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    color: var(--text);
    font-size: var(--fs-meta);
    white-space: nowrap;
  }
  .acts button:hover:not(:disabled) {
    background: var(--active);
  }
  @container review (max-width: 520px) {
    .row {
      grid-template-columns: minmax(0, 1fr);
      grid-template-areas: "path" "meta" "acts";
      row-gap: 2px;
    }
    .acts {
      opacity: 1;
      padding: 2px 0;
    }
  }
</style>
