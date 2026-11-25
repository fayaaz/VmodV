#!/usr/bin/env bash
# Build the VmodV `.deb`.
#
# Runs inside the Dockerised Rust toolchain (`make vmodv-deb`), with the
# repository at /app and the cargo registry volume at /cargo - the same layout
# as tools/vmodv-appimage.sh. The package installs the `modv` wrapper (which
# exports the AppImage-equivalent asset paths) plus the real binary under
# /usr/lib/vmodv, the ISF corpus, the Point10 textures, a desktop entry
# and an icon. GStreamer/PipeWire/Vulkan/ALSA/X11 are declared as host runtime
# dependencies in DEBIAN/control. VmodV's own LICENSE and
# THIRD_PARTY_NOTICES.md install as usr/share/doc/vmodv/copyright and
# usr/share/doc/vmodv/THIRD_PARTY_NOTICES.md: the package's own licence and
# Point10 / CC BY-NC-SA attribution travel with it.
#
# Modes
#   real (default)  cargo build + stage + dpkg-deb + tools/vmodv-deb-check.sh.
#                   DEB_SUFFIX from the environment is appended to the artifact
#                   name and forwarded to the checker.
#   --dry-run       stage the identical package tree from explicit fake inputs
#                   (guarded by crates/modv-app/tests/deb_package.rs): no cargo
#                   build, no dpkg-deb and no checker invocation. The staged
#                   tree is left in place so callers can inspect DEBIAN/control.
#
# Flags (real-mode default in brackets):
#   --dry-run          stage only; run no cargo build, dpkg-deb or checker
#   --project-root DIR repository root [ROOT, default /app]
#   --binary FILE      modv binary [$CARGO_TARGET_DIR/release/modv or <project-root>/crates/target/release/modv]
#   --dist DIR         output directory [<project-root>/dist]
#   --version VER      package version [APP_VERSION / VMODV_VERSION / tools/version.sh]
#   --suffix SUFFIX    appended verbatim to the artifact name [DEB_SUFFIX, empty]
set -euo pipefail

DRY_RUN=0
ROOT="${ROOT:-}"
BINARY=""
DIST_ARG=""
VERSION_ARG=""
SUFFIX_ARG=""

usage() {
    cat <<'EOF'
Usage: tools/vmodv-deb.sh [options]
  --dry-run              stage the package tree from explicit fake inputs (no cargo/dpkg-deb/checker)
  --project-root DIR     repository root (default: ROOT or /app)
  --binary FILE          modv binary (default: $CARGO_TARGET_DIR/release/modv or <project-root>/crates/target/release/modv)
  --dist DIR             output directory (default: <project-root>/dist)
  --version VER          package version (default: APP_VERSION / VMODV_VERSION / tools/version.sh)
  --suffix SUFFIX        artifact-name suffix (default: DEB_SUFFIX or empty)
  -h, --help             show this help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run)         DRY_RUN=1; shift ;;
        --project-root)    ROOT="${2:?--project-root needs a value}"; shift 2 ;;
        --binary)          BINARY="${2:?--binary needs a value}"; shift 2 ;;
        --dist)            DIST_ARG="${2:?--dist needs a value}"; shift 2 ;;
        --version)         VERSION_ARG="${2:?--version needs a value}"; shift 2 ;;
        --suffix)          SUFFIX_ARG="${2:?--suffix needs a value}"; shift 2 ;;
        -h|--help)         usage; exit 0 ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

