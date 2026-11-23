// "Copy reference for Claude": the `@path` / `@path:start-end` text Claude Code reads as a file
// or line-range reference, the same shape the maintainer's ctx-push extension copies.
import { modifiedPositionFor, type LineChange } from "./navigate";

export type RefPathStyle = "relative" | "absolute";
export type LineRange = { start: number; end: number };

const trimSlash = (p: string) => p.replace(/\/+$/, "");

/**
 * `@path`, or `@path:start-end` (1-based) for a selection. The path is relative to `root` under
 * the "relative" style; a file outside the root stays absolute. A single line is `:N-N`.
 */
export function formatRef(absPath: string, root: string, style: RefPathStyle, sel?: LineRange | null): string {
  const base = trimSlash(root);
  const path = style === "relative" && base && absPath.startsWith(`${base}/`) ? absPath.slice(base.length + 1) : absPath;
  return sel ? `@${path}:${sel.start}-${sel.end}` : `@${path}`;
}

/** What goes on the clipboard: the reference and a trailing space, so typing can follow the paste. */
export const clipboardText = (ref: string) => `${ref} `;

/**
 * A selection on either side of the diff, as lines of the file on disk (the modified side). An
 * original-side line is carried across with Monaco's line changes; a deleted line lands on the
 * nearest unchanged one above the cut. Never inverted.
 */
export function selectionOnModified(side: "original" | "modified", sel: LineRange, changes: LineChange[]): LineRange {
  if (side === "modified") return sel;
  const start = modifiedPositionFor(sel.start, 1, changes).line;
  const end = modifiedPositionFor(sel.end, 1, changes).line;
  return { start, end: Math.max(start, end) };
}

/**
 * Which selection a copy takes, from the candidates in preference order. The keyboard
 * (`focusedOnly`) counts only an editor that has focus, so a selection left in the diff while
 * focus is in the file tree is ignored and the whole file is copied; the header button and the
 * menu take any selection.
 */
export function chooseSelection(cands: { range: LineRange | null; focused: boolean }[], focusedOnly: boolean): LineRange | null {
  for (const c of cands) if (c.range && (!focusedOnly || c.focused)) return c.range;
  return null;
}

/**
 * The lines a copy carries: the selection when there is one, else the line of a cursor the user
 * placed (a one-line range, `@path:N-N`, carried from the original side like Open is), else null
 * for the whole file.
 */
export function refLines(
  sel: LineRange | null,
  cursor: { side: "original" | "modified"; line: number } | null,
  changes: LineChange[],
): LineRange | null {
  if (sel) return sel;
  if (!cursor) return null;
  return selectionOnModified(cursor.side, { start: cursor.line, end: cursor.line }, changes);
}

/**
 * The line range a copy may carry on `tab`. Only Pending: its modified side is the file on disk.
 * On Accepted and Rejected the lines would come from text that is not on disk, so the reference
 * is always the whole file.
 */
export function refRange(tab: "pending" | "accepted" | "rejected", sel: LineRange | null | undefined): LineRange | null {
  return tab === "pending" ? (sel ?? null) : null;
}

/** The paste shortcut named after a copy: Cmd+V on macOS, Ctrl+Shift+V (terminals) elsewhere. */
export const pasteHint = (isMac: boolean) => (isMac ? "Cmd+V" : "Ctrl+Shift+V");
