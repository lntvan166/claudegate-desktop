import { describe, expect, it } from "vitest";
import {
  CONFIRM_TEXT,
  canRecheck,
  canSetUp,
  capturing,
  chipLabel,
  detail,
  explanation,
  pythonWarning,
  setupResult,
  setupUnsupported,
  canUpdateHook,
  hookLine,
  hookOutdated,
  hookUpdateBlocked,
  hookUpdateResult,
  updateHookConfirm,
  type CaptureView,
  type HookReplacedView,
  type HookView,
  type InstallView,
} from "./capture";

const v = (status: CaptureView["status"], extra: Partial<CaptureView> = {}): CaptureView => ({
  status,
  version: null,
  note: null,
  pythonMissing: false,
  takenOver: false,
  setupSupported: true,
  hook: null,
  ...extra,
});

describe("chipLabel", () => {
  it("names every state", () => {
    expect(chipLabel(v("vscode"))).toBe("Capturing (VS Code)");
    const stale = { hook: { state: "different" as const, installed: null, bundled: "v1.17.0" } };
    expect(chipLabel(v("vscode", stale))).toBe("Capturing (VS Code) · update hook");
    expect(chipLabel(v("vscode", stale), false, true)).toBe("Capturing (VS Code)");
    expect(chipLabel(v("app", { hook: { state: "newer" as const, installed: "v9.0.0", bundled: "v1.17.0" } }))).toBe("Capturing (this app)");
    expect(chipLabel(v("app", { version: "v1.16.1" }))).toBe("Capturing (this app)");
    expect(chipLabel(v("not-set-up"))).toBe("Not capturing — Set up");
    expect(chipLabel(v("not-set-up", { setupSupported: false }))).toBe("Not capturing");
    expect(chipLabel(v("app", { setupSupported: false }))).toBe("Capturing (this app)");
    expect(chipLabel(v("not-set-up"), false, true)).toBe("Not capturing");
    expect(chipLabel(v("settings-unreadable", { note: "x" }))).toBe("Capture: settings unreadable");
    expect(chipLabel(v("python-missing"))).toBe("Capture: python3 not found");
  });
  it("says checking before the first answer, and something for an unknown state", () => {
    expect(chipLabel(null)).toBe("Capture: checking…");
    expect(chipLabel(v("weird" as CaptureView["status"]))).toBe("Capture: unknown");
  });
  it("says the check failed instead of checking forever", () => {
    expect(chipLabel(null, true)).toBe("Capture: couldn't check");
    expect(chipLabel(v("vscode"), true)).toBe("Capturing (VS Code)");
  });
  it("keeps the capturing label when python3 is missing", () => {
    expect(chipLabel(v("vscode", { pythonMissing: true }))).toBe("Capturing (VS Code)");
    expect(chipLabel(v("app", { pythonMissing: true }))).toBe("Capturing (this app)");
  });
});

describe("capturing", () => {
  it("is true only for the two set-up states", () => {
    expect(capturing(v("vscode"))).toBe(true);
    expect(capturing(v("app"))).toBe(true);
    for (const s of ["not-set-up", "settings-unreadable", "python-missing"] as const) {
      expect(capturing(v(s))).toBe(false);
    }
    expect(capturing(null)).toBe(false);
  });
});

describe("explanation", () => {
  it("explains each non-capturing state and nothing else", () => {
    expect(explanation(v("vscode"))).toBeNull();
    expect(explanation(v("app"))).toBeNull();
    expect(explanation(v("not-set-up"))).toMatch(/Claude Code/);
    expect(explanation(v("python-missing"))).toMatch(/python3/);
  });
  it("carries the unreadable reason", () => {
    const e = explanation(v("settings-unreadable", { note: "not valid JSON: line 1" }))!;
    expect(e).toMatch(/~\/\.claude\/settings\.json/);
    expect(e).toMatch(/not valid JSON: line 1/);
  });
});

