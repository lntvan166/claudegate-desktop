import { describe, expect, it } from "vitest";
import { clipboardText, formatRef, selectionOnModified } from "./ref";

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
