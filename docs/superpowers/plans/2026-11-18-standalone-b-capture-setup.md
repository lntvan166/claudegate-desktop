# ClaudeGate Desktop — Standalone B: Set Up Capture

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On a machine without the VS Code extension, the app installs the ClaudeGate capture hook itself. Where the extension's hook is already set up, the app changes nothing.

**Architecture:** The app bundles `hook.py` and `hook.sh` from the pinned extension tag at build time. A pure detector reads `~/.claude/settings.json` and reports the capture status. A guarded installer ports the extension's settings.json protocol:
1. back up with a timestamp;
2. write through the realpath;
3. verify;
4. roll back if verification fails.

The UI shows a "Capture" status and a "Set up capture" flow with a confirmation step.

**Tech Stack:** As before.

**Spec:** the addendum "Hook setup", in `docs/superpowers/specs/2026-11-13-desktop-reviewer-design.md`.

## Global Constraints

- **Reference implementation (read-only):** the extension at v1.16.1, `src/hookInstaller.ts`. Port these parts: `computeSettingsPatch`, `backupsToPrune`, `verifySettingsContent`, the write → verify → restore flow, and the backup keep count.
- **Constants:**
  - matcher exactly `^(Write|Edit|MultiEdit|Bash)$`
  - wrapper path `~/.claudegate/hook.sh`, content `#!/usr/bin/env bash\npython3 "$HOME/.claudegate/hook.py"`, mode 0755
- **Never install when a ClaudeGate hook entry already exists.** That means a PreToolUse command ending in `/.claudegate/hook.sh` or `hook.bat`, in any matcher. Report it as "set up by VS Code", or as "set up by this app" when the marker `~/.claudegate/.installed-by-desktop` exists.
- **Never touch other hooks or keys in `~/.claude/settings.json`.** Merge into the PreToolUse array only.
- **Updating `hook.py`:** only when the marker exists, and only when the bundled version is newer. The version is the `CLAUDEGATE_HOOK_VERSION` line the build script stamps into the bundled copy. Never update a hook the extension installed.
- **Confirmation first.** Installing requires an explicit confirmation step that names both files that will change.
- **Writers:** the new writes live in `core/src/store.rs` (or a new `core/src/hook_install.rs`, if `no_writes` ALLOWED is extended with a ledgered ruling). The settings.json write is guarded to `~/.claude/settings.json`'s realpath; the hook files are guarded to `~/.claudegate/`.
- **Tests** use a temp `HOME`. Never touch the real `~/.claude`.

## Review Focus

1. **Symlinked settings.json** (dotfiles via chezmoi or bifrost). Write through the realpath and keep the symlink.
2. **settings.json with comments or invalid JSON.** Refuse to install and say why. Never overwrite it.
3. **A verification failure after the write.** Restore the backup byte for byte.
4. **The extension installs its hook after the app did.** The app sees "set up by VS Code" when the marker's hook files have been replaced. It must not fight the extension's sync.
5. **`python3` missing.** The status says capture can't run, and installing is refused.

---

### Task 1: Bundled hook and capture status

**Files:**
- Create: `src-tauri/build-hook.sh`, or a `build.rs` step that copies `.xcheck-cache/claudegate/hooks/hook.py` at the pinned tag into `src-tauri/resources/hook.py` and stamps `CLAUDEGATE_HOOK_VERSION=<tag>`
- Create: `core/src/capture.rs` (pure), `pub enum CaptureStatus { SetUpByExtension, SetUpByApp { version: String }, NotSetUp, SettingsUnreadable(String), PythonMissing }`, and `pub fn capture_status(settings_text: Option<&str>, marker: bool, python3: bool) -> CaptureStatus`
- Test: `core/tests/capture.rs`

- [ ] **Step 1: Write failing tests** for each status:
  - a hook entry under any matcher → `SetUpByExtension`;
  - with the marker → `SetUpByApp`;
  - no hooks key → `NotSetUp`;
  - JSON comments or invalid JSON → `SettingsUnreadable`;
  - python3 missing → `PythonMissing`.
- [ ] **Step 2:** Run the tests and confirm they FAIL. **Step 3:** Implement, then commit the resource file (`hook.py` from the tag). Confirm that `xcheck`'s fetch provides it. **Step 4:** PASS.
- [ ] **Step 5: Commit** with the message `feat: bundled capture hook and capture status`.

### Task 2: Guarded installer

**Files:**
- Create: the installer in `core/src/store.rs` as `pub(crate) fn install_capture(home: &Path, bundled_hook: &str) -> Result<InstallReport, InstallError>`, or in `hook_install.rs` with a ledgered `no_writes` ruling
- Test: `core/tests/install_capture.rs`

**Interfaces:**
- Order of operations:
  1. Re-check the status, and refuse unless it is `NotSetUp`.
  2. Write `hook.py` and `hook.sh` atomically, with `hook.sh` at mode 0755.
  3. Write the marker.
  4. Back up `settings.json` to `settings.json.claudegate-backup-<stamp>`, keeping the newest N (the extension's count).
  5. Compute the merged JSON with the ClaudeGate PreToolUse entry appended.
  6. Write it atomically to the realpath.
  7. Re-read the file and verify it with the ported `verifySettingsContent`.
  8. On failure, restore the backup and remove the hook files and marker the app just wrote.
- [ ] **Step 1: Write failing tests:**
  - a fresh install produces the correct entry and keeps every other key and hook;
  - a symlinked settings file keeps its link, and the target is updated;
  - a missing settings file gets a new one that contains only the hooks block;
  - a forced verification failure restores the backup byte for byte and leaves no hook files behind;
  - a second install is refused (already set up);
  - the backup keep count is pruned.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** with the message `feat: guarded capture setup, the extension's settings protocol`.

### Task 3: Capture UI, and updating the app's own hook

**Files:**
- Modify `src-tauri`:
  - add the commands `capture_status()` and `set_up_capture()`;
  - on start, if the marker exists and the bundled version is newer, update `hook.py` atomically. Never do this when the status is `SetUpByExtension`.
- Modify `ui`:
  - a "Capture" chip in the header. It shows "Capturing (VS Code)", "Capturing (this app)", or "Not capturing — Set up". When the settings file is unreadable or python3 is missing, it shows an explanation instead.
  - a confirmation panel for setup: "Set up capture? This writes ~/.claudegate/hook.py and hook.sh, and adds one hook to ~/.claude/settings.json (a backup is kept)." with the buttons [Set up] [Cancel].
  - After setup, show the result with the backup path.
- Test: `src-tauri/tests/capture_cmd.rs`; add the UI pure functions to `ui/src/lib/capture.test.ts`.

- [ ] **Step 1: Write failing tests.** Cover every chip label. Check that the update runs only for the app's own hook and only when its version is newer. Check that setup is refused unless the status is `NotSetUp`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. Write a sandbox E2E script, `scripts/capture-e2e.sh`, run with a temp `HOME`:
  1. Set up capture.
  2. Feed `hook.py` a Write payload for a file in an app-added project.
  3. Check that a pending entry appears in that project's session.
- [ ] **Step 5: Commit** with the message `feat: capture status and setup in the app`.
