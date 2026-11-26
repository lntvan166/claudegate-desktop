#!/usr/bin/env bash
# Bump the hooks this app bundles to an extension release: scripts/bump-hook.sh <tag> (e.g. v1.17.0).
#
# 1. pins <tag> in xcheck/fetch-extension.sh (EXT_TAG, the only place the tag lives);
# 2. re-fetches .xcheck-cache/claudegate at that tag (a cache at another tag is replaced);
# 3. regenerates src-tauri/resources/hook.py: the tag's hooks/hook.py with
#    `# CLAUDEGATE_HOOK_VERSION=<tag>` as line 2, after the shebang (where `adoptable_hook` and the
#    xcheck look for it), and src-tauri/resources/audit.py: the tag's hooks/audit.py, byte for
#    byte (it carries no stamp; audit.sh is not a file there, the app writes AUDIT_SH_CONTENT);
# 4. runs the xcheck, which fails if the bundled hooks are not the tag's (plus that one line).
#
# Before a release is tagged: scripts/bump-hook.sh --from-cache <version> bundles from whatever
# .xcheck-cache/claudegate has checked out now (a local clone at a branch, say), stamped <version>
# (the planned release). It neither pins EXT_TAG nor fetches; it prints the ref it bundled, and
# refuses a cache with uncommitted changes. Re-run
# the plain form once <version> is tagged, so the pin and the bundle agree again.
#
# Commit the files it changes (fetch-extension.sh, resources/hook.py, resources/audit.py)
# yourself; it never commits, tags or touches anything under your HOME. The ports the xcheck
# compares (core/src/capture.rs and friends) may need updating if the extension changed them.
set -euo pipefail
FROM_CACHE=0
if [ "${1:-}" = "--from-cache" ]; then
  FROM_CACHE=1
  shift
fi
TAG="${1:-}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "usage: scripts/bump-hook.sh vMAJOR.MINOR.PATCH | scripts/bump-hook.sh --from-cache vMAJOR.MINOR.PATCH" >&2
  exit 2
}
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
FETCH=xcheck/fetch-extension.sh
CACHE=.xcheck-cache/claudegate
OUT=src-tauri/resources/hook.py
AUDIT_OUT=src-tauri/resources/audit.py

if [ "$FROM_CACHE" = 1 ]; then
  [ -d "$CACHE/.git" ] || { echo "no checkout at $CACHE" >&2; exit 1; }
  # A dirty tree is no ref at all: the bundle could not be reproduced from what it names.
  if [ -n "$(git -C "$CACHE" status --porcelain)" ]; then
    echo "$CACHE has uncommitted changes: commit or discard them first" >&2
    git -C "$CACHE" status --short >&2
    exit 1
  fi
  echo "bundling from $CACHE at $(git -C "$CACHE" rev-parse --abbrev-ref HEAD) $(git -C "$CACHE" rev-parse --short HEAD), stamped $TAG"
else
  grep -q '^EXT_TAG="v[0-9.]*"$' "$FETCH" || { echo "no EXT_TAG line in $FETCH" >&2; exit 1; }
  sed -i.bak "s/^EXT_TAG=\"v[0-9.]*\"$/EXT_TAG=\"$TAG\"/" "$FETCH" && rm -f "$FETCH.bak"
  if [ -d "$CACHE" ] && [ "$(git -C "$CACHE" describe --tags --exact-match 2>/dev/null || true)" != "$TAG" ]; then
    rm -rf "$CACHE"
  fi
  "$FETCH"
fi
SRC="$CACHE/hooks/hook.py"
AUDIT_SRC="$CACHE/hooks/audit.py"
[ "$(head -n1 "$SRC")" = "#!/usr/bin/env python3" ] || { echo "$SRC: line 1 is not the python3 shebang" >&2; exit 1; }
if grep -q '^# CLAUDEGATE_HOOK_VERSION=' "$SRC"; then
  echo "$SRC already carries a stamp line" >&2
  exit 1
fi
[ -f "$AUDIT_SRC" ] || { echo "$AUDIT_SRC is missing: this ref predates the capture audit" >&2; exit 1; }

TMP="$(mktemp "$OUT.XXXXXX")"
ATMP="$(mktemp "$AUDIT_OUT.XXXXXX")"
trap 'rm -f "$TMP" "$ATMP"' EXIT
{ head -n1 "$SRC"; printf '# CLAUDEGATE_HOOK_VERSION=%s\n' "$TAG"; tail -n +2 "$SRC"; } > "$TMP"
cp "$AUDIT_SRC" "$ATMP"
chmod 0644 "$TMP" "$ATMP"
mv "$TMP" "$OUT"
mv "$ATMP" "$AUDIT_OUT"
trap - EXIT

(cd xcheck && npx tsx run.ts)
if [ "$FROM_CACHE" = 1 ]; then
  echo "bundled hooks are now $TAG from the cache: review and commit $OUT and $AUDIT_OUT"
else
  echo "bundled hooks are now $TAG: review and commit $FETCH, $OUT and $AUDIT_OUT"
fi
