import { describe, expect, it } from "vitest";
import { themeColors, toHex } from "./monacoTheme";

describe("toHex", () => {
  it("converts hex and rgba, applying alpha", () => {
    expect(toHex("#1e7b4f")).toBe("#1e7b4f");
    expect(toHex("#1E7B4F", 0.5)).toBe("#1e7b4f80");
    expect(toHex("#abc")).toBe("#aabbcc");
    expect(toHex("rgba(255, 255, 255, 0.14)")).toBe("#ffffff24");
    expect(toHex("rgb(19, 20, 22)")).toBe("#131416");
    expect(toHex(" #ffffff ", 0)).toBe("#ffffff00");
  });

  it("returns undefined for what it cannot read", () => {
    expect(toHex("")).toBeUndefined();
    expect(toHex("var(--x)")).toBeUndefined();
  });
});

describe("themeColors", () => {
  const tokens: Record<string, string> = {
    "--surface": "#ffffff",
    "--text": "#131416",
    "--positive": "#1e7b4f",
    "--negative": "#c4242a",
    "--faint": "#626468",
    "--scrollbar": "rgba(19, 20, 22, 0.16)",
  };
  it("takes the editor ground and diff tints from the app tokens", () => {
    const c = themeColors((n) => tokens[n] ?? "", false);
    expect(c["editor.background"]).toBe("#ffffff");
    expect(c["editor.foreground"]).toBe("#131416");
    expect(c["diffEditor.insertedLineBackground"]).toMatch(/^#1e7b4f[0-9a-f]{2}$/);
    expect(c["diffEditor.removedTextBackground"]).toMatch(/^#c4242a[0-9a-f]{2}$/);
    expect(c["scrollbarSlider.background"]).toBe("#13141629");
  });

  it("drops colours it could not read rather than passing junk to Monaco", () => {
    const c = themeColors(() => "", true);
    expect(Object.values(c).every((v) => /^#[0-9a-f]{6,8}$/.test(v))).toBe(true);
  });
});
