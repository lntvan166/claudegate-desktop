import { describe, expect, it } from "vitest";
import type { ProjectView } from "./api";
import {
  DEFAULT_SETTINGS,
  appProtectedList,
  protectedChips,
  removedMessage,
  settingsReloadOnChange,
  type SettingsView,
  ageLabel,
  globMatcher,
  groupsBySession,
  hideExtra,
  protectPrompt,
  protectedAmong,
  sourceLabel,
  type Source,
} from "./settings";

describe("sourceLabel", () => {
  it("names every source", () => {
    const want: Record<Source, string> = {
      app: "This app",
      vsCode: "From VS Code",
      cursor: "From Cursor",
      workspace: "From this project's .vscode",
      default: "Default",
    };
    for (const [s, label] of Object.entries(want)) expect(sourceLabel(s as Source)).toBe(label);
  });
});

describe("globMatcher (port of excludeMatcher.ts)", () => {
  const m = globMatcher("/ws/shop", ["**/.env", "**/.env.*", "**/*.pem", "dist", "secrets/**"]);
  it("matches absolute, root-relative and ancestor paths", () => {
    expect(m("/ws/shop/.env")).toBe(true);
    expect(m("/ws/shop/api/.env.local")).toBe(true);
    expect(m("/ws/shop/certs/server.pem")).toBe(true);
    expect(m("/ws/shop/dist/app.js")).toBe(true); // ancestor dir "dist"
    expect(m("/ws/shop/secrets/a/b.txt")).toBe(true);
  });
  it("does not over-match", () => {
    expect(m("/ws/shop/src/env.ts")).toBe(false);
    expect(m("/ws/shop/src/dist.ts")).toBe(false);
    expect(m("/elsewhere/dist/app.js")).toBe(false);
  });
  it("treats regex characters literally and fails open on nothing", () => {
    expect(globMatcher("/r", ["a+b.(x)"])("/r/a+b.(x)")).toBe(true);
    expect(globMatcher("/r", ["a+b.(x)"])("/r/aab.(x)")).toBe(false);
    expect(globMatcher("/r", [])("/r/.env")).toBe(false);
  });
  it("? is one non-slash character", () => {
    const q = globMatcher("/r", ["id_rs?"]);
    expect(q("/r/id_rsa")).toBe(true);
    expect(q("/r/id_rs/a")).toBe(false);
  });
});

describe("protect prompt", () => {
  it("names a single file", () => {
    expect(protectPrompt(["/ws/.env"])).toBe("This looks like a secret file (.env). Accept anyway?");
  });
  it("lists up to three, then +N more", () => {
    expect(protectPrompt(["/a/.env", "/a/id_rsa"])).toBe("These look like secret files (.env, id_rsa). Accept anyway?");
    expect(protectPrompt(["/a/.env", "/a/id_rsa", "/a/k.pem", "/a/x.key", "/a/.npmrc"])).toBe(
      "These look like secret files (.env, id_rsa, k.pem +2 more). Accept anyway?",
    );
  });
  it("protectedAmong keeps only matches, in order", () => {
    expect(protectedAmong(["/r/a.ts", "/r/.env", "/r/b/.npmrc"], "/r", ["**/.env", "**/.npmrc"])).toEqual([
      "/r/.env",
      "/r/b/.npmrc",
    ]);
  });
});

describe("ageLabel (pendingAge.minDays)", () => {
  const now = Date.parse("2026-11-18T12:00:00Z");
  it("0 or less is off", () => {
    expect(ageLabel("2026-01-01T00:00:00Z", now, 0)).toBeNull();
    expect(ageLabel("2026-01-01T00:00:00Z", now, -2)).toBeNull();
  });
  it("labels rows at least N whole days old", () => {
    expect(ageLabel("2026-11-15T12:00:00Z", now, 3)).toBe("3d");
    expect(ageLabel("2026-11-15T12:00:01Z", now, 3)).toBeNull();
    expect(ageLabel("2026-10-19T12:00:00Z", now, 3)).toBe("30d");
  });
  it("missing, bad and future timestamps", () => {
    expect(ageLabel(null, now, 1)).toBeNull();
    expect(ageLabel("nope", now, 1)).toBeNull();
    expect(ageLabel("2026-12-01T00:00:00Z", now, 1)).toBeNull(); // clamps to 0d, under 1
  });
});

const view: ProjectView = {
  root: "/ws/shop",
  missing: false,
  scopes: [
    { root: "/ws/shop", isWorktree: false, shapeUnknown: [], error: null },
    { root: "/ws/shop/wt", isWorktree: true, shapeUnknown: [], error: null },
  ],
  pending: [
    { path: "/ws/shop/a.ts", rel: "a.ts", scope: "/ws/shop", newFile: false, capturedAt: "2026-11-18T09:00:00Z", sessionId: "s1", added: 1, removed: 0, waiting: false, rejectOutcome: "restore" },
    { path: "/ws/shop/b.ts", rel: "b.ts", scope: "/ws/shop", newFile: false, capturedAt: "2026-11-18T08:00:00Z", sessionId: "s1", added: 1, removed: 0, waiting: false, rejectOutcome: "restore" },
    { path: "/ws/shop/c.ts", rel: "c.ts", scope: "/ws/shop", newFile: false, capturedAt: "2026-11-18T10:00:00Z", sessionId: "s2", added: 1, removed: 0, waiting: false, rejectOutcome: "restore" },
    { path: "/ws/shop/d.ts", rel: "d.ts", scope: "/ws/shop", newFile: false, capturedAt: null, sessionId: null, added: 1, removed: 0, waiting: false, rejectOutcome: "restore" },
    { path: "/ws/shop/wt/e.ts", rel: "wt/e.ts", scope: "/ws/shop/wt", newFile: false, capturedAt: "2026-11-18T07:00:00Z", sessionId: "s1", added: 1, removed: 0, waiting: false, rejectOutcome: "restore" },
  ],
  accepted: [
    { id: "t::/ws/shop/x.ts", path: "/ws/shop/x.ts", rel: "x.ts", scope: "/ws/shop", decidedAt: "2026-11-17T10:00:00Z", sessionId: "s0", added: 1, removed: 0 },
  ],
  rejected: [],
  nextRecheckAt: null,
};

