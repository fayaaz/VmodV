//! RED guard for the "fat" VmodV AppImage (slug: `fat-appimage`).
//!
//! Requirement: `make appimage-vmodv` must ship the GStreamer runtime that the
//! build container already has (see the `rust` stage of the `Dockerfile`), so a
//! user does not have to apt-install GStreamer for the AppImage to open a
//! camera, a file or the PipeWire screen source. The finished AppDir must look
//! like this:
//!
//! ```text
//! AppRun                                launcher, see the AppRun contract below
//! vmodv.desktop / vmodv.png / .DirIcon  unchanged
//! usr/bin/modv                          the app binary
//! usr/bin/log-run.sh                    unchanged
//! usr/share/vmodv/isf, .../textures     unchanged
//! usr/lib/gstreamer-1.0/*.so            the plugins pipeline.rs actually uses
//!                                       (coreelements/decodebin3 ->
//!                                       libgstreamer1.0-0, typefind / videoconvert
//!                                       / videoscale / videorate / videotestsrc /
//!                                       appsink -> gstreamer1.0-plugins-base,
//!                                       v4l2src / rtspsrc ->
//!                                       gstreamer1.0-plugins-good, videoparsers /
//!                                       mpegtsdemux -> gstreamer1.0-plugins-bad,
//!                                       pipewiresrc -> gstreamer1.0-pipewire, plus
//!                                       the gstreamer1.0-libav avdec_h264 decoder)
//! usr/libexec/gstreamer-1.0/gst-plugin-scanner
//!                                       (flattened from wherever the distro keeps
//!                                       it: Debian puts it in
//!                                       usr/lib/x86_64-linux-gnu/gstreamer1.0/gstreamer-1.0/)
//! usr/lib/*.so.*                        the transitive userspace closure those
//!                                       plugins and `modv` need (glib/gobject,
//!                                       gstreamer core + base/video/audio/pbutils/
//!                                       tag, orc, libav/ffmpeg, pipewire, alsa,
//!                                       v4l2, ...)
//! usr/share/vmodv/gstreamer-bundle.json machine-readable audit manifest
//! usr/share/doc/<package>/copyright     licence texts (libav/ffmpeg is LGPL/GPL)
//! ```
//!
//! The glibc family (`libc.so.6`, `libm.so.6`, `libdl*`, `libpthread*`,
//! `librt*`, the dynamic loader) is never bundled: an AppImage always runs
//! against the host's own glibc, and copying it is how AppImages break.
//!
//! The `.deb` packaging (`tools/vmodv-deb.sh`, `packaging/vmodv/vmodv.control`,
//! `tools/vmodv-deb-check.sh`) deliberately stays as it is: host `Depends` is
//! the correct model for a distro package. Only the AppImage becomes fat.
//!
//! ## The `--dry-run` contract the GREEN phase must implement
//!
//! `tools/vmodv-appimage.sh` gains a `--dry-run` mode modelled on
//! `tools/windows-portable.sh` / `tools/macos-portable.sh`: it stages exactly
//! the layout above from explicit fake inputs, runs no `cargo build`, downloads
//! nothing and never invokes appimagetool. Flags (real-mode default in
//! brackets):
//!
//! ```text
//! --dry-run                  stage from the fake inputs below (no cargo/network)
//! --project-root DIR         repository root [this script's parent dir, i.e. today's $ROOT]
//! --appdir DIR               AppDir to stage into [<project-root>/dist/VmodV.AppDir]
//! --binary FILE              modv binary [<project-root>/crates/target/release/modv]
//! --plugin-root DIR          directory of GStreamer plugin *.so files
//!                            [/usr/lib/x86_64-linux-gnu/gstreamer-1.0]
//! --lib-root DIR             directory holding the userspace shared-library
//!                            closure to stage [the closure computed from --binary
//!                            plus the plugins, e.g. with lddtree/patchelf]
//! --scanner FILE             gst-plugin-scanner helper
//!                            [/usr/lib/x86_64-linux-gnu/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner]
//! --package-list FILE        newline "<package> <version>" records (what
//!                            `dpkg-query -W -f='${Package} ${Version}\n'` prints
//!                            in real mode); drives the manifest and the licence
//!                            lookup below
//! --doc-root DIR             distro doc root for licence texts [/usr/share/doc]
//! ```
//!
//! `APP_VERSION` keeps working as the version override (it is honoured today),
//! so a dry run never needs `tools/version.sh` or git tags.
//!
//! Staging rules these tests enforce:
//!
//! * plugins  -> `usr/lib/gstreamer-1.0/` (flattened, runtime `*.so` only)
//! * scanner  -> `usr/libexec/gstreamer-1.0/gst-plugin-scanner`
//! * libs     -> `usr/lib/` (`*.so` / `*.so.N` runtime files)
//! * manifest -> `usr/share/vmodv/gstreamer-bundle.json`, a JSON object with a
//!   `"packages"` array holding every `--package-list` name and version
//! * licences -> `usr/share/doc/<package>/copyright` for every bundled package
//! * never    -> `*.a`, `*.la`, `*.pc`, `*.pdb`, headers, and the glibc family
//!
//! ## The `AppRun` contract
//!
//! Before launching the app, `packaging/vmodv/AppRun` must export
//!
//! ```sh
//! GST_PLUGIN_PATH_1_0 / GST_PLUGIN_PATH          = $HERE/usr/lib/gstreamer-1.0
//! GST_PLUGIN_SYSTEM_PATH_1_0 / GST_PLUGIN_SYSTEM_PATH = ""   # empty: no host plugins
//! GST_PLUGIN_SCANNER_1_0 / GST_PLUGIN_SCANNER    = $HERE/usr/libexec/gstreamer-1.0/gst-plugin-scanner
//! GST_REGISTRY                                   = writable per-user path (payload is read-only)
//! LD_LIBRARY_PATH                                = $HERE/usr/lib:...
//! ```
//!
//! Blanking `GST_PLUGIN_SYSTEM_PATH*` is the critical half: without it the
//! host's plugins are still probed and clash with the bundled symbols.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test appimage_bundle -- --test-threads=1"
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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

