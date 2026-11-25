//! Regression tests for resuming a release interrupted after its tag was pushed.
//!
//! `release.yml` pushes the computed tag before creating the GitHub release. If
//! the run is cancelled or release creation fails, a full rerun sees that tag at
//! `HEAD`. Version computation must return the tagged version so the rerun keeps
//! `should_release=true`; the idempotent release job can then create the missing
//! draft and the publish job can make it visible. Non-semver tags must not resume
//! a release, and ordinary commits after the highest tag still determine a bump.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test release_resume -- --test-threads=1"
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "modv-release-resume-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create temporary repository directory");
        let repo = Self { path };
        repo.git_ok(&["init", "-q"]);
        repo
    }

    fn git_ok(&self, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit(&self, message: &str) {
        let output = Command::new("git")
            .args([
                "-c",
                "user.email=release-resume@example.com",
                "-c",
                "user.name=Release Resume Test",
                "-c",
                "commit.gpgSign=false",
            ])
            .arg("-C")
            .arg(&self.path)
            .args(["commit", "-q", "--allow-empty", "-m", message])
            .output()
            .expect("run git commit");
        assert!(
            output.status.success(),
            "git commit {message:?} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn tag(&self, name: &str) {
        self.git_ok(&["-c", "tag.gpgSign=false", "tag", name]);
    }

    fn assert_next_version(&self, expected: &str) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = Command::new("bash")
            .arg(root.join("tools/semver-next.sh"))
            .args(["--repo"])
            .arg(&self.path)
            .current_dir(&root)
            .output()
            .expect("run tools/semver-next.sh");
        assert!(
            output.status.success(),
            "semver-next.sh failed for {}\nstdout:\n{}\nstderr:\n{}",
            self.path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("script stdout is UTF-8");
        assert_eq!(
            stdout.trim(),
            expected,
            "unexpected version for {}\nstderr:\n{}",
            self.path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn base_release() -> TempRepo {
    let repo = TempRepo::new();
    repo.commit("chore: base");
    repo.tag("v2.0.0");
    repo
}

#[test]
fn orphan_tag_at_head_resumes_release() {
    // Failure injection: the workflow pushed v2.1.0, then stopped before the
    // GitHub release existed. A full rerun must select that version again.
    let repo = base_release();
    repo.commit("feat: add thing");
    repo.tag("v2.1.0");

    repo.assert_next_version("2.1.0");
}

#[test]
fn untagged_head_uses_normal_patch_bump() {
    let repo = base_release();
    repo.commit("fix: bug");

    repo.assert_next_version("2.0.1");
}

#[test]
fn untagged_head_after_a_tag_still_bumps_normally() {
    let repo = base_release();
    repo.commit("feat: x");
    repo.tag("v2.1.0");
    repo.commit("fix: later");

    repo.assert_next_version("2.1.1");
}

#[test]
fn non_semver_tag_at_head_does_not_resume() {
    let repo = base_release();
    repo.commit("fix: bug");
    repo.tag("nightly");

    repo.assert_next_version("2.0.1");
}

#[test]
fn highest_semver_tag_at_head_resumes_release() {
    let repo = base_release();
    repo.commit("feat: x");
    repo.tag("v2.1.0");
    repo.tag("v2.2.0");

    repo.assert_next_version("2.2.0");
}
