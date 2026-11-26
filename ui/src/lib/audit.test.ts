import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AuditRefresher,
  EMPTY_AUDIT,
  missedHeader,
  relPath,
  restorePaths,
  rove,
  scopeTag,
  statsLine,
  withoutPaths,
  type AuditItem,
  type AuditView,
} from "./audit";

const item = (path: string, root: string, atMs = 1000): AuditItem => ({ path, atMs, commandHead: "sed -i", root });
const ROOT = "/w/acme-api";
const WT = "/w/acme-api/.worktrees/x";

describe("missedHeader", () => {
  it("is null with nothing missed", () => expect(missedHeader(0)).toBeNull());
  it("is singular for one", () => expect(missedHeader(1)).toBe("1 file Claude changed wasn't captured"));
  it("is plural otherwise", () => expect(missedHeader(3)).toBe("3 files Claude changed weren't captured"));
});

describe("statsLine", () => {
  const s = { ...EMPTY_AUDIT.stats };
  it("keeps captured even at zero and omits other zero terms", () => {
    expect(statsLine(s)).toBe("Last 7 days: 0 captured");
    expect(statsLine({ ...s, captured: 40, missed: 2, implicit: 5 })).toBe("Last 7 days: 40 captured · 2 missed · 5 implicit");
    expect(statsLine({ ...s, captured: 1, missed: 1, phantom: 3, implicit: 4 })).toBe(
      "Last 7 days: 1 captured · 1 missed · 3 phantom · 4 implicit",
    );
  });
});

describe("relPath", () => {
  const scopes = [ROOT, WT];
  it("is relative to the item's own scope", () => {
    expect(relPath(item(`${ROOT}/src/x.go`, ROOT), scopes)).toBe("src/x.go");
    expect(relPath(item(`${WT}/src/x.go`, WT), scopes)).toBe("src/x.go");
  });
  it("uses the deepest scope that holds the path", () => {
    expect(relPath(item(`${WT}/a.go`, ROOT), scopes)).toBe("a.go");
  });
  it("falls back to the full path", () => expect(relPath(item("/elsewhere/a.go", ROOT), scopes)).toBe("/elsewhere/a.go"));
  it("tags a worktree scope, not the project's own", () => {
    expect(scopeTag(item(`${WT}/a.go`, WT), ROOT, [ROOT, WT])).toBe("x");
    expect(scopeTag(item(`${ROOT}/a.go`, ROOT), ROOT, [ROOT, WT])).toBeNull();
  });
});

describe("dismiss", () => {
  const v: AuditView = {
    ...EMPTY_AUDIT,
    missed: [item(`${ROOT}/a.go`, ROOT), item(`${ROOT}/b.go`, ROOT)],
    uncertain: [item(`${ROOT}/c.go`, ROOT)],
  };
  it("removes the rows at once, from both lists", () => {
    const n = withoutPaths(v, [`${ROOT}/a.go`, `${ROOT}/c.go`]);
    expect(n.missed.map((i) => i.path)).toEqual([`${ROOT}/b.go`]);
    expect(n.uncertain).toEqual([]);
    expect(v.missed).toHaveLength(2);
  });
  it("restores them in their old order after a failure, keeping newer rows", () => {
    const n = withoutPaths(v, [`${ROOT}/a.go`]);
    const fresh = { ...n, missed: [...n.missed, item(`${ROOT}/z.go`, ROOT)] };
    const back = restorePaths(fresh, v, [`${ROOT}/a.go`]);
    expect(back.missed.map((i) => i.path)).toEqual([`${ROOT}/a.go`, `${ROOT}/b.go`, `${ROOT}/z.go`]);
  });
});

describe("rove", () => {
  it("clamps at both ends", () => {
    expect(rove(0, "ArrowUp", 3)).toBe(0);
    expect(rove(2, "ArrowDown", 3)).toBe(2);
    expect(rove(1, "ArrowDown", 3)).toBe(2);
    expect(rove(2, "Home", 3)).toBe(0);
    expect(rove(0, "End", 3)).toBe(2);
    expect(rove(0, "End", 0)).toBe(0);
  });
});

describe("AuditRefresher", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  const mk = (work: () => Promise<void>) => new AuditRefresher(work, 2000, () => Date.now());

  it("runs the first request at once and coalesces a burst into one trailing run", async () => {
    const run = vi.fn(async () => {});
    const r = mk(run);
    r.request();
    r.request();
    r.request();
    await vi.advanceTimersByTimeAsync(0);
    expect(run).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1999);
    expect(run).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2);
    expect(run).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(run).toHaveBeenCalledTimes(2);
  });

  it("never has two reads in flight", async () => {
    let open = 0;
    let max = 0;
    let release: () => void = () => {};
    const run = vi.fn(
      () =>
        new Promise<void>((res) => {
          open++;
          max = Math.max(max, open);
          release = () => {
            open--;
            res();
          };
        }),
    );
    const r = mk(run);
    r.request();
    r.request(true);
    r.request(true);
    expect(run).toHaveBeenCalledTimes(1);
    release();
    await vi.advanceTimersByTimeAsync(2100);
    expect(run).toHaveBeenCalledTimes(2);
    release();
    expect(max).toBe(1);
  });

  it("an immediate request made during a read runs right after it, not 2 s later", async () => {
    let release: () => void = () => {};
    const run = vi.fn(() => new Promise<void>((res) => (release = res)));
    const r = mk(run);
    r.request();
    r.request(true);
    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(run).toHaveBeenCalledTimes(2);
    release();
  });

  it("immediate skips the interval", async () => {
    const run = vi.fn(async () => {});
    const r = mk(run);
    r.request();
    await vi.advanceTimersByTimeAsync(0);
    r.request(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(run).toHaveBeenCalledTimes(2);
  });

  it("does not poll: no request, no timer", async () => {
    const run = vi.fn(async () => {});
    const r = mk(run);
    r.request();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(run).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("survives a failing read and stops cleanly", async () => {
    const run = vi.fn(async () => {
      throw new Error("x");
    });
    const r = mk(run);
    r.request();
    await vi.advanceTimersByTimeAsync(0);
    r.request();
    r.stop();
    await vi.advanceTimersByTimeAsync(5000);
    expect(run).toHaveBeenCalledTimes(1);
  });
});
