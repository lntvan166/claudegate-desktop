<script lang="ts">
  import { tick } from "svelte";
  import { iconFor, type IconTheme } from "../lib/icons";
  import { NAV_KEYS, navigate, shortSha, type NavKey } from "./log";
  import { colorVar } from "./repos";
  import { foldersOf, statusLetter, statusText, visibleNodes, type ChangedFile } from "./changes";
  import { combinedTree, noNetChangeText, shaList, warnText, type CombinedFile, type CombinedNode, type CombinedRepo } from "./combined";

  // Combined Changes: the selected commits' files, each once, one folder tree per repository under
  // a repo root row. The same tree the single commit's Changes draws (folders first, Material
  // icons, one tab stop, arrows, Enter opens, Esc hands the keyboard back to the list), with the
  // net status letter, the commits that touched each file and a warning where it falls back.
  let {
    title,
    repos,
    repoName,
    repoColor,
    loading,
    error,
    active,
    theme,
    onopenfile,
    onclear,
    onescape,
    loadingText = "Reading the selected commits…",
  }: {
    title: string;
    repos: CombinedRepo[];
    repoName: (id: string) => string;
    repoColor: (id: string) => number;
    loading: boolean;
    error: string | null;
    /** The file whose diff is open. */
    active: { repo: string; path: string } | null;
    theme: IconTheme;
    onopenfile: (repo: string, f: CombinedFile) => void;
    onclear: () => void;
    onescape: () => void;
    loadingText?: string;
    /** Wired in Task 8 (file history); a no-op until then. */
    onhistory?: (repo: string, path: string) => void;
  } = $props();

  type Row =
    | { kind: "repo"; id: string; repo: CombinedRepo }
    | { kind: "folder"; id: string; repo: CombinedRepo; node: CombinedNode; depth: number }
    | { kind: "file"; id: string; repo: CombinedRepo; node: CombinedNode; file: CombinedFile; depth: number }
    | { kind: "note"; id: string; text: string }
    | { kind: "error"; id: string; text: string };

  // Collapsed folders and repos, keyed `repo \0 rel` (a repo root: `repo \0`).
  let collapsed = $state(new Set<string>());
  let focusId = $state<string | null>(null);
  let treeEl: HTMLDivElement | undefined = $state();

  const key = (repo: string, rel: string) => `${repo}\u0001${rel}`;
  const rows = $derived.by<Row[]>(() => {
    const out: Row[] = [];
    for (const r of repos) {
      out.push({ kind: "repo", id: `repo:${r.repo}`, repo: r });
      if (collapsed.has(key(r.repo, ""))) continue;
      if (r.error) {
        out.push({ kind: "error", id: `error:${r.repo}`, text: `Couldn't read ${repoName(r.repo)}: ${r.error}` });
        continue;
      }
      const mine = new Set([...collapsed].filter((k) => k.startsWith(`${r.repo}\u0001`)).map((k) => k.slice(r.repo.length + 1)));
      for (const v of visibleNodes(combinedTree(r.files), mine)) {
        const node = v.node;
        const id = `${node.kind}:${key(r.repo, node.rel)}`;
        if (node.kind === "folder") out.push({ kind: "folder", id, repo: r, node, depth: v.depth + 1 });
        else if (node.item) out.push({ kind: "file", id, repo: r, node, file: node.item.file, depth: v.depth + 1 });
      }
      const note = noNetChangeText(r.noNetChange);
      if (note) out.push({ kind: "note", id: `note:${r.repo}`, text: note });
    }
    return out;
  });
  const focusable = $derived(rows.filter((r) => r.kind !== "note" && r.kind !== "error"));
  const ids = $derived(focusable.map((r) => r.id));
  const activeId = $derived(active ? `file:${key(active.repo, active.path)}` : null);
  // The one tab stop: the focused row, else the open file, else the first row.
  const tabId = $derived(focusId && ids.includes(focusId) ? focusId : activeId && ids.includes(activeId) ? activeId : (ids[0] ?? null));

  // A new selection starts with everything open and no remembered focus.
  let shownFor: CombinedRepo[] | null = null;
  $effect(() => {
    const sig = repos.map((r) => r.repo).join("\n");
    if (shownFor && shownFor.map((r) => r.repo).join("\n") === sig) {
      shownFor = repos;
      return;
    }
    shownFor = repos;
    collapsed = new Set();
    focusId = null;
  });
  // The open file's folders are never hidden from it.
  $effect(() => {
    const a = active;
    if (!a) return;
    const hide = [key(a.repo, ""), ...foldersOf(a.path).map((f) => key(a.repo, f))];
    if (hide.some((k) => collapsed.has(k))) {
      const next = new Set(collapsed);
      for (const k of hide) next.delete(k);
      collapsed = next;
    }
  });

  const pad = (depth: number) => 12 + depth * 14;
  const subjectLine = (repo: CombinedRepo) => `${repoName(repo.repo)}: ${repo.commits.length} ${repo.commits.length === 1 ? "commit" : "commits"}`;

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
    void focusRow(activeId && ids.includes(activeId) ? activeId : (ids[0] ?? null));
  }

  function toggle(k: string, open?: boolean) {
    const next = new Set(collapsed);
    const want = open ?? next.has(k);
    if (want) next.delete(k);
    else next.add(k);
    collapsed = next;
  }
  const toggleKey = (r: Row) => (r.kind === "repo" ? key(r.repo.repo, "") : r.kind === "folder" ? key(r.repo.repo, r.node.rel) : "");

  function activate(r: Row) {
    if (r.kind === "repo" || r.kind === "folder") toggle(toggleKey(r));
    else if (r.kind === "file") onopenfile(r.repo.repo, r.file);
  }
  const depthOf = (r: Row) => (r.kind === "repo" ? 0 : r.kind === "folder" || r.kind === "file" ? r.depth : 1);

  function onkeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    const i = Math.max(0, ids.indexOf(tabId ?? ""));
    const cur = focusable[i];
    if (NAV_KEYS.includes(e.key)) {
      e.preventDefault();
      const to = navigate(focusable.length, i, e.key as NavKey, 8);
      if (to >= 0) void focusRow(ids[to]);
    } else if (e.key === "ArrowRight" && cur && (cur.kind === "folder" || cur.kind === "repo")) {
      e.preventDefault();
      const k = toggleKey(cur);
      if (collapsed.has(k)) toggle(k, true);
      else if (focusable[i + 1] && depthOf(focusable[i + 1]) > depthOf(cur)) void focusRow(ids[i + 1]);
    } else if (e.key === "ArrowLeft" && cur) {
      e.preventDefault();
      if ((cur.kind === "folder" || cur.kind === "repo") && !collapsed.has(toggleKey(cur))) toggle(toggleKey(cur), false);
      else {
        for (let j = i - 1; j >= 0; j--) {
          if (depthOf(focusable[j]) < depthOf(cur)) {
            void focusRow(ids[j]);
            break;
          }
        }
      }
    } else if (e.key === "Enter" || e.key === " ") {
      if (!cur) return;
      e.preventDefault();
      activate(cur);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onescape();
    }
  }

  const asChanged = (f: CombinedFile): ChangedFile => ({ path: f.path, status: f.status, from: f.from, added: null, removed: null });
