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
   - bumps `NATIVE_VERSION ?=` in the `Makefile`,
   - commits it on `main` as `chore(release): vX.Y.Z` (as `github-actions[bot]`),
   - tags that commit `vX.Y.Z` (annotated),
   - builds the AppImage and `.deb` via `make build` with
     `NATIVE_VERSION=vX.Y.Z` (same artifacts as `make build` locally),
   - publishes a GitHub release titled `vX.Y.Z` with the changelog since the
     previous tag and the two installers attached.
3. Pushes made with `GITHUB_TOKEN` do not trigger workflows, so the release
   commit does not re-trigger the workflow (a `chore(release):` subject
   guard in the workflow double-checks this).

The nightly prerelease (`nightly.yml`) is unaffected: it derives
`${NATIVE_VERSION}-nightly.<YYYYMMDD>` from the Makefile, so it tracks the
latest stable release base.

## Version line and tags

- Release tags are `vX.Y.Z` (e.g. `v2.1.0`). The workflow only considers
  tags matching `v[0-9]*.[0-9]*.[0-9]*`; the legacy `3.x` tags (no `v`
  prefix, unreachable from the rewritten history) and `nightly` / `v1.x-final`
  are ignored.
- The `v2.0.0` tag was placed on the commit that set `NATIVE_VERSION` to
  `2.0.0` (`8b9c40ed`) to seed the automation base. Everything before it is
  accounted for as 2.0.0, so the first automated release computed from it
  is **v2.1.0**. Commit `NATIVE_VERSION` is the single source of truth for
  the binary version baked into installers; `git tag` is the source of
  truth for the release history.

## Verifying locally

```bash
tools/semver-next.sh             # print what the next release would be
tools/semver-next.sh selftest    # run the fixture tests (18 checks)
```

The selftest builds scratch git repos in a temp dir and asserts bump
behaviour for feat/fix/breaking/no-op combinations, precedence rules and the
Makefile fallback used before any tag exists.

## Making a release manually

- Preferred: merge to `main` a commit whose subject is `feat:`/`fix:` (or a
  breaking change) — the workflow cuts the release.
- To force a run: **Actions → Release → Run workflow** (`workflow_dispatch`).
  It computes the version from the same rules; it does not accept arbitrary
  version numbers.
- To skip releasing a given change: use a non-feat/fix type (`chore:`,
  `docs:`, `perf:`…). Those never cut a release.
- The release workflow does not run the test suite; `native.yml` gates
  `main`. If the installer build fails, the tag and version-bump commit
  remain pushed — re-run the failed job to rebuild and publish (the
  `gh release create` step is safe to re-run when the release does not
  exist yet).
