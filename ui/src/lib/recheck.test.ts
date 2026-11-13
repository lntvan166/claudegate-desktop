import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RECHECK_SLACK_MS, Rechecks, recheckDelay } from "./recheck";

describe("recheckDelay", () => {
  it("is null when nothing is waiting", () => {
    expect(recheckDelay(null, 1000)).toBeNull();
    expect(recheckDelay(undefined, 1000)).toBeNull();
  });
  it("waits until the deadline plus slack, never negative", () => {
    expect(recheckDelay(5000, 1000)).toBe(4000 + RECHECK_SLACK_MS);
    expect(recheckDelay(500, 1000)).toBe(RECHECK_SLACK_MS);
  });
});

describe("Rechecks", () => {
  beforeEach(() => vi.useFakeTimers({ now: 0 }));
  afterEach(() => vi.useRealTimers());

  it("fires once per root at its deadline", () => {
    const fired: string[] = [];
    const r = new Rechecks((root) => fired.push(root));
    r.schedule("/a", 1000);
    r.schedule("/b", null);
    vi.advanceTimersByTime(1000 + RECHECK_SLACK_MS - 1);
    expect(fired).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(fired).toEqual(["/a"]);
    vi.advanceTimersByTime(60_000);
    expect(fired).toEqual(["/a"]);
  });

  it("a new deadline replaces the old one, and null cancels it", () => {
    const fired: string[] = [];
    const r = new Rechecks((root) => fired.push(root));
    r.schedule("/a", 1000);
    r.schedule("/a", 5000);
    vi.advanceTimersByTime(2000);
    expect(fired).toEqual([]);
    r.schedule("/a", null);
    vi.advanceTimersByTime(10_000);
    expect(fired).toEqual([]);
  });

  it("clear cancels everything", () => {
    const fired: string[] = [];
    const r = new Rechecks((root) => fired.push(root));
    r.schedule("/a", 1000);
    r.schedule("/b", 2000);
    r.clear();
    vi.advanceTimersByTime(10_000);
    expect(fired).toEqual([]);
  });
});
