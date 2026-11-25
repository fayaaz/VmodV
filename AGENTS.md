# AGENTS.md

VmodV (fork of modV) — modular audio visualisation app. **Rust-only, single-process native application** (`modv`): engine, GPU compositor, media/audio capture and egui UI in one binary. There is no Electron, Node or web renderer in this tree; the final 1.x/Electron implementation is archived on branch `legacy-electron` / tag `v1.x-final` (see `docs/legacy-electron.md`).

Linux is the release target. Windows and macOS are not supported.

## Commands

Everything runs in Docker. The host only needs Docker (with Buildx) and make — there is no host Rust, Node or Yarn.

- `make build` — **the canonical Linux build**: empties `dist/` and writes the AppImage and `.deb` into it. The AppImage is native-architecture (`VmodV-<version>-x86_64.AppImage` on x86_64, `VmodV-<version>-aarch64.AppImage` on arm64); the `.deb` stays amd64-only (`-amd64.deb`). Used by CI, which builds the arm64 AppImage on GitHub's `ubuntu-24.04-arm` runner. The version comes from `VMODV_VERSION` or, by default, from the git tags via `tools/version.sh` (there is no version literal in the tree). `make clean` removes `dist/`.
- `make appimage-vmodv` — AppImage only for the host architecture (`x86_64` or `aarch64`; skips the slow `.deb` packaging). `make appimage` is an alias.
- `make vmodv-deb` — `.deb` only (runs `tools/vmodv-deb-check.sh` after packaging); `make vmodv-deb-check` re-checks an existing `.deb` (binary, symlinks, desktop file, icon, ISF corpus + textures).
- `make docker-package` — build both installers via the Dockerfile `artifacts` target.
- `make dev` — run the VmodV shell (`--release`) with the host Wayland/X11 session, PulseAudio and `/dev/dri` forwarding. Wayland is preferred when the host has a Wayland socket, otherwise X11 (`DISPLAY_BACKEND=wayland|x11|auto`). `make vmodv-debug` uses the debug profile. Pass args with `make dev ARGS="--frames 300"`.
- `make vmodv` / `make vmodv-debug` — same thing; `dev` is an alias for `vmodv`.
- `make engine` — release workspace build. `make engine-run` — run the headless `modv-engine` binary with GUI forwarding.
- `make engine-test` — `cargo test` (pass ARGS to narrow, e.g. `make engine-test ARGS="-p modv-engine scene_timing"`). CI runs the workspace excluding `modv-app`, then `modv-app --lib`, all serial (`--test-threads=1`) because the GPU/kittest suites share one lavapipe device.
- `make engine-lint` — `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check`. `make lint` is an alias (kept for muscle memory). `make format` — `cargo fmt --all`.
- `make test-e2e-vmodv` — the real-binary e2e harness and parity tests (`e2e_harness`, `e2e_process`, `e2e_scenes`, `e2e_defaults`, `e2e_manifest`, `e2e_colour`, `e2e_regressions`), serial. Wayland preferred, X11/Xvfb fallback; with no display at all the harness starts Xvfb. `make test-e2e` is an alias. See `docs/vmodv-e2e.md`.
- `make rave-reencode` — the one remaining Node usage: a throwaway `node:24-slim` container re-encodes `resources/isf/rave.exe.fs` from `tools/rave-words.txt`. Nothing else in the tree needs Node.
- `make shell` — interactive shell in the Rust toolchain container with the repo bind-mounted at `/app` and a cargo cache volume.
- `make docker-reset` — delete the `modv-cargo` cargo volume; the next cargo target reseeds it.
- `make disk-clean-targets` — drop compiled artifacts from the cargo volume but keep the registry cache; `make disk-clean` prunes stale Docker BuildKit cache.

Commands run against the repo bind-mounted at `/app`. Cargo's registry cache lives in the `modv-cargo` named volume, seeded from the `modv-rust:local` toolchain image. Locally, the gitignored `.env` sets `CARGO_TARGET_DIR=/cargo/target`, which the Makefile forwards into every cargo container so build artifacts (including hundreds of stale build-script output dirs) accumulate in that volume instead of the bind-mounted `crates/target`. CI leaves it unset and caches `crates/target` as before.

