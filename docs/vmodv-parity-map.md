# VmodV parity map (P8)

Maps the legacy Playwright suite to the VmodV implementation's coverage: Rust
engine tests, VmodV `modv-ui`/`modv-app` tests, kittest snapshots, headless
CLI checks, manual hardware checks, or an explicit waiver. Every scenario lands
in exactly one bucket. The rows below are the historical mapping; the legacy
spec paths were removed from this tree in the Rust-only cleanup and live on
branch `legacy-electron`/tag `v1.x-final` (see
[legacy-electron.md](./legacy-electron.md)). The VmodV anchors named in each
row are the current, executable coverage.

Status rows last refreshed for the Rust-only tree (2026-11-18). Legend:
**C** covered, **P** partially covered (VmodV panel or manual step still
missing), **W** waived with a reason, **N** not started.

## Post-cleanup verification

`tests/` now only contains `tests/e2e-rust/` (the committed parity manifest) and
`tests/fixtures/module-parity.json` (the registry manifest). The Vitest and
Playwright suites are archived, not executed. VmodV coverage is:

| Layer | Command |
| --- | --- |
| Workspace unit/integration tests | `make engine-test` (`cargo test --workspace`) |
| VmodV process/scene/parity harness | `make test-e2e-vmodv` |
| Lint | `make engine-lint` (clippy `-D warnings`, rustfmt) |
| Packaged AppImage/.deb checks | `make vmodv-deb-check`, `tools/vmodv-smoke.sh` |

The two JS guards that still described VmodV content moved into Rust:
`crates/modv-app/tests/notices.rs` (third-party notices/NC-SA coverage) and
`crates/modv-app/tests/removed_content.rs` (removed Point10 modules/scenes).
`crates/modv-app/tests/e2e_manifest.rs` already guards `tests/e2e-rust/parity.json`.

