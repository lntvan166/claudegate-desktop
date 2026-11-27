<script lang="ts">
  import { keyStep } from "../lib/panes";

  // A drag handle between two columns (default) or two rows (`direction="row"`). It reports sizes
  // (a width, or a height); the caller clamps and stores.
  let {
    label,
    direction = "col",
    value,
    min,
    max,
    left = 0,
    top = 0,
    onresize,
    oncommit,
    onreset,
  }: {
    label: string;
    /** "col": a vertical handle between columns; "row": a horizontal one between rows. */
    direction?: "col" | "row";
    /** The size of the column to the handle's left (or of the row above it, for "row"). */
    value: number;
    min: number;
    max: number;
    /** The x of the 1px edge the handle sits on, in px from its positioned parent's left edge. */
    left?: number;
    /** "row" only: the y of the 1px edge the handle sits on, in px from its positioned parent's top edge. */
    top?: number;
    /** A new width for the left column, while dragging or from the keyboard. */
    onresize: (width: number) => void;
    /** The drag or key press is over: a good time to persist. */
    oncommit: () => void;
    onreset: () => void;
  } = $props();

  let dragging = $state(false);
  const row = $derived(direction === "row");
  let startX = 0;
  let startWidth = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    (e.currentTarget as HTMLElement).focus();
    startX = row ? e.clientY : e.clientX;
    startWidth = value;
    dragging = true;
    document.documentElement.classList.add(row ? "row-resizing" : "col-resizing");
  }

  function move(e: PointerEvent) {
    if (!dragging) return;
    onresize(startWidth + (row ? e.clientY : e.clientX) - startX);
  }

  function up(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    document.documentElement.classList.remove("col-resizing", "row-resizing");
    oncommit();
  }

  function key(e: KeyboardEvent) {
    let next: number | null = null;
    // A row handle moves with the vertical arrows (Up shrinks the row above); a column handle with the horizontal ones.
    const k = row ? (e.key === "ArrowUp" ? "ArrowLeft" : e.key === "ArrowDown" ? "ArrowRight" : "") : e.key;
    const step = keyStep(k, e.shiftKey);
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
  class:row
  style:left={row ? null : `${left}px`}
  style:top={row ? `${top}px` : null}
  role="separator"
  aria-orientation={row ? "horizontal" : "vertical"}
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
  /* Keyboard focus: a visible ring around the handle, not just a tinted 1px line. */
  .splitter:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -1px;
    border-radius: 4px;
  }
  .splitter:hover::after,
  .splitter:focus-visible::after,
  .splitter.dragging::after {
    background: var(--focus-ring);
  }
  .splitter.row {
    top: auto;
    bottom: auto;
    left: 0;
    right: 0;
    width: auto;
    height: 8px;
    margin-left: 0;
    margin-top: -4px;
    cursor: row-resize;
  }
  .splitter.row::after {
    top: 4px;
    bottom: auto;
    left: 0;
    right: 0;
    width: auto;
    height: 1px;
  }
  :global(html.col-resizing),
  :global(html.col-resizing *) {
    cursor: col-resize !important;
    user-select: none !important;
  }
  :global(html.row-resizing),
  :global(html.row-resizing *) {
    cursor: row-resize !important;
    user-select: none !important;
  }
</style>
