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
  | { type: "tab"; tab: DiffKind }
  /** Shift+A / Shift+R: Accept all / Reject all, through their confirmations. */
  | { type: "bulk"; action: "acceptAll" | "rejectAll" }
  /** `?`: the shortcut overlay. */
  | { type: "help" }
  /** `/`: the Pending filter box. */
  | { type: "filter" };

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
    case "A":
      return { type: "bulk", action: "acceptAll" };
    case "R":
      return { type: "bulk", action: "rejectAll" };
    case "?":
      return { type: "help" };
    case "/":
      return { type: "filter" };
  }
  const tab = TABS[e.key];
  return tab ? { type: "tab", tab } : null;
}

type RefKeyLike = { code: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean };

/**
 * Copy reference for Claude: Ctrl+Alt+C (Cmd+Alt+C on macOS) copies the selection, the cursor line, or the whole
 * file with none; adding Shift always copies the whole file. `code`, not `key`: Alt changes the
 * character on macOS.
 */
export function refKeyFor(e: RefKeyLike, isMac: boolean): "selection" | "file" | null {
  if (e.code !== "KeyC" || !e.altKey) return null;
  if (isMac ? !e.metaKey || e.ctrlKey : !e.ctrlKey || e.metaKey) return null;
  return e.shiftKey ? "file" : "selection";
}

type ElementLike = { tagName: string; isContentEditable?: boolean; closest?: (sel: string) => unknown };

/**
 * True when keys belong to what has focus: a field, an editable region (Monaco), a menu, or the
 * Not captured panel (`data-audit-panel`), whose rows have no baseline: the review keys (a, r, ...)
 * must never act on the selected pending row from there.
 */
export function isTypingTarget(el: ElementLike | null | undefined): boolean {
  // A key sent to the window or the document has no tag: nothing is being typed into.
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (el.isContentEditable) return true;
  return !!el.closest?.('[role="menu"], [role="menubar"], [role="listbox"], [data-audit-panel]');
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
  /** A narrow window shows the list or the diff; a file is never decided unseen. */
  diffHidden?: boolean;
  /** Why decisions are off when `readOnly` is set for a reason other than the switch. */
  lockReason?: string;
};

/** What a/r say while read-only mode is on. */
export const READ_ONLY_REFUSAL = "Read-only mode is on";

/** What a/r say in a narrow window while the list, not the file's diff, is on screen. */
export const DIFF_HIDDEN_REFUSAL = "Open the file to see its diff before deciding";

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
  if (c.tab !== "pending" || c.typing || c.menuOpen || c.busy) return null;
  if (c.readOnly) return { refused: c.lockReason ?? READ_ONLY_REFUSAL };
  if (!c.selectedKey) return null;
  if (c.diffHidden) return { refused: DIFF_HIDDEN_REFUSAL };
  if (c.selectedWaiting) return { refused: WAITING_REFUSAL };
  return { action, key: c.selectedKey };
}

export type DiffKey = { type: "change"; dir: 1 | -1 } | { type: "wrap" };

type DiffKeyLike = { code: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean };

/**
 * VS Code's diff keys: Alt+F5 / Shift+Alt+F5 go to the next / previous change, Alt+Z toggles word
 * wrap. `code`, not `key`: Alt changes the character on macOS.
 */
export function diffKeyFor(e: DiffKeyLike): DiffKey | null {
  if (!e.altKey || e.ctrlKey || e.metaKey) return null;
  if (e.code === "F5") return { type: "change", dir: e.shiftKey ? -1 : 1 };
  if (e.code === "KeyZ" && !e.shiftKey) return { type: "wrap" };
  return null;
}

type UndoKeyLike = { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean };

/**
 * Ctrl+Z (Cmd+Z on macOS): the result bar's Undo. `key`, not `code`, so it follows the layout's Z
 * like every other app's undo. Shift (redo) and Alt (word wrap) are not it.
 */
export function isUndoKey(e: UndoKeyLike, isMac: boolean): boolean {
  if (e.key.toLowerCase() !== "z" || e.altKey || e.shiftKey) return false;
  return isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
}

type ViewKeyLike = { key: string; code?: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean };

/** The header switch's shortcut, as the overlay and tooltips spell it. */
export const viewKeyLabel = (isMac: boolean) => (isMac ? "⇧⌘G" : "Ctrl+Shift+G");

/**
 * Ctrl+Shift+G (Shift+Cmd+G on macOS): switches between Review and Git log, VS Code's key for its
 * Source Control view. No review key uses Ctrl or Cmd with Shift, so it cannot clash with one.
 * `code` when present, so a layout without a G key on that position still has it.
 */
export function isViewKey(e: ViewKeyLike, isMac: boolean): boolean {
  if (!e.shiftKey || e.altKey) return false;
  if (isMac ? !e.metaKey || e.ctrlKey : !e.ctrlKey || e.metaKey) return false;
  return e.code ? e.code === "KeyG" : e.key.toLowerCase() === "g";
}
