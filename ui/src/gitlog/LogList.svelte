<script lang="ts">
  import { onDestroy, tick, untrack } from "svelte";
  import ContextMenu from "../components/ContextMenu.svelte";
  import { gitlogMessage, gitlogPage, gitlogWebUrl, openUrl } from "../lib/api";
  import { writeClipboard } from "../lib/clipboard";
  import { isTypingTarget } from "../lib/keys";
  import { OVERLAY_SELECTOR, overlayShown } from "../lib/viewSwitch";
  import {
    EMPTY_QUERY,
    LogQuery,
    NAV_KEYS,
    ROW_H,
    badgeLabel,
    commitAge,
    commitDate,
    debounce,
    footerText,
    indexOfKey,
    keepSelection,
    navigate,
    nearEnd,
    rowKey,
    scrollToReveal,
    shortSha,
    toFilterView,
    visibleRange,
    type FilterState,
    type NavKey,
    type QueryState,
    type RowView,
  } from "./log";
  import { colorVar, type RepoView } from "./repos";

  // The merged commit list: one virtualised column of rows across the ticked repositories, its
  // keyboard, its context menu and Load more. The query it runs is a function of the repositories
  // and the filter; every change to either waits out the debounce and supersedes the one before.
  let {
    repos,
    queryRepos,
    filter,
    active,
    selectedKey = $bindable(null),
    onopen,
    onselect,
    onfocussearch,
    onnotice,
    onloading,
  }: {
    /** Every discovered repository: names and colours for the chips. */
    repos: RepoView[];
    /** The ids to log: the ticked ones. */
    queryRepos: string[];
    filter: FilterState;
    active: boolean;
    selectedKey?: string | null;
    /** Enter or a double-click: open the commit's first changed file (Task 7 wires this). */
    onopen: (row: RowView) => void;
    onselect: (row: RowView | null) => void;
    /** `/`: put the cursor in the search box. */
    onfocussearch: () => void;
    onnotice: (text: string) => void;
    onloading: (loading: boolean) => void;
  } = $props();

  let q = $state<QueryState>(EMPTY_QUERY);
  const query = new LogQuery(
    (id, req, more) => gitlogPage(id, req.repos, req.filter, more),
    (s) => (q = s),
  );

  const byId = $derived(new Map(repos.map((r) => [r.id, r])));
  const branchFilter = $derived(filter.branch);

  // ---- running the query ----
  const requestKey = $derived(JSON.stringify([queryRepos, toFilterView(filter)]));
  let ran: string | null = null;
  let wasActive = false;
  const later = debounce(() => run());

  function run() {
    later.cancel();
    ran = untrack(() => requestKey);
    dismissed = new Set();
    if (queryRepos.length === 0) {
      query.cancel();
      q = EMPTY_QUERY;
      return;
    }
    void query.start({ repos: queryRepos.slice(), filter: toFilterView(filter) });
  }

  $effect(() => {
    const key = requestKey;
    const on = active;
    untrack(() => {
      if (!on) {
        later.cancel();
        wasActive = false;
        return;
      }
      // First time on screen, or back from the review: now. A changed filter: after the debounce.
      if (!wasActive) {
        wasActive = true;
        run();
      } else if (key !== ran) later();
    });
  });
  onDestroy(() => {
    later.cancel();
    query.cancel();
  });
  $effect(() => onloading(q.loading || q.loadingMore));

  // ---- selection and scrolling ----
  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewH = $state(400);
  const rows = $derived(q.rows);
  const range = $derived(visibleRange(scrollTop, viewH, ROW_H, rows.length));
  const slice = $derived(rows.slice(range.start, range.end));
  const selIndex = $derived(indexOfKey(rows, selectedKey));
  const pageRows = $derived(Math.max(1, Math.floor(viewH / ROW_H) - 1));

  // A new list keeps the selection while the commit is still in it.
  $effect(() => {
    const kept = keepSelection(rows, untrack(() => selectedKey));
    if (kept !== untrack(() => selectedKey)) select(null, false);
  });

  function select(row: RowView | null, scroll = true) {
    selectedKey = row ? rowKey(row) : null;
    onselect(row);
    if (row && scroll) revealIndex(indexOfKey(rows, selectedKey));
  }
  function revealIndex(i: number) {
    if (!scroller || i < 0) return;
    const top = scrollToReveal(i, scroller.scrollTop, scroller.clientHeight, ROW_H);
    if (top !== scroller.scrollTop) scroller.scrollTop = top;
  }
  function onscroll() {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
  }
  // Fetch the next page before the end is reached; `more()` itself refuses a second in flight.
  $effect(() => {
    if (!q.done && !q.loading && !q.loadingMore && !q.error && nearEnd(range.end, rows.length)) void query.more();
  });

  /** Runs the query again now (the Refresh button). */
  export function refresh() {
    run();
  }

  /** Puts the keyboard on the list, selecting the first commit when none is. */
  export function focusList() {
    scroller?.focus();
    if (selIndex < 0 && rows.length) select(rows[0], false);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    if (NAV_KEYS.includes(e.key)) {
      e.preventDefault();
      const i = navigate(rows.length, selIndex, e.key as NavKey, pageRows);
      if (i >= 0) select(rows[i]);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const r = rows[selIndex];
      if (r) onopen(r);
    } else if (e.key === "Escape") {
      if (selIndex >= 0) {
        e.preventDefault();
        select(null);
      }
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const r = rows[selIndex];
      if (!r || !scroller) return;
      const b = scroller.getBoundingClientRect();
      menu = { row: r, x: b.left + 80, y: b.top + (selIndex + 1) * ROW_H - scroller.scrollTop };
    }
  }

  // `/` from anywhere in the Git log that is not a field or a menu.
  function onwindowkey(e: KeyboardEvent) {
    if (!active || e.defaultPrevented || e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
    if (isTypingTarget(e.target as HTMLElement | null)) return;
    if (overlayShown(document.querySelectorAll<HTMLElement>(OVERLAY_SELECTOR))) return;
    e.preventDefault();
    onfocussearch();
  }

  // ---- context menu ----
  let menu = $state<{ row: RowView; x: number; y: number } | null>(null);
  const MENU = [
    { id: "sha", label: "Copy Commit ID" },
    { id: "message", label: "Copy Message" },
    { id: "remote", label: "Open on Remote" },
  ];
  function oncontext(e: MouseEvent, row: RowView) {
    e.preventDefault();
    select(row, false);
    menu = { row, x: e.clientX, y: e.clientY };
  }
  async function pick(id: string) {
    const m = menu;
    menu = null;
    void tick().then(() => scroller?.focus());
    if (!m) return;
    const { repo, sha } = m.row;
    try {
      if (id === "sha") {
        await writeClipboard(sha);
        onnotice(`Copied ${shortSha(sha)}`);
      } else if (id === "message") {
        await writeClipboard(await gitlogMessage(repo, sha));
        onnotice("Copied the commit message");
      } else if (id === "remote") {
        const url = await gitlogWebUrl(repo, sha);
        if (!url) onnotice("This repository's remote is not one ClaudeGate can link to");
        else await openUrl(url);
      }
    } catch (e) {
      onnotice(`Couldn't do that: ${e}`);
    }
  }

  // ---- failures ----
  let dismissed = $state(new Set<string>());
  const failKey = (f: { repo: string; reason: string }) => `${f.repo}\u0000${f.reason}`;
  const failures = $derived(q.failures.filter((f) => !dismissed.has(failKey(f))));
  const repoName = (id: string) => byId.get(id)?.name ?? id.replace(/\/\.git\/?$/, "").split("/").pop() ?? id;
  function dismiss(f: { repo: string; reason: string }) {
    dismissed = new Set(dismissed).add(failKey(f));
  }

  const now = $derived(q.rows ? Date.now() : 0); // re-read per new page; ages are not live
  const status = $derived(footerText(rows.length, q.done, q.branchUse, q.loading));
</script>

<svelte:window onkeydown={onwindowkey} />

<section class="loglist" aria-label="Commits">
  {#if failures.length}
    <ul class="failures" aria-label="Repositories that failed">
      {#each failures as f (failKey(f))}
        <li role="alert">
          <span class="fname">{repoName(f.repo)}</span>
          <span class="reason" title={f.reason}>{f.reason}</span>
          <button type="button" aria-label="Dismiss the error for {repoName(f.repo)}" onclick={() => dismiss(f)}>×</button>
        </li>
      {/each}
    </ul>
  {/if}

  <div
    class="scroller"
    class:busy={q.loading}
    bind:this={scroller}
    bind:clientHeight={viewH}
    {onscroll}
    {onkeydown}
    tabindex="0"
    role="listbox"
    aria-label="Commits, newest first"
    aria-busy={q.loading}
    aria-activedescendant={selIndex >= 0 ? `c-${selIndex}` : undefined}
  >
    {#if q.error && rows.length === 0}
      <div class="state error">
        <p class="state-title">Couldn't read the log</p>
        <p class="state-body">{q.error}</p>
        <button type="button" class="again" onclick={run}>Try again</button>
      </div>
    {:else if queryRepos.length === 0}
      <div class="state"><p class="state-title">No repository is shown</p><p class="state-body">Tick one or more in Repositories.</p></div>
    {:else if q.loading && rows.length === 0}
      <div class="state" aria-hidden="true">
        {#each [82, 64, 74, 58, 70] as w, i (i)}<div class="skeleton" style:width="{w}%"></div>{/each}
      </div>
    {:else if rows.length === 0}
      <div class="state"><p class="state-title">No commits match</p><p class="state-body">Widen the date range or clear a filter.</p></div>
    {:else}
      <div class="spacer" style:height="{rows.length * ROW_H}px">
        <div class="window" style:transform="translateY({range.start * ROW_H}px)">
          {#each slice as r, k (rowKey(r))}
            {@const i = range.start + k}
            {@const repo = byId.get(r.repo)}
            {@const badge = badgeLabel(r.source, branchFilter)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              id="c-{i}"
              class="row"
              class:selected={i === selIndex}
              role="option"
              aria-selected={i === selIndex}
              tabindex="-1"
              style:--c={colorVar(repo?.color ?? 0)}
              title={`${r.subject}\n${r.author} · ${commitDate(r.time)}`}
              onclick={() => (select(r, false), scroller?.focus())}
              ondblclick={() => onopen(r)}
              oncontextmenu={(e) => oncontext(e, r)}
            >
              <span class="chip"><span class="dot"></span><span class="cname">{repo?.name ?? repoName(r.repo)}</span></span>
              <span class="sha">{shortSha(r.sha)}</span>
              <span class="subject">
                <span class="text">{r.subject}</span>
                {#if badge}<span class="badge" title="Only on {badge}">{badge}</span>{/if}
              </span>
              <span class="date">{commitAge(r.time, now)}</span>
              <span class="author">{r.author}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <footer class="foot">
    <span class="status" role="status">{status}</span>
    {#if q.error && rows.length}<span class="ferr" title={q.error}>Couldn't load more</span>{/if}
    {#if !q.done && rows.length}
      <button type="button" class="more" disabled={q.loadingMore || q.loading} onclick={() => void query.more()}>{q.loadingMore ? "Loading…" : "Load more"}</button>
    {/if}
  </footer>
</section>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={MENU} onpick={(id) => void pick(id)} onclose={() => ((menu = null), void tick().then(() => scroller?.focus()))} />
{/if}

<style>
  .loglist {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    outline: none;
  }
  .scroller:focus-visible {
    box-shadow: inset 0 0 0 2px var(--focus-ring);
  }
  .scroller.busy .window {
    opacity: 0.55;
  }
  .spacer {
    position: relative;
  }
  .window {
    position: absolute;
    inset: 0 0 auto 0;
  }
  .row {
    display: grid;
    grid-template-columns: 116px 58px minmax(0, 1fr) 70px 96px;
    align-items: center;
    gap: 10px;
    height: 30px; /* ROW_H */
    padding: 0 var(--pad);
    font-size: var(--fs-body);
    cursor: default;
    user-select: none;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.selected {
    background: var(--active);
  }
  .scroller:focus-visible .row.selected {
    box-shadow: inset 2px 0 0 var(--focus-ring);
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
  .cname,
  .text,
  .author {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sha {
    font-family: var(--font-mono);
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .subject {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .badge {
    flex: none;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    height: 18px;
    line-height: 18px;
    padding: 0 6px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--pending);
    border: 1px solid color-mix(in srgb, var(--pending) 45%, transparent);
  }
  .date,
  .author {
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .date {
    text-align: right;
    white-space: nowrap;
  }

  .failures {
    flex: none;
    margin: 0;
    padding: 0;
    list-style: none;
    border-bottom: 1px solid var(--rule);
  }
  .failures li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px var(--pad);
    font-size: var(--fs-small);
    color: var(--error-ink);
    background: var(--error-bg);
  }
  .fname {
    flex: none;
    font-weight: 600;
  }
  .reason {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .failures button {
    flex: none;
    width: 20px;
    height: 20px;
    border-radius: 4px;
    color: inherit;
  }
  .failures button:hover {
    background: color-mix(in srgb, var(--error-ink) 14%, transparent);
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 24px var(--pad);
  }
  .state-title {
    font-weight: 600;
  }
  .state-body {
    max-width: 420px;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .state.error .state-title {
    color: var(--error-ink);
  }
  .again,
  .more {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 600;
  }
  .again:hover,
  .more:hover:not(:disabled) {
    background: var(--hover);
  }
  .more:disabled {
    opacity: 0.6;
  }
  .skeleton {
    height: 12px;
    margin: 8px 0;
    border-radius: 4px;
    background: var(--hover);
  }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 30px;
    padding: 3px var(--pad);
    border-top: 1px solid var(--rule);
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .status {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ferr {
    color: var(--error-ink);
  }
  button:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -1px;
  }

  @media (max-width: 720px) {
    .row {
      grid-template-columns: 92px 54px minmax(0, 1fr) 64px;
      gap: 8px;
      padding: 0 12px;
    }
    .author {
      display: none;
    }
  }
</style>
