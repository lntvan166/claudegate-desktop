# User guide

How ClaudeGate Desktop behaves, screen by screen. For the files it touches, see [What the app writes](what-the-app-writes.md).

## Projects

The rail lists your workspace: the projects you added to it, in the order you added them, with the git worktrees inside each project shown as scopes of it. Every other folder in `~/.claudegate/workspace-roots.json` (VS Code's and the app's) is a known project: still captured, just not on the rail.

**Nested repos.** A registered folder strictly inside another registered folder (say `umbrella/umbrella-orders`, a repo the umbrella `umbrella` gitignores) is not listed on its own. It shows inside the outermost containing project as a scope labelled by its path there and tagged `repo`. It is never offered as a known project of its own: when its parent is outside the workspace, its pending counts toward the parent's row in the picker, and removing the parent from the workspace takes it along. Its rows still live in its own session file (the hook records each edit in the session of the longest matching root), so its decisions write that file and no other, its rows are filtered by its own exclude list (`excludes.json[<nested root>]`, or the app's list when the app added that folder), and its own worktrees are found under it. It does not count against "Worktrees to include". Its app-only hide patterns are its own too: they hide only its rows, and the parent's Settings show and edit them under "Extra patterns in <path>". An entry the parent's session still holds for a file the nested repo has pending, or decided after that entry was captured, is not listed in the parent; it is never removed either. When a revert in the parent reopens a file whose entry in the nested repo's log is excluded there or changes nothing, that entry stays in the nested log, unlisted, until the extension prunes it for a folder it has open. Clearing the nested repo's Accepted or Rejected log takes away what hides such an older entry, so it shows again in the parent; the Clear prompt says how many ("3 older changes in umbrella will show again"). When the containing folder is gone from disk, the nested one is listed on its own again. Folders are compared by whole path segments, so `/a/foobar` is not inside `/a/foo`.

**Add to workspace.** "+ Add to workspace" at the bottom of the rail opens a picker. Type to filter the known projects by name or path; each row shows the project's name, its path (shortened from the left) and its pending count, with projects that have pending first, then the rest by name. ↑ and ↓ move, Enter adds the highlighted one, Esc closes. Adding a known project only puts it on the rail: what is captured does not change and `workspace-roots.json` is not touched. "Choose folder…" (the system picker) and "Type path" add a new folder: they register it for capture, as Add project always did, and put it on the rail.

**Remove from workspace.** Right-click a project, or focus it and press Shift+F10 (or the menu key), for "Remove from workspace"; Delete does the same directly. It only takes the project off the rail. For a project the app added, a confirm step offers "Also stop capturing (forget this folder)": checked, it runs the Remove flow, with its rules: when VS Code or Cursor also uses the folder only the app's record is dropped and capture stays on, the review history is always kept, and read-only mode blocks it.

**Pending elsewhere.** Pending work outside the workspace is never swallowed. When known projects have pending edits, a line sits above "+ Add to workspace" (`● 2 other projects have 7 pending`) and opens the picker; nothing is added on its own. Only captures whose write has landed count; a settled no-op never does. The header total counts the workspace and adds ` · +7 elsewhere`, and so does the tray. An empty workspace says "Add a project to start reviewing", and says when known projects have pending.

**Upgrading.** The first time this version loads, it sets the workspace to every listed project that was not hidden, in the listed order, and drops `hiddenProjects` and `hiddenAt` from `config.json` in the same write.

## Not captured

When Claude changes a file through a shell command that ClaudeGate's hook did not record, the project header says so: "3 files Claude changed weren't captured". Click it to see each file, when it changed and the command that did it, then open it in your editor or dismiss it.

There is no Reject for these. Nothing was saved before the change, so there is no baseline to go back to. Review them in your editor or with git.

A second list, "Changed while Claude was running (may be yours)", holds files that changed during a Claude command but that the command may not have touched. It is collapsed by default.

Changes that arrive from `git pull`, merges, code generators and archive extraction are never shown: they are not edits Claude made to your files. Dismissing a file hides it in this app only until it is missed again. Settings, under Capture, shows the last 7 days as captured, missed, phantom and implicit counts.

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

**Capture without VS Code.** The header's Capture chip says whether Claude Code is wired to the ClaudeGate hook: "Capturing" (with " · update hook" when the hook is outdated) or "Not capturing — Set up" (and why, when `~/.claude/settings.json` is unreadable, or python3 is missing and no hook is set up). A registered hook outranks a missing python3: the chip still says "Capturing" and notes that python3 wasn't found from the app, since the hook runs under Claude Code's environment. The app looks for python3 on PATH and then in the usual install folders, like the editor launcher; on macOS `/usr/bin/python3` counts only when `xcode-select -p` finds the Command Line Tools, so the check never opens the installer. The popover says who registered the hook ("Registered by: VS Code extension" or "Registered by: this app", which means the marker is there and `~/.claudegate/hook.py` carries the app's version stamp; once the extension replaces that hook it reads "this app, then taken over by the VS Code extension") and who keeps `hook.py` current: "Hook: v1.17.0, kept up to date by this app" for a hook carrying the app's stamp, whoever registered it, and "Hook: VS Code extension's version" for an unstamped one. Set up asks first, then writes the rows marked "Set up capture" in [What the app writes](what-the-app-writes.md) and shows the backup it kept. `scripts/capture-e2e.sh` checks it headless in a temp HOME: it adds a project, sets capture up, feeds the installed hook a Write and asserts a pending entry lands in that project's session, then checks the startup hook update and Update hook (it prints `CAPTURE E2E OK`).

