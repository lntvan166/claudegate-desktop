// Where a header chip's popover goes. Pure, so both chips (Update, Capture) share one rule.

/**
 * The popover's left edge, in px from its positioned wrapper's left edge: right-aligned with the
 * chip that opened it, and pulled back inside the window, `margin` px clear of either edge, when
 * that would put it past one (a chip near the left of a narrow window opened it off-screen).
 */
export function popoverLeft(
  chip: { right: number },
  wrapLeft: number,
  width: number,
  viewport: number,
  margin = 16,
): number {
  const w = Math.min(width, viewport - 2 * margin);
  const left = Math.max(margin, Math.min(chip.right - w, viewport - margin - w));
  return left - wrapLeft;
}
