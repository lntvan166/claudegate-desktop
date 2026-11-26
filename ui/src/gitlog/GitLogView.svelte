<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import DiffView from "../components/DiffView.svelte";
  import Splitter from "../components/Splitter.svelte";
  import {
    gitlogChanges,
    gitlogConfig,
    gitlogFile,
    gitlogLastFilter,
    gitlogPane,
    gitlogRepos,
    gitlogWorktreeFile,
    openInEditor,
    setEditor,
  } from "../lib/api";
  import { openedText, type EditorConfig } from "../lib/editors";
  import type { IconTheme } from "../lib/icons";
  import type { Row } from "../lib/rows";
  import Changes from "./Changes.svelte";
  import {
    CHANGES_DEBOUNCE_MS,
    ChangesCache,
    enterAction,
    leadingDebounce,
    changesTitle,
    createGate,
    diffPane,
    type FileView,
    paneMessage,
    pickFile,
    statusText,
    type ChangedFile,
    type DiffPane,
  } from "./changes";
  import Filters from "./Filters.svelte";
  import LogList from "./LogList.svelte";
  import RepoPane from "./RepoPane.svelte";
  import { reposForLaunch } from "../lib/launch";
  import { DEFAULT_FILTER, debounce, parseFilter, rowKey, type FilterState, type RowView } from "./log";
  import {
    DEFAULT_GITLOG_CONFIG,
    PANE_DEFAULTS,
    PANE_MIN,
    fitPanes,
    isTicked,
    paneMax,
    panesFrom,
    type GitLogConfig,
    type GitLogPanes,
    type RepoView,
  } from "./repos";

  // The Git log view. App.svelte mounts it the first time it is opened, never at startup, and keeps
  // it mounted (hidden) afterwards. The commit list and filters (Task 6) and the Changes tree and
  // diff (Task 7) fill the placeholders below.
  let {
    active,
    onnotice,
    config,
    theme,
    onconfig,
  }: {
    /** On screen; the repository list is fetched again each time it comes back. */
    active: boolean;
    onnotice: (text: string) => void;
    /** The editor "Open in editor" uses (the review's own setting). */
    config: EditorConfig;
    theme: IconTheme;
    /** The editor choice changed (from the Open with menu): App keeps the one config. */
    onconfig: (c: EditorConfig) => void;
  } = $props();

  let repoPane: RepoPane | undefined = $state();
  /** Puts the keyboard in the Repositories pane (App calls it after switching to this view). */
  export function focusStart() {
    // The commit list when there is one to read; the Repositories pane when nothing is ticked yet.
    if (logList && queryRepos.length > 0) logList.focusList();
    else void repoPane?.focusStart();
  }

  /**
   * `claudegate log PATH`: discovers again (when `refresh`, after the workspace changed), then ticks
   * the repositories under the path, for this launch only. Resolves false when none of them is
   * discovered, so the caller can say the path isn't in the workspace.
   */
  export async function applyLaunch(t: { path: string; canonical: string | null }, refresh = false): Promise<boolean> {
    await load(refresh);
    const ids = reposForLaunch(repos ?? [], t.path, t.canonical);
    if (ids.length === 0) return false;
    launchTicks = ids;
    cfg = { ...cfg, ticked: ids };
    // Only the selection is cleared: it may point at a repo no longer shown. The filters stay.
    selected = null;
    return true;
  }

  const COLLAPSED_KEY = "claudegate.gitlog.reposCollapsed";

  let repos = $state<RepoView[] | null>(null);
  let reposError = $state<string | null>(null);
  let refreshing = $state(false);
  let cfg = $state<GitLogConfig>(structuredClone(DEFAULT_GITLOG_CONFIG));
  let preferred = $state<GitLogPanes>({ ...PANE_DEFAULTS });
  let collapsed = $state(readCollapsed());
  let innerWidth = $state(typeof window === "undefined" ? 1200 : window.innerWidth);
  const panes = $derived(fitPanes(preferred, innerWidth, collapsed));
  /**
   * The ticks of a `claudegate log PATH` launch, for this launch only: `cfg.ticked` shows them,
   * nothing is written to config.json. The Repos pane writes them out (as the explicit `ticked`
   * list) the first time the user edits ticks, or drops them when the user replaces ticks.
   */
  let launchTicks = $state<string[] | null>(null);
  const shown = $derived((repos ?? []).filter((r) => isTicked(cfg.ticked, r.id)));
  const queryRepos = $derived(shown.map((r) => r.id));

  // The filters. They start from the stored ones and are written back (only that key) once the
  // stored ones were read, so the defaults never overwrite them.
  let filter = $state<FilterState>({ ...DEFAULT_FILTER });
  let filtersRead = false;
  /** The settings were read (or failed to be): the log's first query waits for it, so it never runs with the defaults. */
  let configReady = $state(false);
  /** The filter as last stored or loaded; an unchanged filter is not written again. */
  let lastStored = "";
  let filters: Filters | undefined = $state();
  let logList: LogList | undefined = $state();
  let logBusy = $state(false);
  let selected = $state<RowView | null>(null);
  const saveFilter = debounce(() => {
    lastStored = JSON.stringify(filter);
    void gitlogLastFilter($state.snapshot(filter)).catch(() => {});
  }, 500);
  $effect(() => {
    const now = JSON.stringify(filter);
    if (filtersRead && now !== lastStored) saveFilter();
  });

  // ---- the selected commit's changes and diff -------------------------------------------------
  // Selecting a commit fetches its files after a short rest (holding an arrow key spawns none);
  // a file's two sides are fetched when it is opened. Two generation counters keep an older answer
  // from landing on a newer selection.
  let changesTree: Changes | undefined = $state();
  let diffView: DiffView | undefined = $state();
  /** The commit `files` belong to (may trail `selected` while the next one loads). */
  let changesFor = $state<RowView | null>(null);
  let files = $state<ChangedFile[] | null>(null);
  let filesLoading = $state(false);
  let filesError = $state<string | null>(null);
  let activeFile = $state<ChangedFile | null>(null);
  let diffLoading = $state(false);
  let diffError = $state<string | null>(null);
  let shownPane = $state<DiffPane | null>(null);
  let openError = $state<string | null>(null);
  let focusTreeWhenReady = false;
  const changesCache = new ChangesCache();
  const commitGate = createGate();
  const fileGate = createGate();
  /** The commit whose files are being fetched now. */
  let inflightFor: string | null = null;
  const fetchChanges = leadingDebounce(() => void loadChanges(), CHANGES_DEBOUNCE_MS);

  const repoName = (id: string) => repos?.find((r) => r.id === id)?.name ?? id.replace(/\/\.git\/?$/, "").split("/").pop() ?? id;
  const changesHeading = $derived(
    changesFor && files ? changesTitle(repoName(changesFor.repo), changesFor.sha, files.length) : "Changes",
  );

  const selKey = $derived(selected ? rowKey(selected) : null);
  let lastKey: string | null = null;
  $effect(() => {
    const k = selKey;
    if (k === lastKey) return;
    lastKey = k;
    untrack(() => commitChanged(selected));
  });

  function clearDiff() {
    fileGate.next();
    activeFile = null;
    shownPane = null;
    diffLoading = false;
    diffError = null;
    openError = null;
  }

  function commitChanged(r: RowView | null) {
    commitGate.next();
    fetchChanges.drop();
    // Enter on an earlier commit must not pull the keyboard into this one's tree.
    focusTreeWhenReady = false;
    inflightFor = null;
    filesError = null;
    if (!r) {
      changesFor = null;
      files = null;
      filesLoading = false;
      clearDiff();
      return;
    }
    const hit = changesCache.get(r.repo, r.sha);
    if (hit) return applyChanges(r, hit);
    filesLoading = true;
    fetchChanges();
  }

  async function loadChanges() {
    const r = selected;
    if (!r) return;
    const g = commitGate.current();
    inflightFor = rowKey(r);
    filesLoading = true;
    filesError = null;
    try {
      const list = await gitlogChanges(r.repo, r.sha);
      if (!commitGate.isCurrent(g)) return;
      inflightFor = null;
      changesCache.set(r.repo, r.sha, list);
      applyChanges(r, list);
    } catch (e) {
      if (!commitGate.isCurrent(g)) return;
      inflightFor = null;
      filesError = String(e);
      files = null;
      filesLoading = false;
      changesFor = null;
      clearDiff();
      focusTreeWhenReady = false;
    }
  }

  function applyChanges(r: RowView, list: ChangedFile[]) {
    changesFor = r;
    files = list;
    filesLoading = false;
    filesError = null;
    // The diff follows the selection: the file that was open if this commit has it too, else the first.
    const pick = pickFile(list, untrack(() => activeFile?.path ?? null));
    if (pick) void showFile(pick);
    else clearDiff();
    if (focusTreeWhenReady) {
      focusTreeWhenReady = false;
      void tick().then(() => changesTree?.focus());
    }
  }

  async function showFile(f: ChangedFile) {
    const r = changesFor;
    if (!r) return;
    const g = fileGate.next();
    activeFile = f;
    shownPane = null;
    diffLoading = true;
    diffError = null;
    openError = null;
    try {
      const [before, after] = await Promise.all([
        // An added file has no before; a rename's old path is already known, so no lookup runs.
        f.status === "added"
          ? Promise.resolve<FileView>({ kind: "missing" })
          : gitlogFile(r.repo, r.sha, f.path, "before", f.from ?? undefined),
        gitlogFile(r.repo, r.sha, f.path, "after"),
      ]);
      if (!fileGate.isCurrent(g)) return;
      shownPane = diffPane(f.path, before, after);
      diffLoading = false;
    } catch (e) {
      if (!fileGate.isCurrent(g)) return;
      diffError = String(e);
      diffLoading = false;
    }
  }

  /** Enter on a commit: its changes, with the keyboard in the tree on the first file. */
  function openCommit(row: RowView) {
    const key = rowKey(row);
    const loaded = !!changesFor && !!files && rowKey(changesFor) === key;
    const act = enterAction({ loaded, inflight: inflightFor === key, pending: fetchChanges.hasPending() });
    if (act === "focus") {
      if (!activeFile && files?.length) {
        const first = pickFile(files, null);
        if (first) void showFile(first);
      }
      changesTree?.focus();
      return;
    }
    focusTreeWhenReady = true;
    if (act === "flush") fetchChanges.flush();
    else if (act === "load") void loadChanges();
  }

  function leaveBottom(e: KeyboardEvent) {
    // Esc in the tree (it handles its own) or the diff: back to the commit list.
    if (e.key !== "Escape" || e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    logList?.focusList();
  }

  /** The row the shared diff header draws: a commit's file, never a review row. */
  const diffRow = $derived<Row | null>(
    changesFor && activeFile
      ? {
          key: `${changesFor.sha}:${activeFile.path}`,
          path: activeFile.path,
          rel: activeFile.path,
          scope: changesFor.repo,
          added: activeFile.added ?? 0,
          removed: activeFile.removed ?? 0,
          newFile: activeFile.status === "added",
          decidedAt: null,
          waiting: false,
          rejectOutcome: null,
        }
      : null,
  );
  const external = $derived.by(() => {
    const pane = shownPane;
    const f = activeFile;
    const empty = {
      title: selected ? "Select a file to see its diff" : "Select a commit to see its changes",
      body: selected ? "Click a file in Changes, or press Enter on it." : "Pick one in the log above; its files appear on the left.",
    };
    return {
      payload: pane?.kind === "diff" ? pane.payload : null,
      loading: diffLoading,
      error: diffError,
      notice: !f ? empty : pane && pane.kind !== "diff" ? paneMessage(pane) : null,
      tag:
        f?.status === "deleted"
          ? { text: "Deleted", tone: "deleted" as const }
          : f && (f.status === "renamed" || f.status === "copied")
            ? { text: statusText(f), tone: "new" as const }
            : null,
    };
  });

  /** "Open in editor": the working-tree file, never written to. It may be gone since the commit. */
  async function openInWorktree(line: number, col: number, template: string | null = null): Promise<boolean> {
    const r = changesFor;
    const f = activeFile;
    if (!r || !f) return false;
    openError = null;
    try {
      // The commit's source picks the checkout it lives in (a worktree-only commit opens there).
      const abs = await gitlogWorktreeFile(r.repo, f.path, r.source);
      await openInEditor(abs, line, col, template);
      onnotice(openedText(abs, line, template ?? config.editor));
      return true;
    } catch (e) {
      openError = String(e);
      return false;
    }
  }
  function openAtCursor() {
    const t = diffView?.openTarget() ?? { line: 1, col: 1 };
    void openInWorktree(t.line, t.col);
  }
  async function pickEditor(template: string) {
    const t = diffView?.openTarget() ?? { line: 1, col: 1 };
    if (!(await openInWorktree(t.line, t.col, template))) return;
    try {
      onconfig(await setEditor(template));
    } catch (e) {
      openError = `Opened, but couldn't remember the choice: ${e}`;
    }
  }

  function readCollapsed(): boolean {
    try {
      return localStorage.getItem(COLLAPSED_KEY) === "1";
    } catch {
      return false;
    }
  }
  function toggleCollapsed() {
    collapsed = !collapsed;
    try {
      localStorage.setItem(COLLAPSED_KEY, collapsed ? "1" : "0");
    } catch {}
  }

  let seq = 0;
  async function load(refresh: boolean) {
    const n = ++seq;
    if (refresh) refreshing = true;
    try {
      const list = await gitlogRepos(refresh);
      if (n !== seq) return;
      repos = list;
      reposError = null;
      if (refresh) onnotice(`${list.length === 1 ? "1 repository" : `${list.length} repositories`} found`);
    } catch (e) {
      if (n === seq) reposError = String(e);
    } finally {
      if (n === seq) refreshing = false;
    }
  }

  async function loadConfig() {
    try {
      cfg = await gitlogConfig();
      if (launchTicks) cfg = { ...cfg, ticked: launchTicks };
      preferred = panesFrom(cfg.panes);
      filter = parseFilter(cfg.lastFilter);
      lastStored = JSON.stringify(filter);
      filtersRead = true;
    } catch (e) {
      onnotice(`Couldn't read the Git log settings: ${e}`);
    } finally {
      configReady = true;
    }
  }

  function setPane(pane: keyof GitLogPanes, width: number) {
    const max = paneMax(panes, pane, innerWidth);
    preferred = { ...preferred, [pane]: Math.min(max, Math.max(PANE_MIN[pane], Math.round(width))) };
  }
  function storePane(pane: keyof GitLogPanes) {
    void gitlogPane(pane, preferred[pane]).then(
      (c) => (cfg = { ...cfg, panes: c.panes }),
      () => {},
    );
  }
  function resetPane(pane: keyof GitLogPanes) {
    preferred = { ...preferred, [pane]: PANE_DEFAULTS[pane] };
    storePane(pane);
  }

  onMount(() => {
    void loadConfig();
  });

  // Every time the view comes on screen (the first time included): the workspace may have
  // changed while the review was up, and ahead/behind is as of now.
  let wasActive = false;
  $effect(() => {
    if (active && !wasActive) void load(false);
    wasActive = active;
  });
</script>

<svelte:window bind:innerWidth />

<div class="gitlog-view" class:hidden={!active} style:--repos-w="{panes.repos}px" style:--changes-w="{panes.changes}px">
  <RepoPane
    bind:this={repoPane}
    {repos}
    error={reposError}
    bind:cfg
    {collapsed}
    {refreshing}
    ontoggle={toggleCollapsed}
    onrefresh={() => void load(true)}
    {onnotice}
    {launchTicks}
    onlaunchdone={() => (launchTicks = null)}
    onslash={() => filters?.focusSearch()}
  />

  {#if !collapsed}
    <div class="split split-repos">
      <Splitter
        label="Resize repositories"
        value={panes.repos}
        min={PANE_MIN.repos}
        max={paneMax(panes, "repos", innerWidth)}
        left={panes.repos}
        onresize={(w) => setPane("repos", w)}
        oncommit={() => storePane("repos")}
        onreset={() => resetPane("repos")}
      />
    </div>
  {/if}

  <div class="sheet">
    <div class="log">
      <Filters bind:this={filters} bind:filter loading={logBusy} onrefresh={() => logList?.refresh()} onexit={() => logList?.focusList()} />
      {#if (repos === null && !reposError) || !configReady}
        <div class="state" aria-hidden="true">
          {#each [82, 64, 74] as w, i (i)}<div class="skeleton" style:width="{w}%"></div>{/each}
        </div>
      {:else}
        <LogList
          bind:this={logList}
          repos={repos ?? []}
          {queryRepos}
          {filter}
          {active}
          onopen={openCommit}
          onselect={(r) => (selected = r)}
          onfocussearch={() => filters?.focusSearch()}
          onloading={(b) => (logBusy = b)}
          {onnotice}
        />
      {/if}
    </div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bottom" onkeydown={leaveBottom}>
      <Changes
        bind:this={changesTree}
        title={changesHeading}
        {files}
        loading={filesLoading}
        error={filesError}
        activePath={activeFile?.path ?? null}
        {theme}
        onopenfile={(f) => !filesLoading && void showFile(f)}
        onescape={() => logList?.focusList()}
      />
      <div class="split split-changes">
        <Splitter
          label="Resize changes"
          value={panes.changes}
          min={PANE_MIN.changes}
          max={paneMax(panes, "changes", innerWidth)}
          left={panes.changes - 1}
          onresize={(w) => setPane("changes", w)}
          oncommit={() => storePane("changes")}
          onreset={() => resetPane("changes")}
        />
      </div>
      <div class="diffcol">
        <DiffView
          bind:this={diffView}
          row={diffRow}
          kind="pending"
          root={changesFor?.repo ?? ""}
          version={0}
          {theme}
          {config}
          {openError}
          {external}
          onopen={openAtCursor}
          onopenat={(line, col) => void openInWorktree(line, col)}
          onpick={(t) => void pickEditor(t)}
          ondismiss={() => (openError = null)}
          onnotice={onnotice}
        />
      </div>
    </div>
  </div>
</div>

<style>
  .gitlog-view {
    grid-column: 1 / -1;
    position: relative;
    display: grid;
    grid-template-columns: var(--repos-w) minmax(0, 1fr);
    min-height: 0;
    min-width: 0;
  }
  .gitlog-view.hidden {
    display: none;
  }
  .split {
    position: absolute;
    inset: 0;
    width: 0;
    pointer-events: none;
  }
  .split > :global(.splitter) {
    pointer-events: auto;
  }
  .split-repos {
    top: var(--radius-sheet);
    bottom: calc(var(--sheet-inset) + var(--radius-sheet));
  }

  .sheet {
    position: relative;
    display: grid;
    grid-template-rows: minmax(0, 11fr) minmax(0, 9fr);
    min-height: 0;
    margin: 0 var(--sheet-inset) var(--sheet-inset) 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sheet);
    overflow: hidden;
    overflow: clip;
  }
  .log {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-bottom: 1px solid var(--rule);
  }
  .bottom {
    position: relative;
    display: grid;
    grid-template-columns: var(--changes-w) minmax(0, 1fr);
    min-height: 0;
  }
  .bottom > :global(.changes) {
    border-right: 1px solid var(--rule);
  }
  .diffcol {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .diffcol > :global(.diff) {
    flex: 1;
  }

  .state {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 24px var(--pad);
  }
  .skeleton {
    height: 12px;
    margin: 8px 0;
    border-radius: 4px;
    background: var(--hover);
  }

  /* A narrow window: repositories on top; the commit list, then the commit's files over its diff. */
  @media (max-width: 720px) {
    .gitlog-view {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
    }
    .gitlog-view > :global(.repos) {
      max-height: 220px;
    }
    .split {
      display: none;
    }
    .sheet {
      margin: 0 var(--sheet-inset) var(--sheet-inset);
    }
    .bottom {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(0, 2fr) minmax(0, 3fr);
    }
    .bottom > :global(.changes) {
      border-right: 0;
      border-bottom: 1px solid var(--rule);
    }
  }
</style>
