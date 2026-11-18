#!/usr/bin/env bash
# Packaged-app smoke suite for the native Rust 2.0 shell.
#
# Usage:
#   tools/native-smoke.sh [options]
#
#   --appimage PATH       native AppImage to exercise (otherwise the newest
#                         dist/VmodV-Native-*.AppImage is used when present)
#   --scene NAME          default Scene applied by the windowed checks
#                         (default: "Mycelium Bloom")
#   --frames N            headless benchmark frame count (default: 600)
#   --soak-seconds N      additionally run the windowed app for N seconds and
#                         report fps + peak RSS (default: 0 = off; the full
#                         2 h soak is deliberately not part of this script)
#   --skip-windowed       headless checks only (no display required)
#   --keep                keep the temporary userData/log tree on exit
#   -h, --help            show this help
#
# Environment:
#   MODV_SMOKE_APPIMAGE   same as --appimage
#   MODV_SMOKE_BIN        native `modv` binary to use instead of an AppImage
#                         (default fallback: crates/target/release/modv)
#   MODV_SMOKE_SCENE      same as --scene
#   MODV_SMOKE_FRAMES     same as --frames
#   MODV_SMOKE_SOAK_SECONDS  same as --soak-seconds
#   MODV_SMOKE_TIMEOUT    per-command timeout in seconds (default: 300)
#   MODV_SMOKE_KEEP       set to 1 for --keep
#
# What it verifies (all against a throwaway MODV_USER_DATA, never the real
# ~/.config/VmodV):
#
#   a. `--headless --frames N --bench` exits 0, prints one JSON object, and
#      the frame count, fps/frame-time and RSS fields are sane.
#   b. windowed `--frames 20 --apply-scene <default>` exits 0 on the host
#      display and logs the applied Scene and final frame count. Skipped with
#      a message when no Wayland/X11 display is available.
#   c. state round-trip: the first windowed run writes `engine-state.json`;
#      a second run restores the same Group/Module counts.
#   d. log wrapper: the packaged AppRun/log-run.sh writes
#      `<userData>/logs/vmodv-YYYY-MM-DD.log` with the launch header while
#      stdout stays machine-readable.
#   e. crash recovery: SIGKILL a running instance (before the 30 s periodic
#      save), restart, and confirm the last exit snapshot is restored without
#      a crash. A clean "starting empty" report is accepted on a fresh tree.
#
# The script never builds anything. When no native AppImage is present it
# assembles the same AppDir layout the `appimage-native` target produces
# (AppRun + log-run.sh + the release binary + the ISF/texture trees) around
# crates/target/release/modv. Build that binary with `make native`/`make
# native-debug` first, or pass --appimage.
#
# Examples:
#   tools/native-smoke.sh
#   tools/native-smoke.sh --frames 600 --soak-seconds 60
#   tools/native-smoke.sh --appimage dist/VmodV-Native-2.0.0-x86_64.AppImage

set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=${ROOT:-$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)}

SCENE=${MODV_SMOKE_SCENE:-"Mycelium Bloom"}
HEADLESS_FRAMES=${MODV_SMOKE_FRAMES:-600}
SOAK_SECONDS=${MODV_SMOKE_SOAK_SECONDS:-0}
TIMEOUT=${MODV_SMOKE_TIMEOUT:-300}
KEEP=${MODV_SMOKE_KEEP:-0}
APPIMAGE=${MODV_SMOKE_APPIMAGE:-}
BIN_OVERRIDE=${MODV_SMOKE_BIN:-}
SKIP_WINDOWED=0
WINDOWED_FRAMES=20
CRASH_LEADER=""
SOAK_LEADER=""
WATCHDOG=""

