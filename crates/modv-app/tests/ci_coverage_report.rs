//! REL-09: CI must publish a browsable line-coverage report.
//!
//! The Rust `test` job runs the canonical suite under `cargo llvm-cov` and
//! turns the recorded profiles into an HTML artifact. These guards keep the
//! three pieces together: the toolchain image must carry the coverage tools,
//! the Makefile must expose a `coverage-report` recipe, and the PR workflow
//! must invoke it and upload `coverage/html`. Without all three, coverage is
//! either unmeasured (plain `cargo test`) or measured but never published.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test ci_coverage_report -- --test-threads=1"
//! ```

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The recipe lines of a named Makefile target (tab-indented, including
/// continuations), stopping at the next target or non-recipe line.
fn makefile_recipe(makefile: &str, target: &str) -> String {
    let mut recipe = String::new();
    let mut in_recipe = false;
    for line in makefile.lines() {
        if !in_recipe {
            if line.starts_with(&format!("{target}:")) {
                in_recipe = true;
            }
            continue;
        }
        if line.starts_with('\t') {
            recipe.push_str(line);
            recipe.push('\n');
        } else {
            break;
        }
    }
    assert!(
        !recipe.is_empty(),
        "Makefile target `{target}` not found or has no recipe"
    );
    recipe
}

/// The body of a named workflow job, bounded by the next job key.
fn job_block(contents: &str, job: &str) -> String {
    let lines: Vec<&str> = contents.lines().collect();
    let key = format!("  {job}:");
    let start = lines
        .iter()
        .position(|line| *line == key)
        .unwrap_or_else(|| panic!("no `{job}` job in workflow"));
    let mut end = lines.len();
    for (offset, line) in lines.iter().enumerate().skip(start + 1) {
        if line.starts_with("  ") && !line.starts_with("   ") && line.contains(':') {
            end = offset;
            break;
        }
    }
    lines[start + 1..end].join("\n")
}

#[test]
fn rust_toolchain_image_includes_coverage_tools() {
    let dockerfile = read_text(&repo_root().join("Dockerfile"));
    let rust_stage = dockerfile
        .split("FROM base AS rust")
        .nth(1)
        .expect("Dockerfile has no `rust` stage")
        .split("\nFROM ")
        .next()
        .expect("cannot bound the `rust` stage");
    assert!(
        rust_stage.contains("llvm-tools-preview"),
        "the `rust` image must install the llvm-tools-preview component; \
         cargo-llvm-cov cannot merge profiles without it"
    );
    assert!(
        rust_stage.contains("cargo install cargo-llvm-cov"),
        "the `rust` image must install cargo-llvm-cov so CI coverage runs \
         need no network at test time"
    );
    assert!(
        rust_stage.contains("--version 0.6.14"),
        "cargo-llvm-cov must be version-pinned so coverage output is reproducible"
    );
}

#[test]
fn makefile_can_instrument_tests_and_build_the_report() {
    let makefile = read_text(&repo_root().join("Makefile"));
    assert!(
        makefile.contains("COVERAGE ?= 0"),
        "Makefile must expose a COVERAGE switch (off by default, on in CI)"
    );
    assert!(
        makefile.contains("llvm-cov --no-report"),
        "the test wrapper must instrument each suite without reporting until \
         all suites have finished"
    );

    let recipe = makefile_recipe(&makefile, "coverage-report");
    assert!(
        recipe.contains("llvm-cov report"),
        "`coverage-report` must invoke `cargo llvm-cov report`"
    );
    assert!(
        recipe.contains("--html") && recipe.contains("--output-dir"),
        "`coverage-report` must write a browsable HTML report; got:\n{recipe}"
    );
    assert!(
        recipe.contains("--lcov") && recipe.contains("--output-path"),
        "`coverage-report` must also write LCOV for tooling; got:\n{recipe}"
    );
}

#[test]
fn pr_ci_measures_coverage_and_publishes_the_html_artifact() {
    let workflow = read_text(&repo_root().join(".github/workflows/vmodv.yml"));
    let test = job_block(&workflow, "test");
    assert!(
        test.contains("COVERAGE: \"1\""),
        "the `test` job must set COVERAGE=1 so its suites run under llvm-cov"
    );
    let report = test
        .find("make coverage-report")
        .expect("the `test` job must run `make coverage-report` after its suites");
    // Every suite contributes its own profraw file; merging before any one of
    // them has run would silently drop that suite's coverage. COVERAGE=1 at
    // job level instruments all `make` test steps, and `--no-report` lets the
    // profiles accumulate in the shared target dir until this single merge.
    for suite in [
        "make engine-test ARGS=\"--workspace",
        "make engine-test ARGS=\"-p modv-app --lib",
        "make test-static-guards",
        "make test-shader-gpu",
        "make test-e2e-vmodv",
    ] {
        let position = test
            .find(suite)
            .unwrap_or_else(|| panic!("the `test` job must run `{suite}`"));
        assert!(
            position < report,
            "`{suite}` must run before the coverage report so its profile is merged"
        );
    }
    let upload = test
        .find("actions/upload-artifact@v4")
        .expect("the `test` job must upload the coverage artifact");
    assert!(
        report < upload,
        "the HTML artifact must be uploaded after it is generated"
    );
    let upload_block = &test[upload..];
    assert!(
        upload_block.contains("coverage/html"),
        "the uploaded artifact must contain the HTML report directory"
    );
    assert!(
        upload_block.contains("if-no-files-found: error"),
        "a missing coverage report must fail the job, not silently upload nothing"
    );
}