/// Repository root, relative to this crate's manifest directory.
fn repo_root() -> PathBuf {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    repo_root.canonicalize().unwrap_or(repo_root)
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

/// The distro packages the fake `--package-list` claims were staged, as
/// `<package> <version>` (the `dpkg-query -W -f='${Package} ${Version}\n'`
/// shape). Every name and version here is the real one the build container
/// (`modv-rust:local`, Debian 13) ships, so the manifest contract the GREEN
/// phase implements against `dpkg-query` is the same contract.
const PACKAGES: [(&str, &str); 27] = [
    ("gstreamer1.0-plugins-base", "1.26.2-1+deb13u2"),
    ("gstreamer1.0-plugins-good", "1.26.2-1+deb13u2"),
    ("gstreamer1.0-plugins-bad", "1.26.2-3+deb13u3"),
    ("gstreamer1.0-libav", "1.26.2-1+deb13u1"),
    ("gstreamer1.0-pipewire", "1.4.2-1"),
    ("libgstreamer1.0-0", "1.26.2-2"),
    ("libgstreamer-plugins-base1.0-0", "1.26.2-1+deb13u2"),
    ("libgstreamer-plugins-bad1.0-0", "1.26.2-3+deb13u3"),
    ("libglib2.0-0t64", "2.84.4-3~deb13u5"),
    ("liborc-0.4-0t64", "1:0.4.41-1"),
    ("libpipewire-0.3-0t64", "1.4.2-1"),
    ("libasound2t64", "1.2.14-1+deb13u1"),
    ("libv4l-0t64", "1.30.1-1"),
    ("libv4lconvert0t64", "1.30.1-1"),
    ("libavcodec61", "7:7.1.5-0+deb13u1"),
    ("libavutil59", "7:7.1.5-0+deb13u1"),
    ("libavformat61", "7:7.1.5-0+deb13u1"),
    ("libdrm2", "2.4.124-2"),
    ("libwayland-client0", "1.23.1-3"),
    ("libatk1.0-0t64", "2.56.2-1+deb13u2"),
    ("libatspi2.0-0t64", "2.56.2-1+deb13u2"),
    ("libpango-1.0-0", "1.56.3-1"),
    ("libpangocairo-1.0-0", "1.56.3-1"),
    ("libcairo2", "1.18.4-1+b1"),
    ("libcairo-gobject2", "1.18.4-1+b1"),
    ("libharfbuzz0b", "10.2.0-1+deb13u1"),
    ("libfontconfig1", "2.15.0-2.3"),
];

/// Plugin stubs standing in for the bundled GStreamer plugins: the real file
/// names the container's `/usr/lib/x86_64-linux-gnu/gstreamer-1.0/` holds for
/// the elements `crates/modv-media/src/video/pipeline.rs` requires (note that
/// GStreamer 1.26 merged videoconvert+scale into `libgstvideoconvertscale.so`,
/// and that `v4l2src` lives in `libgstvideo4linux2.so`, not `libgstv4l2.so`).
const PLUGINS: [&str; 9] = [
    "libgstcoreelements.so",
    "libgsttypefindfunctions.so",
    "libgstvideoconvertscale.so",
    "libgstvideoparsersbad.so",
    "libgstvideo4linux2.so",
    "libgstrtsp.so",
    "libgstmpegtsdemux.so",
    "libgstpipewire.so",
    "libgstlibav.so",
];

/// Userspace runtime libraries standing in for the transitive closure: the real
/// sonames those plugins and `modv` link against in the container.
const RUNTIME_LIBS: [&str; 30] = [
    "libglib-2.0.so.0",
    "libgobject-2.0.so.0",
    "libgio-2.0.so.0",
    "libgstreamer-1.0.so.0",
    "libgstbase-1.0.so.0",
    "libgstvideo-1.0.so.0",
    "libgstaudio-1.0.so.0",
    "libgstpbutils-1.0.so.0",
    "libgsttag-1.0.so.0",
    "libgstmpegts-1.0.so.0",
    "libgstrtsp-1.0.so.0",
    "libgstnet-1.0.so.0",
    "libgstcodecparsers-1.0.so.0",
    "libgstallocators-1.0.so.0",
    "liborc-0.4.so.0",
    "libavcodec.so.61",
    "libavutil.so.59",
    "libavformat.so.61",
    "libpipewire-0.3.so.0",
    "libasound.so.2",
    "libv4l2.so.0",
    "libv4lconvert.so.0",
    "libdrm.so.2",
    "libwayland-client.so.0",
    "libatk-1.0.so.0",
    "libatspi.so.0",
    "libcairo.so.2",
    "libpango-1.0.so.0",
    "libharfbuzz.so.0",
    "libfontconfig.so.1",
];

/// The glibc family: never bundled, even though a real `lddtree` walk reports it.
const GLIBC_FAMILY: [&str; 6] = [
    "libc.so.6",
    "libm.so.6",
    "libdl.so.2",
    "libpthread.so.0",
    "librt.so.1",
    "ld-linux-x86-64.so.2",
];

/// Development-only decoys planted in the fake sysroot: they must never be
/// staged (they are most of what a distro -dev package installs).
const DEVELOPMENT_ONLY: [&str; 6] = [
    "libgstreamer-1.0.a",
    "libglib-2.0.a",
    "libgstvideo-1.0.la",
    "gstreamer-1.0.pc",
    "glib-2.0.pc",
    "libavcodec.pdb",
];

/// Variables `AppRun` must export before launching the app.
const LAUNCHER_VARS: [&str; 8] = [
    "GST_PLUGIN_PATH_1_0",
    "GST_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SCANNER_1_0",
    "GST_PLUGIN_SCANNER",
    "GST_REGISTRY",
    "LD_LIBRARY_PATH",
];

/// Flags `tools/vmodv-appimage.sh` must accept (see the module doc comment).
const DRY_RUN_FLAGS: [&str; 9] = [
    "--dry-run",
    "--project-root",
    "--appdir",
    "--binary",
    "--plugin-root",
    "--lib-root",
    "--scanner",
    "--doc-root",
    "--package-list",
];

/// Stand-in for the packaged binary/`log-run.sh`: records the launcher's
/// environment and exits successfully.
const PROBE_SCRIPT: &str = concat!(
    "#!/bin/sh\n",
    "# Test probe standing in for the packaged launch chain.\n",
    "if [ -n \"${APPIMAGE_TEST_ENV_FILE:-}\" ]; then\n",
    "    env > \"$APPIMAGE_TEST_ENV_FILE\"\n",
    "fi\n",
    "exit 0\n",
);

fn is_glibc_family(name: &str) -> bool {
    [
        "libc.so",
        "libm.so",
        "libdl.so",
        "libpthread.so",
        "librt.so",
        "ld-",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

fn is_development_only(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension, "a" | "la" | "pc" | "pdb" | "h" | "hpp"))
        .unwrap_or(false)
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", path.display()));
}

/// True when `script` exports `name` with an empty value: `export NAME=""`,
/// `export NAME=''`, a bare `export NAME=`, or a `NAME=""` assignment that a
/// separate `export NAME ...` line turns into an export.
fn exports_empty_value(script: &str, name: &str) -> bool {
    let prefix = format!("export {name}=");
    let empty_assignments = [
        format!("{name}=\"\""),
        format!("{name}=''"),
        format!("{name}="),
    ];
    let exported = script.lines().any(|line| {
        let line = line.trim();
        line.starts_with("export ") && line.split_whitespace().any(|word| word == name)
    });

    script.lines().any(|line| {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(prefix.as_str()) {
            let value = rest.trim();
            let quoted = value.len() >= 2
                && matches!(
                    (value.chars().next(), value.chars().last()),
                    (Some('"'), Some('"')) | (Some('\''), Some('\''))
                );
            return if quoted {
                value[1..value.len() - 1].is_empty()
            } else {
                value.is_empty()
            };
        }
        empty_assignments.iter().any(|form| line == form) && exported
    })
}

/// Parse the `NAME=value` dump written by [`PROBE_SCRIPT`]. An empty value
/// survives as an empty string; an unset variable simply has no entry.
fn parse_env_dump(path: &Path) -> BTreeMap<String, String> {
    let text = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("cannot read launcher env dump {}: {error}", path.display())
    });
    let mut dumped = BTreeMap::new();
    for line in text.lines() {
        if let Some((name, value)) = line.split_once('=')
            && !name.is_empty()
        {
            dumped.insert(name.to_string(), value.to_string());
        }
    }
    dumped
}

