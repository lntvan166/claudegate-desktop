<script lang="ts">
  import { onMount, tick } from "svelte";
  import { captureStatus, setUpCapture } from "../lib/api";
  import {
    CONFIRM_TEXT,
    canRecheck,
    canSetUp,
    capturing,
    chipLabel,
    detail,
    setupResult,
    type CaptureView,
    type SetupResult,
  } from "../lib/capture";

  let { readOnly }: { readOnly: boolean } = $props();

  let view: CaptureView | null = $state(null);
  let open = $state(false);
  let confirming = $state(false);
  let busy = $state(false);
  let error: string | null = $state(null);
  let result: SetupResult | null = $state(null);

  let wrap: HTMLDivElement | undefined = $state();
  let chip: HTMLButtonElement | undefined = $state();
  let pop: HTMLDivElement | undefined = $state();
  let cancelBtn: HTMLButtonElement | undefined = $state();

  const tone = $derived(!view ? "idle" : capturing(view) ? "ok" : "off");

  async function refresh(recheck = false) {
    try {
      view = await captureStatus(recheck);
      if (recheck) error = null;
    } catch (e) {
      error = `Couldn't read the capture status: ${e}`;
    }
  }

  function toggle() {
    if (open) return close();
    open = true;
    error = null;
    result = null;
    void refresh();
  }

  function close(refocus = true) {
    open = false;
    confirming = false;
    if (refocus) chip?.focus();
  }

  async function askSetUp() {
    error = null;
    result = null;
    confirming = true;
    await tick();
    cancelBtn?.focus();
  }

  async function cancelConfirm() {
    confirming = false;
    await tick();
    pop?.querySelector<HTMLButtonElement>("[data-setup]")?.focus();
  }

  async function confirmSetUp() {
    busy = true;
    error = null;
    try {
      const r = await setUpCapture();
      view = r.capture;
      result = setupResult(r);
      void refresh();
    } catch (e) {
      error = String(e);
      void refresh();
    } finally {
      busy = false;
      confirming = false;
    }
  }

  async function checkAgain() {
    busy = true;
    result = null;
    await refresh(true);
    busy = false;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (confirming) void cancelConfirm();
      else close();
      return;
    }
    // While confirming, Tab stays on the two answers.
    if (e.key === "Tab" && confirming && pop) {
      const b = [...pop.querySelectorAll<HTMLButtonElement>("[data-answer]:not(:disabled)")];
      const i = b.indexOf(document.activeElement as HTMLButtonElement);
      if (b.length) {
        e.preventDefault();
        b[(i + (e.shiftKey ? b.length - 1 : 1)) % b.length].focus();
      }
    }
  }

  function outside(e: PointerEvent) {
    if (open && !wrap?.contains(e.target as Node)) close(false);
  }

  onMount(() => {
    void refresh();
    // Setup can happen elsewhere (VS Code, another window): read again when the window comes back.
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });
</script>

<svelte:window onpointerdown={outside} />

<div class="capture" bind:this={wrap}>
  <button
    class="chip"
    class:open
    bind:this={chip}
    aria-haspopup="dialog"
    aria-expanded={open}
    title="Whether Claude Code's edits are captured for review"
    onclick={toggle}
  >
    <span class="dot {tone}" aria-hidden="true"></span>
    {chipLabel(view)}
  </button>

  {#if open && view}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="pop" role="dialog" aria-label="Capture" data-confirm tabindex="-1" bind:this={pop} {onkeydown}>
      {#if confirming}
        <p class="body strong">{CONFIRM_TEXT}</p>
        <div class="buttons">
          <button class="btn go" data-answer disabled={busy} onclick={() => void confirmSetUp()}>
            {busy ? "Setting up…" : "Set up"}
          </button>
          <button class="btn" data-answer bind:this={cancelBtn} onclick={() => void cancelConfirm()}>Cancel</button>
        </div>
      {:else}
        <p class="title"><span class="dot {tone}" aria-hidden="true"></span>{chipLabel(view)}</p>
        <p class="body">{detail(view)}</p>
        {#if view.status === "not-set-up" && readOnly}
          <p class="body muted">Read-only mode is on. Turn it off to set up capture.</p>
        {/if}
        {#if result}
          <p class="banner {result.tone}" role="status">{result.text}</p>
        {/if}
        {#if error}
          <p class="banner error" role="alert">{error}</p>
        {/if}
        {#if canSetUp(view, readOnly) || canRecheck(view)}
          <div class="buttons">
            {#if canSetUp(view, readOnly)}
              <button class="btn go" data-setup disabled={busy} onclick={() => void askSetUp()}>Set up…</button>
            {/if}
            {#if canRecheck(view)}
              <button class="btn" disabled={busy} onclick={() => void checkAgain()}>Check again</button>
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .capture {
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
    color: var(--muted);
    transition:
      color 120ms var(--ease),
      background 120ms var(--ease);
  }
  .chip:hover,
  .chip.open {
    color: var(--text);
    background: var(--hover);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.ok {
    background: var(--positive);
  }
  .dot.off {
    background: var(--warning-ink);
  }

  .pop {
    position: absolute;
    z-index: 45;
    top: calc(100% + 6px);
    right: 0;
    width: 340px;
    max-width: calc(100vw - 32px);
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--menu-shadow);
    display: grid;
    gap: 10px;
    font-size: var(--fs-small);
    animation: pop 100ms var(--ease);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pop {
      animation: none;
    }
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--fs-body);
    font-weight: 600;
    color: var(--text);
  }
  .body {
    line-height: 1.5;
    color: var(--text);
  }
  .body.strong {
    font-weight: 500;
  }
  .body.muted {
    color: var(--muted);
  }
  .banner {
    padding: 8px 10px;
    border-radius: var(--radius-control);
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  .banner.success {
    color: var(--positive);
    background: color-mix(in srgb, var(--positive) 12%, transparent);
  }
  .banner.warning {
    color: var(--warning-ink);
    background: var(--warning-bg);
  }
  .banner.error {
    color: var(--error-ink);
    background: var(--error-bg);
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
