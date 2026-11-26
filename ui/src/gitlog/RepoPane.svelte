<script lang="ts">
  import { tick } from "svelte";
  import ContextMenu from "../components/ContextMenu.svelte";
  import { writeClipboard } from "../lib/clipboard";
  import {
    gitlogExclude,
    gitlogInclude,
    gitlogProfileApply,
    gitlogProfileDelete,
    gitlogProfileRename,
    gitlogProfileSave,
    gitlogScanDepth,
    gitlogShowAll,
    gitlogShowOnly,
    gitlogTick,
    gitlogUntick,
  } from "../lib/api";
  import {
    SCAN_DEPTH_MAX,
    aheadBehind,
    applyProfile,
    colorVar,
    fuzzyFilter,
    hasProfile,
    isTicked,
    profileNames,
    rangeOf,
    repoPath,
    tickedCount,
    worktreesOf,
    type GitLogConfig,
    type RepoView,
  } from "./repos";

  // The Repositories pane: which repos the merged log shows. Every change goes to config.json
  // through one add-only, remove-only or set-one-key command, and the pane shows what it answers.
  let {
    repos,
    error,
    cfg = $bindable(),
    collapsed,
    refreshing,
    ontoggle,
    onrefresh,
    onnotice,
    onslash,
  }: {
    /** null while the first discovery runs. */
    repos: RepoView[] | null;
    error: string | null;
    cfg: GitLogConfig;
    collapsed: boolean;
    refreshing: boolean;
    ontoggle: () => void;
    onrefresh: () => void;
    onnotice: (text: string) => void;
    /** `/` in the empty filter: the Git log search takes it. */
    onslash?: () => void;
  } = $props();

  let query = $state("");
  const all = $derived(repos ?? []);
  const shown = $derived(fuzzyFilter(all, query));
  const order = $derived(shown.map((r) => r.id));
  const nTicked = $derived(tickedCount(cfg.ticked, all));
  const names = $derived(profileNames(cfg.profiles));

  /** The selected rows (multi-select); `anchor` is where a Shift range starts. */
  let selection = $state<string[]>([]);
  let anchor = $state<string | null>(null);
  /** The row with keyboard focus (roving tabindex). */
  let cursor = $state<string | null>(null);
  let listEl: HTMLDivElement | undefined = $state();
  let searchEl: HTMLInputElement | undefined = $state();

  let menu = $state<{ x: number; y: number; ids: string[] } | null>(null);
  let profilesOpen = $state(false);
  let profilesEl: HTMLDivElement | undefined = $state();
  let profilesBtn: HTMLButtonElement | undefined = $state();
  let saveName = $state("");
  let renaming = $state<{ from: string; to: string } | null>(null);
  let deleting = $state<string | null>(null);
  let discoveryOpen = $state(false);
  let newPattern = $state("");
  let busy = $state(false);

  // Rows that left the list (filtered out, or no longer discovered) leave the selection too.
  $effect(() => {
    const visible = new Set(order);
    if (selection.some((id) => !visible.has(id))) selection = selection.filter((id) => visible.has(id));
    if (cursor && !visible.has(cursor)) cursor = order[0] ?? null;
  });

  /** The profile whose repos are exactly the ticked ones, if any. */
  const activeProfile = $derived.by(() => {
    if (cfg.ticked === null || all.length === 0) return null;
    const on = new Set(all.filter((r) => isTicked(cfg.ticked, r.id)).map((r) => r.id));
    return (
      names.find((n) => {
        const p = new Set(cfg.profiles[n].filter((id) => all.some((r) => r.id === id)));
        return p.size === on.size && [...p].every((id) => on.has(id));
      }) ?? null
    );
  });
  const profileLabel = $derived(cfg.ticked === null ? "All repositories" : (activeProfile ?? "Custom selection"));

  async function run(what: string, f: () => Promise<GitLogConfig>): Promise<boolean> {
    if (busy) return false;
    busy = true;
    try {
      cfg = await f();
      return true;
    } catch (e) {
      onnotice(`Couldn't ${what}: ${e}`);
      return false;
    } finally {
      busy = false;
    }
  }

  const nameOf = (id: string) => all.find((r) => r.id === id)?.name ?? id;
  const many = (ids: string[]) => (ids.length === 1 ? nameOf(ids[0]) : `${ids.length} repositories`);

  /** The rows an action on `id` covers: the selection when `id` is part of it, else `id` alone. */
  function targets(id: string): string[] {
    return selection.includes(id) && selection.length > 1 ? selection.slice() : [id];
  }

  async function toggleTick(id: string) {
    const ids = targets(id);
    const on = !isTicked(cfg.ticked, id);
    await run(on ? "show it" : "hide it", () => (on ? gitlogTick(ids) : gitlogUntick(ids)));
  }

  async function showOnly(ids: string[]) {
    if (await run("show only that", () => gitlogShowOnly(ids))) onnotice(`Showing only ${many(ids)}`);
  }
  async function hide(ids: string[]) {
    if (await run("hide it", () => gitlogUntick(ids))) onnotice(`${many(ids)} hidden`);
  }
  async function showAll() {
    if (await run("show every repository", () => gitlogShowAll())) onnotice("Showing every repository");
  }

  function select(id: string, e: MouseEvent | KeyboardEvent) {
    cursor = id;
    if (e.shiftKey) {
      selection = rangeOf(order, anchor, id);
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      selection = selection.includes(id) ? selection.filter((x) => x !== id) : [...selection, id];
      anchor = id;
      return;
    }
    selection = [id];
    anchor = id;
  }

  async function focusRow(id: string | null) {
    cursor = id;
    await tick();
    if (id) listEl?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`)?.focus();
  }

  function rowKey(e: KeyboardEvent, id: string) {
    const i = order.indexOf(id);
    const move = (j: number) => {
      const next = order[Math.max(0, Math.min(order.length - 1, j))];
      if (!next) return;
      if (e.shiftKey) selection = rangeOf(order, anchor ?? id, next);
      else {
        selection = [next];
        anchor = next;
      }
      void focusRow(next);
    };
    if (e.key === "ArrowDown") move(i + 1);
    else if (e.key === "ArrowUp") {
      if (i === 0 && !e.shiftKey) {
        searchEl?.focus();
      } else move(i - 1);
    } else if (e.key === "Home") move(0);
    else if (e.key === "End") move(order.length - 1);
    else if (e.key === " ") {
      if (!selection.includes(id)) selection = [id];
      void toggleTick(id);
    } else if ((e.key === "a" || e.key === "A") && (e.ctrlKey || e.metaKey)) {
      selection = order.slice();
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      openMenu(id, r.left + 24, r.bottom);
    } else if (e.key === "Escape" && selection.length > 1) {
      selection = [id];
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  /** Where the keyboard lands when the view comes on screen: the list's current row, else the filter. */
  export async function focusStart() {
    await tick();
    if (collapsed) {
      document.querySelector<HTMLElement>('.repos button[aria-expanded]')?.focus();
      return;
    }
    const id = cursor ?? order[0] ?? null;
    const row = id ? listEl?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`) : null;
    (row ?? searchEl)?.focus();
  }

  function openMenu(id: string, x: number, y: number) {
    if (!selection.includes(id)) {
      selection = [id];
      anchor = id;
    }
    cursor = id;
    menu = { x, y, ids: targets(id) };
  }

  const menuItems = (ids: string[]) => {
    const n = ids.length;
    return [
      { id: "only", label: n === 1 ? "Show only this" : `Show only these ${n}` },
      { id: "hide", label: n === 1 ? "Hide" : `Hide these ${n}` },
      { id: "all", label: "Show all" },
      { id: "copy", label: n === 1 ? "Copy path" : `Copy ${n} paths` },
    ];
  };

  async function pick(action: string, ids: string[]) {
    menu = null;
    if (action === "only") await showOnly(ids);
    else if (action === "hide") await hide(ids);
    else if (action === "all") await showAll();
    else if (action === "copy") {
      const paths = ids.map((id) => {
        const r = all.find((x) => x.id === id);
        return r ? repoPath(r) : id;
      });
      try {
        await writeClipboard(paths.join("\n"));
        onnotice(n1(paths.length, "path") + " copied");
      } catch (e) {
        onnotice(`Couldn't copy: ${e}`);
      }
    }
    if (cursor) void focusRow(cursor);
  }
  const n1 = (n: number, w: string) => (n === 1 ? `1 ${w}` : `${n} ${w}s`);

  function searchKey(e: KeyboardEvent) {
    if (e.key === "Escape" && query) {
      query = "";
      e.preventDefault();
      e.stopPropagation();
    } else if (e.key === "/" && !query && onslash) {
      e.preventDefault();
      onslash();
    } else if (e.key === "ArrowDown" || e.key === "Enter") {
      const first = order[0];
      if (!first) return;
      e.preventDefault();
      selection = [first];
      anchor = first;
      void focusRow(first);
    }
  }

  // ---- profiles ----
  async function openProfiles() {
    profilesOpen = !profilesOpen;
    renaming = null;
    deleting = null;
    saveName = "";
    if (profilesOpen) {
      await tick();
      profilesEl?.querySelector<HTMLElement>("button, input")?.focus();
    }
  }
  function closeProfiles() {
    profilesOpen = false;
    profilesBtn?.focus();
  }
  function profilesOutside(e: PointerEvent) {
    if (!profilesOpen) return;
    const t = e.target as Node;
    if (!profilesEl?.contains(t) && !profilesBtn?.contains(t)) profilesOpen = false;
  }

  async function apply(name: string) {
    const { missing } = applyProfile(cfg, cfg.profiles[name] ?? [], all);
    if (!(await run("apply the profile", () => gitlogProfileApply(name)))) return;
    profilesOpen = false;
    onnotice(
      missing > 0
        ? `${name} applied; ${n1(missing, "repository")} in it ${missing === 1 ? "isn't" : "aren't"} in your workspace any more`
        : `${name} applied`,
    );
    profilesBtn?.focus();
  }
  async function save() {
    const name = saveName.trim();
    if (!name) return;
    const replacing = hasProfile(cfg.profiles, name);
    if (await run("save the profile", () => gitlogProfileSave(name))) {
      onnotice(replacing ? `${name} updated` : `${name} saved`);
      saveName = "";
    }
  }
  async function rename() {
    const r = renaming;
    if (!r) return;
    const to = r.to.trim();
    if (!to || to === r.from) {
      renaming = null;
      return;
    }
    if (await run("rename the profile", () => gitlogProfileRename(r.from, to))) {
      renaming = null;
      onnotice(`${r.from} renamed to ${to}`);
    }
  }
  async function remove(name: string) {
    deleting = null;
    if (await run("delete the profile", () => gitlogProfileDelete(name))) onnotice(`${name} deleted`);
  }
  function popKey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    if (renaming) renaming = null;
    else if (deleting) deleting = null;
    else closeProfiles();
  }
  async function startRename(name: string) {
    deleting = null;
    renaming = { from: name, to: name };
    await tick();
    profilesEl?.querySelector<HTMLInputElement>("input[data-rename]")?.select();
  }

  // ---- discovery ----
  async function setDepth(d: number) {
    if (d < 0 || d > SCAN_DEPTH_MAX) return;
    if (await run("change the scan depth", () => gitlogScanDepth(d))) onrefresh();
  }
  async function addPattern() {
    const p = newPattern.trim();
    if (!p) return;
    if (await run("exclude it", () => gitlogExclude(p))) {
      newPattern = "";
      onrefresh();
    }
  }
  async function removePattern(p: string) {
    if (await run("include it again", () => gitlogInclude(p))) onrefresh();
  }

  const worktreeTitle = (r: RepoView) =>
    worktreesOf(r)
      .map((c) => `${c.branch ?? "detached"}  ${c.path}`)
      .join("\n");
