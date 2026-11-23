import { describe, expect, it } from "vitest";
import { COLLAPSED_MAX, loadCollapsedKeys, loadMode, loadWrap, saveCollapsedKeys } from "./uiState";

describe("loadMode", () => {
  it("restores list", () => expect(loadMode("list")).toBe("list"));
  it("defaults to tree", () => {
    expect(loadMode(null)).toBe("tree");
    expect(loadMode("grid")).toBe("tree");
  });
});

describe("collapsed keys", () => {
  it("round-trips", () => {
    const keys = ["pending|/a|src", "pending|group|/a/.worktrees/x"];
    expect(loadCollapsedKeys(saveCollapsedKeys(new Set(keys)))).toEqual(keys);
  });

  it("ignores junk", () => {
    expect(loadCollapsedKeys(null)).toEqual([]);
    expect(loadCollapsedKeys("{")).toEqual([]);
    expect(loadCollapsedKeys('{"a":1}')).toEqual([]);
    expect(loadCollapsedKeys('["a", 3, null, "b"]')).toEqual(["a", "b"]);
  });

  it("keeps only the newest", () => {
    const many = Array.from({ length: COLLAPSED_MAX + 10 }, (_, i) => `k${i}`);
    const kept = loadCollapsedKeys(saveCollapsedKeys(many));
    expect(kept).toHaveLength(COLLAPSED_MAX);
    expect(kept[0]).toBe("k10");
    expect(kept.at(-1)).toBe(`k${COLLAPSED_MAX + 9}`);
  });
});

describe("loadWrap", () => {
  it("is off unless stored on", () => {
    expect(loadWrap(null)).toBe(false);
    expect(loadWrap("off")).toBe(false);
    expect(loadWrap("on")).toBe(true);
  });
});
