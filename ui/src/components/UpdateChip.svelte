<script lang="ts">
  import { onMount } from "svelte";
  import { checkUpdate, installUpdate } from "../lib/api";
  import { writeClipboard } from "../lib/clipboard";
  import { FIRST_CHECK_MS, RECHECK_MS, chipLabel, summary, type UpdateView } from "../lib/update";

  // Nothing shows until a newer release exists; a failed check (offline) stays silent.
  let update: UpdateView | null = $state(null);
  let open = $state(false);
  let busy = $state(false);
  let copied = $state(false);
  let error: string | null = $state(null);

  let wrap: HTMLDivElement | undefined = $state();
  let chip: HTMLButtonElement | undefined = $state();

  async function check() {
    try {
      update = await checkUpdate();
    } catch {
      // Offline, or GitHub unreachable: try again at the next interval.
    }
  }

  function close(refocus = true) {
    open = false;
    if (refocus) chip?.focus();
  }

  async function install() {
    busy = true;
    error = null;
    try {
      await installUpdate(); // restarts the app on success
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  async function copy() {
    if (!update?.command) return;
    try {
      await writeClipboard(update.command);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      error = `Couldn't copy: ${e}`;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  }

  function outside(e: PointerEvent) {
    if (open && !wrap?.contains(e.target as Node)) close(false);
  }

  onMount(() => {
    const first = setTimeout(() => void check(), FIRST_CHECK_MS);
    const every = setInterval(() => void check(), RECHECK_MS);
    return () => {
      clearTimeout(first);
      clearInterval(every);
    };
  });
</script>

<svelte:window onpointerdown={outside} />

{#if update}
  <div class="update" bind:this={wrap}>
    <button
      class="chip"
      class:open
      bind:this={chip}
      aria-haspopup="dialog"
      aria-expanded={open}
      title="A newer ClaudeGate is available"
      onclick={() => (open ? close() : ((open = true), (error = null)))}
    >
      <span class="dot" aria-hidden="true"></span>
      {chipLabel(update)}
    </button>

    {#if open}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="pop" role="dialog" aria-label="Update" data-confirm tabindex="-1" {onkeydown}>
        <p class="title">ClaudeGate {update.version}</p>
        <p class="body">{summary(update)}</p>
        {#if update.command}
          <code class="cmd">{update.command}</code>
        {/if}
        {#if update.notes}
          <p class="notes">{update.notes}</p>
        {/if}
        {#if error}
          <p class="banner error" role="alert">{error}</p>
        {/if}
        <div class="buttons">
          {#if update.canInstall}
            <button class="btn go" disabled={busy} onclick={() => void install()}>
              {busy ? "Installing…" : "Install and restart"}
            </button>
          {:else}
            <button class="btn go" onclick={() => void copy()}>{copied ? "Copied" : "Copy command"}</button>
          {/if}
          <button class="btn" disabled={busy} onclick={() => close()}>Later</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .update {
    position: relative;
  }
  .chip {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    height: 24px;
    padding: 0 8px;
    border-radius: var(--radius-control);
    color: var(--primary);
    transition: background 120ms var(--ease);
  }
  .chip:hover,
  .chip.open {
    background: var(--hover);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--primary);
  }
  .pop {
    position: absolute;
    z-index: 45;
    top: calc(100% + 6px);
    right: 0;
    width: 380px;
    max-width: calc(100vw - 32px);
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--menu-shadow);
    display: grid;
    gap: 10px;
    font-size: var(--fs-small);
  }
  .title {
    font-size: var(--fs-body);
    font-weight: 600;
    color: var(--text);
  }
  .body {
    line-height: 1.5;
    color: var(--text);
  }
  .notes {
    line-height: 1.5;
    color: var(--muted);
    white-space: pre-line;
    max-height: 160px;
    overflow: auto;
  }
  .cmd {
    display: block;
    padding: 8px 10px;
    border-radius: var(--radius-control);
    background: var(--hover);
    font-family: var(--font-mono);
    font-size: var(--fs-small);
    line-height: 1.45;
    overflow-wrap: anywhere;
    user-select: all;
  }
  .banner.error {
    padding: 8px 10px;
    border-radius: var(--radius-control);
    color: var(--error-ink);
    background: var(--error-bg);
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
  .btn {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-weight: 600;
    white-space: nowrap;
    color: var(--text);
  }
  .btn:hover:not(:disabled) {
    background: var(--hover);
  }
  .btn.go {
    color: var(--on-primary);
    background: var(--primary);
    border-color: var(--primary);
  }
  .btn.go:hover:not(:disabled) {
    background: color-mix(in srgb, var(--primary) 88%, var(--text));
  }
  .btn:disabled {
    opacity: 0.6;
  }
</style>
