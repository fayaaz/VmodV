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

/// Recursively collect every regular file under `dir` (std-only, no walkdir).
fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read directory {}: {error}", dir.display()));
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(error) => panic!("cannot read an entry in {}: {error}", dir.display()),
        };
        if path.is_dir() {
            collect_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
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

    // Development-only decoys in BOTH copied trees. A real GStreamer devel MSI
    // installs thousands of these (static libs, PDBs, libtool archives and
    // pkg-config files) alongside the runtime DLLs; plain text is enough here.
    for dir in [fake_gst.join("bin"), fake_gst.join("lib/gstreamer-1.0")] {
        for name in ["stub.a", "stub.pdb", "stub.la", "stub.pc"] {
            let decoy = dir.join(name);
            fs::write(&decoy, b"decoy development-only file\n")
                .unwrap_or_else(|error| panic!("cannot write {}: {error}", decoy.display()));
        }
    }

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

    // Only runtime files belong in the ZIP. Development-only files (*.a,
    // *.pdb, *.la, *.pc) make up ~84% of the produced archive and are useless
    // at runtime, so both copied GStreamer trees must be trimmed of them while
    // keeping every DLL.
    let staged_gst = staging.join("gstreamer");
    let mut staged_files = Vec::new();
    collect_files(&staged_gst, &mut staged_files);

    for dll in [
        "gstreamer/bin/gstreamer-1.0.dll",
        "gstreamer/lib/gstreamer-1.0/libstub.dll",
    ] {
        let path = staging.join(dll);
        assert!(
            staged_files.contains(&path),
            "runtime DLL was dropped from the staged tree: {} (staged files: {staged_files:#?})",
            path.display()
        );
    }

    for path in &staged_files {
        let is_development_only = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| matches!(extension, "a" | "pdb" | "la" | "pc"))
            .unwrap_or(false);
        assert!(
            !is_development_only,
            "development-only file was staged into the portable ZIP: {}; \
             exclude *.a, *.pdb, *.la and *.pc from both copied GStreamer trees",
            path.display()
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
        ("vmodv.yml", repo_root.join(".github/workflows/vmodv.yml")),
        (
            "release.yml",
            repo_root.join(".github/workflows/release.yml"),
        ),
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

/// Both `windows-portable` jobs must use a Windows-native pkg-config to read
/// the GStreamer devel `.pc` files. `pkg-config-lite` splits its search path on
/// `:` (POSIX semantics), so the drive letter in
/// `C:/gstreamer/1.0/msvc_x86_64/lib/pkgconfig` is misread as two separate
/// paths (`C` and `/gstreamer/...`) and `glib-2.0` is never found — the release
/// Windows build then fails at the cargo build step with a pkg-config error.
/// Installing Windows-native `pkgconf` via Chocolatey is not enough, though:
/// its MSI adds pkgconf.exe's directory to PATH only for future processes,
/// while the runner captured PATH at startup. A bare `PKG_CONFIG=pkgconf`
/// therefore fails with a CreateProcess error in the glib-sys build script.
/// Each workflow must refresh PATH from the registry, resolve
/// `Get-Command pkgconf.exe` to an absolute path and export that absolute path
/// as `PKG_CONFIG`, without leaving the old bare line behind. Moreover,
/// pkgconf 3.0.10 (the Windows MSI) ALSO splits `PKG_CONFIG_PATH` on ':', so a
/// drive-letter path like `C:/gstreamer/...` is still mangled into `C` +
/// `/gstreamer/...`. Windows resolves a path beginning with '/' as rooted on
/// the process's current drive, so the devel `.pc` files must be copied into a
/// colon-free root-level `gst-pkgconfig` directory (on the system drive and the
/// workspace drive) and `PKG_CONFIG_PATH=/gst-pkgconfig` exported.
///
/// Chocolatey must not be a dependency at all: its V2 feed intermittently
/// returns 503 / "Unable to find package 'pkgconf'". Each workflow must instead
/// download a pinned official pkgconf MSI (`pkgconf-x64-3.0.11.msi` from the
/// pkgconf 3.0.11 GitHub release) into the already-cached `.gst-cache`
/// directory, verify its SHA256 and install it silently with `msiexec.exe`.
#[test]
fn ci_windows_uses_native_pkgconf_for_drive_letter_paths() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);

    let workflows = [
        ("vmodv.yml", repo_root.join(".github/workflows/vmodv.yml")),
        (
            "release.yml",
            repo_root.join(".github/workflows/release.yml"),
        ),
    ];

    const BROKEN_TOOL: &str = "pkgconfiglite";
    // Chocolatey's V2 feed proved flaky (HTTP 503 / "Unable to find package
    // 'pkgconf'"), so pkgconf must instead be installed from a pinned official
    // MSI URL cached in `.gst-cache`. Chocolatey must no longer be a dependency.
    const CHOCO_INSTALL: &str = "choco install pkgconf";
    const PKGCONF_MSI: &str = "pkgconf-x64-3.0.11.msi";
    const MSI_EXEC: &str = "msiexec.exe";
    const PKGCONF_RELEASE_URL: &str = "pkgconf/releases/download/pkgconf-3.0.11/";
    const RESOLVE_PKGCONF: &str = "Get-Command pkgconf.exe";
    const ABSOLUTE_SELECTION: &str = "PKG_CONFIG=$pkgconf";
    // Matches the old `echo "PKG_CONFIG=pkgconf" >> "$GITHUB_ENV"` line only.
    // The robust `PKG_CONFIG=$pkgconf` form does not contain this substring.
    const LEGACY_BARE_SELECTION: &str = "PKG_CONFIG=pkgconf\"";
    // pkgconf 3.0.10 splits PKG_CONFIG_PATH on ':' too, so the drive-letter
    // path is still mangled. The fix is a colon-free `/gst-pkgconfig` path
    // (rooted on the current drive on Windows), not the C:/gstreamer/... value.
    const GST_PKGCONFIG_PATH: &str = "PKG_CONFIG_PATH=/gst-pkgconfig";
    // CI run 44672421805: the Git Bash `echo ... >> "$GITHUB_ENV"` export was
    // rewritten by MSYS path conversion into
    // `C:/Program Files/Git/gst-pkgconfig`, which pkgconf then split on ':', so
    // the .pc files staged at C:\gst-pkgconfig were never found. The value must
    // instead be written literally from the pwsh install step, using the same
    // `Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append` form as the
    // adjacent `PKG_CONFIG=$pkgconf` export. We assert this precise token (the
    // equivalent `Add-Content -Path $env:GITHUB_ENV -Value '...'` form is not
    // accepted by this guard).
    const PWSH_GST_PKGCONFIG_EXPORT: &str = "\"PKG_CONFIG_PATH=/gst-pkgconfig\" | Out-File";
    // The Git Bash line that triggers MSYS conversion must be removed from the
    // "Configure GStreamer build environment" step in both workflows.
    const BASH_GST_PKGCONFIG_EXPORT: &str =
        "echo \"PKG_CONFIG_PATH=/gst-pkgconfig\" >> \"$GITHUB_ENV\"";
    // The pwsh install step must stage the GStreamer devel .pc files into the
    // root-level gst-pkgconfig directories. `Copy-Item` + `*.pc` prove that
    // staging happens, not merely that a path string is exported.
    const GST_PKGCONFIG_DIR: &str = "gst-pkgconfig";
    const PC_COPY_COMMAND: &str = "Copy-Item";
    const PC_FILE_GLOB: &str = "*.pc";
    // Each `Start-Process msiexec.exe` invocation must capture the process
    // object with `-PassThru` and throw when its exit code is non-zero. Without
    // it a failed MSI install is silent, so a later step fails far from the
    // real cause. Accept either the captured process' `.ExitCode` or the
    // `$LASTEXITCODE` automatic variable as the checked value.
    const MSI_PASSTHRU: &str = "-PassThru";
    const MSI_EXIT_CODE: &str = ".ExitCode";
    const MSI_LASTEXITCODE: &str = "$LASTEXITCODE";
    // The GStreamer devel MSI does not reliably create the hard-coded
    // `...\lib\pkgconfig` directory (CI run 44667087284 failed with
    // "Cannot find path 'C:\gstreamer\1.0\msvc_x86_64\lib\pkgconfig'"). The
    // `.pc` directory must be discovered by finding a known `.pc` file under
    // the install root instead of being assumed.
    const GST_PC_DISCOVERY: &str = "Get-ChildItem";
    const GST_PC_MARKER: &str = "glib-2.0.pc";
    // Precise form of the removed hard-coded `$gstPc` assignment. Using the
    // full `<variable> = '<literal>'` spelling avoids rejecting other, still
    // valid uses of the C:/gstreamer path (GITHUB_PATH lines, comments, etc.).
    const LEGACY_HARDCODED_GST_PC: &str = r"$gstPc = 'C:\gstreamer\1.0\msvc_x86_64\lib\pkgconfig'";
    // CI run 44669943303: both GStreamer MSIs exited 0 but no `glib-2.0.pc`
    // existed under C:\gstreamer — the MSI's default install location on
    // windows-latest is not the expected root. The GStreamer runtime and devel
    // installs must pass INSTALLDIR explicitly as a public MSI property. A
    // verbose MSI log (under .gst-cache) must also be emitted so a future
    // install-location regression is diagnosable. The pinned pkgconf MSI needs
    // neither: it is only downloaded/verified/installed, not located by path.
    const GST_INSTALL_DIR: &str = r"INSTALLDIR=C:\gstreamer\1.0\msvc_x86_64";
    const MSI_VERBOSE_LOG: &str = "/L*V";
    // CI run 44674949582: even after the pwsh step wrote the colon-free
    // `PKG_CONFIG_PATH=/gst-pkgconfig` to GITHUB_ENV literally, the cargo build
    // script still saw `C:/Program Files/Git/gst-pkgconfig`. Cause: the
    // "Build modv (x86_64-pc-windows-msvc, release)" step ran under `shell:
    // bash` (Git Bash/MSYS), which rewrites POSIX-looking env values such as
    // `/gst-pkgconfig` when launching native `cargo.exe`. The step must use
    // `shell: pwsh` so the literal value reaches cargo. Token: the step's
    // `name:` line immediately followed by the `shell: pwsh` line (8-space
    // indentation, as written in both workflow files; both use LF endings).
    const BUILD_STEP_NAME: &str = "- name: Build modv (x86_64-pc-windows-msvc, release)";
    const BUILD_STEP_PWSH: &str =
        "- name: Build modv (x86_64-pc-windows-msvc, release)\n        shell: pwsh";
    // CI run 44680000492: the linker saw our hard-coded
    // `/LIBPATH:C:\gstreamer\1.0\msvc_x86_64\lib` but `gstvideo-1.0.lib` was not
    // there — the GStreamer devel MSI placed the import libraries elsewhere.
    // The pwsh install step must therefore discover the real library directory
    // at runtime by locating a known import library (`gstvideo-1.0.lib`) under
    // the install root, then derive RUSTFLAGS from that discovered directory
    // (`-L$libDir`) rather than hard-coding the expected path.
    const GST_LIB_MARKER: &str = "gstvideo-1.0.lib";
    // The `-Filter` argument ties the search to `Get-ChildItem` (the devel .pc
    // discovery uses the same cmdlet with a different filter), proving the
    // import library is actually searched for rather than merely named.
    const GST_LIB_DISCOVERY: &str = "-Filter 'gstvideo-1.0.lib'";
    const RUSTFLAGS_LIB_DIR: &str = "RUSTFLAGS=-L$libDir";
    // The old hard-coded library search path must be removed: the MSI's real
    // install location differs, so this /LIBPATH points at a directory that
    // does not contain gstvideo-1.0.lib, causing LNK1181 at final link.
    const LEGACY_HARDCODED_RUSTFLAGS: &str = r"RUSTFLAGS=-LC:\gstreamer\1.0\msvc_x86_64\lib";
    // CI run 44682481541: the build and link now succeed because RUSTFLAGS is
    // derived from the discovered `$libDir`, but the "Stage portable layout"
    // step still passes a hard-coded `--gstreamer-root "C:/gstreamer/1.0/msvc_x86_64"`.
    // The MSI actually installed GStreamer under the nested
    // `C:\gstreamer\1.0\msvc_x86_64\1.0\msvc_x86_64` path (visible from the
    // discovered gstvideo-1.0.lib), so staging cannot find `bin/` and fails with
    // "missing GStreamer bin directory". The pwsh install step must derive the
    // real root from `$libDir` (one directory above the import libs), convert it
    // to forward slashes and export it as GSTREAMER_ROOT; the stage step must
    // then consume `$GSTREAMER_ROOT` instead of the literal.
    const GSTREAMER_ROOT_EXPORT: &str = "GSTREAMER_ROOT=";
    const GST_ROOT_DERIVATION: &str = "Split-Path -Parent $libDir";
    const STAGE_GSTREAMER_ROOT: &str = "--gstreamer-root \"$GSTREAMER_ROOT\"";
    // The old literal must be gone from the stage step, otherwise staging keeps
    // copying from a bin/ directory that does not exist at that path.
    const LEGACY_HARDCODED_GSTREAMER_ROOT: &str =
        "--gstreamer-root \"C:/gstreamer/1.0/msvc_x86_64\"";

    for (name, path) in workflows {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

        assert!(
            !text.contains(BROKEN_TOOL),
            "{name}: forbidden broken tool `{BROKEN_TOOL}` is still present; \
             Windows drive-letter pkg-config paths require Windows-native pkgconf"
        );
        assert!(
            text.contains(PKGCONF_MSI),
            "{name}: missing pinned pkgconf MSI `{PKGCONF_MSI}`; \
             Chocolatey is no longer a dependency"
        );
        assert!(
            text.contains(MSI_EXEC),
            "{name}: missing `msiexec.exe` install invocation for the pinned pkgconf MSI"
        );
        assert!(
            text.contains(PKGCONF_RELEASE_URL),
            "{name}: missing pinned pkgconf release URL `{PKGCONF_RELEASE_URL}`"
        );
        assert!(
            !text.contains(CHOCO_INSTALL),
            "{name}: forbidden Chocolatey install `{CHOCO_INSTALL}` is still present; \
             the V2 feed is flaky, so pkgconf must come from the pinned official MSI"
        );
        assert!(
            text.contains(RESOLVE_PKGCONF),
            "{name}: missing explicit pkgconf.exe resolution with hard failure `{RESOLVE_PKGCONF}`; \
             a bare command name cannot be found because the MSI-changed PATH is absent from the runner's environment"
        );
        assert!(
            text.contains(ABSOLUTE_SELECTION),
            "{name}: missing absolute pkgconf path export `{ABSOLUTE_SELECTION}`"
        );
        assert!(
            !text.contains(LEGACY_BARE_SELECTION),
            "{name}: legacy bare pkg-config selection `{LEGACY_BARE_SELECTION}` is still present; \
             it would overwrite the absolute PKG_CONFIG path written to GITHUB_ENV"
        );
        assert!(
            text.contains(GST_PKGCONFIG_PATH),
            "{name}: missing the colon-free GStreamer pkgconfig path `{GST_PKGCONFIG_PATH}`; \
             a drive-letter path is still split on ':' by pkgconf 3.0.10"
        );
        assert!(
            text.contains(PWSH_GST_PKGCONFIG_EXPORT),
            "{name}: missing the PowerShell export `{PWSH_GST_PKGCONFIG_EXPORT}`; \
             the colon-free PKG_CONFIG_PATH must be written from the pwsh install step \
             (where the value is stored literally), not from a Git Bash step where \
             MSYS path conversion rewrites the leading '/'"
        );
        assert!(
            !text.contains(BASH_GST_PKGCONFIG_EXPORT),
            "{name}: forbidden Git Bash line `{BASH_GST_PKGCONFIG_EXPORT}` is still present; \
             MSYS path conversion rewrites /gst-pkgconfig into a colon-containing \
             'C:/Program Files/Git/gst-pkgconfig' that pkgconf cannot parse"
        );
        assert!(
            text.contains(GST_PKGCONFIG_DIR),
            "{name}: missing the root-level `{GST_PKGCONFIG_DIR}` directory; \
             the devel .pc files must be staged into a colon-free root-level directory"
        );
        assert!(
            text.contains(PC_COPY_COMMAND),
            "{name}: missing `{PC_COPY_COMMAND}` staging of the GStreamer devel .pc files \
             into the root-level gst-pkgconfig directories"
        );
        assert!(
            text.contains(PC_FILE_GLOB),
            "{name}: missing the `{PC_FILE_GLOB}` glob; the GStreamer devel .pc files \
             must be copied into the root-level gst-pkgconfig directories"
        );
        assert!(
            text.contains(MSI_PASSTHRU),
            "{name}: missing `-PassThru` on the `Start-Process msiexec.exe` calls; \
             without the captured process object a failed MSI install cannot be detected"
        );
        assert!(
            text.contains(MSI_EXIT_CODE) || text.contains(MSI_LASTEXITCODE),
            "{name}: missing an MSI exit-code check (`.ExitCode` or `$LASTEXITCODE`); \
             a failed runtime, devel or pkgconf MSI install must throw instead of being silent"
        );
        assert!(
            text.contains(GST_PC_DISCOVERY),
            "{name}: missing `{GST_PC_DISCOVERY}` discovery of the GStreamer devel .pc \
             directory; the hard-coded path may not exist after the MSI install"
        );
        assert!(
            text.contains(GST_PC_MARKER),
            "{name}: missing `{GST_PC_MARKER}` discovery marker; the .pc directory must \
             be located by finding a known .pc file under the GStreamer install root"
        );
        assert!(
            !text.contains(LEGACY_HARDCODED_GST_PC),
            "{name}: the hard-coded `{LEGACY_HARDCODED_GST_PC}` assignment is still present; \
             the devel MSI does not reliably create that directory, so it must be discovered"
        );
        assert!(
            text.contains(GST_INSTALL_DIR),
            "{name}: missing explicit GStreamer MSI install root `{GST_INSTALL_DIR}`; \
             the MSI's default install location does not put glib-2.0.pc where discovery expects it"
        );
        assert!(
            text.contains(MSI_VERBOSE_LOG),
            "{name}: missing verbose MSI log flag `{MSI_VERBOSE_LOG}` on the GStreamer installs; \
             a future install-location regression must be diagnosable from a log"
        );
        assert!(
            text.contains(BUILD_STEP_PWSH),
            "{name}: the `{BUILD_STEP_NAME}` step must use `shell: pwsh`, not `bash`; \
             Git Bash/MSYS rewrites the POSIX-looking `PKG_CONFIG_PATH=/gst-pkgconfig` \
             into `C:/Program Files/Git/gst-pkgconfig` when launching native cargo.exe, \
             so pkgconf cannot find the staged .pc files (CI run 44674949582)"
        );
        assert!(
            text.contains(GST_LIB_MARKER),
            "{name}: missing import-library discovery marker `{GST_LIB_MARKER}`; \
             the GStreamer devel MSI places gstvideo-1.0.lib outside the hard-coded \
             lib path, so its real location must be discovered at runtime \
             (CI run 44680000492)"
        );
        assert!(
            text.contains(GST_LIB_DISCOVERY),
            "{name}: missing a `Get-ChildItem` search for `{GST_LIB_MARKER}` \
             (`{GST_LIB_DISCOVERY}`); the import library directory must be discovered, \
             not assumed"
        );
        assert!(
            text.contains(RUSTFLAGS_LIB_DIR),
            "{name}: missing dynamic rustc library search path export `{RUSTFLAGS_LIB_DIR}`; \
             RUSTFLAGS must be derived from the discovered library directory instead \
             of a hard-coded path (CI run 44680000492)"
        );
        assert!(
            !text.contains(LEGACY_HARDCODED_RUSTFLAGS),
            "{name}: the hard-coded `{LEGACY_HARDCODED_RUSTFLAGS}` export is still present; \
             the devel MSI does not place gstvideo-1.0.lib at that path, so the final \
             link fails with LNK1181 — derive RUSTFLAGS from the discovered directory"
        );
        assert!(
            text.contains(GSTREAMER_ROOT_EXPORT),
            "{name}: missing the `{GSTREAMER_ROOT_EXPORT}` export from the pwsh install step; \
             the real GStreamer root must be exported to GITHUB_ENV (with forward slashes) \
             so the stage step can find the installed bin/ directory (CI run 44682481541)"
        );
        assert!(
            text.contains(GST_ROOT_DERIVATION),
            "{name}: missing GStreamer root derivation `{GST_ROOT_DERIVATION}`; \
             the real root must be derived from the discovered import-library directory \
             (`$libDir`) because the MSI nests its install one level deeper than expected \
             (CI run 44682481541)"
        );
        assert!(
            text.contains(STAGE_GSTREAMER_ROOT),
            "{name}: the Stage portable layout step must pass `{STAGE_GSTREAMER_ROOT}`; \
             it must consume the exported GStreamer root so staging copies the actual \
             bin/ and lib/gstreamer-1.0/ directories (CI run 44682481541)"
        );
        assert!(
            !text.contains(LEGACY_HARDCODED_GSTREAMER_ROOT),
            "{name}: the hard-coded `{LEGACY_HARDCODED_GSTREAMER_ROOT}` is still present in \
             the Stage portable layout step; the MSI's real install location differs, so \
             staging fails with 'missing GStreamer bin directory' — use `$GSTREAMER_ROOT` \
             (CI run 44682481541)"
        );
    }
}

