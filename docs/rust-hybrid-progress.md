# Rust hybrid engine — progress tracker

Tracks implementation of [rust-hybrid.md](./rust-hybrid.md) on the `rust-hybrid`
branch. Updated as milestones land; each box is a commit or a small group of
commits. Test-first per AGENTS.md; Rust tests are authoritative for ported
logic, the Vitest suite stays until parity is signed off.

Tooling: the engine builds only inside Docker (`make engine` / `engine-test` /
`engine-lint` / `engine-run`); `crates/` is a workspace. The engine binary is
`crates/target/release/modv-engine`. `make disk-clean` prunes BuildKit cache;
`make build`/`appimage` refuse to start below 20 GB free.

## Verification snapshot

- Rust workspace: **1811 tests pass** (modv-app 305, modv-engine 439,
  modv-ipc 42, modv-media 233, modv-render 22, modv-renderers 768).
- Vitest: **1165 tests pass** across 94 files (engine mirror-parity replay,
  preview/read helpers, supervisor restart budget, preload scoping, sync
  replay, plugin host, engine media controller/paths, module meta and payload
  shapes).
- Real-music analyser check (opt-in fixture): 12 s of the noiseblock radio
  stream decoded to 48 kHz mono, 1199 analysis frames — bands peak at
  bass 0.85 / mid 0.48 / treble 0.30, beat envelope 1.0, tempo 160 BPM, lane
  spread 0.42 (`MODV_AUDIO_FIXTURE=… cargo test -p modv-media --test
  audio_stream_fixture`).
- Packaged-engine smoke (CDP, `MODV_ENGINE=1`): the Gallery renders all **432**
  registered modules (36 shader + 15 2D + 27 Point10 + 354 ISF) with engine
  thumbnails; Scene hover previews paint the hovered cell; BPM tweens animate
  (Chromantic Line `smoothing` 0.252 → 0.209 over 0.9 s); the status bar shows
  live FPS/size and the detected BPM.
- **End-to-end music reaction with engine-owned capture**: playing the
  noiseblock radio stream to the host sink and launching with
  `MODV_AUDIO_SOURCE=pipewire` logs "audio source pipewire owns capture" and
  ignores the renderer pump; over 2.5 s the engine bands move
  (bass 0.73→0.30→0.73, mid 0.58→0.84) at **160 BPM** and the linked
  Chromatic Line gain tracks it (1.217–1.269).
- Headless benchmark (packaged engine, 1280×720, 600 frames):
  **5885 fps**, p99 0.332 ms.
- Final packaged build (all review fixes, Point10 textures bundled): the
  engine logs **zero** asset fallbacks, runs at 107 fps at 1080×720 with the
  status bar live, and the live radio still drives Boxy (140 BPM, bass 0.35,
  linked gain moving).
- Latest packaged AppImage (`MODV_ENGINE=1`, all review fixes): full e2e
  **144/144**, including engineParity. Media smoke checked directly: V4L2
  enumeration lists the two UVC cameras (`/dev/video0`, `/dev/video2`) with
  caps, `/dev/video0` starts and paints a Webcam module, `groups/UPDATE_GROUP`,
  `groups/updateGroupName` and `groups/removeGroup` update the engine and the
  mirror, and no engine SIGSEGV on startup (the previous libcamera provider
  crash is gone).
- Engine-mode UI verification (packaged app, CDP): Inspector
  alpha/enable/blend commits reach the engine and the compositor (alpha 0.5
  halves the contribution, disabling every module leaves an opaque black
  frame); restored Webcam scenes (`WEBCAM2`, `WEBCAM2-EDGEBLUR`) render the
  live feed with both cameras running and `enabled` arriving as booleans; the
  media library's project-prefixed paths (`/default/video/…`) resolve under
  `media.path`; a Gallery-added module has its prop/meta Inputs in both the
  engine and the mirror (Inspector populated); Module Enable and Group
  Clearing/Pipeline accept the Checkbox's 1/0 values; `rave.exe` renders upright
  (texture-free ISF generators now use the legacy bottom-left coordinate
  system).
- Engine vs legacy at 1920×1080 (same scenes, Electron UI open): the engine
  holds **120 fps** in every scene (Smoke Room, Geometric Tunnel, Chromatic
  Cascade, Rolling Fire, DVD Afterglow) at 720p/1080p/1440p; legacy holds its
  60 fps cap. With the Gallery visible the engine's p99 was 46–52 ms and
  200–300 frame gaps/10 s from thumbnail polling; after the visible-only
  polling, volumetric poster cache, one-preview-per-frame budget and
  640×360/30 fps preview it is **p99 23.9 ms, max 31.7 ms, 1 stutter/10 s**.
  Preview forwarding costs ~0.02 ms mean (the engine presents its own window;
  the VMP1 ring is UI-only) and is not the stutter cause.