</script>

<section class="changes" aria-label="Changes" aria-busy={loading}>
  <div class="head">
    <p class="pane-title" {title}>{title}</p>
    <button type="button" class="clear" title="Clear selection: back to the focused commit" onclick={onclear}>Clear selection</button>
  </div>
  {#if error}
    <p class="state-body pad" role="alert">Couldn't load the changes: {error}</p>
  {:else if repos.length === 0}
    {#if loading}
      <p class="state-body pad" role="status">{loadingText}</p>
      <div class="skeletons" aria-busy="true" aria-label="Loading the changes">
        {#each [70, 52, 62] as w, i (i)}<div class="skeleton" style:width="{w}%"></div>{/each}
      </div>
    {:else}
      <p class="state-body pad">Select commits to see the files they changed.</p>
    {/if}
  {:else}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="tree" role="tree" aria-label="Changed files" tabindex="-1" bind:this={treeEl} {onkeydown} class:stale={loading}>
      {#each rows as r (r.id)}
        {#if r.kind === "repo"}
          {@const open = !collapsed.has(key(r.repo.repo, ""))}
          <button
            class="row root"
            role="treeitem"
            aria-selected="false"
            aria-level={1}
            aria-expanded={open}
            data-id={r.id}
            tabindex={tabId === r.id ? 0 : -1}
            style:--c={colorVar(repoColor(r.repo.repo))}
            title={subjectLine(r.repo)}
            onclick={() => {
              focusId = r.id;
              toggle(key(r.repo.repo, ""));
            }}
          >
            <svg class="chev" class:open viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
              <path d="M6 4.5 9.5 8 6 11.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
            <span class="chip"><span class="dot"></span><span class="cname">{repoName(r.repo.repo)}</span></span>
            <span class="spacer"></span>
            <span class="meta num">{r.repo.commits.length} {r.repo.commits.length === 1 ? "commit" : "commits"}</span>
          </button>
        {:else if r.kind === "folder"}
          {@const open = !collapsed.has(key(r.repo.repo, r.node.rel))}
          <button
            class="row"
            role="treeitem"
            aria-selected="false"
            aria-level={r.depth + 1}
            aria-expanded={open}
            data-id={r.id}
            tabindex={tabId === r.id ? 0 : -1}
            style:padding-left="{pad(r.depth)}px"
            title={r.node.rel}
            onclick={() => {
              focusId = r.id;
              toggle(key(r.repo.repo, r.node.rel));
            }}
          >
            <svg class="chev" class:open viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
              <path d="M6 4.5 9.5 8 6 11.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
            <img class="icon" src={iconFor(r.node.name, "folder", open, theme)} alt="" draggable="false" />
            <span class="name">{r.node.name}</span>
          </button>
        {:else if r.kind === "file"}
          {@const f = r.file}
          {@const warn = warnText(f, r.repo.subjects)}
          {@const isActive = active?.repo === r.repo.repo && active?.path === f.path}
          <button
            class="row file"
            class:selected={isActive}
            role="treeitem"
            aria-selected={isActive}
            aria-level={r.depth + 1}
            data-id={r.id}
            tabindex={tabId === r.id ? 0 : -1}
            style:padding-left="{pad(r.depth)}px"
            title="{f.path} · {statusText(asChanged(f))} · {f.shas.map(shortSha).join(', ')}"
            aria-label="{r.node.name}, {statusText(asChanged(f))}, {f.shas.length} {f.shas.length === 1 ? 'commit' : 'commits'}{warn ? `, warning: ${warn}` : ''}"
            onclick={() => {
              focusId = r.id;
              onopenfile(r.repo.repo, f);
            }}
          >
            <span class="chev-gap" aria-hidden="true"></span>
            <img class="icon" src={iconFor(r.node.name, "file", false, theme)} alt="" draggable="false" />
            <span class="name">{r.node.name}</span>
            {#if f.from}<span class="from" aria-hidden="true">← {f.from.split("/").pop()}</span>{/if}
            <span class="spacer"></span>
            {#if warn}
              <svg class="warn" viewBox="0 0 16 16" width="14" height="14" role="img" aria-label="Warning"><title>{warn}</title>
                <path d="M8 2 1.5 13.5h13L8 2Z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
                <path d="M8 6.5v3.2M8 11.6v.1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
              </svg>
            {/if}
            <span class="shas num" aria-hidden="true">{shaList(f.shas)}</span>
            <span class="badge s-{f.status}" aria-hidden="true">{statusLetter(asChanged(f))}</span>
          </button>
        {:else if r.kind === "note"}
          <p class="note" style:padding-left="{pad(1)}px">{r.text}</p>
        {:else}
          <p class="note err" role="alert" style:padding-left="{pad(1)}px">{r.text}</p>
        {/if}
      {/each}
    </div>
  {/if}
</section>

<style>
  .changes {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: auto;
  }
  .head {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px var(--pad) 4px;
  }
  .pane-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--muted);
  }
  .clear {
    flex: none;
    height: 22px;
    padding: 0 8px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    color: var(--muted);
  }
  .clear:hover {
    background: var(--hover);
    color: var(--text);
  }
  .clear:focus-visible,
  .row:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
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
  .row.root {
    padding-left: 12px;
    margin-top: 2px;
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
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 20px;
    padding: 0 7px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--c);
    background: color-mix(in srgb, var(--c) 12%, transparent);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--c);
  }
  .cname,
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta,
  .shas {
    flex: none;
    font-size: var(--fs-meta);
    color: var(--faint);
  }
  .shas {
    font-family: var(--font-mono);
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
  .warn {
    flex: none;
    color: var(--pending);
  }
  .note {
    margin: 0;
    padding-right: 10px;
    height: var(--row-h);
    line-height: var(--row-h);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-meta);
    color: var(--faint);
  }
  .note.err {
    color: var(--negative);
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
  .badge.s-added {
    color: var(--untracked);
  }
  .badge.s-deleted {
    color: var(--negative);
  }
  .badge.s-renamed {
    color: var(--muted);
  }
</style>
