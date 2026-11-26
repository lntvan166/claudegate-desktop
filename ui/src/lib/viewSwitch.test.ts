import { describe, expect, it } from "vitest";
import { firstShown, isShown, needsFocusMove, overlayShown } from "./viewSwitch";

const el = (shown: boolean, connected = true) => ({ isConnected: connected, checkVisibility: () => shown });

describe("the view switch's focus rules", () => {
  it("counts only connected, rendered elements", () => {
    expect(isShown(el(true))).toBe(true);
    expect(isShown(el(false))).toBe(false);
    expect(isShown(el(true, false))).toBe(false);
    expect(isShown(null)).toBe(false);
    // Without checkVisibility, a box with no client rects is hidden.
    expect(isShown({ isConnected: true, getClientRects: () => ({ length: 0 }) })).toBe(false);
    expect(isShown({ isConnected: true, getClientRects: () => ({ length: 1 }) })).toBe(true);
  });
  it("picks the first candidate on screen: the remembered row, else a fallback", () => {
    const remembered = el(false);
    const tree = el(true);
    expect(firstShown([remembered, null, tree])).toBe(tree);
    expect(firstShown([el(true, false)])).toBeNull();
  });
  it("a confirmation in the hidden view does not block the shortcut", () => {
    expect(overlayShown([el(false)])).toBe(false);
    expect(overlayShown([el(false), el(true)])).toBe(true);
    expect(overlayShown([])).toBe(false);
  });
  it("moves focus off a hidden element, leaves the header switch alone", () => {
    expect(needsFocusMove(el(false), false)).toBe(true);
    expect(needsFocusMove(null, true)).toBe(true);
    expect(needsFocusMove(el(true), false)).toBe(false);
  });
});
