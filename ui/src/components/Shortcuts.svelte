<script lang="ts">
  import { onMount, tick } from "svelte";

  let { editorName, onclose }: { editorName: string; onclose: () => void } = $props();

  let box: HTMLDivElement | undefined = $state();
  /** Focus returns where it was when the overlay closes. */
  const before = typeof document === "undefined" ? null : (document.activeElement as HTMLElement | null);

  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  const groups: { title: string; keys: [string[], string][] }[] = $derived([
    {
      title: "Move",
      keys: [
        [["J", "K"], "Next and previous file"],
        [["N", "P"], "Next and previous pending, across worktrees and repos"],
        [["1", "2", "3"], "Pending, accepted, rejected"],
        [["/"], "Filter the pending list; Esc clears it"],
        [["["], "Collapse or expand projects"],
        [["←", "→"], "Fold or unfold a folder in the list"],
        [[isMac ? "⌥F5" : "Alt+F5", isMac ? "⇧⌥F5" : "Shift+Alt+F5"], "Next and previous change in the diff"],
        [[isMac ? "⌥Z" : "Alt+Z"], "Wrap long lines in the diff, or stop"],
      ],
    },
    {
      title: "Decide",
      keys: [
        [["A", "R"], "Accept or reject the focused file or folder"],
        [["⇧A"], "Accept all (asks first above 5 files); with a filter, accept the matching files (asks first)"],
        [["⇧R"], "Reject all (asks first); with a filter, reject the matching files"],
        [["Delete"], "Remove the focused project from your workspace"],
        [["Shift+F10"], "The focused project's menu"],
        [[isMac ? "⌘Z" : "Ctrl+Z"], "Undo, while the result bar offers it"],
      ],
    },
    {
      title: "Open",
      keys: [
        [["O"], `Open in ${editorName} at the cursor, or the first change`],
        [["F12"], `Open in ${editorName} at the cursor`],
        [["Double-click"], `Open a file in ${editorName}`],
        [[isMac ? "⌘ click" : "Ctrl click"], `Open in ${editorName} at that token`],
        [[isMac ? "⌘⌥C" : "Ctrl+Alt+C"], "Copy a reference for Claude: the selection, the cursor line, or the file"],
        [[isMac ? "⌘⌥⇧C" : "Ctrl+Alt+Shift+C"], "Copy a reference to the whole file"],
        [["Right-click"], "On a pending row: copy a reference, or hide its extension or folder from review"],
        [["Esc"], "Close this, a menu, or the narrow diff"],
        [["?"], "Show or hide these shortcuts"],
      ],
    },
  ]);

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" || e.key === "?") {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    // Focus stays in the dialog.
    if (e.key === "Tab") {
      e.preventDefault();
      box?.querySelector<HTMLButtonElement>("button")?.focus();
    }
  }

  function close() {
    onclose();
    void tick().then(() => before?.focus?.());
  }

  onMount(() => {
    box?.querySelector<HTMLButtonElement>("button")?.focus();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={close}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <div
    class="sheet"
    role="dialog"
    aria-modal="true"
    aria-labelledby="shortcuts-title"
    tabindex="-1"
    bind:this={box}
    {onkeydown}
    onclick={(e) => e.stopPropagation()}
  >
    <header>
      <h2 id="shortcuts-title">Keyboard shortcuts</h2>
      <button class="close" aria-label="Close shortcuts" title="Close (Esc)" onclick={close}>
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>
    </header>
    <div class="cols">
      {#each groups as g (g.title)}
        <section>
          <h3>{g.title}</h3>
          <dl>
            {#each g.keys as [keys, what] (what)}
              <div>
                <dt>{#each keys as key (key)}<kbd>{key}</kbd>{/each}</dt>
                <dd>{what}</dd>
              </div>
            {/each}
          </dl>
        </section>
      {/each}
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    padding: 16px;
    background: color-mix(in srgb, var(--frame) 70%, transparent);
    animation: fade 120ms var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .sheet {
    width: min(880px, 100%);
    max-height: 100%;
    overflow: auto;
    padding: 18px 20px 20px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--menu-shadow);
  }
  .sheet:focus {
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
  }
  h2 {
    margin: 0;
    font-size: var(--fs-section);
    font-weight: 600;
  }
  .close {
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
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 18px 24px;
  }
  h3 {
    margin: 0 0 8px;
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--muted);
  }
  /* One key column per section, as wide as its longest chord ("Ctrl+Alt+Shift+C"), so a
     chord never runs into its description. */
  dl {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 7px 10px;
    margin: 0;
    font-size: var(--fs-small);
  }
  dl div {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    align-items: baseline;
  }
  dt {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    justify-content: flex-end;
  }
  dd {
    margin: 0;
    color: var(--muted);
    line-height: 1.4;
  }
  kbd {
    min-width: 20px;
    padding: 1px 5px;
    border: 1px solid var(--control-border);
    border-bottom-width: 2px;
    border-radius: var(--radius-chip);
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    text-align: center;
    white-space: nowrap;
    color: var(--text);
    background: var(--raised);
  }
</style>
