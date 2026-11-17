# Rust full rewrite — progress tracker

Tracks the native single-process implementation of
[rust-full-rewrite.md](./rust-full-rewrite.md) on the `rust-hybrid` branch.
The Electron app and the JS engine/UI stay in the tree and remain the default;
the native path is opt-in with `MODV_UI=native` / `make native`.

Last updated: 2026-11-17.

## Current status

| Milestone | Status | Notes |
| --- | --- | --- |
| P0 — framework spike | **GO (with one caveat)** | `modv-ui` + real `modv` binary; shared `wgpu` device, egui native-texture preview, output window, RangeControl/Scene-cell probes, kittest golden. Release evidence recorded below; lavapipe reaches 128 fps empty / 57 fps with a default Scene (below the 60 fps content target). |
| P1 — app shell | **Partial** | winit UI window, output window, File/Edit/Scene/View/Help menus, status bar, theme, `window-state.json` persistence + monitor clamping, keyboard shortcuts (Ctrl+K/S/Enter, 1–9, F1, F, Escape, Ctrl+drag), About/Shortcuts/Tempo/Command-palette/Scene-timing/row-names/tween-curve/input-link windows, golden snapshot. Undo/redo are disabled and Preferences is a stub; logging, splash and native file dialogs remain. |
| P2 — engine in-process | **Core done** | The shell owns `Store` + `Compositor` + `MediaHub` directly; the UI reads an `EngineSnapshot` and emits actions. No IPC, no Vuex mirror, no preview ring on this path. Engine-owned PipeWire/cpal audio is live. `modv-engine` headless still uses the socket. |
| P3 — controls | **Partial** | RangeControl (slider, typed value, reset, E/T badges), checkbox, enum, text, colour (RGBA), vec2/3/4 lane controls, tween control + keyframe curve editor, texture picker, event button, palette swatch preview (read-only), font value (read-only), focused-input link editor. The palette editor, font registry, video transport and the 2D vector pad remain. |
| P4 — panels | **Partial** | Groups/Modules, Gallery (All/sections/search, hover previews, drag-to-Group, marquee names), Scenes (performance start/stop, in-order/random, min/max/fade, row monitors, row names/timing/copy, per-cell disable), Inspector incl. the focused Input Link editor with Audio/MIDI/Tween/OSC/State/Expression, Media (ordered/remembered/restored cameras, resolution/FPS, test pattern, source status), NDI, OSC, MIDI, Audio, Info, Plugins (deliberately disabled), BPM/tap, status graph/meters and the Ctrl+K palette. Native file/screen media pickers and the media-manager dialogs remain. |
| P5 — docking | **Done (custom layout)** | Resizable Groups/Inspector/dock/centre sections; every right-panel tab detaches to the strip, a dock column or an in-app floating window; panel sizes, dock width, tab placements and window bounds persist to `<userData>/native-layout.json` (atomic, debounced, clamped). This is the hand-rolled `LayoutState`/`LayoutSettings` model, not `egui_tiles`. |
| P6 — media manager | **Partial** | `MediaHub` is owned by the native shell: camera panel (ordered/remembered/restored inputs + resolution/FPS), test pattern and source status, with engine `media/start-file`/`start-screen` plumbing. User ISF folders are watched and hot-reloaded (`notify`, 500 ms debounce), and `--preset` load + File → Save preset round-trip. Native file/screen pickers, media-manager file dialogs, project profiles and the full media-tree watch remain. |
| P7 — MIDI/plugins | **Partial** | Expressions run in-engine (Boa AST allowlist), native MIDI input is live (`midir`, hot-plug, Note/CC/latches, clock→BPM), and the MIDI panel edits the four device flags. The opt-in Boa plugin host (D5) remains; the Plugins panel is deliberately disabled until it lands. |
| P8/P9 — parity/packaging | **Started (P8); native AppImage + .deb + CI** | `docs/native-parity-map.md` refreshed. `make appimage-native` and `make native-deb` build the native installers in Docker (ISF corpus + textures bundled; host GStreamer/PipeWire/Vulkan declared by the `.deb`), `make native-deb-check` verifies the archive, and `.github/workflows/native.yml` runs Rust lint/tests plus both packages. Electron remains the default release. Flatpak, the 2.0.0 version bump, switching the nightly release to Rust and deleting Electron remain. |

## What exists today

- `crates/modv-ui` — egui UI crate: `EngineSnapshot` read models, `UiState`
  action/command queues, theme, shell, resizable/dockable layout, Groups,
  Gallery (drag sources), the Scenes grid with row monitors, Inspector, Media,
  Audio, MIDI, NDI, OSC, Info, Plugins, the control set (RangeControl,
  ColorControl, VecControl, TweenControl + curve editor, TextureControl) and
  `egui_kittest` coverage.
- `crates/modv-app` is now a library plus two binaries:
  - `modv-engine` (`src/main.rs`) — unchanged engine/headless/supervisor path.
  - `modv` (`src/bin/modv.rs`) — the native shell (`src/native.rs`).
