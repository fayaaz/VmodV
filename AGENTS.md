# AGENTS.md

VmodV (fork of modV) — modular audio visualisation app. **Rust-only, single-process native application** (`modv`): engine, GPU compositor, media/audio capture and egui UI in one binary. There is no Electron or Node runtime in this tree; the final 1.x/Electron implementation is archived on branch `legacy-electron` / tag `v1.x-final` (see `docs/legacy-electron.md`). The additive wasm browser shell lives in `crates/modv-web` (see below) and is not the desktop `Store` path.

Linux is the primary release target: a fat AppImage (x86_64/aarch64) and an amd64 `.deb`. CI and releases also ship **best-effort portable ZIPs** for Windows and macOS — no installers, no Linux feature parity (camera, screen and system-audio capture are Linux-only; macOS needs GStreamer installed separately). See the "Windows and macOS (best effort)" section of `README.md`.

## Commands

Everything runs in Docker. The host only needs Docker (with Buildx) and make — there is no host Rust, Node or Yarn.

- `make build` — **the canonical Linux build**: empties `dist/` and writes the AppImage and `.deb` into it. The AppImage is native-architecture (`VmodV-<version>-x86_64.AppImage` on x86_64, `VmodV-<version>-aarch64.AppImage` on arm64); the `.deb` stays amd64-only (`-amd64.deb`). The version comes from `VMODV_VERSION` or, by default, from the git tags via `tools/version.sh` (there is no version literal in the tree); `make version` prints it. `make clean` removes `dist/`. Locally the build base is `debian:trixie-slim`; releases build `.deb`s against several Ubuntu bases via `BASE_IMAGE`/`DEB_SUFFIX`.
- `make appimage` — AppImage only for the host architecture (`x86_64` or `aarch64`; skips the slow `.deb` packaging). The builder verifies the fat bundle with `tools/vmodv-appimage-check.sh`; `make vmodv-appimage-check` re-checks an existing AppImage.
- `make vmodv-deb` — `.deb` only (amd64 hosts only; runs `tools/vmodv-deb-check.sh` after packaging); `make vmodv-deb-check` re-checks an existing `.deb` (binary, symlinks, desktop file, icon, ISF corpus + textures, launcher/logging contract).
- `make docker-package` — build both installers via the Dockerfile `artifacts` target.
- `make dev` — run the VmodV shell (`--release`) with the host Wayland/X11 session, audio and `/dev/dri` forwarding. Wayland is preferred when the host has a Wayland socket, otherwise X11 (`DISPLAY_BACKEND=wayland|x11|auto`). `make vmodv-debug` uses the debug profile. Pass args with `make dev ARGS="--frames 300"`.
- `make vmodv` / `make vmodv-debug` — same thing; `dev` is an alias for `vmodv`.
- `make engine` — release workspace build. `make engine-run` — run the headless `modv-engine` binary with GUI forwarding.
- `make engine-test` — `cargo test` (pass ARGS to narrow, e.g. `make engine-test ARGS="-p modv-engine scene_timing"`). CI runs the workspace excluding `modv-app`, then `modv-app --lib`, all serial (`--test-threads=1`) because the GPU/kittest suites share one lavapipe device, plus static guards (`appimage_bundle`, `appimage_arm64`, `deb_package`, `windows_portable_layout`, `macos_portable_layout`, `ci_*`, `notices`, `removed_content`, `undo_history`, …).
- `make engine-lint` — `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check`. `make lint` is an alias (kept for muscle memory). `make format` — `cargo fmt --all`.
- `make test-e2e-vmodv` — the real-binary e2e harness and parity tests (`e2e_harness`, `e2e_process`, `e2e_scenes`, `e2e_defaults`, `e2e_manifest`, `e2e_colour`, `e2e_regressions`, `e2e_engine_status`, `e2e_scene_editing`, `e2e_user_modules`), serial. Wayland preferred, X11/Xvfb fallback; with no display at all the harness starts Xvfb. `make test-e2e` is an alias. See `docs/vmodv-e2e.md`.
- `make web-check` — wasm32 compile gate for `crates/modv-web` (the browser shell). `make web-build` writes `web/pkg/modv_web.{js,wasm}`; `make web-serve` serves `web/` on port 8000. See `docs/vmodv-web.md`.
- `make preview-wasm` — rebuilds the WebAssembly preview runtime in `docs/assets/wasm/` for the module catalogue; `make module-docs` regenerates `docs/modules/*.json`; `make pages-data` does both. See `docs/modules/README.md`.
- `make rave-reencode` — the one remaining Node usage: a throwaway `node:24-slim` container re-encodes `resources/isf/rave.exe.fs` from `tools/rave-words.txt`. Nothing else in the tree needs Node.
- `make shell` — interactive shell in the Rust toolchain container with the repo bind-mounted at `/app` and a cargo cache volume.
- `make docker-reset` — delete the `modv-cargo` cargo volume; the next cargo target reseeds it.
- `make disk-clean-targets` — drop compiled artifacts from the cargo volume but keep the registry cache; `make disk-clean` prunes BuildKit cache unused for a week. Builds fail early below 20 GB free (`MIN_FREE_GB`).

