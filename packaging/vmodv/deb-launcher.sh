#!/bin/sh
# Debian launcher for the VmodV shell. The AppImage's AppRun exports the same
# defaults relative to its mount point; this resolves the `/usr` prefix from
# its own path so the installed launcher uses /usr/share/vmodv and an extracted
# package tree works without root. An explicit --isf-dir or user environment
# still wins (the engine checks --isf-dir, then MODV_ISF_DIR).
set -eu
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PREFIX=$(CDPATH= cd -- "$HERE/.." && pwd)

BIN="$PREFIX/lib/vmodv/modv"
[ -x "$BIN" ] || BIN=/usr/lib/vmodv/modv
LOG_RUN="$PREFIX/lib/vmodv/log-run.sh"
[ -x "$LOG_RUN" ] || LOG_RUN=/usr/lib/vmodv/log-run.sh

: "${MODV_ISF_DIR:=$PREFIX/share/vmodv/isf}"
: "${MODV_ASSETS_DIR:=$PREFIX/share/vmodv/textures}"
export MODV_ISF_DIR MODV_ASSETS_DIR
if [ -x "$LOG_RUN" ]; then
    exec "$LOG_RUN" "$BIN" "$@"
fi
exec "$BIN" "$@"
