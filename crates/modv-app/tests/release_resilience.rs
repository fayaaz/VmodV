//! Release workflow guards: create a draft, validate, then publish.
//!
//! The release workflow used to make `release` wait for the Windows and macOS
//! build jobs, and to build the AppImage in the same job that called
//! `gh release create`. It also published that release before any lint or test
//! run. The contract now is:
//!
//! 1. `release` depends on `version` only: it tags the commit and creates a
//!    GitHub release draft with its notes, without waiting for any artifact.
//! 2. A validation job runs `make engine-lint` and the test suite.
//! 3. A separate publish job depends on both `release` and validation, respects
//!    `should_release`, and changes the draft to `--draft=false`. It never
//!    depends on a platform build job.
//! 4. Each build job hands its artifact to a separate attach job that also
//!    waits for `release`. Artifacts may arrive after publication; a failed
//!    build skips only its own attachment. The macOS matrix is the exception:
//!    either architecture failing must not drop the other architecture's ZIP.
//! 5. Manual `workflow_dispatch` runs are restricted to `refs/heads/main`.
//!
//! Pure text scanning of `.github/workflows/release.yml`: no YAML crate, no
//! network and no env mutation. Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test release_resilience -- --test-threads=1"
//! ```

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

fn release_workflow() -> String {
    let path = repo_root().join(".github/workflows/release.yml");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The body of a top-level GitHub Actions job (a two-space-indented key).
fn job_block(text: &str, job: &str) -> Option<String> {
    let key = format!("  {job}:");
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| *line == key)?;
    let mut end = lines.len();
    for (offset, line) in lines.iter().enumerate().skip(start + 1) {
        if line.starts_with("  ") && !line.starts_with("   ") && line.contains(':') {
            end = offset;
            break;
        }
    }
    Some(lines[start + 1..end].join("\n"))
}

fn job_names(text: &str) -> Vec<String> {
    let mut in_jobs = false;
    text.lines()
        .filter_map(|line| {
            if line == "jobs:" {
                in_jobs = true;
                return None;
            }
            if !in_jobs {
                return None;
            }
            let line = line.trim_end();
            if !line.starts_with("  ") || line.starts_with("   ") {
                return None;
            }
            let key = line.trim();
            let name = key.strip_suffix(':')?;
            let is_job_id = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            is_job_id.then(|| name.to_string())
        })
        .collect()
}

/// `needs:` in either inline (`needs: [a, b]`) or block (`needs:\n - a`) form.
fn needs_of(block: &str) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim().strip_prefix("needs:") else {
            continue;
        };
        let rest = rest.trim();
        if let Some(items) = rest.strip_prefix('[') {
            return items
                .trim_end_matches(']')
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect();
        }
        if rest.is_empty() {
            let mut out = Vec::new();
            for follow in lines.iter().skip(index + 1) {
                let follow = follow.trim();
                if let Some(item) = follow.strip_prefix("- ") {
                    out.push(item.trim().to_string());
                } else if follow.is_empty() {
                    continue;
                } else {
                    break;
                }
            }
            return out;
        }
        return vec![rest.to_string()];
    }
    Vec::new()
}

/// One shell invocation, bounded by the next step rather than later job steps.
fn shell_command(block: &str, invocation: &str) -> Option<String> {
    let start = block.find(invocation)?;
    let command = &block[start..];
    let end = command.find("\n      - name:").unwrap_or(command.len());
    Some(command[..end].to_string())
}

fn if_conditions_at_indent(block: &str, indent: Option<usize>) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    let mut conditions = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("if:") else {
            continue;
        };
        let line_indent = line.len() - trimmed.len();
        if indent.is_some_and(|expected| line_indent != expected) {
            continue;
        }
        let mut condition = rest.trim().to_string();
        for follow in lines.iter().skip(index + 1) {
            let follow_trimmed = follow.trim_start();
            let follow_indent = follow.len() - follow_trimmed.len();
            if follow_trimmed.is_empty() {
                continue;
            }
            if follow_indent <= line_indent {
                break;
            }
            if !condition.is_empty() {
                condition.push(' ');
            }
            condition.push_str(follow_trimmed);
        }
        conditions.push(condition);
    }
    conditions
}

