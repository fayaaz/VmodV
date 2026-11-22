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
//! * never    -> `*.a`, `*.la`, `*.pc`, `*.pdb`, headers, and the glibc family:
//!   `libc.so.6`, `libm.so.6`, `libdl*`, `libpthread*`, `librt*`, the dynamic
//!   loader *and* the rest of the libc6-owned helpers a real `lddtree` walk
//!   reports (`libresolv`, `libmvec`, `libnsl`, `libutil`, `libcrypt`, `libanl`,
//!   `libthread_db`)
//! * canonicalised inputs: an input root handed over as a symlink stages like
//!   the real directory it points at
//! * RUNPATHs: every staged ELF gets an `$ORIGIN`-relative RUNPATH through
//!   `patchelf --set-rpath <rpath> <file>` (`patchelf` has no `--` separator), a
//!   non-zero patchelf status is fatal, and the script only uses `realpath`
//!   options the build container's coreutils actually has (`-f` is not one, and
//!   it killed the first real `make appimage-vmodv` run)
//!
//! `--dry-run` and the real build share one staging pipeline: the same
//! `collect_plugins` / staging / licence / RUNPATH code runs in both modes, and
//! the real-mode extras (the `cargo build`, the `lddtree` closure walk, the
//! `dpkg` package list and `appimagetool`) are skipped when the input they would
//! produce is already supplied. So a dry run exercises the code that ships.
//!
//! ## The bundle-check contract (canonical build + CI)
//!
//! CI already BUILDS the fat AppImage with `make build` in both
//! `.github/workflows/vmodv.yml` and `.github/workflows/release.yml`; it must
//! also VERIFY it, and the same gate applies to the canonical local build. The
//! gate is a standalone `tools/vmodv-appimage-check.sh`, exposed as
//! `make vmodv-appimage-check`, invoked by `tools/vmodv-appimage.sh` on the
//! artifact it just produced (so `make build` fails on a thin bundle), by the
//! vmodv.yml test job
//! (`make engine-test ARGS="-p modv-app --test appimage_bundle -- --test-threads=1"`)
//! and package job, and by the release.yml publish job before publishing. The
//! `.deb` packaging (`tools/vmodv-deb.sh`, `packaging/vmodv/vmodv.control`,
//! `packaging/vmodv/deb-launcher.sh`, `tools/vmodv-deb-check.sh`) and
//! `packaging/vmodv/AppRun` / `packaging/vmodv/log-run.sh` stay UNCHANGED.
//!
//! CLI: `bash tools/vmodv-appimage-check.sh <path>` — exactly one positional
//! argument:
//!
//! * a directory: an already-assembled AppDir, checked in place;
//! * a `*.AppImage` file: extracted with `--appimage-extract` (no FUSE needed)
//!   into a temporary directory, then checked exactly like an AppDir.
//!
//! Content assertions (each exits non-zero on the FIRST failure, naming the
//! missing behaviour):
//!
//! * `usr/bin/modv` and `AppRun` are present and executable;
//! * at least **100** plugin `*.so` files under `usr/lib/gstreamer-1.0/` (this
//!   exact floor is the documented contract);
//! * `usr/libexec/gstreamer-1.0/gst-plugin-scanner` is present;
//! * `usr/lib/` holds `libgstreamer-1.0.so*` plus at least one of
//!   `libgstbase-1.0.so*`, `libgstvideo-1.0.so*`, `libgstaudio-1.0.so*`,
//!   `libgstpbutils-1.0.so*`;
//! * `usr/share/vmodv/gstreamer-bundle.json` is present, JSON-parseable and
//!   holds at least one package/version entry;
//! * at least one `usr/share/doc/*/copyright` licence text is present;
//! * zero glibc-family files: `libc.so*`, `libm.so*`, `libdl*`, `libpthread*`,
//!   `librt*`, `ld-linux*`, `libresolv*`, `libmvec*`, `libnsl*`, `libutil*`,
//!   `libcrypt.so.1*`, `libanl*`, `libthread_db*` — plus no `libc6` licence text;
//! * zero host display-stack files: `libGL.so*`, `libGLX*`, `libGLdispatch*`,
//!   `libOpenGL*`, `libEGL*`, `libGLES*`, `libglapi*`, `libgbm*`,
//!   `libvulkan*`, `libwayland*`;
//! * zero development-only artifacts: `*.a`, `*.la`, `*.pc`, `*.pdb`;
//! * `readelf -d` shows an `$ORIGIN`-relative RUNPATH (never an absolute path,
//!   never missing — fail closed) on `usr/bin/modv`, the scanner, a sample of
//!   `usr/lib/*.so*` and a sample of the plugin `*.so`s. Error text names the
//!   offending path and `RUNPATH`.
//!
//! A readable size/content summary is printed to stdout: the AppImage byte
//! size for an image input, payload byte totals and counts otherwise, with
//! lines containing `plugins:`, `libraries:`, `licences:` and `bytes:`. NO size
//! budget is asserted.
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
use std::os::unix::fs::symlink;
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
const PACKAGES: [(&str, &str); 26] = [
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
    // No libwayland-client0 entry: the Wayland client stack is host-side, see
    // [`HOST_DISPLAY_LIBRARIES`].
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
const RUNTIME_LIBS: [&str; 29] = [
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
    // `libwayland-*` would be here in a raw `lddtree` walk: it is deliberately
    // absent because it is host display plumbing, see [`HOST_DISPLAY_LIBRARIES`].
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

/// The rest of the libc6-owned runtime library family a real `lddtree` walk
/// reports next to `libc.so.6`. They belong to the host's glibc exactly like
/// `libc.so.6`/`libm.so.6` do. The first fat AppImage build leaked
/// `libresolv.so.2` and `libmvec.so.1` into `usr/lib` because the filter only
/// knew the six names in [`GLIBC_FAMILY`].
const LIBC6_OWNED_LIBRARIES: [&str; 7] = [
    "libresolv.so.2",
    "libmvec.so.1",
    "libnsl.so.1",
    "libutil.so.1",
    "libcrypt.so.1",
    "libanl.so.1",
    "libthread_db.so.1",
];

/// The host's own Wayland and GLVND dispatch stack. A real `lddtree` walk pulls
/// them in through `libgstwayland-1.0`, `libgstgl-1.0`, `libgdk-3` and
/// `libSDL2-2.0` (the GLVND pair arrive under `libGL.so.1`, which is excluded, so
/// nothing bundled even needs them), but they must stay host-side like
/// EGL/vulkan/GBM: `AppRun` puts `usr/lib` *ahead* of the system paths, so a
/// staged copy shadows the user's — for the *host's* Mesa vulkan ICDs too, which
/// link libwayland-client themselves and then fail to load with
/// `undefined symbol: wl_fixes_interface`, leaving wgpu with no adapter at all.
const HOST_DISPLAY_LIBRARIES: [&str; 7] = [
    "libwayland-client.so.0",
    "libwayland-cursor.so.0",
    "libwayland-egl.so.1",
    "libwayland-server.so.0",
    "libGLX.so.0",
    "libGLdispatch.so.0",
    "libOpenGL.so.0",
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

/// Every library that belongs to the host's glibc: the classic six plus the
/// libc6-owned helpers in [`LIBC6_OWNED_LIBRARIES`].
fn is_glibc_family(name: &str) -> bool {
    [
        "libc.so",
        "libm.so",
        "libdl.so",
        "libpthread.so",
        "librt.so",
        "ld-",
        "libresolv.so",
        "libmvec.so",
        "libnsl.so",
        "libutil.so",
        "libcrypt.so",
        "libanl.so",
        "libthread_db.so",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

/// Content that satisfies the staging script's ELF magic check without being a
/// loadable ELF: enough to make the script's ELF detection (and therefore its
/// RUNPATH patching) engage in a dry run, where no real ELF exists.
fn elf_stub_contents(name: &str) -> String {
    format!("\u{7f}ELF\u{2}\u{1}\u{1}\u{0} fake ELF stub {name}\n")
}

/// A `patchelf` stand-in. It records each invocation as a `# <argument count>`
/// line followed by that many argument lines, then exits with
/// `PATCH_RPATH_STATUS`. That lets a test assert both the argument shape
/// (`patchelf` has no `--` separator: it takes `--` as the file name and
/// applies no RUNPATH) and that a non-zero status is fatal for the build.
const PATCHELF_SHIM: &str = concat!(
    "#!/bin/sh\n",
    "# Records its arguments, then exits with PATCH_RPATH_STATUS.\n",
    "if [ -n \"${PATCH_RPATH_LOG:-}\" ]; then\n",
    "    {\n",
    "        printf '# %s\\n' \"$#\"\n",
    "        for argument in \"$@\"; do printf '%s\\n' \"$argument\"; done\n",
    "    } >> \"$PATCH_RPATH_LOG\"\n",
    "fi\n",
    "exit \"${PATCH_RPATH_STATUS:-0}\"\n",
);

/// Parse the invocation log written by [`PATCHELF_SHIM`]: one `# <count>` line
/// per invocation, followed by `<count>` argument lines.
fn parse_patchelf_log(path: &Path) -> Vec<Vec<String>> {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read patchelf log {}: {error}", path.display()));
    let mut invocations: Vec<Vec<String>> = Vec::new();
    let mut declared_counts: Vec<usize> = Vec::new();
    for line in text.lines() {
        if let Some(count) = line.strip_prefix("# ") {
            let count = count
                .trim()
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("bad patchelf argument count {line:?}: {error}"));
            declared_counts.push(count);
            invocations.push(Vec::new());
        } else if let Some(arguments) = invocations.last_mut() {
            arguments.push(line.to_string());
        }
    }
    for (arguments, count) in invocations.iter().zip(&declared_counts) {
        assert_eq!(
            arguments.len(),
            *count,
            "the patchelf shim logged {arguments:?} for an invocation it reported as \
             taking {count} arguments"
        );
    }
    invocations
}

/// The `--set-rpath <rpath>` a recorded `patchelf` invocation applied to the
/// bundle path ending in `suffix`, if any.
fn recorded_rpath(invocations: &[Vec<String>], suffix: &str) -> Option<String> {
    invocations.iter().find_map(|arguments| {
        arguments
            .get(2)
            .filter(|file| file.ends_with(suffix))
            .map(|_| arguments[1].clone())
    })
}

/// Install [`PATCHELF_SHIM`] as `patchelf` inside `shim_dir` and return the file
/// it records invocations to.
fn install_patchelf_shim(shim_dir: &Path) -> PathBuf {
    fs::create_dir_all(shim_dir)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", shim_dir.display()));
    write_executable(&shim_dir.join("patchelf"), PATCHELF_SHIM);
    shim_dir.join("patchelf-invocations.txt")
}

/// `PATH` with `shim_dir` in front, so the staging script resolves `patchelf`
/// to the shim while keeping every other tool it needs.
fn shimmed_path(shim_dir: &Path) -> String {
    let inherited = std::env::var_os("PATH")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{}:{inherited}", shim_dir.display())
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
    /// Owns the fake sysroot and the staging AppDir: its `Drop` removes both,
    /// including when an assertion panics. Tests that need a scratch directory
    /// alongside the fake sysroot create it under `holder`.
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
        // The libc6-owned helpers `lddtree` reports alongside them: they are the
        // host glibc's too, and the first fat build staged two of them.
        for name in LIBC6_OWNED_LIBRARIES {
            fs::write(lib_root.join(name), format!("fake libc6 library {name}\n"))
                .unwrap_or_else(|error| panic!("cannot write libc6 stub {name}: {error}"));
        }

        // The host Wayland client stack, as a real closure reports it.
        for name in HOST_DISPLAY_LIBRARIES {
            fs::write(
                lib_root.join(name),
                format!("fake host wayland library {name}\n"),
            )
            .unwrap_or_else(|error| panic!("cannot write host display stub {name}: {error}"));
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

    /// The `tools/vmodv-appimage.sh --dry-run` invocation for these fake inputs.
    ///
    /// The flag pre-assertion keeps a script that has no `--dry-run` mode from
    /// falling through into a real `cargo build` + appimagetool download. A test
    /// that needs something more (a `patchelf` shim on `PATH`, an input root
    /// handed over as a symlink) configures the returned [`Command`] through
    /// [`FakeToolchain::stage_with`]: the script's argument parser lets a
    /// repeated flag override the earlier value.
    fn command(&self) -> Command {
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

        let mut command = Command::new("bash");
        command
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
            .current_dir(&repo_root);
        command
    }

    /// Invoke the dry run with the fake inputs, optionally adding environment or
    /// overriding a flag (the script's parser lets a repeated flag win).
    fn stage_with(&self, configure: impl FnOnce(&mut Command)) -> Output {
        let mut command = self.command();
        configure(&mut command);
        command
            .output()
            .expect("spawn bash tools/vmodv-appimage.sh --dry-run")
    }

    /// Run a dry run configured by `configure` and require success, reporting the
    /// script's output.
    fn staged_with(&self, configure: impl FnOnce(&mut Command)) -> PathBuf {
        let output = self.stage_with(configure);
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

    /// Run the dry run and require success, reporting the script's output.
    fn staged(&self) -> PathBuf {
        self.staged_with(|_| {})
    }

    /// Rewrite the fake sysroot's binary, plugins, libraries and scanner with the
    /// ELF magic bytes the staging script looks for, so its ELF detection — and
    /// therefore its RUNPATH patching — engages in a dry run too. Licence texts
    /// and the package list stay plain text.
    fn stubs_as_elfs(&self) {
        let mut targets = vec![self.binary.clone(), self.scanner.clone()];
        targets.extend(PLUGINS.iter().map(|name| self.plugin_root.join(name)));
        targets.extend(RUNTIME_LIBS.iter().map(|name| self.lib_root.join(name)));
        for path in &targets {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            fs::write(path, elf_stub_contents(name)).unwrap_or_else(|error| {
                panic!("cannot rewrite {} as an ELF stub: {error}", path.display())
            });
        }
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

/// Container-tool compatibility guard. `realpath` in the build container is GNU
/// coreutils': it canonicalises by default and has **no `-f`**. An unsupported
/// flag exits 1, and under `set -euo pipefail` that aborted the whole real
/// `make appimage-vmodv` build at `tools/vmodv-appimage.sh:216` — right after a
/// 3-minute `cargo build`, with no artifact and no bundle to inspect.
#[test]
fn appimage_script_only_uses_realpath_options_the_build_container_has() {
    let script = repo_root().join("tools/vmodv-appimage.sh");
    let script_text = fs::read_to_string(&script)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", script.display()));
    let unsupported = |token: &str| {
        token == "-f"
            || (token.starts_with("--canonicalize")
                && token != "--canonicalize-existing"
                && token != "--canonicalize-missing")
    };
    for line in script_text.lines() {
        // Only the code half of a line matters: the header explains the pitfall.
        let code = line.split('#').next().unwrap_or_default();
        if !code.contains("realpath") {
            continue;
        }
        let offenders: Vec<&str> = code
            .split_whitespace()
            .filter(|token| unsupported(token))
            .collect();
        assert!(
            offenders.is_empty(),
            "tools/vmodv-appimage.sh calls realpath with {offenders:?}:\n  {}\nThe build \
             container's coreutils `realpath` already canonicalises and has no `-f`, so the \
             call exits 1 and `set -euo pipefail` aborts the AppImage build. Canonicalise with \
             a plain `realpath -- <path>`.",
            code.trim_end(),
        );
    }
}

/// Input roots are canonicalised by the shared staging path, so a distro layout
/// that puts a GStreamer tree behind a symlink stages exactly like the real one
/// (`find` does not descend into a symlink it is given as a starting point).
#[test]
fn dry_run_stages_from_symlinked_input_roots() {
    let toolchain = FakeToolchain::new();
    let links = toolchain.holder.path().join("symlinked-inputs");
    fs::create_dir_all(&links)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", links.display()));
    let plugin_link = links.join("gstreamer-1.0");
    let lib_link = links.join("lib");
    symlink(&toolchain.plugin_root, &plugin_link)
        .unwrap_or_else(|error| panic!("cannot symlink {}: {error}", plugin_link.display()));
    symlink(&toolchain.lib_root, &lib_link)
        .unwrap_or_else(|error| panic!("cannot symlink {}: {error}", lib_link.display()));

    let appdir = toolchain.staged_with(|command| {
        command.arg("--plugin-root").arg(&plugin_link);
        command.arg("--lib-root").arg(&lib_link);
    });

    for name in PLUGINS {
        let plugin = appdir.join("usr/lib/gstreamer-1.0").join(name);
        assert!(
            plugin.is_file(),
            "{} is not a file: --plugin-root was handed over as the symlink {} and staging \
             must canonicalise its input roots (find does not descend into a symlink given as \
             a starting point), exactly as it must for the distro's real plugin tree",
            plugin.display(),
            plugin_link.display(),
        );
    }
    for name in RUNTIME_LIBS {
        let library = appdir.join("usr/lib").join(name);
        assert!(
            library.is_file(),
            "{} is not a file: --lib-root was handed over as the symlink {}",
            library.display(),
            lib_link.display(),
        );
    }
}

/// The libc6-owned helpers a real `lddtree` walk reports next to `libc.so.6` are
/// host glibc too. The first fat AppImage staged `libresolv.so.2` and
/// `libmvec.so.1` into `usr/lib` because the filter only knew the classic six.
#[test]
fn dry_run_never_stages_libc6_owned_runtime_libraries() {
    let toolchain = FakeToolchain::new();
    for name in LIBC6_OWNED_LIBRARIES {
        assert!(
            toolchain.lib_root.join(name).is_file(),
            "the fake sysroot does not plant {name}: the filter gap this test guards would \
             not be exercised",
        );
    }
    let appdir = toolchain.staged();
    for path in toolchain.files() {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let owned_by_libc6 = LIBC6_OWNED_LIBRARIES.contains(&name);
        assert!(
            !owned_by_libc6,
            "libc6-owned library {} was bundled into {}: libresolv, libmvec, libnsl, libutil, \
             libcrypt, libanl and libthread_db belong to the host glibc exactly like libc.so.6 \
             and libm.so.6 do, and shipping them next to a host libc.so.6 is how an AppImage \
             breaks on the user's machine",
            path.display(),
            appdir.display(),
        );
    }
}

/// `AppRun` puts `usr/lib` *ahead* of the system paths, so a staged copy of the
/// build container's Wayland and GLVND dispatch libraries shadows the user's —
/// for the *host's* Mesa vulkan ICDs too. Those are built against a newer
/// libwayland and then fail to load with `undefined symbol:
/// wl_fixes_interface`, which leaves wgpu reporting `no compatible GPU adapter
/// found` on a machine whose driver is perfectly good (exactly what the first fat
/// AppImage did on the reference host). The host display stack is host-side
/// plumbing like EGL/vulkan/GBM: the plugins that use it still ship and resolve
/// it from the user's machine.
#[test]
fn dry_run_leaves_the_host_display_stack_host_side() {
    let toolchain = FakeToolchain::new();
    let plugin = "libgstwayland-1.0.so";
    fs::write(
        toolchain.plugin_root.join(plugin),
        format!("fake GStreamer plugin {plugin}\n"),
    )
    .expect("write the fake GStreamer Wayland plugin");

    let appdir = toolchain.staged();

    for name in HOST_DISPLAY_LIBRARIES {
        assert!(
            toolchain.lib_root.join(name).is_file(),
            "the fake sysroot does not plant {name}: a closure that reports the host Wayland \
             stack is precisely the case this test guards",
        );
    }
    for path in toolchain.files() {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        assert!(
            !HOST_DISPLAY_LIBRARIES.contains(&name),
            "host display library {} was bundled into {}: staging the container's libwayland-* \
             or GLVND dispatch libraries shadows the user's copies through AppRun's \
             LD_LIBRARY_PATH, and the host's Mesa vulkan ICDs then fail with `undefined symbol: \
             wl_fixes_interface`, leaving the app with no GPU adapter at all",
            path.display(),
            appdir.display(),
        );
    }

    // Over-exclusion guard: the plugin that needs Wayland still ships.
    let staged_plugin = appdir.join("usr/lib/gstreamer-1.0").join(plugin);
    assert!(
        staged_plugin.is_file(),
        "{} is not a file: excluding the host Wayland stack must not drop the GStreamer Wayland \
         plugin, which resolves libwayland from the user's machine",
        staged_plugin.display()
    );
}

/// RUNPATHs must actually be applied. `patchelf` has no `--` end-of-options
/// separator: `patchelf --set-rpath <rpath> -- <file>` treats `--` as the input
/// file, errors out, and leaves the bundle with the distro's absolute library
/// paths — which the first fat build did for all 640 of its ELFs, invisibly,
/// because the image still ran against the host's own GStreamer.
#[test]
fn dry_run_applies_origin_relative_runpaths_to_every_bundled_elf() {
    let toolchain = FakeToolchain::new();
    toolchain.stubs_as_elfs();
    let shim_dir = toolchain.holder.path().join("shim-bin");
    let log = install_patchelf_shim(&shim_dir);

    let appdir = toolchain.staged_with(|command| {
        command.env("PATH", shimmed_path(&shim_dir));
        command.env("PATCH_RPATH_LOG", &log);
        command.env("PATCH_RPATH_STATUS", "0");
    });

    assert!(
        log.is_file(),
        "{} was never written: the dry run staged ELF stubs without ever calling patchelf, so \
         the bundle's RUNPATHs would keep the distro paths and the shared staging path would \
         never be exercised",
        log.display(),
    );
    let invocations = parse_patchelf_log(&log);
    let expected = 1 + PLUGINS.len() + RUNTIME_LIBS.len() + 1;
    assert_eq!(
        invocations.len(),
        expected,
        "the bundle patched {} ELFs, expected {expected}: the staged binary, the plugin \
         scanner, every library under usr/lib and every plugin under usr/lib/gstreamer-1.0 \
         must get an $ORIGIN-relative RUNPATH ({} was staged into {})",
        invocations.len(),
        toolchain.files().len(),
        appdir.display(),
    );
    for arguments in &invocations {
        assert_eq!(
            arguments.len(),
            3,
            "patchelf was called as {arguments:?}: it takes `--set-rpath <rpath> <file>` and has \
             no `--` end-of-options separator, so with `--` it treats the separator as the file \
             name, fails, and applies no RUNPATH at all"
        );
        assert_eq!(
            arguments[0], "--set-rpath",
            "patchelf was called as {arguments:?}: the bundle's RUNPATHs are set with \
             `--set-rpath <rpath> <file>`",
        );
        assert!(
            arguments[1].starts_with("$ORIGIN"),
            "patchelf was called as {arguments:?}: a bundled ELF must resolve its neighbours \
             inside the read-only image, so every RUNPATH is $ORIGIN-relative",
        );
        assert!(
            Path::new(&arguments[2]).starts_with(&appdir),
            "patchelf was called on {} outside the staged AppDir {}",
            arguments[2],
            appdir.display(),
        );
    }
    for (bundled, rpath) in [
        ("usr/bin/modv", "$ORIGIN/../lib"),
        (
            "usr/libexec/gstreamer-1.0/gst-plugin-scanner",
            "$ORIGIN/../../lib",
        ),
        ("usr/lib/libgstreamer-1.0.so.0", "$ORIGIN"),
        ("usr/lib/libavcodec.so.61", "$ORIGIN"),
        ("usr/lib/gstreamer-1.0/libgstcoreelements.so", "$ORIGIN/.."),
        ("usr/lib/gstreamer-1.0/libgstlibav.so", "$ORIGIN/.."),
    ] {
        let applied = recorded_rpath(&invocations, bundled);
        assert_eq!(
            applied.as_deref(),
            Some(rpath),
            "no patchelf invocation applied {rpath} to the staged {bundled}: the bundle's ELFs \
             keep the distro's absolute library paths and resolve against the host instead of \
             each other inside the image",
        );
    }
}

/// ... and a failing `patchelf` must stop the build instead of being logged as a
/// warning: a bundle with unpatched RUNPATHs still runs on the build host, so a
/// warning is how the defect stays invisible until a user's machine differs.
#[test]
fn dry_run_aborts_when_patchelf_reports_failure() {
    let toolchain = FakeToolchain::new();
    toolchain.stubs_as_elfs();
    let shim_dir = toolchain.holder.path().join("shim-bin");
    let log = install_patchelf_shim(&shim_dir);

    let output = toolchain.stage_with(|command| {
        command.env("PATH", shimmed_path(&shim_dir));
        command.env("PATCH_RPATH_LOG", &log);
        command.env("PATCH_RPATH_STATUS", "1");
    });
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        log.is_file(),
        "{} was never written: the dry run never called patchelf, so a patchelf failure could \
         not be observed at all and the bundle's RUNPATHs are unverified",
        log.display(),
    );
    assert!(
        !output.status.success(),
        "tools/vmodv-appimage.sh exited {} although patchelf reported failure for the staged \
         bundle: a bundle whose RUNPATHs were not applied silently resolves libraries against \
         the host instead of inside the read-only image, which is invisible in the build \
         container. Treat a non-zero patchelf status as fatal.\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status,
    );
    assert!(
        stderr.contains("patchelf"),
        "tools/vmodv-appimage.sh failed without naming patchelf, so the RUNPATH failure is not \
         diagnosable from the build log:\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
}

// ---------------------------------------------------------------------------
// The bundle-check gate: tools/vmodv-appimage-check.sh (slug appimage-ci-check).
// ---------------------------------------------------------------------------

/// The exact plugin `*.so` count floor the checker enforces under
/// `usr/lib/gstreamer-1.0/`. Documented in the module comment so GREEN
/// implements exactly this contract.
const CHECKER_MIN_PLUGINS: usize = 100;

/// The standalone bundle checker the canonical build and both workflows must
/// run on the fat AppImage / AppDir.
const CHECKER_SCRIPT: &str = "tools/vmodv-appimage-check.sh";

fn checker_path() -> PathBuf {
    repo_root().join(CHECKER_SCRIPT)
}

/// Invoke the checker with a single positional `<path>` argument (an AppDir or
/// a built `.AppImage`), exactly as the Makefile and the workflows will.
fn run_checker(target: &Path) -> Output {
    Command::new("bash")
        .arg(checker_path())
        .arg(target)
        .current_dir(repo_root())
        .output()
        .expect("spawn bash tools/vmodv-appimage-check.sh")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

/// Compile a tiny real ELF in `dir` with gcc/patchelf (both in the toolchain
/// image) so `readelf -d` produces genuine RUNPATH evidence. `rpath = None`
/// leaves the ELF with no RUNPATH at all — the "broken RUNPATH evidence" case.
fn compile_runpath_elf(
    dir: &Path,
    name: &str,
    source: &str,
    shared: bool,
    rpath: Option<&str>,
) -> PathBuf {
    fs::create_dir_all(dir)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", dir.display()));
    let source_path = dir.join(format!("{name}.c"));
    fs::write(&source_path, source)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", source_path.display()));
    let binary = dir.join(name);
    let mut compiler = Command::new("gcc");
    compiler.arg("-o").arg(&binary);
    if shared {
        compiler.arg("-shared").arg("-fPIC");
    }
    compiler.arg(&source_path);
    let built = compiler.output().expect("run gcc for the checker fixture");
    assert!(
        built.status.success(),
        "gcc failed building the checker's ELF fixture {}:\n{}",
        binary.display(),
        output_text(&built)
    );
    if let Some(rpath) = rpath {
        let patched = Command::new("patchelf")
            .arg("--set-rpath")
            .arg(rpath)
            .arg(&binary)
            .output()
            .expect("run patchelf for the checker fixture");
        assert!(
            patched.status.success(),
            "patchelf failed setting {rpath} on {}:\n{}",
            binary.display(),
            output_text(&patched)
        );
    }
    binary
}

/// Recursive directory copy (std-only) preserving the executable bits the
/// checker asserts. Negative cases run on copies of one staged good bundle.
fn copy_dir_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", dst.display()));
    for entry in
        fs::read_dir(src).unwrap_or_else(|error| panic!("cannot read {}: {error}", src.display()))
    {
        let entry = entry
            .unwrap_or_else(|error| panic!("cannot read an entry in {}: {error}", src.display()));
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)
            .unwrap_or_else(|error| panic!("cannot stat {}: {error}", from.display()));
        if metadata.is_dir() {
            copy_dir_tree(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap_or_else(|error| {
                panic!(
                    "cannot copy {} to {}: {error}",
                    from.display(),
                    to.display()
                )
            });
            fs::set_permissions(
                &to,
                fs::Permissions::from_mode(metadata.permissions().mode()),
            )
            .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", to.display()));
        }
    }
}

/// Stage a bundle the checker must ACCEPT: the normal dry-run AppDir (manifest,
/// licences, AppRun, payload) with its text stubs replaced by real ELFs carrying
/// `$ORIGIN`-relative RUNPATHs, and enough plugins to clear the documented
/// floor of 100.
fn stage_checked_bundle() -> FakeToolchain {
    let toolchain = FakeToolchain::new();
    let appdir = toolchain.staged();
    let elf_work = toolchain.holder.path().join("checker-elf-work");

    let lib_elf = compile_runpath_elf(
        &elf_work,
        "lib-runtime-elf.so",
        "int runtime_stub(void) { return 0; }\n",
        true,
        Some("$ORIGIN"),
    );
    let plugin_elf = compile_runpath_elf(
        &elf_work,
        "lib-plugin-elf.so",
        "int plugin_stub(void) { return 0; }\n",
        true,
        Some("$ORIGIN/.."),
    );
    let binary_elf = compile_runpath_elf(
        &elf_work,
        "modv-elf",
        "int main(void) { return 0; }\n",
        false,
        Some("$ORIGIN/../lib"),
    );
    let scanner_elf = compile_runpath_elf(
        &elf_work,
        "scanner-elf",
        "int main(void) { return 0; }\n",
        false,
        Some("$ORIGIN/../../lib"),
    );

    let lib_dir = appdir.join("usr/lib");
    for entry in fs::read_dir(&lib_dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", lib_dir.display()))
    {
        let path = entry
            .unwrap_or_else(|error| {
                panic!("cannot read an entry in {}: {error}", lib_dir.display())
            })
            .path();
        if path.is_file() {
            fs::copy(&lib_elf, &path)
                .unwrap_or_else(|error| panic!("cannot overwrite {}: {error}", path.display()));
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", path.display()));
        }
    }

    let plugin_dir = appdir.join("usr/lib/gstreamer-1.0");
    let mut plugin_count = 0usize;
    for entry in fs::read_dir(&plugin_dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", plugin_dir.display()))
    {
        let path = entry
            .unwrap_or_else(|error| {
                panic!("cannot read an entry in {}: {error}", plugin_dir.display())
            })
            .path();
        if path.is_file() {
            fs::copy(&plugin_elf, &path)
                .unwrap_or_else(|error| panic!("cannot overwrite {}: {error}", path.display()));
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", path.display()));
            plugin_count += 1;
        }
    }
    for index in plugin_count..(CHECKER_MIN_PLUGINS + 5) {
        let path = plugin_dir.join(format!("libgstbundled{index:03}.so"));
        fs::copy(&plugin_elf, &path)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", path.display()));
    }

    let binary = appdir.join("usr/bin/modv");
    fs::copy(&binary_elf, &binary)
        .unwrap_or_else(|error| panic!("cannot overwrite {}: {error}", binary.display()));
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", binary.display()));
    let scanner = appdir.join("usr/libexec/gstreamer-1.0/gst-plugin-scanner");
    fs::copy(&scanner_elf, &scanner)
        .unwrap_or_else(|error| panic!("cannot overwrite {}: {error}", scanner.display()));
    fs::set_permissions(&scanner, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", scanner.display()));

    toolchain
}

/// Run the checker on a broken copy and require a non-zero exit that NAMES the
/// missing behaviour (the first failed assertion), not a silent failure.
fn assert_checker_rejects(appdir: &Path, expected: &str, label: &str) {
    let output = run_checker(appdir);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "tools/vmodv-appimage-check.sh ACCEPTED {label}: the bundle gate does not gate.\n{text}"
    );
    assert!(
        text.contains(expected),
        "tools/vmodv-appimage-check.sh rejected {label} without naming {expected:?}, so the \
         failure is not diagnosable from CI:\n{text}"
    );
}

/// A well-formed fat AppDir must pass the checker and produce a readable
/// size/content summary. At RED the checker script is missing, so this fails
/// on the invocation (the intended missing behaviour).
#[test]
fn checker_accepts_a_well_formed_fat_appdir() {
    let toolchain = stage_checked_bundle();
    let output = run_checker(&toolchain.appdir);
    assert!(
        output.status.success(),
        "tools/vmodv-appimage-check.sh rejected a well-formed fat AppDir ({}): the gate would \
         fail a good release.\n{}",
        toolchain.appdir.display(),
        output_text(&output),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for token in ["plugins:", "libraries:", "licences:", "bytes:"] {
        assert!(
            stdout.contains(token),
            "the checker's size/content summary does not include {token:?}, so it is not the \
             readable summary CI needs:\n{stdout}"
        );
    }
}

/// The gate really gates: each broken copy of the good bundle must be rejected
/// with an error naming the first failed assertion.
#[test]
fn checker_gate_rejects_broken_bundles() {
    let toolchain = stage_checked_bundle();
    let good = &toolchain.appdir;

    // (a) no plugins at all: camera/file/screen pipelines would be empty.
    let broken = toolchain.holder.path().join("broken-no-plugins");
    copy_dir_tree(good, &broken);
    fs::remove_dir_all(broken.join("usr/lib/gstreamer-1.0"))
        .expect("remove the plugin directory from the broken copy");
    assert_checker_rejects(
        &broken,
        "gstreamer-1.0",
        "an AppDir with no usr/lib/gstreamer-1.0 plugin directory",
    );

    // (b) no auditable manifest of bundled package versions.
    let broken = toolchain.holder.path().join("broken-no-manifest");
    copy_dir_tree(good, &broken);
    fs::remove_file(broken.join("usr/share/vmodv/gstreamer-bundle.json"))
        .expect("remove the bundle manifest from the broken copy");
    assert_checker_rejects(
        &broken,
        "gstreamer-bundle.json",
        "an AppDir with no GStreamer bundle manifest",
    );

    // (c) a bundled glibc family member shadows the host's libc.
    let broken = toolchain.holder.path().join("broken-glibc");
    copy_dir_tree(good, &broken);
    fs::write(broken.join("usr/lib/libc.so.6"), "fake host libc\n")
        .expect("plant a fake libc.so.6 in the broken copy");
    assert_checker_rejects(&broken, "libc.so.6", "an AppDir that bundles libc.so.6");

    // (d) a bundled host display library breaks the user's GPU stack.
    let broken = toolchain.holder.path().join("broken-wayland");
    copy_dir_tree(good, &broken);
    fs::write(
        broken.join("usr/lib/libwayland-client.so.0"),
        "fake host wayland\n",
    )
    .expect("plant a fake libwayland-client.so.0 in the broken copy");
    assert_checker_rejects(
        &broken,
        "libwayland",
        "an AppDir that bundles libwayland-client.so.0",
    );

    // (e) no RUNPATH evidence on the binary: it resolves against the host.
    let broken = toolchain.holder.path().join("broken-runpath");
    copy_dir_tree(good, &broken);
    let bare = compile_runpath_elf(
        &toolchain.holder.path().join("checker-elf-work"),
        "modv-no-runpath-elf",
        "int main(void) { return 0; }\n",
        false,
        None,
    );
    let binary = broken.join("usr/bin/modv");
    fs::copy(&bare, &binary).expect("replace modv with an ELF with no RUNPATH");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .expect("chmod the RUNPATH-less modv");
    assert_checker_rejects(
        &broken,
        "RUNPATH",
        "an AppDir whose usr/bin/modv has no $ORIGIN RUNPATH",
    );
}

/// The canonical local build must run the checker: a `make` target exists and
/// the AppImage staging script invokes the checker on the artifact it builds,
/// so `make build` fails on a thin bundle just like `vmodv-deb` does.
#[test]
fn canonical_build_invokes_the_bundle_checker() {
    let root = repo_root();
    let makefile = fs::read_to_string(root.join("Makefile"))
        .unwrap_or_else(|error| panic!("cannot read Makefile: {error}"));
    assert!(
        makefile.contains("vmodv-appimage-check"),
        "the Makefile has no vmodv-appimage-check target: the canonical local build cannot \
         verify the fat AppImage it just made:\n{makefile}"
    );
    let script = fs::read_to_string(root.join("tools/vmodv-appimage.sh"))
        .unwrap_or_else(|error| panic!("cannot read tools/vmodv-appimage.sh: {error}"));
    assert!(
        script.contains(CHECKER_SCRIPT),
        "tools/vmodv-appimage.sh never invokes {CHECKER_SCRIPT} on the AppImage it produced: \
         `make appimage-vmodv` (and therefore `make build`) would ship a thin bundle unchecked"
    );
}

/// Both workflows must verify the built AppImage, not just build it.
#[test]
fn ci_and_release_workflows_run_the_bundle_check() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/vmodv.yml"))
        .unwrap_or_else(|error| panic!("cannot read vmodv.yml: {error}"));
    assert!(
        ci.contains("--test appimage_bundle"),
        "vmodv.yml has no step running `make engine-test ARGS=\"-p modv-app --test \
         appimage_bundle -- --test-threads=1\"`: CI never exercises the bundle tests"
    );
    assert!(
        ci.contains("vmodv-appimage-check"),
        "vmodv.yml's package job does not run the bundle check on the AppImage it built"
    );
    let release = fs::read_to_string(root.join(".github/workflows/release.yml"))
        .unwrap_or_else(|error| panic!("cannot read release.yml: {error}"));
    assert!(
        release.contains("vmodv-appimage-check"),
        "release.yml's publish job does not run the bundle check before publishing the AppImage"
    );
}
