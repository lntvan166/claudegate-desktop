import { describe, expect, it } from "vitest";
import {
  actionableCount,
  folderPrefix,
  nextAfterDecision,
  parseDecideError,
  recordIdFor,
  summarize,
  undoFor,
  type DecideReport,
} from "./decide";

const ok = (done: number, left: string[] = [], errors: [string, string][] = []): DecideReport => ({ done, left, errors });
const file = (key: string, scope = "/w") => ({ kind: "file" as const, scope, key });

describe("undoFor", () => {
  it("undoes an accept by reverting the new record, found by path", () => {
    expect(undoFor("accept", file("/w/a.ts"), ok(1))).toEqual({
      action: "revert",
      target: { kind: "file", scope: "/w", key: "/w/a.ts" },
      byPath: true,
    });
  });

  it("undoes a reject by reapplying the same path in the same scope", () => {
    expect(undoFor("reject", file("/w/a.ts", "/w/.worktrees/x"), ok(1))).toEqual({
      action: "reapply",
      target: { kind: "file", scope: "/w/.worktrees/x", key: "/w/a.ts" },
      byPath: false,
    });
  });

  it("a reject that left the file on disk can still be undone", () => {
    expect(undoFor("reject", file("/w/a.ts"), ok(1, ["/w/a.ts"]))?.action).toBe("reapply");
  });

  it("revert, reapply and clear have no undo", () => {
    expect(undoFor("revert", file("id"), ok(1))).toBeNull();
    expect(undoFor("reapply", file("/w/a.ts"), ok(1))).toBeNull();
    expect(undoFor("clearAccepted", { kind: "all" }, ok(3))).toBeNull();
    expect(undoFor("clearRejected", { kind: "all" }, ok(3))).toBeNull();
  });

  it("bulk and folder decisions have no undo", () => {
    expect(undoFor("accept", { kind: "all" }, ok(4))).toBeNull();
    expect(undoFor("reject", { kind: "folder", scope: "/w", prefix: "/w/src" }, ok(2))).toBeNull();
  });

  it("nothing decided, or a failure, has no undo", () => {
    expect(undoFor("accept", file("/w/a.ts"), ok(0))).toBeNull();
    expect(undoFor("reject", file("/w/a.ts"), ok(0, [], [["/w/a.ts", "EACCES"]]))).toBeNull();
  });
});

describe("recordIdFor", () => {
  const rec = (path: string, decidedAt: string, scope = "/w") => ({ id: `${decidedAt}::${path}`, path, scope, decidedAt });
  it("picks the newest record for the path in the scope", () => {
    const list = [
      rec("/w/a.ts", "2026-11-16T10:00:00.000Z"),
      rec("/w/a.ts", "2026-11-16T12:00:00.000Z"),
      rec("/w/a.ts", "2026-11-16T13:00:00.000Z", "/other"),
      rec("/w/b.ts", "2026-11-16T14:00:00.000Z"),
    ];
    expect(recordIdFor(list, "/w", "/w/a.ts")).toBe("2026-11-16T12:00:00.000Z::/w/a.ts");
  });
  it("is null when there is none", () => {
    expect(recordIdFor([], "/w", "/w/a.ts")).toBeNull();
  });
});

describe("nextAfterDecision", () => {
  const rows = [{ key: "a" }, { key: "b" }, { key: "c" }];
  it("picks the next row", () => {
    expect(nextAfterDecision(rows, "a")).toBe("b");
    expect(nextAfterDecision(rows, "b")).toBe("c");
  });
  it("falls back to the previous row at the end", () => {
    expect(nextAfterDecision(rows, "c")).toBe("b");
  });
  it("is null for a single row", () => {
    expect(nextAfterDecision([{ key: "a" }], "a")).toBeNull();
  });
  it("is null when the key is not listed", () => {
    expect(nextAfterDecision(rows, "z")).toBeNull();
  });
});

describe("actionableCount", () => {
  it("counts only rows whose write has landed", () => {
    expect(actionableCount([{ waiting: false }, { waiting: true }, { waiting: false }])).toBe(2);
  });
});

describe("folderPrefix", () => {
  it("joins the scope root and the folder's rel", () => {
    expect(folderPrefix("/w", "src/api")).toBe("/w/src/api");
    expect(folderPrefix("/w/", "src")).toBe("/w/src");
    expect(folderPrefix("C:\\w", "src")).toBe("C:\\w/src");
  });
});

describe("parseDecideError", () => {
  it("reads a store error with its changed files and partial report", () => {
    const raw = JSON.stringify({ kind: "store", message: "disk full", changed: ["/w/a"], partial: { done: 1, left: [], errors: [] } });
    expect(parseDecideError(raw)).toEqual({ kind: "store", message: "disk full", changed: ["/w/a"], partial: { done: 1, left: [], errors: [] } });
  });
  it("reads the plain kinds", () => {
    for (const kind of ["readOnly", "notFound", "badTarget", "invalid"]) {
      expect(parseDecideError(JSON.stringify({ kind, message: "m" }))).toEqual({ kind, message: "m" });
    }
  });
  it("turns a non-JSON string into invalid with the raw text", () => {
    expect(parseDecideError("boom")).toEqual({ kind: "invalid", message: "boom" });
  });
  it("turns JSON of the wrong shape, and non-strings, into invalid", () => {
    expect(parseDecideError('{"kind":"weird","message":"x"}')).toEqual({ kind: "invalid", message: '{"kind":"weird","message":"x"}' });
    expect(parseDecideError("[1]").kind).toBe("invalid");
    expect(parseDecideError(new Error("e"))).toEqual({ kind: "invalid", message: "Error: e" });
  });
});

describe("summarize", () => {
  it("names a single file", () => {
    expect(summarize("accept", file("/w/src/client.ts"), ok(1))).toEqual({ text: "Accepted client.ts", failed: false });
    expect(summarize("reject", file("/w/src/client.ts"), ok(1))).toEqual({ text: "Rejected client.ts", failed: false });
    expect(summarize("reapply", file("/w/src/client.ts"), ok(1))).toEqual({ text: "Reapplied client.ts", failed: false });
  });
  it("names a reverted record by its path, not its id", () => {
    expect(summarize("revert", file("2026-11-16T10:00:00.000Z::/w/src/client.ts"), ok(1)).text).toBe("Reverted client.ts");
  });
  it("counts a bulk decision, with what was left and what failed", () => {
    expect(summarize("accept", { kind: "all" }, ok(12)).text).toBe("Accepted 12 files");
    expect(summarize("accept", { kind: "all" }, ok(1)).text).toBe("Accepted 1 file");
    expect(summarize("reject", { kind: "all" }, ok(12, ["a", "b"], [["c", "x"]]))).toEqual({
      text: "Rejected 12 · 2 left on disk · 1 failed",
      failed: true,
    });
    expect(summarize("clearAccepted", { kind: "all" }, ok(3)).text).toBe("Cleared 3 records");
  });
  it("a single file that failed says so", () => {
    expect(summarize("reject", file("/w/a.ts"), ok(0, [], [["/w/a.ts", "EACCES"]]))).toEqual({
      text: "Couldn't reject a.ts",
      failed: true,
    });
  });
});