Commands run against the repo bind-mounted at `/app`. Cargo's registry cache lives in the `modv-cargo` named volume, seeded from the `modv-rust:local` toolchain image. Locally, the gitignored `.env` sets `CARGO_TARGET_DIR=/cargo/target`, which the Makefile forwards into every cargo container so build artifacts (including hundreds of stale build-script output dirs) accumulate in that volume instead of the bind-mounted `crates/target`. CI leaves it unset and caches `crates/target` as before.

Docker image builds go through Buildx and are cached in CI: the Linux jobs build the `rust`/`wasm` images they need with `docker/build-push-action` and `cache-from`/`cache-to: type=gha` (per-architecture scopes for the AppImages, per-Ubuntu-base scopes for the `.deb` matrix, a separate `wasm-toolchain` scope for the browser shell). `make` forwards `DOCKER_BUILD_ARGS` to `docker-rust`/`docker-wasm`, and skips an image whose `org.vmodv.source` label matches the current `Dockerfile` hash and `BASE_IMAGE`, so the later `make` calls in a job reuse the image its build-push step already loaded (`preview-wasm`, `web-check` and `web-build` all build through `docker-wasm`). The guard for this is `crates/modv-app/tests/ci_docker_cache.rs`.

CI has four workflows:

- `.github/workflows/vmodv.yml` — lint/tests + Linux AppImage/`.deb` packaging (x86_64 on `ubuntu-latest`, arm64 AppImage on `ubuntu-24.04-arm`) + a Windows portable build, on pushes/PRs to `main` (path-filtered). The `package` and `windows-portable` jobs are skipped on `main` pushes because releases build those artifacts.
- `.github/workflows/release.yml` — automatic semantic-versioning releases from `main` (see `docs/releasing.md`): lint/tests (`validate`), AppImages, the Ubuntu `.deb` matrix, Windows/macOS portables, then a **draft** release that is published only after validation passes; artifact attach jobs run independently.
- `.github/workflows/pages.yml` — builds the wasm browser shell with `make web-build` and deploys `web/` to GitHub Pages on every push to `main` (with `/modules/` and `/app/` redirects). The generated module catalogue itself is no longer deployed.
- macOS portable bundles (x86_64/arm64) are built **only** by `release.yml` on `main`; the expensive macOS runners are not used for PRs or feature branches.
- `.github/workflows/nightly-cleanup.yml` — nightly (03:00 UTC) + manual cleanup of stale draft releases left by failed/cancelled `release.yml` runs; calls `tools/prune-draft-releases.sh`, which only deletes >1-day-old, `vX.Y.Z`-tagged, bot-authored drafts carrying the release workflow's notes marker. Never touches PRs, published releases or tags. Guarded by `crates/modv-app/tests/release_cleanup.rs`.