/// The job-level guard, excluding conditions belonging to individual steps.
fn job_condition(block: &str) -> Option<String> {
    if_conditions_at_indent(block, Some(4)).into_iter().next()
}

/// Counts invocations of a command in executable workflow lines.
///
/// Whole-line `#` comments often name the command a step runs (for example to
/// explain its cache scope); those are documentation, not builds. The match
/// also stops at command-name boundaries so `make vmodv-deb` does not count
/// `make vmodv-deb-check`.
fn command_invocations(text: &str, command: &str) -> usize {
    let is_boundary = |c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_');
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .map(|line| {
            let mut occurrences = 0;
            let mut rest = line;
            while let Some(index) = rest.find(command) {
                let before = rest[..index].chars().next_back();
                let after = rest[index + command.len()..].chars().next();
                if before.is_none_or(is_boundary) && after.is_none_or(is_boundary) {
                    occurrences += 1;
                }
                rest = &rest[index + command.len()..];
            }
            occurrences
        })
        .sum()
}

#[test]
fn release_job_never_waits_for_a_build_job() {
    let text = release_workflow();
    let release = job_block(&text, "release").expect("release.yml has no `release` job");
    let needs = needs_of(&release);

    for build in [
        "build-appimage",
        "windows-portable",
        "macos-portable",
        "linux-debs",
        "publish-debs",
    ] {
        assert!(
            !needs.iter().any(|item| item == build),
            "`release` waits for `{build}` in needs {needs:?}: a failed build would stop \
             the release from going out"
        );
    }
    assert_eq!(
        needs,
        vec!["version".to_string()],
        "`release` must depend on `version` only so it can tag and create the draft \
         before any build or validation outcome is known"
    );
}

#[test]
fn release_is_drafted_then_published_only_after_validation() {
    let text = release_workflow();
    let release = job_block(&text, "release").expect("release.yml has no `release` job");
    let create = shell_command(&release, "gh release create")
        .expect("the `release` job must create a GitHub release with `gh release create`");

    assert!(
        create.split_whitespace().any(|arg| arg == "--draft"),
        "`gh release create` must create a draft so validation can fail without exposing \
         a stable release:\n{create}"
    );
    assert!(
        !create.contains("dist/"),
        "`gh release create` must create the draft with notes only; attaching build \
         artifacts here makes draft creation fail when a build did not produce a file:\n\
         {create}"
    );

    let names = job_names(&text);
    let validation = names.iter().find(|name| {
        let block = job_block(&text, name).unwrap_or_default();
        block.contains("make engine-lint")
            && (block.contains("make engine-test") || block.contains("cargo test"))
    });
    assert!(
        validation.is_some(),
        "no validation job runs both `make engine-lint` and `make engine-test` (or \
         `cargo test`); publication must wait for lint and tests (jobs: {names:?})"
    );
    let validation = validation.expect("checked above");
    let validation_block = job_block(&text, validation).expect("checked above");
    assert!(
        validation_block.contains("make engine-lint"),
        "validation job `{validation}` must run `make engine-lint`"
    );
    assert!(
        validation_block.contains("make engine-test") || validation_block.contains("cargo test"),
        "validation job `{validation}` must run `make engine-test` or `cargo test`"
    );

    let publish = names.iter().find(|name| {
        let block = job_block(&text, name).unwrap_or_default();
        block.contains("gh release edit") && block.contains("--draft=false")
    });
    assert!(
        publish.is_some(),
        "no job converts the validated draft with `gh release edit ... --draft=false` \
         (jobs: {names:?})"
    );
    let publish = publish.expect("checked above");
    let publish_block = job_block(&text, publish).expect("checked above");
    let edit = shell_command(&publish_block, "gh release edit").expect("checked above");
    assert!(
        edit.split_whitespace().any(|arg| arg == "--draft=false"),
        "publish job `{publish}` must pass `--draft=false` to `gh release edit` so the \
         release becomes visible only after validation:\n{edit}"
    );

    let needs = needs_of(&publish_block);
    for required in ["release", validation.as_str()] {
        assert!(
            needs.iter().any(|item| item == required),
            "publish job `{publish}` must depend on `{required}` so it runs only after \
             the draft exists and validation succeeds; got needs {needs:?}"
        );
    }
    for build in [
        "build-appimage",
        "windows-portable",
        "macos-portable",
        "linux-debs",
        "publish-debs",
    ] {
        assert!(
            !needs.iter().any(|item| item == build),
            "publish job `{publish}` waits for `{build}` in needs {needs:?}; platform \
             builds must not gate publication and may attach artifacts later"
        );
    }
    let publish_condition = job_condition(&publish_block);
    assert!(
        publish_condition.as_deref().is_some_and(|condition| {
            condition.contains("needs.version.outputs.should_release == 'true'")
        }),
        "publish job `{publish}` must retain a job-level `should_release == 'true'` \
         guard; got condition {publish_condition:?}"
    );

    assert!(
        if_conditions_at_indent(&text, Some(4))
            .iter()
            .any(|condition| {
                condition.contains("workflow_dispatch")
                    && condition.contains("github.ref == 'refs/heads/main'")
            }),
        "manual `workflow_dispatch` must be restricted to `refs/heads/main` by a \
         job-level `if:` condition tying `workflow_dispatch` to \
         `github.ref == 'refs/heads/main'`; got job conditions {:?}",
        if_conditions_at_indent(&text, Some(4))
    );
}

