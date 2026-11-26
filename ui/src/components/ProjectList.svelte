<script lang="ts">
  import { tick } from "svelte";
  import type { ProjectSummary } from "../lib/api";
  import { initials } from "../lib/rows";
  import { elsewhereLine, pickerRows, shortPath } from "../lib/workspace";
  import ContextMenu from "./ContextMenu.svelte";

  let {
    projects,
    selected,
    loading = false,
    collapsed = false,
    onselect,
    ontoggle,
    known = [],
    elsewhere = { projects: 0, pending: 0 },
    appRoots = [],
    onpick = async () => null,
    onadd = async () => null,
    onaddknown = async () => null,
    onremove = async () => {},
    oninvscode = async () => false,
    readOnly = false,
  }: {
    /** The workspace: the projects on the rail, in the user's order. */
    projects: ProjectSummary[];
    selected: string | null;
    loading?: boolean;
    /** Shown as a rail of initials, for more review space. */
    collapsed?: boolean;
    onselect: (root: string) => void;
    ontoggle: () => void;
    /** Listed projects outside the workspace: offered in the Add to workspace picker. */
    known?: ProjectSummary[];
    /** Known projects with pending work that has landed, and how much. */
    elsewhere?: { projects: number; pending: number };
    /** Roots the app added: marked "Added here"; removing one can also stop capture. */
    appRoots?: string[];
    /** The native folder picker; null when cancelled. */
    onpick?: () => Promise<string | null>;
    /** Registers the folder for capture, adds it to the workspace and selects it; an error message, or null. */
    onadd?: (path: string) => Promise<string | null>;
    /** Adds a known project to the workspace (capture is unchanged); an error message, or null. */
    onaddknown?: (root: string) => Promise<string | null>;
    /** Takes a project off the rail; `stopCapture` also runs the Remove flow (app-added roots only). */
    onremove?: (root: string, stopCapture: boolean) => Promise<void>;
    /** Whether VS Code or Cursor also uses the folder (stopping capture then only forgets it). */
    oninvscode?: (root: string) => Promise<boolean>;
    /** Registering and unregistering write a shared file, so read-only mode blocks them. */
    readOnly?: boolean;
  } = $props();

  const READ_ONLY = "Read-only mode is on. Turn it off in Settings to add or remove projects.";

  /** The Add to workspace picker. */
  let picking = $state(false);
  let query = $state("");
  let active = $state(0);
  let searchInput = $state<HTMLInputElement | null>(null);
  let pickError = $state<string | null>(null);
  const rows = $derived(pickerRows(known, query));
  $effect(() => {
    if (active >= rows.length) active = Math.max(0, rows.length - 1);
  });

  /** The typed-path form: the keyboard route, and where a refused add explains itself. */
  let adding = $state(false);
  let addPath = $state("");
  let addError = $state<string | null>(null);
  let busy = $state(false);
  let pathInput = $state<HTMLInputElement | null>(null);

  /** A project's context menu. */
  let menu = $state<{ root: string; x: number; y: number } | null>(null);
  /** The app-added project whose Remove from workspace is awaiting confirmation. */
  let confirming = $state<string | null>(null);
  let stopCapture = $state(false);
  let cancelButton = $state<HTMLButtonElement | null>(null);
  /** For the stop-capture wording: null while being checked. */
  let confirmInVsCode = $state<boolean | null>(null);

  const isAppAdded = (root: string) => appRoots.includes(root);
  /** A project's row, or its chip on the collapsed rail. */
  const rowButton = (root: string) =>
    document.querySelector<HTMLButtonElement>(`.project[data-root="${CSS.escape(root)}"], .chip[data-root="${CSS.escape(root)}"]`);
  let addButtonEl = $state<HTMLButtonElement | null>(null);
  let addChipEl = $state<HTMLButtonElement | null>(null);
  /** The picker expanded a collapsed rail: closing it collapses the rail again. */
  let expandedForPicker = false;
  /** Likewise for a Remove confirm asked from a collapsed chip (the confirm needs the full rail). */
  let expandedForConfirm = false;

  /** Ends a Remove confirm: collapses the rail again if the confirm expanded it. */
  function endConfirm() {
    confirming = null;
    if (expandedForConfirm) {
      expandedForConfirm = false;
      if (!collapsed) ontoggle();
    }
  }

  function cancelConfirm(root: string) {
    endConfirm();
    void focusAfter(root);
  }

  /** Focus after the DOM settles: `root`'s row, else the add button (or the collapsed add chip). */
  async function focusAfter(root: string | null) {
    await tick();
    requestAnimationFrame(() => {
      const row = root ? rowButton(root) : null;
      (row ?? (collapsed ? addChipEl : addButtonEl))?.focus();
    });
  }

  function openPicker() {
    expandedForPicker = collapsed;
    if (collapsed) ontoggle();
    picking = true;
    query = "";
    active = 0;
    pickError = null;
    queueMicrotask(() => searchInput?.focus());
  }

  /** Closes the picker; focus goes to `focusRoot`'s row, else back to what opened it. */
  function closePicker(focusRoot: string | null = null) {
    picking = false;
    adding = false;
    addError = null;
    addPath = "";
    pickError = null;
    if (expandedForPicker) {
      expandedForPicker = false;
      if (!collapsed) ontoggle();
    }
    void focusAfter(focusRoot);
  }

  async function addKnown(root: string) {
    if (busy) return;
    busy = true;
    pickError = null;
    try {
      const err = await onaddknown(root);
      if (err) pickError = err;
      else closePicker(root);
    } finally {
      busy = false;
    }
  }

  /** ↑/↓ move, Enter adds, Esc closes; typing anywhere in the picker goes to the search box. */
  function onPickerKey(e: KeyboardEvent) {
    // The picker owns its keys: none reaches the review's shortcuts behind it.
    e.stopPropagation();
    const inSearch = e.target === searchInput;
    if (e.key === "Escape") {
      e.preventDefault();
      if (adding) {
        adding = false;
        addError = null;
        queueMicrotask(() => searchInput?.focus());
      } else closePicker();
      return;
    }
    if (adding) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length === 0) return;
      active = e.key === "ArrowDown" ? (active + 1) % rows.length : (active - 1 + rows.length) % rows.length;
      document.getElementById(`known-${active}`)?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (e.key === "Enter" && inSearch) {
      e.preventDefault();
      const r = rows[active];
      if (r) void addKnown(r.root);
      return;
    }
    // Space (and Enter) on a button presses it; only other typing goes to the search box.
    const onButton = e.target instanceof HTMLButtonElement;
    if (!inSearch && !onButton && e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      query += e.key;
      searchInput?.focus();
    }
  }

  async function submit(path: string) {
    if (!path.trim() || busy) return;
    if (readOnly) {
      addError = READ_ONLY;
      return;
    }
    busy = true;
    addError = null;
    try {
      const err = await onadd(path.trim());
      if (err) {
        addPath = path.trim();
        addError = err;
        adding = true;
        return;
      }
      // The new project is the selected one now.
      await tick();
      closePicker(selected);
    } finally {
      busy = false;
    }
  }

  async function chooseFolder() {
    if (busy) return;
    if (readOnly) {
      addError = READ_ONLY;
      adding = true;
      return;
    }
    let path: string | null = null;
    try {
      path = await onpick();
    } catch (e) {
      // No picker (or it failed): fall back to typing the path.
      addError = `Couldn't open the folder picker: ${e}`;
      openForm();
      return;
    }
    if (path) await submit(path);
  }

  function openForm() {
    adding = true;
    queueMicrotask(() => pathInput?.focus());
  }

  function openMenu(root: string, x: number, y: number) {
    if (confirming) endConfirm();
    menu = { root, x, y };
  }

  function closeMenu(refocus = true) {
    const root = menu?.root;
    menu = null;
    if (refocus && root) queueMicrotask(() => rowButton(root)?.focus());
  }

  /** Remove from workspace: at once for a project VS Code registered, after a confirm for one the app added. */
  function askRemove(root: string) {
    if (!isAppAdded(root)) {
      void removeAndRefocus(root, false);
      return;
    }
    if (collapsed) {
      expandedForConfirm = true;
      ontoggle();
    }
    confirming = root;
    stopCapture = false;
    confirmInVsCode = null;
    if (!readOnly) {
      oninvscode(root).then(
        (v) => {
          if (confirming === root) confirmInVsCode = v;
        },
        () => {
          if (confirming === root) confirmInVsCode = false;
        },
      );
    }
    // Cancel takes focus: the safe answer is the default.
    queueMicrotask(() => cancelButton?.focus());
  }

  async function confirmRemove(root: string) {
    const stop = stopCapture && !readOnly;
    endConfirm();
    await removeAndRefocus(root, stop);
  }

  /** Removes `root` from the workspace, then focuses the next row (or the one before, or the add button). */
  async function removeAndRefocus(root: string, stop: boolean) {
    const i = projects.findIndex((p) => p.root === root);
    const next = projects[i + 1] ?? projects[i - 1] ?? null;
    await onremove(root, stop);
    const still = projects.some((p) => p.root === root);
    void focusAfter(still ? root : (next?.root ?? null));
  }

  /** Delete removes the focused project from the workspace; the menu key or Shift+F10 opens its menu. */
  function onProjectKey(e: KeyboardEvent, root: string) {
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      e.stopPropagation();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      openMenu(root, r.left + 24, r.bottom - 4);
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === "Delete") {
      e.preventDefault();
      e.stopPropagation();
      askRemove(root);
    }
  }

  function railName(p: ProjectSummary): string {
    if (p.missing) return `${p.name}, folder missing`;
    return `${p.name}, ${p.pending === 0 ? "nothing" : p.pending} pending`;
  }

  /** `/home/me/src/x` -> `~/src/x` when the home prefix is recognisable. */
  function tidy(path: string): string {
    return path.replace(/^\/(?:home|Users)\/[^/]+(?=\/|$)/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
  }

  const elsewhereText = $derived(elsewhereLine(elsewhere));
</script>

{#snippet confirmBox(p: ProjectSummary, i: number)}
  <div
    class="confirm"
    role="alertdialog"
    aria-labelledby="confirm-{i}"
    tabindex="-1"
    data-confirm
    onkeydown={(e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        cancelConfirm(p.root);
      }
    }}
  >
    <p id="confirm-{i}">Remove {p.name} from your workspace? You can add it back from Add to workspace.</p>
    <label class="check" class:disabled={readOnly}>
      <input type="checkbox" bind:checked={stopCapture} disabled={readOnly} />
      <span>Also stop capturing (forget this folder)</span>
    </label>
    {#if stopCapture || readOnly}
      {#if readOnly}
        <p class="confirm-note">{READ_ONLY}</p>
      {:else if confirmInVsCode}
        <p class="confirm-note">
          Remove {p.name} from the app's projects? VS Code also uses this folder, so capture stays on. Its review history is kept.
        </p>
      {:else}
        <p class="confirm-note">
          Stop capturing Claude's edits in {p.name}? Its review history is kept.
          <span class="confirm-warn">If this folder is open in VS Code or Cursor, capture there stops until you reload that window.</span>
        </p>
      {/if}
    {/if}
    <div class="confirm-actions">
      <button
        class="btn"
        bind:this={cancelButton}
        onclick={() => cancelConfirm(p.root)}>Cancel</button
      >
      <button class="btn danger" disabled={stopCapture && !readOnly && confirmInVsCode === null} onclick={() => void confirmRemove(p.root)}>
        Remove
      </button>
    </div>
  </div>
{/snippet}

{#snippet elsewhereButton()}
  {#if elsewhereText}
    <button class="elsewhere" title="Known projects outside your workspace have pending edits. Nothing is added on its own." onclick={openPicker}>
      <span class="dot" aria-hidden="true">●</span>
      <span class="elsewhere-text">{elsewhereText}</span>
    </button>
  {/if}
{/snippet}

{#snippet addButton()}
  <button class="add" bind:this={addButtonEl} disabled={busy} aria-haspopup="dialog" aria-expanded={picking} onclick={openPicker}>
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
    </svg>
    <span>Add<span class="add-tail">{" to workspace"}</span></span>
  </button>
{/snippet}

<nav class="projects" class:rail={collapsed} aria-label="Projects">
  <header>
    {#if !collapsed}<span class="label">Projects</span>{/if}
    <button
      class="toggle"
      aria-expanded={!collapsed}
      aria-label={collapsed ? "Expand projects" : "Collapse projects"}
      title={collapsed ? "Expand projects  [" : "Collapse projects  ["}
      onclick={ontoggle}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <rect x="2" y="3" width="12" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" />
        <path d="M6 3v10" stroke="currentColor" stroke-width="1.3" />
        <path
          d={collapsed ? "M8.8 6.5 10.3 8l-1.5 1.5" : "M10.3 6.5 8.8 8l1.5 1.5"}
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
  </header>

  {#if collapsed}
    <div class="rail-list">
      {#each projects as p (p.root)}
        <button
          class="chip"
          class:active={p.root === selected}
          class:missing={p.missing}
          data-root={p.root}
          title={p.name}
          aria-label={railName(p)}
          aria-current={p.root === selected ? "true" : undefined}
          aria-haspopup="menu"
          aria-keyshortcuts="Delete Shift+F10"
          onclick={() => onselect(p.root)}
          oncontextmenu={(e) => {
            e.preventDefault();
            openMenu(p.root, e.clientX, e.clientY);
          }}
          onkeydown={(e) => onProjectKey(e, p.root)}
        >
          <span class="initials" aria-hidden="true">{initials(p.name)}</span>
          {#if p.pending > 0}<span class="badge num" aria-hidden="true">{p.pending > 99 ? "99+" : p.pending}</span>{/if}
        </button>
      {/each}
      <button
        class="chip add-chip"
        bind:this={addChipEl}
        title={elsewhereText ? `Add to workspace · ${elsewhereText}` : "Add to workspace"}
        aria-label={elsewhereText ? `Add to workspace, ${elsewhereText}` : "Add to workspace"}
        onclick={openPicker}
      >
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
        {#if elsewhereText}<span class="chip-dot" aria-hidden="true"></span>{/if}
      </button>
    </div>
  {:else}
  <div class="list">
    {#if loading && projects.length === 0}
      {#each [0, 1, 2] as i (i)}
        <div class="skeleton" style:animation-delay="{i * 120}ms"></div>
      {/each}
    {:else if projects.length === 0}
      <div class="empty">
        <p class="empty-title">Add a project to start reviewing</p>
        <p class="empty-body">
          {#if elsewhereText}{elsewhereText}: add one to review it here.{:else}Pick a folder Claude works in, or one ClaudeGate in VS Code already captures.{/if}
        </p>
        {#if !picking}
          <div class="empty-add">{@render addButton()}</div>
        {/if}
      </div>
    {:else}
      {#each projects as p, i (p.root)}
        <div class="item">
          <button
            class="project"
            class:active={p.root === selected}
            class:missing={p.missing}
            data-root={p.root}
            title={p.root}
            aria-current={p.root === selected ? "true" : undefined}
            aria-haspopup="menu"
            aria-keyshortcuts="Delete Shift+F10"
            onclick={() => onselect(p.root)}
            oncontextmenu={(e) => {
              e.preventDefault();
              openMenu(p.root, e.clientX, e.clientY);
            }}
            onkeydown={(e) => onProjectKey(e, p.root)}
          >
            <span class="text">
              <span class="name-row">
                <span class="name">{p.name}</span>
                {#if isAppAdded(p.root)}<span class="marker" title="You added this project here; ClaudeGate in VS Code did not">Added here</span>{/if}
              </span>
              {#if p.missing}
                <span class="sub warn">Folder missing</span>
              {:else}
                <span class="sub path">{tidy(p.root)}</span>
              {/if}
            </span>
            {#if p.pending > 0}
              <span class="count num" aria-label="{p.pending} pending">{p.pending}</span>
            {:else if !p.missing}
              <span class="count num zero" aria-label="nothing pending">0</span>
            {/if}
          </button>
        </div>
        {#if confirming === p.root}{@render confirmBox(p, i)}{/if}
      {/each}
    {/if}
  </div>
  {#if picking || projects.length > 0 || loading}
  <div class="add-box" class:open={picking}>
    {#if picking}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="picker" role="dialog" aria-label="Add to workspace" tabindex="-1" onkeydown={onPickerKey}>
        <div class="picker-head">
          <input
            class="search"
            bind:this={searchInput}
            bind:value={query}
            oninput={() => (active = 0)}
            placeholder="Search projects by name or path"
            spellcheck="false"
            autocomplete="off"
            role="combobox"
            aria-expanded="true"
            aria-controls="known-list"
            aria-activedescendant={rows.length > 0 && !adding ? `known-${active}` : undefined}
            aria-label="Search known projects"
          />
          <button class="close" aria-label="Close" title="Close (Esc)" onclick={() => closePicker()}>
            <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
              <path d="M4.5 4.5 11.5 11.5M11.5 4.5 4.5 11.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
            </svg>
          </button>
        </div>
        <p class="picker-label" id="known-label">Known projects</p>
        {#if rows.length === 0}
          <p class="picker-none">
            {known.length === 0 ? "Every known project is in your workspace." : "No known project matches."}
          </p>
        {:else}
          <ul class="known" id="known-list" role="listbox" aria-labelledby="known-label">
            {#each rows as r, i (r.root)}
              <li
                id="known-{i}"
                class="known-row"
                class:active={i === active}
                role="option"
                aria-selected={i === active}
                title={r.root}
                onpointermove={() => (active = i)}
              >
                <span class="text">
                  <span class="name">{r.name}</span>
                  <span class="sub path">{shortPath(r.root, 34)}</span>
                </span>
                {#if r.pending > 0}<span class="count num" aria-label="{r.pending} pending">{r.pending}</span>{/if}
                <button class="btn add-known" tabindex="-1" disabled={busy} onclick={() => void addKnown(r.root)}>Add</button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if pickError}<p class="add-error" role="alert">{pickError}</p>{/if}
        {#if adding}
          <form
            class="add-form"
            onsubmit={(e) => {
              e.preventDefault();
              void submit(addPath);
            }}
          >
            <label class="add-label" for="add-path">Folder to add</label>
            <input
              id="add-path"
              class="path-input"
              bind:this={pathInput}
              bind:value={addPath}
              placeholder="/home/you/src/project"
              spellcheck="false"
              autocomplete="off"
              aria-invalid={addError ? "true" : undefined}
              aria-describedby={addError ? "add-error" : undefined}
            />
            {#if addError}<p id="add-error" class="add-error" role="alert">{addError}</p>{/if}
            <div class="add-actions">
              <button type="submit" class="btn primary" disabled={busy || !addPath.trim()}>Add</button>
              <button type="button" class="btn" onclick={() => void chooseFolder()}>Choose…</button>
              <button
                type="button"
                class="btn quiet"
                onclick={() => {
                  adding = false;
                  addError = null;
                }}>Cancel</button
              >
            </div>
          </form>
        {:else}
          <div class="picker-foot">
            <button class="btn" disabled={busy} title="Pick a folder; Claude's edits there are captured from then on" onclick={() => void chooseFolder()}>Choose folder…</button>
            <button class="type-path" title="Type the folder's path instead" onclick={openForm}>Type path</button>
          </div>
        {/if}
      </div>
    {:else}
      {@render elsewhereButton()}
      <div class="add-row">{@render addButton()}</div>
    {/if}
  </div>
  {:else if elsewhereText}
    <div class="add-box">{@render elsewhereButton()}</div>
  {/if}
  {/if}
</nav>

{#if menu}
  {@const m = menu}
  <ContextMenu
    x={m.x}
    y={m.y}
    items={[{ id: "remove", label: "Remove from workspace", hint: "Delete" }]}
    onpick={(id) => {
      const root = m.root;
      closeMenu(false);
      if (id === "remove") askRemove(root);
    }}
    onclose={() => closeMenu()}
  />
{/if}

<style>
  .projects {
    display: flex;
    flex-direction: column;
    min-height: 0;
    /* The column's own width, which its Splitter sets. */
    container: projects / inline-size;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    padding: 0 10px 0 20px;
    flex: none;
  }
  .rail header {
    justify-content: center;
    padding: 0;
  }

  .toggle {
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
  .toggle:hover {
    color: var(--text);
    background: var(--hover);
  }

  .rail-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 4px 0 12px;
  }
  .chip {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--muted);
    transition: background 120ms var(--ease);
  }
  .chip:hover {
    background: var(--hover);
    color: var(--text);
  }
  .chip.active {
    background: var(--active);
    color: var(--text);
  }
  .chip.missing .initials {
    color: var(--faint);
    text-decoration: line-through;
  }
  .badge {
    position: absolute;
    top: -3px;
    right: -5px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 8px;
    font-size: 10px;
    font-weight: 600;
    line-height: 16px;
    text-align: center;
    color: var(--frame);
    background: var(--pending);
  }

  .label {
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 0 10px 12px;
  }

  .item {
    position: relative;
  }
  .item + .item {
    margin-top: 2px;
  }
  .name-row {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
  }
  .marker {
    flex: none;
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--muted);
    padding: 0 5px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-chip);
    line-height: 1.5;
  }

  .confirm {
    margin: 4px 0 6px;
    padding: 10px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-control);
    background: var(--hover);
  }
  .confirm p {
    margin: 0 0 8px;
    font-size: var(--fs-small);
    line-height: 1.45;
  }
  .confirm-warn {
    display: block;
    margin-top: 4px;
    color: var(--muted);
  }
  .confirm-actions,
  .add-actions {
    display: flex;
    gap: 6px;
  }
  .btn {
    height: 26px;
    padding: 0 10px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--text);
    border: 1px solid var(--rule);
  }
  .btn:hover:not(:disabled) {
    background: var(--active);
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .btn.primary {
    border-color: var(--text);
  }
  .btn.quiet {
    border-color: transparent;
    color: var(--muted);
  }
  .btn.danger {
    color: var(--danger);
  }

  .add-box {
    flex: none;
    padding: 6px 10px 10px;
    border-top: 1px solid var(--rule);
  }
  .add-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .add {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .add:hover:not(:disabled) {
    color: var(--text);
    background: var(--hover);
  }
  .type-path {
    flex: none;
    white-space: nowrap;
    height: 26px;
    padding: 0 8px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    color: var(--faint);
  }
  .type-path:hover {
    color: var(--text);
    background: var(--hover);
  }
  .add-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .add-label {
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .path-input {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-chip);
    background: var(--frame);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: var(--fs-small);
  }
  .path-input[aria-invalid="true"] {
    border-color: var(--warning-ink);
  }
  .add-error {
    margin: 0;
    font-size: var(--fs-small);
    line-height: 1.45;
    color: var(--warning-ink);
  }
  .add-chip {
    margin-top: 4px;
    color: var(--faint);
    border: 1px dashed var(--rule);
  }

  .confirm .check {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 8px;
    font-size: var(--fs-small);
    cursor: pointer;
  }
  .check.disabled {
    color: var(--muted);
    cursor: default;
  }
  .confirm .confirm-note {
    padding: 6px 8px;
    border-left: 2px solid var(--danger);
    background: var(--frame);
    border-radius: var(--radius-chip);
  }

  .add-box.open {
    padding-top: 8px;
  }
  .elsewhere {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: 28px;
    margin-bottom: 2px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    text-align: left;
    color: var(--text);
  }
  .elsewhere:hover {
    background: var(--hover);
  }
  .elsewhere .dot {
    flex: none;
    font-size: 9px;
    color: var(--pending);
  }
  .elsewhere-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip-dot {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--pending);
  }

  .picker {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: min(440px, 62vh);
    min-height: 0;
  }
  .picker:focus {
    outline: none;
  }
  .picker-head {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .search {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-chip);
    background: var(--frame);
    color: var(--text);
    font-size: var(--fs-small);
  }
  .search:focus {
    border-color: var(--focus-ring);
    outline: none;
  }
  .close {
    display: grid;
    place-items: center;
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-control);
    color: var(--faint);
  }
  .close:hover {
    color: var(--text);
    background: var(--hover);
  }
  .picker-label {
    margin: 2px 2px 0;
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--muted);
  }
  .picker-none {
    margin: 0 2px;
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .known {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .known-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 3px 4px 3px 8px;
    border-radius: var(--radius-control);
  }
  .known-row.active {
    background: var(--active);
  }
  .known-row .name {
    font-size: var(--fs-small);
  }
  .add-known {
    flex: none;
    height: 24px;
    padding: 0 8px;
  }
  .picker-foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    padding-top: 6px;
    border-top: 1px solid var(--rule);
  }
  .empty-add {
    margin-top: 10px;
  }
  .empty-add .add {
    flex: none;
    width: auto;
    border: 1px dashed var(--rule);
  }

  .project {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 44px;
    padding: 5px 10px;
    border-radius: var(--radius-control);
    transition: background 120ms var(--ease);
  }
  .project:hover {
    background: var(--hover);
  }
  .project.active {
    background: var(--active);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .name {
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .missing .name {
    color: var(--muted);
  }

  .sub {
    font-size: var(--fs-meta);
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub.path {
    font-family: var(--font-mono);
  }
  .sub.warn {
    color: var(--warning-ink);
    font-weight: 500;
  }

  .count {
    flex: none;
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--pending);
  }
  .count.zero {
    color: var(--faint);
    font-weight: 400;
  }

  .skeleton {
    height: 44px;
    margin: 2px 0;
    border-radius: var(--radius-control);
    background: var(--hover);
    animation: pulse 1.4s ease-in-out infinite both;
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }

  .empty {
    padding: 12px 10px;
  }

  /* A narrow column: the name keeps its room. "Added here" and the footer's "project" stay
     for screen readers; the remove button still marks a project you added, on hover. */
  @container projects (max-width: 199px) {
    .marker,
    .add-tail {
      position: absolute;
      width: 1px;
      height: 1px;
      padding: 0;
      border: 0;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }
  }
  .empty-title {
    margin: 0 0 4px;
    font-weight: 500;
  }
  .empty-body {
    margin: 0;
    font-size: var(--fs-small);
    color: var(--muted);
    line-height: 1.5;
  }
</style>
