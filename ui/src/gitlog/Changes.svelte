<script lang="ts">
  import { tick } from "svelte";
  import { iconFor, type IconTheme } from "../lib/icons";
  import ContextMenu from "../components/ContextMenu.svelte";
  import { isTypingTarget } from "../lib/keys";
  import { HISTORY_KEY, NAV_KEYS, navigate, type NavKey } from "./log";
  import {
    changeTree,
    countsText,
    foldersOf,
    nodeId,
    statusLetter,
    statusText,
    visibleNodes,
    type ChangedFile,
    type ChangeNode,
  } from "./changes";

  // The selected commit's changed files, as the review's tree draws them: folders first, Material
  // icons, one row per file with its A/M/D/R badge and +/- counts. One tab stop (roving), arrows
  // move, Enter opens the file's diff, Esc hands the keyboard back to the commit list.
  let {
    title,
    repo = null,
    files,
    loading,
    error,
    activePath,
    theme,
    onopenfile,
    onescape,
    onhistory,
  }: {
    title: string;
    /** The repository the files belong to (File History needs it); null: none. */
    repo?: string | null;
    /** null: no commit selected (or its files are not here yet). */
    files: ChangedFile[] | null;
    loading: boolean;
    error: string | null;
    /** The file whose diff is open. */
    activePath: string | null;
    theme: IconTheme;
    onopenfile: (file: ChangedFile) => void;
    onescape: () => void;
    /** "File History": the commits that changed this file. */
    onhistory?: (repo: string, path: string) => void;
  } = $props();

  let collapsed = $state(new Set<string>());
  let focusId = $state<string | null>(null);
  let treeEl: HTMLDivElement | undefined = $state();

  const nodes = $derived(files ? changeTree(files) : []);
  const rows = $derived(visibleNodes(nodes, collapsed));
  const ids = $derived(rows.map((r) => nodeId(r.node)));
  // The one tab stop: the focused row, else the open file, else the first row.
  const tabId = $derived(
    focusId && ids.includes(focusId) ? focusId : activePath && ids.includes(`file:${activePath}`) ? `file:${activePath}` : (ids[0] ?? null),
  );

  // A different commit: its folders start open, the old focus goes.
  let shownFor: ChangedFile[] | null = null;
  $effect(() => {
    if (files === shownFor) return;
    shownFor = files;
    collapsed = new Set();
    focusId = null;
  });
  // The open file's folders are never hidden from it.
  $effect(() => {
    const p = activePath;
    if (!p) return;
    const hide = foldersOf(p);
    // A compacted chain (`src/api`) is one node keyed by its deepest folder; opening it covers its parents.
    if (hide.some((f) => collapsed.has(f))) {
      const next = new Set(collapsed);
      for (const f of hide) next.delete(f);
      collapsed = next;
    }
  });

  const pad = (depth: number) => 12 + depth * 14;

  function rowEl(id: string): HTMLElement | null {
    return treeEl?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`) ?? null;
  }
  async function focusRow(id: string | null) {
    if (!id) return;
    focusId = id;
    await tick();
    rowEl(id)?.focus();
  }

  /** Puts the keyboard in the tree: on the open file, else the first row. */
  export function focus() {
    void focusRow(activePath && ids.includes(`file:${activePath}`) ? `file:${activePath}` : (ids[0] ?? null));
  }

  function toggle(n: ChangeNode, open?: boolean) {
    const next = new Set(collapsed);
    const want = open ?? next.has(n.rel);
    if (want) next.delete(n.rel);
    else next.add(n.rel);
    collapsed = next;
  }

  // The context menu of a file row, and the `h` key: File History.
  const MENU = [{ id: "history", label: "File History", hint: HISTORY_KEY.toUpperCase() }];
  let menu = $state<{ x: number; y: number; path: string; id: string } | null>(null);
  function history(path: string) {
    if (repo && onhistory) onhistory(repo, path);
  }
  function openMenu(e: MouseEvent, path: string, id: string) {
    if (!repo || !onhistory) return;
    e.preventDefault();
    focusId = id;
    menu = { x: e.clientX, y: e.clientY, path, id };
  }

  function activate(n: ChangeNode) {
    if (n.kind === "folder") toggle(n);
    else if (n.item) onopenfile(n.item.file);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    const i = Math.max(0, ids.indexOf(tabId ?? ""));
    const cur = rows[i];
    if (NAV_KEYS.includes(e.key)) {
      e.preventDefault();
      const to = navigate(rows.length, i, e.key as NavKey, 8);
      if (to >= 0) void focusRow(ids[to]);
    } else if (e.key === "ArrowRight" && cur?.node.kind === "folder") {
      e.preventDefault();
      if (collapsed.has(cur.node.rel)) toggle(cur.node, true);
      else if (rows[i + 1] && rows[i + 1].depth > cur.depth) void focusRow(ids[i + 1]);
    } else if (e.key === "ArrowLeft" && cur) {
      e.preventDefault();
      if (cur.node.kind === "folder" && !collapsed.has(cur.node.rel)) toggle(cur.node, false);
      else {
        // Up to the enclosing folder.
        for (let j = i - 1; j >= 0; j--) {
          if (rows[j].depth < cur.depth) {
            void focusRow(ids[j]);
            break;
          }
        }
      }
    } else if (e.key === "Enter" || e.key === " ") {
      if (!cur) return;
      e.preventDefault();
      activate(cur.node);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onescape();
    } else if (e.key.toLowerCase() === HISTORY_KEY && !e.shiftKey && !isTypingTarget(e.target as Element) && cur?.node.kind === "file" && cur.node.item) {
      e.preventDefault();
      history(cur.node.item.file.path);
    } else if ((e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) && cur?.node.kind === "file" && cur.node.item && repo && onhistory) {
      e.preventDefault();
      const r = rowEl(nodeId(cur.node))?.getBoundingClientRect();
      menu = { x: r ? r.left + 24 : 0, y: r ? r.bottom : 0, path: cur.node.item.file.path, id: nodeId(cur.node) };
    }
  }
</script>

<section class="changes" aria-label="Changes">
  <p class="pane-title" title={title}>{title}</p>
  {#if error}
    <p class="state-body pad" role="alert">Couldn't load the changes: {error}</p>
  {:else if files === null}
    {#if loading}
      <div class="skeletons" aria-busy="true" aria-label="Loading the changes">
        {#each [70, 52, 62] as w, i (i)}<div class="skeleton" style:width="{w}%"></div>{/each}
      </div>
    {:else}
      <p class="state-body pad">Select a commit to see the files it changed.</p>
    {/if}
  {:else if files.length === 0}
    <p class="state-body pad">This commit changes no files.</p>
  {:else}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="tree" role="tree" aria-label="Changed files" tabindex="-1" bind:this={treeEl} {onkeydown} class:stale={loading}>
      {#each rows as r (nodeId(r.node))}
        {@const n = r.node}
        {@const id = nodeId(n)}
        {#if n.kind === "folder"}
          {@const open = !collapsed.has(n.rel)}
          <button
            class="row"
            role="treeitem"
            aria-selected="false"
            aria-level={r.depth + 1}
            aria-expanded={open}
            data-id={id}
            tabindex={tabId === id ? 0 : -1}
            style:padding-left="{pad(r.depth)}px"
            title={n.rel}
            onclick={() => {
              focusId = id;
              toggle(n);
            }}
          >
            <svg class="chev" class:open viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
              <path d="M6 4.5 9.5 8 6 11.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
            <img class="icon" src={iconFor(n.name, "folder", open, theme)} alt="" draggable="false" />
            <span class="name">{n.name}</span>
          </button>
        {:else if n.item}
          {@const f = n.item.file}
          <button
            class="row file"
            class:selected={f.path === activePath}
            role="treeitem"
            aria-selected={f.path === activePath}
            aria-level={r.depth + 1}
            data-id={id}
            tabindex={tabId === id ? 0 : -1}
            style:padding-left="{pad(r.depth)}px"
            title="{f.path} · {statusText(f)}"
            aria-label="{n.name}, {statusText(f)}, {countsText(f)}"
            onclick={() => {
              focusId = id;
              onopenfile(f);
            }}
            oncontextmenu={(e) => openMenu(e, f.path, id)}
          >
            <span class="chev-gap" aria-hidden="true"></span>
            <img class="icon" src={iconFor(n.name, "file", false, theme)} alt="" draggable="false" />
            <span class="name">{n.name}</span>
            {#if f.from}<span class="from" aria-hidden="true">← {f.from.split("/").pop()}</span>{/if}
            <span class="spacer"></span>
            <span class="counts num" aria-hidden="true">
              {#if f.added === null || f.removed === null}
                <span class="faint">binary</span>
              {:else}
                <span class="add">+{f.added}</span><span class="del">−{f.removed}</span>
              {/if}
            </span>
            <span class="badge s-{f.status}" aria-hidden="true">{statusLetter(f)}</span>
          </button>
        {/if}
      {/each}
    </div>
  {/if}
</section>

{#if menu}
  {@const m = menu}
  <ContextMenu
    x={m.x}
    y={m.y}
    items={MENU}
    onpick={() => {
      const path = m.path;
      menu = null;
      history(path);
    }}
    onclose={() => {
      const id = m.id;
      menu = null;
      void tick().then(() => rowEl(id)?.focus());
    }}
  />
{/if}

<style>
  .changes {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: auto;
  }
  .pane-title {
    flex: none;
    padding: 12px var(--pad) 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--muted);
  }
  .pad {
    padding: 4px var(--pad);
  }
  .state-body {
    max-width: 420px;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .skeletons {
    padding: 6px var(--pad);
  }
  .skeleton {
    height: 12px;
    margin: 8px 0;
    border-radius: 4px;
    background: var(--hover);
  }
  .tree {
    padding: 4px 0 16px;
  }
  .tree:focus {
    outline: none;
  }
  .tree.stale {
    opacity: 0.6;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: var(--row-h);
    padding-right: 10px;
    overflow: hidden;
    white-space: nowrap;
    text-align: left;
    color: var(--text);
  }
  .row:hover {
    background: var(--hover);
  }
  .row.selected {
    background: var(--active);
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
  .chev-gap {
    flex: none;
    width: 14px;
  }
  .icon {
    flex: none;
    width: 16px;
    height: 16px;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .from {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--fs-meta);
    color: var(--faint);
  }
  .spacer {
    flex: 1;
    min-width: 4px;
  }
  .counts {
    display: inline-flex;
    flex: none;
    gap: 6px;
    font-size: var(--fs-meta);
  }
  .add {
    color: var(--positive);
  }
  .del {
    color: var(--negative);
  }
  .faint {
    color: var(--faint);
  }
  .badge {
    flex: none;
    width: 16px;
    text-align: center;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--pending);
  }
  .badge.s-added,
  .badge.s-copied {
    color: var(--untracked);
  }
  .badge.s-deleted {
    color: var(--negative);
  }
  .badge.s-renamed,
  .badge.s-typeChanged {
    color: var(--muted);
  }
  .row:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
</style>
