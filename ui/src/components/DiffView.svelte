<script lang="ts">
  import { onDestroy, onMount, untrack, type Snippet } from "svelte";
  import { getDiff, type DiffKind, type DiffPayload } from "../lib/api";
  import type { DecideAction } from "../lib/decide";
  import { WAITING_REFUSAL } from "../lib/keys";
  import { editorLabel, type EditorConfig } from "../lib/editors";
  import { iconFor, type IconTheme } from "../lib/icons";
  import { languageFor } from "../lib/language";
  import { modifiedPositionFor } from "../lib/navigate";
  import { deletedOnDisk, diffErrorText, paneState, type PaneState } from "../lib/paneState";
  import { loadWrap, readStore, WRAP_KEY, writeStore } from "../lib/uiState";
  import { chooseSelection, refLines, selectionOnModified, type LineRange } from "../lib/ref";
  import type { Row } from "../lib/rows";
  import OpenWith from "./OpenWith.svelte";

  type MonacoModule = typeof import("../lib/monaco");
  type Monaco = MonacoModule["monaco"];
  type DiffEditor = import("monaco-editor/editor/editor.api.js").editor.IStandaloneDiffEditor;

  let {
    row,
    kind,
    root,
    version,
    theme,
    config,
    firstLine = $bindable(1),
    openError,
    onopen,
    onpick,
    ondismiss,
    footer,
    canDecide = false,
    lockText = null,
    busy = false,
    ondecide = () => {},
    onnotice = () => {},
    onback = null,
    onopenat = () => {},
    oncopyref = () => {},
    copyRefHint = "Ctrl+Alt+C",
    external = null,
  }: {
    row: Row | null;
    kind: DiffKind;
    root: string;
    /** Bumped when the project reloads, so the selected diff refreshes in place. */
    version: number;
    theme: IconTheme;
    config: EditorConfig;
    /** The first changed line on the new side, for "Open in editor". */
    firstLine?: number;
    openError: string | null;
    onopen: () => void;
    onpick: (template: string) => void;
    ondismiss: () => void;
    /** A slim bar under the diff (the decision result). */
    footer?: Snippet;
    /** Decisions are on for this project (off in read-only mode and other locks). */
    canDecide?: boolean;
    /** Why decisions are off, for the disabled buttons' tooltip. */
    lockText?: string | null;
    busy?: boolean;
    /** Decide the shown row: accept/reject on Pending, revert on Accepted, reapply on Rejected. */
    ondecide?: (action: DecideAction) => void;
    onnotice?: (message: string) => void;
    /** Narrow window: the diff covers the list; Back (or Esc) returns to it. */
    onback?: (() => void) | null;
    /** Open the file at a position on its current (modified) side: Ctrl/Cmd+click, F12, the menu. */
    onopenat?: (line: number, col: number) => void;
    /** Copy a reference for Claude: the selection's (or cursor's) lines on the file's side, or null for the file. */
    oncopyref?: (sel: LineRange | null) => void;
    /** The shortcut shown in the Copy @ref button's tooltip. */
    copyRefHint?: string;
    /**
     * The Git log view hands in the two texts itself (a commit's file against its parent's), so
     * nothing is fetched and the review's own controls (decisions, copy reference) stay away.
     * `notice` replaces the diff with a message (binary, too large, nothing selected).
     */
    external?: {
      payload: DiffPayload | null;
      loading: boolean;
      error: string | null;
      notice: { title: string; body: string } | null;
      /** A chip beside the path: "Deleted", "Renamed from a.ts". */
      tag?: { text: string; tone: "new" | "deleted" } | null;
      /** No +/- counts (a combined file's counts are not known before the diff is drawn). */
      noStats?: boolean;
      /** A note under the header (what a combined diff includes). Never drawn on the review. */
      banner?: { tone: "info" | "warn"; text: string } | null;
    } | null;
  } = $props();

  /** The last cursor position in either side of the diff, for "Open at cursor". */
  let cursor: { side: "original" | "modified"; line: number; col: number } | null = null;
  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

  /** Opens the file at `line`:`col` of `side`; the original side is carried across the changes. */
  function openAt(side: "original" | "modified", line: number, col: number) {
    if (side === "modified") return onopenat(line, col);
    const p = modifiedPositionFor(line, col, editor?.getLineChanges() ?? []);
    onopenat(p.line, p.col);
  }

  /**
   * Where "Open in editor" lands: the cursor the user last placed in either side (the original
   * side carried across the changes), else the first change.
   */
  export function openTarget(): { line: number; col: number } {
    if (!cursor) return { line: firstLine, col: 1 };
    if (cursor.side === "modified") return { line: cursor.line, col: cursor.col };
    return modifiedPositionFor(cursor.line, cursor.col, editor?.getLineChanges() ?? []);
  }

  /**
   * `openTarget` once this row's diff is computed: a double-click selects the row and opens it in
   * one gesture, before the first change is known. Gives up after a moment (line 1 then).
   */
  export async function openTargetWhenReady(): Promise<{ line: number; col: number }> {
    const want = rowIdent;
    const deadline = Date.now() + 1500;
    while (Date.now() < deadline && rowIdent === want) {
      if (pane !== "editor" && pane !== "loading") break;
      if (pane === "editor" && shownIdent === want && editor?.getLineChanges()) break;
      await new Promise((r) => setTimeout(r, 30));
    }
    return openTarget();
  }

  /** "Open at cursor (F12)" from the menu: the last cursor, else the first change. */
  function openAtCursor() {
    const t = openTarget();
    onopenat(t.line, t.col);
  }

  /**
   * The lines a reference carries, as lines of the file on disk: the selection, else the line of
   * the cursor the user placed, else null (the file). With `focusedOnly` (the keyboard) a selection
   * or cursor counts only in the side that has focus; the header button and menu take whichever
   * side holds one, the side last used first.
   */
  export function selection(focusedOnly = false): LineRange | null {
    if (!editor) return null;
    const sides = [
      { side: "original" as const, e: editor.getOriginalEditor() },
      { side: "modified" as const, e: editor.getModifiedEditor() },
    ].sort((a, b) => Number(b.side === cursor?.side) - Number(a.side === cursor?.side));
    const changes = editor.getLineChanges() ?? [];
    const sel = chooseSelection(
      sides.map(({ side, e }) => {
        const sel = e.getSelection();
        const range = sel && !sel.isEmpty() ? selectionOnModified(side, { start: sel.startLineNumber, end: sel.endLineNumber }, changes) : null;
        return { range, focused: e.hasTextFocus() };
      }),
      focusedOnly,
    );
    const placed = cursor && (!focusedOnly || sides.find((s) => s.side === cursor!.side)!.e.hasTextFocus()) ? cursor : null;
    return refLines(sel, placed, changes);
  }

  type CodeEditor = import("monaco-editor/editor/editor.api.js").editor.IStandaloneCodeEditor;

  /** VS Code's go-to gesture on one side: Ctrl/Cmd+hover underlines the word, click or F12 opens. */
  function wireNavigation(m: Monaco, side: "original" | "modified", e: CodeEditor) {
    const link = e.createDecorationsCollection();
    const held = (ev: { ctrlKey: boolean; metaKey: boolean }) => (isMac ? ev.metaKey : ev.ctrlKey);
    const onText = (t: { type: number; position: unknown }) => t.type === m.editor.MouseTargetType.CONTENT_TEXT && !!t.position;
    e.onMouseMove((ev) => {
      const pos = ev.target.position;
      if (!held(ev.event) || !pos || !onText(ev.target)) return link.clear();
      const w = e.getModel()?.getWordAtPosition(pos);
      const range = w
        ? new m.Range(pos.lineNumber, w.startColumn, pos.lineNumber, w.endColumn)
        : new m.Range(pos.lineNumber, pos.column, pos.lineNumber, pos.column + 1);
      link.set([{ range, options: { inlineClassName: "cg-link" } }]);
    });
    e.onMouseLeave(() => link.clear());
    e.onKeyUp((ev) => {
      if (!held(ev)) link.clear();
    });
    e.onMouseDown((ev) => {
      const pos = ev.target.position;
      if (!held(ev.event) || !ev.event.leftButton || !pos || !onText(ev.target)) return;
      ev.event.preventDefault();
      link.clear();
      openAt(side, pos.lineNumber, pos.column);
    });
    e.onDidChangeCursorPosition((ev) => {
      // Only a cursor the user placed; Monaco's own moves (a new model, revealing the first change)
      // must not replace "the first change" with line 1.
      if (ev.source !== "mouse" && ev.source !== "keyboard") return;
      cursor = { side, line: ev.position.lineNumber, col: ev.position.column };
    });
    // Monaco's right-click menu: the diff's entry for "Copy reference for Claude" (the review's:
    // a commit's file has no reference to copy).
    if (!external) e.addAction({
      id: `claudegate.copyRef.${side}`,
      label: "Copy reference for Claude",
      contextMenuGroupId: "9_cutcopypaste",
      contextMenuOrder: 9,
      run: (ed) => {
        const sel = ed.getSelection();
        const lines = sel && !sel.isEmpty() ? { start: sel.startLineNumber, end: sel.endLineNumber } : null;
        const changes = editor?.getLineChanges() ?? [];
        // No selection: this side's cursor, when the user placed it.
        const placed = cursor?.side === side ? cursor : null;
        oncopyref(refLines(lines && selectionOnModified(side, lines, changes), placed, changes));
      },
    });
    // VS Code's diff keys; the window answers them too while focus is elsewhere.
    e.addAction({
      id: `claudegate.nextChange.${side}`,
      label: "Go to next change",
      keybindings: [m.KeyMod.Alt | m.KeyCode.F5],
      run: () => goToChange(1),
    });
    e.addAction({
      id: `claudegate.previousChange.${side}`,
      label: "Go to previous change",
      keybindings: [m.KeyMod.Shift | m.KeyMod.Alt | m.KeyCode.F5],
      run: () => goToChange(-1),
    });
    e.addAction({
      id: `claudegate.toggleWrap.${side}`,
      label: "Toggle word wrap",
      keybindings: [m.KeyMod.Alt | m.KeyCode.KeyZ],
      run: () => toggleWrap(),
    });
    e.addAction({
      id: `claudegate.openAtCursor.${side}`,
      label: "Open at cursor in editor",
      keybindings: [m.KeyCode.F12],
      run: (ed) => {
        const p = ed.getPosition();
        if (p) openAt(side, p.lineNumber, p.column);
      },
    });
  }

  /**
   * Alt+F5 / Shift+Alt+F5: the next or previous change, from the cursor. Where it lands becomes
   * the cursor, so Open and `o` go there next.
   */
  export function goToChange(dir: 1 | -1) {
    if (!editor || pane !== "editor") return;
    const mod = editor.getModifiedEditor();
    // Step from where the user is: a cursor in the original side, carried across the changes.
    if (cursor) {
      const t = openTarget();
      mod.setPosition({ lineNumber: t.line, column: t.col });
    }
    editor.goToDiff(dir === 1 ? "next" : "previous");
    const p = mod.getPosition();
    if (p) cursor = { side: "modified", line: p.lineNumber, col: p.column };
  }

  /** Alt+Z: wrap long lines in both sides, remembered across restarts. */
  let wrap = $state(loadWrap(readStore(WRAP_KEY)));
  export function toggleWrap() {
    wrap = !wrap;
    editor?.updateOptions({ wordWrap: wrap ? "on" : "off", diffWordWrap: wrap ? "on" : "off" });
    writeStore(WRAP_KEY, wrap ? "on" : "off");
    onnotice(wrap ? "Word wrap on" : "Word wrap off");
  }

  /** The decisions the header offers for the shown row's tab. */
  const decisions = $derived<{ action: DecideAction; label: string; key?: string; tone: string }[]>(
    kind === "pending"
      ? [
          { action: "accept", label: "Accept", key: "A", tone: "accept" },
          { action: "reject", label: "Reject", key: "R", tone: "reject" },
        ]
      : kind === "accepted"
        ? [{ action: "revert", label: "Revert", tone: "back" }]
        : [{ action: "reapply", label: "Reapply", tone: "back" }],
  );
  const waitingRow = $derived(kind === "pending" && !!row?.waiting);
  function decideTitle(d: { label: string; key?: string }): string {
    if (!canDecide) return lockText ?? "Decisions are off";
    if (waitingRow) return WAITING_REFUSAL;
    if (d.label === "Revert") return "Revert: back to pending review";
    if (d.label === "Reapply") return "Reapply Claude's version";
    return `${d.label} (${d.key})`;
  }

  const SIDE_KEY = "claudegate.diffSideBySide";
  let sideBySide = $state(true);
  try {
    sideBySide = localStorage.getItem(SIDE_KEY) !== "inline";
  } catch {}

  let payload = $state<DiffPayload | null>(null);
  /** The row `payload` was fetched for; models are keyed on it, never on the current selection. */
  let payloadIdent = $state("");
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let host: HTMLDivElement | undefined = $state();
  let monaco: Monaco | null = null;
  let editor: DiffEditor | null = $state(null);
  /** Which row the models belong to; a refresh of the same row updates text in place. */
  let shownIdent = "";
  let seq = 0;

  const rowIdent = $derived(row ? `${kind}\u0000${row.key}` : "");
  /** The payload only when it belongs to the selected row: never another row's text under this one. */
  const shown = $derived(row && payloadIdent === rowIdent ? payload : null);
  /** Exactly one of empty / loading / error / binary / editor is on screen. */
  const pane = $derived<PaneState | "notice">(external?.notice ? "notice" : paneState(row, shown, loading, loadError));

  // Fetch the selected row's diff; `version` re-runs it when the project reloads.
  $effect(() => {
    const r = row;
    const k = kind;
    void version;
    const s = ++seq;
    const ext = external;
    if (ext) {
      // Handed in by the Git log: the text is already here (or on its way), keyed on the same row.
      if (!r) {
        payload = null;
        payloadIdent = "";
      } else {
        payload = ext.payload;
        payloadIdent = ext.payload ? `${k}\u0000${r.key}` : "";
      }
      loadError = ext.error;
      loading = ext.loading;
      return;
    }
    if (!r) {
      payload = null;
      payloadIdent = "";
      loadError = null;
      loading = false;
      return;
    }
    const ident = `${k}\u0000${r.key}`;
    loading = true;
    // A different row drops the last one's error; a refresh of the same row keeps what it shows.
    if (untrack(() => payloadIdent) !== ident) loadError = null;
    getDiff(root, r.scope, k, r.key).then(
      (p) => {
        if (s !== seq) return;
        payloadIdent = ident;
        payload = p;
        loadError = null;
        loading = false;
      },
      (e) => {
        if (s !== seq) return;
        payload = null;
        payloadIdent = "";
        loadError = String(e);
        loading = false;
      },
    );
  });

  // A different row starts at line 1 until its diff is computed. Keyed on the row's key, not the
  // object: a refresh hands in a fresh object for the same row and must keep the computed line.
  $effect(() => {
    void rowIdent;
    untrack(() => {
      firstLine = 1;
      cursor = null;
    });
  });

  let setDiffTheme: MonacoModule["setDiffTheme"] | null = null;

  async function ensureEditor() {
    if (editor || !host) return;
    const [{ monaco: m, setDiffTheme: setT }] = await Promise.all([
      import("../lib/monaco"),
      document.fonts?.load('13px "JetBrains Mono"').catch(() => undefined),
      document.fonts?.load('italic 13px "JetBrains Mono"').catch(() => undefined),
    ]);
    if (editor || !host) return;
    monaco = m;
    setDiffTheme = setT;
    setT(theme === "dark");
    const ed = m.editor.createDiffEditor(host, {
      readOnly: true,
      originalEditable: false,
      domReadOnly: true,
      automaticLayout: true,
      renderSideBySide: sideBySide,
      // The chosen layout holds down to a 560px pane; below that two columns cannot be read.
      useInlineViewWhenSpaceIsLimited: true,
      renderSideBySideInlineBreakpoint: 560,
      enableSplitViewResizing: true,
      renderMarginRevertIcon: false,
      // VS Code's diff editor: +/- indicators, the overview ruler, word-level highlights.
      renderIndicators: true,
      renderOverviewRuler: true,
      ignoreTrimWhitespace: false,
      hideUnchangedRegions: { enabled: true, contextLineCount: 3, minimumLineCount: 5, revealLineCount: 20 },
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      stickyScroll: { enabled: false },
      bracketPairColorization: { enabled: true },
      folding: false,
      glyphMargin: false,
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13,
      lineHeight: 20,
      fontLigatures: false,
      wordWrap: wrap ? "on" : "off",
      diffWordWrap: wrap ? "on" : "off",
      fixedOverflowWidgets: true,
      ariaLabel: "Diff",
    });
    wireNavigation(m, "original", ed.getOriginalEditor());
    wireNavigation(m, "modified", ed.getModifiedEditor());
    // Monaco pins the original side's wrap off (`wordWrapOverride2`) while the diff is inline, and
    // never unpins it when the diff goes back to side by side (the inline button, or the window
    // widening past the inline breakpoint). Unpin it whenever the original side is laid out, so
    // Alt+Z wraps both sides.
    const original = ed.getOriginalEditor();
    const unpinWrap = () => {
      if (original.getLayoutInfo().width > 0) original.updateOptions({ wordWrapOverride2: "inherit" });
    };
    original.onDidLayoutChange(unpinWrap);
    original.onDidChangeConfiguration(unpinWrap);
    ed.onDidUpdateDiff(() => {
      const changes = ed.getLineChanges() ?? [];
      const c = changes[0];
      // A pure deletion has modifiedEndLineNumber 0 and starts on the line before the cut.
      firstLine = c ? Math.max(1, c.modifiedStartLineNumber) : 1;
    });
    editor = ed;
  }

  function sideText(s: DiffPayload["before"]): string {
    return s.kind === "text" ? s.text : "";
  }

  /** Detach and dispose the models, so a hidden editor holds no file. */
  function clearModels(ed: DiffEditor) {
    const cur = ed.getModel();
    if (!cur) return;
    ed.setModel(null);
    cur.original.dispose();
    cur.modified.dispose();
    shownIdent = "";
  }

  // Push the payload into the editor: new models for a new row, in-place text for a refresh.
  $effect(() => {
    const p = shown;
    const ed = editor;
    const state = pane;
    if (state !== "editor" || !p) {
      if (ed) untrack(() => clearModels(ed));
      return;
    }
    if (!ed) {
      void ensureEditor();
      return;
    }
    untrack(() => {
      const m = monaco!;
      const ident = payloadIdent;
      const before = sideText(p.before);
      const after = sideText(p.after);
      const cur = ed.getModel();
      if (cur && ident === shownIdent) {
        const changed = cur.original.getValue() !== before || cur.modified.getValue() !== after;
        if (!changed) return;
        // Claude wrote the shown file again: new text, same place. setValue alone sends the
        // cursor and the scroll back to line 1.
        const kept = ed.saveViewState();
        if (cur.original.getValue() !== before) cur.original.setValue(before);
        if (cur.modified.getValue() !== after) cur.modified.setValue(after);
        if (kept) ed.restoreViewState(kept);
        return;
      }
      const lang = languageFor(p.path);
      ed.setModel({ original: m.editor.createModel(before, lang), modified: m.editor.createModel(after, lang) });
      cur?.original.dispose();
      cur?.modified.dispose();
      shownIdent = ident;
    });
  });

  $effect(() => {
    const t = theme;
    if (editor && setDiffTheme) untrack(() => setDiffTheme!(t === "dark"));
  });

  function setSide(v: boolean) {
    sideBySide = v;
    editor?.updateOptions({ renderSideBySide: v });
    try {
      localStorage.setItem(SIDE_KEY, v ? "side" : "inline");
    } catch {}
  }

  onMount(() => {
    // Warm the editor chunk while the user picks a file.
    void import("../lib/monaco");
  });

  onDestroy(() => {
    const m = editor?.getModel();
    editor?.dispose();
    m?.original.dispose();
    m?.modified.dispose();
  });

  function fileName(rel: string) {
    return rel.split("/").pop() ?? rel;
  }
  const label = $derived(editorLabel(config.editor));