CI is `.github/workflows/vmodv.yml` (Rust lint/tests + both installers on pushes/PRs to `main`/`rust-hybrid`) and `.github/workflows/release.yml` (automatic semantic-versioning releases from `main`, see `docs/releasing.md`). There is no Node/browser CI workflow and no rolling nightly — the semantic releases are the installers' distribution channel.

Guards ported from the deleted JS tree live in `crates/modv-app/tests/notices.rs` (third-party notices / NC-SA attribution) and `crates/modv-app/tests/removed_content.rs` (removed Point10 scenes/modules, Cube/three renderer). Do not reintroduce the removed content.

## Test-driven development

**All changes are test-first.** Write a failing test *before* writing implementation code. Never implement a feature or fix a bug by editing source first and adding tests afterwards. Do not consider a task complete while its tests are absent, failing, or skipped.

Workflow for every change:

1. Write the test that encodes the desired behaviour (a unit test wherever possible).
2. Run it and confirm it fails for the right reason (assertion failure, not a compile error). If it passes immediately, the test is not exercising the new behaviour — fix the test.
3. Write the minimum implementation to make it pass.
4. Re-run the test, then refactor with the test green.
5. Run the relevant suite(s) and `make engine-lint` before reporting completion.

- **Default to a unit test.** Pure logic (audio analysis, detectors, data transforms, state reducers, layout policy) must be covered by a unit test first: `make engine-test ARGS="-p <crate> <test_name>"`. New tests live next to the code (`#[cfg(test)]` modules or `crates/<crate>/tests/`).
- UI widget behaviour is covered with `egui_kittest` (including golden snapshots in `crates/modv-ui/tests/`). When a snapshot legitimately changes, re-baseline it and say so in the commit.
- Use the real-binary e2e suite **only** for behaviour that genuinely needs the full app (process lifecycle, rendering, Scene application): `make test-e2e-vmodv`. An e2e test that covers logic which could be unit-tested is not a substitute for the unit test. The harness drives the built `modv` binary over a JSON-lines control socket — see `docs/vmodv-e2e.md`.
- Keep regression tests for bugs fixed; the failing test must reproduce the reported behaviour before the fix.
- Tests must be committed alongside the change — a change without its test is incomplete.

## Rust 3D VJing TDD Rules (subagent-enforced)

- Always run `cargo test --lib` for unit tests and `cargo test --test '*'` for integration tests.
- Never mock `wgpu` GPU hardware contexts unless necessary; isolate pure math, FFT audio analysis, and state machines into testable structs.
- Every task must follow RED -> GREEN -> REFACTOR. Never write production code without a failing test first.

> This repo has no host Rust toolchain: run those cargo commands in Docker via `make engine-test ARGS="-p <crate> --lib <filter>"` (unit) or `make engine-test ARGS="-p <crate> --test <name>"` (integration), and lint with `make engine-lint`. The "Test-driven development" section above remains authoritative for suite scoping. When you invoke `/tdd`, the `orchestrator` agent enforces these rules via worktree-isolated `tdd-tester`, `tdd-implementer` and `tdd-reviewer` subagents.

## Architecture (single process)

The `modv` binary (`crates/modv-app/src/bin/modv.rs` → `src/vmodv.rs`) owns `Store` + `Compositor` + `MediaHub` in-process. There is no IPC boundary, no Vuex mirror and no web view. The headless `modv-engine` binary (`crates/modv-app/src/main.rs`) shares the engine crates for CLI/engine-only use (`--headless`, `--screenshot`, `--state`, `--isf-dir`, `--socket`).

Crates (`crates/Cargo.toml`):

