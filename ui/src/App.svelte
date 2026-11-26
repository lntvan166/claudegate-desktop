<script lang="ts">
  import { onMount, tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import ActionBar from "./components/ActionBar.svelte";
  import DiffView from "./components/DiffView.svelte";
  import FileTree from "./components/FileTree.svelte";
  import ProjectList from "./components/ProjectList.svelte";
  import ResultBar from "./components/ResultBar.svelte";
  import Settings from "./components/Settings.svelte";
  import CaptureChip from "./components/CaptureChip.svelte";
  import UpdateChip from "./components/UpdateChip.svelte";
  import ContextMenu from "./components/ContextMenu.svelte";
  import ReviewTabs from "./components/ReviewTabs.svelte";
  import Shortcuts from "./components/Shortcuts.svelte";
  import Splitter from "./components/Splitter.svelte";
  import GitLogView from "./gitlog/GitLogView.svelte";
  import {
    decide,
    addProject,
    getConfig,
    getProject,
    launchPath,
    onLaunch,
    pickFolder,
    projectInVsCode,
    getSettings,
    setExtraHide,
    setProjectExclude,
    takeBackExclude,
    setHookLog,
    setSetting,
    type SettingKey,
    listProjects,
    onChanged,
    openInEditor,
    setEditor,
    setPendingCount,
    addToWorkspace,
    removeFromWorkspace,
    setReadOnly,
    usingMock,
    type DiffKind,
    type EditorConfig,
    type ProjectSummary,
    type ProjectView,
  } from "./lib/api";
  import {
    bulkKeys,
    nextAfterDecision,
    isSettingsUnreadable,
    parseDecideError,
    recordIdFor,
    storeFailure,
    summarize,
    undoFor,
    undoKeys,
    type BulkAction,
    type DecideAction,
    type DecideError,
    type DecideReport,
    type DecideTarget,
    type ResultMessage,
    type Undo,
  } from "./lib/decide";
  import { editorLabel, openedText, ZED } from "./lib/editors";
  import type { IconTheme } from "./lib/icons";
  import { actionFor, decisionFor, diffKeyFor, DIFF_HIDDEN_REFUSAL, isUndoKey, isTypingTarget, isViewKey, nextIndex, READ_ONLY_REFUSAL, refKeyFor, viewKeyLabel, WAITING_REFUSAL } from "./lib/keys";
  import { writeClipboard } from "./lib/clipboard";
  import { firstShown, needsFocusMove, overlayShown, OVERLAY_SELECTOR } from "./lib/viewSwitch";
  import { clipboardText, formatRef, pasteHint, refRange, type LineRange } from "./lib/ref";
  import type { RowRef } from "./lib/rows";
  import { basename, groupsFor, restoreSelection, rowsOf, sameRow, scopeKind, scopeLabel, selectionAfterList, sortProjects, type Row } from "./lib/rows";
  import { elsewhereOf, elsewhereSuffix, inWorkspace, launchOwnership, splitWorkspace } from "./lib/workspace";
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
    stacked,
    toggleCollapsed,
    withWidth,
    type Pane,
    type Widths,
  } from "./lib/panes";
  import { headerTotal, projectLine } from "./lib/counts";
  import {
    COLLAPSED_KEY,
    MODE_KEY,
    loadCollapsedKeys,
    loadMode,
    readStore,
    saveCollapsedKeys,
    writeStore,
    type ListMode,
  } from "./lib/uiState";
  import { collapseKeysToReveal, fileOrder, firstVisibleRow, visibleItems } from "./lib/visible";
  import { filterBulk, filterGroups, normalizeFilter, stillShown } from "./lib/filter";
  import { addFolderFor, addLabel, findPendingRow, knownNotice, launchNotice, type LaunchTarget } from "./lib/launch";
  import { alreadyExcluded, excludedNotice, extensionGlob, folderGlob, hidePrompt } from "./lib/excludeSuggest";
  import {
    DEFAULT_SETTINGS,
    globMatcher,
    groupsBySession,
    hideExtra,
    protectedAmong,
    protectPrompt,
    removedMessage,
    settingsReloadOnChange,
    type SettingsView,
  } from "./lib/settings";

  const STORE_KEY = "claudegate.selectedProject";

  /** The header switch: the review, or the Git log. */
  type AppView = "review" | "gitlog";
  let appView = $state<AppView>("review");
  /** The Git log view is mounted the first time it is opened, never at startup, then kept. */
  let gitlogMounted = $state(false);
  let gitlogView: GitLogView | undefined = $state();
  /** What had the keyboard in the review when it was hidden, to give it back on return. */
  let reviewFocus: HTMLElement | null = null;
  function setView(v: AppView) {
    if (v === appView) return;
    if (v === "gitlog") {
      gitlogMounted = true;
      // Settings live in the review's sheet; Esc must not close them unseen.
      settingsOpen = false;
      rowMenu = null;
      // Nor answer a question nobody can see: a bulk or "looks like a secret" confirmation.
      actionBar?.cancel();
      protectAsk = null;
      const a = document.activeElement;
      reviewFocus = a instanceof HTMLElement && a !== document.body ? a : null;
    }
    appView = v;
    void tick().then(() => moveFocusFor(v));
  }
  /** Focus off whatever the switch just hid: into the Repositories pane, or back where the review left it. */
  function moveFocusFor(v: AppView) {
    if (v !== appView) return;
    const a = document.activeElement as HTMLElement | null;
    if (!needsFocusMove(a, !a || a === document.body)) return;
    if (v === "gitlog") {
      gitlogView?.focusStart();
      return;
    }
    const back = firstShown([reviewFocus]);
    if (back) back.focus();
    else if (fileTree) void fileTree.focusCurrent();
  }
  /** A menu or confirmation on screen (the hidden view's never count). */
  const overlayUp = () => overlayShown(document.querySelectorAll<HTMLElement>(OVERLAY_SELECTOR));

  let projects = $state<ProjectSummary[]>([]);
  let loadingList = $state(true);
  let listError = $state<string | null>(null);
  let selected = $state<string | null>(null);
  /** The project as fetched; `view` is it without the app-only hidden rows. */
  let rawView = $state<ProjectView | null>(null);
  let viewError = $state<string | null>(null);
  /** Every setting as it resolves for the selected project (`settingsFor` is that project). */
  let settings = $state<SettingsView>(DEFAULT_SETTINGS);
  let settingsFor = $state<string | null>(null);
  let settingsOpen = $state(false);
  let settingsButton: HTMLButtonElement | undefined = $state();
  /** The selected project's app-only hide patterns, once its settings have loaded. */
  const extraHide = $derived(settingsFor === selected ? settings.extraHide : []);
  /** The hide patterns of the repos nested in it, each for its own rows. */
  const nestedHide = $derived(settingsFor === selected ? (settings.nestedExtraHide ?? []) : []);
  const view = $derived(rawView ? hideExtra(rawView, extraHide, nestedHide) : null);
  const bySession = $derived(settings.groupBySession.value);
  /** The tab's groups: by scope, or by Claude session when that setting is on. */
  function groupsOf(v: ProjectView, kind: DiffKind) {
    return bySession ? groupsBySession(v, kind) : groupsFor(v, kind);
  }
  /** The tab's groups with the Pending filter applied (the other tabs are never filtered). */
  function shownGroups(v: ProjectView, kind: DiffKind) {
    const g = groupsOf(v, kind);
    return kind === "pending" ? filterGroups(g, v, filter) : g;
  }
  /** The protected globs for the selected project; the defaults until its settings have loaded. */
  const protectedGlobs = $derived(settingsFor === selected ? settings.protected.value : DEFAULT_SETTINGS.protected.value);
  const isProtected = $derived(globMatcher(view?.root ?? "", protectedGlobs));
  /** An accept of protected files waiting for its answer. */
  let protectAsk = $state<{ text: string; go: () => void; label: string; aria: string; lock: boolean } | null>(null);
  let protectEl: HTMLDivElement | undefined = $state();
  let tab = $state<DiffKind>("pending");
  /** The Pending filter box, per project: kept across refreshes and decisions, cleared on a switch. */
  let filterText = $state("");
  const filter = $derived(normalizeFilter(filterText));
  let reviewTabs: ReviewTabs | undefined = $state();
  let fileTree: FileTree | undefined = $state();
  // A row the filter hides must not stay selected: a/r would still decide a file nobody can see.
  $effect(() => {
    if (tab === "pending" && filter !== null && selectedRow && !stillShown(selectedRow, groups)) selectedRow = null;
  });
  let mode = $state<ListMode>(loadMode(readStore(MODE_KEY)));
  function setMode(m: ListMode) {
    mode = m;
    writeStore(MODE_KEY, m);
  }
  let selectedRow = $state<Row | null>(null);
  let theme = $state<IconTheme>("dark");
  /** Collapsed folders and groups, keyed per tab; shared with FileTree so j/k see the same rows. */
  const collapsed = new SvelteSet<string>(loadCollapsedKeys(readStore(COLLAPSED_KEY)));
  // Collapsed folders and groups survive a restart; their keys carry the scope's root.
  $effect(() => {
    writeStore(COLLAPSED_KEY, saveCollapsedKeys(collapsed));
  });
  let config = $state<EditorConfig>({ editor: ZED });
  let firstLine = $state(1);
  let openError = $state<string | null>(null);
  /** Bumped on every reload of the selected project so the open diff refreshes. */
  let viewVersion = $state(0);
  /** A decision is in flight. The UI runs one at a time; every action is disabled meanwhile. */
  let busy = $state(false);
  /** The result bar under the diff. */
  let result = $state<ResultMessage | null>(null);
  let resultSeq = 0;
  /** The `?` overlay. */
  let helpOpen = $state(false);
  let actionBar: ActionBar | undefined = $state();
  /** A short status said through the live region (and shown briefly), e.g. why a key did nothing. */
  let notice = $state<{ id: number; text: string } | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  function announce(text: string) {
    notice = { id: (notice?.id ?? 0) + 1, text };
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 3000);
  }
  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  let diffView: DiffView | undefined = $state();
  /** The right-click menu on a file row. */
  let rowMenu = $state<{ x: number; y: number; row: Row } | null>(null);

  /**
   * "Copy reference for Claude": `@path` (or `@path:start-end` for a selection) for `row`, relative
   * to the project's primary root, on the clipboard with a trailing space.
   */
  async function copyRef(row: Row, sel: LineRange | null = null) {
    if (!selected) return;
    // Line numbers only on Pending, where the diff's modified side is the file on disk.
    const ref = formatRef(row.path, selected, settings.copyRefPathStyle, refRange(tab, sel));
    try {
      await writeClipboard(clipboardText(ref));
      announce(`Copied ${ref} — paste in Claude with ${pasteHint(isMac)}`);
    } catch (e) {
      show({ text: `Couldn't copy the reference: ${e instanceof Error ? e.message : e}`, tone: "error" });
    }
  }
  /** The keyboard and header-button entry: the diff's selection, cursor line, or the file for the selected row. */
  function copySelectedRef(wholeFile: boolean, focusedOnly: boolean) {
    if (!selectedRow) {
      announce("Select a file to copy its reference");
      return;
    }
    void copyRef(selectedRow, wholeFile ? null : (diffView?.selection(focusedOnly) ?? null));
  }

  /** The backend refused for a reason the config does not show (its settings file is unreadable). */
  let settingsBlocked = $state<string | null>(null);

  /** The widths the user chose; `panes` is what fits the window right now. */
  let preferred = $state<Widths>(readPanes());
  /** The Projects column shown as a rail of initials; its stored width is kept for expanding. */
  let projectsCollapsed = $state(readCollapsed());
  /** On only around a toggle, so dragging a splitter never animates. */
  let animatePanes = $state(false);
  let innerWidth = $state(typeof window === "undefined" ? 1200 : window.innerWidth);
  /** A narrow window stacks the projects on top, where a rail has no meaning, and hides the diff. */
  const narrow = $derived(stacked(innerWidth));
  const railed = $derived(projectsCollapsed && !narrow);
  /** Narrow only: the selected file's diff covers the list, full height, until Back or Esc. */
  let narrowDiff = $state(false);
  const narrowDiffOpen = $derived(narrow && narrowDiff && !!selectedRow);
  /** Narrow only: settings, like a diff, take the full height. */
  const narrowFull = $derived(narrowDiffOpen || (narrow && settingsOpen));
  const diffShown = $derived(!narrow || narrowDiffOpen);
  // The file left the list (decided, or gone): back to the list, never an empty full-height pane.
  $effect(() => {
    if (!selectedRow) narrowDiff = false;
  });
  function openNarrowDiff() {
    if (narrow) narrowDiff = true;
  }
  function closeNarrowDiff() {
    narrowDiff = false;
  }
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
  /** The rail lists the workspace only; the other listed projects wait in the picker. */
  const split = $derived(splitWorkspace(projects, config.workspace));
  const totalPending = $derived(split.workspace.reduce((s, p) => s + p.pending, 0));
  /** Pending work in known projects outside the workspace: never in the total, never out of sight. */
  const elsewhere = $derived(elsewhereOf(split.known));
  const counts = $derived({
    pending: view?.pending.length ?? 0,
    accepted: view?.accepted.length ?? 0,
    rejected: view?.rejected.length ?? 0,
  });
  const groups = $derived(view ? shownGroups(view, tab) : []);
  const shownPendingRows = $derived(tab === "pending" ? groups.reduce((n, g) => n + g.rows.length, 0) : 0);
  /** The screen's one finding: what is pending, what isn't written yet, for how long. */
  const finding = $derived(
    !view || view.missing
      ? null
      : projectLine(
          view.pending,
          view.scopes.map((s) => ({ root: s.root, label: scopeLabel(view!, s), kind: scopeKind(s) })),
        ),
  );
  const troubled = $derived(view ? view.scopes.filter((s) => s.error) : []);
  /** Fields this version does not know, from every scope written by a newer ClaudeGate. */
  const unknownFields = $derived(view ? [...new Set(view.scopes.flatMap((s) => s.shapeUnknown))] : []);
  const readOnly = $derived(config.readOnly === true);
  /** Why decisions are off for this project, or null when they are on. */
  const lock = $derived.by((): { text: string; detail?: string; action?: "turnOff" | "retry" } | null => {
    if (unknownFields.length > 0) {
      return {
        text: "This project was written by a newer ClaudeGate. Update the app to make decisions here.",
        detail: `Unknown session fields: ${unknownFields.join(", ")}`,
      };
    }
    // The scope's own notice below says which session and why.
    if (troubled.length > 0) return { text: "This project's review log couldn't be read, so decisions are paused." };
    if (settingsBlocked) return { text: settingsBlocked, action: "retry" };
    if (readOnly) return { text: "Read-only mode is on", action: "turnOff" };
    return null;
  });
  const canDecide = $derived(!!view && !view.missing && !lock);
  /** What a/r and Shift+A/R say while decisions are off: the switch, or the lock's own reason. */
  const lockReason = $derived(lock ? (lock.action === "turnOff" ? READ_ONLY_REFUSAL : lock.text) : null);
  /** Undo and Retry are decisions too: not offered while decisions are off. */
  const shownResult = $derived(
    result && !canDecide ? { ...result, undo: undefined, retry: undefined } : result,
  );
  /** What the tab's bulk actions touch and count: see `bulkKeys`. */
  const bulk = $derived.by(() => {
    if (!view) return [];
    const keys = bulkKeys(view, tab);
    if (tab !== "pending" || filter === null) return keys;
    const shown = new Set(groups.flatMap((g) => g.rows.map((r) => r.path)));
    return filterBulk(keys, shown);
  });
  /** Clear on this tab: the older parent entries only its records hide, which show again after. */
  const clearReveals = $derived(
    view && tab !== "pending"
      ? {
          n: view.scopes.reduce((s, sc) => s + ((tab === "accepted" ? sc.revealedByClearAccepted : sc.revealedByClearRejected) ?? 0), 0),
          where: basename(view.root),
        }
      : undefined,
  );
  const rejectOutcomes = $derived(Object.fromEntries((view?.pending ?? []).map((p) => [p.path, p.rejectOutcome])));

  // Responses can arrive out of order when events land in quick succession.
  let viewSeq = 0;

  /** Re-fetches a project when one of its "waiting for write" rows may have settled. */
  const rechecks = new Rechecks((root) => refresh(root));

  function refresh(root: string) {
    if (root === selected) void loadView(root);
    else void getProject(root).then(patchSummary, () => {});
  }

  /**
   * Fetches and shows a project. Returns what it fetched even when a newer load superseded it, so a
   * caller acting on the result never reads the older `view`; null when the fetch failed.
   */
  async function loadView(root: string): Promise<ProjectView | null> {
    const seq = ++viewSeq;
    try {
      const v = await getProject(root);
      if (seq !== viewSeq || root !== selected) return v;
      rawView = v;
      viewError = null;
      viewVersion++;
      patchSummary(v);
      if (selectedRow) {
        // Keep the selection on the fresh row object so its counts update; drop it if it is gone.
        selectedRow = restoreSelection(shownGroups(hideExtra(v, extraHide, nestedHide), tab), selectedRow);
      }
      return v;
    } catch (e) {
      if (seq === viewSeq) viewError = String(e);
      return null;
    }
  }

  /**
   * The project's pending rows as its tab shows them, without the app-only hidden ones (the
   * core's summary leaves them out too). Before the project's settings have loaded the hide
   * patterns are unknown, so the summary's own count stands until the next list.
   */
  function shownPending(v: ProjectView, current: number): number {
    if (v.root !== settingsFor) return current;
    return hideExtra(v, settings.extraHide, settings.nestedExtraHide ?? []).pending.length;
  }

  function patchSummary(v: ProjectView) {
    projects = sortProjects(
      projects.map((p) =>
        p.root === v.root
          ? {
              ...p,
              missing: v.missing,
              pending: shownPending(v, v.countedPending ?? p.pending),
              ready: v.root === settingsFor ? shownReady(v) : (v.countedReady ?? p.ready),
              nextRecheckAt: v.nextRecheckAt,
            }
          : p,
      ),
    );
    rechecks.schedule(v.root, v.nextRecheckAt);
  }

  /** `shownPending` without the rows still waiting for their write. */
  function shownReady(v: ProjectView): number {
    return hideExtra(v, settings.extraHide, settings.nestedExtraHide ?? []).pending.filter((p) => !p.waiting).length;
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
    // Only a project on the rail stays selected; otherwise pick among the workspace.
    const shown = splitWorkspace(projects, config.workspace).workspace;
    const keep = selected && shown.some((p) => p.root === selected) ? selected : null;
    const next = keep ? ({ kind: "keep", root: keep } as const) : selectionAfterList(shown, null, remembered);
    switch (next.kind) {
      case "keep":
        await loadView(next.root);
        break;
      case "select":
        select(next.root);
        if (usingMock) void previewRow();
        // Dev preview only: `#settings` opens the Settings screen, for screenshots and the inspector.
        if (usingMock && new URLSearchParams(location.hash.slice(1)).has("settings")) openSettings();
        // Dev preview only: `#gitlog` opens the Git log view.
        if (usingMock && new URLSearchParams(location.hash.slice(1)).has("gitlog")) setView("gitlog");
        break;
      case "clear":
        selected = null;
        rawView = null;
        break;
    }
  }

  function select(root: string) {
    viewError = null;
    if (root !== selected) {
      selected = root;
      filterText = "";
      rawView = null;
      selectedRow = null;
      result = null;
      settingsBlocked = null;
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
    const row = view ? groupsOf(view, "pending").flatMap((g) => g.rows).find((r) => r.path.endsWith(want)) : undefined;
    if (row) selectedRow = row;
  }

  /** The filter box hands the keyboard to the list; with no rows to focus, it just lets go. */
  function leaveFilter() {
    if (fileTree) void fileTree.focusCurrent();
    else (document.activeElement as HTMLElement | null)?.blur();
  }

  function setTab(t: DiffKind) {
    if (t === tab) return;
    tab = t;
    // The new tab's first row on screen (after the filter), so its diff shows at once. Focus stays
    // where it was: a click on a tab does not pull the keyboard into the list.
    selectedRow = view ? firstVisibleRow(shownGroups(view, t), { mode, showGroups, tab: t, collapsed }) : null;
  }

  async function open(row: Row, line: number, template: string | null = null, col = 1) {
    openError = null;
    try {
      await openInEditor(row.path, line, col, template);
      announce(openedText(row.path, line, template ?? config.editor));
    } catch (e) {
      openError = String(e);
    }
  }

  /** Open the shown file where the user is looking: the diff's cursor, else its first change. */
  function openSelected(template: string | null = null) {
    if (!selectedRow) return;
    const t = diffView?.openTarget() ?? { line: firstLine, col: 1 };
    void open(selectedRow, t.line, template, t.col);
  }

  /** A row's ↗ or a double-click: the shown row opens where the diff points, any other at the top. */
  async function openRow(row: Row) {
    if (!sameRow(row, selectedRow) || !diffView) return open(row, 1);
    const t = await diffView.openTargetWhenReady();
    if (sameRow(row, selectedRow)) await open(row, t.line, null, t.col);
  }

  async function pickEditor(template: string) {
    openSelected(template);
    try {
      config = await setEditor(template);
    } catch (e) {
      openError = `Opened, but couldn't remember the choice: ${e}`;
    }
  }

  /** Selects the pending row `ref` (scope and key) in `v` (a fresh fetch), unfolding whatever hides it. */
  function selectPending(v: ProjectView | null, ref: RowRef) {
    if (!v || tab !== "pending") return;
    const row = restoreSelection(shownGroups(hideExtra(v, extraHide, nestedHide), "pending"), ref);
    if (!row) return;
    for (const k of collapseKeysToReveal(row, "pending", collapsed)) collapsed.delete(k);
    selectedRow = row;
  }

  function show(r: Omit<ResultMessage, "id">) {
    result = { ...r, id: ++resultSeq };
  }

  function report(
    root: string,
    action: DecideAction,
    target: DecideTarget,
    rep: DecideReport,
    since: string,
    decided?: { scope: string; path: string }[],
    note?: string,
  ) {
    const s = summarize(action, target, rep);
    const u = undoFor(action, target, rep, decided);
    show({
      text: s.text,
      tone: note && s.tone === "ok" ? "warn" : s.tone,
      note,
      details: rep.errors.length > 0 ? rep.errors : undefined,
      undo: u ? () => void runUndo(root, u, since) : undefined,
    });
  }

  async function failed(root: string, action: DecideAction, err: DecideError, retry: () => void) {
    if (err.kind === "store") {
      const f = storeFailure(action, err);
      show({ text: err.message, tone: "error", note: f.note, retry: f.retry ? retry : undefined });
      return;
    }
    if (err.kind === "readOnly") {
      // Refused for read-only. The settings file is the one cause the view cannot show; the switch,
      // a newer session shape and an unreadable session show in the banner once reloaded.
      if (isSettingsUnreadable(err)) {
        settingsBlocked = err.message;
        result = null;
        return;
      }
      try {
        config = await getConfig();
      } catch {}
      if (lock) {
        result = null;
        return;
      }
    }
    show({ text: err.message, tone: "error" });
  }

  /**
   * Runs one decision. `row` is the file decided from the row or the keyboard; when it is the
   * selected pending row, the next pending row is selected afterwards (auto-advance).
   * `reselect` selects that pending path afterwards (after an Undo brings it back).
   */
  async function runDecide(
    action: DecideAction,
    target: DecideTarget,
    opts: { row?: Row | null; reselect?: RowRef; note?: string; protectedOk?: boolean } = {},
  ) {
    const root = selected;
    if (busy || !root || !view) return;
    // Accepting a file that looks like a secret asks first, wherever the accept came from.
    if (action === "accept" && !opts.protectedOk) {
      const paths = target.kind === "file" ? [target.key] : (target.keys ?? []);
      const hits = protectedAmong(paths, view.root, protectedGlobs);
      if (hits.length > 0) {
        void askProtect(protectPrompt(hits), () => void runDecide(action, target, { ...opts, protectedOk: true }));
        return;
      }
    }
    // A capture whose write has not landed has nothing to decide yet; the controls say so too.
    if (target.kind === "file" && (action === "accept" || action === "reject")) {
      const item = view.pending.find((p) => p.path === target.key && p.scope === target.scope);
      if (item?.waiting) {
        announce(WAITING_REFUSAL);
        return;
      }
    }
    const advanceFrom =
      settings.autoAdvance.value && target.kind === "file" && tab === "pending" && opts.row && sameRow(opts.row, selectedRow) ? opts.row : null;
    const nextKey = advanceFrom ? nextAfterDecision(fileOrder(shownGroups(view, "pending"), mode), advanceFrom) : null;
    // Undo of an accept looks for a record stamped at or after this moment, never an older one.
    const since = new Date().toISOString();
    // A bulk or folder accept/reject: the confirmed rows, with their scopes, for its Undo.
    const scoped = target.kind === "all" ? target.scopedKeys : undefined;
    const confirmed = target.kind !== "file" ? target.keys : undefined;
    const decided =
      (scoped || confirmed) && (action === "accept" || action === "reject")
        ? view.pending
            .filter((p) =>
              scoped ? scoped.some((k) => sameRow(k, { scope: p.scope, key: p.path })) : confirmed!.includes(p.path),
            )
            .map((p) => ({ scope: p.scope, path: p.path }))
        : undefined;
    busy = true;
    try {
      const rep = await decide(root, action, target);
      if (root !== selected) return;
      const fresh = await loadView(root);
      // A failed decision leaves the row where it was: stay on it.
      if (nextKey && rep.done > 0) selectPending(fresh, nextKey);
      else if (opts.reselect && rep.done > 0) selectPending(fresh, opts.reselect);
      report(root, action, target, rep, since, decided, opts.note);
    } catch (e) {
      const err = parseDecideError(e);
      if (root !== selected) return;
      // Whatever the refusal, the list may be out of date (that is often why it was refused).
      await loadView(root);
      await failed(root, action, err, () => void runDecide(action, target, opts));
    } finally {
      busy = false;
    }
  }

  /**
   * Undoes a decision: exactly the files it decided, with explicit keys. An accept's records are
   * resolved against a fresh fetch, never `view` (a superseded reload can leave it a step behind).
   */
  async function runUndo(root: string, u: Undo, since: string) {
    if (busy || root !== selected) return;
    let keys: string[] = u.rows.map((r) => r.path);
    let missing: string[] = [];
    if (u.byPath) {
      busy = true;
      let fresh: ProjectView | null = null;
      try {
        fresh = await getProject(root);
      } catch {}
      busy = false;
      if (root !== selected) return;
      const found = fresh ? undoKeys(u, fresh.accepted, since) : { keys: [], missing: keys };
      if (found.keys.length === 0) {
        show({ text: "Couldn't find that accept to undo — use Revert in the Accepted tab", tone: "error" });
        return;
      }
      keys = found.keys;
      missing = found.missing;
    }
    const note =
      missing.length > 0
        ? `Couldn't find ${missing.length === 1 ? "1 accept" : `${missing.length} accepts`} to undo (${missing.map(basename).join(", ")}); use Revert in the Accepted tab.`
        : undefined;
    if (u.target.kind === "file") {
      await runDecide(u.action, { ...u.target, key: keys[0] }, { reselect: { scope: u.rows[0].scope, key: u.rows[0].path }, note });
    } else if (u.target.kind === "all" && !u.byPath) {
      // The decided rows, each in its own scope: the same path may be a row in two scopes.
      await runDecide(u.action, { kind: "all", keys, scopedKeys: u.rows.map((r) => ({ scope: r.scope, key: r.path })) }, { note });
    } else {
      await runDecide(u.action, u.target.kind === "all" ? { kind: "all", keys } : { ...u.target, keys }, { note });
    }
  }

  /** `keys` are the rows the user counted and confirmed; the backend decides no other. */
  function runBulk(a: BulkAction, keys: string[]) {
    // Each confirmed key with the scope of the row the user saw it in.
    const want = new Set(keys);
    const scopedKeys = view ? rowsOf(view, tab).filter((r) => want.has(r.key)).map((r) => ({ scope: r.scope, key: r.key })) : undefined;
    const all: DecideTarget = { kind: "all", keys, scopedKeys };
    const action: DecideAction =
      a === "acceptAll"
        ? "accept"
        : a === "rejectAll"
          ? "reject"
          : a === "revertAll"
            ? "revert"
            : a === "reapplyAll"
              ? "reapply"
              : tab === "accepted"
                ? "clearAccepted"
                : "clearRejected";
    void runDecide(action, all);
  }

  /** Adds a known project to the rail and selects it. Never changes what is captured. */
  async function addToWorkspaceAt(root: string): Promise<string | null> {
    try {
      // One root, add-only: the UI's copy of the list may be stale.
      config = await addToWorkspace(root);
      select(root);
      const name = projects.find((p) => p.root === root)?.name ?? basename(root);
      announce(`${name} added to your workspace`);
      return null;
    } catch (e) {
      return String(e);
    }
  }

  /**
   * Takes a project off the rail (view only); `stopCapture` also runs the Remove flow first. The
   * selection moves to the next project on the rail, or the one before, or nothing.
   */
  async function removeFromWorkspaceAt(root: string, stopCapture: boolean) {
    const project = projects.find((p) => p.root === root);
    const name = project?.name ?? basename(root);
    const rail = split.workspace;
    const i = rail.findIndex((p) => p.root === root);
    const next = rail[i + 1] ?? rail[i - 1] ?? null;
    try {
      // One root (and the nested roots it folds), remove-only: the UI's copy of the list may be stale.
      const r = await removeFromWorkspace(root, project?.nested ?? [], stopCapture);
      config = r.config;
      if (r.workspaceError) {
        // Capture did stop; only taking it off the rail failed. Say exactly that.
        await loadProjects();
        show({
          text: `${removedMessage(name, r.removed?.unregistered ?? false).replace(/\.?$/, ".")} It stays on your workspace, which couldn't be saved: ${r.workspaceError}`,
          tone: "warn",
        });
        return;
      }
      if (selected === root) {
        if (next) select(next.root);
        else {
          selected = null;
          rawView = null;
        }
      }
      if (r.removed) {
        await loadProjects();
        announce(removedMessage(name, r.removed.unregistered));
      } else {
        announce(`${name} removed from your workspace; it is still captured`);
      }
    } catch (e) {
      show({ text: `Couldn't remove ${name}: ${e}`, tone: "error" });
    }
  }

  /** Registers `path` in workspace-roots.json and the workspace, then selects it. Resolves to an error message. */
  async function addProjectAt(path: string): Promise<string | null> {
    try {
      const r = await addProject(path);
      config = await getConfig();
      await loadProjects();
      select(r.root);
      const name = projects.find((p) => p.root === r.root)?.name ?? basename(r.root);
      announce(r.already ? `${name} is already a project` : `${name} added; Claude's edits there are now captured`);
      return null;
    } catch (e) {
      return String(e);
    }
  }

  /**
   * `claudegate [PATH]` asked for a path no workspace project owns; the answer is the user's.
   * `known` is the known project outside the workspace that owns it, if one does.
   */
  let launchAsk = $state<(LaunchTarget & { known?: { root: string; name: string } }) | null>(null);

  /** Selects the project owning `t.path` (and the pending row, for a file); otherwise asks. */
  async function openLaunchTarget(t: LaunchTarget) {
    launchAsk = null;
    settingsOpen = false;
    setView("review");
    // A worktree scope can live anywhere, even inside another project's root; only a project's
    // view lists them, so roots and scopes are matched together, longest prefix wins.
    const views = await Promise.all(projects.map((p) => getProject(p.root).catch(() => null)));
    const own = launchOwnership(
      t.path,
      projects.map((p, i) => ({ root: p.root, scopes: views[i]?.scopes.map((s) => s.root) ?? [] })),
      (root) => projects.some((p) => p.root === root && inWorkspace(p, config.workspace)),
      t.canonical,
    );
    if (own.kind !== "workspace") {
      const p = own.kind === "known" ? projects.find((x) => x.root === own.root) : undefined;
      launchAsk = p ? { ...t, known: { root: p.root, name: p.name } } : t;
      return;
    }
    const owner = own.root;
    tab = "pending";
    select(owner);
    if (t.isDir) return;
    const v = await loadView(owner);
    if (!v || owner !== selected) return;
    const rows = groupsOf(hideExtra(v, extraHide, nestedHide), "pending").flatMap((g) => g.rows);
    const row = findPendingRow(t.path, rows) ?? (t.canonical ? findPendingRow(t.canonical, rows) : null);
    if (row) selectedRow = row;
  }

  async function addLaunchFolder() {
    const t = launchAsk;
    launchAsk = null;
    if (!t) return;
    if (t.known) {
      const err = await addToWorkspaceAt(t.known.root);
      if (err) show({ text: `Couldn't add ${t.known.name} to your workspace: ${err}`, tone: "error" });
      else await openLaunchTarget(t);
      return;
    }
    const err = await addProjectAt(addFolderFor(t));
    if (err) show({ text: `Couldn't add the project: ${err}`, tone: "error" });
  }

  async function changeReadOnly(on: boolean) {
    try {
      config = await setReadOnly(on);
      if (!on) settingsBlocked = null;
    } catch (e) {
      show({ text: `Couldn't change read-only mode: ${e}`, tone: "error" });
    }
  }

  async function onLock(action: "turnOff" | "retry") {
    if (action === "turnOff") await changeReadOnly(false);
    else settingsBlocked = null;
  }

  const showGroups = $derived(bySession || (view?.scopes.length ?? 0) > 1);

  /** Shows the inline "looks like a secret" question; `go` runs the accept once it is answered yes. */
  async function askProtect(
    text: string,
    go: () => void,
    how: { label: string; aria: string; lock: boolean } = { label: "Accept anyway", aria: "Confirm accept", lock: true },
  ) {
    protectReturn = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    protectAsk = { text, go, ...how };
    await tick();
    // Cancel is the safe default.
    protectEl?.querySelector<HTMLButtonElement>("[data-cancel]")?.focus();
  }
  let protectReturn: HTMLElement | null = null;
  function answerProtect(yes: boolean) {
    const a = protectAsk;
    protectAsk = null;
    if (yes) a?.go();
    else protectReturn?.focus();
  }
  function protectKeys(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      answerProtect(false);
      return;
    }
    if (e.key !== "Tab" || !protectEl) return;
    const b = [...protectEl.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    const i = b.indexOf(document.activeElement as HTMLButtonElement);
    b[e.shiftKey ? (i <= 0 ? b.length - 1 : i - 1) : i === b.length - 1 ? 0 : i + 1]?.focus();
    e.preventDefault();
  }
  // Another project, another tab or decisions turned off: the question no longer applies.
  $effect(() => {
    void selected;
    void tab;
    if (!canDecide) protectAsk = null;
  });
  $effect(() => {
    void selected;
    void tab;
    return () => (protectAsk = null);
  });

  async function loadSettings(root: string | null) {
    try {
      const s = await getSettings(root);
      if (root !== selected) return;
      settings = s;
      settingsFor = root;
    } catch {}
  }
  $effect(() => {
    void loadSettings(selected);
  });

  /** Saves through `save`, keeps what it returns, and reloads what the change affects. */
  async function applySettings(save: () => Promise<SettingsView>, what: string, reload = false): Promise<boolean> {
    try {
      const s = await save();
      settings = s;
      settingsFor = selected;
      if (s.warning) show({ text: s.warning, tone: "warn" });
      if (reload) {
        if (selected) await loadView(selected);
        void loadProjects();
      }
      return true;
    } catch (e) {
      show({ text: `Couldn't change ${what}: ${e}`, tone: "error" });
      // The refusal may come from state the screen no longer matches (VS Code took the project
      // over, read-only turned on elsewhere): show what is true now.
      void loadSettings(selected);
      if (reload) {
        if (selected) void loadView(selected);
        void loadProjects();
      }
      return false;
    }
  }
  const SETTING_NAMES: Record<SettingKey, string> = {
    autoAdvance: "auto-advance",
    groupBySession: "grouping",
    pendingAgeMinDays: "the age setting",
    maxAttached: "the worktree limit",
    protected: "the secret file patterns",
    copyRefPathStyle: "the reference path style",
  };
  const changeSetting = (key: SettingKey, value: unknown) =>
    applySettings(() => setSetting(key, value, selected), SETTING_NAMES[key], key === "maxAttached");
  const changeHookLog = (on: boolean) => applySettings(() => setHookLog(on, selected), "the hook log");
  // Both change which rows show, so the project and every count reload.
  const changeExtraHide = (patterns: string[]) =>
    selected ? applySettings(() => setExtraHide(selected!, patterns), "the hidden patterns", true) : Promise.resolve(false);
  /** A nested repo's own hide patterns, edited from its parent's Settings. */
  const changeNestedHide = (root: string, patterns: string[]) =>
    selected ? applySettings(() => setExtraHide(root, patterns, selected!), "the hidden patterns", true) : Promise.resolve(false);
  const takeBack = () =>
    selected ? applySettings(() => takeBackExclude(selected!), "the exclude list", true) : Promise.resolve(false);
  const changeExclude = (globs: string[] | null) =>
    selected ? applySettings(() => setProjectExclude(selected!, globs), "the exclude list", true) : Promise.resolve(false);

  /** The context menu's hide items for `row`, from its extension and its folder (null hides one). */
  function hideItems(row: Row) {
    if (tab !== "pending") return [];
    const out: { id: string; label: string }[] = [];
    const ext = extensionGlob(row.rel);
    if (ext) out.push({ id: `hide:${ext}`, label: `Hide every ${ext.slice(4)} file from review` });
    const dir = folderGlob(row.path, row.scope);
    if (dir) out.push({ id: `hide:${dir}`, label: `Hide everything under ${dir.slice(3, -3)}/ from review` });
    return out;
  }

  /** Asks before adding `glob` to the project's exclude; nothing is decided, only hidden. */
  async function askHide(glob: string) {
    if (!selected || !view) return;
    if (lockReason) {
      announce(lockReason);
      return;
    }
    if (settingsFor !== selected) await loadSettings(selected);
    if (!view || settingsFor !== selected) {
      announce("Couldn't read this project's settings.");
      return;
    }
    if (settings.exclude.value.includes(glob)) {
      announce(alreadyExcluded(glob));
      return;
    }
    if (settings.excludeOwner === "managedByVsCode") {
      announce("Managed by VS Code: a VS Code window wrote its own exclude list for this project, so the app no longer changes it. Take it back in Settings.");
      return;
    }
    if (settings.excludeOwner !== "owned") {
      announce("This project's exclude list is set by VS Code. Change it there, or hide more files in Settings.");
      return;
    }
    const match = globMatcher(view.root, [glob]);
    const n = view.pending.filter((p) => match(p.path)).length;
    await askProtect(hidePrompt(glob, n), () => void hideGlob(glob), { label: "Hide from review", aria: "Confirm hide", lock: false });
  }

  async function hideGlob(glob: string) {
    if (!selected) return;
    // The writer Settings uses: the effective list plus the glob.
    if (await changeExclude([...settings.exclude.value, glob])) announce(excludedNotice(glob));
  }

  function openSettings() {
    setView("review");
    settingsOpen = true;
    void loadSettings(selected);
  }
  async function closeSettings() {
    settingsOpen = false;
    await tick();
    settingsButton?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (e.defaultPrevented || e.repeat && e.key === "o") return;
    // The "not a ClaudeGate project" prompt answers Esc like every other popover.
    if (e.key === "Escape" && launchAsk && !document.querySelector('[role="menu"], [data-confirm]')) {
      e.preventDefault();
      launchAsk = null;
      return;
    }
    // Ctrl+Shift+G: Review ⇄ Git log, from anywhere but a menu or a confirmation.
    if (isViewKey(e, isMac)) {
      e.preventDefault();
      if (e.repeat || overlayUp()) return;
      setView(appView === "review" ? "gitlog" : "review");
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === ",") {
      e.preventDefault();
      if (settingsOpen) void closeSettings();
      else openSettings();
      return;
    }
    // The Git log view has its own keys; the review's are off while it is up. `?` still shows them.
    if (appView === "gitlog") {
      if (e.key === "?" && !e.ctrlKey && !e.metaKey && !e.altKey && !isTypingTarget(e.target as HTMLElement | null)) {
        e.preventDefault();
        helpOpen = !helpOpen;
      }
      return;
    }
    // The settings screen answers Esc itself; review keys are off while it is open.
    if (settingsOpen) {
      if (e.key === "Escape" && !document.querySelector('[role="menu"], [data-confirm]')) {
        e.preventDefault();
        void closeSettings();
      }
      return;
    }
    // Esc leaves the narrow full-height diff, wherever focus is (Monaco included), unless a menu
    // or a confirmation is up: Esc answers those first.
    if (e.key === "Escape" && narrow && narrowDiff && !document.querySelector('[role="menu"], [data-confirm]')) {
      e.preventDefault();
      closeNarrowDiff();
      return;
    }
    if (helpOpen) return; // the overlay handles its own keys
    // Copy reference works inside Monaco (a read-only diff) but not while typing in a field.
    const ref = refKeyFor(e, isMac);
    if (ref) {
      const t = e.target as HTMLElement | null;
      const inDiff = !!t?.closest?.(".monaco-editor");
      if (e.repeat || (isTypingTarget(t) && !inDiff) || document.querySelector("[data-confirm]")) return;
      e.preventDefault();
      // Keyboard: only a selection in a focused editor counts; from the tree it is the whole file.
      copySelectedRef(ref === "file", true);
      return;
    }
    if (isTypingTarget(e.target as HTMLElement | null)) return;
    // Ctrl/Cmd+Z: the result bar's Undo, only while it offers one and nothing else is asking.
    if (isUndoKey(e, isMac)) {
      const undo = shownResult?.undo;
      if (!undo || busy || e.repeat || document.querySelector('[role="menu"], [data-confirm]')) return;
      e.preventDefault();
      undo();
      return;
    }
    // Alt+F5 / Alt+Z from the list: the diff's own keys (Monaco answers them itself when focused).
    const dk = diffKeyFor(e);
    if (dk) {
      if (!selectedRow || !diffShown || document.querySelector('[role="menu"], [data-confirm]')) return;
      e.preventDefault();
      if (dk.type === "change") diffView?.goToChange(dk.dir);
      else diffView?.toggleWrap();
      return;
    }
    const d = decisionFor(e, {
      tab,
      typing: false,
      menuOpen: !!document.querySelector('[role="menu"], [data-confirm]'),
      readOnly: !!lock,
      lockReason: lockReason ?? undefined,
      busy: busy || e.repeat,
      selectedKey: selectedRow?.key ?? null,
      selectedWaiting: selectedRow?.waiting ?? false,
      diffHidden: !diffShown,
    });
    if (d && "refused" in d) {
      e.preventDefault();
      announce(d.refused);
      return;
    }
    if (d && selectedRow && canDecide) {
      e.preventDefault();
      const row = selectedRow;
      void runDecide(d.action, { kind: "file", scope: row.scope, key: row.path }, { row });
      return;
    }
    const a = actionFor(e);
    if (a?.type === "help") {
      helpOpen = !helpOpen;
      e.preventDefault();
      return;
    }
    if (a?.type === "bulk") {
      if (document.querySelector('[role="menu"], [data-confirm]') || !view || view.missing || tab !== "pending") return;
      e.preventDefault();
      if (lockReason) announce(lockReason);
      else if (bulk.length === 0) announce("Nothing to decide");
      else actionBar?.trigger(a.action);
      return;
    }
    if (a?.type === "filter") {
      if (document.querySelector('[role="menu"], [data-confirm]') || !view || view.missing || narrowDiffOpen) return;
      e.preventDefault();
      setTab("pending");
      void tick().then(() => reviewTabs?.focusFilter());
      return;
    }
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
        const files = visibleItems(groups, { mode, showGroups, tab, collapsed }).filter(
          (i) => i.kind === "file",
        );
        const i = nextIndex(
          files.findIndex((f) => sameRow(f.row, selectedRow)),
          files.length,
          a.dir,
        );
        if (i >= 0) selectedRow = files[i].row!;
        break;
      }
      case "pending": {
        const order = fileOrder(shownGroups(view, "pending"), mode);
        const cur = tab === "pending" ? order.findIndex((r) => sameRow(r, selectedRow)) : -1;
        const i = nextIndex(cur, order.length, a.dir);
        if (i < 0) break;
        const row = order[i];
        for (const k of collapseKeysToReveal(row, "pending", collapsed)) collapsed.delete(k);
        tab = "pending";
        selectedRow = row;
        break;
      }
      case "open":
        openSelected();
        break;
      case "tab":
        setTab(a.tab);
        break;
    }
    e.preventDefault();
  }

  // The tray tooltip shows the same total as the toolbar.
  $effect(() => {
    void setPendingCount(totalPending, elsewhere.pending).catch(() => {});
  });

  /**
   * Nothing in the window scrolls sideways except inside Monaco. A WebView without `overflow: clip`
   * still lets focus() and scrollIntoView() scroll a clipped box, which slid the review column
   * under the window's left edge; undo any such scroll as it happens.
   */
  function pinScroll(e: Event) {
    const el = e.target;
    if (!(el instanceof HTMLElement) || el.closest(".monaco-editor")) return;
    if (el.scrollLeft !== 0) el.scrollLeft = 0;
  }
  function pinWindow() {
    if (window.scrollX !== 0 || window.scrollY !== 0) window.scrollTo(0, 0);
  }

  onMount(() => {
    document.addEventListener("scroll", pinScroll, true);
    window.addEventListener("scroll", pinWindow);
    const mq = matchMedia("(prefers-color-scheme: light)");
    const sync = () => (theme = mq.matches ? "light" : "dark");
    sync();
    mq.addEventListener("change", sync);

    // The config first: it says which projects are on the rail, so the first pick is one of them.
    const listLoaded = getConfig()
      .then(
        (c) => (config = c),
        () => {},
      )
      .then(() => loadProjects());
    window.addEventListener("keydown", onKey);
    const off = onChanged(({ roots }) => {
      // A takeover, a config change or the selected project's own change can alter what Settings shows.
      if (settingsReloadOnChange(roots, selected)) void loadSettings(selected);
      if (roots.length === 0) {
        void loadProjects();
        return;
      }
      for (const root of roots) refresh(root);
    });

    // A launch path: asked for once (the first event can fire before this listener exists), then
    // heard for every later launch. It waits for the project list so there is something to match.
    const offLaunch = onLaunch((t) => void listLoaded.then(() => openLaunchTarget(t)));
    void listLoaded
      .then(() => launchPath())
      .then(async (ts) => {
        for (const t of ts) await openLaunchTarget(t);
      }, () => {});

    return () => {
      void offLaunch.then((f) => f());
      mq.removeEventListener("change", sync);
      document.removeEventListener("scroll", pinScroll, true);
      window.removeEventListener("scroll", pinWindow);
      window.removeEventListener("keydown", onKey);
      rechecks.clear();
      void off.then((f) => f());
    };
  });
