#!/usr/bin/env bash
# Headless proof that "Set up capture" really captures: in a mktemp sandbox HOME, add a project
# the way the app does, set capture up, then feed the INSTALLED hook (through the registered
# command) a PreToolUse Write and check the pending entry lands in that project's session; then a
# Bash write through both registered commands, the capture hook and the capture-audit hook, and
# check the audit line counts it as captured.
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
[ -f "$HOME/.claudegate/audit.py" ] && [ -x "$HOME/.claudegate/audit.sh" ] || fail "audit files not installed"
cmp -s "$HOME/.claudegate/audit.py" "$REPO/src-tauri/resources/audit.py" || fail "audit.py not the bundled one"
ACMD="$(py "
import json,os
s=json.load(open(os.path.expanduser('~/.claude/settings.json')))
cmds=set()
for ev in ('PostToolUse','PostToolUseFailure'):
    ours=[h['command'] for e in s['hooks'][ev] if e.get('matcher')=='^Bash\$' for h in e['hooks']]
    assert len(ours)==1, (ev, s['hooks'][ev])
    cmds.add(ours[0])
assert len(cmds)==1, cmds
assert list(s['hooks']).index('PreToolUse') < list(s['hooks']).index('PostToolUse') < list(s['hooks']).index('PostToolUseFailure'), list(s['hooks'])
print(cmds.pop())")" || fail "audit entries in settings.json after setup"
[ "$ACMD" = "$HOME/.claudegate/audit.sh" ] || fail "registered audit command is $ACMD"
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

# 3b. A Bash write: the capture hook before it, the audit hook after it, both as registered.
BASH_IN="$(printf '{"session_id":"cap-e2e","tool_use_id":"toolu_e2e_1","cwd":"%s","tool_name":"Bash","tool_input":{"command":"printf v2 > c.txt"}' "$WS")"
printf '%s,"hook_event_name":"PreToolUse"}' "$BASH_IN" | "$CMD" || fail "the installed hook failed on Bash"
printf 'v2' > "$WS/c.txt"
OUT="$(printf '%s,"hook_event_name":"PostToolUse","tool_response":{}}' "$BASH_IN" | "$ACMD" 2>&1)" || fail "the audit hook failed"
[ -z "$OUT" ] || fail "the audit hook printed: $OUT"
py "
import glob,json,os
lines=[json.loads(l) for f in glob.glob(os.path.expanduser('~/.claudegate/audit/*.jsonl')) for l in open(f)]
mine=[l for l in lines if l['toolUseId']=='toolu_e2e_1']
assert len(mine)==1, lines
assert mine[0]['captured']==1 and mine[0]['missedCount']==0, mine[0]
" || fail "the audit line does not count c.txt as captured"
echo "audit: c.txt captured and audited"

# 4. Startup update: an older app-installed hook is replaced, settings.json is not touched.
printf 'v1.0.0' > "$HOME/.claudegate/.installed-by-desktop"
printf '#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\n' > "$HOME/.claudegate/hook.py"
printf '# an older audit.py\n' > "$HOME/.claudegate/audit.py"
SUM="$(md5sum < "$HOME/.claude/settings.json")"
ASH="$(md5sum < "$HOME/.claudegate/audit.sh")"
"$BIN" update | grep -q "Updated { from: \"v1.0.0\", to: \"$VER\" }" || fail "update did not run"
cmp -s "$HOME/.claudegate/hook.py" "$REPO/src-tauri/resources/hook.py" || fail "hook.py not updated"
cmp -s "$HOME/.claudegate/audit.py" "$REPO/src-tauri/resources/audit.py" || fail "audit.py not updated alongside"
[ "$(md5sum < "$HOME/.claudegate/audit.sh")" = "$ASH" ] || fail "update touched audit.sh"
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

# 6. The startup update keeps a stamped hook current with no marker (the extension registered the
#    entry), and never creates the marker; it never touches an unstamped hook.
printf '#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\n' > "$HOME/.claudegate/hook.py"
"$BIN" update | grep -q "Updated { from: \"v1.0.0\", to: \"$VER\" }" || fail "startup update skipped a stamped hook with no marker"
cmp -s "$HOME/.claudegate/hook.py" "$REPO/src-tauri/resources/hook.py" || fail "stamped hook not updated"
[ ! -e "$HOME/.claudegate/.installed-by-desktop" ] || fail "startup update created the marker"
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(0)  # the extension\n' > "$HOME/.claudegate/hook.py"
"$BIN" update | grep -q '^Skipped' || fail "startup update touched an unstamped hook"
grep -q 'the extension' "$HOME/.claudegate/hook.py" || fail "unstamped hook changed"

echo "CAPTURE E2E OK"