describe("groupsBySession", () => {
  const time = (iso: string) => iso.slice(11, 16);
  it("groups pending by session, newest session first, Unknown last, per scope", () => {
    const g = groupsBySession(view, "pending", time);
    expect(g.map((x) => x.label)).toEqual([
      "Session 2 · 10:00",
      "Session 1 · 08:00",
      "Unknown session",
      "wt · Session 1 · 07:00",
    ]);
    expect(g[1].rows.map((r) => r.path)).toEqual(["/ws/shop/a.ts", "/ws/shop/b.ts"]);
    // Folder actions still act on a real scope; keys are unique per group.
    expect(g.every((x) => x.scope.root === x.rows[0].scope)).toBe(true);
    expect(new Set(g.map((x) => x.key)).size).toBe(g.length);
    expect(g[0].rows[0].group).toBe(g[0].key);
  });
  it("groups records by decidedAt", () => {
    const g = groupsBySession(view, "accepted", time);
    expect(g.map((x) => x.label)).toEqual(["Session 1 · 10:00"]);
  });
});

describe("hideExtra", () => {
  it("drops rows matching the app-only patterns, on every tab", () => {
    const v = hideExtra(view, ["wt/**", "x.ts"]);
    expect(v.pending.map((p) => p.rel)).toEqual(["a.ts", "b.ts", "c.ts", "d.ts"]);
    expect(v.accepted).toEqual([]);
    expect(hideExtra(view, [])).toBe(view);
  });
});

describe("session groups in the visible tree", () => {
  it("ids and collapse keys follow the group key, and reveal finds them", async () => {
    const { visibleItems, collapseKeysToReveal, groupCollapseKey } = await import("./visible");
    const g = groupsBySession(view, "pending", (i) => i.slice(11, 16));
    const items = visibleItems(g, { mode: "list", showGroups: true, tab: "pending", collapsed: { has: () => false } });
    const groups = items.filter((i) => i.kind === "group");
    expect(groups.length).toBe(4);
    expect(new Set(items.map((i) => i.id)).size).toBe(items.length);
    const row = g[1].rows[0];
    const key = groupCollapseKey("pending", g[1].key!);
    expect(collapseKeysToReveal(row, "pending", [key, groupCollapseKey("pending", g[0].key!)])).toEqual([key]);
  });
});

describe("protected chips (a project's .vscode globs added back to an app list)", () => {
  const base = { ...DEFAULT_SETTINGS };
  it("makes the project's own globs non-removable and labels them", () => {
    const s: SettingsView = {
      ...base,
      protected: { value: ["**/.env", "secrets/**"], source: "app" },
      protectedWorkspace: ["secrets/**"],
    };
    expect(protectedChips(s)).toEqual([
      { glob: "**/.env", removable: true, note: null },
      { glob: "secrets/**", removable: false, note: "from this project's .vscode" },
    ]);
  });
  it("saves only the app's own list, never the project's globs", () => {
    const s: SettingsView = {
      ...base,
      protected: { value: ["**/.env", "**/*.pem", "secrets/**"], source: "app" },
      protectedWorkspace: ["secrets/**"],
    };
    expect(appProtectedList(s)).toEqual(["**/.env", "**/*.pem"]);
  });
  it("is not removable at all when the list is not the app's", () => {
    const s: SettingsView = { ...base, protected: { value: ["**/.env"], source: "vsCode" }, protectedWorkspace: [] };
    expect(protectedChips(s)).toEqual([{ glob: "**/.env", removable: false, note: null }]);
  });
});

describe("settingsReloadOnChange", () => {
  it("reloads on a relist (empty roots) and on a change touching the selected project", () => {
    expect(settingsReloadOnChange([], "/a")).toBe(true);
    expect(settingsReloadOnChange([], null)).toBe(true);
    expect(settingsReloadOnChange(["/b", "/a"], "/a")).toBe(true);
  });
  it("leaves it alone for other projects' changes", () => {
    expect(settingsReloadOnChange(["/b"], "/a")).toBe(false);
    expect(settingsReloadOnChange(["/b"], null)).toBe(false);
  });
});

describe("removedMessage", () => {
  it("says the history is kept when the root was unregistered", () => {
    expect(removedMessage("shop", true)).toBe("shop removed; its review history is kept");
  });
  it("words the forget-only case", () => {
    expect(removedMessage("shop", false)).toBe(
      "shop is no longer managed here. VS Code also uses this folder, so capture stays on and it stays in your list.",
    );
  });
});
