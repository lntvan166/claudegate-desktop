<script lang="ts">
  import { tick } from "svelte";
  import type { DiffKind } from "../lib/api";
  import type { BulkAction } from "../lib/decide";

  let {
    tab,
    count,
    busy,
    onaction,
  }: {
    tab: DiffKind;
    /** How many files (or records, for Clear) the tab's bulk actions touch. */
    count: number;
    busy: boolean;
    onaction: (a: BulkAction) => void;
  } = $props();

  /** The action waiting for confirmation, if any. */
  let confirming = $state<BulkAction | null>(null);
  let region: HTMLDivElement | undefined = $state();

  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

  const prompt = $derived.by(() => {
    if (confirming === "rejectAll") {
      return {
        text: `Reject ${plural(count, "file", "files")}? Originals are restored and files Claude created are deleted.`,
        go: `Reject ${count}`,
      };
    }
    if (confirming === "clear") {
      const list = tab === "accepted" ? "Accepted" : "Rejected";
      return {
        text: `Clear ${plural(count, "record", "records")} from ${list}? Files on disk stay as they are.`,
        go: `Clear ${count}`,
      };
    }
    return null;
  });

  async function ask(a: BulkAction) {
    confirming = a;
    await tick();
    // Cancel is the safe default.
    region?.querySelector<HTMLButtonElement>("[data-cancel]")?.focus();
  }

  function cancel() {
    confirming = null;
  }

  function go() {
    const a = confirming;
    confirming = null;
    if (a) onaction(a);
  }

  // Nothing left to act on (the list emptied underneath): the question no longer applies.
  $effect(() => {
    if (count === 0) confirming = null;
  });

  /** Focus stays inside the question until it is answered; Esc answers no. */
  function trap(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancel();
      return;
    }
    if (e.key !== "Tab" || !region) return;
    const buttons = [...region.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    if (buttons.length === 0) return;
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.shiftKey ? (i <= 0 ? buttons.length - 1 : i - 1) : i === buttons.length - 1 ? 0 : i + 1;
    buttons[next].focus();
    e.preventDefault();
  }
</script>

{#snippet check()}
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M3.5 8.5 6.5 11.5 12.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/snippet}
{#snippet cross()}
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
  </svg>
{/snippet}
{#snippet undo()}
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M5.5 3.5 2.5 6.5l3 3M2.5 6.5h7a3.5 3.5 0 0 1 0 7H7" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/snippet}

{#if confirming && prompt}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="bar confirm" role="group" aria-label="Confirm" data-confirm bind:this={region} onkeydown={trap}>
    <p class="question">{prompt.text}</p>
    <div class="buttons">
      <button class="btn danger" disabled={busy} onclick={go}>{prompt.go}</button>
      <button class="btn" data-cancel onclick={cancel}>Cancel</button>
    </div>
  </div>
{:else}
  <div class="bar" role="toolbar" aria-label="{tab} actions">
    {#if tab === "pending"}
      <button class="btn" disabled={busy || count === 0} onclick={() => onaction("acceptAll")}>
        {@render check()} Accept all
      </button>
      <button class="btn" disabled={busy || count === 0} onclick={() => void ask("rejectAll")}>
        {@render cross()} Reject all
      </button>
    {:else}
      <button
        class="btn"
        disabled={busy || count === 0}
        onclick={() => onaction(tab === "accepted" ? "revertAll" : "reapplyAll")}
      >
        {@render undo()}
        {tab === "accepted" ? "Revert all" : "Reapply all"}
      </button>
      <button class="btn quiet" disabled={busy || count === 0} onclick={() => void ask("clear")}>Clear</button>
    {/if}
    {#if count > 0}
      <span class="count num">{plural(count, tab === "pending" ? "file" : "record", tab === "pending" ? "files" : "records")}</span>
    {/if}
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--rule);
    flex: none;
  }

  .count {
    margin-left: auto;
    font-size: var(--fs-small);
    color: var(--muted);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    font-size: var(--fs-small);
    font-weight: 500;
    white-space: nowrap;
    color: var(--text);
    background: var(--surface);
    transition:
      background 120ms var(--ease),
      border-color 120ms var(--ease);
  }
  .btn:hover:not(:disabled) {
    background: var(--hover);
  }
  .btn:active:not(:disabled) {
    background: var(--active);
  }
  .btn:disabled {
    color: var(--faint);
    opacity: 0.6;
  }
  .btn.quiet {
    border-color: transparent;
    color: var(--muted);
  }
  .btn.quiet:hover:not(:disabled) {
    color: var(--text);
  }
  .btn.danger {
    border-color: transparent;
    color: #fff;
    background: var(--danger);
  }
  .btn.danger:hover:not(:disabled) {
    background: var(--danger-hover);
  }

  .confirm {
    flex-wrap: wrap;
    align-items: center;
    row-gap: 8px;
    background: var(--raised);
  }
  .question {
    flex: 1 1 200px;
    min-width: 0;
    font-size: var(--fs-small);
    line-height: 1.45;
    color: var(--text);
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
</style>
