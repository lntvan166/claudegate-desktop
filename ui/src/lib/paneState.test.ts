import { describe, expect, it } from "vitest";
import { paneState } from "./paneState";

const text = { kind: "text" as const, text: "x" };
const bin = { kind: "binary" as const };
const missing = { kind: "missing" as const };
const row = { key: "a" };

describe("paneState", () => {
  it("is empty with no selection, whatever else is left over", () => {
    expect(paneState(null, null, false, null)).toBe("empty");
    // The bug: a payload from the cleared row kept the editor up under the empty state.
    expect(paneState(null, { before: text, after: text }, false, null)).toBe("empty");
    expect(paneState(null, { before: text, after: text }, true, "boom")).toBe("empty");
  });

  it("is loading for a selected row with nothing fetched yet", () => {
    expect(paneState(row, null, true, null)).toBe("loading");
    expect(paneState(row, null, false, null)).toBe("loading");
  });

  it("keeps the editor while the same row re-fetches, never flashing empty", () => {
    expect(paneState(row, { before: text, after: text }, true, null)).toBe("editor");
  });

  it("shows the error over a stale payload", () => {
    expect(paneState(row, null, false, "nope")).toBe("error");
    expect(paneState(row, { before: text, after: text }, false, "nope")).toBe("error");
  });

  it("is binary when either side is binary", () => {
    expect(paneState(row, { before: bin, after: text }, false, null)).toBe("binary");
    expect(paneState(row, { before: missing, after: bin }, false, null)).toBe("binary");
  });

  it("is the editor for text, including a new file", () => {
    expect(paneState(row, { before: missing, after: text }, false, null)).toBe("editor");
  });
});
