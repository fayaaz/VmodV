#!/usr/bin/env bash
# Build the VmodV "fat" AppImage.
#
# Runs inside the Dockerised Rust toolchain (`make appimage-vmodv`), with the
# repository at /app and the cargo registry volume at /cargo. The AppImage
# bundles the `modv` binary, the ISF corpus and the Point10 textures, plus the
# GStreamer runtime installed in the build container: the plugin *.so files
# from gstreamer1.0-plugins-base/-good/-bad, gstreamer1.0-libav and
# gstreamer1.0-pipewire (usr/lib/gstreamer-1.0), gst-plugin-scanner
# (usr/libexec/gstreamer-1.0, flattened from the distro's multiarch directory)
# and the transitive userspace shared-library closure those plugins and `modv`
# need (usr/lib: gstreamer core + base/video/audio/pbutils/tag, glib/gobject,
# orc, libav/ffmpeg, pipewire, alsa, v4l2, ...). Users do not have to
# apt-install GStreamer for camera, file or screen capture to work.
#
# The glibc family never comes from the bundle: an AppImage always runs against
# the host's own glibc and copying it is how AppImages break. That is libc.so.6,
# libm.so.6, libdl*, libpthread*, librt*, the dynamic loader, and the rest of the
# libc6-owned helpers a library walk reports next to them (libresolv, libmvec,
# libnsl, libutil, libcrypt, libanl, libthread_db). Host graphics/display
# libraries stay host-side too — libGL/libEGL/GLES/glapi/gbm, the Vulkan loader,
# the GLVND dispatch layer (libGLX/libGLdispatch/libOpenGL) and the libwayland
# client stack: wgpu must talk to the user's own driver, and because AppRun
# prefixes usr/lib to LD_LIBRARY_PATH a staged copy of any of them shadows the
# host's — including for the host driver's own libraries, whose libwayland symbols
# then fail to resolve. Development-only artifacts (*.a, *.la, *.pc, *.pdb,
# headers) are never staged either.
#
# Every staged ELF gets an $ORIGIN-relative RUNPATH so the copies resolve each
# other inside the (read-only) image instead of falling back to the host's
# libraries. patchelf takes the file as its positional argument —
# `patchelf --set-rpath <rpath> <file>`; it has no `--` end-of-options separator,
# so `-- <file>` makes it treat `--` as the input file, error, and apply nothing.
# A patchelf failure is fatal, and in a real build so is patching nothing at all:
# an unpatched bundle still works on the build machine, which is what kept this
# failure invisible.
#
# The staged distro packages and versions are recorded in
# usr/share/vmodv/gstreamer-bundle.json, and every bundled package's
# /usr/share/doc/<package>/copyright licence text is shipped alongside (the
# libav/ffmpeg decoder carries LGPL/GPL redistribution obligations).
#
# packaging/vmodv/AppRun is the launcher: it exports GST_PLUGIN_PATH*,
# GST_PLUGIN_SCANNER*, an EMPTY GST_PLUGIN_SYSTEM_PATH* (so the host's plugins
# are never probed against the bundled core), a per-user writable GST_REGISTRY
# (the payload is read-only) and prefixes LD_LIBRARY_PATH with usr/lib.
#
# The .deb packaging stays host-dependent on purpose (tools/vmodv-deb.sh,
# packaging/vmodv/vmodv.control): host Depends is the right model for a distro
# package. Only the AppImage is fat.
#
# Modes
#   real (default)  cargo build + stage the container's GStreamer runtime +
#                   RUNPATHs + appimagetool (`make appimage-vmodv`).
#   --dry-run       stage the identical AppDir layout from explicit fake
#                   inputs (guarded by crates/modv-app/tests/appimage_bundle.rs):
#                   no cargo build, no appimagetool, no network. APP_VERSION
#                   overrides the version, so no git tags are consulted.
#
# Both modes run one staging pipeline: collect the plugins, stage plugins +
# scanner + libraries + manifest + licences, apply the RUNPATHs. The real-mode
# extras (the cargo build, the lddtree closure walk, the dpkg package list and
# appimagetool) are exactly the steps whose output an explicit flag already
# supplies, so a --dry-run exercises the shipped code instead of a parallel copy
# of it.
#
# Flags (real-mode default in brackets):
#   --dry-run
#   --project-root DIR   [directory containing this script's parent dir]
#   --appdir DIR         [<project-root>/dist/VmodV.AppDir]
#   --binary FILE        [<project-root>/crates/target/release/modv]
#   --plugin-root DIR    [/usr/lib/<multiarch>/gstreamer-1.0]
#   --lib-root DIR       [userspace closure computed from --binary + the
#                        plugins + --scanner with lddtree]
#   --scanner FILE       [/usr/lib/<multiarch>/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner]
#   --package-list FILE  [dpkg-query -W -f='${Package} ${Version}\n' records
#                        for the staged plugins/scanner/library closure]
#   --doc-root DIR       [/usr/share/doc]
set -euo pipefail

