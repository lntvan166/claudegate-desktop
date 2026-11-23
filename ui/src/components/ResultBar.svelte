<script lang="ts">
  import { RESULT_HIDE_MS, type ResultMessage } from "../lib/decide";

  let {
    result,
    busy,
    ondismiss,
  }: {
    result: ResultMessage | null;
    busy: boolean;
    ondismiss: () => void;
  } = $props();

  let hovered = $state(false);
  let focused = $state(false);
  let showDetails = $state(false);

  // A new result starts with its details folded.
  $effect(() => {
    void result?.id;
    showDetails = false;
  });

  // Auto-hide after 6 s, paused while hovered or focused; failures stay until dismissed.
  $effect(() => {
    if (!result || result.tone !== "ok" || hovered || focused || showDetails) return;
    const t = setTimeout(ondismiss, RESULT_HIDE_MS);
    return () => clearTimeout(t);
  });

  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  const short = (p: string) => p.replace(/\\/g, "/").split("/").slice(-2).join("/");
</script>

{#if result}
  <div
    class="result {result.tone}"
    role={result.tone === "error" ? "alert" : "status"}
    onmouseenter={() => (hovered = true)}
    onmouseleave={() => (hovered = false)}
    onfocusin={() => (focused = true)}
    onfocusout={() => (focused = false)}
  >
    <div class="line">
      {#if result.tone === "ok"}
        <svg class="glyph" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M3.5 8.5 6.5 11.5 12.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      {:else}
        <svg class="glyph" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M8 4.8v3.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          <circle cx="8" cy="11" r="0.9" fill="currentColor" />
        </svg>
      {/if}
      <p class="text">{result.text}</p>
      {#if result.undo}
        <button class="act" disabled={busy} title={isMac ? "Undo (⌘Z)" : "Undo (Ctrl+Z)"} aria-keyshortcuts={isMac ? "Meta+Z" : "Control+Z"} onclick={result.undo}>Undo</button>
      {/if}
      {#if result.details?.length}
        <button class="act" aria-expanded={showDetails} onclick={() => (showDetails = !showDetails)}>Details</button>
      {/if}
      {#if result.retry}
        <button class="act" disabled={busy} onclick={result.retry}>Retry</button>
      {/if}
      <button class="dismiss" aria-label="Dismiss" title="Dismiss" onclick={ondismiss}>
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>
    </div>
    {#if result.note}<p class="note">{result.note}</p>{/if}
    {#if showDetails && result.details}
      <ul class="details">
        {#each result.details as [path, message] (path)}
          <li><span class="path" title={path}>{short(path)}</span> <span class="why">{message}</span></li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .result {
    flex: none;
    padding: 0 10px 0 var(--pad);
    border-top: 1px solid var(--rule);
    background: var(--raised);
    font-size: var(--fs-small);
    user-select: text;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
  }
  .glyph {
    flex: none;
    color: var(--positive);
  }
  .warn .glyph {
    color: var(--warning-ink);
  }
  .error {
    background: var(--error-bg);
    color: var(--error-ink);
  }
  .error .glyph {
    color: inherit;
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .error .text {
    white-space: normal;
    padding: 10px 0;
  }

  .act {
    flex: none;
    height: 26px;
    padding: 0 8px;
    border-radius: var(--radius-control);
    font-weight: 600;
    color: var(--text);
    transition: background 120ms var(--ease);
  }
  .error .act {
    color: inherit;
  }
  .act:hover:not(:disabled) {
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  .act:disabled {
    opacity: 0.5;
  }

  .dismiss {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-chip);
    color: var(--muted);
  }
  .error .dismiss {
    color: inherit;
  }
  .dismiss:hover {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .note {
    margin: -4px 0 10px 22px;
    line-height: 1.45;
  }

  .details {
    margin: 0 0 10px 22px;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 3px;
  }
  .path {
    font-family: var(--font-mono);
  }
  .why {
    color: var(--muted);
  }
  .error .why {
    color: inherit;
    opacity: 0.85;
  }
</style>
