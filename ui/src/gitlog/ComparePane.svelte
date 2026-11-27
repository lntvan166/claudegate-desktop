<script lang="ts">
  import { tick, untrack } from "svelte";
  import {
    gitlogBranches,
    gitlogCompare,
    gitlogCompareMode,
    gitlogComparePair,
    gitlogCompareRecent,
    gitlogFavorite,
    gitlogUnfavorite,
  } from "../lib/api";
  import type { IconTheme } from "../lib/icons";
  import BranchPicker from "./BranchPicker.svelte";
  import CompareColumns from "./CompareColumns.svelte";
  import {
    CompareLoad,
    EMPTY_COMPARE,
    comparable,
    compareKey,
    compareMessage,
    listed,
    missingText,
    repoCounts,
    summaryLabel,
    swapPair,
    type BranchName,
    type CompareMode,
    type CompareOpen,
    type CompareSide,
    type CompareState,
    type Pair,
  } from "./compare";
  import { colorVar, type BranchKey, type GitLogConfig, type RepoView } from "./repos";

  // Compare mode of the Git log: two branches across the ticked repositories. The toolbar picks the
  // pair and how to show it; the list holds only the repositories that differ (and those that can't
  // be compared, saying why); the columns show the selected one. One compare answers every
  // repository at once; nothing is watched, ↻ reads again.
  let {
    shown,
    configReady,
    repos,
    queryRepos,
    cfg,
    theme,
    open,
    onconfig,
    onopen,
    onclear,
    onnotice,
  }: {
    /** Compare is on screen: nothing is compared before it first is. */
    shown: boolean;
    configReady: boolean;
    repos: RepoView[];
    /** The ticked repositories, in the Repo List's order: the scope. */
    queryRepos: string[];
    cfg: GitLogConfig;
    theme: IconTheme;
    open: CompareOpen | null;
    /** The compare keys of the config changed (pair, recent, favourites, mode). */
    onconfig: (c: GitLogConfig) => void;
    onopen: (o: CompareOpen) => void;
    /** The open diff no longer belongs to what is shown (a new pair, a swap, another repository). */
    onclear: () => void;
    onnotice: (text: string) => void;
  } = $props();

  let pair = $state<Pair | null>(null);
  let started = false;
  let cs = $state<CompareState>(EMPTY_COMPARE);
  const load = new CompareLoad((id, rs, p) => gitlogCompare(id, rs, p), (s) => (cs = s));
  /** The pair and scope the shown rows (or the read in flight) answer. */
  let loadedKey = "";

  const mode = $derived<CompareMode>(cfg.compareMode === "commits" ? "commits" : "files");
  const rows = $derived(cs.view?.rows ?? []);
  const listedRows = $derived(rows.filter(listed));
  const missingRows = $derived(rows.filter((r) => r.kind === "missing"));
  const ticked = $derived(queryRepos.length);
  // While a read runs, the old rows stay (dimmed) only when they have something to show.
  const message = $derived(cs.error ? null : compareMessage(pair, ticked, cs.view && (!cs.loading || listedRows.length > 0) ? cs.view.rows : null));
  const summary = $derived(
    !comparable(pair) || ticked === 0 ? "" : cs.loading ? `Reading ${ticked === 1 ? "1 repository" : `${ticked} repositories`}…` : cs.view ? summaryLabel(rows, ticked) : "",
  );

  const repoById = $derived(new Map(repos.map((r) => [r.id, r])));
  const nameOf = (id: string) => repoById.get(id)?.name ?? id.replace(/\/\.git\/?$/, "").split("/").pop() ?? id;

  // ---- the read ----
  $effect(() => {
    if (!shown || !configReady) return;
    const repoIds = queryRepos;
    untrack(() => {
      if (!started) {
        started = true;
        pair = cfg.comparePair ? { ...cfg.comparePair } : null;
        // First use: the picker opens on the left box.
        if (!pair) void tick().then(() => openPicker("left"));
      }
    });
    const p = pair;
    const key = compareKey(p, repoIds);
    untrack(() => {
      if (key === loadedKey) return;
      loadedKey = key;
      if (comparable(p) && repoIds.length > 0) void load.start(repoIds.slice(), { ...p });
      else load.clear();
    });
  });

  /** ↻: the same pair and scope, read again; branch names too, the next time the picker opens. */
  function refresh() {
    names = null;
    namesFor = "\u0000stale";
    if (!comparable(pair) || queryRepos.length === 0) return;
    loadedKey = compareKey(pair, queryRepos);
    void load.start(queryRepos.slice(), { ...pair });
  }

  async function store(p: Pair) {
    try {
      await gitlogComparePair(p.left, p.leftRemote, p.right, p.rightRemote);
      onconfig(await gitlogCompareRecent(p.left, p.leftRemote, p.right, p.rightRemote));
    } catch (e) {
      onnotice(`Couldn't save the branch pair: ${e}`);
    }
  }

  function setPair(p: Pair) {
    pair = p;
    onclear();
    if (comparable(p)) void store(p);
  }

  function pickSide(side: CompareSide, b: BranchKey) {
    const cur = pair ?? { left: "", leftRemote: false, right: "", rightRemote: false };
    setPair(side === "left" ? { ...cur, left: b.name, leftRemote: b.remote } : { ...cur, right: b.name, rightRemote: b.remote });
  }

  /** ⇄: the result shown is mirrored, not read again (a read still in flight is restarted instead). */
  function swap() {
    if (!pair) return;
    const next = swapPair(pair);
    const settled = !cs.loading && !!cs.view && loadedKey === compareKey(pair, queryRepos);
    if (settled) {
      load.mirror();
      loadedKey = compareKey(next, queryRepos);
    }
    setPair(next);
  }

  async function setMode(m: CompareMode) {
    if (m === mode) return;
    onconfig({ ...cfg, compareMode: m });
    try {
      onconfig(await gitlogCompareMode(m));
    } catch (e) {
      onnotice(`Couldn't save the compare view: ${e}`);
    }
  }

  // ---- the picker ----
  let pickerSide = $state<CompareSide | null>(null);
  /**
   * The box the picker hangs from. Kept apart from `pickerSide`: closing clears the side first and
   * then hands focus back to the anchor, which must still be the box it opened from.
   */
  let pickerAnchor = $state<HTMLElement | null>(null);
  let leftBox: HTMLButtonElement | undefined = $state();
  let rightBox: HTMLButtonElement | undefined = $state();
  let names = $state<BranchName[] | null>(null);
  let namesFor = "\u0000none";
  let namesSeq = 0;

  function openPicker(side: CompareSide) {
    pickerAnchor = (side === "left" ? leftBox : rightBox) ?? null;
    pickerSide = side;
    readNames();
  }

  /** Branch names for the ticked repositories, unless they are already here for exactly those. */
  function readNames() {
    const k = queryRepos.join("\n");
    if (namesFor === k) return;
    namesFor = k;
    names = null;
    const n = ++namesSeq;
    gitlogBranches(queryRepos.slice()).then(
      (list) => n === namesSeq && (names = list),
      (e) => {
        if (n !== namesSeq) return;
        names = [];
        onnotice(`Couldn't read the branch names: ${e}`);
      },
    );
  }
  // The scope changed while the picker is open (the first use can open it before the repositories
  // are listed): its names follow.
  $effect(() => {
    void queryRepos;
    if (pickerSide) untrack(readNames);
  });

  async function favorite(b: BranchKey, on: boolean) {
    try {
      onconfig(await (on ? gitlogFavorite(b.name, b.remote) : gitlogUnfavorite(b.name, b.remote)));
    } catch (e) {
      onnotice(`Couldn't save the favourite: ${e}`);
    }
  }

  // ---- the repo list ----
  let selected = $state<string | null>(null);
  let listEl: HTMLDivElement | undefined = $state();
  let columns: CompareColumns | undefined = $state();
  // The first repository when the list fills; the selection stays on its repository while it is listed.
  $effect(() => {
    const ids = listedRows.map((r) => r.repo);
    untrack(() => {
      if (selected && ids.includes(selected)) return;
      selected = ids[0] ?? null;
    });
  });
  const selectedRow = $derived(listedRows.find((r) => r.repo === selected) ?? null);
  // Another repository (or the same one under a new pair): the open diff goes.
  let lastSel = "";
  $effect(() => {
    const k = selectedRow ? `${selectedRow.repo}\u0000${selectedRow.kind === "differs" ? `${selectedRow.leftSha}\u0000${selectedRow.rightSha}` : selectedRow.kind}` : "";
    untrack(() => {
      if (lastSel && k !== lastSel) onclear();
      lastSel = k;
    });
  });

  function select(id: string, scroll = true) {
    selected = id;
    if (scroll) void tick().then(() => listEl?.querySelector(`[data-repo="${CSS.escape(id)}"]`)?.scrollIntoView({ block: "nearest" }));
  }

  function listKey(e: KeyboardEvent) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    const ids = listedRows.map((r) => r.repo);
    const i = selected ? ids.indexOf(selected) : -1;
    let to = -1;
    if (e.key === "ArrowDown") to = Math.min(ids.length - 1, i + 1);
    else if (e.key === "ArrowUp") to = Math.max(0, i - 1);
    else if (e.key === "Home") to = 0;
    else if (e.key === "End") to = ids.length - 1;
    else if (e.key === "Enter" || e.key === "ArrowRight") {
      e.preventDefault();
      void columns?.focus();
      return;
    }
    if (to < 0) return;
    e.preventDefault();
    select(ids[to]);
  }

  /** The keyboard into Compare: the repo list when it has rows, else the left branch box. */
  export function focus() {
    if (listEl && listedRows.length > 0) listEl.focus();
    else leftBox?.focus();
  }

  const sideLabel = (side: CompareSide) => {
    const name = side === "left" ? pair?.left : pair?.right;
    const remote = side === "left" ? pair?.leftRemote : pair?.rightRemote;
    return { name: name || "Pick a branch", empty: !name, remote: !!name && !!remote };
  };
  const lb = $derived(sideLabel("left"));
  const rb = $derived(sideLabel("right"));