- `make lint`: clean (one pre-existing warning in an untouched component).
- e2e: **144/144 pass** against the packaged AppImage — the full legacy suite
  plus the engine-mode specs (`engineSkeleton`: preview frames publish, engine
  kill survived and restarted, renderer creates no WebGL/WebGPU context;
  `engineParity`: 432 registered modules, audio-link min/max, Point10
  transparency, output preview pixels).
- Packaged-engine smoke (CDP against the AppImage, `MODV_ENGINE=1`): the UI
  mirror receives all **59 seeded Scenes** via `state/sync`; applying the
  default Scene "Lone Chrome" builds the Foreground group + Point10 Chromatic
  Line module and the composited preview renders (8 % lit, full-range pixels);
  performance mode starts 3 rows, runs crossfades and stops without ghosts.
- Abrupt Electron death (SIGKILL/container teardown) leaves **no orphaned
  engine** (`PR_SET_PDEATHSIG`).

## Phase 0 — Tooling

- [x] Rust stage + engine-build stage in `Dockerfile`; `.dockerignore` /
      `.gitignore` exclude `crates/target` and core dumps at any depth
- [x] Makefile targets `docker-rust`, `engine`, `engine-test`, `engine-lint`,
      `engine-run`, `disk-guard`, `disk-clean`; named cargo registry volume
- [x] `crates/` workspace skeleton (modv-app, modv-engine, modv-ipc,
      modv-media, modv-render, modv-renderers)
- [x] electron-builder `extraResources: crates/target/release/modv-engine →
      modv-engine`
- [x] lavapipe (Mesa Vulkan) + libxkbcommon-x11 in the toolchain/rust stages

## M0 — Spike — complete

See [rust-hybrid-spike.md](./rust-hybrid-spike.md): native Wayland Vulkan at
vsync (120 fps, p99 ≈ vsync period, 3.4 % CPU, 103 MB RSS), headless 6137 fps
at 720p, ChromaticLine ported with unit tests, go/no-go = **GO**.

## M1 — Engine skeleton — complete

Control protocol, VMP1 preview ring, engine supervisor (spawn/connect retry,
heartbeat watchdog, restart budget, snapshot restore), shutdown wiring,
`window.engine` preload API, renderer engine client, `MODV_ENGINE=1` gating,
engine e2e spec. Fixed along the way: X11 `libxkbcommon-x11` dependency,
`fs.rmSync` behaviour in this Electron build, bench-seconds suicide in
supervisor mode, lost `engine/state` events, engine-mode renderer TypeErrors.

## M2 — Shader / ISF — complete

- [x] modv-engine state model (groups/modules/inputs/props/blend/SWAP), delta
      stream, preset round-trip + Scene/playlist detection (96 tests)
- [x] modv-render FrameGraph, LRU texture pool, canvas-semantics blend WGSL
      for all 28 modes (21 tests)
- [x] Shader renderer: uniform contract, wrapper shim, all 36 modules in WGSL,
      naga-validated (24 tests)
- [x] ISF: parser/upgrader, ES→desktop-450 preprocessor with the full macro
      set, naga GLSL→WGSL translator, overrides hook, multi-pass runtime with
      PERSISTENT/FLOAT buffers; corpus report — **354/354 fragments and 36/36
      vertex sidecars translate (100 %) with no overrides**, including hoisted
      global initialisers, sampler parameters, fall-through returns and matrix
      varyings (`docs/isf-corpus-report.md`)
- [x] ISF modules registered in the engine: corpus bundled as `resources/isf`,
      discovered via `--isf-dir`, props/meta synthesised, compositor rendering
      with per-frame uniforms + group-canvas image inputs (354 registered)
- [x] Engine integration: EngineState store, GPU compositor, real previews,
      mutations + delta stream (44+ tests)
- [x] Mirror parity: `--emit-fixtures` + `tests/unit/engine-mirror.test.js`
      replay 19 mutations into the real Vuex mirror with deep-equal state

## M3 — 2D modules — complete

- [x] wgpu canvas-2D facade (paths/arcs, 4× MSAA fill/stroke, transforms,
      clip, drawImage incl. self-sampling, cosmic-text/swash text) — 54 tests
- [x] 15 hand-written 2D modules ported with JS meta/props; per-module canvases
      in the compositor; Ball renders through the packaged preview (CDP)
- [x] Palette tween colours advance on the frame clock

## M4 — Point10 — complete (26 of 26 scene classes)

