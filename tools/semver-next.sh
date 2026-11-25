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
# The base version is the highest `v*.*.*`-style tag; if none exists the
# script errors out (the tree keeps no version literal, so the tags are the
# only source of truth). The bare next version is printed on stdout — no
# output means no release. Diagnostics go to stderr.
#
# Resume: when HEAD is already tagged with a strictly-valid `vX.Y.Z`, that
# version is printed instead of scanning commits (highest tag wins if there
# are several). `release.yml` pushes the tag before creating the release, so
# a cancelled/failed run leaves the tag at HEAD; a rerun must recompute the
# same version for the idempotent release/publish jobs to finish the job.
# Non-semver tags at HEAD (e.g. `nightly`) never trigger a resume.
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

head_semver_tag() { # repo root is $1; highest strictly-valid vX.Y.Z at HEAD, or nothing
  local repo=$1 tag base
  while IFS= read -r tag; do
    [ -n "$tag" ] || continue
    base=${tag#v}
    if [[ $base =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
      echo "$base"
      return 0
    fi
  done < <(git -C "$repo" tag --points-at HEAD --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname)
  return 0
}

compute() { # repo root is $1; prints bare version or nothing
  local repo=$1 latest base range resume
  latest=$(latest_semver_tag "$repo")
  if [ -z "$latest" ]; then
    echo "error: no vX.Y.Z tag found; tag an initial release (e.g. v2.0.0) first" >&2
    exit 2
  fi
  resume=$(head_semver_tag "$repo")
  if [ -n "$resume" ]; then
    echo "resuming release: HEAD is already tagged v$resume" >&2
    echo "$resume"
    return 0
  fi
  base=${latest#v}
  if ! [[ $base =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: tag $latest is not a bare vX.Y.Z version" >&2
    exit 2
  fi
  range="$latest..HEAD"

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
    echo "scanned $count commit(s) since ${latest}: nothing releaseable (need feat:/fix:/breaking)" >&2
    exit 0
  fi

  local maj min pat
  IFS=. read -r maj min pat <<<"$base"
  case $bump in
    major) maj=$((maj + 1)); min=0; pat=0 ;;
    minor) min=$((min + 1)); pat=0 ;;
    patch) pat=$((pat + 1)) ;;
  esac
  echo "scanned $count commit(s) since ${latest}; bump=$bump base=$base" >&2
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

  # check_head_tag <expected> <commit message> <tag>...
  #   base commit tagged v2.0.0, then <commit message>, then every <tag> on HEAD
  check_head_tag() {
    local expected=$1 msg=$2; shift 2
    total=$((total + 1))
    rm -rf "$tmp/repo"; mkdir -p "$tmp/repo"; cd "$tmp/repo"
    git init -q
    git config user.email selftest@example.com
    git config user.name selftest
    git commit -q --allow-empty -m "chore: base"
    git tag v2.0.0
    git commit -q --allow-empty -m "$msg"
    local tag
    for tag in "$@"; do
      git tag "$tag"
    done
    local got; got=$("$SCRIPT" --repo "$tmp/repo" 2>/dev/null)
    if [ "$got" = "$expected" ]; then
      echo "ok   - expected '${expected:-<no release>}'"
    else
      echo "FAIL - expected '${expected:-<no release>}', got '${got:-<no release>}'" >&2
      fails=$((fails + 1))
    fi
  }

  # feat: -> minor
  check "2.1.0" "feat: add thing"
  check "2.1.0" "feat(vmodv-ui): scoped feature"
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
  # HEAD sitting exactly on the release tag is a resumable release (see check_head_tag)
  check "2.0.0"
  check "2.1.0" "feat: a" "fix: b" "chore: c" "docs: d"
  # mixed case scope-less breaking with bodies before the !: marker
  check "3.0.0" $'feat(engine): subject\n\nBREAKING CHANGE: footer wins over type'

  # resuming a release whose tag was pushed but whose release never appeared
  check_head_tag "2.1.0" "feat: add thing" v2.1.0
  check_head_tag "2.1.0" "chore: nothing releaseable" v2.1.0
  check_head_tag "2.2.0" "feat: x" v2.1.0 v2.2.0
  check_head_tag "2.0.1" "fix: bug" nightly

  # no tags at all -> error out (the tree keeps no version literal)
  total=$((total + 1))
  rm -rf "$tmp/repo"; mkdir -p "$tmp/repo"; cd "$tmp/repo"
  git init -q
  git config user.email selftest@example.com
  git config user.name selftest
  git commit -q --allow-empty -m "feat: untagged repo"
  if "$SCRIPT" --repo "$tmp/repo" >/dev/null 2>&1; then
    echo "FAIL - no tags: expected non-zero exit" >&2
    fails=$((fails + 1))
  else
    echo "ok   - no tags: exits with an error"
  fi

  echo "selftest: $((total - fails))/$total checks passed"
  [ "$fails" -eq 0 ]
}

case $ACTION in
  compute) compute "$REPO" ;;
  selftest) selftest ;;
esac
