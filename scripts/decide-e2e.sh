#!/usr/bin/env bash
# Headless proof that a hook capture made DURING a decision survives it.
# Everything happens inside a mktemp sandbox (HOME, CLAUDEGATE_HOME, the desktop config);
# the real ~/.claudegate and ~/.claudegate-desktop are never touched.
# Needs .xcheck-cache/claudegate (./xcheck/fetch-extension.sh). Prints "E2E OK" and exits 0.
set -euo pipefail
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"  # keep the toolchain once HOME moves
export PATH="$CARGO_HOME/bin:$PATH"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
HOOK="$REPO/.xcheck-cache/claudegate/hooks/hook.py"
[ -f "$HOOK" ] || { echo "missing $HOOK; run ./xcheck/fetch-extension.sh"; exit 1; }

fail() { echo "E2E FAIL: $*"; exit 1; }
SB="$(mktemp -d)"
trap 'rm -rf "$SB"' EXIT

export HOME="$SB/home"
export CLAUDEGATE_HOME="$SB/home/.claudegate"
export CLAUDEGATE_DESKTOP_CONFIG="$SB/home/.claudegate-desktop/config.json"
DESKTOP="$SB/home/.claudegate-desktop"
WS="$SB/ws"
mkdir -p "$HOME" "$CLAUDEGATE_HOME/sessions" "$DESKTOP" "$WS"
printf 'a0\n' > "$WS/a.txt"
printf 'b0\n' > "$WS/b.txt"
printf '{"%s": 1}\n' "$WS" > "$CLAUDEGATE_HOME/workspace-roots.json"

hook() { # hook <file>: a PreToolUse Write capture, as Claude Code sends it
  printf '{"session_id":"e2e","cwd":"%s","hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"%s"}}' "$WS" "$1" \
    | python3 "$HOOK"
}
session() { ls "$CLAUDEGATE_HOME"/sessions/*.json 2>/dev/null | head -1; }
py() { python3 -I -c "import json,sys; s=json.load(open('$(session)')); $1"; }

(cd "$REPO" && cargo build -q -p claudegate-desktop --example decide_e2e) || fail "build"
BIN="$REPO/target/debug/examples/decide_e2e"
[ -x "$BIN" ] || fail "example binary not found"

# 1. Claude edits a.txt: hook snapshots a0, then the file changes.
hook "$WS/a.txt"; printf 'a1\n' > "$WS/a.txt"
[ -n "$(session)" ] || fail "hook wrote no session file"
py "assert '$WS/a.txt' in s['files']" || fail "a.txt not pending after hook"

# 2. Accept a.txt while the hook captures b.txt between the decide's load and persist.
# b.txt must be captured with its ORIGINAL content, so run the hook first, then change the file.
DURING="printf '{\"session_id\":\"e2e\",\"cwd\":\"$WS\",\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"Write\",\"tool_input\":{\"file_path\":\"$WS/b.txt\"}}' | python3 '$HOOK'; printf 'b1\n' > '$WS/b.txt'"
"$BIN" "$WS" "$DESKTOP" accept "$WS/a.txt" "$DURING" || fail "accept"

py "assert [r['path'] for r in s['accepted']] == ['$WS/a.txt'], s['accepted']" || fail "a.txt not in accepted[]"
py "assert '$WS/a.txt' not in s['files']" || fail "a.txt still pending"
py "assert '$WS/b.txt' in s['files'], list(s['files']); assert s['files']['$WS/b.txt']['originalContent']=='b0\n'" \
  || fail "b.txt capture made during the accept was lost"
py "assert s['status']=='active'" || fail "status should be active (b.txt pending)"
N="$(find "$DESKTOP/backups" -name '*.json' | wc -l)"
[ "$N" -eq 1 ] || fail "expected 1 backup, found $N"
[ "$(cat "$WS/a.txt")" = "a1" ] || fail "accept must keep Claude's a.txt"

# 3. Claude edits a.txt again; reject restores the accepted baseline.
hook "$WS/a.txt"; printf 'a2\n' > "$WS/a.txt"
py "assert '$WS/a.txt' in s['files']" || fail "a.txt second edit not pending"
"$BIN" "$WS" "$DESKTOP" reject "$WS/a.txt" || fail "reject"
[ "$(cat "$WS/a.txt")" = "a1" ] || fail "reject did not restore a.txt (got: $(cat "$WS/a.txt"))"
py "assert '$WS/a.txt' in s['rejected'] and '$WS/a.txt' not in s['files']" || fail "a.txt not recorded as rejected"
py "assert '$WS/b.txt' in s['files']" || fail "b.txt lost after reject"

echo "E2E OK"
