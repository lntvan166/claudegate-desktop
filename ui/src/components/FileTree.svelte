<script lang="ts">
  import { tick } from "svelte";
  import type { SvelteSet } from "svelte/reactivity";
  import type { DiffKind } from "../lib/api";
  import { folderPrefix, rejectPrompt, type DecideAction, type DecideTarget, type Prompt } from "../lib/decide";
  import { iconFor, type IconTheme } from "../lib/icons";
  import { WAITING_REFUSAL } from "../lib/keys";
  import { nodeKey, type Group, type Row } from "../lib/rows";
  import { relTime } from "../lib/time";
  import type { TreeNode } from "../lib/tree";
  import {
    fileId,
    folderCollapseKey,
    folderId,
    groupCollapseKey,
    groupId,
    nodesFor,
    visibleItems,
  } from "../lib/visible";

  let {
    groups,
    showGroups,
    tab,
    mode,
    selectedKey,
    theme,
    collapsed,
    editorName,
    onselect,
    onopen,
    canDecide = false,
    busy = false,
    ondecide = () => {},
    onnotice = () => {},
  }: {
    groups: Group[];
    showGroups: boolean;
    tab: DiffKind;
    mode: "tree" | "list";
    selectedKey: string | null;
    theme: IconTheme;
    /** Collapsed folders and groups, keyed per tab. Owned by App so keyboard order can see it. */
    collapsed: SvelteSet<string>;
    editorName: string;
    /** `pointer` for a click or Enter on the row, `key` when arrow keys move the selection. */
    onselect: (row: Row, via?: "pointer" | "key") => void;
    onopen: (row: Row) => void;
    /** Show the accept/reject (revert, reapply) actions; false in read-only mode. */
    canDecide?: boolean;
    /** A decision is in flight: every action is disabled. */
    busy?: boolean;
    /** `row` is the file decided, or null for a folder. */
    ondecide?: (action: DecideAction, target: DecideTarget, row: Row | null) => void;
    /** Says why an action is unavailable (through the app's live region). */
    onnotice?: (message: string) => void;
  } = $props();

  let treeEl: HTMLDivElement | undefined = $state();

  const trees = $derived(groups.map((g) => ({ group: g, nodes: nodesFor(g.rows, mode) })));
  const items = $derived(visibleItems(groups, { mode, showGroups, tab, collapsed }));

  /** The row that was last focused inside the tree; the tree's single tab stop follows it. */
  let focusId = $state<string | null>(null);
  const tabId = $derived.by(() => {
    if (focusId && items.some((i) => i.id === focusId)) return focusId;
    const sel = selectedKey ? items.find((i) => i.row?.key === selectedKey) : undefined;
    return sel?.id ?? items[0]?.id ?? null;
  });

  function toggle(key: string) {
    if (collapsed.has(key)) collapsed.delete(key);
    else collapsed.add(key);
  }

  function countFiles(n: TreeNode<Row>): number {
    return n.kind === "file" ? 1 : (n.children ?? []).reduce((s, c) => s + countFiles(c), 0);
  }

  /** The rows a folder action decides: every file under `n` whose write has landed. */
  function actionableRows(n: TreeNode<Row>): Row[] {
    if (n.kind === "file") return n.item && !n.item.waiting ? [n.item] : [];
    return (n.children ?? []).flatMap(actionableRows);
  }

  /** A folder reject waiting for its answer, shown inline under the folder's row. */
  let folderAsk = $state<{ id: string; scope: string; prefix: string; keys: string[]; prompt: Prompt } | null>(null);
  let askEl: HTMLDivElement | undefined = $state();

  async function askFolderReject(id: string, scope: string, n: TreeNode<Row>) {
    // The rows counted in the question are the rows sent: nothing that lands later is rejected.
    const rows = actionableRows(n);
    const keys = rows.map((r) => r.path);
    const prompt = rejectPrompt(rows.map((r) => ({ path: r.path, rejectOutcome: r.rejectOutcome ?? "restore" })), n.rel);
    folderAsk = { id, scope, prefix: folderPrefix(scope, n.rel), keys, prompt };
    await tick();
    // Cancel is the safe default.
    askEl?.querySelector<HTMLButtonElement>("[data-cancel]")?.focus();
  }

  function cancelFolderAsk() {
    const id = folderAsk?.id;
    folderAsk = null;
    if (id) void focusItem(id);
  }

  function confirmFolderAsk() {
    const a = folderAsk;
    folderAsk = null;
    if (a) ondecide("reject", { kind: "folder", scope: a.scope, prefix: a.prefix, keys: a.keys }, null);
  }

  /** Focus stays inside the question until it is answered; Esc answers no. */
  function trapAsk(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancelFolderAsk();
      return;
    }
    if (e.key !== "Tab" || !askEl) return;
    const buttons = [...askEl.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    if (buttons.length === 0) return;
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.shiftKey ? (i <= 0 ? buttons.length - 1 : i - 1) : i === buttons.length - 1 ? 0 : i + 1;
    buttons[next].focus();
    e.preventDefault();
  }

  // The folder left the list, or decisions were turned off: the question no longer applies.
  $effect(() => {
    if (folderAsk && (!canDecide || tab !== "pending" || !items.some((i) => i.id === folderAsk!.id))) folderAsk = null;
  });

  function dirOf(rel: string): string {
    const i = rel.lastIndexOf("/");
    return i === -1 ? "" : rel.slice(0, i);
  }

  /** The sheet's inner padding (12px) holds at depth 0, in every pane width and rail state. */
  const pad = (depth: number) => 12 + depth * 14;

  function rowEl(id: string): HTMLElement | null {
    return treeEl?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`) ?? null;
  }

  async function focusItem(id: string) {
    focusId = id;
    await tick();
    rowEl(id)?.focus();
  }

  /** Arrow keys (and j/k) move through visible rows; selection follows focus onto files. */
  function onkeydown(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const id = (e.target as HTMLElement).closest<HTMLElement>("[data-id]")?.dataset.id;
    const i = items.findIndex((x) => x.id === id);
    if (i === -1) return;
    const cur = items[i];
    let to: number | null = null;
    switch (e.key) {
      case "ArrowDown":
      case "j":
        to = Math.min(items.length - 1, i + 1);
        break;
      case "ArrowUp":
      case "k":
        to = Math.max(0, i - 1);
        break;
      case "Home":
        to = 0;
        break;
      case "End":
        to = items.length - 1;
        break;
      case "ArrowRight":
        if (cur.collapseKey && !cur.open) collapsed.delete(cur.collapseKey);
        else if (cur.collapseKey && items[i + 1]?.parent === cur.id) to = i + 1;
        break;
      case "ArrowLeft":
        if (cur.collapseKey && cur.open) collapsed.add(cur.collapseKey);
        else if (cur.parent) to = items.findIndex((x) => x.id === cur.parent);
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (to === null || to < 0) return;
    const target = items[to];
    void focusItem(target.id);
    if (target.row && target.row.key !== selectedKey) onselect(target.row, "key");
  }

  function onfocusin(e: FocusEvent) {
    const id = (e.target as HTMLElement).closest<HTMLElement>("[data-id]")?.dataset.id;
    if (id) focusId = id;
  }

  // Keep the selected row in view, and carry focus with it when the tree has focus.
  $effect(() => {
    const key = selectedKey;
    if (!key) return;
    void tick().then(() => {
      const el = rowEl(`f:${key}`);
      if (!el) return;
      el.scrollIntoView({ block: "nearest" });
      if (treeEl?.contains(document.activeElement) && document.activeElement !== el) {
        focusId = `f:${key}`;
        el.focus();
      }
    });
  });
</script>

{#snippet guides(depth: number)}
  {#each { length: depth } as _, i (i)}
    <span class="guide" style:left="{pad(i) + 7}px" aria-hidden="true"></span>
  {/each}
{/snippet}

{#snippet chevron(open: boolean)}
  <svg class="chev" class:open viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <path d="M6 4.5 9.5 8 6 11.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/snippet}

{#snippet check()}
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M3.5 8.5 6.5 11.5 12.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/snippet}

{#snippet cross()}
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
  </svg>
{/snippet}

{#snippet back()}
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M5.5 3.5 2.5 6.5l3 3M2.5 6.5h7a3.5 3.5 0 0 1 0 7H7" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/snippet}

{#snippet stats(row: Row)}
  {#if row.added > 0 || row.removed > 0}
    <span class="stats num">
      {#if row.added > 0}<span class="add">+{row.added}</span>{/if}
      {#if row.removed > 0}<span class="del">−{row.removed}</span>{/if}
    </span>
  {/if}
{/snippet}

{#snippet node(n: TreeNode<Row>, depth: number, scope: string)}
  {#if n.kind === "folder"}
    {@const key = folderCollapseKey(tab, scope, n.rel)}
    {@const open = !collapsed.has(key)}
    {@const id = folderId(scope, n.rel)}
    {@const folderActs = canDecide && tab === "pending"}
    <div class={folderActs ? "item" : "plain"} role="none">
    <button
      class="row folder"
      role="treeitem"
      aria-expanded={open}
      aria-selected="false"
      data-id={id}
      tabindex={tabId === id ? 0 : -1}
      style:padding-left="{pad(depth)}px"
      title={n.rel}
      onclick={() => toggle(key)}
    >
      {@render guides(depth)}
      {@render chevron(open)}
      <img class="icon" src={iconFor(n.name, "folder", open, theme)} alt="" draggable="false" />
      <span class="name">{n.name}</span>
      {#if !open}<span class="meta num">{countFiles(n)}</span>{/if}
    </button>
    {#if folderActs}
      {@const prefix = folderPrefix(scope, n.rel)}
      <div class="actions">
        <button
          class="act accept"
          tabindex="-1"
          disabled={busy || actionableRows(n).length === 0}
          aria-label="Accept folder {n.rel}"
          title="Accept folder"
          onclick={() => {
            const keys = actionableRows(n).map((r) => r.path);
            ondecide("accept", { kind: "folder", scope, prefix, keys }, null);
          }}
        >{@render check()}</button>
        <button
          class="act reject"
          tabindex="-1"
          disabled={busy || actionableRows(n).length === 0}
          aria-label="Reject folder {n.rel}"
          title="Reject folder"
          onclick={() => void askFolderReject(id, scope, n)}
        >{@render cross()}</button>
      </div>
    {/if}
    </div>
    {#if folderAsk?.id === id}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="ask"
        role="group"
        aria-label="Confirm"
        data-confirm
        style:padding-left="{pad(depth) + 16}px"
        bind:this={askEl}
        onkeydown={trapAsk}
      >
        <p class="question">{folderAsk.prompt.text}</p>
        <div class="ask-buttons">
          <button class="btn danger" disabled={busy} onclick={confirmFolderAsk}>{folderAsk.prompt.go}</button>
          <button class="btn" data-cancel onclick={cancelFolderAsk}>Cancel</button>
        </div>
      </div>
    {/if}
    {#if open}
      <div role="group">
        {#each n.children ?? [] as child (nodeKey(child))}
          {@render node(child, depth + 1, scope)}
        {/each}
      </div>
    {/if}
  {:else if n.item}
    {@const row = n.item}
    {@const id = fileId(row)}
    <div class="item" class:selected={row.key === selectedKey} role="none">
      <button
        class="row file"
        class:selected={row.key === selectedKey}
        class:modified={tab === "pending" && !row.newFile}
        class:untracked={tab === "pending" && row.newFile}
        role="treeitem"
        aria-selected={row.key === selectedKey}
        data-id={id}
        tabindex={tabId === id ? 0 : -1}
        style:padding-left="{pad(depth) + 16}px"
        title={row.path}
        onclick={() => onselect(row, "pointer")}
      >
        {@render guides(depth)}
        <img class="icon" src={iconFor(n.name, "file", false, theme)} alt="" draggable="false" />
        <span class="name">{n.name}</span>
        {#if mode === "list" && dirOf(n.rel)}<span class="dir">{dirOf(n.rel)}</span>{/if}
        <span class="spacer"></span>
        {#if row.decidedAt}<span class="meta">{relTime(row.decidedAt)}</span>{/if}
        {#if row.waiting}<span class="meta waiting" title="Captured before Claude wrote it; the write hasn't landed yet">waiting for write…</span>{/if}
        {@render stats(row)}
      </button>
      <!-- Mouse shortcuts; the keyboard equivalents are A, R and O on the selected row. -->
      <div class="actions">
        {#if row.waiting}
          <!-- The row's own label sits under these buttons; this copy keeps it readable. -->
          <span class="meta waiting" title="Captured before Claude wrote it; the write hasn't landed yet">waiting for write…</span>
        {/if}
        {#if canDecide && tab === "pending" && row.waiting}
          <!-- aria-disabled, not disabled: the tooltip still shows and a click says why. -->
          <button
            class="act accept"
            tabindex="-1"
            aria-disabled="true"
            aria-label="Accept {n.name}: {WAITING_REFUSAL}"
            title={WAITING_REFUSAL}
            onclick={() => onnotice(WAITING_REFUSAL)}
          >{@render check()}</button>
          <button
            class="act reject"
            tabindex="-1"
            aria-disabled="true"
            aria-label="Reject {n.name}: {WAITING_REFUSAL}"
            title={WAITING_REFUSAL}
            onclick={() => onnotice(WAITING_REFUSAL)}
          >{@render cross()}</button>
        {:else if canDecide && tab === "pending"}
          <button
            class="act accept"
            tabindex="-1"
            disabled={busy}
            aria-label="Accept {n.name}"
            title="Accept (A)"
            onclick={() => ondecide("accept", { kind: "file", scope: row.scope, key: row.path }, row)}
          >{@render check()}</button>
          <button
            class="act reject"
            tabindex="-1"
            disabled={busy}
            aria-label="Reject {n.name}"
            title="Reject (R)"
            onclick={() => ondecide("reject", { kind: "file", scope: row.scope, key: row.path }, row)}
          >{@render cross()}</button>
        {:else if canDecide && tab === "accepted"}
          <button
            class="act"
            tabindex="-1"
            disabled={busy}
            aria-label="Revert {n.name}"
            title="Revert: back to pending"
            onclick={() => ondecide("revert", { kind: "file", scope: row.scope, key: row.key }, row)}
          >{@render back()}</button>
        {:else if canDecide && tab === "rejected"}
          <button
            class="act"
            tabindex="-1"
            disabled={busy}
            aria-label="Reapply {n.name}"
            title="Reapply Claude's version"
            onclick={() => ondecide("reapply", { kind: "file", scope: row.scope, key: row.path }, row)}
          >{@render back()}</button>
        {/if}
      {#if row.key !== selectedKey}
      <!-- The selected row's file is open in the diff, whose header has Open in…; no second ↗. -->
      <button class="act open" tabindex="-1" aria-label="Open {n.name} in {editorName}" title="Open in {editorName}" onclick={() => onopen(row)}>
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
      </button>
      {/if}
      </div>
    </div>
  {/if}
{/snippet}

<div class="tree" role="tree" aria-label="{tab} files" tabindex="-1" bind:this={treeEl} {onkeydown} {onfocusin}>
  {#each trees as { group, nodes } (group.scope.root)}
    {#if showGroups}
      {@const key = groupCollapseKey(tab, group.scope.root)}
      {@const open = !collapsed.has(key)}
      {@const id = groupId(group.scope.root)}
      <button
        class="row group"
        role="treeitem"
        aria-expanded={open}
        aria-selected="false"
        data-id={id}
        tabindex={tabId === id ? 0 : -1}
        title={group.scope.root}
        onclick={() => toggle(key)}
      >
        {@render chevron(open)}
        {#if group.scope.isWorktree}
          <svg class="glyph" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <circle cx="4.5" cy="3.5" r="1.6" fill="none" stroke="currentColor" stroke-width="1.3" />
            <circle cx="4.5" cy="12.5" r="1.6" fill="none" stroke="currentColor" stroke-width="1.3" />
            <circle cx="11.5" cy="5.5" r="1.6" fill="none" stroke="currentColor" stroke-width="1.3" />
            <path d="M4.5 5.1v5.8M11.5 7.1c0 2.4-2.2 3-5.4 3.9" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
          </svg>
        {:else}
          <svg class="glyph" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <path d="M3 2.5h8.5a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H3zM3 2.5v11M5.5 5.5h4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round" />
          </svg>
        {/if}
        <span class="name">{group.label}</span>
        {#if group.scope.isWorktree}<span class="tag">worktree</span>{/if}
        <span class="spacer"></span>
        <span class="meta num">{group.rows.length}</span>
      </button>
      {#if open}
        <div role="group">
          {#each nodes as n (nodeKey(n))}
            {@render node(n, 1, group.scope.root)}
          {/each}
        </div>
      {/if}
    {:else}
      {#each nodes as n (nodeKey(n))}
        {@render node(n, 0, group.scope.root)}
      {/each}
    {/if}
  {/each}
</div>

<style>
  /* A folder reject's question, inline under its row (the ActionBar's confirm, row-sized). */
  .ask {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 10px;
    padding: 8px 12px 8px 0;
    background: var(--raised);
    border-block: 1px solid var(--rule);
  }
  .question {
    flex: 1 1 180px;
    min-width: 0;
    font-size: var(--fs-small);
    line-height: 1.45;
    color: var(--text);
  }
  .ask-buttons {
    display: flex;
    gap: 6px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    white-space: nowrap;
    color: var(--text);
    background: var(--surface);
    transition: background 120ms var(--ease);
  }
  .btn:hover:not(:disabled) {
    background: var(--hover);
  }
  .btn.danger {
    border-color: transparent;
    color: #fff;
    background: var(--danger);
  }
  .btn.danger:hover:not(:disabled) {
    background: var(--danger-hover);
  }
  .btn:disabled {
    opacity: 0.6;
  }

  .tree {
    padding: 6px 0 16px;
  }
  .tree:focus {
    outline: none;
  }

  .item {
    position: relative;
  }
  /* Row actions: on hover or focus, and always on the selected row (the extension's inline actions). */
  .actions {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 8px 0 6px;
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms var(--ease);
  }
  .item:hover .actions,
  .item:focus-within .actions,
  .item.selected .actions {
    opacity: 1;
    pointer-events: auto;
  }
  /* Opaque on the row's own tone, so a long name runs under the buttons, not through them. */
  .item:hover .actions,
  .item:focus-within .actions {
    background: var(--hover);
  }
  .item.selected .actions {
    background: var(--active);
  }
  .act {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: var(--radius-chip);
    color: var(--muted);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .act:hover:not(:disabled):not([aria-disabled="true"]) {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 9%, transparent);
  }
  .act.accept:hover:not(:disabled):not([aria-disabled="true"]) {
    color: var(--positive);
  }
  .act.reject:hover:not(:disabled):not([aria-disabled="true"]) {
    color: var(--negative);
  }
  .act:disabled,
  .act[aria-disabled="true"] {
    opacity: 0.45;
  }
  .act[aria-disabled="true"]:hover {
    color: var(--muted);
    background: none;
  }
  .actions .meta.waiting {
    margin-right: 6px;
    white-space: nowrap;
  }
  .item:hover > .row .stats,
  .item:hover > .row .meta,
  .item:focus-within > .row .stats,
  .item:focus-within > .row .meta,
  .item.selected > .row .stats,
  .item.selected > .row .meta {
    visibility: hidden;
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: var(--row-h);
    padding-right: 14px;
    white-space: nowrap;
    /* A row never paints past its own width, so the list never gains a sideways scroll. */
    overflow: hidden;
    color: var(--text);
  }
  .row:hover,
  .item:hover > .row {
    background: var(--hover);
  }
  .row.selected {
    background: var(--active);
  }

  .guide {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--guide);
  }

  .chev {
    flex: none;
    margin-right: -2px;
    color: var(--faint);
    transition: transform 120ms var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }

  .icon {
    flex: none;
    width: 16px;
    height: 16px;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .file.modified .name {
    color: var(--pending);
  }
  .file.untracked .name {
    color: var(--untracked);
  }

  .dir {
    font-size: var(--fs-small);
    color: var(--faint);
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    flex-shrink: 1;
  }

  .spacer {
    flex: 1;
    min-width: 8px;
  }

  .meta {
    flex: none;
    font-size: var(--fs-meta);
    color: var(--faint);
  }
  .meta.waiting {
    font-style: italic;
  }

  .stats {
    flex: none;
    display: inline-flex;
    gap: 6px;
    font-size: var(--fs-meta);
  }
  .add {
    color: var(--positive);
  }
  .del {
    color: var(--negative);
  }

  .group {
    height: 30px;
    padding-left: 12px;
    margin-top: 6px;
    font-weight: 500;
    color: var(--muted);
  }
  .group:first-child {
    margin-top: 0;
  }
  .group .glyph {
    flex: none;
    color: var(--faint);
  }
  .group .name {
    color: var(--text);
  }
  .tag {
    flex: none;
    padding: 0 6px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    line-height: 18px;
    color: var(--muted);
    background: var(--raised);
  }
</style>
