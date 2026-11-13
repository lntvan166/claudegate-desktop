import { describe, expect, it } from "vitest";
import { iconFor } from "./icons";

describe("iconFor", () => {
  it("matches exact file names first", () => {
    expect(iconFor("package.json", "file", false)).toMatch(/\/nodejs\.svg$/);
  });

  it("matches by extension", () => {
    expect(iconFor("utils.ts", "file", false)).toMatch(/\/typescript\.svg$/);
  });

  it("prefers the longest extension suffix", () => {
    expect(iconFor("utils.test.ts", "file", false)).toMatch(/\/test-ts\.svg$/);
    expect(iconFor("types.d.ts", "file", false)).toMatch(/\/typescript-def\.svg$/);
  });

  it("is case-insensitive", () => {
    expect(iconFor("PACKAGE.JSON", "file", false)).toMatch(/\/nodejs\.svg$/);
    expect(iconFor("App.TS", "file", false)).toMatch(/\/typescript\.svg$/);
  });

  it("uses named folder icons, open and closed", () => {
    expect(iconFor("src", "folder", true)).toMatch(/\/folder-src-open\.svg$/);
    expect(iconFor("src", "folder", false)).toMatch(/\/folder-src\.svg$/);
  });

  it("names a compacted folder by its last segment", () => {
    expect(iconFor("pkg/core/src", "folder", true)).toMatch(/\/folder-src-open\.svg$/);
  });

  it("falls back to the default icons", () => {
    expect(iconFor("weird.zzz", "file", false)).toMatch(/\/file\.svg$/);
    expect(iconFor("nothing-special-here", "folder", false)).toMatch(/\/folder\.svg$/);
    expect(iconFor("nothing-special-here", "folder", true)).toMatch(/\/folder-open\.svg$/);
  });

  it("uses light variants on a light theme when the theme has one", () => {
    expect(iconFor("jinja", "folder", false, "light")).toMatch(/\/folder-jinja_light\.svg$/);
    expect(iconFor("utils.ts", "file", false, "light")).toMatch(/\/typescript\.svg$/);
  });
});
