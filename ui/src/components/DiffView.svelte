<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { getDiff, type DiffKind, type DiffPayload } from "../lib/api";
  import { editorLabel, type EditorConfig } from "../lib/editors";
  import { iconFor, type IconTheme } from "../lib/icons";
  import { languageFor } from "../lib/language";
  import { paneState } from "../lib/paneState";
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
  } = $props();

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
  const pane = $derived(paneState(row, shown, loading, loadError));

  // Fetch the selected row's diff; `version` re-runs it when the project reloads.
  $effect(() => {
    const r = row;
    const k = kind;
    void version;
    const s = ++seq;
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
    untrack(() => (firstLine = 1));
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
      wordWrap: "off",
      diffWordWrap: "off",
      fixedOverflowWidgets: true,
      ariaLabel: "Diff",
    });
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
        if (cur.original.getValue() !== before) cur.original.setValue(before);
        if (cur.modified.getValue() !== after) cur.modified.setValue(after);
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
      <img src={iconFor(fileName(row.rel), "file", false, theme)} alt="" width="16" height="16" draggable="false" />
      <span class="diff-path" title={row.path}>
        {#if row.rel.includes("/")}<span class="diff-dir">{row.rel.slice(0, row.rel.lastIndexOf("/") + 1)}</span>{/if}<span class="diff-file">{fileName(row.rel)}</span>
      </span>
      {#if row.newFile}<span class="tag new">New file</span>{/if}
      <span class="diff-stats num">
        <span class="add">+{row.added}</span>
        <span class="del">−{row.removed}</span>
      </span>
      <div class="view" role="group" aria-label="Diff layout">
        <button class:on={sideBySide} aria-pressed={sideBySide} title="Side by side" onclick={() => setSide(true)}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <rect x="2" y="3" width="12" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" />
            <path d="M8 3v10" stroke="currentColor" stroke-width="1.3" />
          </svg>
        </button>
        <button class:on={!sideBySide} aria-pressed={!sideBySide} title="Inline" onclick={() => setSide(false)}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <rect x="2" y="3" width="12" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" />
            <path d="M4.5 6.5h7M4.5 9.5h5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
          </svg>
        </button>
      </div>
      <OpenWith {config} {onopen} {onpick} />
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

  <div class="body">
    <!-- display:none, not visibility:hidden: Monaco sets an inline `visibility: visible` on both
         sides of the diff, which beat the inherited hidden and drew code under the empty state. -->
    <div class="editor" hidden={pane !== "editor"} bind:this={host}></div>
    {#if pane === "empty"}
      <div class="placeholder">
        <p class="ph-title">Select a file to see its changes</p>
        <dl class="keys">
          <div><dt><kbd>J</kbd> <kbd>K</kbd></dt><dd>Next and previous file</dd></div>
          <div><dt><kbd>N</kbd> <kbd>P</kbd></dt><dd>Next and previous pending, across worktrees</dd></div>
          <div><dt><kbd>O</kbd></dt><dd>Open in {label} at the first change</dd></div>
          <div><dt><kbd>1</kbd> <kbd>2</kbd> <kbd>3</kbd></dt><dd>Pending, accepted, rejected</dd></div>
        </dl>
      </div>
    {:else if pane === "loading"}
      <div class="placeholder loading" aria-busy="true">
        <p class="ph-body">Loading diff…</p>
      </div>
    {:else if pane === "error"}
      <div class="placeholder">
        <p class="ph-title">Couldn't load this diff</p>
        <p class="ph-body path">{loadError}</p>
      </div>
    {:else if pane === "binary"}
      <div class="placeholder">
        <p class="ph-title">Binary file, not shown</p>
      </div>
    {/if}
  </div>
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
    flex: none;
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

  .view {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }
  .view button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-control);
    color: var(--faint);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .view button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .view button.on {
    color: var(--text);
    background: var(--active);
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
    width: 20px;
    height: 20px;
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
    grid-template-columns: 64px auto;
    gap: 12px;
  }
  .keys dt {
    text-align: right;
    white-space: nowrap;
  }
  .keys dd {
    margin: 0;
  }
  /* A narrow pane (a small window) keeps the path and the open action; the rest steps aside. */
  @container diff (max-width: 560px) {
    .diff-stats,
    .view,
    .tag.new {
      display: none;
    }
    .diff-path {
      margin-right: auto;
    }
  }
  @container diff (max-width: 360px) {
    .diff-head :global(.split .main span) {
      display: none;
    }
  }
</style>
