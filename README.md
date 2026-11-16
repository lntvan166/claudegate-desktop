# ClaudeGate Desktop

A standalone desktop reviewer for the session files written by the ClaudeGate VS Code extension. It reads `~/.claudegate/sessions/`, shows the pending, accepted and rejected changes Claude Code has made, and lets you decide them, without needing an editor open.

It writes only when you make a decision (see Decisions). Browsing never writes, and the read-only switch turns decisions off.

## Running

```bash
npm run tauri dev          # the desktop app against your real ~/.claudegate
npm --prefix ui run dev    # browser preview with fixture data, no Tauri
```

Point the app at a sandbox instead of your real data with `CLAUDEGATE_HOME`:

```bash
CLAUDEGATE_HOME=/tmp/cg-sandbox npm run tauri dev
```

Headless (no window on your desktop):

```bash
env -u WAYLAND_DISPLAY GDK_BACKEND=x11 xvfb-run -a <command>
```

## Decisions

Each action works on one file, one folder or the whole project, in the same way as the VS Code extension.

- **Accept** moves a pending change to Accepted and keeps Claude's version on disk.
- **Reject** puts the original back on disk and moves the change to Rejected. A file Claude created is deleted. A file with no recorded original that isn't known to be Claude's new file is left on disk and reported as left, since there is nothing safe to restore.
- **Revert** (on an Accepted row) takes the change out of Accepted and makes it pending again, with the old original as its baseline.
- **Reapply** (on a Rejected row) writes Claude's rejected version back to disk and makes it pending again.
- **Clear** empties the Accepted or Rejected log. It does not touch your files.

Folder and project-wide accept and reject skip excluded files and files with no real change. A single-file action applies to any pending row.

**Undo.** After accepting or rejecting a single file, Undo reverses it: an accept is reverted, a reject is reapplied, so the file holds Claude's version again. Bulk and folder actions have no Undo.

**Backups.** Before every write to a session file, the app copies the current one to `~/.claudegate-desktop/backups/<session hash>/<UTC timestamp>.json` and keeps the newest 50 per project. If the backup fails, the write is refused.

**Read-only mode.** Decisions are refused, with a message, in three cases:

- the read-only switch is on (`readOnly` in `~/.claudegate-desktop/config.json`, off by default);
- a project's session was written in a format this version doesn't know (a newer ClaudeGate), so the app won't risk rewriting it;
- the settings file or a session file is unreadable or invalid. Fix or delete it.

**Alongside VS Code.** The app and the extension can run at once. Both take the same `<session>.lock`, re-read the file just before writing and merge in what the other (and the Claude hook) wrote in between, so a capture made while you decide is kept. `scripts/decide-e2e.sh` checks this headless in a temp sandbox: it runs the extension's hook during an accept and asserts the capture survives (it prints `E2E OK`).

### Proving it is read-only

`scripts/readonly-proof.sh [--no-app] [--bin PATH] [--print-bin] [HOME_DIR] [SECONDS]` snapshots every file under the ClaudeGate home (default `~/.claudegate`), runs the built binary for SECONDS (default 60), snapshots again, and prints `UNCHANGED` (exit 0) or each changed, added or removed file with its last `hook.log` mention (exit 1). The extension's hook may legitimately write in that window; the log line shows who. The binary is `--bin PATH`, else the newer of `target/release` and `target/debug`; the script prints its path, build time and the current `HEAD`, and warns when any file under `core/src`, `src-tauri/src` or `ui/src` is newer than it (`--print-bin` prints just that and exits). `--no-app` skips the launch; set `READONLY_PROOF_HEADLESS=1` to run the app under xvfb. Snapshots live in a temp dir, never inside the home. A static check, `core/tests/no_writes.rs`, also fails `cargo test` if any source file outside `src-tauri/src/config.rs` uses a write API.
