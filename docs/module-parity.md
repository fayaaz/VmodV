# Module parity — AppImage (legacy) vs the Rust engine

Status: **updated audit**, 2026-11-17 (engine on `rust-hybrid`, HEAD
`dd8914c2`). Supersedes the 2026-11-12 audit (HEAD `a13dacd8`), whose headcount
(engine 429 / legacy 432) is preserved in [Audit history](#audit-history) —
nearly everything it found has since landed. Companion to
[rust-hybrid-progress.md](./rust-hybrid-progress.md) (M7 gaps) and
[rust-hybrid-review.md](./rust-hybrid-review.md).

Scope: the Gallery **module set** — what the two runtimes in the AppImage
register and how. With the default `MODV_ENGINE=0` the AppImage runs the
legacy worker renderer and everything below is a non-issue; the differences
appear only with `MODV_ENGINE=1` (the M7 default-flip target).

Verification for this update: the registry sources in `crates/`, the JS
`import.meta.glob` registration in `index.worker.js`, `state/sync` semantics,
and a programmatic pass over the seeded default Scenes (63 distinct module
names — all resolve). Commit references are on `rust-hybrid` unless noted.

## Headcount (2026-11-17)

| Bucket | Legacy worker | Rust engine (`state/sync`) |
|--------|--------------:|---------------------------:|
| 2D (plain) | 15 | 15 |
| Point10 scenes | 27 | 27 |
| `three` renderer | 0 — removed 2026-11-16 | 0 — never existed |
| Shader modules (incl. 30 `Point10Effects`) | 36 | 36 |
| ISF corpus | 354 | 354 (same `.fs` files, scanned at startup) |
| **Gallery total** | **432** | **432** |

Extraction sources — legacy: the `import.meta.glob` registrations in
`src/renderer/src/application/worker/index.worker.js`
(`renderers/2d/modules/*.js` → 15 + 27,
`renderers/shader/modules/*.js` → 6 + 30,
`isf/modules/isf/*.fs` → 354). Engine: `modv-renderers` registries
(`two_d::all_modules` → 15, `point10::all_scenes` → 27,
`shader::modules::all_modules` → 36) plus the AppImage's `resources/isf`
(the `electron-builder.yml` `isf` `extraResources` copies the same corpus;
the count comes from `isf-corpus-report.md` and the `354/354` translate
baseline the corpus gate asserts).

ISF corpus growth vs the first audit (351 → 354 `.fs`, 36 `.vs`) came from
the built-in NC-SA 3D Shadertoy scenes (`Helix Eye`, `Traced Tunnel`,
`Singularity Rain`, `Hot Shower`, …) and the v16 default-Scene work.

Shader count vs the first audit: the JS `Point10Effects.js` and the engine
both dropped **`Point10 Old Movie`** (the engine test suite asserts it is
*not* registered — "the Old Movie effect is retired"); 6 + 30 = **36** on
both sides, matching `MODULE_COUNT` in `shader/modules/mod.rs`.

The built-in module sets are now **identical** — the delta is zero. Nothing
was registered in the engine that is absent from legacy at any point; the
remaining items are inert modules, third-party loading and hygiene (below).

## Differences

### 1. Missing modules — **none** (closed)

The first audit's three missing modules are all resolved:

| Module | Resolution |
|--------|------------|
| `Point10 Earth` | **Landed 2026-11-16** (`cbb24d8a`) — WGSL port on the UltraHDR pattern (`crates/modv-renderers/src/point10/scenes/earth.rs`, 2078 LOC), registered (`Point10SceneKind::Earth`), 3d-scenes category, prop schema matching `sceneProps.js`; Earth's textures were already bundled and load through the shared asset loader |
| `Point10 Sun` | **Landed 2026-11-16** (`cbb24d8a`) — same commit (`sun.rs`), procedural, no textures |
| `Cube` | **Removed 2026-11-16** (`f7d35db0`) — `three` renderer and its sole module removed from *both* runtimes. Presets containing a `three` module load with the module skipped and logged; nothing shipped references it |

Further retirement: **`Point10 Geometric Tunnel`** was removed from both
runtimes (`34528ff6`, 2026-11-16) — JS wrapper/scene, the Rust port and the
SSAO pass — with the five affected default Scenes retuned in place (names
kept for the refresh list; `DEFAULTS_VERSION` **16** in JS and Rust; the
module joined `REMOVED_DEFAULT_SCENE_MODULES` so older installs rebuild the
stale Scenes too). Point10 wrappers in the legacy Gallery went 28 → 27.

### 2. Registered but inert in engine mode — **closed** (except third-party)

| Module(s) | Legacy | Engine (2026-11-17) |
|-----------|--------|----------------------|
| `Texture 2D` | draws the media-library image assigned to its `texture` prop | **Fixed** (`e4cf3e26`) — the prop resolves through `texture_sources::TextureRequest` (LRU image pool, media-library paths under the compositor media root) and draws via `DrawContext::input_texture` |
| ISF modules with image inputs (288+ shaders, 392 image declarations as of the first audit — corpus has grown since) | an assigned texture prop is resolved and bound | **Fixed** (`e4cf3e26`) — ISF image inputs resolve assigned texture props to GPU textures: image assets (LRU by path+size, `image`-crate decode), live surfaces by id (`isf:`/`two-d:`/`point10:` module outputs, `group:` canvases, `camera:`/`video:`/`screen:` sources); unresolved/inherit/null keep the group canvas (the JS *unassigned* default); a self-referencing module falls back to the canvas (wgpu forbids aliasing the target). Filters are alpha-masked only when at least one input still binds the group canvas, so assigned-photo filters are no longer erased over transparent rows |
| 27 transition-capable ISF modules (`Point10 Crossfade`, `Point10 Disolve`, …) | run as Scene transitions | **Fixed** (`f591a108`) — `scene_transition.rs` binds the captured from/to frames as `startImage`/`endImage` for a registered transition-capable ISF module; `Scenes.vue` lists them in the picker automatically (its `transitionOptions` filter collects registered modules carrying `startImage`/`endImage`/`progress` props), and unknown names still filter to Fade. Overall behaviour mirrors legacy |
| Third-party modules (user ISF `.fs` via `media/isf/`, user Canvas2D JS via `media/module/`) | webpack-compiled, `eval`'d, hot-registered via `modules/registerModule` | **Open.** No `modules/registerModule` arm in the engine; the `media/addMedia` path throws in engine mode; the built-in registry is fixed at startup. User Canvas2D JS modules are **dropped by decision** (see [rust-full-rewrite.md](./rust-full-rewrite.md) D7). User ISF `.fs`: the translate/register pipeline exists; only the user-directory watch/reload + an engine registration arm is missing — still open |

### 3. Small prop/meta mismatches (all verified 2026-11-17 — all still open)

- **Grid Stretch** — JS meta declares the dead key `meyda: ["zcr","rms"]`
  (the worker only honours `audioFeatures`), so legacy requests **no** audio
  features and reads `features.zcr` unconditionally (NaN unless another
  module requested it). The engine registers `audio_features:
  ["zcr","rms"]` and has the same `zcr` bool. Engine is strictly more
  correct; a one-line JS hygiene fix (drop the dead key) remains.
- **Line `color`** — JS `type: "paletteControl"` (an unregistered dataType,
  inert; draw uses `strokeStyle = "#fff" || color`) vs engine
  `color_255("color", [255, 255, 255, 1])`, an off-scale default whose value
  the port doesn't read (stroke is always `#fff`). Cosmetic; reconcile types.
- **Buffer `bufferId`** — JS default is a *function* (`() => Date.now()`
  string, evaluated per registration); engine ships `""` (documented as the
  JS-default-can't-be-a-function note; cold starts differ in the aux output
  name only).
- **`Point10` wrapper `webgpu: true`** (AttractorParticles, BallOfFire,
  DvdRoom, RollingFire, SmokeRoom) — not modelled in the engine, harmless:
  wgpu erased the WebGPU/WebGL split the JS flag fed (predictive prewarm).
- **2D `previewWithOutput` / `strict` / `abs`** — `TwoDModuleMeta` carries no
  `previewWithOutput` and `TwoDProp` carries no `strict`/`abs` ("prop-metadata gaps", documented in `module_registry.rs`). The handful of JS declarations that set them live in the exhaustive companion tables in `crates/modv-app/src/module_registry.rs` (`TWO_D_PROP_FLAGS` — 11 entries, Ball/Line/Concentrics/Polygon/Text/Waveform/X Drips — and `TWO_D_PREVIEW_WITH_OUTPUT` — `Grid Stretch`, `Pixelate`), carried into the shared prop-coercion schema. Promoting them onto `TwoDProp`/`TwoDModuleMeta` remains open (see hygiene, item 7); behaviour matches the JS today.
- **Known name mismatch** — `module_registry.rs` still carries the stale
  "Known name mismatch" note (it claims the JS Gallery calls two modules
  `X-Drips`/`Texture2d`; the JS registers `X Drips`/`Texture 2D`/
  `Grid Stretch`, identical to Rust). Delete the note.
- **Module counts in docs** — normalised here to **432/432**. Both the old
  audit (429/432) and `rust-hybrid-progress.md`'s snapshot (429) and the
  `engineParity` `>300` assertion are inconsistent or non-committal; items 6
  and 7 below own the fix.
- **`DEFAULTS_VERSION` 16** both sides after the v16 retune; the seeded
  Scenes reference **63 distinct module names** — all resolve in the engine
  registry (checked programmatically: ISF stems case-insensitively, plus
  `Concentrics`/`Line`/`Polygon`/shader modules/`Point10Effects`/Point10
  scenes).

### 4. Dead code (open)

- `src/renderer/src/application/renderers/isf/modules/GreatBallOfFire.js` —
  still present, still never registered (the JS ISF glob only covers
  `isf/modules/isf/*.fs`); the corpus `GreatBallOfFire.fs` *is* registered in
  both runtimes. Delete the JS duplicate.

### Verified clean (2026-11-17)

- All **36 shader modules**: meta/props/defaults/ranges match exactly
  (engine `shader/modules/*.rs` vs JS), incl. the six single-effect modules.
- The **30 `Point10Effects`**: match, incl. the four audio effects
  (`Point10 Contrast`, `Point10 Color Distort`, `Point10 RGB Shift`,
  `Point10 Digital Glitch`) carrying `audioFeatures:
  ["bass","mid","treble"]`; `Point10 Old Movie` is retired on both sides.
- All **15 2D modules**: meta, props and defaults match (`Counter`/`Text`
  omitting author/version matching the JS spread).
- All **27 Point10 scenes**: prop schemas match the JS `sceneProps.js`
  tables exactly (labels, defaults, min/max/step, SMOKE_FIRE overrides,
  CHROMATIC_LINE_PROPS), as do responsiveness levers, Gallery categories
  (2d-scenes for Chromatic Line/Wave/Mycelium/WaterRipple, 3d-scenes
  elsewhere), `type: "2d"` and `audioFeatures`.
- ISF: **354/354 register** in both runtimes; **354/354 fragments and 36/36
  vertex sidecars translate with zero overrides** (see
  [isf-corpus-report.md](./isf-corpus-report.md)).
- Gallery ordering is irrelevant (tiles sort by `meta.name`).
- The six native Scene transitions and the performance-mode crossfade rule
  (`scene_transition_pick.rs`: every performance-mode cell change is a Fade)
  are ported and tested; transition selection filters the allowed list
  against availability.
- Seeded default Scenes: **63/63 module names resolve** (see above).

## Recommendations (status 2026-11-17)

1. ~~Port `Point10 Earth` and `Sun`~~ — **done** (2026-11-16, `cbb24d8a`).
2. ~~Decide `Cube`~~ — **done** (2026-11-16, `f7d35db0`): removed from both
   runtimes; nothing references it; a load of a `three`-module preset logs
   and skips.
3. ~~Texture registry~~ — **done** (2026-11-13, `e4cf3e26`): image pool LRU,
   media-library path resolution, live
   `isf:`/`two-d:`/`point10:`/`group:`/`camera:`/`video:`/`screen:` sources,
   assigned ISF image inputs and `Texture 2D` bind and render.
4. ~~ISF transitions for the 27 corpus shaders~~ — **done** (2026-11-13,
   `f591a108`): captured from/to frames bind as `startImage`/`endImage`;
   unknown names filter to Fade; the picker lists only registered
   transition-capable modules.
5. **Third-party loading — open.** Implement the user ISF directory scan
   (startup + watch/reload via the media manager) translating and registering
   dropped `.fs` — the translate/register pipeline exists; only the watch +
   index plumbing and an engine `modules/registerModule` arm are missing.
   User Canvas2D JS modules: dropped by decision (rust-full-rewrite.md D7),
   remove the dead `eval(text).default` path.
6. **Regression guard — open.** A test/script asserting the JS
   glob-extracted registry equals the Rust registry, plus an exact e2e
   assertion (`engineParity.spec.js` asserts `>300` today; assert 432 or
   read the count from the fixture).
7. **Hygiene — partial.** Remaining: delete `GreatBallOfFire.js`; delete the
   stale "Known name mismatch" note in `module_registry.rs`; promote
   `TWO_D_PROP_FLAGS` / `TWO_D_PREVIEW_WITH_OUTPUT` (the exhaustive
   companion tables in `module_registry.rs`, kept beside the JS corpus
   instead of on `TwoDProp`/`TwoDModuleMeta`) onto those types; reconcile
   the Line `color` type and Buffer `bufferId` default; drop the dead
   `meyda:` key from Grid Stretch; normalise the 424/428/429/431/432 counts
   in docs (`rust-hybrid-progress.md`, `rust-hybrid-review.md`).
8. **Earth/Sun textures through the registry — open (optional).** Both load
   via the shared asset loader with the bundled textures; routing them
   through `texture_sources` would give them the same media-root/path
   semantics for free. Low priority.

Net: the built-in Gallery is at **432/432** parity. The remaining delta —
third-party ISF registration (5) and the regression guard (6) — is
tracked, small, and does not block the M7 default-flip.

## Audit history

| Date | HEAD | Finding |
|------|------|---------|
| 2026-11-12 | `a13dacd8` | engine 429 vs legacy 432: `Point10 Earth`, `Point10 Sun`, `Cube` missing; `Texture 2D` and ISF image inputs inert; 27 ISF transitions Fade; counts 424/428/429 inconsistent |
| 2026-11-13 | — | `e4cf3e26` texture registry; `f591a108` ISF transitions — items 3 and 4 closed |
| 2026-11-16 | — | `cbb24d8a` Earth/Sun ported; `f7d35db0` `Cube`/`three` removed; `34528ff6` Geometric Tunnel retired both sides, `DEFAULTS_VERSION` 16 — items 1, 2 closed, headcount 432/432 |
| 2026-11-17 | `dd8914c2` | This update: verified current state (63/63 default-Scene module names resolve; two inert classes fixed; stale items listed) |