DRY_RUN=0
ROOT=""
APPDIR=""
BINARY=""
PLUGIN_ROOT=""
LIB_ROOT=""
SCANNER=""
DOC_ROOT=""
PACKAGE_LIST=""

# Staged inputs, filled in by the discovery steps.
PLUGIN_FILES=()
declare -gA PLUGIN_NAMES=()
CLOSURE_ORIGINS=()

usage() {
    cat <<'EOF'
Usage: tools/vmodv-appimage.sh [options]
  --dry-run              stage the fat AppDir from explicit fake inputs (no cargo/appimagetool)
  --project-root DIR     repository root (default: this script's parent dir)
  --appdir DIR           AppDir to stage into (default: <project-root>/dist/VmodV.AppDir)
  --binary FILE          modv binary (default: <project-root>/crates/target/release/modv)
  --plugin-root DIR      directory of GStreamer plugin *.so files
  --lib-root DIR         directory holding the userspace shared-library closure
  --scanner FILE         gst-plugin-scanner helper
  --package-list FILE    newline "<package> <version>" records (dpkg-query shape)
  --doc-root DIR         distro doc root for licence texts (default: /usr/share/doc)
  -h, --help             show this help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run)         DRY_RUN=1; shift ;;
        --project-root)    ROOT="${2:?--project-root needs a value}"; shift 2 ;;
        --appdir)          APPDIR="${2:?--appdir needs a value}"; shift 2 ;;
        --binary)          BINARY="${2:?--binary needs a value}"; shift 2 ;;
        --plugin-root)     PLUGIN_ROOT="${2:?--plugin-root needs a value}"; shift 2 ;;
        --lib-root)        LIB_ROOT="${2:?--lib-root needs a value}"; shift 2 ;;
        --scanner)         SCANNER="${2:?--scanner needs a value}"; shift 2 ;;
        --doc-root)        DOC_ROOT="${2:?--doc-root needs a value}"; shift 2 ;;
        --package-list)    PACKAGE_LIST="${2:?--package-list needs a value}"; shift 2 ;;
        -h|--help)         usage; exit 0 ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

fail() {
    echo "vmodv-appimage: $*" >&2
    exit 1
}

# Resolve the repository root from this script's location when not supplied.
if [ -z "$ROOT" ]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
fi

OUT="$ROOT/dist"
[ -n "$APPDIR" ] || APPDIR="$OUT/VmodV.AppDir"
[ -n "$DOC_ROOT" ] || DOC_ROOT=/usr/share/doc
VERSION="${APP_VERSION:-${VMODV_VERSION:-$(bash "$ROOT/tools/version.sh")}}"
IMAGE="$OUT/VmodV-$VERSION-x86_64.AppImage"

[ -d "$ROOT/resources/isf" ] || fail "missing ISF corpus at $ROOT/resources/isf"
[ -d "$ROOT/resources/point10/textures" ] || \
    fail "missing textures at $ROOT/resources/point10/textures"

# ---------------------------------------------------------------------------
# Filters and small predicates shared by both modes.
# ---------------------------------------------------------------------------

# The glibc family always comes from the host: the classic six plus the
# libc6-owned helpers an lddtree walk reports alongside libc.so.6.
is_glibc_family() {
    case "$1" in
        libc.so*|libm.so*|libdl.so*|libpthread.so*|librt.so*|ld-*| \
        libresolv.so*|libmvec.so*|libnsl.so*|libutil.so*|libcrypt.so*| \
        libanl.so*|libthread_db.so*) return 0 ;;
        *) return 1 ;;
    esac
}

