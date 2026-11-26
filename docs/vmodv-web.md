# VmodV in the browser (wasm)

The browser shell is an additive target: the native `modv` binary is unchanged.
`crates/modv-web` compiles the platform-neutral core — `modv-engine`,
`modv-renderers`, the real `Compositor`, and the existing `modv-ui` egui shell —
to `wasm32-unknown-unknown` and runs it with eframe/egui-wgpu.

The browser is not a second renderer. The live output is the same compositor,
the same module registry and the same UI code as the desktop app.

## Build and run

Everything builds in Docker; there is no host Rust toolchain.

```bash
make web-check    # cargo check for wasm32 (fast gate)
make web-build    # writes web/pkg/modv_web.{js,wasm}
make web-serve    # http://127.0.0.1:8000/
```

A plain static server is enough. `web/index.html` and `web/main.js` are served
next to the generated `web/pkg/` directory. `web/pkg` is gitignored; CI rebuilds
it and deploys it at `/modules/` on GitHub Pages (the old `/app/` link
redirects there).

WebGPU is preferred. When the browser exposes no working WebGPU adapter (or
the machine has none), eframe falls back to wgpu's WebGL2 backend and the shell
still runs shader, 2D and fragment-based Point10 modules. Point10 scenes with
compute passes (Smoke Room, Rolling Fire, DVD Room, …) need WebGPU.

## What works

- The real **Groups → Gallery → Inspector** shell, including group enable,
  alpha, blend mode and module add/remove/reorder.
- **Input Links**: every module prop, module alpha/enabled and group
  alpha/enabled has an input id. The Inspector's `+ link` editor can bind them
  to audio features (bass/mid/treble/kick/FFT features), a freehand Draw curve
  or a Tween. Links evaluate once per 60 Hz output frame; Draw/Tween curves
  animate even while the microphone is silent.
- The full **bundled ISF corpus**: 36 shader modules, the 16 2D canvas
  modules, the 27 Point10 scenes and the 402 `resources/isf` shaders (481
  modules). The corpus is embedded in the wasm binary, so there is no asset
  fetch or CORS dependency. Shader/ISF props are editable and feed the
  compositor exactly like the native path.
- The live compositor output is registered as an egui native texture; there is
  no browser readback in the preview path.
- **Audio**: the shell starts **silent** — no capture or synthesis on open.
  The microphone selector and button request WebAudio capture, then feed the
  samples through the native `AudioAnalyser` (FFT, bands, Meyda features,
  BeatDetektor and TempoDetector), so B-M-T, kicks and BPM match the desktop
  pipeline.
- **Default Scenes**: the 56 versioned default Scenes are seeded into the
  Scenes tab. A cell/whole-Scene apply rebuilds the live stack with its module
  props, input links and tweens; capture, rename, reorder, enable, timing and
  cell actions update the Scene documents in place.
- **Persistence**: the live stack, Tweens, Scenes and performance config are
  saved to `localStorage` (`vmodv-web-state`) after structural edits and
  throttled prop edits, and restored on refresh. New default Scenes are seeded
  when the saved defaults version is older.
- **Output tab**: `Open output` opens `output.html`, a chrome-less second
  engine instance that restores the saved stack and renders it full-window. It
  has its own optional microphone button and never writes the saved state.
- **Status/health**: real adapter/backend (`webgpu` or `webgl2`), FPS, frame
  time, resolution and audio activity.

## Deliberate cut lines

These are out of scope for this branch, not accidental omissions:

- Filesystem persistence: presets, `scenes.json` files, project profiles,
  media library folders and user ISF/MilkDrop hot-reload. The browser keeps
  its own state in `localStorage` instead.
- MilkDrop presets. The ISF corpus is embedded and parsed at startup; user
  ISF/MilkDrop folders still need the browser file API.
- Input-link expressions, MIDI/OSC link sources and plugin-provided features
  are not wired in the browser shell.
- OSC, NDI, MIDI and the local JavaScript plugin host.
- Automatic performance-mode Scene advancing and playlists. The Scenes grid,
  manual apply and capture work; the performance tick/playlist runtime is
  native-only.
- Native output windows. `Open output` opens a browser tab running a second
  chrome-less instance; there is no cross-tab GPU texture sharing.
- The JSONL control socket and the native e2e harness. Browser smoke tests use
  Playwright/CDP against the canvas.

## Architecture

| Piece | Role |
| --- | --- |
| `crates/modv-app` `web` feature | Compiles only the platform-neutral core: compositor, module registry, previews, quality, scene transitions, texture sources. Native modules are `cfg`'d out; `modv-media`, `modv-ipc`, winit, MIDI, notify and the shell are desktop-only target dependencies. |
| `crates/modv-web/src/state.rs` | Pure `WebState`: one or more Groups, active modules, prop/meta edits, `UiAction` dispatch and the `EngineSnapshot` read model. Unit-tested on the host. |
| `crates/modv-web/src/app.rs` | eframe app: real `Compositor::render_frame`, egui texture registration, `modv_ui::shell::draw`, action/command drain, and the chrome-less output view. |
| `crates/modv-web/src/scenes.rs` | Default Scene seeding, Scene apply/capture and the performance read model. |
| `crates/modv-web/src/storage.rs` | `localStorage` load/save for the persisted live stack. |
| `crates/modv-web/src/audio.rs` | Demo audio and the native `AudioSnapshot` → `AudioFrame` mapping. |
| `crates/modv-web/src/mic.rs` | `getUserMedia` → `AnalyserNode` → native `AudioAnalyser` → `AudioFrame`. |
| `web/` | Canvas page, boot/error overlay, microphone controls, output tab. |

The compositor still owns group targets, module textures, ISF/Point10/2D
instances and the present pipeline. The only core change needed for WebGPU was
the blend pass's uniform binding: `BlendUniform` is padded to 16 bytes because
the browser baseline does not guarantee `BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED`.

## Tests

```bash
make engine-test ARGS="-p modv-web --lib"
make engine-test ARGS="-p modv-render blend_uniform_is_webgpu_aligned"
make web-check
```

`make web-build` produces a ~21 MB uncompressed wasm (`~5–7 MB` gzip depending
on the server). Most of it is Boa (expressions/plugins), naga and egui/wgpu.
A later trim can feature-gate Boa and run `wasm-opt`.
