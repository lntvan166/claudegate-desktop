# Development

## Prerequisites

- Rust (stable) and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.
- Node.js 22 (what CI uses) and npm.
- python3, to run the capture hook in the end-to-end scripts.

```bash
npm ci                    # the Tauri CLI
npm --prefix ui ci        # the Svelte UI
./xcheck/fetch-extension.sh   # the pinned ClaudeGate extension, into .xcheck-cache/
```

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

The browser preview uses the fixtures in `ui/src/lib/mock.ts`. Query and hash flags switch it into other states, for example `?audit=some` (files that were not captured), `?capture=app` (capture set up by the app), `?workspace=empty`, `#gitlog` (open on the Git log view) and `#settings`. The comments in `mock.ts` list the rest.

## Tests

```bash
npm --prefix ui test          # UI unit tests (vitest)
npm --prefix ui run check     # svelte-check type checking
cargo test --workspace        # the Rust core and the Tauri shell
```

CI (`.github/workflows/ci.yml`) runs the core tests, the cross-check and the UI checks on every push to `main` and every pull request.

Two core tests read real data and are ignored by default. They only read, and only run when you point them at a folder:

```bash
CLAUDEGATE_REAL_SESSIONS=~/.claudegate/sessions cargo test -p claudegate-core --test real_sessions -- --ignored --nocapture
```

`core/tests/real_projects.rs` works the same way with `CLAUDEGATE_REAL_HOME`.

## Cross-check with the extension (xcheck)

The core ports parts of the extension: the session merge and migration, accept and reject, the no-op prune, the exclude matcher and the settings defaults. `xcheck/` runs the extension's own TypeScript on the fixtures in `fixtures/` and compares the results. It also checks that the bundled `hook.py` is the pinned tag's hook plus its version stamp:

```bash
./xcheck/fetch-extension.sh
cd xcheck && npm ci && npx tsx run.ts
```

The extension tag is pinned as `EXT_TAG` in `xcheck/fetch-extension.sh`. See [Releasing](releasing.md#bumping-the-bundled-hook) for how to move it.

## End-to-end scripts

Each runs headless in a temp sandbox (its own `HOME` and `CLAUDEGATE_HOME`) and never touches your real `~/.claudegate` or `~/.claude`.

| Script | What it checks | Prints on success |
|---|---|---|
| `scripts/decide-e2e.sh` | The extension's hook captures during an accept, and the capture survives the app's write. | `E2E OK` |
| `scripts/capture-e2e.sh` | Capture setup in a temp HOME: a hooked Write lands as a pending entry; the startup hook update and Update hook. | `CAPTURE E2E OK` |
| `scripts/claudegate-wrapper-test.sh` | The `claudegate` launcher's argv handling (`claudegate`, `claudegate log`, `--`). | |
| `scripts/readonly-proof.sh` | Snapshots the ClaudeGate home before and after running the app. See [What the app writes](what-the-app-writes.md#checking-what-it-writes). | `UNCHANGED` |

`scripts/decide-e2e.sh` and `scripts/capture-e2e.sh` need `.xcheck-cache/claudegate` (run `./xcheck/fetch-extension.sh` first).

## Manual test

[`manual-test-decisions.md`](manual-test-decisions.md) is a checklist for running the app next to the VS Code extension on a real machine, with a scratch repo.

## Repository layout

| Path | What it holds |
|---|---|
| `core/` | The Rust core: session model, decisions, store, capture setup, projects, git log. No UI. |
| `src-tauri/` | The Tauri shell: commands, watcher, config, tray, the `claudegate` launcher and the bundled hook in `resources/`. |
| `ui/` | The Svelte 5 UI, with Monaco for diffs. |
| `xcheck/` | Cross-checks the core's ports against the extension's code. |
| `scripts/` | End-to-end checks and `bump-hook.sh`. |
| `fixtures/` | Shared test inputs, such as the exclude matcher cases. |
