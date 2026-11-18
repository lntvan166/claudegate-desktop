#!/usr/bin/env bash
# Show what the app writes under the ClaudeGate home while it runs.
# Usage: readonly-proof.sh [--no-app] [--bin PATH] [--print-bin] [HOME_DIR] [SECONDS]
#   HOME_DIR     default $HOME/.claudegate
#   SECONDS      default 60 (how long the app runs)
#   --no-app     skip launching the app (dry run of the snapshot/diff logic)
#   --bin PATH   the binary to run; default is the newer of target/release and target/debug
#   --print-bin  report which binary would run (path, build time, HEAD, staleness) and exit
# Exit 0 = UNCHANGED, 1 = changed. Never writes inside HOME_DIR.
# It sees the app's own writes as well as the hook's: for projects the app added, the startup
# exclude sync (excludes.json), the re-add of a dropped root (workspace-roots.json) and session
# upkeep can all change files here, read-only mode or not for the first two. Expect UNCHANGED
# only on a home where the app owns no projects, or nothing is out of sync. See README.md,
# "What the app writes".
set -u

NO_APP=0
PRINT_BIN=0
BIN=""
ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --no-app) NO_APP=1 ;;
    --print-bin) PRINT_BIN=1 ;;
    --bin)
      [ $# -ge 2 ] || { echo "--bin needs a path" >&2; exit 2; }
      BIN="$2"; shift ;;
    *) ARGS+=("$1") ;;
  esac
  shift
done
HOME_DIR="${ARGS[0]:-$HOME/.claudegate}"
DURATION="${ARGS[1]:-60}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
TAB="$(printf '\t')"

# The binary to prove: --bin, else the newest of the two profiles. Preferring release
# whenever it existed let a stale release build "prove" code it did not contain.
pick_bin() {
  if [ -n "$BIN" ]; then
    [ -x "$BIN" ] || { echo "--bin is not an executable file: $BIN" >&2; exit 2; }
    return
  fi
  local rel="$REPO/target/release/claudegate-desktop" dbg="$REPO/target/debug/claudegate-desktop"
  if [ -x "$rel" ] && [ -x "$dbg" ]; then
    if [ "$dbg" -nt "$rel" ]; then BIN="$dbg"; else BIN="$rel"; fi
  elif [ -x "$rel" ]; then BIN="$rel"
  elif [ -x "$dbg" ]; then BIN="$dbg"
  else echo "no built binary under target/ (run cargo build, or pass --bin)" >&2; exit 2
  fi
}

describe_bin() {
  echo "binary: $BIN (built $(date -r "$BIN" '+%Y-%m-%d %H:%M:%S %z'))"
  echo "HEAD: $(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  local newer=""
  for d in core/src src-tauri/src ui/src; do
    [ -d "$REPO/$d" ] || continue
    newer="$(find "$REPO/$d" -type f -newer "$BIN" -print 2>/dev/null | head -n 1)"
    [ -n "$newer" ] && break
  done
  if [ -n "$newer" ]; then
    echo "WARNING: $BIN is older than ${newer#"$REPO"/}; rebuild, or this proves stale code" >&2
  fi
}

if [ "$PRINT_BIN" = 1 ]; then
  pick_bin
  describe_bin
  exit 0
fi

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
  pick_bin
  describe_bin
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
echo "  (the app's own writes show here too: exclude sync, re-added roots, upkeep of its projects)"
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
