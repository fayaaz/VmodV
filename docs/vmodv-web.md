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
  modules, the 27 Point10 scenes and the 396 `resources/isf` shaders (475
  modules). The corpus is embedded in the wasm binary, so there is no asset
  fetch or CORS dependency. Shader/ISF props are editable and feed the
  compositor exactly like the native path.
- The live compositor output is registered as an egui native texture; there is
  no browser readback in the preview path.
- **Audio**: the shell starts **silent** — no capture or synthesis on open.
  The "Use microphone" button requests WebAudio capture and feeds the existing
  `AudioFrame` contract (bands, wave, FFT magnitudes) into the modules.
- **Status/health**: real adapter/backend (`webgpu` or `webgl2`), FPS, frame
  time, resolution and audio activity.

## Deliberate cut lines

These are out of scope for this branch, not accidental omissions:

- Filesystem persistence: presets, `scenes.json`, project profiles, media
  library folders and user ISF/MilkDrop hot-reload.
- MilkDrop presets. The ISF corpus is embedded and parsed at startup; user
  ISF/MilkDrop folders still need the browser file API.
- Input-link expressions, MIDI/OSC link sources and plugin-provided features
  are not wired in the browser shell.
- OSC, NDI, MIDI and the local JavaScript plugin host.
- Performance-mode Scenes/playlists (the Scenes tab renders its empty state).
- Extra native output windows. The browser has one canvas; fullscreen uses the
  browser's fullscreen API instead.
- The JSONL control socket and the native e2e harness. Browser smoke tests use
  Playwright/CDP against the canvas.

## Architecture

| Piece | Role |
| --- | --- |
| `crates/modv-app` `web` feature | Compiles only the platform-neutral core: compositor, module registry, previews, quality, scene transitions, texture sources. Native modules are `cfg`'d out; `modv-media`, `modv-ipc`, winit, MIDI, notify and the shell are desktop-only target dependencies. |
| `crates/modv-web/src/state.rs` | Pure `WebState`: one or more Groups, active modules, prop/meta edits, `UiAction` dispatch and the `EngineSnapshot` read model. Unit-tested on the host. |
| `crates/modv-web/src/app.rs` | eframe app: real `Compositor::render_frame`, egui texture registration, `modv_ui::shell::draw`, action/command drain. |
| `crates/modv-web/src/audio.rs` | Demo audio and the WebAudio dB → band conversion. |
| `crates/modv-web/src/mic.rs` | `getUserMedia` → `AnalyserNode` → `AudioFrame`. |
| `web/` | Canvas page, boot/error overlay, microphone button. |

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