usage() {
    sed -n '2,/^set -euo pipefail$/p' "$0" |
        sed -e '/^set -euo pipefail$/d' -e 's/^# \{0,1\}//'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --appimage)
            APPIMAGE=${2:?--appimage needs a path}
            shift 2
            ;;
        --scene)
            SCENE=${2:?--scene needs a name}
            shift 2
            ;;
        --frames)
            HEADLESS_FRAMES=${2:?--frames needs a number}
            shift 2
            ;;
        --soak-seconds)
            SOAK_SECONDS=${2:?--soak-seconds needs a number}
            shift 2
            ;;
        --skip-windowed)
            SKIP_WINDOWED=1
            shift
            ;;
        --keep)
            KEEP=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "native-smoke: unknown argument $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

case "$HEADLESS_FRAMES" in
    '' | *[!0-9]* | 0)
        echo "native-smoke: --frames must be a positive integer" >&2
        exit 2
        ;;
esac
case "$SOAK_SECONDS" in
    '' | *[!0-9]*)
        echo "native-smoke: --soak-seconds must be an integer" >&2
        exit 2
        ;;
esac

TMP=$(mktemp -d "${TMPDIR:-/tmp}/modv-native-smoke.XXXXXX")
cleanup() {
    if [ -n "$WATCHDOG" ]; then
        kill "$WATCHDOG" 2>/dev/null || true
    fi
    if [ -n "$CRASH_LEADER" ]; then
        kill -KILL -- "-$CRASH_LEADER" 2>/dev/null || true
    fi
    if [ -n "$SOAK_LEADER" ]; then
        kill -KILL -- "-$SOAK_LEADER" 2>/dev/null || true
    fi
    if [ "$KEEP" = "1" ]; then
        echo "native-smoke: kept $TMP" >&2
    else
        rm -rf "$TMP"
    fi
}
trap cleanup EXIT

say() {
    printf 'native-smoke: %s\n' "$*"
}

fail() {
    printf 'native-smoke: FAIL: %s\n' "$*" >&2
    exit 1
}

show_outputs() {
    local name=$1
    if [ -s "$TMP/$name.out" ]; then
        echo "--- $name stdout (tail) ---" >&2
        tail -20 "$TMP/$name.out" >&2
    fi
    if [ -s "$TMP/$name.err" ]; then
        echo "--- $name stderr (tail) ---" >&2
        tail -30 "$TMP/$name.err" >&2
    fi
}

# Run the packaged app (or the assembled AppDir) under a hard timeout.
run_app() {
    local name=$1
    shift
    local out="$TMP/$name.out"
    local err="$TMP/$name.err"
    local status=0
    timeout --kill-after=5 "$TIMEOUT" "${APP_CMD[@]}" "$@" >"$out" 2>"$err" ||
        status=$?
    if [ "$status" -eq 0 ]; then
        return 0
    fi
    show_outputs "$name"
    fail "$name exited $status (timeout $(($TIMEOUT)) s)"
}