- `modv-engine` — state model, props/input links, presets, Scenes (timing/rows/capture/playlists, default-scene data), tweens, expressions (Boa, AST-allowlist sandbox), the plugin sandbox (`modv-engine::plugins`) and `ModuleRegistry`.
- `modv-render` — wgpu device/surface, texture pool, frame graph, canvas-blend WGSL, compositor.
- `modv-renderers` — shader modules in WGSL, the ISF parser/translator/runtime, the 2D canvas facade + built-in 2D modules, Point10 infrastructure, ISF corpus tooling.
- `modv-media` — audio (bands/tempo/phrase/features, BeatDetektor, cpal/PipeWire capture), GStreamer video (cameras via v4l2, files via uridecodebin, portal screen nodes, test patterns), MIDI (`midir`), OSC, NDI.
- `modv-ipc` — control socket framing + the VMP1 preview ring (used by the headless engine path; the VmodV shell does not use the ring for its UI).
- `modv-ui` — egui UI: `EngineSnapshot` read models, `UiState` action/command queues, theme, shell, resizable/dockable layout (`LayoutState`/`LayoutSettings`, hand-rolled — not `egui_tiles`), Groups, Gallery, Scenes grid, Inspector, Media, Audio, MIDI, NDI, OSC, Info, Plugins, and the control set (RangeControl, colours, vec lanes, tween curve editor, freehand Draw curve editor, texture picker).
- `modv-app` — the binaries, plus the VmodV shell (`vmodv.rs`), window/surface management, the quit coordinator, media dialogs/imports/watchers, the `MediaHub` integration, MIDI/NDI/audio settings, UI layout persistence, preset load/save, ISF hot-reload (`notify`, 500 ms debounce) and the plugin runtime.

Key wiring: the UI reads an `EngineSnapshot` built from the authoritative `Store` and emits `UiAction`s that are dispatched before the next render. The compositor's main texture is registered with `egui-wgpu` as a native texture — the live preview has no readback. Gallery tiles and Scene hover cells render through the engine's isolated preview caches (one preview answered per frame, LRU-cached, uploaded to a bounded egui texture LRU); volumetric Point10 scenes cache a static RGBA8 poster. Each performance-mode row's gutter shows a slow-cadence snapshot of its live Group (GPU-GPU blit, no readback).

## Code map — where to start

Each crate's `lib.rs` opens with a `//!` module map; `crates/modv-engine/src/lib.rs` is the most detailed (it maps modules to their legacy JS origins). Read it before changing a subsystem.

| Task | Open first |
| --- | --- |
| State model, props, inputs, presets, expressions | `crates/modv-engine/src/{engine_state,props,inputs,preset,expressions}.rs` |
| Scenes: timing, rows/cells, capture, playlists, defaults | `crates/modv-engine/src/scene_*.rs`, `scenes_playlist.rs`, `default_scenes.rs` |
| UI → engine data flow | `crates/modv-ui/src/state.rs` (`UiAction`/`UiCommand`), `crates/modv-ui/src/model.rs` (`EngineSnapshot`), `crates/modv-app/src/vmodv.rs` (dispatch), `crates/modv-app/src/compositor.rs` (render) |
| Audio analysis / capture | `crates/modv-media/src/audio/` (`analyser`, `bands`, `beat_detektor`, `tempo`, `phrase`, `features`, `capture`, `pipewire`); app settings in `crates/modv-app/src/audio*.rs` |
| Video / MIDI / OSC / NDI | `crates/modv-media/src/video/`, `midi.rs`, `osc.rs`, `ndi.rs` |
| ISF / WGSL / 2D / Point10 | `crates/modv-renderers/src/{isf,shader,two_d,point10}/`; corpus in `resources/isf/` |
| GPU device, frame graph, textures | `crates/modv-render/src/{device,frame_graph,texture_pool,surface}.rs` |
| Control socket | `crates/modv-ipc/src/` (headless) vs `crates/modv-app/src/control_socket.rs` (native) |

Non-obvious invariants: the UI never mutates the `Store` (it emits `UiAction`s dispatched in `vmodv.rs`); GPU/kittest suites share one lavapipe device so tests are serial; run `make` from the worktree so Docker bind-mounts it at `/app`.

## Persistence (userData)

User data lives in `~/.config/VmodV` (Linux; `MODV_USER_DATA` overrides). Formats shared with the archived 1.x app are unchanged: presets, `scenes.json`, Scenes playlists (`modv-scenes-playlist`), the `media/` profile tree and `window-state.json`. VmodV state is additive: `vmodv-layout.json` (panel/tab placement + dock width), `vmodv-media.json` (ordered/remembered cameras), `vmodv-audio.json`, `vmodv-project.json`, `vmodv-plugins.json` (plugin settings/consent, master switch normalised to disabled until the user consents), `engine-state.json`. `MODV_USER_DATA` overrides the directory for diagnostics; `make vmodv`/`dev` run with the host's real userData unless you set it.

