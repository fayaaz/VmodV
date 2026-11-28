#!/usr/bin/env bash
# Capture the VmodV shell's real main output to a PNG for one named Point10
# scene.
#
# Usage:
#   tools/vmodv-shot.sh <scene-name> <output.png> [width height] [warm-frames]
#
#   <scene-name>   canonical registry name, e.g. "Plane9 Waves of Sin"
#   <output.png>   destination PNG; must live under the repo (bind mount)
#   width height   output window / capture size (default: 960 540)
#   warm-frames    frames to advance after the scene is live (default: 300,
#                  about 5 s at 60 fps — Waves of Sin needs >110 ticks)
#
# Environment:
#   MODV_SHOT_READY_TIMEOUT   seconds to wait for the control socket (1200)
#   MODV_SHOT_WARM_TIMEOUT    seconds allowed for the warm-up frames (240)
#   MODV_SHOT_STALL_TIMEOUT   seconds without a frame before failing (15)
#
# What it does:
#   1. starts `make vmodv` inside the Rust container under a throwaway
#      userData tree (`scratch/shot-run/<scene>/userData`) with a seeded
#      `window-state.json` for the requested output size;
#   2. drives the JSON-lines control socket (host python3): removes the boot
#      Group, creates a Group, makes the named module active (enabled), adds it
#      to the Group, verifies the engine does not own audio capture, injects
#      the deterministic `meyda/SET_BANDS` payload, and waits for warm frames;
#   3. requests a screenshot of the composited main output, checks the PNG,
#      prints the resolved scene name / frame count / resolution / path, then
#      quits and removes the container.
#
# The trap always quits the app (when reachable), kills the container and
# stops `make`, including on failure. Script state (userData, logs, socket) is
# confined to `scratch/`, apart from a short-lived symlink in the temp dir.
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

READY_TIMEOUT=${MODV_SHOT_READY_TIMEOUT:-1200}
WARM_TIMEOUT=${MODV_SHOT_WARM_TIMEOUT:-240}
STALL_TIMEOUT=${MODV_SHOT_STALL_TIMEOUT:-15}

