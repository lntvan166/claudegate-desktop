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
  }
  const tab = TABS[e.key];
  return tab ? { type: "tab", tab } : null;
}

type ElementLike = { tagName: string; isContentEditable?: boolean; closest?: (sel: string) => unknown };

/** True when keys belong to what has focus: a field, an editable region (Monaco), or a menu. */
export function isTypingTarget(el: ElementLike | null | undefined): boolean {
  if (!el) return false;
  const tag = el.tagName.toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (el.isContentEditable) return true;
  return !!el.closest?.('[role="menu"], [role="menubar"], [role="listbox"]');
}
