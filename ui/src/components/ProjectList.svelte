<script lang="ts">
  import type { ProjectSummary } from "../lib/api";
  import { initials } from "../lib/rows";

  let {
    projects,
    selected,
    loading = false,
    collapsed = false,
    onselect,
    ontoggle,
    hidden = [],
    onhide = () => {},
    onshow = () => {},
    appRoots = [],
    onpick = async () => null,
    onadd = async () => null,
    onremove = async () => {},
    oninvscode = async () => false,
    readOnly = false,
  }: {
    projects: ProjectSummary[];
    selected: string | null;
    loading?: boolean;
    /** Shown as a rail of initials, for more review space. */
    collapsed?: boolean;
    onselect: (root: string) => void;
    ontoggle: () => void;
    /** Projects the user hid: listed only in the "Hidden projects" disclosure. */
    hidden?: ProjectSummary[];
    onhide?: (root: string) => void;
    onshow?: (root: string) => void;
    /** Roots the app added: marked "Added here" and removable. */
    appRoots?: string[];
    /** The native folder picker; null when cancelled. */
    onpick?: () => Promise<string | null>;
    /** Adds and selects the project; resolves to an error message, or null on success. */
    onadd?: (path: string) => Promise<string | null>;
    onremove?: (root: string) => Promise<void>;
    /** Whether VS Code or Cursor also uses the folder (Remove then leaves capture on). */
    oninvscode?: (root: string) => Promise<boolean>;
    /** Add and Remove write a shared file, so read-only mode blocks them. */
    readOnly?: boolean;
  } = $props();

  const READ_ONLY = "Read-only mode is on. Turn it off in Settings to add or remove projects.";

  let showHidden = $state(false);

  /** The typed-path form: the keyboard route, and where a refused add explains itself. */
  let adding = $state(false);
  let addPath = $state("");
  let addError = $state<string | null>(null);
  let busy = $state(false);
  let pathInput = $state<HTMLInputElement | null>(null);
  /** The project whose Remove is awaiting confirmation. */
  let confirming = $state<string | null>(null);
  let cancelButton = $state<HTMLButtonElement | null>(null);
  /** For the confirmation's wording: null while being checked. */
  let confirmInVsCode = $state<boolean | null>(null);

  const isAppAdded = (root: string) => appRoots.includes(root);

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
      adding = false;
      addPath = "";
    } finally {
      busy = false;
    }
  }

  async function chooseFolder() {
    if (busy) return;
    if (collapsed) ontoggle();
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

  function closeForm() {
    adding = false;
    addError = null;
    addPath = "";
  }

  function askRemove(root: string) {
    confirming = root;
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
    confirming = null;
    await onremove(root);
  }

  /** `h` hides the focused project, as its Hide button does; Delete asks to remove an app-added one. */
  function onProjectKey(e: KeyboardEvent, root: string) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === "h") {
      e.preventDefault();
      e.stopPropagation();
      onhide(root);
    } else if ((e.key === "Delete" || e.key === "Backspace") && isAppAdded(root)) {
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
</script>

{#snippet confirmBox(p: ProjectSummary, i: number)}
  <div
    class="confirm"
    role="alertdialog"
    aria-labelledby="confirm-{i}"
    tabindex="-1"
    onkeydown={(e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        confirming = null;
      }
    }}
  >
    {#if readOnly}
      <p id="confirm-{i}">{READ_ONLY}</p>
    {:else if confirmInVsCode}
      <p id="confirm-{i}">
        Remove {p.name} from the app's projects? VS Code also uses this folder, so capture stays on. Its review history is kept.
      </p>
    {:else}
      <p id="confirm-{i}">
        Stop capturing Claude's edits in {p.name}? Its review history is kept.
        <span class="confirm-warn">If this folder is open in VS Code or Cursor, capture there stops until you reload that window.</span>
      </p>
    {/if}
    <div class="confirm-actions">
      <button class="btn" bind:this={cancelButton} onclick={() => (confirming = null)}>Cancel</button>
      {#if !readOnly}
        <button class="btn danger" disabled={confirmInVsCode === null} onclick={() => void confirmRemove(p.root)}>Remove</button>
      {/if}
    </div>
  </div>
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
          title={p.name}
          aria-label={railName(p)}
          aria-current={p.root === selected ? "true" : undefined}
          onclick={() => onselect(p.root)}
        >
          <span class="initials" aria-hidden="true">{initials(p.name)}</span>
          {#if p.pending > 0}<span class="badge num" aria-hidden="true">{p.pending > 99 ? "99+" : p.pending}</span>{/if}
        </button>
      {/each}
      <button class="chip add-chip" title="Add project" aria-label="Add project" onclick={() => void chooseFolder()}>
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
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
        <p class="empty-title">No projects yet</p>
        <p class="empty-body">Open a folder in VS Code with ClaudeGate installed, or add one below, and it will show up here.</p>
      </div>
    {:else}
      {#each projects as p, i (p.root)}
        <div class="item">
          <button
            class="project"
            class:active={p.root === selected}
            class:missing={p.missing}
            title={p.root}
            aria-current={p.root === selected ? "true" : undefined}
            aria-keyshortcuts={isAppAdded(p.root) ? "h Delete" : "h"}
            onclick={() => onselect(p.root)}
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
          {#if isAppAdded(p.root)}
            <button class="remove" tabindex="-1" aria-label="Remove {p.name}" title="Remove this project (Delete)" onclick={() => askRemove(p.root)}>
              <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
                <path d="M4.5 4.5 11.5 11.5M11.5 4.5 4.5 11.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
              </svg>
            </button>
          {/if}
          <button class="hide" tabindex="-1" aria-label="Hide {p.name}" title="Hide this project (H)" onclick={() => onhide(p.root)}>
            <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
              <path d="M2 8s2.2-4 6-4 6 4 6 4-2.2 4-6 4-6-4-6-4Z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
              <circle cx="8" cy="8" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
              <path d="M3 13 13 3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
            </svg>
          </button>
        </div>
        {#if confirming === p.root}{@render confirmBox(p, i)}{/if}
      {/each}
    {/if}
  </div>
  {#if hidden.length > 0}
    <div class="hidden-box">
      <button class="disclosure" aria-expanded={showHidden} onclick={() => (showHidden = !showHidden)}>
        <svg class="chev" class:open={showHidden} viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M6 4.5 9.5 8 6 11.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        Hidden projects ({hidden.length})
      </button>
      {#if showHidden}
        <ul class="hidden-list">
          {#each hidden as p, i (p.root)}
            <li>
              <button class="hidden-name" class:active={p.root === selected} title={p.root} onclick={() => onselect(p.root)}>{p.name}</button>
              {#if isAppAdded(p.root)}
                <button class="show" aria-label="Remove {p.name}" title="Remove this project" onclick={() => askRemove(p.root)}>Remove</button>
              {/if}
              <button class="show" aria-label="Show {p.name}" title="Show this project again" onclick={() => onshow(p.root)}>Show</button>
            </li>
            {#if confirming === p.root}<li class="confirm-row">{@render confirmBox(p, projects.length + i)}</li>{/if}
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
  <div class="add-box">
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
          onkeydown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              closeForm();
            }
          }}
        />
        {#if addError}<p id="add-error" class="add-error" role="alert">{addError}</p>{/if}
        <div class="add-actions">
          <button type="submit" class="btn primary" disabled={busy || !addPath.trim()}>Add</button>
          <button type="button" class="btn" onclick={() => void chooseFolder()}>Choose…</button>
          <button type="button" class="btn quiet" onclick={closeForm}>Cancel</button>
        </div>
      </form>
    {:else}
      <div class="add-row">
        <button class="add" disabled={busy} onclick={() => void chooseFolder()}>
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          </svg>
          <span>Add<span class="add-tail">{" project"}</span></span>
        </button>
        <button class="type-path" title="Type the folder's path instead" onclick={openForm}>Type path</button>
      </div>
    {/if}
  </div>
  {/if}
</nav>

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
  /* Hide: on hover or focus, over the count. */
  .hide {
    position: absolute;
    top: 50%;
    right: 6px;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-top: -13px;
    border-radius: var(--radius-chip);
    color: var(--muted);
    opacity: 0;
    pointer-events: none;
    transition:
      opacity 120ms var(--ease),
      background 120ms var(--ease);
  }
  /* Filled with the row's own tone only while shown, so a long name runs under it, not through it. */
  .item:hover .hide,
  .item:focus-within .hide {
    opacity: 1;
    pointer-events: auto;
    background: var(--hover);
  }
  .item:has(.project.active):hover .hide,
  .item:has(.project.active):focus-within .hide {
    background: var(--active);
  }
  .item .hide:hover,
  .item:has(.project.active) .hide:hover:hover {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 9%, var(--hover));
  }
  .item:hover .count,
  .item:focus-within .count {
    visibility: hidden;
  }
  /* Remove: beside Hide, only on projects the app added. */
  .remove {
    position: absolute;
    top: 50%;
    right: 36px;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-top: -13px;
    border-radius: var(--radius-chip);
    color: var(--muted);
    opacity: 0;
    pointer-events: none;
    transition:
      opacity 120ms var(--ease),
      background 120ms var(--ease);
  }
  .item:hover .remove,
  .item:focus-within .remove {
    opacity: 1;
    pointer-events: auto;
    background: var(--hover);
  }
  .item:has(.project.active):hover .remove,
  .item:has(.project.active):focus-within .remove {
    background: var(--active);
  }
  .item .remove:hover,
  .item:has(.project.active) .remove:hover:hover {
    color: var(--danger);
    background: color-mix(in srgb, var(--text) 9%, var(--hover));
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
  .hidden-list li.confirm-row {
    display: block;
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

  .hidden-box {
    flex: none;
    padding: 6px 10px 12px;
    border-top: 1px solid var(--rule);
  }
  .disclosure {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    height: 28px;
    padding: 0 6px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .disclosure:hover {
    color: var(--text);
    background: var(--hover);
  }
  .chev {
    transition: transform 120ms var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .hidden-list {
    margin: 2px 0 0;
    padding: 0;
    list-style: none;
  }
  .hidden-list li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 24px;
  }
  .hidden-name {
    flex: 1;
    min-width: 0;
    height: 28px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .hidden-name.active {
    color: var(--text);
    font-weight: 500;
  }
  .show {
    flex: none;
    height: 24px;
    padding: 0 8px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--text);
  }
  .show:hover {
    background: var(--hover);
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
