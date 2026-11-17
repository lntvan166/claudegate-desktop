<script lang="ts">
  import { tick } from "svelte";
  import type { DiffKind } from "../lib/api";
  import { bulkLabel } from "../lib/counts";
  import { bulkPrompt, rejectPrompt, type BulkAction, type RejectOutcome } from "../lib/decide";

  let {
    tab,
    keys,
    busy,
    outcomes = {},
    tabCount = 0,
    onaction,
  }: {
    tab: DiffKind;
    /** The files (or records, for Clear) the tab's bulk actions touch; see `bulkKeys`. */
    keys: string[];
    busy: boolean;
    /** Pending: what a reject does to each key, so Reject all names the files it deletes. */
    outcomes?: Record<string, RejectOutcome>;
    /** The tab's own count; the bar says "N to review" when it acts on fewer. */
    tabCount?: number;
    /** `keys` are exactly the rows counted in the question the user answered. */
    onaction: (a: BulkAction, keys: string[]) => void;
  } = $props();

  const count = $derived(keys.length);
  /** The action waiting for confirmation, if any, and the rows its question counted. */
  let confirming = $state<BulkAction | null>(null);
  let asked = $state<string[]>([]);
  let region: HTMLDivElement | undefined = $state();

  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

  const prompt = $derived(
    confirming === "rejectAll"
      ? rejectPrompt(asked.map((path) => ({ path, rejectOutcome: outcomes[path] ?? "restore" })))
      : confirming
        ? bulkPrompt(confirming, tab, asked.length)
        : null,
  );

  async function ask(a: BulkAction) {
    asked = [...keys];
    confirming = a;
    await tick();
    // Cancel is the safe default; a question whose answer destroys nothing (Accept all) defaults
    // to its own action.
    const sel = prompt && !prompt.danger ? "[data-go]" : "[data-cancel]";
    region?.querySelector<HTMLButtonElement>(sel)?.focus();
  }

  /** Shift+A / Shift+R: the same buttons, with the same confirmations. */
  export function trigger(a: "acceptAll" | "rejectAll") {
    if (busy || count === 0 || tab !== "pending" || confirming) return;
    if (a === "acceptAll") acceptAll();
    else void ask("rejectAll");
  }

  /** Accept all: at once for a few files, after "Accept N files?" for more. */
  function acceptAll() {
    if (bulkPrompt("acceptAll", tab, keys.length)) void ask("acceptAll");
    else onaction("acceptAll", [...keys]);
  }

  function cancel() {
    confirming = null;
  }

  function go() {
    const a = confirming;
    confirming = null;
    if (a) onaction(a, asked);
  }

  // Nothing left to act on (the list emptied underneath): the question no longer applies.
  $effect(() => {
    if (count === 0) confirming = null;
  });

  /** Esc answers no from anywhere, unless the Open-with menu is up (Esc is the menu's first). */
  function escAnywhere(e: KeyboardEvent) {
    if (!confirming || e.key !== "Escape" || e.defaultPrevented) return;
    if (document.querySelector('[role="menu"]')) return;
    e.preventDefault();
    cancel();
  }

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

<svelte:window onkeydown={escAnywhere} />

{#if confirming && prompt}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="bar confirm" role="group" aria-label="Confirm" data-confirm bind:this={region} onkeydown={trap}>
    <p class="question">{prompt.text}</p>
    <div class="buttons">
      <button class="btn" class:danger={prompt.danger} class:primary={!prompt.danger} data-go disabled={busy} onclick={go}>{prompt.go}</button>
      <button class="btn" data-cancel onclick={cancel}>Cancel</button>
    </div>
  </div>
{:else}
  <div class="bar" role="toolbar" aria-label="{tab} actions">
    {#if tab === "pending"}
      <button class="btn" disabled={busy || count === 0} onclick={() => acceptAll()}>
        {@render check()} Accept all
      </button>
      <button class="btn" disabled={busy || count === 0} onclick={() => void ask("rejectAll")}>
        {@render cross()} Reject all
      </button>
    {:else}
      <button
        class="btn"
        disabled={busy || count === 0}
        onclick={() => void ask(tab === "accepted" ? "revertAll" : "reapplyAll")}
      >
        {@render undo()}
        {tab === "accepted" ? "Revert all" : "Reapply all"}
      </button>
      <button class="btn quiet" disabled={busy || count === 0} onclick={() => void ask("clear")}>Clear</button>
    {/if}
    {#if count > 0}
      <span class="count num" title={count !== tabCount ? "Files whose write has landed; the rest can't be decided yet" : undefined}>{bulkLabel(count, tabCount, tab)}</span>
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
    color: var(--on-danger);
    background: var(--danger);
  }
  .btn.danger:hover:not(:disabled) {
    background: var(--danger-hover);
  }
  .btn.primary {
    border-color: transparent;
    color: var(--on-primary);
    background: var(--primary);
  }
  .btn.primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--primary) 88%, var(--text));
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
