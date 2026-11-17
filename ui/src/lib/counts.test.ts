import { describe, expect, it } from "vitest";
import { bulkLabel, headerTotal, projectLine } from "./counts";

const p = (pending: number) => ({ pending });

describe("headerTotal (UX wave 5)", () => {
  it("says how many pending in how many projects", () => {
    expect(headerTotal([p(13), p(2), p(0)])).toBe("15 pending in 2 projects");
    expect(headerTotal([p(1), p(0)])).toBe("1 pending in 1 project");
  });
  it("says nothing is pending", () => {
    expect(headerTotal([p(0)])).toBe("Nothing pending");
    expect(headerTotal([])).toBe("Nothing pending");
  });
});

describe("bulkLabel (UX wave 5)", () => {
  it("counts files when the bulk set is the whole tab", () => {
    expect(bulkLabel(12, 12, "pending")).toBe("12 files");
    expect(bulkLabel(1, 1, "pending")).toBe("1 file");
    expect(bulkLabel(3, 3, "accepted")).toBe("3 records");
  });
  it("reads 'to review' when it differs from the tab count (waiting rows, rows outside the root)", () => {
    expect(bulkLabel(12, 13, "pending")).toBe("12 to review");
  });
});

describe("projectLine (UX wave 5)", () => {
  const at = (min: number) => new Date(Date.UTC(2026, 10, 17, 12, 0) - min * 60_000).toISOString();
  const now = Date.UTC(2026, 10, 17, 12, 0);
  const item = (scope: string, waiting = false, capturedAt: string | null = at(30)) => ({ scope, waiting, capturedAt });

  it("leads with pending and the files not written yet, never 'waiting'", () => {
    const pending = [...Array.from({ length: 12 }, () => item("/w")), item("/w", true, at(0))];
    const l = projectLine(pending, [{ root: "/w", label: "w" }], now);
    expect(l.text).toBe("13 pending · 1 not written yet · oldest 30m ago");
    expect(l.text).not.toContain("waiting");
  });

  it("keeps the per-worktree breakdown out of the line, in its full form for a tooltip", () => {
    const pending = [item("/w"), item("/w"), item("/w/.wt/a"), item("/w/.wt/b")];
    const scopes = [
      { root: "/w", label: "shop" },
      { root: "/w/.wt/a", label: ".wt/a" },
      { root: "/w/.wt/b", label: ".wt/b" },
    ];
    const l = projectLine(pending, scopes, now);
    expect(l.text).toBe("4 pending in 3 worktrees · oldest 30m ago");
    expect(l.detail).toBe("2 in shop, 1 in .wt/a, 1 in .wt/b");
  });

  it("says when nothing is pending", () => {
    expect(projectLine([], [{ root: "/w", label: "w" }], now)).toEqual({ text: "Nothing pending", detail: null });
  });
});
