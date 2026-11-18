<script lang="ts">
  import { onMount, tick } from "svelte";
  import type { EditorConfig } from "../lib/editors";
  import { menuOptions } from "../lib/editors";
  import { appProtectedList, protectedChips, sourceLabel, type Resolved, type SettingKey, type SettingsView } from "../lib/settings";

  let {
    settings,
    config,
    project,
    hidden,
    onset,
    onhooklog,
    onextrahide,
    onexclude,
    ontakeback,
    oneditor,
    onreadonly,
    onshow,
    onclose,
    full = false,
  }: {
    settings: SettingsView;
    config: EditorConfig;
    /** The selected project, whose exclude list and hide patterns are shown; null when none. */
    project: { root: string; name: string } | null;
    /** Hidden projects: they can be shown again from here. */
    hidden: { root: string; name: string }[];
    /** Saves one app override; `null` drops it. Resolves false when it failed (the app says why). */
    onset: (key: SettingKey, value: unknown) => Promise<boolean>;
    onhooklog: (on: boolean) => Promise<boolean>;
    onextrahide: (patterns: string[]) => Promise<boolean>;
    /** The owned project's exclude list; `null` resets it to the defaults. */
    onexclude: (globs: string[] | null) => Promise<boolean>;
    /** For an app project VS Code took over: the app manages its exclude again. */
    ontakeback: () => Promise<boolean>;
    /** Resolves false when it failed (the app says why). */
    oneditor: (template: string) => Promise<boolean>;
    onreadonly: (on: boolean) => void;
    onshow: (root: string) => void;
    onclose: () => void;
    /** Narrow window: the screen is the whole sheet, so its title is the page's h1. */
    full?: boolean;
  } = $props();

  let closeBtn: HTMLButtonElement | undefined = $state();
  onMount(() => {
    void tick().then(() => closeBtn?.focus());
  });

  /** A quiet "Saved" after each change; every control saves itself. */
  let saved = $state(0);
  let savedTimer: ReturnType<typeof setTimeout> | undefined;
  function stamp(ok: boolean) {
    if (!ok) return;
    saved++;
    clearTimeout(savedTimer);
    savedTimer = setTimeout(() => (saved = 0), 1600);
  }
  const set = async (key: SettingKey, value: unknown) => stamp(await onset(key, value));

  /** Number fields: what is typed, until it is committed or found wrong. */
  let draft = $state<Record<string, string>>({});
  let fieldError = $state<Record<string, string>>({});
  function commitNumber(key: "pendingAgeMinDays" | "maxAttached", min: number) {
    const raw = draft[key];
    if (raw === undefined) return;
    const n = Number(raw.trim());
    if (raw.trim() === "" || !Number.isInteger(n) || n < min) {
      fieldError = { ...fieldError, [key]: min === 0 ? "A whole number of days, 0 or more." : "A whole number, 1 or more." };
      return;
    }
    fieldError = { ...fieldError, [key]: "" };
    const { [key]: _, ...rest } = draft;
    draft = rest;
    if (n !== settings[key].value) void set(key, n);
  }

  /** The "add a pattern" boxes, keyed by list. */
  let adding = $state<Record<string, string>>({});
  type PatternList = "protected" | "extraHide" | "exclude";
  const saveList = (list: PatternList, next: string[]) =>
    list === "protected" ? onset("protected", next) : list === "exclude" ? onexclude(next) : onextrahide(next);
  async function addPattern(list: PatternList, current: string[]) {
    const p = (adding[list] ?? "").trim();
    if (!p || current.includes(p)) {
      adding = { ...adding, [list]: "" };
      return;
    }
    const ok = await saveList(list, [...current, p]);
    stamp(ok);
    if (ok) adding = { ...adding, [list]: "" };
  }
  async function removePattern(list: PatternList, current: string[], p: string) {
    stamp(await saveList(list, current.filter((x) => x !== p)));
  }

  const editors = $derived(menuOptions(config));
  /** Secret files: which chips can be removed here, and which come from this project's .vscode. */
  const pChips = $derived(new Map(protectedChips(settings).map((c) => [c.glob, c])));
  /** The app's own protected list: what an edit saves (never the project's added-back globs). */
  const appProtected = $derived(appProtectedList(settings));
  const owner = $derived(settings.excludeOwner);
  const readOnly = $derived(config.readOnly === true);
  /** The Take back confirmation is open. */
  let confirmTakeBack = $state(false);
  async function takeBack() {
    const ok = await ontakeback();
    stamp(ok);
    if (ok) confirmTakeBack = false;
  }
  const excludeOwned = $derived(owner === "owned" || settings.exclude.source === "app");
  const managedBy = $derived(
    settings.exclude.source === "cursor" ? "Cursor" : settings.exclude.source === "default" ? null : "VS Code",
  );

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