/// The fake toolchain input tree the `--dry-run` contract is driven with:
/// a sysroot-shaped directory holding plugins, runtime libraries, the plugin
/// scanner, doc/licence texts and a package list, plus the decoys that must be
/// filtered out.
struct FakeToolchain {
    /// Kept only for its `Drop` impl: it removes the fake sysroot and the
    /// staging AppDir, including when an assertion panics.
    #[expect(
        dead_code,
        reason = "the guard owns the cleanup; nothing needs to read the path"
    )]
    holder: TempDirGuard,
    binary: PathBuf,
    plugin_root: PathBuf,
    lib_root: PathBuf,
    scanner: PathBuf,
    doc_root: PathBuf,
    package_list: PathBuf,
    appdir: PathBuf,
}

impl FakeToolchain {
    fn new() -> Self {
        let holder = TempDirGuard::new(&unique_dir_name("vmodv-appimage-bundle"));
        let sysroot = holder.path().join("sysroot");
        let lib_root = sysroot.join("usr/lib/x86_64-linux-gnu");
        let plugin_root = lib_root.join("gstreamer-1.0");
        let doc_root = sysroot.join("usr/share/doc");
        // The distro keeps the scanner in a multiarch directory; staging must
        // flatten it to usr/libexec/gstreamer-1.0/.
        let scanner =
            sysroot.join("usr/lib/x86_64-linux-gnu/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner");
        let binary = sysroot.join("modv");
        let package_list = sysroot.join("packages.list");
        let appdir = holder.path().join("VmodV.AppDir");

        let scanner_dir = scanner
            .parent()
            .expect("the fake scanner has a parent directory")
            .to_path_buf();
        let directories = [
            lib_root.clone(),
            plugin_root.clone(),
            doc_root.clone(),
            scanner_dir,
            sysroot.join("usr/lib/x86_64-linux-gnu/pkgconfig"),
            sysroot.join("usr/include/gstreamer-1.0/gst"),
        ];
        for dir in &directories {
            fs::create_dir_all(dir)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", dir.display()));
        }

        fs::write(&binary, "fake modv binary stub\n").expect("write the fake modv binary");

        for name in PLUGINS {
            fs::write(
                plugin_root.join(name),
                format!("fake GStreamer plugin {name}\n"),
            )
            .unwrap_or_else(|error| panic!("cannot write plugin stub {name}: {error}"));
        }
        for name in RUNTIME_LIBS {
            fs::write(
                lib_root.join(name),
                format!("fake runtime library {name}\n"),
            )
            .unwrap_or_else(|error| panic!("cannot write runtime library {name}: {error}"));
        }
        for name in GLIBC_FAMILY {
            fs::write(lib_root.join(name), format!("fake glibc {name}\n"))
                .unwrap_or_else(|error| panic!("cannot write glibc stub {name}: {error}"));
        }

        // Development-only artifacts a distro -dev package drops next to the
        // runtime libraries: static libs, libtool archives, pkg-config files,
        // debug symbol files and headers.
        for name in DEVELOPMENT_ONLY {
            let target = if name.ends_with(".pc") {
                sysroot
                    .join("usr/lib/x86_64-linux-gnu/pkgconfig")
                    .join(name)
            } else {
                lib_root.join(name)
            };
            fs::write(&target, format!("development-only decoy {name}\n"))
                .unwrap_or_else(|error| panic!("cannot write decoy {}: {error}", target.display()));
        }
        fs::write(
            plugin_root.join("libgstcoreelements.a"),
            "development-only decoy\n",
        )
        .expect("write a plugin static-library decoy");
        fs::write(
            plugin_root.join("libgstvideoconvertscale.la"),
            "development-only decoy\n",
        )
        .expect("write a plugin libtool-archive decoy");
        fs::write(
            plugin_root.join("libgstpipewire.pdb"),
            "development-only decoy\n",
        )
        .expect("write a plugin debug-file decoy");
        fs::write(
            sysroot.join("usr/include/gstreamer-1.0/gst/gst.h"),
            "development-only header decoy\n",
        )
        .expect("write a header decoy");

        fs::write(&scanner, "fake gst-plugin-scanner\n")
            .expect("write the fake gst-plugin-scanner");
        fs::set_permissions(&scanner, fs::Permissions::from_mode(0o755))
            .expect("chmod the fake gst-plugin-scanner");

        // One licence text per package; the libav decoder plugin carries the
        // GPL text, the libraries it links against carry LGPL.
        let mut list = String::new();
        for (package, version) in PACKAGES {
            let licence = if package == "gstreamer1.0-libav" {
                "GNU GENERAL PUBLIC LICENSE\nVersion 2, June 1991\n"
            } else {
                "GNU LESSER GENERAL PUBLIC LICENSE\nVersion 2.1, February 1999\n"
            };
            let doc_dir = doc_root.join(package);
            fs::create_dir_all(&doc_dir)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", doc_dir.display()));
            fs::write(
                doc_dir.join("copyright"),
                format!("{package} {version}\n{licence}"),
            )
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", package));
            list.push_str(&format!("{package} {version}\n"));
        }
        fs::write(&package_list, list).expect("write the fake package list");

        Self {
            holder,
            binary,
            plugin_root,
            lib_root,
            scanner,
            doc_root,
            package_list,
            appdir,
        }
    }

