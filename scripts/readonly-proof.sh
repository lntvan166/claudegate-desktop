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

[ -d "$HOME_DIR" ] || { echo "no such directory: $HOME_DIR" >&2; exit 2; }
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

snap() { find "$HOME_DIR" -type f -printf '%p %s %T@\n' | sort > "$1"; }

snap "$TMP/before.txt"
echo "before: $(wc -l < "$TMP/before.txt") files, sha256 $(sha256sum < "$TMP/before.txt" | cut -c1-16)"

if [ "$NO_APP" = 0 ]; then
  BIN="$REPO/target/release/claudegate-desktop"
  [ -x "$BIN" ] || BIN="$REPO/target/debug/claudegate-desktop"
  [ -x "$BIN" ] || { echo "no built binary under target/ (run cargo build)" >&2; exit 2; }
  echo "running $BIN for ${DURATION}s against $HOME_DIR"
  if [ -n "${READONLY_PROOF_HEADLESS:-}" ]; then
    CLAUDEGATE_HOME="$HOME_DIR" env -u WAYLAND_DISPLAY GDK_BACKEND=x11 xvfb-run -a "$BIN" &
  else
    CLAUDEGATE_HOME="$HOME_DIR" "$BIN" &
  fi
  PID=$!
  sleep "$DURATION"
  # stop the app (and xvfb-run's children)
  pkill -P "$PID" 2>/dev/null
  kill "$PID" 2>/dev/null
  sleep 1
  kill -9 "$PID" 2>/dev/null
  wait "$PID" 2>/dev/null
fi

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

LOG="$HOME_DIR/hook.log"
while IFS="$TAB" read -r kind path; do
  echo "  $kind: $path"
  if [ -f "$LOG" ]; then
    last="$(grep -F -- "$path" "$LOG" | tail -n 1)"
    [ -n "$last" ] && echo "    last hook.log: $last"
  fi
done < "$TMP/changes.tsv"
exit 1
