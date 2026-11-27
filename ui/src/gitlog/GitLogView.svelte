<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import DiffView from "../components/DiffView.svelte";
  import Splitter from "../components/Splitter.svelte";
  import {
    gitlogBlame,
    gitlogBlameCancel,
    gitlogBlameSetting,
    gitlogChanges,
    gitlogCombined,
    gitlogCombinedCancel,
    gitlogCombinedLeft,
    gitlogConfig,
    gitlogFile,
    gitlogLastFilter,
    gitlogPane,
    gitlogRepos,
    gitlogRevFile,
    gitlogWorktreeFile,
    openInEditor,
    setEditor,
  } from "../lib/api";
  import { openedText, type EditorConfig } from "../lib/editors";
  import type { IconTheme } from "../lib/icons";
  import type { Row } from "../lib/rows";
  import Changes from "./Changes.svelte";
  import CombinedChanges from "./CombinedChanges.svelte";
  import {
    CombinedLoad,
    combinedFirst,
    combinedPlan,
    enterStep,
    reconcileOpen,
    staleStep,
    combinedTitle,
    isCombined,
    itemsOf,
    orderRepos,
    EMPTY_COMBINED,
    type CombinedFile,
    type CombinedState,
    type CombinedLeft,
    combinedNotice,
    bannerWhenSettled,
  } from "./combined";
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
    beforeFrom,
    statusText,
    type ChangedFile,
    type DiffPane,
  } from "./changes";
  import { combinedBlameSides, commitBlameSides, compareBlameSides, withBlame, type BlameLine, type BlameRef, type BlameSides } from "./blame";
  import ComparePane from "./ComparePane.svelte";
  import { compareBanner, compareOpenKey, type CompareOpen } from "./compare";
  import Filters from "./Filters.svelte";
  import LogList from "./LogList.svelte";
  import { EMPTY_SELECTION, type Selection } from "./selection";
  import RepoPane from "./RepoPane.svelte";
  import { reposForLaunch } from "../lib/launch";
  import { DEFAULT_FILTER, debounce, queryGate, HISTORY_PREFIX, isStale, sameHistory, parseFilter, rowKey, type FilterState, type HistoryReq, type RowView } from "./log";
  import {
    DEFAULT_GITLOG_CONFIG,
    PANE_DEFAULTS,
    PANE_MIN,
    ROW_MIN,
    TOP_DEFAULT,
    clampTop,
    fitPanes,
    fitTop,
    topFrom,
    topMax,
    topToStored,
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
    if (mode === "compare") return comparePane?.focus();
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
    picked = [];
    selection = EMPTY_SELECTION;
    return true;
  }

  const COLLAPSED_KEY = "claudegate.gitlog.reposCollapsed";

  /**
   * Log or Compare: two modes of the one view. Compare sets the filter bar and commit list aside
   * (both stay mounted, with their query and selection) and shows the branch comparison instead.
   */
  let mode = $state<"log" | "compare">("log");
  let comparePane: ComparePane | undefined = $state();
  async function setMode(m: "log" | "compare") {
    if (m === mode) return;
    mode = m;
    // An "Open in editor" error belongs to the diff it was raised on, which the other mode does not show.
    openError = null;
    await tick();
    if (m === "compare") comparePane?.focus();
    else logList?.focusList();
  }

  let repos = $state<RepoView[] | null>(null);
  let reposError = $state<string | null>(null);
  let refreshing = $state(false);
  let cfg = $state<GitLogConfig>(structuredClone(DEFAULT_GITLOG_CONFIG));
  let preferred = $state<GitLogPanes>({ ...PANE_DEFAULTS });
  /** The top row's share of the sheet (null: the default), and the sheet's height it is a share of. */
  let topShare = $state<number | null>(null);
  let sheetH = $state(0);
  /**
   * Compare's own top share: its columns need more room than the log's Changes, so its diff row
   * starts lower (COMPARE_TOP) and is sized apart from the log's. Not stored.
   */
  const COMPARE_TOP = 0.38;
  let compareShare = $state<number | null>(null);
  const topH = $derived(sheetH > 0 ? fitTop(mode === "compare" ? (compareShare ?? COMPARE_TOP) : topShare, sheetH) : 0);
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
  /** Every picked commit, in list order (Combined Changes, Task 5). `selected` is the focused one. */
  let picked = $state<RowView[]>([]);
  let selection = $state<Selection>(EMPTY_SELECTION);
  /**
   * File History: the one file's commits replace the log (the filter is set aside, not lost).
   * It opens from a file row in Changes or Combined Changes and closes with ✕.
   */
  let history = $state<HistoryReq | null>(null);
  function openHistory(repo: string, path: string) {
    // Already showing it: nothing reloads, so nothing may be reset either.
    if (sameHistory(history, repo, path)) return;
    // No commit of the old list is carried over, and several commits cannot be picked here.
    selection = EMPTY_SELECTION;
    selected = null;
    picked = [];
    history = { repo, path };
    focusWhenLoaded();
  }
  // The keyboard goes to the new list (selecting its first commit) once its first page has landed,
  // not onto the rows of the list it replaces.
  let focusStage: 0 | 1 | 2 = 0;
  function focusWhenLoaded() {
    focusStage = 1;
  }
  $effect(() => {
    const busy = logBusy;
    if (focusStage === 1 && busy) focusStage = 2;
    else if (focusStage === 2 && !busy) {
      focusStage = 0;
      void tick().then(() => logList?.focusList());
    }
  });
  function closeHistory() {
    if (!history) return;
    selection = EMPTY_SELECTION;
    selected = null;
    picked = [];
    history = null;
    focusWhenLoaded();
  }
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

  // ---- Combined Changes: two or more picked commits -----------------------------------------
  // The Changes column then shows each file once across the selection. With one commit nothing
  // below runs and the single-commit path above is exactly what it was.
  const combinedOn = $derived(isCombined(picked));
  let combinedState = $state<CombinedState>(EMPTY_COMBINED);
  const combinedLoad = new CombinedLoad(
    (id, items) => gitlogCombined(id, items),
    (s) => onCombined(s),
    queryGate,
    (id) => void gitlogCombinedCancel(id).catch(() => {}),
  );
  const fetchCombined = leadingDebounce(() => {
    if (isCombined(picked)) void combinedLoad.start(itemsOf(picked));
  }, CHANGES_DEBOUNCE_MS);
  let combinedTree: CombinedChanges | undefined = $state();
  /** The combined file whose diff is open. */
  let cActive = $state<{ repo: string; file: CombinedFile } | null>(null);
  /** The whole `gitlogCombinedLeft` answer of the open file (null unless it took the merge path): a fallback in it overrides the file's mode in the banner. */
  let cLeft = $state<CombinedLeft | null>(null);
  /** Enter was pressed before the answer landed: open the first file when it does. */
  let enterWanted = false;
  /** The open file's read was cut short by a superseded query: read it again when the answer lands. */
  let cRetry = false;
  const combinedRepos = $derived(combinedState.view ? orderRepos(combinedState.view.repos, (repos ?? []).map((r) => r.id)) : []);
  const combinedHeading = $derived(combinedTitle(picked.length, new Set(picked.map((r) => r.repo)).size));
  /** The picked commits' subjects by sha, for the repo rows' tooltips. */
  const commitSubjects = $derived(Object.fromEntries(picked.map((r) => [r.sha, r.subject])));
  const repoColor = (id: string) => repos?.find((r) => r.id === id)?.color ?? 0;

  // Declared before the commit effect below: when the selection goes back to one commit, the
  // combined diff is cleared first, and then the single commit's files may open theirs.
  const pickKey = $derived(combinedOn ? picked.map(rowKey).join("\n") : "");
  let lastPick = "";
  $effect(() => {
    const k = pickKey;
    if (k === lastPick) return;
    lastPick = k;
    untrack(() => {
      enterWanted = false;
      if (!k) {
        fetchCombined.drop();
        combinedLoad.clear();
        if (cActive) {
          cActive = null;
          clearDiff();
        }
        cLeft = null;
        return;
      }
      combinedState = { ...combinedState, loading: true, error: null };
      fetchCombined();
    });
  });

  function onCombined(s: CombinedState) {
    combinedState = s;
    if (s.loading) return;
    if (s.error) {
      enterWanted = false;
      if (cActive) {
        cActive = null;
        clearDiff();
      }
      return;
    }
    if (!s.view) return;
    // The open file follows the selection first (also when Enter is waiting on this answer):
    // gone closes it, changed or cut short re-reads it, as it was keeps its diff.
    const cur = cActive;
    if (cur) {
      const again = orderRepos(s.view.repos, (repos ?? []).map((r) => r.id))
        .find((r) => r.repo === cur.repo)
        ?.files.find((f) => f.path === cur.file.path);
      const act = reconcileOpen(cur.file, again, cRetry);
      if (act === "close") {
        cActive = null;
        cRetry = false;
        clearDiff();
      } else if (act === "reread" && again) void showCombinedFile(cur.repo, again);
    }
    if (enterWanted) {
      enterWanted = false;
      openFirstCombined();
    }
  }

  const asChanged = (f: CombinedFile): ChangedFile => ({ path: f.path, status: f.status, from: f.from, added: null, removed: null });

  async function showCombinedFile(repo: string, f: CombinedFile, retried = false) {
    const g = fileGate.next();
    cRetry = false;
    cActive = { repo, file: f };
    cLeft = null;
    shownPane = null;
    diffLoading = true;
    diffError = null;
    openError = null;
    const qid = combinedLoad.id;
    let answer: CombinedLeft | null = null;
    const plan = combinedPlan(f);
    const readRev = (): Promise<FileView> =>
      f.beforeRef === null ? Promise.resolve<FileView>({ kind: "missing" }) : gitlogRevFile(repo, f.beforeRef, f.beforePath);
    const left = async (): Promise<FileView> => {
      if (plan.left === "missing") return { kind: "missing" };
      if (plan.left === "rev") return readRev();
      const ans = await gitlogCombinedLeft(qid, repo, f.last, f.path, [...f.shas].reverse(), queryGate.begin());
      answer = ans;
      if (ans.kind === "text") return { kind: "text", value: ans.value };
      // The backend could not take the selection apart after all: one diff across it.
      return readRev();
    };
    try {
      const [before, after] = await Promise.all([
        left(),
        plan.right === "missing" ? Promise.resolve<FileView>({ kind: "missing" }) : gitlogRevFile(repo, f.last, f.path),
      ]);
      if (!fileGate.isCurrent(g)) return;
      cLeft = answer;
      shownPane = diffPane(f.path, before, after);
      diffLoading = false;
    } catch (e) {
      if (!fileGate.isCurrent(g)) return;
      if (isStale(e)) {
        const step = staleStep({ superseded: combinedLoad.id !== qid, loading: combinedState.loading || fetchCombined.hasPending(), retried });
        if (step === "wait") {
          // The answer that is coming reads the file again (`cRetry`), even if its entry is unchanged.
          cRetry = true;
          return;
        }
        if (step === "retry") {
          // Against the answer now shown: the entry may have changed or be gone.
          const now = combinedRepos.find((r) => r.repo === repo)?.files.find((x) => x.path === f.path);
          if (now) return void showCombinedFile(repo, now, true);
          cActive = null;
          return clearDiff();
        }
      }
      diffError = String(e);
      diffLoading = false;
    }
  }

  function openFirstCombined() {
    if (!cActive) {
      const first = combinedFirst(combinedRepos);
      if (first) void showCombinedFile(first.repo, first.file);
    }
    void tick().then(() => combinedTree?.focus());
  }

  /** Enter with several commits picked: the first file of the combined tree, the keyboard in the tree. */
  function enterCombined() {
    const step = enterStep({ hasView: !!combinedState.view, loading: combinedState.loading, pending: fetchCombined.hasPending() });
    if (step === "open") return openFirstCombined();
    enterWanted = true;
    if (step === "flush") fetchCombined.flush();
    else if (step === "start") void combinedLoad.start(itemsOf(picked));
  }

  /** Back to the focused commit alone. */
  function clearSelection() {
    if (!selected) return;
    // The list owns the selection and tells us (`onselect`), so `picked` follows from it.
    logList?.selectOnly(rowKey(selected));
    logList?.focusList();
  }

  // The single commit's path. A combined selection pauses it; going back to one commit starts it again.
  const selKey = $derived(combinedOn ? "\u0001combined" : selected ? rowKey(selected) : null);
  let lastKey: string | null = null;
  $effect(() => {
    const k = selKey;
    if (k === lastKey) return;
    lastKey = k;
    untrack(() => (combinedOn ? leaveSingle() : commitChanged(selected)));
  });

  function leaveSingle() {
    commitGate.next();
    fetchChanges.drop();
    focusTreeWhenReady = false;
    inflightFor = null;
    filesLoading = false;
    clearDiff();
  }

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
    // In File History it follows the file's path at that commit (it may have been renamed since).
    const pick = pickFile(list, untrack(() => (history ? (r.file?.path ?? null) : (activeFile?.path ?? null))));
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
        // An added file has no before; every other status already knows its before path, so no lookup runs.
        f.status === "added"
          ? Promise.resolve<FileView>({ kind: "missing" })
          : gitlogFile(r.repo, r.sha, f.path, "before", beforeFrom(f)),
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
    if (combinedOn) return enterCombined();
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
    if (mode === "compare") comparePane?.focus();
    else logList?.focusList();
  }

  // ---- Compare: a file opened from the columns ---------------------------------------------
  // Its own diff state, so the log's open file is still there when Log comes back.
  let cmpOpen = $state<CompareOpen | null>(null);
  let cmpPane = $state<DiffPane | null>(null);
  let cmpLoading = $state(false);
  let cmpError = $state<string | null>(null);
  const cmpGate = createGate();

  function clearCompareDiff() {
    cmpGate.next();
    cmpOpen = null;
    cmpPane = null;
    cmpLoading = false;
    cmpError = null;
    if (mode === "compare") openError = null;
  }

  async function showCompareFile(o: CompareOpen) {
    const g = cmpGate.next();
    cmpOpen = o;
    cmpPane = null;
    cmpLoading = true;
    cmpError = null;
    openError = null;
    const f = o.file;
    const missing = Promise.resolve<FileView>({ kind: "missing" });
    try {
      const [before, after] = await Promise.all(
        o.kind === "side"
          ? [
              // The merge base at the path before a rename on this side, against this side's tip.
              f.status === "added" ? missing : gitlogRevFile(o.repo, o.base, f.from ?? f.path),
              f.status === "deleted" ? missing : gitlogRevFile(o.repo, o.tip, f.path),
            ]
          : [f.status === "added" ? missing : gitlogFile(o.repo, o.commit.sha, f.path, "before", beforeFrom(f)), gitlogFile(o.repo, o.commit.sha, f.path, "after")],
      );
      if (!cmpGate.isCurrent(g)) return;
      cmpPane = diffPane(f.path, before, after);
      cmpLoading = false;
    } catch (e) {
      if (!cmpGate.isCurrent(g)) return;
      cmpError = String(e);
      cmpLoading = false;
    }
  }

  /** A compare commit as the single-commit helpers take it. */
  const commitRow = (o: Extract<CompareOpen, { kind: "commit" }>): RowView => ({
    repo: o.repo,
    sha: o.commit.sha,
    time: o.commit.time,
    author: o.commit.author,
    email: "",
    subject: o.commit.subject,
    parents: o.commit.parents,
    source: "HEAD",
  });

  const compareRow = $derived<Row | null>(
    cmpOpen
      ? {
          key: `cmp:${compareOpenKey(cmpOpen)}`,
          path: cmpOpen.file.path,
          rel: cmpOpen.file.path,
          scope: cmpOpen.repo,
          added: cmpOpen.file.added ?? 0,
          removed: cmpOpen.file.removed ?? 0,
          newFile: cmpOpen.file.status === "added",
          decidedAt: null,
          waiting: false,
          rejectOutcome: null,
        }
      : null,
  );
  const compareExternal = $derived.by(() => {
    const pane = cmpPane;
    const o = cmpOpen;
    const f = o?.file;
    return {
      payload: pane?.kind === "diff" ? pane.payload : null,
      loading: cmpLoading,
      error: cmpError,
      banner: bannerWhenSettled(o?.kind === "side" ? compareBanner(o.side, o.pair) : null, { loading: cmpLoading, error: cmpError }),
      notice: !f
        ? { title: "Select a file to see its diff", body: "Click a file in the columns below, or press Enter on it." }
        : pane && pane.kind !== "diff"
          ? paneMessage(pane)
          : null,
      tag:
        f?.status === "deleted"
          ? { text: "Deleted", tone: "deleted" as const }
          : f && (f.status === "renamed" || f.status === "copied")
            ? { text: statusText(f), tone: "new" as const }
            : null,
    };
  });

  /** The row the shared diff header draws: a commit's file, never a review row. */
  const singleRow = $derived<Row | null>(
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
  const combinedRow = $derived<Row | null>(
    cActive
      ? {
          key: `${cActive.file.last}:${cActive.file.path}`,
          path: cActive.file.path,
          rel: cActive.file.path,
          scope: cActive.repo,
          added: 0,
          removed: 0,
          newFile: cActive.file.status === "added",
          decidedAt: null,
          waiting: false,
          rejectOutcome: null,
        }
      : null,
  );
  const diffRow = $derived(mode === "compare" ? compareRow : combinedOn ? combinedRow : singleRow);
  const cBanner = $derived(
    bannerWhenSettled(
      cActive ? combinedNotice(cActive.file, combinedRepos.find((r) => r.repo === cActive!.repo)?.subjects ?? {}, cLeft) : null,
      { loading: diffLoading, error: diffError },
    ),
  );
  const combinedExternal = $derived.by(() => {
    const pane = shownPane;
    const f = cActive?.file;
    return {
      payload: pane?.kind === "diff" ? pane.payload : null,
      loading: diffLoading,
      error: diffError,
      noStats: true,
      banner: cBanner,
      notice: !f
        ? { title: "Select a file to see its diff", body: "Click a file in Changes, or press Enter on it." }
        : pane && pane.kind !== "diff"
          ? paneMessage(pane)
          : null,
      tag:
        f?.status === "deleted"
          ? { text: "Deleted", tone: "deleted" as const }
          : f?.status === "renamed"
            ? { text: statusText(asChanged(f)), tone: "new" as const }
            : null,
    };
  });
  const singleExternal = $derived.by(() => {
    const pane = shownPane;
    const f = activeFile;
    const empty = {
      title: selected ? "Select a file to see its diff" : "Select a commit to see its changes",
      body: selected ? "Click a file in Changes, or press Enter on it." : "Pick one in the log below; its files appear in Changes.",
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

  /** The revisions each side of the open diff is blamed at; null when no file is open. */
  function blameSides(): BlameSides | null {
    if (mode === "compare") {
      const o = cmpOpen;
      if (!o) return null;
      if (o.kind === "commit") return commitBlameSides(commitRow(o), o.file);
      const b = compareBlameSides(o.repo, o.base, o.tip, o.file.path, o.file.from ?? null);
      return { original: o.file.status === "added" ? null : b.original, modified: o.file.status === "deleted" ? null : b.modified };
    }
    if (combinedOn) return cActive ? combinedBlameSides(cActive.repo, cActive.file, cLeft?.kind === "text") : null;
    return changesFor && activeFile ? commitBlameSides(changesFor, activeFile) : null;
  }

  /** Who last changed a line of one side, asked as the cursor rests on it; a superseded or failed read is no answer. */
  async function blameAt(side: "original" | "modified", line: number): Promise<BlameLine | null> {
    const sides = blameSides();
    const ref: BlameRef | null | undefined = sides?.[side];
    if (!ref) return null;
    try {
      return await gitlogBlame(queryGate.begin(), ref.repo, ref.rev, ref.path, line);
    } catch {
      return null;
    }
  }

  async function setBlame(on: boolean) {
    try {
      const c = await gitlogBlameSetting(on);
      cfg = withBlame(cfg, c);
    } catch (e) {
      onnotice(`Couldn't save the Blame setting: ${e}`);
    }
  }

  const blame = $derived({ on: cfg.blame, at: blameAt, toggle: (on: boolean) => void setBlame(on), cancel: () => void gitlogBlameCancel(queryGate.begin()).catch(() => {}) });
  const external = $derived({ ...(mode === "compare" ? compareExternal : combinedOn ? combinedExternal : singleExternal), blame });

  /** "Open in editor": the working-tree file, never written to. It may be gone since the commit. */
  async function openInWorktree(line: number, col: number, template: string | null = null): Promise<boolean> {
    const r = changesFor;
    const f = activeFile;
    const c = cActive;
    const o = cmpOpen;
    const target = mode === "compare" ? (o ? { repo: o.repo, path: o.file.path, source: undefined } : null) : combinedOn ? (c ? { repo: c.repo, path: c.file.path, source: undefined } : null) : r && f ? { repo: r.repo, path: f.path, source: r.source } : null;
    if (!target) return false;
    openError = null;
    try {
      // The commit's source picks the checkout it lives in (a worktree-only commit opens there);
      // a combined file opens in the repository's main checkout.
      const abs = await gitlogWorktreeFile(target.repo, target.path, target.source);
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
      topShare = topFrom(cfg.panes);
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

  function setTop(h: number) {
    if (sheetH <= 0) return;
    if (mode === "compare") compareShare = clampTop(h, sheetH) / sheetH;
    else topShare = clampTop(h, sheetH) / sheetH;
  }
  function storeTop() {
    if (mode === "compare" || topShare === null) return;
    void gitlogPane("top", topToStored(topShare)).then(
      (c) => (cfg = { ...cfg, panes: c.panes }),
      () => {},
    );
  }
  function resetTop() {
    if (mode === "compare") {
      compareShare = COMPARE_TOP;
      return;
    }
    topShare = TOP_DEFAULT;
    storeTop();
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
    onslash={() => (mode === "log" ? filters?.focusSearch() : undefined)}
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

  <div class="sheet" bind:clientHeight={sheetH}>
    <!-- The commit list comes first in the DOM, so Tab runs list, tree, diff; the top row is drawn above it. -->
    <div class="log">
      <div class="modes" role="group" aria-label="Git log mode">
        <button type="button" aria-pressed={mode === "log"} onclick={() => void setMode("log")}>Log</button>
        <button type="button" aria-pressed={mode === "compare"} onclick={() => void setMode("compare")}>Compare</button>
      </div>
      {#if mode === "compare" || comparePane}
        <div class="cmpwrap" class:hidden={mode !== "compare"}>
          <ComparePane
            bind:this={comparePane}
            shown={active && mode === "compare"}
            {configReady}
            repos={repos ?? []}
            {queryRepos}
            {cfg}
            {theme}
            open={cmpOpen}
            onconfig={(c) => (cfg = { ...cfg, comparePair: c.comparePair, compareRecent: c.compareRecent, compareFavorites: c.compareFavorites, compareMode: c.compareMode })}
            onopen={(o) => void showCompareFile(o)}
            onclear={clearCompareDiff}
            {onnotice}
          />
        </div>
      {/if}
      <div class="logbody" class:hidden={mode !== "log"}>
      <Filters bind:this={filters} bind:filter loading={logBusy} disabled={!!history} onrefresh={() => logList?.refresh()} onexit={() => logList?.focusList()} />
      {#if history}
        <div class="history" role="group" aria-label="File History">
          <span class="htext" role="status">{HISTORY_PREFIX}<code title={history.path}>{history.path}</code></span>
          <button type="button" class="hclose" aria-label="Close File History" title="Close File History (Esc)" onclick={closeHistory}>
            <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="m4 4 8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
          </button>
        </div>
      {/if}
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
          {history}
          onhistoryclose={closeHistory}
          multi={!history}
          onopen={openCommit}
          bind:selection
          onselect={(r, p) => ((selected = r), (picked = p))}
          onfocussearch={() => filters?.focusSearch()}
          onloading={(b) => (logBusy = b)}
          {onnotice}
          hidden={mode !== "log"}
        />
      {/if}
      </div>
    </div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="top" class:solo={mode === "compare"} style:height={topH > 0 ? `${topH}px` : `${TOP_DEFAULT * 100}%`} onkeydown={leaveBottom}>
      <!-- In Compare the diff stands alone (the files are in the columns below); the tree stays mounted, hidden, so its folders are as they were when Log comes back. -->
      <div class="chwrap" class:hidden={mode === "compare"}>
      {#if combinedOn}
        <CombinedChanges
          bind:this={combinedTree}
          title={combinedHeading}
          repos={combinedRepos}
          repoName={repoName}
          {repoColor}
          loading={combinedState.loading}
          error={combinedState.error}
          active={cActive ? { repo: cActive.repo, path: cActive.file.path } : null}
          {theme}
          loadingText="Reading {picked.length} commits…"
          onopenfile={(repo, f) => void showCombinedFile(repo, f)}
          onclear={clearSelection}
          canClear={!!selected}
          {commitSubjects}
          onescape={() => logList?.focusList()}
          onhistory={openHistory}
        />
      {:else}
        <Changes
          bind:this={changesTree}
          title={changesHeading}
          {files}
          loading={filesLoading}
          error={filesError}
          activePath={activeFile?.path ?? null}
          {theme}
          repo={changesFor?.repo ?? null}
          onopenfile={(f) => !filesLoading && void showFile(f)}
          onescape={() => logList?.focusList()}
          onhistory={openHistory}
        />
      {/if}
      </div>
      <div class="split split-changes" class:hidden={mode === "compare"}>
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
          root={mode === "compare" ? (cmpOpen?.repo ?? "") : combinedOn ? (cActive?.repo ?? "") : (changesFor?.repo ?? "")}
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
    {#if topH > 0}
      <Splitter
        direction="row"
        label="Resize diff and commit list"
        value={topH}
        min={ROW_MIN}
        max={topMax(sheetH)}
        top={topH - 1}
        onresize={setTop}
        oncommit={storeTop}
        onreset={resetTop}
      />
    {/if}
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
    display: flex;
    flex-direction: column;
    min-height: 0;
    margin: 0 var(--sheet-inset) var(--sheet-inset) 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sheet);
    overflow: hidden;
    overflow: clip;
  }
  /* Filters and the commit list: the bottom row, after the top row on screen though before it in the DOM. */
  .log {
    order: 2;
    flex: 1 1 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  /* Changes | diff: the top row, its height set from the splitter. */
  .top {
    order: 1;
    flex: none;
    position: relative;
    display: grid;
    grid-template-columns: var(--changes-w) minmax(0, 1fr);
    min-height: 0;
    border-bottom: 1px solid var(--rule);
  }
  .chwrap {
    display: contents;
  }
  .chwrap.hidden {
    display: none;
  }
  .chwrap > :global(.changes) {
    border-right: 1px solid var(--rule);
  }
  .top.solo {
    grid-template-columns: minmax(0, 1fr);
  }
  .split-changes.hidden,
  .cmpwrap.hidden,
  .logbody.hidden {
    display: none;
  }
  .logbody {
    display: contents;
  }
  .cmpwrap {
    flex: 1 1 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .modes {
    flex: none;
    display: inline-flex;
    align-self: flex-start;
    margin: 6px var(--pad) 0;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .modes button {
    height: 24px;
    padding: 0 12px;
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .modes button + button {
    border-left: 1px solid var(--control-border);
  }
  .modes button[aria-pressed="true"] {
    background: var(--active);
    color: var(--text);
    font-weight: 600;
  }
  .modes button:hover:not([aria-pressed="true"]) {
    background: var(--hover);
  }
  .modes button:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
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

  .history {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    padding: 4px var(--pad);
    border-bottom: 1px solid var(--rule);
    background: var(--hover);
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .htext {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .htext code {
    font-family: var(--font-mono);
    color: var(--text);
  }
  .hclose {
    flex: none;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 4px;
    color: var(--muted);
  }
  .hclose:hover {
    background: var(--active);
    color: var(--text);
  }
  .hclose:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
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

  /* A narrow window: repositories on top; then the commit's files over its diff, with the commit list below. */
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
    .top {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(0, 2fr) minmax(0, 3fr);
    }
    .top.solo {
      grid-template-rows: minmax(0, 1fr);
    }
    .chwrap > :global(.changes) {
      border-right: 0;
      border-bottom: 1px solid var(--rule);
    }
  }
</style>