#[test]
fn e2e_validation_runs_in_its_own_job() {
    // The real-binary suites need a private Xvfb display and the shared
    // lavapipe device, so they cannot safely overlap with the GPU/kittest
    // suites in `validate`. They still gate publication, but as a separate
    // job on its own runner so the two legs run concurrently and a failure
    // in either one stops the release.
    let text = release_workflow();
    let validate = job_block(&text, "validate").expect("release.yml has no `validate` job");
    let e2e = job_block(&text, "validate-e2e")
        .expect("release.yml has no `validate-e2e` job for the real-binary suites");

    assert!(
        !validate.contains("make test-e2e-vmodv"),
        "`validate` still runs `make test-e2e-vmodv`; move the real-binary suites \
         into `validate-e2e` so they do not hold up lint/unit tests on one runner"
    );
    assert!(
        e2e.contains("make test-e2e-vmodv"),
        "`validate-e2e` must run `make test-e2e-vmodv` so the real-binary suites \
         still gate main pushes"
    );
    assert_eq!(
        needs_of(&e2e),
        vec!["version".to_string()],
        "`validate-e2e` must depend on `version` only, like `validate`, so it \
         runs on every main push including those that do not cut a release"
    );
    let condition = e2e
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("if:"))
        .map(str::trim);
    assert!(
        condition.is_none_or(|condition| !condition.contains("should_release")),
        "`validate-e2e` is gated on `should_release`; main pushes without a \
         version bump must still be validated (got if: {condition:?})"
    );

    let publish = job_block(&text, "publish").expect("release.yml has no `publish` job");
    let needs = needs_of(&publish);
    for required in ["validate", "validate-e2e"] {
        assert!(
            needs.iter().any(|item| item == required),
            "`publish` must depend on `{required}` so publication waits for both \
             validation legs; got needs {needs:?}"
        );
    }
}

#[test]
fn every_build_job_has_an_independent_attach_job() {
    let text = release_workflow();
    let names = job_names(&text);
    let builds = [
        "build-appimage",
        "windows-portable",
        "macos-portable",
        "linux-debs",
    ];

    for build in builds {
        let attach = names.iter().find(|name| {
            if name.as_str() == "release" || *name == build {
                return false;
            }
            let Some(block) = job_block(&text, name) else {
                return false;
            };
            let needs = needs_of(&block);
            needs.iter().any(|item| item == "release")
                && needs.iter().any(|item| item == build)
                && block.contains("gh release upload")
        });
        assert!(
            attach.is_some(),
            "no attach job waits for both `release` and `{build}` to upload its artifact \
             (jobs: {names:?}): artifacts must be attached when their build succeeds \
             without letting that build gate the release"
        );
    }
}

#[test]
fn command_invocations_ignore_comments_and_longer_command_names() {
    let text = "\
  # `make vmodv-deb` explains the toolchain cache scope.
  # `make vmodv-deb` is mentioned again here.
        run: make vmodv-deb BASE_IMAGE=ubuntu:24.04
        run: make vmodv-deb-check
";

    assert_eq!(
        command_invocations(text, "make vmodv-deb"),
        1,
        "a comment or the longer `make vmodv-deb-check` must not be counted as a build"
    );
    assert_eq!(
        command_invocations(
            "run: make vmodv-deb\nrun: make vmodv-deb\n",
            "make vmodv-deb"
        ),
        2,
        "two real build lines must still be reported as duplicates"
    );
}

