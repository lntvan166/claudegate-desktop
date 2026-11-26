// The header switch's focus and overlay rules. The two views stay mounted, one hidden with
// display:none, so anything that asks "is a menu or confirmation up?" must only count what is on
// screen, and focus must never be left on an element the switch just hid.

/** What the helpers need of an element; a real HTMLElement fits. */
export type Focusable = {
  isConnected: boolean;
  checkVisibility?: () => boolean;
  getClientRects?: () => { length: number };
};

/** On screen: connected and rendered (not inside a display:none ancestor). */
export function isShown(el: Focusable | null | undefined): boolean {
  if (!el || !el.isConnected) return false;
  if (typeof el.checkVisibility === "function") return el.checkVisibility();
  return (el.getClientRects?.().length ?? 0) > 0;
}

/** The first candidate that is on screen, in order. */
export function firstShown<T extends Focusable>(candidates: (T | null | undefined)[]): T | null {
  return candidates.find((c): c is T => isShown(c)) ?? null;
}

/** Whether a menu or a confirmation is open on screen; hidden ones (in the other view) don't count. */
export function overlayShown(all: Iterable<Focusable>): boolean {
  for (const el of all) if (isShown(el)) return true;
  return false;
}

export const OVERLAY_SELECTOR = '[role="menu"], [data-confirm]';

/**
 * Whether the switch must move focus: when it sits on something the switch hid (or nowhere).
 * Focus on the header switch itself, or anything else still on screen, stays where it is.
 */
export function needsFocusMove(active: Focusable | null | undefined, isBody: boolean): boolean {
  return isBody || !isShown(active);
}
