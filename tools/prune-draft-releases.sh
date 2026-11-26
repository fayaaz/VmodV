#!/usr/bin/env bash
# Nightly cleanup of stale automated draft releases.
#
# release.yml tags a commit and creates its GitHub release as a *draft* before
# validation runs; `publish` flips it visible only if validation succeeds. A
# failed or cancelled main run therefore leaves an invisible draft behind. This
# script deletes drafts that automation cut from `main` and that are older than
# a day (MAX_AGE_SECONDS), leaving published releases, recent drafts,
# hand-written drafts and all git tags alone. Tags are deliberately preserved:
# they are the version source of truth for tools/semver-next.sh.
#
# Test seam: RELEASES_TSV_FILE supplies pre-projected rows (tag, created_at,
# author login, body) instead of calling `gh api`; DRY_RUN=true only reports.
set -euo pipefail

MAX_AGE_SECONDS="${MAX_AGE_SECONDS:-86400}"
MARKER='Automated release cut from `main` by the semantic-versioning workflow.'
REPO="${GITHUB_REPOSITORY:-${GH_REPO:-}}"
DRY_RUN="${DRY_RUN:-false}"
SEMVER_RE='^v[0-9]+\.[0-9]+\.[0-9]+$'

fail() {
    printf 'prune-draft-releases: %s\n' "$*" >&2
    exit 1
}

[ -n "$REPO" ] || fail 'GITHUB_REPOSITORY is not set'

if [ -n "${RELEASES_TSV_FILE:-}" ]; then
    rows=$(cat "$RELEASES_TSV_FILE")
else
    # gh's --jq uses its built-in gojq, so no system jq is required. @tsv keeps
    # one row per draft and escapes embedded newlines in release bodies.
    rows=$(gh api --paginate "repos/${REPO}/releases" --jq '
        .[]
        | select(.draft == true)
        | select((.author.login // "") | endswith("[bot]"))
        | [.tag_name, .created_at, (.author.login // ""), (.body // "")]
        | @tsv
    ')
fi

now=$(date -u +%s)
cutoff=$((now - MAX_AGE_SECONDS))
pruned=0

while IFS=$'\t' read -r tag created author body; do
    [ -n "$tag" ] || continue

    # Only semantic-version releases cut by the automated workflow from main.
    [[ "$tag" =~ $SEMVER_RE ]] || continue
    [[ "$author" == *'[bot]' ]] || continue
    [[ "$body" == *"$MARKER"* ]] || continue

    created_epoch=$(date -u -d "$created" +%s) || continue
    ((created_epoch < cutoff)) || continue

    if [ "$DRY_RUN" = 'true' ]; then
        printf 'would prune %s\n' "$tag"
    else
        gh release delete "$tag" --yes --repo "$REPO"
        printf 'pruned %s\n' "$tag"
    fi
    pruned=$((pruned + 1))
done <<< "$rows"

printf 'pruned %d stale automated draft release(s)\n' "$pruned"