describe("pythonWarning", () => {
  it("notes a missing python3 next to a capturing state only", () => {
    const w =
      "python3 wasn't found from this app; capture runs under Claude Code's environment, so it may still work";
    expect(pythonWarning(v("vscode", { pythonMissing: true }))).toBe(w);
    expect(pythonWarning(v("app", { pythonMissing: true }))).toBe(w);
    expect(pythonWarning(v("vscode"))).toBeNull();
    expect(pythonWarning(v("python-missing"))).toBeNull();
    expect(pythonWarning(null)).toBeNull();
  });
});

describe("detail", () => {
  it("says the extension took over when it replaced the app's hook", () => {
    expect(detail(v("vscode", { takenOver: true }))).toMatch(/^The VS Code extension now manages the hook\./);
    expect(detail(v("vscode"))).not.toMatch(/now manages/);
  });
  it("does not claim the app keeps the extension's hook updated", () => {
    expect(detail(v("vscode", { takenOver: true }))).toContain("the extension has since replaced its hook, and this app leaves it alone.");
    expect(detail(v("vscode", { takenOver: true }))).not.toMatch(/keeps it up to date/);
  });
  it("says to add a project when there are none, only while capturing", () => {
    const hint = "Add a project to start capturing its edits.";
    expect(detail(v("app"), 0)).toContain(hint);
    expect(detail(v("vscode"), 0)).toContain(hint);
    expect(detail(v("app"), 2)).not.toContain(hint);
    expect(detail(v("app"))).not.toContain(hint);
    expect(detail(v("not-set-up"), 0)).not.toContain(hint);
  });
});

describe("actions", () => {
  it("offers Set up only when not set up and writes are allowed", () => {
    expect(canSetUp(v("not-set-up"), false)).toBe(true);
    expect(canSetUp(v("not-set-up"), true)).toBe(false);
    for (const s of ["vscode", "app", "settings-unreadable", "python-missing"] as const) {
      expect(canSetUp(v(s), false)).toBe(false);
    }
    expect(canSetUp(null, false)).toBe(false);
  });
  it("offers no Set up where core says setup is unsupported", () => {
    expect(canSetUp(v("not-set-up", { setupSupported: false }), false)).toBe(false);
    expect(canSetUp(v("not-set-up", { setupSupported: true }), false)).toBe(true);
    expect(setupUnsupported(v("not-set-up", { setupSupported: false }), false)).toBe(true);
    expect(setupUnsupported(v("not-set-up", { setupSupported: false }), true)).toBe(false);
    expect(setupUnsupported(v("app", { setupSupported: false }), false)).toBe(false);
    expect(setupUnsupported(v("not-set-up"), false)).toBe(false);
    expect(setupUnsupported(null, false)).toBe(false);
  });
  it("offers Check again for the non-capturing states", () => {
    expect(canRecheck(v("python-missing"))).toBe(true);
    expect(canRecheck(v("settings-unreadable"))).toBe(true);
    expect(canRecheck(v("not-set-up"))).toBe(true);
    expect(canRecheck(v("vscode"))).toBe(false);
    expect(canRecheck(v("app"))).toBe(false);
    expect(canRecheck(v("app", { pythonMissing: true }))).toBe(true);
  });
});

describe("confirmation copy", () => {
  it("is the plan's exact text, naming both files", () => {
    expect(CONFIRM_TEXT).toBe(
      "Set up capture? This writes ~/.claudegate/hook.py and hook.sh, and adds one hook to ~/.claude/settings.json (a backup is kept).",
    );
  });
});

