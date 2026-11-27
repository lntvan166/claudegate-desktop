import { rowKey, type RowView } from "./log";

// The commit list's selection: one commit (a plain click, as always) or several (Ctrl/Cmd and
// Shift), for Combined Changes. Keys are `rowKey(row)` strings; `order` is the list's row order.
// A port of Polylog's selectionModel.

export const LIMIT_NOTICE = "Combined Changes shows up to 100 commits";

export const SELECTION_LIMIT = 100;

export type Selection = {
  /** Selected rows, in list order. */
  keys: string[];
  /** The row the keyboard is on (the active row). */
  focus: string | null;
  /** Where a Shift range starts: the last plain-clicked or Ctrl-clicked row. */
  anchor: string | null;
};

export type Gesture =
  | { kind: "click"; key: string; ctrl: boolean; shift: boolean }
  /** An arrow (or Page/Home/End) key moved to `key`. */
  | { kind: "arrow"; key: string; shift: boolean }
  | { kind: "context"; key: string };

export const single = (key: string): Selection => ({ keys: [key], focus: key, anchor: key });

function range(order: readonly string[], from: string, to: string): string[] {
  const a = order.indexOf(from);
  const b = order.indexOf(to);
  if (a < 0 || b < 0) return [to];
  return order.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/** The keys in list order, without duplicates. */
function inOrder(keys: Iterable<string>, order: readonly string[]): string[] {
  const set = new Set(keys);
  return order.filter((k) => set.has(k));
}

export function applyGesture(s: Selection, g: Gesture, order: readonly string[]): { next: Selection; overLimit: boolean } {
  let next: Selection;
  if (g.kind === "context") {
    next = s.keys.includes(g.key) ? s : single(g.key);
  } else if (g.shift) {
    const anchor = s.anchor ?? g.key;
    const span = range(order, anchor, g.key);
    const keys = g.kind === "click" && g.ctrl ? inOrder([...s.keys, ...span], order) : span;
    next = { keys, focus: g.key, anchor };
  } else if (g.kind === "click" && g.ctrl) {
    if (!s.keys.includes(g.key)) next = { keys: inOrder([...s.keys, g.key], order), focus: g.key, anchor: g.key };
    else if (s.keys.length === 1) next = s;
    else {
      const keys = s.keys.filter((k) => k !== g.key);
      const focus = s.focus === g.key ? keys[0] : s.focus;
      next = { keys, focus, anchor: s.anchor === g.key ? focus : s.anchor };
    }
  } else {
    next = single(g.key);
  }
  return next.keys.length > SELECTION_LIMIT ? { next: s, overLimit: true } : { next, overLimit: false };
}

/** After the list reloads: the selected commits still listed stay selected. */
export function keepListed(s: Selection, order: readonly string[]): Selection {
  const keys = inOrder(s.keys, order);
  if (keys.length === 0) return { keys: [], focus: null, anchor: null };
  const focus = s.focus !== null && keys.includes(s.focus) ? s.focus : keys[0];
  const anchor = s.anchor !== null && keys.includes(s.anchor) ? s.anchor : focus;
  return { keys, focus, anchor };
}

export const EMPTY_SELECTION: Selection = { keys: [], focus: null, anchor: null };

/** "ctrl" is Cmd on macOS and Ctrl elsewhere; with `multi` off every click is a plain click. */
export function clickGesture(e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, key: string, mac: boolean, multi: boolean): Gesture {
  if (!multi) return { kind: "click", key, ctrl: false, shift: false };
  return { kind: "click", key, ctrl: mac ? e.metaKey : e.ctrlKey, shift: e.shiftKey };
}

/** The selected rows, in list order. */
export function pickedRows(rows: RowView[], s: Selection): RowView[] {
  const set = new Set(s.keys);
  return rows.filter((r) => set.has(rowKey(r)));
}
