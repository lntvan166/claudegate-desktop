import dracula from "@shikijs/themes/dracula";
import githubLight from "@shikijs/themes/github-light";
import { describe, expect, it } from "vitest";
import { buildTheme, fromTextmate as fromShiki, themeName } from "./monacoTheme";
const rule = (t: ReturnType<typeof buildTheme>, token: string) => t.rules.find((r) => r.token === token);

describe("buildTheme (dark: Shiki's Dracula)", () => {
  const t = buildTheme(true, fromShiki(dracula));

  it("sits on Dracula's ground", () => {
    expect(t.base).toBe("vs-dark");
    expect(t.colors["editor.background"]).toBe("#282a36");
    expect(t.colors["editor.foreground"]).toBe("#f8f8f2");
    expect(t.colors["editorLineNumber.foreground"]).toBe("#6272a4");
    expect(t.colors["editor.selectionBackground"]).toBe("#44475a");
  });

  it("keeps Dracula's VS Code diff tints over whatever the TextMate theme says", () => {
    expect(t.colors["diffEditor.insertedTextBackground"]).toBe("#50fa7b20");
    expect(t.colors["diffEditor.removedTextBackground"]).toBe("#ff555550");
    expect(t.colors["diffEditor.insertedLineBackground"]).toBe("#50fa7b14");
    expect(t.colors["diffEditor.removedLineBackground"]).toBe("#ff555530");
    expect(t.colors["diffEditorGutter.insertedLineBackground"]).toBe("#50fa7b30");
    expect(t.colors["diffEditorOverview.insertedForeground"]).toBe("#50fa7b80");
    expect(t.colors["diffEditor.diagonalFill"]).toBe("#44475a80");
  });

  it("takes its token rules from the TextMate theme, scope for scope", () => {
    expect(rule(t, "entity.name.function")?.foreground).toBe("50fa7b");
    expect(rule(t, "variable.parameter")?.foreground).toBe("ffb86c");
    expect(rule(t, "keyword")?.foreground).toBe("ff79c6");
  });

  it("keeps editor colours Shiki brings that we do not set", () => {
    const shiki = fromShiki(dracula);
    const extra = Object.keys(shiki.colors).find((k) => !(k in buildTheme(true, { ...shiki, colors: {} }).colors));
    expect(extra).toBeDefined();
    expect(t.colors[extra!]).toBe(shiki.colors[extra!]);
  });
});

describe("buildTheme (light: GitHub Light tokens on the Alucard ground)", () => {
  const t = buildTheme(false, fromShiki(githubLight));

  it("sits on a light ground, never Dracula's dark one", () => {
    expect(t.base).toBe("vs");
    expect(t.colors["editor.background"]).toBe("#f8f8f2");
    expect(Object.values(t.colors)).not.toContain("#282a36");
  });

  it("uses VS Code's light diff tints", () => {
    expect(t.colors["diffEditor.insertedTextBackground"]).toBe("#9ccc2c40");
    expect(t.colors["diffEditor.insertedLineBackground"]).toBe("#9bb95533");
    expect(t.colors["diffEditor.removedTextBackground"]).toBe("#ff000033");
    expect(t.colors["diffEditor.removedLineBackground"]).toBe("#ff00001a");
  });

  it("colours functions the way GitHub Light does", () => {
    expect(rule(t, "entity.name")?.foreground).toBe("6f42c1");
  });
});

describe("both themes", () => {
  it("pass Monaco only well-formed colours", () => {
    for (const dark of [true, false]) {
      const t = buildTheme(dark, fromShiki(dark ? dracula : githubLight));
      expect(Object.values(t.colors).every((v) => /^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(v))).toBe(true);
      expect(t.rules.every((r) => r.foreground === undefined || /^[0-9a-f]{6}([0-9a-f]{2})?$/.test(r.foreground))).toBe(true);
    }
  });

  it("have distinct names", () => {
    expect(themeName(true)).not.toBe(themeName(false));
  });
});
