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
  type CaptureView,
  type InstallView,
} from "./capture";

const v = (status: CaptureView["status"], extra: Partial<CaptureView> = {}): CaptureView => ({
  status,
  version: null,
  note: null,
  pythonMissing: false,
  takenOver: false,
  setupSupported: true,
  ...extra,
});

describe("chipLabel", () => {
  it("names every state", () => {
    expect(chipLabel(v("vscode"))).toBe("Capturing (VS Code)");
    expect(chipLabel(v("app", { version: "v1.16.1" }))).toBe("Capturing (this app)");
    expect(chipLabel(v("not-set-up"))).toBe("Not capturing — Set up");
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
