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

## Installing and updating

Installers are on [GitHub Releases](https://github.com/lntvan166/claudegate-desktop/releases): a `.deb` and an AppImage for Linux, and a universal `.dmg` for macOS. They are not code-signed, so on macOS right-click the app and choose Open the first time.

The app checks the latest release 5 seconds after it starts and then every six hours. When a newer version is out, an "Update" chip appears in the header:

- **macOS and AppImage:** "Install and restart" downloads the update, checks its signature against the public key in `tauri.conf.json`, and replaces the app.
- **`.deb`:** the app never touches a package dpkg owns. It shows a `curl … && pkexec apt install` command to copy.

A failed check (offline) shows nothing. The update check writes no ClaudeGate state.

**Releasing:** bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json` and `ui/package.json`, then push a `v<version>` tag. `.github/workflows/release.yml` builds and signs the installers and `latest.json`, and leaves a draft release to publish. Signing uses the `TAURI_SIGNING_PRIVATE_KEY` secret. Its only copies are that secret and the age-encrypted backup in bifrost (`~/.tauri/claudegate-desktop.key`). If the key is lost, installed apps can no longer verify an update.

**Bumping the bundled hook.** The app ships the extension's `hooks/hook.py` from the tag pinned as `EXT_TAG` in `xcheck/fetch-extension.sh`, committed as `src-tauri/resources/hook.py` with `# CLAUDEGATE_HOOK_VERSION=<tag>` as its second line. After an extension release, run `scripts/bump-hook.sh v1.17.0`: it changes `EXT_TAG`, re-fetches `.xcheck-cache/claudegate` at that tag, regenerates `resources/hook.py` with the stamp, and runs the xcheck, which fails unless the bundled hook is the tag's hook plus that one line. Then run `cargo test --workspace` and `bash scripts/capture-e2e.sh`, and commit `xcheck/fetch-extension.sh` and `src-tauri/resources/hook.py`. If the xcheck reports a port that drifted (the settings patch, say), update it in `core/src/capture.rs` first.

## From the terminal and Zed

The `.deb` installs a `claudegate` command next to the app:

```bash
claudegate .          # review the project that owns this folder
claudegate src/a.ts   # the same, with that file's row selected
claudegate            # the current directory
```

The app opens on the project that owns the path (a worktree or a nested repo counts as its project). When that project is known but not in your workspace, it says so and offers an Add to workspace button. If no project owns it, it says so and offers an Add project button, which registers the folder for capture and adds it to the workspace. It never adds one by itself. A second launch hands its path to the running window and focuses it, so there is only ever one window. The command detaches from the terminal and prints nothing.

To run it from Zed, add a task to `~/.config/zed/tasks.json`:

```json
{ "label": "ClaudeGate: review", "command": "claudegate", "args": ["$ZED_WORKTREE_ROOT"], "use_new_terminal": false, "reveal": "never", "hide": "always" }
```

Bind it to a key in `keymap.json` with `task::Spawn`, for example `{ "context": "Workspace", "bindings": { "ctrl-alt-g": ["task::Spawn", { "task_name": "ClaudeGate: review" }] } }`.

On macOS there is no CLI install yet; `open -a ClaudeGate --args <path>` works.

## Projects

The rail lists your workspace: the projects you added to it, in the order you added them, with the git worktrees inside each project shown as scopes of it. Every other folder in `~/.claudegate/workspace-roots.json` (VS Code's and the app's) is a known project: still captured, just not on the rail.

**Nested repos.** A registered folder strictly inside another registered folder (say `umbrella/umbrella-orders`, a repo the umbrella `umbrella` gitignores) is not listed on its own. It shows inside the outermost containing project as a scope labelled by its path there and tagged `repo`. It is never offered as a known project of its own: when its parent is outside the workspace, its pending counts toward the parent's row in the picker, and removing the parent from the workspace takes it along. Its rows still live in its own session file (the hook records each edit in the session of the longest matching root), so its decisions write that file and no other, its rows are filtered by its own exclude list (`excludes.json[<nested root>]`, or the app's list when the app added that folder), and its own worktrees are found under it. It does not count against "Worktrees to include". Its app-only hide patterns are its own too: they hide only its rows, and the parent's Settings show and edit them under "Extra patterns in <path>". An entry the parent's session still holds for a file the nested repo has pending, or decided after that entry was captured, is not listed in the parent; it is never removed either. When a revert in the parent reopens a file whose entry in the nested repo's log is excluded there or changes nothing, that entry stays in the nested log, unlisted, until the extension prunes it for a folder it has open. Clearing the nested repo's Accepted or Rejected log takes away what hides such an older entry, so it shows again in the parent; the Clear prompt says how many ("3 older changes in umbrella will show again"). When the containing folder is gone from disk, the nested one is listed on its own again. Folders are compared by whole path segments, so `/a/foobar` is not inside `/a/foo`.