There is no Node/Electron CI workflow and no rolling nightly **release** (the nightly job above only prunes stale drafts) — releases are the installers' distribution channel. Guards ported from the deleted JS tree live in `crates/modv-app/tests/notices.rs` (third-party notices / NC-SA attribution) and `crates/modv-app/tests/removed_content.rs` (removed Point10 scenes/modules, Cube/three renderer). Do not reintroduce the removed content. Additional static guards cover CI caches/coverage, packaging layouts, branding, release resilience/resume and the Pages workflow.

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
- `crates/modv-app/tests/ci_test_coverage.rs` fails if a new top-level `modv-app` integration target is not named by CI or the Makefile e2e recipe — add it to both.
- Tests must be committed alongside the change — a change without its test is incomplete.

## Rust 3D VJing TDD Rules (subagent-enforced)

- Always run `cargo test --lib` for unit tests and `cargo test --test '*'` for integration tests.
- Never mock `wgpu` GPU hardware contexts unless necessary; isolate pure math, FFT audio analysis, and state machines into testable structs.
- Every task must follow RED -> GREEN -> REFACTOR. Never write production code without a failing test first.

> This repo has no host Rust toolchain: run those cargo commands in Docker via `make engine-test ARGS="-p <crate> --lib <filter>"` (unit) or `make engine-test ARGS="-p <crate> --test <name>"` (integration), and lint with `make engine-lint`. The "Test-driven development" section above remains authoritative for suite scoping. When you invoke `/tdd`, the `orchestrator` agent enforces these rules via worktree-isolated `tdd-tester`, `tdd-implementer` and `tdd-reviewer` subagents.

## Architecture (single process)

The `modv` shell (`crates/modv-app/src/bin/modv.rs` → `src/vmodv.rs`) owns `Store` + `Compositor` + `MediaHub` in-process. There is no IPC boundary, no Vuex mirror and no web view. The headless `modv-engine` binary (`crates/modv-app/src/main.rs`) shares the engine crates for CLI/benchmark/screenshot runs (`--headless`, `--screenshot`, `--print-adapter`, `--emit-fixtures`, `--isf-dir`; `--state` is used by the windowed paths). The additive wasm browser shell (`crates/modv-web`, wasm32) composes the same `Compositor`, module registry and `modv-ui` shell through the `modv-app` `web` feature; it is not the desktop `Store` path (no filesystem, Scenes persistence, OSC/NDI/MIDI or plugins).

Nine workspace crates (`crates/Cargo.toml`, edition 2024, internal version `0.1.0`):

- `modv-engine` — state model, props/input links, presets, Scenes (timing/rows/capture/playlists, default-scene data + migrations), tweens, expressions (Boa, AST-allowlist sandbox), the plugin sandbox (`modv-engine::plugins`) and `ModuleRegistry`.
- `modv-render` — wgpu device/surface, texture pool, frame graph, canvas-blend WGSL, compositor.
- `modv-renderers` — shader modules in WGSL, the ISF parser/translator/runtime, the 2D canvas facade + built-in 2D modules, MilkDrop, user-authored WGSL/JSON modules, Point10 infrastructure, ISF corpus tooling.
- `modv-media` — audio (bands/tempo/phrase/features, BeatDetektor, cpal/PipeWire capture), GStreamer video (cameras via v4l2, files via uridecodebin, portal screen nodes, test patterns), MIDI (`midir`), OSC, NDI.
- `modv-ipc` — control socket framing + the VMP1 preview ring (used by the headless engine path; the VmodV shell does not use the ring for its UI).
- `modv-ui` — egui UI: `EngineSnapshot` read models, `UiState` action/command queues, theme, shell, resizable/dockable layout (`LayoutState`/`LayoutSettings`, hand-rolled — not `egui_tiles`), Groups, Gallery, Scenes grid, Inspector, Media, Audio, MIDI, NDI, OSC, Info, Plugins, in-app file dialog and startup splash, and the control set (RangeControl, colours, vec lanes with a 2D pad, tween curve editor, freehand Draw curve editor, texture picker with video transport). The frame below the Group list is the Rust **Help** panel (legacy Electron `Info View`): `src/help.rs` holds the portable feature docs and `UiState::help_topic` tracks the last clicked feature. New user-facing text must call Groups "Groups"; `row`/`layer` survive only in legacy JSON keys and internal identifiers.
- `modv-app` — the binaries (`modv`, `modv-engine` and the `gen-module-docs` helper), plus the VmodV shell (`vmodv.rs`), window/surface management, the quit coordinator, media dialogs/imports/watchers, the `MediaHub` integration, MIDI/NDI/audio settings, UI layout/render-settings persistence, preset load/save with undo/redo, structured daily logging, ISF hot-reload (`notify`, 500 ms debounce) and the plugin runtime. Native code is excluded under the `web` feature; desktop is simply the no-`web` configuration.
- `modv-preview-wasm` — WebAssembly runtime for the GitHub Pages module-catalogue previews (clock, deterministic audio simulation, engine uniform byte layouts); the WebGPU draw happens in `docs/assets/preview.js`. Not used by `modv-web`.
- `modv-web` — the browser shell: platform-neutral state plus wasm-only app/mic/storage/output-view modules, built by `make web-build`.

