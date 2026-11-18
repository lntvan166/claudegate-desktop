import { describe, expect, it } from "vitest";
import { chooseSelection, clipboardText, formatRef, pasteHint, refRange, selectionOnModified } from "./ref";

const ROOT = "/home/dev/shop";

describe("formatRef", () => {
  it("is relative to the project root by default", () => {
    expect(formatRef(`${ROOT}/src/app.ts`, ROOT, "relative")).toBe("@src/app.ts");
  });
  it("is absolute when the style says so", () => {
    expect(formatRef(`${ROOT}/src/app.ts`, ROOT, "absolute")).toBe(`@${ROOT}/src/app.ts`);
  });
  it("stays absolute for a file outside the root", () => {
    expect(formatRef("/etc/hosts", ROOT, "relative")).toBe("@/etc/hosts");
    expect(formatRef("/home/dev/shop-two/a.ts", ROOT, "relative")).toBe("@/home/dev/shop-two/a.ts");
  });
  it("tolerates a trailing slash on the root", () => {
    expect(formatRef(`${ROOT}/a.ts`, `${ROOT}/`, "relative")).toBe("@a.ts");
  });
  it("adds :start-end for a selection", () => {
    expect(formatRef(`${ROOT}/src/app.ts`, ROOT, "relative", { start: 12, end: 20 })).toBe("@src/app.ts:12-20");
  });
  it("keeps a single line as :N-N, as ctx-push does", () => {
    expect(formatRef(`${ROOT}/src/app.ts`, ROOT, "relative", { start: 7, end: 7 })).toBe("@src/app.ts:7-7");
  });
  it("puts a worktree file relative to the primary root", () => {
    expect(formatRef(`${ROOT}/.claude/worktrees/x/a.ts`, ROOT, "relative")).toBe("@.claude/worktrees/x/a.ts");
  });
});

describe("clipboardText", () => {
  it("ends with one space so the paste can be followed by typing", () => {
    expect(clipboardText("@src/app.ts")).toBe("@src/app.ts ");
  });
});

describe("selectionOnModified", () => {
  it("passes a modified-side selection through", () => {
    expect(selectionOnModified("modified", { start: 3, end: 5 }, [])).toEqual({ start: 3, end: 5 });
  });
  it("carries an original-side selection across the changes", () => {
    // Two lines inserted at the top: original line 1-2 are modified lines 3-4.
    const changes = [{ originalStartLineNumber: 0, originalEndLineNumber: 0, modifiedStartLineNumber: 1, modifiedEndLineNumber: 2 }];
    expect(selectionOnModified("original", { start: 1, end: 2 }, changes)).toEqual({ start: 3, end: 4 });
  });
  it("falls back to the nearest line for a deleted range and never inverts", () => {
    // Original lines 5-6 were deleted; modified start is the line above the cut.
    const changes = [{ originalStartLineNumber: 5, originalEndLineNumber: 6, modifiedStartLineNumber: 4, modifiedEndLineNumber: 0 }];
    expect(selectionOnModified("original", { start: 5, end: 6 }, changes)).toEqual({ start: 4, end: 4 });
  });
});

describe("chooseSelection", () => {
  const stale = [{ range: { start: 12, end: 20 }, focused: false }];
  it("keyboard with focus in the tree ignores a stale diff selection: the whole file", () => {
    const sel = chooseSelection(stale, true);
    expect(sel).toBeNull();
    expect(formatRef(`${ROOT}/src/app.ts`, ROOT, "relative", sel)).toBe("@src/app.ts");
  });
  it("keyboard with the editor focused takes its selection", () => {
    expect(chooseSelection([{ range: { start: 3, end: 4 }, focused: true }], true)).toEqual({ start: 3, end: 4 });
  });
  it("the button and menu take a selection whatever has focus", () => {
    expect(chooseSelection(stale, false)).toEqual({ start: 12, end: 20 });
  });
  it("skips an empty side", () => {
    expect(chooseSelection([{ range: null, focused: true }, { range: { start: 1, end: 2 }, focused: true }], true)).toEqual({ start: 1, end: 2 });
  });
});

describe("refRange (M8)", () => {
  const sel = { start: 3, end: 7 };
  it("keeps a selection on the Pending tab, where the modified side is the file on disk", () => {
    expect(refRange("pending", sel)).toEqual(sel);
  });
  it("is always the whole file on Accepted and Rejected: those lines are not on disk", () => {
    expect(refRange("accepted", sel)).toBeNull();
    expect(refRange("rejected", sel)).toBeNull();
    expect(formatRef(`${ROOT}/a.ts`, ROOT, "relative", refRange("accepted", sel))).toBe("@a.ts");
  });
});

describe("pasteHint", () => {
  it("is Cmd+V on macOS and Ctrl+Shift+V elsewhere (terminals)", () => {
    expect(pasteHint(true)).toBe("Cmd+V");
    expect(pasteHint(false)).toBe("Ctrl+Shift+V");
  });
});
