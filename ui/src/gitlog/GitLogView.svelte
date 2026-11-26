<script lang="ts">
  import { onMount } from "svelte";
  import Splitter from "../components/Splitter.svelte";
  import { gitlogConfig, gitlogLastFilter, gitlogPane, gitlogRepos } from "../lib/api";
  import Filters from "./Filters.svelte";
  import LogList from "./LogList.svelte";
  import RepoPane from "./RepoPane.svelte";
  import { DEFAULT_FILTER, debounce, parseFilter, type FilterState, type RowView } from "./log";
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
  }: {
    /** On screen; the repository list is fetched again each time it comes back. */
    active: boolean;
    onnotice: (text: string) => void;
  } = $props();

  let repoPane: RepoPane | undefined = $state();
  /** Puts the keyboard in the Repositories pane (App calls it after switching to this view). */
  export function focusStart() {
    // The commit list when there is one to read; the Repositories pane when nothing is ticked yet.
    if (logList && queryRepos.length > 0) logList.focusList();
    else void repoPane?.focusStart();
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
  const shown = $derived((repos ?? []).filter((r) => isTicked(cfg.ticked, r.id)));
  const queryRepos = $derived(shown.map((r) => r.id));

  // The filters. They start from the stored ones and are written back (only that key) once the
  // stored ones were read, so the defaults never overwrite them.
  let filter = $state<FilterState>({ ...DEFAULT_FILTER });
  let filtersRead = false;
  let filters: Filters | undefined = $state();
  let logList: LogList | undefined = $state();
  let logBusy = $state(false);
  let selected = $state<RowView | null>(null);
  const saveFilter = debounce(() => void gitlogLastFilter($state.snapshot(filter)).catch(() => {}), 500);
  $effect(() => {
    JSON.stringify(filter);
    if (filtersRead) saveFilter();
  });

  /** Enter on a commit: Task 7 opens its first changed file in the diff here. */
  function openCommit(_row: RowView) {}

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
      preferred = panesFrom(cfg.panes);
      filter = parseFilter(cfg.lastFilter);
      filtersRead = true;
    } catch (e) {
      onnotice(`Couldn't read the Git log settings: ${e}`);
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
      {#if repos === null && !reposError}
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
    <div class="bottom">
      <section class="changes" aria-label="Changes">
        <p class="pane-title">Changes</p>
        <p class="state-body pad">Select a commit to see the files it changed.</p>
      </section>
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
      <section class="diff" aria-label="Diff">
        <p class="state-body pad">Select a file to see its diff.</p>
      </section>
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
  .changes {
    min-height: 0;
    border-right: 1px solid var(--rule);
    overflow: auto;
  }
  .diff {
    min-height: 0;
    overflow: auto;
  }
  .pane-title {
    padding: 12px var(--pad) 4px;
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--muted);
  }
  .pad {
    padding: 4px var(--pad);
  }
  .diff .pad {
    padding-top: 12px;
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
  .state-body {
    max-width: 420px;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }

  /* A narrow window: repositories on top, the commit list below; no Changes or diff yet. */
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
      grid-template-rows: minmax(0, 1fr);
      margin: 0 var(--sheet-inset) var(--sheet-inset);
    }
    .bottom {
      display: none;
    }
  }
</style>
