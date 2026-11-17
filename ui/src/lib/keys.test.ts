import { describe, expect, it } from "vitest";
import { actionFor, decisionFor, isTypingTarget, nextIndex, WAITING_REFUSAL, DIFF_HIDDEN_REFUSAL } from "./keys";

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
    expect(actionFor(k("["))).toEqual({ type: "projects" });
  });

  it("leaves modified and unknown keys to the system", () => {
    expect(actionFor(k("j", { ctrlKey: true }))).toBeNull();
    expect(actionFor(k("o", { metaKey: true }))).toBeNull();
    expect(actionFor(k("1", { altKey: true }))).toBeNull();
    expect(actionFor(k("J"))).toBeNull();
    expect(actionFor(k("x"))).toBeNull();
    expect(actionFor(k("[", { ctrlKey: true }))).toBeNull();
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
    // A key sent to the window has no tag.
    expect(isTypingTarget({} as unknown as El)).toBe(false);
  });
});

describe("decisionFor", () => {
  const ctx = {
    tab: "pending" as const,
    typing: false,
    menuOpen: false,
    readOnly: false,
    busy: false,
    selectedKey: "/w/a.ts" as string | null,
    selectedWaiting: false,
  };

  it("a accepts and r rejects the selected pending row", () => {
    expect(decisionFor(k("a"), ctx)).toEqual({ action: "accept", key: "/w/a.ts" });
    expect(decisionFor(k("r"), ctx)).toEqual({ action: "reject", key: "/w/a.ts" });
  });

  it("is ignored on the Accepted and Rejected tabs", () => {
    expect(decisionFor(k("a"), { ...ctx, tab: "accepted" })).toBeNull();
    expect(decisionFor(k("r"), { ...ctx, tab: "rejected" })).toBeNull();
  });

  it("is ignored while typing, with the menu open, in read-only mode, or mid-decision", () => {
    expect(decisionFor(k("a"), { ...ctx, typing: true })).toBeNull();
    expect(decisionFor(k("a"), { ...ctx, menuOpen: true })).toBeNull();
    expect(decisionFor(k("r"), { ...ctx, readOnly: true })).toBeNull();
    expect(decisionFor(k("r"), { ...ctx, busy: true })).toBeNull();
  });

  it("needs a selected row and an unmodified key", () => {
    expect(decisionFor(k("a"), { ...ctx, selectedKey: null })).toBeNull();
    expect(decisionFor(k("a", { ctrlKey: true }), ctx)).toBeNull();
    expect(decisionFor(k("A"), ctx)).toBeNull();
    expect(decisionFor(k("x"), ctx)).toBeNull();
  });
});

describe("decisionFor on a waiting row", () => {
  const ctx = {
    tab: "pending" as const,
    typing: false,
    menuOpen: false,
    readOnly: false,
    busy: false,
    selectedKey: "/w/a.ts" as string | null,
    selectedWaiting: true,
  };

  it("refuses a and r with the reason, never a decision", () => {
    expect(decisionFor(k("a"), ctx)).toEqual({ refused: WAITING_REFUSAL });
    expect(decisionFor(k("r"), ctx)).toEqual({ refused: WAITING_REFUSAL });
    expect(WAITING_REFUSAL).toBe("Claude hasn't written this yet");
  });

  it("stays silent for keys that are not decisions, or when the key would be ignored anyway", () => {
    expect(decisionFor(k("x"), ctx)).toBeNull();
    expect(decisionFor(k("a"), { ...ctx, typing: true })).toBeNull();
    expect(decisionFor(k("a"), { ...ctx, tab: "accepted" })).toBeNull();
  });
});

describe("decisionFor while the diff is hidden (narrow window)", () => {
  const ctx = {
    tab: "pending" as const,
    typing: false,
    menuOpen: false,
    readOnly: false,
    busy: false,
    selectedKey: "/w/a.ts" as string | null,
    diffHidden: true,
  };

  it("never decides a file whose diff is not on screen, and says why", () => {
    expect(decisionFor(k("a"), ctx)).toEqual({ refused: DIFF_HIDDEN_REFUSAL });
    expect(decisionFor(k("r"), ctx)).toEqual({ refused: DIFF_HIDDEN_REFUSAL });
  });

  it("decides once the diff is shown", () => {
    expect(decisionFor(k("a"), { ...ctx, diffHidden: false })).toEqual({ action: "accept", key: "/w/a.ts" });
  });
});
