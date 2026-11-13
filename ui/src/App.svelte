<script lang="ts">
  import { onMount } from "svelte";
  import FileTree from "./components/FileTree.svelte";
  import ProjectList from "./components/ProjectList.svelte";
  import ReviewTabs from "./components/ReviewTabs.svelte";
  import {
    getProject,
    listProjects,
    onChanged,
    usingMock,
    type DiffKind,
    type ProjectSummary,
    type ProjectView,
  } from "./lib/api";
  import { iconFor, type IconTheme } from "./lib/icons";
  import { basename, groupsFor, scopeLabel, sortProjects, type Row } from "./lib/rows";
  import { relTime } from "./lib/time";

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
    return `${n} waiting${where}${age}`;
  });
  const troubled = $derived(view ? view.scopes.filter((s) => s.error || s.shapeUnknown.length > 0) : []);

  // Responses can arrive out of order when events land in quick succession.
  let viewSeq = 0;

  async function loadView(root: string) {
    const seq = ++viewSeq;
    try {
      const v = await getProject(root);
      if (seq !== viewSeq || root !== selected) return;
      view = v;
      viewError = null;
      patchSummary(v);
      if (selectedRow && !groupsFor(v, tab).some((g) => g.rows.some((r) => r.key === selectedRow!.key))) {
        selectedRow = null;
      }
    } catch (e) {
      if (seq === viewSeq) viewError = String(e);
    }
  }

  function patchSummary(v: ProjectView) {
    projects = sortProjects(
      projects.map((p) => (p.root === v.root ? { ...p, missing: v.missing, pending: v.pending.length } : p)),
    );
  }

  async function loadProjects() {
    try {
      projects = sortProjects(await listProjects());
      listError = null;
    } catch (e) {
      listError = String(e);
    } finally {
      loadingList = false;
    }
    if (!selected || !projects.some((p) => p.root === selected)) {
      let remembered: string | null = null;
      try {
        remembered = localStorage.getItem(STORE_KEY);
      } catch {}
      const pick =
        projects.find((p) => p.root === remembered) ?? projects.find((p) => p.pending > 0) ?? projects[0];
      if (pick) select(pick.root);
      else {
        selected = null;
        view = null;
      }
    } else {
      await loadView(selected);
    }
  }

  function select(root: string) {
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

  function setTab(t: DiffKind) {
    tab = t;
    selectedRow = null;
  }

  onMount(() => {
    const mq = matchMedia("(prefers-color-scheme: light)");
    const sync = () => (theme = mq.matches ? "light" : "dark");
    sync();
    mq.addEventListener("change", sync);

    void loadProjects();
    const off = onChanged(({ roots }) => {
      if (roots.length === 0) {
        void loadProjects();
        return;
      }
      for (const root of roots) {
        if (root === selected) void loadView(root);
        else void getProject(root).then(patchSummary, () => {});
      }
    });

    return () => {
      mq.removeEventListener("change", sync);
      void off.then((f) => f());
    };
  });

  function fileName(rel: string) {
    return rel.split("/").pop() ?? rel;
  }
</script>

<div class="app">
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

  <ProjectList {projects} {selected} loading={loadingList} onselect={select} />

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
              selectedKey={selectedRow?.key ?? null}
              onselect={(r) => (selectedRow = r)}
            />
          {/if}
        </div>
      {/if}
    </section>

    <main class="diff" aria-label="Diff">
      {#if selectedRow}
        <div class="diff-head">
          <img src={iconFor(fileName(selectedRow.rel), "file", false, theme)} alt="" width="16" height="16" draggable="false" />
          <span class="diff-path">
            {#if selectedRow.rel.includes("/")}<span class="diff-dir">{selectedRow.rel.slice(0, selectedRow.rel.lastIndexOf("/") + 1)}</span>{/if}<span class="diff-file">{fileName(selectedRow.rel)}</span>
          </span>
          {#if selectedRow.newFile}<span class="tag new">New file</span>{/if}
          <span class="diff-stats num">
            <span class="add">+{selectedRow.added}</span>
            <span class="del">−{selectedRow.removed}</span>
          </span>
        </div>
      {/if}
      <div class="placeholder">
        <p>{selectedRow ? "The side-by-side diff will appear here." : "Select a file to see its changes."}</p>
      </div>
    </main>
  </div>
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
    grid-template-rows: var(--toolbar-h) minmax(0, 1fr);
    height: 100vh;
    background: var(--frame);
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

  .diff {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .diff-head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 40px;
    padding: 0 var(--pad);
    border-bottom: 1px solid var(--rule);
    flex: none;
  }
  .diff-path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: var(--fs-small);
    user-select: text;
  }
  .diff-dir {
    color: var(--muted);
  }
  .diff-file {
    font-weight: 500;
  }
  .tag.new {
    padding: 0 6px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    line-height: 18px;
    color: var(--untracked);
    background: var(--raised);
  }
  .diff-stats {
    margin-left: auto;
    display: inline-flex;
    gap: 8px;
    font-size: var(--fs-small);
  }
  .add {
    color: var(--positive);
  }
  .del {
    color: var(--negative);
  }

  @media (max-width: 1000px) {
    .app {
      --sidebar-w: 184px;
      --tree-w: 340px;
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
    .diff {
      display: none;
    }
  }

  .placeholder {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 24px;
    text-align: center;
    color: var(--muted);
  }
</style>