    /// Invoke `tools/vmodv-appimage.sh --dry-run` with the fake inputs.
    ///
    /// The flag pre-assertion keeps a script that has no `--dry-run` mode from
    /// falling through into a real `cargo build` + appimagetool download.
    fn stage(&self) -> Output {
        let repo_root = repo_root();
        let script = repo_root.join("tools/vmodv-appimage.sh");
        assert!(
            script.is_file(),
            "tools/vmodv-appimage.sh is missing: the AppImage cannot bundle GStreamer"
        );
        let script_text = fs::read_to_string(&script)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", script.display()));
        for flag in DRY_RUN_FLAGS {
            assert!(
                script_text.contains(flag),
                "tools/vmodv-appimage.sh does not accept {flag}: it must gain a --dry-run \
                 mode that stages the fat AppDir from fake inputs (plugins from --plugin-root \
                 into usr/lib/gstreamer-1.0, the scanner into usr/libexec/gstreamer-1.0, the \
                 --lib-root closure into usr/lib, the manifest into \
                 usr/share/vmodv/gstreamer-bundle.json and --doc-root/<package>/copyright) \
                 without running cargo or appimagetool"
            );
        }

        Command::new("bash")
            .arg(&script)
            .arg("--dry-run")
            .arg("--project-root")
            .arg(&repo_root)
            .arg("--appdir")
            .arg(&self.appdir)
            .arg("--binary")
            .arg(&self.binary)
            .arg("--plugin-root")
            .arg(&self.plugin_root)
            .arg("--lib-root")
            .arg(&self.lib_root)
            .arg("--scanner")
            .arg(&self.scanner)
            .arg("--doc-root")
            .arg(&self.doc_root)
            .arg("--package-list")
            .arg(&self.package_list)
            .env("APP_VERSION", "0.0.0-test")
            .current_dir(&repo_root)
            .output()
            .expect("spawn bash tools/vmodv-appimage.sh --dry-run")
    }

