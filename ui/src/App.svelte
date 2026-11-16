<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import DiffView from "./components/DiffView.svelte";
  import FileTree from "./components/FileTree.svelte";
  import ProjectList from "./components/ProjectList.svelte";
  import ReviewTabs from "./components/ReviewTabs.svelte";
  import Splitter from "./components/Splitter.svelte";
  import {
    getConfig,
    getProject,
    listProjects,
    onChanged,
    openInEditor,
    setEditor,
    setPendingCount,
    usingMock,
    type DiffKind,
    type EditorConfig,
    type ProjectSummary,
    type ProjectView,
  } from "./lib/api";
  import { editorLabel, ZED } from "./lib/editors";
  import type { IconTheme } from "./lib/icons";
  import { actionFor, isTypingTarget, nextIndex } from "./lib/keys";
  import { basename, groupsFor, scopeLabel, selectionAfterList, sortProjects, type Row } from "./lib/rows";
  import { Rechecks } from "./lib/recheck";
  import {
    MIN,
    STORE_KEY as PANES_KEY,
    layout,
    load as loadPanes,
    loadCollapsed,
    maxFor,
    reset as resetPane,
    save as savePanes,
    toggleCollapsed,
    withWidth,
    type Pane,
    type Widths,
  } from "./lib/panes";
  import { relTime } from "./lib/time";
  import { collapseKeysToReveal, fileOrder, visibleItems } from "./lib/visible";

  const STORE_KEY = "claudegate.selectedProject";

  let projects = $state<ProjectSummary[]>([]);
  let loadingList = $state(true);
  let listError = $state<string | null>(null);
  let selected = $state<string | null>(null);
  let view = $state<ProjectView | null>(null);
  let viewError = $state<string | null>(null);
  let tab = $state<DiffKind>("pending");
  let mode = $state<"tree" | "list">("tree");
  let selectedRow = $state<Row | null>(null);
  let theme = $state<IconTheme>("dark");
  /** Collapsed folders and groups, keyed per tab; shared with FileTree so j/k see the same rows. */
  const collapsed = new SvelteSet<string>();
  let config = $state<EditorConfig>({ editor: ZED });
  let firstLine = $state(1);
  let openError = $state<string | null>(null);
  /** Bumped on every reload of the selected project so the open diff refreshes. */
  let viewVersion = $state(0);

  /** The widths the user chose; `panes` is what fits the window right now. */
  let preferred = $state<Widths>(readPanes());
  /** The Projects column shown as a rail of initials; its stored width is kept for expanding. */
  let projectsCollapsed = $state(readCollapsed());
  /** On only around a toggle, so dragging a splitter never animates. */
  let animatePanes = $state(false);
  let innerWidth = $state(typeof window === "undefined" ? 1200 : window.innerWidth);
  /** A narrow window stacks the projects on top, where a rail has no meaning. */
  const railed = $derived(projectsCollapsed && innerWidth > 720);
  const panes = $derived(layout(preferred, innerWidth, railed));

  function readStored(): string | null {
    try {
      return localStorage.getItem(PANES_KEY);
    } catch {
      return null;
    }
  }
  function readPanes(): Widths {
    return loadPanes(readStored());
  }
  function readCollapsed(): boolean {
    return loadCollapsed(readStored());
  }
  function setPane(pane: Pane, width: number) {
    preferred = withWidth(preferred, pane, width, innerWidth, railed);
  }
  function storePanes() {
    try {
      localStorage.setItem(PANES_KEY, savePanes(preferred, projectsCollapsed));
    } catch {}
  }
  let animTimer: ReturnType<typeof setTimeout> | undefined;
  function toggleProjects() {
    animatePanes = true;
    clearTimeout(animTimer);
    animTimer = setTimeout(() => (animatePanes = false), 200);
    projectsCollapsed = toggleCollapsed(projectsCollapsed);
    storePanes();
  }
  function resetPaneWidth(pane: Pane) {
    preferred = resetPane(preferred, pane, innerWidth);
    storePanes();
  }

  const current = $derived(projects.find((p) => p.root === selected) ?? null);
  const totalPending = $derived(projects.reduce((s, p) => s + p.pending, 0));
  const counts = $derived({
    pending: view?.pending.length ?? 0,
    accepted: view?.accepted.length ?? 0,
    rejected: view?.rejected.length ?? 0,
  });
  const groups = $derived(view ? groupsFor(view, tab) : []);
  /** The screen's one finding: what is waiting, where, and for how long. */
  const finding = $derived.by(() => {
    if (!view || view.missing) return "";
    const n = view.pending.length;
    if (n === 0) return "Nothing waiting for review";
    const parts = groupsFor(view, "pending").map((g) => `${g.rows.length} in ${basename(g.scope.root)}`);
    const oldest = view.pending
      .map((p) => p.capturedAt)
      .filter((t): t is string => !!t)
      .sort()[0];
    const where = parts.length > 1 ? ` · ${parts.join(", ")}` : "";
    const age = oldest ? ` · oldest ${relTime(oldest)}` : "";
    const w = view.pending.filter((p) => p.waiting).length;
    const unwritten = w > 0 ? ` · ${w} not written yet` : "";
    return `${n} waiting${where}${age}${unwritten}`;
  });
  const troubled = $derived(view ? view.scopes.filter((s) => s.error || s.shapeUnknown.length > 0) : []);

  // Responses can arrive out of order when events land in quick succession.
  let viewSeq = 0;

  /** Re-fetches a project when one of its "waiting for write" rows may have settled. */
  const rechecks = new Rechecks((root) => refresh(root));

  function refresh(root: string) {
    if (root === selected) void loadView(root);
    else void getProject(root).then(patchSummary, () => {});
  }

  async function loadView(root: string) {
    const seq = ++viewSeq;
    try {
      const v = await getProject(root);
      if (seq !== viewSeq || root !== selected) return;
      view = v;
      viewError = null;
      viewVersion++;
      patchSummary(v);
      if (selectedRow) {
        // Keep the selection on the fresh row object so its counts update; drop it if it is gone.
        const key = selectedRow.key;
        selectedRow = groupsFor(v, tab).flatMap((g) => g.rows).find((r) => r.key === key) ?? null;
      }
    } catch (e) {
      if (seq === viewSeq) viewError = String(e);
    }
  }

  function patchSummary(v: ProjectView) {
    projects = sortProjects(
      projects.map((p) =>
        p.root === v.root
          ? { ...p, missing: v.missing, pending: v.pending.length, nextRecheckAt: v.nextRecheckAt }
          : p,
      ),
    );
    rechecks.schedule(v.root, v.nextRecheckAt);
  }

  async function loadProjects() {
    try {
      projects = sortProjects(await listProjects());
      listError = null;
      rechecks.clear();
      for (const p of projects) rechecks.schedule(p.root, p.nextRecheckAt);
    } catch (e) {
      listError = String(e);
    } finally {
      loadingList = false;
    }
    let remembered: string | null = null;
    try {
      remembered = localStorage.getItem(STORE_KEY);
    } catch {}
    const next = selectionAfterList(projects, selected, remembered);
    switch (next.kind) {
      case "keep":
        await loadView(next.root);
        break;
      case "select":
        select(next.root);
        if (usingMock) void previewRow();
        break;
      case "clear":
        selected = null;
        view = null;
        break;
    }
  }

  function select(root: string) {
    viewError = null;
    if (root !== selected) {
      selected = root;
      view = null;
      selectedRow = null;
      try {
        localStorage.setItem(STORE_KEY, root);
      } catch {}
    }
    void loadView(root);
  }

  /** Dev preview only: `#row=<path suffix>` selects that pending row, for screenshots. */
  async function previewRow() {
    const want = new URLSearchParams(location.hash.slice(1)).get("row");
    if (!want || !selected) return;
    await loadView(selected);
    const row = view ? groupsFor(view, "pending").flatMap((g) => g.rows).find((r) => r.path.endsWith(want)) : undefined;
    if (row) selectedRow = row;
  }

  function setTab(t: DiffKind) {
    if (t === tab) return;
    tab = t;
    selectedRow = null;
  }

  async function open(row: Row, line: number, template: string | null = null) {
    openError = null;
    try {
      await openInEditor(row.path, line, template);
    } catch (e) {
      openError = String(e);
    }
  }

  async function pickEditor(template: string) {
    if (selectedRow) void open(selectedRow, firstLine, template);
    try {
      config = await setEditor(template);
    } catch (e) {
      openError = `Opened, but couldn't remember the choice: ${e}`;
    }
  }

  const lineFor = (row: Row) => (row.key === selectedRow?.key ? firstLine : 1);

  function onKey(e: KeyboardEvent) {
    if (e.defaultPrevented || e.repeat && e.key === "o") return;
    if (isTypingTarget(e.target as HTMLElement | null)) return;
    const a = actionFor(e);
    if (a?.type === "projects") {
      // Not while the Open-with menu is up, wherever focus is.
      if (document.querySelector('[role="menu"]')) return;
      toggleProjects();
      e.preventDefault();
      return;
    }
    if (!a || !view || view.missing) return;
    switch (a.type) {
      case "row": {
        const files = visibleItems(groups, { mode, showGroups: view.scopes.length > 1, tab, collapsed }).filter(
          (i) => i.kind === "file",
        );
        const i = nextIndex(
          files.findIndex((f) => f.row!.key === selectedRow?.key),
          files.length,
          a.dir,
        );
        if (i >= 0) selectedRow = files[i].row!;
        break;
      }
      case "pending": {
        const order = fileOrder(groupsFor(view, "pending"), mode);
        const cur = tab === "pending" ? order.findIndex((r) => r.key === selectedRow?.key) : -1;
        const i = nextIndex(cur, order.length, a.dir);
        if (i < 0) break;
        const row = order[i];
        for (const k of collapseKeysToReveal(row, "pending", collapsed)) collapsed.delete(k);
        tab = "pending";
        selectedRow = row;
        break;
      }
      case "open":
        if (selectedRow) void open(selectedRow, firstLine);
        break;
      case "tab":
        setTab(a.tab);
        break;
    }
    e.preventDefault();
  }

  // The tray tooltip shows the same total as the toolbar.
  $effect(() => {
    void setPendingCount(totalPending).catch(() => {});
  });

  onMount(() => {
    const mq = matchMedia("(prefers-color-scheme: light)");
    const sync = () => (theme = mq.matches ? "light" : "dark");
    sync();
    mq.addEventListener("change", sync);

    void loadProjects();
    getConfig().then(
      (c) => (config = c),
      () => {},
    );
    window.addEventListener("keydown", onKey);
    const off = onChanged(({ roots }) => {
      if (roots.length === 0) {
        void loadProjects();
        return;
      }
      for (const root of roots) refresh(root);
    });

    return () => {
      mq.removeEventListener("change", sync);
      window.removeEventListener("keydown", onKey);
      rechecks.clear();
      void off.then((f) => f());
    };
  });
