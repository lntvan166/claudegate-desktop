import { describe, expect, it } from "vitest";
import { CHROME, DEFAULTS, MIN, RAIL, STACK_MAX, fit, keyStep, layout, load, loadCollapsed, maxFor, resize, reset, save, toggleCollapsed, withWidth } from "./panes";

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

describe("collapsed projects column", () => {
  const w = { projects: 300, tree: 400 };

  it("takes only the rail's width, and gives the rest to the tree and diff", () => {
    expect(layout(w, WIDE, true)).toEqual({ projects: RAIL, tree: 400 });
    // At 900 the expanded tree is squeezed to fit; collapsed, it gets the projects' width back.
    expect(layout(w, 900, false)).toEqual(fit(w, 900));
    expect(fit(w, 900).tree).toBeLessThan(400);
    expect(layout(w, 900, true).tree).toBe(400);
    expect(layout({ projects: 300, tree: 9999 }, 900, true).tree).toBe(900 - CHROME - MIN.diff - RAIL);
  });

  it("lets the tree grow into the freed width", () => {
    expect(maxFor(layout(w, 1200, true), "tree", 1200)).toBe(1200 - CHROME - MIN.diff - RAIL);
  });

  it("toggles, and expanding restores the stored projects width", () => {
    expect(toggleCollapsed(false)).toBe(true);
    expect(toggleCollapsed(true)).toBe(false);
    const c = toggleCollapsed(false);
    expect(layout(w, WIDE, c).projects).toBe(RAIL);
    expect(layout(w, WIDE, toggleCollapsed(c))).toEqual({ projects: 300, tree: 400 });
  });

  it("resizing the tree while collapsed keeps the stored projects width", () => {
    const next = withWidth(w, "tree", 600, WIDE, true);
    expect(next).toEqual({ projects: 300, tree: 600 });
    expect(withWidth(w, "tree", 9999, 1200, true).tree).toBe(1200 - CHROME - MIN.diff - RAIL);
    expect(withWidth(w, "projects", 260, WIDE, false)).toEqual({ projects: 260, tree: 400 });
    expect(withWidth(w, "tree", 9999, 1200, false)).toEqual({ projects: 300, tree: 1200 - CHROME - MIN.diff - 300 });
  });

  it("persists alongside the widths", () => {
    expect(loadCollapsed(save(w, true))).toBe(true);
    expect(loadCollapsed(save(w, false))).toBe(false);
    expect(load(save(w, true))).toEqual(w);
    // Stored before this existed, or garbage: expanded.
    expect(loadCollapsed('{"projects":250,"tree":400}')).toBe(false);
    expect(loadCollapsed(null)).toBe(false);
    expect(loadCollapsed("nope")).toBe(false);
    expect(loadCollapsed('{"collapsed":"yes"}')).toBe(false);
  });
});

describe("stacked", () => {
  it("matches the CSS breakpoint: at or below 720px the diff pane is hidden", async () => {
    const { stacked, STACK_MAX } = await import("./panes");
    expect(STACK_MAX).toBe(720);
    expect(stacked(720)).toBe(true);
    expect(stacked(480)).toBe(true);
    expect(stacked(721)).toBe(false);
  });
});

describe("layout never pushes a pane past the sheet (UX wave 8)", () => {
  it("in every rail state and window width, the panes fit and the diff keeps its minimum once it can", () => {
    for (const collapsed of [false, true]) {
      for (let total = STACK_MAX + 1; total <= 2600; total += 7) {
        for (const tree of [1, 240, 372, 900, 5000]) {
          for (const projects of [1, 228, 600, 5000]) {
            const w = layout({ projects, tree }, total, collapsed);
            const diff = total - CHROME - w.projects - w.tree;
            expect(w.tree).toBeGreaterThanOrEqual(MIN.tree);
            expect(w.projects).toBeGreaterThanOrEqual(collapsed ? RAIL : MIN.projects);
            if (total >= CHROME + MIN.projects + MIN.tree + MIN.diff) expect(diff).toBeGreaterThanOrEqual(MIN.diff);
            expect(diff).toBeGreaterThan(0);
            // A drag to the splitter's own max lands inside the same bounds.
            const max = maxFor(w, "tree", total);
            const after = layout(withWidth({ projects, tree }, "tree", max + 500, total, collapsed), total, collapsed);
            expect(total - CHROME - after.projects - after.tree).toBeGreaterThanOrEqual(Math.min(MIN.diff, diff));
          }
        }
      }
    }
  });
});