</script>

<svelte:window onpointerdown={profilesOutside} />

{#if menu}
  {@const m = menu}
  <ContextMenu x={m.x} y={m.y} items={menuItems(m.ids)} onpick={(id) => void pick(id, m.ids)} onclose={() => (menu = null)} />
{/if}

<nav class="repos" class:rail={collapsed} aria-label="Repositories">
  <header>
    {#if !collapsed}<span class="label">Repositories</span>{/if}
    <div class="head-actions">
      {#if !collapsed}
        <button class="icon" aria-label="Refresh the repository list" title="Look for repositories again" disabled={refreshing} onclick={onrefresh}>
          <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true" class:spin={refreshing}>
            <path d="M13 8a5 5 0 1 1-1.5-3.6M13 2.5v2.8h-2.8" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </button>
      {/if}
      <button
        class="icon"
        aria-expanded={!collapsed}
        aria-label={collapsed ? "Expand repositories" : "Collapse repositories"}
        title={collapsed ? "Expand repositories" : "Collapse repositories"}
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
    </div>
  </header>

  {#if collapsed}
    <ul class="rail-list" aria-label="Shown repositories">
      {#each all.filter((r) => isTicked(cfg.ticked, r.id)) as r (r.id)}
        <li class="dot" style:--c={colorVar(r.color)} title={r.name}><span class="sr">{r.name}</span></li>
      {/each}
    </ul>
  {:else}
    <div class="profile-row">
      <button
        class="profile-btn"
        bind:this={profilesBtn}
        aria-haspopup="dialog"
        aria-expanded={profilesOpen}
        title="Profiles: named sets of repositories"
        onclick={() => void openProfiles()}
      >
        <span class="profile-name">{profileLabel}</span>
        <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true"><path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
      </button>
      <span class="count" aria-live="polite">{repos ? `${nTicked} of ${all.length}` : ""}</span>
    </div>

    {#if profilesOpen}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="pop" role="dialog" aria-label="Profiles" tabindex="-1" bind:this={profilesEl} onkeydown={popKey}>
        {#if names.length === 0}
          <p class="pop-empty">No profiles yet. Tick the repositories you want together, then save them under a name.</p>
        {:else}
          <ul class="profiles">
            {#each names as name (name)}
              {@const n = cfg.profiles[name].length}
              <li class:current={name === activeProfile}>
                {#if renaming?.from === name}
                  <form class="inline" onsubmit={(e) => (e.preventDefault(), void rename())}>
                    <input data-rename aria-label="New name for {name}" bind:value={renaming.to} maxlength="80" />
                    <button class="small go" type="submit" disabled={busy}>Rename</button>
                  </form>
                {:else if deleting === name}
                  <div class="inline ask">
                    <span>Delete {name}?</span>
                    <button class="small danger" disabled={busy} onclick={() => void remove(name)}>Delete</button>
                    <button class="small" onclick={() => (deleting = null)}>Cancel</button>
                  </div>
                {:else}
                  <button class="apply" disabled={busy} onclick={() => void apply(name)} title="Show exactly these {n} repositories">
                    <span class="pname">{name}</span>
                    <span class="pcount">{n}</span>
                  </button>
                  <button class="icon" aria-label="Rename {name}" title="Rename" onclick={() => void startRename(name)}>
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M10.5 3.5l2 2L6 12H4v-2z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /></svg>
                  </button>
                  <button class="icon" aria-label="Delete {name}" title="Delete" onclick={() => ((deleting = name), (renaming = null))}>
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /></svg>
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
        <form class="save" onsubmit={(e) => (e.preventDefault(), void save())}>
          <label class="save-label" for="profile-save">Save the {cfg.ticked === null ? "whole list" : `${nTicked} shown`} as</label>
          <div class="inline">
            <input id="profile-save" placeholder="Profile name" bind:value={saveName} maxlength="80" />
            <button class="small go" type="submit" disabled={busy || !saveName.trim()}>{hasProfile(cfg.profiles, saveName) ? "Replace" : "Save"}</button>
          </div>
        </form>
      </div>
    {/if}

    <div class="search">
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" stroke-width="1.3" /><path d="M10.2 10.2 13 13" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /></svg>
      <input bind:this={searchEl} bind:value={query} placeholder="Filter repositories" aria-label="Filter repositories" onkeydown={searchKey} spellcheck="false" />
    </div>

    <div
      class="list"
      role="listbox"
      aria-label="Repositories"
      aria-multiselectable="true"
      tabindex="-1"
      bind:this={listEl}
    >
      {#if error}
        <p class="state error">Couldn't list the repositories: {error}</p>
      {:else if !repos}
        {#each [70, 54, 62, 46] as w, i (i)}<div class="skeleton" style:width="{w}%"></div>{/each}
      {:else if all.length === 0}
        <p class="state">No git repositories in your workspace. Add a project that is a repository, or one with repositories inside it.</p>
      {:else if shown.length === 0}
        <p class="state">No repository matches. Press Esc in the filter to clear it.</p>
      {:else}
        {#each shown as r (r.id)}
          {@const on = isTicked(cfg.ticked, r.id)}
          {@const wt = worktreesOf(r).length}
          {@const ab = aheadBehind(r)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="row"
            class:selected={selection.includes(r.id)}
            class:off={!on}
            role="option"
            aria-selected={selection.includes(r.id)}
            aria-checked={on}
            aria-describedby="repo-tick-hint"
            tabindex={(cursor ?? order[0]) === r.id ? 0 : -1}
            data-id={r.id}
            title={repoPath(r)}
            onclick={(e) => select(r.id, e)}
            onkeydown={(e) => rowKey(e, r.id)}
            oncontextmenu={(e) => {
              e.preventDefault();
              openMenu(r.id, e.clientX, e.clientY);
            }}
          >
            <!-- The row is the checkbox for assistive tech (aria-checked, Space); this is its mouse target. -->
            <button
              class="tick"
              class:on
              aria-hidden="true"
              tabindex="-1"
              disabled={busy}
              onclick={(e) => {
                e.stopPropagation();
                void toggleTick(r.id);
              }}
            >
              {#if on}<svg viewBox="0 0 16 16" width="10" height="10" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
            </button>
            <span class="dot" style:--c={colorVar(r.color)} aria-hidden="true"></span>
            <span class="name">{r.name}</span>
            {#if wt > 0}
              <span class="wt" title={worktreeTitle(r)} aria-label="{wt} worktree{wt === 1 ? '' : 's'}">
                <svg viewBox="0 0 16 16" width="10" height="10" aria-hidden="true"><circle cx="5" cy="4" r="1.6" fill="none" stroke="currentColor" stroke-width="1.3" /><circle cx="5" cy="12" r="1.6" fill="none" stroke="currentColor" stroke-width="1.3" /><circle cx="11" cy="6" r="1.6" fill="none" stroke="currentColor" stroke-width="1.3" /><path d="M5 5.6v4.8M11 7.6c0 2-2 2.4-5 3.2" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>{wt}
              </span>
            {/if}
            {#if ab}<span class="ab" title="Behind / ahead of its upstream, as of the last fetch">{ab}</span>{/if}
          </div>
        {/each}
      {/if}
    </div>

    <p id="repo-tick-hint" class="sr">Space shows or hides the selected repositories in the log</p>

    <footer>
      <button class="disclose" aria-expanded={discoveryOpen} onclick={() => (discoveryOpen = !discoveryOpen)}>
        <svg viewBox="0 0 16 16" width="10" height="10" aria-hidden="true" class:open={discoveryOpen}><path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
        Discovery
      </button>
      {#if discoveryOpen}
        <div class="discovery">
          <div class="depth">
            <span id="depth-label">Folder depth</span>
            <div class="stepper" role="group" aria-labelledby="depth-label">
              <button class="small" aria-label="Shallower" disabled={busy || cfg.scanDepth <= 0} onclick={() => void setDepth(cfg.scanDepth - 1)}>−</button>
              <span class="depth-n">{cfg.scanDepth}</span>
              <button class="small" aria-label="Deeper" disabled={busy || cfg.scanDepth >= SCAN_DEPTH_MAX} onclick={() => void setDepth(cfg.scanDepth + 1)}>+</button>
            </div>
          </div>
          <p class="hint">How many folders below each workspace project to look for repositories.</p>
          <span class="sub">Skipped folders</span>
          {#if cfg.excludeRepos.length > 0}
            <ul class="patterns">
              {#each cfg.excludeRepos as p (p)}
                <li>
                  <code>{p}</code>
                  <button class="icon" aria-label="Look in {p} again" title="Look here again" disabled={busy} onclick={() => void removePattern(p)}>
                    <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /></svg>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <form class="inline" onsubmit={(e) => (e.preventDefault(), void addPattern())}>
            <input placeholder="Folder name or glob" aria-label="Folder name or glob to skip" bind:value={newPattern} spellcheck="false" />
            <button class="small" type="submit" disabled={busy || !newPattern.trim()}>Skip</button>
          </form>
        </div>
      {/if}
    </footer>
  {/if}
</nav>

<style>
  .repos {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    container: repos / inline-size;
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
  .label {
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .head-actions {
    display: flex;
    gap: 2px;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex: none;
    border-radius: var(--radius-control);
    color: var(--faint);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
    background: var(--hover);
  }
  .icon:disabled {
    opacity: 0.5;
  }
  .spin {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }

  .profile-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 12px 0 14px;
    flex: none;
  }
  .profile-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    height: 26px;
    padding: 0 6px;
    border-radius: var(--radius-control);
    font-weight: 600;
    color: var(--text);
  }
  .profile-btn:hover,
  .profile-btn[aria-expanded="true"] {
    background: var(--hover);
  }
  .profile-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    flex: none;
    font-size: var(--fs-meta);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .pop {
    position: absolute;
    top: 62px;
    left: 10px;
    right: 10px;
    z-index: 40;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--menu-shadow);
    animation: pop 100ms var(--ease);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
  }
  .pop-empty {
    margin: 4px 6px 8px;
    font-size: var(--fs-small);
    line-height: 1.45;
    color: var(--muted);
  }
  .profiles {
    margin: 0 0 6px;
    padding: 0;
    list-style: none;
  }
  .profiles li {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 30px;
  }
  .apply {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    border-radius: 5px;
    text-align: left;
  }
  .apply:hover,
  .apply:focus-visible {
    background: var(--hover);
  }
  .current .pname {
    font-weight: 600;
  }
  .current .apply::before {
    content: "";
    width: 3px;
    height: 14px;
    margin-left: -5px;
    border-radius: 2px;
    background: var(--mark);
  }
  .pname {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pcount {
    flex: none;
    font-size: var(--fs-meta);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .save {
    padding: 8px 4px 2px;
    border-top: 1px solid var(--rule);
  }
  .save-label,
  .sub {
    display: block;
    margin-bottom: 5px;
    font-size: var(--fs-meta);
    color: var(--muted);
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }
  .ask span {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-small);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  input {
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font: inherit;
    font-size: var(--fs-small);
    color: var(--text);
    background: var(--surface);
  }
  input:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -1px;
  }
  .small {
    flex: none;
    height: 26px;
    min-width: 26px;
    padding: 0 9px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 600;
    white-space: nowrap;
  }
  .small:hover:not(:disabled) {
    background: var(--hover);
  }
  .small:disabled {
    opacity: 0.5;
  }
  .small.go {
    color: var(--on-primary);
    background: var(--primary);
    border-color: var(--primary);
  }
  .small.danger {
    color: var(--on-danger);
    background: var(--danger);
    border-color: var(--danger);
  }

  .search {
    position: relative;
    display: flex;
    align-items: center;
    margin: 6px 12px 6px;
    flex: none;
    color: var(--faint);
  }
  .search svg {
    position: absolute;
    left: 8px;
    pointer-events: none;
  }
  .search input {
    padding-left: 26px;
    background: color-mix(in srgb, var(--surface) 60%, transparent);
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px 8px;
    outline: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 7px;
    height: var(--row-h);
    padding: 0 6px;
    border-radius: var(--radius-control);
    cursor: default;
    user-select: none;
    outline: none;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.selected {
    background: var(--active);
  }
  .row:focus-visible {
    box-shadow: inset 0 0 0 1px var(--focus-ring);
  }
  .tick {
    display: grid;
    place-items: center;
    flex: none;
    width: 14px;
    height: 14px;
    border: 1.3px solid var(--control-border);
    border-radius: 3px;
    color: var(--on-primary);
  }
  .tick.on {
    background: var(--primary);
    border-color: var(--primary);
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--c);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row.off .name {
    color: var(--muted);
  }
  .row.off .dot {
    opacity: 0.45;
  }
  .wt {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    flex: none;
    font-size: var(--fs-meta);
    color: var(--muted);
  }
  .ab {
    flex: none;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--pending);
    font-variant-numeric: tabular-nums;
  }
  .state {
    margin: 8px 6px;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .state.error {
    color: var(--error-ink);
    user-select: text;
    overflow-wrap: anywhere;
  }
  .skeleton {
    height: 10px;
    margin: 10px 8px;
    border-radius: 4px;
    background: var(--hover);
    animation: pulse 1.4s ease-in-out infinite both;
  }
  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }

  footer {
    flex: none;
    padding: 4px 12px 12px;
    border-top: 1px solid var(--rule);
  }
  .disclose {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 4px;
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .disclose:hover {
    color: var(--text);
  }
  .disclose svg {
    transition: transform 120ms var(--ease);
  }
  .disclose svg.open {
    transform: rotate(90deg);
  }
  .discovery {
    padding: 6px 2px 0;
  }
  .depth {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--fs-small);
  }
  .stepper {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .depth-n {
    min-width: 14px;
    text-align: center;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .hint {
    margin: 4px 0 10px;
    font-size: var(--fs-meta);
    line-height: 1.45;
    color: var(--muted);
  }
  .patterns {
    margin: 0 0 6px;
    padding: 0;
    list-style: none;
  }
  .patterns li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }
  code {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
  }

  .rail-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 9px;
    margin: 0;
    padding: 8px 0 12px;
    list-style: none;
  }
  .rail-list .dot {
    width: 10px;
    height: 10px;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
