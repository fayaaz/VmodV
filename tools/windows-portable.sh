#!/usr/bin/env bash
# Stage a portable Windows ZIP layout for the native `modv` binary.
#
# The extracted layout is self-contained and works from any directory:
#
#   <root>/modv.exe
#   <root>/isf/                         ISF corpus (*.fs / *.vs)
#   <root>/textures/                    Point10 texture corpus
#   <root>/gstreamer/bin/               GStreamer runtime DLLs
#   <root>/gstreamer/lib/gstreamer-1.0/ GStreamer plugins
#   <root>/VmodV.cmd                    launcher that exports the env vars
#
# `VmodV.cmd` sets MODV_PORTABLE_ROOT to its own directory, then points
# MODV_ISF_DIR, MODV_ASSETS_DIR and GST_PLUGIN_PATH inside it before launching
# modv.exe. Nothing is hard-coded to an install location.
#
# Modes
#   real (default)   Used by the Windows CI job. Defaults:
#                      --binary     <project-root>/crates/target/x86_64-pc-windows-msvc/release/modv.exe
#                      --gstreamer-root $GSTREAMER_ROOT or C:/gstreamer/1.0/msvc_x86_64
#                      --project-root  <directory containing this script's parent>
#                      --root          <project-root>/dist/windows-portable
#   --dry-run        Used by the Linux integration test. Takes explicit fake
#                    inputs (a stub binary and a stub GStreamer tree) and copies
#                    them into the real resources, so the whole layout can be
#                    verified without a Windows runner.
#
# Usage:
#   tools/windows-portable.sh [--dry-run] [--root DIR] [--binary FILE] \
#       [--gstreamer-root DIR] [--project-root DIR]
#
# Camera / screen capture relies on v4l2src / pipewiresrc and remains Linux-only
# at runtime; those pipelines compile on Windows but have no capture sources
# there. This packaging increment does not port camera/screen capture.
set -euo pipefail

DRY_RUN=0
ROOT=""
BINARY=""
GST_ROOT=""
PROJECT_ROOT=""

usage() {
    cat <<'EOF'
Usage: tools/windows-portable.sh [options]
  --dry-run              stage from explicit fake inputs (Linux-testable)
  --root DIR             staging root (default: <project-root>/dist/windows-portable)
  --binary FILE          path to modv.exe
  --gstreamer-root DIR   GStreamer 1.x MSVC install root
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
    ROOT="$PROJECT_ROOT/dist/windows-portable"
fi

if [ -z "$BINARY" ]; then
    BINARY="$PROJECT_ROOT/crates/target/x86_64-pc-windows-msvc/release/modv.exe"
fi

if [ -z "$GST_ROOT" ]; then
    GST_ROOT="${GSTREAMER_ROOT:-C:/gstreamer/1.0/msvc_x86_64}"
fi

fail() {
    echo "windows-portable: $*" >&2
    exit 1
}

[ -f "$BINARY" ] || fail "binary not found: $BINARY"
[ -d "$PROJECT_ROOT/resources/isf" ] || fail "missing ISF corpus at $PROJECT_ROOT/resources/isf"
[ -d "$PROJECT_ROOT/resources/point10/textures" ] || \
    fail "missing textures at $PROJECT_ROOT/resources/point10/textures"
[ -d "$GST_ROOT/bin" ] || fail "missing GStreamer bin directory at $GST_ROOT/bin"
[ -d "$GST_ROOT/lib/gstreamer-1.0" ] || \
    fail "missing GStreamer plugin directory at $GST_ROOT/lib/gstreamer-1.0"

# Start from a clean staging root so stale files from a previous run never leak
# into the ZIP.
rm -rf "$ROOT"
mkdir -p "$ROOT"

# The executable.
cp -f "$BINARY" "$ROOT/modv.exe"

# Application assets. `cp -a` preserves the directory trees verbatim.
cp -a "$PROJECT_ROOT/resources/isf" "$ROOT/isf"
cp -a "$PROJECT_ROOT/resources/point10/textures" "$ROOT/textures"

# GStreamer runtime + plugins.
mkdir -p "$ROOT/gstreamer/lib"
cp -a "$GST_ROOT/bin" "$ROOT/gstreamer/bin"
cp -a "$GST_ROOT/lib/gstreamer-1.0" "$ROOT/gstreamer/lib/gstreamer-1.0"

# The launcher. `%~dp0` is the launcher's own directory with a trailing
# backslash, so every path stays relative to wherever the ZIP was extracted.
# Write with a CRLF-free here-doc; Windows accepts LF batch files.
cat > "$ROOT/VmodV.cmd" <<'LAUNCHER'
@echo off
rem Portable launcher for VmodV. All paths are relative to this file so the
rem extracted ZIP works from any directory.
set "MODV_PORTABLE_ROOT=%~dp0"
set "MODV_ISF_DIR=%MODV_PORTABLE_ROOT%isf"
set "MODV_ASSETS_DIR=%MODV_PORTABLE_ROOT%textures"
set "GST_PLUGIN_PATH=%MODV_PORTABLE_ROOT%gstreamer\lib\gstreamer-1.0"
set "PATH=%MODV_PORTABLE_ROOT%gstreamer\bin;%PATH%"
start "" "%MODV_PORTABLE_ROOT%modv.exe" %*
LAUNCHER

echo "Staged portable Windows layout at $ROOT"
if [ "$DRY_RUN" -eq 1 ]; then
    echo "dry-run complete"
fi