</script>

<svelte:window bind:innerWidth />

<div class="app" class:animate={animatePanes} style:--sidebar-w="{panes.projects}px" style:--tree-w="{panes.tree}px">
  <header class="toolbar">
    <div class="brand">
      <svg class="mark" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
        <path
          d="M4 21V10a8 8 0 0 1 16 0v11M8.5 21v-9.5a3.5 3.5 0 0 1 7 0V21M2.5 21h19"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      <span class="wordmark">ClaudeGate</span>
    </div>
    <div class="status">
      {#if usingMock}<span class="hint">Fixture data</span>{/if}
      <span class="hint" title="This version shows changes. Accept and reject them in VS Code.">
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <rect x="3.5" y="7" width="9" height="6.5" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
        Read-only
      </span>
      <span class="total" class:none={totalPending === 0}>
        <span class="num">{totalPending}</span> pending
      </span>
    </div>
  </header>

  <ProjectList
    {projects}
    {selected}
    loading={loadingList}
    collapsed={railed}
    onselect={select}
    ontoggle={toggleProjects}
  />

  {#if !railed}
  <div class="split split-projects">
    <Splitter
      label="Resize projects"
      value={panes.projects}
      min={MIN.projects}
      max={maxFor(panes, "projects", innerWidth)}
      left={panes.projects}
      onresize={(w) => setPane("projects", w)}
      oncommit={storePanes}
      onreset={() => resetPaneWidth("projects")}
    />
  </div>
  {/if}

  <div class="sheet">
    <section class="review" aria-label="Changes">
      {#if listError}
        <div class="state">
          <p class="state-title">Couldn't load projects</p>
          <p class="state-body path">{listError}</p>
        </div>
      {:else if !current}
        <div class="state">
          <p class="state-body">{loadingList ? "" : "Select a project."}</p>
        </div>
      {:else if current.missing || view?.missing}
        <div class="title-row">
          <h1>{current.name}</h1>
          <p class="finding warn">Folder missing</p>
        </div>
        <div class="state">
          <p class="state-title">This folder is no longer on disk</p>
          <p class="state-body">It was moved or deleted, so ClaudeGate has nothing to show for it.</p>
          <p class="state-path">{current.root}</p>
        </div>
      {:else}
        <div class="title-row">
          <h1>{current.name}</h1>
          <p class="finding" class:has-pending={(view?.pending.length ?? 0) > 0}>{finding || "\u00a0"}</p>
        </div>
        <ReviewTabs {tab} {counts} {mode} ontab={setTab} onmode={(m) => (mode = m)} />
        {#each troubled as s (s.root)}
          <div class="notice" class:error={!!s.error}>
            {#if s.error}
              Couldn't read the session for <b>{scopeLabel(view!, s)}</b>: {s.error}
            {:else}
              <b>{scopeLabel(view!, s)}</b> has session fields this version doesn't know:
              <span class="path">{s.shapeUnknown.join(", ")}</span>
            {/if}
          </div>
        {/each}
        <div class="scroll">
          {#if viewError}
            <div class="state small">
              <p class="state-title">Couldn't load this project</p>
              <p class="state-body path">{viewError}</p>
            </div>
          {:else if !view}
            <div class="loading" aria-label="Loading">
              {#each [0, 1, 2, 3, 4, 5] as i (i)}
                <div class="skeleton" style:width="{[62, 48, 70, 40, 56, 66][i]}%" style:animation-delay="{i * 90}ms"></div>
              {/each}
            </div>
          {:else if groups.length === 0}
            <div class="state small">
              {#if tab === "pending"}
                <p class="state-title">All caught up</p>
                <p class="state-body">Claude's next edits to this project will show up here.</p>
              {:else}
                <p class="state-title">Nothing {tab} yet</p>
                <p class="state-body">Files you {tab === "accepted" ? "accept" : "reject"} in VS Code are listed here.</p>
              {/if}
            </div>
          {:else}
            <FileTree
              {groups}
              showGroups={view.scopes.length > 1}
              {tab}
              {mode}
              {theme}
              {collapsed}
              editorName={editorLabel(config.editor)}
              selectedKey={selectedRow?.key ?? null}
              onselect={(r) => (selectedRow = r)}
              onopen={(r) => void open(r, lineFor(r))}
            />
          {/if}
        </div>
      {/if}
    </section>

    <div class="split">
      <Splitter
        label="Resize file list"
        value={panes.tree}
        min={MIN.tree}
        max={maxFor(panes, "tree", innerWidth)}
        left={panes.tree - 1}
        onresize={(w) => setPane("tree", w)}
        oncommit={storePanes}
        onreset={() => resetPaneWidth("tree")}
      />
    </div>

    <DiffView
      row={selectedRow}
      kind={tab}
      root={selected ?? ""}
      version={viewVersion}
      {theme}
      {config}
      bind:firstLine
      {openError}
      onopen={() => selectedRow && void open(selectedRow, firstLine)}
      onpick={(t) => void pickEditor(t)}
      ondismiss={() => (openError = null)}
    />
  </div>
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
    grid-template-rows: var(--toolbar-h) minmax(0, 1fr);
    position: relative;
    height: 100vh;
    background: var(--frame);
  }

  /* Collapsing or expanding the projects slides the columns, briefly; a drag never animates. */
  .app.animate,
  .app.animate .sheet {
    transition: grid-template-columns 140ms var(--ease);
  }
  @media (prefers-reduced-motion: reduce) {
    .app.animate,
    .app.animate .sheet {
      transition: none;
    }
  }

  /* Splitters float over the edges they move; the wrapper takes no grid cell of its own. */
  .split {
    position: absolute;
    inset: 0;
    width: 0;
    pointer-events: none;
  }
  .split > :global(.splitter) {
    pointer-events: auto;
  }
  /* Clear of the sheet's rounded corners, where its edge is no longer a straight line. */
  .split-projects {
    top: calc(var(--toolbar-h) + var(--radius-sheet));
    bottom: calc(var(--sheet-inset) + var(--radius-sheet));
  }

  /* Toolbar and project list share the frame tint with no rule between them (the L). */
  .toolbar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 20px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .mark {
    color: var(--mark);
  }
  .wordmark {
    font-size: var(--fs-section);
    font-weight: 600;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 18px;
    font-size: var(--fs-small);
  }
  .hint {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
  }
  .total {
    font-weight: 500;
    color: var(--pending);
  }
  .total.none {
    color: var(--muted);
  }

  /* The sheet: inset from the window's right and bottom edges, 1px edge, no shadow. */
  .sheet {
    position: relative;
    display: grid;
    grid-template-columns: var(--tree-w) minmax(0, 1fr);
    min-height: 0;
    margin: 0 var(--sheet-inset) var(--sheet-inset) 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sheet);
    overflow: hidden;
  }

  .review {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    border-right: 1px solid var(--rule);
  }

  .title-row {
    padding: 18px var(--pad) 12px;
    flex: none;
  }
  h1 {
    font-size: var(--fs-title);
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.25;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .finding {
    margin-top: 3px;
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .finding.warn {
    color: var(--warning-ink);
    font-weight: 500;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .notice {
    margin: 10px 12px 0;
    padding: 8px 10px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    line-height: 1.45;
    color: var(--warning-ink);
    background: var(--warning-bg);
    user-select: text;
  }
  .notice.error {
    color: var(--error-ink);
    background: var(--error-bg);
  }
  .notice b {
    font-weight: 600;
  }

  .path {
    font-family: var(--font-mono);
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 24px var(--pad);
  }
  .state.small {
    padding-top: 20px;
  }
  .state-title {
    font-weight: 600;
  }
  .state-body {
    max-width: 300px;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .state-path {
    margin-top: 10px;
    padding: 4px 8px;
    border-radius: var(--radius-chip);
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--muted);
    background: var(--raised);
    user-select: text;
    word-break: break-all;
  }

  .loading {
    padding: 14px var(--pad);
  }
  .skeleton {
    height: 12px;
    margin: 0 0 12px;
    border-radius: 4px;
    background: var(--hover);
    animation: pulse 1.4s ease-in-out infinite both;
  }
  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }


  /* Narrow windows (a browser preview, a tiled window): one column, projects on top. */
  @media (max-width: 720px) {
    .app {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: var(--toolbar-h) auto minmax(0, 1fr);
    }
    .toolbar {
      padding: 0 16px;
    }
    .hint {
      display: none;
    }
    .app > :global(.projects) {
      max-height: 184px;
    }
    .sheet {
      grid-template-columns: minmax(0, 1fr);
      margin: 0 var(--sheet-inset) var(--sheet-inset);
    }
    .review {
      border-right: 0;
    }
    .sheet > :global(.diff),
    .split {
      display: none;
    }
  }

</style>
