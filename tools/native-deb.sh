#!/usr/bin/env bash
# Build the native Rust modV `.deb`.
#
# Runs inside the Dockerised Rust toolchain (`make native-deb`), with the
# repository at /app and the cargo registry volume at /cargo - the same layout
# as tools/native-appimage.sh. The package installs the `modv` wrapper (which
# exports the AppImage-equivalent asset paths) plus the real binary under
# /usr/lib/vmodv-native, the ISF corpus, the Point10 textures, a desktop entry
# and an icon. GStreamer/PipeWire/Vulkan/ALSA/X11 are declared as host runtime
# dependencies in DEBIAN/control.
set -euo pipefail

ROOT="${ROOT:-/app}"
VERSION="${APP_VERSION:-${NATIVE_VERSION:-0.1.0}}"
DIST="${DIST_DIR:-$ROOT/dist}"
case "$DIST" in
    /*) ;;
    *) DIST="$ROOT/$DIST" ;;
esac

PKG="VmodV-Native-$VERSION-amd64"
PKGROOT="$DIST/$PKG"
DEB="$DIST/$PKG.deb"

cleanup() {
    rm -rf "$PKGROOT"
}
trap cleanup EXIT

command -v dpkg-deb >/dev/null || {
    echo "error: dpkg-deb not found; install dpkg in the Rust toolchain image" >&2
    exit 1
}

rm -rf "$PKGROOT" "$DEB"
install -d -m 0755 \
    "$PKGROOT/DEBIAN" \
    "$PKGROOT/usr/bin" \
    "$PKGROOT/usr/lib/vmodv-native" \
    "$PKGROOT/usr/share/vmodv" \
    "$PKGROOT/usr/share/applications" \
    "$PKGROOT/usr/share/icons/hicolor/512x512/apps" \
    "$PKGROOT/usr/share/doc/vmodv-native"

# The crate version is packaging metadata's default; APP_VERSION/NATIVE_VERSION
# only override the .deb version (the AppImage does the same).
cargo build --locked --release --manifest-path "$ROOT/crates/Cargo.toml" \
    -p modv-app --bin modv

install -Dm755 "$ROOT/crates/target/release/modv" \
    "$PKGROOT/usr/lib/vmodv-native/modv"
install -Dm755 "$ROOT/packaging/native/deb-launcher.sh" "$PKGROOT/usr/bin/modv"
ln -s modv "$PKGROOT/usr/bin/vmodv-native"

cp -a "$ROOT/src/renderer/src/application/renderers/isf/modules/isf" \
    "$PKGROOT/usr/share/vmodv/isf"
cp -a "$ROOT/src/renderer/src/application/point10/textures" \
    "$PKGROOT/usr/share/vmodv/textures"
install -Dm644 "$ROOT/packaging/native/vmodv-native.desktop" \
    "$PKGROOT/usr/share/applications/vmodv-native.desktop"
install -Dm644 "$ROOT/resources/icon.png" \
    "$PKGROOT/usr/share/icons/hicolor/512x512/apps/vmodv-native.png"
install -Dm644 "$ROOT/LICENSE" "$PKGROOT/usr/share/doc/vmodv-native/copyright"

# cp -a preserves the checkout's permissions; normalise the payload so the
# archive does not depend on the host umask/ownership.
find "$PKGROOT/usr/share/vmodv" -type d -exec chmod 0755 {} +
find "$PKGROOT/usr/share/vmodv" -type f -exec chmod 0644 {} +

INSTALLED_KIB=$(du -sk "$PKGROOT/usr" | cut -f1)

# The dependency list in packaging/native/vmodv-native.control is justified
# against `readelf -d crates/target/release/modv` (libasound/libpipewire/
# libgstreamer*/libglib are NEEDED) and the dlopened runtimes: libvulkan1 for
# wgpu, libxkbcommon-x11-0 for winit's X11 fallback, and the GStreamer plugin
# packages the MediaHub uses (v4l2/uridecodebins, decoders, pipewiresrc). The
# t64 Debian/Ubuntu packages provide the legacy libasound2 / libpipewire-0.3-0 /
# libglib2.0-0 names, so the manifest keeps those for compatibility across
# Debian 12 and trixie/Ubuntu 24.04+.
sed -e "s/@VERSION@/$VERSION/g" -e "s/@INSTALLED_SIZE@/$INSTALLED_KIB/g" \
    "$ROOT/packaging/native/vmodv-native.control" >"$PKGROOT/DEBIAN/control"
chmod 0644 "$PKGROOT/DEBIAN/control"

# Standard Debian md5sums for the regular payload files.
(
    cd "$PKGROOT"
    find usr -type f -print0 | sort -z | xargs -0 md5sum
) >"$PKGROOT/DEBIAN/md5sums"
chmod 0644 "$PKGROOT/DEBIAN/md5sums"

dpkg-deb --build --root-owner-group "$PKGROOT" "$DEB"
rm -rf "$PKGROOT"

if [ -f "$ROOT/tools/native-deb-check.sh" ]; then
    ROOT="$ROOT" DIST_DIR="$DIST" APP_VERSION="$VERSION" \
        bash "$ROOT/tools/native-deb-check.sh"
fi

echo "native .deb: $DEB ($(du -h "$DEB" | cut -f1))"
