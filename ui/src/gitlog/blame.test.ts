import { describe, expect, it } from "vitest";
// @ts-expect-error Monaco ships this module without types.
import { marked } from "monaco-editor/base/common/marked/marked.js";
import { blameHover, blameText, combinedBlameSides, commitBlameSides, escapeMarkdown, BLAME_DEBOUNCE_MS } from "./blame";
import type { ChangedFile } from "./changes";
import type { CombinedFile } from "./combined";
import type { RowView } from "./log";

const NOW = Date.UTC(2026, 0, 10);
const T = Math.floor(Date.UTC(2026, 0, 7) / 1000);
const b = (o: Partial<{ summary: string; author: string; email: string }> = {}) => ({
  sha: "a1b2c3d4e5f6a7b8c9d0a1b2c3d4e5f6a7b8c9d0",
  author: "Ann",
  email: "ann@example.com",
  time: T,
  summary: "Fix the thing",
  ...o,
});

describe("blame text", () => {
  it("formats the end-of-line text", () => {
    expect(blameText(b(), NOW)).toBe("Fix the thing, Ann (3d ago)");
    const long = "x".repeat(60);
    expect(blameText(b({ summary: long }), NOW)).toBe(`${"x".repeat(49)}…, Ann (3d ago)`);
    const fifty = "y".repeat(50);
    expect(blameText(b({ summary: fifty }), NOW)).toBe(`${fifty}, Ann (3d ago)`);
    expect(BLAME_DEBOUNCE_MS).toBe(150);
  });

  it("escapes markdown in the hover", () => {
    const h = blameHover(b({ summary: "*bold* [x](javascript:alert(1))", author: "<img src=x>", email: "a_b@x.io" }), NOW);
    const bare = (s: string, c: string) => [...s].some((ch, i) => ch === c && s[i - 1] !== "\\");
    // The first paragraph holds the template's own parentheses around the date; the subject is the second.
    const subject = h.split("\n\n")[1];
    for (const c of ["[", "(", "*"]) expect(bare(subject, c), c).toBe(false);
    expect(h).toContain("\\*bold\\* \\[x\\]\\(javascript\\:alert\\(1\\)\\)");
    expect(h).toContain("\\<img src=x\\>");
    expect(h.endsWith("\n\n`a1b2c3d`")).toBe(true);
    expect(escapeMarkdown("a\\b`c*d_e{f}g[h]i(j)k#l+m-n.o!p|q<r>s")).toBe("a\\\\b\\`c\\*d\\_e\\{f\\}g\\[h\\]i\\(j\\)k\\#l\\+m\\-n\\.o\\!p\\|q\\<r\\>s");
  });
});

describe("hover rendering", () => {
  // Monaco renders hover markdown with this same marked.
  const html = (s: string) => marked.parse(s, { async: false }) as string;
  it("renders links and strikethrough as plain text", () => {
    for (const subject of ["see https://github.com/x", "http://localhost/x", "~~x~~", "www.example.com/a", "javascript:alert(1)"]) {
      const out = html(blameHover(b({ summary: subject }), NOW).split("\n\n")[1]);
      expect(out, subject).not.toMatch(/<a |<del|href/);
    }
  });
  it("cuts the subject by code point, never inside an emoji", () => {
    const text = blameText(b({ summary: "😀".repeat(60) }), NOW);
    expect(text.startsWith(`${"😀".repeat(49)}…, Ann`)).toBe(true);
    expect(text).not.toMatch(/[\ud800-\udbff](?![\udc00-\udfff])/);
  });
});

const row = (parents: string[]): RowView => ({ repo: "/r/.git", sha: "c".repeat(40), time: 1, author: "a", email: "e", subject: "s", parents, source: "HEAD" });
const file = (status: ChangedFile["status"], path = "src/a.ts", from: string | null = null): ChangedFile => ({ path, status, from, added: 1, removed: 0 });

describe("blame sides", () => {
  const P = "p".repeat(40);
  it("blames each side at its own revision", () => {
    const r = row([P]);
    expect(commitBlameSides(r, file("modified"))).toEqual({
      original: { repo: "/r/.git", rev: P, path: "src/a.ts" },
      modified: { repo: "/r/.git", rev: r.sha, path: "src/a.ts" },
    });
    expect(commitBlameSides(r, file("added"))).toEqual({ original: null, modified: { repo: "/r/.git", rev: r.sha, path: "src/a.ts" } });
    expect(commitBlameSides(r, file("deleted"))).toEqual({ original: { repo: "/r/.git", rev: P, path: "src/a.ts" }, modified: null });
    expect(commitBlameSides(r, file("renamed", "src/new.ts", "src/old.ts"))).toEqual({
      original: { repo: "/r/.git", rev: P, path: "src/old.ts" },
      modified: { repo: "/r/.git", rev: r.sha, path: "src/new.ts" },
    });
    const root = commitBlameSides(row([]), file("modified"));
    expect(root.original).toBeNull();
    expect(root.modified?.rev).toBe(r.sha);
  });

  const cf = (o: Partial<CombinedFile> = {}): CombinedFile => ({
    path: "n.ts", from: null, status: "modified", shas: ["1"], last: "l".repeat(40), beforeRef: "b".repeat(40), beforePath: "o.ts",
    included: [], mode: "fast", reason: null, ...o,
  });
  it("does not blame a merged left side", () => {
    expect(combinedBlameSides("/r", cf(), true).original).toBeNull();
    expect(combinedBlameSides("/r", cf(), false)).toEqual({
      original: { repo: "/r", rev: "b".repeat(40), path: "o.ts" },
      modified: { repo: "/r", rev: "l".repeat(40), path: "n.ts" },
    });
    expect(combinedBlameSides("/r", cf({ beforeRef: null }), false).original).toBeNull();
    expect(combinedBlameSides("/r", cf({ status: "deleted" }), false).modified).toBeNull();
  });
});

describe("compare blame", () => {
  it("blames a compare diff at the base and the tip", async () => {
    const { compareBlameSides } = await import("./blame");
    const base = "b".repeat(40);
    const tip = "t".repeat(40);
    expect(compareBlameSides("/r/.git", base, tip, "src/new.ts", "src/old.ts")).toEqual({
      original: { repo: "/r/.git", rev: base, path: "src/old.ts" },
      modified: { repo: "/r/.git", rev: tip, path: "src/new.ts" },
    });
    expect(compareBlameSides("/r/.git", base, tip, "a.ts", null).original).toEqual({ repo: "/r/.git", rev: base, path: "a.ts" });
  });
});
