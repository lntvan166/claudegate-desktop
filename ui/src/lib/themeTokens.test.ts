// UX wave 5: the chrome is Dracula (dark) and Alucard (light), and every text token is readable.
import { describe, expect, it } from "vitest";
import { contrast } from "./contrast";
import { ALUCARD, DRACULA } from "./monacoTheme";

// Read with Node's fs (the tests run in Node); typed loosely, as the app's tsconfig has no Node types.
const fsName = "node:fs";
const { readFileSync } = (await import(/* @vite-ignore */ fsName)) as { readFileSync: (p: URL, enc: string) => string };
const css = readFileSync(new URL("../theme.css", import.meta.url), "utf8");

/** The custom properties declared in the first block that follows `marker`. */
function tokens(marker: string): Record<string, string> {
  const start = css.indexOf(marker);
  const body = css.slice(start, css.indexOf("}", start));
  return Object.fromEntries([...body.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\b/g)].map((m) => [m[1], m[2].toLowerCase()]));
}
const light = tokens("/* light:");
const dark = tokens("@media (prefers-color-scheme: dark)");

describe("dark chrome is Dracula", () => {
  it("uses the palette the brief names", () => {
    expect(dark.frame).toBe("#21222c");
    expect(dark.surface).toBe("#282a36");
    expect(dark.active).toBe("#44475a");
    expect(dark.border).toBe("#343746");
    expect(dark.text).toBe("#f8f8f2");
    expect(dark["focus-ring"]).toBe("#bd93f9");
    expect(dark.positive).toBe("#50fa7b");
    expect(dark.negative).toBe("#ff5555");
    expect(dark.pending).toBe("#ffb86c");
    expect(dark["waiting-ink"]).toBe("#f1fa8c");
  });
  it("shares one surface with the diff, so there is no seam next to Monaco", () => {
    expect(dark.surface).toBe(DRACULA.background);
  });
});

describe("light chrome is Alucard", () => {
  it("sits on the light diff theme's ground", () => {
    expect(light.surface).toBe(ALUCARD.background);
    expect(light.frame).toBe(ALUCARD.backgroundDarker);
    expect(light.text).toBe(ALUCARD.foreground);
  });
});

describe("text tokens are readable (WCAG AA, 4.5:1) on the surface", () => {
  for (const [mode, t] of [["dark", dark], ["light", light]] as const) {
    for (const k of ["text", "muted", "faint", "pending", "untracked", "positive", "negative", "mark", "waiting-ink"]) {
      it(`${mode} --${k}`, () => {
        expect(contrast(t[k], t.surface)).toBeGreaterThanOrEqual(4.5);
      });
    }
    it(`${mode}: muted and pending stay readable on the selected row`, () => {
      expect(contrast(t.text, t.active)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(t.pending, t.active)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(t.muted, t.active)).toBeGreaterThanOrEqual(4.5);
    });
    it(`${mode}: the destructive and primary buttons' labels are readable`, () => {
      expect(contrast(t["on-danger"], t.danger)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(t["on-primary"], t.primary)).toBeGreaterThanOrEqual(4.5);
    });
  }
  it("Dracula's own comment colour is not, which is why muted is lighter", () => {
    expect(contrast("#6272a4", "#282a36")).toBeLessThan(4.5);
  });
});
