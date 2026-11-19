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
2. If a bump is warranted, the `release` job:
   - tags the pushed commit `vX.Y.Z` (annotated, as `github-actions[bot]`) —
     no version-bump commit is created; the tree contains no version
     literal,
   - pushes the tag,
   - builds the AppImage and `.deb` via `make build` with
     `NATIVE_VERSION=vX.Y.Z` (same artifacts as `make build` locally),
   - publishes a GitHub release titled `vX.Y.Z` with the changelog since the
     previous tag and the two installers attached.
3. The tag step is idempotent: re-running a failed workflow leaves an
   existing `vX.Y.Z` tag in place and re-pushes it, so re-runs after a
   build failure do not die on the tag already existing.

This is the only release channel: the old rolling `nightly` prerelease was
removed — download the latest `vX.Y.Z` release for current artifacts.

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
  keeps no `NATIVE_VERSION` literal; `NATIVE_VERSION` is a make variable
  whose default comes from `tools/version.sh`:
  1. `$NATIVE_VERSION` when set in the environment,
  2. the `vX.Y.Z` tag on HEAD when building a tagged checkout,
  3. the highest `vX.Y.Z` tag otherwise (the latest released version),
  4. `0.0.0-dev` when no tag exists or git is unavailable (also the
     fallback for container builds, whose `.git`-less context is fed the
     version by the host instead).

## Building locally with a version

```bash
make version                  # print the version tools/version.sh resolves
make build                    # installers named after the resolved version
NATIVE_VERSION=2.2.0 make build   # explicit override
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
- The release workflow does not run the test suite; `native.yml` gates
  `main`. If the installer build fails, the tag remains pushed — re-run the
  failed job to rebuild and publish (the `gh release create` step is safe
  to re-run when the release does not exist yet).