</script>

<svelte:window bind:innerWidth />

{#snippet protectBar()}
  {#if protectAsk}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="protect" role="group" aria-label={protectAsk.aria} data-confirm bind:this={protectEl} onkeydown={protectKeys}>
      {#if protectAsk.lock}
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <rect x="3.5" y="7" width="9" height="6.5" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
      {/if}
      <p class="protect-text">{protectAsk.text}</p>
      <div class="protect-buttons">
        <button class="protect-btn go" disabled={busy} onclick={() => answerProtect(true)}>{protectAsk.label}</button>
        <button class="protect-btn" data-cancel onclick={() => answerProtect(false)}>Cancel</button>
      </div>
    </div>
  {/if}
{/snippet}

{#if rowMenu}
  {@const m = rowMenu}
  <ContextMenu
    x={m.x}
    y={m.y}
    items={[{ id: "copy", label: "Copy reference for Claude", hint: isMac ? "⌘⌥C" : "Ctrl+Alt+C" }, ...hideItems(m.row)]}
    onpick={(id) => {
      rowMenu = null;
      if (id === "copy") void copyRef(m.row);
      else if (id.startsWith("hide:")) void askHide(id.slice(5));
    }}
    onclose={() => (rowMenu = null)}
  />
{/if}

{#if helpOpen}
  <Shortcuts editorName={editorLabel(config.editor)} onclose={() => (helpOpen = false)} />
{/if}

<!-- The live region: always in the DOM so screen readers hear each new status. -->
<div class="notice-region" role="status" aria-live="polite">
  {#if notice}
    {#key notice.id}<p class="toast">{notice.text}</p>{/key}
  {/if}
</div>

{#if launchAsk}
  <div class="launch-ask" role="alertdialog" aria-label={launchAsk.known ? "Not in your workspace" : "Not a ClaudeGate project"}>
    {#if launchAsk.known}
      <p class="path">{knownNotice(launchAsk, launchAsk.known.name)}</p>
      <button class="protect-btn go" onclick={() => void addLaunchFolder()}>Add to workspace</button>
    {:else}
      <p class="path">{launchNotice(launchAsk)}</p>
      {#if launchAsk.exists}
        <button class="protect-btn go" onclick={() => void addLaunchFolder()}>{addLabel(launchAsk)}</button>
      {/if}
    {/if}
    <button class="protect-btn" onclick={() => (launchAsk = null)}>Dismiss</button>
  </div>
{/if}

<div class="app" class:animate={animatePanes} class:narrow-diff={narrowFull} class:gitlog={appView === "gitlog"} style:--sidebar-w="{panes.projects}px" style:--tree-w="{panes.tree}px">
  <header class="toolbar">
    <div class="brand">
      <img class="mark" src="/claudegate.png" width="20" height="20" alt="" draggable="false" />
      <span class="wordmark">ClaudeGate</span>
    </div>
    <div class="switch" role="group" aria-label="View" title="Switch view ({viewKeyLabel(isMac)})">
      <button aria-pressed={appView === "review"} class:on={appView === "review"} onclick={() => setView("review")}>Review</button>
      <button aria-pressed={appView === "gitlog"} class:on={appView === "gitlog"} onclick={() => setView("gitlog")}>Git log</button>
    </div>
    <div class="status">
      {#if usingMock}<span class="hint">Fixture data</span>{/if}
      <UpdateChip />
      <CaptureChip {readOnly} projectCount={loadingList || listError ? null : projects.length} />
      <button
        class="chip"
        class:on={readOnly}
        aria-pressed={readOnly}
        title={readOnly ? "Read-only mode is on: decisions are blocked. Click to turn it off." : "Turn on read-only mode to block every decision."}
        onclick={() => void changeReadOnly(!readOnly)}
      >
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <rect x="3.5" y="7" width="9" height="6.5" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.3" />
          {#if readOnly}
            <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.3" />
          {:else}
            <path d="M5.5 7V5a2.5 2.5 0 0 1 4.9-.7" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
          {/if}
        </svg>
        <span class="chip-label">Read-only</span>
      </button>
      <span class="total" class:none={totalPending === 0} title={headerTotal(split.workspace) + elsewhereSuffix(elsewhere.pending)}>
        <span class="total-long">{headerTotal(split.workspace)}{elsewhereSuffix(elsewhere.pending)}</span>
        <!-- A narrow window: the count alone; the list right below says where. -->
        <span class="total-short" aria-hidden="true">{totalPending === 0 ? "Nothing pending" : `${totalPending} pending`}{elsewhereSuffix(elsewhere.pending)}</span>
      </span>
      <button
        class="chip gear"
        class:on={settingsOpen}
        bind:this={settingsButton}
        aria-pressed={settingsOpen}
        aria-label="Settings"
        title="Settings (Ctrl+,)"
        onclick={() => (settingsOpen ? void closeSettings() : openSettings())}
      >
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <circle cx="8" cy="8" r="2.2" fill="none" stroke="currentColor" stroke-width="1.3" />
          <path
            d="M8 1.8v1.6M8 12.6v1.6M14.2 8h-1.6M3.4 8H1.8M12.4 3.6l-1.1 1.1M4.7 11.3l-1.1 1.1M12.4 12.4l-1.1-1.1M4.7 4.7 3.6 3.6"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
          />
          <circle cx="8" cy="8" r="4.6" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
      </button>
    </div>
  </header>

  <ProjectList
    projects={split.workspace}
    known={split.known}
    {elsewhere}
    appRoots={config.appRoots ?? []}
    onpick={pickFolder}
    onadd={addProjectAt}
    onaddknown={addToWorkspaceAt}
    onremove={removeFromWorkspaceAt}
    oninvscode={projectInVsCode}
    {readOnly}
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

  <div class="sheet" class:narrow-diff={narrowFull}>
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
          <p class="finding" title={finding?.detail ?? finding?.text}>{finding?.text ?? "\u00a0"}</p>
        </div>
        <ReviewTabs bind:this={reviewTabs} bind:filter={filterText} shown={shownPendingRows} total={counts.pending} {tab} {counts} {mode} ontab={setTab} onmode={setMode} onleave={leaveFilter} />
        {#each troubled as s (s.root)}
          <div class="notice error">
            Couldn't read the session for <b>{scopeLabel(view!, s)}</b>: {s.error}
          </div>
        {/each}
        {#if view && lock}
          <div class="banner" role="status">
            <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
              <rect x="3.5" y="7" width="9" height="6.5" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.3" />
              <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.3" />
            </svg>
            <div class="banner-text">
              <p>{lock.text}</p>
              {#if lock.detail}<p class="banner-detail path">{lock.detail}</p>{/if}
            </div>
            {#if lock.action}
              <button class="banner-btn" onclick={() => void onLock(lock!.action!)}>
                {lock.action === "turnOff" ? "Turn off" : "Try again"}
              </button>
            {/if}
          </div>
        {:else if view && canDecide && bulk.length > 0}
          {#key tab}
            <ActionBar bind:this={actionBar} {tab} keys={bulk} reappear={clearReveals} {busy} outcomes={rejectOutcomes} tabCount={filter !== null && tab === "pending" ? bulk.length : counts[tab]} filter={tab === "pending" ? filter : null} onaction={runBulk} />
          {/key}
        {/if}
        {#if !narrowDiffOpen}{@render protectBar()}{/if}
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
              {#if tab === "pending" && filter !== null && counts.pending > 0}
                <p class="state-title">No pending files match</p>
                <p class="state-body">Press Esc in the filter box to clear it.</p>
              {:else if tab === "pending"}
                <p class="state-title">All caught up</p>
                <p class="state-body">Claude's next edits to this project will show up here.</p>
              {:else}
                <p class="state-title">Nothing {tab} yet</p>
                <p class="state-body">Files you {tab === "accepted" ? "accept" : "reject"} are listed here.</p>
              {/if}
            </div>
          {:else}
            <FileTree
              bind:this={fileTree}
              {groups}
              {showGroups}
              isProtected={(p) => isProtected(p)}
              ageMinDays={settings.pendingAgeMinDays.value}
              {tab}
              {mode}
              {theme}
              {collapsed}
              editorName={editorLabel(config.editor)}
              selected={selectedRow}
              onselect={(r, via) => {
                selectedRow = r;
                if (via === "pointer") openNarrowDiff();
              }}
              onopen={(r) => void openRow(r)}
              oncontextmenu={(row, x, y) => (rowMenu = { x, y, row })}
              canDecide={canDecide && !narrow}
              {busy}
              ondecide={(action, target, row) => void runDecide(action, target, { row })}
              onnotice={announce}
              lockReason={narrow ? DIFF_HIDDEN_REFUSAL : lockReason}
            />
          {/if}
        </div>
      {/if}
      {#if narrow && !narrowDiffOpen}
        <!-- The diff pane (the bar's usual home) is hidden here; store errors and "left on disk" must stay visible. -->
        <ResultBar result={shownResult} {busy} ondismiss={() => (result = null)} />
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

    {#if settingsOpen}
    <Settings
      {settings}
      {config}
      project={current ? { root: current.root, name: current.name } : null}
      onset={changeSetting}
      onhooklog={changeHookLog}
      onextrahide={changeExtraHide}
      onnestedhide={changeNestedHide}
      onexclude={changeExclude}
      ontakeback={takeBack}
      oneditor={(t) =>
        setEditor(t).then(
          (c) => {
            config = c;
            return true;
          },
          (e) => {
            show({ text: `Couldn't change the editor: ${e}`, tone: "error" });
            return false;
          },
        )}
      onreadonly={(on) => void changeReadOnly(on)}
      onclose={() => void closeSettings()}
      full={narrow}
    />
    {:else}
    <DiffView
      bind:this={diffView}
      row={selectedRow}
      kind={tab}
      root={selected ?? ""}
      version={viewVersion}
      {theme}
      {config}
      bind:firstLine
      {openError}
      onopen={() => selectedRow && void open(selectedRow, firstLine)}
      onopenat={(line, col) => selectedRow && void open(selectedRow, line, null, col)}
      oncopyref={(sel) => selectedRow && void copyRef(selectedRow, sel)}
      copyRefHint={isMac ? "Cmd+Alt+C" : "Ctrl+Alt+C"}
      onpick={(t) => void pickEditor(t)}
      ondismiss={() => (openError = null)}
      canDecide={canDecide && diffShown}
      lockText={lock?.text ?? null}
      {busy}
      ondecide={(action) => {
        const row = selectedRow;
        if (!row) return;
        void runDecide(action, { kind: "file", scope: row.scope, key: action === "revert" ? row.key : row.path }, { row });
      }}
      onnotice={announce}
      onback={narrow ? closeNarrowDiff : null}
    >
      {#snippet footer()}
        {#if narrowDiffOpen}{@render protectBar()}{/if}
        {#if !narrow || narrowDiffOpen}
          <ResultBar result={shownResult} {busy} ondismiss={() => (result = null)} />
        {/if}
      {/snippet}
    </DiffView>
    {/if}
  </div>

  {#if gitlogMounted}
    <GitLogView bind:this={gitlogView} active={appView === "gitlog"} onnotice={announce} {config} {theme} onconfig={(c) => (config = c)} />
  {/if}
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
    grid-template-rows: var(--toolbar-h) minmax(0, 1fr);
    position: relative;
    height: 100vh;
    background: var(--frame);
    overflow: hidden;
    overflow: clip;
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

  /* The Git log view takes the whole row under the toolbar; the review stays mounted, hidden. */
  .app.gitlog > :global(.projects),
  .app.gitlog > .split-projects,
  .app.gitlog > .sheet {
    display: none;
  }

  /* The header switch: two segments, the current one raised. */
  .switch {
    display: inline-flex;
    flex: none;
    padding: 2px;
    border-radius: calc(var(--radius-control) + 2px);
    background: var(--hover);
  }
  .switch button {
    height: 24px;
    padding: 0 12px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
    white-space: nowrap;
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .switch button:hover {
    color: var(--text);
  }
  .switch button.on {
    color: var(--text);
    background: var(--surface);
    box-shadow: 0 0 0 1px var(--border);
    font-weight: 600;
  }
  .switch button:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -1px;
  }

  /* Toolbar and project list share the frame tint with no rule between them (the L). */
  .toolbar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-width: 0;
    padding: 0 20px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    flex: none;
  }
  .mark {
    display: block;
    width: 20px;
    height: 20px;
  }
  .wordmark {
    font-size: var(--fs-section);
    font-weight: 600;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 18px;
    min-width: 0;
    font-size: var(--fs-small);
  }
  /* Only the two status chips may give way (their labels cut, their popovers say it all). */
  .status > :global(*) {
    flex: none;
  }
  .status > :global(.capture),
  .status > :global(.update) {
    flex: 0 1 auto;
    min-width: 44px;
  }
  .hint {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
    color: var(--muted);
  }
  @media (max-width: 960px) {
    .hint {
      display: none;
    }
  }
  /* The read-only switch: a quiet chip, filled when on. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
    height: 24px;
    padding: 0 8px;
    border-radius: var(--radius-control);
    color: var(--muted);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .chip:hover {
    color: var(--text);
    background: var(--hover);
  }
  .chip.on {
    color: var(--warning-ink);
    background: var(--warning-bg);
    font-weight: 500;
  }

  .gear {
    width: 28px;
    padding: 0;
    justify-content: center;
  }
  .chip.gear.on {
    color: var(--text);
    background: var(--active);
    font-weight: 400;
  }

  /* "This looks like a secret file": inline, before any accept of a protected file. */
  .protect {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 10px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--rule);
    font-size: var(--fs-small);
    color: var(--warning-ink);
    background: var(--warning-bg);
    flex: none;
  }
  .protect > svg {
    flex: none;
  }
  .protect-text {
    flex: 1 1 200px;
    min-width: 0;
    line-height: 1.45;
    font-weight: 500;
  }
  .protect-buttons {
    display: flex;
    gap: 6px;
  }
  .protect-btn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid color-mix(in srgb, currentColor 35%, transparent);
    border-radius: var(--radius-control);
    font-weight: 600;
    white-space: nowrap;
  }
  .protect-btn:hover {
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  .protect-btn.go {
    color: var(--on-primary);
    background: var(--primary);
    border-color: var(--primary);
  }

  /* The one header item that may cut, after the status chips: the gear must always fit. */
  .status > .total {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .total {
    font-weight: 500;
    white-space: nowrap;
    color: var(--pending);
  }
  .total-short {
    display: none;
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
    /* clip, where supported: content can never be scrolled sideways, not even by focus() or
       scrollIntoView, which can scroll an overflow:hidden box and slide the tree off its edge. */
    overflow: clip;
  }

  .review {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
    overflow: clip;
    border-right: 1px solid var(--rule);
    /* The file column's own width (the Splitter moves it), so its header, bars and rows fold
       by the room they really have rather than by the window's width. */
    container: review / inline-size;
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
  /* Two lines at most, whatever the project: the per-worktree breakdown is its tooltip. */
  .finding {
    margin-top: 3px;
    font-size: var(--fs-small);
    color: var(--muted);
    text-wrap: pretty;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
  .finding.warn {
    color: var(--warning-ink);
    font-weight: 500;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: auto;
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
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--rule);
    font-size: var(--fs-small);
    line-height: 1.45;
    color: var(--warning-ink);
    background: var(--warning-bg);
    flex: none;
  }
  .banner > svg {
    flex: none;
  }
  .banner-text {
    flex: 1;
    min-width: 0;
  }
  .banner-text p:first-child {
    font-weight: 500;
  }
  .banner-detail {
    font-size: var(--fs-meta);
    opacity: 0.85;
    user-select: text;
    overflow-wrap: anywhere;
  }
  .banner-btn {
    flex: none;
    height: 28px;
    padding: 0 10px;
    border: 1px solid color-mix(in srgb, currentColor 35%, transparent);
    border-radius: var(--radius-control);
    font-weight: 600;
    white-space: nowrap;
    transition: background 120ms var(--ease);
  }
  .banner-btn:hover {
    background: color-mix(in srgb, currentColor 10%, transparent);
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


  /* A short status, bottom centre, above everything; it fades on its own. */
  .notice-region {
    position: fixed;
    left: 50%;
    bottom: 24px;
    z-index: 50;
    transform: translateX(-50%);
    pointer-events: none;
  }
  .launch-ask {
    position: fixed;
    top: calc(var(--toolbar-h) + 12px);
    left: 50%;
    z-index: 50;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: min(90vw, 720px);
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    color: var(--text);
    background: var(--surface);
    box-shadow: var(--menu-shadow);
  }
  .launch-ask p {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .toast {
    padding: 7px 12px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    white-space: nowrap;
    color: var(--surface);
    background: var(--text);
    box-shadow: var(--menu-shadow);
    animation: toast-in 140ms var(--ease);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }

  /* A narrow window: the header keeps every control, in fewer words. The full total and the
     read-only label stay for screen readers. 760px leaves room for the view switch. */
  @media (max-width: 760px) {
    .wordmark,
    .chip-label,
    .total-long {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }
    .total-short {
      display: inline;
    }
  }

  /* Narrow windows (a browser preview, a tiled window): one column, projects on top. */
  @media (max-width: 720px) {
    .app {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: var(--toolbar-h) auto minmax(0, 1fr);
    }
    .toolbar {
      gap: 12px;
      padding: 0 16px;
    }
    .status {
      gap: 8px;
      min-width: 0;
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
    .sheet > :global(.settings),
    .split {
      display: none;
    }
    .app.gitlog {
      grid-template-rows: var(--toolbar-h) minmax(0, 1fr);
    }
    .switch button {
      padding: 0 8px;
    }
    /* A file opened in a narrow window: its diff, full height, in place of the list. */
    .app.narrow-diff {
      grid-template-rows: var(--toolbar-h) minmax(0, 1fr);
    }
    .app.narrow-diff > :global(.projects) {
      display: none;
    }
    .sheet.narrow-diff > .review {
      display: none;
    }
    .sheet.narrow-diff > :global(.diff),
    .sheet.narrow-diff > :global(.settings) {
      display: flex;
    }
  }

</style>