Logging: the VmodV shell writes to **stderr**; the packaged AppImage/`.deb` launch through `packaging/vmodv/log-run.sh`, which tees stderr into `<userData>/logs/vmodv-YYYY-MM-DD.log` and prunes to seven daily files. "Open logs" in the UI opens that folder. (A structured in-app logger is still a P1 residual.)

## JS in this tree (not Electron)

- **Expressions** run in-engine through Boa (`crates/modv-engine/src/expressions.rs` + `crates/modv-app/src/expressions.rs`); the AST allowlist validator is the sandbox bound (no wall-clock budget exists on the compiled-handle path).
- **JS plugins** run in the opt-in, consent-gated Boa plugin host (`crates/modv-app/src/plugin_runtime.rs`): master switch off by default, per-plugin enable switches, discovery from the project profile's `plugin/` folder and `<userData>/plugins`. `modV.ws` is **not** wired (posts are rejected), and there is no filesystem watcher yet (explicit re-scan). The Plugins panel stays disabled until consent.
- **No Node runtime** is shipped or required. `tools/rave-*.mjs` are the only Node scripts (ISF asset tooling, run via `make rave-reencode` in a throwaway container).
- ISF shaders are GLSL translated to WGSL at load; the corpus lives in `resources/isf` and is watched/hot-reloaded from the user ISF folders.

## Point10

Point10 scenes are ported (not vendored) into `crates/modv-renderers` — WGSL/TSL shaders and scene logic, sharing the particle-sprite/audio-bridge helpers. Some scenes are deliberately background-free so they composite over the layer below; a full-frame raymarch such as The Cave is opaque instead. Plane9 XML presets imported from `../Point10` (Sun, Earth, The Cave) keep their own RTT/post chains and live in `scenes/*.rs`; their textures are copied into `resources/point10/textures/` and credited in `THIRD_PARTY_NOTICES.md`. `tests/fixtures/module-parity.json` is the registry manifest guarded by `e2e_manifest` (currently 27 Point10 scenes; Glass Lens was dropped because its upstream CodePen states no licence).

## Display & GPU notes

- `winit` is built with both `wayland` and `x11` features; Wayland is preferred on Wayland sessions, X11 is the supported fallback (`libxkbcommon-x11` is declared by the `.deb`). `make` prefers the host Wayland socket (`DISPLAY_BACKEND=auto`); the e2e harness likewise prefers Wayland and falls back to X11/Xvfb.
- Rendering is Vulkan via `wgpu`. Docker/CI run with Mesa's `mesa-vulkan-drivers` (lavapipe); **(`--device /dev/dri` alone is not enough — the Vulkan ICD is required)**. The container runs as the host UID/GID, so the Makefile adds the `/dev/dri` device GIDs via `--group-add`; without them the GPU nodes are unreadable and wgpu falls back to lavapipe (or fails).
- The reference AMD host reaches ~140 fps at 960×540 with a default Scene; lavapipe measures ~57 fps. The lavapipe number is compositor-sensitive (window resize/occlusion changes it).

## Packaging & release

