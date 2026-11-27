# Changelog

All notable user-visible changes to ClaudeGate Desktop are listed here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-11-27

### Added

- **Git log view.** A second view, switched from the header or with Ctrl+Shift+G: one merged log of the commits in every repository of the workspace, with a Repositories pane (ticks, profiles, ahead/behind counts from the last fetch) and filters for message, author (with **Me**), path, branch and date. Selecting a commit shows its changed files and a Monaco diff. Right-click a commit to copy its ID or message, or open it on GitHub, GitLab, Bitbucket or Azure DevOps. The view only reads; it never writes into a repository.
- **Combined Changes.** Select up to 100 commits with Ctrl-click, Shift-click or Shift+↑/↓, in any repositories, and review each file once with its net status. A three-way line merge takes the selected commits' changes out of the newest version. When it cannot, the file is marked ⚠ and the diff header names every unselected commit the diff includes.
- **File History.** Right-click a changed file or press `h` to list the commits that touched it, across renames.
- **Blame.** The line under the cursor in a Git log diff shows who last changed it, with a hover for details. It can be turned off.
- **Compare Branches.** A Compare mode in the Git log view compares two branches across repositories, with a branch picker that keeps recent pairs and favourites.
- **Not captured.** The project header says when Claude changed files through a shell command that the hook did not record, and lists each file with the command. Settings shows the last 7 days of capture counts. The app installs and repairs the extension's capture-audit hook for this.
- **`claudegate` command.** The `.deb` installs `claudegate [PATH]` to open the project that owns PATH, and `claudegate log [PATH]` to open the Git log view. Only one window ever runs; a second launch hands its path to it.
- **Workspace.** The rail lists the projects you added, in your order. **+ Add to workspace** opens a picker of known projects or adds a new folder. Pending work in projects outside the workspace is counted, never hidden.
- **Nested repos.** A registered repo inside another project shows as a scope of that project.
- **Update hook.** The Capture chip shows the installed hook's version and offers to update it to the bundled one, after a backup. A hook the app stamped is kept up to date at startup. Setup can adopt a leftover `hook.py` it can vouch for.
- The window remembers its size, position and maximised state.
- A filter for pending files, and "hide from review" on a row.
- Ctrl+Z (Cmd+Z) runs the result bar's Undo. Copy @ref takes the cursor line when nothing is selected. Open in editor lands on the diff's cursor.

### Changed

- The bundled capture hook is the extension's v1.18.0.
- The workspace replaces hidden projects. On first launch it is set to every listed project that was not hidden.

### Fixed

- Revert and reapply never lose a change across a parent project and a nested repo.
- A session write refuses a file on disk it cannot read, instead of overwriting it.
- The app falls back to native Wayland when XWayland refuses it.
- Narrow windows no longer clip column headers, popovers or the shortcut sheet.

## [0.3.0] - 2026-11-19

### Added

- **In-app updates.** An Update chip appears when a newer release is out. On macOS and the AppImage, "Install and restart" downloads the update, checks its signature and replaces the app. For a `.deb` install it shows an `apt install` command to copy.

## [0.2.0] - 2026-11-18

### Added

- **Decisions.** Accept, reject, revert, reapply and clear, per file, folder or project, with Undo. Reject restores the original and deletes a file Claude created. Bulk actions ask first and say what will be restored or deleted.
- **Read-only mode.** A switch that hides every decision and refuses those writes. Decisions also pause when a session or the settings file cannot be read, or was written by a newer ClaudeGate.
- **Backups.** The app copies a session file before it writes it and keeps the newest 50 per project.
- **Works next to VS Code.** The app takes the extension's session lock and merges what the extension and the hook wrote in between, so a capture made during a decision is kept.
- **Capture without VS Code.** The Capture chip says whether Claude Code is wired to the ClaudeGate hook. **Set up** installs the bundled hook and registers it in `~/.claude/settings.json`, with a backup and a check after the write.
- **Projects.** Add a folder so the hook captures there, and remove it again. The app keeps the exclude list for projects it added.
- **Settings.** A settings screen that falls back to the extension's settings, with per-project exclude and protected patterns.
- Copy a Claude `@reference` for the file or selection (Ctrl+Alt+C).
- Ctrl/Cmd+click and F12 open the file at that line in your editor.
- Dracula and Alucard themes, VS Code's TextMate highlighting and JetBrains Mono in diffs.
- Resizable Projects, Review and Diff columns; the Projects column collapses into a rail.
- A shortcut overlay and keyboard access to every action.
- Linux `.deb` and AppImage, and a macOS universal `.dmg`, built by the release workflow. The app has its own icon in the dock, window and tray.

## [0.1.0] - 2026-11-13

### Added

- First version: a read-only viewer for the ClaudeGate extension's session files.
- Project list, Pending, Accepted and Rejected tabs, and a file tree with icons.
- Monaco diffs, keyboard navigation, Open in editor and a pending count in the tray.
- The app reloads when a session file changes.

[Unreleased]: https://github.com/lntvan166/claudegate-desktop/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/lntvan166/claudegate-desktop/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/lntvan166/claudegate-desktop/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/lntvan166/claudegate-desktop/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/lntvan166/claudegate-desktop/tree/v0.1.0