#[test]
fn each_artifact_is_built_only_once_in_the_release_workflow() {
    let text = release_workflow();
    // Build once, persist as a workflow artifact, then let an `attach-*` job
    // download and upload it. A second build step would waste a full Rust
    // toolchain/Windows GStreamer/macOS runner pass for the same bytes.
    for (artifact, command) in [
        ("AppImage", "make appimage"),
        ("AppImage bundle check", "make vmodv-appimage-check"),
        ("Windows ZIP", "bash tools/windows-portable.sh"),
        ("macOS bundle", "bash tools/macos-portable.sh"),
        ("Ubuntu .deb", "make vmodv-deb"),
    ] {
        let count = command_invocations(&text, command);
        assert_eq!(
            count, 1,
            "{artifact} is built {count} times in release.yml via `{command}`; \
             build it once and attach the uploaded artifact"
        );
    }
}

#[test]
fn main_pushes_do_not_rebuild_release_artifacts_in_ci_workflows() {
    // release.yml owns all release builds on main, including macOS (which no
    // other workflow builds any more) and manual runs of that workflow.
    // vmodv.yml also runs on main, so without a ref-based guard the same
    // AppImage / Windows ZIP is compiled twice for one commit. Event-based
    // guards are not enough: a `workflow_dispatch` on main still duplicates
    // the release build. Lint and tests are deduplicated in `validate`.
    let ci = fs::read_to_string(repo_root().join(".github/workflows/vmodv.yml"))
        .expect("read vmodv.yml");

    for (name, text, job) in [
        ("vmodv.yml", ci.as_str(), "package"),
        ("vmodv.yml", ci.as_str(), "windows-portable"),
    ] {
        let block = job_block(text, job).unwrap_or_default();
        let guarded = block.lines().map(str::trim).any(|line| {
            line.starts_with("if:")
                && line.contains("github.ref != 'refs/heads/main'")
                && !line.contains("github.event_name")
        });
        assert!(
            guarded,
            "{name}'s `{job}` job still builds on main events; release.yml builds \
             the same artifact for that commit — guard the CI job with a \
             ref-based `if:` that also skips manual runs on main"
        );
    }
}

#[test]
fn main_push_lints_and_tests_once_before_publishing() {
    // Both workflows trigger on pushes to main. release.yml's `validate` and
    // `validate-e2e` gate `publish`, so they must be the single place that
    // lints and runs the full suite on that ref; vmodv.yml stands down for
    // main pushes (it still owns the same coverage on PRs and manual runs).
    let ci = fs::read_to_string(repo_root().join(".github/workflows/vmodv.yml"))
        .expect("read vmodv.yml");
    let release = release_workflow();
    let main_ref_guard = "github.ref != 'refs/heads/main'";

    for job in ["lint", "test"] {
        let block = job_block(&ci, job).unwrap_or_default();
        let condition = block
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix("if:"))
            .map(str::trim);
        assert!(
            condition.is_some_and(|condition| {
                condition.contains(main_ref_guard) && !condition.contains("github.event_name")
            }),
            "vmodv.yml's `{job}` job still runs on main, where release.yml validates \
             the same commit; guard it with a ref-based `if:` so manual runs skip \
             that ref too (got if: {condition:?})"
        );
    }

    let validate = job_block(&release, "validate").expect("release.yml has no `validate` job");
    let condition = validate
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("if:"))
        .map(str::trim);
    assert!(
        condition.is_none_or(|condition| !condition.contains("should_release")),
        "release.yml's `validate` job is gated on `should_release`, so a main push \
         with no qualifying commits runs no lint or tests at all; it must gate on \
         the push alone (got if: {condition:?})"
    );
    assert!(
        validate.contains("make engine-lint"),
        "release.yml's `validate` must run `make engine-lint`"
    );
    for command in [
        "make engine-test ARGS=\"--workspace --exclude modv-app",
        "make engine-test ARGS=\"-p modv-app --lib",
        "make test-static-guards",
    ] {
        assert!(
            validate.contains(command),
            "release.yml's `validate` must run `{command}` so main pushes keep \
             the same coverage CI gives PRs, exactly once"
        );
    }
}

