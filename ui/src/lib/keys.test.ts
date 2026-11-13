import { describe, expect, it } from "vitest";
import { actionFor, isTypingTarget, nextIndex } from "./keys";

describe("nextIndex", () => {
  it("clamps without wrapping", () => {
    expect(nextIndex(0, 3, -1)).toBe(0);
    expect(nextIndex(2, 3, 1)).toBe(2);
    expect(nextIndex(1, 3, 1)).toBe(2);
    expect(nextIndex(1, 3, -1)).toBe(0);
  });

  it("starts from an end when nothing is selected", () => {
    expect(nextIndex(-1, 3, 1)).toBe(0);
    expect(nextIndex(-1, 3, -1)).toBe(2);
  });

  it("has nowhere to go in an empty list", () => {
    expect(nextIndex(-1, 0, 1)).toBe(-1);
    expect(nextIndex(0, 0, -1)).toBe(-1);
  });
});

const k = (key: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean }> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  ...mods,
});

describe("actionFor", () => {
  it("maps the review keys", () => {
    expect(actionFor(k("j"))).toEqual({ type: "row", dir: 1 });
    expect(actionFor(k("ArrowDown"))).toEqual({ type: "row", dir: 1 });
    expect(actionFor(k("k"))).toEqual({ type: "row", dir: -1 });
    expect(actionFor(k("ArrowUp"))).toEqual({ type: "row", dir: -1 });
    expect(actionFor(k("n"))).toEqual({ type: "pending", dir: 1 });
    expect(actionFor(k("p"))).toEqual({ type: "pending", dir: -1 });
    expect(actionFor(k("o"))).toEqual({ type: "open" });
    expect(actionFor(k("1"))).toEqual({ type: "tab", tab: "pending" });
    expect(actionFor(k("2"))).toEqual({ type: "tab", tab: "accepted" });
    expect(actionFor(k("3"))).toEqual({ type: "tab", tab: "rejected" });
  });

  it("leaves modified and unknown keys to the system", () => {
    expect(actionFor(k("j", { ctrlKey: true }))).toBeNull();
    expect(actionFor(k("o", { metaKey: true }))).toBeNull();
    expect(actionFor(k("1", { altKey: true }))).toBeNull();
    expect(actionFor(k("J"))).toBeNull();
    expect(actionFor(k("x"))).toBeNull();
  });
});

type El = { tagName: string; isContentEditable?: boolean; closest?: (s: string) => unknown };
const el = (tagName: string, extra: Partial<El> = {}): El => ({ tagName, closest: () => null, ...extra });

describe("isTypingTarget", () => {
  it("is true for fields, editable regions and open menus", () => {
    expect(isTypingTarget(el("INPUT"))).toBe(true);
    expect(isTypingTarget(el("TEXTAREA"))).toBe(true);
    expect(isTypingTarget(el("SELECT"))).toBe(true);
    expect(isTypingTarget(el("DIV", { isContentEditable: true }))).toBe(true);
    expect(isTypingTarget(el("BUTTON", { closest: (s) => (s.includes("menu") ? {} : null) }))).toBe(true);
  });

  it("is false for rows, buttons and nothing", () => {
    expect(isTypingTarget(el("BUTTON"))).toBe(false);
    expect(isTypingTarget(el("BODY"))).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });
});
