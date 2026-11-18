// The Capture chip's words and choices. Mirrors src-tauri/src/capture.rs (`CaptureView`,
// `InstallView`); kept pure so every label and branch is tested.

export type CaptureStatus = "vscode" | "app" | "not-set-up" | "settings-unreadable" | "python-missing";

export type CaptureView = {
  status: CaptureStatus;
  /** The marker's version, for `app`. */
  version: string | null;
  /** Why, for `settings-unreadable`. */
  note: string | null;
  /** For `vscode`/`app`: python3 wasn't found from this app (a warning; capture may still work). */
  pythonMissing: boolean;
  /** For `vscode`: this app set capture up and the extension has since replaced the hook. */
  takenOver: boolean;
};

export type InstallView = {
  settings: string;
  backup: string | null;
  created: string[];
  version: string;
  /** Setup worked, but something needs attention: shown as a warning. */
  note: string | null;
  /** The status read right after setup. */
  capture: CaptureView;
};

/** The plan's exact copy; it names both files setup changes. */
export const CONFIRM_TEXT =
  "Set up capture? This writes ~/.claudegate/hook.py and hook.sh, and adds one hook to ~/.claude/settings.json (a backup is kept).";

const LABELS: Record<CaptureStatus, string> = {
  vscode: "Capturing (VS Code)",
  app: "Capturing (this app)",
  "not-set-up": "Not capturing — Set up",
  "settings-unreadable": "Capture: settings unreadable",
  "python-missing": "Capture: python3 not found",
};

/** `failed`: the status could not be read at all (shown instead of "checking…" forever). */
export function chipLabel(v: CaptureView | null, failed = false): string {
  if (!v) return failed ? "Capture: couldn't check" : "Capture: checking…";
  return LABELS[v.status] ?? "Capture: unknown";
}

export function capturing(v: CaptureView | null): boolean {
  return v?.status === "vscode" || v?.status === "app";
}

/** What the popover says about the state. */
export function detail(v: CaptureView): string {
  switch (v.status) {
    case "vscode":
      if (v.takenOver) {
        return "The VS Code extension now manages the hook. This app set capture up first; the extension has since replaced its hook, and keeps it up to date.";
      }
      return "The ClaudeGate VS Code extension set up the hook, so Claude Code's edits are captured. The extension keeps it up to date.";
    case "app":
      return `This app set up the hook${v.version ? ` (${v.version})` : ""}, so Claude Code's edits are captured. The app updates its own hook when it starts.`;
    default:
      return explanation(v) ?? "";
  }
}

/** The short explanation for a state that is not capturing; null while capturing. */
export function explanation(v: CaptureView | null): string | null {
  switch (v?.status) {
    case "not-set-up":
      return "Claude Code isn't running the ClaudeGate hook, so its edits in your projects aren't captured for review.";
    case "settings-unreadable":
      return `~/.claude/settings.json can't be read as plain JSON${v.note ? ` (${v.note})` : ""}. The app won't change it; fix the file, then check again.`;
    case "python-missing":
      return "The hook runs with python3, which wasn't found on this computer. Install Python 3, then check again.";
    default:
      return null;
  }
}

export function canSetUp(v: CaptureView | null, readOnly: boolean): boolean {
  return v?.status === "not-set-up" && !readOnly;
}

export function canRecheck(v: CaptureView | null): boolean {
  return !!v && (!capturing(v) || v.pythonMissing);
}

/** The note next to "Capturing (…)" when this app could not find python3. */
export function pythonWarning(v: CaptureView | null): string | null {
  return capturing(v) && v?.pythonMissing
    ? "python3 wasn't found from this app; capture runs under Claude Code's environment, so it may still work"
    : null;
}

export type SetupResult = { tone: "success" | "warning"; text: string };

export function setupResult(r: InstallView): SetupResult {
  const backup = r.backup ? `Your previous settings are in ${r.backup}.` : "There was no settings.json before setup.";
  if (r.note) return { tone: "warning", text: `Capture is set up, with a warning: ${r.note} ${backup}` };
  if (!capturing(r.capture)) {
    return { tone: "warning", text: `Setup finished, but the status now reads “${chipLabel(r.capture)}”. ${backup}` };
  }
  return { tone: "success", text: `Capture is set up. ${backup}` };
}
