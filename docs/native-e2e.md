# Native e2e process harness

Status: harness foundation (first native porting task)
Binary: `modv` (the native single-process shell, `crates/modv-app/src/bin/modv.rs`)
Tests: `crates/modv-app/tests/e2e_harness.rs` + `crates/modv-app/tests/e2e/mod.rs`

The Electron suite (`tests/e2e/spec/**`, Playwright) launches Electron and
drives the worker store through the page object. The native replacement must
prove the same behaviour against the **real** Rust binary: no mocked store, no
in-process test host. This document specifies the control protocol, the
process harness, the display/CI requirements, and the rules for porting the
existing specs with intent parity plus explicit waivers.

The harness covers the process/lifecycle/state/pixel layer. Specs that need a
specific panel interaction are ported on top of it in buckets (see
[Porting the Playwright specs](#porting-the-playwright-specs)); they must not
bypass the real binary.

---

## Control socket

`modv --socket <PATH>` (alias `--control-socket <PATH>`) starts an opt-in
Unix-domain server. Without the flag no socket is created and the shell behaves
exactly as before. The Electron/engine `modv-engine` binary keeps its existing
binary-framed `modv-ipc` protocol on `--socket`; this JSON-lines protocol is
served only by the native `modv` shell.

### Framing

One JSON object per line, UTF-8, `\n`-terminated. Requests:

```json
{"id": 1, "cmd": "state"}
{"id": 2, "cmd": "dispatch", "action": "groups/createGroup", "payload": {"name": "Effects"}}
{"id": 3, "cmd": "screenshot", "path": "/tmp/frame.png", "width": 640, "height": 360}
{"id": 4, "cmd": "save"}
{"id": 5, "cmd": "quit"}
{"id": 6, "cmd": "ping"}
{"id": 7, "cmd": "version"}
```

Responses are correlated by the exact `id` value (number, string, …):

```json
{"id": 1, "ok": true, "result": { ... }}
{"id": 3, "ok": false, "error": "compositor is not ready"}
```

- `ok: true` always carries `result`; `ok: false` always carries a string
  `error` and no `result`.
- Responses are written in request order per connection. Requests are drained
  once per frame, so a request is answered within one frame (~16 ms at 60 fps)
  plus the work it asks for.
- Malformed lines get a response too (with `"id": null` when the id is not
  readable) and do not close the connection.
- `MAX_LINE_BYTES` is 1 MiB; a client that never sends a newline is
  disconnected. A client that stops reading is disconnected after 64 MiB of
  buffered output, so a dead peer can never stall or OOM the app.

### Commands

| `cmd` | Fields | Result | Notes |
| --- | --- | --- | --- |
| `ping` | — | `{"pong": true, "pid": <pid>}` | Readiness probe; does not touch the store. |
| `version` | — | `{"binary": "modv", "version": "<crate version>", "protocol": 1}` | Protocol version is `control_socket::PROTOCOL_VERSION`. |
| `state` | — | Store snapshot + shell runtime | See [State shape](#state-shape). |
| `dispatch` | `action` (string), `payload` (any JSON, optional) | The store action's JSON result | Runs on the winit thread; unknown actions follow `Store::dispatch` (usually `null`). Store errors are `ok: false`. |
| `screenshot` | `path` (string), `width`/`height` (optional positive integers) | `{"path", "width", "height", "bytes"}` | Renders the current store state, reads the compositor's main texture back, box-downsamples and writes a PNG. Missing dimensions default to the compositor's main size. |
| `save` | — | `{"path", "framesPresented"}` | Writes the same `engine-state.json` snapshot as File → Save engine snapshot. Crash/restart tests use it to establish the snapshot a supervisor restores before killing the process. |
| `quit` | — | `{"quitting": true}` | Same path as File → Quit: saves `engine-state.json`/`scenes.json`, window state, layout, then exits 0. The acknowledgement is flushed before the process exits. |

`state`, `ping` and `version` are pure reads. `dispatch`, `save` and
`screenshot` run on the winit loop between frames. A screenshot includes an
explicit `render_engine(None)` so a `dispatch` earlier in the same request batch
is visible in the captured pixels.

### State shape

`state` is `Store::sync_state()` — the preset keys plus the Scene/runtime
sidecars — with shell-owned sections added:

```text
groups              live Groups (render order is derived, not stored)
modules             active module instances keyed by id
inputs              { focusedInput, inputs, inputLinks }
expressions         { assignments }
tweens              tween documents
midi                input-link-readable MIDI state
registered          module registry (Gallery)
scenes              saved Scene documents (default seeded Scenes on a fresh userData)
performance         performance config + row names + disabled cells
defaultsSeeded      bool
defaultsVersion     int
seededDefaultScenes default names already seeded
appliedSceneId      last applied Scene id (null when none)
windows             engine-owned output windows
audio               { bpm, kick, engineOwned, captureRate, selection, device,
                      bands: { bass, mid, treble, beat }, lanes, cells, sampleRate }
midiRuntime         { available, error, learning }
runtime             { frames, fps, frameMs, resolution, window, outputOpen,
                      userData, statePath, profile, socket }
_engine             { framesPresented }   (same sidecar as engine-state.json)
```

`groups`/`modules`/`inputs`/`scenes`/`performance` are the authoritative store
values; `runtime` and `audio` are filled from the shell. `runtime.window` is the
main window's live logical size (a saved `window-state.json` overrides
`--width`/`--height` after the first launch), while `runtime.resolution` is the
compositor main texture, which follows the output window.

### Lifecycle and threading

- The socket is bound **before** the shell's expensive init (media library,
  plugins, GPU/window), so a harness can connect while startup is still
  running; `ping` only answers once the frame loop is live.
- One dedicated thread owns the listener and every client stream. It never
  touches the `Store` or `Compositor`: requests are sent over an mpsc channel
  with a per-request reply sender, and the winit loop answers them at the next
  frame. The thread writes the response lines from the reply channel.
- A stale socket node (owner died) is removed on bind. A live socket, a
  regular file or a symlink is **not** deleted: `bind` fails with
  `AddrInUse` (same contract as `modv-ipc`). The socket is `0600`.
- On normal exit (`quit`, window close, `--frames`, dead parent) the socket
  file is unlinked by `ControlServer::drop`; a replaced socket is never
  unlinked because the server compares the device/inode it bound.

### Using it manually

```bash
cargo run -p modv-app --bin modv -- --socket /tmp/modv-control.sock
# in another shell
printf '%s\n' '{"id":1,"cmd":"ping"}' | socat - UNIX-CONNECT:/tmp/modv-control.sock
```

---

## Rust process harness

`crates/modv-app/tests/e2e/mod.rs` is the shared harness module;
`crates/modv-app/tests/e2e_harness.rs` contains the control-socket proofs
(default-seeded state + clean quit, dispatch/Scene/screenshot, error and
reconnect handling) and `crates/modv-app/tests/e2e_process.rs` ports the
process-level Playwright intents (kill/restart, main-window render, default
Group, window placement, Scene JSON opened like a preset).

### Spawning

```rust
let mut app = NativeHarness::builder()
    .width(640)
    .height(360)
    .arg("--apply-scene")
    .arg("Scene name")
    .spawn();

app.wait_ready();                       // ping until the frame loop is live
```

Restart tests own one `userData` tree across several launches: create it with
`e2e::temp_root("name")`, pass `root.join("userData")` to
`NativeHarnessBuilder::user_data`, and remove the root when the test ends. The
harness never deletes an explicit userData directory, so `engine-state.json`,
`scenes.json` and `window-state.json` survive the process boundary.

`spawn()` launches `env!("CARGO_BIN_EXE_modv")` (the binary Cargo just built)
with:

- a unique temp `MODV_USER_DATA` (or the explicit `user_data(...)` path),
  `HOME` and `XDG_RUNTIME_DIR` (so defaults are seeded into a throwaway
  `engine-state.json`/`scenes.json`, never the developer's
  `~/.config/VmodV`);
- `--socket <unique path>` and the requested `--width`/`--height`;
- any extra CLI args (`--apply-scene`, `--preset`, `--scene`, `--playlist`,
  `--start-performance`, `--frames`, …);
- stdout + stderr redirected to `<temp>/modv.log`;
- a virtual display when there is no `DISPLAY`/`WAYLAND_DISPLAY` (below).

### API

| Method | Purpose |
| --- | --- |
| `wait_ready()` / `wait_ready_for(timeout)` | Poll `ping`; panic with the app log on early exit/timeout. |
| `state()` | `state` result as `serde_json::Value`. |
| `dispatch(action, payload)` | `dispatch`, panicking on a store error. |
| `try_dispatch(action, payload)` | `dispatch` returning `Result<Value, String>` (assert error paths). |
| `call(request)` | Send any request; injects the id, returns `result` or `Err(error)`. |
| `screenshot(path, width, height)` | Write a PNG; returns the wire result. Use `None` dimensions for the full main texture. |
| `quit()` | Graceful `quit` (reply is flushed before exit). |
| `save()` | Write `engine-state.json` now (`save`); returns the path and frame count. |
| `wait_exit(timeout)` / `assert_exit(code)` | Reap the child; asserts and prints logs on the wrong status/timeout. |
| `wait_for_state(timeout, predicate)` | Poll `state` until a predicate holds (no arbitrary sleeps in tests). |
| `logs()` / `log_path()` | The combined stdout/stderr log. |
| `user_data()`, `state_path()`, `socket_path()`, `temp_dir()`, `pid()` | Diagnostics and on-disk assertions. |
| `disconnect()` | Drop the persistent client; the next request reconnects. |
| `kill()` | Force-kill the process (tests that need crash files). |
| `kill_and_wait()` | SIGKILL and return the `ExitStatus` (assert `signal() == 9`). |

`Drop` kills the child if it is still alive. The temp tree is removed on
success unless `MODV_E2E_KEEP=1` (or `.keep(true)`); a panicking test always
keeps it and prints the path, so the log and screenshots survive CI failures.

### Display selection

Order:

1. `DISPLAY` (X11) — used as-is.
2. `WAYLAND_DISPLAY` — used as-is.
3. `Xvfb :N -screen 0 1920x1080x24 -nolisten tcp -ac`, with a free `:N`
   checked against `/tmp/.X11-unix/X<N>` and `/tmp/.X<N>-lock`. The harness
   waits for the X socket before spawning `modv`.
4. `sway` with `WLR_BACKENDS=headless`, `WLR_LIBINPUT_NO_DEVICES=1` and a
   private `XDG_RUNTIME_DIR`; the harness discovers the `wayland-*` socket and
   sets `WAYLAND_DISPLAY`.

Each `NativeHarness` owns its display and kills it in `Drop`. Tests must run
serially (`--test-threads=1`): the software Vulkan adapter is shared and
multiple native apps on one CI GPU/display oversubscribe it.

### Docker / CI

The `rust` stage of the `Dockerfile` installs `xvfb`, `xauth` and winit's X11
runtime libraries, plus `mesa-vulkan-drivers` (lavapipe) for the Vulkan
adapter. No host display, GPU or sound server is required. Run:

```bash
make engine-test ARGS="-p modv-app --test e2e_harness -- --test-threads=1"
make engine-test ARGS="-p modv-app --test e2e_process -- --test-threads=1"
```

`.github/workflows/native.yml` runs the unit tests
(`-p modv-app --lib`) and the harness (separate steps, serial). A developer
host with a real display/GPU simply inherits `DISPLAY`/`WAYLAND_DISPLAY`; the
harness does not change the running desktop.

Timeouts exist at every boundary: 90 s readiness, 60 s per request, 30 s for
quit/exit, 10 s for display startup. A hang fails with the app log rather than
blocking cargo forever.

---

## Porting the Playwright specs

Each of the 53 specs / 139 tests under `tests/e2e/spec/**` lands in exactly one
bucket:

1. **Process equivalent** — launch `NativeHarness`, drive the same action(s)
   through `dispatch`, assert `state`, pixels and exit. This is the default for
   anything that only mutates the store or compositor.
2. **Native-only replacement** — the Electron mechanism no longer exists (for
   example the web worker, GL contexts, DOM drag events). Assert the native
   intent (state transition, rendered frame, persisted file), not the old DOM.
3. **Explicit waiver** — recorded with a reason. A waiver must say what the
   legacy test protected and what now protects it (a Rust unit test, a
   packaged smoke check, manual hardware verification, or "no equivalent
   surface"). "Not ported yet" is not a waiver.

### Mapping guide

| Playwright pattern | Native harness equivalent |
| --- | --- |
| `modVApp.evaluateWorkerState(...)` / `checkWorkerAndMainState` | `app.state()["..."]`; there is only one authoritative store. |
| `modV.store.dispatch(action, payload)` | `app.dispatch(action, payload)`. |
| `modV.store.commit(mutation, payload)` | `app.dispatch(mutation, payload)` — the store aliases commits to the same handler. |
| `waitFor`/`expect.poll` on state | `app.wait_for_state(timeout, predicate)`. |
| canvas `getImageData` / screenshot expectations | `app.screenshot(path, Some(w), Some(h))`, then decode/compare with the `image` crate. |
| launch flags / packaged app assumptions | builder `.arg(...)`; `MODV_USER_DATA` is always a temp tree. |
| `beforeAll` app launch | one `NativeHarness` per test (or a shared harness in the test module when the spec needs one process). |
| window close / quit cleanup | `app.quit()` + `app.assert_exit(0)` + assert `engine-state.json`/`scenes.json`/`native-layout.json`. |
| keyboard/mouse on a panel | port the **intent** through the same store action, or add a narrow `ui`/`command` socket command that maps to a `UiAction`. Do not reintroduce a DOM. |
| ISF/Gallery hover pixels | assert the engine preview/compositor frame (the native shell has no DOM hover); add an egui `kittest` only for the actual widget behaviour. |

For UI-only behaviour (tab switching, panel layout, drag handles), prefer an
egui `kittest` interaction test; use the process harness when the assertion is
about the live store, persistence or presented pixels. The harness can expose
more native commands later (for example `ui` → `UiAction`) without changing
the framing.

Assertions should be **deterministic**: dispatch state, wait for a frame count
(`runtime.frames`) or a state predicate, then capture/inspect. Avoid
wall-clock sleeps; the harness timeouts are failure bounds, not pacing.

---

## Current proofs

`tests/e2e_harness.rs`:

- `native_shell_serves_state_seeds_defaults_and_quits_cleanly` — ping/version,
  one `"New Group"` default Group, seeded default Scenes, audio/tween/input
  sections, quit acknowledgement, exit 0 and a valid `engine-state.json`.
- `dispatch_updates_state_and_screenshot_writes_a_png` — creates a Group,
  applies the first default Scene with Groups, observes `appliedSceneId` and
  live Groups in `state`, writes a 320×180 PNG and decodes it.
- `control_socket_reports_errors_reconnects_and_logs` — malformed commands and
  store errors come back as `ok: false`; a dropped client does not stop the
  server; the log records the listening path; quit is clean.

`tests/e2e_process.rs`:

- `engine_skeleton_survives_sigkill_and_restores_state` — presents a frame,
  writes a `save` snapshot, SIGKILLs the shell (`signal() == 9`), relaunches
  over the same userData and observes the restored Group and a new pid. The
  Electron `MODV_ENGINE` supervisor path is a windowed `modv-engine` child; it
  has no test seam that survives without Electron, so the native
  single-process shell ports the intent: the same snapshot format, the same
  kill without a graceful quit, and a clean replacement process. The
  supervisor's watchdog/restart budget itself is covered by its own Rust unit
  tests, not the process harness.
- `renders_the_main_window_at_the_requested_size` — readiness, live
  `runtime.frames`, `runtime.window` equals the requested 640×360, and a
  decoded 640×360 (16:9) PNG.
- `fresh_native_start_creates_exactly_one_default_group` — one enabled
  `"New Group"` on a fresh userData; after adding a user Group and quitting,
  the relaunch restores exactly two Groups with the same default id (no
  duplicate).
- `window_state_survives_a_restart` — a 720×405 launch saves
  `window-state.json`; a relaunch requesting 800×600 keeps the saved main
  window and the saved `output-0` compositor size.
- `a_scene_json_opened_like_a_preset_is_applied_as_a_scene` — a Scene document
  passed as `--preset` is listed, sets `appliedSceneId`/`currentSceneId`, and
  its Group + Ball Module are live (no default Group is seeded on top).

### Native startup parity

The native shell owns the store instead of an Electron renderer, so it
reproduces the renderer's boot dispatch itself. On a **fresh** native start —
no restored `engine-state.json`, no successful startup preset/Scene (a
playlist import does not replace the live stack), no `--apply-scene` — it
dispatches `groups/createGroup` with `enabled: 1`, which yields the same
`"New Group"` (and control inputs) as `application/index.js:446-456`. A
restored snapshot, preset/Scene document, applied Scene or any existing Group
wins, so nothing is duplicated. Headless (`--headless`/`--screenshot`) and
`modv-engine` paths never run this: they keep their empty-start behaviour.
