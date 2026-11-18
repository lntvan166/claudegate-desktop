# ClaudeGate Desktop

A standalone desktop reviewer for the session files written by the ClaudeGate VS Code extension. It reads `~/.claudegate/sessions/`, shows the pending, accepted and rejected changes Claude Code has made, and lets you decide them, without needing an editor open.

It writes when you make a decision, when you manage projects and their settings, and on its own to keep up the projects it added (see "What the app writes" below). Browsing is not write-free: loading or watching a project the app added runs upkeep on its session. The read-only switch blocks every write to the files shared with the extension except two that only maintain projects you already added.

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

**Backups.** Before a write to a session file, the app copies the current one to `~/.claudegate-desktop/backups/<session hash>/<UTC timestamp>.json` and keeps the newest 50 per project. If the backup fails, the write is refused. One exception: an upkeep write takes no backup when all it does is prune settled no-op entries (the file on disk equals the recorded baseline, or a file Claude was to create never appeared, so nothing is lost) and/or set the `excludeCleanupDone` latch. Upkeep that drops an excluded or out-of-workspace entry, or trims the accepted log, is backed up like a decision. Without the exception, routine upkeep would rotate the decision backups out of the 50 slots within hours.

**Read-only mode.** Decisions are refused in these cases, and the app hides the actions and shows a banner:

- the read-only switch is on (`readOnly` in `~/.claudegate-desktop/config.json`, off by default). The banner reads "Read-only mode is on" with a Turn off button; a refused write says "ClaudeGate Desktop is in read-only mode. Turn it off to make decisions."
- a project's session was written in a format this version doesn't know, so the app won't risk rewriting it: "This project was written by a newer ClaudeGate. Update the app to make decisions here."
- the settings file is unreadable or invalid: "ClaudeGate Desktop couldn't read its settings file (~/.claudegate-desktop/config.json), so decisions are paused. Fix or delete that file."
- a project's session file is unreadable or invalid: "This project's review log couldn't be read, so decisions are paused."

**Alongside VS Code.** The app and the extension can run at once. Both take the same `<session>.lock`, re-read the file just before writing and merge in what the other (and the Claude hook) wrote in between, so a capture made while you decide is kept. `scripts/decide-e2e.sh` checks this headless in a temp sandbox: it runs the extension's hook during an accept and asserts the capture survives (it prints `E2E OK`).

### What the app writes

Every write the app can make, and whether the read-only switch blocks it. "Shared" files live in the ClaudeGate home (`~/.claudegate`) and are also written by the VS Code extension and its hook. An unreadable `config.json` counts as read-only for every row marked "Yes".

