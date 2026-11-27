# Architecture

ClaudeGate Desktop is a standalone desktop reviewer for the session files written by the ClaudeGate VS Code extension. It reads `~/.claudegate/sessions/`, shows the pending, accepted and rejected changes Claude Code has made, and lets you decide them, without needing an editor open.

## The pieces

```
 Claude Code (terminal or editor)
        │  PreToolUse hook, before every Write / Edit / MultiEdit / Bash
        ▼
 ~/.claudegate/hook.py ── snapshots the file as it is now
        │
        ▼
 ~/.claudegate/sessions/<md5(project root)>.json   ◀── also read and written by the VS Code extension
        │  pending entries: path + original content
        ▼
 ClaudeGate Desktop ── watches the folder, shows the diff (original vs disk),
                       accept keeps the file, reject writes the original back
```

- **The hook.** `hook.py` comes from the [ClaudeGate extension](https://github.com/lntvan166/claudegate). Claude Code runs it before each edit, so the original is saved before the file changes. For shell commands it works out which files the command writes. The app bundles a copy (`src-tauri/resources/hook.py`) so it can set capture up without VS Code.
- **The capture audit.** `audit.py`, also from the extension, runs after each Bash call and records files that changed without a capture. The app reads those records to show the **Not captured** list.
- **Session files.** One JSON file per project under `~/.claudegate/sessions/`. Pending entries hold the original content; the accepted and rejected logs hold what was decided. The extension and the app both take `<session>.lock`, re-read the file just before writing, and merge in what the other (and the hook) wrote in between.
- **The app's own state.** `~/.claudegate-desktop/config.json` (settings, workspace, Git log profiles) and `~/.claudegate-desktop/backups/` (a copy of each session file before the app writes it). The extension never reads them.

Every file the app can write is listed in [What the app writes](what-the-app-writes.md).

## Code

| Path | What it holds |
|---|---|
| `core/` | Rust. The session model and migration, the decisions, the locked store, capture setup, projects and worktrees, the capture audit reader, and the read-only Git log (`core/src/gitlog/`). It has no UI and no Tauri dependency. |
| `src-tauri/` | The Tauri 2 shell: commands, the file watcher, `config.json`, the tray, window state, the updater and the `claudegate` launcher. |
| `ui/` | Svelte 5. The project rail, the review tree, Monaco diffs with TextMate highlighting through Shiki, settings and the Git log view. |
| `xcheck/` | Runs the extension's own TypeScript against the core's ports. See [Development](development.md#cross-check-with-the-extension-xcheck). |

`core/tests/no_writes.rs` fails the build if a write API appears outside the three allowed files. See [What the app writes](what-the-app-writes.md#checking-what-it-writes).

## The Git log view

The Git log view reads repositories with `git log`, `git show` and `git blame`, through a pooled runner with cancellation. It never writes into a repository. The Combined Changes diff uses a three-way line merge written in Rust (`core/src/gitlog/line_merge.rs`) rather than `git merge-file`, because that command writes objects into `.git`.
