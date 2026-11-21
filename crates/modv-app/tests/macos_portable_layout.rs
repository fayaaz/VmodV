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
fn ci_workflow_builds_and_uploads_macos_bundle() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);

    let macos_workflow = repo_root.join(".github/workflows/macos.yml");
    assert!(
        macos_workflow.is_file(),
        ".github/workflows/macos.yml is missing — a macOS build workflow is not implemented"
    );
    let release_workflow = repo_root.join(".github/workflows/release.yml");
    assert!(
        release_workflow.is_file(),
        "{} is not a file",
        release_workflow.display()
    );

    let macos_text = fs::read_to_string(&macos_workflow)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", macos_workflow.display()));
    for token in [
        "macos-15-intel",
        "brew install",
        "gstreamer",
        "PKG_CONFIG_PATH",
        "cargo build --release --manifest-path crates/Cargo.toml -p modv-app --bin modv",
        "tools/macos-portable.sh",
        "actions/upload-artifact@v4",
        "workflow_dispatch",
        "vmodv-native-macos-x86_64-portable",
        "VmodV-Native-macOS-x86_64-portable-",
        // arm64 is built alongside x86_64 via an explicit, pinned matrix.
        "arch: arm64",
        "runner: macos-15",
        "matrix.arch",
        "fail-fast: false",
        "vmodv-native-macos-arm64-portable",
        "VmodV-Native-macOS-arm64-portable-",
        "${{ runner.os }}-${{ matrix.arch }}-cargo-",
    ] {
        assert!(
            macos_text.contains(token),
            ".github/workflows/macos.yml does not mention {token}"
        );
    }

    assert!(
        !macos_text.contains("macos-latest"),
        ".github/workflows/macos.yml still targets the capacity-constrained `macos-latest` arm64 runner"
    );

    let release_text = fs::read_to_string(&release_workflow)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", release_workflow.display()));
    for token in [
        "macos",
        "vmodv-native-macos",
        "actions/download-artifact@v4",
        "dist-macos/*.zip",
        "vmodv-native-macos-x86_64-portable",
        // The release publishes both architectures' portable bundles.
        "arch: arm64",
        "runner: macos-15",
        "matrix.arch",
        "vmodv-native-macos-arm64-portable",
    ] {
        assert!(
            release_text.contains(token),
            ".github/workflows/release.yml does not mention {token}"
        );
    }
}
