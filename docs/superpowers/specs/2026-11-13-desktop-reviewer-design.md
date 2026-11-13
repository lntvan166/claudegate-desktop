# ClaudeGate Desktop: a standalone reviewer app

Date: 2026-11-13
Status: approved 2026-11-13 (name stays ClaudeGate)

## Intent

ClaudeGate only works inside VS Code and Cursor today. The maintainer is trying Zed and wants a
**standalone ClaudeGate**: a desktop app that reviews Claude's edits for any project, whatever
editor is open (Zed, VS Code, Neovim, or none). Zed extensions cannot add panels, tree views,
badges or custom diff editors, so the review UI cannot live inside Zed.

The maintainer cares most about two things:

- **It is beautiful**: real file icons, a polished UI, a diff view as good as VS Code's.
- **It cannot hurt the production extension.** ClaudeGate is published on the VS Code
  Marketplace and Open VSX with real users. The app must not change, slow, or endanger it.

## What we found that shapes this

1. **The app and the extension share runtime files.** `hook.py` and every open VS Code window
   already read and write `~/.claudegate/sessions/<md5(root)>.json`, `workspace-roots.json` and
   `excludes.json`. Writers take an advisory lock (`<session>.lock`, `O_CREAT|O_EXCL`, stale after
   3 s; see `SessionManager.acquireLock` in the extension), re-read the file, and merge before
   writing (`mergeFreshCaptures` with `MergeGuards`, `src/reviewModel.ts`). Code separation alone
   does not protect production; the runtime rules below do.
2. **The hook only captures projects VS Code has registered.** `hook.py`
   (`workspace_root_for_file`) drops any edit outside a root listed in `workspace-roots.json`
   with `skip-no-root`; only the extension writes that list. On the maintainer's machine 676 of
   884 `hook.log` lines are `skip-no-root`, 595 for a project only ever opened in Zed. So for
   Zed-only work the app must be able to register projects.