- `modv-ui` previews: Gallery tiles and Scene cells request the engine's
  isolated preview at most every 150 ms; the shell answers one per frame,
  uploads it to a bounded 32-entry egui texture LRU and the UI draws it. The
  live compositor main texture is registered with `egui-wgpu` directly (no
  readback). The read-back pixels are premultiplied RGBA, so the upload path
  un-premultiplies them (`modv_ui::un_premultiply_rgba`) before creating the
  egui texture — the old flatten-to-opaque kept the alpha-scaled RGB and
  turned transparent halos into dark fringes over light backgrounds.
- Scenes-grid row monitors: each performance row's gutter shows a
  slow-cadence snapshot of its live Group. The shell copies the compositor's
  already-rendered Group texture (a plain GPU-GPU blit, no readback, no
  isolated state) at the cadence picked by the pure
  `modv_ui::panels::row_monitor` policy — at most one row per frame, every
  row inside a ~3 fps band — and registers it with `egui-wgpu` as a native
  texture. Rows without a live Group show a subtle "no layer" placeholder,
  monitors dim to 35% while performance mode is off, and the snapshot is
  dropped with its Group on Scene applies/resizes so no texture leaks across
  crossfades.
- `window_state.rs` reads/writes the same `<userData>/window-state.json` as
  the Electron app, with atomic writes and the JS normalisation rules.
- `ui_layout.rs` persists the native panel/tab/window document to
  `<userData>/native-layout.json`; `media_settings.rs` remembers the ordered
  camera selection and per-camera modes in `<userData>/native-media.json`.
- `make native` / `make native-debug` run the native shell with the host
  Wayland/X11 session; `MODV_UI=native make dev` selects it from the usual dev
  command. Electron remains the default (`make dev`).

## Verification

- Native debug smoke (`make native-debug ARGS="--frames 120"`): window +
  output window open, 432 registered modules, 62 seeded Scenes, ~41–44 fps in
  a debug build at 960×540 on the reference AMD GPU.
