#!/usr/bin/env bash
# Bump the hook this app bundles to an extension release: scripts/bump-hook.sh <tag> (e.g. v1.17.0).
#
# 1. pins <tag> in xcheck/fetch-extension.sh (EXT_TAG, the only place the tag lives);
# 2. re-fetches .xcheck-cache/claudegate at that tag (a cache at another tag is replaced);
# 3. regenerates src-tauri/resources/hook.py: the tag's hooks/hook.py with
#    `# CLAUDEGATE_HOOK_VERSION=<tag>` as line 2, after the shebang (where `adoptable_hook` and the
#    xcheck look for it);
# 4. runs the xcheck, which fails if the bundled hook is not the tag's hook plus that one line.
#
# Commit the two files it changes (fetch-extension.sh, resources/hook.py) yourself; it never
# commits, tags or touches anything under your HOME. The ports the xcheck compares
# (core/src/capture.rs and friends) may need updating if the extension changed them.
set -euo pipefail
TAG="${1:-}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage: scripts/bump-hook.sh vMAJOR.MINOR.PATCH" >&2; exit 2; }
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
FETCH=xcheck/fetch-extension.sh
CACHE=.xcheck-cache/claudegate
OUT=src-tauri/resources/hook.py

grep -q '^EXT_TAG="v[0-9.]*"$' "$FETCH" || { echo "no EXT_TAG line in $FETCH" >&2; exit 1; }
sed -i.bak "s/^EXT_TAG=\"v[0-9.]*\"$/EXT_TAG=\"$TAG\"/" "$FETCH" && rm -f "$FETCH.bak"

if [ -d "$CACHE" ] && [ "$(git -C "$CACHE" describe --tags --exact-match 2>/dev/null || true)" != "$TAG" ]; then
  rm -rf "$CACHE"
fi
"$FETCH"
SRC="$CACHE/hooks/hook.py"
[ "$(head -n1 "$SRC")" = "#!/usr/bin/env python3" ] || { echo "$SRC: line 1 is not the python3 shebang" >&2; exit 1; }
if grep -q '^# CLAUDEGATE_HOOK_VERSION=' "$SRC"; then
  echo "$SRC already carries a stamp line" >&2
  exit 1
fi

TMP="$(mktemp "$OUT.XXXXXX")"
trap 'rm -f "$TMP"' EXIT
{ head -n1 "$SRC"; printf '# CLAUDEGATE_HOOK_VERSION=%s\n' "$TAG"; tail -n +2 "$SRC"; } > "$TMP"
chmod 0644 "$TMP"
mv "$TMP" "$OUT"
trap - EXIT

(cd xcheck && npx tsx run.ts)
echo "bundled hook is now $TAG: review and commit $FETCH and $OUT"
