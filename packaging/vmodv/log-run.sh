#!/bin/sh
# Run the VmodV shell with its stderr copied into the shared daily log.
#
# Electron writes `<userData>/logs/vmodv-YYYY-MM-DD.log`; the VmodV shell only
# writes to stderr, so the packaged launchers put this wrapper in front of the
# real binary. stdout is deliberately untouched: `--bench`/`--headless` JSON
# must stay machine-readable. The sh preamble re-execs the body under bash for
# the stderr tee; without bash it runs the binary unlogged rather than
# refusing to start.
if [ "$#" -eq 0 ]; then
    echo "usage: log-run.sh BINARY [ARGS...]" >&2
    exit 2
fi

if [ -z "${MODV_LOG_RUN_BASH:-}" ]; then
    if ! command -v bash >/dev/null 2>&1; then
        echo "log-run: bash not found; running without a log file" >&2
        exec "$@"
    fi
    MODV_LOG_RUN_BASH=1
    export MODV_LOG_RUN_BASH
    exec bash "$0" "$@"
fi

# The application's userData directory is `$MODV_USER_DATA` when the VmodV
# shell is told to use one, otherwise the Electron `app.setName("VmodV")`
# path. `user_data_dir()` in crates/modv-app/src/window_state.rs is the source
# of truth for the VmodV side.
if [ -n "${MODV_USER_DATA:-}" ]; then
    LOG_DIR="$MODV_USER_DATA/logs"
else
    LOG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/VmodV/logs"
fi

if ! mkdir -p "$LOG_DIR" 2>/dev/null; then
    echo "log-run: cannot create $LOG_DIR; running without a log file" >&2
    exec "$@"
fi

LOG="$LOG_DIR/vmodv-$(date +%F).log"
if ! : >>"$LOG" 2>/dev/null; then
    echo "log-run: cannot write $LOG; running without a log file" >&2
    exec "$@"
fi

ARGV=$(printf '%q ' "$@")
printf '%s INFO [vmodv-launch] pid=%s argv=%s\n' \
    "$(date +%Y-%m-%dT%H:%M:%S%z)" "$$" "${ARGV% }" >>"$LOG" 2>/dev/null || true

prune_logs() {
    local file base day cutoff
    cutoff=$(date -d '7 days ago' +%F 2>/dev/null || true)
    if [ -z "$cutoff" ]; then
        cutoff=$(date -v-7d +%F 2>/dev/null || true)
    fi

    # Drop files whose dated name is older than seven days. Today's file is
    # never a candidate even if the clock moved backwards.
    for file in "$LOG_DIR"/vmodv-*.log; do
        [ -f "$file" ] || continue
        [ "$file" = "$LOG" ] && continue
        base=${file##*/}
        day=${base#vmodv-}
        day=${day%.log}
        case "$day" in
            [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) ;;
            *) continue ;;
        esac
        if [ -n "$cutoff" ] && [[ "$day" < "$cutoff" ]]; then
            rm -f -- "$file" 2>/dev/null || true
        fi
    done

    # Match Electron's MAX_LOG_FILES = 7 as well: today plus six older files.
    local -a files=()
    for file in "$LOG_DIR"/vmodv-*.log; do
        [ -f "$file" ] || continue
        [ "$file" = "$LOG" ] && continue
        files+=("$file")
    done
    if [ "${#files[@]}" -gt 6 ]; then
        mapfile -t files < <(printf '%s\n' "${files[@]}" | LC_ALL=C sort)
        local excess i
        excess=$((${#files[@]} - 6))
        for ((i = 0; i < excess; i += 1)); do
            rm -f -- "${files[i]}" 2>/dev/null || true
        done
    fi
}
prune_logs || true

# Keep the child in the foreground and its signals/goes-to-the-terminal
# behaviour intact. `tee` is an explicit coprocess so its PID can be waited on:
# a process substitution is not a job, and `wait` would return before the tail
# of stderr reached the log (or leave tee running) on a fast exit.
coproc LOG_TEE { tee -a "$LOG" >&2; }
"$@" 2>&"${LOG_TEE[1]}"
STATUS=$?
exec {LOG_TEE[1]}>&-
wait "$LOG_TEE_PID" 2>/dev/null || true
exit "$STATUS"
