import { describe, expect, it } from "vitest";
import { CHROME, DEFAULTS, MIN, fit, keyStep, load, resize, reset, save } from "./panes";

const WIDE = 1600;

describe("fit", () => {
  it("keeps widths that fit", () => {
    expect(fit({ projects: 300, tree: 500 }, WIDE)).toEqual({ projects: 300, tree: 500 });
  });

  it("clamps each pane to its minimum", () => {
    expect(fit({ projects: 20, tree: 10 }, WIDE)).toEqual({ projects: MIN.projects, tree: MIN.tree });
  });

  it("shrinks the tree, then the projects, when the window narrows", () => {
    // 900 leaves 890 inside the chrome; the diff needs 360.
    const w = fit({ projects: 228, tree: 372 }, 900);
    expect(w.projects + w.tree + MIN.diff + CHROME).toBeLessThanOrEqual(900);
    expect(w.projects).toBe(228);
    expect(w.tree).toBe(900 - CHROME - MIN.diff - 228);
    const narrow = fit({ projects: 400, tree: 600 }, 800);
    expect(narrow.tree).toBe(MIN.tree);
    expect(narrow.projects).toBe(800 - CHROME - MIN.diff - MIN.tree);
  });

  it("fits the three minimums at the window's minimum width", () => {
    const w = fit({ projects: 9999, tree: 9999 }, 900);
    expect(w.projects + w.tree + MIN.diff + CHROME).toBeLessThanOrEqual(900);
    expect(w.projects).toBeGreaterThanOrEqual(MIN.projects);
    expect(w.tree).toBeGreaterThanOrEqual(MIN.tree);
  });
});

describe("resize and reset", () => {
  it("moves one pane and clamps it", () => {
    expect(resize({ projects: 228, tree: 372 }, "tree", 100, WIDE).tree).toBe(472);
    expect(resize({ projects: 228, tree: 372 }, "tree", -1000, WIDE).tree).toBe(MIN.tree);
    expect(resize({ projects: 228, tree: 372 }, "tree", 5000, 1200).tree).toBe(1200 - CHROME - MIN.diff - 228);
  });

  it("resets one pane to its default and leaves the other", () => {
    expect(reset({ projects: 300, tree: 600 }, "tree", WIDE)).toEqual({ projects: 300, tree: DEFAULTS.tree });
    expect(reset({ projects: 300, tree: 600 }, "projects", WIDE)).toEqual({ projects: DEFAULTS.projects, tree: 600 });
  });
});

describe("keyStep", () => {
  it("steps 16px on arrows and 64px with Shift", () => {
    expect(keyStep("ArrowRight", false)).toBe(16);
    expect(keyStep("ArrowLeft", false)).toBe(-16);
    expect(keyStep("ArrowRight", true)).toBe(64);
    expect(keyStep("ArrowLeft", true)).toBe(-64);
    expect(keyStep("ArrowUp", false)).toBeNull();
    expect(keyStep("a", false)).toBeNull();
  });
});

describe("load and save", () => {
  it("round-trips", () => {
    expect(load(save({ projects: 250, tree: 400 }))).toEqual({ projects: 250, tree: 400 });
  });

  it("falls back to the defaults on anything malformed", () => {
    for (const raw of [null, "", "{", "null", "42", "[]", '{"projects":"wide","tree":400}', '{"projects":250}', '{"projects":NaN}', '{"projects":-5,"tree":1e999}']) {
      expect(load(raw)).toEqual(DEFAULTS);
    }
  });
});