    /// Run the dry run and require success, reporting the script's output.
    fn staged(&self) -> PathBuf {
        let output = self.stage();
        assert!(
            output.status.success(),
            "tools/vmodv-appimage.sh --dry-run failed with status {:?}\n\
             stdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            self.appdir.is_dir(),
            "tools/vmodv-appimage.sh --dry-run staged nothing: {} is not a directory",
            self.appdir.display()
        );
        self.appdir.clone()
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if self.appdir.is_dir() {
            collect_files(&self.appdir, &mut files);
        }
        files
    }
}

/// The AppImage staging script must expose the `--dry-run` contract before any
/// of the bundle behaviour can be verified.
#[test]
fn appimage_script_advertises_the_fat_bundle_dry_run_contract() {
    let script = repo_root().join("tools/vmodv-appimage.sh");
    assert!(
        script.is_file(),
        "tools/vmodv-appimage.sh is missing: the AppImage cannot bundle GStreamer"
    );
    let script_text = fs::read_to_string(&script)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", script.display()));

    for flag in DRY_RUN_FLAGS {
        assert!(
            script_text.contains(flag),
            "tools/vmodv-appimage.sh does not accept {flag}: the fat AppImage (bundling the \
             container's GStreamer runtime) is not implemented. See the module doc comment of \
             crates/modv-app/tests/appimage_bundle.rs for the exact flag contract."
        );
    }
}