describe("setupResult", () => {
  const ok: InstallView = {
    settings: "/h/.claude/settings.json",
    backup: "/h/.claude/settings.json.claudegate-2026-11-18T10-00-00.000Z.bak",
    created: ["/h/.claudegate/hook.py", "/h/.claudegate/hook.sh"],
    version: "v1.16.1",
    note: null,
    adopted: false,
    capture: v("app", { version: "v1.16.1" }),
  };
  const ADOPTED = "The hook.py left by an earlier install was reused.";
  it("says when a leftover hook.py was reused", () => {
    expect(setupResult(ok).text).not.toContain(ADOPTED);
    const r = setupResult({ ...ok, adopted: true });
    expect(r.tone).toBe("success");
    expect(r.text.endsWith(` ${ADOPTED}`)).toBe(true);
    expect(setupResult({ ...ok, adopted: true, note: "a note" }).text.endsWith(` ${ADOPTED}`)).toBe(true);
    expect(setupResult({ ...ok, adopted: true, capture: v("not-set-up") }).text.endsWith(` ${ADOPTED}`)).toBe(true);
  });
  it("is a success naming the backup", () => {
    const r = setupResult(ok);
    expect(r.tone).toBe("success");
    expect(r.text).toContain("Capture is set up");
    expect(r.text).toContain(ok.backup!);
  });
  it("says when there was no settings file to back up", () => {
    expect(setupResult({ ...ok, backup: null }).text).toMatch(/no settings\.json before/);
  });
  it("is a warning when setup carries a note", () => {
    const r = setupResult({ ...ok, note: "settings.json was changed by another program" });
    expect(r.tone).toBe("warning");
    expect(r.text).toContain("changed by another program");
  });
  it("does not repeat the backup sentence when the note already names it", () => {
    const note = `settings.json was changed by another program right after setup. Your pre-setup copy is ${ok.backup}.`;
    const r = setupResult({ ...ok, note });
    expect(r.text.split(ok.backup!).length - 1).toBe(1);
    expect(r.text).not.toContain("Your previous settings are in");
  });
  it("tells a capturing setup with no projects to add one", () => {
    expect(setupResult(ok, 0).text).toContain("Add a project to start capturing its edits.");
    expect(setupResult(ok, 1).text).not.toContain("Add a project");
  });
  it("is a warning when the status read afterwards is not capturing", () => {
    const r = setupResult({ ...ok, capture: v("not-set-up") });
    expect(r.tone).toBe("warning");
  });
});

const hk = (state: HookView["state"], installed: string | null = null): HookView => ({ state, installed, bundled: "v1.17.0" });

