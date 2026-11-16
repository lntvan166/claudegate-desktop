import { describe, expect, it } from "vitest";
import {
  folderPrefix,
  isSettingsUnreadable,
  nextAfterDecision,
  parseDecideError,
  recordIdFor,
  SETTINGS_UNREADABLE_MESSAGE,
  bulkKeys,
  bulkPrompt,
  folderRejectPrompt,
  storeFailure,
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
  const since = "2026-11-16T11:00:00.000Z";
  it("picks the record this decision made: same scope and path, decided at or after it started", () => {
    const list = [
      rec("/w/a.ts", "2026-11-16T10:00:00.000Z"),
      rec("/w/a.ts", "2026-11-16T12:00:00.000Z"),
      rec("/w/a.ts", "2026-11-16T13:00:00.000Z", "/other"),
      rec("/w/b.ts", "2026-11-16T14:00:00.000Z"),
    ];
    expect(recordIdFor(list, "/w", "/w/a.ts", since)).toBe("2026-11-16T12:00:00.000Z::/w/a.ts");
  });
  it("never falls back to an older accept of the same path", () => {
    const list = [rec("/w/a.ts", "2026-11-16T10:00:00.000Z"), rec("/w/a.ts", "2026-11-16T10:30:00.000Z")];
    expect(recordIdFor(list, "/w", "/w/a.ts", since)).toBeNull();
  });
  it("counts a record stamped at the very start", () => {
    expect(recordIdFor([rec("/w/a.ts", since)], "/w", "/w/a.ts", since)).toBe(`${since}::/w/a.ts`);
  });
  it("is null when there is none", () => {
    expect(recordIdFor([], "/w", "/w/a.ts", since)).toBeNull();
  });
});

describe("isSettingsUnreadable", () => {
  it("is true only for the settings-file refusal", () => {
    expect(isSettingsUnreadable({ kind: "readOnly", message: SETTINGS_UNREADABLE_MESSAGE })).toBe(true);
    expect(isSettingsUnreadable({ kind: "readOnly", message: "ClaudeGate Desktop is in read-only mode. Turn it off to make decisions." })).toBe(false);
    expect(isSettingsUnreadable({ kind: "readOnly", message: "cannot read session file: permission denied" })).toBe(false);
    expect(isSettingsUnreadable({ kind: "invalid", message: SETTINGS_UNREADABLE_MESSAGE })).toBe(false);
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
    expect(parseDecideError(raw)).toEqual({
      kind: "store",
      message: "disk full",
      changed: ["/w/a"],
      partial: { done: 1, left: [], errors: [] },
      snapshot: null,
    });
    const withSnap = JSON.stringify({ kind: "store", message: "m", changed: ["/w/a"], partial: null, snapshot: "/d/files/s" });
    expect(parseDecideError(withSnap).snapshot).toBe("/d/files/s");
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
    expect(summarize("accept", file("/w/src/client.ts"), ok(1))).toEqual({ text: "Accepted client.ts", tone: "ok" });
    expect(summarize("reject", file("/w/src/client.ts"), ok(1))).toEqual({ text: "Rejected client.ts", tone: "ok" });
    expect(summarize("reapply", file("/w/src/client.ts"), ok(1))).toEqual({ text: "Reapplied client.ts", tone: "ok" });
  });
  it("names a reverted record by its path, not its id", () => {
    expect(summarize("revert", file("2026-11-16T10:00:00.000Z::/w/src/client.ts"), ok(1)).text).toBe("Reverted client.ts");
  });
  it("a single reject that left the file on disk says so, as a warning", () => {
    expect(summarize("reject", file("/w/scripts/seed.py"), ok(1, ["/w/scripts/seed.py"]))).toEqual({
      text: "Rejected seed.py · left on disk (no original to restore)",
      tone: "warn",
    });
  });
  it("counts a bulk decision, with what was left and what failed", () => {
    expect(summarize("accept", { kind: "all" }, ok(12)).text).toBe("Accepted 12 files");
    expect(summarize("accept", { kind: "all" }, ok(1)).text).toBe("Accepted 1 file");
    expect(summarize("reject", { kind: "all" }, ok(12, ["a", "b"], [["c", "x"]]))).toEqual({
      text: "Rejected 12 · 2 left on disk · 1 failed",
      tone: "warn",
    });
    expect(summarize("reject", { kind: "all" }, ok(3, ["a"])).tone).toBe("warn");
    expect(summarize("clearAccepted", { kind: "all" }, ok(3)).text).toBe("Cleared 3 records");
  });
  it("a single file that failed says so, as an error", () => {
    expect(summarize("reject", file("/w/a.ts"), ok(0, [], [["/w/a.ts", "EACCES"]]))).toEqual({
      text: "Couldn't reject a.ts",
      tone: "error",
    });
  });
});

