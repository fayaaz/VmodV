#!/usr/bin/env bash
# Verify a "fat" VmodV AppImage / AppDir (slug: appimage-ci-check).
#
# Usage: bash tools/vmodv-appimage-check.sh <path>
#
#   <path> may be an already-assembled AppDir (checked in place) or a built
#   *.AppImage (extracted with --appimage-extract, no FUSE needed, then checked
#   exactly like an AppDir).
#
# This is the gate that CI (vmodv.yml / release.yml) and the canonical local
# build (make build -> make appimage-vmodv) run on the bundle they produce. It
# is deliberately content-based: it fails on the FIRST broken assertion, naming
# the missing behaviour, and prints a readable size/content summary. No size
# budget is asserted — a fat bundle is expected to be large.
set -euo pipefail

fail() {
    echo "vmodv-appimage-check: $*" >&2
    exit 1
}

if [ "$#" -ne 1 ]; then
    echo "usage: bash tools/vmodv-appimage-check.sh <AppDir|*.AppImage>" >&2
    exit 2
fi

INPUT="$1"
[ -e "$INPUT" ] || fail "$INPUT does not exist: there is no AppImage/AppDir to verify"

# Resolve the AppDir to check. An AppImage is extracted into a temp dir.
TMPDIR_EXTRACT=""
cleanup() {
    if [ -n "$TMPDIR_EXTRACT" ]; then
        rm -rf -- "$TMPDIR_EXTRACT"
    fi
}
trap cleanup EXIT

case "$INPUT" in
    *.AppImage)
        TMPDIR_EXTRACT="$(mktemp -d)"
        (
            cd "$TMPDIR_EXTRACT"
            "$INPUT" --appimage-extract >/dev/null
        ) || fail "$INPUT: --appimage-extract failed: cannot inspect this AppImage"
        APPDIR="$TMPDIR_EXTRACT/squashfs-root"
        [ -d "$APPDIR" ] || fail "$INPUT: --appimage-extract produced no squashfs-root AppDir"
        IMAGE_BYTES="$(stat -c %s "$INPUT" 2>/dev/null || echo 0)"
        SIZE_NOTE="(AppImage: $(basename -- "$INPUT"))"
        ;;
    *)
        [ -d "$INPUT" ] || fail "$INPUT is neither a directory nor a *.AppImage"
        APPDIR="$INPUT"
        IMAGE_BYTES=""
        SIZE_NOTE="(AppDir payload)"
        ;;
esac

# --- 1. launcher + main binary are present and executable -------------------
[ -f "$APPDIR/usr/bin/modv" ] || fail "usr/bin/modv is missing: the AppImage has no application binary"
[ -x "$APPDIR/usr/bin/modv" ] || fail "usr/bin/modv is not executable: the AppImage cannot launch"
[ -f "$APPDIR/AppRun" ] || fail "AppRun is missing: the AppImage has no launcher"
[ -x "$APPDIR/AppRun" ] || fail "AppRun is not executable: the AppImage cannot launch"

# --- 2. the bundled GStreamer plugin set (the documented floor of 100) ------
PLUGIN_DIR="$APPDIR/usr/lib/gstreamer-1.0"
[ -d "$PLUGIN_DIR" ] || fail "usr/lib/gstreamer-1.0 plugin directory is missing: camera/file/screen pipelines would be empty"
PLUGIN_COUNT="$(find "$PLUGIN_DIR" -maxdepth 1 -type f -name '*.so' | wc -l)"
[ "$PLUGIN_COUNT" -ge 100 ] || fail "only $PLUGIN_COUNT GStreamer plugin *.so files under usr/lib/gstreamer-1.0 (need at least 100): the bundle is thin"

# --- 3. the flattened plugin scanner ----------------------------------------
SCANNER="$APPDIR/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
[ -f "$SCANNER" ] || fail "usr/libexec/gstreamer-1.0/gst-plugin-scanner is missing: GStreamer cannot scan the bundled plugins"
[ -x "$SCANNER" ] || fail "usr/libexec/gstreamer-1.0/gst-plugin-scanner is not executable"