Key wiring: the UI reads an `EngineSnapshot` built from the authoritative `Store` and emits `UiAction`s that are dispatched before the next render. The compositor's main texture is registered with `egui-wgpu` as a native texture — the live preview has no readback. Gallery tiles and Scene hover cells render through the engine's isolated preview caches (one preview answered per frame, LRU-cached, uploaded to a bounded egui texture LRU); volumetric Point10 scenes cache a static RGBA8 poster. Each performance-mode row's gutter shows a slow-cadence snapshot of its live Group (GPU-GPU blit, no readback).

## Code map — where to start

Each crate's `lib.rs` opens with a `//!` module map; `crates/modv-engine/src/lib.rs` is the most detailed (it maps modules to their legacy JS origins). Read it before changing a subsystem.

| Task | Open first |
| --- | --- |
| State model, props, inputs, presets, expressions | `crates/modv-engine/src/{engine_state,props,inputs,preset,expressions}.rs` |
| Scenes: timing, rows/cells, capture, playlists, defaults | `crates/modv-engine/src/scene_*.rs`, `scenes_playlist.rs`, `default_scenes.rs`, `default_scene_migrations.rs` |
| UI → engine data flow | `crates/modv-ui/src/state.rs` (`UiAction`/`UiCommand`), `crates/modv-ui/src/model.rs` (`EngineSnapshot`), `crates/modv-app/src/vmodv.rs` (dispatch), `crates/modv-app/src/compositor.rs` (render) |
| Audio analysis / capture | `crates/modv-media/src/audio/` (`analyser`, `bands`, `beat_detektor`, `tempo`, `phrase`, `features`, `capture`, `pipewire`); app settings in `crates/modv-app/src/audio*.rs` |
| Video / MIDI / OSC / NDI | `crates/modv-media/src/video/`, `midi.rs`, `osc.rs`, `ndi.rs` |
| ISF / WGSL / 2D / MilkDrop / Point10 / user modules | `crates/modv-renderers/src/{isf,shader,two_d,milkdrop,point10,user_module}*`; corpus in `resources/isf/` |
| GPU device, frame graph, textures | `crates/modv-render/src/{device,frame_graph,texture_pool,surface}.rs` |
| Persistence, logging, undo/redo, media watch, plugins | `crates/modv-app/src/{ui_layout,render_settings,window_state,logging,store_ops,media_watch,plugin_runtime}.rs` |
| Browser shell and Pages deployment | `crates/modv-web/src/`, `web/`, `tools/build-web.sh`, `.github/workflows/pages.yml` |
| Module catalogue + preview runtime | `crates/modv-preview-wasm/`, `docs/modules/README.md`, `make pages-data` |
| CI / release / packaging guards | `crates/modv-app/tests/{ci_*,appimage_*,deb_package,*_portable_layout,release_*,pages_workflow,module_docs}.rs` |
| Control socket | `crates/modv-ipc/src/` (headless) vs `crates/modv-app/src/control_socket.rs` (shell JSON-lines protocol) |