</script>

<section class="compare" aria-label="Compare branches">
  <div class="ctool">
    <div class="what" role="group" aria-label="Branches to compare">
      <button
        type="button"
        class="bbox left"
        class:empty={lb.empty}
        bind:this={leftBox}
        aria-haspopup="listbox"
        aria-expanded={pickerSide === "left"}
        title="The branch to merge from"
        aria-label="Merge from: {lb.name}{lb.remote ? ' (remote)' : ''}"
        onclick={() => (pickerSide === "left" ? (pickerSide = null) : openPicker("left"))}
      ><span class="arrow" aria-hidden="true">◀</span><span class="bn">{lb.name}</span><span class="caret" aria-hidden="true">▾</span></button>
      <button type="button" class="icon-btn" title="Swap sides" aria-label="Swap sides" disabled={!pair || !pair.left || !pair.right} onclick={swap}>⇄</button>
      <button
        type="button"
        class="bbox right"
        class:empty={rb.empty}
        bind:this={rightBox}
        aria-haspopup="listbox"
        aria-expanded={pickerSide === "right"}
        title="The branch it goes into"
        aria-label="Merge into: {rb.name}{rb.remote ? ' (remote)' : ''}"
        onclick={() => (pickerSide === "right" ? (pickerSide = null) : openPicker("right"))}
      ><span class="arrow" aria-hidden="true">▶</span><span class="bn">{rb.name}</span><span class="caret" aria-hidden="true">▾</span></button>
    </div>
    <div class="how">
      <div class="seg" role="group" aria-label="Show">
        <button type="button" aria-pressed={mode === "files"} onclick={() => void setMode("files")}>Files</button>
        <button type="button" aria-pressed={mode === "commits"} onclick={() => void setMode("commits")}>Commits</button>
      </div>
      <button type="button" class="icon-btn" title="Read the branches again" aria-label="Refresh" disabled={!comparable(pair) || ticked === 0} onclick={refresh}>
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" class:spin={cs.loading}><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2.5v2.5h-2.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
      </button>
    </div>
    <span class="summary" role="status">{summary}</span>
  </div>

  {#if cs.error}
    <p class="message" role="alert">Couldn't compare the branches: {cs.error}</p>
  {:else if message}
    <p class="message">{message}</p>
    {#if cs.view && missingRows.length > 0 && pair && listedRows.length === 0}
      <details class="missing solo">
        <summary>{missingText(pair, missingRows.length)}</summary>
        <ul>{#each missingRows as r (r.repo)}<li>{nameOf(r.repo)}</li>{/each}</ul>
      </details>
    {/if}
  {:else if pair}
    <div class="cbody" class:stale={cs.loading}>
      <div class="side">
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="rlist"
          role="listbox"
          tabindex="0"
          aria-label="Repositories that differ"
          aria-activedescendant={selected ? `cmp-r-${listedRows.findIndex((r) => r.repo === selected)}` : undefined}
          bind:this={listEl}
          onkeydown={listKey}
        >
          {#each listedRows as r, i (r.repo)}
            {@const repo = repoById.get(r.repo)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="rrow"
              class:selected={r.repo === selected}
              role="option"
              id="cmp-r-{i}"
              data-repo={r.repo}
              aria-selected={r.repo === selected}
              tabindex="-1"
              title={r.kind === "error" ? r.reason : r.kind === "nobase" ? "The two branches share no ancestor here" : undefined}
              style:--c={colorVar(repo?.color ?? 0)}
              onclick={() => {
                select(r.repo, false);
                listEl?.focus();
              }}
            >
              <span class="chip"><span class="dot"></span><span class="cname">{nameOf(r.repo)}</span></span>
              <span class="counts" class:why={r.kind !== "differs"}>{repoCounts(r)}</span>
            </div>
          {/each}
        </div>
        {#if missingRows.length > 0}
          <details class="missing">
            <summary>{missingText(pair, missingRows.length)}</summary>
            <ul>{#each missingRows as r (r.repo)}<li>{nameOf(r.repo)}</li>{/each}</ul>
          </details>
        {/if}
      </div>
      {#if selectedRow && cs.view}
        <CompareColumns bind:this={columns} row={selectedRow} name={nameOf(selectedRow.repo)} {pair} {mode} queryId={cs.view.queryId} {theme} {open} {onopen} onescape={() => listEl?.focus()} />
      {/if}
    </div>
  {/if}
</section>

{#if pickerSide && pickerAnchor}
  {@const side = pickerSide}
  <BranchPicker
    names={names}
    favorites={cfg.compareFavorites}
    recent={cfg.compareRecent}
    anchor={pickerAnchor}
    onpick={(b) => pickSide(side, b)}
    onpair={(p) => setPair({ ...p })}
    onfavorite={(b, on) => void favorite(b, on)}
    onclose={() => (pickerSide = null)}
  />
{/if}

<style>
  .compare {
    --cmp-left: var(--repo-9);
    --cmp-right: var(--repo-4);
    flex: 1 1 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }
  .ctool {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 16px;
    padding: 6px var(--pad);
    border-bottom: 1px solid var(--rule);
  }
  .what,
  .how {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }
  .what {
    flex: 0 1 auto;
  }
  .bbox {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 260px;
    min-width: 0;
    height: 26px;
    padding: 0 8px 0 7px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    color: var(--text);
  }
  .bbox.left {
    box-shadow: inset 3px 0 0 var(--cmp-left);
  }
  .bbox.right {
    box-shadow: inset 3px 0 0 var(--cmp-right);
  }
  .bbox.empty .bn {
    color: var(--muted);
  }
  .bbox:hover,
  .icon-btn:hover:not(:disabled) {
    background: var(--hover);
  }
  .bbox .arrow {
    flex: none;
    font-size: 10px;
  }
  .bbox.left .arrow {
    color: var(--cmp-left);
  }
  .bbox.right .arrow {
    color: var(--cmp-right);
  }
  .bn {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
  }
  .caret {
    flex: none;
    color: var(--faint);
    font-size: 10px;
  }
  .icon-btn {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-control);
    color: var(--muted);
  }
  .icon-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .seg button {
    height: 24px;
    padding: 0 10px;
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .seg button + button {
    border-left: 1px solid var(--control-border);
  }
  .seg button[aria-pressed="true"] {
    background: var(--active);
    color: var(--text);
    font-weight: 600;
  }
  .seg button:hover:not([aria-pressed="true"]) {
    background: var(--hover);
  }
  .bbox:focus-visible,
  .icon-btn:focus-visible,
  .seg button:focus-visible,
  .rlist:focus-visible,
  .missing summary:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .spin {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .summary {
    flex: 1 1 auto;
    min-width: 0;
    text-align: right;
    font-size: var(--fs-small);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .message {
    padding: 24px var(--pad);
    max-width: 560px;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .cbody {
    flex: 1;
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    min-height: 0;
  }
  .cbody.stale {
    opacity: 0.6;
  }
  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--rule);
  }
  .rlist {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 4px 0;
  }
  .rlist:focus {
    outline: none;
  }
  .rrow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: var(--row-h);
    padding: 0 10px;
    cursor: pointer;
  }
  .rrow:hover {
    background: var(--hover);
  }
  .rrow.selected {
    background: var(--active);
  }
  .rlist:focus-visible .rrow.selected {
    box-shadow: inset 3px 0 0 var(--focus-ring);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 20px;
    padding: 0 7px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--c);
    background: color-mix(in srgb, var(--c) 12%, transparent);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--c);
  }
  .cname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .counts {
    margin-left: auto;
    flex: none;
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--muted);
    white-space: nowrap;
  }
  .counts.why {
    font-family: var(--font);
    font-style: italic;
  }
  .missing {
    flex: none;
    max-height: 40%;
    overflow: auto;
    border-top: 1px solid var(--rule);
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .missing.solo {
    margin: 0 var(--pad);
    border: 0;
  }
  .missing summary {
    padding: 6px 10px;
    cursor: pointer;
  }
  .missing ul {
    list-style: none;
    margin: 0;
    padding: 0 10px 8px 24px;
  }
  .missing li {
    padding: 1px 0;
    color: var(--text);
  }

  /* A narrow window: the repo list over the columns. */
  @media (max-width: 720px) {
    .cbody {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
    }
    .side {
      max-height: 104px;
      border-right: 0;
      border-bottom: 1px solid var(--rule);
    }
    .summary {
      flex-basis: 100%;
      text-align: left;
    }
  }
</style>
