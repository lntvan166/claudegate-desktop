#!/usr/bin/env bash
# Headless proof that "Set up capture" really captures: in a mktemp sandbox HOME, add a project
# the way the app does, set capture up, then feed the INSTALLED hook (through the registered
# command) a PreToolUse Write and check the pending entry lands in that project's session.
# Also checks the startup update of the app's own hook.py, and Update hook replacing the
# extension's. The real ~/.claude, ~/.claudegate and
# ~/.claudegate-desktop are never touched. Prints "CAPTURE E2E OK" and exits 0.
set -euo pipefail
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"  # keep the toolchain once HOME moves
export PATH="$CARGO_HOME/bin:$PATH"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
# The bundled hook's version, so a hook bump (scripts/bump-hook.sh) needs no edit here.
VER="$(sed -n 's/^# CLAUDEGATE_HOOK_VERSION=//p' "$REPO/src-tauri/resources/hook.py")"

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
[ "$(cat "$HOME/.claudegate/.installed-by-desktop")" = "$VER" ] || fail "marker"
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
"$BIN" update | grep -q "Updated { from: \"v1.0.0\", to: \"$VER\" }" || fail "update did not run"
cmp -s "$HOME/.claudegate/hook.py" "$REPO/src-tauri/resources/hook.py" || fail "hook.py not updated"
[ "$(cat "$HOME/.claudegate/.installed-by-desktop")" = "$VER" ] || fail "marker not updated"
[ "$(md5sum < "$HOME/.claude/settings.json")" = "$SUM" ] || fail "update touched settings.json"
"$BIN" update | grep -q '^Skipped' || fail "a second update was not skipped"

# 5. Update hook: the extension's setup (no marker, its unstamped hook.py) is brought to the bundled
#    hook, with a backup; hook.sh and settings.json are not touched, and capture still works.
rm "$HOME/.claudegate/.installed-by-desktop"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)  # the extension, older\n' > "$HOME/.claudegate/hook.py"
chmod 0640 "$HOME/.claudegate/hook.py"
"$BIN" status | py "import json,sys; v=json.load(sys.stdin); assert v['status']=='vscode' and v['hook']=={'state':'different','installed':None,'bundled':'$VER'}, v" \
  || fail "status before Update hook"
SH="$(md5sum < "$HOME/.claudegate/hook.sh")"
OUT="$("$BIN" update-hook)" || fail "update-hook"
BAK="$(printf '%s' "$OUT" | py "import json,sys; v=json.load(sys.stdin); assert v['capture']['hook']['state']=='upToDate', v; print(v['backup'])")" \
  || fail "update-hook result: $OUT"
case "$BAK" in "$HOME/.claudegate-desktop/backups/hook/hook-"*.py) ;; *) fail "backup at $BAK" ;; esac
grep -q 'the extension, older' "$BAK" || fail "backup content"
py "import os; assert all(os.stat(p).st_mode & 0o777 == 0o640 for p in ['$BAK', os.path.expanduser('~/.claudegate/hook.py')])" || fail "mode not kept"
cmp -s "$HOME/.claudegate/hook.py" "$REPO/src-tauri/resources/hook.py" || fail "hook.py not the bundled one"
[ ! -e "$HOME/.claudegate/.installed-by-desktop" ] || fail "update-hook wrote the marker"
[ "$(md5sum < "$HOME/.claude/settings.json")" = "$SUM" ] || fail "update-hook touched settings.json"
[ "$(md5sum < "$HOME/.claudegate/hook.sh")" = "$SH" ] || fail "update-hook touched hook.sh"
printf 'v1\n' > "$WS/b.txt"
printf '{"session_id":"cap-e2e","cwd":"%s","hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"%s"}}' \
  "$WS" "$WS/b.txt" | "$CMD" || fail "the updated hook failed"
py "import json; s=json.load(open('$SESSION')); f=s['files']['$WS/b.txt']; assert f['reviewStatus']=='pending', f" \
  || fail "the updated hook did not capture b.txt"
set +e; "$BIN" update-hook >/dev/null 2>&1; rc=$?; set -e
[ "$rc" -eq 1 ] || fail "a second update-hook was not refused (exit $rc)"

echo "CAPTURE E2E OK"
