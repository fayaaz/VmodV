#!/usr/bin/env bash
# Build the VmodV AppImage.
#
# Runs inside the Dockerised Rust toolchain (`make appimage-vmodv`), with the
# repository at /app and the cargo registry volume at /cargo. The AppImage
# bundles the `modv` binary, the ISF corpus and the Point10 textures; host
# GStreamer/PipeWire/Vulkan/ALSA are runtime dependencies (the same ones the
# hybrid engine already documents).
set -euo pipefail

ROOT="${ROOT:-/app}"
VERSION="${APP_VERSION:-${VMODV_VERSION:-$(bash "$ROOT/tools/version.sh")}}"
OUT="$ROOT/dist"
APPDIR="$OUT/VmodV.AppDir"
IMAGE="$OUT/VmodV-$VERSION-x86_64.AppImage"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/vmodv"

cargo build --release --manifest-path "$ROOT/crates/Cargo.toml" -p modv-app --bin modv

install -Dm755 "$ROOT/crates/target/release/modv" "$APPDIR/usr/bin/modv"
install -Dm755 "$ROOT/packaging/vmodv/log-run.sh" \
    "$APPDIR/usr/bin/log-run.sh"
cp -a "$ROOT/resources/isf" \
    "$APPDIR/usr/share/vmodv/isf"
cp -a "$ROOT/resources/point10/textures" \
    "$APPDIR/usr/share/vmodv/textures"
install -Dm755 "$ROOT/packaging/vmodv/AppRun" "$APPDIR/AppRun"
install -Dm644 "$ROOT/packaging/vmodv/vmodv.desktop" \
    "$APPDIR/vmodv.desktop"
install -Dm644 "$ROOT/resources/icon.png" "$APPDIR/vmodv.png"
ln -sf vmodv.png "$APPDIR/.DirIcon"

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
