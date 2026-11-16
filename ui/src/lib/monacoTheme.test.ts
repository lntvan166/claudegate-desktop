import { describe, expect, it } from "vitest";
import { buildTheme } from "./monacoTheme";

const rule = (t: ReturnType<typeof buildTheme>, token: string) => t.rules.find((r) => r.token === token);

describe("buildTheme (dark: Dracula)", () => {
  const t = buildTheme(true);

  it("sits on Dracula's ground", () => {
    expect(t.base).toBe("vs-dark");
    expect(t.colors["editor.background"]).toBe("#282a36");
    expect(t.colors["editor.foreground"]).toBe("#f8f8f2");
    expect(t.colors["editorLineNumber.foreground"]).toBe("#6272a4");
    expect(t.colors["editor.selectionBackground"]).toBe("#44475a");
  });

  it("uses Dracula's VS Code diff tints", () => {
    expect(t.colors["diffEditor.insertedTextBackground"]).toBe("#50fa7b20");
    expect(t.colors["diffEditor.removedTextBackground"]).toBe("#ff555550");
    expect(t.colors["diffEditor.insertedLineBackground"]).toBe("#50fa7b14");
    expect(t.colors["diffEditor.removedLineBackground"]).toBe("#ff555530");
    expect(t.colors["diffEditor.diagonalFill"]).toBe("#44475a80");
  });

  it("colours tokens the way Dracula does", () => {
    expect(rule(t, "keyword")?.foreground).toBe("ff79c6");
    expect(rule(t, "string")?.foreground).toBe("f1fa8c");
    expect(rule(t, "comment")?.foreground).toBe("6272a4");
    expect(rule(t, "number")?.foreground).toBe("bd93f9");
    expect(rule(t, "type")).toMatchObject({ foreground: "8be9fd", fontStyle: "italic" });
  });
});

describe("buildTheme (light: Alucard)", () => {
  const t = buildTheme(false);

  it("sits on a light ground, never Dracula's dark one", () => {
    expect(t.base).toBe("vs");
    expect(t.colors["editor.background"]).toBe("#f8f8f2");
    expect(t.colors["editor.foreground"]).toBe("#1f1f1f");
    expect(Object.values(t.colors)).not.toContain("#282a36");
  });

  it("uses VS Code's light diff tints", () => {
    expect(t.colors["diffEditor.insertedTextBackground"]).toBe("#9ccc2c40");
    expect(t.colors["diffEditor.insertedLineBackground"]).toBe("#9bb95533");
    expect(t.colors["diffEditor.removedTextBackground"]).toBe("#ff000033");
    expect(t.colors["diffEditor.removedLineBackground"]).toBe("#ff00001a");
  });

  it("keeps the hue roles, darkened for contrast", () => {
    expect(rule(t, "keyword")?.foreground).toBe("a3144d");
    expect(rule(t, "string")?.foreground).toBe("846e15");
    expect(rule(t, "comment")?.foreground).toBe("6c664b");
  });
});

describe("both themes", () => {
  it("pass Monaco only well-formed colours", () => {
    for (const dark of [true, false]) {
      const t = buildTheme(dark);
      expect(Object.values(t.colors).every((v) => /^#[0-9a-f]{6}([0-9a-f]{2})?$/.test(v))).toBe(true);
      expect(t.rules.every((r) => r.foreground === undefined || /^[0-9a-f]{6}$/.test(r.foreground))).toBe(true);
    }
  });
});
