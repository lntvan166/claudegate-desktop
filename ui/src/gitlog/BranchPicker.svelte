<script module lang="ts">
  let instances = 0;
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { highlight, inReposText, noMatchText, pickerGroups, type BranchName, type Pair, type PickerItem } from "./compare";
  import type { BranchKey } from "./repos";

  // The Branch picker: a search box over Recent pairs, Favorites, Local and Remote. It filters the
  // names it was given as you type and reads nothing from git. A branch is its name and kind: a
  // local `origin/x` and the remote-tracking one are two rows, and the one clicked is the one picked.
  let {
    names,
    favorites,
    recent,
    anchor,
    onpick,
    onpair,
    onfavorite,
    onclose,
  }: {
    /** null while the names are being read. */
    names: BranchName[] | null;
    favorites: BranchKey[];
    recent: Pair[];
    anchor: HTMLElement;
    onpick: (b: BranchKey) => void;
    onpair: (p: Pair) => void;
    onfavorite: (b: BranchKey, on: boolean) => void;
    onclose: () => void;
  } = $props();

  const uid = `bp${++instances}`;
  let query = $state("");

  let inputEl: HTMLInputElement | undefined = $state();
  let rootEl: HTMLDivElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();
  let box = $state({ left: 0, top: 0, maxHeight: 320 });

  const groups = $derived(pickerGroups(names ?? [], favorites, recent, query));
  const items = $derived(groups.flatMap((g) => g.items));
  const emptyText = $derived(names === null ? "Reading branch names…" : names.length === 0 ? "No branches in the ticked repositories." : noMatchText(query.trim()));

  // The active row is a row, not a position: a favourite toggled with Ctrl+D adds or removes a
  // Favorites row above it, and typing reorders the list. It is kept by its key (the group and the
  // each-key), and only when that row is gone does it fall back to the nearest position.
  const itemKey = (it: PickerItem) => (it.kind === "pair" ? `p${it.pair.left}|${it.pair.leftRemote}|${it.pair.right}|${it.pair.rightRemote}` : `${it.remote}:${it.name}`);
  const keys = $derived(groups.flatMap((g) => g.items.map((it) => `${g.title}\u0000${itemKey(it)}`)));
  let activeKey = $state<string | null>(null);
  /** The position last chosen: where the active row falls back to when its branch is gone. */
  let lastIndex = 0;
  const active = $derived.by(() => {
    if (activeKey !== null) {
      const at = keys.indexOf(activeKey);
      if (at >= 0) return at;
      // Its Favorites copy went (un-favourited there): the same branch in its own group.
      const item = activeKey.slice(activeKey.indexOf("\u0000"));
      const same = keys.findIndex((k) => k.endsWith(item));
      if (same >= 0) return same;
    }
    return Math.max(0, Math.min(lastIndex, keys.length - 1));
  });
  // The pointer takes the active row only when it moves (mousemove, not mouseenter): rows that
  // shift under a resting pointer, as a Ctrl+D toggle makes them, must not steal it from the keys.
  function setActive(i: number) {
    lastIndex = i;
    activeKey = keys[i] ?? null;
  }

  function place() {
    const r = anchor.getBoundingClientRect();
    const width = rootEl?.offsetWidth ?? 320;
    const top = r.bottom + 2;
    box = {
      left: Math.max(4, Math.min(r.left, window.innerWidth - width - 4)),
      top,
      maxHeight: Math.max(120, window.innerHeight - top - 56),
    };
  }

  onMount(() => {
    place();
    inputEl?.focus();
    const down = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!rootEl?.contains(t) && !anchor.contains(t)) close();
    };
    window.addEventListener("mousedown", down, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("resize", place);
    };
  });

  function close() {
    onclose();
    anchor.focus();
  }

  function choose(it: PickerItem | undefined) {
    if (!it) return;
    if (it.kind === "pair") onpair(it.pair);
    else onpick({ name: it.name, remote: it.remote });
    close();
  }

  async function move(d: number) {
    setActive(Math.max(0, Math.min(items.length - 1, active + d)));
    await tick();
    listEl?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function key(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      void move(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(items[active]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "d") {
      // The keyboard way to ★: the star itself is not a tab stop.
      e.preventDefault();
      const it = items[active];
      if (it?.kind === "name") onfavorite({ name: it.name, remote: it.remote }, !it.favorite);
    } else if (e.key === "Tab") {
      // Leaving the search box leaves the picker: focus goes back to the anchor, and the browser moves on from there.
      close();
    }
  }

  const sideTag = (remote: boolean) => (remote ? "remote" : "local");
</script>

<div class="picker" bind:this={rootEl} style:left="{box.left}px" style:top="{box.top}px" role="presentation">
  <input
    bind:this={inputEl}
    bind:value={query}
    oninput={() => {
      activeKey = null;
      lastIndex = 0;
    }}
    onkeydown={key}
    class="search"
    type="text"
    placeholder="Search branches"
    spellcheck="false"
    autocomplete="off"
    role="combobox"
    aria-expanded="true"
    aria-controls="{uid}-list"
    aria-keyshortcuts="Control+D Meta+D"
    aria-autocomplete="list"
    aria-activedescendant={items.length ? `${uid}-pick-${active}` : undefined}
    aria-label="Search branches"
  />
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="list" id="{uid}-list" role="listbox" tabindex="-1" bind:this={listEl} style:max-height="{box.maxHeight}px" onmousedown={(e) => e.preventDefault()}>
    {#if items.length === 0}
      <div class="empty">{emptyText}</div>
    {:else}
      {#each groups as g, gi (g.title)}
        <div role="group" aria-labelledby="{uid}-g{gi}">
        <div class="group" id="{uid}-g{gi}" role="presentation">{g.title}</div>
        {#each g.items as it, k (it.kind === "pair" ? `p${it.pair.left}|${it.pair.leftRemote}|${it.pair.right}|${it.pair.rightRemote}` : `${it.remote}:${it.name}`)}
          {@const i = groups.slice(0, gi).reduce((n, x) => n + x.items.length, 0) + k}
          {#if it.kind === "pair"}
            <div class="row" class:active={i === active} role="option" id="{uid}-pick-{i}" data-i={i} aria-selected={i === active} tabindex="-1" onclick={() => choose(it)} onmousemove={() => i !== active && setActive(i)}>
              <span class="side left" aria-hidden="true">◀</span>
              <span class="name">{it.pair.left}</span>
              {#if it.pair.leftRemote}<span class="tag">remote</span>{/if}
              <span class="side right" aria-hidden="true">▶</span>
              <span class="name">{it.pair.right}</span>
              {#if it.pair.rightRemote}<span class="tag">remote</span>{/if}
            </div>
          {:else}
            {@const h = highlight(it.name, query)}
            <div class="row" class:active={i === active} role="option" id="{uid}-pick-{i}" data-i={i} aria-selected={i === active} tabindex="-1" onclick={() => choose(it)} onmousemove={() => i !== active && setActive(i)}>
              <button
                class="star"
                class:on={it.favorite}
                tabindex="-1"
                title="{it.favorite ? 'Remove from Favorites' : 'Add to Favorites'} (Ctrl+D)"
                aria-label="Favorite {it.name} ({sideTag(it.remote)})"
                aria-pressed={it.favorite}
                onclick={(e) => {
                  e.stopPropagation();
                  onfavorite({ name: it.name, remote: it.remote }, !it.favorite);
                }}>★</button
              >
              <span class="name">{h.pre}{#if h.hit}<mark>{h.hit}</mark>{/if}{h.post}</span>{#if it.favorite}<span class="sr">, favourite</span>{/if}
              {#if g.title === "Favorites" && it.remote}<span class="tag">remote</span>{/if}
              <span class="count">{inReposText(it.count)}</span>
            </div>
          {/if}
        {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .picker {
    position: fixed;
    z-index: 50;
    width: 340px;
    max-width: calc(100vw - 8px);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    box-shadow: var(--menu-shadow);
    overflow: hidden;
  }
  .search {
    box-sizing: border-box;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-bottom: 1px solid var(--rule);
    outline: none;
    background: transparent;
    font: inherit;
    font-size: var(--fs-small);
    color: var(--text);
  }
  .search:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .list {
    overflow-y: auto;
    padding: 4px 0;
  }
  .group {
    padding: 6px 10px 2px;
    font-size: var(--fs-small);
    color: var(--faint);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: var(--row-h);
    padding: 0 10px 0 4px;
    font-size: var(--fs-small);
    color: var(--text);
    cursor: pointer;
  }
  .row.active {
    background: var(--hover);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row > .side + .name {
    flex: 0 1 auto;
  }
  .side {
    color: var(--muted);
    padding-left: 6px;
  }
  mark {
    background: transparent;
    color: var(--mark);
    font-weight: 600;
  }
  .count,
  .tag {
    flex: none;
    color: var(--faint);
  }
  .star {
    flex: none;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--faint);
    font-size: 13px;
    cursor: pointer;
    border-radius: var(--radius-chip);
  }
  .star:hover {
    color: var(--text);
  }
  .star.on {
    color: var(--pending);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .empty {
    padding: 10px;
    font-size: var(--fs-small);
    color: var(--muted);
  }
</style>