`make build` (Dockerfile `appimage`/`deb` stages) empties `dist/` and emits the native-arch AppImage (`x86_64` or `aarch64`) plus the amd64-only `.deb` (`vmodv` package: `modv` binary + `vmodv` compat symlink, desktop file, icon, ISF corpus and textures under `/usr/share/vmodv`). CI and releases build the arm64 AppImage natively on the `ubuntu-24.04-arm` runner; there is no arm64 `.deb`. Runtime dependencies (Vulkan/lavapipe, PipeWire, ALSA, GStreamer, `libxkbcommon-x11`) are host packages; the `.deb` declares them. `VMODV_VERSION` names the artifacts; `release.yml` passes the computed release version when building stable artifacts, and locally it defaults to the git-tag version (`tools/version.sh`; no version literal in the tree). The Cargo workspace stays at `0.1.0` internally. Installer checks: `make vmodv-deb-check`, `tools/vmodv-smoke.sh`. **Stable releases are automatic:** `.github/workflows/release.yml` computes the next version from conventional commits on `main` (`feat:` → minor, `fix:` → patch, `<type>!:` / `BREAKING CHANGE:` → major, anything else → no release), tags the released commit `vX.Y.Z` (no version-bump commit), builds and publishes a GitHub release. The `v2.0.0` tag seeds the base line. See `docs/releasing.md`. **Default-Scenes overhauls are `feat(scenes):`, never `feat!:` / `BREAKING CHANGE:`** — removing modules from the seeded Scenes or retiring seeded Scenes (e.g. the v20 heavy-Point10/Chromatic Line/Smoke Room removals, which dropped ten seeded Scenes on upgrade) is routine overhaul work, shipped exactly like the Strobe Riot (v18) and v19 bumps. Reserve `!` / `BREAKING CHANGE:` for breaks that warrant a major bump; the seeded defaults changing for all installs is not one. **The Electron build is not maintained here** — do not "fix" the archived tree; retrieve it read-only from `legacy-electron`/`v1.x-final`.

## Known residuals

This is the single list of work left from the Rust rewrite. The migration
trackers that used to live in `docs/` were removed once 2.0 shipped; their
history is in git, and the pre-cleanup Electron tree is on `legacy-electron`.

- **UI (P1/P3):** undo/redo is disabled and Preferences is a stub. No structured
  daily logger (`Open logs` is a stub) or splash. Palette and font values are
  read-only previews, video textures are typed paths with no transport
  controls, and there is no 2D vector pad.
- **Media (P6):** native file/screen pickers, media-manager file dialogs,
  project profiles and the full media-tree watch are missing. User ISF hot
  reload, the camera panel and preset load/save already work.
- **Plugins (P7):** the opt-in Boa plugin host is not implemented, so the
  Plugins panel stays disabled. `modV.ws` posts are rejected and plugin
  discovery has no filesystem watcher (explicit re-scan only).
- **Testing/parity (P8):** beyond `tools/vmodv-smoke.sh` there is no
  e2e/perf/soak harness. Remaining kittest gaps: Groups keyboard delete,
  tri-state `enabled` and explicit `inheritFrom`, Gallery hover preview,
  Scene-grid automation, packaged window-state/quit checks and host PipeWire
  audio. No NDI, camera or multi-GPU hardware verification is recorded.
- **Packaging (P9):** Flatpak and the clean-VM installer pass (AppImage/`.deb`
  on a fresh VM, desktop integration) remain.
- **Windows:** only `output-0` is wired; the engine `windows/*` registry and
  window-state map already have room for more.
- **Hygiene:** `TWO_D_PROP_FLAGS` / `TWO_D_PREVIEW_WITH_OUTPUT` are still
  companion tables in `module_registry.rs` rather than fields on `TwoDProp` /
  `TwoDModuleMeta`, and a few inert 2D prop-metadata differences (Line `color`
  type, Buffer `bufferId` default, Grid Stretch's dead `meyda:` key) are
  unreconciled. Earth/Sun textures could also be routed through
  `texture_sources`.
- The engine-only headless binary keeps the `modv-ipc` control socket; the
  VmodV shell serves a JSON-lines control protocol on `--socket` instead
  (`docs/vmodv-e2e.md`).
- The committed tree is Rust-only; anything referencing `src/`, `package.json`
  or the worker store is describing the archive — update the docs, not the
  references.

## Key documents

- This file (`AGENTS.md`) is the source of truth for what works and what is left.
- `docs/vmodv-e2e.md` — control socket and real-binary harness.
- `docs/modules/README.md` — generated GitHub Pages module catalogue and its WebAssembly/WebGPU previews (`make pages-data`).
- `docs/vmodv-2.0-release.md` — release artifacts and userData layout.
- `docs/releasing.md` — automated semantic-version releases.
- `docs/legacy-electron.md` — how to retrieve the archived 1.x tree.
- Subsystem notes: `docs/audio-capture-rate.md`, `docs/isf-translation-notes.md`,
  `docs/isf-corpus-report.md`, `docs/compatibility-matrix.md`,
  `docs/milkdrop-presets.md`.