**Add to workspace.** "+ Add to workspace" at the bottom of the rail opens a picker. Type to filter the known projects by name or path; each row shows the project's name, its path (shortened from the left) and its pending count, with projects that have pending first, then the rest by name. ↑ and ↓ move, Enter adds the highlighted one, Esc closes. Adding a known project only puts it on the rail: what is captured does not change and `workspace-roots.json` is not touched. "Choose folder…" (the system picker) and "Type path" add a new folder: they register it for capture, as Add project always did, and put it on the rail.

**Remove from workspace.** Right-click a project, or focus it and press Shift+F10 (or the menu key), for "Remove from workspace"; Delete does the same directly. It only takes the project off the rail. For a project the app added, a confirm step offers "Also stop capturing (forget this folder)": checked, it runs the Remove flow, with its rules: when VS Code or Cursor also uses the folder only the app's record is dropped and capture stays on, the review history is always kept, and read-only mode blocks it.

**Pending elsewhere.** Pending work outside the workspace is never swallowed. When known projects have pending edits, a line sits above "+ Add to workspace" (`● 2 other projects have 7 pending`) and opens the picker; nothing is added on its own. Only captures whose write has landed count; a settled no-op never does. The header total counts the workspace and adds ` · +7 elsewhere`, and so does the tray. An empty workspace says "Add a project to start reviewing", and says when known projects have pending.

**Upgrading.** The first time this version loads, it sets the workspace to every listed project that was not hidden, in the listed order, and drops `hiddenProjects` and `hiddenAt` from `config.json` in the same write.

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

**Capture without VS Code.** The header's Capture chip says whether Claude Code is wired to the ClaudeGate hook: "Capturing (VS Code)", "Capturing (this app)", or "Not capturing — Set up" (and why, when `~/.claude/settings.json` is unreadable, or python3 is missing and no hook is set up). A registered hook outranks a missing python3: the chip still says "Capturing (…)" and notes that python3 wasn't found from the app, since the hook runs under Claude Code's environment. The app looks for python3 on PATH and then in the usual install folders, like the editor launcher; on macOS `/usr/bin/python3` counts only when `xcode-select -p` finds the Command Line Tools, so the check never opens the installer. "This app" means the marker is there and `~/.claudegate/hook.py` carries the app's version stamp; once the extension replaces the hook, the chip says the VS Code extension manages it. Set up asks first, then writes the rows marked "Set up capture" below and shows the backup it kept. `scripts/capture-e2e.sh` checks it headless in a temp HOME: it adds a project, sets capture up, feeds the installed hook a Write and asserts a pending entry lands in that project's session, then checks the startup hook update and Update hook (it prints `CAPTURE E2E OK`).

**Update hook.** While a hook is registered, the chip's popover also says which `hook.py` is installed against the one this app bundles, for example "Hook: VS Code extension's version · this app has v1.17.0" (an unstamped `hook.py` is the extension's, and its version is not guessed), "Hook: v1.16.1 · this app has v1.17.0", "Hook: a modified copy of v1.17.0 · this app has v1.17.0", or "Hook: hook.py is missing". The stamp is read from line 2 only, the line the build puts there. When the hook is older, modified, unstamped or missing, an **Update hook…** button asks "Replace ~/.claudegate/hook.py with the version bundled in this app (v1.17.0)? hook.sh and ~/.claude/settings.json are not changed. The current file is backed up." (for an unstamped hook it adds that it is "the VS Code extension's version, which may be newer than v1.17.0"; for a modified copy, that it is one) and, on Update, replaces it and shows where the backup went. A hook stamped newer than the bundled one reads "Hook: v1.18.0 is newer than this app's v1.17.0" and gets no button; the write refuses it too, so the app never downgrades the hook. When the marker is there but the extension had replaced the hook, Update hook makes it stamped again, so this app counts it as its own: the result says "This app will keep this hook up to date until the VS Code extension replaces it", meaning the startup update now maintains it. The marker itself is not written. It exists for working without VS Code: the extension refreshes `hook.py` itself only when it activates, so a machine that no longer opens VS Code keeps an old hook. It works whoever set capture up, and it is never automatic. If the extension later rewrites `hook.py`, the app lets it; the startup update still only touches a hook this app owns. Read-only mode hides the button.

### What the app writes

Every write the app can make, and whether the read-only switch blocks it. "Shared" files live in the ClaudeGate home (`~/.claudegate`) and are also written by the VS Code extension and its hook. An unreadable `config.json` counts as read-only for every row marked "Yes".

