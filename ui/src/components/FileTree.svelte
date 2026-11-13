<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import type { DiffKind } from "../lib/api";
  import { iconFor, type IconTheme } from "../lib/icons";
  import type { Group, Row } from "../lib/rows";
  import { relTime } from "../lib/time";
  import { buildTree, type TreeNode } from "../lib/tree";

  let {
    groups,
    showGroups,
    tab,
    mode,
    selectedKey,
    theme,
    onselect,
  }: {
    groups: Group[];
    showGroups: boolean;
    tab: DiffKind;
    mode: "tree" | "list";
    selectedKey: string | null;
    theme: IconTheme;
    onselect: (row: Row) => void;
  } = $props();

  /** Collapsed folders and groups, keyed per tab so each tab keeps its own shape. */
  const collapsed = new SvelteSet<string>();

  const trees = $derived(
    groups.map((g) => ({
      group: g,
      nodes: mode === "tree" ? buildTree(g.rows) : flat(g.rows),
    })),
  );

  function flat(rows: Row[]): TreeNode<Row>[] {
    return [...rows]
      .sort((a, b) => a.rel.toLowerCase().localeCompare(b.rel.toLowerCase()))
      .map((r) => ({ name: r.rel.split("/").pop() ?? r.rel, rel: r.rel, kind: "file", item: r }));
  }

  function toggle(key: string) {
    if (collapsed.has(key)) collapsed.delete(key);
    else collapsed.add(key);
  }

  function countFiles(n: TreeNode<Row>): number {
    return n.kind === "file" ? 1 : (n.children ?? []).reduce((s, c) => s + countFiles(c), 0);
  }

  function dirOf(rel: string): string {
    const i = rel.lastIndexOf("/");
    return i === -1 ? "" : rel.slice(0, i);
  }

  const pad = (depth: number) => 10 + depth * 14;
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
    {@const key = `${tab}|${scope}|${n.rel}`}
    {@const open = !collapsed.has(key)}
    <button
      class="row folder"
      role="treeitem"
      aria-expanded={open}
      aria-selected="false"
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
    {#if open}
      {#each n.children ?? [] as child (child.rel)}
        {@render node(child, depth + 1, scope)}
      {/each}
    {/if}
  {:else if n.item}
    {@const row = n.item}
    <button
      class="row file"
      class:selected={row.key === selectedKey}
      class:modified={tab === "pending" && !row.newFile}
      class:untracked={tab === "pending" && row.newFile}
      role="treeitem"
      aria-selected={row.key === selectedKey}
      style:padding-left="{pad(depth) + 16}px"
      title={row.path}
      onclick={() => onselect(row)}
    >
      {@render guides(depth)}
      <img class="icon" src={iconFor(n.name, "file", false, theme)} alt="" draggable="false" />
      <span class="name">{n.name}</span>
      {#if mode === "list" && dirOf(n.rel)}<span class="dir">{dirOf(n.rel)}</span>{/if}
      <span class="spacer"></span>
      {#if row.decidedAt}<span class="meta">{relTime(row.decidedAt)}</span>{/if}
      {@render stats(row)}
    </button>
  {/if}
{/snippet}

<div class="tree" role="tree" aria-label="{tab} files">
  {#each trees as { group, nodes } (group.scope.root)}
    {#if showGroups}
      {@const key = `${tab}|group|${group.scope.root}`}
      {@const open = !collapsed.has(key)}
      <button class="row group" role="treeitem" aria-expanded={open} aria-selected="false" title={group.scope.root} onclick={() => toggle(key)}>
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
        {#each nodes as n (n.rel)}
          {@render node(n, 1, group.scope.root)}
        {/each}
      {/if}
    {:else}
      {#each nodes as n (n.rel)}
        {@render node(n, 0, group.scope.root)}
      {/each}
    {/if}
  {/each}
</div>

<style>
  .tree {
    padding: 6px 0 16px;
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
    color: var(--text);
  }
  .row:hover {
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
    padding-left: 8px;
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