Non-obvious invariants: the UI never mutates the `Store` (it emits `UiAction`s dispatched in `vmodv.rs`); GPU/kittest suites share one lavapipe device so tests are serial; run `make` from the worktree so Docker bind-mounts it at `/app`.

## Persistence (userData)

User data lives in `~/.config/VmodV` on Linux (`%APPDATA%\VmodV` on Windows, `~/Library/Application Support/VmodV` on macOS; `MODV_USER_DATA` overrides everywhere). Formats shared with the archived 1.x app are unchanged: presets, `scenes.json`, Scenes playlists (`modv-scenes-playlist`), the `media/` profile tree and `window-state.json`. VmodV state is additive: `vmodv-layout.json` (panel/tab placement + dock width), `vmodv-media.json` (ordered/remembered cameras), `vmodv-audio.json`, `vmodv-render.json` (internal render scale), `vmodv-project.json`, `vmodv-plugins.json` (plugin settings/consent, master switch normalised to disabled until the user consents), `engine-state.json`, and `vmodv-preset.json` as the preset save fallback. `make vmodv`/`dev` run with the host's real userData unless you set `MODV_USER_DATA`.

Logging: the shell writes to **stderr** and also initialises a structured daily log (`crates/modv-app/src/logging.rs`) at `<userData>/logs/vmodv-YYYY-MM-DD.log`, pruned to seven daily files; "Open logs" in the UI opens that folder. The packaged AppImage/`.deb` launch through `packaging/vmodv/log-run.sh`, which tees stderr into the same log. Runtime error paths still go through `eprintln!` rather than the structured logger — integration there is incomplete.

## JS in this tree (not Electron)

- **Expressions** run in-engine through Boa (`crates/modv-engine/src/expressions.rs` + `crates/modv-app/src/expressions.rs`); the AST allowlist validator is the sandbox bound (no wall-clock budget exists on the compiled-handle path).
- **JS plugins** run in the opt-in, consent-gated Boa plugin host (`crates/modv-engine/src/plugins.rs` + `crates/modv-app/src/plugin_runtime.rs`): master switch off by default, per-plugin enable switches, discovery from the project profile's `plugin/` folder and `<userData>/plugins`, frame hooks and RGBA capture. The Plugins tab is available, but its discovered-plugin list and controls are disabled until enabled and consented. `modV.ws` is **not** wired (posts are rejected), and there is no filesystem watcher yet (explicit re-scan).
- **No Node runtime** is shipped or required. `tools/rave-*.mjs` are the only Node scripts (ISF asset tooling, run via `make rave-reencode` in a throwaway container).
- ISF shaders are GLSL translated to WGSL at load; the corpus lives in `resources/isf` and is watched/hot-reloaded from the user ISF folders.

## Point10

Point10 scenes are ported (not vendored) into `crates/modv-renderers` — WGSL/TSL shaders and scene logic, sharing the particle-sprite/audio-bridge helpers. Some scenes are deliberately background-free so they composite over the layer below; a full-frame raymarch such as The Cave is opaque instead. Plane9 XML presets imported from `../Point10` (Sun, Earth, The Cave) keep their own RTT/post chains and live in `scenes/*.rs`; their textures are copied into `resources/point10/textures/` and credited in `THIRD_PARTY_NOTICES.md`. `tests/fixtures/module-parity.json` is the registry manifest (currently 51 Point10 scenes, 33 of them Plane9/Dahl ports; Glass Lens was dropped because its upstream CodePen states no licence), checked by the registry parity test in `module_registry.rs` and by `module_docs.rs`. `e2e_manifest` guards a different file, `tests/e2e-rust/parity.json` (the Electron → Rust port inventory).

## Display & GPU notes