| Write | File(s) | Blocked by read-only? | Why |
|---|---|---|---|
| Decisions (accept, reject, revert, reapply, clear, Undo) | the project's session file (shared); the workspace files rejected or reapplied; a backup | Yes | The switch exists to stop these. |
| Add project, and Remove from workspace with "Also stop capturing" | `workspace-roots.json` and `excludes.json` (shared); `config.json` | Yes | They change what the hook captures for every ClaudeGate. |
| Set a project's exclude list | `excludes.json` (shared); `config.json` | Yes | The hook reads it. |
| Take back an exclude list from VS Code | `excludes.json` (shared); `config.json` | Yes | Overwrites what VS Code wrote. |
| Hook-log toggle | `hooklog.enabled` (shared) | Yes | Turns logging on or off for the extension too. |
| App settings (overrides, extra hide patterns, editor, path style, the read-only switch itself) | `config.json` | No | The app's own file, never read by the extension; the switch must be able to turn itself off. |
| Workspace | `config.json` (`workspace`) | No | Which projects the rail shows. Add to workspace adds one root and Remove from workspace removes one (with any root nested in it), each under the config lock, never rewriting the list from the UI's copy. On the first load without `workspace` it is derived from the listed roots minus `hiddenProjects`, and `hiddenProjects`/`hiddenAt` are dropped, in one write; nothing is written once it exists. |
| Watcher re-add of the app's own roots | `workspace-roots.json` (shared) | No | The extension's unlocked rewrite can drop a root the app added; putting it back restores capture you already chose. It writes only when one of the app's roots is missing. |
| Exclude sync, at startup and on every change of `excludes.json` or `config.json` | `excludes.json` (shared); `config.json` (`excludeWritten`, `excludeWrittenPrev`, `excludeWrittenAt`, `managedByVsCode`) | No | Maintains the ownership you chose when you added the project, and records a VS Code takeover. It compares first and writes only when something changed. |
| Upkeep of the app's own projects, after loading one and on its session events | the project's session files (shared), and a backup unless the exception above applies | Yes | Only for projects the app added and VS Code has not taken over, and never while VS Code's or Cursor's `storage.json` shows the folder open (their own extension keeps that session). |
| Backups | `~/.claudegate-desktop/backups/` | Follows the write it precedes | Taken only before a session write that is itself allowed. |
| Set up capture: hook files | `hook.py` and `hook.sh` (mode 0755) in `~/.claudegate` (shared) | Yes | They are what Claude Code runs before every edit. Created, never replaced, with one exception: an existing `hook.py` is adopted (replaced atomically with the bundled, stamped hook, its mode kept) only when it is a plain file holding byte for byte either the bundled hook or the bundled hook minus its `CLAUDEGATE_HOOK_VERSION` line, which is the extension's copy at the same tag (typically left behind when the extension was uninstalled). It is compared with what was read right before the rename, and any change refuses setup. Any other `hook.py` refuses setup, and an existing `hook.sh` is kept only when it is byte-for-byte the extension's wrapper. Setup may create the `~/.claudegate/` and `~/.claude/` folders; a failed setup removes them only if empty, removes only the files it created and puts an adopted `hook.py` back while it still holds setup's bytes, and only while `~/.claude/settings.json` has no ClaudeGate entry (or is gone): a registered entry is never left pointing at a deleted hook. |
| Set up capture: marker | `~/.claudegate/.installed-by-desktop` (shared folder), holding the bundled hook's version (e.g. `v1.16.1`) | Yes | Records that this app, not the extension, set capture up. A marker that is not a regular file (a symlink, say) refuses setup; a failed setup puts an old marker back only while it still holds setup's bytes. |
| Set up capture: Claude Code settings | `~/.claude/settings.json`, at its realpath (a symlink keeps its link) | Yes | Adds one `PreToolUse` entry and keeps every other key and hook. Only when the file is absent or a plain JSON object (no comments) and no ClaudeGate entry exists. The new content is written to a temp file and synced; then, immediately before the rename, setup re-reads the file and refuses if anything changed since it first read it. Afterwards it reads the file back: if the bytes are still setup's and fail the check, the backup is restored byte for byte (or a file setup created is removed), again comparing right before that rename. If another program has rewritten the file in the meantime, it is left as that program wrote it; setup counts as done when the file still has a ClaudeGate entry (the same test the status uses) and keeps the hook files then, or when the file cannot be read back. **Known limit:** the file has no lock, so a write by Claude Code between that last re-read and the rename syscall (microseconds) is replaced by setup's; the backup holds the file as it was before setup. |
| Set up capture: settings backups | `~/.claude/settings.json.claudegate-<UTC ISO time>.bak` | Follows the write it precedes | The extension's name and its ring: both keep the newest 5, pruned only after a setup succeeds. A failed backup refuses the settings write. The older `settings.json.claudegate.bak` is never pruned. |
| Startup update of the app's own hook | `~/.claudegate/hook.py` (shared), replaced atomically with its mode kept; then the marker gets the new version | Yes | At launch, only when every one holds: the marker exists, the status is "set up by this app" (a ClaudeGate entry is registered and the marker is there), the installed `hook.py` carries a `CLAUDEGATE_HOOK_VERSION` stamp (the extension's copy has none, so a `hook.py` the extension rewrote stays), the bundled version is newer than the stamp in the installed `hook.py` (the file that runs, not the marker), python3 was found, and read-only is off. `hook.py` is compared with what was read right before the rename. Never runs when VS Code set capture up, and never writes `hook.sh` or `~/.claude/settings.json`. |
| Update hook | `~/.claudegate/hook.py` (shared), replaced atomically with its mode kept (created, never replacing anything, when it is missing) | Yes | Only after the user confirms Update hook…, and only while a ClaudeGate entry is registered in `~/.claude/settings.json` (set up by VS Code or by this app). Refused when `hook.py` is a symlink or not a regular file, when it already is the bundled hook, when it is stamped newer than the bundled hook (no downgrade), and off unix. `hook.py` is compared with the bytes read right before the rename, and any change since (the extension syncing it, say) refuses. Never writes `hook.sh`, the marker or `~/.claude/settings.json`, and never runs by itself. |
| Update hook: backup | `~/.claudegate-desktop/backups/hook/hook-<UTC timestamp>.py`, with the replaced file's mode | Follows the write it precedes | The `hook.py` that was there, written and synced before the replace; a failed backup refuses it. When the replace is then refused or fails, the backup just made is removed (hook.py is unchanged), so the folder never grows past 10. The newest 10 are kept, pruned only after an update succeeds. |
| `config.json` | `~/.claudegate-desktop/config.json` | No | Created with the defaults on first launch when missing, and rewritten by every row above that records something in it. Never inside the ClaudeGate home. |
| Window size and place | `.window-state.json` in the OS's app config folder: `~/.config/dev.claudegate.desktop/` on Linux (`$XDG_CONFIG_HOME` when set), `~/Library/Application Support/dev.claudegate.desktop/` on macOS | No | The window's size, position and whether it is maximised, restored at the next launch. Written by `tauri-plugin-window-state` once the window has been still for a moment after a move or resize, and again when the app quits, so a killed app keeps it too. A saved position on no current monitor is ignored. On a GNOME Wayland session the app runs through XWayland so the position can be restored; `CLAUDEGATE_WAYLAND=1` keeps native Wayland, which restores the size only. Never inside the ClaudeGate home. |

