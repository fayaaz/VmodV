#!/usr/bin/env bash
# Print the VmodV release version, bare (no leading `v`).
#
# The git tags are the single source of truth for the version; no version
# literal is kept in the tree. Precedence:
#   1. $NATIVE_VERSION when set (explicit override, e.g. `NATIVE_VERSION=2.2.0 make build`)
#   2. the `vX.Y.Z` tag on HEAD, if any (a release checkout)
#   3. the highest `vX.Y.Z` tag (local builds; matches the latest release)
#   4. `0.0.0-dev` when no such tag exists or git is unavailable
#
# The Docker build context excludes .git, so container builds receive the
# version from the host via --build-arg / -e APP_VERSION instead of running
# this script.
set -euo pipefail

if [ -n "${NATIVE_VERSION:-}" ]; then
  echo "$NATIVE_VERSION"
  exit 0
fi

ROOT="${ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

tag=""
if command -v git >/dev/null 2>&1 && [ -d "$ROOT/.git" ]; then
  tag=$(git -C "$ROOT" describe --tags --exact-match --match 'v[0-9]*.[0-9]*.[0-9]*' HEAD 2>/dev/null || true)
  if [ -z "$tag" ]; then
    tag=$(git -C "$ROOT" tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n1)
  fi
fi

if [ -z "$tag" ]; then
  echo "0.0.0-dev"
  exit 0
fi
echo "${tag#v}"