3. The schema, decision semantics and worktree rules are documented in the extension's
   `CLAUDE.md` ("Session State Schema", "Every Panel Action Must Span the Same Sessions the Panel
   Draws"). The app follows them; this doc does not restate them.

## Goals

- Review pending Claude edits for any registered project from one desktop window: list, diff,
  accept, reject, per file, per folder, all; plus the Accepted and Rejected history with
  revert and reapply.
- A diff view of VS Code quality: side by side, word-level highlights, syntax colours, smooth
  scrolling on large files.
- File icons from Material Icon Theme (the set the maintainer uses in Zed); polished light and
  dark themes.
- Live: new captures appear without a refresh; a decision taken in VS Code shows in the app and
  the reverse.
- Register a project from the app, so the hook captures Zed-only work.
- Tray icon with the pending count; a notification when new pending edits arrive.
- "Open in editor": open the file at a line in Zed or VS Code.

## Non-goals (v1)

- No AI or model calls. ClaudeGate is review-only.
- No hunk-level accept/reject (declined for the extension; same here).
- No editing Claude's version inside the diff before accepting.
- No installing the hook for people without VS Code (see Later).
- No macOS notarization or Windows signing.

## Anti-goals

- **No change to the production extension.** Its source, tests, `hook.py`, VSIX, release flow and
  marketplace listings are untouched by this project. Any extension change (such as a hook
  fallback for unregistered projects) is a separate decision, made later, in the extension repo.
- **Never lose or corrupt a capture or a decision.** A pretty app that clobbers a hook write or
  resurrects a cleared record is worse than no app.
- **Never "repair" what it does not understand.** A session written by a newer extension or hook
  is read, not rewritten.
- It must not look like a debug tool.

## Approaches considered

1. **Terminal app (ratatui) handing deep diffs to `zed --diff`.** Cheapest; works over SSH.
   Rejected: cannot deliver the icons and UI the maintainer wants.
2. **gpui + gpui-kit** (the stack Herdr's desktop app uses). Native, Zed/Herdr look. Rejected
   for v1: pre-1.0 API, and no ready side-by-side diff, so the diff view becomes the project.
3. **Tauri: Rust core, web UI with Monaco's diff editor.** **Chosen.** Monaco's diff editor is
   the one VS Code uses, so the diff is VS Code quality from day one; Material Icon Theme ships
   as MIT SVGs; all file and session work stays in Rust. Costs: a system webview (WebKitGTK on
   Linux) and a ~10 MB installer.
4. **No app: fix capture in `hook.py` and review in VS Code**, or a small Python command plus
   `zed --diff`. Far cheaper, and considered seriously. Rejected because the maintainer wants a
   standalone ClaudeGate, and both require changing the production extension's `hook.py`.

Also rejected: a local web page (leaves the desktop) and a Zed slash-command/MCP extension (only
in Zed's AI panel, which the maintainer disables, and routes review through the model).

## Decisions made with the maintainer

- Standalone desktop app built with Tauri.
- **Separate repo** (working name `claudegate-desktop`), so nothing in the extension's repo,
  CI or release flow is touched. Chosen over a shared repo for safety, accepting that a change
  to the extension's session format will not automatically fail the app's tests (the read-only
  fallback below makes that safe).
- The extension's source is left alone.
- **Read-only first**: the app ships as a viewer before it is allowed to write.
- Diff view: Monaco. Icons: Material Icon Theme.

## Design

### Repo layout (`claudegate-desktop`)

```
core/        Rust library: session model, shape check, lock + merge, decisions, upkeep,
             roots registry, excludes, worktree discovery. No Tauri dependency.
src-tauri/   Tauri shell: commands for the UI, watching, tray, notifications
ui/          Vite + Svelte + TypeScript, Monaco, Material Icon Theme SVGs
fixtures/    contract cases (see Testing)
```

Svelte is the designer's call (small bundle, simple reactivity); an equivalent is fine.

### Protecting production

These rules apply to every build, and outrank features.

1. **Known-shape check.** The core keeps an explicit list of the session fields it understands,
   top-level and per entry, as of the extension version it was built against (currently
   v1.16.1, which added `excludeCleanupDone`). Any session with a field outside that list, or a
   field of an unexpected type, opens **read-only** for that project, with a banner: "This
   project was written by a newer ClaudeGate. Update the app to make decisions here." Updating
   the list is a manual step when the extension adds fields.
2. **Unknown data round-trips.** Even where writing is allowed, the model keeps any unrecognised
   value it reads (`#[serde(flatten)]`) and writes it back unchanged.
3. **Same write protocol as the extension.** Lock (`<session>.lock`, 200 ms wait, 3 s stale, the
   extension's values since the app is an interactive writer), re-read from disk, migrate, merge
   with guards (a port of `mergeFreshCaptures` / `MergeGuards`), set `status`, atomic write
   (temp + rename), release.
4. **Backup before every write.** The session file is copied to
   `~/.claudegate-desktop/backups/<hash>/<timestamp>.json` before each write; the last 50 per
   project are kept. The app's own state lives in `~/.claudegate-desktop/`, never in
   `~/.claudegate/`, except the three shared files it must write.
5. **Upkeep only where nothing else does it.** `SessionManager` runs upkeep on load
   (`reconcilePending` drops no-op and temp-file entries after `RECONCILE_GRACE_MS`;
   `healOversized`; `cleanupExcludedOnce`; `pruneOutOfWorkspaceEntries`). The app runs the
   ported equivalents **only for projects the app registered**. Projects that came from VS Code
   are left to VS Code. For display, every count uses the real-change test
   (`hasRealPendingChange`) so phantom entries never show.
6. **Roots: append, never rewrite.** "Add project" appends one root to `workspace-roots.json`
   with a merge and never removes or reorders the extension's entries. It refuses a folder that
   is nested inside, or contains, an existing root, naming that root, because `hook.py` routes to
   the longest match and a nested root would pull captures out of a session VS Code shows (a git
   worktree under a root needs no registration; it is found as a worktree scope). The app
   records the roots it added in `~/.claudegate-desktop/config.json`. "Remove" applies only to
   those, unregisters the root, and keeps its session file so re-adding restores it. The
   extension writes `workspace-roots.json` without a lock, so it can drop a root the app just
   added; the app watches the file and re-adds its own missing roots.
7. **Excludes: own only what the app added.** For a root the app registers, it writes the
   extension's default `claudegate.exclude` patterns into `excludes.json` (merge, its own root's
   key only) and offers a pattern editor. Patterns for VS Code's roots are shown read-only.
8. **Global read-only switch** in settings, and **`CLAUDEGATE_HOME`** to point the app at a
   sandbox instead of `~/.claudegate`. Development and tests never use the real directory.

### Rust core

- Session model and shape check (rules 1–2).
- Workspace hash `md5(resolved path)`, lower-cased on Windows, as in `hook.py`.
- Decisions with the extension's semantics: accept, reject, revert, reapply, clear; per file,
  per folder, all. Reject restores the original (following symlinks, preserving mode), deletes
  only a confident-new file, leaves an uncertain null-baseline file. Bulk actions skip excluded
  and no-op entries.
- Exclude matcher ported from `src/excludeMatcher.ts`.
- Worktree discovery as in `src/worktrees.ts` / `worktreeSessionRegistry.ts`; a project is its
  primary session plus attached worktree sessions, and every action spans all of them.

### Tauri shell

- Commands for the UI: projects, project state, file contents (before/after), decisions. The UI
  never touches the filesystem.
- One watcher on the sessions directory and the two shared lists; coalesced events; parse only
  the session that changed.
- Tray with the total pending count and a menu of projects with pending edits.
- Notifications when a project gains real pending edits, after the reconcile grace window, at
  most one per project per short window; clicking focuses that project.
- Open in editor: presets `zed {path}:{line}` and `code -g {path}:{line}`, or a custom template.

### UI

```
╔ ClaudeGate ────────────────────────────────────────────── ● 7 pending ═╗
║ PROJECTS          │ PENDING  ACCEPTED  REJECTED                         ║
║ ● shop-api     3  │ ▾ src                    │  before   │  after       ║
║ ● herdr-pokemon 4 │   [ts] utils.ts   ✓ ✗    │  Monaco side-by-side     ║
║   claudegate      │   ▾ api                  │  diff, word highlights,  ║
║ + Add project     │     [ts] client.ts ✓ ✗   │  syntax colours          ║
║                   │ [md] README.md     ✓ ✗   │                          ║
║                   │ Accept all · Reject all  │ Open in Zed ↗            ║
╚═══════════════════╧══════════════════════════╧══════════════════════════╝
```

- Left: projects with pending counts, marked "added here" or "from VS Code"; Add project.
- Middle: Pending / Accepted / Rejected tabs as trees (flat-list toggle) with Material Icon Theme
  icons, per-row ✓/✗ and folder actions; worktree groups as in the extension. In read-only mode
  the actions are hidden and the banner explains why.
- Right: Monaco diff, read-only, side by side with an inline toggle; new file has an empty left
  side, deleted file an empty right side.
- Keys: `j/k` move, `a` accept, `r` reject, `n/p` next/previous pending across worktrees, `o` open
  in editor.
- Light and dark following the OS. Monaco and icons bundled; works fully offline.

### Testing

- `cargo test` over `fixtures/`: each case is an input session, a disk session changed
  concurrently, an action, and the expected result. Cases cover every decision, merge guards,
  upkeep, shape check, roots and excludes.
- **Cross-check against the extension without changing it**: the app's CI checks out the
  extension repo at a pinned release tag and runs a small TS harness (in the app repo) that feeds
  the same fixtures through the extension's exported pure functions in `src/reviewModel.ts`, and
  the exclude cases through `src/excludeMatcher.ts`. A disagreement fails the app's CI, not the
  extension's. Bumping the pinned tag is how the app notices extension changes.
- All tests and dev runs use `CLAUDEGATE_HOME` sandboxes.

### Distribution

GitHub Actions builds Linux (AppImage, .deb), macOS (.dmg, unsigned) and Windows (.msi,
unsigned), attached to GitHub Releases. Linux is the primary target.

## Milestones

1. **Core, read side**: session model, shape check, worktree discovery, real-change test,
   fixtures and the cross-check harness.
2. **Viewer** (first usable release, cannot write): projects, pending/accepted/rejected trees with
   icons, Monaco diff, live updates, open in editor, tray count.
3. **Decisions**: write protocol, backups, accept/reject/revert/reapply/clear across scopes,
   keyboard flow, read-only fallback.
4. **Projects**: add/remove roots, excludes for app-added roots, upkeep for app-added roots,
   notifications.
5. **Packaging**: CI builds and releases.

Milestone 2 is the first build the maintainer runs against real data; it is safe by
construction because it has no write path.

## Later (out of scope)

- **Other agents** (Codex CLI, Cursor, Gemini CLI, Antigravity). As of Nov 2026 each has a
  before-tool hook that could snapshot originals the way Claude's `PreToolUse` does. The
  maintainer uses Claude only, so v1 is Claude-only. If added later: the app installs its own
  per-agent hooks (a Rust `hook --agent <name>` subcommand, no Python) that write to the app's
  own store under `~/.claudegate-desktop/`, **never** to the extension's session files, which
  the extension rewrites wholesale and would mangle. The UI then labels each change with its
  agent. v1 does not build this seam speculatively; keeping the core's session code in one
  module is enough to add it later. A multi-agent app would also want an agent-neutral name.

- Installing the hook for users without VS Code (port of the extension's guarded
  `settings.json` install).
- Any extension change, such as a `hook.py` fallback that captures unregistered projects without
  the app. Decided separately, in the extension repo.

## Left to the builder

- Svelte structure, state management, styling system.
- Monaco languages beyond the common set (TS/JS, Go, Python, Rust, JSON, YAML, Markdown, shell,
  SQL, HTML/CSS).
- Watcher library and coalescing interval.
- Visual design within the layout above; show the maintainer a mockup before polishing.
