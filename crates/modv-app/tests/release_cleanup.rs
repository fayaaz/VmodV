//! REL-12: nightly pruning of stale automated draft releases.
//!
//! `release.yml` tags and creates the GitHub release as a draft as soon as a
//! version is known, then `publish` flips it visible only if `validate` passes.
//! A failed or cancelled main run therefore leaves an invisible draft behind
//! forever. The nightly cleanup must delete only drafts that automation
//! created from `main`, older than a day, and must leave published releases,
//! recent drafts, hand-written drafts and git tags untouched.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test release_cleanup -- --test-threads=1"
//! ```

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn date_offset(offset: &str) -> String {
    let output = Command::new("date")
        .args(["-u", "-d", offset, "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .expect("run date");
    assert!(
        output.status.success(),
        "date -d {offset} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("date output is UTF-8")
        .trim()
        .to_owned()
}

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(rows: &str) -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "modv-release-cleanup-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join("bin")).expect("create fixture directory");
        fs::write(dir.join("releases.tsv"), rows).expect("write release fixture");
        // Stub `gh` so the non-dry-run path can be exercised without a network
        // call or a real token. Fixture mode bypasses `gh api`; only the delete
        // invocations reach this stub, and they are recorded for assertions.
        fs::write(
            dir.join("bin/gh"),
            concat!(
                "#!/usr/bin/env bash\n",
                "{ printf '%s' \"$*\" | tr '\\n' ' '; printf '\\n'; } >> \"$GH_LOG\"\n",
                "if [ \"${1:-}\" = api ]; then cat \"$GH_STUB_ROWS_FILE\"; fi\n",
            ),
        )
        .expect("write gh stub");
        fs::set_permissions(dir.join("bin/gh"), fs::Permissions::from_mode(0o755))
            .expect("make gh stub executable");
        Self { dir }
    }

    fn run(&self, dry_run: bool) -> Output {
        let root = repo_root();
        let path = std::env::var("PATH").unwrap_or_default();
        let mut command = Command::new("bash");
        command
            .arg(root.join("tools/prune-draft-releases.sh"))
            .current_dir(&root)
            .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
            .env("GH_LOG", self.dir.join("gh.log"))
            .env("GITHUB_REPOSITORY", "modv/test")
            .env("RELEASES_TSV_FILE", self.dir.join("releases.tsv"))
            .env("DRY_RUN", if dry_run { "true" } else { "false" });
        command.output().expect("run prune-draft-releases.sh")
    }

    fn run_live(&self) -> Output {
        let root = repo_root();
        let path = std::env::var("PATH").unwrap_or_default();
        let mut command = Command::new("bash");
        command
            .arg(root.join("tools/prune-draft-releases.sh"))
            .current_dir(&root)
            .env("PATH", format!("{}:{path}", self.dir.join("bin").display()))
            .env("GH_LOG", self.dir.join("gh.log"))
            .env("GH_STUB_ROWS_FILE", self.dir.join("releases.tsv"))
            .env("GITHUB_REPOSITORY", "modv/test")
            .env_remove("RELEASES_TSV_FILE")
            .env("DRY_RUN", "true");
        command.output().expect("run prune-draft-releases.sh")
    }

    fn gh_log(&self) -> String {
        read_text(&self.dir.join("gh.log"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// The exact marker written by release.yml's "Generate release notes" step.
/// A draft without it was not cut automatically from `main`.
const MARKER: &str = "Automated release cut from `main` by the semantic-versioning workflow.";

fn fixture_rows() -> String {
    let old = date_offset("-2 days");
    let recent = date_offset("-2 hours");
    format!(
        "v9.9.9\t{old}\tgithub-actions[bot]\t{MARKER}\n\
         v9.9.8\t{recent}\tgithub-actions[bot]\t{MARKER}\n\
         v9.9.7\t{old}\toctocat\t{MARKER}\n\
         v9.9.6\t{old}\tgithub-actions[bot]\tHand-written draft notes\n\
         nightly\t{old}\tgithub-actions[bot]\t{MARKER}\n\
         v9.9.4\t{old}\tgithub-actions[bot]\t{MARKER}\n"
    )
}

#[test]
fn prunes_only_old_automated_semver_drafts_in_dry_run() {
    let fixture = Fixture::new(&fixture_rows());
    let output = fixture.run(true);
    assert!(
        output.status.success(),
        "prune script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("script stdout is UTF-8");
    assert!(
        stdout.contains("would prune v9.9.9"),
        "old automated draft was not selected: {stdout}"
    );
    assert!(
        stdout.contains("would prune v9.9.4"),
        "second old automated draft was not selected: {stdout}"
    );
    for kept in ["v9.9.8", "v9.9.7", "v9.9.6", "nightly"] {
        assert!(
            !stdout.contains(&format!("prune {kept}")),
            "{kept} must be kept but was selected: {stdout}"
        );
    }
    assert!(
        !fixture.dir.join("gh.log").exists(),
        "dry run must not call gh at all"
    );
}

#[test]
fn deletes_only_matching_drafts_and_preserves_tags() {
    let fixture = Fixture::new(&fixture_rows());
    let output = fixture.run(false);
    assert!(
        output.status.success(),
        "prune script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fixture.gh_log().lines().collect::<Vec<_>>(),
        vec![
            "release delete v9.9.9 --yes --repo modv/test",
            "release delete v9.9.4 --yes --repo modv/test",
        ],
        "only stale automated drafts may be deleted, without --cleanup-tag"
    );
}

#[test]
fn live_mode_queries_gh_api_for_drafts_without_deleting_in_dry_run() {
    let fixture = Fixture::new(&fixture_rows());
    let output = fixture.run_live();
    assert!(
        output.status.success(),
        "prune script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("script stdout is UTF-8");
    assert!(
        stdout.contains("would prune v9.9.9"),
        "gh api rows were not pruned: {stdout}"
    );
    let log = fixture.gh_log();
    let calls = log.lines().collect::<Vec<_>>();
    assert_eq!(
        calls.len(),
        1,
        "dry run must only query gh, never delete: {log}"
    );
    assert!(
        calls[0].contains("api --paginate repos/modv/test/releases --jq"),
        "unexpected gh api query: {}",
        calls[0]
    );
}

#[test]
fn nightly_workflow_runs_the_pruner_on_main_only_releases() {
    let root = repo_root();
    let workflow = read_text(&root.join(".github/workflows/nightly-cleanup.yml"));
    for required in [
        "schedule:",
        "cron: '0 3 * * *'",
        "workflow_dispatch:",
        "permissions:\n  contents: write",
        "bash tools/prune-draft-releases.sh",
    ] {
        assert!(
            workflow.contains(required),
            "nightly-cleanup.yml is missing {required:?}"
        );
    }
    for forbidden in [
        "pull_request:",
        "gh release create",
        "gh release edit",
        "--cleanup-tag",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "nightly cleanup must not contain {forbidden:?}"
        );
    }
}

#[test]
fn pruner_guards_the_release_origin_and_age() {
    let script = read_text(&repo_root().join("tools/prune-draft-releases.sh"));
    for required in [
        "select(.draft == true)",
        "MAX_AGE_SECONDS:-86400",
        "Automated release cut from `main` by the semantic-versioning workflow.",
        "endswith(\"[bot]\")",
        "^v[0-9]+\\.[0-9]+\\.[0-9]+$",
    ] {
        assert!(
            script.contains(required),
            "prune-draft-releases.sh is missing {required:?}"
        );
    }
}
