<script lang="ts">
  import { keyStep } from "../lib/panes";

  // A vertical drag handle between two columns. It reports widths; the caller clamps and stores.
  let {
    label,
    value,
    min,
    max,
    left,
    onresize,
    oncommit,
    onreset,
  }: {
    label: string;
    /** The width of the column to the handle's left. */
    value: number;
    min: number;
    max: number;
    /** The x of the 1px edge the handle sits on, in px from its positioned parent's left edge. */
    left: number;
    /** A new width for the left column, while dragging or from the keyboard. */
    onresize: (width: number) => void;
    /** The drag or key press is over: a good time to persist. */
    oncommit: () => void;
    onreset: () => void;
  } = $props();

  let dragging = $state(false);
  let startX = 0;
  let startWidth = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    (e.currentTarget as HTMLElement).focus();
    startX = e.clientX;
    startWidth = value;
    dragging = true;
    document.documentElement.classList.add("col-resizing");
  }

  function move(e: PointerEvent) {
    if (!dragging) return;
    onresize(startWidth + e.clientX - startX);
  }

  function up(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    document.documentElement.classList.remove("col-resizing");
    oncommit();
  }

  function key(e: KeyboardEvent) {
    let next: number | null = null;
    const step = keyStep(e.key, e.shiftKey);
    if (step !== null) next = value + step;
    else if (e.key === "Home") next = min;
    else if (e.key === "End") next = max;
    if (next === null) return;
    // The app's j/k/1/2/3 shortcuts listen on window; arrows here belong to the handle.
    e.preventDefault();
    e.stopPropagation();
    onresize(next);
    oncommit();
  }
</script>

<!-- A focusable separator with a value is a widget (WAI-ARIA window splitter); Svelte cannot tell. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="splitter"
  class:dragging
  style:left="{left}px"
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={Math.round(value)}
  aria-valuemin={min}
  aria-valuemax={Math.round(max)}
  tabindex="0"
  title="Drag to resize · double-click to reset"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onlostpointercapture={up}
  onkeydown={key}
  ondblclick={onreset}
></div>

<style>
  .splitter {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 5;
    width: 8px;
    margin-left: -4px;
    cursor: col-resize;
    touch-action: none;
    outline: none;
  }
  .splitter::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    /* Sits on the 1px edge it replaces: the handle's left is that edge's x, minus half the hit area. */
    left: 4px;
    width: 1px;
    background: var(--border);
    transition: background 120ms var(--ease);
  }
  .splitter:hover::after,
  .splitter:focus-visible::after,
  .splitter.dragging::after {
    background: var(--focus-ring);
  }
  :global(html.col-resizing),
  :global(html.col-resizing *) {
    cursor: col-resize !important;
    user-select: none !important;
  }
</style>
