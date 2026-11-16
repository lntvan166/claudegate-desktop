import { describe, expect, it } from "vitest";
import { LANGUAGES } from "./language";
import { GRAMMARS, createDiffHighlighter, SHIKI_THEME } from "./grammars";

describe("GRAMMARS", () => {
  it("has a lazy loader for every language but plaintext", () => {
    for (const l of LANGUAGES) {
      if (l === "plaintext") expect(GRAMMARS).not.toHaveProperty(l);
      else expect(typeof GRAMMARS[l], l).toBe("function");
    }
  });

  it("each loader yields the grammar it is named for", async () => {
    for (const [id, load] of Object.entries(GRAMMARS)) {
      const regs = (await load()).default;
      const names = regs.flatMap((r) => [r.name, ...(r.aliases ?? [])]);
      expect(names, id).toContain(id);
    }
  });
});

describe("createDiffHighlighter", () => {
  it("colours Go the way VS Code's Dracula does: func green, parameters orange", async () => {
    const hl = await createDiffHighlighter();
    await hl.loadLanguage((await GRAMMARS.go()).default);
    const [line] = hl.codeToTokensBase("func Damage(level int) int {", { lang: "go", theme: SHIKI_THEME.dark });
    const color = (text: string) => line.find((t) => t.content.trim() === text)?.color?.toLowerCase();
    expect(color("func")).toBe("#ff79c6");
    expect(color("Damage")).toBe("#50fa7b");
    expect(color("level")).toBe("#ffb86c");
  });

  it("starts with no grammars loaded, so the first paint pays for none", async () => {
    const hl = await createDiffHighlighter();
    expect(hl.getLoadedLanguages().filter((l) => l !== "plaintext" && l !== "text" && l !== "txt" && l !== "plain")).toEqual([]);
    expect(hl.getLoadedThemes().sort()).toEqual([SHIKI_THEME.dark, SHIKI_THEME.light].sort());
  });
});
