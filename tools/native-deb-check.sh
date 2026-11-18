#!/usr/bin/env bash
# Verify the native `.deb` built by tools/native-deb.sh without root.
#
# `make native-deb-check` runs this inside the Dockerised Rust toolchain, and
# tools/native-deb.sh calls it at the end of every build. It inspects the
# archive metadata and file list, extracts the payload to a temporary directory
# and checks that the packaged binary starts (`--help`/`--version`) and that
# the desktop entry points at the launcher.
set -euo pipefail

ROOT="${ROOT:-/app}"
VERSION="${APP_VERSION:-${NATIVE_VERSION:-0.1.0}}"
DIST="${DIST_DIR:-$ROOT/dist}"
case "$DIST" in
    /*) ;;
    *) DIST="$ROOT/$DIST" ;;
esac
DEB="$DIST/VmodV-Native-$VERSION-amd64.deb"

# The corpus count is derived from the checkout when available so a future ISF
# addition does not need this script edited; the fixed fallback is the current
# corpus size (354) for checking an artifact outside the repository.
EXPECTED_ISF="${EXPECTED_ISF:-}"
if [ -z "$EXPECTED_ISF" ]; then
    if [ -d "$ROOT/src/renderer/src/application/renderers/isf/modules/isf" ]; then
        EXPECTED_ISF=$(find "$ROOT/src/renderer/src/application/renderers/isf/modules/isf" \
            -type f -name '*.fs' | wc -l | tr -d ' ')
    else
        EXPECTED_ISF=354
    fi
fi
EXPECTED_TEXTURES="${EXPECTED_TEXTURES:-}"
if [ -z "$EXPECTED_TEXTURES" ]; then
    if [ -d "$ROOT/src/renderer/src/application/point10/textures" ]; then
        EXPECTED_TEXTURES=$(find "$ROOT/src/renderer/src/application/point10/textures" \
            -type f | wc -l | tr -d ' ')
    else
        EXPECTED_TEXTURES=25
    fi
fi

fail() {
    echo "native-deb-check: $*" >&2
    exit 1
}

[ -f "$DEB" ] || fail "missing $DEB (run 'make native-deb' first)"
command -v dpkg-deb >/dev/null || fail "dpkg-deb is not available"

field() {
    dpkg-deb -f "$DEB" "$1"
}

assert_field() {
    local name=$1 expected=$2 actual
    actual=$(field "$name")
    [ "$actual" = "$expected" ] || fail "$name is '$actual', expected '$expected'"
}

assert_field Package vmodv-native
assert_field Version "$VERSION"
assert_field Architecture amd64
assert_field Section sound
assert_field Priority optional
assert_field Homepage "https://github.com/fayaaz/modV"

MAINTAINER=$(field Maintainer)
[ -n "$MAINTAINER" ] || fail "Maintainer is empty"
DESCRIPTION=$(field Description | sed -n '1p')
[ -n "$DESCRIPTION" ] || fail "Description is empty"
INSTALLED_SIZE=$(field Installed-Size)
case "$INSTALLED_SIZE" in
    '' | *[!0-9]*) fail "Installed-Size is not numeric: '$INSTALLED_SIZE'" ;;
esac
[ "$INSTALLED_SIZE" -gt 0 ] || fail "Installed-Size is zero"

# Runtime dependencies: the NEEDED libraries from `readelf -d` plus the
# dlopened graphics backends and the GStreamer plugins the media path needs.
DEPENDS_NAMES=$(field Depends | tr ',' '\n' | sed 's/^ *//; s/ *$//' | cut -d' ' -f1)
for dep in \
    libc6 \
    libasound2 \
    libpipewire-0.3-0 \
    libgstreamer1.0-0 \
    libgstreamer-plugins-base1.0-0 \
    libglib2.0-0 \
    libvulkan1 \
    libxkbcommon-x11-0 \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-libav \
    gstreamer1.0-pipewire; do
    if ! grep -qx "$dep" <<<"$DEPENDS_NAMES"; then
        fail "Depends is missing $dep (got: $(field Depends))"
    fi
done

CONTENTS=$(dpkg-deb -c "$DEB")

has_path() {
    grep -qE " $1\$" <<<"$CONTENTS" || fail "missing $1"
}

has_path '\./usr/bin/modv'
has_path '\./usr/lib/vmodv-native/modv'
has_path '\./usr/lib/vmodv-native/log-run.sh'
has_path '\./usr/share/applications/vmodv-native.desktop'
has_path '\./usr/share/icons/hicolor/512x512/apps/vmodv-native.png'
grep -qE ' \./usr/bin/vmodv-native -> modv$' <<<"$CONTENTS" ||
    fail "missing usr/bin/vmodv-native symlink"
grep -qE '^-rwxr-xr-x root/root .* \./usr/bin/modv$' <<<"$CONTENTS" ||
    fail "usr/bin/modv is not a root-owned executable"
grep -qE '^-rwxr-xr-x root/root .* \./usr/lib/vmodv-native/modv$' <<<"$CONTENTS" ||
    fail "usr/lib/vmodv-native/modv is not a root-owned executable"
grep -qE '^-rwxr-xr-x root/root .* \./usr/lib/vmodv-native/log-run.sh$' <<<"$CONTENTS" ||
    fail "usr/lib/vmodv-native/log-run.sh is not a root-owned executable"

