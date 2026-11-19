import { describe, expect, it } from "vitest";
import { alreadyExcluded, describeGlob, excludedNotice, extensionGlob, folderGlob, hidePrompt } from "./excludeSuggest";

describe("extensionGlob", () => {
  it("globs by the last extension", () => {
    expect(extensionGlob("/repo/docs/plan.md")).toBe("**/*.md");
    expect(extensionGlob("/repo/a.test.ts")).toBe("**/*.ts");
  });
  it("refuses names that would produce an over-broad pattern", () => {
    expect(extensionGlob("/repo/Makefile")).toBeUndefined();
    expect(extensionGlob("/repo/.env")).toBeUndefined();
    expect(extensionGlob("/repo/trailing.")).toBeUndefined();
  });
});

describe("folderGlob", () => {
  it("is relative to the scope root", () => {
    expect(folderGlob("/repo/docs/plans/x.md", "/repo")).toBe("**/docs/plans/**");
    expect(folderGlob("C:\\repo\\docs\\x.md", "C:\\repo")).toBe("**/docs/**");
  });
  it("refuses a file at the root", () => {
    expect(folderGlob("/repo/x.md", "/repo")).toBeUndefined();
    expect(folderGlob("/repo/x.md", "/repo/")).toBeUndefined();
  });
  it("falls back to the directory's own name outside the root, or with no root", () => {
    expect(folderGlob("/elsewhere/tmp/x.md", "/repo")).toBe("**/tmp/**");
    expect(folderGlob("/repo/docs/x.md")).toBe("**/docs/**");
  });
});

describe("describeGlob", () => {
  it("turns a pattern into a sentence", () => {
    expect(describeGlob("**/*.md")).toBe("every .md file");
    expect(describeGlob("**/docs/plans/**")).toBe("everything under docs/plans/");
    expect(describeGlob("weird[pattern")).toBe("weird[pattern");
  });
});

describe("prompts", () => {
  it("use the agreed copy", () => {
    expect(hidePrompt("**/*.md", 4)).toBe(
      "Hide every .md file from review? 4 pending file(s) would leave the list, and future ones would not appear. This does NOT accept or reject anything — the changes stay on disk and the entries stay in the session. Pattern: **/*.md",
    );
    expect(alreadyExcluded("**/*.md")).toBe('"**/*.md" is already excluded.');
    expect(excludedNotice("**/*.md")).toBe("Excluded **/*.md. Remove it in Settings to bring those files back.");
  });
});
