#!/usr/bin/env bash
# Headless proof that "Set up capture" really captures: in a mktemp sandbox HOME, add a project
# the way the app does, set capture up, then feed the INSTALLED hook (through the registered
# command) a PreToolUse Write and check the pending entry lands in that project's session.
# Also checks the startup update of the app's own hook.py. The real ~/.claude, ~/.claudegate and
# ~/.claudegate-desktop are never touched. Prints "CAPTURE E2E OK" and exits 0.
set -euo pipefail
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"  # keep the toolchain once HOME moves
export PATH="$CARGO_HOME/bin:$PATH"
REPO="$(cd "$(dirname "$0")/.." && pwd)"

fail() { echo "CAPTURE E2E FAIL: $*"; exit 1; }
SB="$(mktemp -d)"
trap 'rm -rf "$SB"' EXIT

(cd "$REPO" && cargo build -q -p claudegate-desktop --example capture_e2e) || fail "build"
BIN="$REPO/target/debug/examples/capture_e2e"
[ -x "$BIN" ] || fail "example binary not found"

export HOME="$SB/home"
export CLAUDEGATE_HOME="$HOME/.claudegate"
export CLAUDEGATE_DESKTOP_CONFIG="$HOME/.claudegate-desktop/config.json"
WS="$SB/ws"
mkdir -p "$HOME/.claude" "$WS"
printf 'v0\n' > "$WS/a.txt"
# An existing Claude Code config with someone else's hook: setup must keep both.
cat > "$HOME/.claude/settings.json" <<'JSON'
{
  "model": "opus",
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "/usr/local/bin/other-hook" } ] }
    ]
  }
}
JSON

# 0. The driver refuses to run without the sandbox variables, or with them outside HOME.
for unset in CLAUDEGATE_HOME CLAUDEGATE_DESKTOP_CONFIG; do
  set +e; env -u "$unset" "$BIN" status >/dev/null 2>&1; rc=$?; set -e
  [ "$rc" -eq 2 ] || fail "capture_e2e ran without $unset (exit $rc, want 2)"
done
set +e; CLAUDEGATE_HOME="$SB/outside" "$BIN" status >/dev/null 2>&1; rc=$?; set -e
[ "$rc" -eq 2 ] || fail "capture_e2e ran with CLAUDEGATE_HOME outside HOME (exit $rc, want 2)"

py() { python3 -I -c "$1"; }

# 1. Add the project the app's way, then set capture up.
"$BIN" status | py "import json,sys; v=json.load(sys.stdin); assert v['status']=='not-set-up', v" || fail "status before setup"
"$BIN" add "$WS" >/dev/null || fail "add project"
OUT="$("$BIN" setup)" || fail "setup"
printf '%s' "$OUT" | py "import json,sys; v=json.load(sys.stdin); assert v['capture']['status']=='app', v; assert v['backup'] and v['note'] is None, v" \
  || fail "setup result: $OUT"
[ -f "$HOME/.claudegate/hook.py" ] && [ -x "$HOME/.claudegate/hook.sh" ] || fail "hook files not installed"
[ "$(cat "$HOME/.claudegate/.installed-by-desktop")" = "v1.16.1" ] || fail "marker"
ls "$HOME/.claude/settings.json.claudegate-"*.bak >/dev/null 2>&1 || fail "no settings backup"
CMD="$(py "
import json,os
s=json.load(open(os.path.expanduser('~/.claude/settings.json')))
assert s['model']=='opus', s
pre=s['hooks']['PreToolUse']
assert any(h['command']=='/usr/local/bin/other-hook' for e in pre for h in e['hooks']), pre
ours=[h['command'] for e in pre if e.get('matcher')=='^(Write|Edit|MultiEdit|Bash)\$' for h in e['hooks']]
assert len(ours)==1, pre
print(ours[0])")" || fail "settings.json after setup"
[ "$CMD" = "$HOME/.claudegate/hook.sh" ] || fail "registered command is $CMD"
set +e; "$BIN" setup >/dev/null 2>&1; rc=$?; set -e
[ "$rc" -eq 1 ] || fail "a second setup was not refused (exit $rc)"

# 2. Claude Code runs the registered command before a Write in the project.
printf '{"session_id":"cap-e2e","cwd":"%s","hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"%s"}}' \
  "$WS" "$WS/a.txt" | "$CMD" || fail "the installed hook failed"
printf 'v1\n' > "$WS/a.txt"

# 3. The pending entry is in that project's session.
SESSION="$CLAUDEGATE_HOME/sessions/$(printf '%s' "$(cd "$WS" && pwd -P)" | md5sum | cut -d' ' -f1).json"
[ -f "$SESSION" ] || fail "no session for the project ($(ls "$CLAUDEGATE_HOME/sessions" 2>&1))"
py "import json; s=json.load(open('$SESSION')); f=s['files']['$WS/a.txt']; assert f['reviewStatus']=='pending' and f['originalContent']=='v0\n', f" \
  || fail "a.txt is not pending with its original content"
echo "capture: pending entry for a.txt in $(basename "$SESSION")"

# 4. Startup update: an older app-installed hook is replaced, settings.json is not touched.
printf 'v1.0.0' > "$HOME/.claudegate/.installed-by-desktop"
printf '#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\n' > "$HOME/.claudegate/hook.py"
SUM="$(md5sum < "$HOME/.claude/settings.json")"
"$BIN" update | grep -q 'Updated { from: "v1.0.0", to: "v1.16.1" }' || fail "update did not run"
cmp -s "$HOME/.claudegate/hook.py" "$REPO/src-tauri/resources/hook.py" || fail "hook.py not updated"
[ "$(cat "$HOME/.claudegate/.installed-by-desktop")" = "v1.16.1" ] || fail "marker not updated"
[ "$(md5sum < "$HOME/.claude/settings.json")" = "$SUM" ] || fail "update touched settings.json"
"$BIN" update | grep -q '^Skipped' || fail "a second update was not skipped"

echo "CAPTURE E2E OK"