# --- 4. GStreamer core + at least one companion library ---------------------
LIB_DIR="$APPDIR/usr/lib"
find "$LIB_DIR" -maxdepth 1 -type f -name 'libgstreamer-1.0.so*' -print -quit | grep -q . || \
    fail "usr/lib/libgstreamer-1.0.so* is missing: the bundled plugins cannot link against GStreamer core"
core_companion=0
for candidate in libgstbase-1.0.so libgstvideo-1.0.so libgstaudio-1.0.so libgstpbutils-1.0.so; do
    if find "$LIB_DIR" -maxdepth 1 -type f -name "$candidate*" -print -quit | grep -q .; then
        core_companion=1
        break
    fi
done
[ "$core_companion" -eq 1 ] || fail "no libgstbase-1.0.so*/libgstvideo-1.0.so*/libgstaudio-1.0.so*/libgstpbutils-1.0.so* under usr/lib: the GStreamer core is unusable"

# --- 5. machine-readable bundle manifest ------------------------------------
MANIFEST="$APPDIR/usr/share/vmodv/gstreamer-bundle.json"
[ -f "$MANIFEST" ] || fail "usr/share/vmodv/gstreamer-bundle.json is missing: the bundled package versions cannot be audited"
if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); p=d.get("packages") if isinstance(d,dict) else None; assert isinstance(p,list) and len(p)>=1 and all(isinstance(e,dict) and e.get("name") and e.get("version") for e in p)' "$MANIFEST" >/dev/null 2>&1 || \
        fail "usr/share/vmodv/gstreamer-bundle.json is not parseable JSON with at least one package/version entry"
elif command -v jq >/dev/null 2>&1; then
    jq -e '.packages | length >= 1' "$MANIFEST" >/dev/null 2>&1 || \
        fail "usr/share/vmodv/gstreamer-bundle.json is not parseable JSON with at least one package/version entry"
else
    grep -q '"packages"' "$MANIFEST" || fail "usr/share/vmodv/gstreamer-bundle.json has no packages array"
    grep -q '"name"' "$MANIFEST" || fail "usr/share/vmodv/gstreamer-bundle.json has no package/version entry"
fi

# --- 6. at least one licence text -------------------------------------------
LICENCE_COUNT="$(find "$APPDIR/usr/share/doc" -type f -name copyright 2>/dev/null | wc -l)"
[ "$LICENCE_COUNT" -ge 1 ] || fail "no usr/share/doc/<package>/copyright licence text: redistributing libav/ffmpeg needs its licence shipped"

# --- 7..9. forbidden files: glibc family, host display stack, dev artifacts -
# Restrict the sweep to usr/lib (which includes the gstreamer-1.0 plugin dir):
# that is where a fat bundle stages binaries, and the negative cases plant
# their decoys there.
check_forbidden() {
    local label="$1" pattern="$2" hit
    while IFS= read -r hit; do
        [ -n "$hit" ] || continue
        fail "bundled $label file ${hit#$APPDIR/} (matched $pattern): it must stay host-side"
    done < <(find "$LIB_DIR" \( -type f -o -type l \) -name "$pattern" -print)
}

# glibc family — an AppImage must run against the host's own glibc. Match the
# sonames, not every name starting with e.g. "libpthread": libpthreadpool.so.0
# and librtmp.so.1 are unrelated userspace libraries a real closure pulls in.
check_forbidden "glibc-family" 'libc.so*'
check_forbidden "glibc-family" 'libm.so*'
check_forbidden "glibc-family" 'libdl.so*'
check_forbidden "glibc-family" 'libpthread.so*'
check_forbidden "glibc-family" 'librt.so*'
check_forbidden "glibc-family" 'ld-linux*'
check_forbidden "glibc-family" 'libresolv.so*'
check_forbidden "glibc-family" 'libmvec.so*'
check_forbidden "glibc-family" 'libnsl.so*'
check_forbidden "glibc-family" 'libutil.so*'
check_forbidden "glibc-family" 'libcrypt.so.1*'
check_forbidden "glibc-family" 'libanl.so*'
check_forbidden "glibc-family" 'libthread_db.so*'
# No licence text for libc6 itself either.
if [ -f "$APPDIR/usr/share/doc/libc6/copyright" ]; then
    fail "bundled libc6 licence text (usr/share/doc/libc6/copyright): libc6 belongs to the host"
