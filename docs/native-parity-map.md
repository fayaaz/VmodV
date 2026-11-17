# Native parity map (P8 draft)

Maps the legacy Playwright suite under `tests/e2e/spec/` to the native
implementation's coverage: Rust engine tests, native `modv-ui`/`modv-app`
tests, kittest snapshots, headless CLI checks, manual hardware checks, or an
explicit waiver. The plan expects every scenario to land in exactly one
bucket; this is the first pass at HEAD `71f707ad` and is regenerated at P8
start.

Legend: **C** covered, **P** partially covered (native panel or manual step
still missing), **W** waived with a reason, **N** not started.

| Legacy spec | Native replacement / anchor | Status |
| --- | --- | --- |
| `general/rendersTheMainWindow.spec.js` | `modv-ui/tests/shell_snapshot.rs` (golden shell); `crates/target/release/modv --frames N` smoke | C |
| `general/renderersAreRegistered.spec.js` | Rust engine registry tests (`modv-app`/`modv-renderers`) + native Gallery snapshot | C |
| `general/defaultGroupIsFocusedByDefault.spec.js` | native Groups panel selects the first Group; kittest panel smoke pending | P |
| `general/moduleDataCommitsOnlyWhenChanged.spec.js` | engine delta tests + native action queue | C |
| `general/fullScreenGlContextsAreNotAntialiased.spec.js` | Waived: the native app has no GL contexts | W |
| `general/galleryItemPreviewRendersModule.spec.js` | `native.rs::answer_preview_requests` + Rust preview tests | P (kittest hover pending) |
| `general/galleryItemPreviewsInheritTheComposition.spec.js` | `render_module_preview(with_output)` tests + native `with_output` requests | C |
| `general/galleryGroupInheritsOnlyWhenHovered.spec.js` | engine compositor tests | C |
| `general/isfGalleryCategories.spec.js` | `ModuleMetaView::gallery_category` unit test + registry tests | C |
| `general/searchGalleryAndAddModuleToGroup.spec.js` | native Gallery search + command palette; kittest interaction pending | P |
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
| `main/quitCleansUp.spec.js` | native `NativeApp::shutdown` + engine snapshot/shutdown tests; `--frames` smoke | P (manual packaged check) |
| `main/windowState.spec.js` | `modv-app/src/window_state.rs` unit tests (normalise/atomic/clamp) | C |
| `packaging/linuxBundleIsTrimmed.spec.js` | P9 packaging gate (not started) | N |
| `presets/generatesAPresetWithExpectedKeys.spec.js` | Rust preset round-trip tests (existing) | C |
| `scenes/defaultScenes.spec.js` | `default_scenes` Rust tests + native 62-Scene seed | C |
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
| `engine/engineSkeleton.spec.js` | Boundary deleted; native shell tests replace it: single-process smoke, preview texture, output window, `--frames` traces | P (native e2e harness pending) |
| `engine/engineParity.spec.js` | Boundary deleted; registry/count tests + native Gallery read model; exact-count guard is P8 work | P |
| `main/windowState.spec.js` (packaged app) | window-state unit tests + manual packaged checklist | P |

## Coverage buckets

- **Rust engine tests** are the reference for every state/Scene/performance/
  audio/compositor/preset behaviour; they are already the source of truth from
  the hybrid.
- **Native unit tests** cover `modv-ui` widgets/action payloads,
  `window_state`, native snapshot flattening and startup Scene application.
- **Kittest** covers the shell layout, the RangeControl, the Scenes grid and
  the golden shell image; every new panel gets a render/interaction smoke.
- **Manual/hardware** covers PipeWire system audio, cameras, NDI and the
  packaged AppImage/.deb on a clean VM.
- **Waived** currently only the GL-context spec (no GL in the native app);
  packaging is scheduled for P9 rather than waived.

## Open P8 work

1. Turn the `P` rows into `C` with kittest interactions or native integration
   tests (Groups keyboard delete, Gallery hover/drag, Scene grid automation,
   window-state packaged check).
2. Add the exact registered-module count guard (module-parity item 6) and the
   third-party ISF registration path (P6).
3. Record the clean-VM manual checklist for audio/cameras/NDI.