{#snippet source<T>(key: SettingKey, r: Resolved<T>)}
  <div class="source">
    <span class="badge" class:app={r.source === "app"}>{sourceLabel(r.source)}</span>
    {#if r.source === "app"}
      <button class="link" onclick={() => void set(key, null)} title="Drop this app's value and use the inherited one">Reset</button>
    {:else if r.source === "workspace"}
      <!-- An app value applies everywhere: say so before a project's own value is copied up. -->
      <button
        class="link"
        onclick={() => void set(key, r.value)}
        title="Copies this project's .vscode value into the app, where it then applies to every project"
      >Override for all projects</button>
    {:else}
      <button class="link" onclick={() => void set(key, r.value)} title="Keep this value in the app, whatever VS Code says later">Override</button>
    {/if}
  </div>
{/snippet}

{#snippet toggle(on: boolean, label: string, change: (on: boolean) => void, disabled = false)}
  <button class="switch" role="switch" aria-checked={on} aria-label={label} {disabled} onclick={() => change(!on)}>
    <span class="knob"></span>
  </button>
{/snippet}

{#snippet chips(
  list: string[],
  removable: boolean | ((p: string) => boolean),
  onremove: (p: string) => void,
  label: string,
  noteFor: (p: string) => string | null = () => null,
)}
  {#if list.length === 0}
    <p class="none">None</p>
  {:else}
    <ul class="chips" aria-label={label}>
      {#each list as p (p)}
        {@const note = noteFor(p)}
        <li class="chip" title={note ? `${p} (${note}; change it there)` : undefined}>
          <span class="glob">{p}</span>
          {#if note}<span class="chip-note">{note}</span>{/if}
          {#if typeof removable === "function" ? removable(p) : removable}
            <button class="x" aria-label="Remove {p}" title="Remove" onclick={() => onremove(p)}>
              <svg viewBox="0 0 16 16" width="10" height="10" aria-hidden="true">
                <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
              </svg>
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

{#snippet adder(list: PatternList, current: string[], label: string)}
  <form class="adder" onsubmit={(e) => { e.preventDefault(); void addPattern(list, current); }}>
    <input
      type="text"
      aria-label={label}
      placeholder="**/secrets/**"
      spellcheck="false"
      autocomplete="off"
      value={adding[list] ?? ""}
      oninput={(e) => (adding = { ...adding, [list]: e.currentTarget.value })}
    />
    <button class="btn" type="submit" disabled={!(adding[list] ?? "").trim()}>Add</button>
  </form>
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="settings" aria-labelledby="settings-title" {onkeydown}>
  <header class="head">
    <svelte:element this={full ? "h1" : "h2"} id="settings-title" class="title-h">Settings</svelte:element>
    <span class="saved" role="status" aria-live="polite">{saved ? "Saved" : ""}</span>
    <button class="close" bind:this={closeBtn} aria-label="Close settings" title="Close (Esc)" onclick={onclose}>
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
      </svg>
    </button>
  </header>

  <div class="body">
    <p class="intro">
      Each value comes from this app, else from the ClaudeGate extension in VS Code or Cursor, else its default.
      Override keeps it here; Reset hands it back.
    </p>

    <h3 class="group">Review</h3>

    <div class="row">
      <div class="label">
        <p class="title">Move to the next file after a decision</p>
        <p class="desc">Accepting or rejecting the selected file selects the next pending one.</p>
        {@render source("autoAdvance", settings.autoAdvance)}
      </div>
      <div class="control">
        {@render toggle(settings.autoAdvance.value, "Move to the next file after a decision", (on) => void set("autoAdvance", on))}
      </div>
    </div>

    <div class="row">
      <div class="label">
        <p class="title">Group by Claude session</p>
        <p class="desc">List files under the session that changed them, newest session first.</p>
        {@render source("groupBySession", settings.groupBySession)}
      </div>
      <div class="control">
        {@render toggle(settings.groupBySession.value, "Group by Claude session", (on) => void set("groupBySession", on))}
      </div>
    </div>

    <div class="row">
      <div class="label">
        <label class="title" for="set-age">Show age after</label>
        <p class="desc" id="set-age-desc">Pending files at least this many days old show their age, like “3d”. 0 turns it off.</p>
        {@render source("pendingAgeMinDays", settings.pendingAgeMinDays)}
      </div>
      <div class="control">
        <div class="number">
          <input
            id="set-age"
            type="number"
            min="0"
            step="1"
            inputmode="numeric"
            aria-describedby="set-age-desc{fieldError.pendingAgeMinDays ? ' set-age-err' : ''}"
            aria-invalid={!!fieldError.pendingAgeMinDays}
            value={draft.pendingAgeMinDays ?? String(settings.pendingAgeMinDays.value)}
            oninput={(e) => (draft = { ...draft, pendingAgeMinDays: e.currentTarget.value })}
            onchange={() => commitNumber("pendingAgeMinDays", 0)}
            onkeydown={(e) => e.key === "Enter" && commitNumber("pendingAgeMinDays", 0)}
          />
          <span class="unit">days</span>
        </div>
        {#if fieldError.pendingAgeMinDays}<p class="err" id="set-age-err">{fieldError.pendingAgeMinDays}</p>{/if}
      </div>
    </div>

    <div class="row">
      <div class="label">
        <label class="title" for="set-max">Worktrees to include</label>
        <p class="desc" id="set-max-desc">The most git worktrees inside a project whose changes are listed with it.</p>
        {@render source("maxAttached", settings.maxAttached)}
      </div>
      <div class="control">
        <div class="number">
          <input
            id="set-max"
            type="number"
            min="1"
            step="1"
            inputmode="numeric"
            aria-describedby="set-max-desc{fieldError.maxAttached ? ' set-max-err' : ''}"
            aria-invalid={!!fieldError.maxAttached}
            value={draft.maxAttached ?? String(settings.maxAttached.value)}
            oninput={(e) => (draft = { ...draft, maxAttached: e.currentTarget.value })}
            onchange={() => commitNumber("maxAttached", 1)}
            onkeydown={(e) => e.key === "Enter" && commitNumber("maxAttached", 1)}
          />
        </div>
        {#if fieldError.maxAttached}<p class="err" id="set-max-err">{fieldError.maxAttached}</p>{/if}
      </div>
    </div>

    <h3 class="group">Files</h3>

    <div class="row wide">
      <div class="label">
        <p class="title">Secret files</p>
        <p class="desc">Files matching these show a lock, and accepting one asks first.</p>
        {@render source("protected", settings.protected)}
      </div>
      <div class="control">
        {@render chips(
          settings.protected.value,
          (p) => pChips.get(p)?.removable === true,
          (p) => void removePattern("protected", appProtected, p),
          "Secret file patterns",
          (p) => pChips.get(p)?.note ?? null,
        )}
        {#if settings.protected.source === "app"}
          {@render adder("protected", appProtected, "Add a secret file pattern")}
        {:else}
          <p class="note">To edit this list, Override it first.</p>
        {/if}
      </div>
    </div>

    {#if project}
      <div class="row wide">
        <div class="label">
          <p class="title">Excluded from {project.name}</p>
          <p class="desc">Changes to these are not captured or listed.</p>
          <div class="source">
            {#if owner === "managedByVsCode"}
              <span class="badge">Managed by VS Code</span>
            {:else}
              <span class="badge" class:app={excludeOwned}>{sourceLabel(settings.exclude.source)}</span>
              {#if owner === "owned" && settings.exclude.source === "app" && !readOnly}
                <button class="link" onclick={() => void onexclude(null).then(stamp)} title="Use the extension's default list again">Reset to defaults</button>
              {/if}
            {/if}
          </div>
        </div>
        <div class="control">
          {#if owner === "owned"}
            {@render chips(settings.exclude.value, !readOnly, (p) => void removePattern("exclude", settings.exclude.value, p), `Excluded from ${project.name}`)}
            {#if readOnly}
              <p class="note">Read-only mode is on. Turn it off to change this list.</p>
            {:else}
              {@render adder("exclude", settings.exclude.value, "Add a pattern to exclude")}
              <p class="note">Added in this app, so the app hands this list to the capture hook.</p>
            {/if}
          {:else}
            {@render chips(settings.exclude.value, false, () => {}, `Excluded from ${project.name}`)}
            {#if owner === "managedByVsCode"}
              <p class="note">Managed by VS Code. A VS Code window wrote its own list for this project, so the app no longer changes it. Change it there, or hide more files below.</p>
              {#if readOnly}
                <p class="note">Read-only mode is on. Turn it off to take this list back.</p>
              {:else if confirmTakeBack}
                <div class="confirm" role="group" aria-label="Take back the exclude list">
                  <p class="note">ClaudeGate will manage this project's exclude again. If it's open in VS Code, VS Code will overwrite it on its next start.</p>
                  <button class="link" onclick={() => void takeBack()}>Take back</button>
                  <button class="link" onclick={() => (confirmTakeBack = false)}>Cancel</button>
                </div>
              {:else}
                <button class="link" onclick={() => (confirmTakeBack = true)} title="Let this app manage the project's exclude list again">Take back</button>
              {/if}
            {:else if managedBy}
              <p class="note">Managed by {managedBy}. Change it there, or hide more files below.</p>
            {:else if !excludeOwned}
              <p class="note">The extension's default list.</p>
            {/if}
          {/if}
        </div>
      </div>

      <div class="row wide">
        <div class="label">
          <p class="title">Extra patterns (hide in app only)</p>
          <p class="desc">Hidden from this app's lists for {project.name}. Claude's changes to them are still captured.</p>
          <div class="source"><span class="badge app">This app</span></div>
        </div>
        <div class="control">
          {@render chips(settings.extraHide, true, (p) => void removePattern("extraHide", settings.extraHide, p), "Extra patterns")}
          {@render adder("extraHide", settings.extraHide, "Add a pattern to hide")}
        </div>
      </div>
    {:else}
      <p class="none pad">Select a project to see what it excludes.</p>
    {/if}

    <h3 class="group">Capture</h3>

    <div class="row">
      <div class="label">
        <p class="title">Hook log</p>
        <p class="desc">Write every capture to ~/.claudegate/hook.log. Shared with the VS Code extension.</p>
        <div class="source"><span class="badge" class:app={settings.hookLog.source === "app"}>{settings.hookLog.value ? "On for every ClaudeGate" : "Default"}</span></div>
      </div>
      <div class="control">
        {@render toggle(settings.hookLog.value, "Hook log", (on) => void onhooklog(on).then(stamp), readOnly)}
        {#if readOnly}<p class="note">Read-only mode is on. Turn it off to change the hook log.</p>{/if}
      </div>
    </div>

    <h3 class="group">App</h3>

    <div class="row">
      <div class="label">
        <p class="title" id="set-editor">Open files in</p>
        <p class="desc">The editor “Open” uses, at the line you are looking at.</p>
      </div>
      <div class="control">
        <div class="segmented" role="group" aria-labelledby="set-editor">
          {#each editors as o (o.template)}
            <button aria-pressed={o.current} class:on={o.current} title={o.template} onclick={() => void oneditor(o.template).then(stamp)}>
              {o.label}
            </button>
          {/each}
        </div>
      </div>
    </div>

    <div class="row">
      <div class="label">
        <p class="title" id="set-refstyle">Path in copied references</p>
        <p class="desc">“Copy reference for Claude” writes @src/app.ts (relative to the project) or the full path. Files outside the project always use the full path.</p>
      </div>
      <div class="control">
        <div class="segmented" role="group" aria-labelledby="set-refstyle">
          {#each [["relative", "Relative"], ["absolute", "Absolute"]] as [v, text] (v)}
            <button aria-pressed={settings.copyRefPathStyle === v} class:on={settings.copyRefPathStyle === v} onclick={() => void set("copyRefPathStyle", v)}>{text}</button>
          {/each}
        </div>
      </div>
    </div>

    <div class="row">
      <div class="label">
        <p class="title">Read-only mode</p>
        <p class="desc">Block every accept, reject, revert and reapply until it is turned off.</p>
      </div>
      <div class="control">
        {@render toggle(config.readOnly === true, "Read-only mode", (on) => onreadonly(on))}
      </div>
    </div>

    <div class="row wide last">
      <div class="label">
        <p class="title">Hidden projects</p>
        <p class="desc">Left out of the project list, the total and the tray.</p>
      </div>
      <div class="control">
        {#if hidden.length === 0}
          <p class="none">None hidden.</p>
        {:else}
          <ul class="hidden-list">
            {#each hidden as h (h.root)}
              <li>
                <span class="hname" title={h.root}>{h.name}</span>
                <button class="btn" onclick={() => onshow(h.root)}>Show</button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  </div>
</section>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    container: settings / inline-size;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 44px;
    padding: 0 10px 0 var(--pad);
    border-bottom: 1px solid var(--rule);
    flex: none;
  }
  h1.title-h {
    font-size: var(--fs-title);
  }
  .title-h {
    font-size: var(--fs-section);
    font-weight: 600;
  }
  .saved {
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .close {
    margin-left: auto;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-control);
    color: var(--muted);
  }
  .close:hover {
    color: var(--text);
    background: var(--hover);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px var(--pad) 32px;
  }
  .intro {
    max-width: 62ch;
    margin: 14px 0 6px;
    font-size: var(--fs-body);
    line-height: 1.55;
    color: var(--muted);
  }
  .group {
    margin: 26px 0 2px;
    font-size: var(--fs-body);
    font-weight: 600;
    color: var(--text);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px 24px;
    align-items: start;
    padding: 14px 0;
    border-bottom: 1px solid var(--rule);
  }
  .row.last {
    border-bottom: 0;
  }
  /* Lists need the width: their control sits under the label. */
  .row.wide {
    grid-template-columns: minmax(0, 1fr);
  }
  @container settings (min-width: 640px) {
    .row.wide {
      grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.1fr);
    }
  }
  .title {
    display: block;
    font-size: var(--fs-body);
    font-weight: 500;
    line-height: 1.4;
  }
  .desc {
    margin-top: 2px;
    max-width: 52ch;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .source {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }
  .badge {
    padding: 0 7px;
    border-radius: var(--radius-chip);
    font-size: var(--fs-meta);
    font-weight: 500;
    line-height: 18px;
    color: var(--muted);
    background: var(--raised);
    white-space: nowrap;
  }
  .badge.app {
    color: var(--mark);
    background: color-mix(in srgb, var(--mark) 12%, transparent);
  }
  .link {
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--mark);
    padding: 1px 2px;
    border-radius: var(--radius-chip);
  }
  .link:hover {
    text-decoration: underline;
  }
  .control {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    min-width: 0;
  }
  .row:not(.wide) .control {
    align-items: flex-end;
    padding-top: 1px;
  }

  .switch {
    position: relative;
    width: 32px;
    height: 18px;
    flex: none;
    border-radius: 9px;
    background: var(--control-border);
    transition: background 120ms var(--ease);
  }
  .switch[aria-checked="true"] {
    background: var(--positive);
  }
  .switch:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--surface);
    transition: transform 120ms var(--ease);
  }
  .switch[aria-checked="true"] .knob {
    transform: translateX(14px);
  }
  @media (prefers-reduced-motion: reduce) {
    .switch,
    .knob {
      transition: none;
    }
  }

  .number {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  input {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font: inherit;
    font-size: var(--fs-body);
    color: var(--text);
    background: var(--surface);
  }
  .number input {
    width: 72px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  input[aria-invalid="true"] {
    border-color: var(--negative);
  }
  .unit {
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .err {
    font-size: var(--fs-small);
    color: var(--error-ink);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    padding: 0 6px;
    border-radius: var(--radius-chip);
    background: var(--raised);
  }
  .glob {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--text);
  }
  .chip-note {
    margin-left: 4px;
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .x {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    margin-right: -3px;
    border-radius: 3px;
    color: var(--muted);
  }
  .x:hover {
    color: var(--text);
    background: var(--hover);
  }
  .adder {
    display: flex;
    gap: 6px;
    width: 100%;
    max-width: 340px;
  }
  .adder input {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: var(--fs-small);
  }
  .note,
  .none {
    align-self: stretch;
    text-align: left;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--muted);
  }
  .none.pad {
    padding: 14px 0;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn:hover:not(:disabled) {
    background: var(--hover);
  }
  .btn:disabled {
    color: var(--muted);
  }

  .segmented {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    border-radius: var(--radius-control);
    background: var(--raised);
  }
  .segmented button {
    height: 24px;
    padding: 0 10px;
    border-radius: 4px;
    font-size: var(--fs-small);
    font-weight: 500;
    color: var(--muted);
  }
  .segmented button:hover {
    color: var(--text);
  }
  .segmented button.on {
    color: var(--text);
    background: var(--active);
  }

  .hidden-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    max-width: 340px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .hidden-list li {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .hname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-body);
  }
</style>