/// The staged AppDir carries the GStreamer plugins, the plugin scanner, the
/// shared-library closure and the unchanged application payload.
#[test]
fn dry_run_bundles_the_gstreamer_runtime_into_the_appdir() {
    let toolchain = FakeToolchain::new();
    let appdir = toolchain.staged();
    let staged = toolchain.files();

    // The binary comes from --binary (dry run) and not from a cargo build.
    let staged_binary = appdir.join("usr/bin/modv");
    assert!(
        staged_binary.is_file(),
        "{} is not a file: --binary must be staged into usr/bin/modv",
        staged_binary.display()
    );
    let binary_text = fs::read_to_string(&staged_binary)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", staged_binary.display()));
    assert!(
        binary_text.contains("fake modv binary stub"),
        "{} was not staged from --binary: {binary_text}",
        staged_binary.display()
    );

    // Plugins, flattened into usr/lib/gstreamer-1.0 whatever the distro's
    // multiarch plugin directory is.
    let plugin_dir = appdir.join("usr/lib/gstreamer-1.0");
    assert!(
        plugin_dir.is_dir(),
        "{} is not a directory: the GStreamer plugins must be bundled under \
         usr/lib/gstreamer-1.0/ so the app can build its camera/file/screen pipelines",
        plugin_dir.display()
    );
    for name in PLUGINS {
        let plugin = plugin_dir.join(name);
        assert!(
            plugin.is_file(),
            "{} is not a file: the {name} plugin was not bundled",
            plugin.display()
        );
    }

    // The plugin scanner helper GStreamer execs while probing plugins.
    let scanner = appdir.join("usr/libexec/gstreamer-1.0/gst-plugin-scanner");
    assert!(
        scanner.is_file(),
        "{} is not a file: gst-plugin-scanner must be bundled under \
         usr/libexec/gstreamer-1.0/",
        scanner.display()
    );

    // The transitive userspace closure, and nothing dropped from it.
    for name in RUNTIME_LIBS {
        let bundled = staged
            .iter()
            .filter(|file| {
                file.file_name().and_then(|value| value.to_str()) == Some(name)
                    && file.starts_with(appdir.join("usr/lib"))
            })
            .count();
        assert!(
            bundled >= 1,
            "{name} was not bundled under {}/usr/lib: the GStreamer plugins' shared-library \
             closure (glib/gobject, gstreamer core + base/video/audio/pbutils/tag, orc, \
             libav/ffmpeg, pipewire, alsa, v4l2) must be copied so the host does not need \
             GStreamer packages installed",
            appdir.display()
        );
    }

    // Plugins belong in the plugin directory, libraries outside it.
    for name in RUNTIME_LIBS {
        assert!(
            !appdir.join("usr/lib/gstreamer-1.0").join(name).exists(),
            "{name} was staged into usr/lib/gstreamer-1.0: only plugins belong there"
        );
    }
    for name in PLUGINS {
        assert!(
            !appdir.join("usr/lib").join(name).exists(),
            "{name} was staged into usr/lib: plugins must be staged into usr/lib/gstreamer-1.0"
        );
    }

    // The unchanged application payload must survive the new staging.
    for relative in [
        "usr/bin/log-run.sh",
        "usr/share/vmodv/isf/Bloom.fs",
        "AppRun",
        "vmodv.desktop",
    ] {
        let path = appdir.join(relative);
        assert!(
            path.is_file(),
            "{} is not a file: the fat AppImage must keep the existing payload",
            path.display()
        );
    }
    let textures = appdir.join("usr/share/vmodv/textures");
    assert!(
        textures.is_dir()
            && fs::read_dir(&textures)
                .expect("read the staged textures directory")
                .filter_map(Result::ok)
                .any(|entry| entry.path().is_file()),
        "{} contains no bundled texture files",
        textures.display()
    );
}

/// The bundle is auditable: a machine-readable manifest of the staged distro
/// packages/versions, plus the licence texts those packages carry.
#[test]
fn dry_run_records_the_bundle_manifest_and_licence_texts() {
    let toolchain = FakeToolchain::new();
    let appdir = toolchain.staged();

    let manifest = appdir.join("usr/share/vmodv/gstreamer-bundle.json");
    assert!(
        manifest.is_file(),
        "{} is not a file: the AppImage must record which distro packages/versions were \
         bundled for auditability",
        manifest.display()
    );
    let manifest_text = fs::read_to_string(&manifest)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", manifest.display()));
    assert!(
        manifest_text.trim_start().starts_with('{') && manifest_text.contains("\"packages\""),
        "{} is not a machine-readable package manifest (expected a JSON object with a \
         \"packages\" array):\n{manifest_text}",
        manifest.display()
    );
    for (package, version) in PACKAGES {
        assert!(
            manifest_text.contains(package),
            "{} does not record the bundled package {package}:\n{manifest_text}",
            manifest.display()
        );
        assert!(
            manifest_text.contains(version),
            "{} does not record the version {version} of bundled package {package}:\n\
             {manifest_text}",
            manifest.display(),
        );
    }

    // Licence texts: libav/ffmpeg bits carry LGPL/GPL redistribution obligations,
    // and the staged text must be the distro's own text from --doc-root.
    for (package, _) in PACKAGES {
        let fixture = toolchain.doc_root.join(package).join("copyright");
        let fixture_text = fs::read_to_string(&fixture).unwrap_or_else(|error| {
            panic!(
                "cannot read the fixture licence {}: {error}",
                fixture.display()
            )
        });
        let copyright = appdir.join("usr/share/doc").join(package).join("copyright");
        assert!(
            copyright.is_file(),
            "{} is not a file: the licence text of bundled package {package} must be \
             bundled with the libraries it covers",
            copyright.display()
        );
        let staged_text = fs::read_to_string(&copyright)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", copyright.display()));
        assert!(
            staged_text.contains(fixture_text.trim_end()),
            "{} does not carry the licence text staged from {}: \n{staged_text}",
            copyright.display(),
            fixture.display(),
        );
    }
    let libav_copyright = appdir.join("usr/share/doc/gstreamer1.0-libav/copyright");
    assert!(
        libav_copyright.is_file(),
        "{} is not a file: the GPL text covering the bundled libav/ffmpeg decoder must be \
         bundled",
        libav_copyright.display()
    );
    let libav_text = fs::read_to_string(&libav_copyright)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", libav_copyright.display()));
    assert!(
        libav_text.contains("GNU GENERAL PUBLIC LICENSE"),
        "{} does not carry the GPL text for the bundled libav decoder:\n{libav_text}",
        libav_copyright.display()
    );
}