describe("the hook line and Update hook", () => {
  it("says which hook is installed and which this app has, for each state", () => {
    expect(hookLine(v("vscode", { hook: hk("different") }))).toBe("Hook: VS Code extension's version · this app has v1.17.0");
    expect(hookLine(v("vscode", { hook: hk("older", "v1.16.1") }))).toBe("Hook: v1.16.1 · this app has v1.17.0");
    expect(hookLine(v("app", { hook: hk("different", "v2.0.0") }))).toBe("Hook: v2.0.0 · this app has v1.17.0");
    expect(hookLine(v("vscode", { hook: hk("missing") }))).toBe("Hook: hook.py is missing · this app has v1.17.0");
    expect(hookLine(v("app", { hook: hk("upToDate") }))).toBe("Hook: v1.17.0, the version bundled in this app");
  });
  it("has no hook line when nothing is registered", () => {
    expect(hookLine(null)).toBeNull();
    expect(hookLine(v("vscode"))).toBeNull();
    for (const s of ["not-set-up", "settings-unreadable", "python-missing"] as const) {
      expect(hookLine(v(s, { hook: hk("different") }))).toBeNull();
    }
  });
  it("offers Update hook… for every state but upToDate", () => {
    for (const st of ["older", "different", "missing"] as const) {
      expect(canUpdateHook(v("vscode", { hook: hk(st, st === "older" ? "v1.16.1" : null) }), false)).toBe(true);
      expect(canUpdateHook(v("app", { hook: hk(st) }), false)).toBe(true);
      expect(hookOutdated(v("vscode", { hook: hk(st) }))).toBe(true);
    }
    expect(canUpdateHook(v("vscode", { hook: hk("upToDate") }), false)).toBe(false);
    expect(canUpdateHook(v("vscode"), false)).toBe(false);
    expect(canUpdateHook(v("not-set-up", { hook: hk("different") }), false)).toBe(false);
    expect(canUpdateHook(null, false)).toBe(false);
  });
  it("hides the button in read-only mode and says why", () => {
    const out = v("vscode", { hook: hk("different") });
    expect(canUpdateHook(out, true)).toBe(false);
    expect(hookUpdateBlocked(out, true)).toBe("Read-only mode is on. Turn it off to update the hook.");
    expect(hookUpdateBlocked(out, false)).toBeNull();
    expect(hookUpdateBlocked(v("vscode", { hook: hk("upToDate") }), true)).toBeNull();
  });
  it("hides the button where core can't write", () => {
    expect(canUpdateHook(v("vscode", { hook: hk("different"), setupSupported: false }), false)).toBe(false);
  });
  it("confirms with the brief's copy, naming the bundled version", () => {
    expect(updateHookConfirm(hk("different"))).toBe(
      "Replace ~/.claudegate/hook.py (the VS Code extension's version, which may be newer than v1.17.0) with the version bundled in this app (v1.17.0)? hook.sh and ~/.claude/settings.json are not changed. The current file is backed up.",
    );
    expect(updateHookConfirm(hk("different", "v1.17.0"))).toBe(
      "Replace ~/.claudegate/hook.py, a modified copy of v1.17.0, with the version bundled in this app (v1.17.0)? hook.sh and ~/.claude/settings.json are not changed. The current file is backed up.",
    );
    expect(updateHookConfirm(hk("older", "v1.16.1"))).toBe(
      "Replace ~/.claudegate/hook.py with the version bundled in this app (v1.17.0)? hook.sh and ~/.claude/settings.json are not changed. The current file is backed up.",
    );
    expect(updateHookConfirm(hk("missing"))).toContain("There is no current file to back up.");
  });
  it("reports the result with the backup path", () => {
    const r = (backup: string | null, after: HookView["state"]): HookReplacedView => ({
      hook: "/h/.claudegate/hook.py",
      backup,
      from: hk("different"),
      to: "v1.17.0",
      appOwnsNow: false,
      capture: v("vscode", { hook: hk(after) }),
    });
    expect(hookUpdateResult(r("/h/.claudegate-desktop/backups/hook/hook-x.py", "upToDate"))).toEqual({
      tone: "success",
      text: "hook.py is now v1.17.0. The previous file is in /h/.claudegate-desktop/backups/hook/hook-x.py.",
    });
    expect(hookUpdateResult(r(null, "upToDate")).text).toBe("hook.py is now v1.17.0. There was no previous file.");
    const w = hookUpdateResult(r("/b.py", "different"));
    expect(w.tone).toBe("warning");
    expect(w.text).toContain("VS Code extension's version");
    expect(w.text).toContain("/b.py");
  });
});

describe("detail with an outdated hook", () => {
  it("does not claim the extension keeps an outdated hook up to date", () => {
    expect(detail(v("vscode", { hook: hk("different") }))).not.toContain("keeps it up to date");
    expect(detail(v("vscode", { hook: hk("upToDate") }))).toContain("The extension keeps it up to date.");
  });
});

describe("a newer hook, a modified copy, and taking the hook back", () => {
  it("says a newer hook is newer and offers no Update", () => {
    const n = v("vscode", { hook: hk("newer", "v1.18.0") });
    expect(hookLine(n)).toBe("Hook: v1.18.0 is newer than this app's v1.17.0");
    expect(hookOutdated(n)).toBe(false);
    expect(canUpdateHook(n, false)).toBe(false);
    expect(hookUpdateBlocked(n, true)).toBeNull();
    expect(detail(n)).toContain("The extension keeps it up to date.");
  });
  it("keeps the button for a modified copy of the bundled version", () => {
    const m = v("app", { hook: hk("different", "v1.17.0") });
    expect(hookLine(m)).toBe("Hook: a modified copy of v1.17.0 · this app has v1.17.0");
    expect(canUpdateHook(m, false)).toBe(true);
  });
  it("says plainly when the app takes a taken-over hook back", () => {
    const r: HookReplacedView = {
      hook: "/h/.claudegate/hook.py",
      backup: "/b.py",
      from: hk("different"),
      to: "v1.17.0",
      appOwnsNow: true,
      capture: v("app", { hook: hk("upToDate") }),
    };
    expect(hookUpdateResult(r).text).toBe(
      "hook.py is now v1.17.0. The previous file is in /b.py. This app will keep this hook up to date until the VS Code extension replaces it.",
    );
  });
});
