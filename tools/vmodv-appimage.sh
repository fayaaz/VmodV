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
# The glibc family (libc.so.6, libm.so.6, libdl*, libpthread*, librt*, the
# dynamic loader) is never bundled: an AppImage always runs against the host's
# own glibc and copying it is how AppImages break. Host graphics drivers
# (libGL/libEGL/GLES/glapi/gbm, the Vulkan loader) stay host-side too — wgpu
# must talk to the user's own driver. Development-only artifacts (*.a, *.la,
# *.pc, *.pdb, headers) are never staged either.
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
#                   patchelf RPATHs + appimagetool (`make appimage-vmodv`).
#   --dry-run       stage the identical AppDir layout from explicit fake
#                   inputs (guarded by crates/modv-app/tests/appimage_bundle.rs):
#                   no cargo build, no appimagetool, no network. APP_VERSION
#                   overrides the version, so no git tags are consulted.
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
# Filters shared by both modes.
# ---------------------------------------------------------------------------

# The glibc family always comes from the host.
is_glibc_family() {
    case "$1" in
        libc.so*|libm.so*|libdl.so*|libpthread.so*|librt.so*|ld-*) return 0 ;;
        *) return 1 ;;
    esac
}

# Host graphics drivers must come from the user's machine, not the bundle.
is_host_driver_lib() {
    case "$1" in
        libGL.so*|libGLES*.so*|libEGL.so*|libglapi.so*|libgbm.so*|libvulkan.so*|libcuda.so*|libnvidia*) return 0 ;;
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