# The host's graphics/display stack must come from the user's machine, not the
# bundle. AppRun prefixes usr/lib to LD_LIBRARY_PATH, so a staged copy of these
# libraries also shadows them for the *host's* GPU drivers: the Mesa vulkan ICDs
# link libwayland-client themselves, and loading the build container's older copy
# makes them fail with `undefined symbol: wl_fixes_interface`, which leaves wgpu
# with no adapter at all. The Wayland client stack is therefore host-side, like
# EGL/vulkan/GBM.
is_host_display_stack_lib() {
    local name="$1"
    case "$name" in
        libGL.so*|libGLES*.so*|libEGL.so*|libglapi.so*|libgbm.so*| \
            libvulkan.so*|libcuda.so*|libnvidia*) return 0 ;;
        # The GLVND dispatch layer: it exists to bind to the host's GL vendor, so
        # a staged copy would sit under the host's own libGL.so.1.
        libGLX.so*|libGLdispatch.so*|libOpenGL.so*) return 0 ;;
        libwayland.so*|libwayland-client.so*|libwayland-cursor.so*| \
            libwayland-egl.so*|libwayland-server.so*) return 0 ;;
        *) return 1 ;;
    esac
}

# Runtime shared objects only: never *.a, *.la, *.pc, *.pdb or headers.
is_runtime_library_name() {
    case "$1" in
        *.so|*.so.*) return 0 ;;
        *) return 1 ;;
    esac
}

# The one policy for what may be staged into usr/lib, shared by the closure walk
# and the staging step: a runtime shared object that is neither the host's own
# (glibc, GPU drivers) nor a plugin, which belongs in usr/lib/gstreamer-1.0.
is_staged_runtime_library() {
    local name="$1"
    is_runtime_library_name "$name" || return 1
    is_glibc_family "$name" && return 1
    is_host_display_stack_lib "$name" && return 1
    [[ -z "${PLUGIN_NAMES[$name]:-}" ]] || return 1
    return 0
}

# ELF magic, without needing readelf: the RUNPATH pass only touches real ELFs,
# which is also what lets a --dry-run push text stubs through the same code.
is_elf_file() {
    local magic=''
    IFS= read -rn 4 magic < "$1" || true
    [ "$magic" = $'\x7fELF' ]
}

# Absolute, symlink-free path. coreutils' `realpath` canonicalises by default and
# has no `-f`: an unsupported option exits 1 and `set -e` would abort the build.
canonicalise() {
    realpath -- "$1"
}

# Canonicalise every supplied input once, so a tree the distro keeps behind a
# symlink stages exactly like the directory it points at (`find` does not
# descend into a symlink it is given as a starting point) and so ownership
# lookups see the path dpkg knows.
canonicalise_inputs() {
    local name value resolved
    for name in BINARY PLUGIN_ROOT LIB_ROOT SCANNER DOC_ROOT PACKAGE_LIST; do
        value="${!name}"
        [ -n "$value" ] || continue
        resolved="$(canonicalise "$value")" || fail "cannot resolve $name=$value"
        printf -v "$name" '%s' "$resolved"
    done
}

# ---------------------------------------------------------------------------
# Real-mode discovery: plugin tree, scanner, library closure, package list.
# ---------------------------------------------------------------------------

discover_plugin_root() {
    local dir
    for dir in /usr/lib/*/gstreamer-1.0; do
        if [ -d "$dir" ]; then
            printf '%s\n' "$dir"
            return 0
        fi
    done
    return 1
}

discover_scanner() {
    local dir
    for dir in /usr/lib/*/gstreamer1.0/gstreamer-1.0; do
        if [ -f "$dir/gst-plugin-scanner" ]; then
            printf '%s/gst-plugin-scanner\n' "$dir"
            return 0
        fi
    done
    return 1
}

