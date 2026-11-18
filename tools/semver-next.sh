#!/usr/bin/env bash
# Compute the next semantic version from conventional commits.
#
# Bump rules (highest precedence wins):
#   breaking   subject matches `<type>(<scope>)?!:` (case-insensitive), or a
#              body line matches `BREAKING CHANGE:` / `BREAKING-CHANGE:`
#   minor      subject matches `feat(<scope>)?:`
#   patch      subject matches `fix(<scope>)?:`
#   otherwise  no release is warranted
#
# The base version is the highest `v*. *.*`-style tag (see below); when no such
# tag exists (e.g. the 2.0.0 seed), `NATIVE_VERSION ?= X.Y.Z` from the Makefile
# is used instead. The bare next version is printed on stdout — no output
# means no release. Diagnostics go to stderr.
#
# Usage:
#   tools/semver-next.sh              compute the next version for the repo
#   tools/semver-next.sh --repo DIR   same, for the repo at DIR
#   tools/semver-next.sh selftest     run the built-in fixture tests
set -euo pipefail

REPO=.
ACTION=compute
SCRIPT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")
case "${1:-}" in
  selftest) ACTION=selftest ;;
  --repo) REPO=${2:?--repo needs a directory} ;;
  '') ;;
  *) echo "usage: $0 [--repo DIR | selftest]" >&2; exit 2 ;;
esac

latest_semver_tag() { # repo root is $1
  git -C "$1" tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n1
}

compute() { # repo root is $1; prints bare version or nothing
  local repo=$1 latest base range
  latest=$(latest_semver_tag "$repo")
  if [ -n "$latest" ]; then
    base=${latest#v}
    range="$latest..HEAD"
  else
    base=$(sed -n 's/^NATIVE_VERSION ?= *//p' "$repo/Makefile" 2>/dev/null | head -n1 | tr -d '[:space:]')
    range=HEAD
  fi
  if ! [[ $base =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: no usable base version (latest tag: ${latest:-none}, Makefile NATIVE_VERSION: ${base:-empty})" >&2
    exit 2
  fi

  local breaking=0 feat=0 fix=0 count=0 body subject
  while IFS= read -r -d '' body; do
    count=$((count + 1))
    subject=${body%%$'\n'*}
    if grep -qiE '^[a-z]+(\([^)]*\))?!:' <<<"$subject"; then breaking=1; fi
    if grep -qiE '^BREAKING[- ]CHANGE:' <<<"$body"; then breaking=1; fi
    if grep -qiE '^feat(\([^)]*\))?:' <<<"$subject"; then feat=1; fi
    if grep -qiE '^fix(\([^)]*\))?:' <<<"$subject"; then fix=1; fi
  done < <(git -C "$repo" log --no-merges -z --format='%B' $range)

  if [ "$breaking" -eq 1 ]; then
    bump=major
  elif [ "$feat" -eq 1 ]; then
    bump=minor
  elif [ "$fix" -eq 1 ]; then
    bump=patch
  else
    echo "scanned $count commit(s) since ${latest:-the Makefile base}: nothing releaseable (need feat:/fix:/breaking)" >&2
    exit 0
  fi

  local maj min pat
  IFS=. read -r maj min pat <<<"$base"
  case $bump in
    major) maj=$((maj + 1)); min=0; pat=0 ;;
    minor) min=$((min + 1)); pat=0 ;;
    patch) pat=$((pat + 1)) ;;
  esac
  echo "scanned $count commit(s) since ${latest:-Makefile base $base}; bump=$bump base=$base" >&2
  echo "$maj.$min.$pat"
}

selftest() {
  TMPD=$(mktemp -d)
  trap 'rm -rf "$TMPD"' EXIT
  local tmp=$TMPD
  local fails=0 total=0

  # check <expected> <commit message>...  (multi-line bodies via $'...')
  check() {
    local expected=$1; shift
    total=$((total + 1))
    rm -rf "$tmp/repo"; mkdir -p "$tmp/repo"; cd "$tmp/repo"
    git init -q
    git config user.email selftest@example.com
    git config user.name selftest
    git commit -q --allow-empty -m "chore: base"
    git tag v2.0.0
    local msg
    for msg in "$@"; do
      git commit -q --allow-empty -F - <<<"$msg"
    done
    local got; got=$("$SCRIPT" --repo "$tmp/repo")
    if [ "$got" = "$expected" ]; then
      echo "ok   - expected '${expected:-<no release>}'"
    else
      echo "FAIL - expected '${expected:-<no release>}', got '${got:-<no release>}'" >&2
      fails=$((fails + 1))
    fi
  }

  # feat: -> minor
  check "2.1.0" "feat: add thing"
  check "2.1.0" "feat(native-ui): scoped feature"
  check "2.1.0" "FEAT: case-insensitive type"
  # fix: -> patch
  check "2.0.1" "fix: bug"
  check "2.0.1" "fix(core): scoped fix" "fix: another"
  # breaking: -> major
  check "3.0.0" "feat!: breaking"
  check "3.0.0" "feat(scope)!: scoped breaking"
  check "3.0.0" "fix!: breaking fix"
  check "3.0.0" "chore(scope)!: breaking chore"
  check "3.0.0" $'chore: x\n\nBREAKING CHANGE: module api changed'
  check "3.0.0" $'fix: y\n\nBREAKING-CHANGE: dashed footer'
  # precedence: breaking > feat > fix; feat beats fix
  check "2.1.0" "fix: a" "feat: b"
  check "3.0.0" "feat: a" "fix: b" "feat!: c"
  # non-releaseable types produce no release
  check "" "chore: a" "docs: b" "perf: c" "refactor: d" "test: e" "ci: f" "style: g" "build: h" "revert: i"
  check "" # no commits since the tag
  check "2.1.0" "feat: a" "fix: b" "chore: c" "docs: d"
  # mixed case scope-less breaking with bodies before the !: marker
  check "3.0.0" $'feat(engine): subject\n\nBREAKING CHANGE: footer wins over type'

  # no tags at all -> base comes from Makefile NATIVE_VERSION
  total=$((total + 1))
  rm -rf "$tmp/repo"; mkdir -p "$tmp/repo"; cd "$tmp/repo"
  git init -q
  git config user.email selftest@example.com
  git config user.name selftest
  printf 'NATIVE_VERSION ?= 1.4.5\n' > Makefile
  git add Makefile; git commit -q -m "chore: base"
  git commit -q --allow-empty -m "feat: from makefile base"
  got=$("$SCRIPT" --repo "$tmp/repo" 2>/dev/null)
  if [ "$got" = "1.5.0" ]; then
    echo "ok   - Makefile base '1.4.5' -> '1.5.0'"
  else
    echo "FAIL - Makefile base expected '1.5.0', got '${got:-<no release>}'" >&2
    fails=$((fails + 1))
  fi

  echo "selftest: $((total - fails))/$total checks passed"
  [ "$fails" -eq 0 ]
}

case $ACTION in
  compute) compute "$REPO" ;;
  selftest) selftest ;;
esac
