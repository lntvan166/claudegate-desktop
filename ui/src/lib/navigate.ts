// Ctrl/Cmd+click and F12 open the file at a position. The file on disk is the modified side; a
// position on the original side is carried across with Monaco's line changes.

/** Monaco's ILineChange (line numbers only). An end of 0 means no lines on that side. */
export type LineChange = {
  originalStartLineNumber: number;
  originalEndLineNumber: number;
  modifiedStartLineNumber: number;
  modifiedEndLineNumber: number;
};

const span = (start: number, end: number) => (end === 0 ? 0 : end - start + 1);

/**
 * Where original-side `line`:`col` is on the modified side. An unchanged line moves by the lines
 * inserted and removed above it and keeps its column (`exact`). A changed line goes to the start of
 * its new text; a deleted one to the line above the cut (the nearest unchanged line); both at
 * column 1.
 */
export function modifiedPositionFor(
  line: number,
  col: number,
  changes: LineChange[],
): { line: number; col: number; exact: boolean } {
  let delta = 0;
  for (const c of changes) {
    const os = c.originalStartLineNumber;
    const oe = c.originalEndLineNumber;
    if (oe !== 0 && line >= os && line <= oe) {
      // For a pure deletion Monaco's modified start is already the line above the cut.
      return { line: Math.max(1, c.modifiedStartLineNumber), col: 1, exact: false };
    }
    // Earlier: a replaced/deleted range ending above the line, or an insertion after a line above it.
    const earlier = oe === 0 ? os < line : oe < line;
    if (earlier) delta += span(c.modifiedStartLineNumber, c.modifiedEndLineNumber) - span(os, oe);
  }
  return { line: Math.max(1, line + delta), col, exact: true };
}
