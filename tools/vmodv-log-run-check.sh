#!/usr/bin/env bash
# Behavioural checks for packaging/vmodv/log-run.sh.
#
# tools/vmodv-deb-check.sh calls this so every `make vmodv-deb` exercises the
# launcher wrapper, and `make vmodv-deb-check` re-runs it on its own. The test
# uses a stub command rather than the real binary: that keeps it independent of
# GPUs, windows and the Rust build while still checking the stderr-only tee,
# exit-code propagation, the dated log name and rotation. It also assembles an
# AppDir-shaped tree around the packaged AppRun so the AppImage launcher is
# checked without mounting or opening an image.
set -euo pipefail

ROOT="${ROOT:-/app}"
if [ ! -d "$ROOT/packaging/vmodv" ]; then
    SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
fi

LOG_RUN="$ROOT/packaging/vmodv/log-run.sh"
APPRUN="$ROOT/packaging/vmodv/AppRun"
APPIMAGE_SCRIPT="$ROOT/tools/vmodv-appimage.sh"
TODAY=$(date +%F)

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail() {
    echo "vmodv-log-run-check: $*" >&2
    exit 1
}

[ -f "$LOG_RUN" ] || fail "missing $LOG_RUN"
[ -x "$LOG_RUN" ] || fail "$LOG_RUN is not executable"
[ -f "$APPRUN" ] || fail "missing $APPRUN"
grep -q 'MODV_ISF_DIR' "$APPRUN" ||
    fail "AppRun no longer exports MODV_ISF_DIR"
grep -q 'MODV_ASSETS_DIR' "$APPRUN" ||
    fail "AppRun no longer exports MODV_ASSETS_DIR"

STUB="$TMP/stub-modv"
cat >"$STUB" <<'STUB'
#!/bin/sh
printf 'stdout-line\n'
printf 'stderr-line\n' >&2
exit "${STUB_EXIT:-0}"
STUB
chmod +x "$STUB"

assert_log() {
    local log=$1 expected=$2
    [ -f "$log" ] || fail "missing log file $log"
    grep -qF -- "$expected" "$log" ||
        fail "$log does not contain '$expected'"
    if grep -qF -- 'stdout-line' "$log"; then
        fail "$log captured stdout (it must only tee stderr)"
    fi
    grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}[+-][0-9]{4} .*pid=[0-9]+' "$log" ||
        fail "$log has no ISO-timestamp/pid launch header"
}

# 1. MODV_USER_DATA wins; stderr is tee'd, stdout stays untouched, the child's
# exit code comes back, and a stale dated file is pruned while today's is kept.
DATA="$TMP/user-data"
mkdir -p "$DATA/logs"
: >"$DATA/logs/vmodv-2000-01-01.log"

set +e
MODV_USER_DATA="$DATA" STUB_EXIT=7 \
    "$LOG_RUN" "$STUB" alpha "two words" >"$TMP/run.out" 2>"$TMP/run.err"
STATUS=$?
set -e

[ "$STATUS" -eq 7 ] || fail "log-run returned $STATUS, expected the child's 7"
[ "$(cat "$TMP/run.out")" = "stdout-line" ] ||
    fail "log-run changed stdout: $(cat "$TMP/run.out")"
grep -qF -- 'stderr-line' "$TMP/run.err" ||
    fail "stderr was not still visible on the terminal"
LOG="$DATA/logs/vmodv-$TODAY.log"
assert_log "$LOG" 'stderr-line'
grep -qF -- 'alpha' "$LOG" || fail "launch header is missing argv"
[ -e "$DATA/logs/vmodv-2000-01-01.log" ] &&
    fail "stale vmodv-2000-01-01.log was not pruned"
[ -f "$LOG" ] || fail "today's log was pruned"

# 1b. A desktop launch can leave stderr closed. The wrapper must still log the
# child's stderr (and not let tee die on a closed fd and SIGPIPE the app).
CLOSED_DATA="$TMP/closed-data"
set +e
MODV_USER_DATA="$CLOSED_DATA" STUB_EXIT=3 \
    "$LOG_RUN" "$STUB" >"$TMP/closed.out" 2>&-