[ -n "$ROOT" ] || ROOT="/app"
BINARY="${BINARY:-${CARGO_TARGET_DIR:-$ROOT/crates/target}/release/modv}"
DIST="${DIST_ARG:-${DIST_DIR:-$ROOT/dist}}"
case "$DIST" in
    /*) ;;
    *) DIST="$ROOT/$DIST" ;;
esac
VERSION="${VERSION_ARG:-${APP_VERSION:-${VMODV_VERSION:-$(bash "$ROOT/tools/version.sh")}}}"
SUFFIX="${SUFFIX_ARG:-${DEB_SUFFIX:-}}"

PKG="VmodV-$VERSION-amd64$SUFFIX"
PKGROOT="$DIST/$PKG"
DEB="$DIST/$PKG.deb"

cleanup() {
    rm -rf "$PKGROOT"
}
# A dry run leaves the staged tree in place so callers can inspect DEBIAN/control.
if [ "$DRY_RUN" -eq 0 ]; then
    trap cleanup EXIT
    command -v dpkg-deb >/dev/null || {
        echo "error: dpkg-deb not found; install dpkg in the Rust toolchain image" >&2
        exit 1
    }
fi

rm -rf "$PKGROOT" "$DEB"
install -d -m 0755 \
    "$PKGROOT/DEBIAN" \
    "$PKGROOT/usr/bin" \
    "$PKGROOT/usr/lib/vmodv" \
    "$PKGROOT/usr/share/vmodv" \
    "$PKGROOT/usr/share/applications" \
    "$PKGROOT/usr/share/icons/hicolor/512x512/apps" \
    "$PKGROOT/usr/share/doc/vmodv"

# The crate version is packaging metadata's default; APP_VERSION/VMODV_VERSION
# only override the .deb version (the AppImage does the same).
if [ "$DRY_RUN" -eq 0 ]; then
    cargo build --locked --release --manifest-path "$ROOT/crates/Cargo.toml" \
        -p modv-app --bin modv
fi

install -Dm755 "$BINARY" \
    "$PKGROOT/usr/lib/vmodv/modv"
install -Dm755 "$ROOT/packaging/vmodv/log-run.sh" \
    "$PKGROOT/usr/lib/vmodv/log-run.sh"
install -Dm755 "$ROOT/packaging/vmodv/deb-launcher.sh" "$PKGROOT/usr/bin/modv"
ln -s modv "$PKGROOT/usr/bin/vmodv"

cp -a "$ROOT/resources/isf" \
    "$PKGROOT/usr/share/vmodv/isf"
cp -a "$ROOT/resources/point10/textures" \
    "$PKGROOT/usr/share/vmodv/textures"
install -Dm644 "$ROOT/packaging/vmodv/vmodv.desktop" \
    "$PKGROOT/usr/share/applications/vmodv.desktop"
install -Dm644 "$ROOT/resources/icon.png" \
    "$PKGROOT/usr/share/icons/hicolor/512x512/apps/vmodv.png"
install -Dm644 "$ROOT/LICENSE" "$PKGROOT/usr/share/doc/vmodv/copyright"
install -Dm644 "$ROOT/THIRD_PARTY_NOTICES.md" \
    "$PKGROOT/usr/share/doc/vmodv/THIRD_PARTY_NOTICES.md"

# cp -a preserves the checkout's permissions; normalise the payload so the
# archive does not depend on the host umask/ownership.
find "$PKGROOT/usr/share/vmodv" -type d -exec chmod 0755 {} +
find "$PKGROOT/usr/share/vmodv" -type f -exec chmod 0644 {} +

INSTALLED_KIB=$(du -sk "$PKGROOT/usr" | cut -f1)

# The dependency list in packaging/vmodv/vmodv.control is justified
# against `readelf -d crates/target/release/modv` (libasound/libpipewire/
# libgstreamer*/libglib are NEEDED) and the dlopened runtimes: libvulkan1 for
# wgpu, libxkbcommon-x11-0 for winit's X11 fallback, and the GStreamer plugin
# packages the MediaHub uses (v4l2/uridecodebins, decoders, pipewiresrc). The
# t64 Debian/Ubuntu packages provide the legacy libasound2 / libpipewire-0.3-0 /
# libglib2.0-0 names, so the manifest keeps those for compatibility across
# Debian 12 and trixie/Ubuntu 24.04+.
sed -e "s/@VERSION@/$VERSION/g" -e "s/@INSTALLED_SIZE@/$INSTALLED_KIB/g" \
    "$ROOT/packaging/vmodv/vmodv.control" >"$PKGROOT/DEBIAN/control"
chmod 0644 "$PKGROOT/DEBIAN/control"

# Standard Debian md5sums for the regular payload files.
(
    cd "$PKGROOT"
    find usr -type f -print0 | sort -z | xargs -0 md5sum
) >"$PKGROOT/DEBIAN/md5sums"
chmod 0644 "$PKGROOT/DEBIAN/md5sums"

if [ "$DRY_RUN" -eq 0 ]; then
    dpkg-deb --build --root-owner-group "$PKGROOT" "$DEB"
    rm -rf "$PKGROOT"

    if [ -f "$ROOT/tools/vmodv-deb-check.sh" ]; then
        ROOT="$ROOT" DIST_DIR="$DIST" APP_VERSION="$VERSION" DEB_SUFFIX="$SUFFIX" \
            bash "$ROOT/tools/vmodv-deb-check.sh"
    fi
    echo "VmodV .deb: $DEB ($(du -h "$DEB" | cut -f1))"
else
    STAGED=$(find "$PKGROOT" -type f | wc -l | tr -d ' ')
    echo "VmodV .deb dry run (staged, not built): $DEB"
    echo "  staged files: $STAGED, installed: ${INSTALLED_KIB} KiB"
fi