usage() {
    sed -n '2,/^set -euo pipefail$/p' "$0" |
        sed -e '/^set -euo pipefail$/d' -e 's/^# \{0,1\}//'
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

if [ "$#" -lt 2 ] || [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
    usage
    [ "$#" -lt 2 ] && exit 2
    exit 0
fi

SCENE=$1
OUT=$2
WIDTH=${3:-960}
HEIGHT=${4:-540}
WARM=${5:-300}

case "$WIDTH$HEIGHT$WARM" in
    ''|*[!0-9]*) die "width, height and warm-frames must be positive integers" ;;
esac
[ "$WIDTH" -gt 0 ] && [ "$HEIGHT" -gt 0 ] || die "width and height must be positive"
[ "$WARM" -ge 0 ] || die "warm-frames must be zero or greater"

# The socket and PNG must be visible to the host: both have to live under the
# bind-mounted repo (which the container sees at /app).
OUT_HOST=$(realpath -m -- "$OUT")
case "$OUT_HOST" in
    "$ROOT"/*) ;;
    *) die "output path must be inside the repo bind mount ($ROOT): $OUT" ;;
esac
OUT_REL=${OUT_HOST#"$ROOT"/}
OUT_CONT=/app/$OUT_REL

SLUG=$(printf '%s' "$SCENE" | tr '[:upper:]' '[:lower:]' |
    sed -e 's/[^a-z0-9][^a-z0-9]*/-/g' -e 's/^-//' -e 's/-$//')
[ -n "$SLUG" ] || SLUG=scene

RUN_DIR=$ROOT/scratch/shot-run/$SLUG
USER_DATA_HOST=$RUN_DIR/userData
SOCKET_HOST=$RUN_DIR/control.sock
# Unix-domain socket paths are capped at ~108 bytes; a deep worktree path can
# exceed that on the host even though the container sees /app/... . Connect
# through a short symlink in the temp dir (the socket itself still lives under
# the bind-mounted repo).
SHOT_TMP=${MODV_SHOT_TMPDIR:-/tmp/opencode}
mkdir -p -- "$SHOT_TMP"
SOCKET_LINK=$SHOT_TMP/vmodv-shot-$$
SOCKET_CLIENT=$SOCKET_LINK/control.sock
MAKE_LOG=$RUN_DIR/make.log
USER_DATA_CONT=/app/scratch/shot-run/$SLUG/userData
SOCKET_CONT=/app/scratch/shot-run/$SLUG/control.sock
CONTAINER=vmodv-shot-$$

rm -rf -- "$RUN_DIR"
mkdir -p -- "$RUN_DIR" "$USER_DATA_HOST" "$(dirname -- "$OUT_HOST")"
rm -f -- "$SOCKET_LINK"
ln -s -- "$RUN_DIR" "$SOCKET_LINK"

# The output window (and therefore the compositor's main texture) opens at
# this size when window-state.json pre-seeds output-0.
cat >"$USER_DATA_HOST/window-state.json" <<EOF
{"version":1,"mainWindow":null,"windows":{"output-0":{"x":0,"y":0,"width":$WIDTH,"height":$HEIGHT,"maximized":false,"fullscreen":false}}}
EOF

MAKE_PID=""
cleanup() {
    local status=$?
    trap - EXIT INT TERM
    if [ -n "$MAKE_PID" ] && kill -0 "$MAKE_PID" 2>/dev/null; then
        kill "$MAKE_PID" 2>/dev/null || true
    fi
    # `--rm` normally removes the container, but a killed docker client can
    # leave it running; force-remove it by the name we gave it.
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    rm -f -- "$SOCKET_LINK"
    if [ -n "$MAKE_PID" ]; then
        wait "$MAKE_PID" 2>/dev/null || true
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

docker rm -f "$CONTAINER" >/dev/null 2>&1 || true

echo "shot: launching the shell for \"$SCENE\" (${WIDTH}x${HEIGHT}, ${WARM} warm frames)" >&2
# PULSE_SOCKET=/nonexistent defeats the PulseAudio mount. On a Wayland host
# the shell still tries PipeWire first, so the explicit `--audio-source input`
# with a device name nothing matches guarantees no capture feed starts: the
# store keeps `engineOwned == false` and injected `meyda/SET_BANDS` stays
# authoritative (verified below).
DOCKER_RUN_ARGS="-e MODV_USER_DATA=$USER_DATA_CONT --name $CONTAINER" \
    PULSE_SOCKET=/nonexistent \
    make -C "$ROOT" vmodv \
    ARGS="--socket $SOCKET_CONT --audio-source input --audio-device vmodv-shot-no-such-input" \
    >"$MAKE_LOG" 2>&1 &
MAKE_PID=$!

deadline=$((SECONDS + READY_TIMEOUT))
while [ ! -S "$SOCKET_HOST" ]; do
    if ! kill -0 "$MAKE_PID" 2>/dev/null; then
        tail -n 40 "$MAKE_LOG" >&2 || true
        die "the shell exited before its control socket appeared (log: $MAKE_LOG)"
    fi
    if [ "$SECONDS" -ge "$deadline" ]; then
        tail -n 40 "$MAKE_LOG" >&2 || true
        die "control socket never appeared within ${READY_TIMEOUT}s ($SOCKET_HOST; log: $MAKE_LOG)"
    fi
    sleep 1
done

python3 - "$SOCKET_CLIENT" "$OUT_HOST" "$OUT_CONT" "$SCENE" \
    "$WIDTH" "$HEIGHT" "$WARM" "$WARM_TIMEOUT" "$STALL_TIMEOUT" <<'PY'
import json
import os
import socket
import sys
import time

socket_path, out_host, out_container, scene, width, height, warm, warm_timeout, stall_timeout = (
    sys.argv[1:]
)
width = int(width)
height = int(height)
warm = int(warm)
warm_timeout = float(warm_timeout)
stall_timeout = float(stall_timeout)

BANDS = {
    "sampleRate": 44100,
    "lanes": [0.2, 0, 0, 0, 0.8, 0, 0, 0, 0, 0, 0, 0],
}


class ShotError(Exception):
    pass


class Client:
    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(60)
        last = None
        deadline = time.monotonic() + 30
        while True:
            try:
                self.sock.connect(path)
                break
            except OSError as error:
                last = error
                if time.monotonic() >= deadline:
                    raise ShotError(f"could not connect to {path}: {last}")
                time.sleep(0.1)
        self.file = self.sock.makefile("rwb")
        self.next_id = 0

    def request(self, cmd, **fields):
        self.next_id += 1
        request = {"id": self.next_id, "cmd": cmd}
        request.update(fields)
        self.sock.sendall(json.dumps(request, separators=(",", ":")).encode() + b"\n")
        line = self.file.readline()
        if not line:
            raise ShotError(f"{cmd}: the shell closed the control socket")
        try:
            response = json.loads(line)
        except ValueError as error:
            raise ShotError(f"{cmd}: invalid response {line[:200]!r}: {error}")
        if response.get("id") != request["id"]:
            raise ShotError(f"{cmd}: response id {response.get('id')!r} != {request['id']!r}")
        if not response.get("ok"):
            raise ShotError(f"{cmd} failed: {response.get('error')}")
        return response.get("result")

    def quit(self):
        try:
            self.request("quit")
        except (ShotError, OSError):
            pass
        try:
            self.file.close()
        except OSError:
            pass
        try:
            self.sock.close()
        except OSError:
            pass


def state(client):
    return client.request("state")


def dispatch(client, action, payload):
    return client.request("dispatch", action=action, payload=payload)


client = Client(socket_path)
try:
    client.request("ping")
    snapshot = state(client)

    # Drop the boot Group so the capture only shows the requested module.
    for group in snapshot.get("groups") or []:
        group_id = group.get("id")
        if group_id:
            dispatch(client, "groups/removeGroup", {"groupId": group_id})

    group = dispatch(
        client,
        "groups/createGroup",
        {
            "id": "shot",
            "name": "Shot",
            "enabled": 1,
            "alpha": 1.0,
            "clearing": True,
            "inherit": False,
            "pipeline": False,
            "compositeOperation": "normal",
        },
    )
    group_id = group.get("id") if isinstance(group, dict) else None
    if not group_id:
        raise ShotError(f"groups/createGroup did not return an id: {group!r}")

    module = dispatch(
        client,
        "modules/makeActiveModule",
        {
            "moduleName": scene,
            "moduleMeta": {
                "enabled": True,
                "alpha": 1.0,
                "compositeOperation": "normal",
            },
        },
    )
    module_id = module.get("$id") if isinstance(module, dict) else None
    if not module_id:
        raise ShotError(f"modules/makeActiveModule did not return $id: {module!r}")
    # An unknown module name still becomes an active record, but with a
    # `$status` error ("not registered") and nothing to render. Treat that as
    # a failed dispatch so a typo cannot produce a blank capture.
    statuses = module.get("$status") or []
    if statuses:
        details = "; ".join(
            str(entry.get("message", entry)) if isinstance(entry, dict) else str(entry)
            for entry in statuses
        )
        raise ShotError(f"{scene!r} is not a registered module: {details}")
    resolved = module.get("$moduleName") or scene

    dispatch(
        client,
        "groups/ADD_MODULE_TO_GROUP",
        {"groupId": group_id, "moduleId": module_id},
    )

    snapshot = state(client)
    live = (snapshot.get("modules") or {}).get(module_id)
    if not live:
        raise ShotError(f"{resolved!r} is not a live module after ADD_MODULE_TO_GROUP")

    audio = snapshot.get("audio") or {}
    if audio.get("engineOwned") is not False:
        raise ShotError(
            "audio.engineOwned is not false "
            f"({audio.get('engineOwned')!r}); live capture would override the injected bands"
        )
    dispatch(client, "meyda/SET_BANDS", BANDS)

    start = int(snapshot.get("runtime", {}).get("frames", 0))
    target = start + warm
    last_frames = start
    last_progress = time.monotonic()
    warm_deadline = time.monotonic() + warm_timeout
    while True:
        snapshot = state(client)
        frames = int(snapshot.get("runtime", {}).get("frames", 0))
        now = time.monotonic()
        if frames >= target:
            break
        if frames > last_frames:
            last_frames = frames
            last_progress = now
        elif now - last_progress >= stall_timeout:
            raise ShotError(f"frames stopped advancing at {frames} (target {target})")
        if now >= warm_deadline:
            raise ShotError(
                f"only {frames - start} of {warm} warm frames advanced in "
                f"{warm_timeout:.0f}s"
            )
        time.sleep(0.15)

    result = client.request(
        "screenshot", path=out_container, width=width, height=height
    )
    if not os.path.isfile(out_host):
        raise ShotError(f"screenshot did not create {out_host}")
    size = os.path.getsize(out_host)
    if size == 0:
        raise ShotError(f"screenshot wrote an empty PNG: {out_host}")
    with open(out_host, "rb") as png:
        header = png.read(24)
    if len(header) < 24 or header[:8] != b"\x89PNG\r\n\x1a\n":
        raise ShotError(f"{out_host} is not a PNG")
    png_width = int.from_bytes(header[16:20], "big")
    png_height = int.from_bytes(header[20:24], "big")
    if (png_width, png_height) != (width, height):
        raise ShotError(
            f"PNG is {png_width}x{png_height}, expected {width}x{height}"
        )

    print(f"scene: {resolved}")
    print(f"frames: {frames}")
    print(f"resolution: {png_width}x{png_height}")
    print(f"png: {out_host} ({size} bytes)")
    print(
        "SHOT_JSON "
        + json.dumps(
            {
                "scene": resolved,
                "frames": frames,
                "width": png_width,
                "height": png_height,
                "bytes": size,
                "path": out_host,
                "engineOwned": audio.get("engineOwned"),
                "screenshot": result,
            }
        )
    )
except (ShotError, OSError) as error:
    print(f"error: {error}", file=sys.stderr)
    sys.exit(1)
finally:
    client.quit()
PY