# Flatten every staged plugin path into PLUGIN_FILES / PLUGIN_NAMES.
collect_plugins() {
    PLUGIN_FILES=()
    declare -gA PLUGIN_NAMES=()
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
# executes nothing), materialise it into $LIB_ROOT, and remember the original
# distro paths so the manifest can name their owning packages.
build_library_closure() {
    command -v lddtree >/dev/null 2>&1 || \
        fail "lddtree (pax-utils) is missing from the build container: the fat AppImage cannot compute the library closure"
    LIB_ROOT="$WORK/lib-closure"
    mkdir -p "$LIB_ROOT"
    CLOSURE_ORIGINS=()
    local path name rp copied=0
    mapfile -t closure_paths < <(
        lddtree -l --skip-missing --skip-non-elfs -- \
            "$BINARY" "$SCANNER" "${PLUGIN_FILES[@]}" 2>/dev/null | sort -u
    )
    for path in "${closure_paths[@]}"; do
        [ -n "$path" ] && [ -f "$path" ] || continue
        name="${path##*/}"
        is_runtime_library_name "$name" || continue
        is_glibc_family "$name" && continue
        is_host_driver_lib "$name" && continue
        [ -n "${PLUGIN_NAMES[$name]:-}" ] && continue
        cp -p -- "$path" "$LIB_ROOT/$name"
        rp="$(realpath -f -- "$path")"
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
    local file owner rp
    for file in "${PLUGIN_FILES[@]}" "$SCANNER" "${CLOSURE_ORIGINS[@]}"; do
        rp="$(realpath -f -- "$file" 2>/dev/null || printf '%s' "$file")"
        owner="$(dpkg -S -- "$rp" 2>/dev/null | head -n 1 | cut -d: -f1 || true)"
        [ -n "$owner" ] || continue
        PKG_NAMES["${owner%%:*}"]=1
    done
    [ "${#PKG_NAMES[@]}" -gt 0 ] || \
        fail "no staged GStreamer file is owned by a dpkg package: the bundle manifest would be empty"
    dpkg-query -W -f='${Package} ${Version}\n' "${!PKG_NAMES[@]}" | LC_ALL=C sort > "$PACKAGE_LIST"
}

# ---------------------------------------------------------------------------
# Staging (shared by --dry-run and the real build).
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
        if is_glibc_family "$name" || is_host_driver_lib "$name" || \
           [ -n "${PLUGIN_NAMES[$name]:-}" ]; then
            continue
        fi
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

# Point the bundled ELFs at the bundle itself. The staged copies keep the
# distro NEEDED entries; $ORIGIN-relative RUNPATHs make them resolve each
# other inside (read-only) image without shadowing the host. Skipped for
# --dry-run: the fake fixtures are text stubs, not ELF files.
patch_bundle_rpaths() {
    command -v patchelf >/dev/null 2>&1 || \
        fail "patchelf is missing from the build container: the bundle cannot make its library paths self-contained"
    patch_one() {
        local file="$1" rpath="$2"
        patchelf --set-rpath "$rpath" -- "$file" || \
            echo "vmodv-appimage: warning: patchelf failed on $file" >&2
    }
    patch_one "$APPDIR/usr/bin/modv" '$ORIGIN/../lib'
    patch_one "$APPDIR/usr/libexec/gstreamer-1.0/gst-plugin-scanner" '$ORIGIN/../../lib'
    local file
    while IFS= read -r -d '' file; do
        patch_one "$file" '$ORIGIN'
    done < <(find "$APPDIR/usr/lib" -maxdepth 1 -type f -print0)
    while IFS= read -r -d '' file; do
        patch_one "$file" '$ORIGIN/..'
    done < <(find "$APPDIR/usr/lib/gstreamer-1.0" -maxdepth 1 -type f -print0)
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
    [ -f "$BINARY" ] || fail "--dry-run needs --binary FILE"
    [ -d "$PLUGIN_ROOT" ] || fail "--dry-run needs --plugin-root DIR"
    [ -d "$LIB_ROOT" ] || fail "--dry-run needs --lib-root DIR"
    [ -f "$SCANNER" ] || fail "--dry-run needs --scanner FILE"
    [ -f "$PACKAGE_LIST" ] || fail "--dry-run needs --package-list FILE"
    [ -d "$DOC_ROOT" ] || fail "--dry-run needs --doc-root DIR"
else
    if [ -z "$BINARY" ]; then
        BINARY="${CARGO_TARGET_DIR:-$ROOT/crates/target}/release/modv"
    fi
    [ -n "$PLUGIN_ROOT" ] || PLUGIN_ROOT="$(discover_plugin_root)" || \
        fail "no GStreamer plugin directory (/usr/lib/*/gstreamer-1.0) in the build container: rebuild the modv-rust image"
    [ -n "$SCANNER" ] || SCANNER="$(discover_scanner)" || \
        fail "no gst-plugin-scanner (/usr/lib/*/gstreamer1.0/gstreamer-1.0) in the build container: rebuild the modv-rust image"
    WORK="$(mktemp -d)"
fi

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/vmodv"

if [ "$DRY_RUN" -eq 0 ]; then
    cargo build --release --manifest-path "$ROOT/crates/Cargo.toml" -p modv-app --bin modv
    [ -f "$BINARY" ] || fail "cargo build did not produce $BINARY"
fi

collect_plugins
if [ "$DRY_RUN" -eq 0 ]; then
    build_library_closure
    build_package_list
fi

stage_application_payload
stage_gstreamer_plugins
stage_gstreamer_scanner
stage_gstreamer_libraries
write_bundle_manifest
copy_bundle_licences

if [ "$DRY_RUN" -eq 0 ]; then
    patch_bundle_rpaths

    TOOL=/tmp/appimagetool
    if [ ! -x "$TOOL" ]; then
        curl -L --fail --retry 3 -o "$TOOL" \
            https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
        chmod +x "$TOOL"
    fi

    ARCH=x86_64 "$TOOL" --appimage-extract-and-run --no-appstream \
        "$APPDIR" "$IMAGE"

    rm -rf "$APPDIR"
    echo "VmodV AppImage: $IMAGE"
else
    echo "dry-run complete: fat AppDir staged at $APPDIR"
fi
