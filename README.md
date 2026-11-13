# ClaudeGate Desktop

A standalone desktop viewer for the session files written by the ClaudeGate VS Code extension. It reads `~/.claudegate/sessions/` and shows the pending, accepted and rejected changes Claude Code has made, without needing an editor open.

**v0 never writes.** It does not accept, reject or modify session files or your workspace.

## Running

```bash
npm run tauri dev          # the desktop app against your real ~/.claudegate (read-only)
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

### Proving it is read-only

`scripts/readonly-proof.sh [--no-app] [--bin PATH] [--print-bin] [HOME_DIR] [SECONDS]` snapshots every file under the ClaudeGate home (default `~/.claudegate`), runs the built binary for SECONDS (default 60), snapshots again, and prints `UNCHANGED` (exit 0) or each changed, added or removed file with its last `hook.log` mention (exit 1). The extension's hook may legitimately write in that window; the log line shows who. The binary is `--bin PATH`, else the newer of `target/release` and `target/debug`; the script prints its path, build time and the current `HEAD`, and warns when any file under `core/src`, `src-tauri/src` or `ui/src` is newer than it (`--print-bin` prints just that and exits). `--no-app` skips the launch; set `READONLY_PROOF_HEADLESS=1` to run the app under xvfb. Snapshots live in a temp dir, never inside the home. A static check, `core/tests/no_writes.rs`, also fails `cargo test` if any source file outside `src-tauri/src/config.rs` uses a write API.