- [x] Scene trait/SceneContext, AudioBridge + P10Audio, shared particle
      alpha/sprite + random, postfx chain (UnrealBloom/OutputPass/ShaderPass/
      Composer), compositor integration, interim band mapping
- [x] Batch 1: ChromaticLine (audio-driven), ChromaticWave, ParticleField,
      SpiralParticles, Waveform
- [x] Batch 2: SquareTunnel, LightTunnel, Wormhole (100k billboards, 5 px
      sprites), ItIsComingForYou — composer bloom/ACES, Boxy
      `preserveBloomAlpha`, transparent-background parity with a lavapipe
      alpha test
- [x] Batch 3: WaterRipple (WaveSim CPU field with a 64-point Node reference
      match), GlassLens (raymarched glass over an internal Chromatic Wave
      backdrop), Mycelium, AroundPlus — shared asset loader with procedural
      fallbacks, phrase-colour helpers
- [x] Batch 4: UltraHDR (torus-knot morph + analytic mirror BRDF), MirrorCube
      (ping-pong feedback), ColorCube (512 instances), Boxy (panorama env +
      `preserveBloomAlpha`), SoundCube (16-bit trail); shared cube mesh and
      p9 shading helpers
- [x] Batch 5a: HollowCube (15k instances + fbm/Shader2 chain),
      AttractorParticles (TSL compute → fragment MRT sim), DvdRoom (TSL smoke →
      seeded puffs, procedural logo)
- [ ] Batch 5b in flight: GeometricTunnel (SSAO), RollingFire, SmokeRoom,
      LatentBackrooms (Afterimage)
- [x] WebGPU/TSL fidelity: SmokeRoom, DvdRoom, RollingFire and BallOfFire now
      run the JS GPU volume simulation + raymarch (compute passes, shadow
      march, fire ramp); the puff/billboard substitutes are gone. BallOfFire
      (frozen ball, transparent overlay) registered as the 26th scene.
      The one-time curl-noise precompute now runs with the scene's uniforms
      written (grid dims + turbulence frequency) — the zeroed block had made
      its `gid >= u.grid.xyz` guard reject every workgroup, leaving the field
      empty and silently removing all thermal/ambient curl turbulence from the
      four volume scenes (the fire puffed radially instead of trailing the
      ball). AttractorParticles remains the fragment-MRT port (structurally
      matching)
- [ ] Golden images (optional; behaviour parity is the priority), warm
      cache/instance eviction

## M5 — Scenes / performance — complete

- [x] scenes core logic (timing/rows/transition-pick/capture/preview-data/
      columns/cells/link-defaults) — 269 tests
- [x] `default-scenes.js` → `data/default-scenes.json` (59 Scenes,
      DEFAULTS_VERSION 12) + seeding/refresh/removal logic
- [x] Scene apply/save/capture, playlists, scenes.json persistence, row-name
      remap, scene-list deltas
- [x] Performance runtime: start/stop/tick, per-row dwell, crossfade ghosts,
      row-fade blending assigned to both group ids, Fade for manual changes,
      event-driven deltas
- [x] Scene hover previews via `preview/scene`: an isolated compositor renders
      one Scene row (fresh ids, saved props) at the requested size, cached per
      `(sceneId,row,width,height)` and invalidated by `scene_list_revision`
      (~1.1 ms warm at 240×135); Scenes.vue polls at ~30 fps and blits with
      `putImageData` (legacy worker path unchanged)
- [x] Gallery/output previews in the UI: the main panel blits the composited
      preview ring; gallery tiles poll `preview/module`, which the supervisor
      queues (`Reply::Deferred`) and the window loop answers up to 4/frame
      from an isolated LRU-cached module instance (shader/2D/Point10/ISF)
- [ ] Scene hover previews through `preview/scene` (in flight)
- [x] Shader whole-Scene transitions: the six native effects (crossfade/
      dissolve/pixelate/glitch/wipe/circle) as WGSL passes, the outgoing frame
      captured at transition start, Fade fallback (performance mode always
      fades), manual changes honour `allowedTransitions`
- [x] Rename/removal migration for default Scenes: version-keyed rules,
      seeded-only renames (id/payload preserved) and removals, collision and
      idempotency handling, applied before seeding; the production table is
      empty until the first rename ships with a bump

## M6 — Media — partial

- [x] AudioBands, tempo/phrase detectors, feature smoothing (37 tests)
- [x] Meyda feature extractors, BeatDetektor, MIDI clock → BPM (43 tests)
- [x] OSC server, raw MIDI ingestion, NDI send/receive via runtime FFI
      (51 tests; NDI needs the SDK at runtime)
