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
  } = $props();

  let showHidden = $state(false);

  /** `h` hides the focused project, as its Hide button does. */
  function onProjectKey(e: KeyboardEvent, root: string) {
    if (e.key !== "h" || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    e.stopPropagation();
    onhide(root);
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
        <p class="empty-body">Open a folder in VS Code with ClaudeGate installed and it will show up here.</p>
      </div>
    {:else}
      {#each projects as p (p.root)}
        <div class="item">
          <button
            class="project"
            class:active={p.root === selected}
            class:missing={p.missing}
            title={p.root}
            aria-current={p.root === selected ? "true" : undefined}
            aria-keyshortcuts="h"
            onclick={() => onselect(p.root)}
            onkeydown={(e) => onProjectKey(e, p.root)}
          >
            <span class="text">
              <span class="name">{p.name}</span>
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
          <button class="hide" tabindex="-1" aria-label="Hide {p.name}" title="Hide this project (H)" onclick={() => onhide(p.root)}>
            <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
              <path d="M2 8s2.2-4 6-4 6 4 6 4-2.2 4-6 4-6-4-6-4Z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
              <circle cx="8" cy="8" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
              <path d="M3 13 13 3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
            </svg>
          </button>
        </div>
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
          {#each hidden as p (p.root)}
            <li>
              <button class="hidden-name" class:active={p.root === selected} title={p.root} onclick={() => onselect(p.root)}>{p.name}</button>
              <button class="show" aria-label="Show {p.name}" title="Show this project again" onclick={() => onshow(p.root)}>Show</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
  {/if}
</nav>

<style>
  .projects {
    display: flex;
    flex-direction: column;
    min-height: 0;
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
    background: var(--hover);
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms var(--ease);
  }
  .item:hover .hide,
  .item:focus-within .hide {
    opacity: 1;
    pointer-events: auto;
  }
  .item:has(.project.active) .hide {
    background: var(--active);
  }
  .hide:hover {
    color: var(--text);
  }
  .item:hover .count,
  .item:focus-within .count {
    visibility: hidden;
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
