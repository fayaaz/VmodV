//! REL-03: guard CI coverage for every top-level modv-app integration target.
//!
//! Clippy compiles integration test targets, but compilation is not execution:
//! licence, removed-content, release and packaging guards can rot unnoticed if
//! CI never runs them. This fast text/filesystem check derives the target stems
//! from `crates/modv-app/tests/*.rs` and requires each one to be selected by
//! an explicit `--test` argument in a Makefile recipe (`test-static-guards` or
//! `test-e2e-vmodv`). Both `.github/workflows/vmodv.yml` and `release.yml`
//! must invoke those recipes so the list has a single source of truth and PR
//! CI and release validation cannot drift apart.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test ci_test_coverage -- --test-threads=1"
//! ```

use std::collections::BTreeSet;
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

/// Active lines: no blanks, no comments (`#` starts one even mid-line, so a
/// commented-out `--test` cannot fake coverage). Makefile lines are limited to
/// recipes, where the cargo invocation actually lives.
fn active_lines(contents: &str, recipes_only: bool) -> impl Iterator<Item = &str> {
    contents
        .lines()
        .filter(move |line| {
            (!recipes_only || line.starts_with('\t'))
                && !line.trim().is_empty()
                && !line.trim_start().starts_with('#')
        })
        .map(|line| line.split('#').next().unwrap_or_default())
}

