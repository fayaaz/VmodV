# Native parity map (P8 draft)

Maps the legacy Playwright suite under `tests/e2e/spec/` to the native
implementation's coverage: Rust engine tests, native `modv-ui`/`modv-app`
tests, kittest snapshots, headless CLI checks, manual hardware checks, or an
explicit waiver. The plan expects every scenario to land in exactly one
bucket; this is the first pass at HEAD `71f707ad` and is regenerated at P8
start. Status rows refreshed at `f239ad56` (2026-11-17).

Legend: **C** covered, **P** partially covered (native panel or manual step
still missing), **W** waived with a reason, **N** not started.

| Legacy spec | Native replacement / anchor | Status |
| --- | --- | --- |
| `general/rendersTheMainWindow.spec.js` | `modv-ui/tests/shell_snapshot.rs` (golden shell); `crates/target/release/modv --frames N` smoke | C |
| `general/renderersAreRegistered.spec.js` | Rust engine registry tests (`modv-app`/`modv-renderers`) + native Gallery snapshot | C |
| `general/defaultGroupIsFocusedByDefault.spec.js` | native Groups panel exists; first-Group selection and its kittest smoke are still missing | P |
| `general/moduleDataCommitsOnlyWhenChanged.spec.js` | engine delta tests + native action queue | C |
| `general/fullScreenGlContextsAreNotAntialiased.spec.js` | Waived: the native app has no GL contexts | W |
| `general/galleryItemPreviewRendersModule.spec.js` | `native.rs::answer_preview_requests` + Rust preview tests | P (kittest hover pending) |
| `general/galleryItemPreviewsInheritTheComposition.spec.js` | `render_module_preview(with_output)` tests + native `with_output` requests | C |
| `general/galleryGroupInheritsOnlyWhenHovered.spec.js` | engine compositor tests | C |
| `general/isfGalleryCategories.spec.js` | `ModuleMetaView::gallery_category` unit test + registry tests | C |
| `general/searchGalleryAndAddModuleToGroup.spec.js` | native Gallery search + command palette; Gallery→Group drag kittest (`tests/gallery_drag_drop.rs`) | C |
| `groups/createsADefaultGroup.spec.js` | native Groups `+` / engine `groups/createGroup` tests | C |
| `groups/newGroupButtonCreatesANewGroup.spec.js` | same | C |
| `groups/groupNameCanBeChanged.spec.js` | native inline group rename + engine action tests | C |
| `groups/alphaStateCanBeSet.spec.js` | native alpha slider + engine `UPDATE_GROUP_BY_KEY` tests | C |
| `groups/blendModeStateCanBeSet.spec.js` | native blend combo + engine blend tests | C |
| `groups/clearingStateCanBeToggledBetween0and1.spec.js` | native Clear checkbox + engine coercion tests | C |
| `groups/enabledStateCanBeToggledBetween01and2.spec.js` | native enable checkbox (0/1); tri-state 2 remains a legacy-only control | P |
| `groups/inheritAndInheritFromStateCanBeSet.spec.js` | native Inherit checkbox; explicit `inheritFrom` picker not yet exposed | P |
| `groups/pipelineStateCanBeToggledBetween0and1.spec.js` | native Pipeline checkbox + engine tests | C |
| `groups/groupsCanBeRearranged.spec.js` | native up/down `groups/orderByIds` | C |
| `groups/backspaceRemovesFocusedGroup.spec.js` | native `x` button; keyboard delete pending | P |
| `inputLinks/inputConfigUpdatesWhenControlIsFocused.spec.js` | native focused-input link editor (`widgets/input_link.rs` tests) | C |
| `main/quitCleansUp.spec.js` | native `NativeApp::shutdown` + engine snapshot/shutdown tests; `tools/native-smoke.sh` checks exit-snapshot write and SIGKILL recovery | P (packaged quit cleanup still manual) |
| `main/windowState.spec.js` | `modv-app/src/window_state.rs` unit tests (normalise/atomic/clamp) | C |
| `packaging/linuxBundleIsTrimmed.spec.js` | `tools/native-deb-check.sh` verifies the native package contents; `native.yml` builds the AppImage/.deb. The Electron bundle is unchanged | C |
| packaged native smoke | `tools/native-smoke.sh`: headless `--frames --bench`, windowed apply-scene, state round-trip, AppRun log, SIGKILL recovery, optional bounded soak | C (native AppImage run) |
| `presets/generatesAPresetWithExpectedKeys.spec.js` | Rust preset round-trip tests (existing) | C |
| `scenes/defaultScenes.spec.js` | `default_scenes` Rust tests + native 61-Scene seed | C |
| `scenes/captureLiveState.spec.js` | engine `scenes/captureLiveStateToScene` tests + native Scene menu action | C |
| `scenes/sceneSnapshotsAndPerformance.spec.js` | `scene_ops`/performance tests + native Scenes panel | P (kittest automation pending) |
| `scenes/performanceGrid.spec.js` | `scene_rows`/`scene_columns` tests + native grid | P |
| `scenes/performanceTiming.spec.js` | `scene_timing` tests + native perf bar | C |
| `scenes/performanceCellTiming.spec.js` | `scene_timing` tests | C |
| `scenes/performanceTransitions.spec.js` | `performance`/`scene_transition` tests + native transition capture | C |
| `scenes/gallerylessPerformance.spec.js` | engine performance tests | C |
| `scenes/hoverPreviewIsolation.spec.js` | `scene_preview` isolated-compositor tests | C |
| `scenes/hoverPreviewUsesGalleryContexts.spec.js` | engine preview tests | C |
| `point10/*.spec.js` (8 specs) | existing lavapipe goldens + `modv-renderers` GPU tests; native compositor uses the same engine | C |
| `audio/systemAudio.spec.js` | engine `AudioFeed` tests; native opens PipeWire monitor with input fallback (verified on host); host-PipeWire manual check | P |
| `audio/tempoDetector.spec.js` | `modv-media` tempo tests | C |
| `engine/engineSkeleton.spec.js` | Boundary deleted; native shell tests replace it: single-process smoke, preview texture, output window, `--frames` traces. `tools/native-smoke.sh` covers the packaged lifecycle | P (preview/output kittest pending) |
| `engine/engineParity.spec.js` | Boundary deleted; registry/count tests, the 432-module guard and the native Gallery read model | C |
| `main/windowState.spec.js` (packaged app) | window-state unit tests + manual packaged checklist | P |