# Flatten every plugin under $PLUGIN_ROOT into PLUGIN_FILES / PLUGIN_NAMES.
collect_plugins() {
    mapfile -d '' -t PLUGIN_FILES < <(find "$PLUGIN_ROOT" -type f -name '*.so' -print0)
    [ "${#PLUGIN_FILES[@]}" -gt 0 ] || \
        fail "no GStreamer plugin *.so files found under $PLUGIN_ROOT"
    local file
    for file in "${PLUGIN_FILES[@]}"; do
        PLUGIN_NAMES["${file##*/}"]=1
    done
}

# Compute the transitive userspace shared-library closure of the binary, the
# scanner and every plugin with lddtree (pax-utils; a file-level ELF walk that
# executes nothing), materialise it into $LIB_ROOT, and remember the canonical
# distro paths so the manifest can name their owning packages.
build_library_closure() {
    command -v lddtree >/dev/null 2>&1 || \
        fail "lddtree (pax-utils) is missing from the build container: the fat AppImage cannot compute the library closure"
    LIB_ROOT="$WORK/lib-closure"
    mkdir -p "$LIB_ROOT"
    local path name rp copied=0
    declare -A seen=()
    mapfile -t closure_paths < <(
        lddtree -l --skip-missing --skip-non-elfs -- \
            "$BINARY" "$SCANNER" "${PLUGIN_FILES[@]}" 2>/dev/null | sort -u
    )
    for path in "${closure_paths[@]}"; do
        [ -n "$path" ] && [ -f "$path" ] || continue
        name="${path##*/}"
        is_staged_runtime_library "$name" || continue
        rp="$(canonicalise "$path")" || fail "cannot resolve $path"
        [ -z "${seen[$rp]:-}" ] || continue
        seen["$rp"]=1
        cp -p -- "$rp" "$LIB_ROOT/$name"
        CLOSURE_ORIGINS+=("$rp")
        copied=$((copied + 1))
    done
    [ "$copied" -gt 0 ] || fail "lddtree produced an empty userspace library closure"
    echo "library closure: $copied shared objects collected via lddtree"
}

# Turn the staged files' owning dpkg packages into the "<package> <version>"
# list the manifest and licence staging consume.
build_package_list() {
    command -v dpkg-query >/dev/null 2>&1 || \
        fail "dpkg-query is missing from the build container: the bundle manifest cannot record package versions"
    PACKAGE_LIST="$WORK/packages.list"
    declare -gA PKG_NAMES=()
    local file owner
    for file in "${PLUGIN_FILES[@]}" "$SCANNER" "${CLOSURE_ORIGINS[@]}"; do
        [ -n "$file" ] || continue
        owner="$(dpkg -S -- "$file" 2>/dev/null | head -n 1 | cut -d: -f1 || true)"
        [ -n "$owner" ] || continue
        PKG_NAMES["$owner"]=1
    done
    [ "${#PKG_NAMES[@]}" -gt 0 ] || \
        fail "no staged GStreamer file is owned by a dpkg package: the bundle manifest would be empty"
    dpkg-query -W -f='${Package} ${Version}\n' "${!PKG_NAMES[@]}" | LC_ALL=C sort > "$PACKAGE_LIST"
}

# ---------------------------------------------------------------------------
# Staging: the pipeline both modes run.
# ---------------------------------------------------------------------------

stage_application_payload() {
    install -Dm755 -- "$BINARY" "$APPDIR/usr/bin/modv"
    install -Dm755 -- "$ROOT/packaging/vmodv/log-run.sh" "$APPDIR/usr/bin/log-run.sh"
    cp -a -- "$ROOT/resources/isf" "$APPDIR/usr/share/vmodv/isf"
    cp -a -- "$ROOT/resources/point10/textures" "$APPDIR/usr/share/vmodv/textures"
    install -Dm755 -- "$ROOT/packaging/vmodv/AppRun" "$APPDIR/AppRun"
    install -Dm644 -- "$ROOT/packaging/vmodv/vmodv.desktop" "$APPDIR/vmodv.desktop"
    install -Dm644 -- "$ROOT/resources/icon.png" "$APPDIR/vmodv.png"
    ln -sf vmodv.png "$APPDIR/.DirIcon"
}