/// Filtering: development-only artifacts and the host's own glibc stay out.
#[test]
fn dry_run_never_stages_development_only_files_or_glibc() {
    let toolchain = FakeToolchain::new();
    let appdir = toolchain.staged();
    let staged = toolchain.files();

    assert!(
        !staged.is_empty(),
        "{} staged no files at all",
        appdir.display()
    );

    for name in DEVELOPMENT_ONLY {
        let leaked = staged
            .iter()
            .find(|file| file.file_name().and_then(|value| value.to_str()) == Some(name));
        assert!(
            leaked.is_none(),
            "development-only file was bundled into the AppImage: {} (staged from the fake \
             sysroot); stage runtime *.so/*.so.N files only",
            leaked.map_or_else(|| name.to_string(), |path| path.display().to_string())
        );
    }
    assert!(
        !appdir.join("usr/include").exists(),
        "{} exists: headers are development-only and must not be bundled",
        appdir.join("usr/include").display()
    );

    for path in &staged {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        assert!(
            !is_development_only(name),
            "development-only file was bundled into the AppImage: {}; exclude *.a, *.la, *.pc, \
             *.pdb and headers from the staged GStreamer trees",
            path.display()
        );
        assert!(
            !is_glibc_family(name),
            "glibc family member was bundled into the AppImage: {}; libc.so.6, libm.so.6, \
             libdl*, libpthread*, librt* and the dynamic loader must come from the host \
             (bundling them is how AppImages break on the user's machine)",
            path.display()
        );
    }
}

/// The launcher contract on the packaged `AppRun` file itself.
#[test]
fn apprun_exports_the_bundled_gstreamer_plugin_environment() {
    let apprun = repo_root().join("packaging/vmodv/AppRun");
    assert!(
        apprun.is_file(),
        "packaging/vmodv/AppRun is missing: the AppImage has no launcher"
    );
    let apprun_text = fs::read_to_string(&apprun)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", apprun.display()));

    for name in LAUNCHER_VARS {
        assert!(
            apprun_text.contains(name),
            "packaging/vmodv/AppRun does not export {name}: the bundled AppImage must point \
             GStreamer at its own plugins, scanner and libraries before launching modv:\n\
             {apprun_text}"
        );
    }
    for name in ["GST_PLUGIN_SYSTEM_PATH_1_0", "GST_PLUGIN_SYSTEM_PATH"] {
        assert!(
            exports_empty_value(&apprun_text, name),
            "packaging/vmodv/AppRun does not export {name} to an EMPTY value: unless the host \
             plugin search path is blanked, the host's GStreamer plugins are still probed and \
             clash with the bundled symbols:\n{apprun_text}"
        );
    }
    for token in ["usr/lib/gstreamer-1.0", "usr/libexec/gstreamer-1.0"] {
        assert!(
            apprun_text.contains(token),
            "packaging/vmodv/AppRun does not reference {token}: GST_PLUGIN_PATH and \
             GST_PLUGIN_SCANNER must point inside the bundle:\n{apprun_text}"
        );
    }
    // The asset paths and the logging launch chain stay as they are.
    for token in ["MODV_ISF_DIR", "MODV_ASSETS_DIR", "log-run.sh"] {
        assert!(
            apprun_text.contains(token),
            "packaging/vmodv/AppRun no longer mentions {token}"
        );
    }
}