| Legacy spec | VmodV replacement / anchor | Status |
| --- | --- | --- |
| `general/rendersTheMainWindow.spec.js` | `modv-ui/tests/shell_snapshot.rs` (golden shell); `crates/target/release/modv --frames N` smoke | C |
| `general/renderersAreRegistered.spec.js` | Rust engine registry tests (`modv-app`/`modv-renderers`) + VmodV Gallery snapshot | C |
| `general/defaultGroupIsFocusedByDefault.spec.js` | VmodV Groups panel exists; first-Group selection and its kittest smoke are still missing | P |
| `general/moduleDataCommitsOnlyWhenChanged.spec.js` | engine delta tests + VmodV action queue | C |
| `general/fullScreenGlContextsAreNotAntialiased.spec.js` | Waived: the VmodV app has no GL contexts | W |
| `general/galleryItemPreviewRendersModule.spec.js` | `vmodv.rs::answer_preview_requests` + Rust preview tests | P (kittest hover pending) |
| `general/galleryItemPreviewsInheritTheComposition.spec.js` | `render_module_preview(with_output)` tests + VmodV `with_output` requests | C |
| `general/galleryGroupInheritsOnlyWhenHovered.spec.js` | engine compositor tests | C |
| `general/isfGalleryCategories.spec.js` | `ModuleMetaView::gallery_category` unit test + registry tests | C |
| `general/searchGalleryAndAddModuleToGroup.spec.js` | VmodV Gallery search + command palette; Gallery→Group drag kittest (`tests/gallery_drag_drop.rs`) | C |
| `groups/createsADefaultGroup.spec.js` | VmodV Groups `+` / engine `groups/createGroup` tests | C |
| `groups/newGroupButtonCreatesANewGroup.spec.js` | same | C |
| `groups/groupNameCanBeChanged.spec.js` | VmodV inline group rename + engine action tests | C |
| `groups/alphaStateCanBeSet.spec.js` | VmodV alpha slider + engine `UPDATE_GROUP_BY_KEY` tests | C |
| `groups/blendModeStateCanBeSet.spec.js` | VmodV blend combo + engine blend tests | C |
| `groups/clearingStateCanBeToggledBetween0and1.spec.js` | VmodV Clear checkbox + engine coercion tests | C |
| `groups/enabledStateCanBeToggledBetween01and2.spec.js` | VmodV enable checkbox (0/1); tri-state 2 remains a legacy-only control | P |
| `groups/inheritAndInheritFromStateCanBeSet.spec.js` | VmodV Inherit checkbox; explicit `inheritFrom` picker not yet exposed | P |
| `groups/pipelineStateCanBeToggledBetween0and1.spec.js` | VmodV Pipeline checkbox + engine tests | C |
| `groups/groupsCanBeRearranged.spec.js` | VmodV up/down `groups/orderByIds` | C |
| `groups/backspaceRemovesFocusedGroup.spec.js` | VmodV `x` button; keyboard delete pending | P |
| `inputLinks/inputConfigUpdatesWhenControlIsFocused.spec.js` | VmodV focused-input link editor (`widgets/input_link.rs` tests) | C |
| `main/quitCleansUp.spec.js` | VmodV `VmodvApp::shutdown` + engine snapshot/shutdown tests; `tools/vmodv-smoke.sh` checks exit-snapshot write and SIGKILL recovery | P (packaged quit cleanup still manual) |
| `main/windowState.spec.js` | `modv-app/src/window_state.rs` unit tests (normalise/atomic/clamp) | C |
| `packaging/linuxBundleIsTrimmed.spec.js` | `tools/vmodv-deb-check.sh` verifies the VmodV package contents; `vmodv.yml` builds the AppImage/.deb. This is the only bundle now: the Electron build is archived | C |
| packaged VmodV smoke | `tools/vmodv-smoke.sh`: headless `--frames --bench`, windowed apply-scene, state round-trip, AppRun log, SIGKILL recovery, optional bounded soak | C (VmodV AppImage run) |
| `presets/generatesAPresetWithExpectedKeys.spec.js` | Rust preset round-trip tests (existing) | C |
| user third-party preset loading (ISF / MilkDrop) | user-ISF watch/hot reload and MilkDrop `.mlk` discovery stay in place (500 ms debounce); no preset corpus is bundled — see [docs/milkdrop-presets.md](./milkdrop-presets.md) | C |
| `scenes/defaultScenes.spec.js` | `default_scenes` Rust tests + VmodV 56-Scene seed | C |
| `scenes/captureLiveState.spec.js` | engine `scenes/captureLiveStateToScene` tests + VmodV Scene menu action | C |
| `scenes/sceneSnapshotsAndPerformance.spec.js` | `scene_ops`/performance tests + VmodV Scenes panel | P (kittest automation pending) |
| `scenes/performanceGrid.spec.js` | `scene_rows`/`scene_columns` tests + VmodV grid | P |
| `scenes/performanceTiming.spec.js` | `scene_timing` tests + VmodV perf bar | C |
| `scenes/performanceCellTiming.spec.js` | `scene_timing` tests | C |
| `scenes/performanceTransitions.spec.js` | `performance`/`scene_transition` tests + VmodV transition capture | C |
| `scenes/gallerylessPerformance.spec.js` | engine performance tests | C |
| `scenes/hoverPreviewIsolation.spec.js` | `scene_preview` isolated-compositor tests | C |
| `scenes/hoverPreviewUsesGalleryContexts.spec.js` | engine preview tests | C |
| `point10/*.spec.js` (8 specs) | existing lavapipe goldens + `modv-renderers` GPU tests; VmodV compositor uses the same engine | C |
| `audio/systemAudio.spec.js` | engine `AudioFeed` tests; VmodV opens PipeWire monitor with input fallback (verified on host); host-PipeWire manual check | P |
| `audio/tempoDetector.spec.js` | `modv-media` tempo tests | C |
| `engine/engineSkeleton.spec.js` | Boundary deleted; VmodV shell tests replace it: single-process smoke, preview texture, output window, `--frames` traces. `tools/vmodv-smoke.sh` covers the packaged lifecycle | P (preview/output kittest pending) |
| `engine/engineParity.spec.js` | Boundary deleted; registry/count tests, the 432-module guard and the VmodV Gallery read model | C |
| `main/windowState.spec.js` (packaged app) | window-state unit tests + manual packaged checklist | P |

## Coverage buckets

- **Rust engine tests** are the reference for every state/Scene/performance/
  audio/compositor/preset behaviour; they are already the source of truth from
  the hybrid.
- **VmodV unit tests** cover `modv-ui` widgets/action payloads,
  `window_state`, VmodV snapshot flattening and startup Scene application.
- **Kittest** covers the shell layout, the RangeControl, the Scenes grid and
  the golden shell image; every new panel gets a render/interaction smoke.
- **Manual/hardware** is still needed for PipeWire system audio, cameras, NDI
  and the packaged AppImage/.deb on a clean VM. The NDI/OSC panel tests are
  headless action/format coverage, not hardware verification.
- **Waived** currently only the GL-context spec (no GL in the VmodV app);
  packaging is covered by the VmodV AppImage/`.deb` checks rather than waived.

## VmodV 2.0 verification evidence (2026-11-18)

`tools/vmodv-smoke.sh` was added as the packaged-app hardening gate. It never
builds and always uses a throwaway `MODV_USER_DATA` (with a per-command
timeout and cleanup). It prefers a built `dist/VmodV-*.AppImage`, then
`--appimage`. When neither exists it assembles the same AppDir layout around
`crates/target/release/modv` from `packaging/vmodv/{AppRun,log-run.sh}`; that
fallback was validated before the release build landed. The evidence below is
from the actual `dist/VmodV-2.0.0-x86_64.AppImage` (launched with
`--appimage-extract-and-run`).

