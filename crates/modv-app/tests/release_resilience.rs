//! RED guard: cutting a release must never be gated on platform builds.
//!
//! The release workflow used to make `release` wait for the Windows and macOS
//! build jobs, and to build the AppImage in the same job that called
//! `gh release create`. A slow, flaky or capacity-starved build (the macOS
//! arm64 runner, for example) therefore stopped the release from ever going
//! out. The contract now is:
//!
//! 1. `release` depends on `version` only: it tags the commit and creates the
//!    GitHub release with its notes, without waiting for any artifact.
//! 2. `gh release create` publishes no files — a release with zero assets is
//!    still a release and is still visible to users.
//! 3. Each build job hands its artifact to a separate attach job that also
//!    waits for `release`. When that build succeeds its artifact is uploaded;
//!    when it fails the attach job is skipped but the release stands.
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
        "`release` must depend on `version` only so it can tag and publish before any \
         build outcome is known"
    );
}

#[test]
fn release_creation_does_not_require_a_built_artifact() {
    let text = release_workflow();
    let release = job_block(&text, "release").expect("release.yml has no `release` job");
    let start = release
        .find("gh release create")
        .expect("the `release` job must publish a GitHub release with `gh release create`");
    // Bound the scan to the create invocation, not later steps in the job.
    let command = &release[start..];
    let command = command
        .find("\n        - name:")
        .map(|end| &command[..end])
        .unwrap_or(command);

    assert!(
        !command.contains("dist/"),
        "`gh release create` must publish the release with notes only; attaching \
         build artifacts here makes the release itself fail when a build did not \
         produce a file:\n{command}"
    );
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
fn each_artifact_is_built_only_once_in_the_release_workflow() {
    let text = release_workflow();
    // Build once, persist as a workflow artifact, then let an `attach-*` job
    // download and upload it. A second build step would waste a full Rust
    // toolchain/Windows GStreamer/macOS runner pass for the same bytes.
    for (artifact, command) in [
        ("AppImage", "make appimage-vmodv"),
        ("AppImage bundle check", "make vmodv-appimage-check"),
        ("Windows ZIP", "bash tools/windows-portable.sh"),
        ("macOS bundle", "bash tools/macos-portable.sh"),
        ("Ubuntu .deb", "make vmodv-deb"),
    ] {
        let count = text.matches(command).count();
        assert_eq!(
            count, 1,
            "{artifact} is built {count} times in release.yml via `{command}`; \
             build it once and attach the uploaded artifact"
        );
    }
}

#[test]
fn main_pushes_do_not_rebuild_release_artifacts_in_ci_workflows() {
    // release.yml triggers on every push to main and owns all release builds.
    // vmodv.yml and macos.yml also trigger on main, so without a guard the very
    // same AppImage / Windows ZIP / macOS bundle is compiled twice for one
    // commit. Lint and tests still run on main; only the duplicate builds go.
    let ci = fs::read_to_string(repo_root().join(".github/workflows/vmodv.yml"))
        .expect("read vmodv.yml");
    let macos = fs::read_to_string(repo_root().join(".github/workflows/macos.yml"))
        .expect("read macos.yml");

    for (name, text, job) in [
        ("vmodv.yml", ci.as_str(), "package"),
        ("vmodv.yml", ci.as_str(), "windows-portable"),
        ("macos.yml", macos.as_str(), "macos-portable"),
    ] {
        let block = job_block(text, job).unwrap_or_default();
        let guarded = block
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with("if:") && line.contains("refs/heads/main"));
        assert!(
            guarded,
            "{name}'s `{job}` job still builds on main pushes; release.yml builds \
             the same artifact for that commit — guard the CI job with an `if:` \
             that skips main pushes"
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