stage_gstreamer_plugins() {
    mkdir -p "$APPDIR/usr/lib/gstreamer-1.0"
    local file
    for file in "${PLUGIN_FILES[@]}"; do
        cp -p -- "$file" "$APPDIR/usr/lib/gstreamer-1.0/${file##*/}"
    done
    echo "GStreamer plugins: ${#PLUGIN_FILES[@]} staged into usr/lib/gstreamer-1.0"
}

stage_gstreamer_scanner() {
    mkdir -p "$APPDIR/usr/libexec/gstreamer-1.0"
    cp -p -- "$SCANNER" "$APPDIR/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
    chmod 755 "$APPDIR/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
}

stage_gstreamer_libraries() {
    mkdir -p "$APPDIR/usr/lib"
    local file name staged=0
    mapfile -d '' -t lib_files < <(find "$LIB_ROOT" -type f \( -name '*.so' -o -name '*.so.*' \) -print0)
    for file in "${lib_files[@]}"; do
        name="${file##*/}"
        is_staged_runtime_library "$name" || continue
        cp -p -- "$file" "$APPDIR/usr/lib/$name"
        staged=$((staged + 1))
    done
    echo "library closure: $staged shared objects staged into usr/lib"
}

write_bundle_manifest() {
    local out="$APPDIR/usr/share/vmodv/gstreamer-bundle.json"
    mkdir -p "$(dirname -- "$out")"
    {
        printf '{\n'
        printf '  "schema": "vmodv-gstreamer-bundle",\n'
        printf '  "appVersion": "%s",\n' "$VERSION"
        printf '  "gstPluginDir": "usr/lib/gstreamer-1.0",\n'
        printf '  "gstLibraryDir": "usr/lib",\n'
        printf '  "packages": [\n'
        local first=1 name version
        while read -r name version; do
            [ -n "$name" ] || continue
            version="${version:-}"
            if [ "$first" -eq 1 ]; then first=0; else printf ',\n'; fi
            printf '    {"name": "%s", "version": "%s"}' "$name" "$version"
        done < "$PACKAGE_LIST"
        printf '\n  ]\n}\n'
    } > "$out"
}

copy_bundle_licences() {
    local name version src dst staged=0
    while read -r name version; do
        [ -n "$name" ] || continue
        src="$DOC_ROOT/$name/copyright"
        [ -f "$src" ] || fail "missing licence text for bundled package $name: $src"
        dst="$APPDIR/usr/share/doc/$name/copyright"
        mkdir -p "$(dirname -- "$dst")"
        cp -p -- "$src" "$dst"
        staged=$((staged + 1))
    done < "$PACKAGE_LIST"
    echo "licences: $staged package copyright texts staged into usr/share/doc"
}

# Point one staged ELF at the bundle. Non-ELF staged files (a --dry-run's stubs,
# a shell script) are left alone; every patchelf failure is fatal.
patched_elfs=0
patch_bundle_elf() {
    local file="$1" rpath="$2"
    is_elf_file "$file" || return 0
    command -v patchelf >/dev/null 2>&1 || \
        fail "patchelf is missing from the build container: the bundle cannot make its library paths self-contained"
    # `patchelf --set-rpath <rpath> <file>`: the file is patchelf's positional
    # argument. `-- <file>` is not an option separator — patchelf takes `--` as
    # the input file, fails, and leaves the distro's RUNPATH untouched.
    patchelf --set-rpath "$rpath" "$file" || \
        fail "patchelf --set-rpath '$rpath' failed on $file: the staged bundle would keep the distro's absolute library paths and resolve against the host instead of inside the image"
    patched_elfs=$((patched_elfs + 1))
}

patch_bundle_rpaths() {
    local file
    patch_bundle_elf "$APPDIR/usr/bin/modv" '$ORIGIN/../lib'
    patch_bundle_elf "$APPDIR/usr/libexec/gstreamer-1.0/gst-plugin-scanner" '$ORIGIN/../../lib'
    while IFS= read -r -d '' file; do
        patch_bundle_elf "$file" '$ORIGIN'
    done < <(find "$APPDIR/usr/lib" -maxdepth 1 -type f -print0)
    while IFS= read -r -d '' file; do
        patch_bundle_elf "$file" '$ORIGIN/..'
    done < <(find "$APPDIR/usr/lib/gstreamer-1.0" -maxdepth 1 -type f -print0)
    # A real build that patched nothing is the bug this guards against: the
    # bundle still runs, because it is quietly using the host's libraries.
    [ "$DRY_RUN" -eq 1 ] || [ "$patched_elfs" -gt 0 ] || \
        fail "no staged ELF got a RUNPATH: the bundle would resolve GStreamer against the host, not itself"
    echo "RUNPATHs: $patched_elfs bundled ELFs point at the bundle itself"
}

