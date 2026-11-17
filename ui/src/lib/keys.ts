import type { DiffKind } from "./api";

/** One step through a list of `len`, clamped at both ends (no wrap); from nothing, the near end. */
export function nextIndex(current: number, len: number, dir: 1 | -1): number {
  if (len <= 0) return -1;
  if (current < 0 || current >= len) return dir === 1 ? 0 : len - 1;
  return Math.min(len - 1, Math.max(0, current + dir));
}

export type KeyAction =
  | { type: "row"; dir: 1 | -1 }
  | { type: "pending"; dir: 1 | -1 }
  | { type: "open" }
  | { type: "projects" }
  | { type: "tab"; tab: DiffKind };

type KeyLike = { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean };

const TABS: Record<string, DiffKind> = { "1": "pending", "2": "accepted", "3": "rejected" };

/** The review shortcut for a key press, or null to leave it to the system. */
export function actionFor(e: KeyLike): KeyAction | null {
  if (e.ctrlKey || e.metaKey || e.altKey) return null;
  switch (e.key) {
    case "j":
    case "ArrowDown":
      return { type: "row", dir: 1 };
    case "k":
    case "ArrowUp":
      return { type: "row", dir: -1 };
    case "n":
      return { type: "pending", dir: 1 };
    case "p":
      return { type: "pending", dir: -1 };
    case "o":
      return { type: "open" };
    case "[":
      return { type: "projects" };
  }
  const tab = TABS[e.key];
  return tab ? { type: "tab", tab } : null;
}

type ElementLike = { tagName: string; isContentEditable?: boolean; closest?: (sel: string) => unknown };

/** True when keys belong to what has focus: a field, an editable region (Monaco), or a menu. */
export function isTypingTarget(el: ElementLike | null | undefined): boolean {
  // A key sent to the window or the document has no tag: nothing is being typed into.
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (el.isContentEditable) return true;
  return !!el.closest?.('[role="menu"], [role="menubar"], [role="listbox"]');
}

export type DecisionContext = {
  tab: DiffKind;
  /** Focus is in a field, an editable region or a menu. */
  typing: boolean;
  /** The Open-with menu or an inline confirmation is up. */
  menuOpen: boolean;
  readOnly: boolean;
  /** A decision is already in flight; the UI runs one at a time. */
  busy: boolean;
  /** The selected row's key on the current tab. */
  selectedKey: string | null;
  /** The selected row is a capture whose write has not landed: it cannot be decided yet. */
  selectedWaiting?: boolean;
};

/** What a/r say on a row Claude has not written yet (the disabled buttons' tooltip too). */
export const WAITING_REFUSAL = "Claude hasn't written this yet";

export type KeyDecision = { action: "accept" | "reject"; key: string } | { refused: string };

/**
 * `a` accepts and `r` rejects the selected pending row; `refused` when the key applies but the row
 * cannot be decided (the reason is announced); null when the key does not apply now.
 */
export function decisionFor(e: KeyLike, c: DecisionContext): KeyDecision | null {
  if (e.ctrlKey || e.metaKey || e.altKey) return null;
  const action = e.key === "a" ? "accept" : e.key === "r" ? "reject" : null;
  if (!action) return null;
  if (c.tab !== "pending" || c.typing || c.menuOpen || c.readOnly || c.busy || !c.selectedKey) return null;
  if (c.selectedWaiting) return { refused: WAITING_REFUSAL };
  return { action, key: c.selectedKey };
}