</script>

<main class="diff" aria-label="Diff">
  {#if row}
    <div class="diff-head">
      {#if onback}
        <button class="back" aria-label="Back to the file list" title="Back to the file list (Esc)" onclick={onback}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <path d="M10 3.5 5.5 8l4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
          <span>Back</span>
        </button>
      {/if}
      <img src={iconFor(fileName(row.rel), "file", false, theme)} alt="" width="16" height="16" draggable="false" />
      <span class="diff-path" title={row.path}>
        {#if row.rel.includes("/")}<span class="diff-dir"><bdi>{row.rel.slice(0, row.rel.lastIndexOf("/") + 1)}</bdi></span>{/if}<span class="diff-file">{fileName(row.rel)}</span>
      </span>
      {#if row.newFile}<span class="tag new">New file</span>{/if}
      {#if external?.tag}<span class="tag {external.tag.tone}">{external.tag.text}</span>{/if}
      {#if !external && deletedOnDisk(shown)}<span class="tag deleted" title="The file is no longer on disk">Deleted</span>{/if}
      {#if row.waiting && !external}
        <!-- No change on disk yet: "+0 −0" would read as an empty edit, so say what it is. -->
        <span class="tag waiting" title="Captured before Claude wrote it; the write hasn't landed yet. It can be decided once it does.">Not written yet</span>
        <span class="diff-stats-gap"></span>
      {:else if external?.noStats}
        <span class="diff-stats-gap"></span>
      {:else}
        <span class="diff-stats num">
          <span class="add">+{row.added}</span>
          <span class="del">−{row.removed}</span>
        </span>
      {/if}
      <div class="view" role="group" aria-label="Diff layout">
        <button class:on={sideBySide} aria-pressed={sideBySide} aria-label="Side-by-side diff" title="Side by side" onclick={() => setSide(true)}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <rect x="2" y="3" width="12" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" />
            <path d="M8 3v10" stroke="currentColor" stroke-width="1.3" />
          </svg>
        </button>
        <button class:on={!sideBySide} aria-pressed={!sideBySide} aria-label="Inline diff" title="Inline" onclick={() => setSide(false)}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <rect x="2" y="3" width="12" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" />
            <path d="M4.5 6.5h7M4.5 9.5h5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
          </svg>
        </button>
      </div>
      {#if !external}
      <div class="decide" role="group" aria-label="Decide this file">
        {#each decisions as d (d.action)}
          <!-- Waiting: aria-disabled so the tooltip shows and a click says why; locked: disabled. -->
          <button
            class="dbtn {d.tone}"
            disabled={!canDecide || busy}
            aria-disabled={waitingRow ? "true" : undefined}
            aria-keyshortcuts={d.key?.toLowerCase()}
            title={decideTitle(d)}
            onclick={() => (waitingRow ? onnotice(WAITING_REFUSAL) : ondecide(d.action))}
          >
            {#if d.tone === "accept"}
              <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M3.5 8.5 6.5 11.5 12.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
            {:else if d.tone === "reject"}
              <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
            {:else}
              <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M5.5 3.5 2.5 6.5l3 3M2.5 6.5h7a3.5 3.5 0 0 1 0 7H7" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
            {/if}
            <span class="dlabel">{d.label}</span>{#if d.key}<kbd class="dkey">{d.key}</kbd>{/if}
          </button>
        {/each}
      </div>
      <button
        class="copyref"
        title="Copy a reference for Claude: the selection, the cursor line, or the file ({copyRefHint})"
        aria-label="Copy reference for Claude"
        aria-keyshortcuts="Control+Alt+C"
        onclick={() => oncopyref(selection(false))}
      >
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M10.5 3.5v-.2a.8.8 0 0 0-.8-.8H3.3a.8.8 0 0 0-.8.8v6.4a.8.8 0 0 0 .8.8h.2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
        </svg>
        <span class="crlabel">Copy @ref</span>
      </button>
      {/if}
      <OpenWith {config} onopen={pane === "editor" ? openAtCursor : onopen} {onpick} onopencursor={pane === "editor" ? openAtCursor : null} />
    </div>
  {/if}

  {#if row && external?.banner}
    <div class="banner {external.banner.tone}" role="note">
      {#if external.banner.tone === "warn"}
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M8 2 1.5 13.5h13L8 2Z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
          <path d="M8 6.5v3.2M8 11.6v.1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        </svg>
      {/if}
      <p>{external.banner.text}</p>
    </div>
  {/if}

  {#if openError}
    <div class="alert" role="alert">
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.3" />
        <path d="M8 4.8v3.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        <circle cx="8" cy="11" r="0.9" fill="currentColor" />
      </svg>
      <p>{openError}</p>
      <button class="dismiss" aria-label="Dismiss" title="Dismiss" onclick={ondismiss}>
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>
    </div>
  {/if}

  {#if external && !row}
    <div class="diff-head" aria-hidden="true"></div>
  {/if}
  <div class="body">
    <!-- display:none, not visibility:hidden: Monaco sets an inline `visibility: visible` on both
         sides of the diff, which beat the inherited hidden and drew code under the empty state. -->
    <div class="editor" hidden={pane !== "editor"} bind:this={host}></div>
    {#if pane === "notice" && external?.notice}
      <div class="placeholder">
        <p class="ph-title">{external.notice.title}</p>
        <p class="ph-body">{external.notice.body}</p>
      </div>
    {:else if pane === "empty"}
      <div class="placeholder">
        <p class="ph-title">Select a file to see its changes</p>
        <dl class="keys">
          <div><dt><kbd>J</kbd> <kbd>K</kbd></dt><dd>Next and previous file</dd></div>
          <div><dt><kbd>N</kbd> <kbd>P</kbd></dt><dd>Next and previous pending, across worktrees and repos</dd></div>
          <div><dt><kbd>A</kbd> <kbd>R</kbd></dt><dd>Accept or reject the selected file or focused folder</dd></div>
          <div><dt><kbd>O</kbd></dt><dd>Open in {label} at the cursor, or the first change</dd></div>
          <div><dt><kbd>{copyRefHint}</kbd></dt><dd>Copy a reference for Claude: selection, cursor line or file</dd></div>
          <div><dt><kbd>{copyRefHint.replace(/C$/, "Shift+C")}</kbd></dt><dd>Copy a reference to the whole file</dd></div>
          <div><dt><kbd>1</kbd> <kbd>2</kbd> <kbd>3</kbd></dt><dd>Pending, accepted, rejected</dd></div>
          <div><dt><kbd>[</kbd></dt><dd>Collapse or expand projects</dd></div>
          <div><dt><kbd>?</kbd></dt><dd>All shortcuts</dd></div>
        </dl>
      </div>
    {:else if pane === "loading"}
      <div class="placeholder loading" aria-busy="true">
        <p class="ph-body">Loading diff…</p>
      </div>
    {:else if pane === "error"}
      <div class="placeholder">
        <p class="ph-title">Couldn't load this diff</p>
        <p class="ph-body path">{diffErrorText(loadError ?? "")}</p>
      </div>
    {:else if pane === "binary"}
      <div class="placeholder">
        <p class="ph-title">Binary file, not shown</p>
      </div>
    {/if}
  </div>
  {@render footer?.()}
</main>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    container: diff / inline-size;
  }
  .diff-head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 10px 0 var(--pad);
    border-bottom: 1px solid var(--rule);
    flex: none;
  }
  /* Short of room, the folders give way from the left ("…/routes/orders.ts") and the file's
     own name is the last thing cut. */
  .diff-path {
    display: flex;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: var(--fs-small);
    user-select: text;
  }
  .diff-dir {
    flex: 0 1000 auto;
    min-width: 2ch;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    color: var(--muted);
  }
  .diff-file {
    flex: none;
    font-weight: 500;
  }
  .tag.new {
    flex: none;
    padding: 0 6px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    line-height: 18px;
    color: var(--untracked);
    background: var(--raised);
  }
  .tag.deleted {
    flex: none;
    padding: 0 6px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    line-height: 18px;
    color: var(--negative);
    background: var(--raised);
  }
  .tag.waiting {
    flex: none;
    padding: 0 6px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    font-weight: 500;
    line-height: 18px;
    white-space: nowrap;
    color: var(--waiting-ink);
    background: var(--waiting-bg);
  }
  .diff-stats-gap {
    margin-left: auto;
  }
  .diff-stats {
    margin-left: auto;
    display: inline-flex;
    gap: 8px;
    padding-right: 6px;
    font-size: var(--fs-small);
    flex: none;
  }
  .add {
    color: var(--positive);
  }
  .del {
    color: var(--negative);
  }

  .back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    height: 28px;
    margin-left: -6px;
    padding: 0 8px 0 4px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .back:hover {
    color: var(--text);
    background: var(--hover);
  }

  .decide {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
  }
  .dbtn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    white-space: nowrap;
    color: var(--text);
    background: var(--surface);
    transition:
      background 120ms var(--ease),
      color 120ms var(--ease);
  }
  .dbtn:hover:not(:disabled):not([aria-disabled="true"]) {
    background: var(--hover);
  }
  .dbtn.accept:hover:not(:disabled):not([aria-disabled="true"]) svg {
    color: var(--positive);
  }
  .dbtn.reject:hover:not(:disabled):not([aria-disabled="true"]) svg {
    color: var(--negative);
  }
  .dbtn svg {
    color: var(--muted);
  }
  .dbtn:disabled,
  .dbtn[aria-disabled="true"] {
    color: var(--faint);
    opacity: 0.6;
  }
  .dkey {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--faint);
  }

  .copyref {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
    height: 28px;
    padding: 0 8px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    white-space: nowrap;
    color: var(--muted);
    transition:
      background 120ms var(--ease),
      color 120ms var(--ease);
  }
  .copyref:hover {
    color: var(--text);
    background: var(--hover);
  }
  .copyref:active {
    background: var(--active);
  }

  /* Side by side or inline: the same segmented control as the file list's tree/list toggle. */
  .view {
    display: flex;
    align-items: center;
    flex: none;
  }
  .view button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--control-border);
    color: var(--faint);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .view button:first-child {
    border-radius: var(--radius-control) 0 0 var(--radius-control);
  }
  .view button:last-child {
    margin-left: -1px;
    border-radius: 0 var(--radius-control) var(--radius-control) 0;
  }
  .view button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .view button.on {
    color: var(--text);
    background: var(--active);
  }

  .banner {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 6px var(--pad);
    border-bottom: 1px solid var(--rule);
    font-size: var(--fs-small);
    line-height: 1.45;
    flex: none;
    user-select: text;
  }
  .banner p {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .banner svg {
    flex: none;
    margin-top: 2px;
  }
  .banner.info {
    color: var(--muted);
  }
  .banner.warn {
    color: var(--warning-ink);
    background: var(--warning-bg);
  }
  .alert {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 10px 12px 0;
    padding: 8px 8px 8px 10px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    line-height: 1.45;
    color: var(--error-ink);
    background: var(--error-bg);
    user-select: text;
    flex: none;
  }
  .alert > svg {
    flex: none;
    margin-top: 2px;
  }
  .alert p {
    flex: 1;
    min-width: 0;
  }
  .dismiss {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-chip);
    color: inherit;
  }
  .dismiss:hover {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .body {
    position: relative;
    /* Monaco's own z-indexes stay inside; the Open-with menu floats above them. */
    isolation: isolate;
    flex: 1;
    min-height: 0;
  }
  .editor {
    position: absolute;
    inset: 0;
  }
  .editor[hidden] {
    display: none;
  }
  /* Ctrl/Cmd held over a token: it reads as a link, as in VS Code. */
  .editor :global(.cg-link) {
    text-decoration: underline;
    cursor: pointer;
  }
  /* Quick fetches never show it; only a slow one fades in. */
  .loading {
    animation: appear 120ms var(--ease) 250ms both;
  }
  @keyframes appear {
    from {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .loading {
      animation: none;
    }
  }

  .placeholder {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 24px;
    text-align: center;
    color: var(--muted);
  }
  .ph-title {
    font-weight: 600;
    color: var(--text);
  }
  .ph-body {
    max-width: 420px;
    font-size: var(--fs-small);
    line-height: 1.5;
  }
  .path {
    font-family: var(--font-mono);
    user-select: text;
  }

  .keys {
    display: grid;
    gap: 6px;
    margin: 14px 0 0;
    font-size: var(--fs-small);
    text-align: left;
  }
  .keys div {
    display: grid;
    grid-template-columns: 132px auto;
    gap: 12px;
  }
  .keys dt {
    text-align: right;
    white-space: nowrap;
  }
  .keys dd {
    margin: 0;
  }
  /* A narrow pane (a small window) keeps the path and the open action; the rest steps aside,
     least useful first, so the file's name always has room to be read. */
  @container diff (max-width: 820px) {
    .dkey,
    .crlabel {
      display: none;
    }
  }
  @container diff (max-width: 680px) {
    .diff-stats,
    .view,
    .tag.new,
    .tag.deleted {
      display: none;
    }
    .diff-path {
      margin-right: auto;
    }
  }
  @container diff (max-width: 500px) {
    .diff-head :global(.split .main span) {
      display: none;
    }
  }
  @container diff (max-width: 380px) {
    .dlabel {
      display: none;
    }
  }
</style>
