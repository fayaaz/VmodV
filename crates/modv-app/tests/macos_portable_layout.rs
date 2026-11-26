//! RED guard for the portable macOS app bundle layout.
//!
//! The runtime contract is: a staging root filled by
//! `tools/macos-portable.sh --dry-run` yields a real `.app` bundle
//! (`VmodV.app/Contents/MacOS/modv` + `Info.plist` + `Resources/isf` +
//! `Resources/textures`) and a `VmodV.command` launcher that exports
//! `MODV_PORTABLE_ROOT`, `MODV_ISF_DIR`, `MODV_ASSETS_DIR`,
//! `GST_PLUGIN_PATH` and `DYLD_FALLBACK_LIBRARY_PATH` before starting the
//! bundled binary.
//!
//! At RED the script and the CI workflow are absent, so the very first
//! assertion fails on missing behaviour — not a compile error.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test macos_portable_layout -- --test-threads=1"
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A unique, collision-free directory name under `std::env::temp_dir()`.
fn unique_dir_name(prefix: &str) -> String {
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{}-{nanos}-{counter}", std::process::id())
}

/// Minimal RAII guard: removes the whole tree on drop, including on panic.
struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(name);
        if path.exists() {
            fs::remove_dir_all(&path).expect("remove a stale temp dir from a previous run");
        }
        fs::create_dir_all(&path).expect("create the temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn dry_run_stages_the_portable_macos_app_bundle() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);

    // One holder tree for both the fake inputs and the staging root, so a
    // single guard cleans everything up.
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-macos-portable"));
    let base = holder.path();

    // Fake binary (stub contents are fine: the script copies it as a file).
    let input = base.join("input");
    fs::create_dir_all(&input).expect("create the fake input dir");
    let fake_bin = input.join("modv");
    fs::write(&fake_bin, b"fake modv stub\n").expect("write the fake binary");

    // The portable root is a distinct directory inside the holder.
    let staging = base.join("portable-root");

    let script = repo_root.join("tools/macos-portable.sh");
    let output = Command::new("bash")
        .arg(&script)
        .arg("--dry-run")
        .arg("--root")
        .arg(&staging)
        .arg("--binary")
        .arg(&fake_bin)
        .arg("--project-root")
        .arg(&repo_root)
        .current_dir(&repo_root)
        .output()
        .expect("spawn bash tools/macos-portable.sh --dry-run");

    // FIRST assertion: make the missing behaviour explicit at RED.
    assert!(
        script.is_file(),
        "tools/macos-portable.sh is missing — macOS packaging is not implemented"
    );

    assert!(
        output.status.success(),
        "macos-portable.sh --dry-run failed with status {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Staged binary inside a real .app bundle.
    let staged_bin = staging.join("VmodV.app/Contents/MacOS/modv");
    assert!(
        staged_bin.is_file(),
        "{} is not a file",
        staged_bin.display()
    );

    // Info.plist must declare the executable and a bundle identifier.
    let info_plist = staging.join("VmodV.app/Contents/Info.plist");
    assert!(
        info_plist.is_file(),
        "{} is not a file",
        info_plist.display()
    );
    let plist_text = fs::read_to_string(&info_plist)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", info_plist.display()));
    for token in ["CFBundleExecutable", "modv", "CFBundleIdentifier"] {
        assert!(
            plist_text.contains(token),
            "{} does not mention {token}:\n{plist_text}",
            info_plist.display()
        );
    }

    // ISF corpus is copied from <project-root>/resources/isf.
    let staged_isf = staging.join("VmodV.app/Contents/Resources/isf/Bloom.fs");
    assert!(
        staged_isf.is_file(),
        "{} is not a file",
        staged_isf.display()
    );

    // Point10 textures are copied from <project-root>/resources/point10/textures.
    let staged_textures = staging.join("VmodV.app/Contents/Resources/textures");
    assert!(
        staged_textures.is_dir(),
        "{} is not a directory",
        staged_textures.display()
    );
    let has_texture = fs::read_dir(&staged_textures)
        .expect("read the staged textures directory")
        .filter_map(Result::ok)
        .any(|entry| entry.path().is_file());
    assert!(
        has_texture,
        "{} contains no copied texture files",
        staged_textures.display()
    );

    // The launcher exports the env vars and starts the bundled binary.
    let launcher = staging.join("VmodV.command");
    assert!(launcher.is_file(), "{} is not a file", launcher.display());
    let launcher_text = fs::read_to_string(&launcher)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", launcher.display()));
    for token in [
        "MODV_PORTABLE_ROOT",
        "MODV_ISF_DIR",
        "MODV_ASSETS_DIR",
        "GST_PLUGIN_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "VmodV.app/Contents/MacOS/modv",
    ] {
        assert!(
            launcher_text.contains(token),
            "{} does not mention {token}:\n{launcher_text}",
            launcher.display()
        );
    }
}

#[test]
fn macos_bundles_are_built_only_on_main() {
    // macOS runners are expensive. release.yml is the sole builder and it
    // triggers only on main; PR and feature-branch macOS bundles are gone.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap_or(root);
    let dir = root.join(".github/workflows");
    for entry in fs::read_dir(&dir).expect("read .github/workflows") {
        let path = entry.expect("read workflow entry").path();
        let is_workflow = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "yml" || ext == "yaml");
        if !path.is_file() || !is_workflow {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name == "release.yml" {
            continue;
        }
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(
            !text.contains("tools/macos-portable.sh") && !text.contains("macos-15"),
            "{name} still builds macOS bundles; those expensive runners must \
             run only on main via release.yml"
        );
    }
    let release =
        fs::read_to_string(root.join(".github/workflows/release.yml")).expect("read release.yml");
    assert!(
        release.contains("  push:\n    branches:\n      - main"),
        "release.yml must trigger macOS builds only on main"
    );
    assert!(
        release.contains("tools/macos-portable.sh"),
        "release.yml no longer builds the macOS bundles at all"
    );
}

#[test]
fn workflows_do_not_reference_the_retired_rust_hybrid_branch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap_or(root);
    let dir = root.join(".github/workflows");
    for entry in fs::read_dir(&dir).expect("read .github/workflows") {
        let path = entry.expect("read workflow entry").path();
        let is_workflow = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "yml" || ext == "yaml");
        if !path.is_file() || !is_workflow {
            continue;
        }
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(
            !text.contains("rust-hybrid"),
            "{} still references the retired `rust-hybrid` branch",
            path.display()
        );
    }
}

#[test]
fn release_workflow_builds_both_macos_architectures() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);
    let release_workflow = repo_root.join(".github/workflows/release.yml");
    let release_text = fs::read_to_string(&release_workflow)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", release_workflow.display()));
    for token in [
        "macos-15-intel",
        "arch: arm64",
        "runner: macos-15",
        "matrix.arch",
        "fail-fast: false",
        "brew install",
        "PKG_CONFIG_PATH",
        "cargo build --release --manifest-path crates/Cargo.toml -p modv-app --bin modv",
        "tools/macos-portable.sh",
        "vmodv-macos-x86_64-portable",
        "VmodV-macOS-x86_64-portable-",
        "vmodv-macos-arm64-portable",
        "VmodV-macOS-arm64-portable-",
        "actions/upload-artifact@v4",
        "actions/download-artifact@v4",
        "dist-macos/*.zip",
    ] {
        assert!(
            release_text.contains(token),
            ".github/workflows/release.yml does not mention {token}"
        );
    }
    assert!(
        !release_text.contains("macos-latest"),
        "release.yml still targets the capacity-constrained `macos-latest` arm64 runner"
    );
}