| Write | File(s) | Blocked by read-only? | Why |
|---|---|---|---|
| Decisions (accept, reject, revert, reapply, clear, Undo) | the project's session file (shared); the workspace files rejected or reapplied; a backup | Yes | The switch exists to stop these. |
| Add or Remove project | `workspace-roots.json` and `excludes.json` (shared); `config.json` | Yes | They change what the hook captures for every ClaudeGate. |
| Set a project's exclude list | `excludes.json` (shared); `config.json` | Yes | The hook reads it. |
| Take back an exclude list from VS Code | `excludes.json` (shared); `config.json` | Yes | Overwrites what VS Code wrote. |
| Hook-log toggle | `hooklog.enabled` (shared) | Yes | Turns logging on or off for the extension too. |
| App settings (overrides, extra hide patterns, hidden projects, editor, path style, the read-only switch itself) | `config.json` | No | The app's own file, never read by the extension; the switch must be able to turn itself off. |
| Watcher re-add of the app's own roots | `workspace-roots.json` (shared) | No | The extension's unlocked rewrite can drop a root the app added; putting it back restores capture you already chose. It writes only when one of the app's roots is missing. |
| Exclude sync, at startup and on every change of `excludes.json` or `config.json` | `excludes.json` (shared); `config.json` (`excludeWritten`, `excludeWrittenPrev`, `excludeWrittenAt`, `managedByVsCode`) | No | Maintains the ownership you chose when you added the project, and records a VS Code takeover. It compares first and writes only when something changed. |
| Upkeep of the app's own projects, after loading one and on its session events | the project's session files (shared), and a backup unless the exception above applies | Yes | Only for projects the app added and VS Code has not taken over, and never while VS Code's or Cursor's `storage.json` shows the folder open (their own extension keeps that session). |
| Backups | `~/.claudegate-desktop/backups/` | Follows the write it precedes | Taken only before a session write that is itself allowed. |
| Set up capture: hook files | `hook.py` and `hook.sh` (mode 0755) in `~/.claudegate` (shared) | Yes | They are what Claude Code runs before every edit. Created only, never replaced: an existing `hook.py` (the extension's) refuses setup, and an existing `hook.sh` is kept only when it is byte-for-byte the extension's wrapper. A failed setup removes only the files it created. |
| Set up capture: marker | `~/.claudegate/.installed-by-desktop` (shared folder), holding the bundled hook's version (e.g. `v1.16.1`) | Yes | Records that this app, not the extension, set capture up. |
| Set up capture: Claude Code settings | `~/.claude/settings.json`, at its realpath (a symlink keeps its link) | Yes | Adds one `PreToolUse` entry and keeps every other key and hook. Only when the file is absent or a plain JSON object (no comments) and no ClaudeGate entry exists. The new content is written to a temp file and synced; then, immediately before the rename, setup re-reads the file and refuses if anything changed since it first read it. Afterwards it reads the file back: if the bytes are still setup's and fail the check, the backup is restored byte for byte (or a file setup created is removed), again comparing right before that rename. If another program has rewritten the file in the meantime, it is left as that program wrote it; setup counts as done when the file still has a ClaudeGate entry (the same test the status uses) and keeps the hook files then, or when the file cannot be read back. **Known limit:** the file has no lock, so a write by Claude Code between that last re-read and the rename syscall (microseconds) is replaced by setup's; the backup holds the file as it was before setup. |
| Set up capture: settings backups | `~/.claude/settings.json.claudegate-<UTC ISO time>.bak` | Follows the write it precedes | The extension's name and its ring: both keep the newest 5, pruned only after a setup succeeds. A failed backup refuses the settings write. The older `settings.json.claudegate.bak` is never pruned. |
| `config.json` | `~/.claudegate-desktop/config.json` | No | Created with the defaults on first launch when missing, and rewritten by every row above that records something in it. Never inside the ClaudeGate home. |

So with read-only on, the app can still write `workspace-roots.json` (re-add) and `excludes.json` (sync) for the projects it added, and its own `config.json`. With no app-added projects, neither of those has anything to do.

**Known limit: a lost write with the extension.** The extension writes `workspace-roots.json` and `excludes.json` without a lock. The app re-reads each file right before its atomic rename and starts over if it changed, but a write by the extension inside the few microseconds between that check and the rename can still be lost (or the extension, writing from a stale read, can drop the app's). The re-add and exclude sync above repair the app's side on the next change event. Session files do not have this gap: both sides take `<session>.lock`.

### Checking what it writes

`scripts/readonly-proof.sh [--no-app] [--bin PATH] [--print-bin] [HOME_DIR] [SECONDS]` snapshots every file under the ClaudeGate home (default `~/.claudegate`), runs the built binary for SECONDS (default 60), snapshots again, and prints `UNCHANGED` (exit 0) or each changed, added or removed file with its last `hook.log` mention (exit 1). The extension's hook may legitimately write in that window; the log line shows who. The binary is `--bin PATH`, else the newer of `target/release` and `target/debug`; the script prints its path, build time and the current `HEAD`, and warns when any file under `core/src`, `src-tauri/src` or `ui/src` is newer than it (`--print-bin` prints just that and exits). `--no-app` skips the launch; set `READONLY_PROOF_HEADLESS=1` to run the app under xvfb. Snapshots live in a temp dir, never inside the home.

The proof sees the app's own writes too, not only the hook's: on a home where the app has added projects, a run can report the startup exclude sync, a re-added root or upkeep of those projects' sessions (see "What the app writes"). Expect `UNCHANGED` only on a home where the app owns no projects, or where everything is already in sync and nothing needs upkeep.

A static check, `core/tests/no_writes.rs`, also fails `cargo test` if any source file uses a write API outside its allow-list: `src-tauri/src/config.rs` (the app's `~/.claudegate-desktop`), `core/src/store.rs` (session files, locks, temp files, backups, `workspace-roots.json`, `excludes.json`, the hook-log sentinel, and capture setup's hook files, marker, `~/.claude/settings.json` and its backups) and `core/src/workspace_write.rs` (workspace files on reject and reapply). A trailing `#[cfg(test)]` module is not scanned.
