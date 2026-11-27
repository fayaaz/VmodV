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
it on every push to `main` and deploys it at the GitHub Pages site root through
GitHub Actions (`actions/deploy-pages`). The old `/modules/` and `/app/` links
redirect to the root. The static `/download/` landing page (`web/download/`)
ships with the same artifact; it reads the GitHub releases API to link the
latest AppImage/`.deb` and Windows/macOS portable ZIPs, and is linked from the
shell's boot overlay.

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
  modules, the 40 Point10 scenes and the 402 `resources/isf` shaders (494
  modules). The corpus is embedded in the wasm binary, so there is no asset
  fetch or CORS dependency. Shader/ISF props are editable and feed the
  compositor exactly like the native path.
- The live compositor output is registered as an egui native texture; there is
  no browser readback in the preview path.
- **Audio**: the shell starts **silent** — no capture or synthesis on open.
  The microphone selector and button request WebAudio capture, then feed the
  samples through the native `AudioAnalyser` (FFT, bands, Meyda features,
  BeatDetektor and TempoDetector), so B-M-T, kicks and BPM match the desktop
  pipeline. The context resumes on focus/visibility so backgrounding does not
  stall the engine.
- **Default Scenes**: the 56 versioned default Scenes are seeded into the
  Scenes tab. A cell/whole-Scene apply rebuilds the live stack with its module
  props, input links and tweens; capture, rename, reorder, enable, timing,
  timing-unit conversion, cell create/clear/copy/paste and row names update the
  Scene documents in place.
- **Performance mode**: per-row dwell timers independently advance enabled,
  non-empty, non-disabled Scene cells in `in-order` or `random` order. Each
  advance crossfades the outgoing row Group over `transitionSeconds`; Stop
  completes in-flight fades. The Scenes grid marks the live Scene per row.
- **Transitions**: whole-Scene changes default to the Fade alpha path, with a
  picker for `None`, the six native shader transitions and registered ISF
  transition shaders. Shader transitions capture the outgoing frame once and
  run through the compositor's `scene_transition` pass; unavailable captures
  or pipelines fall back to Fade.
- **Previews and monitors**: Gallery hover previews and Scene-cell hover
  previews render through isolated GPU preview slots registered with
  egui-wgpu; the Scenes gutter refreshes live row-monitor thumbnails from the
  compositor's Group textures without readback.
- **Persistence**: the live stack, Tweens, Scenes and performance config are
  saved to `localStorage` (`vmodv-web-state`) after structural edits and
  throttled prop edits, and restored on refresh. New default Scenes are seeded
  when the saved defaults version is older.
- **Output view**: `Open output` hides the UI and draws the compositor's main
  texture over the whole canvas in the same tab. The output renders at the
  real viewport resolution instead of the UI's 960px budget (capped at 4K),
  with no `captureStream` encoding or pop-up window, so it is not softened. `F`
  toggles the browser's fullscreen mode, `Escape` leaves it, and a cog
  revealed by hovering the top-right corner returns to the UI.
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
- Performance playlists and the playlist editor. Per-row auto-advance,
  crossfades and the transition picker work; playlist sequencing is
  native-only.
- Native output windows. `Open output` uses the same tab and canvas; there is
  no separate window, cross-tab texture sharing or capture stream. A fully
  hidden tab still pauses the browser's rendering loop.
- The JSONL control socket and the native e2e harness. Browser smoke tests use
  Playwright/CDP against the canvas.

## Architecture

| Piece | Role |
| --- | --- |
| `crates/modv-app` `web` feature | Compiles only the platform-neutral core: compositor, module registry, previews, quality, scene transitions, texture sources. Native modules are `cfg`'d out; `modv-media`, `modv-ipc`, winit, MIDI, notify and the shell are desktop-only target dependencies. |
| `crates/modv-web/src/state.rs` | Pure `WebState`: one or more Groups, active modules, prop/meta edits, `UiAction` dispatch and the `EngineSnapshot` read model. Unit-tested on the host. |
| `crates/modv-web/src/app.rs` | eframe app: real `Compositor::render_frame`, transition capture/pass wiring, GPU previews and row monitors, `modv_ui::shell::draw`, the same-tab output view and action/command drain. |
| `crates/modv-web/src/output_view.rs` | Host-tested output-view state, full-viewport render-target policy, letterbox geometry and cog reveal zone. |
| `crates/modv-web/src/scenes.rs` | Default Scene seeding, Scene apply/capture/editing, transitions and the performance read model. |
| `crates/modv-web/src/storage.rs` | `localStorage` load/save for the persisted live stack. |
| `crates/modv-web/src/audio.rs` | Demo audio and the native `AudioSnapshot` → `AudioFrame` mapping. |
| `crates/modv-web/src/mic.rs` | `getUserMedia` → `AnalyserNode` → native `AudioAnalyser` → `AudioFrame`. |
| `web/` | Canvas page, boot/error overlay, microphone controls, same-tab output view. |

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