fi

# Host graphics/display stack: wgpu must talk to the user's own driver, and
# AppRun puts usr/lib first on LD_LIBRARY_PATH.
for pattern in 'libGL.so*' 'libGLX*' 'libGLdispatch*' 'libOpenGL*' 'libEGL*' 'libGLES*' 'libglapi*' 'libgbm*' 'libvulkan*' 'libwayland*'; do
    check_forbidden "host display-stack" "$pattern"
done

# Development-only artifacts never belong in a runtime bundle.
for pattern in '*.a' '*.la' '*.pc' '*.pdb'; do
    check_forbidden "development-only" "$pattern"
done

# --- 10. every sampled ELF must carry a $ORIGIN-relative RUNPATH -------------
check_runpath() {
    local file="$1" info line value part
    [ -f "$file" ] || fail "$file is missing, so its RUNPATH cannot be verified"
    command -v readelf >/dev/null 2>&1 || \
        fail "readelf is unavailable: cannot verify the RUNPATH of ${file#$APPDIR/} (fail closed)"
    info="$(readelf -d "$file" 2>/dev/null)" || info=""
    [ -n "$info" ] || \
        fail "readelf shows no dynamic section for ${file#$APPDIR/}: its RUNPATH cannot be verified (fail closed)"
    line="$(printf '%s\n' "$info" | grep -m1 -E '\((RUNPATH|RPATH)\)' || true)"
    [ -n "$line" ] || \
        fail "${file#$APPDIR/} has no RUNPATH: it would resolve GStreamer against the host, not itself"
    printf '%s\n' "$line" | grep -q '\$ORIGIN' || \
        fail "${file#$APPDIR/} RUNPATH is not \$ORIGIN-relative: $line"
    value="$(printf '%s\n' "$line" | sed -n 's/.*\[\(.*\)\].*/\1/p')"
    [ -n "$value" ] || \
        fail "${file#$APPDIR/} RUNPATH has no readable value: $line"
    local IFS=:
    for part in $value; do
        case "$part" in
            '$ORIGIN'*) ;;
            *) fail "${file#$APPDIR/} RUNPATH component '$part' is absolute, not \$ORIGIN-relative" ;;
        esac
    done
}

check_runpath "$APPDIR/usr/bin/modv"
check_runpath "$SCANNER"

sampled=0
while IFS= read -r lib; do
    check_runpath "$lib"
    sampled=$((sampled + 1))
    [ "$sampled" -ge 5 ] && break
done < <(find "$LIB_DIR" -maxdepth 1 -type f \( -name '*.so' -o -name '*.so.*' \) | sort)
[ "$sampled" -gt 0 ] || fail "no runtime libraries under usr/lib to verify a RUNPATH on (fail closed)"

sampled=0
while IFS= read -r plugin; do
    check_runpath "$plugin"
    sampled=$((sampled + 1))
    [ "$sampled" -ge 5 ] && break
done < <(find "$PLUGIN_DIR" -maxdepth 1 -type f -name '*.so' | sort)
[ "$sampled" -gt 0 ] || fail "no GStreamer plugins under usr/lib/gstreamer-1.0 to verify a RUNPATH on (fail closed)"

# --- summary (content + size, no budget asserted) ---------------------------
LIB_COUNT="$(find "$LIB_DIR" -maxdepth 1 -type f \( -name '*.so' -o -name '*.so.*' \) | wc -l)"
PAYLOAD_BYTES="$(find "$APPDIR" -type f -printf '%s\n' | awk '{ total += $1 } END { print total + 0 }')"
TOTAL_BYTES="${IMAGE_BYTES:-$PAYLOAD_BYTES}"

echo "appimage-check: fat GStreamer bundle verified at $APPDIR"
echo "plugins: $PLUGIN_COUNT GStreamer plugin *.so files under usr/lib/gstreamer-1.0"
echo "libraries: $LIB_COUNT runtime shared objects under usr/lib"
echo "licences: $LICENCE_COUNT package copyright texts under usr/share/doc"
echo "bytes: $TOTAL_BYTES $SIZE_NOTE (payload $PAYLOAD_BYTES bytes)"