- **P0 release evidence** (`--apply-scene "Lone Chrome"`, host display):
  - Reference AMD RX 9070 XT: **138–143 fps** at 960×470–540 in the
    unobstructed runs; a later run on the same host settled at **59.7 fps**
    after the window manager resized/occluded the output (Fifo pacing), so
    the number is compositor-sensitive. PipeWire audio capture was active
    (`audio source pipewire owns capture at 48000 Hz`) and MIDI was available.
  - lavapipe (Docker, `VK_ICD_FILENAMES=lvp_icd.json`): **128 fps** empty,
    **57.5 fps** with Lone Chrome at 960×540.
  - RSS with the full 354-ISF registry + one applied Scene: **~157 MB**
    (vs the Electron/hybrid app's multi-hundred-MB floor).
  - Written verdict: **GO** for the framework, with the lavapipe content
    number (57 fps) recorded as the one caveat; the reference GPU is well
    above the 60 fps target.
- Earlier verification pass (`08254041`, before the P4/P5 panel expansion):
  `cargo test -p modv-engine --lib` **459 passed**; `-p modv-app --lib`
  **422 passed** (serial; GPU-safe), including the MIDI, user-ISF watcher and
  preset round-trip tests; `modv-ui` 25 unit + 3 widget smoke + 1 golden;
  `make engine-lint` clean.
- Current test inventory as of `40fdb8fb` (counted from `#[test]` in the
  tree, not a fresh full run): `modv-engine` **476**, `modv-app` lib **519**,
  `modv-ui` **216** unit + kittest/integration, `modv-media` **257**,
  `modv-renderers` **794**. The committed tree at `f239ad56` has **218** after
  the concurrent theme commit's two `modv-ui` slider tests.
- Last recorded full workspace run: **1951 passed** at `6e5cf6be`, before the
  P4/P5 panel expansion. `.github/workflows/native.yml` now runs clippy/fmt,
  the workspace suite (with `modv-app` serial) and the native package builds
  on pushes and PRs; this docs-only refresh did not re-run the full suite.
- The former full-workspace failure (`789 passed, 1 failed`:
  `point10::scene_tests::smoke_room_volume_is_structured`) is closed:
  `9e4a5a40` re-baselined the check on the real curl field.

## Next slices

1. P3 residuals: palette editor, font registry, video transport and the 2D
   vector pad.
2. P6 residuals: native file/screen pickers, media-manager file dialogs,
   project profiles and the full media-tree watch.
3. P7: opt-in Boa plugin host (D5) and enabling the Plugins panel.
4. P8: native e2e/perf/soak harness, multi-output windows, and manual
   GPU/NDI/camera checks (no NDI hardware verification is recorded yet).
5. P9: Flatpak, the 2.0.0 version bump, switching the nightly release to Rust
   and deleting Electron only after sign-off.
6. P1 residuals: structured daily logger, splash and native file dialogs.

## Review findings folded in

- P0 review nits: RangeControl registered default, dead `SceneGridState.context`
  removed, kittest name corrected, action helpers shared, texture picker uses
  engine ids.
- Review rounds 56–68: native audio restored (PipeWire monitor → input
  fallback), camera enumeration cached at 1 Hz, restored window bounds clamped
  to the primary monitor, whole-scene shader transitions captured/prepared,
  preview errors re-reported periodically, palette focus/shortcut races fixed,
  the invalid `add` blend token replaced with `lighter`, Info/Plugins tabs
  added, and the golden snapshot re-baselined.
- Landed from other review-fix commits: Scene-cell live-Group clear and
  strict stamped capture, default-Scene bass-link smoothing ids, image decoder
  header probe, `removeGroup` module release, stale support/docs claims, ISF
  `three` category aliases and the literal `\n` comment.
- Expressions sandbox hardened (`b84553ac`): AST allowlist/validator replaces
  the bypassable denylist, constructor/prototype/eval/globalThis/import/
  require/allocating-method/BigInt/regex/loop shapes are rejected, the
  compiled-function global is no longer discoverable, and normal preset
  expressions are pinned. Boa 0.22 offers no wall-clock budget on the
  compiled-handle call path; the structural caps and allowlist are the bound,
  documented in the module.

## P6/P7 additions

- **User ISF hot reload** (`19a71602`): `notify` watches `<userData>/media/isf`
  and each project `isf/` folder, coalesces 500 ms bursts, rebuilds
  `ModuleRegistry` and swaps it into the compositor. Verified live: dropping a
  second `.fs` raised the registered count 433 → 434 without restart.
- **Preset round-trip** (`0d83d639`): `--preset <path>` loads a six-key preset
  at startup and File → Save preset writes it back; unit test restores into a
  fresh Store.
- **MIDI** (`539854bb`, `4522f73f`): `midir` hot-plug at 1 Hz, Note On toggling,
  CC normalisation, `ccLatch`/`noteOnLatch`, 24-PPQN clock→BPM, and the same
  `midi/*` actions the engine stores. MIDI tab exposes the four flags.
- **Native audio** (`9d3c1fc4`): engine-owned PipeWire monitor → input fallback,
  live on the host (`audio source pipewire owns capture at 48000 Hz`).

## P4/P5/P9 additions

- **Gallery/Groups** (`3bd065d2`, `73feb02a`): tiles are drag sources into any
  Group block; the Gallery is an all-module multi-row grid and long names
  marquee on hover.
- **Scenes grid** (`56745632`, `4c3af40f`, `578b802a`, `6c5f3687`): per-row
  live layer monitors, in-order/random performance order, native-only per-cell
  disable flags and named-row cell mapping.
- **Media/tween/rate** (`b6ae28a0`, `c211457e`, `006c9704`, `e8f02b08`,
  `4e57f81b`): ordered/remembered/restored cameras, advertised resolution/FPS
  pickers, the keyframe curve editor, and the live capture rate in the Audio
  panel, status bar, Tempo window and Info panel.
- **NDI/OSC** (`dbc2aa76`): engine-backed tabs for NDI output/discovery/
  receivers and OSC listeners/addresses. Covered by kittest action/format
  tests only; no NDI network run is recorded.
- **Layout** (`f359e459`, `d2951184`): resizable sections, in-app floating
  windows and detachable/dockable tabs persisted in `native-layout.json`; no
  `egui_tiles`.
- **Packaging** (`da724d45`, `a331368c`): native AppImage and `.deb`,
  `native-deb-check`, and the `native.yml` lint/test/package workflow.

## Known issues / not yet done

- **P1** still lacks a structured daily logger (`Open logs` is a stub), a splash
  screen and native file dialogs.
- **P3** palette and font values are read-only previews, video textures are
  typed paths without transport, and vec controls have no 2D pad.
- **P6** has user-ISF watch/hot reload, the camera panel and preset load/save;
  it still lacks native file/screen pickers, media-manager file dialogs,
  project profile creation and a full media-tree watcher.
- **P7** has expressions, native audio and a `midir` client with a MIDI panel;
  the opt-in Boa plugin host (D5) is still missing and the Plugins panel stays
  disabled.
- **P8** has no native e2e/perf/soak harness yet; native coverage is unit +
  kittest plus the headless CLI. No NDI/GPU hardware verification is recorded
  for the new panels.
- Native output windows are single (`output-0`); the engine `windows/*`
  registry supports more, and the window-state map is ready for more keys.
- **P9** has a native AppImage (`make appimage-native`), Debian package
  (`make native-deb`, checked by `make native-deb-check`) and a Rust-only CI
  workflow (`.github/workflows/native.yml`). Flatpak, the 2.0.0 native version
  bump, switching `nightly.yml` to Rust and the Electron/Node deletion are
  still pending.

## Constraints honoured

- No Electron/Chromium/Node deletion in this run.
- Legacy JS UI and engine remain selectable; `MODV_UI=native` is opt-in.
- Preset/Scene/playlist formats and the media directory are unchanged.
- `modv-ui` is `#![forbid(unsafe_code)]`; the shell keeps the engine's
  single-device/single-queue rule.