has_display() {
    if [ -n "${WAYLAND_DISPLAY:-}" ] && [ -S "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$WAYLAND_DISPLAY" ]; then
        return 0
    fi
    if [ -n "${DISPLAY:-}" ]; then
        local num=${DISPLAY#*:}
        num=${num%%.*}
        [ -S "/tmp/.X11-unix/X$num" ] && return 0
    fi
    return 1
}

# Resolve the artefact under test. Prefer an explicit AppImage, then a built
# native AppImage, then the release binary in an AppDir-shaped temp tree.
if [ -n "$APPIMAGE" ]; then
    [ -f "$APPIMAGE" ] || fail "AppImage not found: $APPIMAGE"
    MODE="AppImage ($APPIMAGE)"
    APP_CMD=("$APPIMAGE" --appimage-extract-and-run)
elif [ -z "$BIN_OVERRIDE" ] && compgen -G "$ROOT/dist/VmodV-Native-*.AppImage" >/dev/null; then
    APPIMAGE=$(ls -t "$ROOT"/dist/VmodV-Native-*.AppImage | head -n1)
    MODE="AppImage ($APPIMAGE)"
    APP_CMD=("$APPIMAGE" --appimage-extract-and-run)
else
    BIN=${BIN_OVERRIDE:-"$ROOT/crates/target/release/modv"}
    [ -x "$BIN" ] || fail "native binary not found at $BIN; run 'make native' or pass --appimage"
    APPDIR="$TMP/VmodV-Native.AppDir"
    mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/vmodv"
    cp "$ROOT/packaging/native/AppRun" "$APPDIR/AppRun"
    cp "$ROOT/packaging/native/log-run.sh" "$APPDIR/usr/bin/log-run.sh"
    cp "$BIN" "$APPDIR/usr/bin/modv"
    chmod +x "$APPDIR/AppRun" "$APPDIR/usr/bin/log-run.sh" "$APPDIR/usr/bin/modv"
    ln -sfn "$ROOT/src/renderer/src/application/renderers/isf/modules/isf" \
        "$APPDIR/usr/share/vmodv/isf"
    ln -sfn "$ROOT/src/renderer/src/application/point10/textures" \
        "$APPDIR/usr/share/vmodv/textures"
    MODE="assembled AppDir ($BIN)"
    APP_CMD=("$APPDIR/AppRun")
fi

say "mode: $MODE"
say "temp userData: $TMP (removed on exit unless --keep)"

# ————————————— (a) headless benchmark —————————————
say "(a) headless --frames $HEADLESS_FRAMES --bench"
run_app headless --headless --frames "$HEADLESS_FRAMES" --bench
python3 - "$TMP/headless.out" "$HEADLESS_FRAMES" <<'PY'
import json
import sys

path, expected = sys.argv[1], int(sys.argv[2])
with open(path, encoding="utf-8") as handle:
    report = json.load(handle)

required = {
    "frames",
    "wallSeconds",
    "fps",
    "meanMs",
    "p50Ms",
    "p90Ms",
    "p99Ms",
    "minMs",
    "maxMs",
    "cpuPercent",
    "rssMb",
}
missing = sorted(required - report.keys())
if missing:
    raise SystemExit(f"headless JSON is missing {missing}: {report}")
if report["frames"] != expected:
    raise SystemExit(f"headless frames={report['frames']}, expected {expected}")
if not (report["fps"] >= 0 and report["meanMs"] >= 0):
    raise SystemExit(f"headless timing is not finite/positive: {report}")
if not (report["rssMb"] > 0):
    raise SystemExit(f"headless rssMb is not positive: {report}")
print(
    "native-smoke: headless metrics: "
    f"frames={report['frames']} fps={report['fps']:.2f} "
    f"mean={report['meanMs']:.2f}ms p50={report['p50Ms']:.2f}ms "
    f"p90={report['p90Ms']:.2f}ms p99={report['p99Ms']:.2f}ms "
    f"max={report['maxMs']:.2f}ms cpu={report['cpuPercent']:.1f}% "
    f"rss={report['rssMb']:.1f}MB"
)
PY

# ————————————— (d) packaged log wrapper —————————————
say "(d) packaged AppRun writes the dated log"
LOG_DATA="$TMP/user-data-log"
MODV_USER_DATA="$LOG_DATA" timeout --kill-after=5 "$TIMEOUT" \
    "${APP_CMD[@]}" --headless --frames 5 --bench \
    >"$TMP/log.out" 2>"$TMP/log.err" ||
    {
        show_outputs log
        fail "log-wrapper headless run failed"
    }
TODAY=$(date +%F)
LOG_FILE="$LOG_DATA/logs/vmodv-$TODAY.log"
[ -f "$LOG_FILE" ] || fail "packaged launcher did not write $LOG_FILE"
grep -q '\[native-launch\].*argv=.*--headless' "$LOG_FILE" ||
    fail "$LOG_FILE has no native-launch header with argv"
if grep -q '"frames"' "$LOG_FILE"; then
    fail "$LOG_FILE captured stdout (the JSON report must stay on stdout)"
fi
say "log wrapper: $LOG_FILE"

DISPLAY_OK=1
if [ "$SKIP_WINDOWED" = "1" ]; then
    DISPLAY_OK=0
    say "(b) windowed check skipped (--skip-windowed)"
elif has_display; then
    say "(b) windowed --frames $WINDOWED_FRAMES --apply-scene '$SCENE'"
else
    DISPLAY_OK=0
    say "(b) windowed check skipped: no Wayland/X11 display available"
fi

if [ "$DISPLAY_OK" = "1" ]; then
    run_app windowed --frames "$WINDOWED_FRAMES" --apply-scene "$SCENE"
    grep -q 'ui: INFO applied Scene' "$TMP/windowed.err" ||
        {
            show_outputs windowed
            fail "windowed run did not apply the Scene '$SCENE'"
        }
    grep -q '^native: frames=' "$TMP/windowed.err" ||
        {
            show_outputs windowed
            fail "windowed run did not print the final frame count"
        }
    say "windowed: $(grep '^native: frames=' "$TMP/windowed.err" | tail -n1)"

    # ————————————— (c) state round-trip —————————————
    say "(c) state round-trip through engine-state.json"
    STATE_DATA="$TMP/user-data-state"
    MODV_USER_DATA="$STATE_DATA" timeout --kill-after=5 "$TIMEOUT" \
        "${APP_CMD[@]}" --frames "$WINDOWED_FRAMES" --apply-scene "$SCENE" \
        >"$TMP/state-1.out" 2>"$TMP/state-1.err" ||
        {
            show_outputs state-1
            fail "first state run failed"
        }
    STATE_FILE="$STATE_DATA/engine-state.json"
    [ -f "$STATE_FILE" ] || fail "first run did not write $STATE_FILE"
    SAVED_GROUPS=0
    SAVED_MODULES=0
    read -r SAVED_GROUPS SAVED_MODULES < <(
        python3 - "$STATE_FILE" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    state = json.load(handle)
groups = state.get("groups", [])
modules = state.get("modules", {})
if not groups or not modules:
    raise SystemExit(f"snapshot is empty: {len(groups)} groups, {len(modules)} modules")
print(len(groups), len(modules))
PY
    )
    [ "$SAVED_GROUPS" -gt 0 ] && [ "$SAVED_MODULES" -gt 0 ] ||
        fail "snapshot has no Groups/Modules"
    MODV_USER_DATA="$STATE_DATA" timeout --kill-after=5 "$TIMEOUT" \
        "${APP_CMD[@]}" --frames "$WINDOWED_FRAMES" \
        >"$TMP/state-2.out" 2>"$TMP/state-2.err" ||
        {
            show_outputs state-2
            fail "second state run failed"
        }
    RESTORE_LINE=$(grep 'ui: INFO restored engine state from' "$TMP/state-2.err" | tail -n1 || true)
    [ -n "$RESTORE_LINE" ] || {
        show_outputs state-2
        fail "second run did not restore the saved snapshot"
    }
    if [[ "$RESTORE_LINE" =~ \(([0-9]+)\ groups,\ ([0-9]+)\ modules\) ]]; then
        RESTORED_GROUPS=${BASH_REMATCH[1]}
        RESTORED_MODULES=${BASH_REMATCH[2]}
    else
        fail "could not parse the restore summary: $RESTORE_LINE"
    fi
    [ "$RESTORED_GROUPS" = "$SAVED_GROUPS" ] &&
        [ "$RESTORED_MODULES" = "$SAVED_MODULES" ] ||
        fail "restored $RESTORED_GROUPS/$RESTORED_MODULES, expected $SAVED_GROUPS/$SAVED_MODULES"
    say "state round-trip: $SAVED_GROUPS Groups / $SAVED_MODULES Modules restored"

    # ————————————— (e) crash recovery —————————————
    say "(e) SIGKILL before the 30 s periodic save, then restart"
    CRASH_LOG="$TMP/crash-1"
    setsid "${APP_CMD[@]}" --frames 100000 --apply-scene "$SCENE" \
        >"$CRASH_LOG.out" 2>"$CRASH_LOG.err" &
    CRASH_LEADER=$!
    # Wait for the window/Scene to come up, then kill the whole launcher tree
    # (the AppRun -> log-run.sh -> modv chain shares this process group).
    sleep 4
    kill -0 "$CRASH_LEADER" 2>/dev/null ||
        {
            show_outputs crash-1
            fail "crash-test instance exited before SIGKILL"
        }
    kill -KILL -- "-$CRASH_LEADER" 2>/dev/null || true
    wait "$CRASH_LEADER" 2>/dev/null || true
    CRASH_LEADER=""
    MODV_USER_DATA="$STATE_DATA" timeout --kill-after=5 "$TIMEOUT" \
        "${APP_CMD[@]}" --frames 10 \
        >"$TMP/crash-2.out" 2>"$TMP/crash-2.err" ||
        {
            show_outputs crash-2
            fail "restart after SIGKILL failed"
        }
    CRASH_RESTORE=$(grep 'ui: INFO restored engine state from' "$TMP/crash-2.err" | tail -n1 || true)
    CRASH_EMPTY=$(grep 'ui: INFO no state snapshot at' "$TMP/crash-2.err" | tail -n1 || true)
    if [ -n "$CRASH_RESTORE" ]; then
        [[ "$CRASH_RESTORE" =~ \(([0-9]+)\ groups,\ ([0-9]+)\ modules\) ]] ||
            fail "could not parse crash restore summary: $CRASH_RESTORE"
        [ "${BASH_REMATCH[1]}" = "$SAVED_GROUPS" ] &&
            [ "${BASH_REMATCH[2]}" = "$SAVED_MODULES" ] ||
            fail "crash restart restored ${BASH_REMATCH[1]}/${BASH_REMATCH[2]}, expected $SAVED_GROUPS/$SAVED_MODULES"
        say "crash recovery: restored the last snapshot ($SAVED_GROUPS Groups / $SAVED_MODULES Modules)"
    elif [ -n "$CRASH_EMPTY" ]; then
        say "crash recovery: clean empty state reported after SIGKILL"
    else
        show_outputs crash-2
        fail "restart after SIGKILL neither restored a snapshot nor reported an empty state"
    fi
fi

# ————————————— bounded soak (optional) —————————————
if [ "$SOAK_SECONDS" -gt 0 ]; then
    if [ "$DISPLAY_OK" != "1" ]; then
        say "soak skipped: no display for a windowed $SOAK_SECONDS s run"
    else
        say "soak: windowed --bench-seconds $SOAK_SECONDS"
        SOAK_OUT="$TMP/soak"
        setsid "${APP_CMD[@]}" --bench-seconds "$SOAK_SECONDS" --apply-scene "$SCENE" \
            >"$SOAK_OUT.out" 2>"$SOAK_OUT.err" &
        SOAK_LEADER=$!
        (
            sleep $((SOAK_SECONDS + TIMEOUT)) || true
            kill -KILL -- "-$SOAK_LEADER" 2>/dev/null || true
        ) &
        WATCHDOG=$!
        PEAK_RSS_KB=0
        while kill -0 "$SOAK_LEADER" 2>/dev/null; do
            RSS_KB=$(ps -eo pgid=,rss= | awk -v group="$SOAK_LEADER" '$1 == group { total += $2 } END { print total + 0 }')
            if [ "$RSS_KB" -gt "$PEAK_RSS_KB" ]; then
                PEAK_RSS_KB=$RSS_KB
            fi
            sleep 0.5
        done
        SOAK_STATUS=0
        wait "$SOAK_LEADER" || SOAK_STATUS=$?
        SOAK_LEADER=""
        kill "$WATCHDOG" 2>/dev/null || true
        WATCHDOG=""
        [ "$SOAK_STATUS" -eq 0 ] || {
            show_outputs soak
            fail "soak run exited $SOAK_STATUS"
        }
        SOAK_LINE=$(grep '^native: frames=' "$SOAK_OUT.err" | tail -n1 || true)
        [ -n "$SOAK_LINE" ] || {
            show_outputs soak
            fail "soak run did not print the final frame count"
        }
        say "soak metrics: $SOAK_LINE peakGroupRss=$((PEAK_RSS_KB / 1024))MB"
    fi
fi

say "PASS ($MODE)"