# ---------------------------------------------------------------------------
# Packaging (real mode only).
# ---------------------------------------------------------------------------

build_appimage() {
    local tool=/tmp/appimagetool
    if [ ! -x "$tool" ]; then
        curl -L --fail --retry 3 -o "$tool" \
            https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
        chmod +x "$tool"
    fi
    ARCH=x86_64 "$tool" --appimage-extract-and-run --no-appstream "$APPDIR" "$IMAGE"
    rm -rf "$APPDIR"
    echo "VmodV AppImage: $IMAGE"
}

# ---------------------------------------------------------------------------
# Main.
# ---------------------------------------------------------------------------

WORK=""
cleanup() {
    if [ -n "$WORK" ]; then
        rm -rf "$WORK"
    fi
}
trap cleanup EXIT

if [ "$DRY_RUN" -eq 1 ]; then
    # A dry run supplies every input the real build would discover.
    [ -f "$BINARY" ] || fail "--dry-run needs --binary FILE"
    [ -d "$PLUGIN_ROOT" ] || fail "--dry-run needs --plugin-root DIR"
    [ -d "$LIB_ROOT" ] || fail "--dry-run needs --lib-root DIR"
    [ -f "$SCANNER" ] || fail "--dry-run needs --scanner FILE"
    [ -f "$PACKAGE_LIST" ] || fail "--dry-run needs --package-list FILE"
    [ -d "$DOC_ROOT" ] || fail "--dry-run needs --doc-root DIR"
else
    [ -n "$BINARY" ] || BINARY="${CARGO_TARGET_DIR:-$ROOT/crates/target}/release/modv"
    [ -n "$PLUGIN_ROOT" ] || PLUGIN_ROOT="$(discover_plugin_root)" || \
        fail "no GStreamer plugin directory (/usr/lib/*/gstreamer-1.0) in the build container: rebuild the modv-rust image"
    [ -n "$SCANNER" ] || SCANNER="$(discover_scanner)" || \
        fail "no gst-plugin-scanner (/usr/lib/*/gstreamer1.0/gstreamer-1.0) in the build container: rebuild the modv-rust image"
    WORK="$(mktemp -d)"
fi

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/vmodv"

# The cargo build is the only stage a dry run replaces outright.
if [ "$DRY_RUN" -eq 0 ]; then
    cargo build --release --manifest-path "$ROOT/crates/Cargo.toml" -p modv-app --bin modv
    [ -f "$BINARY" ] || fail "cargo build did not produce $BINARY"
fi

canonicalise_inputs
collect_plugins

# Discovery fills in only what the caller did not hand over: a real build walks
# the closure with lddtree and reads the owning packages from dpkg, a dry run
# passes both in.
if [ -z "$LIB_ROOT" ]; then
    build_library_closure
fi
if [ -z "$PACKAGE_LIST" ]; then
    build_package_list
fi

stage_application_payload
stage_gstreamer_plugins
stage_gstreamer_scanner
stage_gstreamer_libraries
write_bundle_manifest
copy_bundle_licences
patch_bundle_rpaths

if [ "$DRY_RUN" -eq 0 ]; then
    build_appimage
    # Gate the canonical build on the artifact it just produced: a thin bundle
    # fails `make appimage-vmodv` (and therefore `make build`) here, exactly as
    # vmodv-deb checks the .deb after packaging. The dry run is exercised by the
    # appimage_bundle tests with text stubs that have no RUNPATH, so it is not
    # checked here.
    bash "$ROOT/tools/vmodv-appimage-check.sh" "$IMAGE"
else
    echo "dry-run complete: fat AppDir staged at $APPDIR"
fi
