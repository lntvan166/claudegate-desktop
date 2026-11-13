#!/usr/bin/env bash
# Pinned checkout of the ClaudeGate extension; the tag lives here and only here.
EXT_TAG="v1.16.1"
set -euo pipefail
cd "$(dirname "$0")/.."
if [ ! -d .xcheck-cache/claudegate ]; then
  git clone --depth 1 --branch "$EXT_TAG" https://github.com/lntvan166/claudegate .xcheck-cache/claudegate
fi
