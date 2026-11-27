<script module lang="ts">
  import type { ChangedFile } from "./changes";
  import type { CompareFiles, SideCommit } from "./compare";

  // Answers are keyed on commit ids, so they never go stale: a refresh that finds the same tips
  // reads nothing again, and a swap finds its answer under the mirrored key.
  const CACHE_MAX = 60;
  const filesCache = new Map<string, CompareFiles>();
  const commitsCache = new Map<string, SideCommit[]>();
  const changesCache = new Map<string, ChangedFile[]>();
  function remember<T>(m: Map<string, T>, k: string, v: T) {
    m.delete(k);
    m.set(k, v);
    if (m.size > CACHE_MAX) m.delete(m.keys().next().value as string);
  }
</script>

<script lang="ts">
  import { tick, untrack } from "svelte";
  import { gitlogChanges, gitlogCompareCommits, gitlogCompareFiles } from "../lib/api";
  import { iconFor, type IconTheme } from "../lib/icons";
  import Changes from "./Changes.svelte";
  import { countsText, statusLetter, statusText } from "./changes";
  import {
    COMMIT_PAGE,
    bothStripText,
    columnHeader,
    emptyColumnText,
    pairDuplicates,
    sidePage,
    type CompareMode,
    type CompareOpen,
    type CompareRow,
    type CompareSide,
    type Pair,
  } from "./compare";
  import { commitAge, commitDate, isStale, shortSha } from "./log";

  // The selected repository's two sides: what each branch has that the other has not, as a folder
  // tree of files (Files) or as commits (Commits), and the On both strip under them. Reads are tied
  // to the compare query that listed the repository; a superseded one is simply dropped.
  let {
    row,
    name,
    pair,
    mode,
    queryId,
    theme,
    open,
    onopen,
    onescape,
  }: {
    row: CompareRow;
    name: string;
    pair: Pair;
    mode: CompareMode;
    queryId: number;
    theme: IconTheme;
    /** The file whose diff is open. */
    open: CompareOpen | null;
    onopen: (o: CompareOpen) => void;
    /** Esc: back to the repo list. */
    onescape: () => void;
  } = $props();

  const SIDES: CompareSide[] = ["left", "right"];
  const d = $derived(row.kind === "differs" ? row : null);
  const sideName = (s: CompareSide) => (s === "left" ? pair.left : pair.right);
  const dupCount = $derived(d ? Math.max(d.sameLeft, d.sameRight) : 0);

  // ---- Files ----
  let files = $state<CompareFiles | null>(null);
  let filesError = $state<string | null>(null);
  let filesLoading = $state(false);
  let filesSeq = 0;
  const filesKey = (l: string, r: string) => `${row.repo}\u0000${l}\u0000${r}`;

  async function loadFiles() {
    if (!d) return;
    const seq = ++filesSeq;
    const hit = filesCache.get(filesKey(d.leftSha, d.rightSha));
    const flipped = filesCache.get(filesKey(d.rightSha, d.leftSha));
    if (hit || flipped) {
      files = hit ?? { left: flipped!.right, right: flipped!.left, both: flipped!.both };
      filesError = null;
      filesLoading = false;
      return;
    }
    filesLoading = true;
    filesError = null;
    try {
      const f = await gitlogCompareFiles(queryId, row.repo, d.leftSha, d.rightSha, d.base);
      remember(filesCache, filesKey(d.leftSha, d.rightSha), f);
      if (seq !== filesSeq) return;
      files = f;
      filesLoading = false;
    } catch (e) {
      // A newer compare took over: its answer brings the rows (and this read) again.
      if (seq !== filesSeq || isStale(e)) return;
      filesError = String(e);
      filesLoading = false;
    }
  }

  // ---- Commits ----
  type SideState = { commits: SideCommit[] | null; limit: number; loading: boolean; error: string | null };
  const fresh = (): SideState => ({ commits: null, limit: COMMIT_PAGE, loading: false, error: null });
  let sides = $state<Record<CompareSide, SideState>>({ left: fresh(), right: fresh() });
  const commitSeq: Record<CompareSide, number> = { left: 0, right: 0 };
  const commitsKey = (side: CompareSide, l: string, r: string, max: number) => `${row.repo}\u0000${side}\u0000${l}\u0000${r}\u0000${max}`;

  async function loadCommits(side: CompareSide, limit: number) {
    if (!d) return;
    const seq = ++commitSeq[side];
    const max = limit + 1;
    const other: CompareSide = side === "left" ? "right" : "left";
    const hit = commitsCache.get(commitsKey(side, d.leftSha, d.rightSha, max)) ?? commitsCache.get(commitsKey(other, d.rightSha, d.leftSha, max));
    if (hit) {
      sides[side] = { commits: hit, limit, loading: false, error: null };
      return;
    }
    sides[side] = { ...sides[side], loading: true, error: null };
    try {
      const list = await gitlogCompareCommits(queryId, row.repo, side, d.leftSha, d.rightSha, max);
      remember(commitsCache, commitsKey(side, d.leftSha, d.rightSha, max), list);
      if (seq !== commitSeq[side]) return;
      sides[side] = { commits: list, limit, loading: false, error: null };
    } catch (e) {
      if (seq !== commitSeq[side] || isStale(e)) return;
      sides[side] = { ...sides[side], loading: false, error: String(e) };
    }
  }

  /** "Show 500 more": the whole list again, one page longer. */
  function more(side: CompareSide) {
    void loadCommits(side, sides[side].limit + COMMIT_PAGE);
  }

  // The On both strip lists the duplicates; in Files mode their commits are read when it is unfolded.
  let stripOpen = $state(false);
  const duplicates = $derived(
    sides.left.commits && sides.right.commits
      ? pairDuplicates(
          sides.left.commits.filter((c) => c.mark === "="),
          sides.right.commits.filter((c) => c.mark === "="),
        )
      : null,
  );

  // A different repository, pair or tips: everything is read again (or found in the cache).
  const readKey = $derived(d ? `${row.repo}\u0000${d.leftSha}\u0000${d.rightSha}\u0000${d.base}` : row.repo);
  let lastRead = "";
  let lastMode: CompareMode | null = null;
  $effect(() => {
    const k = readKey;
    const m = mode;
    const strip = stripOpen;
    untrack(() => read(k, m, strip));
  });
  function read(k: string, m: CompareMode, strip: boolean) {
    if (k !== lastRead) {
      lastRead = k;
      filesSeq++;
      commitSeq.left++;
      commitSeq.right++;
      files = null;
      filesError = null;
      filesLoading = false;
      sides = { left: fresh(), right: fresh() };
      expanded = { left: null, right: null };
      lastMode = null;
    }
    if (!d) return;
    if (m === "files" && lastMode !== "files") void loadFiles();
    const wantCommits = m === "commits" || strip;
    for (const s of SIDES) if (wantCommits && !sides[s].commits && !sides[s].loading && !sides[s].error) void loadCommits(s, sides[s].limit);
    lastMode = m;
  }

  // ---- a commit's files, one expanded per column ----
  let expanded = $state<Record<CompareSide, { sha: string; files: ChangedFile[] | null; error: string | null } | null>>({ left: null, right: null });
  async function toggleCommit(side: CompareSide, c: SideCommit) {
    if (expanded[side]?.sha === c.sha) {
      expanded[side] = null;
      return;
    }
    const k = `${row.repo}\u0000${c.sha}`;
    const hit = changesCache.get(k);
    expanded[side] = { sha: c.sha, files: hit ?? null, error: null };
    if (hit) return;
    try {
      const list = await gitlogChanges(row.repo, c.sha);
      remember(changesCache, k, list);
      if (expanded[side]?.sha === c.sha) expanded[side] = { sha: c.sha, files: list, error: null };
    } catch (e) {
      if (expanded[side]?.sha === c.sha) expanded[side] = { sha: c.sha, files: null, error: String(e) };
    }
  }

  const openSide = (side: CompareSide, f: ChangedFile) => {
    if (!d) return;
    onopen({ kind: "side", repo: row.repo, side, file: f, base: d.base, tip: side === "left" ? d.leftSha : d.rightSha, pair });
  };
  const activeSidePath = (side: CompareSide) => (open?.kind === "side" && open.repo === row.repo && open.side === side ? open.file.path : null);
  const isOpenCommitFile = (c: SideCommit, f: ChangedFile) => open?.kind === "commit" && open.repo === row.repo && open.commit.sha === c.sha && open.file.path === f.path;

  // ---- keyboard in a commit column: ↑/↓ over its rows, Enter, Esc ----
  const cols: Partial<Record<CompareSide, HTMLDivElement>> = $state({});
  function colRows(side: CompareSide): HTMLElement[] {
    return Array.from(cols[side]?.querySelectorAll<HTMLElement>("[data-nav]") ?? []);
  }
  function colKey(e: KeyboardEvent, side: CompareSide) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    const list = colRows(side);
    const i = list.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const to = Math.max(0, Math.min(list.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      list[to]?.focus();
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      list[e.key === "Home" ? 0 : list.length - 1]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onescape();
    }
  }

  /** The keyboard into the left column (its first row). */
  export async function focus() {
    await tick();
    const first = cols.left?.querySelector<HTMLElement>("[data-nav], button.row, [role='treeitem']") ?? cols.right?.querySelector<HTMLElement>("[data-nav], button.row, [role='treeitem']");
    first?.focus();
  }