#[test]
fn attach_jobs_can_resolve_the_github_repository() {
    // `gh release upload` discovers OWNER/REPO from the local git remote unless
    // GH_REPO is set. The attach jobs have no checkout today, so gh fails with
    // "fatal: not a git repository" and no artifact is attached. Every job
    // that calls `gh release upload` must establish the repository one way or
    // the other.
    let text = release_workflow();
    for job in [
        "attach-appimage",
        "attach-windows",
        "attach-macos",
        "publish-debs",
    ] {
        let block = job_block(&text, job).unwrap_or_default();
        let resolves_repo = block.contains("GH_REPO:")
            || block
                .lines()
                .any(|line| line.trim() == "uses: actions/checkout@v4");
        assert!(
            resolves_repo,
            "`{job}` runs `gh release upload` without a checkout or GH_REPO; \
             gh cannot infer the repository and the upload fails with \
             'fatal: not a git repository'"
        );
    }
}

#[test]
fn one_failed_macos_architecture_still_attaches_the_other() {
    // `macos-portable` is a two-leg matrix (x86_64 + arm64) with fail-fast
    // off. GitHub reports the whole job as failed when either leg fails, so
    // the default `success()` gate on `needs` skips `attach-macos` and the
    // surviving architecture's ZIP never reaches the release.
    let text = release_workflow();
    let block = job_block(&text, "attach-macos").expect("release.yml has no `attach-macos` job");

    let if_condition = block
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("if:"))
        .map(str::trim)
        .expect("`attach-macos` has no `if:` guard");
    assert!(
        if_condition.contains("!cancelled()") || if_condition.contains("always()"),
        "`attach-macos` still runs only when every macOS architecture succeeds: \
         one failed leg skips the job and the other leg's ZIP is never attached \
         (if: {if_condition})"
    );
    assert!(
        !if_condition.contains("needs.macos-portable.result == 'success'"),
        "`attach-macos` still explicitly requires the whole macOS matrix to \
         succeed, so a single failed architecture drops the other ZIP \
         (if: {if_condition})"
    );

    let tolerant_downloads = block.matches("continue-on-error: true").count();
    assert!(
        tolerant_downloads >= 2,
        "`attach-macos` has {tolerant_downloads} best-effort artifact downloads; \
         a missing architecture's artifact must not abort the job before the \
         surviving ZIP is attached"
    );

    assert!(
        block.contains("nullglob") || block.contains("compgen -G"),
        "`attach-macos` must guard the `dist-macos/*.zip` glob before calling \
         `gh release upload`; with no surviving architecture the literal \
         pattern is passed to gh and the attach job fails instead of no-oping"
    );
}

#[test]
fn macos_build_failure_cannot_block_the_release() {
    // The reported regression: a missing/failed macOS leg left no release at
    // all. Pin the two halves of the fix explicitly, even if the generic tests
    // above are later refactored.
    let text = release_workflow();
    let release = job_block(&text, "release").expect("release.yml has no `release` job");
    assert!(
        !needs_of(&release)
            .iter()
            .any(|item| item == "macos-portable"),
        "`release` still waits for `macos-portable`"
    );
    assert!(
        text.contains("gh release upload"),
        "no job uploads any artifact to the release after it is created"
    );
}

#[test]
fn previous_tag_excludes_the_version_being_released() {
    // On a resumed run the tag being released already exists at HEAD and is
    // therefore the highest tag. Selecting it as `previous_tag` gives the
    // release notes an empty `vX.Y.Z..vX.Y.Z` range and a self-comparing link.
    let text = release_workflow();
    let version = job_block(&text, "version").expect("release.yml has no `version` job");
    let compute = shell_command(&version, "tools/semver-next.sh")
        .expect("`version` must compute the next version with tools/semver-next.sh");
    let prev = compute
        .lines()
        .find(|line| line.trim_start().starts_with("PREV="))
        .expect("`version` must select a previous tag for release notes");

    assert!(
        prev.contains("grep -v") && prev.contains("${VERSION}"),
        "the previous-tag selection still includes the version being released \
         ({prev}); on a resumed run that yields empty notes and a \
         vX.Y.Z...vX.Y.Z compare link"
    );
}