/// Runtime half of the launcher contract: the exported values actually isolate
/// GStreamer inside the (read-only) AppImage payload.
#[test]
fn apprun_blanks_the_host_gstreamer_plugin_path_at_launch() {
    let repo_root = repo_root();
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-appimage-apprun"));
    let appdir = holder.path().join("VmodV.AppDir");
    let home = holder.path().join("home");

    // A payload-shaped AppDir so the launcher's paths resolve.
    for dir in [
        "usr/bin",
        "usr/lib/gstreamer-1.0",
        "usr/libexec/gstreamer-1.0",
        "usr/share/vmodv/isf",
        "usr/share/vmodv/textures",
    ] {
        fs::create_dir_all(appdir.join(dir)).unwrap_or_else(|error| {
            panic!("cannot create {}: {error}", appdir.join(dir).display())
        });
    }
    fs::create_dir_all(&home).expect("create the fake per-user home");
    write_executable(
        &appdir.join("usr/libexec/gstreamer-1.0/gst-plugin-scanner"),
        "#!/bin/sh\nexit 0\n",
    );
    // The probe stands in for both launch-chain members.
    write_executable(&appdir.join("usr/bin/log-run.sh"), PROBE_SCRIPT);
    write_executable(&appdir.join("usr/bin/modv"), PROBE_SCRIPT);

    let apprun = appdir.join("AppRun");
    fs::copy(repo_root.join("packaging/vmodv/AppRun"), &apprun)
        .expect("copy packaging/vmodv/AppRun into the fake AppDir");
    fs::set_permissions(&apprun, fs::Permissions::from_mode(0o755)).expect("chmod the fake AppRun");

    let env_file = holder.path().join("launch-environment.txt");
    // Pretend the host has its own GStreamer installed: exactly the situation
    // that makes a fat AppImage clash if the launcher does not blank it.
    let output = Command::new(&apprun)
        .env("HOME", &home)
        .env(
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
        )
        .env("GST_PLUGIN_SYSTEM_PATH", "/usr/lib/gstreamer-1.0")
        .env("APPIMAGE_TEST_ENV_FILE", &env_file)
        .current_dir(&appdir)
        .output()
        .expect("run the fake AppDir AppRun");

    assert!(
        output.status.success(),
        "AppRun exited with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        env_file.is_file(),
        "{} was not written: AppRun must launch the payload (usr/bin/log-run.sh / usr/bin/modv)",
        env_file.display()
    );
    let dumped = parse_env_dump(&env_file);

    for name in LAUNCHER_VARS {
        assert!(
            dumped.contains_key(name),
            "AppRun did not export {name}: the bundle is invisible to GStreamer. \
             Effective environment: {dumped:#?}"
        );
    }

    // The host plugin path must be blanked, not merely left alone.
    for name in ["GST_PLUGIN_SYSTEM_PATH_1_0", "GST_PLUGIN_SYSTEM_PATH"] {
        let value = dumped.get(name).map(String::as_str).unwrap_or("<unset>");
        assert!(
            value.is_empty(),
            "AppRun left {name}={value}: the host's GStreamer plugins leak into the bundle \
             and clash with the bundled symbols; AppRun must export it empty"
        );
    }

    for name in ["GST_PLUGIN_PATH_1_0", "GST_PLUGIN_PATH"] {
        let value = dumped.get(name).map(String::as_str).unwrap_or_default();
        assert!(
            value.contains("usr/lib/gstreamer-1.0"),
            "{name}={value} does not point at the bundled plugin directory usr/lib/gstreamer-1.0"
        );
    }
    for name in ["GST_PLUGIN_SCANNER_1_0", "GST_PLUGIN_SCANNER"] {
        let value = dumped.get(name).map(String::as_str).unwrap_or_default();
        assert!(
            value.contains("usr/libexec/gstreamer-1.0"),
            "{name}={value} does not point at the bundled gst-plugin-scanner in \
             usr/libexec/gstreamer-1.0"
        );
        assert!(
            Path::new(value).is_file(),
            "{name}={value} is not a bundled scanner file"
        );
    }

    // The payload is mounted read-only, so the registry must live per-user.
    let registry = dumped
        .get("GST_REGISTRY")
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        !registry.is_empty(),
        "AppRun exported GST_REGISTRY as empty: GStreamer writes its plugin registry at \
         startup and the AppImage payload is read-only, so it needs a writable per-user path"
    );
    let registry_path = Path::new(registry);
    assert!(
        !registry_path.starts_with(&appdir),
        "GST_REGISTRY={registry} points inside the read-only AppImage payload ({})",
        appdir.display()
    );
    let registry_dir = registry_path
        .parent()
        .unwrap_or_else(|| panic!("GST_REGISTRY={registry} has no parent directory"));
    fs::create_dir_all(registry_dir).unwrap_or_else(|error| {
        panic!(
            "cannot create GST_REGISTRY directory {}: {error}",
            registry_dir.display()
        )
    });
    let probe = registry_dir.join("appimage-bundle-registry-probe");
    fs::write(&probe, b"probe").unwrap_or_else(|error| {
        panic!(
            "GST_REGISTRY={registry} is not writable ({}: {error}); it must be a per-user path",
            registry_dir.display()
        )
    });
    let _ = fs::remove_file(&probe);

    // The bundled libraries must be what the loader sees first.
    let ld_library_path = dumped
        .get("LD_LIBRARY_PATH")
        .map(String::as_str)
        .unwrap_or_default();
    let first = ld_library_path.split(':').next().unwrap_or_default();
    let first = if first.is_empty() {
        ld_library_path
    } else {
        first
    };
    let bundled_lib_dir = appdir.join("usr/lib");
    let canonical_first = fs::canonicalize(first)
        .unwrap_or_else(|error| panic!("LD_LIBRARY_PATH={first} cannot be resolved: {error}"));
    assert!(
        canonical_first.starts_with(fs::canonicalize(&bundled_lib_dir).unwrap_or_else(
            |error| panic!("cannot resolve {}: {error}", bundled_lib_dir.display())
        )),
        "LD_LIBRARY_PATH={ld_library_path} is not prefixed with the bundled {} : the staged \
         closure would never be loaded",
        bundled_lib_dir.display()
    );
}