STATUS=$?
set -e
[ "$STATUS" -eq 3 ] || fail "closed-stderr run returned $STATUS, expected 3"
[ "$(cat "$TMP/closed.out")" = "stdout-line" ] ||
    fail "closed-stderr run changed stdout"
assert_log "$CLOSED_DATA/logs/vmodv-$TODAY.log" 'stderr-line'

# 2. Without MODV_USER_DATA the log lives under $XDG_CONFIG_HOME/VmodV, and
# without XDG_CONFIG_HOME under $HOME/.config/VmodV, matching user_data_dir().
XDG="$TMP/xdg"
mkdir -p "$XDG"
(
    unset MODV_USER_DATA
    HOME="$TMP/home"
    XDG_CONFIG_HOME="$XDG"
    export HOME XDG_CONFIG_HOME
    "$LOG_RUN" "$STUB" >"$TMP/xdg.out" 2>"$TMP/xdg.err"
)
assert_log "$XDG/VmodV/logs/vmodv-$TODAY.log" 'stderr-line'

HOME_ONLY="$TMP/home-only"
mkdir -p "$HOME_ONLY"
(
    unset MODV_USER_DATA XDG_CONFIG_HOME
    HOME="$HOME_ONLY"
    export HOME
    "$LOG_RUN" "$STUB" >"$TMP/home.out" 2>"$TMP/home.err"
)
assert_log "$HOME_ONLY/.config/VmodV/logs/vmodv-$TODAY.log" 'stderr-line'

# 3. No bash on PATH: fail soft. The child still runs, stderr is untouched and
# a warning explains why no log was written.
set +e
PATH=/nonexistent MODV_USER_DATA="$TMP/no-bash" STUB_EXIT=7 \
    "$LOG_RUN" "$STUB" >"$TMP/no-bash.out" 2>"$TMP/no-bash.err"
STATUS=$?
set -e
[ "$STATUS" -eq 7 ] || fail "bash-less fallback returned $STATUS, expected 7"
[ "$(cat "$TMP/no-bash.out")" = "stdout-line" ] ||
    fail "bash-less fallback changed stdout"
grep -qF -- 'stderr-line' "$TMP/no-bash.err" ||
    fail "bash-less fallback dropped stderr"
grep -qi 'bash' "$TMP/no-bash.err" ||
    fail "bash-less fallback did not warn about logging"
[ ! -d "$TMP/no-bash" ] ||
    fail "bash-less fallback created a log directory"

# 4. AppRun goes through the wrapper. Assemble the AppImage's usr/bin layout
# (AppRun + log-run.sh + the binary) and run it, which is the same dispatch the
# packaged image performs without mounting or opening a window.
APP_DATA="$TMP/app-data"
APP_DIR="$TMP/VmodV.AppDir"
mkdir -p "$APP_DIR/usr/bin"
cp "$APPRUN" "$APP_DIR/AppRun"
cp "$LOG_RUN" "$APP_DIR/usr/bin/log-run.sh"
cp "$STUB" "$APP_DIR/usr/bin/modv"
chmod +x "$APP_DIR/AppRun" "$APP_DIR/usr/bin/log-run.sh" "$APP_DIR/usr/bin/modv"

set +e
MODV_USER_DATA="$APP_DATA" STUB_EXIT=9 \
    "$APP_DIR/AppRun" --smoke >"$TMP/app.out" 2>"$TMP/app.err"
STATUS=$?
set -e
[ "$STATUS" -eq 9 ] || fail "AppRun returned $STATUS, expected 9"
assert_log "$APP_DATA/logs/vmodv-$TODAY.log" 'stderr-line'
grep -qF -- '--smoke' "$APP_DATA/logs/vmodv-$TODAY.log" ||
    fail "AppRun did not pass its arguments through the wrapper"

grep -q 'log-run.sh' "$APPIMAGE_SCRIPT" ||
    fail "tools/vmodv-appimage.sh does not install log-run.sh"

echo "vmodv-log-run-check: OK ($LOG_RUN)"
