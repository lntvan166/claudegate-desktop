import { describe, expect, it } from "vitest";
import {
  ChangesCache,
  CHANGES_DEBOUNCE_MS,
  changeTree,
  changesTitle,
  countsText,
  createGate,
  diffPane,
  foldersOf,
  formatBytes,
  paneMessage,
  pickFile,
  statusLetter,
  statusText,
  treeOrder,
  visibleNodes,
  type ChangedFile,
  type FileView,
} from "./changes";

const f = (path: string, status: ChangedFile["status"] = "modified", extra: Partial<ChangedFile> = {}): ChangedFile => ({
  path,
  status,
  from: null,
  added: 1,
  removed: 0,
  ...extra,
});
const text = (value: string): FileView => ({ kind: "text", value });

describe("title", () => {
  it("reads repo, short id, file count", () => {
    expect(changesTitle("acme-api", "3f9a2c1".padEnd(40, "0"), 2)).toBe("acme-api · 3f9a2c1 · 2 files");
    expect(changesTitle("acme-api", "3f9a2c1".padEnd(40, "0"), 1)).toBe("acme-api · 3f9a2c1 · 1 file");
    expect(changesTitle("svc-lib", "abcdef0123", 0)).toBe("svc-lib · abcdef0 · 0 files");
  });
});

describe("badges and counts", () => {
  it("maps statuses to letters", () => {
    expect(["added", "modified", "deleted", "renamed", "copied", "typeChanged"].map((s) => statusLetter(f("a", s as ChangedFile["status"])))).toEqual(["A", "M", "D", "R", "C", "T"]);
  });
  it("names a rename's old path", () => {
    expect(statusText(f("b.ts", "renamed", { from: "a.ts" }))).toBe("Renamed from a.ts");
    expect(statusText(f("b.ts", "added"))).toBe("Added");
  });
  it("counts, and says binary when there are none", () => {
    expect(countsText(f("a", "modified", { added: 3, removed: 1 }))).toBe("+3 −1");
    expect(countsText(f("a.png", "modified", { added: null, removed: null }))).toBe("binary");
  });
});

describe("tree", () => {
  const files = [f("src/api/user.ts"), f("README.md"), f("src/api/order.ts", "renamed", { from: "src/old.ts" }), f("docs/a.md"), f("docs/b/c.md")];
  it("puts folders first, compacts single-child chains, and keeps renames at their new path", () => {
    const rows = visibleNodes(changeTree(files), new Set());
    expect(rows.map((r) => `${" ".repeat(r.depth)}${r.node.name}`)).toEqual([
      "docs",
      " b",
      "  c.md",
      " a.md",
      "src/api",
      " order.ts",
      " user.ts",
      "README.md",
    ]);
    const order = rows.find((r) => r.node.name === "order.ts")!;
    expect(order.node.item?.file.from).toBe("src/old.ts");
    expect(order.node.rel).toBe("src/api/order.ts");
  });
  it("hides the children of a collapsed folder", () => {
    const nodes = changeTree(files);
    const all = visibleNodes(nodes, new Set()).length;
    const closed = visibleNodes(nodes, new Set(["src/api"])).length;
    expect(closed).toBe(all - 2);
  });
  it("lists files in tree order, and picks the open file again when the next commit has it", () => {
    expect(treeOrder(files).map((x) => x.path)[0]).toBe("docs/b/c.md");
    expect(pickFile(files, "README.md")?.path).toBe("README.md");
    expect(pickFile(files, "gone.ts")?.path).toBe("docs/b/c.md");
    expect(pickFile([], "a")).toBeNull();
  });
  it("names the folders above a path", () => {
    expect(foldersOf("a/b/c.ts")).toEqual(["a", "a/b"]);
    expect(foldersOf("c.ts")).toEqual([]);
  });
});

describe("diff pane state", () => {
  it("diffs text against text", () => {
    expect(diffPane("a.ts", text("a"), text("b"))).toEqual({ kind: "diff", payload: { path: "a.ts", before: { kind: "text", text: "a" }, after: { kind: "text", text: "b" } } });
  });
  it("an added file diffs against nothing, a deleted one the other way", () => {
    const added = diffPane("a.ts", { kind: "missing" }, text("x"));
    expect(added.kind === "diff" && added.payload.before).toEqual({ kind: "missing" });
    const gone = diffPane("a.ts", text("x"), { kind: "missing" });
    expect(gone.kind === "diff" && gone.payload.after).toEqual({ kind: "missing" });
  });
  it("binary on either side wins", () => {
    expect(diffPane("a.png", { kind: "binary" }, text("x")).kind).toBe("binary");
    expect(diffPane("a.png", { kind: "tooLarge", value: 9 }, { kind: "binary" }).kind).toBe("binary");
  });
  it("too large carries the larger size", () => {
    expect(diffPane("a.sql", { kind: "tooLarge", value: 3_000_000 }, { kind: "tooLarge", value: 5_000_000 })).toEqual({ kind: "tooLarge", bytes: 5_000_000 });
    expect(diffPane("a.sql", text("x"), { kind: "tooLarge", value: 2_500_000 })).toEqual({ kind: "tooLarge", bytes: 2_500_000 });
  });
  it("two missing sides is missing", () => {
    expect(diffPane("a", { kind: "missing" }, { kind: "missing" })).toEqual({ kind: "missing" });
  });
  it("words each non-diff state", () => {
    expect(paneMessage({ kind: "binary" }).title).toMatch(/binary/i);
    expect(paneMessage({ kind: "tooLarge", bytes: 2_500_000 }).body).toContain("2.4 MB");
    expect(paneMessage({ kind: "missing" }).title).toMatch(/nothing/i);
  });
  it("formats sizes", () => {
    expect(formatBytes(12)).toBe("12 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(300 * 1024)).toBe("300 KB");
  });
});

describe("races", () => {
  it("only the newest generation may land", () => {
    const g = createGate();
    const a = g.next();
    const b = g.next();
    expect(g.isCurrent(a)).toBe(false);
    expect(g.isCurrent(b)).toBe(true);
  });
  it("the cache keeps the most recent commits", () => {
    const c = new ChangesCache(2);
    c.set("r", "1", [f("a")]);
    c.set("r", "2", [f("b")]);
    expect(c.get("r", "1")).toBeDefined(); // 1 is now the newest
    c.set("r", "3", [f("c")]);
    expect(c.get("r", "2")).toBeUndefined();
    expect(c.get("r", "1")).toBeDefined();
    expect(c.get("other", "1")).toBeUndefined();
  });
  it("debounces a held arrow key", () => {
    expect(CHANGES_DEBOUNCE_MS).toBeGreaterThanOrEqual(60);
    expect(CHANGES_DEBOUNCE_MS).toBeLessThanOrEqual(120);
  });
});