/// Every build job in both CI workflows must persist its expensive caches
/// across GitHub Actions runs: the Rust registry/git (mounted into Docker via
/// `CARGO_VOLUME`), the workspace `crates/target` build directory, and — on the
/// Windows jobs — the two large GStreamer MSI downloads. Without this, each run
/// re-downloads crates and rebuilds the whole workspace from scratch, which is
/// where most of the CI time is spent.
///
/// The cache must be keyed on the dependency lockfile so a lockfile change
/// invalidates it, and each Linux Docker job plus each Windows job needs its
/// own `actions/cache@v4` step. Pure string scanning: no YAML crate, no
/// network, no env mutation.
#[test]
fn ci_workflows_cache_cargo_and_gstreamer_downloads() {
    const CACHE_ACTION: &str = "actions/cache@v4";
    const CACHE_STEP: &str = "uses: actions/cache@v4";
    const LOCKFILE_KEY: &str = "hashFiles('crates/Cargo.lock')";
    const CARGO_CACHE_DIR: &str = ".cargo-cache";
    const CARGO_VOLUME_ENV: &str = "CARGO_VOLUME: ${{ github.workspace }}/.cargo-cache";
    const GST_CACHE_DIR: &str = ".gst-cache";
    // GitHub Actions caches are immutable: the old GStreamer-only key cannot be
    // updated to also hold the newly added pkgconf MSI. The key must be bumped
    // so pkgconf-3.0.11 is cached too, otherwise it re-downloads every run.
    const GST_CACHE_KEY: &str = "gstreamer-msi-1.24.13-pkgconf-3.0.11";
    const TARGET_CACHE_PATH: &str = "crates/target";

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);

    // (workflow name, path, minimum number of cache steps required).
    // vmodv.yml: lint + test + package (Linux Docker) + windows-portable.
    // release.yml: release (Linux Docker) + windows-portable.
    let workflows = [
        (
            "vmodv.yml",
            repo_root.join(".github/workflows/vmodv.yml"),
            4,
        ),
        (
            "release.yml",
            repo_root.join(".github/workflows/release.yml"),
            2,
        ),
    ];

    for (name, path, min_cache_steps) in workflows {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

        for token in [
            CACHE_ACTION,
            LOCKFILE_KEY,
            CARGO_CACHE_DIR,
            CARGO_VOLUME_ENV,
            GST_CACHE_DIR,
            TARGET_CACHE_PATH,
        ] {
            assert!(
                text.contains(token),
                "{name}: missing required caching token `{token}`"
            );
        }

        // The `.gst-cache` key must reflect the newly cached pkgconf MSI.
        // GitHub Actions caches are immutable, so an old cache entry keyed only
        // on the GStreamer version cannot be updated with pkgconf-x64-3.0.11.msi;
        // the pkgconf MSI would be re-downloaded on every run.
        assert!(
            text.contains(GST_CACHE_KEY),
            "{name}: the `.gst-cache` key must include `{GST_CACHE_KEY}`; \
             GitHub Actions caches are immutable, so an old key would force \
             pkgconf-3.0.11 to be re-downloaded on every run"
        );

        let cache_steps = text.matches(CACHE_STEP).count();
        assert!(
            cache_steps >= min_cache_steps,
            "{name}: expected at least {min_cache_steps} occurrences of `{CACHE_STEP}`, \
             found {cache_steps}"
        );
    }
}