describe("storeFailure", () => {
  const err = (changed: string[], snapshot: string | null = null) => ({
    kind: "store" as const,
    message: "could not write the session file",
    changed,
    partial: { done: 0, left: [], errors: [] },
    snapshot,
  });

  it("never offers Retry for a reject: the files are already restored", () => {
    expect(storeFailure("reject", err(["/w/a"], "/d/files/s")).retry).toBe(false);
    expect(storeFailure("reject", err([])).retry).toBe(false);
  });

  it("points a reject at the snapshot that holds Claude's version", () => {
    expect(storeFailure("reject", err(["/w/a"], "/d/files/s")).note).toBe("Claude's version was saved to /d/files/s");
  });

  it("says nothing changed when the preflight refused", () => {
    expect(storeFailure("reject", err([])).note).toBe("Nothing was changed.");
    expect(storeFailure("accept", err([]))).toEqual({ note: "Nothing was changed.", retry: true });
  });

  it("reapply names the snapshot of what it overwrote, and can be retried", () => {
    expect(storeFailure("reapply", err(["/w/a"], "/d/files/s"))).toEqual({
      note: "The files' previous contents were saved to /d/files/s",
      retry: true,
    });
  });

  it("other actions keep Retry", () => {
    expect(storeFailure("accept", err(["/w/a"])).retry).toBe(true);
  });
});

describe("bulkPrompt", () => {
  it("asks before Reject all", () => {
    expect(bulkPrompt("rejectAll", "pending", 3)).toEqual({
      text: "Reject 3 files? Originals are restored and files Claude created are deleted.",
      go: "Reject 3",
      danger: true,
    });
  });

  it("asks before Revert all, in the extension's words", () => {
    expect(bulkPrompt("revertAll", "accepted", 4)).toEqual({
      text: "Revert all 4 accepted file(s) back to pending review?",
      go: "Revert 4",
      danger: false,
    });
  });

  it("asks before Reapply all, in the extension's words", () => {
    expect(bulkPrompt("reapplyAll", "rejected", 2)).toEqual({
      text: "Re-apply Claude's version to all 2 rejected file(s) on disk?",
      go: "Reapply 2",
      danger: true,
    });
  });

  it("asks before Clear, naming the list", () => {
    expect(bulkPrompt("clear", "accepted", 1)?.text).toBe("Clear 1 record from Accepted? Files on disk stay as they are.");
    expect(bulkPrompt("clear", "rejected", 5)?.go).toBe("Clear 5");
  });

  it("Accept all needs no question: it changes no file", () => {
    expect(bulkPrompt("acceptAll", "pending", 3)).toBeNull();
  });
});

describe("folderRejectPrompt", () => {
  it("uses the extension's words and the folder's own name", () => {
    expect(folderRejectPrompt(3, "src/api")).toEqual({
      text: 'Reject 3 file(s) in "api"? This restores their original content.',
      go: "Reject 3",
      danger: true,
    });
  });
});

describe("bulkKeys", () => {
  const p = (path: string, scope = "/w", waiting = false) => ({ path, scope, waiting });
  const r = (id: string, path: string) => ({ id, path });
  const view = {
    pending: [p("/w/a.ts"), p("/w/b.ts", "/w", true), p("/tmp/out.log"), p("/w/.wt/x/c.ts", "/w/.wt/x"), p("/wx/d.ts")],
    accepted: [r("t1::/w/a.ts", "/w/a.ts"), r("t2::/w/e.ts", "/w/e.ts")],
    rejected: [r("t3::/w/f.ts", "/w/f.ts")],
  };

  it("pending: rows whose write landed, inside their scope root (what the backend acts on)", () => {
    expect(bulkKeys(view, "pending")).toEqual(["/w/a.ts", "/w/.wt/x/c.ts"]);
  });

  it("accepted: record ids; rejected: paths", () => {
    expect(bulkKeys(view, "accepted")).toEqual(["t1::/w/a.ts", "t2::/w/e.ts"]);
    expect(bulkKeys(view, "rejected")).toEqual(["/w/f.ts"]);
  });

  it("a scope root with a trailing slash or backslashes still matches", () => {
    expect(bulkKeys({ ...view, pending: [p("C:\\w\\a.ts", "C:\\w\\")] }, "pending")).toEqual(["C:\\w\\a.ts"]);
  });
});
