//! REL-08: Docker image builds in CI reuse the GitHub Actions BuildKit cache.
//!
//! The Rust toolchain image installs a large apt/GStreamer/Vulkan layer stack
//! plus rustup. A fresh runner that cannot import that cache redoes all of it,
//! once per job. This guard keeps every build path cached:
//!
//! * `docker/build-push-action` steps must import *and* export `type=gha`
//!   cache (read-only steps let the cache rot; export-only steps never hit).
//! * Workflows must not run inline `docker build` themselves: those bypass the
//!   cache and the Makefile build path. The Makefile owns image builds.
//! * The Makefile image builds (`docker-rust`, `docker-wasm`) use
//!   `docker buildx build --load` and forward `DOCKER_BUILD_ARGS`, so CI can
//!   pass `--cache-from type=gha` through `make` and reuse the image the
//!   workflow already built. Everything that needs an image depends on those
//!   targets instead of rebuilding it inline.
//! * Workflows that build images pass `DOCKER_BUILD_ARGS` to the Makefile, so
//!   the `make` targets do not fall back to an uncached rebuild.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test ci_docker_cache -- --test-threads=1"
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

fn workflow_files() -> Vec<PathBuf> {
    let dir = repo_root().join(".github/workflows");
    let mut paths = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .filter_map(|entry| {
            let path = entry.expect("read workflow entry").path();
            let is_yaml = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "yml" || ext == "yaml");
            is_yaml.then_some(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "no workflows found in {}", dir.display());
    paths
}

/// The recipe block for a Makefile target: the target line and every following
/// tab-indented recipe line (including `\` continuations), stopping at the next
/// target or non-recipe line.
fn makefile_recipe(makefile: &str, target: &str) -> String {
    let mut recipe = String::new();
    let mut in_recipe = false;
    for line in makefile.lines() {
        if !in_recipe {
            if line.starts_with(&format!("{target}:")) {
                in_recipe = true;
                recipe.push_str(line);
                recipe.push('\n');
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

/// Every workflow that builds a Docker image.
fn docker_workflows() -> Vec<(PathBuf, String)> {
    workflow_files()
        .into_iter()
        .map(|path| {
            let text = read_text(&path);
            (path, text)
        })
        .filter(|(_, text)| {
            text.contains("docker/build-push-action") || text.contains("docker build")
        })
        .collect()
}

#[test]
fn build_push_steps_import_and_export_the_gha_cache() {
    let mut steps = 0;
    for (path, text) in docker_workflows() {
        let uses = text.matches("uses: docker/build-push-action").count();
        let imports = text.matches("cache-from: type=gha").count();
        let exports = text.matches("cache-to: type=gha").count();
        assert_eq!(
            uses,
            imports,
            "REL-08: {} has {uses} docker/build-push-action step(s) but {imports} \
             `cache-from: type=gha` line(s); every image build must import the cache",
            path.display()
        );
        assert_eq!(
            uses,
            exports,
            "REL-08: {} has {uses} docker/build-push-action step(s) but {exports} \
             `cache-to: type=gha` line(s); a build that never exports leaves the cache \
             cold for every later job",
            path.display()
        );
        steps += uses;
    }
    assert!(
        steps > 0,
        "REL-08: expected at least one docker/build-push-action step in CI"
    );
}

#[test]
fn workflows_do_not_run_inline_docker_builds() {
    for (path, text) in docker_workflows() {
        for (index, line) in text.lines().enumerate() {
            let command = line.trim_start();
            if command.starts_with('#') {
                continue;
            }
            // Also catch a one-line `run: docker build ...` step.
            let command = command
                .strip_prefix("run:")
                .map(str::trim_start)
                .unwrap_or(command);
            for prefix in ["docker build ", "docker buildx build "] {
                assert!(
                    !command.starts_with(prefix),
                    "REL-08: {}:{} runs an inline `{prefix}...`; workflow image builds \
                     must use docker/build-push-action with type=gha cache, and make \
                     builds must go through the Makefile's cached `docker-rust` target",
                    path.display(),
                    index + 1
                );
            }
        }
    }
}

#[test]
fn makefile_image_builds_use_buildx_and_forward_cache_args() {
    let makefile = read_text(&repo_root().join("Makefile"));
    for target in ["docker-rust", "docker-wasm"] {
        let recipe = makefile_recipe(&makefile, target);
        assert!(
            recipe.contains("buildx build"),
            "REL-08: Makefile target `{target}` must build with `docker buildx build` \
             so `DOCKER_BUILD_ARGS` cache flags reach BuildKit; recipe was:\n{recipe}"
        );
        assert!(
            recipe.contains("--load"),
            "REL-08: Makefile target `{target}` must pass `--load` so the cached \
             image stays available to the following `docker run`; recipe was:\n{recipe}"
        );
        assert!(
            recipe.contains("$(DOCKER_BUILD_ARGS)"),
            "REL-08: Makefile target `{target}` must forward `$(DOCKER_BUILD_ARGS)` \
             so CI can pass `--cache-from type=gha`; recipe was:\n{recipe}"
        );
    }

    // Consumers must build through the cached image target, not inline.
    for target in ["preview-wasm", "web-check", "web-build"] {
        let recipe = makefile_recipe(&makefile, target);
        assert!(
            recipe.contains("docker-wasm"),
            "REL-08: Makefile target `{target}` must depend on `docker-wasm` rather \
             than rebuilding the wasm image inline; recipe was:\n{recipe}"
        );
        assert!(
            !recipe.contains("docker build"),
            "REL-08: Makefile target `{target}` must not run an uncached \
             `docker build`; recipe was:\n{recipe}"
        );
    }
}

#[test]
fn docker_workflows_pass_cache_args_to_make() {
    for (path, text) in docker_workflows() {
        assert!(
            text.contains("DOCKER_BUILD_ARGS:"),
            "REL-08: {} builds Docker images but does not set `DOCKER_BUILD_ARGS`; \
             the `make` targets would rebuild the toolchain image without the GHA cache",
            path.display()
        );
    }
}