## Coverage buckets

- **Rust engine tests** are the reference for every state/Scene/performance/
  audio/compositor/preset behaviour; they are already the source of truth from
  the hybrid.
- **Native unit tests** cover `modv-ui` widgets/action payloads,
  `window_state`, native snapshot flattening and startup Scene application.
- **Kittest** covers the shell layout, the RangeControl, the Scenes grid and
  the golden shell image; every new panel gets a render/interaction smoke.
- **Manual/hardware** is still needed for PipeWire system audio, cameras, NDI
  and the packaged AppImage/.deb on a clean VM. The NDI/OSC panel tests are
  headless action/format coverage, not hardware verification.
- **Waived** currently only the GL-context spec (no GL in the native app);
  packaging is covered by the native AppImage/`.deb` checks rather than waived.

## Native 2.0 verification evidence (2026-11-18)

`tools/native-smoke.sh` was added as the packaged-app hardening gate. It never
builds and always uses a throwaway `MODV_USER_DATA` (with a per-command
timeout and cleanup). It prefers a built `dist/VmodV-Native-*.AppImage`, then
`--appimage`. When neither exists it assembles the same AppDir layout around
`crates/target/release/modv` from `packaging/native/{AppRun,log-run.sh}`; that
fallback was validated before the release build landed. The evidence below is
from the actual `dist/VmodV-Native-2.0.0-x86_64.AppImage` (launched with
`--appimage-extract-and-run`).

| Check | Result |
| --- | --- |
| `--headless --frames 600 --bench` | exit 0, valid JSON, 600 frames |
| frame time / RSS | 5956.59 fps, mean 0.20 ms, p50 0.16 ms, p90 0.17 ms, p99 0.38 ms, 99.4 MB RSS |
| windowed `--frames 20 --apply-scene "Mycelium Bloom"` | exit 0, Scene applied (3 Groups) |
| state round-trip | `engine-state.json` written; second run restored 3 Groups / 2 Modules |
| SIGKILL recovery | killed 4 s in (before the 30 s periodic save); restart restored the same snapshot, exit 0 |
| packaged log wrapper | `<userData>/logs/vmodv-YYYY-MM-DD.log` written with `[native-launch]` header; stdout JSON untouched |
| bounded soak | 60 s windowed: 3498 frames, 58.30 fps, peak process-group RSS 335 MB |
| display backends | forced Wayland-only and X11-only `--frames 5` runs both exit 0 on RADV RX 9070 XT |
| OSC | `modv-media` real-UDP loopback test + `store_ops::osc_datagrams_apply_to_state_and_mirror` pass; a live native run bound `0.0.0.0:3333` |
| module parity | committed manifest guard: 432 = 15 2D + 27 Point10 + 36 shader + 354 ISF; 61 default Scenes reference 62 distinct names, all resolve on both sides |

The app snapshot deliberately excludes `osc` runtime data (it is not a preset
key), so OSC is covered at the socket/store boundary rather than by asserting a
value in `engine-state.json`.

### Hardware / environment waivers

- **NDI hardware — waived.** This host has no `libndi.so.5`; native startup
  logs `NDI unavailable` and continues cleanly. Sender/receiver behaviour
  against real NDI hardware is not verifiable here.
- **Multi-GPU matrix — waived.** Only one adapter was available (AMD RADV
  GFX1201). NVIDIA/Intel/llvmpipe and multi-adapter layouts are untested.
- **Clean-VM installers — waived here.** The packaged native AppImage was
  launched with `--appimage-extract-and-run`, but installing the AppImage and
  `.deb` on a clean VM (and desktop-menu/launcher integration) remains a
  release checklist item.
- **Host audio/camera content — waived.** A native run opened the PipeWire
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
  persisted to `native-layout.json` (`f359e459`, `d2951184`);
- NDI and OSC panels (`dbc2aa76`);
- live capture-rate reporting (`e8f02b08`, `4e57f81b`);
- Gallery drag-to-Group and marquee names (`3bd065d2`, `73feb02a`);
- native AppImage, `.deb`, `native-deb-check` and `native.yml` (`da724d45`,
  `a331368c`);
- the exact 432-module registry guard (`10ac008d`, `dde600a4`);
- user-ISF watch/hot reload and preset load/save (`19a71602`, `0d83d639`).

Still open:

1. Turn the remaining `P` rows into `C` with kittest interactions or native
   integration tests: Groups keyboard delete, tri-state enabled and explicit
   `inheritFrom`, Gallery hover-preview kittest, Scene grid automation,
   packaged window-state/quit checks and host PipeWire audio.
2. Add a native e2e/perf/soak harness. `tools/native-smoke.sh` now covers the
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
7. P9: Flatpak, the 2.0.0 version bump, switching the nightly release to Rust,
   deleting Electron/Node after sign-off, and docs/migration guides.
8. The clean-VM manual checklist for audio/cameras/NDI. No NDI/GPU hardware
   verification is recorded by this refresh; see the hardware waivers above
   for exactly what the native smoke gate does and does not cover.