**Update hook.** While a hook is registered, the chip's popover also says which `hook.py` is installed against the one this app bundles, for example "Hook: VS Code extension's version · this app has v1.17.0" (an unstamped `hook.py` is the extension's, and its version is not guessed), "Hook: v1.16.1 · this app has v1.17.0", "Hook: a modified copy of v1.17.0 · this app has v1.17.0", or "Hook: hook.py is missing". The stamp is read from line 2 only, the line the build puts there. When the hook is older, modified, unstamped or missing, an **Update hook…** button asks "Replace ~/.claudegate/hook.py with the version bundled in this app (v1.17.0)? hook.sh and ~/.claude/settings.json are not changed. The current file is backed up." (for an unstamped hook it adds that it is "the VS Code extension's version, which may be newer than v1.17.0"; for a modified copy, that it is one) and, on Update, replaces it and shows where the backup went. A hook stamped newer than the bundled one reads "Hook: v1.18.0 is newer than this app's v1.17.0" and gets no button; the write refuses it too, so the app never downgrades the hook. When the marker is there but the extension had replaced the hook, Update hook makes it stamped again, so this app counts it as its own: the result says "This app will keep this hook up to date until the VS Code extension replaces it", and the chip's "Registered by" line becomes "this app". The marker itself is not written. It exists for working without VS Code: the extension refreshes `hook.py` itself only when it activates, so a machine that no longer opens VS Code keeps an old hook. It works whoever set capture up. After it, the result says "This app will keep this hook up to date until the VS Code extension replaces it": the hook now carries the app's stamp, so the startup update keeps it current on every later bump, with no click. If the extension later rewrites `hook.py` (by hash, when VS Code starts), it is unstamped again and the app backs off, so the two never fight. Read-only mode hides the button.

## Git log

The header switch `[ Review │ Git log ]` (or Ctrl+Shift+G) opens a second view: one merged log of the commits in every repository of your workspace. It only reads. It runs `git log`, `git show` and `git blame` and never writes into a repository, not even git objects.

- **Repositories pane.** Tick which repositories to show. Profiles save a set of ticks. The ↓/↑ counts come from the last fetch; the app does not fetch.
- **Filters.** Message search, authors (with **Me**), a path or glob, a branch, and a date range (24 h by default). Each one is passed to `git log` in every repository.
- **Changes and diff.** Select a commit to see its changed files, then a file to see its diff. Enter opens the first file, `/` focuses search. Right-click a commit to copy its ID or message, or open it on the remote (GitHub, GitLab, Bitbucket, Azure DevOps).
- **Combined Changes.** Ctrl-click, Shift-click or Shift+↑/↓ to select up to 100 commits, in any repositories. The tree then lists each file once, grouped by repository, with the commits that touched it and its net status. The diff takes the selected commits' changes out of the newest version with a three-way line merge. When that is not possible (a conflict, a binary or a file too large), the file is marked ⚠ and the diff falls back to the plain range diff; the header names every unselected commit that range includes.
- **File History.** Right-click a changed file, or press `h`, to list the commits that touched it, following renames. The other filters are set aside until you close the "History of …" banner.
- **Blame.** In a Git log diff, the line under the cursor shows who last changed it, as of the commit shown. Hover it for details. It can be turned off.
- **Compare.** The `[ Log │ Compare ]` switch compares two branches across all ticked repositories. Pick the left (◀) and right (▶) branches; recent pairs and favourites are kept. It lists the repositories that differ with `◀n ▶n =n`, the files (or commits) only on each side, and the diff from the merge base.

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

**Git log from the terminal.** `claudegate log [PATH]` opens the Git log view instead of the review; with no PATH it uses the current directory. The repositories under PATH are ticked for that launch only: every discovered repository with a checkout (main or worktree) at or under PATH, plus the repository PATH sits inside. Nothing is written to `config.json` at launch. The first time you change ticks in the Repositories pane, the launch ticks are saved as the explicit `ticked` list and your change applies to them; "Show only", "Show all" or applying a profile replaces them; changing the scan depth or the exclude patterns leaves them as they are, in memory. Discovery stays workspace-only: when none of PATH's repositories is in your workspace the same "isn't in your workspace" notice as a review launch appears, with Add to workspace, after which the repositories are discovered again and ticked. A running app switches to the Git log view and takes focus. A folder literally named `log` is `claudegate ./log`; `--` ends option parsing (`claudegate log -- log`). `scripts/claudegate-wrapper-test.sh` checks the wrapper's argv.

On macOS there is no CLI install yet; `open -a ClaudeGate --args <path>` works.

## Installing and updating

Installers are on [GitHub Releases](https://github.com/lntvan166/claudegate-desktop/releases): a `.deb` and an AppImage for Linux, and a universal `.dmg` for macOS. They are not code-signed, so on macOS right-click the app and choose Open the first time.

The app checks the latest release 5 seconds after it starts and then every six hours. When a newer version is out, an "Update" chip appears in the header:

- **macOS and AppImage:** "Install and restart" downloads the update, checks its signature against the public key in `tauri.conf.json`, and replaces the app.
- **`.deb`:** the app never touches a package dpkg owns. It shows a `curl … && pkexec apt install` command to copy.

A failed check (offline) shows nothing. The update check writes no ClaudeGate state.