So with read-only on, the app can still write `workspace-roots.json` (re-add) and `excludes.json` (sync) for the projects it added, and its own `config.json`. With no app-added projects, neither of those has anything to do.

**Known limit: a lost write with the extension.** The extension writes `workspace-roots.json` and `excludes.json` without a lock. The app re-reads each file right before its atomic rename and starts over if it changed, but a write by the extension inside the few microseconds between that check and the rename can still be lost (or the extension, writing from a stale read, can drop the app's). The re-add and exclude sync above repair the app's side on the next change event. Session files do not have this gap: both sides take `<session>.lock`.

### Checking what it writes

`scripts/readonly-proof.sh [--no-app] [--bin PATH] [--print-bin] [HOME_DIR] [SECONDS]` snapshots every file under the ClaudeGate home (default `~/.claudegate`), runs the built binary for SECONDS (default 60), snapshots again, and prints `UNCHANGED` (exit 0) or each changed, added or removed file with its last `hook.log` mention (exit 1). The extension's hook may legitimately write in that window; the log line shows who. The binary is `--bin PATH`, else the newer of `target/release` and `target/debug`; the script prints its path, build time and the current `HEAD`, and warns when any file under `core/src`, `src-tauri/src` or `ui/src` is newer than it (`--print-bin` prints just that and exits). `--no-app` skips the launch; set `READONLY_PROOF_HEADLESS=1` to run the app under xvfb. Snapshots live in a temp dir, never inside the home.

The proof sees the app's own writes too, not only the hook's: on a home where the app has added projects, a run can report the startup exclude sync, a re-added root or upkeep of those projects' sessions (see "What the app writes"). Expect `UNCHANGED` only on a home where the app owns no projects, or where everything is already in sync and nothing needs upkeep.

A static check, `core/tests/no_writes.rs`, also fails `cargo test` if any source file uses a write API outside its allow-list: `src-tauri/src/config.rs` (the app's `~/.claudegate-desktop`), `core/src/store.rs` (session files, locks, temp files, backups, `workspace-roots.json`, `excludes.json`, the hook-log sentinel, capture setup's hook files, marker, `~/.claude/settings.json` and its backups, the startup update of the app's own `hook.py`, and Update hook's `hook.py` and its backups) and `core/src/workspace_write.rs` (workspace files on reject and reapply). A trailing `#[cfg(test)]` module is not scanned.