- [x] Audio analysis parity in Rust: `AudioAnalyser` (2048 Blackman FFT,
      bands/lanes/cells/AGC, tempo/phrase/BeatDetektor, Meyda, smoothing)
      with a 24-frame JS reference fixture within 1 % and a real-music check
- [x] Audio capture primitives: cpal input devices and a native PipeWire
      monitor stream (replaces `pw-record`), bounded queues, `AudioFeed`
- [x] **Video/media capture in the engine (WS-10)**: GStreamer camera
      enumeration and capture (v4l2), video files (uridecodebin, looping),
      PipeWire portal screen nodes (pipewiresrc) and test patterns, newest-wins
      frame slots; engine MediaHub uploads frames into pooled textures and the
      Webcam module draws the selected 1-based camera source. Renderer UI
      drives engine sources in engine mode (no getUserMedia/ImageCapture/
      WebCodecs), shows status and reconnects. Validated with a real camera
      (non-native 1000×700), a PipeWire screen node (1920×1080) and a file clip.
      Screen capture in the UI is gated until Electron exposes the portal
      PipeWire node id (the engine path is ready)
- [x] Expressions: Boa-based `new Function` evaluation in the engine, applied
      at every JS `applyExpression` site, with every-frame assignments and the
      expressions/* actions
- [x] Plugins: engine-mode JS plugin host in the renderer (hooks, props,
      stores, UI extensions) forwarding writes to the engine
- [x] Renderer → engine audio feed (default source): the pump forwards the
      full payload and beats; the engine drives shader/2D/ISF/Point10 audio
- [x] Engine-side input-link evaluation: getter/state/mutation links,
      smoothing, min/max, meta inputs, `inputs/*` actions, 10 Hz deltas
- [x] Engine-owned capture wiring: `--audio-source renderer|pipewire|
      input[:device]` (env `MODV_AUDIO_SOURCE`), `--audio-device`,
      `--list-audio-devices`. The engine pumps `AudioFeed` before links/render,
      owns bands/BPM/kick, ignores the renderer's writes while active
      (warned once) and republishes meter bands at 10 Hz; capture failures
      fall back to the renderer pump. `renderer` stays the default
- [x] Tween runtime (WS-12a): `TweenDescriptor`/`TweenRuntime`/`TweenSet` port
      `buildFrames` exactly (BPM durations, Math.round frame counts,
      loop/alternate, steps, 41 penner easings verified against anime.js);
      Input Links resolve tween paths read-then-advance, `beats/SET_BPM`
      rebuilds durations and `fps/SET_FPS` rebuilds frames
- [x] Expression assignments (`new Function` in the JS) and JS plugins:
      Boa evaluates assignments in-engine; plugin hooks run in a renderer-side
      host that forwards writes to the engine
- [x] Camera/screen/video capture (GStreamer): cameras (V4L2 enumeration via
      the `v4l` crate after the libcamera provider crashed), files and portal
      screen nodes; the UI gates screen capture until Electron exposes a
      PipeWire node id

## Known gaps (M7 sign-off)

- **Module parity vs the legacy AppImage**: quantified in
  [module-parity.md](./module-parity.md) — both runtimes register the same
  **432** modules (36 shader + 15 2D + 27 Point10 + 354 ISF). `Point10 Earth`
  and `Point10 Sun` are ported, and the `three` renderer/`Cube` is gone from
  both sides; Texture 2D, ISF image inputs and ISF transitions are
  engine-owned now. The remaining gap is third-party module loading (no engine
  install path), plus the prop/meta hygiene items listed there.
- **Engine stays opt-in (`MODV_ENGINE=1`).** Cameras and video files are now
  engine-owned, but engine-mode **screen capture** waits on an Electron API
  that exposes the portal PipeWire node id (the GStreamer `pipewiresrc` path is
  implemented and tested against a node). Video pause/playback-rate transport
  is not modelled (restart the source). The built-in `grab-canvas` plugin
  needs the worker's composited output canvas and is disabled with a reason.
- Expressions and plugin hooks are now engine-supported; `osc` in expression
  scope is still the empty object (`osc/*` listeners feed `osc.data` Input
  Links, but expressions are not handed that state). Expression source is
  capped and a denylist rejects native allocation/loop/context escapes; the
  engine trusts user presets but not hostile shared playlists with arbitrary
  JS.
- Known compositing limitation: Point10/ISF scene targets use straight alpha
  (`alpha = max(rgb)`) while the internal frame graph is premultiplied. Over
  opaque/bright backdrop rows the engine adds slightly too much colour;
  default Scenes put these layers on black rows, so current content is
  unaffected (documented decision: keep the scene outputs straight until a
  composite-time premultiply flag lands).
- ISF transition-shaped modules render as Scene transitions: the six native
  transitions plus the 27 transition-capable ISF corpus shaders; unknown names
  still filter to Fade. Third-party ISF transition shaders follow the
  module-loading gap above (no engine install path yet).
- The ISF corpus translates 354/354 fragments and 36/36 vertex sidecars with
  no overrides; the override mechanism (embedded, fingerprinted, validated)
  remains for third-party shaders. The corpus gate asserts a checked-in
  baseline (100 %).
- The packaged engine links `libpipewire-0.3` for system-audio capture; the
  AppImage relies on the host providing it (standard on current distros). The
  `.deb` depends on the distro's package set; `MODV_ENGINE=0` (the default)
  never launches the engine, so the legacy renderer still runs without it.
- Engine-mode UI keeps MIDI, OSC and local fonts enabled: `midi/*`
  device/channel state, explicit OSC listeners (`osc.data` Input Links) and
  `fonts/SET_LOCAL_FONTS` all have engine arms, so the MIDI panel, the BPM
  panel's MIDI source and the OSC Listen/Stop control work. Output Windows are
  native engine windows now (see "Post-review parity fixes"), so the View menu
  item and the `windows/createWindow` action stay enabled in both modes. The
  boot warning names only JavaScript plugins.
- Shader transitions freeze the outgoing frame and render at full output size
  (the JS re-renders the outgoing groups and caps the transition target at
  1920×1080): behaviour parity, not pixel parity.
- Remaining P3 review items (defensive slicing, preset null-hole tolerance,
  hostile `sceneRow` decode, hex timings, `stroke_text` mask re-uploads, final
  delta flush, windowless supervisor parking) are listed at the end of
  `docs/rust-hybrid-review.md`.

## M7 — Parity and release — in progress

- [ ] Full e2e green **against the engine default** (currently opt-in); keep
      `MODV_LEGACY_RENDERER` until parity
- [ ] Performance harness results vs baseline; release packaging verification
      on a clean machine
- [ ] Remaining ISF overrides; goldens for all renderers/scenes
- [ ] User JS modules/plugins known-gap documentation and the Rust plugin ABI

## Post-review parity fixes

- **Engine-mode Output Windows.** The engine opens extra native winit windows
  that present the same composited frame: `windows/createWindow { id?, name?,
  width?, height?, x?, y?, monitor?, fullscreen? }` records a spec in
  `crates/modv-app/src/windows.rs`, queues a native Open and emits the same
  `windows/ADD_WINDOW` mirror record the legacy action built;
  `windows/removeWindow { id }` queues Close and emits
  `windows/REMOVE_WINDOW` (also emitted when the window manager closes a
  window). The frame loop drains the commands once per frame, creates one
  `winit::Window` + `wgpu::Surface` per output on the shared device/queue with
  `AutoNoVsync`, and blits the shared main texture into every output surface in
  the primary frame's command encoder — no second render. Sizes are clamped to
  64..=8192 logical pixels; `F`/double-click toggles each window's fullscreen
  and Escape leaves it. `window-handler.js` dispatches the engine actions
  instead of `window.open`, the View → New Output Window menu item is enabled in
  engine mode, `state/sync` carries the open windows, and the renderer replays
  them through `windows/SET_WINDOWS`. The multi-surface path has an env-gated
  host smoke test (`MODV_ENGINE_WINDOW_SMOKE=1`); the action/registry semantics
  are plain `modv-app` unit tests.
- **Output-window fullscreen.** `F` (without Ctrl/Meta/Alt) and a left
  double click toggle borderless fullscreen on the engine's native window
  (winit `set_fullscreen`, Escape leaves it); the legacy Output Window popup
  uses the same shortcuts and fullscreens its `documentElement` because its
  canvas has been transferred to the worker. Covered by `modv-app` unit tests
  and `tests/unit/output-window-controls.test.js`.
- **GeometricTunnel surface maps.** The port bound neither the brushed-metal
  grain (`grainMap`/`roughnessMap`/`emissiveMap`) nor the equirect studio
  environment, so the hoops rendered as flat paint. Both maps are uploaded and
  sampled triplanar in the surface shader; the base tint is now the sRGB →
  linear conversion of the JS `0x3a4358` (using the raw bytes made the metal
  ~5x too bright).
- **ISF filters preserve group transparency.** Filters with an `inputImage`
  sample the group canvas but usually return alpha 1. The engine keeps a
  transparent canvas between rows, so Vignette/Edge Blur and co. painted the
  whole group opaque black and hid every row below. The compositor now masks a
  filter's output alpha by the canvas alpha (generators, which have no image
  input, are unchanged). Chromatic Abberation uses the alpha-preserving
  mainImage wrapper for the same reason. Default Scenes: version 13 drops
  CMYK Halftone from Halftone Harbor and fixes MBOX Light Tunnel's `dhue` at 0
  (no hue tween).
- **Review round-20 fixes.** Hover previews flatten over opaque black on
  read-back (`putImageData` is straight alpha; the live frame is
  premultiplied and presented over black), so translucent tiles no longer
  darken. A request that resolves after the pointer leaves is dropped instead
  of repainting the cleared tile. Switching between two monitors no longer
  tears down the new capture: a stale `MediaStreamTrack.stop()` targets its
  own capture session instead of the module-wide `active` slot. The Audio
  panel delegates `SET_SYSTEM_AUDIO_STATE` to `modV.setSystemAudioState`
  (engine mode skips the UI-only commit). `backgroundThrottling: false` is
  now engine-only, and the legacy output popup ignores auto-repeat F and text
  controls.
- **Gallery hover previews are lazy and composited over the live output.**
  Engine-mode tiles are blank until the pointer enters (no idle/visibility
  polling), then request ~10 fps of `preview/module` with `withOutput: true`.
  The engine scales the current main frame into the preview's base, seeds
  shader/ISF image inputs with it and blends the module layer over it, so a
  filter previews on what is actually playing and a generator appears on top.
  Leaving the tile stops the requests and clears its canvas. Isolated previews
  remain the default for other callers (`withOutput` false).
- **Per-output monitor audio inputs (Firefox-style).** The Audio Input picker
  now lists "Monitor of …" for every PipeWire output device (`pw-dump` sink
  discovery in `src/main/audio-monitors.js`), plus the browser's microphones.
  Selecting one records that sink's monitor with `pw-record --target <sink>`
  and feeds the renderer's analyser, which drives both the UI meters and the
  engine's audio-reactive scenes. The single legacy "System Audio (loopback)"
  entry stays as the fallback when no sinks are discovered (or on non-Linux).
  Requires the host PipeWire tools (`pw-dump`/`pw-record`); the `.deb` should
  depend on `pipewire-bin`.
- **Texture-free ISF generators are upright.** `gl_FragCoord` is bottom-left
  in WebGL but top-left in wgpu. Texture-free shaders already used the
  bottom-left vertex shader, but the pixel coordinate stayed top-left, so
  generators that build their camera from `gl_FragCoord` (The Pool Rooms,
  `st_Ms2SD1`, and 29 other corpus raymarchers) rendered vertically mirrored.
  The translator now routes `gl_FragCoord` (and `isf_FragCoord`) through a
  Y-flipped helper for texture-free shaders; textured shaders keep the
  top-left origin so input sampling stays upright.
- **Audio pump while the output window is watched.** Engine mode drives the
  renderer's analyser from `requestAnimationFrame`; Chromium throttled it when
  the main window was occluded by the engine's own output window, freezing
  `meyda/SET_BANDS` and every audio-reactive scene (the DVD logo's beat pulse
  included). The main window now sets `backgroundThrottling: false`, asserted
  by `engineSkeleton.spec.js`.
- **ISF sidecar filters render with the WebGL vertex conventions.**
  Two defects made every `.vs` filter look wrong next to the legacy renderer.
  (1) The runtime drew sidecar shaders with the shared oversized full-screen
  triangle. Those shaders evaluate non-linear vertex maths from
  `isf_FragNormCoord` (`clamp`ed blur offsets, Sobel taps); at the triangle's
  off-screen corners the clamp saturates before the visible area, so
  interpolating back across the screen stretched the coordinates — Edge Blur
  sampled about half the frame, anchored at the bottom-left (the reported
  "extra zoom" / diffuse overlay). Sidecars now draw the six-vertex quad the
  WebGL renderer uses, evaluated on the screen corners. (2) The generated
  skeleton emitted the GL bottom-left `isf_FragNormCoord` while wgpu samples
  textures from the top-left, so sidecar filters sampled the group canvas
  vertically mirrored; it now matches `DEFAULT_VERTEX_WGSL`. New GPU tests:
  a pass-through sidecar over a red-top/blue-bottom source must not flip, and
  a `clamp`ed varying must read the clamped edge value (its absence was the
  triangle's half-size stretch). The Edge Blur test now runs the pass chain
  long enough to fill its one-frame-lagged buffers.
- **Input Link smoothing works end to end.** `meyda/getSmoothingId` and
  `meyda/removeSmoothingId` are implemented in the engine (the slider used to
  hit the dispatch wildcard, receive `null` and drop its args). The component
  captures the id before clearing its field, restores it from the saved link
  args on reopen, and the near-silence gate in `audio-bands.js` stops the AGC
  turning dither into a full-scale bass reading. Input Links read the
  `AudioBands` grid means again, matching pre-Rust levels.
- **Engine default Scenes are on version 13.** The embedded
  `default-scenes.json` now matches the JS set: `Halftone Harbor` no longer
  contains `CMYK Halftone`, and `MBOX Light Tunnel` fixes `MBOX3`'s `dhue` at
  0 and drops its BPM tween. `ensure_default_scenes` applies the
  `get_default_scenes_to_replace` payload swap before seeding (id, name,
  timing and enabled flag kept), so an install already on v12 receives the
  edits in place instead of only fresh installs getting them.
- **Point10 previews honour boolean props.** The Gallery/Scene preview path
  read props with `Value::as_f64`, so `useAgc: false` was dropped and preview
  tiles kept AGC on; it now uses the same coercion as the live compositor.
  The Audio Feature smoothing slider also captures its smoothing id before
  clearing the field and restores it from the saved link args, so turning
  smoothing down actually releases the entry.
- **The Scene grid can save and manage Scenes in engine mode.** Saving the
  live stack (`scenes/saveCurrentAsScene`) builds the six-key preset document
  from the settled layer stack, adds Scene metadata and appends it; before
  this the button hit the dispatch wildcard and no file was written. The
  column actions (`addEmptyScene`, `moveScene`, `setSceneEnabled`,
  `updateSceneName`, `updateSceneTiming`, `resetSceneTiming`) are implemented
  too. The grid cell actions (`createSceneCellGroup`,
  `captureSceneRowToScene`, `clearSceneCell`, `copySceneCellToScene`) and the
  Gallery focus lifecycle (`groups/disableGalleryItemModules`,
  `groups/focusGalleryItem`), `groups/orderByIds` and
  `groups/duplicateModule` (fresh inputs, links and expressions carried over)
  are implemented, as are `beats/SET_BPM_SOURCE` (tap/MIDI BPM only applies
  from the selected source), `fonts/SET_LOCAL_FONTS`, `dataTypes/createType`,
  the `ndi/*` actions and `osc/createServer`. Still hitting the wildcard:
  `groups/SWAP`/`modules/SWAP` (acknowledged no-ops), `media/*`, `midi/learn`,
  `scenePreview/*` and the `outputs/*` auxiliary-canvas actions
  (`windows/createWindow` is implemented natively now).
- **Point10 Old Movie retired.** The sepia/scratch shader effect is removed from
  both catalogues (`Point10Effects.js` and the Rust WGSL port), leaving 30
  Point10 effects / 36 shader modules. Its only shipped preset use — Boxy
  Arcade's Effects row — now references the `Sepia Tone` ISF filter, and the
  defaults moved to version 14 so installs on version 13 receive the edit in
  place. A removal guard and engine-side tests pin the absence.
- **ISF image inputs sample real textures.** A saved texture prop resolves to
  a GPU texture instead of always binding the group canvas: image assets
  (decoded, cached LRU, media-root relative), live module/row outputs
  (`isf:`/`two-d:`/`point10:`/`group:`) and running media sources
  (`camera:`/`video:`/`screen:`). Unassigned props keep the canvas, and
  filters are alpha-masked only when they actually sample it, so an assigned
  photo filter is no longer erased over a transparent group.
- **Engine-owned Output Windows.** `windows/createWindow`/`removeWindow` open
  extra native winit/wgpu windows that blit the primary composited frame;
  closing one (window manager or action) drops its surface and updates the
  mirror. The renderer no longer calls `window.open` in engine mode, and the
  View menu / Output Windows controls work.
- **Engine-owned screen capture.** `media/pick-screen` opens the desktop
  portal's own ScreenCast picker on a worker thread, defers the reply until
  the user answers, starts the returned PipeWire node and keeps the portal
  session alive until `media/stop-source`; `media/unavailable` / `cancelled`
  are reported distinctly. The Screen Capture panel uses it in engine mode.
- **MIDI and OSC are wired** (see the earlier entry): device/channel state,
  MIDI-clock BPM, explicit OSC listeners, `osc.data` Input Links.
- `media/setMediaDirectoryPath` now reaches the engine's compositor, so ISF
  texture props resolve media-library image paths.
- **NDI input and output are engine-owned.** The runtime keeps one lazy
  sender on its own thread (drop-oldest queue, measured-FPS or target rate,
  optional nearest-neighbour resize) and one capture thread per enabled
  receiver; receiver frames upload to `ndi:<receiverId>` textures that ISF
  texture props, Texture 2D and the Webcam source list resolve. Discovery is
  asynchronous; the store implements the JS `ndi/*` mutations/actions with
  their exact delta payloads and reports `available: false` (panel notice,
  actions disabled) when libndi cannot be loaded. Live send/receive needs a
  host with libndi (set `MODV_NDI_LIB` if it is not in the loader path).
- **Input Link tween editor works in engine mode**: `dataTypes/createType`
  forwards `tween` creation to the engine tween store and returns the
  descriptor the editor links to. The A/B debug `groups/SWAP`/`modules/SWAP`
  commits are acknowledged no-ops (the engine has no swap buffer).
- **MediaStream selection state is UI-plane**: source/video-settings commits
  go to the mirror only in engine mode, so the audio pump and device pickers
  read the same state without engine warnings.
- **ISF Scene transitions render in engine mode.** TransitionEffect gained an
  `Isf { name, document }` variant; the reserved `Fade`/`None` names win
  before the corpus lookup (the corpus ships `Fade.fs`), the six natives stay
  unchanged, and a transition-shaped document (`startImage` + `endImage` +
  `progress`) renders between the captured outgoing texture and the incoming
  frame with the transition timeline as `progress`. Compile/draw failures
  fall back to Fade. The pass keeps a bounded 4-shader IsfRuntime, and the
  performance picker now includes every registry transition-shaped module.
  GPU tests cover classification, a red→blue mix, the uncompilable fallback
  and an end-to-end frame; the performance tests cover the shader path and a
  filter falling back to Fade.
- **The `three` renderer and `Cube` are removed from both runtimes.** The
  engine never registered `Cube` (no `three` module type, the last Gallery
  gap); the legacy worker's `renderers/three/` tree is deleted along with the
  worker glob, the ThreeJS Gallery category and the `three` aliases in
  `module-category.js`. `renderersAreRegistered` now expects
  `["2d", "isf", "shader"]`. `tests/unit/cube-removed.test.js` guards against
  reintroduction (files gone, no source/test references, seeded default
  Scenes do not reference `Cube`). The `three` npm package stays — Point10
  scenes import it directly.
- **`Point10 Geometric Tunnel` is removed from both runtimes** (module,
  wrapper, Rust port and its exclusive SSAO pass). The five default Scenes
  whose Background/Foreground hero was it were retuned, not deleted:
  Geometric Tunnel -> Square Tunnel, Golden Spiral Tunnel -> Light Tunnel,
  Reactor Glow -> Color Cube, Crosshatch Cipher -> Wormhole, Digital Crystal
  Tunnel's screen overlay -> Spiral Particles (keeping its treble alpha
  link). The four Background heroes gained a bass-driven alpha swell
  (0.45 -> 1) so they breathe on the beat. DEFAULTS_VERSION is 16 in JS and
  Rust; the two renamed Scenes the refresh list missed (Golden Spiral
  Tunnel, Reactor Glow) joined it, and the module joined
  REMOVED_DEFAULT_SCENE_MODULES, so installs on any previous version rebuild
  the five in place (ids/timing/enabled kept). The seeded-name refresh path
  and the long-neglected-library path both cover them. Unit coverage pins
  the new heroes, the bass alpha links and the in-place refresh;
  point10-removed-effects.test.js guards the removal.
- **Screen blend mode implemented** (see the earlier blend commit): the
  engine now renders `screen` correctly, which every default Scene's
  Foreground group (and HOTSHOWERRAVE) already used — it used to degrade to
  `normal` with a warning.
- **Scene timing can be edited in bars, and phrase flips drive transitions.**
  `performance.timingUnit` (`"seconds"` default, `"bars"`) changes what the
  stored `minSeconds`/`maxSeconds` mean; the global pair and every Scene
  `timingOverride` convert through seconds at the live BPM via
  `scenes/setTimingUnit`, so switching units keeps the durations. A bar is 4
  beats and an invalid BPM falls back to 120. `scenes/tick` now accepts the
  analyser's one-frame `phrase_drop` pulse and applies the legacy
  `shouldAdvanceRow` rule: the per-cell deadline forces the change, while a
  phrase flip changes Scene only when `random() < switchChance(dwell, min,
  max)` (0 before the minimum, 1 at/after the maximum). The native Scenes
  bar/dialog show the active unit with a resolved-seconds hint; `SceneView`
  exposes an override's minimum and maximum separately (it used to pass the
  maximum for both). JS compatibility: Electron ignores `timingUnit` and
  would read bar numbers as seconds, so switch back to seconds before opening
  the project in an older build. Unit coverage: conversion boundaries at
  120/60/0/NaN BPM, `setTimingUnit` persistence, phrase/no-phrase tick
  behaviour and the kittest selector/dialog round trip; the screenshot suite
  was re-baselined.