- `winit` is built with both `wayland` and `x11` features; Wayland is preferred on Wayland sessions, X11 is the supported fallback (`libxkbcommon-x11` is declared by the `.deb`). `make` prefers the host Wayland socket (`DISPLAY_BACKEND=auto`); the e2e harness likewise prefers Wayland and falls back to X11/Xvfb.
- Rendering is Vulkan via `wgpu`. Docker/CI run with Mesa's `mesa-vulkan-drivers` (lavapipe); **(`--device /dev/dri` alone is not enough — the Vulkan ICD is required)**. The container runs as the host UID/GID, so the Makefile adds the `/dev/dri` device GIDs via `--group-add`; without them the GPU nodes are unreadable and wgpu falls back to lavapipe (or fails).
- The reference AMD host reaches ~140 fps at 960×540 with a default Scene; lavapipe measures ~57 fps. The lavapipe number is compositor-sensitive (window resize/occlusion changes it).

## Packaging & release

`make build` (Dockerfile `appimage`/`deb` stages) empties `dist/` and emits the native-arch AppImage (`x86_64` or `aarch64`) plus the amd64-only `.deb` (`vmodv` package: `modv` binary + `vmodv` compat symlink, desktop file, icon, ISF corpus and textures under `/usr/share/vmodv`). The AppImage is a **fat** bundle: it carries GStreamer plugins and their library closure, with `$ORIGIN`-relative RUNPATHs, and is checked by `make vmodv-appimage-check`. CI and releases build the arm64 AppImage natively on the `ubuntu-24.04-arm` runner; there is no arm64 `.deb`. The release builds four suffixed Ubuntu `.deb`s (`-ubuntu24.04`, `-ubuntu26.04`, `-ubuntu26.10`, `-ubuntudevel`); the unsuffixed local `.deb` is a CI artifact, not a release asset. Runtime dependencies (Vulkan/lavapipe, PipeWire, ALSA, GStreamer, `libxkbcommon-x11`) are host packages; the `.deb` declares them. `VMODV_VERSION` names the artifacts; `release.yml` passes the computed release version when building stable artifacts, and locally it defaults to the git-tag version (`tools/version.sh`; no version literal in the tree). Installer checks: `make vmodv-appimage-check`, `make vmodv-deb-check`, `tools/vmodv-smoke.sh`. Windows portable ZIPs are built in CI (`tools/windows-portable.sh`); macOS bundles are built only by `release.yml` on main (`tools/macos-portable.sh`) and attached to releases, with layout guards in `crates/modv-app/tests/*_portable_layout.rs`.

**Stable releases are automatic:** `.github/workflows/release.yml` computes the next version from conventional commits on `main` (`feat:` → minor, `fix:` → patch, `<type>!:` / `BREAKING CHANGE:` → major, anything else → no release), tags the released commit `vX.Y.Z` (no version-bump commit), creates a **draft** GitHub release, publishes it after the `validate` job (lint + serial tests) passes, and attaches the platform artifacts via independent jobs. The `v2.0.0` tag seeds the base line. See `docs/releasing.md`. **Default-Scenes overhauls are `feat(scenes):`, never `feat!:` / `BREAKING CHANGE:`** — removing modules from the seeded Scenes or retiring seeded Scenes (e.g. the v20 heavy-Point10/Chromatic Line/Smoke Room removals, which dropped ten seeded Scenes on upgrade) is routine overhaul work, shipped exactly like the Strobe Riot (v18) and v19 bumps. Reserve `!` / `BREAKING CHANGE:` for breaks that warrant a major bump; the seeded defaults changing for all installs is not one.

## Known residuals

This is the list of work left from the Rust rewrite. The migration trackers that used to live in `docs/` were removed once 2.0 shipped; their history is in git, and the pre-cleanup Electron tree is on `legacy-electron`.

