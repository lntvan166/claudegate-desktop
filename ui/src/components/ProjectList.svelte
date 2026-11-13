<script lang="ts">
  import type { ProjectSummary } from "../lib/api";

  let {
    projects,
    selected,
    loading = false,
    onselect,
  }: {
    projects: ProjectSummary[];
    selected: string | null;
    loading?: boolean;
    onselect: (root: string) => void;
  } = $props();

  /** `/home/me/src/x` -> `~/src/x` when the home prefix is recognisable. */
  function tidy(path: string): string {
    return path.replace(/^\/(?:home|Users)\/[^/]+(?=\/|$)/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
  }
</script>

<nav class="projects" aria-label="Projects">
  <header>
    <span class="label">Projects</span>
  </header>

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
        <button
          class="project"
          class:active={p.root === selected}
          class:missing={p.missing}
          title={p.root}
          aria-current={p.root === selected ? "true" : undefined}
          onclick={() => onselect(p.root)}
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
      {/each}
    {/if}
  </div>
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
    height: 32px;
    padding: 0 20px;
    flex: none;
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
  .project + .project {
    margin-top: 2px;
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
