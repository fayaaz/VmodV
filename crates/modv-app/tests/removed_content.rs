//! Guard the Point10 removals after the Electron tree is gone.
//!
//! This folds the three JS guards (`point10-removed-effects.test.js`,
//! `cube-removed.test.js` and `point10-disco-ball-removal.test.js`) into one
//! Rust test with the assertions that still apply to the native tree:
//!
//! * the removed module/scene implementation files are absent (including the
//!   Rust Geometric Tunnel port and its exclusive SSAO pass);
//! * the built-in registry does not register a removed name;
//! * the seeded default Scenes do not reference a removed name;
//! * no source file reintroduces the removed implementation tokens.
//!
//! The `Point10 Old Movie` retirement is already pinned by
//! `modv-renderers::shader::modules::point10_effects`'s own test and the
//! default-Scene migration tests, so it is not duplicated here.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test removed_content"
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use modv_app::module_registry::ModuleRegistry;

/// Removed implementation files (legacy JS paths stay here so a revived
/// Electron tree is caught too).
const REMOVED_FILES: &[&str] = &[
    "src/renderer/src/application/point10/scenes/SpectrumBars.js",
    "src/renderer/src/application/point10/scenes/Ferrofluid.js",
    "src/renderer/src/application/renderers/2d/modules/Point10SpectrumBars.js",
    "src/renderer/src/application/renderers/2d/modules/Point10Ferrofluid.js",
    "src/renderer/src/application/point10/scenes/GeometricTunnel.js",
    "src/renderer/src/application/renderers/2d/modules/Point10GeometricTunnel.js",
    "src/renderer/src/application/point10/scenes/DiscoBall.js",
    "src/renderer/src/application/renderers/2d/modules/Point10DiscoBall.js",
    "src/renderer/src/application/renderers/three/index.js",
    "src/renderer/src/application/renderers/three/modules/Cube.js",
    // The Rust port and its exclusive SSAO pass are removed with it.
    "crates/modv-renderers/src/point10/scenes/geometric_tunnel.rs",
    "crates/modv-renderers/src/point10/postfx/ssao.rs",
    "crates/modv-renderers/src/point10/postfx/ssao.wgsl",
];

/// Registry names that must not come back. `Point10 Old Movie` is covered by
/// the shader registry's own retirement test.
const REMOVED_NAMES: &[&str] = &[
    "Point10 Spectrum Bars",
    "Point10 Ferrofluid",
    "Point10 Geometric Tunnel",
    "Point10 Disco Ball",
    "Point10 Cube Floor",
];

/// Exact strings that must not survive in the default-Scene data.
const REMOVED_SCENE_REFERENCES: &[&str] = &[
    "Point10 Spectrum Bars",
    "Point10 Ferrofluid",
    "Point10 Geometric Tunnel",
    "Point10 Disco Ball",
    "SpectrumBars",
    "Point10Ferrofluid",
    "GeometricTunnel",
    "geometric_tunnel",
    "DiscoBall",
];

/// Implementation tokens that must not be wired into the native source. The
/// migration tables in `modv-engine/src/default_scenes.rs` intentionally name
/// retired modules so old presets can be rebuilt, so that file is excluded.
const FORBIDDEN_TOKENS: &[&str] = &[
    "SpectrumBars",
    "Point10Ferrofluid",
    "geometric_tunnel",
    "GeometricTunnel",
    "DiscoBall",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root resolves")
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "target" {
                walk(&path, found);
            }
        } else {
            found.push(path);
        }
    }
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files);
    files.retain(|path| {
        path.extension().and_then(|extension| extension.to_str()) == Some("rs")
            || path.extension().and_then(|extension| extension.to_str()) == Some("wgsl")
    });
    files
}

#[test]
fn removed_implementation_files_are_absent() {
    let root = repo_root();
    for relative in REMOVED_FILES {
        assert!(
            !root.join(relative).exists(),
            "{relative} is a removed module/scene but the file is back"
        );
    }
}

#[test]
fn the_registry_does_not_register_removed_modules_or_scenes() {
    let registry = ModuleRegistry::builtin();
    let registered = registry
        .registered_modules()
        .as_object()
        .expect("registered modules are an object")
        .clone();

    for name in REMOVED_NAMES {
        assert!(
            !registered.contains_key(*name),
            "{name} is still registered by ModuleRegistry"
        );
    }

    let point10_names: Vec<&str> = modv_renderers::point10::all_scenes()
        .iter()
        .map(|scene| scene.name())
        .collect();
    for name in REMOVED_NAMES {
        assert!(
            !point10_names.contains(name),
            "{name} is still returned by point10::all_scenes"
        );
    }
}

#[test]
fn the_seeded_default_scenes_do_not_reference_removed_content() {
    let root = repo_root();
    let path = root.join("crates/modv-engine/data/default-scenes.json");
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

    for reference in REMOVED_SCENE_REFERENCES {
        assert!(
            !contents.contains(reference),
            "default-scenes.json still references removed content: {reference}"
        );
    }
}

#[test]
fn no_native_source_reintroduces_a_removed_implementation() {
    let root = repo_root();
    let removed_scenes = root.join("crates/modv-engine/src/default_scenes.rs");
    let mut offenders = Vec::new();

    for path in rust_sources(&root) {
        if path == removed_scenes || path.ends_with("crates/modv-app/tests/removed_content.rs") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        for token in FORBIDDEN_TOKENS {
            if source.contains(token) {
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                offenders.push(format!("{relative}: {token}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "removed Point10 content is referenced again: {offenders:?}"
    );
}
