# Releasing (automatic semantic versioning)

Stable releases of VmodV are cut automatically by
`.github/workflows/release.yml` on every push to `main`. Versions follow
[SemVer](https://semver.org) and are driven by
[Conventional Commits](https://www.conventionalcommits.org/) subjects.

## Bump rules

| Commit subject / body              | Bump  | Example          |
| ---------------------------------- | ----- | ---------------- |
| `<type>!:` or `BREAKING CHANGE:` in the body | major | 2.1.0 → 3.0.0 |
| `feat:` / `feat(<scope>):`         | minor | 2.1.0 → 2.2.0    |
| `fix:` / `fix(<scope>):`           | patch | 2.1.0 → 2.1.1    |
| Anything else (`chore`, `docs`, `perf`, `refactor`, `test`, `ci`, `style`, `build`…) | none  | no release      |

Precedence when a set of commits contains more than one kind: **major >
minor > patch**. `BREAKING CHANGE:` (or `BREAKING-CHANGE:`) may appear in
any commit body, whatever its type. Types are matched case-insensitively.
Merge commits are ignored.

## How it works

1. On each push to `main`, the `Compute next version` job runs
   `tools/semver-next.sh`. It finds the highest `v*.*.*` tag, scans the
   conventional commits since it and prints the next version (or nothing
   when no release is warranted).
2. If a bump is warranted, the `release` job (which depends on `version`
   only — never on a build job):
   - tags the pushed commit `vX.Y.Z` (annotated, as `github-actions[bot]`) —
     no version-bump commit is created; the tree contains no version
     literal,
   - pushes the tag,
   - publishes a GitHub release titled `vX.Y.Z` with the changelog since the
     previous tag. `gh release create` attaches no files, so the release goes
     out even when no artifact was built.
3. Each artifact is produced **once** by its own job — `build-appimage`
   (native x86_64 and aarch64 legs), `windows-portable`, `macos-portable`
   (both architectures) and `linux-debs` (four Ubuntu bases, amd64-only) — and
   uploaded as a workflow artifact.
   A separate `attach-*` job waits for the release and for that one build:
   when the build succeeds it downloads the artifact and runs
   `gh release upload --clobber`. `build-appimage` and `macos-portable` are
   two-architecture matrix jobs, so their attach jobs also run when only one
   leg succeeds and upload the surviving architecture's AppImage or ZIP. A
   failed, slow or capacity-starved build (a macOS or arm64 runner that never
   acquires capacity, for example) costs only its own assets; the release and
   every other platform's artifacts still go out.
   On pushes to `main` the packaging jobs in `vmodv.yml` and `macos.yml` are
   skipped so the same AppImages (x86_64 and aarch64), Windows ZIP and macOS
   bundles are not built twice for one commit. PRs and other branches still
   package in CI.
4. The tag and create steps are idempotent: re-running a failed workflow
   leaves an existing `vX.Y.Z` tag in place and re-pushes it, and leaves an
   existing release in place, so re-runs after a build failure do not die on
   the tag or release already existing.
5. The release is created as a **draft**; `publish` makes it visible only after
   both `validate` (lint, unit and static guard tests) and `validate-e2e`
   (real-binary suites, on a separate runner) pass. A failed or cancelled run
   can therefore strand an
   invisible draft. `.github/workflows/nightly-cleanup.yml` runs at 03:00 UTC
   and calls `tools/prune-draft-releases.sh` to delete drafts older than a day
   that automation cut from `main`. It matches only `vX.Y.Z`-tagged releases
   authored by a `[bot]` account whose notes carry the workflow's marker —
   published releases, recent drafts, hand-written drafts, git tags and PRs
   are untouched.

This is the only release channel: the old rolling `nightly` prerelease was
removed (the nightly job only prunes stale drafts) — download the latest
`vX.Y.Z` release for current artifacts.

## Version line and tags

- Release tags are `vX.Y.Z` (e.g. `v2.1.0`). The workflow only considers
  tags matching `v[0-9]*.[0-9]*.[0-9]*`; the legacy `3.x` tags (no `v`
  prefix, unreachable from the rewritten history) and `v1.x-final` are
  ignored.
- The `v2.0.0` tag was placed on the commit that set the installer version
  to `2.0.0` (`8b9c40ed`) to seed the automation base. Everything before it
  is accounted for as 2.0.0, so the first automated release computed from it
  was **v2.1.0**; the tag pushes made by the workflow point at the exact
  commit being released rather than at a bot-made version-bump commit.
- `git tag` is the single source of truth for the version. The Makefile
  keeps no `VMODV_VERSION` literal; `VMODV_VERSION` is a make variable
  whose default comes from `tools/version.sh`:
  1. `$VMODV_VERSION` when set in the environment,
  2. the `vX.Y.Z` tag on HEAD when building a tagged checkout,
  3. the highest `vX.Y.Z` tag otherwise (the latest released version),
  4. `0.0.0-dev` when no tag exists or git is unavailable (also the
     fallback for container builds, whose `.git`-less context is fed the
     version by the host instead).

## Building locally with a version

```bash
make version                  # print the version tools/version.sh resolves
make build                    # installers named after the resolved version
VMODV_VERSION=2.2.0 make build   # explicit override
tools/semver-next.sh          # print what the next release would be
tools/semver-next.sh selftest # run the fixture tests (18 checks)
```

The selftest builds scratch git repos in a temp dir and asserts bump
behaviour for feat/fix/breaking/no-op combinations, precedence rules and the
untagged-repository error path.

## Making a release manually

- Preferred: merge to `main` a commit whose subject is `feat:`/`fix:` (or a
  breaking change) — the workflow cuts the release.
- To force a run: **Actions → Release → Run workflow** (`workflow_dispatch`).
  It computes the version from the same rules; it does not accept arbitrary
  version numbers.
- To skip releasing a given change: use a non-feat/fix type (`chore:`,
  `docs:`, `perf:`…). Those never cut a release.
- The release workflow does not run the test suite; `vmodv.yml` gates
  `main`. A build failure never blocks the release: re-run the failed job to
  rebuild and attach just that asset (`gh release upload --clobber` is safe
  to re-run against an existing release).
