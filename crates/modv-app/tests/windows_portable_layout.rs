//! RED guard for the portable Windows ZIP layout.
//!
//! The runtime contract is: a ZIP extracted anywhere yields
//! `<root>/modv.exe`, `<root>/isf/`, `<root>/textures/`,
//! `<root>/gstreamer/bin/` + `<root>/gstreamer/lib/gstreamer-1.0/` and a
//! `VmodV.cmd` launcher that exports `MODV_PORTABLE_ROOT`, `MODV_ISF_DIR`,
//! `MODV_ASSETS_DIR` and `GST_PLUGIN_PATH` before starting `modv.exe`.
//!
//! `tools/windows-portable.sh --dry-run` must stage that layout from a fake
//! binary and a fake GStreamer root (so the script is testable in the Linux
//! Docker toolchain, not only on a Windows runner). At RED the script is
//! absent, so the status assertion fails with bash's "No such file" error —
//! the missing behaviour, not a compile error.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test windows_portable_layout -- --test-threads=1"
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
fn dry_run_stages_the_portable_windows_layout() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);

    // One holder tree for both the fake inputs and the staging root, so a
    // single guard cleans everything up.
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-windows-portable"));
    let base = holder.path();

    // Fake binary (stub contents are fine: the script copies it as a file).
    let input = base.join("input");
    fs::create_dir_all(&input).expect("create the fake input dir");
    let fake_exe = input.join("modv.exe");
    fs::write(&fake_exe, b"fake modv.exe stub\n").expect("write the fake binary");

    // Fake GStreamer root with the two subtrees the launcher needs.
    let fake_gst = input.join("gstreamer");
    fs::create_dir_all(fake_gst.join("bin")).expect("create the fake gst bin dir");
    fs::create_dir_all(fake_gst.join("lib/gstreamer-1.0")).expect("create the fake gst plugin dir");
    fs::write(fake_gst.join("bin/gstreamer-1.0.dll"), b"stub").expect("write the fake gst dll");
    fs::write(fake_gst.join("lib/gstreamer-1.0/libstub.dll"), b"stub")
        .expect("write the fake gst plugin");

    // The portable root is a distinct directory inside the holder.
    let staging = base.join("portable-root");

    let script = repo_root.join("tools/windows-portable.sh");
    let output = Command::new("bash")
        .arg(&script)
        .arg("--dry-run")
        .arg("--root")
        .arg(&staging)
        .arg("--binary")
        .arg(&fake_exe)
        .arg("--gstreamer-root")
        .arg(&fake_gst)
        .arg("--project-root")
        .arg(&repo_root)
        .current_dir(&repo_root)
        .output()
        .expect("spawn bash tools/windows-portable.sh --dry-run");

    assert!(
        output.status.success(),
        "windows-portable.sh --dry-run failed with status {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Staged binary.
    let staged_exe = staging.join("modv.exe");
    assert!(
        staged_exe.is_file(),
        "{} is not a file",
        staged_exe.display()
    );

    // ISF corpus is copied from <project-root>/resources/isf.
    let staged_isf = staging.join("isf/Bloom.fs");
    assert!(
        staged_isf.is_file(),
        "{} is not a file",
        staged_isf.display()
    );

    // Point10 textures are copied from <project-root>/resources/point10/textures.
    let staged_textures = staging.join("textures");
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

    // GStreamer runtime + plugin trees.
    for relative in ["gstreamer/bin", "gstreamer/lib/gstreamer-1.0"] {
        let path = staging.join(relative);
        assert!(path.is_dir(), "{} is not a directory", path.display());
    }

    // The launcher exports the four env vars and starts modv.exe.
    let launcher = staging.join("VmodV.cmd");
    assert!(launcher.is_file(), "{} is not a file", launcher.display());
    let launcher_text = fs::read_to_string(&launcher)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", launcher.display()));
    for token in [
        "MODV_PORTABLE_ROOT",
        "MODV_ISF_DIR",
        "MODV_ASSETS_DIR",
        "GST_PLUGIN_PATH",
        "modv.exe",
    ] {
        assert!(
            launcher_text.contains(token),
            "{} does not mention {token}:\n{launcher_text}",
            launcher.display()
        );
    }
}

/// Expand the PowerShell `$base` interpolation used by the Windows CI steps
/// and return every effective GStreamer MSI download URL as a plain string.
///
/// The workflows currently write:
///   $base = 'https://gstreamer.freedesktop.org/data/pkg/windows/<ver>/msvc'
///   $runtime = "$base/gstreamer-1.0-msvc-x86_64-<ver>.msi"
///   $devel = "$base/gstreamer-1.0-devel-msvc-x86_64-<ver>.msi"
/// so the absolute URLs only exist once `$base` is substituted. If a workflow
/// ever inlines full URLs instead, the substitution is a no-op and the scan
/// still works. No YAML crate, no network — deterministic string scanning.
fn extract_gstreamer_msi_urls(text: &str) -> Vec<String> {
    const MARKER: &str = "https://gstreamer.freedesktop.org/data/pkg/windows/";

    let base = text.find(MARKER).map(|start| {
        let rest = &text[start..];
        let end = rest
            .find(|c: char| c == '\'' || c == '"' || c.is_whitespace())
            .unwrap_or(rest.len());
        rest[..end].to_string()
    });
    let expanded = match &base {
        Some(base) => text.replace("$base", base.as_str()),
        None => text.to_string(),
    };

    let is_url_char =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/' | ':');
    let mut urls = Vec::new();
    let mut from = 0;
    while let Some(offset) = expanded[from..].find(MARKER) {
        let start = from + offset;
        let rest = &expanded[start..];
        let end = rest.find(|c: char| !is_url_char(c)).unwrap_or(rest.len());
        let token = &rest[..end];
        if token.ends_with(".msi") {
            urls.push(token.to_string());
        }
        from = start + std::cmp::max(end, 1);
    }
    urls
}

/// Both `windows-portable` jobs pin GStreamer MSI downloads. The 1.24.0
/// release was removed from the GStreamer index (HTTP 404), so the pin drifted
/// and broke CI. This guard keeps both workflows on the verified 1.24.13
/// artifacts and catches any future drift between them.
#[test]
fn ci_gstreamer_msi_urls_point_at_the_verified_release() {
    const EXPECTED_RUNTIME: &str = "https://gstreamer.freedesktop.org/data/pkg/windows/1.24.13/msvc/gstreamer-1.0-msvc-x86_64-1.24.13.msi";
    const EXPECTED_DEVEL: &str = "https://gstreamer.freedesktop.org/data/pkg/windows/1.24.13/msvc/gstreamer-1.0-devel-msvc-x86_64-1.24.13.msi";

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);

    let workflows = [
        ("native.yml", repo_root.join(".github/workflows/native.yml")),
        ("release.yml", repo_root.join(".github/workflows/release.yml")),
    ];

    for (name, path) in workflows {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let urls = extract_gstreamer_msi_urls(&text);

        assert!(
            urls.iter().any(|url| url == EXPECTED_RUNTIME),
            "{name}: missing verified runtime MSI URL {EXPECTED_RUNTIME}; found: {urls:#?}"
        );
        assert!(
            urls.iter().any(|url| url == EXPECTED_DEVEL),
            "{name}: missing verified devel MSI URL {EXPECTED_DEVEL}; found: {urls:#?}"
        );
        assert!(
            !urls.iter().any(|url| url.contains("/1.24.0/")),
            "{name}: stale /1.24.0/ GStreamer MSI URL(s) still present; found: {urls:#?}"
        );
    }
}
