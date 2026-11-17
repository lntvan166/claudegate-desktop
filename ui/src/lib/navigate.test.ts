import { describe, expect, it } from "vitest";
import { modifiedPositionFor, type LineChange } from "./navigate";

// Monaco's ILineChange: an end of 0 means "no lines on that side" (a pure insertion or deletion),
// and the start then names the line the change sits after.
const ch = (os: number, oe: number, ms: number, me: number): LineChange => ({
  originalStartLineNumber: os,
  originalEndLineNumber: oe,
  modifiedStartLineNumber: ms,
  modifiedEndLineNumber: me,
});

describe("modifiedPositionFor (UX wave 9)", () => {
  it("keeps line and column with no changes", () => {
    expect(modifiedPositionFor(7, 3, [])).toEqual({ line: 7, col: 3, exact: true });
  });

  it("shifts an unchanged line by the lines inserted and removed above it", () => {
    // 3 lines inserted after line 2, then lines 10-11 replaced by one line (12 on the new side).
    const changes = [ch(2, 0, 3, 5), ch(10, 11, 13, 13)];
    expect(modifiedPositionFor(1, 4, changes)).toEqual({ line: 1, col: 4, exact: true });
    expect(modifiedPositionFor(2, 4, changes)).toEqual({ line: 2, col: 4, exact: true });
    expect(modifiedPositionFor(5, 2, changes)).toEqual({ line: 8, col: 2, exact: true });
    expect(modifiedPositionFor(12, 6, changes)).toEqual({ line: 14, col: 6, exact: true });
  });

  it("maps a changed line to the start of its new text, at column 1", () => {
    expect(modifiedPositionFor(11, 9, [ch(10, 11, 13, 14)])).toEqual({ line: 13, col: 1, exact: false });
  });

  it("maps a deleted line to the nearest unchanged line, the one above the cut", () => {
    // Lines 4-6 deleted; on the new side the cut sits after line 3.
    expect(modifiedPositionFor(5, 2, [ch(4, 6, 3, 0)])).toEqual({ line: 3, col: 1, exact: false });
    // Deleted at the very top: line 1.
    expect(modifiedPositionFor(1, 2, [ch(1, 2, 0, 0)])).toEqual({ line: 1, col: 1, exact: false });
  });

  it("an insertion right after a line does not move that line", () => {
    expect(modifiedPositionFor(4, 1, [ch(4, 0, 5, 9)])).toEqual({ line: 4, col: 1, exact: true });
    expect(modifiedPositionFor(5, 1, [ch(4, 0, 5, 9)])).toEqual({ line: 10, col: 1, exact: true });
  });
});
