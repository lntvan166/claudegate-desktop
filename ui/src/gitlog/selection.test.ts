import { describe, expect, it } from "vitest";
import { rowKey, type RowView } from "./log";
import {
  EMPTY_SELECTION,
  SELECTION_LIMIT,
  applyGesture,
  clickGesture,
  keepListed,
  pickedRows,
  samePicked,
  single,
  type Selection,
} from "./selection";

const order = Array.from({ length: 10 }, (_, i) => `k${i}`);
const one = (k: string): Selection => ({ keys: [k], focus: k, anchor: k });
const click = (s: Selection, key: string, ctrl = false, shift = false) => applyGesture(s, { kind: "click", key, ctrl, shift }, order);

describe("selection", () => {
  it("a plain click selects one commit", () => {
    expect(click(one("k1"), "k3").next).toEqual({ keys: ["k3"], focus: "k3", anchor: "k3" });
  });
  it("Ctrl-click adds and removes, never the last one", () => {
    const added = click(one("k3"), "k5", true).next;
    expect(added).toEqual({ keys: ["k3", "k5"], focus: "k5", anchor: "k5" });
    const removed = click(added, "k5", true).next;
    expect(removed.keys).toEqual(["k3"]);
    expect(removed.focus).toBe("k3");
    expect(click(one("k3"), "k3", true).next).toEqual(one("k3"));
  });
  it("Shift-click selects from the anchor", () => {
    expect(click(one("k3"), "k6", false, true).next).toEqual({ keys: ["k3", "k4", "k5", "k6"], focus: "k6", anchor: "k3" });
    expect(click(one("k3"), "k1", false, true).next.keys).toEqual(["k1", "k2", "k3"]);
  });
  it("Ctrl+Shift-click adds the range", () => {
    const s = click(click(one("k1"), "k5", true).next, "k7", true, true).next;
    expect(s.keys).toEqual(["k1", "k5", "k6", "k7"]);
  });
  it("Shift+arrow extends, a plain arrow goes back to one", () => {
    const ext = applyGesture(one("k3"), { kind: "arrow", key: "k4", shift: true }, order).next;
    expect(ext).toEqual({ keys: ["k3", "k4"], focus: "k4", anchor: "k3" });
    expect(applyGesture(ext, { kind: "arrow", key: "k5", shift: false }, order).next).toEqual(one("k5"));
  });
  it("right-click keeps a selection it is inside, else selects that row", () => {
    const two = click(one("k3"), "k5", true).next;
    expect(applyGesture(two, { kind: "context", key: "k3" }, order).next).toEqual(two);
    expect(applyGesture(two, { kind: "context", key: "k8" }, order).next).toEqual(one("k8"));
  });
  it("more than 100 commits is refused", () => {
    const many = Array.from({ length: 150 }, (_, i) => `m${i}`);
    const start = one("m0");
    const r = applyGesture(start, { kind: "click", key: "m100", ctrl: false, shift: true }, many);
    expect(r.overLimit).toBe(true);
    expect(r.next).toEqual(start);
    const full: Selection = { keys: many.slice(0, SELECTION_LIMIT), focus: "m0", anchor: "m0" };
    const r2 = applyGesture(full, { kind: "click", key: "m120", ctrl: true, shift: false }, many);
    expect(r2.overLimit).toBe(true);
    expect(r2.next).toEqual(full);
    expect(applyGesture(start, { kind: "click", key: "m99", ctrl: false, shift: true }, many).overLimit).toBe(false);
  });
  it("a reload keeps the commits still listed", () => {
    const s: Selection = { keys: ["k2", "k4", "k6"], focus: "k4", anchor: "k2" };
    expect(keepListed(s, ["k2", "k6", "k9"])).toEqual({ keys: ["k2", "k6"], focus: "k2", anchor: "k2" });
    expect(keepListed(s, ["k4"])).toEqual({ keys: ["k4"], focus: "k4", anchor: "k4" });
    expect(keepListed(s, ["k9"])).toEqual(EMPTY_SELECTION);
  });
  it("clickGesture maps Cmd on macOS and Ctrl elsewhere", () => {
    const e = { ctrlKey: true, metaKey: false, shiftKey: true };
    expect(clickGesture(e, "k1", false, true)).toEqual({ kind: "click", key: "k1", ctrl: true, shift: true });
    expect(clickGesture(e, "k1", true, true)).toEqual({ kind: "click", key: "k1", ctrl: false, shift: true });
    expect(clickGesture({ ctrlKey: false, metaKey: true, shiftKey: false }, "k1", true, true)).toMatchObject({ ctrl: true });
    expect(clickGesture(e, "k1", false, false)).toEqual({ kind: "click", key: "k1", ctrl: false, shift: false });
  });
  it("pickedRows keeps list order", () => {
    const rows: RowView[] = ["a", "b", "c"].map((sha) => ({ repo: "acme-web", sha, time: 0, author: "x", email: "", subject: sha, parents: [], source: "" }));
    const s: Selection = { keys: [rowKey(rows[2]), rowKey(rows[0])], focus: null, anchor: null };
    expect(pickedRows(rows, s).map((r) => r.sha)).toEqual(["a", "c"]);
    expect(single("k1")).toEqual(one("k1"));
  });
  it("samePicked sees a replaced row object", () => {
    const a: RowView = { repo: "acme-api", sha: "a", time: 0, author: "x", email: "", subject: "a", parents: [], source: "" };
    expect(samePicked([a], [a])).toBe(true);
    expect(samePicked([a], [{ ...a }])).toBe(false);
    expect(samePicked([a], [])).toBe(false);
  });
});