/// Every `--test <target>` selected on an active line.
fn selected_targets(contents: &str, recipes_only: bool) -> BTreeSet<String> {
    active_lines(contents, recipes_only)
        .flat_map(|line| {
            let tokens = line.split_whitespace().collect::<Vec<_>>();
            tokens
                .windows(2)
                .filter(|pair| pair[0] == "--test")
                .map(|pair| {
                    pair[1]
                        .trim_matches(|ch| matches!(ch, '"' | '\'' | ',' | ';'))
                        .to_owned()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The body of an `on:` trigger, bounded by the next top-level trigger key.
fn trigger_block(contents: &str, trigger: &str) -> String {
    let lines: Vec<&str> = contents.lines().collect();
    let key = format!("  {trigger}:");
    let start = lines
        .iter()
        .position(|line| *line == key)
        .unwrap_or_else(|| panic!("no `{trigger}` trigger found"));
    lines
        .iter()
        .skip(start + 1)
        .take_while(|line| {
            let top_level = line.starts_with("  ") && !line.starts_with("   ");
            !(top_level && line.contains(':'))
        })
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn main_push_validation_skips_docs_only_commits() {
    // release.yml's `validate` and `validate-e2e` run lint, all guards and the
    // e2e suites on every main push. Without a path filter a docs-only commit
    // pays for that full run, which the old path-filtered vmodv.yml job avoided.
    let root = repo_root();
    let release = read_text(&root.join(".github/workflows/release.yml"));
    let push = trigger_block(&release, "push");
    assert!(
        push.contains("paths:"),
        "release.yml's push trigger has no path filter: docs-only main pushes \
         run the full lint/guard/e2e suite"
    );
    for path in [
        "'.github/workflows/**'",
        "'crates/**'",
        "'resources/**'",
        "'tools/**'",
        "'Makefile'",
        "'Dockerfile'",
    ] {
        assert!(
            push.contains(path),
            "release.yml's push filter must include {path} so changes to that \
             tree still trigger validation; got:\n{push}"
        );
    }
}

#[test]
fn workflow_only_changes_trigger_ci_validation() {
    // Without workflow files in vmodv.yml's path filters, a PR touching only
    // release.yml would skip lint and the guards entirely.
    let root = repo_root();
    let workflow = read_text(&root.join(".github/workflows/vmodv.yml"));
    let hits = active_lines(&workflow, false)
        .filter(|line| line.trim() == "- '.github/workflows/**'")
        .count();
    assert_eq!(
        hits, 2,
        "vmodv.yml must list `.github/workflows/**` in both the push and \
         pull_request path filters so workflow-only changes are linted and \
         tested; found {hits} entries"
    );
}

/// The body of a named job in a workflow, bounded by the next job key.
fn job_block(contents: &str, job: &str) -> String {
    let key = format!("  {job}:");
    let lines: Vec<&str> = contents.lines().collect();
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
fn gpu_shader_tests_run_before_e2e_in_the_same_job() {
    let root = repo_root();
    let makefile = read_text(&root.join("Makefile"));
    assert!(
        active_lines(&makefile, true).any(|line| {
            line.contains("run-cargo,test")
                && line.contains("-p modv-renderers")
                && line.contains("test_gpu")
        }),
        "Makefile must expose a `test-shader-gpu` recipe that runs the GPU shader tests"
    );

    for (name, file, job) in [
        ("vmodv.yml", "vmodv.yml", "test"),
        ("release.yml", "release.yml", "validate-e2e"),
    ] {
        let workflow = read_text(&root.join(".github/workflows").join(file));
        let block = job_block(&workflow, job);
        let shader = block
            .lines()
            .position(|line| line.contains("make test-shader-gpu"))
            .unwrap_or_else(|| panic!("{name}'s `{job}` job must run `make test-shader-gpu`"));
        let e2e = block
            .lines()
            .position(|line| line.contains("make test-e2e-vmodv"))
            .unwrap_or_else(|| panic!("{name}'s `{job}` job must run `make test-e2e-vmodv`"));
        assert!(
            shader < e2e,
            "{name}'s `{job}` job runs the GPU shader tests after e2e; they must gate it"
        );
        assert!(
            active_lines(&workflow, false).any(|line| {
                line.contains("engine-test ARGS=\"--workspace") && line.contains("--skip test_gpu")
            }),
            "{name}'s workspace test step must `--skip test_gpu` so the dedicated \
             shader step does not run the same suite twice"
        );
    }
}

#[test]
fn every_top_level_integration_target_runs_in_ci() {
    let root = repo_root();
    let tests_dir = root.join("crates/modv-app/tests");
    let targets = fs::read_dir(&tests_dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", tests_dir.display()))
        .filter_map(|entry| {
            let path = entry.expect("read integration test entry").path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                return None;
            }
            Some(
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .expect("UTF-8 integration test stem")
                    .to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();

    let workflow = read_text(&root.join(".github/workflows/vmodv.yml"));
    assert!(
        active_lines(&workflow, false).any(|line| line.contains("make test-e2e-vmodv")),
        "REL-03: .github/workflows/vmodv.yml must invoke `make test-e2e-vmodv`; \
         otherwise integration targets listed only in that Makefile recipe never run in CI"
    );
    let release = read_text(&root.join(".github/workflows/release.yml"));
    for (name, text) in [("vmodv.yml", &workflow), ("release.yml", &release)] {
        assert!(
            active_lines(text, false).any(|line| line.contains("make test-static-guards")),
            "REL-03: {name} must invoke `make test-static-guards`; the static \
             guard `--test` list lives in that Makefile recipe so PR CI and \
             release validation cannot drift apart"
        );
        assert!(
            active_lines(text, false).any(|line| line.contains("make test-e2e-vmodv")),
            "REL-03: {name} must invoke `make test-e2e-vmodv` so the real-binary \
             suites run on that ref"
        );
    }
    let workflow_targets = selected_targets(&workflow, false);

    let makefile = read_text(&root.join("Makefile"));
    let makefile_targets = selected_targets(&makefile, true);
    let covered = workflow_targets
        .union(&makefile_targets)
        .cloned()
        .collect::<BTreeSet<_>>();

    let missing = targets.difference(&covered).cloned().collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "REL-03: top-level crates/modv-app/tests/*.rs targets not executed by CI: \
         {missing:?}. Add `--test <name>` to the Makefile's `test-static-guards` \
         recipe (or `test-e2e-vmodv` for real-binary tests) — both workflows \
         invoke those recipes. Clippy compiling a test does not run it."
    );

    let unknown = covered.difference(&targets).cloned().collect::<Vec<_>>();
    assert!(
        unknown.is_empty(),
        "REL-03: `--test` targets in .github/workflows/vmodv.yml or Makefile \
         do not name existing top-level crates/modv-app/tests/*.rs files: {unknown:?}. \
         Fix the stale or misspelled target names."
    );
}
