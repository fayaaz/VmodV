//! RED test for filesystem discovery of user-authored WGSL + JSON modules.
//!
//! Scope (BUGS.md item 5, user-chosen design): a user drops a descriptor
//! (`.json`) plus optionally a sibling `.wgsl` file into a module directory
//! under their media folder. Discovery must find those descriptors, attach
//! sibling WGSL source, skip malformed/hidden/temp files and deduplicate by
//! module name — without touching GPU or registry code.
//!
//! # TEMPORARY RED SHIM — DELETE IN GREEN
//!
//! `load_user_modules` does not exist in production yet. The local shim below
//! has the exact expected signature and returns an empty `Vec` so this test
//! compiles today and fails on the assertion (never a compile error).
//!
//! GREEN implementer: delete the shim function and its comment block, add
//! `load_user_modules` to the `use modv_renderers::user_module::{...}` import
//! and implement discovery in `crates/modv-renderers/src/user_module.rs`.
//! Keep the assertions unchanged.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use modv_renderers::user_module::UserModuleDescriptor;

// ---------------------------------------------------------------------------
// BEGIN TEMPORARY RED SHIM (delete in GREEN, import the real API instead).
// ---------------------------------------------------------------------------
fn load_user_modules(_dirs: &[PathBuf]) -> Vec<UserModuleDescriptor> {
    Vec::new()
}
// ---------------------------------------------------------------------------
// END TEMPORARY RED SHIM.
// ---------------------------------------------------------------------------

const SIBLING_WGSL: &str = "@fragment fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> { return vec4<f32>(p.x / 100.0, 0.0, 0.0, 1.0); }";
const INLINE_WGSL: &str =
    "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }";

/// Tiny std-only temp directory with a unique name; removed on drop.
struct FixtureDir {
    root: PathBuf,
}

impl FixtureDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "vmodv_user_module_discovery_{}_{}_{}",
            std::process::id(),
            nanos,
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("create fixture root");
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture subdirectory");
        }
        fs::write(path, contents).expect("write fixture file");
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn name_of(descriptor: &UserModuleDescriptor) -> &str {
    &descriptor.name
}

#[test]
fn load_user_modules_discovers_sibling_and_inline_descriptors_only() {
    let fixture = FixtureDir::new();

    // (a) Valid descriptor naming a sibling WGSL file. Note the uppercase
    // extension on the second descriptor: discovery must be case-insensitive.
    fixture.write(
        "module-a/00_sibling.json",
        r#"{"name":"Sine Field","wgsl":"shader.wgsl","entry":"fs_main","audio":true}"#,
    );
    fixture.write("module-a/shader.wgsl", SIBLING_WGSL);

    // (b) Valid inline descriptor with no sibling WGSL file at all.
    fixture.write(
        "module-a/01_inline.JSON",
        &format!(r#"{{"name":"Inline Glow","wgslSource":"{INLINE_WGSL}"}}"#),
    );

    // (c) Malformed descriptor: must be skipped, not panic or yield a module.
    fixture.write("module-a/02_broken.json", r#"{"name":"Broken", oops"#);

    // (d) Hidden and temp files with otherwise valid contents: skipped.
    fixture.write(
        "module-a/.hidden.json",
        &format!(r#"{{"name":"Hidden","wgslSource":"{INLINE_WGSL}"}}"#),
    );
    fixture.write(
        "module-a/~temp.json",
        &format!(r#"{{"name":"Temp","wgslSource":"{INLINE_WGSL}"}}"#),
    );

    // A duplicate name in a later directory must be dropped; the first
    // (sorted) directory's copy wins, including its sibling WGSL source.
    fixture.write(
        "module-b/00_duplicate.json",
        &format!(r#"{{"name":"Sine Field","wgslSource":"{INLINE_WGSL}"}}"#),
    );

    let dirs = vec![fixture.root.join("module-a"), fixture.root.join("module-b")];
    let got = load_user_modules(&dirs);

    let expected = vec![
        UserModuleDescriptor {
            name: "Sine Field".to_string(),
            author: String::new(),
            version: "1.0.0".to_string(),
            description: String::new(),
            category: None,
            license: None,
            wgsl_source: SIBLING_WGSL.to_string(),
            entry: "fs_main".to_string(),
            audio: true,
            preview_with_output: true,
            props: Vec::new(),
        },
        UserModuleDescriptor {
            name: "Inline Glow".to_string(),
            author: String::new(),
            version: "1.0.0".to_string(),
            description: String::new(),
            category: None,
            license: None,
            wgsl_source: INLINE_WGSL.to_string(),
            entry: "fs_main".to_string(),
            audio: false,
            preview_with_output: true,
            props: Vec::new(),
        },
    ];

    assert_eq!(
        got.iter().map(name_of).collect::<Vec<_>>(),
        vec!["Sine Field", "Inline Glow"],
        "discovered module names/order (malformed, hidden, temp and duplicate files must be skipped)"
    );
    assert_eq!(
        got, expected,
        "discovered descriptors (sibling and inline WGSL sources must be preserved)"
    );
}
