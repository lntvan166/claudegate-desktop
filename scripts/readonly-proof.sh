#!/usr/bin/env bash
# Prove the viewer never writes under the ClaudeGate home.
# Usage: readonly-proof.sh [--no-app] [HOME_DIR] [SECONDS]
#   HOME_DIR  default $HOME/.claudegate
#   SECONDS   default 60 (how long the app runs)
#   --no-app  skip launching the app (dry run of the snapshot/diff logic)
# Exit 0 = UNCHANGED, 1 = changed. Never writes inside HOME_DIR.
set -u

NO_APP=0
ARGS=()
for a in "$@"; do
  if [ "$a" = "--no-app" ]; then NO_APP=1; else ARGS+=("$a"); fi
done
HOME_DIR="${ARGS[0]:-$HOME/.claudegate}"
DURATION="${ARGS[1]:-60}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
TAB="$(printf '\t')"

case "$DURATION" in
  ''|*[!0-9]*|0) echo "duration must be a positive integer, got: $DURATION" >&2; exit 2 ;;
esac
if [ -n "${READONLY_PROOF_HEADLESS:-}" ] && ! command -v xvfb-run >/dev/null 2>&1; then
  echo "READONLY_PROOF_HEADLESS is set but xvfb-run is not installed" >&2; exit 2
fi
[ -d "$HOME_DIR" ] || { echo "no such directory: $HOME_DIR" >&2; exit 2; }
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

snap() { find "$HOME_DIR" -type f -printf '%p %s %T@\n' | sort > "$1"; }

START="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
snap "$TMP/before.txt"
echo "before: $(wc -l < "$TMP/before.txt") files, sha256 $(sha256sum < "$TMP/before.txt" | cut -c1-16)"

if [ "$NO_APP" = 0 ]; then
  BIN="$REPO/target/release/claudegate-desktop"
  [ -x "$BIN" ] || BIN="$REPO/target/debug/claudegate-desktop"
  [ -x "$BIN" ] || { echo "no built binary under target/ (run cargo build)" >&2; exit 2; }
  echo "running $BIN for ${DURATION}s against $HOME_DIR"
  if [ -n "${READONLY_PROOF_HEADLESS:-}" ]; then
    CLAUDEGATE_HOME="$HOME_DIR" setsid env -u WAYLAND_DISPLAY GDK_BACKEND=x11 xvfb-run -a "$BIN" &
  else
    CLAUDEGATE_HOME="$HOME_DIR" setsid "$BIN" &
  fi
  PID=$!
  # setsid made PID a process-group leader: kill the whole group on any exit.
  stop_app() { kill -- "-$PID" 2>/dev/null; pkill -P "$PID" 2>/dev/null; kill "$PID" 2>/dev/null; }
  trap 'stop_app; rm -rf "$TMP"' EXIT
  trap 'echo interrupted >&2; exit 130' INT TERM
  sleep "$DURATION" &
  wait $!
  stop_app
  sleep 1
  kill -9 -- "-$PID" 2>/dev/null
  wait "$PID" 2>/dev/null
fi
END="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

snap "$TMP/after.txt"
echo "after:  $(wc -l < "$TMP/after.txt") files, sha256 $(sha256sum < "$TMP/after.txt" | cut -c1-16)"

if cmp -s "$TMP/before.txt" "$TMP/after.txt"; then
  echo UNCHANGED
  exit 0
fi

echo "CHANGED:"
tsv() { awk '{p=$1; for(i=2;i<=NF-2;i++)p=p" "$i; print p "\t" $(NF-1) " " $NF}' "$1" | sort -t "$TAB" -k1,1; }
tsv "$TMP/before.txt" > "$TMP/b.tsv"
tsv "$TMP/after.txt" > "$TMP/a.tsv"
cut -f1 "$TMP/b.tsv" > "$TMP/bp"; cut -f1 "$TMP/a.tsv" > "$TMP/ap"
{
  comm -23 "$TMP/bp" "$TMP/ap" | sed "s/^/removed$TAB/"
  comm -13 "$TMP/bp" "$TMP/ap" | sed "s/^/added$TAB/"
  join -t "$TAB" "$TMP/b.tsv" "$TMP/a.tsv" | awk -F'\t' '$2!=$3{print "modified\t" $1}'
} | sort -t "$TAB" -k2 > "$TMP/changes.tsv"

while IFS="$TAB" read -r kind path; do
  echo "  $kind: $path"
done < "$TMP/changes.tsv"

# hook.log lines are "<iso-ts>Z <event> <workspace file>"; they never name the
# session file, so show what the hook did during the run window instead.
LOG="$HOME_DIR/hook.log"
echo "hook.log activity $START .. $END (UTC, last 20):"
win=""
[ -f "$LOG" ] && win="$(awk -v s="$START" -v e="$END" '$1>=s && $1<=e' "$LOG" | tail -n 20)"
if [ -n "$win" ]; then
  printf '%s\n' "$win" | sed 's/^/  /'
  echo "  (hook or extension wrote during the window; changes above are likely theirs)"
elif grep -q '/sessions/' "$TMP/changes.tsv"; then
  echo "  no hook activity in the window - investigate"
else
  echo "  none"
fi
exit 1