isf_count=$(grep -cE '^-.* \./usr/share/vmodv/isf/.*\.fs$' <<<"$CONTENTS" || true)
[ "$isf_count" -eq "$EXPECTED_ISF" ] ||
    fail "packaged ISF corpus has $isf_count .fs files, expected $EXPECTED_ISF"

texture_count=$(grep -cE '^-.* \./usr/share/vmodv/textures/.*$' <<<"$CONTENTS" || true)
[ "$texture_count" -eq "$EXPECTED_TEXTURES" ] ||
    fail "packaged textures: $texture_count files, expected $EXPECTED_TEXTURES"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
dpkg-deb -x "$DEB" "$TMP"

[ -x "$TMP/usr/lib/vmodv-native/modv" ] || fail "binary lacks the executable bit"
grep -q 'MODV_ISF_DIR' "$TMP/usr/bin/modv" ||
    fail "launcher does not define MODV_ISF_DIR"
grep -q 'MODV_ASSETS_DIR' "$TMP/usr/bin/modv" ||
    fail "launcher does not define MODV_ASSETS_DIR"
grep -q 'log-run.sh' "$TMP/usr/bin/modv" ||
    fail "launcher does not run the binary through log-run.sh"
grep -q '^Exec=modv$' "$TMP/usr/share/applications/vmodv-native.desktop" ||
    fail "desktop entry does not launch modv"
grep -q '^Icon=vmodv-native$' "$TMP/usr/share/applications/vmodv-native.desktop" ||
    fail "desktop entry does not reference the vmodv-native icon"
ICON_TYPE=$(file -b "$TMP/usr/share/icons/hicolor/512x512/apps/vmodv-native.png")
case "$ICON_TYPE" in
    PNG*image*data*) ;;
    *) fail "icon is not a PNG: $ICON_TYPE" ;;
esac

# clap answers --help/--version before any GPU, window or socket work, so this
# stays a packaging check rather than a rendering smoke test. The launcher
# resolves its prefix and goes through log-run.sh, so it also writes the daily
# log from this extracted tree.
LAUNCH_DATA="$TMP/launch-data"
MODV_USER_DATA="$LAUNCH_DATA" "$TMP/usr/bin/modv" --help >/dev/null
BINARY_VERSION=$(MODV_USER_DATA="$LAUNCH_DATA" "$TMP/usr/bin/modv" --version)
case "$BINARY_VERSION" in
    modv-engine\ [0-9]*) ;;
    *) fail "unexpected --version output: '$BINARY_VERSION'" ;;
esac
LAUNCH_LOG="$LAUNCH_DATA/logs/vmodv-$(date +%F).log"
[ -f "$LAUNCH_LOG" ] || fail "launcher did not create $LAUNCH_LOG"
grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}[+-][0-9]{4} .*pid=[0-9]+' "$LAUNCH_LOG" ||
    fail "launcher log has no ISO-timestamp/pid launch header"
grep -qF -- "$BINARY_VERSION" "$LAUNCH_LOG" &&
    fail "launcher log captured stdout from --version"
grep -qF -- 'Usage:' "$LAUNCH_LOG" &&
    fail "launcher log captured stdout from --help"

# A rejected flag writes to stderr and exits non-zero; both the terminal and
# the log must see clap's error, while stdout stays empty.
set +e
MODV_USER_DATA="$LAUNCH_DATA" "$TMP/usr/bin/modv" --not-a-real-modv-flag \
    >"$TMP/launch.out" 2>"$TMP/launch.err"
LAUNCH_STATUS=$?
set -e
[ "$LAUNCH_STATUS" -ne 0 ] || fail "invalid flag unexpectedly exited 0"
[ ! -s "$TMP/launch.out" ] || fail "invalid flag wrote to stdout"
grep -q 'error:' "$TMP/launch.err" ||
    fail "invalid flag did not report an error on stderr"
grep -q 'error:' "$LAUNCH_LOG" ||
    fail "launcher did not tee the child's stderr into $LAUNCH_LOG"

# The AppImage's AppRun expects the binary and wrapper under usr/bin. Recreate
# that layout from the extracted payload and run the packaged launcher, which
# covers the AppImage dispatch without mounting an image or opening a window.
APP_DIR="$TMP/appdir"
mkdir -p "$APP_DIR/usr/bin"
install -m0755 "$ROOT/packaging/native/AppRun" "$APP_DIR/AppRun"
install -m0755 "$TMP/usr/lib/vmodv-native/modv" "$APP_DIR/usr/bin/modv"
install -m0755 "$TMP/usr/lib/vmodv-native/log-run.sh" \
    "$APP_DIR/usr/bin/log-run.sh"
APP_DATA="$TMP/apprun-data"
MODV_USER_DATA="$APP_DATA" "$APP_DIR/AppRun" --help >/dev/null
APP_LOG="$APP_DATA/logs/vmodv-$(date +%F).log"
[ -f "$APP_LOG" ] || fail "AppRun did not create $APP_LOG"
grep -q 'native-launch' "$APP_LOG" ||
    fail "AppRun did not go through log-run.sh"

if [ -f "$ROOT/tools/native-log-run-check.sh" ]; then
    ROOT="$ROOT" bash "$ROOT/tools/native-log-run-check.sh"
fi

echo "native-deb-check: OK $DEB"
echo "  isf files: $isf_count, textures: $texture_count, installed: ${INSTALLED_SIZE} KiB"
echo "  packaged binary: $BINARY_VERSION"
