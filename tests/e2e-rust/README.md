# Electron e2e → Rust parity inventory

`parity.json` maps every Electron Playwright test to a Rust test that asserts
the same intent, or to an explicit waiver. The port is complete: there are no
open `port-store`, `port-ui` or `port-process` buckets left. It is not a list
of tests to delete — each entry names the Rust location that keeps the
behaviour covered once the Electron suite goes away.

## Counts

The tree has **50 `*.spec.js` files and 147 executed test cases**:

- 49 files live under `tests/e2e/spec/` (Playwright's configured `testDir`).
- `tests/e2e/main.spec.js` is a root-level placeholder whose ten `test(...)`
  calls are all commented out; it contributes a file entry with zero tests.
- 133 `test(...)` declarations: 129 literal strings plus four parameterised
  templates. The templates expand to 18 cases (tempo 120/128/140/174/178,
  Chromatic Line/Wave, nine imported Point10 scenes, Rolling Fire/Smoke Room),
  giving 147 executed cases in total.

Final accounting: **137 covered + 10 waived = 147 tests** across 50 files.
The JS guard pins the live tree and these totals; the Rust guard pins the
manifest after the Electron suite is deleted.

## Statuses

- **covered** — a Rust test asserts the same behaviour. `rust` names one or
  more `crates/...:test_name` (or `tools/...` check) entries.
- **waived** — not meaningful in the native shell for a concrete reason. The
  `notes` field names the Electron/worker dependency and, where applicable, the
  Rust test that covers the surviving intent.

`notes` also records native semantic differences where the intent is covered:
row applies mint fresh Group ids, the kill-survival test uses the native
single-process shell instead of a supervised engine child, and the Scene-open
path now applies by default.

## Coverage by area

| Area | Files | Tests | Covered | Waived | Rust evidence |
| --- | ---: | ---: | ---: | ---: | --- |
| audio | 2 | 16 | 16 | 0 | `crates/modv-media/src/audio/tempo.rs`, `crates/modv-ui/src/model.rs`, `crates/modv-ui/tests/audio_panel.rs` |
| engine | 3 | 3 | 3 | 0 | `crates/modv-app/tests/e2e_process.rs`, `src/store_ops.rs`, `src/module_registry.rs`, `src/compositor.rs` |
| general | 10 | 17 | 13 | 4 | `src/module_registry.rs`, `src/compositor.rs`, `src/point10/scene_tests.rs`, `crates/modv-ui/tests/e2e_groups.rs`, `crates/modv-ui/tests/e2e_scenes_grid.rs` |
| groups | 11 | 11 | 11 | 0 | `crates/modv-ui/tests/e2e_groups.rs` |
| inputLinks | 1 | 1 | 1 | 0 | `crates/modv-ui/tests/e2e_input_config.rs`, `src/store_ops.rs` |
| main | 2 | 2 | 1 | 1 | `crates/modv-app/tests/e2e_process.rs`, `src/window_state.rs` |
| packaging | 1 | 1 | 0 | 1 | `tools/native-deb-check.sh`, `tools/native-smoke.sh` |
| point10 | 8 | 18 | 18 | 0 | `crates/modv-renderers/src/point10/scene_tests.rs`, `.../scenes/*.rs` |
| presets | 1 | 1 | 1 | 0 | `crates/modv-engine/src/preset.rs`, `src/fixtures.rs` |
| scenes | 10 | 77 | 73 | 4 | `crates/modv-app/tests/e2e_scenes.rs`, `e2e_defaults.rs`, `e2e_process.rs`, `crates/modv-ui/tests/e2e_scenes_grid.rs`, `src/scene_ops.rs`, `src/performance.rs` |
| main (legacy placeholder) | 1 | 0 | 0 | 0 | — |
| **Total** | **50** | **147** | **137** | **10** | |

The process-level tests launch the real `modv` binary through the native
harness (`crates/modv-app/tests/e2e/mod.rs`), so window state, startup
seeding and Scene-open behaviour are exercised end to end rather than only
through the store.

## Waivers (10)

| Spec | Reason |
| --- | --- |
| `general/fullScreenGlContextsAreNotAntialiased` | Inspects Chromium WebGL context attributes (`antialias:false`) on worker `OffscreenCanvas` aux outputs. Native ISF/shader passes are wgpu pipelines with no multisample state to configure. |
| `general/galleryGroupInheritsOnlyWhenHovered` (×2) | The legacy worker Gallery Group canvas does not exist natively. Hover previews are isolated offscreen renders; `compositor.rs:module_preview_composites_over_the_live_output` covers the compositing intent, and there is no idle Gallery canvas to repaint. |
| `general/moduleDataCommitsOnlyWhenChanged` | Vuex commit de-duplication in the worker loop. Native module data is updated in place with no per-frame commit/mirror message to suppress. |
| `main/quitCleansUp` | Asserts Chromium's main/renderer/GPU/utility child tree is gone after `app.quit()`. The native shell is one process with no such tree; `tools/native-smoke.sh` already checks clean windowed exits. |
| `packaging/linuxBundleIsTrimmed` | Asserts electron-builder `app.asar.unpacked` contents, `hapticjs`/`grandiose` addons and Chromium locale `.pak` files. Native packaging has separate `native-deb`/`native-appimage` checks and contains none of those payloads. |
| `scenes/hoverPreviewIsolation` | Regression for deleting the shared WebGL unit-quad vertex buffer when a preview ISF renderer is disposed. Native previews use separate wgpu module instances; `scene_preview.rs:cache_keeps_the_other_cells_instances_while_a_hover_moves` covers isolation. |
| `scenes/hoverPreviewUsesGalleryContexts` | Asserts the worker's shared live ISF `OffscreenCanvas` keeps its output size during a hover. Native previews never resize the live surface. |
| `scenes/performanceGrid` — editing a grid cell saves added modules | Native has no in-cell module-editor dialog. The same end state is reachable by creating the cell Group (`scenes/createSceneCellGroup`), editing the live Group via the Gallery/Groups pane, then capturing it back (`scenes/captureSceneRowToScene`) or clearing it (`scenes/clearSceneCell`, now confirmed). The grid widgets are asserted by `e2e_scenes_grid.rs`; the store half by `scene_ops.rs:scene_cell_actions_create_capture_and_clear_rows`. |
| `scenes/performanceGrid` — drag a module from its panel into the Group | There is no editor module panel to drag from. Native has the equivalent drag: a Gallery tile dropped on a Group block queues `ui/addModuleToGroup` (`gallery_drag_drop.rs:dragging_a_gallery_tile_onto_a_group_queues_the_add_action`), and a Library asset can be dragged onto its texture prop. |

## Closed port gaps

The port closed the gaps the previous inventory listed as `port-*`:

- **Scene loading** — `scenes/loadSceneFromJson` now defaults to applying, and
  opening a Scene JSON through the media dialog applies it:
  `a_scene_json_can_be_loaded_directly` and
  `a_scene_json_opened_like_a_preset_is_applied_as_a_scene`.
- **Fresh-start default Group** — a fresh native userData seeds exactly one
  `New Group`, matching Electron (there is no Gallery Group canvas):
  `fresh_native_start_creates_exactly_one_default_group` and
  `native_shell_serves_state_seeds_defaults_and_quits_cleanly`.
- **Kill/restart and window state** — the native shell restores its saved
  snapshot after SIGKILL and remembers window placement across restarts:
  `engine_skeleton_survives_sigkill_and_restores_state` and
  `window_state_survives_a_restart`.
- **Scene restore** — saved Groups/Modules/Input Links/Tweens, full-store
  serialisation, legacy tweens/autopilot/timing, gallery-less performance and
  named-row/scoped applies are asserted in `e2e_scenes.rs`.
- **UI interactions** — Groups-pane controls, the Scenes grid, and the Input
  Config focus workflow are driven with egui kittest in `e2e_groups.rs`,
  `e2e_scenes_grid.rs` and `e2e_input_config.rs`.
- **Audio, ISF, Point10 and defaults** — the tempo robustness cases, named ISF
  categories/draw/animation, Point10 transparency, and the default-Scene
  audits now have named Rust tests.

## Guards

`tests/unit/e2e-parity-manifest.test.js` parses the specs with the same
parameterised-loop expansion as the old inventory, then fails when:

- a spec file is missing from, or extra in, the manifest;
- a test is missing, duplicated, or stale;
- a status is not `covered`/`waived` (including any returned `port-*`);
- a `covered` entry has no Rust reference, or a `waived` entry has no reason;
- a named Rust test/path no longer exists;
- the totals drift from 137 covered + 10 waived = 147.

`crates/modv-app/tests/e2e_manifest.rs` embeds the same JSON with
`include_str!` and checks the final shape without the JS suite: 147 entries,
no `port-*` statuses, covered entries name existing Rust test functions,
waivers carry a reason, and no file is listed twice. It runs with:

```sh
make engine-test ARGS="-p modv-app --test e2e_manifest"
```

The JS guard runs with:

```sh
make test-unit ARGS="tests/unit/e2e-parity-manifest.test.js"
```

`parity.json` is deterministic: spec files are sorted, tests keep source order,
and object keys are emitted in a fixed order.
