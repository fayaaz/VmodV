#!/usr/bin/env bash
# Stage a portable macOS app bundle for the native `modv` binary.
#
# The staged layout is a real .app bundle plus a shell launcher:
#
#   <root>/VmodV.app/Contents/MacOS/modv              the executable
#   <root>/VmodV.app/Contents/Info.plist              bundle metadata
#   <root>/VmodV.app/Contents/Resources/isf/          ISF corpus (*.fs / *.vs)
#   <root>/VmodV.app/Contents/Resources/textures/     Point10 texture corpus
#   <root>/VmodV.command                              launcher that exports the env vars
#
# `VmodV.command` sets MODV_PORTABLE_ROOT to its own directory, then points
# MODV_ISF_DIR, MODV_ASSETS_DIR, GST_PLUGIN_PATH and DYLD_FALLBACK_LIBRARY_PATH
# at the bundle and a Homebrew GStreamer install before launching the binary.
# Nothing is hard-coded to an install location.
#
# GStreamer is a HOST prerequisite on macOS: it is installed with Homebrew
# (`brew install gstreamer gst-plugins-base`), not bundled here. The launcher
# discovers the Homebrew prefix at runtime. Camera / screen / system-audio
# capture relies on v4l2src / pipewiresrc and remains Linux-only at runtime.
#
# Modes
#   real (default)   Used by the macOS CI job. Defaults:
#                      --binary      <project-root>/crates/target/release/modv
#                      --root        <project-root>/dist/macos-portable
#                      --project-root <directory containing this script's parent>
#   --dry-run        Used by the Linux integration test. Takes an explicit fake
#                    binary and the real project root so the whole layout can be
#                    verified without a macOS runner.
#
# Usage:
#   tools/macos-portable.sh [--dry-run] [--root DIR] [--binary FILE] \
#       [--gstreamer-root DIR] [--project-root DIR]
set -euo pipefail

DRY_RUN=0
ROOT=""
BINARY=""
GST_ROOT=""
PROJECT_ROOT=""

usage() {
    cat <<'EOF'
Usage: tools/macos-portable.sh [options]
  --dry-run              stage from explicit fake inputs (Linux-testable)
  --root DIR             staging root (default: <project-root>/dist/macos-portable)
  --binary FILE          path to the `modv` executable
  --gstreamer-root DIR   accepted for CLI parity; GStreamer is a Homebrew host prerequisite
  --project-root DIR     repository root (defaults to this script's parent dir)
  -h, --help             show this help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run)         DRY_RUN=1; shift ;;
        --root)            ROOT="${2:?--root needs a value}"; shift 2 ;;
        --binary)          BINARY="${2:?--binary needs a value}"; shift 2 ;;
        --gstreamer-root)  GST_ROOT="${2:?--gstreamer-root needs a value}"; shift 2 ;;
        --project-root)    PROJECT_ROOT="${2:?--project-root needs a value}"; shift 2 ;;
        -h|--help)         usage; exit 0 ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

# Resolve the repository root from this script's location when not supplied.
if [ -z "$PROJECT_ROOT" ]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
fi

if [ -z "$ROOT" ]; then
    ROOT="$PROJECT_ROOT/dist/macos-portable"
fi

if [ -z "$BINARY" ]; then
    BINARY="$PROJECT_ROOT/crates/target/release/modv"
fi

fail() {
    echo "macos-portable: $*" >&2
    exit 1
}

[ -f "$BINARY" ] || fail "binary not found: $BINARY"
[ -d "$PROJECT_ROOT/resources/isf" ] || fail "missing ISF corpus at $PROJECT_ROOT/resources/isf"
[ -d "$PROJECT_ROOT/resources/point10/textures" ] || \
    fail "missing textures at $PROJECT_ROOT/resources/point10/textures"

APP="$ROOT/VmodV.app"
CONTENTS="$APP/Contents"
MACOS_DIR="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"

# Start from a clean staging root so stale files from a previous run never leak
# into the bundle/ZIP.
rm -rf "$ROOT"
mkdir -p "$MACOS_DIR" "$RESOURCES"

# The executable.
cp -f "$BINARY" "$MACOS_DIR/modv"
chmod +x "$MACOS_DIR/modv"

# Application assets. `cp -a` preserves the directory trees verbatim.
cp -a "$PROJECT_ROOT/resources/isf" "$RESOURCES/isf"
cp -a "$PROJECT_ROOT/resources/point10/textures" "$RESOURCES/textures"

# Bundle metadata. Escape the version so a hostile APP_VERSION cannot break the
# XML. `CFBundleExecutable` must match the staged binary name exactly.
version="${APP_VERSION:-0.0.0-dev}"
esc_version=$(printf '%s' "$version" | sed \
    -e 's/&/\&amp;/g' \
    -e 's/</\&lt;/g' \
    -e 's/>/\&gt;/g' \
    -e 's/"/\&quot;/g' \
    -e "s/'/\&apos;/g")

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>modv</string>
    <key>CFBundleIdentifier</key>
    <string>com.fayaaz.vmodv</string>
    <key>CFBundleName</key>
    <string>VmodV</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>${esc_version}</string>
</dict>
</plist>
PLIST

# The launcher. `BASH_SOURCE[0]` is this file's own path, so every path stays
# relative to wherever the bundle was extracted. GStreamer plugins/libraries
# are discovered from a Homebrew install (Apple Silicon / Intel prefixes).
cat > "$ROOT/VmodV.command" <<'LAUNCHER'
#!/usr/bin/env bash
# Portable launcher for VmodV. All paths are relative to this file so the
# extracted bundle works from any directory.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

export MODV_PORTABLE_ROOT="$HERE"
export MODV_ISF_DIR="$HERE/VmodV.app/Contents/Resources/isf"
export MODV_ASSETS_DIR="$HERE/VmodV.app/Contents/Resources/textures"

# GStreamer is a Homebrew host prerequisite. Prefer the Apple Silicon prefix,
# then fall back to the Intel one.
BREW_PREFIX=""
if [ -d /opt/homebrew/lib/gstreamer-1.0 ]; then
    BREW_PREFIX="/opt/homebrew"
elif [ -d /usr/local/lib/gstreamer-1.0 ]; then
    BREW_PREFIX="/usr/local"
fi
if [ -n "$BREW_PREFIX" ]; then
    export GST_PLUGIN_PATH="${GST_PLUGIN_PATH:+$GST_PLUGIN_PATH:}$BREW_PREFIX/lib/gstreamer-1.0"
    export DYLD_FALLBACK_LIBRARY_PATH="${DYLD_FALLBACK_LIBRARY_PATH:+$DYLD_FALLBACK_LIBRARY_PATH:}$BREW_PREFIX/lib"
fi

exec "$HERE/VmodV.app/Contents/MacOS/modv" "$@"
LAUNCHER
chmod +x "$ROOT/VmodV.command"

echo "Staged portable macOS layout at $ROOT"
if [ "$DRY_RUN" -eq 1 ]; then
    echo "dry-run complete"
fi