- **UI:** no Preferences screen (settings live in dedicated Output/Render, Audio, MIDI, NDI, OSC and Quality panels). The structured daily logger is initialised and "Open logs" works, but runtime errors still go only to stderr. Otherwise undo/redo, splash, editable palette/font, video transport controls and the 2D vector pad are in place and kittest-covered.
- **Browser shell (`crates/modv-web`, deployed to Pages from `main`):** built-in shader/2D/Point10 modules, Groups/Gallery/Inspector, microphone audio (silent until enabled), the 56 default Scenes, per-row performance auto-advance with Fade/shader transitions, Scene-cell and Gallery hover previews, live row monitors, `localStorage` persistence of the live stack/Scenes/tweens, a same-tab output view (full-viewport resolution up to 4K, `F` fullscreen, cog back to the UI) and WebGL2 fallback work. The bundled ISF corpus is embedded in the wasm binary and registered at startup. Still missing: MilkDrop, user ISF folders, filesystem presets/project profiles, performance playlists, OSC/NDI/MIDI/plugins and native output windows (the browser output uses the same tab and canvas). Input Links support audio features, Draw and Tween sources; link expressions and MIDI/OSC sources are still missing. Point10 compute scenes need WebGPU; the WebGL2 fallback logs and skips them.
- **Media:** no native/portal file picker for arbitrary media assets or screen selection (the egui picker covers presets, playlists and ISF folders; screen capture uses XDG portal dialogs). Project profiles, the debounced full media-tree watch, user ISF hot reload, the camera panel and preset load/save already work.
- **Plugins:** the opt-in Boa host and Plugins panel are implemented; remaining gaps are `modV.ws` being unwired, no filesystem watcher (explicit re-scan only) and no bundled `grab-canvas`.
- **Testing/parity:** ten real-binary e2e suites (see `make test-e2e-vmodv`) plus `tools/vmodv-smoke.sh` with an optional bounded soak — the "no e2e harness" gap is closed. Remaining gaps: Gallery hover-preview kittest coverage, and host PipeWire/NDI/camera/multi-GPU hardware verification. CI/release/packaging contracts are covered by static guards (`ci_*`, `appimage_*`, `deb_package`, `*_portable_layout`, `release_*`, `pages_workflow`).
- **Packaging:** Flatpak and the clean-VM installer pass (AppImage/`.deb` on a fresh VM, desktop integration) remain. Windows/macOS portable ZIPs have no installers or Linux feature parity.
- **Windows:** only `output-0` is wired; the engine `windows/*` registry and window-state map already have room for more. Camera, screen and system-audio capture are Linux-only; macOS needs GStreamer installed via Homebrew.
- **Hygiene:** `TWO_D_PROP_FLAGS` / `TWO_D_PREVIEW_WITH_OUTPUT` are still companion tables in `module_registry.rs` rather than fields on `TwoDProp` / `TwoDModuleMeta`, and a few inert 2D prop-metadata differences (Line `color` type, Buffer `bufferId` default, Grid Stretch's dead `meyda:` key) are unreconciled. Earth/Sun textures could also be routed through `texture_sources`.
- The engine-only headless binary keeps the `modv-ipc` control socket; the VmodV shell serves a JSON-lines control protocol on `--socket` instead (`docs/vmodv-e2e.md`).
- The committed tree is Rust-only; anything referencing `src/`, `package.json` or the worker store is describing the archive — update the docs, not the references.

## Key documents

- This file (`AGENTS.md`) is the source of truth for what works and what is left.
- `README.md` — user-facing install/run guide, including Linux installers and best-effort Windows/macOS portables.
- `CHANGES.md` — a checked-off task list keyed by commit hashes / `DEFAULTS_VERSION` markers, not a versioned changelog.
- `docs/vmodv-e2e.md` — control socket and real-binary harness (its coverage-boundary note still lists three suites as out-of-scope, but all ten now run in CI).
- `docs/modules/README.md` — generated module catalogue data and its WebAssembly/WebGPU previews (`make pages-data`; no longer deployed — Pages serves the browser shell at the root instead).
- `docs/vmodv-web.md` — browser/wasm shell, build commands, cut lines and architecture.
- `docs/vmodv-2.0-release.md` — release artifacts and userData layout (version literals there are historical; tags now drive versions).
- `docs/releasing.md` — automated semantic-version releases.
- `docs/legacy-electron.md` — how to retrieve the archived 1.x tree.
- `tests/e2e-rust/README.md` — Electron → Rust spec parity inventory.
- Subsystem notes: `docs/audio-capture-rate.md`, `docs/isf-translation-notes.md`, `docs/isf-corpus-report.md`, `docs/compatibility-matrix.md`, `docs/milkdrop-presets.md`.
