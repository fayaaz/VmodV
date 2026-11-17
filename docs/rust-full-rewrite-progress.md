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
| P1 — app shell | **Partial** | winit UI window, output window, menus, status bar, theme, `window-state.json` persistence + monitor clamping, shortcuts, About/Shortcuts/Tempo/Command-palette windows, golden snapshot. Logging, splash, native file dialogs remain. |
| P2 — engine in-process | **Core done** | The shell owns `Store` + `Compositor` + `MediaHub` directly; the UI reads an `EngineSnapshot` and emits actions. No IPC, no Vuex mirror, no preview ring on this path. Engine-owned PipeWire/cpal audio is live. `modv-engine` headless still uses the socket. |
| P3 — controls | **Partial** | RangeControl, checkbox, enum, text, colour (RGBA), vec2/3/4, tween, texture picker, event button, prop reset, focused-input link editor. Palette editor, font registry, video transport and the 2D vector pad remain. |
| P4 — panels | **Partial** | Groups/Modules, Gallery (hover previews), Scenes (performance controls, rename, timing, row names, cell copy), Inspector, Media (cameras/sources/test pattern), Info, Plugins, BPM/tap, status graph/meters, Ctrl+K palette. MIDI/NDI/OSC panels and the full media-manager/ISF watcher remain. |
| P5 — docking | Not started | `egui_tiles` is the P0 choice (egui 0.37-compatible). Current layout is fixed panels. |
| P6 — media manager | Not started | `MediaHub` is not yet owned by the native app; ISF watch/registration and the project media watcher remain. |
| P7 — MIDI/plugins | Not started | Expressions already run in-engine; the Boa plugin host and `midir` client remain. |
| P8/P9 — parity/packaging | Not started | No Electron deletion in this run (user decision). |

## What exists today

- `crates/modv-ui` — egui UI crate: `EngineSnapshot` read models, `UiState`
  action/command queues, theme, shell, Groups/Gallery/Scenes/Inspector,
  RangeControl, ColorControl, VecControl, TweenControl, TextureControl and
  `egui_kittest` smoke tests.
- `crates/modv-app` is now a library plus two binaries:
  - `modv-engine` (`src/main.rs`) — unchanged engine/headless/supervisor path.
  - `modv` (`src/bin/modv.rs`) — the native shell (`src/native.rs`).
- `modv-ui` previews: Gallery tiles and Scene cells request the engine's
  isolated preview at most every 150 ms; the shell answers one per frame,
  uploads it to a bounded 32-entry egui texture LRU and the UI draws it. The
  live compositor main texture is registered with `egui-wgpu` directly (no
  readback).
- `window_state.rs` reads/writes the same `<userData>/window-state.json` as
  the Electron app, with atomic writes and the JS normalisation rules.
- `make native` / `make native-debug` run the native shell with the host
  Wayland/X11 session; `MODV_UI=native make dev` selects it from the usual dev
  command. Electron remains the default (`make dev`).

## Verification

- `cargo test -p modv-ui`: 25 unit + 3 kittest tests pass.
- Native debug smoke (`make native-debug ARGS="--frames 120"`): window +
  output window open, 432 registered modules, 62 seeded Scenes, ~41–44 fps in
  a debug build at 960×540 on the reference AMD GPU.
- **P0 release evidence** (`--apply-scene "Lone Chrome"`, host display):
  - Reference AMD RX 9070 XT: **138–143 fps** at 960×470–540, PipeWire audio
    capture active (`audio source pipewire owns capture at 48000 Hz`).
  - lavapipe (Docker, `VK_ICD_FILENAMES=lvp_icd.json`): **128 fps** empty,
    **56.9 fps** with Lone Chrome at 960×540.
  - RSS with the full 354-ISF registry + one applied Scene: **~157 MB**
    (vs the Electron/hybrid app's multi-hundred-MB floor).
  - Written verdict: **GO** for the framework, with the lavapipe content
    number (57 fps) recorded as the one caveat; the reference GPU is well
    above the 60 fps target.
- `cargo test -p modv-engine --lib`: **459 passed**.
- `cargo test -p modv-app --lib`: **414 passed** (serial; GPU-safe).
- `cargo test -p modv-ui`: 25 unit + 3 widget smoke + 1 golden snapshot.
- `make engine-lint` (workspace clippy `-D warnings` + fmt): clean.
- Full workspace run: 789 passed, 1 failed — `point10::scene_tests::
  smoke_room_volume_is_structured` (the fire covers the frame). It reproduces
  on both radv and lavapipe with the same Point10 code as the hybrid baseline
  (`cf9be174`); it is not introduced by the native shell. Tracked as a known
  pre-existing GPU-test failure.

## Next slices

1. Finish the P4 panel set: focused-input link editor, Audio/Video + Screen +
   MIDI/BPM/NDI/OSC panels, command palette/Search, Plugins panel, status
   graph.
2. P5 `egui_tiles` docking + layout persistence; port the golden-layout
   default arrangement.
3. P6 `MediaHub` ownership in the native app (`camera:`/`video:`/`screen:`
   texture sources), media watcher and user ISF watch/registration.
4. P7 Boa plugin host + `midir` MIDI.
5. P0 evidence: release build on lavapipe and the reference GPU, frame trace
   proving no readbacks on the live preview, RSS comparison, signed go/no-go.
6. P8/P9 after the user signs off; no Electron deletion before then.

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

## Known issues / not yet done

- **Point10 volume GPU test**: `smoke_room_volume_is_structured` fails on this
  host on both radv and lavapipe; the scene code is unchanged from the hybrid
  baseline. Not a native-shell regression; left for a rendering pass.
- **P5 docking** is not started; the shell uses fixed panels.
- **P6** has preset load/save (`--preset`) but no media watcher, file dialogs
  or third-party ISF watch/registration.
- **P7** has expressions + native audio but no Boa plugin host or `midir`.
- **P9** packaging/CI and the Electron deletion are deliberately out of scope
  for this run.
- Native output windows are single (`output-0`); the engine `windows/*`
  registry supports more, and the window-state map is ready for more keys.
- There is no structured daily logger in the native shell yet; engine/app
  lines still go to stderr.

## Constraints honoured

- No Electron/Chromium/Node deletion in this run.
- Legacy JS UI and engine remain selectable; `MODV_UI=native` is opt-in.
- Preset/Scene/playlist formats and the media directory are unchanged.
- `modv-ui` is `#![forbid(unsafe_code)]`; the shell keeps the engine's
  single-device/single-queue rule.