</script>

<div class="columns-wrap">
  {#if row.kind === "nobase"}
    <p class="note pad">{name} has no common history for {pair.left} and {pair.right}: the two branches share no ancestor, so there is nothing to compare.</p>
  {:else if row.kind === "error"}
    <p class="note pad" role="alert">git could not compare {name}: {row.reason}</p>
  {:else if d}
    <div class="columns">
      {#each SIDES as side (side)}
        {@const s = sides[side]}
        <div class="col {side}" bind:this={cols[side]} aria-label="{side === 'left' ? '◀' : '▶'} {sideName(side)} only">
          {#snippet header(n: number | null)}
            <div class="chead">
              <p class="ctitle"><span class="arrow {side}" aria-hidden="true">{side === "left" ? "◀" : "▶"}</span> <span class="bname">{sideName(side)}</span> only</p>
              {#if n !== null}<p class="csub">{columnHeader(side, mode, n, pair)}</p>{/if}
            </div>
          {/snippet}
          {#if mode === "files"}
            {@const list = files ? files[side] : null}
            {#snippet fhead()}{@render header(list ? list.length : null)}{/snippet}
            <Changes
              title=""
              label="{sideName(side)} only, files"
              files={filesError ? null : list}
              loading={filesLoading}
              error={filesError}
              activePath={activeSidePath(side)}
              {theme}
              both={files ? new Set(files.both) : null}
              emptyText={emptyColumnText(side, pair)}
              onopenfile={(f) => openSide(side, f)}
              {onescape}
              header={fhead}
            />
          {:else}
            {@const page = s.commits ? sidePage(s.commits, s.limit) : null}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="clist" onkeydown={(e) => colKey(e, side)}>
              {@render header(page ? (side === "left" ? d.left : d.right) : null)}
              {#if s.error}
                <p class="note pad" role="alert">Couldn't read the commits: {s.error}</p>
              {:else if !page}
                <div class="skeletons" aria-busy="true" aria-label="Reading the commits">
                  {#each [70, 52, 62] as w, i (i)}<div class="skeleton" style:width="{w}%"></div>{/each}
                </div>
              {:else if page.rows.length === 0}
                <p class="note pad">{emptyColumnText(side, pair)}</p>
              {:else}
                <ul class="commits" class:stale={s.loading}>
                  {#each page.rows as c (c.sha)}
                    {@const ex = expanded[side]?.sha === c.sha ? expanded[side] : null}
                    <li>
                      <button
                        class="crow"
                        data-nav
                        aria-expanded={!!ex}
                        title="{c.subject}&#10;{shortSha(c.sha)} · {c.author} · {commitDate(c.time)}"
                        onclick={() => void toggleCommit(side, c)}
                      >
                        <svg class="chev" class:open={!!ex} viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
                          <path d="M6 4.5 9.5 8 6 11.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
                        </svg>
                        <span class="subj">{c.subject}</span>
                        <span class="meta">{c.author} · {commitAge(c.time)}{#if ex?.files} · {ex.files.length} {ex.files.length === 1 ? "file" : "files"}{/if}</span>
                      </button>
                      {#if ex}
                        {#if ex.error}
                          <p class="note sub" role="alert">Couldn't read its files: {ex.error}</p>
                        {:else if !ex.files}
                          <p class="note sub">Reading its files…</p>
                        {:else if ex.files.length === 0}
                          <p class="note sub">This commit changes no files.</p>
                        {:else}
                          <ul class="cfiles">
                            {#each ex.files as f (f.path)}
                              {@const base = f.path.split("/").pop() ?? f.path}
                              <li>
                                <button
                                  class="frow"
                                  class:selected={isOpenCommitFile(c, f)}
                                  data-nav
                                  title="{f.path} · {statusText(f)}"
                                  aria-label="{base}, {statusText(f)}, {countsText(f)}"
                                  onclick={() => onopen({ kind: "commit", repo: row.repo, commit: c, file: f })}
                                >
                                  <img class="icon" src={iconFor(base, "file", false, theme)} alt="" draggable="false" />
                                  <span class="fname">{base}</span>
                                  <span class="fdir">{f.path.slice(0, Math.max(0, f.path.length - base.length - 1))}</span>
                                  <span class="spacer"></span>
                                  <span class="counts num" aria-hidden="true">
                                    {#if f.added === null || f.removed === null}<span class="faint">binary</span>{:else}<span class="add">+{f.added}</span><span class="del">−{f.removed}</span>{/if}
                                  </span>
                                  <span class="badge s-{f.status}" aria-hidden="true">{statusLetter(f)}</span>
                                </button>
                              </li>
                            {/each}
                          </ul>
                        {/if}
                      {/if}
                    </li>
                  {/each}
                </ul>
                {#if page.more}
                  <button class="more" data-nav disabled={s.loading} onclick={() => more(side)}>{s.loading ? "Reading…" : `Show ${COMMIT_PAGE} more`}</button>
                {/if}
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
    {#if dupCount > 0}
      <details class="strip" bind:open={stripOpen}>
        <summary>{bothStripText(dupCount)}</summary>
        {#if duplicates}
          <ul class="dups">
            {#each duplicates as dup, i (i)}
              <li>
                <span class="dsha left">{#if dup.left}<span aria-hidden="true">◀</span> {shortSha(dup.left)}{/if}</span>
                <span class="dsha right">{#if dup.right}<span aria-hidden="true">▶</span> {shortSha(dup.right)}{/if}</span>
                <span class="dsubj">{dup.subject}</span>
              </li>
            {/each}
          </ul>
        {:else if sides.left.error || sides.right.error}
          <p class="note pad" role="alert">Couldn't read the commits: {sides.left.error ?? sides.right.error}</p>
        {:else}
          <p class="note pad">Reading the commits…</p>
        {/if}
      </details>
    {/if}
  {/if}
</div>

<style>
  .columns-wrap {
    /* ◀ blue, ▶ yellow: the chip palette's own accents, readable on the surface in both themes. */
    --cmp-left: var(--repo-9);
    --cmp-right: var(--repo-4);
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .columns {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    min-height: 0;
  }
  .col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .col.left {
    border-right: 1px solid var(--rule);
  }
  .col > :global(.changes),
  .clist {
    flex: 1;
    min-height: 0;
    overflow: auto;
    /* A row scrolled into view (focus, a click) lands below the sticky header, not under it. */
    scroll-padding-top: 56px;
  }
  .chead {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 8px var(--pad) 6px;
    background: var(--surface);
    border-bottom: 1px solid var(--rule);
  }
  .ctitle {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--text);
  }
  .bname {
    font-family: var(--font-mono);
  }
  .csub {
    margin-top: 2px;
    font-size: var(--fs-meta);
    color: var(--muted);
  }
  .arrow.left {
    color: var(--cmp-left);
  }
  .arrow.right {
    color: var(--cmp-right);
  }
  .note {
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .pad {
    padding: 10px var(--pad);
    max-width: 560px;
  }
  .note.sub {
    padding: 2px var(--pad) 4px 40px;
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
  .commits,
  .cfiles,
  .dups {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .commits {
    padding: 4px 0 12px;
  }
  .commits.stale {
    opacity: 0.6;
  }
  .crow,
  .frow {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: var(--row-h);
    padding: 0 10px 0 8px;
    overflow: hidden;
    white-space: nowrap;
    text-align: left;
    color: var(--text);
  }
  .frow {
    padding-left: 30px;
  }
  .crow:hover,
  .frow:hover {
    background: var(--hover);
  }
  .frow.selected {
    background: var(--active);
  }
  .crow:focus-visible,
  .frow:focus-visible,
  .more:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .chev {
    flex: none;
    color: var(--faint);
    transition: transform 120ms var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .subj {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--fs-meta);
    color: var(--faint);
  }
  .icon {
    flex: none;
    width: 16px;
    height: 16px;
  }
  .fname {
    flex: none;
  }
  .fdir {
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
  .more {
    margin: 4px var(--pad) 12px;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    color: var(--text);
  }
  .more:hover:not(:disabled) {
    background: var(--hover);
  }
  .strip {
    flex: none;
    max-height: 40%;
    overflow: auto;
    border-top: 1px solid var(--rule);
    background: var(--hover);
    font-size: var(--fs-small);
  }
  .strip summary {
    padding: 6px var(--pad);
    cursor: pointer;
    color: var(--muted);
  }
  .strip summary:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .dups li {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 2px var(--pad);
    white-space: nowrap;
  }
  .dsha {
    flex: none;
    width: 78px;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
  }
  .dsha.left {
    color: var(--cmp-left);
  }
  .dsha.right {
    color: var(--cmp-right);
  }
  .dsubj {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text);
  }
</style>
