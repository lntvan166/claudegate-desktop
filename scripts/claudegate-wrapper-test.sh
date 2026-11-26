#!/bin/sh
# Tests src-tauri/claudegate: what argv the app gets. Uses a fake app binary in a temp dir.
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
out=$tmp/argv
cat > "$tmp/fake" <<FAKE
#!/bin/sh
printf '%s\n' "\$@" > "$out.part"
mv "$out.part" "$out"
FAKE
chmod +x "$tmp/fake"
mkdir -p "$tmp/w/sub"
fail=0
run() { # expected (with | between args), then wrapper args
  want=$1; shift
  rm -f "$out"
  (cd "$tmp/w" && CLAUDEGATE_DESKTOP_BIN="$tmp/fake" sh "$here/src-tauri/claudegate" "$@")
  i=0; while [ ! -f "$out" ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done
  got=$(tr '\n' '|' < "$out" 2>/dev/null)
  if [ "$got" = "$want" ]; then echo "ok - claudegate $*"; else echo "FAIL - claudegate $*: got '$got' want '$want'"; fail=1; fi
}
w=$tmp/w
run "$w|"
run "$w/sub|" sub
run "$w/./log|" ./log
run "$w/log|" -- log
run "log|--|$w|" log
run "log|--|$w/sub|" log sub
run "log|--|$w/./log|" log ./log
run "log|--|$w/log|" log -- log
run "log|--|/abs/x|" log /abs/x
# A missing binary is exit 127 with a message.
(CLAUDEGATE_DESKTOP_BIN="$tmp/none" sh "$here/src-tauri/claudegate" log >/dev/null 2>&1); [ $? -eq 127 ] && echo "ok - missing app" || { echo "FAIL - missing app"; fail=1; }
exit $fail