| Check | Result |
| --- | --- |
| `--headless --frames 600 --bench` | exit 0, valid JSON, 600 frames |
| frame time / RSS | 5956.59 fps, mean 0.20 ms, p50 0.16 ms, p90 0.17 ms, p99 0.38 ms, 99.4 MB RSS |
| windowed `--frames 20 --apply-scene "Mycelium Bloom"` | exit 0, Scene applied (3 Groups) |
| state round-trip | `engine-state.json` written; second run restored 3 Groups / 2 Modules |
| SIGKILL recovery | killed 4 s in (before the 30 s periodic save); restart restored the same snapshot, exit 0 |
| packaged log wrapper | `<userData>/logs/vmodv-YYYY-MM-DD.log` written with `[vmodv-launch]` header; stdout JSON untouched |
| bounded soak | 60 s windowed: 3498 frames, 58.30 fps, peak process-group RSS 335 MB |
| display backends | forced Wayland-only and X11-only `--frames 5` runs both exit 0 on RADV RX 9070 XT |
| OSC | `modv-media` real-UDP loopback test + `store_ops::osc_datagrams_apply_to_state_and_mirror` pass; a live VmodV run bound `0.0.0.0:3333` |
| module parity | committed manifest guard: 432 = 15 2D + 27 Point10 + 36 shader + 354 ISF; 56 default Scenes reference 62 distinct module names, all resolve on both sides |

The app snapshot deliberately excludes `osc` runtime data (it is not a preset
key), so OSC is covered at the socket/store boundary rather than by asserting a
value in `engine-state.json`.

### Hardware / environment waivers

- **NDI hardware — waived.** This host has no `libndi.so.5`; VmodV startup
  logs `NDI unavailable` and continues cleanly. Sender/receiver behaviour
  against real NDI hardware is not verifiable here.
- **Multi-GPU matrix — waived.** Only one adapter was available (AMD RADV
  GFX1201). NVIDIA/Intel/llvmpipe and multi-adapter layouts are untested.
- **Clean-VM installers — waived here.** The packaged VmodV AppImage was
  launched with `--appimage-extract-and-run`, but installing the AppImage and
  `.deb` on a clean VM (and desktop-menu/launcher integration) remains a
  release checklist item.
- **Host audio/camera content — waived.** A VmodV run opened the PipeWire
  monitor capture at 48 kHz, but no audio-content assertion or camera
  enumeration against hardware is included.
- **2 h soak — waived.** Only the bounded 60 s windowed soak above was run;
  long soaks are not appropriate on a developer desktop.

## Open P8 work

Closed since the first pass (2026-11-17):

- camera resolution/FPS and remembered/restored camera order (`c211457e`,
  `b6ae28a0`);
- tween curve editor (`006c9704`);
- resizable sections, in-app floating windows and detachable/dockable tabs
  persisted to `vmodv-layout.json` (`f359e459`, `d2951184`);
- NDI and OSC panels (`dbc2aa76`);
- live capture-rate reporting (`e8f02b08`, `4e57f81b`);
- Gallery drag-to-Group and marquee names (`3bd065d2`, `73feb02a`);
- VmodV AppImage, `.deb`, `vmodv-deb-check` and `vmodv.yml` (`da724d45`,
  `a331368c`);
- the exact 432-module registry guard (`10ac008d`, `dde600a4`);
- user-ISF watch/hot reload and preset load/save (`19a71602`, `0d83d639`).

Still open:

1. Turn the remaining `P` rows into `C` with kittest interactions or VmodV
   integration tests: Groups keyboard delete, tri-state enabled and explicit
   `inheritFrom`, Gallery hover-preview kittest, Scene grid automation,
   packaged window-state/quit checks and host PipeWire audio.
2. Add a VmodV e2e/perf/soak harness. `tools/vmodv-smoke.sh` now covers the
   packaged headless/windowed lifecycle, state round-trip, crash recovery,
   launcher logging and a bounded soak; the remaining gap is kittest/UI
   interaction coverage plus a clean-VM installer pass.
3. P6 media manager: native file/screen pickers, media-manager file dialogs,
   project profiles and the full media-tree watch.
4. P7: the opt-in Boa plugin host (D5), then enable the Plugins panel.
5. P3 residuals: palette editor, font registry, video transport and the 2D
   vector pad.
6. Multi-output windows (single `output-0` today), a structured daily logger
   and the P1 splash/file dialogs.
7. P9: Flatpak and the clean-VM installer pass. The stable release pipeline is
   the automated semantic-versioning workflow (`release.yml`, see
   `docs/releasing.md`); Electron/Node deletion is done and the archived tree is
   documented in [legacy-electron.md](./legacy-electron.md).
8. The clean-VM manual checklist for audio/cameras/NDI. No NDI/GPU hardware
   verification is recorded by this refresh; see the hardware waivers above
   for exactly what the VmodV smoke gate does and does not cover.
