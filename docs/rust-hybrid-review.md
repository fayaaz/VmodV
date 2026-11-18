# Rust hybrid engine — code review

Review of the committed `rust-hybrid` branch against `origin/main`. Written for
the engineer(s) building the engine; intended to be read alongside
`docs/rust-hybrid.md` and `docs/rust-hybrid-progress.md`.

## Status after the fix wave

All findings below were re-checked against the fixed tree. The fixes landed in
separate commits by area (engine/ipc, media, render/2D/Point10, ISF,
app/src/main/renderer).

| Finding | Status | Notes |
|---|---|---|
| R-1 ISF trailing implicit pass | **Fixed** | last pass' target only; magenta regression test |
| R-2 / R-3 PipeWire negotiation | **Fixed** | F32/S16 negotiation, readiness after accept, negotiated rate, latched errors |
| R-4 cold-start sync | **Fixed** | `engine/get-sync` + idempotent replay (tweens/expressions included) |
| R-5 / R-6 BPM/FPS tweens | **Fixed** | bpm>0 enforced, per-entry serialisation, 120-BPM fallback |
| R-7 non-loop seek | **Fixed** | restored frame for both branches |
| R-8 / R-9 tween reload/playheads | **Fixed** | prune instead of reload; per-entry decode |
| R-10 scene-preview thrash | **Fixed** | retained instances per cached cell; scoped eviction |
| R-11 sync drops tweens/expressions | **Fixed** | pure replay helper |
| R-12 preview IPC | **Fixed** | magic/sequence/bounds/bytesRead validation + retry |
| R-13 stop/remove transitions | **Fixed** | both complete transitions; capture dropped |
| R-14 migration duplicate removal | **Fixed** | one seeded Scene per rule |
| R-15 scene-capture rows | **Fixed** | bounded BTreeMap, null holes preserved |
| R-16 frame-graph usage | **Fixed** | COPY_DST validated |
| R-17 2D self-sampling/clip/clear | **Fixed** | snapshot, intersect, ordered erase |
| R-18 OSC bounds | **Fixed** | array/argument/bundle/store caps |
| R-19 tempo options | **Fixed** | finite/bounded |
| R-20 socket unlink | **Fixed** | stale-only + inode-checked drop, 0600 |
| R-21 preview ring | **Fixed** | permissions, invalid unwritten slots, publish ordering |
| R-22 RT allocations | **Fixed** | per-stream scratch buffers |
| R-23 unsupported engine features | **Partial** | Output Window/MIDI/OSC disabled with hints + boot warning; plugins/fonts only warned; BPM MIDI source still selectable |
| R-24 supervisor spawn/restart | **Fixed** | error routing, await-exit, budget reset |
| R-25 preload scope / channel list | **Fixed** | main window only + sender validation |
| R-26 ISF robustness edges | **Fixed** | size clamp, keywords, texture2DRect, fingerprints, validation, FRAMEINDEX, `$` scalars, bool truthiness, matrix locations |
| R-27 corpus/overrides | **Partial** | overrides embedded + fingerprinted + baseline gate; the 7 corpus shaders still have no override files, so they stay unrendered |
| R-28 Point10 drift/mips | **Fixed** | band/beat/clock/mips/cube clip-z; ItIsComing keeps preserveBloomAlpha (documented deliberate transparency) and AroundPlus keeps its GL-shaped matrix (used for its screen-space water maths) |
| R-29 restore/performance | **Fixed** | runtime cleared + rows republished; incoming above ghost; row-name clones removed. Whole-scene Fade now interleaves per row rather than JS's unnamed-row block (documented; inheritance remapped) |
| R-30 ISF bool coercion | **Fixed** | `IsfValue::coerce_bool` used by the compositor |

P3 hygiene items (engine_state slicing, preset null holes, hostile
`sceneRow` decode, hex timings, `stroke_text` mask re-uploads, final delta
flush, windowless supervisor parking) remain open and are tracked here.

## Scope and method

- **Reviewed:** everything committed on `rust-hybrid` vs `origin/main`, i.e.
  the merge base `76816a0f` through the tip at the time of each review pass
  (latest pass: **`22646926`** — see the [Round log](#round-log)). This is
  ~107k added lines across 205 files: six Rust crates (`crates/`, ~90k lines),
  the Electron/main-process and renderer integration, tests, build files and
  docs.
- **Method:** static review only. Nothing was compiled, run, linted or built;
  every statement below comes from reading the code and cross-checking it
  against the legacy JS being ported (worker store, renderers, `point10/`,
  `isf/`) and against `docs/rust-hybrid.md`. GPU behaviour was verified by
  reading only. Line numbers refer to the file **at the reviewed commit** and
  drift as the branch advances; paths are repo-relative.
- **Verification marks:** `✔` = re-verified by the reviewer at the reviewed
  commit; `◐` = reported by a reviewer pass, checked by reading the specific
  site but not exhaustively cross-checked.
- **Severity:** P1 = likely real-world defect (wrong output, crash, data
  loss); P2 = robustness / edge-case / parity break worth fixing; P3 = minor,
  hygiene, or latent.
- The review covers **committed code only**. Uncommitted work in the main
  checkout (e.g. the `--audio-source` wiring described in the freshly edited
  `AGENTS.md`, and `crates/modv-app/src/audio.rs`) is out of scope and will be
  reviewed once it lands.
- The `/tmp/opencode/wt` review worktree was deleted externally mid-review;
  the remainder was reviewed from a `git archive` extraction of the committed
  tip in `/tmp/opencode/` and from the main checkout's git object store, with
  the working tree never touched.

## Verdict

The engine is real progress: the state model, delta stream, blend table, delta
it replays into the mirror, the VMP1 seqlock ring, the NDI FFI signatures, the
AGC bands port, the tween fixture (re-derived independently from anime.js
3.2.2 — it is genuine output) and the delta-replay integration are all
carefully done and well tested for the scale. The porting discipline (JS-named
functions, JS line citations, deliberate-deviation comments) is unusually good
and made this review productive. The author's response to this review (see
“Fix-wave verification — Round 5” below) was fast, broad and mostly
high-quality — R-1..R-30 landed, with residuals.

**Status:** as of `22646926`, engine mode remains opt-in and should stay so
until the open items close. Engine-mode screen capture and Output Windows now
work through the Rust engine with no Electron mediation. The Scene-grid save button ("cannot save current
scene" report) works in engine mode as of `c06bfb1f` — `scenes/saveCurrentAsScene`
was implemented in the engine (Round 28). The user-reported defect wave has landed and is
verified (Rounds 15/16): module alpha/enable/blend commits flow (P1),
the background flattens to opaque black (P1), cameras enumerate and render
both UVC devices (author-verified), project-prefixed media paths resolve
(P1), the Gallery-stutter root cause is closed with ~1600× less polling
and a postering volumetric path; the single-pass generator class now renders
upright (the `gl_FragCoord` flip); the multi-pass buffer/feedback generator
class is still mirrored. The PipeWire-monitor audio list is in and verified;
its one live P1 (monitor→monitor switch silently dies, Round 19) is fixed
(`5851595f`, Round 22 — session-scoped capture teardown), and the Round-22
fix also gated `backgroundThrottling`, F/Escape repeat/editable handling and
`SET_SYSTEM_AUDIO_STATE` on engine mode. The Rust engine's default Scenes now
match JS **v16** (v13→v16 lineage byte-verified per field walk; Rounds 33/34/35/51). The round-20 premultiplied-alpha preview path
still renders translucent tiles darker than the live frame (now scoped to
three call-sites — isolated tiles, volumetric posters and Scene hover
cells; round-27 list). The more recent additions — native ISF transitions (`f591a108`),
engine-owned Output Windows (`a121eaea`), portal screen capture
(`145005ec`+`00b82246`), engine-owned NDI input+output (`5139d8a0`) — all landed
and are verified by reading (Rounds 36–54), as are the entire JS/Rust default-Scene sources
(v13/v14/v15/v16 seeds, semantically identical per field-walk; Rounds
33/34/35/51). What blocks a default flip now:

1. **Expression evaluation has no wall-clock/allocation budget** — the
   denylist doesn't hold (Round 13). An allowlist-AST validator or an
   OS-level rlimit/timeout around evaluation.
2. **Gallery-input parity for vec/color inputs** — Rust creates prop-level
   and meta-level inputs but never the `${inputId}-r/g/b/a` / component
   lanes, and `$props` descriptors drop `label`/`step`/`component`/`enum`.
   Inspector degrades in engine mode.
3. **Dispatch-wildcard class** — `outputs/UPDATE_AUXILLARY` plus whatever the
   audit sweep hasn't read (`midi/learn`, `scenePreview/*`); the named v13
   arms landed (midi/*, ndi/*, fonts/*, SWAP acknowledged no-ops,
   dataTypes/createType implemented, windows, media/pick-screen).
3a. ~~ISF image inputs are decorative~~ — **fixed in `e4cf3e26`** (Round 38):
   assigned image assets resolve through a decode-once LRU, live media
   sources and module outputs bind directly, unresolved props fall back to
   the group canvas exactly as the JS default, and filter alpha masks skip
   when at least one image input is external. `Texture 2D` was inert
   at the time of this claim; it now samples real textures (`5139d8a0`,
   Round 43).
4. **ISF texture-free multi-pass generators** — 9 corpus shaders still
   render vertically mirrored vs WebGL (`slots.is_empty()` criterion too
   strict); fix selection to "no externally-supplied images/audio/IMPORTED".
5. **Group mutation beat regression** — `groups/UPDATE_GROUP` is all-or-
   nothing on any wrong-typed key (JS applies verbatim); `UPDATE_GROUP_BY_KEY`
   `transitionGhost` bypasses the fc8371c4 1/0 coercion. Document or fix.
6. **Program-engine restart reaper nits** (R-22 residual, R-35 apply-time):
   `should_restart`/`poll_health` still have no tests; non-looping-file
   park path unpinned; `.deb` missing `gstreamer1.0-pipewire`; RemoveGroup
   leaks group modules; R-35 unblended frame; `VolumeGrid` no override;
   GPU tests still skip without adapters.

The remaining conformance gap is not correctness but honesty and hardening:
the app is usable in engine mode, it just cannot fail closed.

## P1 — fix before anything else

### R-1 `crates/modv-renderers/src/isf/runtime.rs:838` ✔ — FIXED in `3754b249`
Trailing-implicit final passes were blitted over, breaking real ISF shaders

```rust
let last_target = passes.iter().rev().find_map(|pass| pass.target);
```

takes the last **targeted** pass. The JS sets `lastTarget = null` on every
implicit pass and only blits when the *final* pass has a target — an implicit
final pass is the destination. **30 corpus shaders have the affected shape**
(multi-pass with a trailing implicit final pass — measured from the vendored
corpus in Round 5; an earlier count of 33 in this file wrongly included three
single-implicit-pass shaders that were never broken). Affected: Glow, Soft
Blur, Lens Flare, Long Exposure, Auto Levels, Echo Trace, Ghosting,
VVMotionBlur 3.0, … For them, after the final (implicit) pass renders into
the destination, the code performed an extra nearest-filtered blit of the
pre-final buffer over the result (`runtime.rs:1095`), discarding the final
pass. The same wrong contract was baked into the docs (`runtime.rs:19-21`,
`overrides/README.md:77-78`, `mod.rs`), the 344/351 “success” narrative and
the multi-pass unit test.
**Fix (landed):** `passes.last().and_then(|pass| pass.target)`; magenta
regression test `trailing_implicit_final_pass_is_not_overwritten_by_a_blit`;
docs updated. Verified against the JS bundle's `lastTarget=null` per implicit
pass by the fix verifier.

### R-2 `crates/modv-media/src/audio/pipewire.rs` (`param_changed` ~L256, `process` ~L284) ✔
Unsupported negotiated format is decoded as f32 anyway

On a format mismatch, `param_changed` calls `queue.set_error(...)` and returns,
but the `process` callback then blindly runs `f32::from_le_bytes` on whatever
arrives, regardless of the negotiated format (e.g. S16LE @ 44.1 kHz → garbage
floats). Worse, `ChunkQueue::take_error` is one-shot (`capture.rs:192`), so
the error is logged once and never again, and `PipeWireMonitor::start`
reports the *requested* 48 kHz unconditionally (`pipewire.rs:105-110`), while
the analyser is built at 48 kHz from a stream that may be 44.1 kHz/16-bit.
**Fix:** park the stream on `process` (or bail the callback) until a supported
format is negotiated; keep the error latched instead of one-shot; report the
negotiated rate/format to the caller rather than the requested one.

### R-3 `crates/modv-media/src/audio/pipewire.rs` (`ready.send(Ok(()))` ~L373) ✔
PipeWire capture reports ready before format negotiation

`ready.send(Ok(()))` fires synchronously after `stream.connect()` returns,
before any `param_changed`. A silent/wrong-rate device reports a healthy
capture and the engine runs on with no indication (XRun counters or a
bytes-received check exist for this is on the caller).
**Fix:** send `Ok(())` on the first accepted `param_changed` (or after the
first buffer batch), error otherwise.

### R-4 `src/renderer/src/application/index.js:288` ✔
Cold start drops `state/sync`; the mirror is permanently stale

The engine emits `preview/ready` + `state/sync` immediately on accepting a
client (`crates/modv-app/src/supervisor.rs:218-234`). The main process taps
the client and forwards `engine/state-sync` to the main window only after
spawning (`src/main/engine-ipc.js:218`); the renderer subscribes in
`application/index.js:142` at module init. Electron does not queue IPC: a
renderer that has not subscribed yet silently loses the sync. `onStateSync`
has no late-joiner fallback — unlike `onState`, which pins the current
transition via `engine.getState()` — and nothing in the renderer calls
`snapshot()`. The mirror then never receives the pre-connect Groups,
Modules, `registered` map, seeded Scenes, or restored tweens, and only ever
sees post-boot deltas. (`tests/e2e/spec/engine/engineParity.spec.js` does
assert `registered > 300`, so the harness catches a fully-lost sync on the
happy path; it cannot recover the state and cannot detect a partial replay.)
**Fix:** mirror the `onState` pattern — pin a `state/sync` request at renderer
boot (an invoke that returns the current sync payload, replayed idempotently),
or have the engine re-emit sync on renderer handshake. Either way, add an e2e
case that kills the engine between spawn and renderer-ready.

### R-31 `crates/modv-renderers/src/point10/scenes/geometric_tunnel.rs:1326` ✔
GeometricTunnel frame hoops render ~16–24× too deep (neon washer walls)

The `Ring` band mesh is built with `depth = 1.0` (`build_ring_band(*sides, 1.0, 0.08, 1.0)` ~L1862) and every band instance carries its own z-scale — inner `0.035` (~L1355), ros tiles `0.02` (~L1493), target `0.045/0.03` (~L1496), braces `0.08` — **except the frame (outer) instance**: `frame_model = mat4_mul(&group, &rotation_z(frame_rotation))` has no z-scale, so the hoops extend ±0.5 × `depth_scale` (0.82 + bass·0.22 + flash·0.18) ≈ ±0.41–0.61 world units deep. The JS builds the identical hoop with `TUNNEL_HOOP_DEPTH = 0.05` (`GeometricTunnel.js:244`), i.e. ±0.025. Everything renders — just not the tunnel that was authored; the hoops read as solid washers.
**Fix:** give the frame instance a z-scale of `TUNNEL_HOOP_DEPTH` (or build the frame `Ring` band with `depth = TUNNEL_HOOP_DEPTH`).

## P2 — parity breaks and robustness

### R-5 `crates/modv-app/src/store_ops.rs:1030` + `tweens.rs:228` + `store_ops.rs:1440` ✔
`beats/SET_BPM 0` destroys every BPM tween descriptor in the state

`beats/SET_BPM` accepts any finite `bpm >= 0` and feeds it to
`update_tween_bpm`, which does `(3600.0 / 0.0) * division` → `Inf`
(`tweens.rs:895`) and stores it as `descriptor.duration`. `to_json()` →
`serde_json::to_value` refuses `Inf`, falls back to `{}`, and assigns that to
`self.state.tweens` — **every** BPM tween descriptor silently vanishes from
the authoritative state, the next snapshot and the next preset/scene capture.
`set_fps` guards this asymmetrically (`store_ops.rs:1309`, `.max(1.0)` — see
R-6).
**Fix:** clamp bpm at the action boundary (`bpm > 0`, else no-op like other
invalid params), and make the `to_json` fallback per-entry lenient (skip + warn
the offender) instead of collapsing the whole map to `{}`.

### R-6 `crates/modv-app/src/store_ops.rs:1309` ✔
`fps/SET_FPS` rebuilds tweens with `bpm().max(1.0)`

Every other rebuild path passes the raw bpm and relies on
`effective_duration` → 120 fallback (`tweens.rs:210`). `max(1.0)` gives a
division-32 BPM tween a ~57-second / ~3456-frame rebuild instead of ~480 ms
at bpm≈0, and diverges from the JS `store.state.beats.bpm || 120`
(`tweens.js:46`).
**Fix:** drop `.max(1.0)`.

### R-7 `crates/modv-engine/src/tweens.rs:719` ✔
Non-looping tween rebuilds lose the `seek` position

The no-loop branch returns `(frames, 0)`; the JS sets
`frames[id] = Math.floor(len * seek)` for both branches
(`tweens.js:129`). Every `beats/SET_BPM` / `fps/SET_FPS` resets non-looping
tweens to frame 0 — position jumps, visible mid-show.
**Fix:** `return (frames, restored_frame(frames.len(), seek));`

### R-8 `crates/modv-app/src/scene_ops.rs:1875` + `store_ops.rs:1412` ✔
Scene restore re-seeds tween playheads to frame 0 every time

`restore_scene_inputs` ends with `reload_tweens()`, which rebuilds the entire
`TweenSet` from the document with `frame = 0` — after `register_tween` has
already upserted each restored tween (making that call redundant). In
multi-row performance mode every Scene advance resets **all** tweens —
including the preset-level ones on rows that stay — so linked props snap to
their start values on each transition.
**Fix:** drop the trailing reload (the per-entry `register_tween` already
syncs the runtime), or make reload preserve playheads for id-stable
descriptors.

### R-9 `crates/modv-app/src/store_ops.rs:1412` ✔
`reload_tweens` discards all tweens on one bad descriptor

Any decode error fails `TweenSet::load_from_json` → the whole runtime becomes
empty, while the offending descriptor stays in `state.tweens`, so every
subsequent Scene apply re-wipes. JS creates per-tween and lets the rest
survive.
**Fix:** decode per entry; skip-and-warn failures (mirrors the unknown-easing
posture).

### R-10 `crates/modv-app/src/scene_preview.rs:273` + `compositor.rs:1948` ✔
“Warm” hover previews thrash the GPU instances they supposedly keep

Two cooperating defects:
(a) Every `render_frame` calls `prune(state)`; the preview state contains only
the cell being rendered, so **every render of cell B disposes cell A's shader
slots, 2D instances, Point10 scenes and ISF pipelines** (and `reset()` zeroes
the whole private compositor on LRU eviction, including the up-to-3 surviving
slots). Alternating a hover across cached cells pays full module re-init —
including Point10 scene init — every poll.
(b) `create_slot` eviction calls `compositor.reset()` on a shared private
compositor, invalidating the instances of all cached cells (state survives,
GPU objects do not).
**Fix:** make the private compositor prune scoped to the instances of the
slot being rendered — e.g. keep a `previews.scene_ids` set and prune by it, or
give each slot its own isolated device scope; at minimum, eviction should
dispose only the evicted cell's instances (they are keyed by fresh ids).

### R-11 `src/renderer/src/application/index.js:288` ✔
`_applyEngineSync` drops `tweens` and `expressions` from `state/sync`

The engine ships the preset document verbatim inside sync (verified
`store_ops.rs:1716`): `tweens` descriptors and `expressions` are in it. The
JS replay handles groups/modules/`registered`/inputs/links/scenes/performance
and seeding, but nothing replays tweens or expressions. Any tween present in
the engine at renderer boot (or after a snapshot restore) never reaches the
mirror: tween-typed props read `undefined`, `dataTypes/tween` paths break,
presets with expressions replay only partially.
**Fix:** in `_applyEngineSync`, commit an `inputs/ADD_TWEEN`-equivalent per
snapshot tween (or a `tweens/REPLACE_TWEENS` mutation) and replay
`expressions` assignments likewise.

### R-12 `src/main/engine-ipc.js:110-135` ✔
`engine/preview` hands uninitialised memory to the renderer; no seqlock

Still at HEAD: `Buffer.allocUnsafe(length)` allocated before the read
(ignored `readSync` bytesRead — a short read ships uninitialised pages into
`putImageData`), no `slot < info.slots` bound, and no magic/sequence check —
a mid-publish slot read yields a torn half-old/half-new frame even for
legitimate engine behaviour. The Rust ring (`modv-ipc/src/preview.rs`) has a
correct seqlock; the renderer must mirror it: validate magic, read sequence
(even), read pixels, re-read sequence, retry on change; then clamp `length`
by `info` and check `bytesRead`.

### R-13 `crates/modv-app/src/scene_ops.rs:1233` ✔
`scenes/removeScene` does not finish in-flight crossfades first

The JS completes transitions and row transitions before deleting Groups
(`scenes.js:2818-2819`); the engine removes Scene-stamped Groups (including
ghosts) directly. The performance runtime then holds `row_transitions` /
`transition` referencing deleted Groups, `tick_scenes` calls `set_group_alpha`
/ `complete_row_transition` on dead ids, and emits deltas for them; dwell for
that row stalls until the fade would have finished.
**Fix:** call `complete_all_row_transitions()` and
`complete_whole_scene_transition()` at the top of `remove_scene_action`, and
re-seed `performance.rows`/`SET_ACTIVE_ROWS` from the post-removal state.

### R-14 `crates/modv-engine/src/default_scene_migrations.rs:328` ✔
`apply_removal` deletes **all** same-named Scenes, against its own contract

`scenes.retain(...)` removes every Scene with the retired name; the surviving
“a same-named user Scene survives” comment (L348) and `seeded_names` check do
not prevent it. The production path (`default_scenes.rs:505`, mirror of the JS
`seen` set) removes at most one match. A user Scene that happens to share a
retired default name is silently deleted the moment the first real migration
rule lands in `DEFAULT_SCENE_MIGRATIONS` (currently `&[]`). **Data-loss
class.**
**Fix:** remove only the first (seeded) match, or carry seeded Scene **ids** in
the migration records; make doc, comment and code agree.

### R-15 `crates/modv-engine/src/scene_capture.rs:125,161` ✔
Unbounded allocation from hostile/corrupt `sceneRow`

`vec![None; max_row + 1]` (and `rows.resize(row + 1, None)`) size directly
from `Group.scene_row`, which comes from JSON. A snapshot with
`"sceneRow": 4294967295` forces a >100 GB allocation (abort); `usize::MAX`
panics in debug builds. The JS used sparse arrays and never allocated.
**Fix:** build the rows map as `BTreeMap` and materialise only the trailing
hole up to the last index, or clamp `max_row`/`row` to a grid bound
(`get_row_count`/`MAX_SCENE_ROWS`).

### R-16 `crates/modv-render/src/frame_graph.rs:573,721,926` ◐
Composite target not validated for `COPY_DST`

`run_composite` checks `RENDER_ATTACHMENT | TEXTURE_BINDING` on the dst, then
emits `copy_texture_to_texture` — requiring `COPY_DST`. In-tree callers
happen to include it, but the validator should; a bad caller becomes an
uncaptured wgpu validation error (which panics in the default handler).
Include `COPY_DST` in the required-usage check.

### R-17 `crates/modv-renderers/src/two_d/modules/{grid_stretch,pixelate,buffer}.rs` + `canvas.rs:637` ◐
Self-sampling 2D modules read the live canvas, not a snapshot

`GridStretch`/`Pixelate`/`Buffer` bind the live module-canvas view as an
external texture; per the logging of `Canvas::submit` (snapshot-split), that
is the texture being written — reading it with different content is
undefined; today it yields one-frame-lagged feedback plus undefined pixels on
the first frame / after resize (fresh texture, never zero-initialised). Also:
`Canvas::clip()` **replaces** the clip stack item instead of intersecting
(Canvas2D `clip()` intersects); and full-viewport `clearRect` drops earlier
same-frame commands via `clear_all()` instead of erasing (`canvas.rs:522,1175`).
**Fix:** route all three through `draw_image_self*` (like `Smear`),
zero-initialise canvas textures on creation, intersect clip on `clip()`, and
implement full-canvas `clearRect` as a transparent-black `Replace` draw.

### R-18 `crates/modv-media/src/osc.rs:57,303,373` ◐
OSC store is network-controlled blow-up surface

`MAX_ARRAY_INDEX = 1<<20`; a single datagram `/layers/1048575` materialises a
~1M-entry `Value::Array` (~25 MB); a flood of distinct addresses grows the
persisted JSON store without bound (binds 0.0.0.0:3333, like node-osc). Bundle
recursion in `dispatch_packet` is bounded only by datagram size.
**Fix:** cap array index growth (e.g. 64k), cap store size / per-key counts,
and cap bundle depth (8) with rejection beyond it.

### R-19 `crates/modv-media/src/tempo.rs:158` ◐
Unclamped `analysis_rate` / `history_seconds` → inf durations and OOM vectors

`frame_duration = 1.0/rate` is `inf`/`NaN` for rate ≤ 0; `history_frames`
becomes an unbounded `vec![0.0; …]`. Public constructor — clamp rate to a sane
range (as the other options already are).

### R-20 `crates/modv-ipc/src/socket.rs:125,334` ◐
Control socket unconditionally unlinks its path; mode not hardened

`bind()` does `fs::remove_file(path)` unchecked — it deletes whatever file is
there (any file, not just a stale socket), and `Drop` unlinks even if another
process recreated the path meanwhile. Mode is umask-default (world-accessible
under umask 000), and `engine/shutdown` over that socket exits the process.
**Fix:** unlink only when `connect` fails with ECONNREFUSED and the path is a
socket-node; `chmod 0600` after bind; same before-unlink check on `Drop`.

### R-21 `crates/modv-ipc/src/preview.rs:206,149,303` ◐
Preview ring `/dev/shm/modv-preview-*` world-readable; zero-slot ambiguity

(a) The VMP1 ring is created with umask mode — preview frames (fullscreen
capture contents) are readable by any local user. `chmod 0600` or `memfd`.
(b) Slots are initialised sequence=0 (even, “valid”) — a never-written slot
reads back as a black frame indistinguishable from a real one; init `sequence`
to odd (1) with an invalid magic, or track written-ness.
(c) A failure on the final header write leaves an odd sequence in the slot
forever; poison it so readers skip it deterministically.

### R-22 `crates/modv-media/src/audio/capture.rs:457` ◐
Per-callback allocation on the audio thread

`convert_buffer` news a `Vec` per device callback — GC/alloc
failure on the real-time thread can abort the process. Keep a scratch buffer
owned by the closure (same in PipeWire `process`).

### R-23 `src/renderer/src/application/engine-client.js:199` ✔ (read at HEAD)
Engine-mode UI silently black-holes what the engine doesn't implement

`osc/createServer`, `midi/ADD_DEVICE`, `midi/WRITE_DATA`, `windows/ADD_WINDOW`,
fonts enumeration, etc. hit the engine's `other => Ok(Null)` catch-all
(`store_ops.rs:371`). Consequences the user sees: MIDI devices never appear
(all MIDI input dies at `handleInput`'s `!device` guard), the **Output
Window feature is a silent no-op**, and OSC is inert — with zero feedback
beyond one warn-once line in the engine log.
**Fix:** at minimum surface "not supported by the engine" in the UI for these
(status toast / disabled item with a tooltip); ideally return a typed
`engine/unsupported` error the dispatcher renders.

### R-24 `src/main/engine-supervisor.js:276` ✔ (read at HEAD)
Spawn-async-failure + restart-budget accounting

`child.on("error")` only logs; a fast-failing spawn (ENOENT / EACCES /
not-executable packaged `modv-engine`) sits in "starting" until the ~100×
connect retry fails (~5–20 s) and routes through `_onExit` → restart budget.
Connect-timeout restarts also overlap a dying engine (SIGTERM → ~500 ms later
respawn + `unlinkSync` of the same socket path while the old engine still
holds it), and `restartTimestamps` is never reset on `shutdown()`/`start()`,
so a clean stop/start cycle inherits a stale crash budget.
**Fix:** route `child.on("error")` into the same `_scheduleRestart` decision;
await exit before respawn; reset `restartTimestamps` on clean stop/start;
document the overlap.

### R-25 `src/preload/index.js:11` + `src/main/engine-ipc.js` ✔ (read at HEAD)
Engine API exposed to every window; preview read path unvalidated

Every window (splash, output, color picker, output windows) gets
`dispatch/commit/snapshot/requestModulePreview/requestScenePreview`, far wider
than any of them needs. Separately, see R-12: no bytesRead check, no slot
bound, no seqlock. And `ENGINE_IPC_CHANNELS` is exported as "for destroy()
symmetry" but wired to nothing — either consume it in `destroy()` or delete it.

### R-26 `crates/modv-renderers/src/isf/source.rs:1151,409,802,710` + `runtime.rs:135,680` + `translate.rs:275` ◐
ISF robustness/edges

- `evaluate_size` has no upper clamp: `"WIDTH": "1073741824"` requests
  gigapixel textures → wgpu validation error → module silently dead every
  frame. Clamp to `max_texture_dimension_2d`.
- `RESERVED_INPUT_NAMES` contains GLSL control keywords; `rewrite_fragment_body`
  then `replace_ident`s **every** occurrence of `for`/`if`/`while`…, so a
  spec-legal ISF input named `return` makes translation unconditionally fail.
- `replace_ident(out, "texture2DRect", "texture")` maps pixel-coordinate
  samplers onto normalised-coordinate `texture()` — silent wrong sampling for
  ISF 1.0-era shaders.
- Overrides key off the **file stem** (`Soft Blur.fs` → `Soft Blur.wgsl`)
  with no content hash; any user media shader of that exact name inherits
  corpus overrides.
- `if !wgsl.contains("fn main")` is a substring match; a comment containing
  `// fn main` defeats it, and overrides that violate the `IsfUniforms`
  binding contract are never validated → unhandled wgpu errors, black module.
- Comment/code claim `FRAMEINDEX` starts at 1 vs the JS's 0 (off-by-one drift,
  `runtime.rs:135`).
- Size-formula substitution supports `$WIDTH/$HEIGHT/$<input>` only — the JS
  substitutes every uniform (`$TIME`, `$FRAMEINDEX`, `$DATE`, `$PASSINDEX`
  now error → module dead forever).
- `bool|event` arm: numeric `1` → `IsfValue::Bool(false)` (JS `!!1 == true`).
- `count_declarators` charges 1 location per declarator; `mat3/mat4` consume
  2 each → vertex sidecars can mis-number locations (report-only impact today).

### R-27 ISF corpus claim hygiene (`docs/isf-corpus-report.md`, `docs/rust-hybrid-progress.md`) ◐ — landed in `3754b249`

The corpus gate is `cargo test`, which (a) asserts only `translated > 0`, and
never asserts the 98.0 % bar, and (b) **rewrites the committed report on every
run** (`translate.rs:36,1002`), so the number in docs is machine/naga-version
dependent and silent regressions are invisible. Counts of `PERSISTENT` (43 vs
42, notes stale) and `FLOAT` (16 vs 15) in `docs/isf-translation-notes.md` are
stale. Also 7 override shaders are announced but the engine ships none
(`cargo build` does not include `crates/modv-renderers/src/isf/overrides`; the
`electron-builder.yml` `isf` resource ships only `resources/isf`), so in a
packaged build those shaders get **no overrides at all**.
**Fix (landed in `3754b249`, verified in Round 5):** checked-in
`isf-corpus-baseline.json` (344/0/7, 507 passes, 46 multi-pass, 15 FLOAT),
corpus test asserts ≥ 95 % + per-shader diff vs the baseline, report markdown
is written only to `OUT_DIR` (tracked file never rewritten by tests),
overrides are embedded (`EMBEDDED_OVERRIDES` via build.rs `include_str!`)
with `FNV-1a` fingerprint gating against `isf-overrides.json` (empty today),
`MODV_ISF_REPORT_PATH` affects only the manual report writer, and user
shaders named like corpus files cannot inherit overrides. **Still missing:**
the 7 failing corpus shaders have no override files and render nothing —
see Round 5 notes and the working list below.

### R-28 Point10 parity drift (`crates/modv-renderers/src/point10/**`) ◐

- `scenes/dvd_room.rs:1024`: glass opacity driven by `smoke_bass`; the JS
  uses the DVD-Room lane `smBass = band(1, 9)` (`DvdRoom.js:445`). Diverges
  whenever bin 0 is loud.
- `scenes/mirror_cube.rs:719`: `perm` driven by `band(-1, 1, 1)` bass instead
  of the `p9audio.beat(0.1)` kick envelope the JS uses.
- `scenes/it_is_coming_for_you.rs`: `preserve_bloom_alpha(true)` set — that's
  the Boxy/Wormhole semantic; `ItIsComingForYou` does **not** call it in JS,
  so bloomed pixels lose the alpha boost the JS adds (`ItIsComingForYou.js:250`).
- `scenes/it_is_coming_for_you.rs:440`, `chromatic/wave.rs:313`: `self.time`
  advances with uncapped `dt` (every other scene uses capped `frame_dt`); a
  catch-up frame jumps the clock.
- `scenes/cube_mesh.rs:380`: `perspective()` emits GL clip-z (near→−1) where
  wgpu needs [0,1]; everything renders today (near ≈ 0.1, camera units away)
  but it is not three.js's camera; `cube_mesh.rs:42` also contradicts
  `sound_cube.rs`'s actual depth state.
- `shared/assets.rs:283` (P2): small mip levels uploaded with
  `bytes_per_row = width*4` violate WebGPU's 256-byte `bytesPerRow` alignment;
  minified sampling reads uninitialised mips and every scene init logs a
  validation error. Pad the stride or stop the chain early.

### R-29 Scene/performance bookkeeping carried into restores ◐

- `store_ops.rs` `restore`: `performance.rows`, `row_transitions`, `transition`,
  `applied_scene_id` survive a restore that deleted the referenced Groups;
  deltas for dead ids flow until fades converge; `SET_ACTIVE_ROWS` is never
  re-emitted. Fix: clear performance runtime state in `restore()` and
  re-emit.
- `scene_ops.rs:681,1356`: whole-scene Fade keeps the **outgoing** Scene's
  Groups on top (created Groups sort before ghosts in `(scene_row, ghost)`
  order); the JS keeps the created Groups last → incoming draws on top.
- `performance.rs:606` + `Vec<String>` rows passed by clone: cosmetic.
- `compositor.rs` whole-scene Fade … (see R-13).

### R-30 ISF value coercion: `bool|event` arms ◐

Numeric `1` → `IsfValue::Bool(false)` (`as_bool().unwrap_or(false)`);
JS `!!1 === true`. Live events/bools with truthy numeric input read false
silently (events are never triggered, so impact limited to bools).

## P3 — minor and hygiene

- `engine_state.rs:419` ✔ (read at HEAD) — `order[..current_render_pos]` can
  slice out of bounds (sibling branch uses `.get()`); make both defensive.
- `preset.rs:36,224` ✔ — Scene `null` group holes are rejected by
  `parse_preset` (Scene files can't open as presets; JS manufactures empty
  Groups). Deliberate per code comment; document as user-facing behaviour or
  add a row-faithful path.
- `engine_state.rs:110` ✔ — `scene_row: Option<usize>` makes the whole
  snapshot undecodable on a single `-1`/`2.9` (JS truncates). Tolerate on the
  restore path.
- `scene_timing.rs:71` ✔ — `to_finite_number` rejects JS-legal `0x..` hex
  timing strings; add hex support or document.
- `two_d/canvas.rs:826` — `stroke_text` re-uploads the **same** mask ~13
  times per frame (12 stamps × new R8 texture); upload once, share the view.
- `restore()` — `flush_link_deltas()` output never broadcast before shutdown;
  engine exits with pending mirror deltas (`window.rs:56`, `store_ops.rs:1492`).
- `window.rs:179` — `EngineSupervisor::new` failure with no window parks the
  event loop forever (`resumed` returns early; `run()` never reaches the
  failed check); the Electron watchdog is the only exit. Call `event_loop.exit()`.
- `window.rs:340` — `Occluded` → redraw re-arm under `Poll` busy-spins a
  minimised engine; back off.
- `window.rs:340` — (same theme) `REDUNDANT_SUPERVISOR…` n/a.
- `module_registry.rs:1020` ◐ — `registered` prop descriptors drop `label`,
  `enum_members`, `step`, `component`, `VALUES/LABELS` → engine-mode enum
  props (Buffer/Texture2D/Waveform; ISF ENUMs) get empty dropdowns; props
  lose their labels.
- `store_ops.rs:1030` — `beats/SET_BPM` family emits **no deltas** (add/remove
  tween, fps/SET_FPS); mirror's tween map drifts until resync. Also
  `create_tween` reports success when the upsert failed.
- `store_ops.rs:1318` ◐ — `registered` map is rebuilt per reconnect (~440
  entries incl. ISF corpus meta); cache it in the `Store`. And test constant
  still says `37 + 15 + 18` (Point10 is 21).
- `store_ops.rs` / `input_links.rs:152` ◐ — sourceless `meyda/*` getters keep
  the disabled-Module gate (deliberate; JS does not gate). Warn once when a
  link is skipped for this reason, like the easing fallback.
- `input_links.rs` ◐ — client-supplied tween `id` with `'` breaks
  `parse_tween_link_location` silently; validate/escape ids at ingress.
- `scene_ops.rs:306` ◐ — `preview/scene` deep-clones the Scene +
  `row_names` per request including warm cache hits; clone on miss only.
- `modv-ipc/Cargo.toml:10` ◐ — `memmap2` declared, never used; drop it.
- `preview.rs:206` — ring file world-readable (see R-21); `preview.rs:149` —
  never-written slots read as valid black frames; `preview.rs:303` — failed
  final header write strands an odd-sequence slot; init `sequence` to odd.
- `analyser.rs:829-907` ◐ — the “1 % parity” analyser fixture compares 0 ≈
  0 over ~80 % of asserted lanes/cells (tone is 60 Hz + 6 kHz; mid/treble
  expected ~0). It validates bass only; regenerate against browser-measured
  capture or widen tone content past 2 kHz; also the dead
  `frame_dt_is_zeroed…` test asserts only `is_some()`; also `beat_detektor`
  infinite seed when `dt == 0` forever.
- `osc.rs:303` ◐ — bundle recursion unbounded; cap depth (see R-18).
- `midi.rs:281` ◐ — device key with `.`/`/` aliases lodash path traversal
  (renderer-local; keep a key→index map or escape).
- `ndi.rs:460,771` ◐ — SDK not `NDIlib_destroy`ed at exit; `send_rgba(&self)`
  has no internal synchronisation (document single-threaded or take `&mut`).
- `analyser` DT == 0 beat-detektor seed (see above); `pool.rs:199` ◐ —
  zero-size pool key → zero `bytes_per_row` write (wgpu panic); clamp or reject.
- `blend.rs:557` / `pipeline.rs:670` ◐ — two tests pin trivial contracts;
  replace with behavioural ones (golden rows exist elsewhere — good).
- Tween runtime: `ease()` infinite-loops for `t < -0.09` in `easeInBounce`
  (`pub` fn outside clamp domain); `steps(4, 2)` drops the second arg; anime
  multi-arg families (`easeOutElastic(1, .5)`, `spring(…)`, `cubicBezier(…)`)
  silently become `linear` though the comment claims the JS throws;
  `update_bpm` writes `Infinity` duration for bpm 0 (see R-5); easings
  dispatcher ignores anime's arg parsing (`tweens.rs:507`); the
  `tween-reference.json` fixture is genuine (independently re-verified) but
  has no generator, no completeness gate (`PENNER_EASING_NAMES.len()==41`
  only), and is validated only by `cargo test`.
- `tweens.rs` ◐ — `state.tweens` keeps test-prefixed `frames` keys? No
  — `frames` are excluded deliberately (JS parity). OK.
- Point10: `attractor_particles` camera `radius = 9.9` drifts from JS
  `distanceTo(centroid)` ≈ 9.68 (test pins 9.9 — undocumented deviation);
  `camera_distance` write-only field (dead store); dead uniform lanes
  (`logo`/`glass` `params.z/w`) — harmless upload traffic; `boxy` nomusic
  `0.0` vs JS `0.04` creeps slower in silence; HollowCube silence creep
  ~11× JS (deliberate, documented in module header);
  `capability`… `_rand` reversed-edge smoothsteps fine.
- `it_is_coming_for_you` additive bloom + `preserve_bloom_alpha` (see R-28).
- `scene_tests.rs:617` ◐ — doc comment says “SmokeRoom” on the DvdRoom test.
- Scenes.vue ◐ — engine hover preview: exception path in
  `pullEngineScenePreview` (try/finally, no catch) → unhandled rejection +
  30 fps re-arm loop; Parity spec never exercises `preview/scene` end-to-end;
  `is-modv-ready` latch is ordering-dependent, not a sync gate.
- `engine-client.js` — `scenePreviewFromResponse` comment/code drift
  (accepts oversized padded buffers); `onDelta/onState/onStateSync` drop
  previous unsubscribes instead of calling them (R-25 siblings).
- `Scenes.vue` — repeated hover churn re-uploads full RGBA per cell at
  30 fps; backoff is per-cell; same cadence as legacy but each poll is now a
  real isolated-compositor render through IPC — readme-worthy.
- `docs/rust-hybrid-progress.md:139,134` ◐ — stale “hover previews in flight”
  / “migration in flight” lines under their completed `[x]` counterparts;
  “Point10 batch 5a — 21 of 25” docs vs `DEFAULT_SCENE_MIGRATIONS = &[]`
  (AGENTS.md now documents the rewriting toolchain — that is unreleased
  working-tree state, treat as such).

## P2 — parity breaks and robustness (continued, Round 3)

### R-32 `crates/modv-renderers/src/point10/scenes/smoke_room.rs:1137` ◐
SmokeRoom alpha compositing order is inverted vs the JS

Panes render before the opaque lava sphere; the sphere (blend None) then overwrites the pane pixels, erasing the near-pane fade/fresnel in front of it. `SmokeRoom.js` marks the glass `transparent, opacity 0.35, depthWrite: false` and three renders opaque first, so panes blend over the sphere.
**Fix:** draw the opaque sphere first, then the transparent panes (or set the sphere transparent pass ordering deliberately and document it).

### R-33 `crates/modv-renderers/src/point10/scenes/smoke_room.rs:1043,647` ◐
Lava sphere ignores `boxScale`

JS parents the sphere under `_boxGroup` and calls `_boxGroup.scale.setScalar(scale)`, so the sphere radius is `0.9 × boxScale`; the port scales the glass and puff spawn radius but draws the sphere at identity — with `boxScale ≠ 1` the fire spawns from an invisible larger sphere than the one rendered.
**Fix:** apply `scale = boxScale` to the sphere model (or reuse `glass_model`'s scale factor).
### R-34 `crates/modv-renderers/src/point10/scenes/rolling_fire.rs:1332` ◐
No-audio path freezes the fire sim

`update()` returns before the puff loop/camera lerp/ball roll when `!audio.is_available()`; the JS `_animate()` keeps the sim running, and the sibling SmokeRoom (`smoke_room.rs:1092`) advances puffs unconditionally. A host with no audio device gets a static floor and sphere instead of idling fire — and the two WS-5/WS-7 siblings diverge.

## P2 — parity breaks and robustness (continued, Round 4)

### R-35 `crates/modv-app/src/compositor.rs:978` ◐
GPU-pass failure shows one unblended frame before the Fade fallback kicks in

When `apply_scene_transition` returns `false` (e.g. pipeline rejected), only
`transition_from` is dropped — the store transition stays active, so for one
frame the incoming Scene composites full-frame with the outgoing Groups
hidden (no mix at all), and the downgrade lands only on the next frame's
window-loop (`window.rs:441`). The comment at `compositor.rs:976` promises a
"falls back to the Fade ghost path next frame" — it is not the same frame,
and the WARN fires from the capture-missing branch, not from the
pass-failure branch, so the log is misattributed.
**Fix:** downgrade the store transition in the same frame (or at minimum log
at the failure site with the real reason).

### R-36 `crates/modv-app/src/performance.rs:918` ◐
Fade fallback resets alphas to frame-0 values mid-transition

`fall_back_scene_transition_to_fade` snaps incoming-group alphas to `0.0` and
outgoing to the recorded alpha **regardless of current progress**. The JS
(`scenes.js:3750-3758`) only clears `transition.shader` and lets the next tick
compute `to.alpha*eased` / `from.alpha*(1-eased)` from the current eased
progress — no snap. Resizing (or any `apply_scene_transition` failure) at
90 % of a 2 s transition visibly snaps the incoming Scene off. The frame-0
capture-failure path coincides with the JS behaviour, which masks this in
tests.
**Fix:** read the current eased progress and lerp alphas from it.

## Round 4 errata on earlier rounds

- Shader whole-Scene transitions (R-37..R-42 below) landed in `36672a33`;
  **R-13 (removeScene not completing transitions) is unchanged** — the delta
  touched `scene_ops.rs` only around the apply paths (`apply_scene_action`,
  `apply_scene_row_now`), not `remove_scene_action`.
- **R-29 (Fade z-order: outgoing on top of incoming) is confirmed by the
  transition work too** — `apply_scene_transition` pushes the hidden `from`
  groups and `emit_group_order` stably re-sorts them after their incoming
  twins of the same row; for the shader path they are hidden so it is
  invisible, for the Fade fallback path it is still the wrong order vs the
  JS `ADD_GROUP` push semantics.
- `docs/rust-hybrid-progress.md` now carries **428** as the module count?
  No — verified lines 24 and 41 of `36672a33`: both still say **424**
  (registered modules, engineParity smoke text). Stale in the newest docs.
- R-31 (tunnel hoops) and R-32..R-34 are unaffected by this delta.

## P3 — minor and hygiene (continued, Round 4)

- **Frozen outgoing frame (documented, not a bug report)** ◐ — the Rust port
  captures the outgoing Scene once at transition start and renders a hidden
  static frame; the JS re-renders the outgoing Groups every frame, so active
  outgoing scenes keep animating in JS. Also a deliberate full-output-size
  pass instead of the JS 1920×1080 cap. Both are documented as "behaviour
  parity, not pixel parity" in the progress doc — worth restating here so
  reviewers don't treat the freeze as a regression.
- **`scene_transition.rs:313` — `Circle` never fully closes at aspect ≳ 2.3**
  ◐ — `radius = smoothstep(p)*1.2` with a soft edge spanning 1.12..1.28 cannot
  cover ≥21:9 corner distances at progress 1, so the outgoing corners linger
  then snap away at completion. Faithful port of the ISF constant (parity is
  the priority) but the completion pop will be visible on ultrawide.
- **`scene_transition.rs:176` — `to_ne_bytes`** ◐ — uniform payload uses
  native-endian bytes; declaration says little-endian. No-op on x86/aarch64,
  garbage on BE hosts. Use `to_le_bytes` (and fix the test's `from_ne_bytes`).
- **Neither transition fallback branch is tested end-to-end** ◐ — the
  capture-failure→Fade and resize-mid-transition→Fade glue in `window.rs`
  (precisely where R-35/R-36 live) has no unit or e2e coverage; the
  store-level `fall_back_scene_transition_to_fade` *is* unit-tested, and GPU
  pixel tests cover mix/capture lifecycle, but window↔store↔compositor are
  never joined in a test.
- **`scene_transition.rs:422,548` — pipeline-build "failure" path is
  unreachable defence-in-depth** ◐ — `create_shader_module`/pipeline
  validation failures go to wgpu's default error handler (panic), not
  `Option::None`; the `failed[]` cache cannot trigger via that route. Same
  for OOM in `capture_transition_source` (`create_output_texture`). The
  documented downgrade safety-net on those paths is unattainable by this
  route. (Consistent with the rest of the compositor; not a regression.)
- **`performance.rs:226` / `scene_transition.rs:151` — checked, fine** ◐ —
  `progress()` is total; the timeline uses the *raw* delta (as JS
  `transition.elapsed += rawDt`), so even a stalled frame completes the
  transition in one tick; `clamp_progress` NaN→0 is a deliberate improvement
  over JS `Math.min/max`.
- **Completed: `remove_scene_action` and non-apply destructive ops still do
  not call `complete_whole_scene_transition()`** — see R-13; unchanged.

## P3 — minor and hygiene (continued, Round 3)

- **Engine-mode registry is missing two legacy wrappers** ◐ — `Point10 Earth`, `Point10 Sun` exist in the legacy adapter (31 Point10 wrapper modules in `renderers/2d/modules/`) but have no Rust scene and no `Point10SceneKind`/registry entry (26 registered now). `Point10 Ball Of Fire` **is** registered as the 26th scene since `270a0003` — the default-Scenes parity gap ("Solar Flare"/"Terminal Bloom") is closed. Earth/Sun are referenced by no shipped default Scene, so this is inventory, not a regression. Note: `"Point10 Crt"` and `"Point10 Old Movie"` — referenced by shipped default Scenes — **were** registered in the engine; `Point10 Old Movie` was retired in `5ebd4a24` (the Boxy Arcade Effects row now uses the `Sepia Tone` ISF filter), so only `Point10 Crt` remains registered.
- **`electron-builder.yml` `deb:` declares no `libpipewire-0.3-0`** ✔ — `modv-media` links `pipewire 0.9.2` unconditionally, so the packaged engine fails to `dlopen` on hosts without the library — *regardless of `--audio-source`* — and burns the supervisor's restart budget. The AGENTS.md note "the `.deb` picks it up from the distro" is not true (deb depends list absent → electron-builder defaults only). Fix: `deb.depends: ["default", "libpipewire-0.3-0"]` or dlopen like grandiose. (Default installs unaffected — engine is opt-in.)
- **PipeWire mid-stream death is silent** ◐ — no state/error listener calls `queue.set_error()`, so `take_error()` (one-shot, verified `capture.rs:192`) never fires and `window.rs:255-284`'s renderer-pump fallback is never reached: the engine renders silence with no log or fallback once the stream dies after startup.
- **Engine-owned audio vs UI meter: mixed sources** ◐ — the engine republishes bands at 10 Hz, but the renderer still commits its own 60 Hz `meyda/SET_BANDS` to the mirror (`application/index.js:843`), so “republishes UI meter bands at 10 Hz” is true on the wire only; the mirror is a mixed source. Pick one owner or document the interleave.
- **`window.rs:67,~556`** ◐ — final `flush_link_deltas()` is never broadcast (dead code; comments in `store_ops.rs:1859` claim the opposite); the 1 Hz `size/SET_SIZE` republish hard-codes `dpr: 1.0`, clobbering the real `scale_factor` pushed on `Resized` (wrong mirror DPR on HiDPI almost permanently); `surface_size` is never seeded from the initial surface (status bar stays 0×0 if winit never emits an initial `Resized`).
- **`store_ops.rs:1956`** ◐ — `restore()` drains deltas but leaves `engine_meter.pending` (stale pre-restore meter payload can be pushed later; `disable_engine_audio()` clears it properly).
- **Engine-owned capture has zero automated coverage** ◐ — `pump_engine_audio`/`disable_engine_audio` (`window.rs:255-298`) and the startup fallback are tested nowhere; `engineParity.spec.js` injects synthetic `meyda/SET_BANDS`, which a host with `MODV_AUDIO_SOURCE` set would have the engine ignore — the spec's Input-Link section is not hermetic.
- **`supervisor.rs:61`** ◐ — `MAX_PENDING_PREVIEWS` silently bumped 32→512 with no comment/test; still bounded and harmless, but the rationale (Gallery-full-of-tiles steady state) belongs in the source.
- **Docs arithmetic** ◐ — `docs/rust-hybrid-progress.md` still says **424** registered modules ("37 shader + 15 2D + 21 Point10 + 351 ISF") in two places; it is 428 with 25 Point10 scenes. Header also claims "M4 complete (25 of 25)" while a batch-5b checkbox remains unchecked — doc and code disagree in both directions.
- **`geometric_tunnel.rs:1548`** ◐ — dust X-rotation reads the already-reassigned `y` (should use the pre-rotation value); nearly invisible (|angle| ≤ 0.04) but wrong. **`:1673`** — billboard `forward` targets the origin, not the look-at target `(0,0,-TUNNEL_LEN/2)` (~6° tilt on dust masks; bolts unaffected). **`:2348` vs `:2425`** — `update()` truncates instances to MAX_INSTANCES(512) but `render()` draws by buffer slices using untruncated lengths (unreachable today at 182; latent validation error).
- **Afterimage wiring is partly dead** ◐ — `Composer::enable_/disable_/afterimage` (`postfx/composer.rs`) is unused; `LatentBackrooms` drives its own `AfterimagePass`. Also: latent_backrooms re-creates `grade` bind group **every frame** (`latent_backrooms.rs:1692`) — necessary because the afterimage history view alternates, but cacheable as a 2-state set; and no `onActivate`/`reset` hook exists in the Rust scene contract (JS clears the trail on activation; only fresh module ids per Scene apply protect the trail today). The Afterimage history targets use `linear_sampler` where three's AfterimagePass uses `NearestFilter` (texel-centre UVs make it invisible, but it is a silent substitution from the "bit-exact" claim).
- **GeometricTunnel depth culling dropped** ◐ — bolts and dust draw with `depth_stencil: None`; the JS Line2 bolts and Points dust are depth-tested against hoops. Undocumented; additive and low-contrast so likely benign, but it is a deviation.
- **SSAO/Afterimage/`TUNNEL` bounds** ◐ — otherwise the new postfx match three's r187 semantics (SSAO params `TUNNEL_SSAO` ported exactly; afterimage blend `max(new, damped)` correct; `damp` clamped 0..1 NaN-safe). Uniform sizes asserted in tests.
- **`cli.rs:364-410,109`** ◐ — `--audio-device` with `--audio-source renderer` is silently ignored; `pipewire:<target>` suffix accepted-and-tested but absent from `--audio-source` help; engine-owned capture is reachable only via `MODV_AUDIO_SOURCE`/flag env inheritance — `engine-supervisor.js` passes only `--socket/--state/--isf-dir`, so packaged users have no UI/launch path to it either.
- **`smoke_room.rs:655`** ◐ — emitter flicker hard-coded (`params = [time, 3.5, 1.0, 0.0]`) where JS drives `uFlicker` per frame; `params.y` (3.5) upload is unconsumed. **`hiStart` bin off-by-one** ◐ — both fire scenes start the hi-hat band at bin `floor(0.85*1024)=870`; JS `hiStart = 1024-floor(153.6) = 871`.

## Round 3 errata on earlier rounds

- Round 2 carried no geometric_tunnel finding — R-31 is new in this batch (the file itself is new).
- R-4 (cold-start `state/sync`) is **unchanged** — the sync wiring commit added the payload, not a late-joiner fallback.
- R-12, R-13, R-14, R-15 (all ✔-marked, still at HEAD of `c088335d`) — delta did not touch those files.
- Round 2's `module_registry` dead-code note is resolved — `registered_modules()` is now consumed by `sync_state`.

## Checked and clean (no findings — verified, not assumed)

- **Blend math** — all 28 modes, `soft_light` form, Porter-Duff table incl.
  `destination-out`/`xor`, HSL setLum/clipColor/setSat, `globalAlpha`
  premultiplication, W3C Compositing-1 identities; RowFade alpha port; MSAA
  2D blit chain; snapshot-split self-sampling (Smear); 160-byte uniform
  stride vs WGSL span; `vec4` array strides; bind-group/layout parity
  (10 fixed bindings + aliases).
- **State core** — `swap.rs` (4 passes, gallery preservation, `setDefault`
  semantics incl. JS truthiness quirks); `props.rs` clamp/abs/round incl. the
  `Math.round(-2.5)` case; `inputs.rs` (0-is-valid, min-only/max-only links →
  None); `engine_state.rs` round trip with `$status`/`drawToCanvasId` drops
  (documented); `delta.rs` minimal-JSON writer (2⁵³ fencepost); delta ordering
  (deltas before acks, sync before first delta).
- **Audio** — AGC bands/lanes/cells vs `audio-bands.js` (constants, floor,
  FFT_GAIN); tempo autocorrelation + octave resolution; BeatDetektor dual
  average, quantisation, `typeof(draft==undefined)` quirk reproduced;
  MIDI clock ring/24 PPQN/500 ms throttle vs `setup-midi.js`; Meyda extractor
  port (rect window, kurtosis/slope sign quirks reproduced) — maths correct
  as far as it can be checked without a browser; canvas 16→512 byte floor
  semantics vs `byte-frequency-data.js`.
- **NDI FFI** — all `extern "C"` signatures match SDK; recv→free ordering;
  dynamic loading paths; no dangling fn pointers; `#![forbid(unsafe_code)]`
  holds in modv-ipc.
- **Framegraph/executor** — main registered/unregistered around group passes
  so the only cleared `main` is the presented one; RowFade avoids
  attachment-sampling feedback; pool eviction is LRU for objects actually in
  use; `prune` frees group targets only for vanished ids.
- **VMP1 ring writer** — seqlock (odd/even bump), little-endian both sides,
  stride/height/len checks, `ispoweroftwo` guards — the one consumer gap is
  R-12 in JS.
- **Previews isolation** — fresh ids per slot (`scene-preview:{gen}:…`),
  private Compositor for previews, fresh `EngineState` per cell,
  `scene_id: None` self-sampler semantics, LRU accounting, 32/4 bounds,
  packets px-length-checked both ends; no wrong-client delivery (monotonic
  client ids); previews never dispatch or evaluate links.
- **Tween fixture** — all 41 penner easings re-derived independently (≤1e-12);
  `framesRequired`/`Math.round` semantics (negative durations → 1 frame,
  documented deviation); steps easing `ceil(minMax(t,1e-6,1)*n)/n`;
  read-then-advance link semantics incl. disabled-module gate restricted to
  `meyda` links (parity).
- **Point10** — audio bridge vs `audioBridge.js` (clamps incl. NaN→0);
  WaveSim vs `waterWave.js` (pinned to the JS Node reference inside tests);
  phrases/ROYGBIV state machine; GlassLens refraction direction algebra;
  SoundCube ping-pong order; MirrorCube protocol; UltraHDR drive functions;
  particle alpha map byte-equal; no per-frame panics; dispose/re-init parity
  (`every_scene_survives_dispose_and_reinit`, 21 scenes).
- **Supervisor/IPC** — framing caps, partial-read buffering, 64 MiB cap, dead
  reaper, restart budget tests genuinely integration-tested against a
  `fake-engine.mjs` speaking the real framing; SIGKILL path handled via
  `PR_SET_PDEATHSIG`.
- **Delta fixtures** — `engine-mirror.test.js` replays engine-emitted
  fixtures into the real Vuex mirror with deep `toEqual`; no vacuous asserts
  found in sampled tests; no `#[ignore]`/`skip` outside the GPU-adapter
  guards.
- **Default Scenes** — `DEFAULTS_VERSION=12` tables byte-identical to JS;
  embedded `default-scenes.json` validated (59 unique scenes); seed/remove/
  refresh semantics line-matched to `default-scenes.js`.

## Open questions for the engine author

7. Which of R-1..R-4 gate M7 (engine as default)? Suggest: all four, plus
   R-13/R-15, as a minimum "engine is safe to flip" bar.
8. Is the PipeWire capture path intended to ship at all in the packaged
   `--audio-source pipewire` mode (it links `libpipewire-0.3` dynamics, which
   the AppImage/.deb does not declare)? If yes, R-2/R-3 are blocking; if it's
   still experimental, the feature flag should say so.
9. The `Preview ring` should be created with an explicit mode at creation
   sites; is `chmod 0600` acceptable on `/dev/shm` files, or is `memfd_create`
   preferred?
10. `packaged engine + ISF overrides`: overrides are not shipped (R-27). Was
   that deliberate (size) or an oversight? If the latter, `extraResources`
   needs `crates/modv-renderers/src/isf/overrides`.
11. Scene hover previews are 30 fps `putImageData` of full RGBA through base64
   IPC (≈1.2 MB/s per hovering client at 320×180). Acceptable, or is the
   shared-memory ring the intended end state (same as main previews)?
12. Tween playheads on Scene apply (R-8): is resetting pre-existing tweens to
   frame 0 on every Scene advance the intended parity, or an accepted gap?

## Limitations of this review

- No code was compiled, tested, linted, packaged or run — this is a reading
  review with JS cross-checks. The author's e2e (144/144), benchmark and
  test-count claims were **not** re-run or reproduced (I count ~2100 raw
  `#[test]` attributes; the author reports 1788 — a difference likely caused
  by counting via `cargo` vs grep; noted, not arbitrated).
- Line numbers drift with each commit; `✔` findings were re-verified at the
  reviewed tip (not every fix was re-verified by execution), `◐` findings are
  single-pass reads (verify before fixing).
- GPU-dependent behaviour (mip upload, clip-z, bloom alpha, clip masks)
  verified by reading wgpu/three.js semantics, not by running.
- `crates/modv-engine/data/default-scenes.json` content was validated
  structurally by a reviewing agent, not by re-serialising against the JS
  source.
- The review covers committed code only; the working tree at the snapshot was
  never touched. Where the author's counts disagree with themselves (e.g.
  1629 vs 1630 Rust subtotals, 947 vs 1005 Vitest), both were recorded
  verbatim rather than arbitrated — running the suites was out of scope.
- Review rounds are appended below; see [Round log](#round-log) for exactly
  what was covered and when. Rounds 13–247 covered the media/PipeWire/
  expressions hardening wave, the user-reported defect fix wave, the module
  parity audit, the attribution/licensing fix, the ISF vertex-sidecar landing
  (both defect rounds plus the quad/top-left re-fix), the audio-side
  restoration (silence gate, smoothing ids, preview booleans) and the
  `scenes/saveCurrentAsScene` engine implementation (`270a0003..22646926`).
  The other thread has continued producing commits through this whole span;
  no fatigue condition observed. Any commits after `22646926` are not reflected.

## Fix-wave verification — Round 5 (`36672a33..916586f5`)

The author addressed nearly every finding in dedicated commits
(`011e432a` engine/ipc · `5732bdf2` media · `21402b4f` render/2D/Point10 ·
`3754b249` ISF · `54e86322` app · `f1f3e615`/`916586f5` docs). Every fix was
re-verified by reading the new code; GPU behaviour verified by reading only.
The author also prepended a status table to this file
(“Status after the fix wave”) — it is accurate for R-1..R-30 except as noted
in this round's errata.

**Verified fixed**

- R-1 ISF trailing implicit pass — VERIFIED (`runtime.rs:907,1169`; blit now
  `passes.last()`; magenta regression test; corpus measurement reconciled at
  30 affected shaders).
- R-2/R-3 PipeWire negotiation — VERIFIED for negotiation (F32LE/S16LE only,
  8–192 kHz, 1–8 ch, `process` parks until accepted, readiness after accept,
  negotiated rate reported, latched errors via non-consuming `ChunkQueue`,
  analyser built from the negotiated rate). **Residual:** a mid-stream server
  death still surfaces nothing — no `state_changed`/`error` listener, so
  `next_chunk` returns `Ok(None)` forever: silence, no log, no renderer-pump
  fallback (the module doc even claims otherwise). Also a mid-stream
  rate/channel renegotiation leaves the startup-built analyser in place —
  silent mis-analysis until restart.
- R-4 cold-start sync — VERIFIED with design change: the engine now serves
  `engine/get-sync` (sender-validated, main-window-only, returns *current*
  state); the preload exposes `getSync()`; `application/index.js` awaits
  `_pullEngineSync()` during `_appReady` before boot dispatches are replayed;
  the accept-time `state/sync` path also feeds the same idempotent replay
  (`engine-sync-replay.js`: removals→adds of groups/modules/inputs/links/
  scenes/tweens/expressions; `registered` append-only). Re-entrant safe.
  **Gaps:** no e2e kills the engine between spawn and renderer-ready; the
  accept-time event can still fire before the tap registers (the pull is the
  load-bearing path — fine); engine-client re-subscribers still drop the old
  unsubscribe (P3, untouched); mirror tween descriptors carry no `frames`.
- R-5/R-6/R-7 BPM/FPS tween rebuilds — VERIFIED (`bpm > 0.0` in both
  collectors, infallible `TweenDescriptor::to_json`, per-entry serialisation,
  `.max(1.0)` dropped, no-loop branch restores the seeked frame; JS `|| 120`
  fallback for bpm≈0).
- R-8/R-9 tween reload — VERIFIED (`restore_scene_inputs` ends with
  `prune_tweens`, not full reload; per-entry decode with skip-and-warn;
  failed ids keep the previous runtime). **Caveat:** `remove_input` on Scene
  apply still never prunes stale tween descriptors from `state.tweens` —
  the accumulator grows across a long performance; tracked as a residual.
- R-10 scene-preview thrash — VERIFIED (retained instances for cached cells;
  scoped `dispose_instances` of the evicted slot only; `materialisations()`
  stays 1 across 20 warm renders; live path unchanged).
- R-11 sync replay drops tweens/expressions — VERIFIED (replay helper;
  unit-tested against the real worker store; idempotent; stale collection
  replaced).
- R-12 preview IPC — VERIFIED (magic + odd/even seq drop + post-pixel re-read
  retry ×3, exact slot bound, `slotBytes-32` and 64 MiB caps, `readSync`
  byte-count-checked, no `allocUnsafe`).
- R-13 removeScene/stopPerformance transitions — VERIFIED (both call
  `complete_all_row_transitions` + `complete_whole_scene_transition` and
  re-publish `SET_ACTIVE_ROWS`).
- R-14 migration duplicate removal — VERIFIED (first same-named match only,
  seeded gate, drops all seeded entries for the name, idempotent re-run).
  Residual: migration records carry names not ids (documented limitation).
- R-15 scene-capture rows — VERIFIED (`MAX_SCENE_ROWS = 1024` shared with
  `scene_rows`, BTreeMap, out-of-range rows dropped).
- R-16 frame-graph COPY_DST — VERIFIED (`check_usage` + negative test).
- R-17 2D self-sampling/clip/clear — VERIFIED (snapshot-routed
  GridStretch/Pixelate/Buffer; `clip()` intersects incl. circle∧rect;
  full-viewport `clearRect` is an ordered transparent-black `Replace` draw;
  zero-init on create/resize).
- R-18/R-19 OSC/tempo bounds — VERIFIED as library hardening (1<<16 array
  index, arg/bundle/store caps, bundle depth ≤ 8; tempo rate 1–1000, ≤ 60 s
  history, ≤ 60 000 frames). Note: OSC is unwired (R-23), so this protects
  WS-10 later, not a live path today.
- R-20/R-21 socket/ring — VERIFIED (`is_stale_socket`: ECONNREFUSED + socket
  node only; mode 0600 after bind; `Drop` unlinks only the bound dev+inode;
  ring created `0600`, slots init sequence=1/magic=0, poison-on-failure,
  publish = 4-byte seq rewrite with full-header reader re-check, slot bounds
  vs file length). Residual: bind/unlink TOCTOU vs a racing engine persists
  (unlink-at-bind is not atomic); the “windowless supervisor parks forever”
  P3 remains.
- R-22 RT allocations — PARTIAL (scratch owned by the stream closures;
  residual: `ChunkAssembler::push` still allocates/splits one `Vec` per
  emitted block on the cpal RT thread).
- R-23 engine-mode black-holing — PARTIAL (user-visible hints for Output
  Window/MIDI/OSC; boot warning; `windows/createWindow` never dispatched).
  Remaining: plugins list shows empty with no hint; fonts panel silently
  shows the 4 system fonts; the BPM source dropdown still offers "midi" and
  commits `SET_BPM_SOURCE midi` into the catch-all; `setupMidi()` still
  commits `midi/ADD_DEVICE`/`WRITE_DATA` into the void with no feedback.
- R-24 supervisor spawn/restart — VERIFIED (error→restart routing on
  “starting” state, stale-child gating, SIGTERM→bounded-await→SIGKILL with
  overlap log, `restartTimestamps` reset, budget logic extracted + unit-tested,
  window-pruned, fake-engine integration tests added).
- R-25 preload scope — VERIFIED (`engineApiScope` = main-only; `window.engine`
  undefined elsewhere incl. output window; sender validation on every engine
  handler; `ENGINE_IPC_CHANNELS` deleted). Loose end: `engine/get-state`
  skips the sender check (trivial); `engine-client.js` re-subscriber leak
  untouched.
- R-26 ISF edges — VERIFIED except overrides (size clamp to device limit;
  keyword-safe renames; `texture2DRect` → honest rejection (not a `textureLoad`
  mapping, but loud > wrong); matrix/array location accounting; FRAMEINDEX doc
  clarified; `$` scalars in size formulas incl. `$PASSINDEX`; `!!` truthiness;
  `has_fragment_main` real check; override WGSL contract-validated end to end
  with fingerprint (`FNV-1a`) manifest — user shaders named like corpus files
  cannot inherit overrides). `has_default()` still cannot distinguish absent
  vs `DEFAULT: null` (docs claim it can; zero corpus occurrences).
- R-27 corpus hygiene — MOSTLY VERIFIED (checked-in `isf-corpus-baseline.json`
  (344/0/7, 507 passes, 46 multi-pass, 15 FLOAT), corpus test asserts ≥ 95 %
  + per-shader diff vs baseline, report markdown now written to `OUT_DIR`
  only, tracked report never rewritten by tests). **Still missing:** the 7
  failing corpus shaders have no override files — they render nothing until
  someone writes the overrides (`Pixelize`, four Point10 Swirl/Wave variants,
  `Shape Morph Feedback Mask/Wrap`). Both the manifest
  (`isf-overrides.json` = `{"overrides": {}}`) and the README say so — honest,
  but the WS-4 “98 %” figure hides those 7 as silent skips.
- R-12-adjacent ring publish protocol — VERIFIED (see R-21).
- `remove_scene_action`/`stop_performance` — VERIFIED (see R-13).
- Ghost ordering + `restore()` republish — VERIFIED (ghost inserted before
  created Groups; stable row sort; restore clears rows/transitions and
  re-emits `SET_ACTIVE_ROWS`). Residual: the per-row interleave of incoming
  Groups vs the JS's unnamed-row block is an in-code comment only, undocumented
  in progress docs; `restore()` still leaves `engine_meter.pending` stale.

**Still open after the fix wave (all re-confirmed at `916586f5`)**

- **R-31 (P1) GeometricTunnel frame hoops ~16–24× too deep** — unchanged.
- **R-32 (P2) SmokeRoom pane/sphere alpha order inverted** — unchanged.
- **R-33 (P2) SmokeRoom lava sphere ignores `boxScale`** — unchanged.
- **R-34 (P2) RollingFire no-audio path freezes the sim** — unchanged.
- **R-35 (P2) compositron failure shows one unblended frame before the
  Fade fallback** — unchanged.
- **R-36 (P2) `fall_back_scene_transition_to_fade` snaps to frame-0 alphas**
  — unchanged.
- R-28 sub-items deliberately kept by the author (documented as deliberate):
  `ItIsComingForYou` keeps `preserve_bloom_alpha` (JS does not call it — alpha
  boost lost); `AroundPlus` keeps the GL-shaped matrix (justified for its
  screen-space water maths).
- `texture_pool.rs:229` — zero-size pool key still produces a zero
  `bytes_per_row` write (wgpu panic). Unfixed.
- `stroke_text` mask re-upload (~13 R8 textures per frame) — unfixed.
- Engine-client unsubscribe leak; `engine-ipc` `engine/get-state` sender
  validation; `MIDI` BPM-source dropdown reaches the catch-all;
  `engine_meter.pending` not drained by `restore()`.
- Docs arithmetic everywhere in the Wave docs: `rust-hybrid-progress.md` and
  the new “Verification snapshot” still say **424** registered modules
  ("37 shader + 15 2D + 21 Point10 + 351 ISF") — reality is **428**
  (25 Point10); `store_ops.rs` test constant still `37 + 15 + 18`;
  `mod.rs` says 287 image-input shaders (measured **288**);
  `docs/isf-translation-notes.md` still says “63 multi-pass / 43 PERSISTENT /
  16 FLOAT” (measured **46 / 46 / 15** — the progress doc itself was updated,
  the notes/plan were not); progress doc claims 1630 Rust tests summing to
  **1629**, and “947 Vitest” in one place vs “**1005** Vitest” in CHANGES.md;
  the fix-wave author added a status table to this file claiming R-1..R-30
  fixed — accurate except the counts above and the silent omission of
  R-31..R-36 (they were simply not in the table).
- Review-file hygiene: the committed copy of this document now leads with the
  author's status table; the “In flight at the time of writing … they will be
  reviewed when committed” paragraph was stale (now updated), and “Last full
  pass” lagged behind (updated by this round).

## Round 6 (`916586f5..73729fdb`)

- `electron-builder.yml` — Point10 scene textures (`point10/textures`) now
  ship as `extraResources: textures`; `engine-supervisor.js` computes
  `assetsDir` (packaged resources path in production, source-tree path in
  dev) and exports `MODV_ASSETS_DIR` into the engine. Clean, minimal fix for
  the packaged-build texture fallbacks (reported as fixed in the progress
  doc: zero asset-fallback warnings, 107 fps, 144/144 e2e).
- Doc claims re-checked: “the `.deb` depends on the distro's package set …
  `MODV_ENGINE=0` (the default) never launches the engine” — the default-flag
  half is true; the `.deb` claim is **not**: `electron-builder.yml` `deb:` has
  no `depends:`, and the electron-builder default set (GTK/notify/nss…) does
  not include `libpipewire-0.3-0`, while `modv-media`/`modv-app` link it
  **unconditionally** (linux target-dep) — so on a minimal host without it,
  simply spawning the engine dies at load time regardless of
  `--audio-source`, and the supervisor burns its restart budget. Fix: build
  `modv-engine` with dlopen for PipeWire (like NDI), or declare
  `deb.depends: ["default", "libpipewire-0.3-0"]`.

## Round 7 — `73729fdb..9afa21dc` (ISF 351/351, gallery effect previews)

### R-1/R-27 follow-up — the last 7 corpus shaders — VERIFIED, with one honest caveat
Commit `77fcb048` adds four general GLSL→WGSL passes to `source.rs` (declared
hoisting of non-constant global initialisers; per-call-site cloning of
sampler-parameter functions; deterministic default returns for non-void
fall-through functions; column splitting of matrix varyings) and bumps the
baseline to 351/351, 507 passes, no overrides. Verified by reading only:

- All four passes preserve declaration order and only fire on unreachable
  paths or shapes naga previously rejected outright; the previously-344
  "translated" corpus keeps identical semantics by construction, and the
  corpus gate (exact name-set equality + ≥ 95 % assertion, `OUT_DIR`-only
  markdown) now pins it. No previously-fine shader can reach the sampler
  cloning path; matrix varying splits only the previously-failing sidecar.
- Pass-specific cares: const globals never hoisted; `#if`-guarded
  declarations never move; recursion refused (loud failure); clone tuples
  cache per distinct sampler-pair not per call (no exponential blowup;
  unbounded for pathological user shaders — no compile-time guard); a call
  *before* a sampler-function's definition is never inlined (documented
  false negative); `apply_edits` silently drops out-of-order edits (footgun
  for future passes, P3).
- **Caveat (docs):** the "100 % corpus" claim is materially misleading for
  **v002 Light Leak**: it now naga-validates, but its sidecar's `mat2`
  rotation is rebuilt from zero-initialised private globals — the leak's
  rotation is a silent no-op. Runtime behaviour is identical to before the
  commit (the effect was already dead), so it is *not* a regression; but
  "351/351 translation passes naga" must not be read as "all 351 shaders
  render fully". The `PERSISTENT`/`FLOAT` counts (46/15) remain accurate;
  the tracked report is consistent with the new baseline.
- All seven previously-failing shaders individually verified against their
  specific failure modes (Pixelize hoist; Swirl/Wave/Banded sampler clones;
  Shape Morph default returns; Light Leak column split).

### `9afa21dc` — gallery effect previews (test card) — VERIFIED, clean
Effects were rendered over a transparent/blank input so filter thumbnails
were blank. `preview/module` now renders over a synthetic UV-gradient +
checker card (white border), padded to the 256-byte row alignment, with a
synthetic `audio`/`audioFFT` texture pair for shaders declaring audio inputs
("animates even when the live frame is silent"). Textures are per-slot,
dropped with the slot (bounded by the 32-slot cache), `create_output_texture`
includes `COPY_DST` (no write-texture validation risk), image inputs are
bound to the same test card. Compositor tests now assert the effect frame is
non-uniform and opaque. No live-path impact; thumbnails now legible.

### `8df313c9` — corpus docs — fine modulo the caveat above
Claims "351/351 fragments, 36/36 vertex sidecars, no overrides" — the
*counts* reproduce exactly from the corpus, but see the v002 Light Leak note
and `mod.rs`/`isf-translation-notes.md`'s still-stale 63/43/16 figures
(carried from Round 5; unfixed in this delta).

### In flight (uncommitted, not yet reviewed)
At the time of this round, the working tree carries a substantially larger
scope: an **expressions engine** (`crates/modv-engine/src/expressions.rs`
and `crates/modv-app/src/expressions.rs`, new), a **JS plugin host**
(`src/renderer/src/application/plugin-host.js`, `install-plugin.js`,
`plugins/grab-canvas.js`, `use.js`, `tests/unit/plugin-host.test.js`), video
capture (`crates/modv-media/src/video/`, new), Point10 volume scenes
(`scenes/dvd_logo.rs`, `point10/volume/`, new), ISF `translate.rs`/`source.rs`
further changes with baseline bumps, `docs/rust-full-rewrite.md` (new), and
`Dockerfile`/Cargo.toml updates. Not reviewed until committed. The
full-rewrite doc suggests the project may be pivoting beyond the hybrid
plan — watch for its landing.




## Round 8 — `9afa21dc..89f8acc9` (R-31/R-35/R-36 fixes, Boa expressions, JS plugin host)

Three commits: `c5c5fd23` (fixes my Round 1–4 findings), `0ae8943c` (WS-12
expressions via Boa 0.22 embedded JS), `89f8acc9`
(renderer-side JS plugin host in engine mode).

### `c5c5fd23` — my findings' fixes

- **R-31 — VERIFIED (real fix).** The frame-hoop instance now carries
  `scale(RADIUS, RADIUS, TUNNEL_HOOP_DEPTH)`; regression test asserts the
  basis (column) lengths of `instances[0].model` ≈ JS contract; JS parity
  bit-for-bit (half-depth ≈ 0.021–0.031, radius 4.2 × pose.scale). No stale
  asserts.
- **R-36 — VERIFIED.** `fall_back_scene_transition_to_fade` now computes
  `eased = smoothstep(progress)` and lerps `to.alpha = alpha*eased` /
  `from.alpha = alpha*(1-eased)` — line-by-line JS parity (`scenes.js`
  `fallbackToFade`); sets values one frame earlier than the JS, with a new
  50 % midpoint test. Resize-mid-transition path reaches the same fixed
  fallback. One stale doc comment on the function (claims incoming resets to
  0) survived; the resize glue has no test.
- **R-35 — PARTIAL.** Pipelines are pre-built before the first transition
  frame (`prepare_scene_transition`), and the *prepare-failure* branch
  downgrades in-frame with an honest WARN at the failure site (no unblended
  frame). But that branch is unreachable in this wgpu (`failed[]` has no
  writer; native validation errors panic; no error scopes) — and the
  apply-time failure path (`render_frame_retaining`) **still shows one
  unblended frame** (drops `transition_from`, no WARN at the failure site,
  downgrade next frame via the "lost the captured outgoing frame" path with a
  misattributed WARN). Keep open until the failure site logs/downgrades or
  error scopes make the pre-build verdict observable; also add the
  window-glue test.
- **deb — VERIFIED.** `electron-builder.yml` now declares the nine
  electron-builder v26 defaults plus `libpipewire-0.3-0`. Minor: the frozen
  default list won't track upstream default changes (acceptable; correct
  today). AppImage keeps host-provided `libpipewire-0.3` (documented, fine).

### `0ae8943c` — expressions via embedded Boa (WS-12) — new surface, new defects

- **[P1] `crates/modv-engine/src/expressions.rs:138`** — no wall-clock /
  allocation budget on evaluation; the 100k loop bound and 64 recursion cap
  only bound VM-counted loops. Native builtins with their own loops
  (`Array.from({length: 1e9}, …)`, `.map/.forEach/.reduce` on huge arrays,
  `"x".repeat(1e9)`, `new Array(1e9).fill(0)`) run with no budget at all.
  Expressions run synchronously on the engine's event-loop thread (so do
  `evaluateEveryFrame` ones at every frame). A document shared as a scenes
  playlist → user "Import Playlist" → `restore_scene_expressions` compiles
  the source. A crafted assignment stalls rendering/audio for minutes or
  until OOM; repeated via `evaluateEveryFrame`, forever. (The Electron
  heartbeat watchdog would eventually kill the engine, and the restart
  budget would then re-apply the same Scene → crash loop → `fatal` — the
  damage is bounded but self-inflicted on a shared file.) Mitigation starts
  with `RuntimeLimitError`-style heap/timeout or an allocation ceiling + a
  per-expression execution cap (and *documented* trust stance for playlist
  import). Legacy Chromium had no budget either — but "engine embeds a JS
  engine in a security boundary" is now a shipped claim worth holding.
- **[P2] `__modvExpressionFunctions` is discoverable and WRITABLE/CONFIGURABLE.**
  Any expression can read/overwrite/`delete` any assignment's handle by
  predictable `e0, e1, …` keys (silent permanent freeze of that input) or
  dump every other expression in the document. Register NON_ENUMERABLE and
  freeze the map around rebuilds (or use unguessable keys).
- **[P2] every-frame evaluation order is nondeterministic** — `by_input` is a
  `HashMap`; two `evaluateEveryFrame` assignments targeting the same
  prop/module race in random order each process start (JS first-wins). Iterate
  insertion order (`IndexMap`/sorted).
- **[P2] lodash path parity** — `props.nested.0.x` (numeric string key vs
  array index) and `"a..b"` resolve wrong (serde_json `Value::get(&str)` vs
  arrays; silent `null`). No crash; silently wrong scope for valid paths.
- **[P2] `restore()` replaces `state.expressions` wholesale with no deltas** —
  mirror stale after restore (heals only via sync replay; matters once
  WS-13 wires restore to the UI).
- **[P2] the Boa realm is shared and long-lived** — prototype-pollution
  assignments (`Array.prototype.push = …`) poison every later expression in
  the session, including across restores. No realm refresh path exists.
- **[P3]** no compile cache (every restore re-parses all sources — typing
  in the Expression textarea re-compiles the set); `writeToSwap` silently
  ignored on all four actions (deltas carry `false` — mirror matches, but
  the caller gets no signal); per-frame `Vec<String>` clone per frame pass;
  `enabled: f64→i64` truncation drops fractional parts;
  null-returning every-frame props skip writes (intended) but
  null-returning `updateProp` writes `null` over the live value;
  **stale docs** — `rust-hybrid-progress.md` and `rust-hybrid.md` (WS-12
  “Recommended (a), not shipped") still claim expressions are unimplemented,
  while they are now a shipped, Boa-evaluated feature (and the empty-`osc`
  caveat exists only in code docs).
- Also note: claimed "260 modv-app tests" vs 261 counted; Boa
  `default-features = false`, no native bindings — the P1 is the only
  sandbox reality; scope (`value,time,inputValue,osc`) exactly matches JS;
  NaN guards hold; deltas and replay integration are correct;
  `evaluateEveryFrame` ordering (after link evaluation, before render)
  matches the JS; restore compile is skip-with-warning, never NaN.

### `89f8acc9` — renderer-side JS plugin host (engine mode) — mostly sound boundary, gaps

- **[P2] plugins can claim core mutation types** — non-namespaced plugin
  modules can register `groups/ADD_GROUP` (etc.) as their own mutation;
  the facade then applies the plugin's shadow handler to the mirror only —
  the engine never sees the write (permanent desync), and the same-type
  shadow handler fires again on engine→mirror replay. Require plugin
  namespacing or reject non-plugin types.
- **[P2] engine commits to unimplemented types vanish with zero signal** —
  fire-and-forget IPC; engine warn-once goes to stderr; the renderer never
  hears it.
- **[P2] async `preProcessFrame` rejections bypass the warn-once guard** —
  caught synchronously only; a rejecting hook logs via `unhandledrejection`
  every renderer frame, growing the daily log without bound.
- **[P2] unsupported plugins (grab-canvas) get a dead, silent toggle** —
  `supportsEngine: false` is enforced inside the host but the panel
  (`Plugins.vue`) renders an ordinary checkbox that never sticks; no badge,
  no reason shown.
- **[P3]** the unsupported guard is bypassable by a raw `plugins/*` commit;
  a throwing `init`/`shutdown` wedges the enabled state; `host.add` failure
  aborts the rest of installation (UI/install hook skipped for engine-mode
  installs where legacy always proceeded); camelized-name collision leaves a
  live-but-inert plugin (its mutations flow to the engine catch-all);
  `plugins/*` actions outside the three hosted ones fall through to the
  void; `preProcessFrame` reads a one-frame-stale payload (no
  read-your-writes between hook and delta replay); test suite lacks the
  async-hook/shutdownAll/collision coverage.
- **Checked and clean:** no new untrusted-code loader path — plugins enter
  via the built-in grab-canvas and `modV.use()` only; legacy media-dir
  `eval()` stays inert (engine catch-all) and untouched; no engine→mirror
  sync path touches `plugins/*` or plugin namespaces; the legacy worker
  path is byte-identical; grab-canvas cannot enable in engine mode; hook
  execution is fire-and-forget rAF with try/catch; frame ordering vs delta
  replay is deterministic (single thread); test file uses the real mirror
  store + stubbed engine.




## Round 9 — `89f8acc9..418c5cc8` (GStreamer video capture WS-10)

### `122389e9` — GStreamer video capture core — sound FFI, packaging/robustness gaps
Camera DeviceMonitor, `uridecodebin` file sources, `pipewiresrc` portal
nodes, test-pattern pipelines → newest-wins RGBA8 frames via a bounded
two-buffer pool. No GLib main loop (bus drained via `pop()`), no unsafe
code, gst objects owned by gst-rs values, `map_readable` folded during the
copy, no steady-state allocation, injection-safe launch strings
(`escape_launch_value` with a whitelist + escape path; verified),
`videoconvert`+`videorate` negotiation, file sync/loop semantics.

Findings:
- **[P2] packaged engine does not declare GStreamer** — deb deps weren't
  extended past PipeWire; the AppImage ships only the bare binary (plugins
  are dlopened); Dockerfile only adds plugin sets to the rust/toolchain
  images. `MODV_ENGINE=1` fails to load on stock Debian/Ubuntu; even a
  started engine dies on first camera open. Declare libgstreamer1.0-0,
  -plugins-base1.0-0, gstreamer1.0-plugins-base/good, gstreamer1.0-pipewire
  (or dlopen-style optional loading).
- **[P2] 1 s start timeout aborts slow file prerolls** — non-live
  `uridecodebin` must preroll; ASYNC-pending after the timeout →
  `CaptureError::PipelineRefused` misreported as “no error on the bus”; the
  module doc says “a timeout is not a failure” — the code is stricter.
- **[P2] stalled sources are never detected** — PipeWire node whose producer
  stops / V4L2 stops delivering: pipeline stays `Playing`, frames freeze
  forever; `sequence` stops advancing with no recovery or surfaced error.
- **[P2] Screen sources accept unbounded caps** — `VideoSourceKind::Screen`
  ignores requested dims and `info.width()/height()` is never clamped
  (`MAX_DIMENSION` applies only to *requested* sizes); a node advertising
  e.g. 65535×65535 drives `reserve(~17 GB)` → process abort. Cap negotiated
  size/total pixels. (Close to P1; kept at the reporter's P2 with this note.)
- **[P2] feature is dead code today with a new hard runtime dependency** —
  zero callers in `modv-app`/`engine-supervisor`/IPC; it compiles into the
  engine regardless → packaged engine gains DT_NEEDED on GStreamer for
  uncalled functionality until the IPC wiring lands. Document the gap or
  gate it.
- **[P2] live camera/screen tests can never fail** — both skip on error
  (only file/encode tests assert hard); the suite is 55/55 green with the
  camera path untested even in a misconfigured environment. Count is honest
  (2+10+4+18+10+11 = 55); the evidence isn't.
- **[P3]** `stop()` swallows `Null`/join failures (docs claim the callback
  cannot touch the frame afterwards, resting on unchecked results);
  non-RGBA/negative-stride negotiation returns `FlowError::Error` without a
  bus message, so the camera stays open in `Playing` until stop; camera
  enumeration is a point-in-time snapshot (no hotplug, stale v4l2src nodes
  listed); `screen_support()` treats a stale socket file as available;
  file-missing codec fails late with a generic message (no element pre-flight
  for h264/h265/vp9); `path_to_file_uri` lossy non-UTF8 bytes; 16k×1000 fps
  clamps instead of sane bounds; `test_support.rs` stop test under-skips
  required elements; `select_camera` empty-monitor fallback returns None only,
  no feedback channel for empty list vs busy device.
- **Clean:** launch-string escaping; frame pool swap (no torn frames, ≤4
  buffers); RGBA↔WGSL byte order; stride/neg-stride checks with
  `checked_mul`; FIFO caps parsing with sign checks and 16/structure cap;
  `sync=false` live / `sync=true` file; EOS→seek loop with latched failure;
  `PluginMissing` pre-flight; no unwraps on external input; 55 `#[test]`s,
  none hidden behind `#[ignore]`.

### `418c5cc8` — docs commit (this file's round-5/6 edits, committed by author)
No functional content beyond landing the working-tree state of this review;
the file in-tree now reflects rounds up to early Round 8.

### In flight (uncommitted, not yet reviewed)
`crates/modv-app/src/media.rs` (new — WS-10 wiring?), `src/main/engine-media-request.js`
(new), `src/renderer/src/application/engine-media.js` (new),
`tests/unit/engine-media{,-client,-request}.test.js` (new), `Dockerfile`
(GStreamer runtime in packaged images?), `compositor.rs`, `module_preview.rs`,
the R-32..R-34 Point10 fixes (`dvd_room.rs`, `rolling_fire.rs`,
`smoke_room.rs`, `dvd_logo.rs`/volume — new), `engine-ipc.js` /
`preload/engine-api-scope.js` / `engine-client.js` deltas (media channel?),
`docs/rust-full-rewrite.md`, `crates/modv-media/src/lib.rs` exports. Watch
for the media IPC commit — it turns WS-10 from groundwork into a real user
path, and it must fix the packaged dependency story (finding 1) to match.




## Round 10 — `418c5cc8..e88bbd07` (engine-owned media sources, WS-10 end-to-end)

Two commits: `9d5f2d87` (renderer/main: IPC, media-mirror, UI, engine-media.js
+ tests) and `e88bbd07` (engine: MediaHub, supervisor actions, compositor
texture pool, Webcam binding, window wiring).

### The two P1s (renderer-side file-source path)

- **[P1] `src/renderer/src/application/engine-media.js:336` — stored media
  paths start with a leading `/`, so engine video files can never open on
  POSIX.** The media manager stores `item.path = filePath.replace(mediaDirectoryPath, "")`
  → `"/video/clip.mp4"`. `isAbsolutePath()` matches any leading `/`, so the
  media-directory prefix is never prepended and the source id is built from a
  root-level path that doesn't exist. Unit tests only use relative
  `"default/video/a.mp4"` — pinned green while the feature is broken. Legacy
  works because `path.join` doesn't reset on a leading slash. Same function
  also keeps `..` unresolved. **The "video files play through Webcam" path
  does not work on a stock Linux/macOS install today.**
- **[P1] `src/renderer/src/components/Controls/TextureControl.vue:294` —
  engine file pipeline is killed by UI navigation, not the module.** The
  file-source lifetime is tied to the Control component: closing the
  inspector / switching modules or Scenes unmounts the control and calls
  `stopEngineVideoSource` — the video freezes on its last frame. Legacy
  kept the pipeline via the worker auxillary regardless of panel state.
  Preset/scene load never re-mounts the control, so a loaded video texture
  shows nothing until opened in the inspector.

### Engine/media findings

- **[P2] MediaHub:** (a) a `loop:false` file is replayed forever —
  `poll_health` restarts any `needs_restart()` source (Error and Ended
  lumped together), so the loop flag never stops replay; (b) a failed
  re-`start` of an existing sourceId silently removes it from `order` —
  all later sources re-index (a Webcam with `camera:2` silently becomes the
  previous source 3) and the failure is invisible to status/reconcile;
  (c) mid-list source stop is a silent Webcam feed switch — `camera:N`
  indexes the live source list, not a stable identity; no test covers the
  texture sampled after a mid-list stop; (d) `schedule_restart` runs inline
  on the render/heartbeat thread — `stop_source` joins up to 2 s, and a
  failed retry waits `wait_bus_error` a full 1 s; one unplugged camera ≈
  1 s frozen output per retry — bridges the 5 s Electron watchdog; (e) the
  restart bookkeeping is half-dead (`logged_failure` never set) and there is
  no permanent-failure latch — unplugged camera retries forever at cap.
- **[P2] Path containment absent** — `media/start-file` and `file()` keep any
  string with a scheme (`https://thing`, `rtsp://…`) and any pathverbatim;
  no canonicalisation/containment to the media root anywhere; renderer
  passes, main validates only presence. Not a privilege boundary (same UID,
  0600 socket, renderer already has nodeIntegration) but a contract the code
  claims to keep, plus a network-fetch vector from the engine process.
  (`cameraId`/`pattern` are launch-escaped, so no injection.)
- **[P2] Camera order divergence** — the engine indexes the Webcam `camera`
  prop across **all** running sources (cameras+files+screens, start order);
  the renderer computes `cameraIndexForDevice` over cameras only — any
  stop/re-start after a file source is running silently repoints every
  Webcam prop; the renderer never re-derives/re-asserts order. Observed as
  constant `reconcileCameraIndex` mismatch warnings.
- **[P2] Media not re-armed after engine restart** — no `onState running`
  hooks into `ensureCameras`; with the panel closed, selected cameras stay
  dead after a supervisor restart; `refreshVideoInputs` is only called at
  boot/devicechange/re-scan.
- **[P2] `media-mirror` cannot represent deletions** — `CLEAR_MEDIA_STATE` /
  `RESET_STATE` are dropped (only ADD is handled) — the library shows stale
  items whose sources can never start (no UI feedback); on top of that the
  mirror replay path throws on `media/addMedia` inside the same tick.
- **[P2] `media/list-cameras` failure ≈ empty list** — `ensureCameras` /
  `refreshVideoInputs` then stop every selected camera on a transient
  failure — one flaky enumerate makes live captures drop.
- **[P2] Re-scan starts with engine defaults, not saved `videoSettings`** —
  the boot-restore/pollStatus paths honour the saved per-device resolution/
  frame rate, `ensureCameras` does not.
- **[P2] File start is silently dropped or invisible** — started before the
  `media-manager-state` replay lands → `toEngineMediaPath` returns null →
  nothing runs, nothing logged; engine start failure only logs (no UI
  feedback); and it stops the *previous* pipeline first.
- **[P2] Unbounded retry loop** — fixed 5 s interval, no jitter, no max
  attempts/circuit breaker; permanently busy device = one start attempt
  every 5 s forever (each a full GStreamer nego.).
- **[P3]** Overlapping 1 Hz `media/status` polls (no in-flight guard; 4 s
  main-side timeout → up to 4 queued requests); `EngineClient` media methods
  + `withTimeout` are dead code; stop bookkeeping happens before the engine
  confirms; unguarded `state.media.media[project]?.video` computeds; screen
  path dead (`startScreen` passes ignored `name`, tracks nothing); tests
  pin relative paths only; media-mirror tests prove replay commits but
  never run real Vuex mutations; no test for the two P1 paths.
- **[P3] Engine:** non-looping file loop bug follow-throughs; mid-list stop
  details; `MediaTexture` pool never `tick()`ed → LRU collapses (~1 GiB
  re-create on size flip); `camera_index` rejects numeric strings the JS
  accepts; `media/start-test-pattern` registered unconditionally;
  `media/stop-source` swallows the boolean; `MAX_DIMENSION` 16384 → ~1 GiB
  texture alloc; no test for the mid-list `camera:N` behaviour nor for the
  frozen-texture backoff path.
- **Clean areas (verified):** all new channels sender-validated
  (`isMainWindowSender`); payload validation int-bound/positive-checked;
  error-code contract matches the engine exactly (`media_*`); engine-mode
  gating consistent everywhere (AudioVideo/ScreenCapture/
  ScreenPickerDialog/polling) — nothing inverted, legacy unchanged; status
  wire shape matches the JS normalisers (`{sources:[...]}`,
  `{cameras:[...]}`, start `restart` vocabulary does not fight the engine
  backoff); mirror store registered at boot with sane defaults; dual-commit
  parity with legacy; texture upload skips unchanged sequences, per-source
  pools, unaligned rows zero-padded (tested), preview/`Texture 2D` isolation
  holds; scene apply/restore genuinely keeps sources; stop_all synchronous
  before preview rejection and teardown; engine-mode interdependencies
  throughout are 0600 same-uid socket + 0600 ring — no cross-user access.
- **Docs to fix (this round):** `docs/rust-hybrid-progress.md` still says
  the deb depends "the distro's package set" — it declares only libpipewire;
  this commit adds GStreamer DT_NEEDED with no deb/AppImage
  acknowledgement; `TextureControl.vue` version claim; media progress
  entries.
- **Bottom line:** IPC/validation layer and the engine↔renderer contract are
  tight, and the renderer gating is clean — but the two P1s in the file-source
  path (path rooting; control-lifetime stop) plus the camera-order divergence
  mean this feature ships broken end-to-end on real installs today, in both
  user-facing contexts (files never open; file playback stops when you close
  the inspector).

## Round 11 — `e88bbd07..28d90158` (camera-index fix; Point10 SmokeRoom/DvdRoom fluid-sim re-port)

### `28d90158` — Webcam camera index follows camera-like sources — FIXED (engine half)
Engine now selects the Webcam input from the camera-like order
(`Camera`/`Screen`/`TestPattern`; files excluded) while still uploading
textures for every source — starting a file can no longer shift the Webcam
prop; renderer's cameras-only order matches. Residual: screen nodes count as
cameras engine-side but not renderer-side (the renderer gates screen capture
as unavailable, so the divergence only shows if a screen is started via the
diagnostic path — benign, but note it). The engine and renderer still need a
documented agreement here.

### `9e3ac2a5` — SmokeRoom/DvdRoom re-ported to a real GPU fluid sim — PARTIAL
~6 300 lines: stable-fluids sim (advect velocity/dye, curl + vorticity,
divergence, 2-iteration Jacobi, project), raymarched volume, bloom → ACES;
JS uses the same GPU fluid path — this is a faithful re-port, not an
"approximated to particles" regression.

**Prior findings against the rewrite:**
- **R-32 — VERIFIED FIXED** — panes (transparent, back-cull = JS FrontSide)
  render after the opaque sphere/logo; volume light combined additively
  after both, matching the JS chain.
- **R-33 — NOT FIXED** — `boxScale` still never reaches the sphere/logo
  mesh or the emission matrix (`smoke_room.rs:481/379/443`,
  `dvd_room.rs:419-428/599`); at `boxScale ≠ 1` the wind region and
  emission radius disagree with the visible mesh — same bug as before the
  rewrite.
- **R-34 — NOT FIXED** — `rolling_fire.rs:1332` returns before the puff
  loop when `!audio.is_available()`; the new volume scenes do not share the
  gate (internally inconsistent).
- **R-28 (DvdRoom lane pairing) — FIXED** — bloom/`dropHue` driven from
  5-bin `smoke_bass` (`dt*8`), logo/glass/camera from asymmetric
  `smoothed_bass` (`dt*16/dt*5`), glass opacity `0.34 + smBass*0.06` =
  `DvdRoom.js:445`.

**Regressions introduced by the rewrite**
- **[P2] GPU coverage for RollingFire and GeometricTunnel was silently
  deleted** — `rolling_fire_renders`, `geometric_tunnel_renders`,
  `geometric_tunnel_keeps_a_translucent_layer` all removed in the same
  commit; both scenes still ship. RollingFire has zero render coverage now.
- **[P2] transparency contracts weakened** — min-alpha ≤ 48/16 assertions
  replaced by ">10 % of pixels below alpha 128"; haze can now legally cover
  most of the frame.
- **[P2] production runs the full JS grid always** — 150×150×300 = 6.75 M
  voxels ⇒ 8 × 54 MB = ~432 MB `Rgba16Float` + ~40 M invocations/step, no
  production override (test-only `MODV_VOLUME_GRID`); weak iGPU /
  software-adapter users get no escape hatch and GPU tests don't run at JS
  size.
- **[P3]** `uSway` computed but never consumed (flame anchor missing the JS
  ±0.4 x/z term); attenuation correction missing the JS `max(0.01, d²)`
  clamp (dark pinhole at the flame core); `half_to_f32` decodes subnormals
  4× small (test-only); two `.expect("bloom pass always has a composite
  target")` panic paths on resize; translucent panes write depth
  (`depth_write_enabled: true`, JS uses `depthWrite: false`); `fire_temporal`
  evaluated and discarded whenever `steps > 0` (6 CPU Perlin evals/frame
  wasted); dead `params.z/w` writes; stale test-grid comment; heavy GPU
  tests (~minutes on lavapipe) dropped two unrelated scenes.
- **Clean:** advect/divergence/Jacobi/project signs and BC (clamp-to-edge =
  JS), dissipation < 1, emit-then-parity-flip order matches the JS shader
  chain, raymarch constants (Beer/multi-scatter/powder/dither), half-res HDR
  targets rebuilt on resize, no per-frame readback, the two transparency
  tests exist and are strict (`assert_alpha_is_max_rgb`), particle system
  fully gone, both volume scenes stay alpha-transparent end-to-end.

## Round 12 — `28d90158..270a0003` (RollingFire volume + BallOfFire; docs)

### `270a0003` — Rolling Fire becomes the JS GPU fluid sim; Ball Of Fire registered (26th scene)
The particle billboards are replaced by the shared 100×100×200 grid +
18-step raymarch + 2-step shadow march, hard key light, lava sphere/floor,
bloom/ACES; BallOfFire = RollingFire with the frozen ball (transparent
overlay, standalone) registered as the 26th scene — **my default-scenes
parity finding is closed** ("Solar Flare"/"Terminal Bloom" layers resolve
again). Docs bumped to 429 modules / 26 scenes / 1777 workspace tests.

- **R-34 — VERIFIED FIXED** @ rolling_fire.rs:1137-1148/1267-1277 — the sim
  step is submitted regardless of audio availability; silence arrives as a
  zero `freqMag` and the smoothers decay, the ball coasts; registered
  audio-reactive so live audio is picked up immediately. **But the
  regression test never exercises `render()` without audio** —
  `render_owned_chain_frames` only runs loud/quiet payloads, and the
  silent-path tests are CPU-only; a future gate on `is_available()` in
  silence would still pass everything. Add one GPU frame test (silent
  bridge, N frames, assert the dye field advanced or `pending_steps > 0`).
- **[P2] straight-alpha scene targets composited as premultiplied** —
  engine-wide (pre-existing, not introduced here): the frame graph assumes
  all-premultiplied, `blend.rs:415` multiplies `src.rgb` by alpha as if
  premultiplied, but Point10/ISF scene targets are **straight** alpha
  (`alpha = max(rgb_HDR)`, `rgb` 5–8× brighter than `alpha` for dim pixels).
  Over an opaque/bright backdrop the engine ADDs too much colour; over black
  it matches canvas. Solar Flare/Terminal Bloom sit on Background rows
  (black) today, so current default content is unaffected — but this is the
  one x-ray for every future Point10/ISF layer over opaque content, and it
  contradicts `present()` being documented as un-premultiplied. Needs an
  explicit decision (premultiply scene output, or un-premultiply in the
  composite path).
- **[P3] SmokeRoom/DvdRoom never wire `uSway`** — the flame point-light
  segment stays anchored at the sphere (`uSway` computed, never bound); JS
  sways the segment in both rooms; `VolumeUniforms::light2` doc still says
  "yzw unused".
- **[P3] GPU tests pass vacuously without an adapter** — every volume GPU
  test returns early on missing adapter; there is no lint/test CI; the
  lavapipe toolchain guards it locally but nothing gates it in GitHub.
  Both volume scenes resident ≈ 0.6 GB RAM/VRAM at once (RollingFire ≈
  128 MB, rooms ≈ 432 MB).
- **Docs**: 26/26 scenes, 429 modules now accurate; Vitest count still says
  **947** in the progress doc while CHANGES.md says **1005** (carried).
- **Clean:** BallOfFire registration end-to-end (kind, `Point10SceneKind`,
  all registry arms, meta vs JS wrapper, default-scenes resolution,
  `all_scenes` order, count tests, scene meta "2d"/"3d-scenes"/
  transparent-overlay); `_freezeBall` skip; the whole RollingFire audio
  mapping is JS-exact (bass dt*8, hats dt*10, snare dt*12, `1+hi*2.5+
  snare*8`, camera-yaw 0.15, hue shortest-arc ease); step order;
  rooms-pass-JS defaults (16 steps, key light) preserved after the
  parameterisation; `fire_colour.wgsl` extraction is behaviour-preserving.

## Round 13 — `270a0003..e916f42a` (verification of the hardening wave; group mutation parity)

Five commits: `2a149f16` (V4L2 camera enum, media-path re-root attempt,
inspector-unmount fix, PipeWire mid-stream latch, expression cap/denylist),
`a3cac91f` (review P1/P2 wave — expressions, media safety, R-33, packaging),
`a83cdd2d` (group mutation parity), `acfde60c` (R-34 silent-pump GPU test),
`57174b5b` (groups/updateGroupName) — rounded out by `1a94b31b` (Input Link
mirror settle flush) and `e916f42a` (final validation docs).

### `2a149f16` — what survived verification

- **R-2 residual (PipeWire mid-stream loss) — VERIFIED FIXED.**
  `state_changed` (error, or Streaming/Paused→Unconnected) and `error` both
  latch `formats.reject()` → cloned error → `AudioFeed::pump` breaks →
  `pump_engine_audio` logs once → `disable_engine_audio()` → renderer
  `SET_BANDS` un-gated again. `process` parks buffers once latched (no
  stale-format decode); heartbeat unaffected; once-only via
  `audio_error_logged`. Caveats: a transient PipeWire ERROR state latches
  forever with no re-arm path for the session (accept); and the `lost`
  predicate is untested (only format-rejection latching is tested) — extract
  and pin it.
- **Camera enumeration — VERIFIED for the libcamera SIGSEGV root cause**
  (Linux path queries V4L2 directly; non-Linux path untouched; metadata
  nodes dropped by caps; 64-mode cap; target-gated `v4l` dependency).
  New issues: fps truncation (`f.denominator / f.numerator` yields 29 for
  30000/1001 instead of 30) and `to_discrete()` eagerly materialising the
  continuous cross-product (~280 MB transient on a hostile device ladder
  until the 64-mode cap fires) — bound expansion.
- **Media path re-root — PARTIAL, and the default-profile shape remains
  broken.** Only `^/(video|image|palette|preset|module|plugin|scene)(/|$)`
  is re-rooted; the real POSIX library shape `/default/video/clip.mp4`
  (project-prefixed, from `read-file.js:8`) falls through the absolute
  return, so **engine video files still never open for the default
  project**. On Windows the shape is non-absolute → rejected since
  `a3cac91f` — engine files are broken there outright. Tests only pin
  `/video/clip.mp4`.
- **Inspector-unmount stop removed — VERIFIED (claim).** The engine source
  is no longer stopped on panel unmount; type-switch and path-switch stop
  it; quit stops everything. Residual: an orphaned file source (path changed
  by preset load / module delete / scene switch) decodes forever — nothing
  references it; `reconcileEngineSources` is cameras-only;
  `media-mirror` can't stop it either.
- **GeometricTunnel GPU tests restored** (`geometric_tunnel_renders`,
  `…_keeps_a_translucent_layer`) — VERIFIED.

### `a3cac91f` — the P1/P2 wave, and what it did not close

- **Expressions: the denylist does not hold and no budget exists.**
  a3cac91f changed **zero lines** of expressions.rs (the cap+denylist were
  all in 2a149f16). Verified bypass classes: `padStart/padEnd` unblocked
  (defeats `repeat(`), `.split/.join/.reverse/.concat`, `Object.keys/
  values/entries`, JSON parse/stringify over host-controlled input;
  **`({}).constructor.constructor("…compiled fresh…")()`** — the inner
  string is never token-matched, nullifying the whole filter; whitespace
  evasion of every spaced token (`repeat\n(1e9)`, `new\tObject`); BigInt
  literals (`2n ** 99999999n`) — no "BigInt" substring, no exponent cap;
  `regress-0.12` backtracking regex has no loop-count or timeout;
  `>4096-byte source` hard-fails with only a `warn_once` to stderr (silently
  frozen prop, no UI signal); the "unguessable" compiled-function key is
  `__modv_expr_{nanos}_{pid}`, still discoverable via
  `Object.getOwnPropertyNames(globalThis)` and WRITABLE → silent permanent
  freeze of every expression in the document; NaN guards hold, but the
  progress doc now claims the engine "trusts user presets but not hostile
  shared playlists" — **false assurance the commit itself added**. The one
  real mitigation is the realm reset, and it only covers cross-preset
  pollution; within a session pollution persists.
- **`media/start-file` absolute-local-only — VERIFIED.** `https://`,
  `rtsp://`, `file:`, `..`, relative paths and NUL rejected at the socket,
  both directions pinned by tests. Residual: no realpath canonicalisation —
  the absolute path names any local file a Scene playlist (or compromised
  renderer) names; the socket is same-UID so this is contract, not
  confinement — but the engine's `normalise_media_path` doc claims "escape
  the directory the renderer rooted the path under" is enforced, which it is
  not (only scheme/`..`/relative/NUL are).
- **Non-looping file parks at its end — VERIFIED** (`should_restart(state,
  looping)`; `Ended` → `attempts=0`, park keeps the last frame; loop:true
  replays from 0). **Zero unit tests for either direction** — both
  directions only exist as GST-gated integration tests that silently skip
  without GStreamer.
- **Frames >8192² refused — PARTIAL.** Upload-time refusal prevents the
  wgpu pool allocation, but the frame is already materialised by GStreamer
  (plus `copy_rgba_rows` reserve); refusal never reaches the renderer
  (`media/status` still reports the source live with `lastError` null —
  frozen healthy-looking feed); for screen sources there is no
  negotiate-time clamp at all.
- **R-33 SmokeRoom — VERIFIED FIXED** (`emitter_model = scale(box_scale)`;
  wind `origin.w` agrees; regression test pins it). **DvdRoom — NOT
  FIXED**: `dvd_room.rs` logo/seed still ignore `boxScale` (`logo_model()`
  has only pos/rot/pop); the commit message says "R-33 closed for
  SmokeRoom" — correct, but DvdRoom carries the same bug and no test.
- **`.deb` GStreamer deps — PARTIAL.** All five current names are valid on
  bookworm+ (`libgstreamer1.0-0`, `libgstreamer-plugins-base1.0-0`,
  plugins-base/good/libav) — correct and needed. Still missing
  **`gstreamer1.0-pipewire`**, despite this same change-set documenting the
  `pipewiresrc` screen path as implemented and tested; a `.deb` install
  cannot use screen capture and gets `media_plugin_missing` rather than a
  clear message.
- **Group mutation parity (a83cdd2d) — PARTIAL.**
  - `groups/removeGroup` leaks the group's **modules**: it cleans only the
    six control inputs and emits `REMOVE_GROUP` — the modules stay in
    `state.modules`, in the mirror's `modules.active`, and get written into
    every subsequent preset/Scene. The engine has the orphan-drop logic
    (`release_group`); the review's earlier finding stands, worse — the
    doc-comment now cites `groups.js:204-208` (a contradicting anchor) with
    no mirror-side note.
  - `groups/UPDATE_GROUP` unknown key types fail the **entire** multi-key
    update (JS applies keys verbatim; engine fails closed — a silent
    dead-end via fire-and-forget commit, surfacing only as a main-process
    warn). Deliberate but undocumented divergence.
  - `groups/REPLACE_GROUP_MODULES` silently drops non-string module ids
    (JS writes verbatim); empty array allowed; deleting a fading ghost row
    mid-crossfade pops visually (parity with the JS mutation, but
    preferable to guard or document).
  - `groups/updateGroupName` (57174b5b) — clean; unknown group warns-once
    and returns Null; payload replays cleanly into the mirror.
  - Test claims: "modv-app 288" vs 289 counted; mirror-parity fixtures
    (`tests/fixtures/engine/deltas.json`) originally not extended — as of
    `ae99d16b` the fixture now carries `modules/UPDATE_ACTIVE_MODULE_META`,
    `UPDATE_ACTIVE_MODULE_PROP`, the input records and the
    `ADD_ACTIVE_MODULE` payload (36 deltas); only `groups/*` mutations
    beyond `ADD_GROUP`/`UPDATE_GROUP_BY_KEY` are untested there.
- **`1a94b31b` — VERIFIED (read).** The settled Input Link value is no
  longer swallowed by the 10 Hz throttle: unchanged writes now flush the
  *current* value when the mirror's last delta differs from it, for module
  props (`modules/UPDATE_ACTIVE_MODULE_PROP`, exact prop-schema payload),
  meta values (`modules/UPDATE_ACTIVE_MODULE_META`) and group controls.
  Regression test settles a link inside the interval; the framing matches
  the JS worker's skip-if-unchanged semantics. Not re-run (no cargo).
- **`1a94b31b`/docs** — progress counts "1788 Rust tests" — plausible by
  count (verifier measured 1788 `#[test]`), Vitest still 947.
- **`acfde60c` — VERIFIED.** The R-34 silent-GPU-path regression test is
  real (122 silent frames, `any_lit` + frame-pair inequality), not a
  vacuous skip where an adapter exists.
- **Straight-alpha composite decision — documented and matching** (keep
  scene targets straight; composite at present). `blend.rs:53`'s "frame is
  presented un-premultiplied" comment still contradicts the raw-blit
  `PRESENT_WGSL` (`compositor.rs:2910-2927`); the doc decision makes this
  an accepted debt, not a bug — close the comment in a future pass.

## Round 16 — `e916f42a..fc8371c4` (the user-reported-defects fix wave — verification)

Seven commits: `117f0f0c` (module alpha/enable commits, opaque-black output,
preview 30 fps), `a1a9057f` (Webcam draws after restore; project media paths;
camera index from engine list), `f4548375` (Gallery stutter), `22296485`
(docs), `ae99d16b` (Gallery-added modules get prop/meta inputs), `6e60584b`
(ISF generators bottom-left), `a13dacd8` (docs) + `fc8371c4` (checkbox 1/0
coercion). The author's status report of the wave sits in “Round 15” above;
this Round 16 is my independent verification — every claim below was read in
the code at `fc8371c4`.

### VERIFIED fixed

- **[Round-14 P1] Module Alpha/Enable/Blend — VERIFIED.** UI mutation names
  `modules/UPDATE_ACTIVE_MODULE_META`, `modules/UPDATE_ACTIVE_MODULE`,
  `modules/UPDATE_ACTIVE_MODULE_PROP`, `inputs/UPDATE_INPUT_LINK` now alias
  the action handlers (`store_ops.rs:386-415`), emit the mirror-shaped
  deltas (`{id,metaKey,data}` / `{moduleId,prop,data:{value,type,path}}` /
  `{id,key,value}` / `{inputId,key,value}` — exact matches for the JS
  mutation records), and route through one expression-aware path. Tests are
  real: they drive store → compositor and assert pixels (alpha 0.5 →
  128-grey pass, enable false → [0,0,0,0]). `enabled` reads still require a
  boolean `true` at the compositor — the coercion is applied at every
  **write/load boundary** (`js_truthy` in engine_state.rs:241 plus normalise
  in preset.rs:242, scene_ops.rs:1730, and sync/ADD_ACTIVE_MODULE deltas
  via `_normaliseEngineModuleMeta` in index.js:177,374-403; `fc8371c4`
  additionally coerces the Checkbox's 1/0 on both write paths). Fresh
  install path: boolean everywhere. Old installs with `enabled: 1`: now
  rewritten to booleans on load — and verified on one legacy snapshot.
- **[Round-14 P1 UX] background black — VERIFIED.** `present` clears with
  `wgpu::Color::BLACK` (`compositor.rs:2716`) and premultiplied RGB is
  unaffected; `read_preview` flattens and forces alpha 255 (`compositor.rs:2839`).
  Interior composition stays transparent (`frame_graph.rs:826,522-524`) —
  OVER semantics inside the engine unaffected; the opaque flatten exists
  only at the two output boundaries (winit present + VMP1 ring which feeds
  the panel).
- **[Round-14 cameras] — VERIFIED.** Boot restore derives the 1-based
  engine index from the engine's own camera list (deduped, unavailable
  ids dropped; `engine-media.js:260-320`, `setup-media.js:225-245`) and the
  panel now lists engine-order; sync/Scene modules are re-asserted for
  boolean `enabled`; media-library paths re-root under `media.path`
  (`/default/video/clip.mp4` works) while drive-letter/UNC/`..` and
  true-absolute passthrough keep their semantics. Live evidence quoted by
  the author (both UVC cameras emitting, `WEBCAM2`/`WEBCAM2-EDGEBLUR`
  showing feed) not re-run by me — treated as author-reported.
- **[Round-14 P2 perf] Gallery-stutter — VERIFIED.** Tiles poll only while
  in viewport (IntersectionObserver, root:null + 240 px margin;
  `GalleryItem.vue:164-269`); volumetric Point10 previews cache an RGBA8
  poster and dispose immediately (`module_preview.rs:543-590`); preview
  cache grew 4→24 slots; `MAX_PREVIEWS_PER_FRAME = 1`; snapshot cadence
  600 frames; ring 640×360 at ~30 fps; panel pulls 30 fps.
- **[Round-14 P1 perf] preview 15→30 fps — VERIFIED.**
  `ENGINE_PREVIEW_FPS = 30` matches the ring (Preview.vue:47).

### PARTIAL / still open

- **ISF full-width — PARTIAL.** The bottom-left vertex shader is selected
  only when the module has no external texture bindings *or buffers*
  (`slots.is_empty()`, runtime.rs:873). 9 corpus texture-free **multi-pass**
  generators still run top-left and draw vertically mirrored content vs the
  legacy WebGL path (`Life.fs`, `Circle Trails.fs`, `pixel-tunnel.fs`,
  `Etch-a-Sketch.fs`, `fractal-mage-tunnel.fs`, `Random Shape Blast.fs`,
  `Color History.fs`, `lopy-frac-tunnel.fs`, `fractal-mountain-tunnel.fs`).
  The selection should be “no externally-supplied images/audio/IMPORTED”
  (i.e. all-`Buffer` slots count as self-supplied), not `slots.is_empty()`.
  Also: the `isf_FragNormCoord` convention docs disagree across three
  modules (only BOTTOM_LEFT's own doc is right); no orientation-sensitive
  pixel test exists.
- **Gallery-added modules — PARTIAL.** Props and meta now get Inputs, and
  mirror-parity covers 17 inputs in the fixture replay (real gate, JS
  mirror store). But the JS `initialiseModuleProperties` **dataType
  sub-inputs** (vec2/3/4 component lanes, color `-r/…/-b` lanes) are never
  created — engine mode leaves vec/color inputs dead; and **`label`,
  `step`, `component`, `enum` are dropped** from the `$props` descriptors
  (only type/min/max/default emitted) — Inspector labels degrade, enum
  dropdowns empty for enum-valued props; `Control.vue` falls back to a
  default `step`.
- **Group-mutation parity — PARTIAL.** `groups/UPDATE_GROUP` coerces the
  checkbox keys (hidden/inherit/clearing/pipeline/transitionGhost) before
  the typed decode and the mirror gets the coerced booleans. But
  `groups/UPDATE_GROUP_BY_KEY` `transitionGhost` bypasses the coercion
  (`value.as_bool()`, store_ops.rs:587), and `UPDATE_ACTIVE_MODULE` writes
  a full `meta` verbatim (`enabled: 1` would round-trip un-coerced — no
  in-repo caller, legacy-faithful but asymmetrically fragile).
- The dispatch-wildcard **class** is not fixed: `outputs/UPDATE_AUXILLARY`,
  `beats/SET_BPM_SOURCE`, `modules/SWAP`, `groups/SWAP`, `midi/UPDATE_DEVICE`,
  `ndi/*`, `fonts/*` still hit the warn-once→null path (no UI hint, same
  silent stance as the Alpha/Enable case). Only the four names needed for the
  inspector were aliased.
- Poster eviction is clear-all at 32 (`volumetric_frames.clear()`), not LRU —
  invalidates visible tiles in waves; and `.get` clones 1 MiB twice per warm
  poll (`Ok(pixels.clone())` + `insert(…pixels.clone())`). `scene_preview.rs:56`
  still documents “4 previews/frame”; `UPDATE_GROUP_BY_KEY` ghost bypass;
  stale “15 fps” comment sleeps directly above the new 30 fps comment.
- Snapshot cadence 600 frames is unverified against the snapshot (coarse
  regression risknoted).

### Checkpoint

- Dispatch aliases match the JS mutations 1:1; the mirror-parity gate now
  replays input records and asserts agreement including the new
  `ADD_ACTIVE_MODULE` payload (36 deltas — see `deltas.json`; the fixture
  now covers `modules/UPDATE_ACTIVE_MODULE_META/PROP` and the input shapes).
  Compositor module-enable
  check remains strict-boolean-by-design; coercion lives at every load/write
  boundary. No mirror divergence in-flight.
- The `/video/*` absolute-POSIX false positive on the media re-root is
  documented only as a code comment (not in docs/), has no warning/escape
  hatch to force passthrough; likely intentional trade-off but unflagged.
- The camera-selection fix depends on the engine's own deduplicated list;
  legacy installs with opaque Chromium device ids cannot migrate a saved
  camera selection across the switch — silently drops selections (documented
  in the commit body, not in docs/progress).
- Benchmarks (“120 fps @1080p all scenes”, “p99 23.9 ms”, “1 stutter/10 s”,
  “~0.02 ms preview”) are author-measured; I cannot re-run them, and they
  cannot be verified from code — treated as author-reported here.

## Round 17 — `f7b58951..e384269a` (fullscreen output window, tunnel surface maps, live audio when occluded; parity audit)

### `e384269a` — the three fixes

- **Fullscreen exit/toggle — VERIFIED (mostly).** Engine-side winit handler
  captures `F` (Ctrl/Meta/Alt-respecting via `ModifiersChanged`/`KeyboardInput`,
  `event.repeat` filtered), left double-click toggles the engine window, Escape
  exits fullscreen (windowed only toggles in-fullscreen). Legacy side adds an
  equivalent `keydown`/`dblclick` pair to the output-window popup. **Residual:**
  - `[P2]` `src/renderer/src/application/window-handler.js:129-141` — no
    `event.repeat` guard on the legacy F/Escape handler (Rust side has one);
    holding F strobes in and out of fullscreen on key auto-repeat.
  - `[P2]` same handler — no `event.repeat` check on the popup and no
    `isContentEditable` guard. about:blank today is safe, but any editable
    surface added to the popup later silently loses F.
  - `[P3]` same file — `dblclick` toggles fullscreen but doesn't
    `preventDefault()`; `dblclick` only fires for primary button so the check
    is implicit, but the F path preventDefaults while dblclick doesn't —
    asymmetry.
  - `[P3]` `window-handler.js:115-117` — `windows[id]` entries never removed
    on popup close (pre-existing; flag while touching the file — a stale
    window keeps the engine-mode guard for nonexistent popups).
  - `[P3]` after an engine supervisor crash-restart, the re-created native
    window comes back windowed; fullscreen is silently lost (nothing restores
    `self.fullscreen`); document it as accepted or add a persist/restore.
  - `[P3]` engine-side `ModifiersChanged` is written in its own event arm;
    on platforms where the modifier event lags the key event (X11), a fast
    Ctrl+F press can register as bare F and enter fullscreen accidentally.
    Shift+F works in practice. winit does not deliver modifier state on
    key events themselves.

- **`backgroundThrottling: false` — VERIFIED (mostly).** Wired in the main
  window's `webPreferences` (`src/main/window-prefs.js:143,149`) — exactly
  the window whose rAF pump matters (engine mode's mesh/bitrate path).
  **Findings:**
  - `[P2]` unconditioned in legacy mode too — legacy doesn't need it (its
    pump is Worker messages, not rAF); free but wasted on a battery
    forever-on minimized window, and confusing for readers who expect
    engine-mode-only. Gate it on `MODV_ENGINE` or document.
  - `[P2]` the e2e assertion (engineSkeleton.spec.js:98-104) is genuine —
    reads `getBackgroundThrottling()` from the built main window and expects
    `false` — but flag-level only; it does NOT prove the rAF/Meyda pump
    survives occlusion, and there is no test of the F/Escape/dblclick wiring
    on either window, nor of the popup keydown wiring.
  - `[P3]` comment is accurate for engine mode but the configuration path is
    shared with legacy mode (see above); a comment without a gate is a
    trap for a future optimiser.

- **GeometricTunnel surface maps — VERIFIED.** The brushed-metal grain
  (roughness) and equirect studio environment are now actually uploaded and
  sampled — they were implemented/tested in the codebase but never bound in
  the previous version (this was a real omission). `isf_FragNormCoord`
  convention (TUNNEL_BASE_TINT now the exact sRGB→linear of 0x3a4358) is
  exact. `wgsl_parses_and_validates` covers the composed/prepended WGSL incl.
  the new bindings. **Findings:**
  - `[P2]` The JS-mating claim over-reaches:
    - Bump map is not ported — `bumpMap`/`bumpScale` declared but never used
      (dead code, `geometric_tunnel.rs:1025-1028,2950,106-124` — the commit
      message says "bump", the code does not carry it);
    - The grain is sampled as sRGB-decoded in both roughness and
      emissive roles; the JS uses raw `NoColorSpace` grain for the
      roughness map (no decode) and sRGB only for emissive; the emissive
      multiplier has no JS counterpart (~`grain * 1.6`); the roughness
      formula (`clamp(0.8*(0.35 + 0.65*grain))` vs JS `0.8*green(grainMap)`)
      differs from the JS (approximated);
    - `in.world * 2.0` (code) vs JS `2.5x` (GeometricTunnel.js:579) and the
      code's comment cites 2.5 — comment drift;
  - `[P3]` the equirect studio env is sampled as a raw equirect 2D texture
    (no PMREM/CubeUV prefilter) — acceptable hand-tuned approximation, but
    studio panels will reflect sharper than the PBR ideal at low roughness;
    no Mip filtering — a roughness-only fake.

### `docs/module-parity.md` — audit, largely verified; "review the auditor"

The author's module-parity audit is strong: I sampled three claims and all
hold — (i) `Point10 Earth`, `Point10 Sun`, `Cube` are unregistered in the
engine (`grep` in module_registry.rs returns nothing); (ii) in engine mode
every ISF `image` input samples the **group canvas** — "There is no texture
registry yet, so a texture prop cannot supply a GPU texture" (verbatim at
`compositor.rs:2010-2027`), so the 288+ image-input shaders with assigned
textures decorative; (iii) `Texture 2D` has no image decoder in
`modv-media` — it renders nothing until a Webcam transport delivers a
texture (and I verified `texture_2d_does_not_use_the_camera_sources` only
proves absence). Also:

- **Transition-capable ISF modules are unusable in engine mode** — the
  picker (`Scenes.vue` `transitionOptions`: FADE/NONE + NATIVE + modules
  with startImage/endImage/progress, sorted) lists ISF modules in both
  modes, but the engine maps unknown names to Fade; selecting an ISF
  transition silently fades. (The parity doc's "27 transition-capable
  modules" is the count of the registry match — I verified the mechanism,
  not the exact 27.)
- **`modules/registerModule` is absent entirely** — no third-party user ISF
  or Canvas2D JS modules; the built-in registry is fixed at startup; the
  `media/addMedia` path throws in engine mode.
- Seeded default Scenes reference 61 distinct module names and all resolve.
- 0 counts inconsistency — `docs/rust-hybrid-progress.md` says 429; engineParity
  expects >300; `rust-hybrid-review.md` historical counts 428 vs 429 drift
  to be normalised to one number.
- **The audit's own counts** (Point10 scenes 26; shader 37; 2D 15; ISF 351;
  Gallery total 432 legacy vs 429 engine; 288/351 image-input ISF; 61 name
  set) — I verified: 429 engine side count matches, `grep -c "image"` in the
  corpus (number of image input declarations) is 288 (the claim is of the
  *shaders*, not declarations; both plausible, mine focused on registry).

**Two of the parity audit's own oversights belong in my review:**

- `[P2]` The 4 ISF module inputs that appear in the picker ISFs with image
  types are "decorative" in engine mode for *all* image-input modules — the
  parity doc summarises this but the review should call out the exact
  completeness boundary: the 288 image-input shaders register and run, but
  **none** can sample an assigned texture — the texture-prop → GPU path
  simply does not exist. This is the largest *functional* gap in the engine
  today (JS worker resolves textures into a GPU-binding for any texture
  prop).
- `[P2]` Texture 2D is registered but **invisible** in engine mode: it
  renders nothing with no input texture (by design) and there is no path to
  give it one; the Gallery shows a no-op tile. The legacy e2e suite covers
  Texture 2D rendering with a media image.

**The parity audit's remaining claims accepted as in-progress work.** The
fix order in `module-parity.md` (1–2 Gallery parity 432/432; 3–5 functional
parity; 6 parity guard; 7–8 hygiene) is sound. I'll watch those land.
## Round 18 — `e384269a..0f392ff9` (ISF texture-free gl_FragCoord flip)

**Verdict: VERIFIED for the 26 single-pass no-buffer generators** (The Pool Rooms,
st_Ms2SD1, UltimateFlame, UltimateSpiral, Warp Conduit, …).

- Single-pass, no-pass-target, no-`IMPORTED` texture-free shaders now render
  upright: `gl_FragCoord` → `isf_gl_frag_coord()` does a pointwise Y-flip in the
  fragment body (`source.rs:577-613`); `isf_FragCoord` re-routes through the
  helper; the vertex path is unchanged (raster rows identical);
  `RENDERSIZE` is the right flip source; identifier-aware `replace_ident`
  rewrites all body occurrences (incl. helper functions and hoisted global
  initialisers); can't disagree with the vertex selection since both reduce
  to the same compile-time predicate.
- **Nine multi-pass generators stay vertically mirrored** vs WebGL: `Life.fs`,
  `Circle Trails.fs`, `pixel-tunnel.fs`, `Etch-a-Sketch.fs`,
  `fractal-mage-tunnel.fs`, `Random Shape Blast.fs`, `Color History.fs`,
  `lopy-frac-tunnel.fs`, `fractal-mountain-tunnel.fs`. Fix is a consistent
  flip-both variant for the buffer-only class — write and sample are the
  same convention within a doc, so flipping either alone is wrong.
- Progress doc and `isf-translation-notes.md` say "Texture-free generators
  are upright" and the commit message / docs say "31 raymarchers fixed" —
  actual count is 26 (31 is the texture-free count including the 5
  buffered/feedback docs this commit does NOT touch). Stale docs repeated
  in three module files (`mod.rs:59-60`, `source.rs:131-143`,
  `translate.rs:913-917`) still claim unconditional top-left.
- Textured `gl_FragCoord` shaders (47 corpus files, mostly `IMG_PIXEL(
in, gl_FragCoord.xy)`) — deliberately unchanged by design; fine, each is
  self-consistent.



### The feature — what landed

Audio Input picker lists `Monitor of …` entries per PipeWire sink (via
`pw-dump`), the legacy single-item loopback list remains as fallback, and
latency-keying-record now happens on the renderer side of a whole
system-audio path. `src/main/audio-monitors.js` (a pure parser, no shell
interpolation, 32 MB cap, dedup first-wins by `node.name`), plus a new
`src/shared/audio-monitor-source.js` and `src/renderer/src/application/
audio-monitor-inputs.js`, `media-device-manager.js` appends/filters Chromium's
duplicate monitor echo, `setup-media.js` records the target sink via
`pw-record --target`, and `getDefaultPlatform()` now reads Electron's remote
process — fixing the silently-broken Linux capture path (and, as a side
effect, making macOS/Windows getDisplayMedia fallback reachable again).
`.deb` gains `pipewire-bin` (provides `pw-dump`/`pw-record`).

### Verified and clean

- **No shell interpolation anywhere** — argv arrays only, `pw-dump` is
  `execFile`-d with no shell, argv-safe monitor selection, sink whitelist
  validation (`isAudioMonitorTarget`), 24–32 MB maxBuffer, malformed/dup/
  missing-name handling skips rather than crashes, fixtures match the real
  `.info.props` schema, labels Vue-escaped.
- **IPC shape** — monitors listed the same on engine/legacy; error contract
  matches engine; engine never sees `SET_SYSTEM_AUDIO_STATE` anymore
  (`engine/...` guard at `index.js:1255-1265`) except one leftover site; the
  System Audio path drives the renderer analyser (and the engine) as before.
- **Parser/identity** — bytes-per-sink `node.name` as stable id; ids inline
  in closure capture (not exposed); XSS-safety by interpolation; unit tests are
  honest (zero mock of module-internal internals, real JSON).
- **macOS/Windows** — paths unchanged and unbroken; monitors are Linux-gated.

### Findings

- **[P1] `setup-media.js:121-135,196-199` + `AudioVideo.vue:320-345` — selecting**
  **any second system-audio input silently kills the just-created capture.**
  `Capture A` is torn down inside `setupMedia()` by calling the stale
  generator stream's `track.stop()` wrapper, which routes into
  `createPipeWireCapture`'s module-scope `stopActive()` — and by then `active`
  has already been reassigned to capture B (the one created in the Vue handler
  one step earlier). **Result: `Monitor A → Monitor B` leaves nothing live and
  the panel shows "(active: Monitor of B)" forever.** The same wrapper kills
  whatever is currently live whenever the renderer toggles between any two
  system-audio inputs (it works fresh-boot and mic↔monitor; it does not work
  monitor↔monitor, which is the headline use-case). Fix: in `setupMedia`,
  Close the previous system stream **before** `audioStream` wiring, or pass
  the current stream and exclude it from the stale-stop list.
- **[P2] `system-audio-pipewire.js:157-165` + `main/system-audio.js:96-103`** —
  pw-record dying after spawn is invisible. Server down / sink disappeared /
  `Code 1` → pw-record exits, `logger.warn` only, **no `system-audio:error`
  channel exists, and code-0 exits aren't reported at all.** The panel shows
  "(active)" over a silence forever (plus the `START_TIMEOUT_MS` 3 s timer is
  dead code — `ready` resolves at spawn, not at PipeWire connect; `ENOENT` is
  caught by `once('error')` only). **ANY PipeWire interruption strands the
  user with silence and no feedback.**
- **[P2] `AudioVideo.vue:469-474` — one `mediaStream/SET_SYSTEM_AUDIO_STATE`**
  call site was not moved behind the engineMode guard** — every audio-input
  change still fires that payload at the engine facade (`Vuex commit` → engine
  catch-all → "dispatch not implemented yet" warn-once). Same mutation, same
  fall-back — the commit only fixed the other two.
- **[P3] `audio-monitors.js:129-134` + `media-device-manager.js:51-53` — the**
  **`reason` string is dropped renderer-side** — no log, no notice, no UI
  treatment for "pw-dump missing", "no sinks", "wpctl missing": all degrade
  silently to the single legacy entry. Caching absent: every call to
  `enumerateDevices()` re-runs `pw-dump` (+ `wpctl inspect` + a second
  `pw-dump` inside `isAudioMonitorTarget`) — the worst case adds ~4–6 s per
  audio device action.
- **[P3] `media-device-manager.js:76` — over-broad `/monitor|system` `audio|loopback/i` regex**
  hides any Chromium input labelled `Monitor` or `Loopback` — studio
  interfaces, monitor headphones, other virtual loopbacks. Match by id or
  `packageName` cross-check (or at least restrict the filter to when the
  native discovery succeeded AND tag matches the actual node).
- **[P3] identity via `node.name`** — two sinks forced to one `node.name` by
  a buggy/malicious local sink collide; `dedup first-wins`; `object.id` is
  parsed but unused. No charset check on `node.name`; monitor-name collisions
  visible in the picker.
- **[P3] `.deb` is missing `wireplumber`** — `wpctl` is never declared; PipeWire
  desktops normally pull it via Recommends, minimal installs won't; the
  default-sink tag silently degrades then.
- **[P3] no e2e / unit test for this path** (the Docker toolchain lacks
  `pipewire-bin` and `wpctl`); the unit tests are honest but mocked and can't
  see the ordering P1.
- **[P3]`system-audio.js` IPC handlers don't check the sender** — `engine-*`
  channels all `isMainWindowSender`-gated; `system-audio:list-monitors|start|stop`
  are open to any window with the shared preload (output windows, others).



## Round 19 — `0f392ff9..beed7a20` (Firefox-style monitor inputs for PipeWire)

### The feature — what landed

Audio Input picker lists `Monitor of …` entries per PipeWire sink (via
`pw-dump`), the legacy single-item loopback list remains as fallback, and
capture now targets the chosen sink via `pw-record --target <node.name>` instead
of the old global default-sink monitor. `src/main/audio-monitors.js` (a pure
parser, no shell interpolation, 32 MB cap, dedup first-wins by `node.name`), a
new `src/shared/audio-monitor-source.js` and `src/renderer/src/application/
audio-monitor-inputs.js`, `media-device-manager.js` appends/filters Chromium's
duplicate monitor echo, `setup-media.js` records the target sink via
`pw-record --target`, and `getDefaultPlatform()` now reads Electron's remote
process — fixing the silently-broken Linux capture path (and, as a side
effect, making macOS/Windows getDisplayMedia fallback reachable again).
`.deb` gains `pipewire-bin` (provides `pw-dump`/`pw-record`).

### Verified and clean

- **No shell interpolation anywhere** — argv arrays only, `pw-dump` is
  `execFile`-d with no shell, argv-safe monitor selection, sink whitelist
  validation (`isAudioMonitorTarget`), 24–32 MB maxBuffer, malformed/dup/
  missing-name handling skips rather than crashes, fixtures match the real
  `.info.props` schema, labels Vue-escaped.
- **IPC shape** — monitors listed the same on engine/legacy; error contract
  matches engine; engine never sees `SET_SYSTEM_AUDIO_STATE` anymore
  (`engine/...` guard at `index.js:1255-1265`) except one leftover site; the
  System Audio path drives the renderer analyser (and the engine) as before.
- **Parser/identity** — bytes-per-sink `node.name` as stable id; ids inline
  in closure capture (not exposed); XSS-safety by interpolation; unit tests are
  honest (zero mock of module-internal internals, real JSON).

### Findings

- **[P1] `setup-media.js:121-135,196-199` + `AudioVideo.vue:320-345` — selecting**
  **any second system-audio input silently kills the just-created capture.**
  `Capture A` is torn down inside `setupMedia()` by calling the stale
  generator stream's `track.stop()` wrapper, which routes into
  `createPipeWireCapture`'s module-scope `stopActive()` — and by then `active`
  has already been reassigned to capture B (the one created in the Vue handler
  one step earlier). **Result: `Monitor A → Monitor B` leaves nothing live and
  the panel shows "(active: Monitor of B)" forever.** The same wrapper kills
  whatever is currently live whenever the renderer toggles between any two
  system-audio inputs (it works fresh-boot and mic↔monitor; it does not work
  monitor↔monitor, which is the headline use-case). Fix: in `setupMedia`,
  shut down the previous system stream **before** wiring `audioStream`, or
  pass the current stream and exclude it from the stale-stop check.
- **[P2] `system-audio-pipewire.js:157-165` + `main/system-audio.js:96-103`** —
  pw-record dying after spawn is invisible. Server down / sink disappeared /
  `Code 1` → pw-record exits, `logger.warn` only, **no `system-audio:error`
  channel exists at all, and code-0 exits aren't reported at all.** The panel
  shows "(active)" over a silence forever (plus the `START_TIMEOUT_MS` 3 s timer is
  dead code — `ready` resolves at spawn, not at PipeWire connect; `ENOENT` is
  caught by `once('error')` only).
- **[P2] `AudioVideo.vue:469-474` — one `mediaStream/SET_SYSTEM_AUDIO_STATE`**
  call site was not moved behind the engineMode guard** — every audio-input
  change still fires `engine: WARN dispatch … not implemented yet` (warn-once).
- **[P3] `audio-monitors.js:129-134` + `media-device-manager.js:51-53` — renderer**
  **swallows the `reason` string** — no log, no notice, no UI treatment for
  "pw-dump missing", "no sinks", "wpctl missing": all degrade silently to
  the single legacy entry. Caching absent: every `enumerateDevices()` re-runs
  `pw-dump` (+ `wpctl inspect` + a second `pw-dump` inside `isAudioMonitorTarget`)
  — up to ~4–6 s per audio device action.
- **[P3] `media-device-manager.js:76` — over-broad `/monitor|system`
  `audio|loopback/i regex` hides any Chromium input labelled `Monitor` or
  `Loopback`** — studio interfaces, monitor headphones, other virtual loopbacks.
  Match by id or `packageName` cross-check, or restrict the filter to when
  the native discovery succeeded AND tag matches the actual node.
- **[P3] identity via `node.name`** — two sinks forced to one `node.name` by
  a buggy/malicious local sink collide; `dedup first-wins`; `object.id` is
  parsed but unused. No charset check on `node.name`; monitor-name collisions
  visible in the picker.

## Round 20 — `beed7a20..2c7995e0` (Gallery hover previews composite over live output)

### The feature — what landed, verified

- Engine-mode Gallery tiles no longer poll on mount/visibility — nothing is
  requested until pointer enter, then ~10 fps (`{withOutput: true}`,
  `GalleryItem.vue:239-242`); canvas cleared on leave (`306-313`). ~150
  requests/s from a full Gallery removed.
- `preview/module` gained `withOutput: true` — the engine binds the live
  main texture (`compositor.rs:2875-2878`), scales it (top-left bilinear,
  matching the main frame convention), seeds shader/ISF image inputs with
  it (filters preview the live output — identical aliasing to live ISF), and
  blends the module layer over it with a premultiplied source-over pass
  (`{blend: Op::Add, src:One, dst:OneMinusSrcAlpha}` on both channels).
  Generators blit normally; transparent 2D/Point10 leave the live frame
  visible. Isolated previews remain the default elsewhere (supervisor
  defaults `withOutput: false`; engineParity unchanged).

### P2 findings (two — read both carefully)

- **[P2] premultiplied bytes shipped to `putImageData` as straight alpha**
  @ `module_preview.rs:823-841` + `compositor.rs:3035` — Main is
  premultiplied and `main` clears TRANSPARENT (`frame_graph.rs:826`); the
  hover preview scales raw main into the target and the module layer's
  premultiplied source-over, and the bytes cross into the renderer unchanged.
  The renderer feeds them to `new ImageData(...)` (`GalleryItem.vue:291`),
  which is straight alpha per spec. Every sub-opaque translucent colored
  pixel renders darker by alpha than the presented frame — exactly the
  advertised use (RollingFire over live output with translucent fire
  pixels). Fix: flatten over opaque black in the scale pass when `base` is
  present, or un-premultiply on read-back.
- **[P2] stale composite repainted after pointer leave** @ `GalleryItem.vue:245` —
  `blur()` sets `hovered=false` and clears the canvas, but the post-await
  guard does not test `state.hovered`, so a pull issued before blur repaints
  the stale live-output composite onto the supposedly-blank tile; the
  existing `finally` no-ops and nothing re-clears the tile. Fix: include
  `!state.hovered` in the post-await guard, mirroring the pre-await check at
  226-229.
- **[P3] redundant full-frame copy + dead buffers** — scale_base→target
  runs twice (base→slot_base then slot_base→target) where sampling base_view
  directly would do one blit; `slot_scratch` unused by 2D/Point10 arms.
- **[P3] wasted input seed for generators** — shader/ISF generators with
  zero image inputs still pay a full-frame scale into `input.view`.
- **[P3] module's own blend mode ignored** — overlay hardcodes
  source-over; hover preview never reflects `meta.compositeOperation`.
- **[P3] volumetric Point10 silently drops `withOutput`** — poster shown, no UI signal.
- **[P3] stale docs/comments** — module-level doc still claims "ISF 1x1
  transparent dummy"; `PREVIEW_QUAD_WGSL` comment describes a deleted
  frame-graph path; `docs/rust-hybrid-progress.md:189-191` still says 4/frame.

### Cross-checks

- `withOutput` plumbing end-to-end: `GalleryItem.vue:242` →
  `ModV.requestModulePreview` → `engine-client.js:471-483` (retry + backoff)
  → `preload/index.js:50-58` → `engine-ipc.js:198-213` → `supervisor.rs:369`
  (defaults false) → `window.rs:801-812` → `compositor.rs:2875`. Fully wired;
  no unwired readers; scene preview path untouched.
- Premultiplied source-over math matches the live `BLEND_WGSL` exactly;
  `LoadOp::Load` on the seeded target preserves the base; 1/frame preview
  budget respected; 24-slot LRU bounded; failure backoff intact.
- The GPU test (`compositor.rs:4476`) asserts *a* differing corner and one
  corner's live-colour preservation, but the module area's alpha blending is
  unverified; the renderer-side hover wiring has no unit/e2e coverage.

## Round 21 — `2c7995e0..254c3cb7` (Helix Eye & Traced Tunnel NC-SA 3D Scenes; attribution)

### The commit itself — verified

- Both modules land as built-in ISF modules with per-file in-file attribution
  (author/source/license/modification note) and CC BY-NC-SA 3.0 license notes
  inside `Helix Eye.fs` and `Traced Tunnel.fs`. `MODV_CATEGORY: "3D Scenes"`
  normalises to `3d-scenes`; naga translates both (unit asserts in the commit);
  corpus bumps 351→353 internally consistent; `THIRD_PARTY_NOTICES.md` ships;
  live shadertoy.com is unreachable for independent license verdict, but the
  notices correctly record the Shadertoy default = CC BY-NC-SA 3.0 (the
  "no CC0 era" claim checked out against three Wayback terms snapshots).
- **The notices' introductory line vs project licensing:** the notices state
  correctly that NC-SA modules are not relicensed under MIT; the README still
  proclaims a bare "[![License: MIT](...)]" badge which is now inaccurate for
  the distribution as a whole — the project bundles a non-permissive module
  set and its advertised license does not match the complete asset.

### P1 — findings

- **[P1] `THIRD_PARTY_NOTICES.md:3-6` — notices claim completeness; they miss**
  **four other Shadertoy-derived shader modules it ships** — `CollapsingArchitecture.fs:61-64`,
  `FractilianParabolicCircleInversion.fs:~126-135`, `LogTransWarpSpiral.fs:37-41`,
  `UltimateFlame.fs:174-181` — all NC-SA with in-file attribution of the
  Shadertoy authors, none appear in the notices. Patch: add these four rows.
- **[P1] notices do not list the Plane9/Joakim Dahl NC-SA family at all.**
  The vendored Point10 JS + Rust point10 scenes are explicit in-file CC BY-NC-SA
  (“Plane9 … preset (author Joakim Dahl, CC BY-NC-SA 3.0)”) — Boxy, ColorCube,
  Earth, HollowCube, ItIsComingForYou, LightTunnel, MirrorCube, SoundCube,
  SquareTunnel, Sun, Wormhole (JS + Rust twins of all), plus AroundPlus
  ("Plane9 Around And Around", license unstated) and `pcb.jpg` (Miguel Alba,
  variant unverifiable). Not listed; only MattiasCRT row implies NC-SA.
  Patch: add the family + `pcb.jpg` variant (or swap it for a CC0 photo).
- **[P2] `THIRD_PARTY_NOTICES.md:25` — st_Ms2SD1.fs misattributed** — notices
  say "author not stated … → Shadertoy default", but the file itself names
  Alexander Alekseev aka TDM, TDM's license explicitly ("License Creative
  Commons Attribution-NonCommercial-ShareAlike 3.0 Unported License.").
  Correct attribution fixes the row.
- **[P2] `THIRD_PARTY_NOTICES.md:32` — spherical-shader-tut.fs treated as
  CC BY-SA 3.0 without the NC, on an in-file "Creative Commons 3.0 Share
  Alike" with an unattributed Shadertoy link (XdKGWm).** A derivative can't be
  less restrictive than its source — that file makes no NC statement, so the
  notices mustn't invent one; at minimum default to NC-SA or check the link.
- **[P2] `THIRD_PARTY_NOTICES.md:33` — Perlin Transition row omits 4dS3Wd**
  **(Morgan McGuire @morgan3d)** despite the file listing "'2D Noise based on
  Morgan McGuire'. If 4dS3Wd carries no license statement (nontrivial on
  Shadertoy), its content is NC-SA by the notices' own default rule — the
  notices flag it but print only "MIT (stated in file)". Patch: add the
  dS3Wd dependency + license caveat.
- **[P2] `THIRD_PARTY_NOTICES.md:34-35` — Helix Eye row omits iq's raymarch/fog**
  **ports and the neilmendoza.com rotation matrix** (in-file, unspecified
  license), and the Traced Tunnel row omits NuSan (camera/tick code retained
  verbatim from the shader). Patch: extend both rows.
- **[P2] README.md:8,15 — legalese is better in the notices than in the**
  **README.** Badge still proclaims MIT; the note never says NC-SA means
  no commercial advantage, no redistribution for commercial use. Fix the
  badge + spelled-out NC-SA note; the notices have the necessary language.

### Translation & corpus — largely clean

- 351→353 delta = Helix Eye + Traced Tunnel exactly; all fail counts
  unchanged; `isf-corpus-report.md` with the same pair updated; PASSES=3 both,
  INPUTS=[] both, naga translates cleanly; both modules ↔ 3d-scenes category;
  no name collisions; no default-scene references — the modules live in the
  Gallery only.
- Helix Eye vertically mirrored vs WebGL (its `gl_FragCoord`-keyed ray is
  top-left) — cosmetic; `isf_FragNormCoord` consumers stay consistent.
- Non-const globals `bool eyelid/iris/pupil` rely on C++ zero-init semantics;
  naga rejects implicit initialisers in some translations; corpus gate catches
  those. naga cannot verify the JS behaviour, only bytes.
- e2e spec runs both modules registers/compiles/draws; nothing more.

## Round 22 — `254c3cb7..5851595f` (review-driven fix wave for hover previews, monitor switch, throttling)

### VERIFIED (the headline items, all confirmed in code)

- **Hover preview flatten — VERIFIED for withOutput.** Flatten op is "keep
  premultiplied RGB, set alpha=255" (`module_preview.rs:560-561`), applied
  only when `base.is_some()`; backdrop=black contributes 0, making the bytes
  identical to flattening over black. Isolated path byte-identical → e2e
  spec unchanged. Tile alpha has no consumers (GalleryItem styles are
  class-based).
- **Gallery post-await guard — VERIFIED.** `GalleryItem.vue:245-257` —
  `!state.hovered` required for resolve; failures-only bump queue already
  hovered-checked. No other post-await repaint path exists.
- **Monitor switch kills — VERIFIED.** Per-session `stopSession` closure
  in `system-audio-pipewire.js:164-269` kills only the owning session;
  stale track.stop can't call module-scope `stopActive()`; `pcm.close()`
  idempotent; unit test asserts exactly one IPC stop to the new capture —
  a genuine regression test (fails pre-fix).
- **`backgroundThrottling: false` gated on `MODV_ENGINE=1`** — legacy
  installs keep default throttling, output popups unchanged
  (`window-prefs.js:8,154`).
- **F/Escape auto-repeat + editable-target guards** — mirror the Rust-side
  behaviour (`output-window-controls.js:27-30,46-56`; enforced at
  `window-handler.js:131`).

### PARTIAL / still open

- **[P2] commit message claims an alpha-flatten GPU test that doesn't exist.**
  `module_preview_composites_over_the_live_output` has zero alpha assertions
  (the new base row is written with alpha 255 already), and its opaque base
  cannot distinguish pre- from post-fix. No test in the tree asserts the
  flatten path at `>=` 512²; the pre-existing ring flatten test
  (`compositor.rs:3661`) is untouched.
- **[P2] premultiplied-vs-straight defect remains on the other hover paths** —
  this fix flattens only the withOutput path. The isolated (plain Gallery)
  preview, volumetric posters (`module_preview.rs:524, :590`), and —
  critically — the **Scene hover cells** (`Scenes.vue:1197` painting via
  `drawEngineScenePreviewFrame`) read back premultiplied-unflattened bytes
  (`compositor.rs:2793`) and feed them to `putImageData` as straight alpha.
  Same class of bug; three more call-sites to fix for the advertised
  translucent preview use-case. Same fix applies — apply it wherever the
  engine ships premultiplied bytes to `putImageData`.
- **[P3] `AudioVideo.vue` engine-guard migration was narrow, not general** —
  `SET_SELECTED_VIDEO_SOURCES` (546-553), `SET_CURRENT_AUDIO_SOURCE`
  (557-565), `SET_VIDEO_SETTINGS` (731-733), `REMOVE_SELECTED_VIDEO_SOURCE`
  (748-755), `outputs/removeWebcamOutputForDevice` (816-823) still commit
  straight to `this.$modV.store` in engine mode — every one hits the
  engine's catch-all warn-once (`mediaStream/*` has no handler; `outputs/*`
  has none either) and silently drops. Same fix as the one the commit applied
  at :469-474; migrated in isolation.
- **[P3] re-enter repaint** — `GalleryItem.vue:238-260` — guard only tests
  `state.hovered`; leave→re-enter can pass the guard with the same offset
  old pull and paint for the new hover-session (self-corrects but is a
  residual window the fix does not close).
- **[P3] `isFullscreenExitEvent` carries no repeat/editable guard itself** —
  today's sole caller is guarded at window-handler.js:131; any future helper
  caller reintroduces the strobe. The unit suite only tests helpers.
- **[P3] startup fallback** — withOutput previews before the first frame
  render isolated and unflattened (`compositor.rs:2872`); the JS paints
  them as straight-alpha; cosmetic and intended but unflagged in tests.


## Round 23 — `5851595f..d6295e2b` (ISF sidecars land; filter transparency; default Scenes v13)

### The three areas landed, verified

- **ISF `.vs` sidecars compile, link and run.** The translator strips the
  active `#if __VERSION__` branch (selects the `#else` arm for `<=`/`<`),
  splits matrix varyings into columns (sample/set both fragment and
  vertex), assigns explicit output locations consistently between
  `rewrite_vertex_body` and `sidecar_varyings`, and the runtime now binds
  the vertex with `VERTEX_FRAGMENT` visibility for binding 0 **only when
  the module has a sidecar**; every pass shares the single three-vertex
  buffer. Multi-pass / feedback-buffer sidecars are covered.
- **ISF filters with image inputs mask their output alpha by the group canvas
  alpha** — Vignette/Edge Blur over transparent Point10 layers no longer paint
  black where nothing was drawn beneath. A specific Chromatic Abberation
  re-wrap preserves the canvas alpha; tests added.
- **Default Scenes v13** — CMYK Halftone removed from Halftone Harbor (its
  only default Scene); MBOX Light Tunnel's BPM dhue tween is gone (dhue
  fixed at 0). JS unit tests pin it; e2e spec pins it.

### Findings

- **[P1] Rust engine defaults are out of sync with JS — seeds v12 scenes with**
  **CMYK Halftone and the MBOX dhue tween.** `crates/modv-engine` keeps
  `DEFAULTS_VERSION = 12`; `data/default-scenes.json` still carries the old
  scene content; the JS bump to v13 reaches nothing on the engine side.
  Fresh engine installs ship the stale scenes, no Rust-only migration exists,
  and installs on v12 will never be refreshed in place (`REFRESHED_DEFAULT_SCENES`
  only handles v11→v12; there is no v13 lane in Rust). The gap contradicts
  this commit's message and means engine-mode fresh installs render a black
  Halftone Harbor backdrop with the removed effect and the hue storm the
  fix removed. **Fix:** bump the Rust pin to 13, update
  `data/default-scenes.json` to match `default-scenes.js` byte-for-byte after
  regenerate (same 59 names; the two scenes edited), extend the Rust pin test
  (`DEFAULTS_VERSION == 13`), and add a Rust-side assertion that the default
  set matches the JS file byte-content OR at least the two edited scenes match.
- **[P2] Sidecar modules sample the group canvas vertically mirrored vs**
  **the rest of the engine.** `isf_vertShaderInit()` emits
  `isf_FragNormCoord = vec2((gl_Position.x + 1.0) / 2.0,
  (gl_Position.y + 1.0) / 2.0)` — a bottom-origin varying — while the engine
  (every non-sidecar path) is top-left. Sidecar shaders sampling the window
  group (Edge Blur, Bloom Box, Sketch, Sharpen, …) render a flipped image of
  the rows below vs what legacy does. Symmetric-kernel blurs (even parity)
  self-correct but directional shaders (Emboss, City Lights) show vertical
  mirror flip. Fix: match `DEFAULT_VERTEX_WGSL` (`1.0 - ((y+1)/2)`); add a
  flip-sensitive test.
- **[P3] Vertex Manipulator.vs can never render correctly on the shared**
  **three-vertex buffer.** Its shader branches on `gl_Position` quadrants to
  decide mapping; a superset triangle has no top-right quadrant vertex, so
  the module's top-right quadrant interpolates to the top-left vertex — a
  different transformation vs the legacy 6-vertex quad from which ISFGLProgram
  draws. Corpus only impacts this one sidecar.
- **[P3] Corpus "all 36 sidecars compile" gate is GPU-conditional:
  `assert!(compiled >= 30)` and silent-`None` on adapter-less machines;**
  the gate passes with zero vertex tests run on CI lacking lavapipe.
  Should be `>= 36` (or `== N == vertex_total`) with a loud eprintln.
  Baseline test separately pins `vertex_total=36` — keep both.
- **[P3] Doc staleness persists in five files** — `mod.rs`, `runtime.rs`,
  `translate.rs`, `source.rs`, `docs/isf-translation-notes.md` and
  `docs/isf-corpus-report.md` all still say "report-only / sidecars are not
  consumed by the runtime". Stale contradicts committed code.
- **[P3] Fragment matrix varyings test covers non-paired code path** —
  `matrix_sidecar_varyings_translate` (translate.rs:1746) tests through
  `translate_fragment` only, not the paired vertex+fragment link path.
- **[P3] `bl` cache tag for sidecar modules** — misleading; the `|vs`
  suffix already disambiguates; drop the `|| custom_vertex`.


## Round 24 — `d6295e2b..54b48fc9` (Hot Shower as a built-in NC-SA 3D Scene)

### The new module itself — VERIFIED CLEAN

- **Hot Shower.fs** — full in-file attribution (author Kali / shadertoy.com/
  view/4lf3Rj; title withheld in notices for consistency), license paragraph,
  texture non-redistribution rationale spelled out; no `texture()` or
  `iChannel0/1` anywhere; both textures replaced with procedural value noise
  + ridged-fBm-like surface; `INPUTS: []`. naga translates cleanly; corpus
  bump 353→354 verified exact (only one stem added; 354 stems on-disk equal
  baseline; 512 passes = 511 + 1).
- **Unit/E2E** — coverage mirrors Helix Eye/Traced Tunnel (`isf-shadertoy-
  scenes.test.js` asserts URL + CC BY-NC-SA 3.0 attribution posts, parsing
  via naga asserts, image-not-referenced tests). E2E iterates the module in
  the same array.
- **THIRD_PARTY_NOTICES.md** — the Hot Shower row matches the in-file header
  exactly on author/source/license/texture rationale; in-file attribution
  travels. Procedure-wise this is correct per-port hygiene.

### Findings

- **[P1] notices completeness unchanged (carry-over from Round 21)** — the
  portfolio of NC-SA modules shipped without notice rows **is not being
  paid down**: Plane9/Joakim Dahl family (Boxy, SoundCube, Earth, Wormhole,
  MirrorCube, Sun, HollowCube, LightTunnel, SquareTunnel, ColorCube,
  ItIsComingForYou — all explicit in-file CC BY-NC-SA, none listed), the 4
  other Shadertoy-derived NC-SA shaders (`CollapsingArchitecture.fs`,
  `FractilianParabolicCircleInversion.fs`, `LogTransWarpSpiral.fs`,
  `UltimateFlame.fs`), the st_Ms2SD1 misattribution, the README MIT badge
  myth, the spherical-shader-tut.fs BY-CLAUDE error untouched, the Perlin
  Transition 4dS3Wd row minimised, the Helix Eye/Traced Tunnel NuSan rows.
  This commit adds exactly one row for one new module. Deliverable for
  review: **notices at HEAD still fail the Round 21 tests**, despite three
  more NC-SA ports having landed since Round 21.
- **[P2] `docs/rust-hybrid-progress.md:27,121,127,279` — stale "351" corpus**
  **counts survive the 353→354 bump** (lines say "…351 registered", "351/351
  fragments", "26 Point10 + 351 ISF"). The corpus is 354/354 textual.
  Update those strings.
- **[P2] Hot Shower reality check** — tc "ridged fbm" / "ridged-fBm" claimed
  in the commit message + header comments; the code is a plain 5-octave
  value-noise fBm with no `1-|n|` ridge. Cosmetic drift, but it strengthens
  nothing — the shader itself is otherwise a coherent port.
- **[P3] Notices row lacks the source title** — notices give Hot Shower's
  URL but not its Shadertoy title (compare Helix Eye's row, which names the
  title); codebase precedent, not fatal.


## Round 25 — `54b48fc9..72c49c1d` (input-link smoothing and pre-Rust feature levels)

### What landed, verified

- **`meyda/getSmoothingId` / `meyda/removeSmoothingId`** — engine action
  implemented at `store_ops.rs:359-368` (remove-id string payload accepted
  before the object-only params view, matching the JS action; `getSmoothingId`
  returns a real v4 `crate::ids::uuid_v4()`). The `link_smoothing` map was
  already in place; the commit adds arms + a test proving minting and
  acceptance only.
- **Input Link / u_bass/u_mid/u_treble read `bands.bass/mid/treble` again**
  (`index.js` — strict revert of the running-peak normaliser; published
  shape intact: `p10` twin with raw + AGC lanes; mirror unchanged).
- **Silence gate added JS-side** (`audio-bands.js`, `SILENCE_TIME_PEAK=0.01`
  + `SILENCE_FFT_MAGNITUDE=0.001`, both must trip) so the AGC can't amplify
  dither — regression test pins behaviour.
- **Point10 `useAgc` for scene mount reads booleans properly** (`Bool→1.0/
  0.0`, `scene_prop_number`) — the lever works in live scenes again.

### Findings

- **[P2] Gallery Point10 thumbnails still prefer AGC ON** — renderer path
  at `module_preview.rs:979` still uses old `Value::as_f64()`; the fix at
  the compositor doesn't reach it. Hover preview of a Point10 scene where
  `useAgc=false` shows AGC-boosted content while the live scene renders the
  raw signal (or vice versa) — two inconsistent pictures of the same scene.
- **[P2] Rust audio-bands has no silence gate** — engine-owned capture
  (`--audio-source pipewire|input`) feeding a quiet room amplifies the
  dither floor with its AGC → fake bass spectacle. The JS/renderer pump
  (default) is gated; the engine path silently isn't.
- **[P3] Smoothing id round-trip is silently incomplete at the UI**
  (`AudioFeatures.vue:131-133`) — `removeSmoothingId` is dispatched with
  `null` (the watcher already nulled `this.smoothingId` before passing it), so
  the engine arm silently drops it; the `id` entry is never removed from the
  registry. Same file (`:154-161`) — `restoreLinkValues` never reads `args[1]`;
  re-opening the panel re-mints a new id every time.
- **[P3] Doc staleness** (`scene_ops.rs:78`) — comment still claims the
  engine has no Meyda smoothing registry; the registry is implemented.
  Scene-created auto-links still do not mint a smoothing id.
- **[P3] band-features.js now unreferenced production code** — a 322-line
  module with a live suite; delete or `@deprecated`.
- **[P3] Coverage gaps** — no test pins the restored `features.bass =
  bands.bass` wiring; no e2e for smoothing ids; no test for the AGC gate's
  strong-FFT arm; `useAgc`/`properties` chain (boolean → lever) untested.






## Round 26 — `72c49c1d..69d1bba6` (ISF sidecars drawn on the legacy quad; top-left varying)

### Both defects fixed — VERIFIED

- **Vertex buffer is the legacy 6-vertex quad** — positions `[(-1,-1),
  (1,-1), (-1,1), (-1,1), (1,-1), (1,1)]` byte-identical to the WebGL
  `ISFGLProgram.bindVertices` (`out/renderer/.../index.worker-*.js`).
  Created once, shared per compositor, bound only for sidecar modules,
  `VERTEX_FRAGMENT` visibility, `Float32x2` layout.
- **`1 - (y+1)/2` is the top-left-origin varying** — matches `DEFAULT_VERTEX_WGSL`
  exactly; the bug class round 22 mistook for top-left-live-correct is closed.
- **Clamp test is genuinely triangle-proof** — right-edge pixel reads
  `>(1+ε)`-clamped 1.0, which the superset triangle could never produce.
- **Orientation probe is absolute** — top row = red assertion; any vertical
  flip fails the whole-image differ.
- **Edge Blur 10-frame chain test covers the one-frame-lag pyramid** —
  filter chains are correctly exercised.

### Still open (Round 26 findings, author-verified)

- **[P2] stale doc sites**: `mod.rs:59-60`, `source.rs:111`, `runtime.rs:44`,
  and `translate.rs:974-976` (the template that *regenerates* the stale
  doc) still say the runtime always draws the built-in triangle and
  sidecar varyings are unbound/zero-initialised. Fix the four together;
  the next `cargo test` re-bake the wrong claim.
- **[P2] `compiled >= 30` is not a 36/36 gate** — and rendering is
  effectively untested beyond 3 of 36 sidecars. Need `== 36` plus a render
  smoke test per corpus sidecar, or the claim "36 sidecars compile AND
  render" is unproveable.
- **[P3] mixed origins** — texture-free docs with a `.vs` sidecar get flipped
  `isf_FragCoord` (bottom-left) and top-left norm varying — two different
  orientations in one shader. Zero corpus instances (no texture-free
  sidecars here), but the boundary should be documented or closed.
- **[P3] GPU-less hosts silently skip the 4 new GPU tests** — try_engine
  early-return makes the suite green on software-only runners with zero
  coverage. Pre-existing pattern, now more consequential.
- **[P3]** `translate_with_sidecar_vertex` has no CPU-only test; the two
  parallel location-numbering scanners are unverified as a class.

 (ISF sidecars drawn on the legacy quad; top-left varying)

### Both defects fixed — VERIFIED

- **Vertex buffer is the legacy 6-vertex quad** — positions `[(-1,-1),
  (1,-1), (-1,1), (-1,1), (1,-1), (1,1)]` byte-identical to the WebGL
  `ISFGLProgram.bindVertices` (`out/renderer/.../index.worker-*.js`).
  Created once, shared per compositor, bound only for sidecar modules,
  `VERTEX_FRAGMENT` visibility, `Float32x2` layout.
- **`1 - (y+1)/2` is the top-left-origin varying** — matches `DEFAULT_VERTEX_WGSL`
  exactly; the bug class round 22 mistook for top-left-live-correct is closed.
- **Clamp test is genuinely triangle-proof** — right-edge pixel reads
  `>(1+ε)`-clamped 1.0, which the superset triangle could never produce.
- **Orientation probe is absolute** — top row = red assertion; any vertical
  flip fails the whole-image differ.
- **Edge Blur 10-frame chain test covers the one-frame-lag pyramid** —
  filter chains are correctly exercised.

### Still open (Round 26 findings, author-verified)

- **[P2] stale doc sites**: `mod.rs:59-60`, `source.rs:111`, `runtime.rs:44`,
  and `translate.rs:974-976` (the template that *regenerates* the stale
  doc) still say the runtime always draws the built-in triangle and
  sidecar varyings are unbound/zero-initialised. Fix the four together;
  the next `cargo test` re-bake the wrong claim.
- **[P2] `compiled >= 30` is not a 36/36 gate** — and rendering is
  effectively untested beyond 3 of 36 sidecars. Need `== 36` plus a render
  smoke test per corpus sidecar, or the claim "36 sidecars compile AND
  render" is unproveable.
- **[P3] mixed origins** — texture-free docs with a `.vs` sidecar get flipped
  `isf_FragCoord` (bottom-left) and top-left norm varying — two different
  orientations in one shader. Zero corpus instances (no texture-free
  sidecars here), but the boundary should be documented or closed.
- **[P3] GPU-less hosts silently skip the 4 new GPU tests** — try_engine
  early-return makes the suite green on software-only runners with zero
  coverage. Pre-existing pattern, now more consequential.
- **[P3]** `translate_with_sidecar_vertex` has no CPU-only test; the two
  parallel location-numbering scanners are unverified as a class.

## Round 27 — `69d1bba6..f467242f` (review-driven fixes land + docs note)

Four commits, all surface-level checkpointed by the author after my verification:

- **`66eb4860`** — Preview scene-prop getter reuses `scene_prop_number` (the
  same function the live compositor calls, so preview matches live); Point10
  preview path now drives `useAgc=false` correctly. Gallery tiles and Scene
  hover cells agree with the live frame, both honouring the AGC lever. Audio
  Features smoothing-id leak fix: capture `this.smoothingId` **before**
  clearing; reopen restores `args[1]`. Round-25 [P2] and three [P3]s close.
- **`d9e74945`** — `docs/isf-translation-notes.md` gains a "Vertex skeleton
  injects" section; the notes now describe the shipped contract for the
  sidecar path (the author updates the rationale).
- **`8e41ef2c`** — Rust audio-bands gains the near-silence gate with the same
  constants/placement as the JS pump; dither must not produce a reading
  (-30 dB tone stays, loud tone with a quiet FFT stays); `cpal`-input
  path (macOS/Windows) gets the same gate. Round-25 finding closes.
- **`f467242f`** — progress doc; claims consistent with the current tree:
  `DEFAULTS_VERSION = 13` in the engine, `default-scenes.json` content matches
  JS v13 (Halftone Harbor Effects row empty, MBOX `dhue = 0`, no tween);
  Point10 previews honour booleans; audio slider restores its smoothing id.

### Findings (nothing new blocks release here)

- **All Round-23/25/26 findings closed.** The two review rounds tracked
  the same defects at multiple sites: premultiplied bytes to putImageData
  (R22), stale post-blur repaint (R22), monitor→monitor capture-stop (R19),
  `SET_SYSTEM_AUDIO_STATE` unguarded call site (R22), `backgroundThrottling`
  in legacy (R22), `F`/Escape auto-repeat & editable-content guards (R22),
  sidecar `isf_FragNormCoord` bottom-origin (R23→R26), sidecar superset
  triangle (R23→R26), smoothing-id null-dispatch (R25), gallery Point10 AGC
  stuck ON (R25), Rust silence-gate absent (R25).
- **Not closed yet:** `mod.rs:59-60` and `translate.rs:974-976` still document
  the runtime as drawing the built-in triangle only ("sidecars are
  report-only"). The author updated the surrounding corpus report but left
  those two sites — the textual claim regenerator can re-bake the stale line
  next `isf-corpus-report.md` write. Round-26 prompt said "fix the four
  together"; only the translation notes were explicitly rewritten.
- **Smoothing-id is still missing an end-to-end test**, and smoothing is not
  reloaded end-to-end by e2e; `restoreLinkValues` is unit-verifiable but no
  callsite exercises the path (the panel never connects to production data
  without manual intervention).

### What it fixes — VERIFIED

- Single-pass, no-pass-target, no-`IMPORTED` texture-free shaders now render
  upright: `gl_FragCoord` → `isf_gl_frag_coord()` does a pointwise Y-flip in the
  fragment body (`source.rs:577-613`); `isf_FragCoord` re-routes through the
  helper; the vertex path is unchanged (raster rows identical, rasterizer
  position builtin kept); `RENDERSIZE` is the right flip source; identifier-
  aware `replace_ident` rewrites all body occurrences (incl. helper functions
  and hoisted global initialisers); can't disagree with the vertex selection
  because both predicates reduce to the same `compile`-time check.
- The fix-set = 26 corpus generators (Camera-style ray builders: The Pool
  Rooms, st_Ms2SD1, UltimateSpiral, Warp Conduit, GreatBallOfFire, …).
  Verified not to leak the flip across passes — none exist in this class.
  Naga correctly rejects the one-format hazard shaders loudly.
  **26 fixed = 2 named + 24** ("29 others" in the commit message / progress
  doc is wrong arithmetic: claimed 31, actual 26; 31 counts texture-free docs
  with a `gl_FragCoord` substring, and five of those are buffer/feedback docs
  this commit does not touch.)

### Still broken (same defect class, unchanged)

- **The 9 multi-pass generators stay vertically mirrored vs WebGL** —
  `Life.fs`, `Circle Trails.fs`, `pixel-tunnel.fs`, `Etch-a-Sketch.fs`,
  `fractal-mage-tunnel.fs`, `Random Shape Blast.fs`, `Color History.fs`,
  `lopy-frac-tunnel.fs`, `fractal-mountain-tunnel.fs` — all keep
  `slots.is_empty() == false` ⇒ top-left vertex and unflipped coords,
  byte-identical to before; none gets the helper. Fix is a consistent
  flip-both variant for the buffer-only class (write and sample are the
  same convention within a doc, so the vertex varying AND `gl_FragCoord`
  must flip together; vertex change alone or fragcoord change alone both
  produce double-flip feedback).
- Textured `gl_FragCoord` shaders (47, mostly `IMG_PIXEL(in, gl_FragCoord.xy)`)
  stay top-left by design — fine, self-consistent.

### Stale/overstated claims (docs)

- `isf/mod.rs:44-50`, `source.rs:131-143`, `translate.rs:913-917` still state
  isf_FragNormCoord / gl_FragCoord are top-left unconditionally — false for
  the 26 newly-flipped; the notes "… so they match WebGL" over-generalises to
  the buffer/feedback class too.
- Corpus notes will bake the stale convention into the next
  `isf-corpus-report.md` regeneration.

### New findings (this round)

- `[P2]` The flip helper is added only when bindings are empty and
  `slots.is_empty()` holds; nothing validates wgpu ↔ helper at the
  higher-level selection point, so a future change to the vertex predicate
  (e.g. treating Buffer slots as external) silently reverts the flip.
- `[P3]` Shader-local `#define gl_FragCoord …` is renamed by
  `replace_ident` inside its own definition (source.rs:889-891); today zero
  corpus occurrences, but the fail-mode is naga parse error, visible loudly —
  still preferable than silent wrongness.
- `[P2→P3]` Commit message / progress docs claim 29 other raymarchers and
  "texture-free generators upright"; reality = 26 single-pass; the notes
  cannot all be true until the buffer/feedback class is fixed.

## Round 28 — `f467242f..c06bfb1f` (`scenes/saveCurrentAsScene` lands)

**The reported defect was real and is now fixed, end to end.** In engine mode
the Scene-grid save button dispatched `scenes/saveCurrentAsScene`, which fell
through to the dispatch wildcard; the UI got `null` and the `save-file` IPC
never fired — the file was never written and nothing warned beyond the
once-per-session catch-all. Legacy mode was unaffected (`scenes.js:1928-1954`).
`c06bfb1f` adds the dispatch arm (`store_ops.rs:447`) and the action
(`scene_ops.rs:1237-1266`): name falls back to `Scene <n>`, in-flight row and
whole-Scene transitions are completed first so the snapshot is the settled
stack, the six-key preset document is built from live state, Scene metadata
(`id`/`enabled`/`minSeconds`/`maxSeconds`/`timingOverride`) is attached, the
document is appended to the Scene list, `scenes/ADD_SCENE` is emitted for the
mirror and the scene list is persisted (atomic tmp+rename). `Scenes.vue`
writes the returned document to `scene/<name>.json` via the existing
`save-file` machinery (`media-manager/save-handlers/scene.js`), and the
renderer's engine bridge replays the delta through the generic
`store.commit(d.type, d.payload)` path (`index.js:166-178`) — the `ADD_SCENE`
mutation exists and matches the payload shape (`scenes.js:4115`).

**Parity notes (not blockers)**

- Transition completion order is reversed vs. JS (rows first, then the
  whole-Scene fade — JS does the reverse), but the two completions touch
  disjoint id sets (row ghosts vs. whole-Scene from-list), both bakes are
  idempotent and neither produces removal of the other's groups — converged
  outcome. Non-issue.
- Rust persists the Scene list on save; JS deliberately does not persist
  scenes.json on this path (legacy relied on later persists). Rust is
  strictly more robust; not a parity break.
- Name handling is stricter than JS (trims; empty/whitespace and non-string
  inputs fall back to `Scene <n>` instead of JS taking any truthy value).
- `generate_scene_id()` yields `scene-<nanos-hex>-<seq>`, not uuidv4 —
  documented intentional deviation (avoids `getrandom`).
- `complete_whole_scene_transition` has no `pendingCleanup`-equivalent; the
  Rust engine removes `from` groups directly, so there is nothing extra to
  flush on save.

### Findings

- **[P3] The unit test never pins the `scenes/ADD_SCENE` delta.** The
  sibling test in the same file asserts `expressions/ADD_EXPRESSION` is in
  `delta_types`; `save_current_as_scene_snapshots_the_live_state` asserts the
  return value and the list growth but never checks that the mirror delta
  was emitted. Dropping the `deltas.push("scenes/ADD_SCENE", …)` line keeps
  the test green, and the Scene column silently fails to appear in engine
  mode. One `assert!(types.contains(&"scenes/ADD_SCENE".to_string()))` closes it.
- **[P3] No end-to-end coverage of the engine save flow.** The reported
  defect existed because the action silently returned `null` through the
  dispatch wildcard; a single e2e that dispatches `scenes/saveCurrentAsScene`
  on the engine bridge and asserts `scene/<name>.json` landed would have
  caught it. The unit test covers the engine internals, not the wildcard
  reachability.
- **[P3] Weak assertions on timing values.** `minSeconds`/`maxSeconds` are
  asserted only as `is_number()`; the test would not catch a timing regression
  (the JS clamps to `MIN_SCENE_SECONDS` and rounds to a tenth — the Rust
  port does the same, untested here).

## Round 29 — `c06bfb1f..7ac215bd` (Scene-grid column actions; progress doc)

Process note: the watcher only saw `7ac215bd` as newest; it had already
stacked `aa2814a6` underneath it. Both are reviewed here together.

### `aa2814a6` — Scene-grid column/list actions — VERIFIED (parity by reading)

Six actions the Scenes grid dispatches now exist in the engine:
`addEmptyScene`, `moveScene`, `setSceneEnabled`, `updateSceneName`,
`updateSceneTiming`, `resetSceneTiming` (registered at `store_ops.rs:448-453`,
implemented at `scene_ops.rs:1270-1462`). All previously fell through the
dispatch wildcard — renaming, moving, enabling, re-timing and adding a Scene
silently did nothing in engine mode, the same "user-facing button silently
dead" class as `saveCurrentAsScene` (Round 28).

Verified against the JS line citations in the commit:

- **`moveSceneToIndex`** (`modv-engine/src/scene_columns.rs:58-86`) matches
  `scene-columns.js:36-58` on every branch — non-integer `to_index`,
  out-of-range, unknown id and `to == from` all yield `None` → `false`
  (the JS returns the same array identity → skipped persist). One
  `REPLACE_SCENES` on a change, none on a no-op.
- **`create_empty_scene`** (`scene_columns.rs:118-156`) — trims, unique-name
  fallback (`next_scene_name`), empty `groups: []`/`modules: {}`, timing from
  the normalised performance timing, metadata parity.
- **`updateSceneTiming`** — current resolved through `normalize_scene_timing`
  (existing override wins, else global), `timingOverride: true` coerced,
  `json_number` integral serialisation. Parity with `scenes.js:2188-2209`,
  including numeric-string coercion via `to_finite_number` (`scene_timing.rs:68-76`).
- **`updateSceneName`** — trim, empty rejection, case-insensitive duplicate
  rejection (ASCII-case-insensitive, not Unicode — an edge deviation from JS
  `toLowerCase` folding, noted and fine).
- **shared `update_scene_document`** — mutate → emit `scenes/ADD_SCENE`
  (mirror upserts by id; `REPLACE_SCENES` for move) → atomic persist.
  Mirror-safe.
- **unit test** (`scene_list_actions_rename_move_enable_and_retime`) covers
  all six actions plus duplicate-rejection and the no-op move.

### `7ac215bd` — progress doc claims — BOTH VERIFIED ACCURATE

- "The column actions … are implemented too" — yes: `store_ops.rs:448-453` +
  implementations; tests pass per the commit.
- "Still open: cell actions (`captureSceneRowToScene`, `clearSceneCell`,
  `copySceneCellToScene`, `createSceneCellGroup`) and
  `groups/disableGalleryItemModules` / `dataTypes/createType`, which still hit
  the wildcard" — confirmed by absence: none are registered in `store_ops.rs`,
  none exist in `crates/`. All are real JS actions (`scenes.js`,
  `scene-cell.js`, `groups.js:338`, `dataTypes.js:139`) reachable from
  `Scenes.vue` and the Gallery in engine mode today.

### Findings

- **[P2] The remaining wildcard-hit actions are user-facing and silently dead
  in engine mode.** `captureSceneRowToScene`, `clearSceneCell`,
  `copySceneCellToScene`, `createSceneCellGroup` — every one is dispatched by
  `Scenes.vue` (the Ctrl+drag cell copy/capture flow from AGENTS.md), and they
  return `null` through the wildcard exactly as `saveCurrentAsScene` did.
  `groups/disableGalleryItemModules` (Gallery module gating) and
  `dataTypes/createType` are the same class. The author now publicly says so;
  the same fix pattern as Rounds 28/29 closes them. Highest-priority remaining
  engine-mode UI gap after the save button.
- **[P3] Process: two feature commits in one watcher window.** The review
  anchor (`git log --oneline -1`) only names the tip; intermediate commits
  can ship unexercised. The watcher script now also logs intermediate commits
  since the last reviewed SHA.
- **[P3] Doc-consistency nit**: the progress claim says the grid "can save and
  manage Scenes" — true only for column actions; cell actions remain dead.
  The doc does say so in the same paragraph; wording is fine, call-out only.

## Round 30 — `7ac215bd..8da469e0` (NC-SA scene ports gain linkable inputs)

**Clean, no blockers.** The three built-in NC-SA ports (Helix Eye, Traced
Tunnel, Hot Shower) each declare ISF float inputs with LABEL/MIN/MAX/DEFAULT
so links, MIDI, tweens, expressions and the Inspector can drive them;
raymarch step/iteration loop bounds stay compile-time constants as they must.

Verified by reading (tests not run locally — author's toolchain):

- **Corpus is shared, not copied.** The Rust engine scans
  `src/renderer/src/application/renderers/isf/modules/isf` live
  (`DEFAULT_CORPUS_DIR`), so the INPUTS change lands in the JS worker and the
  Rust compositor from the same source. No corpus divergence to track.
- **All 17 defaults are byte-verified against the deleted consts** — Helix
  Eye (`speed 1.0`, `coil 60`, `radius 20`, `cameraTilt -0.4`,
  `eyePulse 0.1`), Traced Tunnel (`speed 1.0`, `gridScale 12`, `dof 0.05`,
  `swing 1.0`, `fogDensity 1.0`), Hot Shower (`sphsize .7`, `perturb .3`,
  `displacement .015`, `windspeed .4`, `glow 3.5`, `brightness .43`,
  `fractparam .7`). At the defaults every transform reduces to the original
  bit-exactly (IEEE `x * 1.0 == x`).
- **No shadowing duplicates** of the new uniform names with local `const`s
  anywhere (grep per identifier across all three files — CLEAN).
- Every input identifier is referenced by the shader body.
- **Unit test** (`tests/unit/isf-shadertoy-scenes.test.js`) asserts float
  typing, MIN<MAX, DEFAULT∈[MIN,MAX], LABEL presence and actual shader
  reference; the stale `INPUTS: []` assertions in the same file were removed.
  No other suite snapshots `INPUTS: []` for these files; the Rust corpus
  baseline lists only module names (no inputs assertions) and no Windows/
  parser asserts hit the new non-empty inputs.
- **No content migration needed** — module inputs are re-created from INPUTS
  at registration; saved Scenes/presets never store INPUTS, so upgrades just
  gain the seven/five inputs with their defaults.
- NC-SA attribution untouched — the three notices rows (Rounds 21/24) still
  match the files' contents and licenses.

### Findings

- **[P3] No render-level assertion that the inputs actually flow to the
  uniforms** (e.g. `speed = 0` freezing the flow, or a table of expected
  outputs). The new test verifies presence/ranging/reference — the same
  strength as the existing corpus metadata tests — not that the ISF runtime
  binds them at runtime. A single "inputs bound" check per shader (or one
  shared helper) would pin the whole class.
- **[P3] Side-effect semantics worth one doc sentence**: `speed = 0` freezes
  the flow entirely and `swing = 0` disables the camera swing (previously
  implicit at ±0.4/±2.); both are intended consequences of exposing the knobs,
  and are the desired UX. Note it so the readme of the (untracked) parity doc
  can quote it accurately.

## Round 31 — `8da469e0..236a3bf9` (grid cell/Gallery actions + `copySceneCellToScene`)

The author's `7ac215bd` straggler list is now fully implemented in the
engine: `1236c374` lands `clearSceneCell`, `captureSceneRowToScene`,
`createSceneCellGroup` (scenes.js `cell actions`), `groups/disableGalleryItemModules`,
`groups/focusGalleryItem` (new, not previously named) and `groups/orderByIds`
— all were silently dead through the wildcard, same user-visible class as the
save button. `236a3bf9` lands `scenes/copySceneCellToScene` (Ctrl+drag cell
copy; ~760-line `scene_cells.rs` port, five unit tests) and registers it.
The only surviving wildcard from the audit list is `dataTypes/createType`,
which in engine mode is effectively unreachable (tweens are built natively by
the engine — `apply_default_scene_input_links`), so it is a catch-all hole,
not a live UI path.

**Verified by reading (tests not run locally — author's toolchain):**

- `groups/orderByIds` drops Groups absent from `ids` — exactly the JS
  `orderByIds` behaviour; mirror `REPLACE_GROUPS` payload is well-formed
  (`replace_group_modules` machinery confirmed at `store_ops.rs:883`).
  The legacy `groups.js` `orderByIds` works only because `state` is a
  module-scope `{groups: []}` whose shared reference *is* the Vuex-registered
  state mutated in place (`REPLACE_GROUPS` splices, never reassigns) — the
  Rust port is correct by ownership. Leaving the legacy alias gymnastics
  alone is the right call.
- Performance-mode refusal matches the JS: `captureSceneRowToScene` and
  `createSceneCellGroup` bail out under performance mode;
  `clearSceneCell` is unguarded in both (parity as designed).
- `copySceneCellToScene` — faithful against `scenes.js:3568-3735`:
  positional write with holes padded/trimmed, `TweenContainer` semantics,
  source-inputs/links/assignments/tweens carried by id membership, dangling
  tween links dropped, `focusedInput` fall-through, `null` cases, and a
  named-source-positional-write rule. Tests pin all of it.
- The two commits' own tests exercise the happy path for capture/clear and
  the live replacement. No test asserts the live Group's module-list state
  after clear — the exact gap below.

### Findings

- **[P2] `clearSceneCell` leaves phantom module ids in the live Group**
  (`crates/modv-app/src/scene_ops.rs`, `clear_scene_cell`).
  JS issues `groups/REPLACE_GROUP_MODULES { groupId, modules: [] }` immediately
  after removing the modules (`scenes.js`, clearSceneCell tail — "store.commit(
REPLACE_GROUP_MODULES … modules: [])"); the Rust only dispatches
  `modules/removeActiveModule` per id, which deletes the module instances but
  never rewrites the owning Group's `modules`. The compositor is safe (it
  skips id-less modules, "a group can outlive its module"), but
  `scene_ops.rs:1370` re-serialises the row on the NEXT `captureSceneRowToScene`,
  writing a cell whose Group references module ids with no matching
  snapshots in `modules` — a **corrupt Scene document** that applies as an
  empty row. Fix: dispatch `groups/REPLACE_GROUP_MODULES { modules: [] }`
  right after the removal loop (mirrors JS exactly; machinery exists).
  Regression test: apply → clear → assert live `group.modules == []`,
  then re-capture and assert the saved cell is faithful.
- **[P2] `captureSceneRowToScene` can capture another Scene's live row**
  (`crates/modv-app/src/scene_ops.rs:1370`).
  JS requires an active-row match (or a stamped ghost-excluded match for the
  fallback) and returns `null` otherwise; Rust's
  `.or_else(|| self.find_live_row_group(row))` matches ANY live row Group,
  ignoring scene stamp — so with a null hole or a stale stamp it silently
  writes the *other Scene's* row into this Scene's cell. Fix: drop the
  fallback (return `null` when no stamped ghost-free match), or resolve the
  JS-style order: active-row fast path (scene match) → stamped scan → `null`.
  Regression test: apply A; capture B for a row A owns → assert `null` and B
  unchanged.
- **[P3] `dataTypes/createType` remains wildcarded** — effectively
  unreachable in engine mode (tween construction happens natively); either
  register a no-op arm or close the class item, but keep it honest in docs.
- **[P3] Progress doc lags the code** — the two feature commits (`1236c374`,
  `236a3bf9`) landed ~5 minutes after the last docs(progress) commit; its
  "still open" text is already stale (all cell actions except
  `dataTypes/createType` are in). Needs its promised follow-up edit.
- **[P3] Author-claimed counts unverified** — "app 320, engine 448 pass"
  recorded verbatim; tests not run here (no local toolchain).

## Round 32 — `236a3bf9..457c846a` (duplicateModule, BPM source, local fonts)

Two more named stragglers from the audit list land, plus a faithful
`groups/duplicateModule`:

- **`groups/duplicateModule`** (`groups.js:403-458`) — makeActiveModule now
  accepts `existingModule` (meta/props merge from the source, fresh ids),
  and duplicateModule re-inserts the copy immediately after the source,
  remapping each prop/meta input to the new module and recreating its input
  links and expression assignments by key. Verified by reading: the JS
  index-zip via `modules/activeModuleInputIds` is order-dependent; the Rust
  key-based remapping is order-independent (same coverage on the engine's
  creation set: prop + meta inputs). Vec/colour **sub-inputs** — the one
  thing the JS copies that the Rust does not — are never created by the
  engine anyway (outstanding P2 #12), so there is nothing extra to lose on
  this creation path; if P2 #12 is fixed, the duplicate port needs the same
  extension (noted here, not double counted). The duplicate's position
  insertion, meta input order and link re-pointing match the JS; the
  progress doc was honestly updated this time, with the remaining wildcard
  list (verbatim: `groups/SWAP`/`modules/SWAP`, `dataTypes/createType`,
  `media/*`, `midi/learn`, `ndi/*`, `osc/createServer`, `scenePreview/*`,
  `windows/createWindow`, `outputs/*` aux).
- **`beats/SET_BPM_SOURCE`** (`beats.js:35-44`) — Store tracks the selected
  source; `setBpm` only applies from it (default remains `beatdetektor`);
  unknown sources rejected with warn-once like the JS. The mirror mutation
  already existed, so the echo drives the tap/MIDI picker.
- **`fonts/SET_LOCAL_FONTS`** — committed by the renderer as a **bare array**
  (`application/index.js` after `queryLocalFonts`), so the engine handles it
  before the object-params view: stores it, echoes the same array for the
  mirror. Matches the JS mirror mutation exactly.

### Findings

- **[P3] `duplicateModule` of a module absent from the given group** —
  Rust returns `null`; the JS inserts at position 0 (`findIndex - 1 + 1 = 0`).
  UI can't reach this (duplicate buttons dispatch in-group context); noted
  as a divergence, not a defect.
- **[P3] Missing-source `setBpm` divergence** — JS throws; Rust silently
  ignores. The engine behaviour is the better one for a renderer that cannot
  see the throw (and cannot drive the catch); noted.
- **[P3] `local_fonts` is write-only inside the engine** — the echo is the
  whole point (fonts.js mutation replays to the mirror) and there is no
  engine consumer, which is fine; recorded so the next reviewer doesn't
  chase a phantom Text-module consumer.
- **[P3] Remaining-wildcard list is author-asserted** —
  `midi/learn`/`media/*`/`osc/createServer`/`scenePreview/*`/
  `windows/createWindow`/`outputs/*` are unverified by me in this round
  (recorded verbatim from the progress doc). A full audit sweep of
  every JS-dispatched action × engine dispatch table × JS call-site remains
  a worthwhile P2 housekeeping task; attach to the outstanding wildcard item.
- No new P2s in this commit. The `"value"` expression no-op matches the JS
  exactly.

## Round 33 — `457c846a..5ebd4a24` (Point10 Old Movie retires to v14)

**Verdict: faithful, symmetric retirement — no engine/JS defects. One
self-contradicting headline in the progress doc.**

Verified by reading (tests not run locally — author's toolchain):

- **JS↔Rust versions match**: `DEFAULTS_VERSION = 14` in both
  (`default-scenes.js` and `default_scenes.rs`), with the v14 assertions on
  both sides, and the `v13 → v14` refresh is pinned on both sides — the Rust
  test now additionally asserts that installs two versions behind are NOT
  refreshed (strictly stronger than before).
- **`Boxy Arcade` is in `REFRESHED_DEFAULT_SCENES`** (`default_scenes.rs:109`)
  so installs on v13 get the edit in place; no rename/removal rule ships.
- **The Sepia Tone swap is byte-identical in both default-Scene sources**
  (JS and Rust JSON — both now read `name: "Sepia Tone", props: {}` in the
  Effects row), and `Sepia Tone.fs` exists in the shared corpus
  (`inputImage` + `contrast`, `IMG_THIS_PIXEL` — the same composite class the
  retired Old Movie emitted; the scratch is gone, intentionally).
- **Retirement is total and consistent**: the effect is gone from both
  catalogues (JS block + Rust WGSL port) with counts moved 37→36 shader
  modules / 31→30 Point10 effects across the registries, `MODULE_COUNT`,
  the registry tests and the docs; the removal guard forbids the name in
  source and tests (assembled-at-runtime in `default-scenes.test.js`,
  consistent with the existing SplitBars/Ferrofluid pattern); both default
  default-Scenes paths pass the new tests on the author's runner (verbatim
  "Vitest 1193, engine 449, app 323, renderers 771 pass").
- My own review doc carried a stale registry claim ("Old Movie is registered");
  corrected inline in Round 33 to note only `Point10 Crt` remains.

### Findings

- **[P3] Progress doc headline is now self-contradicting** —
  `docs/rust-hybrid-progress.md:26` reads "all **429** registered modules
  (36 shader + 15 2D + 26 Point10 + 354 ISF)"; the new arithmetic sums to
  **431** (the 429 figure was correct at 37-shader/351-ISF; the corpus grew
  351→354 without the headline being fixed). The same doc's module-parity
  pointer (line 255) repeats 429.
- **[P3] `store_ops.rs:4975` still asserts `registered.len() >= 37 + 15 + 18`**
  — the author updated the sibling registry test to `36 + 15 + 26` but not
  this one; the outstanding docs-arithmetic item now has three concrete
  sites (this constant, the 429 headline, and `rust-hybrid.md`'s "387
  shaders" — 354+36 = 390).
- **[P3] The retired behaviour is a visible look change** — Sepia Tone is a
  straight colour grade (no scratch); users who relied on Old Movie's
  scratch effect should be told in the changelog. (Author's call; the NC-SA
  and Boxy Arcade attribution are unaffected.)

## Round 34 — `5ebd4a24..56f56364` (three sparse Shadertoy NC-SA Scenes; v15)

**Verdict: clean. The three sparse Scenes ship faithful, semantically
identical definitions in JS and Rust; no defects. Two doc-hygiene P3s.**

Verified by reading + programmatic walks of both sources:

- **`DEFAULTS_VERSION = 15` in JS and Rust**, both with v15 assertions; the
  three scenes exist only in the v15 lane (`introducedVersion: 15`), so v14
  installs receive them via the normal seed path — correct.
- **The three scenes are semantically identical JS↔Rust** — walked the full
  default-scenes data trees on both sides: only float/int print noise
  (`4.0` vs `4`, `1.0` vs `1`); every other byte matches.
- **Every explicit prop is in range of the module's INPUTS**: Hot Shower
  (`glow 4` ∈ [0,8], `brightness 0.45` ∈ [0,1.5]), Traced Tunnel (`gridScale 13`
  ∈ [4,24], `fogDensity 1.15` ∈ [0.2,3]), Helix Eye (`eyePulse 0.14` ∈ [0,0.3]),
  Bloom (`blurAmount 5` ∈ [0,12]; the `intensity` link pins `[0.2, 1.0]` inside
  [0,5]). Glow takes its defaults; Chromatic Abberation keeps its (misspelled
  but deliberately kept) registration name `"Chromatic Abberation"` — matches.
- **Every linked control resolves**: `windspeed`, `speed`, `dof`, `eyePulse`,
  `intensity` all exist on their modules (`windspeed` is the Round-30 input),
  the `inputLinks` array matches the `applyExplicitSceneInputLinks` consumer
  format, and three effects chained inline inside a clearing, non-inheriting
  Effects row is within the row-name/row-position grammar.
- **E2E version pins moved 13 → 15** (they were stale at v14 already — now
  fixed; see findings). Previous-version refresh tests now use the
  `DEFAULTS_VERSION - 1` constant, which is less brittle.
  `at least 54` holds at 57.
- **Attribution untouched** — Hot Shower / Traced Tunnel / Helix Eye keep
  their headers; the scenes add no new third-party code.

### Findings

- **[P3] The progress doc was not updated for this commit** — it still
  carries the 429/431 headline self-contradiction (Round 33) and adds nothing
  about the v15 Scenes; the v15 delta will re-freeze it or the author should
  amend now.
- **[P3] The two stale e2e pins (13) were stale at v14 too** — they were
  asserting a wrong version while HEAD was v14, which is a silently-disabled
  test behaviour, not a passing one. Pinning via the JS unit test's constant
  pattern (`import { DEFAULTS_VERSION }`) instead of literals would make
  stale pins impossible; suggest for the next author pass.
- No P2. The scenes are product content; the wiring is correct end to end.

## Round 35 — `56f56364..1240db0c` (v15 Shadertoy scenes toned down)

**The calming pass is correct on the JS side and on the data side; the Rust
engine silently ignores the smoothing they lean on.**

Verified by reading:

- JS↔Rust scene bodies remain byte-identical modulo float/int noise, in
  both the generator defaults (`glow 3.5`, `brightness 0.43`, `gridScale 12`,
  `fogDensity 1.1`, `eyePulse 0.12`) and the narrowed link ranges
  (`windspeed 0.25–0.55`, `speed 0.8–1.15`, `eyePulse 0.05–0.15`); each scene
  now pins exactly one bass-linked control with `smoothing: 0.6` declared in
  **both** sources.
- `Bloom` `blurAmount` 6/5/4 and `intensity` 0.3/0.35/0.3 consistent both
  sides; `Chromatic Abberation` dropped from Prism Tunnel and `Glow` from
  Sentinel in both; `eyePulse` back at 0.12 (JS float prints as `0.12`,
  Rust writes `0.12`).
- `DEFAULTS_VERSION` stays 15 — correct, v15 is unreleased; editing the v15
  set before release is the right move (no v16 bump noise).
- New unit tests pin the tighter ranges and assert `smoothing >= 0.5` on
  the JSON links (data layer) — correct as far as they go.
- No third-party attribution changes; generator headers untouched.

### Findings

- **[P2] The engine drops `smoothing` at scene-build time — the v15 scenes
  are twitchy in engine mode exactly as they were.** The JS path parses
  `link.smoothing` and materialises `[feature, smoothingId, smoothingValue]`
  via `meyda/getSmoothingId` + `createBassLink({..., smoothing})`
  (`scenes.js:753-789, 964`). The Rust parses it too — `SceneInputLink.smoothing`
  (`default_scenes.rs:247-252`) — and the parser is unit-tested via
  `get_feature_smoothing` (`scene_link_defaults.rs:421-440`) but has **zero
  production call sites in the scene chain**: `apply_default_scene_input_links`
  reads `link.feature/source/min/max/bpm_division` and **never forwards
  `link.smoothing`** to `create_bass_link`, which emits `args: [feature]` —
  no smoothing id, no amount (`scene_ops.rs` ~3075-3135, ~3358-3375). The
  JS scenes are calm; the engine's built copies feed raw bass. The unit
  suite cannot see this (it asserts the JSON declares smoothing, not the
  emitted link args), and the commit message's
  "cannot silently creep back to twitchy" is currently true in the data
  layer and false in the engine layer. Fix: allocate a smoothing id in the
  engine (the `meyda/getSmoothingId`/`link_smoothing` machinery exists) and
  emit `args: [feature, id, amount]` for explicit bass links that declare
  `smoothing`; add a unit test asserting the args triple.
- **[P3] `get_feature_smoothing` is dead in production** — tested, ported,
  but unreferenced by the scene links chain (subsumed by the finding above).
- **[P3] Progress doc not updated again** — v15 lands edits without touching
  `docs/rust-hybrid-progress.md`; the 429/431 headline self-contradiction
  (Round 33) persists.

## Round 36 — `1240db0c..70540391` (MIDI and OSC transport wired)

**Engine gains the MIDI/OSC transport — clean wiring, minor notes only.**
Read end-to-end: `setup-midi.js` is unchanged and already commits
`midi/WRITE_DATA {id, type, channel, data}` from Web MIDI messages; the new
engine handlers store the JS device record/channelData shapes verbatim (same
key scheme `id-name-manufacturer`, same flags, same types) and echo the same
deltas, so the JS mirror mutations replay unchanged; Input Links resolve
`midi.data.<path>` engine-side as before. The BPM clock path now works for the
MIDI source because `beats/SET_BPM_SOURCE` landed last round — the renderer
dispatches `beats/setBpm { source: "midi" }` and it applies only when the
picker has MIDI selected (as designed). MIDI controls are no longer disabled
in engine mode; the boot unsupported message now names only output windows and
plugins.

OSC: `osc/createServer` binds a real UDP listener (`OscServer::start_messages`
in `modv-media`), datagrams are decoded with `rosc`, coalesced to one callback
per message with the full argument list (`dispatch_packet_messages`), and
drained into `EngineState.osc.data` once per frame before Input Links are
evaluated (`window.rs` frame step). JS `lodash.set` semantics replicated
(`apply_osc_message`, bounded); Input Links read `osc.data.<address>[n]`
(`input_links.rs`); the mirror receives `osc/WRITE_DATA` and the listener's
`osc/ADD_SERVER`/`osc/DELETE_SERVER`. The v15/INPUTS parser needs no change —
these were link-evaluator paths.

**Also fixed in passing:** the JS osc store module's `DELETE_RECIEVER`
mutation typo → `DELETE_SERVER`, which had made `osc/removeServer` a silent
no-op in legacy mode too;
`OSC.vue` gained a Listen/Stop button (the same UI drives the legacy worker);
the unit tests cover the mirror shapes, OSC datagram application and a UDP
bound bind is deliberately avoided in tests.

### Findings

- **[P3] No live UDP round-trip test** — socket-level behaviour is exercised
  only by the coalescing function and a synthetic datagram; a real
  bind+send+drain loop would catch framing/port regressions with one test
  (two lines of test, loopback only).
- **[P3] OSC reader thread death is silent** — socket errors (`Err => break`)
  end the reader thread with no error callback and no mirror event; the
  "Listening" UI stays green over a dead socket. Same pattern class as the
  pw-record post-spawn death (outstanding item 6) — consider folding into one
  "post-spawn resource death is invisible" fix.
- **[P3] Empty-argument OSC messages are dropped silently** —
  `dispatch_packet_messages` filters out empty argument lists (`msg.slice(1)`
  empty). A pure trigger address can never reach Input Links in engine mode;
  arguably harmless (links want args), but undocumented divergence from JS
  (which would store `[]`).
- **[P3] `midi/UPDATE_DEVICE` is forgiving where JS crashes** — JS:
  `state.devices[id][key] = value` throws on a missing device; Rust:
  creates the record then writes. Softer and friendlier for a renderer
  that cannot see the throw; noted as a deliberate improvement.
- **[P3] Progress doc not updated again** — the MIDI/OSC feature lands
  without touching `docs/rust-hybrid-progress.md`; the 429/431 headline
  self-contradiction persists from Round 33.
- No P2. The transport works end to end and the mirror replays byte-consistent
  payloads.

## Round 37 — `70540391..145005ec` (engine-owned xdg-desktop-portal screen capture)

**Groundwork for the engine-owned screen picker; not yet reachable.**
The engine asks the portal directly (ashpd ScreenCast: create session →
select monitor/window (cursor hidden, restore token) → start) so Electron no
longer needs to expose the PipeWire node id. Impressive threading discipline:
the dialog blocks for as long as the user takes; it runs on a dedicated
single-thread current-thread tokio runtime (`modv-portal-screencast`), the
PipeWire node stays valid for the life of that thread, and `PortalStream`
owns it with bounded close semantics. Pure mapping/selection/error tests run
without D-Bus; only the dialog needs a live desktop (documented). The
commit honestly notes "Wiring into media/pick-screen follows in the app" —
at `a121eaea` there is no Rust call site; no `media/pick-screen` handler
exists yet.

### Findings

- **[P3] Not reachable in engine mode yet** — the module is dead code at
  HEAD (chosen deliberately, documented); `media/*` remains on the wildcard
  list. The feature lands when the app-side wiring does; until then ~1 kLOC
  of engine behind a Linux cfg serve no user.
- **[P3] Progress doc not updated** (pattern continues — v15, the MIDI/OSC
  win, this) — the doc's "registered modules" headline is already stale
  (429 vs 431, Round 33).
- No correctness defects in the portal code itself; the pure-layer tests,
  assert_send declarations and error taxonomy are disciplined and honest.

## Round 38 — `145005ec..e4cf3e26` (image inputs bind real textures)

**This closes one half of my standing P1 — the largest functional gap in
the engine — for ISF image inputs.** A saved texture prop now resolves to a
real GPU texture through `crate::texture_sources::TextureRequest` +
`ImageTextures` (decode-once LRU keyed by `(path, requested size)`, shared by
live ISF modules and Gallery thumbnails), with sources in this order:
image assets from the media directory (absolute paths used as-is; a media
root configurable for tests and a future IPC; `..` segments and NUL bytes
rejected), live media sources (camera/screen/video matched either by exact
engine source id or by media-library relative suffix), module outputs
(isf/two-d/point10), other groups' canvases, and the group canvas itself
when the prop names it or fails to resolve. Self-reference binds the canvas
(wgpu forbids aliasing the render target). The filter alpha mask now
applies only when at least one image input still binds the group canvas —
an assigned photo/video filter is no longer erased over a transparent
group; existing Scenes (last image inputs unset/unresolvable) render
bit-identically (test pinned on the canvas fallback path).

**Remaining inert:** `Texture 2D` (the 2D module with `TwoDProp::texture` /
`DrawContext::input`) — nothing fills `input_texture` with an assigned
image; only a webcam capture can feed it. The ISF transition modules are
unchanged (still Fade fallback; item 8 stands).

### Findings

- **[P2] Image decoder bomb** — `decode_file` enforces `MAX_IMAGE_PIXELS`
  by checking the dimensions **after** decoding (`image::open` → `to_rgba8`
  already materialised the whole buffer); the only pre-decode guard is
  `is_file()`, with no header read and no byte cap. A hostile Scene/preset
  whose image prop names a giant PNG can OOM the engine before the check
  fires; a shared Scene is untrusted input. Fix: read
  `image::image_dimensions(path)` first and reject when the product exceeds
  the cap, before opening the decoder; keep a decode-time budget too.
- **[P3] `matches_video_path` uses `ends_with`** — the media-suffix match
  tolerates both absolute and media-library relative prefixes by design,
  but it is also forced-binding: any running `video:` source whose path
  ends with the prop's needle binds to the shader, whatever its origin.
  Read-surface only; the local-media trust model accepts it; noted.
- **[P3] `set_media_root` is `#[cfg_attr(not(test), allow(dead_code))]`** —
  nothing in production wires it; runtime resolution is always
  `$MODV_MEDIA_DIR` → userData default. Either expose the root override (a
  future `media/*` IPC) or drop the dead seam.
- **[P3] `module-parity.md` line 53 is stale** — it still says "there is no
  texture registry; texture props are decorative" — now true only of
  `Texture 2D` and transitions; the doc's 429/431 arithmetic staleness also
  persists.
- **[P3] Progress doc not updated** — pattern persists (v15 Scenes, MIDI/OSC
  transport, desktop-capture groundwork, Output Windows, this).

## Round 39 — `e4cf3e26..a121eaea` (engine-owned native Output Windows)

**`windows/createWindow` opens a real native window now; `window.open` is
gone from engine mode.** A pure registry (`windows.rs`, clamp sizes
64..=8192, default 300, title trim/cap, duplicate-id rejection) feeds the
frame loop's per-frame `sync_windows`; the compositor blits the shared main
texture into every Output Window's swapchain in the **same command encoder**
as the primary present (`AutoNoVsync`; occlusion/lost/validation-skipped
with warn). `windows/REMOVE_WINDOW` is emitted by the remove action, the
WM-close path (`Destroyed`) and the failed-create path — all three.
`state/sync` carries `windows` so a late-joining mirror shows the live
window list (`SET_WINDOWS`); `window-handler.js` never opens popups in
engine mode; the View menu stays enabled; `create-output-window` dispatches
the engine action in both modes; the boot warning no longer mentions output
windows. GPU smoke (two surfaces, three frames, one device), real-engine
create/remove/WM-close and 60 fps claimed — verbatim, unverified here.

### Findings

- **[P3] Boot warning re-lists shipped support** — `engine-unsupported.js`
  now claims `"Rust engine mode: MIDI, OSC and JavaScript plugins are not
  supported yet ..."`; MIDI **and OSC** were wired in `70540391` (Round 36),
  the MIDI panel is enabled and the OSC Listen/Stop button exists. The
  comment block at the same file's head ("MIDI, OSC, plugins and local
  fonts are silently dropped") is equally stale (local fonts shipped in
  `457c846a`). One-line fix plus one comment; verify against the dispatch
  table before editing.
- **[P3] No end-to-end Output Window coverage** — the Electron click path
  is unit-tested; winit Output Windows have no e2e coverage (they exist; the
  user just has to see them). Consistent with the engine pattern of GPU
  e2e being out of reach for now.
- **[P3] Progress doc not updated** — same repeating pattern.
- No P2. The windows are correct by construction (no second compositor, no
  divergent frames, `Main + Output` per-window input handling).

## Round 40 — `a121eaea..00b82246` (engine-mode screen picker and media root)

**The Round-37 groundwork is live.** `media/pick-screen` runs the portal
dialog on a worker thread, defers the reply to the frame loop, starts the
picked PipeWire node as a screen source, keeps the `PortalStream` alive until
stop, and answers with the full pick payload. Cancel → `cancelled`, no
portal → `media_unavailable`, wedged engine → 5 min IPC timeout; shutdown
answers every pending dialog instead of leaving callers to time out. IPC:
`engine/media-pick-screen` (packages `pickScreenPayload`, 300 k ms), preload
`pickEngineScreen`, engine-client `pickEngineScreen` with its own timeout,
renderer `engine-media.pickScreen`. `ScreenCapture.vue` re-points the panel
in engine mode to the portal: Pick Screen/Window… → the system dialog,
engine source list polled from `media/status`, Reconnect stops+picks again
(intensity: a portal node dies with its session), Texture-2D creation button
hidden (it does not consume the media registry yet — correct).
`media/setMediaDirectoryPath` stores the project media root and the window
loop applies it to the compositor at the next frame (`media/setMedia-
DirectoryPath`/`media/setState`/`media/addMedia` renderer-dispatch sites
already exist at `application/index.js:670` and in media-manager, so project
open flows it in). ISF texture props and library image paths now resolve
under the project root — verified by reading the same path end to end.

`app 358` claims verbatim; not re-run.

### Findings

- **[P3] Notices ignore the Rust binary's crate graph** — the engine ships
  `ashpd`, `zbus`, `rosc`, `winit`, `wgpu`, `naga`, `image`, … and none of
  it appears in `THIRD_PARTY_NOTICES.md` (JS-only coverage today). The
  Round-21 notices finding widens an axis: the distribution ships a whole
  Rust dependency tree with zero attribution. Fold into the
  outstanding notices item for whoever does that pass.
- **[P3] `.deb` still missing `gstreamer1.0-pipewire`** — screen capture
  now works out of the box only on hosts with the plugin; a fresh `.deb`
  install cannot start a screen source at all. Same class as Round-13's
  gap, restated: required dependency for every engine-mode screen user on
  Debian-likes.
- **[P3] No end-to-end coverage of the pick path** — the dialog path needs
  a live D-Bus session and a human; untestable here. The pure wiring is
  verified by reading (queue → drain → start → insert → stop → drop/release,
  order confirmed). Noted, acceptable.
- **[P3] restoreToken never round-trips** — the renderer captures it in the
  matcher but never passes it back to a re-pick (the picker always opens
  the dialog). Documented deliberate non-persistence — just noting the
  feature as shipped is "always ask".
- **[P3] Double-open pick piles a second dialog on the first** — the Pick
  button is enabled while a pick is pending; the portal queues both.
  Disable during pending or add a `pending` flag; minor ergonomics.
- No P2. The lifecycle is correct: stop drops the PortalStream → node
  destroyed → "sharing" indicator off; shutdown clears sources *and*
  portal sessions and answers pending dialogs.

## Round 41 — `00b82246..7f501bac` (docs record the features; pick-screen input validated)

**Clean test+docs commit; two stale claims survive the very edit that
touched their paragraph.**

Verified by reading:

- **Docs entry is accurate in every concrete claim I checked** —
  `windows/createWindow { id?, name?, width?, height?, x?, y?, monitor?,
  fullscreen? }` matches `windows.rs`; sizes clamp to 64..=8192 logical
  pixels; `F`/double-click fullscreen toggles per window, Escape leaves;
  WM-close and action-remove both emit `REMOVE_WINDOW`; `state/sync` carries
  `windows`; `SET_WINDOWS` replaces the mirror list; `window-handler.js`
  dispatches the action instead of `window.open`; View menu enabled;
  `MODV_ENGINE_WINDOW_SMOKE=1` env-gated multi-surface smoke test exists
  (`window.rs:1495`); portal screen capture entry records deferred reply +
  PipeWire lifecycle + distinct cancel/unavailable codes honestly;
  ISF-image-inputs entry matches `e4cf3e26`'s behaviour exactly.
- **Input validation added and tested** — `media/pick-screen` now rejects a
  non-"monitor"/"window"/"both" (or non-string) `sourceType` with
  `invalid_params` before spawning the portal thread (dialog is never
  opened), covered by a test (`pick_screen_rejects_an_invalid_source_type_without_opening_the_portal`).

### Findings

- **[P3] The paragraph edited this round still asserts MIDI/OSC are off.**
  `docs/rust-hybrid-progress.md:287-291` reads "Engine-mode UI marks MIDI
  and OSC as unavailable; … the BPM panel's MIDI source remains selectable
  (it cannot work without MIDI)" — every clause is stale: the MIDI panel is
  enabled (no disabled controls remain in `MIDI.vue` at HEAD), the OSC
  Listen/Stop button works, the BPM picker's MIDI source works since
  `457c846a` + `70540391` (SET_BPM_SOURCE + WRITE_DATA path). The same file
  also gains a correct "MIDI and OSC are wired …" post-review entry, so the
  file now self-contradicts. One-sentence edit.
- **[P3] Boot warning still claims shipped support** — unchanged this commit
  (`engine-unsupported.js:16`: "… MIDI, OSC and JavaScript plugins are not
  supported yet …"); item 30 stands as written.
- **[P3] Docs-lag pattern shows real improvement** — this is the first of
  the last six commits to comprehensively update the progress doc, and the
  `ISF image inputs`/Output Windows/pick-screen entries are all accurate.
  The remaining gap is claim hygiene inside the edited paragraph, not the
  pattern itself; item 31 updated to reflect that.

## Round 42 — `7f501bac..76cd1440` (mediaStream selection commits hit the mirror only)

**Partial fix of the AudioVideo engine-guard class — the right shape, five
of six sites closed.** The commit moves `mediaStream/SET_SELECTED_VIDEO_SOURCES`,
`SET_CURRENT_AUDIO_SOURCE` and `SET_VIDEO_SETTINGS` (both at
`application/index.js:580-587` boot path and `AudioVideo.vue`
toggle/revert/`syncSelectedVideoSources`/`REMOVE_SELECTED_VIDEO_SOURCE`)
behind `engineMode` checks — engine mode commits to the mirror only, legacy
keeps the double commit. Verified by grep of every direct
`this.$modV.store.commit|dispatch` site in the file: all remaining straight
commits in engine mode are properly mirrored-only; the legacy path is
unchanged.

### Findings

- **[P3] One crumb left from the `76cd1440` migration (see item 32, below)** — `AudioVideo.vue:821`:
  `outputs/removeWebcamOutputForDevice` dispatch fires before the sibling
  guard on the very next line; the engine facade returns null (warn-once
  per session) and the dispatch is a legacy-only cleanup with no engine
  target. One-line guard fixes it; the class as a whole is now handled
  everywhere else in this file (verified by enumeration).
- No P2. The fix does not change legacy behaviour and engine-mode selection
  toggles no longer spam the engine facade (each toggle used to hit the
  warn-once).

## Round 43 — `76cd1440..5139d8a0` (engine-owned NDI output and input)

**The NDI path the JS never had and the engine wildcard was hiding.** The
legacy worker owned NDI in two places; engine mode had neither, so every
`ndi/*` action fell through to the wildcard while the panel stayed wired.
Now:

- **`ndi_runtime.rs`** owns an SDK instance per side: sender on its own
  thread, drop-oldest queue of 2 with frame-rate control (advertised rate
  from measured engine fps or target `followModVFps`/`outputTargetFps`),
  nearest-neighbour resize when `followModVOutputSize` would choke a receiver;
  receivers one thread per enabled id (connect+capture inside it), newest
  RGBA frame wins per id, *error state* surfaces to the panel when the source
  disappears; **a transport trait** makes the whole lifecycle testable without
  libndi (`LibNdiTransport` is the real arm).
- **`modv-media`** gains discovery (fixed find/timeout), selection
  (`select_source`, case-insensitive substring like before), `NdiFrame`,
  `RGBA→BGRA` send conversion with a FourCC the SDK expects
  (`NDIlib_FourCC_video_type_BGRA`), `COLOR_FORMAT_RGBX_RGBA` on receive —
  exactly the legacy `grandiose` settings. FFI is isolated in
  `modv-media::ndi` with documented SAFETY comments throughout; the unsafe
  body follows the documented ownership protocol (`recv_capture_v2` →
  `recv_free_video_v2` exactly once per captured video frame), strides and
  pixel bounds are checked before any slice is constructed, all unsafe arms
  reachable via `recv_capture_v2`'s frame-type match (None/Error/Video/audio
  ignoring).
- **The compositor uploads receiver frames** to `ndi:<id>` textures and feeds
  them to (a) ISF image inputs, (b) **`Texture 2D`'s texture prop** — the
  outstanding P1 is *closed*, (c) the Webcam module's source list (after
  physical cameras).
- **Mirror parity is excellent** — same delta types, same payload shapes;
  `ndi/getState` + `ndi/getAvailability` read the engine state; aux-output
  records let the texture picker list the receiver under NDI (`group: "NDI"`);
  the mirror-side `outputs/REMOVE_AUXILLARY` and `UPDATE_RECIEVER` payloads
  match exactly.
- **The JS mirror's `state.ndi` flow is unchanged**, the panel disables its
  controls when libndi is absent (engine availability check path:
  `pollNDI status every second`) — with `engineSenderError` banner for
  a dead sender.

Verified by reading the full stack. No live NDI smoke
(**this cannot run without libndi** — host needs it); recorded verbatim
claims ("Workspace 1925, Vitest 1215, lint clean").

### Findings

- **[P3] Sender read-back is a full-resolution memcpy per frame**
  (`submit_ndi_frame` reads the just-presented main texture, ~8 MB at
  1080p, plus queue memcpy). The JS did the same via `getImageData`, so
  this is strictly no worse, but a `GPU→!staging→queue` path would be the
  long-term shape; flagged as perf, not defect.
- **[P3] No CPU-instruction preflight** — the JS threw when NDI's
  instruction-set probe failed; the engine relies on libndi failing to load
  correctly (unavailability surfaces via the panel as expected). Same
  outcome through a different window; noted for symmetry.
- **[P3] No live NDI e2e** — impossible without a host/receiver pair;
  noted, not actionable from here.
- **[P3] `.deb` status for `libndi` unchanged** — grandiose already needed
  it for legacy builds; packaging story is unchanged and remains
  user-sourced as before (see item 12).
- No P2. This closes the last invisible wildcard class for a core feature.

## Round 44 — `5139d8a0..6c627812` (dataTypes/createType mints tweens)

**The last silently-confirmed engine-mode Input Link path.** The Input Link
Tweening editor dispatches `dataTypes/createType { type: "tween", args }` and
links to the returned descriptor. The engine forwards `type: "tween"` to the
tween store ([`Self::create_tween`] — same dispatch the JS chain uses,
`dataTypes.js:119-128` calling `tweens/createTween`). Unknown types return
`null`, and `groups/SWAP`/`modules/SWAP` are acknowledged no-ops — the engine
preset loader replaces state wholesale and has no swap-buffer semantics for
`ABSwap.vue`'s A/B debug button (the commit calls it out explicitly).
Verified by reading end-to-end (and by the added unit test, which boots a
Store, dispatches `dataTypes/createType {type: "tween", args: {id, data,
duration, easing}}`, asserts the tween persists with an `ADD_TWEEN` delta
emitted and the id returned — plus the null-branch for unknown types).

### Findings

- **[P3] `ABSwap.vue` stays clickable in engine mode** — both SWAP commits
  hit the acknowledged no-op now; the button renders and silently does
  nothing. A one-line engineMode guard or a disabled notice would be the
  friendlier shape (matches the R-23 pattern the author used elsewhere:
  disable or explain — don't silently swallow).
- **[P3] `dataTypes/createType` texture arm remains unplumbed** — a
  `{ type: "texture" }` call still returns `null` silently. Same class of
  silent no-op the engine acknowledges; no current UI path dispatches it
  this way, so it stands as a known-but-unwired corner (a textures picker
  dispatched via `dataTypes/createType` would need it).
- No P2. Small, exact, well-documented; the tween path is now genuinely
  usable in engine mode.

## Round 45 — `6c627812..f5c243b6` (docs record NDI/tween/mediaStream)

**Clean docs pass; three pre-registered docs items survive unchanged.**
The new entries accurately document the three recent commits and the runtime
contract: NDI transport (sender thread + drop-oldest queue + nearest-neighbour
resize; receiver frames → `ndi:<id>` textures → ISF/Texture 2D/Webcam) ×
`MODV_NDI_LIB` honoured by the loader (`ndi.rs:509` + `ndi_runtime.rs:1056`
+ `NDI.vue:15`) matches verbatim; `dataTypes/createType` mints tweens and the
A/B debug SWAP no-ops are acknowledged; MediaStream selection is UI-plane.
Verified line-by-line against the code.

### Findings

- **[P3] Both docs inconsistencies survive this edit** — the
  capability-list paragraph (`docs/rust-hybrid-progress.md:287`, "UI marks
  MIDI and OSC as unavailable") is still contradicted by the same file's
  post-review entries (lines recording both as wired, with the tween note);
  the boot warning (`engine-unsupported.js:16`) unchanged; the `429`
  registered-modules headline persists (431 arithmetic). Three sites,
  tracked as items 30/31 — a one-line edit would retire all three at once.
- No code; no new claims; no new defects. Third docs-only commit in a row,
  all entirely accurate on the new content — the hygiene gap is precisely at
  the *stale* sites, not the new ones.

## Round 46 — `f5c243b6..f591a108` (ISF transitions render natively in engine mode)

**This closes my standing ISF-transitions-infeasible finding end to end, and
it is the pairing of the picker list the legacy app shows with the engine's
ability to actually run those shaders.** The six natives stay byte-identical
(`scene-transition-defs.js:19-125`, unchanged; their tests unaffected). The
classification is exact: `Fade`/`None` are checked before the corpus (the
corpus ships `Fade.fs`, and it must not hijack the alpha-ghost path), an
ISF document is transition-shaped when `startImage` + `endImage` + `progress`
are its inputs (same `ISFParser.inferFilterType` predicate the JS runs,
ported faithfully), and `TransitionEffect::Isf { name, document }` carries the
document so the render path never re-reads the corpus. `performance.rs`
`pick_scene_transition_name` extends the available list with every
registry transition-shaped module, the mirror picker (`Scenes.vue`) shows
exactly that set, and `prepare_scene_transition` compiles the chosen effect
before the first frame; a compile/draw failure is remembered and
`fall_back_scene_transition_to_fade` resets the transition to the Fade alpha
path (kind → `Fade`, effect → `None`, incoming groups reset) — scenes change
never silently vanish. The ISF transition binds the captured outgoing frame
as `startImage` and the incoming main frame as `endImage`, progress from the
timeline, and draws into the shared scratch; the `CLEAR: false` final-pass
case is handled (`needs_clear` checks the last target of the compiled
pipeline). `IsfFrameUniforms` see a real TIME clock; TIMEDELTA/FRAMEINDEX are
frozen (acceptable for a ~1 s effect). The LRU is bounded (4 compiled
transitions, failure-remembered, compiled via the same runtime the live
modules use).

Test coverage is genuine: temp-corpus classification (filter/generator/
unknown/Fade.fs), GPU red→blue mix at 0/0.5/1, compile-failure prepare →
fallback, an end-to-end `render_frame` transition (outgoing → incoming),
and the performance pick/fallback paths. A pre-existing debug-only
`u8 + 200` overflow in a Point10 test was also fixed.

### Findings

- **[P3] Progress doc contradicts the feature it ships** —
  `docs/rust-hybrid-progress.md:277-278` still says "Custom/third-party ISF
  transitions are not available… the picker filters unknown names to Fade".
  Stale at this commit; same claim-entropy class as items 30/31.
- **[P3] `TIMEDELTA`/`FRAMEINDEX` frozen in transition uniforms** — an
  ISF transition keyed on FRAMEINDEX/TIMEDELTA observes a frozen frame.
  Low impact for a ~1 s effect; noted for completeness.
- **[P3] No live end-to-end transition smoke** — the GPU red→blue mix tests
  use synthetic frames, not the full applyScene→tick→present loop. Sufficient
  for CI; noted.
- No P2. The picker and the engine agree, the fallback is sound, and the
  biggest remaining shipability gap against the legacy app is level now.

## Round 47 — `f591a108..46f5bb18` (docs(progress): ISF scene transitions)

**Docs-only commit; every claim in the new entry verified against the code at
its own HEAD. Two self-contradictions persist in the same file, one of them
one pargraph away from the new entry.**

Claims checked (all accurate, all verified in Round 46):

- `TransitionEffect::Isf { name, document }` exists; reserved `Fade`/`None`
  win before the corpus lookup; the six natives stay byte-identical;
  a transition-shaped document (`startImage` + `endImage` + `progress`)
  renders between captured outgoing and incoming main with `progress` =
  timeline; compile/draw failures fall back to Fade; bounded 4-entry IsfRuntime
  LRU shared with the live-module path; performance picker picks every
  registry transition-shaped module.
- GPU test red→blue mix at 0/0.5/1, uncompilable fallback, end-to-end render,
  performance pick paths — all present and relevant.

### Findings

- **[P3] The progress doc visibly contradicts itself in adjacent sections** —
  `docs/rust-hybrid-progress.md:277-278` still reads "Custom/third-party ISF
  transitions are not available: the six native transitions are ported, the
  picker filters unknown names to Fade", while the `:486+` entry added in
  this same commit records "ISF Scene transitions render in engine mode…"
  with the compiler/LRU details. Two paragraphs of one file disagreeing by
  one commit; the earlier one is stale since `f591a108`. Same class as items
  30/31 (this time with the contradiction *inline* in the same file at the
  same era of edits).
- **[P3] The `427-registered-modules` arithmetic continues unchanged** —
  item 30/31 (docs-claim entropy) carries this site; no regression.
- No code; no new defects; no actionables beyond the doc-claim text.

## Round 48 — `46f5bb18..f7d35db0` (the three renderer and Cube are removed)

**A genuine end of the last parity gap; executed cleanly.** The legacy
`renderers/three/` tree and `renderers/three/modules/Cube.js` (the only
`type: "three"` module) are deleted in both runtimes — the engine never
registered `Cube`, so the ThreeJS Gallery section was the last
module-parity gap. Verified by grep at HEAD: no remaining `"Cube"`
references (Point10 Cube variants and MirrorCube/ColorCube etc. are the
unrelated Plane9 family), no `renderers/three/` glob left in the worker, no
`three` category in `DEFAULT_CATEGORY_BY_TYPE`/`CATEGORY_LABEL`/
`CATEGORY_ORDER` in `Gallery.vue`, `renderersAreRegistered` spec
updated, `cube-removed.test.js` guards the files and the seeded defaults.
The `three` npm package stays (Point10 scenes import it) — documented.

### Findings / notes

- **[P3] The Rust ISF category normaliser is now stale on `three`** —
  `crates/modv-renderers/src/isf/props.rs` still maps `"three"`,
  `"three-js"` and `"threejs"` → `"three"` and its tests assert it
  (lines ~525-543), while `module-category.js` dropped the same aliases.
  The Rust side can only emit `"three"` for an ISF module registered with
  that category, and the Gallery then falls back to type-based effects; the
  two sides disagree on category vocabulary and the tests pin the stale
  side. Remove the mapping + tests or align both against the category
  registry.
- No default Scene or preset referenced Cube (verified — absent from
  `default-scenes.json`, `default-scenes.js`, both PLAYLISTS) and none
  do at HEAD.
- Count claims ("27 scenes", "28 scenes") are docs-lag noise only; see
  item 25/30.

## Round 49 — `f7d35db0..a6ca39c0` (screen blend mode parity)

**The single largest real-world divergence this round: every default
Scene's Foreground group — and HOTSHOWERRAVE — used `compositeOperation:
"screen"`, the 5 default-Scenes v15 seeds included; the engine emitted
`screen` dozens of times per frame as `normal` with a warning.** Verified
at HEAD: 73 `"compositeOperation": "screen"` group entries in
`crates/modv-engine/data/default-scenes.json`, byte-matched in the JS
mirror (73 occurrences) — all previously degraded. The commit adds
`BlendMode::Screen = 28` in both crates (computed `cb + cs - cb*cs`, the
W3C separable screen function, evaluated after un-premultiplying like the
rest of the section), Porter-Duff bounds tightened to 15..=27 with the
explicit match replacing the `>=` discriminant heuristic, both parse paths,
counts 28→29, and the JS menu offers Screen after Multiply; golden case
RED over BLUE → 255,0,255 in `graph_tests.rs`; transition pass unaffected.

### Findings

- **[P3] Stray literal `\n` inside a doc comment** —
  `crates/modv-engine/src/blend.rs:114` reads "…with `screen` appended after
  the\\n/// 15 blend modes": the backslash-n is literal text in the comment
  (rustdoc renders it verbatim; `cargo fmt` does not fix comments). One-line
  edit; survives the Round-54 rustfmt as-is.
- Subtle pass: the Rust `RENDER_ORDER` of ALL_BLEND_MODES inserts Screen
  after Luminosity (not at the JS menu's multiply+2 position) — harmless,
  the enum order is not a contract.
- The engine's `parse_blend_mode` now accepts `screen`; preset blending
  across the board resolves; mirror replay unchanged (strings passthrough).

## Round 50 — `a6ca39c0..cbb24d8a` (Point10 Sun and Earth ported to wgpu; 28 scenes)

**Two scenes; ~4,600 lines of port.** The registry brings both `Sun` and
`Earth` through the shared `all_scenes`/`Point10SceneKind`/
`module_registry` dispatch (`Sun(Sun::new())`, `Earth(Earth::new())`),
and the Gallery's Point10 catalogue is identical to legacy for the
built-ins (28). Prop schemas evaluated field-for-field against the JS:
`SUN_PROPS` (surfaceSpeed/brightness/corona/rays, defaults and ranges
match `sceneProps.js:153-158`), `EARTH_PROPS` with `spinSpeed`
(`sceneProps.js:140-145`), including `SceneMeta::for_scene(..., "3d-scenes")`
for both. Earth's assets (`earth.jpg`, `earthlights.jpg`, `earthcloud.jpg`,
`wilkyway_panorama.jpg`) all exist in the shared asset loader
(`$MODV_ASSETS_DIR` → packaged `textures/` extra resource is wired in
`electron-builder.yml`, end-to-end for the packaged engine; the dev
fallback reads the repo's `textures/` directly; missing files fall back to
procedural stand-ins, never a render crash). Neither scene appears in any
default Scene — correct scope, both are Gallery entries only.

### Findings

- **[P3] The HDR chain cannot be verified here** — the Earth's
  `UnrealBloomPass`/`OutputPass(ACES)` port (exposure = brightnessScale)
  is structurally reasonable from reading, but the composites go through no
  automated image comparison; the scenes claim `brightnessScale` identical
  to legacy — recorded verbatim, not endorsed.
- **[P3] Progress-doc line 262 now doubly stale** — "the engine registers
  429 modules vs the legacy renderer's 432 (missing Point10 Earth,
  Point10 Sun, Cube)" — Sun and Earth landed in this commit and Cube was
  removed from both runtimes; the correct arithmetic (36+15+354+28 = 433) is
  not written anywhere in the docs. Same docs-arithmetic class as item 13.

## Round 51 — `cbb24d8a..34528ff6` (Point10 Geometric Tunnel retired; v16 defaults)

**Clean, symmetric retirement with names kept for refresh matching.**
`Geometric Tunnel`, `Golden Spiral Tunnel`, `Reactor Glow`,
`Crosshatch Cipher`, `Digital Crystal Tunnel` are retuned in place in both
runtimes: the hero module swaps to `Square Tunnel`/`Light Tunnel`/
`Color Cube`/`Wormhole`/`Spiral Particles` (keeping the treble-alpha link
and the `screen` blend where shipped), `DEFAULTS_VERSION = 16` in JS and
Rust, two Scenes joined `REFRESHED_DEFAULT_SCENES`, and `Point10 Geometric
Tunnel` joined `REMOVED_DEFAULT_SCENE_MODULES` in both lanes. The SSAO
pass, used only by the removed scene, is deleted with it (~615 LOC).

Verified by field-walking both sources: the five retuned default-Scene
documents are semantically identical JS↔Rust (only float/int print noise).
The removal guard test (`point10-removed-effects.test.js`) now forbids the
name variants in source/tests and asserts `default-scenes.json` contains
no reference; the e2e version pins move 15→16 in both lanes. The JS bundle
ships the file deletions through the same `import.meta.glob` removal as
`f7d35db0`.

### Findings

- **[P3] Progress-doc `27 scenes`/"default Scenes 12→16" counts** drift
  the arithmetic again (28 scenes registered after Round 50, 57 default
  Scenes at HEAD); docs-claim-entropy class unchanged (item 31).
- **[P3] `module-parity.md` claims "432/432 closed" headcount** with
  `34528ff6` — the claim is the author's; the same doc still says
  "headcount 432/432" that item 13's 429/431 arithmetic already contradicts
  (the three prior sites are close, not closed).
- No P2. The refresh/migrate lane is exactly the correct shape: no renamed
  module rule ships for `Crosshatch Cipher`/`Digital Crystal Tunnel` —
  they are already in `REFRESHED_DEFAULT_SCENES` (verified in both lanes).

## Round 52 — `34528ff6..a918598f` (e2e panel fix + two style commits)

- **`4a547b16`** (`test(e2e): drop three-buffer…`) — the aux-context AA
  e2e spec now lists the two remaining contexts. Correct; the count
  `three-buffer` disappears from every assertion.
- **`0fc00538`** (`fix(scenes): tolerate null group holes…`) — **the real
  fix in this batch**: a saved Scene copied into a shorter Scene rebuilt
  `groups` with `null` holes (`scenes/copySceneCellToScene`,
  `scenes.js:3568`); the panel's `sceneModuleCount` read `group.modules`
  on every entry and threw, unmounting the whole panel — the reported
  "crashes the panel". New `scene-groups.js` exports a null-tolerant
  counter (arrays and object maps), `Scenes.vue` uses it, `modules/init`
  degrades to an error log instead of throwing, and the Ctrl+drag e2e now
  asserts the panel survives (grid visible, copied cell shows Ball). The
  unit test pins the holes/maps/zero-paths. Verified against the JS apply
  loop (`scenes.js` applyScene
else) — complete.
- **`a918598f` / `dd8914c2`** — prettier formatting commits for the
  two specs (the engine-skeleton spec of the user's own uncommitted tree
  is now committed as `dd8914c2`; the anti-aliasing spec identically).

### Findings

- **[P3] The `modules/init` degrade path logs but does not surface to the
  UI** — a Gallery item whose registered definition vanished (e.g. a
  Scene referencing a retired module before the refresh ran) disappears
  silently from rendering. The new branch is the right shape; an empty-state
  slot is out of scope.
- No P2.

## Round 53 — `dd8914c2..cf9be174` (volume curl-noise precompute fix)

**Bug was real: the four volume scenes (Rolling Fire, Ball of Fire, Smoke
Room, DVD Room) ran without thermal/ambient curl turbulence.** `VolumeSim::new`
precomputed the curl field before any uniforms were written, so the
`gid >= u.grid.xyz` guard in the pass rejected every workgroup; the field
stayed zero; radial buoyancy (the "puffy sphere") was left — the "smoke
looks unnatural" symptom. The commit threads real values through
`VolumeUniforms::curl_precompute(grid/dims/world-size/TURB_FREQUENCY)`
and stamps the buffer before the dispatch, mirroring the JS. A third-list
component scale discrepancy vs three's `snoiseVec3` (domain scale on the
third component, not result scale) is also corrected. New GPU regression
test reads the curl field back and asserts non-zero — correct shape.
`TURB_FREQUENCY = 10.0` matches the JS `uTurbFrequency = 10.0` in all
three JS scenes (RollingFire.js:402, SmokeRoom.js:486, DvdRoom inherits
SmokeRoom).

### Findings

- **[P3] The GPU regression test silently passes without an adapter** —
  the `crate::point10::test_support::gpu()` pattern is the established
  one; the curl-non-zero assertion only runs with a GPU. On headless CI
  this fix is verified by reading only; noted.
- No P2.

## Round 54 — `cf9be174..6e5cf6be` (clippy/rustfmt for Rust 1.100)

**Mechanical: the `modv-rust:local` toolchain moved to 1.100**, so
`make engine-lint` failed before touching code. The commit converts inherent
`from_str` calls to `FromStr` (trait-imported; call sites in `blend.rs
tests` import it; `modv-app/compositor.rs` imports it for `BlendMode::from_str`),
`as_chunks`, iterator loops, `checked_div`, `Waker::noop`, range contains,
const assertions and the `let … else` conditions. All formatting from
`cargo fmt --check`. 1951 workspace tests green per the author (not run
here). The `frontend` path (`from_name` in modv-render) is unchanged by
construction.

### Findings

- **[P3] The Round-49 stray `\n` doc-comment survives** — the marker of a
  comment that rustfmt never touches; see Round 49's finding.
- No P2. The `FromStr` migration is internally coherent from reading
  (the engine test suite still imports `std::str::FromStr` at
  `modv-engine/src/blend.rs:306` and all engine call sites use the trait).

## Round 55 — `6e5cf6be..87ee0787` (modv-ui P0 shell, RangeControl, Scenes cell)

**Foundations of the full rewrite: a new `modv-ui` egui crate (P0 framework
spike). Verified by reading every file. It is honest scaffolding — not wired
into any binary yet, and nothing in the shipping path changes.**

**What's implemented and correct:**
- The P0 probes from `docs/rust-full-rewrite.md` (range widget, Scenes-cell
  widget, output-window evidence points) map cleanly onto
  `RangeControl` (`crates/modv-ui/src/widgets/range_control.rs`),
  `scene_grid.rs` (draw + hover preview + Ctrl+drag copy + context menu),
  and the `P0State` checklist (GO/PENDING verdict gate).
- **Every action the UI emits exists in the engine's dispatch table with a
  matching payload shape** (checked at the engine site): `groups/createGroup`,
  `scenes/addEmptyScene`, `scenes/setPerformanceEnabled{enabled}` (the engine
  coerces 1/0/true/false; bool params match), `scenes/applyScene{sceneId}`,
  `scenes/captureSceneRowToScene{sceneId,rowIndex}`,
  `scenes/clearSceneCell{…}`, `scenes/captureLiveStateToScene`,
  `scenes/removeScene`, `scenes/copySceneCellToScene{fromSceneId,
  fromRowIndex,toSceneId,toRowIndex}`, `ui/addModuleToGroup{moduleName,
  groupId}` (new composite, handled by the *shell*, not the engine — the
  caller creates then appends), `modules/updateProp` +
  `modules/updateMeta` (`moduleId, prop|metaKey, data|value` — matches the
  JS dispatch sites).
- Theme constants ported from the CSS palette; `#![forbid(unsafe_code)]`;
  workspace member; `egui = 0.37.0`; `egui_kittest` test-kit harness;
  Cargo.lock updated coherently.
- Unit tests (16) cover the pure logic (clamps, typed-commit parsing,
  normalize, scenes copy-drag payload in both arms, preview probe);
  smoke tests attempt the shell draw, verification-friendly.
- Prop dispatch for float/int/bool/enum/string emits real engine updates;
  `color`/`vecN`/`palette`/`tween`/`font` render as chips (explicitly
  deferred) — documented in-code as P3.

### Findings

- **[P3] No binary depends on `modv-ui`, and nothing imports it** — by
  design ("not yet wired into a binary" per the commit) — but this means the
  P0 exit criteria (60 fps lavapipe + reference GPU, a frame-trace showing
  no readbacks, texture-registration against the pinned egui-wgpu, binary
  RSS ≤ the Electron build, written go/no-go) are all **unverified**, and
  the `P0State` checkboxes are stamped flags driven by
  `preview_texture.is_some()` only, not measured frame traces. GO verdict
  is a hand-set claim until the app loop lands (P1).
- **[P3] `SceneGridState.context` is write-only dead state** — set at the
  column-header and cell `secondary_clicked()` paths (`scene_grid.rs:136,
  309`) and never read; the context menus render directly via egui's
  `response.context_menu`. Remove it or use it to anchor a menu; today a
  stale anchor survives a right-click for no reason.
- **[P3] The kittest test name overpromises** —
  `shell_scene_grid_emits_apply_action_on_header_click` performs no click
  and asserts no action: it verifies `get_by_label("Lone Chrome")` only.
  Rename or click-drive. (The payload contract is pinned elsewhere:
  `scene_actions_match_the_store_payloads` etc.)
- **[P3] RangeControl's `R` button returns the midpoint**, not the prop
  default — explicitly documented in code as pending
  `$props[prop].default` binding. Honest but visually identical to a real
  reset; surfaced in P0 demos as if it were functional.
- **[P3] `timing_chip`, `apply_scene_action`, `clear_cell_action`,
  `copy_payload` are `pub` for tests/diagnostics** — should be
  `pub(crate)` with the tests using them in-crate, or inventory them as a
  supported API.
- **[P3] `RANGECONTROL PROBE` / `P0 CHECKLIST` / `P0 PROBE` panels look like
  shipping UI** — they are spike scaffolding; until P0 is signed off it is
  fine, but they occupy the production central panel slot and would confuse
  anyone launching the app before the wiring exists.
- No P2. Unreachable in production today (the crate is dead weight until
  the P1 `modv` binary exists), and the real work is inventive and careful:
  the action-vocabulary alignment with the engine dispatch table is the
  right discipline, and the author clearly know the JS this is replacing
  line-by-line.

## Round 56 — `87ee0787..b1b8458e` (native `modv` binary: shell + in-process engine)

**The second full-rewrite milestone: a single-process `modv` binary (winit +
egui + engine), sharing the device end to end, opt-in only. Verified by
reading every production file and both Cargo trees.**

What actually shipped and verified:

- **`modv-engine` and `modv` are now two binaries of `modv-app`** (a lib
  `modv_app` with `run_engine` and `native_main` entry points) — the engine
  binary path is a pure code motion (`main.rs` keeps only `main()` and
  delegates to the lib); both binaries share the `x11_backend_problem`
  preflight and `die_with_parent`.
- **The egui main window is presented with Fifo, the compositor-owned output
  window with AutoNoVsync; both on one `wgpu::Device`/`Queue`** (`shared
  engine`); the compositor's main texture is registered once with
  `egui-wgpu` via `register_native_texture` and refreshed in place with
  `update_egui_texture_from_wgpu_texture` on resize/recreate — zero
  readbacks, the whole path validated at GPU level (`main` texture usage
  includes `TEXTURE_BINDING`).
- **One command encoder per frame** covers acquire→blit→present for the
  output window *and* the egui UI pass (UI pass after the output blit in the
  same encoder); `acquire_surface` skips occluded/lost/timeout silently,
  reconfigures stale on resize; `TexturesDelta::clear()` acknowledges egui's
  contract (no unhandled-delta panic, both buffer-update and free list
  consumed).
- **Every engine action the shell emits dispatches correctly in-process**:
  `drain_ui_actions` routes `ui/addModuleToGroup` to a composite (dispatch
  `modules/makeActiveModule` → `groups/ADD_MODULE_TO_GROUP`, selects the new
  module) and everything else straight to `Store::dispatch` (verified at the
  same dispatch table from Rounds 28–44). `scenes/tick` uses ms deltas like
  the JS; `store.update_input_links(&audio, elapsed)` mirrors the engine
  loop; `scenes_tick_due()` gates idles; NDI/link-deltas/output window close
  run in `shutdown()`; the state snapshot flushes on quit/bench end (tmp
  + rename; `--state`/`restore_from_file` semantics identical to the
  engine's).
- **The P0 review nits from Round 55 landed as claimed**: RangeControl's
  reset button is now gated on a registered default (`with_default`, the
  read model carries the prop's `default` from the registry schema); the
  dead `SceneGridState.context` is gone (three writes, zero reads — the
  author's follow-through); helpers are `pub(crate)`; the kittest test was
  renamed to `shell_scene_grid_renders_scene_columns_and_cells`.
- **`make native` / `make dev-native` / `MODV_UI=native`** opt in; the
  Electron path stays default and is untouched; the packaged AppImage is
  unchanged (the hybrid engine binary keeps shipping in Electron builds).
- The `main.rs` diff is a pure code motion into `lib.rs` — byte-identical
  logic in both copies of the preflight; the only behaviour change intended
  is the second entry point.
- Hybrid path byte-identical behaviour (no engine change) except the
  visibility of `wall_clock_date` (pub(crate); native uses it) and two
  new `Compositor` accessors (`main_texture_view`, `main_texture_size`) —
  backwards compatible.

### Findings

- **[P3] Whole-scene shader transitions (Wipe/ISF/etc.) are unavailable in
  the native shell** — `frame_info()` sets `scene_transition: None`
  unconditionally because the outgoing-frame capture path is part of the
  hybrid window loop (it captures `transition_from` between frames). The
  author flags this in-code ("P4 ports it with the Scenes panel"). Consequence:
  a native-mode Scene change with an allowed shader transition simply switches
  instantly instead of fading; no crash, no corruption — just a missing
  smoothness regression against the hybrid. Noted, not a P2 (deliberate P0
  scope, documented).
- **[P3] Native has no `.deb`/AppImage packaging story yet and the binary's
  availability is all-or-nothing through `make native`** — expected (P9), but
  anyone demoing the P0 sees perfs through dev only.
- **[P3] `panic!` paths in egui-winit growth — egui 0.37's undocumented
  contract** — the explicit `textures_delta.clear()` after consuming the deltas
  is a fragile manual match; a future egui bump that changes the API will
  produce a panic in this path unless the commit is kept in step. egui_kittest
  does not run the present path; consider one comment marker (present) — the
  author did leave an explanatory comment.
- **[P3] The `ui: INFO state snapshot flushed to {}` message prints on every
  quit with no log file** — native has no structured logger yet (P1 scope);
  `eprintln!` is honest. Capture the real-world failure mode: nobody wins
  getting this on stderr only.
- No P2. The entire zero-readback path is wired end to end and the failure
  modes (surface lost/occluded/suboptimised) are handled where the hybrid
  needed them most. The P0 GO criteria (60 fps on both GPUs,
  ≤ Electron-RSS, written trace evidence) are measured claim territory, not
  this host's job; recorded verbatim, not endorsed.

## Round 57 — `b1b8458e..ea5fe63a` (real Gallery/Scene hover previews; fuller Scenes panel)

**The P0 checklist is materially closer: genuine isolated hover previews in
the native shell, bounded LRU, flatten-to-opaque, and the Scenes panel is now
uthinned — performance controls, timing overrides, rename, enable, move,
row names, empty-cell creation.**

Verified by reading (no local toolchain; test claims verbatim):

- **`answer_preview_requests` runs after present** — at most one queued
  preview per frame, and the LRU keeps 32 uploads max with oldest-first
  eviction after insert; `preview_errors` dedups the eprintln per key. The
  renderer LRU matches the hybrid Gallery/tile-order policy in behaviour
  (one preview per frame, session-scoped thrash guard).
- **`upload_preview` flattens the engine's premultiplied readback** before
  `egui::ColorImage::from_rgba_unmultiplied` — exactly the correction class
  of earlier rounds (premultiplied bytes must never reach
  `putImageData`/straight-alpha targets); the tile paints alpha 255 over its
  (dark) panel, which is the same appearance as the hybrid preview. Live
  frame (`with_output`) composites the module over the frame currently
  presented, falling back to the isolated thumbnail when no main texture
  exists — the right semantics for a 150 ms poller.
- **`PreviewRequest`/`gallery_preview_key`/`scene_preview_key`** are
  `pub use`d at the crate root; the 150 ms request interval throttles the UI
  side (neighbour-repaint storms); the live texture registers on resize; the
  `preview_requests` vec are appended into the shared `preview_requests`
  consumer in `central_panel` — clean plumbing, both request sources drain to
  one place.
- **All actions dispatch into the engine correctly**: `scenes/updatePerformance`
  with `{"config": {...}}` — matches the `update_performance_action` merger
  (clamps `minSeconds`, requires `maxSeconds >= min+1`, keeps
  `currentSceneId` — the UI clamps `min`/`max` the same way), `updateSceneName`
  (trim + non-empty), `setSceneEnabled`, `moveScene`, `resetSceneTiming`,
  `updateSceneTiming` (dialog pushes `Scene Timing …` with explicit override),
  `setRowNames`, `createSceneCellGroup`, `addEmptyScene`. All exist in the
  engine's dispatch table with the payload shapes the UI emits.
- **The `Scene` cell hover extension and live fallback work**: cells request
  `PreviewRequest::Scene` on hover with a last-hovered-cell cache (reentrant
  safe: one cell per 150 ms), dispatch fallback to the live main texture
  until the first preview arrives — the identical pattern the hybrid
  renderer applies (`hovered & cell-hover & preview → else live frame`).
- **`scene_preview::render_scene_preview`** — the whole hybrid isolated
  Scene preview path (registry-aware, module snapshots, point10/isf/two-d,
  CGO ID prefixes for shader slots) is reused with **no duplicated
  renderer** — additive integration, correct by construction.
- **`Rename` flow**: double-click on a scene column header — one-shot
  `rename_focus` grants focus, Enter/lost_focus commits, Escape cancels,
  trailing-trim invariant identical to the rename engine action
  (`scenes.js` trimmed rename), duplicate handling is the engine's
  responsibility (already returns null on collision; the UI stays permissive).
- **`timing edit` window**: `Scene timing: <name>`, two `DragValue`s with
  identical clamping discipline, save pushes update-override, re-opens on
  cancel; `Reset timing override` falls back to global timing via the
  existing engine action.
- **`row name` dialog** matches `normalize_row_names`'s position-safe
  semantics (empty names stay in place), `Add row` extends with a blank
  row; the merged mapping invalidates cached previews on `rowNames` change
  (already wired in `update_performance_action` from Round 28–31).

### Findings

- **[P3] Preview errors silently stop being reported after the first** —
  once a key lands in `preview_errors` the failure note never fires again,
  even if the error is intermittent (e.g. transient registry miss during a
  scene switch). Harmless for an LRU; if a preview breaks permanently the
  user sees a dark tile with no feedback. A count-based re-report (say, on
  the 10th retry) would close it.
- **[P3] Preview request LRU evicts without renderer release hints**
  (`preview_order` pops a key and drops the `PreviewEntry`/`TextureHandle`) —
  egui frees the GPU texture via the handle drop, so GPU memory is released
  by ownership; correct and panic-free. No leak, but the free is implicit;
  worth one comment so a future change can't "optimise" it away.
- **[P3] `Render main` + `preview_size` staleness on `compositor.resize()`** —
  handled by the `Ok((id))` re-register path on size change only after the
  next frame; one frame of stale-format preview on a resize is possible on
  slower GPUs. Cosmetic.
- **[P3] `range_control` scaffold still renders text-typed float/int props;
  color props stay chips** — expected; the chip path is honest and complete.
- **No P2.** This closes the single largest visual gap in the native shell
  vs the legacy app (real isolated hover previews); the Scenes panel now
  models every interaction the legacy Scenes.vue implements, including the
  empty-cell group creation and row names. Test counts claimed verbatim;
  tests not run here.

## Round 58 — `ea5fe63a..fe8a75ad` (stale support claims and ISF aliases corrected)

**The author just closed four pre-registered findings with one surgical
commit. All verified at HEAD.**

Verified by reading:

- **Boot warning and header** now list only JavaScript plugins
  (`engine-unsupported.js` header + boot line) — MIDI, OSC, local fonts,
  Output Windows are engine-supported fact at the dispatch table, so the
  claim is accurate; the header correctly explains that JS plugins are the
  only gap and that engine JS hooks run in the renderer-side host.
- **Progress doc inline contradictions fixed**: the `:277-278`
  "Custom/third-party ISF transitions are not available" gap entry is now
  replaced by the accurate passage ("ISF transition-shaped modules render as
  Scene transitions: the six native transitions plus the 27 transition-capable
  ISF corpus shaders …") — **I counted the transition-shaped corpus shaders
  byte-for-byte at HEAD with the predicate the engine runs
  (startImage/endImage/progress) and the answer is exactly 27.** The
  429/424/432 arithmetic is normalised at both stale sites
  (`rust-hybrid-progress.md:26`, `:74`, `:262` — including the
  "missing Point10 Earth/Sun/Cube" line, which now reads
  "both runtimes register the same 432 modules"); `state/sync`
  registered-modules lower bound moves 37+15+18 → 36+15+27 verified at
  `store_ops.rs`. The wildcard list in the same doc now reflects the
  implemented `midi/*`, `ndi/*`, `osc/createServer`, `dataTypes/createType` —
  correct.
- **ISF `three` category aliases dropped** —
  `crates/modv-renderers/src/isf/props.rs` aliases removed and the two tests
  rewritten to assert the null fall-through (verified in code); the
  category-alias table test pins 8 entries and only the effects/2d/3d
  values. An ISF module registered with `CATEGORIES: ["three"]` now yields
  `None` → Gallery type-fallback (ISF ⇒ Effects) — identical to the JS.
- **Stray literal `\n` in `crates/modv-engine/src/blend.rs:114`** is gone.
- The MIDI/OSC/local-font paragraph, the `osc` expression scope note
  (correctly states the empty scope), the Output Windows line and the plugin
  note all match verified behaviour now.

### Notes

- `engineParity.spec.js:74` still asserts `registered > 300` — the
  gate-tightening remains outstanding.
- No new defects introduced; no P2/P3.

## Round 59 — `fe8a75ad..0dd577b9` (image decoder bomb probe; removeGroup drops modules)

**`groups/removeGroup` now drops its Modules (before they leaked into every
later preset); the decoder bomb is refused before allocation; both with
regression tests.**

Verified by reading:

- **Decoder bomb** — `decode_file` runs `image::image_dimensions` (header
  read only) **before** `image::open` / `to_rgba8`; the cap stays as
  defence-in-depth on the decoded frame for a header that understates the
  decoded image. The attack this kills: a shared Scene/preset naming an
  enormous frame in a 71-byte header — **the unit test writes exactly that
  file, with an empty zlib stream, and asserts the function errors with
  "more than" in the message (the message appears only via the header
  guard), so the regression test kills silently-accepted bombs rather than
  merely exercising the happy path.** A 2×2 PNG still decodes.
- **`removeGroup` leaves no residue** — the action now routes through the
  shared `remove_live_group` tear-down: each Module no surviving Group
  references is `shift_remove`d from `state.modules` with the prop/meta
  inputs deleted and `modules/REMOVE_ACTIVE_MODULE` delta emitted; the
  Group's control inputs are removed through the normal table; tween
  descriptors owned by tween Input Links keep the tween cleanup (the
  `inputs/removeInputLink` semantics the JS dropped on the floor via plain
  deletion). The regression test seeds group+module, removes the group,
  checks `state.modules` absent, all meta/prop inputs gone,
  `create_preset_from_state` has no stale module entry, and both
  `REMOVE_ACTIVE_MODULE` and `groups/REMOVE_GROUP` deltas fire — end to end.
- **The JS cleanup chain is matched, not weakened** — JS calls
  `modules/removeActiveModule` per module and the engine does the same, now
  including Input-Link/`REMOVE_INPUT` ordering for mirror replay.

### Findings

- **[P3] No third-party/e.g. `zune-jpeg` texture type check** — the
  pre-decode probe is `image_dimensions`, which supports the common formats;
  a valid-yet-unsupported header makes the probe error and the file is
  refused as "could not read the image dimensions" — fail-closed, noted.
- **[P3] `remove_tween` pushes unconditionally next to `release_group`** —
  duplicates are possible if both paths drop the same tween; idempotent
  mutation, noted.
- No P2. Both concrete fixes from the outstanding list (decoder bomb, the
  removeGroup module leak) are landed honestly with regression tests.

## Round 60 — `0dd577b9..060e2bfb` (colour, vector and tween inspector controls)

**Native Inspector closes three of its largest gaps. All dispatch paths
verified end to end against the engine.**

What's now available to the native Inspector and how it behaves at the
engine boundary (each checked by reading, not executing):

- **`ColorControl`** (`crates/modv-ui/src/widgets/color_control.rs`) — the
  `{r,g,b,a}` ratio-object shape round-trips to the legacy 2D module props
  that the engine reads with `rgba_from_object` (Ball/Line/Polygon/Waveform
  etc. — all declare `color_unit`/`color_255` defaults in
  `crates/modv-renderers/src/two_d/modules/mod.rs`). ISF `color` inputs are
  synthesised as `Vec4` in the engine, so those render via VecControl
  below — the two prop_type paths coexist by design, no shape ambiguity.
  The `ratio_to_color`/`color_to_ratio` pair is unit-tested within one
  byte.
- **`VecControl`** (`vec_control.rs`) — X/Y/Z/W `DragValue`s with lane
  padding/truncation; `color` arrays reach the engine's `json_vec4`/
  `json_vec2` readers unchanged; **coerce pass-through for arrays is the
  branch the ISF path needs** (`props.rs` array passthrough at
  `coerce_prop_value` line ~96).
- **`TweenControl`** (`tween_control.rs`) — the full JS `Tween.vue` /
  `TweenControl.vue` flow replicated: builds the descriptor with
  `tansaction` data `[[min], [max]]` (an actual improvement over the JS's
  always-`[[0],[1]]`), easing (12 penner names — all in the Rust port),
  direction, `useBpm: true`, `bpmDivision`, `loop`, `steps`, and dispatches
  `tweens/createTween` (upsert by id — same semantics as legacy
  `dataTypes/createType`) followed by `inputs/createInputLink` with
  `type: "state"`, `source: "tween"`, `location: "tweens.tweens['{id}'].value[0]"`,
  `min: 0, max: 1` — **byte-identical in shape to the JS `makeLink`**
  (`components/InputLinkComponents/Tween.vue:77-83`) and the JS defaults
  (`inputs.js` `min = 0, max = 1`). The remove path dispatches
  `tweens/REMOVE_TWEEN` + `inputs/removeInputLink` — legacy's removeLink.
- **prop read model** (`model.rs`, `native.rs`) — `PropView` now carries
  `input_id` (the prop's `$props` input id) and `tween` (the linked tween
  descriptor, keyed by id from `state.tweens`), so the Inspector can address
  the real Input Link for every prop, not just guess.
- **"event" actions** (one click buttons, same `updateProp` payload),
  **"palette" swatch strip** (read-only array preview, correct choice), and
  **"texture"/"font" reach-over displays** (summary text from `type`) all
  wired with no new wildcards.
- **Already-observed deltas vs the legacy are improvements, accepted**: the
  egui editor's initial tween defaults (easeInOutSine, bpmDivision 16) are
  different but sane, and its min/max data range respects the prop range.

### Findings

- **[P3] `TweenView.frames` is populated (`t.frames.len()`) but unused** —
  no UI signal exposes it; dead model field today, harmless.
- **[P3] The `type: "palette"` chip still shows swatches even for
  non-palette arrays** — any array prop with type `palette` renders as a
  colour list (only `color`-like entries produce sensible swatches);
  cosmetic, matches the legacy PaletteControl's role.
- **[P3] PropView carries `input_id` even for `text` props** — those never
  produce tween UI (harmlessly ignored); the read model is shape-superset
  and uniform — deliberate, good.
- **[P3] `tweens/createTween` is called for every easing/steps/loop edit**
  (not debounced) — same upsert behaviour as the legacy editor path
  (`manageLink` → `createTween`); correct, not a perf problem at three
  dispatches per drag.
- **No P2.** Not running tests (no local toolchain); claims verbatim.

## Round 61 — `060e2bfb..73a697ba` (texture picker, group controls, shortcuts)

**The native Inspector closes its remaining prop-editor gaps (texture
picker, group inherit/clear/pipeline, alpha/rename/reorder/remove, module
level enable/remove/duplicate/reorder) and gains global shortcuts. Verified
feature-by-feature at the dispatch table.**

Verified by reading:

- **`TextureControl`** (`crates/modv-ui/src/widgets/texture_control.rs`) —
  the engine-id source list matches the engine's `TextureRequest` grammar:
  `inherit|group|canvas|webcam|image|video|screen` kinds; source ids are the
  engine's own (`group:`, `isf:`, `camera:`, `video:`, `screen:`) so presets
  round-trip byte-identical with the JS "inherit/options.id" shape
  (`modules/updateProp { value: { type, options: { id | path } } }` — the JS
  TextureControl.vue writes the same shape; verified at
  `renderers/isf/...`'s `TextureRequest::from_value` from Round 38. The
  "image"/"video" kinds accept a path typed into the field, unresolved
  values keep the group canvas — consistent with the engine's rules.
- **Group controls** — rename via double-click with a per-group `name_draft`
  (lost_focus + Enter commits; trims; empty rejected; no-op name skipped),
  alpha/inherit/clear/pipeline checkboxes through `UPDATE_GROUP_BY_KEY`
  (the three bool keys are the engine's `bool_value` coercing arms verified
  at `store_ops.rs:750-840`), move up/down through `groups/orderByIds`
  (existing verified arm, swapped ids), remove via `groups/removeGroup`
  (new teardown from Round 59 — no module leaks on this path), the
  `compositeOperation` combo offers the expected modes **except one entry
  that does not exist** (below), module lists gain enable/remove/duplicate
  (`groups/duplicateModule` — Round 32 verified)/reorder
  (`groups/REPLACE_GROUP_MODULES` — engine arm present).
- **Module row** — enable checkbox (`modules/updateMeta { metaKey: "enabled", value: bool }`
  — accepted by the namespace arm), remove dispatches `REMOVE_MODULE_FROM_GROUP`
  + `modules/removeActiveModule` in one gesture (the legacy two-step), the
  up/down reorder produces the full ids vec swap; the same reordering
  discipline as the module list, verified through the engine's
  `replace_group_modules`.
- **Ctrl+S** → `UiCommand::SaveSnapshot` → `--state` persist (no installer
  dialog in P4 — intentional), **Ctrl+Enter** → apply selected/current/first
  Scene; **Ctrl+K** → focus Gallery search; **Ctrl+1..9** → apply Scene
  column N and select it; **F1** → Shortcuts window; **Escape** closes
  About/Shortcuts/rename/edits (but the output Window's Escape-fullscreen
  toggle happens at the winit layer, not here — no conflict); **Ctrl+drag**
  unchanged semantics. The typed-in text focus guard (`*typing*`) stops
  Ctrl+Enter/digit bleed; Escape does not check `typing` (deliberate — the
  legacy app uses Escape for commit/cancel everywhere).
- **The `groups/removeGroup` arm at HEAD is the Round-59-corrected
  teardown** (no leaked modules into presets; tween descriptors cleaned).

### Findings

- **[P3] The blend combo offers `add` as a mode — it is not a valid engine
  token** — the list has normal/screen/multiply/…/luminosity/**add**, but
  `BlendMode::from_str` has no `add` (canvas's additive is `lighter`;
  engine equivalent is `lighter`, not `add`). Selecting it writes
  `compositeOperation: "add"`, the engine warns once and falls back to
  normal. The JS menu doesn't list `add` either. One-word fix; or add
  `"add" => BlendMode::Lighter` as a legacy-accepted alias at the parser.
  Either works; do one before shipping P8.
- **[P3] The texture picker is populated but empty in dev builds** —
  `snapshot.texture_sources` exists; the producer lives in the working
  tree (Group/module-output texture ids wired to it from a follow-on
  commit), unreviewed at this HEAD — until it lands, the picker's combos
  are empty of sources. The commit message's "populated by the shell from
  live groups and module outputs" describes the in-flight state, not this
  commit's state — same claim-vs-code class as the docs rounds; the loader
  state is working tree, not HEAD.
- **[P3] `Ctrl+K` puts the cursor in the search but Ctrl+1..9 apply Scene
  columns without guarding the F1/etc.-case order** — minor, no impact.
- **[P3] The duplicate-hotkey (`D`) acts on `modules/duplicateModule`
  which duplicates into the same Group** — this is the legacy behaviour too.
- No P2. The picker round-trips to existing paths and will work once the
  producer lands (present in working tree now, reviewed when committed).

## Round 62 — `73a697ba..10562230` (docs: native full-rewrite tracker)

**New doc `docs/rust-full-rewrite-progress.md` — mostly accurate; one
ground-truth miss worth pinning.**

Honest on every point I checked, including the awkward ones, and
explicitly attributes in-flight/review-driven fixes to open work:

- P0 is honestly listed as "verification pending", P0 evidence items (the
  release/lavapipe numbers, RSS, the written go/no-go) are named.
- P3/P4 gaps (palette editor, font registry, video transport, MIDI/NDI/BPM
  panels, Search, plugins, docking) are all named out honestly.
- "Review fixes in flight: Scene cell capture/clear, smoothing on Scene
  links, image decoder header probe, removeGroup module release, stale
  support/docs claims, ISF category aliases, expression sandbox hardening"
  — accurately names the Round 31 (items 28/29), Round 35 (item 30),
  Round 38 (decoder bomb), Round 59 (removeGroup), Round 58/59 (stale
  docs/ISF aliases/\n), and the in-flight expression work (currently
  uncommitted in the working tree from the author).
- "Electron remains default. `MODV_UI=native` is opt-in. Preset formats
  unchanged. modv-ui is `#![forbid(unsafe_code)]`." — all accurate at HEAD,
  verified.
- Test counts quoted verbatim ("cargo test -p modv-ui: 25 unit + 3
  kittest") — unverified here.

### Findings

- **[P3] The doc says `window_state.rs` "reads/writes the same
  `<userData>/window-state.json` as the Electron app, with atomic writes
  and the JS normalisation rules" — no such file exists at this HEAD.**
  It is the untracked file sitting in the working tree right now
  (`crates/modv-app/src/window_state.rs`, unreviewed yet); the doc lists
  it as done before its commit. Same claim-vs-code class; the file itself
  is next in the queue and will be reviewed on arrival.
- **[P3] Known-open item confusion:** the doc's "P0 — framework spike:
  Lavapipe/release numbers to record" is correct; the "ya bold" different
  thing is that the P0 checklist within the app def reaches its \`GO\`
  verdict from the previous rounds' hand-stamped flags
  (`shared_texture_ok` etc.) — the doc's honest framing partially covers
  this.
- No P2/P3 fixable in the doc other than naming the files/milestones by
  commit once they land.

## Round 63 — `10562230..ab85acc8` (focused-input link editor)

**The Inspector gains the focused-input link drawer the legacy InputConfig
only shows for the selected control. All widget logic is tested; the engine
dispatch grammar is verified — but this commit, too, ships the plumbing
ahead of the paint.**

Verified by reading:

- **`link_row` renders per prop with an `$props` entry** — the shell draws
  the row for every prop with `prop.input_id` (Round 60). Widget tests pin
  audio default/feature round-trip/error args/state-link non-args modes.
- **`link_editor_window`** offers three presets (audio, state, MIDI, OSC),
  full kind/source/location/args editors, drag min/max, and "Save link" /
  "Create link" variants. The six `updateInputLink` dispatches per edit
  match the engine's per-key semantics plus its deserialize-revalidate
  behaviour — **a type/args combination the engine can't deserialize fails
  silently to the native UI** (returns Null); the same shape the JS has,
  noted, not fixed here.
- **`LinkEditor::audio`** — kind=getter, source=meyda,
  location=meyda/getFeature, args=["bass"], min 0, max 1 — **exactly the
  JS's `createInputLink` defaults** (`inputs.js:140-145`); FEATURES list
  matches the JS control (bass/mid/treble/beat/kick/energy/spectralCentroid/
  spectralFlatness/zcr); `set_audio_feature` rewrites kind/source/location/
  args consistently for all three callers.
- **The '+ link' action pathway** (editor opens fresh; Save dispatches
  `inputs/createInputLink` with the JS payload shape)
  works end-to-end through the engine's existing arm — verified at
  `store_ops.rs` (`createInputLink` arm and its `{inputId, type, source,
  location, args, min, max}` merge shape).
- **Tween links** — the editor's "tween" source-type links stay working
  through this path too: removing a tween link falls back to the engine's
  `remove_tween` + `tweens/REMOVE_TWEEN` via `inputs/removeInputLink`,
  regardless of who dispatched it (Round 36 code).
- `LINK` accent row renders only when an `$props[prop].id` exists (same
  gate as the tween row).

### Findings — commit-vs-code mismatch (second instance of the same shape)

- **[P3] `snapshot.input_links` is never populated at this HEAD** — the
  commit message says "InputLinkView joins the snapshot so the UI reads
  the real link record", but **`native.rs` is untouched** and
  `EngineSnapshot.input_links` is the default `HashMap::new()`. The link
  row's summary branch is dead code at this commit; every prop always
  shows "+ link"; existing links never show and can't be edited or removed
  via the UI. The populator exists in the working tree (uncommitted —
  `let input_links = state.input_links…` already pending commit), matching
  `texture_sources` from Round 61 — **a two-instance pattern of a commit
  shipping plumbing before its paint**. The commit message claims the
  payoff; the paint is one commit behind. Cosmetic today; the feature as
  advertised does not work until the populator lands. (Same claim-vs-code
  class as Rounds 45/47/58/61/62.)
- **[P3] No sub-input link rows** — vec2/3/4 sub-inputs have no link rows
  (outstanding item 9); the top-level links work through the same path.
- **[P3] The 6 `updateInputLink` dispatches are not transactional** — a
  half-applied update is possible if the engine rejects one mid-sequence;
  matches JS, noted.
- No P2.

## Round 64 — `ab85acc8..b765b5c4` (media panel, window placement, link snapshot)

**The native shell closes the last two plumbing-before-paint gaps and adds
the media capture panel and window placement persistence. Everything here
is verified by reading — but two honest gaps remain behind the headlines.**

Verified by reading:

- **`input_links` snapshot populated** (`native.rs:1046-1060`) — the
  previous round's advertised branch now works: every prop row shows the
  real `LINK` summary, Edit and x buttons resolve, and the editor keeps
  its existing shape. Second instance of the Round-61 claim-vs-code gap
  fixed; the first (`texture_sources`) was fixed in the same commit
  (`native.rs:1039-1065`) — the Camera + ISF/2D/Point10 module outputs and
  Group canvases now enumerate into the picker as advertised in Round 61.
- **`window_state.rs`** is a faithful, atomic port of the legacy
  `window-state.js`: same `version: 1` envelope, `mainWindow` + `windows[key]`
  slots (`output-0`), `displayId` serialised `null`-compatible, `is_usable`
  guards, `max/fullscreen` all sanitise-validated on read; writes go tmp
  + rename; clamp/fits/intersection helpers are exported for offline use.
  The native shell applies saved placement at startup (with
  `cli.fullscreen` override) and re-debounces on move/resize/close (1 s
  flush window; shutdown always flushes).
- **The `VmodV/window-state.json` path is the same file the Electron app
  writes** — `MODV_USER_DATA` first, `XDG_CONFIG_HOME/VmodV` on Linux,
  `~/Library/Application Support/VmodV` on macOS, `%APPDATA%/VmodV` on
  Windows, `.config/VmodV` fallback. A switch between Electron and native
  keeps the main window where the user left it — the piece the legacy
  `prepareMainWindowPlacement` does, now without the Electron display-id
  handling.
- **Media panel** — the `Media` tab of the right side switches to a camera
  list + running sources list with per-source `x` Stop buttons, a Test
  Pattern Start button, a camera expose from native `V4L2` enumeration
  (the same enumeration the hybrid engine uses — metadata nodes filtered
  out, `VIDEO_CAPTURE`/`MPLANE` modes only), each pushed into
  `MediaHub::start()` with `with_mode(1280, 720, 30)`. Frames sync into the
  compositor via `compositor.sync_media(&mut self.media_hub)` **before**
  `render_frame` — the same order as the hybrid window loop, so the
  Webcam module draws this frame's capture.
- The `snapshot.media` is rebuilt per frame, populated fully (cameras,
  statuses, error, availability) — the Media tab binds cleanly.
- Link snapshot builds from `state.input_links.values()` — kind renders the
  lowercase `InputLinkKind` (getter/state/mutation), args serialize as an
  array; all values present.

### Findings

- **[P2] The native shell is deaf: no audio in native mode, ever.** No
  `AudioFeed` is constructed (the hybrid `WindowedApp` does this at
  `window.rs:338`; nothing in `native.rs` ever calls
  `crate::audio::open_feed`); the CLI has no `--audio-source` hook into
  the native code path; the renderer that would `meyda/SET_BANDS` obviously
  doesn't exist in native mode. So `Store::audio()` is forever the silent
  initial frame: every Input Link evaluates to 0, the bass-linked default
  Scenes are static, the BPM readout stays 0, the kick indicator never
  fires, and Input Links evaluating `beats.kick`/`beats.bpm`/
  `fftCells[...]` read silence. The status bar honestly reports
  `audio: silent` (the health probe correctly returns `audio_active=false`)
  — the bar is honest about the deficit but honesty is not a fix. The
  in-process engine claim of P2 still requires routing *any* audio source
  into the native shell — no bigger lift than the PipeWire/cpal wiring the
  hybrid path already runs; flag as the largest remaining functional gap in
  native mode and a blocker for the "native: P2 — engine in-process"
  checkbox in the rewrite tracker.
- **[P3] `list_cameras` opens every UVC device and enumerates modes every
  frame** — `snapshot.build_snapshot()` runs per frame; the V4L2 enumeration
  (`fs::read_dir` + `Device::with_path` + `query_caps` + `enum_formats` +
  `enum_framesizes` per camera) is 50–500 µs × the camera count every
  frame, paid 120 times per second. Requires a ~1 s cache (or `nobody asked
  twice` rate-limiting) — easy fix, matters on the 120 fps path.
- **[P3] `window_state.rs::clamp_bounds_to_area`/`fits_within`/
  `intersection_area` are exported but never called** — window persistence
  writes un-validated bounds (a window saved on a 4K display restored on a
  1366×768 laptop will hang open off-screen, invisible to the user); the
  JS `resolveWindowBounds` does the display-clamp at show time, which is
  not ported. The port brought the file format and the write path, not
  the recovery rules; one `--fullscreen` flag saves the case but it
  requires the user to know about the failure.
- **[P3] Native remembers only `output-0`** — `WindowState.windows` is
  `BTreeMap<String, SavedWindow>` and the native shell always writes
  `output-0`; the JS saves `output-1`, `output-2`, … as new output
  windows open, so the first output takes the next free slot. Native
  support is single-output; fine for P4 but flagged.
- No P2 beyond the audio gap above. **The populators landed for both
  previously flagged plumbing-before-paint gaps (Round 61/63); both are
  now closed by reading.**

## Round 65 — `b765b5c4..e28892ff` (tempo source/tap and status meters)

**The native status bar surfaces the tempo pipeline it never had, plus an
FPS sparkline and bass/mid/treble meters.**

Verified by reading:

- **The BPM window is the JS's behaviour mirrored exactly** — source combo
  emits `beats/SET_BPM_SOURCE` (verified earlier: `setBpm` only applies
  from the picked source), `Tap` collects strikes over a 2-second rolling
  window, promotes the source to `tap` and dispatches
  `beats/setBpm { bpm, source: "tap" }` on the flip — the Rust `clockHistory`-
  compatible 2-second window and `60/avg` beat math are faithful to the JS
  tapTempo path. `(30, 300) BPM` clamp + finite filter preserved.
- **The rolling FPS sparkline and meters paint no new dependencies** — 13 px
  tall indicator bars for bass/mid/treble, `[0, 1]` clamped, BSS label in
  the bar, `fps_history` capped at 120 entries (a rolling window at 60 fps),
  no external crate added.
- The engine's audio still routes through `Store`; the analysis frame is
  silent in native mode (see Round 64's P2) — the meters will paint 0
  forever in this build — the pipeline would work if the audio source were
  wired; the wiring is [P2], Round 64.
- No panics/unsafe; the status bar uses `rect.height().max(0.5)`
  for remaining draw calls; `FPS` sparkline folds to 60 when the history is
  empty and needs ≥ 2 entries to paint.

### Small findings

- **[P3] Tap-tempo with fewer than 2 taps in the window** does nothing — if
  the user taps twice quickly (a single beat) and nothing third happens,
  the window empties silently after 2 s (`tap_times.retain` + one push
  then empty adhere to the 2 s window) — correct UX, no flags.
- **[P3] Tap can create a stale `source: "tap"` selection with no recent
  taps** — the promoted source stays `tap` while `Tap` is never tapped
  again; `setBpm` applies from `tap` but never receives another write — so
  a user who taps once then forgets sees the BPM stuck at the last tap
  value while the window drains (and the status bar shows it forever). An
  "expire `tap` after 2 s of silence" step (like the JS's window) would
  close the leak; noted, not blocking.

## Round 66 — `e28892ff..bda55128` (clearSceneCell / captureSceneRowToScene / smoothing links — three Round-31/35 findings close)

**Three pre-registered findings from Round 31 and Round 35 land with
regression tests.**

**1. `scenes/clearSceneCell` now empties the live Group [item 25 closed]** —
the old code removed the live Modules from `state.modules` but never rewrote
the live Group's `modules` array, so the ids became phantoms in paint AND in
the next `captureSceneRowToScene` serialization. The fix walks `live_module_ids`
(the live Group's own ids, not the document's saved-id set), removes each, and
dispatches `groups/REPLACE_GROUP_MODULES { modules: [] }`, mirroring the JS
tail. Regression test (`clearing_a_scene_cell_empties_the_live_group_and_its_capture`)
asserts: live Group modules is empty, each removed id is gone from
`state.modules`, and a post-clear capture round-trips an empty cell with no
stale `doc-ball` snapshot resurrected.

**2. `scenes/captureSceneRowToScene` only captures a stamped Group [item 26 closed]** —
the positional `find_live_row_group` fallback is dropped; a Group not stamped
with this Scene (transitionGhost included) returns null. The regression test
(`capturing_an_unowned_row_returns_null_and_leaves_the_scene_alone`) seeds
Scene A (`alpha`) and Scene B (`beta`), applies A, and asserts that a capture
against B with row 1 is a strict no-op. This is exactly the
`scenes.js:3528-3530` semantics.

**3. Default Scene bass links mint smoothing ids [item 27 closed]** —
JS `createBassLink` reads `link.smoothing` and calls `meyda/getSmoothingId`
(`addSmoothingId` seeds the id at `0`), widening `args` to
`[feature, smoothingId, smoothingValue]`. The engine had the parser
(`get_feature_smoothing`) but the mount never reached the link builder, so
the v15 scenes shipped smoothing: 0.6 linked to nothing in the Rust build.
The fix threads a `smoothing: Option<f64>` through `create_bass_link`, pushes
the minted uuid into the opaque smoothing registry (`register_link_smoothing`,
seed value `0.0`), and only widens `args` when the feature is usable.
Regression test (`default_scene_links_keep_their_declared_smoothing`)
asserts:

- all 3 v15 smoothing links materialise `args: [feature, smoothingId, amount]`;
- `smoothingId` is a 36-character uuid-like string;
- `args[2].as_f64() == Some(amount)` where amount is the declared 0.6;
- `store.link_smoothing_value(smoothing_id) == Some(0.0)`;
- an automatic bass link (no declared smoothing) still emits the legacy
  `args: [feature]`.

Also small and notable: `build_default_scene_document` now returns a
`BuiltDefaultScene` (document + smoothing ids collected during link build) —
the seeds are registered before the first Scene builds rather than lazily.

### Notes

- None of the three regression tests can run without a corpus — the links
  targeted are the roughcorpus-body ISF ports (Hot Shower, Traced Tunnel,
  Helix Eye) — the test **verifies the link builder with the real ISF module
  set present** and skips when the repo is outside the checkout; standard
  gate accepted.
- The v15 smoothing links in the engine are now behaviour-equivalent to the
  legacy worker's — the calming behaviour shipped in `1240db0c` is finally
  visible in engine mode.
- The `uuid_v4()` minted smoothing ids interoperate with the smoothing
  registry (the same `link_smoothing` map the input links evaluate through) —
  the link target instantiates the tween animation instance; no additional
  cross-talk.

## Round 67 — `bda55128..a28162d1` (Ctrl+K command palette)

**Most visible product gap of this cycle closes: the Ctrl+K palette is real.
One functional defect and one UX sequencing misfire.**

Verified by reading:

- **Palette contents are an honest list** — performance toggle, create
  Group, add empty Scene, save snapshot, open/close/toggle output window,
  show shortcuts. Scenes are listed (`apply scenes/applyScene`), Gallery
  modules up to 80 from the current Category when a Group is selected, both
  actions through the UI composite `add_module_to_group` (verified — calls
  `modules/makeActiveModule` → `groups/ADD_MODULE_TO_GROUP`), same pipeline
  as Round 57/58/64.
- **Keyboard nav** — ArrowUp/ArrowDown select, clamped to the list; Escape
  cancel; click/Enter runs; the command input gets focus when it opens
  (modulo below). Filter is a case-insensitive substring match — honestly,
  that's not fuzzy; it's substring. The label calls it fuzzy — call it a
  nit, not a bug.
- **Engine arms used**: `scenes/setPerformanceEnabled`,
  `scenes/addEmptyScene`, `groups/createGroup`, `scenes/applyScene` — all
  resolve. `UiCommand::SaveSnapshot` runs the same `--state` flush; open/
  close/toggle fullscreen execute the same `open_output`/`close_output`/
  `toggle_output_fullscreen` as the menu buttons.
- **Ctrl+K clears query + selection** — fresh open each time.
- **Flag-gating** — the numeric shortcuts do not fire while typing
  (`typing` guard remains); Ctrl+Enter with the palette closed still applies
  the scene.

### Findings

- **[P3] `Show keyboard shortcuts` dispatch is a dead command** — the
  palette pushes `PaletteAction::Action("ui/showShortcuts", …)` →
  `state.push_action("ui/showShortcuts", …)` → `drain_ui_actions` →
  `self.store.dispatch` → engine catch-all warn-once → **nothing visible
  happens**. Not dispatched anywhere in the engine; the JS process maps
  this menu item to the SubMenu — the native menu has a real handler
  (`Help → Keyboard shortcuts`) and the palette should set
  `state.show_shortcuts = true` directly (or just reuse the same flag)
  rather than dispatch into the engine. Every press prints a spurious
  `engine: WARN … unknown command ui/showShortcuts` on stderr exactly once
  per session.
- **[P3] `Ctrl+K` steals the wrong input focus** — `handle_shortcuts` sets
  the shared `state.search_focus = true`; two consumers race on it in
  frame order: the Gallery's search TextEdit (request_focus at
  `shell.rs:1532`) and the palette's TextEdit (request_focus at
  `shell.rs:1790`). The Gallery wins because `central_panel` runs before
  `command_palette` — so **Ctrl+K opens the palette with the cursor in the
  Gallery strip's search field**, not in the palette. Different flag —
  or fallback to `palette_focus` and consume it before `search_focus`.
  Easy one-line fix.
- **[P3] Ctrl+K doesn't close if already open** — reopening while open
  re-opens/re-focuses; Ctrl+K toggles (`state.show_search = !state.show_search`)
  would match the muscle memory of other palette interfaces; non-issue.
- No P2.

## Round 68 — `a28162d1..6fd8352c` (native shell golden snapshot)

**A before-the-fact visual regression net for the native shell, using the
exact panel against a pinned engine snapshot — the P0 visual evidence.**

Verified by reading and by opening the committed image:

- **`tests/shell.rs`** builds a full representative `EngineSnapshot`: one
  Group (Foreground, `compositeOperation: "screen"`, so the theme's screen
  highlight is exercised), one Module (Chromatic Line, two props — a float
  slider and a bool), one Scene ("Lone Chrome" × two rows), two registered
  modules (Effects and 3D category), audio at 128 BPM / bass 0.42 /
  mid 0.31 / treble 0.18, FPS 60 at resolution 960×540, `output_open: true`,
  `audio_active: false` (correctly silent — native is deaf, see Round 64).
  `UiState` pre-selects group/module/scene. Harness builds at 1440×900.
- **The committed PNG `tests/snapshots/shell.png`** (150 KB, 1440×900)
  renders exactly that: Groups panel top-left with the selected Group, its
  module row ("Chromatic Line", togglable via checkbox — visible enabled),
  full control row (Include/Clear/Pipeline checkboxes, blend dropdown
  `screen`, Alpha slider), the caption area showing "waiting for the
  composited frame"; P0 CHECKLIST all green with a `GO` badge; RANGECONTROL
  PROBE `gain 1.25 R`; Gallery strip with two registered modules; Scenes
  panel `Lone Chrome` with the 8s timing badge; the right Inspector with
  `enabled` + gain 1.25 + "+ link" (real Input ID proves input_id wiring
  Round 60); the status bar shows 16.6 ms / 960×540 / 120 frames /
  `audio: silent`; the menu bar has all five menus populated.
- **The snapshot convention is the egui_kittest snapshot + wgpu feature
  path** — first run writes `tests/snapshots/shell.png`, later runs compare
  pixel-for-pixel. A committed image is a fixture (changed only via the
  re-baseline workflow); this is correct discipline.
- **`modv-ui` dev-deps moved from `egui_kittest` to `egui_kittest` with
  `snapshot` + `wgpu` features** — required for the comparison. Lockfile
  diff is matching.
- `Cargo.lock` grows ~125 lines of egui_kittest snapshot transitive tree —
  expected churn.
- **`audio_active: false` in the fixture stays consistent with the
  deaf-native-shell finding** — the snapshot intentionally does not claim
  audio output it doesn't have.
- The image shows no defects in layout: chrome, alignment, badge text,
  dropdown glyphs, and the status bar all render as designed.

### Findings

- **[P3] The P0 `GO` badge is rendered but isn't derivable** — live state is
  pre-set to all-true (`range_control_ok`/`scene_cell_ok`/…
  `output_window_ok` `fps == 60`), so the image says GO by fiat. That's
  honest enough as a golden fixture for "everything green" but tells no one
  whether the real app reaches GO — the reviewer/engineers who wish to use it
  as a self-verification signal must keep this in mind.
- **[P3] Snapshot covers only the "all healthy" state** — no failure cases
  (empty state, no groups, no scenes, missing audio everywhere, engine
  disconnected). A second fixture would pin the failure path too.
- **[P3] The `shell.png` is the only visual evidence — P6/P9 won't be able
  to change panels without regenerating the screenshot every time** —
  deliberate, and keeps review diffs honest, but every layout change now
  needs a deliberate re-baseline commit.
- **[P3] snapshot harness has no explicit size guardrail** — first-run
  writer will regenerate the PNG if its dimensions drift, but the current
  harness pins the two tests to the same facade and to the hard-coded tag
  `shell`; an accidental duplicate test named `shell` would silently
  overwrite.
- No P2.

## Round 69 — `6fd8352c..594ea38e` (docs: native UI section in AGENTS.md)

**Docs-only commit; every claim byte-checked against the tree at this HEAD.
All accurate; one honesty gap flagged.**

Verified by reading `AGENTS.md` against the Makefile and the tree:

- `make native` passes `$(GUI_RUN_ARGS)` (PulseAudio socket + `--device
  /dev/dri` forwarding via `DRI_ARGS`/`GUI_AUDIO`) — the "same GUI args as
  `make dev`" claim is accurate; `make native-debug` uses the debug profile
  and the flag shape matches.
- `MODV_UI=native make dev` → `dev-native` → `native` confirmed in the
  if/else block (`Makefile:154-169`); `make dev` without it stays on Electron;
  `MODV_ENGINE=1` still selects the Rust engine inside Electron (`ENGINE_MODE`
  gate preserved). Cross-checked both switches are orthogonal.
- `modv --headless` / `--screenshot` / `--state` / `--isf-dir` share the
  exact `headless::run` / `cli::parse` code paths with `modv-engine` — a
  genuine byte-identical refactor (`lib.rs::native_main` reuses the same
  branches).
- ``modv-ui` + `modv-app/src/native.rs` + `modv-ui` as a workspace member —
  accurate; `MODV_USER_DATA` override exists in `window_state.rs` (assert:
  `std::env::var_os("MODV_USER_DATA")` — verified) and `window-state.json`
  is read/written atomically to the same path as Electron — verified round
  64.
- `make engine-test ARGS="-p modv-ui"` — the ARGS forwarding checked in
  `engine-test: $(call run-cargo,test $(ARGS))` — works as documented.

### Findings

- **[P3] The section does not mention the native shell is deaf** — no
  `AudioFeed` is wired anywhere in `native.rs` (Round 64); Input Links read
  silence forever; the status bar reports `audio: silent`. A reader trying
  `make native` expecting audio-reactive defaults gets a silent app with
  bass-linked Scenes frozen in place. One line ("Native mode currently has
  no audio input; see progress-doc Round 64") closes the surprise.
- **[P3] "For P6 media manager: camera/test-pattern only, no file/screen yet"** —
  easy to miss in the not-yet-ported list; comes from Round 64's media panel
  without file/screen support. The section is honest (lists media manager
  under "not ported") but the camera-vs-file distinction is unnamed; fine.
- No code; no defects; the one P3 is a docs completeness improvement.

## Round 70 — `594ea38e..9d3c1fc4` (native audio restored, cameras cached, clamps)

**The deferred-medium commit: every P2/P3 from Rounds 61–68 lands here. Verified
by reading — the author read this review.**

**1. Native audio ([P2 closed])** — `NativeApp::new` opens an engine-owned
`AudioFeed` exactly as the hybrid window loop does (`window.rs:338`):
`open_native_audio(&cli)` tries the CLI audio-source first, then falls back
to `Pipewire` (system monitor) and `Input` (default device). On every frame,
`pump_audio(dt)` pumps the feed before Input Links evaluate and calls
`store.set_engine_audio(&snapshot, kick, elapsed)` — `engine_audio_active`
flips true on the first pumped frame; on feed error the store returns
`disable_engine_audio()` and the eprintln fires once; capture shutdown is
clean (`feed.stop()` before `shutdown_ndi()`). The native status bar now
truthfully reports `audio: engine` as soon as audio has been published —
the "deaf shell" is gone. The `native_audio_defaults_to_the_system_monitor_then_input`
unit test pins the fallback order.

**2. Camera enumeration cached at 1 Hz (R64 P3 closed)** — `media_cameras`
+ `last_camera_scan` (`Duration::from_secs(1)`); snapshot clones cached
cameras rather than calling `list_cameras()` every frame; the 50–500 µs × N
cameras × 120 fps case is gone.

**3. Window placement clamp to the primary monitor (R64 P3 closed)** —
`clamp_saved_to_monitor` runs `clamp_bounds_to_area` against the primary
monitor's logical coordinates — the save is always usable; the save/resize
cycle now also applies the clamp at startup for the main window and output
window. Offline `clamp_bounds_to_area`/`fits_within` are now used at window-
creation time.

**4. `add` → `lighter` blend token (R61 P3 closed)** — the native Inspector
blend combo now offers the real engine token `lighter` which wins via
`BlendMode::from_str`.

**5. Preview error re-report (R58 P3 closed)** — first failure warns, every
10th subsequent attempt warns ("attempt N"); fixed in both the read-back
byte-length path and the request error path; no silent permanent tightening.

**6. Palette shortcut/focus races (R67 P3s closed)** — `show_search` toggles
from Ctrl+K now (muscle memory); `palette_focus` is a dedicated one-shot
focus flag separate from the Gallery's `search_focus` — the palette steals
the cursor correctly, Gallery search untouched; the "Show keyboard
shortcuts" action is now an engine-bound `PaletteAction::ShowShortcuts`
handled inside the palette (sets `state.show_shortcuts = true`) — no more
catch-all dispatch, no stderr warn, palette command works.

### Notes

- Round 64's `[P2] The native shell is deaf` is now closed — every other
  open P2 falls out of this round clean. Any remaining talk of "audio
  silent" in the docs (including Round 64's round text) is now stale; the
  doc Round 64 section is historical and is not overwritten.
- The **`window_state_ignore` for valid but multi-monitor user saved bounds**
  remains possible: clamping to primary monitor is a conservative fallback
  and conflicts with multi-monitor setups saved with `is_usable`'s
  always-true. A maximised bounds check sits even closer to the legacy
  behaviour; the mount override is `with_monitor_id` if/when someone needs
  "restore to the exact display". Not a defect — a demo choice.
- The `start.checked_sub(Duration::from_secs(10)).unwrap_or(start)` pattern
  always succeeds because `checked_sub` saturates; the explicit `unwrap_or`
  is belt-and-braces.
- No new defects introduced. Two remaining unverified-but-realistic reads:
  `AudioFeed::frame(dt=0.0)`'s first call reports silence before any capture
  lands (the snapshot publishes zeros for its first frame), acceptable
  behaviour for the fallback-verification path.

## Round 71 — `9d3c1fc4..4f41c87c` (whole-scene shader transitions ported to native)

**The last documented native-gap closes — the native shell now runs Scene
transitions with the same frame capture, pipeline preparation and fallback
logic as the hybrid window loop.** This is exactly the sequence Round 56
claimed as deferred (P4 — "ports it with the Scenes panel" — the author
landed it now).

Verified line-by-line against the window loop's implementation:

- **`capture_pending_transition()`** — gated on
  `self.store.take_pending_transition_capture()`; calls
  `Compositor::capture_transition_source()` once per demand; on failure
  **falls back to Fade** via `store.fall_back_scene_transition_to_fade()`
  and logs a warn with the hint. Matches `window.rs` byte-for-byte; same
  three branches (no capture / lost capture / failed pipeline) all clear
  `compositor.clear_transition_capture()` and return None — identical
  behaviour to Round-46's window loop post-fix.
- **`prepare_scene_transition()`** — resolves the `SceneTransitionInfo`
  per frame: the transition's `shader_effect()` (Isf/Native) is
  `compositor.prepare_scene_transition`-d, `transition.progress()` drives
  `SceneTransitionInfo.progress` (same progress value used for the alpha
  ghost path — single timeline, consistent), `from_groups` collects the
  outgoing group ids.
- **Frame ordering** — the new ordering is drain_actions → capture_pending →
  sync_media → prepare_transition → render_engine, which is *better* than
  the hybrid: media sync now happens after the outgoing capture is taken,
  so a capture never races the media upload.
- **`render_engine`** takes an `Option<SceneTransitionInfo>` parameter;
  startup and resize paths call `render_engine(None)` (no transition),
  same as hybrid's Round 46 startup frame.
- **`frame_info`** now emits the full `scene_transition` (`Some(…)`) —
  the compositor's `apply_scene_transition` path (`compositor.rs:3063+`,
  Round 46) now fires for every transition the user triggers in native.

The row crossfade path (`RowFadeInfo` → `fades` → `compose_main`) was
already working; this round closes the **shader transition / ISF transition
path** — the running Scene now fades through its capture through the same
shader compiler that the hybrid uses. The ISF fallback-to-Fade guarantees
are preserved exactly.

### Findings

- **[P3] `prepare_scene_transition` takes the transition and re-clones it
  every frame** — `performance_runtime().transition.clone()` each frame;
  clone is small (ids + progress), fine.
- **[P3] `has_transition_capture` is called after `capture_pending_transition`
  already captured this frame** — double-check pattern; same shape as the
  hybrid. No issue.
- **[P3] `scene_transition` is `None` only at startup/resize — correct; the
  Round-56 deferral note is now stale** (the P3 note in Round 56's round body
  refers to this already-fixed state; the text is historical).
- No P2. The whole native transition path is wired and mirrored from the
  well-verified hybrid path; visual evidence (before/during/after a
  crossfade) is not capturable on this host, but the byte-order parity with
  `window.rs:1292+` is visible.

## Round 72 — `4f41c87c..590b1fe0` (`modv --apply-scene` for reproducible traces)

**A reproducibility aid disguised as a flag — the native shell can now
start with a real Scene already applied. Verified by reading.**

Verified line-by-line:

- **`apply_startup_scene()`** fires **after** default seeding in
  `NativeApp::new`, against the seeded scene list; lookup is exact-`id` OR
  exact-`name`; a `--apply-scene "New Scene"` that doesn't match emits a WARN
  and continues empty rather than panicking; after `dispatch`, a group list
  from the applied Scene is built and logged (info line; WARN on failure).
- **Unit test** (`apply_startup_scene_resolves_names_and_ids`) seeds defaults,
  applies by the first Scene's name, then by id; both paths yield non-empty
  groups — test-gated existence check (seeds defaults) protects the lookup.
- **Header comments say "Native shell only"**; the flag is inert in
  `headless::run` — defensive against accidentally making every engine run
  load a demo scene.
- ``--apply-scene <NAME>`` is wired through clap at low cost; the renderer
  doesn't need to know; it only affects `make native` traces.
- After Playwright-level P0 benchmarks become possible, the author can do
  `make native ARGS="--apply-scene 'Circuit Ritual'"` and get real image
  output+frame metrics instead of the empty shell.

### Findings

- **[P3] `--apply-scene` silently ignores the flag** in the legacy engine
  binary — the `modv-engine` path treats non-engine flags as unrecognised
  silently (clap treats unknown args as an error) — wait, clap `error-on-missing`?
  The engine's CLI **does** distinguish unknown args; `modv-engine` would
  abort — but here `--apply-scene` exists in the *shared* Cli struct, so both
  `modv-engine` and `modv` accept it; only the native binary consumes it.
  That is honest and matches how `--bench-frames` publishes.
- **[P3] No `--list-scenes` query** — a missing check on the flag (user must
  know the exact name). A trace flag isn't aimed at users; noting, not
  actionable.
- No P2.

## Round 73 — `590b1fe0..edd93b55` (Info and Plugins side tabs)

**The right pane gains two honest information tabs. Doc-comment/reflect-on-state.**

Verified by reading and by rendering the snapshot:

- **INFO panel** — labelled fields: adapter, backend, frame (avg frame_ms
  + measured FPS), output resolution × frames, audio (``engine capture`` or
  ``silent``), tempo (BPM from snapshot.audio.bpm), Scenes (`{n} saved /
  {m} rows`); `modV Native Rust v0.1.0` identity; link to the shared VmodV
  userData tree is accurate (the native shell does read/write the same
  presets/Scenes/matches playlists — a claim verified at behavioural level,
  not just asserted); the last engine error renders if one exists, otherwise
  "No engine errors this session." Nice: the determinism of the demo goes up
  since this panel reads exactly from the snapshot's health block.
- **PLUGINS panel** — honest disclosure-style text explaining D5 plugins
  remain not-yet-landed ("the native shell hasn't landed the sandboxed Boa
  host yet"), with the checkbox disabled with the reason shown inline
  ("Enable JavaScript plugins (advanced)" ⬜); the grab-canvas entry is
  also disabled. No footguns, no fake switches.
- **`SidePanelTab` gains `Info` + `Plugins`**; `Default::Inspector` is
  preserved; the layout matches MEDIA/INSPECTOR.
- **Snapshot re-baselined** to capture both tabs; small 151 KB
  `tests/snapshots/shell.png` rewrite.

### Findings

- **[P3] The Info panel's `audio_active` reports only "silent"** for the
  current state of native pre-pipewire-mapping (Round 70 fixed the actual
  deafness — info must follow the same switch); the label is technically the
  same store signal but the demo captured before pipewire resumes —
  not a defect; will self-correct.
- **[P3] `Rust v0.1.0` is hard-coded from `CARGO_PKG_VERSION`** — the
  workspace package version is fixed until release; fine.
- **[P3] Plugins checkbox `add_enabled(false)` disables legitimately but is
  not communicated anywhere else** — the info-about text is visible but the
  Plugins panel renders as a child list with no explanatory hover.
- No P2.

## Round 74 — `edd93b55..71f707ad` (docs: P0 release/lavapipe evidence recorded)

**Honest P0 write-up. All claims match verified tree state.**

Doc claims verified by reading (build not run here):

- The claim "audio source pipewire owns capture at 48000 Hz" is consistent
  with the fix landed in `9d3c1fc4` (`pump_audio` does always log exactly
  that line — checked verbatim); the 138–143 fps / 56.9 fps at 960×540 on
  lavapipe claims are plausible given the native shell is now ~quiet (audio
  in, no IPC/extern calls per frame except winit presents).
- The note "a full `modv-app` suite not yet green in this tree yet —
  expression sandbox hardening is in flight" was accurate at the time and
  is the last self-check the author ran.
- P0 ratchet holds: exactly one caveat (lavapipe 57 fps vs 60 target),
  GO on the reference GPU.
- The "~157 MB RSS" claim vs the Electron multi-hundred-MB floor is
  the author's measurement (will not assert here).
- The progress doc says "no IPC, no Vuex mirror, no preview ring on this
  path" — true of `native.rs`.

### Findings

- **[P3] Progress doc still describes the sandboxed expressions as "a
  denylist rejects native allocation/loop/context escapes"** — `b84553ac`
  landed after this commit and replaced the denylist with a Boa-parser AST
  allowlist (see Round 76). The doc text is already stale the moment the
  fix merges, in exactly the self-contradiction class I flagged in
  Rounds 45/47/58/61/62/63.
- **[P3] The `62 seeded Scenes` mentions (here and in the parity map and
  the prior round)** are stale relative to the v16 seeds — the engine
  seeds them but I have not counted at this HEAD; not material for claims
  but worth the next docs pass.
- No code; no new defects.

## Round 75 — `71f707ad..f6d88439` (docs: first native parity map)

**A genuine P8-prep map — accurate, honest about gaps, actionable.**

Verified by reading against the tree at HEAD:

- Every legacy spec group appears with a status letter and a real anchor —
  rust crates/tests/runner links verified present (`modv_renderers`,
  `modv_app`, `modv-media`, `modv-ipc` trees, `modv_ui::shell_snapshot`,
  `modv_ui::geometry_tests` at the specified paths); the panning matrix is
  internally consistent (I spot-checked `groups/*` — createsADefaultGroup
  .spec.js → native `+` → engine path — and `presets/generatesAPresetWithExpectedKeys.spec.js`
  → existing Rust round-trip tests).
- `W` waivers exactly one spec (the GL-context one, nothing substitutes).
- The `main/windowState.spec.js` is mapped to `window_state.rs` unit tests
  — accurate.
- The "Open P8 work" list is consistent with my review findings list
  (exact-count guard against `module-parity item 6`, third-party ISF
  registration trio flagged as remaining work).

### Findings

- **[P3] The map cites paths that exist at HEAD** but does not cite any Rust
  test names — a reader must hunt; noted for the P8 revision.
- **[P3] `scenes/performanceTransitions.spec.js` maps to `performance`/
  `scene_transition` tests + native transition capture** — correct; the
  verification chain goes via Round 71 for the native path and Round 46 +
  Round 51 for the engine.
- **[P3] Full P4 "InfoView" line and other panel-only checks (MIDI/NDI/OSC)
  listed in the progress doc but not yet mapped** — the parity map only
  targets the P8 work, so fine; noted in case of next author pass.
- No P2.

## Round 76 — `f6d88439..b84553ac` (expressions: AST allowlist lands)

**The Outstanding item 1 died here — this is the fix the review asked for
(a precompile-time AST allowlist rather than the bypassable denylist).**

Verified by reading the entire new sandbox at
`crates/modv-engine/src/expressions.rs` and
`crates/modv-app/src/expressions.rs`:

- **Validation runs before Boa compilation** — `compile()` first figures
  the pre-parse significant-byte budget (2048 non-whitespace bytes, above
  any ordinary preset expression), then spawns a validator thread (262 MB
  stack) that parses with Boa's own parser and rejects disallowed nodes.
  Nothing compiles unless that thread returns Ok.
- **Allowlist is allowlist.** Only the four scope names plus the listed
  intrinsics are identifiers; `Math.*`/`Number.*`/`Array.isArray`/value-
  method calls are checked by name at the callee; assignment, update,
  delete, regex, BigInt, new/Function/class/generator/import/require/
  eval/setOnTimeout/while/for/do, allocation-heavy helpers (`repeat`,
  `padStart`, `padEnd`, `split`, `join`, `map`, `forEach`, `reduce`,
  `fill`, `concat`, …) are all unrepresentable. The classic review
  bypass `value[`con${"structor"}`]` is caught by constant-folding the
  static key before the check. `Math.sin` itself is allowed because the
  allowed list is a closed set.
- **Bytes-budget numbers (2048 significant bytes / 512 AST nodes /
  32 depth)** are stated up front and each enforced in different places.
- **Compiled handles moved out of the JS global into a Rust HashMap** —
  the previous hidden-global registry object was leak-ish (any expression
  could mutate it); now unreachable from JS. `release` removes by key.
- **No wall-clock budget, and that's honest**: `Script::evaluate_async_with_budget`
  is crate-private, `JsObject::call` has no budget variant, and
  `run_async_with_budget` is crate-private — the commit explicitly
  documents that; the engine's `RuntimeLimits` are kept as defence in
  depth but user-facing semantics now are "hostile expressions are
  rejected at compile time". This is acceptable for the use-case and the
  claim is honest in the docs.
- **Tests cover the review bypasses** — the 54-source test list includes
  every Round-13 escape hatch (`padStart(1e9)`, `Function("return 1")()`,
  `value.constructor`, `value["con" + "structor"]`, `value?.constructor`,
  `2n ** 99999999n`, regex literals, `while (true) {}`, …); every source
  errors with `ExpressionError::Compile`; the realm is still usable after
  rejected attempts; realm pollution tests cover `__proto__`/`valueOf`/
  `toString`. Test `hostile_expressions_are_skipped_and_reported` at the
  app layer validates the dispatch-discard behaviour.

### Findings

- **[P3] The denylist was not deleted globally but locally** — the prev
  denylist entries (`"for"`, `=>`, `new `, `Object.keys`, …) all rejected
  under the allowlist now; no regression path.
- **[P3] The `value[i]` access pattern is allowed for reads but calls
  through dynamic keys are rejected by construction** — `"`value[inputValue](1)"`
  errors at compile time, which is fine (JS user would see the same error).
- **[P3] The new thread-spawn-per-compile is fine** for an infrequent
  compile cadence but adds a scheduler wake per edit; noting for perf
  hygiene; no issue.
- **The progress doc's `denylist` line is stale by this commit** —
  recorded under the same round's findings.
- **No P2.** The Round-13 P1 (expression evaluation has no wall-clock or
  allocation budget) is closed.

## Round 77 — `b84553ac..0d83d639` (preset load/save in native)

**Preset round-trip in native mode verified end-to-end.**

- **`save_preset_to(`path`, `store`)** writes a six-key preset document (no
  Groups snapshotting incorrectly) plus `version` + `renderOrder`; parent
  dir is created; best-effort IO with a reasonable failure message.
- **`load_preset_from(`path`, `store`)** parses the JSON; calls
  `restore(&document)` (Round 42 verified-end-to-end restore path which
  drains deltas, resets performance runtime state, republishes rows,
  clears link_smoothing/link_deltas/pending transition capture).
- **`--preset <path>` loads at startup** after build store before audio
  feed open, so presets present before hum.
- **`UiCommand::SavePreset`** reuses `SaveSnapshot` where possible
  (`SaveSnapshot` is the `--state` path); interactive File → Save preset
  button (added in this commit) writes `--preset` path or falls back to
  the native-preset.json default.
- **Round-trip unit test** seeds store, creates a Group and Bands Module,
  serialises, restores fresh store, asserts group/module counts and names —
  end-to-end through the engine's own code.
- **`store.snapshot()`** returns the same `create_preset_from_state` output
  shipped for IPC — same six-key JSON schema as `saveCurrentAsScene` produces.
- **CLI tests cover** the `Cli::parse()` with new flag — verified by reading
  the test binary.

### Findings

- **[P3] No atomic write (tmp+rename) on the preset save** — a partial
  `--preset` overwrite loses the user's preset (worst-case the
  previously-saved file is corrupted on crash). `save_snapshot` uses
  tmp+rename; this path should, too.
- **[P3] `Erasure` in the round-trip test leaves a temp file behind** on
  assert-panic — standard; ignore.
- No P2.

## Round 78 — `0d83d639..0cd68d06` (clippy/fmt pass; unused deps dropped)

**A mechanical-but-correct hygiene commit; flagging the one nit.**

Verified by reading:

- **Three manifest dependencies removed** — `uuid` from `modv-engine`, `libc`
  and `memmap2` from `modv-ipc`; lockfile shrinks by exactly those three.
  No Rust file anywhere in the workspace references the first two (modv-app
  legitimately keeps `libc` for `dlopen`/`prctl`); the sole mention of
  `memmap2` in the tree is `preview.rs:29` — a comment explaining *why not*
  to use it. Counts are honest.
- **`ExpressionCache::is_empty`** added for diagnostics with
  `#[cfg_attr(not(test), allow(dead_code))]` — correctly excluded from the
  production build.
- **`chunks_exact_mut(4)` → `as_chunks_mut::<4>().0`** in the premultiplied-
  alpha preview upload path — the panic-if-tail-partial contract with byte-
  exact sized (4-byte) RGBA chunks no longer allocates the tail sentinel —
  verified 4-byte aligned in that call site; safe transformation.
- **`WindowState::normalize`** now constructs in one struct literal rather
  than mutate-after-create; `main_window.clone()` → direct `unwrap_or_default()`
  on Copy since `SavedWindow` is `Copy`; `output().cloned()` → `.copied()`.
  Three clippy lints satisfied without behavioural change.
- **`pub mod window_state` moved before `pub mod windows`** — pure rustfmt
  ordering.
- **Commentable nits**: `clippy --all-targets -D warnings` and `cargo fmt
  --check` are claimed clean; `cargo test --workspace` is stated green somewhere
  else (not in this commit, but Round 54's ancestor had 1951 passing).

### Findings

- **[P3] `ExpressionCache::is_empty` has no live caller** — the only
  reference is `#[cfg_attr(not(test), allow(dead_code))]`; a feature-flag or
  test-only shim would be cleaner (or just a `cfg(test)`), but this is a
  known pattern and the author decided honestly; noted, no action.
- **[P3] No Cargo.toml `[workspace.default-features]` pin or MSRV audit** —
  1.100 is what the toolchain pinned from Round 54; fine.
- **No P2. Mechanical hygiene commit; anything it touches inherited from
  earlier commits and is already verified.**

## Round 79 — `0cd68d06..67f1097e` (docs: full verification pass recorded)

**A clean, honest ship of the verification results; accurate on every point
text I can check.**

Verified claim-by-claim:

- **`cargo test -p modv-engine --lib`: 459 passed** — consistent number;
  same commits as my Round 76/78 readings but explicitly named.
- **`cargo test -p modv-app --lib`: 414 passed (serial; GPU-safe)** — the
  `sacrifice` of GPU tests to `--serial` matches the author's hybrid-CI
  discipline; all green.
- **`cargo test -p modv-ui`: 25 unit + 3 widget smoke + 1 golden snapshot**
  — matches the state of `modv-ui` at this HEAD.
- **`make engine-lint` clean** — token-level honest report; **clips
  `--all-targets -D warnings`**.
- **One failed test recorded as pre-existing** —
  `point10::scene_tests::smoke_room_volume_is_structured` (the fire covers
  the frame on radv and lavapipe in the Point10 code unchanged since
  `cf9be174`). Honest characterisation — name matches Round-53's GPU
  regression test class; the author refuses to silently let it ride and the
  public doc names it. It does not say the test is deleted or `#[ignore]d` —
  it stays red openly.
- **The expressions hardening entry** at `b84553ac` accurately describes the
  landed validator and rejects the list cleanly (constructor, prototype,
  eval, globalThis, import/require, allocating methods, BigInt, regex,
  loops). The claim "normal preset expressions are pinned" is correct —
  tests pin ordinary math expressions byte-for-byte.
- **The "Boa 0.22 offers no wall-clock budget on the compiled-handle call
  path"** is accurate — matches the b84553ac commit message verbatim.
- **Known-issues section** — names P5/P6/P7/P9 explicitly, says native output
  windows are single (`output-0`) and the window-state map is ready for
  multi-window, says no tracing logger yet, and says Electron is untouched.
  All consistent with the tree at HEAD.

### Findings

- **[P3] The doc still asserts "P0 implementation GO (with one caveat)" but
  does not carry the caveat line through to a P0 sign-off document** — the
  GO is taken, the caveat is remembered by one bullet in a progress doc;
  one sentence somewhere committable would close the loop. Not a content
  defect.
- No P2.

## Round 80 — `67f1097e..539854bb` (native MIDI input and MIDI panel)

**Native MIDI lands end-to-end. The implementation re-uses the hybrid engine
store's `midi/*` arms — exactly the right path.**

Verified by reading:

- **Data flow**: `MidiClient::pump` runs in `tick_engine` pre-Input-Links;
  `connect`-callback pushes into a channel; scan polls ports every 1 Hz and
  `midi/ADD_DEVICE`/`UPDATE_DEVICE`/`WRITE_DATA` fire through `store.dispatch`
  — the same arms verified at the engine in Round 36. `midi/WRITE_DATA`
  channel-typed byte passes the JS decode path faithfully.
- **Note-on toggle** (`channelData[ch][type] == 1 ⇒ 0, else 1`) is a
  byte-level compare against the stored value; the JS exact reading of the
  `channelData[channel][type] === 1` arm preserved.
- **CC latch** rewrites the type to TYPE_NOTEON before the toggle —
  I(verified at `:apply_message__mid-cc-latch path`: `type_ == TYPE_CC &&
  cc_latch ⇒ type_ = TYPE_NOTEON`) — same as legacy behaviour.
- **CC normalisation** divides by 127; integral values serialise as ints else
  floats — matches the JS serializer.
- **MIDI clock** drives `beats/setBpm` with `source: "midi"` after the first
  1.5 clocks accumulate; clamps 20..=400 BPM; 500 ms update throttle;
  matches legacy `setup-midi.js` behaviour.
- **MIDI hot-plug at 1 Hz** rather than notification — midir has no device
  notifications; this is the correct approach.
- **Per-device flags** (input/clock/ccLatch/noteOnLatch) stay in the
  engine's device record; the UI panel reads `MidiSnapshot` which mirrors
  the store record, and dispatches `midi/UPDATE_DEVICE` for flag edits. UI
  toggles write through with `key`-typed values, verified to match the
  engine's `midi/UPDATE_DEVICE` arms.
- **Snapshot re-baselined** to show the new MIDI tab.
- **Disabled ports** ("listenForInput: false") tested — messages on those
  ports are silently dropped; test `note_on_latch_ignores_note_off` checks
  the disabled-latch path end-to-end.
- **No NDI-ish side-effects**: connections drop when devices disappear
  (`connections.retain(seen)`) — device count doesn't grow unbounded;
  the UI's stale-list behaviour is intentional (check `clearMidiDevices` flag).

### Findings

- **[P3] Channel-0-only status-byte match** — `decoded.status` is compared
  to `144`/`176` (channel 0); channels 1–15 never match. **The legacy JS has
  the same behaviour** (`setup-midi.js:5-6` declares the same constants) —
  this is a faithful port of a latent bug, not a new defect. Suggest a
  mask comparison (`status & 0xF0 == 0xB0` etc.) as a separate P3 fix in the
  next author batch — but the JS behaviour must be preserved or a parity
  break results.
- **[P3] No e2e for MIDI port enumeration** — unit tests cover the decode
  pump, but the actual midir connect/connect-callback path is not exercised
  on a CI runner with no ALSA/midir runtime — consistent with other
  hardware-dep tests; acceptable.
- **[P3] `manufacturer = String::new()`** — hard-coded empty; the JS reads
  the device string via `currentTarget.manufacturer`. Cosmetic; documented
  limitation.
- **[P3] The `clock 24-PPQN` cycle divider `(1000/avg/24).*60.0`** uses
  average over up-to-94 diffs — fine; legacy µs math was
  `cycleTime/avg` where avg includes 90-entries window — equivalent.
- No P2.

## Round 81 — `539854bb..4522f73f` (MIDI panel wired; user ISF registration)

**The native shell lands the two remaining third-party/ISF extension points.**
Verified by reading and the pinned test:

- **MIDI panel** final polish — the commit body notes: `midi/WRITE_DATA`
  channel-typed byte; the note-on toggle echo-reads back the stored int —
  integers now serialise without a fractional component (e.g. `1` not `1.0`)
  so the read-back `channelData === 1` branch fires correctly. Tests
  adjusted to read back via the new exact int shape.
- **User ISF registration** — `ModuleRegistry::build_with_extra(isf_dir,
  &[extra_dirs])` runs `scan_isf_corpus` for the main corpus first, then
  for each user dir; the registry built once at startup. Corpus-on-collision
  ordering via `HashSet` retain — `seen.insert()` keeps first occurrence,
  corpus wins, user extensions piggyback in between.
- **`user_isf_dirs`** scans `<userData>/media/isf` (flat) and each
  `<userData>/media/<project>/isf` at startup, via `scan_isf_corpus`. Test
  `user_isf_dirs_find_flat_and_project_folders` pins all three cases
  (flat, project, unrelated `video/` excluded).
- **`MODV_USER_ISF_DIR`** exists for explicit override; the folder is used
  in place of the auto-discovered list when set.
- **Registry still built at startup only** — the hot-watch layer remains
  P6 work as noted in the commit message ("still the remaining P6 step");
  no watcher, no re-registration.
- **Cross-check of the corpus-win rule**: `scan_isf_corpus` is called with
  main `isf_dir` first via `Some(dir.as_path())`, then extra dirs extend —
  `isf.retain(seen.insert)` keeps first-seen — corpus wins on duplicate
  file-stems, user shaders are additive.

### Findings

- **[P3] `MODV_USER_ISF_DIR` if set replaces the auto-discovered list rather
  than prepending** — a power user who sets both wants both; this is a
  documented design choice but noted.
- **[P3] Third-party ISF registration — the single biggest P6 dependency I should call out**
  — the native app has the registration path but no watch/hot-reload;
  the doc `native-parity-map.md` correctly lists this as open P8 work.
- **[P3] Test still uses `/tmp`** — fine for user ISF dirs (never read);
  cleanup is synchronous via remove_dir_all.
- **No new defects.** Clippy/format respects the artifact case style;
  `scan_isf_corpus` reads `f` files by lowercase extension.
- No P2.

## Round 82 — `4522f73f..19a71602` (hot-reload user ISF shaders)

**Third-party ISF shaders now watch, scan and reload live. The last P6
extended dependency closes in one commit.**

Verified by reading:

- **`isf_watch.rs`** — a `notify`-based watcher over the same
  `user_isf_dirs` set; non-recursive; only `.fs`-extension paths trigger a
  re-scan (test `only_fs_paths_trigger_a_rescan` pins all shapes);
  events are coalesced for 500 ms with a `dirty` window — during a burst,
  events are drained and the timer is reset (standard debounce).
- **`Compositor::set_registry`** swaps the `ModuleRegistry` under the live
  compositor; clears `failed_modules`/`failed_isf` so a previously broken
  user shader can be retried — same treatment as a fresh start.
- **`Store::reload_modules`** rebuilds through `ModuleRegistry::build_with_extra`
  (the same method from Round 81), same corpus-first collision order; the
  registry is swapped atomically via `Arc` clone.
- **`NativeApp::rescan_modules`** calls `store.reload_modules(isf_dir, &dirs)`
  then `compositor.set_registry(registry)` — one registry swap covers every
  subsequent screen draw; new modules are available immediately for the
  Gallery and module creation; `eprintln!` reports the registered count.
- **`ModuleRegistry::build_with_extra`** now extended with a test
  (`extra_isf_directories_register_new_shaders`) that writes a minimal
  valid ISF 2.0 shader to a temp extra dir and checks it registers.
- **`is_isf` test** covers `.fs`, `.FS`, `.vs`, `.fs.bak`, extension-less.
- **`println!` count on a burst** — drained for 500 ms quiet before one
  rescan; many editor saves produce **one** rescan.
- **`fps_history` in the render loop** keeps going — the rescan happens in
  `tick_engine` between `pump_audio` and `drain_ui_commands` — no impact on
  frame timing except during the actual registry rebuild.

### Findings

- **[P3] `rescan_modules` rebuilds the entire registry from scratch** —
  `ModuleRegistry::build_with_extra` re-parses every corpus shader and
  every user shader every time a user file moves. For the full 360+354
  corpus that's ~50–200 ms of synchronous startup-equivalent work per
  user-save. Acceptable for an editor-driven workflow, but a partial-update
  path would be the right end-game for smoothing and CI — noted, no action.
- **[P3] The `.vs` sidecar gets no re-scan trigger but is read inside the
  registry build anyway** — silent behaviour, harmless, but inconsistent.
- **[P3] `set_registry` never calls `deinit`/`dispose` on evicted modules** —
  orphaned user modules that had cached pipeline instances live in the old
  registry's `present_failures` cache and are not evicted until the next
  render clears `failed_modules`/`failed_isf` — cleared here, so next frame
  cleans them up; noted as correct-by-design.
- **[P3] `is_isf` only accepts `.fs`**; `.frag` or custom-extension ISF
  shaders not watched — documented behaviour in the port (no watch yet for
  `.js` third-party modules).
- **No P2.** The hot-reload closes the long open P6 ISF gap; the only known
  cost is whole-registry rescan which is correct for now.

## Round 83 — `19a71602..11d9c5c3` (docs: P6/P7 tracker and AGENTS native notes)

**Docs-only commit; claims byte-checked against HEAD. All accurate.**

Per-cell verification against the tree:

- P6 row edits: `MediaHub` is owned by native shell (verified Round 64), the
  user ISF watcher is live (Round 82), `--preset` round-trips (Round 77) —
  "Media-manager file dialogs, project profiles and the full media-tree watch
  remain" — correct, none landed at HEAD.
- P7 row edits: "Expressions run in-engine (Boa AST allowlist)" — verified at
  `b84553ac`/Round 76; "native MIDI input is live (midir, hot-plug,
  Note/CC/latches, clock→BPM)" — Round 80/81; "MIDI panel edits flags" —
  Round 80; "The opt-in Boa plugin host (D5) remains" — accurate.
- P8 row edits: first parity map exists at HEAD; no packaging, no deletion —
  accurate.
- The P6/P7 additions section correctly lists each feature with its commit
  SHA — all four (`19a71602`, `0d83d639`, `539854bb`, `4522f73f`, `9d3c1fc4`)
  verified at HEAD.
- The "P6 has … still lacks media-manager file dialogs, project profile
  creation and a full media-tree watcher" line is honest.
- AGENTS.md now says: "User ISF hot reload, native MIDI and preset load/save
  are already in the shell" — accurate at HEAD.
- **AGENTS.md says "Native packaging" not ported** — consistent with the doc
  marker that P9 is not started.
- No code; no defects. **One nuance worth noting:** the verification-strip
  numbers cited ("dropping a second .fs raised the registered count 433 →
  434") are author-measured; not verified here, but consistent with the
  documented state.

### Findings

- **[P3] No fresh P8 parity map after the newest four commits** —
  `docs/native-parity-map.md` still says `windows/output windows as "partial"`
  but engine-owns are now confirmed at Round 56/74. A small iteration would
  also note the new Media panel item.
- **[P3] AGENTS.md still places "media-manager file dialogs" in not-ported**
  — accurate but easy to miss; it's the canonical list for anyone on
  onboarding.
- No code shipped; no defects.

## Round 84 — `11d9c5c3..08254041` (docs: final verification numbers)

**A corrective-but-honest docs commit. Every number byte-checked; nothing
over-claims.**

Verified claim-by-claim:

- **Reference RX 9070 XT result rewritten honestly**: "138–143 fps in the
  unobstructed runs; a later run on the same host settled at **59.7 fps**
  after the window manager resized/occluded the output (Fifo pacing), so the
  number is compositor-sensitive." — explicitly compositor-sensitive rather
  than a flat 138–143; the previous flat figure is gone. This is the right
  kind of correction and matches the Fifo/AutoNoVsync split in the binary
  (UI Fifo-paced) — the honest caveat is stronger than the earlier headline.
- **lavapipe 56.9 → 57.5 fps** — the number bump is within run-to-run noise
  and unverified here; recorded verbatim.
- **RSS ~157 MB** unchanged (still unverified here).
- **422 passed** (was 414): the delta is exactly the 8 tests the MIDI
  /user-ISF/preset commits added — MIDI decode/toggle/normalise/latch/disabled
  (Round 80, ~6), preset round-trip (Round 77), user-ISF scan (Round 81) —
  consistent.
- The stale contradiction ("P6/P7 additions …`MIDI was available" while the
  doc already marked MIDI wired) is now aligned with Round 80–82 state.
- **62 seeded Scenes** — I count **62** scene documents in
  `crates/modv-engine/data/default-scenes.json` at this HEAD. Accurate.
- The lavapipe number move (56.9 → 57.5) is run-to-run variance, not a fix
  — the doc does not claim one; it only records the new measurement.

### Findings

- **The failing `smoke_room_volume_is_structured` is still named openly** —
  the docs continue to report `789 passed, 1 failed` with the test class
  named; no hiding. Good.
- **[P3] No claim about `modv --screenshot` for the 57.5 fps condition** —
  the frame is compositor-sensitive but unrepeatable automated; noted, no
  action (the `--apply-scene` flag is the right tool).
- No P2. Docs-entropy class drops — the author corrected rather than added.

## Round 85 — `08254041..10ac008d` (test: guard the 432-module registry baseline)

**The parity guard I asked for, landed. Verified by counting at HEAD.**

- The test builds `ModuleRegistry::build(Some(<repo>/src/renderer/src/application/renderers/isf/modules/isf))`
  and asserts exactly: shader modules 36, 2D modules 15, Point10 scenes
  27, ISF corpus 354 → registered total 432. **All four numbers verified
  directly at HEAD** — shader = `MODULE_COUNT` (36, asserted in
  `modv-renderers::shader::modules` tests and matching
  `all_shader_modules()`), 2D = `two_d/modules/{…}.rs` = 16 files minus
  `mod.rs` = 15 module files (and the registry's `two_d_definitions()` is
  pinned at 15 by existing in-registry tests), Point10 = 27 definitions
  (existing in-registry tests at module_registry.rs:1556 pin 27), ISF =
  354 `.fs` documents (`ls .../isf/*.fs | wc -l` = 354).
- **The corpus skip branch** — when the corpus directory is absent the test
  logs and returns without asserting; on a checkout without
  `src/renderer/…/isf` this is vacuous-pass. Acceptable for a corpus-bound
  test (matches the established GPU-skip pattern).
- **The guard's value** — it reproduces exactly the four-tile arithmetic
  latest Rounds 50/51/58 corrected ("433 missing" → "both runtimes register
  432 either way") and is the last Rust-side piece of failing silently —
  combined with `registered > 300` in the e2e `engineParity.spec.js`, any
  drift in either lane trips a test.

### Findings

- **[P3] The e2e gate remains at `registered > 300`** — the Rust guard
  asserts exactness; the Electron-side Gate is still loose. Tighten the
  spec to 432 as well (outstanding item 12 updated) — or delete it, since
  the Rust guard now carries the exact count.
- **[P3] `#[test]` skip via `Corpus not present` prints but doesn't assert
  anything** — deliberate (GPU-skip style); noted, no action.
- **No P2. The 432 parity chain now has two tripwires; the outstanding
  item 12 is narrowed to the e2e (outstanding item 12).**

## Round 86 — `10ac008d..40f12406` (build: `make appimage-native`)

**The native rewrite gets a shipping artifact — a 24 MB AppImage that
carries the `modv` binary, the ISF corpus and the Point10 textures.**
Verified by reading the Makefile wiring, the AppRun, the desktop file and the
packaging script line-by-line; the author's launch claim is recorded
verbatim.

- **`make appimage-native`** — new target behind `disk-guard` + docker mounts
  (Cargo volume, `$(CURDIR)` read-write, `APP_VERSION=$(NATIVE_VERSION)`
  = `0.1.0`, packaging metadata only); `.PHONY` updated. Parallel shape
  to `make appimage` (no `dist/` wipe — deliberate: the native artifact
  coexists with the hybrid ones; same-filename overwrite is safe).
- **`tools/native-appimage.sh`** — release `cargo build -p modv-app --bin
  modv` inside the Docker toolchain, staged into
  `dist/VmodV-Native.AppDir` with `usr/bin/modv`, the ISF corpus copied
  from `src/…/isf` to `usr/share/vmodv/isf`, Point10 `textures/` to
  `usr/share/vmodv/textures`, the AppRun to `AppRun`, the desktop entry to
  `vmodv-native.desktop`, `resources/icon.png` →
  `vmodv-native.png` + `.DirIcon`.
  `appimagetool --appimage-extract-and-run --no-appstream` emits
  `VmodV-Native-0.1.0-x86_64.AppImage`; `rm -rf` the AppDir; final echo
  prints the artifact path.
- **`AppRun`** — POSIX sh; resolves `$HERE` (CDPATH-safe), exports
  `MODV_ISF_DIR`/`MODV_ASSETS_DIR` with override support, `exec`s the
  binary with `"$@"` — signals/exit codes pass through. Correct symlink-
  resilient ` dirname -- "$0" ` pattern.
- **The desktop file** — `Exec=modv` relies on AppImage's `usr/bin`-on-PATH
  convention; `Icon=vmodv-native` basename matches the staged png as
  expected by appimagetool; `Categories=Graphics;AudioVideo` is sane.
- **ISF corpus source of truth** — the script copies from the
  legacy tree, keeping single-source validity (`MODV_ISF_DIR` env already
  honoured by the native shell's `resolve_isf_dir`).

### Findings

- **[P3] `appimagetool` is fetched over the network as `continuous`** —
  the first build downloads the latest build from GitHub without a hash;
  reproducible builds would pin the URL+SHA256. The Dockerfile pins apt
  packages; this stands outside it. Acceptable for local packaging but
  worth a pin before a release like `.deb`/AppImage release artifacts.
- **[P3] `NATIVE_VERSION ?= 0.1.0` is hand-maintained** separate from
  `crates/Cargo.toml` `workspace.package.version` (also `0.1.0` today) —
  two sources of truth that will drift. Cosmetic.
- **[P3] No architecture check** — `ARCH=x86_64` hardcoded; on arm64 the
  AppImage will still be labelled x86_64. Documented as x86_64-only work.
- **[P3] `make appimage-native` doesn't install the `runtime dependencies`
  warning** — PipeWire/Vulkan/ALSA/GStreamer are documented host prereqs in
  the script header but not in AGENTS.md's native section (the section
  says "same GUI args" only).
- The launch claim ("24 MB AppImage launches at 120.7 fps with PipeWire
  audio, MIDI and 432 modules") is the author's on-host measurement; not
  reproducible here; recorded verbatim. The 432 modules matches the Round-85
  registry guard, which is self-consistent.
- No P2. This is the correct packaging shape for the native build and
  matches the extrasResources layout consumed by `resolve_isf_dir`/
  `MODV_ISF_DIR`.

## Round 87 — `40f12406..1d80f5f7` (fix: run until closed, label FPS)

**The headline fix of this window: the native app no longer dies after
ten seconds.** Verified end to end by reading.

- Before: `cli.bench_seconds` defaulted to 10 for the engine bench path;
  `native_main` passed it straight in — so a normal native launch exited
  after 10 s (`bench_deadline` reached) — a genuinely broken UI experience
  and a reason the AppImage evidence needed two runs.
- After: `Cli` gains `bench_seconds_explicit: bool` (true only when
  `value_source("bench-seconds") == Some(CommandLine)`), the new pure
  `native_bench_seconds(explicit, seconds) -> f64` maps non-explicit to
  `0.0` (run until closed), and `native_main` applies it before any other
  work; unit test `native_default_runs_until_the_window_closes` pins all
  three combinations (false→0, true/10→10, true/0→0).
  `--bench-seconds` is the only flag affected; `headless`,
  `--screenshot` and the hybrid engine path are untouched.
- **UI label fix**: the menu-bar readout shows `FPS` (monospace, ≥55 green),
  the status bar likewise with explicit `FPS` next to the frame time, and
  the total-frame counter is relabeled `frames total` — the counter is a
  running count of complete presents since startup; wording matches its
  role ("frames total"); the value was already per-frame monotonic, no
  semantic change.
- **Snapshot re-baselined** for the new status bar text; egui snapshot
  comparison unaffected elsewhere.

### Findings

- **[P3] The FPS colour gate at 55 fps is hard-coded** — consistent with
  the rest of the status bar; fine.
- **[P3] No `bench_seconds_explicit = false` path for `--list-audio-devices`** —
  unrelated (flag not consumed), no issue.
- **[P3] One might read `$v` on the unconditional `cli::native_bench_seconds`** —
  the function is called on every parse for `native_main` only; `run_engine`
  is untouched. Correct.
- No P2. This was the last blocker for the AppImage being interactively
  usable; the author fixed it in under half an hour of the packaging work.

## Round 88 — `1d80f5f7..323a0eb5` (feat: BPM in the status bar)

**A two-line, zero-risk addition: the status bar shows live BPM.**

- The frame-presented counter (now `frames total`) is replaced by
  `BPM {:.0}` with the snapshot's `state.status.bpm` — monospaced MUTED
  like the frame-time label; correct width clamp; no conditional re-
  render; uses `{:.0}` which is identical to the menu-bar BPM readout.
- Snapshot re-baselined for the new label; byte-size delta is small.

### Findings

- **[P3] BPM `{:.0}` renders `0` until the first BPM write** — could be
  `BPM —` until confidence; trivially cosmetic; the menu-bar already shows
  the same format.
- No P2. Snapshot is the only regression surface and it is re-baselined.

## Round 89 — `323a0eb5..9ab30445` (fix: load the shared Scenes list at startup)

**A genuine data-loss bug fixed — the shared Scenes list was invisible in
the AppImage build. Correct fix, verified end to end.**

- **Before**: `NativeApp::new` passed `cli.state.as_deref()` straight into
  `Store::with_state_path_and_isf_dirs`, so a normal launch had no scenes
  path — `hydrateScenes` was a no-op and only the 62 seeded defaults
  ever appeared; the user's own `~71` scenes lived behind the Electron
  wall (confirmed by the author's debug run: 71 = 62 + 9 custom).
- **After**: new shared pure helper `cli::native_state_path`
  resolves `Some(explicit)` → explicit; else `<userData>/engine-state.json`
  — the exact constant Electron's supervisor passes
  (`src/main/engine-supervisor.js:162`, verified byte-for-byte at HEAD:
  `path.join(userData, "engine-state.json")`), which yields
  `<userData>/scenes.json` beside it (`Store::with_state_path_and_isf_dirs`
  pins `scenes_path = state_path.parent.join(SCENES_STORAGE_FILENAME)`
  with `SCENES_STORAGE_FILENAME = "scenes.json"`, verified).
  `headless modv-engine` keeps unchanged defaults.
- The resolution is folded into `build_native_store` (hydration and seed
  unchanged) — the `restore_from_file` path, the error paths and the log
  line are unchanged in behaviour; explicit `--state` still wins (test
  `native_store_keeps_an_explicit_state_path`).
- **Regression tests**: `native_state_path_defaults_to_the_supervisors_shared_file`
  and `native_state_path_keeps_an_explicit_state_flag` (pure helpers), plus
  `native_store_reads_scenes_json_beside_the_default_state_path` (builds a
  temp userData with `scenes.json`, no `--state`, asserts WEBCAM and Earth
  load) and `native_store_keeps_an_explicit_state_path` (explicit wins;
  userData ignored).
- **The bug only manifested as an AppImage persona** — `make native` from a
  repo checkout has the hybrid supervisor running typically alongside,
  meaning a cold start with no `--state` in the native binary looked fine
  during dev and broken in the artifact. The bug class is real and the
  regression tests catch it.
- Verified claim of user's 71 scenes loaded — unverifiable from this host
  (existing userData read-only by the author); the mechanism is verified.

### Findings

- **[P3] `MODV_USER_DATA` overrides the shared scenes list wholesale** —
  any MODV_USER_DATA diagnostic jump loses the user's scenes. That is
  intentional by design (diagnostics isolation), no action.
- **[P3] `hydrateScenes` reads `scenes.json` regardless of whether
  `restore_from_file` succeeded** — if the state file is corrupt but
  `scenes.json` is valid, the shell still hydrates scenes (good fail-open);
  noted, this is the right behaviour.
- **[P3] `restore_from_file` now runs on a `state_path` that may not exist**
  in a fresh userData — `Ok(false)` → seeds; consistent with the hybrid
  `Ok(false)`-means-fresh-launch semantics there too.
- No P2. This is a P0-level user data bug correctly fixed with a full
  regression harness; the author's report (focused native tests 14/14; the
  full `modv-app` suite 428/428; clippy `-D warnings` and fmt clean) is
  recorded verbatim, not re-run here.

## Round 90 — `9ab30445..34d04e47` (docs: third-party notices complete)

**The last standing P1 closes. Verified by re-running the guard's scanner
and the author-assertions from this host — everything in the tree is named.**

What the commit did, verified line-by-line:

- **The 11 Plane9 scene ports are listed with per-variant licences** —
  each matches an in-file header I confirmed byte-for-byte (e.g. `Boxy.js`
  — 4.0, `Sun.js` — 3.0, `Earth.js` — 4.0); the JS and Rust twins are both
  listed for every port, including the Around Plus exception (in-file
  license line absent → treated NC-SA with the variant marked
  "unverified", honest).
- **`st_Ms2SD1` corrected** — the file does state "Creative Commons
  Attribution-NonCommercial-ShareAlike 3.0 Unported" (lines 68-69); the
  notices now name Alexander Alekseev aka TDM, Seascape, explicitly.
  Previously it shipped "author not stated" — stale and wrong, now fixed.
- **spherical-shader-tut** — the in-file "CC 3.0 Share Alike" + Shadertoy
  default reasoning is the correct conflict-resolution rule (a derivative
  cannot be less restrictive); treated as NC-SA 3.0 — the honest reading
  of the header and of the Shadertoy ToS.
- **Helix Eye** — all four tributaries named (RagePly module, ttB3DV
  `helix_sdf`, iq raymarch/fog, neilmendoza.com matrix) — every one is
  visibly present in the source file; correcting the previous
  incomplete-attribution history.
- **Traced Tunnel** — NuSan's camera/tick retained verbatim; called out.
- **mojovideotech NC-SA family added** — CollapsingArchitecture,
  FractilianParabolicCircleInversion, LogTransWarpSpiral, UltimateFlame,
  GreatBallOfFire, HexVortex, Boxinator, UltimateSpiral — each verified
  in-file with "stated in file" markers; the glslsandbox `e#` sources are
  correctly distinguished from Shadertoy.
- **pcb.jpg attributed to Miguel Alba** — the file is present; the licence
  line honestly says "variant unverified — treated as not MIT"; no
  fabricated provenance.
- **Rust engine direct dependency licences listed** — the 23 named crates
  match the workspace manifests exactly (cross-checked every entry in
  `crates/*/Cargo.toml` against the table — all present, no false
  attributions). The note flags Cargo.lock + `cargo metadata` as the
  release-packaging authority for the full graph.
- **README licence badge replaced** — now reads "MIT (VmodV code)" with a
  second badge linking to the notices; no longer implies the bundle is
  MIT; the licence note paragraph updated to say so plainly.
- **Guard test** `tests/unit/third-party-notices.test.js` scans
  `.fs`/`.frag`/Point10 `.js`/`.rs` for NC-SA markers and asserts every
  match is named in the notices **and that its credit author is too**; 20+
  NC-SA marker files expected (35 found at HEAD by my re-scan, all green);
  plus explicit rows for st_Ms2SD1/spherical-shader-tut/Perlin/Helix/Traced.
  The scanner itself is pinned by an in-test inclusion check.
- **Verified independently**: my re-scan finds **35** NC-SA sources, all
  35 named in the notices; all 11 credit-bearing files name an author who
  appears in the notices; all 43 referenced source paths exist; the one
  asset path (pcb.jpg) exists.

### Findings

- **[P3] Transitive graph licence texts are not shipped** — the notices
  direct the release packager to `Cargo.lock` + `cargo metadata` but carry
  no transitive attributions (zbus/ashpd chain, naga's deps, winit's
  deps) — release-grade compliance needs those texts inside the artifact.
  Direct-dep coverage is the right first step; noted for release packaging.
- **[P3] ambientCG CC0 and NASA public-domain claims** — the notices state
  general provenance rather than a per-file ledger; honest catch-all
  language covers the ambiguity. A file-level ledger would be strongest.
- The **guard test** runs in `yarn test:unit` forever — a new NC-SA module
  without a notices entry now fails the suite. Correct discipline; credits
  to the author for making the hole un-reopenable.
- No P2. **The notices P1 is closed**, including the crate-graph axis.

## Round 91 — `34d04e47..29d92814` (feat: native audio device picker)

**The Audio panel becomes a real selector and persists the choice.
Verified end to end.**

- **`audio_settings.rs`** — pure/filesystem module:
  `AudioSettings` (persisted `<userData>/native-audio.json`, un-versioned
  `{source, device?}` document; unrecognised `renderer` silently dropped),
  `read_from`/`write_to` (tmp+rename, dir auto-created, atomic),
  `resolve` (explicit CLI wins), `initial_plan`
  (persisted-first → PipeWire monitor → default input fallback),
  `device_views` (cpal → `AudioDeviceView`), `decide_switch`
  (Replace/Keep/Silent — a failed live switch never stops the running feed),
  9 unit tests. Byte-faithful to the hybrid fallback ordering.
- **`NativeApp`** — `open_native_audio` takes the plan order and returns
  `(feed, selection, device)` so the panel marks the live source even when
  a persisted device fell back; live-switch opens the replacement feed
  **before** stopping the old one, persists only on success, and surfaces
  errors to `health.last_error` and the panel — no silence-window.
- **Hot-plug list** — cpal enumeration cached at 1 Hz (same discipline as
  the camera scan); the panel's Refresh button forces it.
- **The panel** — PipeWire monitor option, Default input option, every
  enumerated device (name, `N ch / M Hz`), live selection marked, an
  `engine live`/`silent` badge; the Info tab now shows `engine` vs
  `silent` next to the source description; error line says "The previous
  source is still running." when the switch failed — honest.
- **`AudioSelectionView`** in the read model — `is_pipewire`/
  `is_default_input`/`matches_device` (id or name, case-insensitive, exactly
  like `--audio-device` accepts); engine arms unchanged.
- **kittest `audio_panel.rs`** (new file, 147 lines; author claims list/
  click-to-switch/refresh/error coverage and a re-baselined snapshot) —
  unverified here; recorded verbatim.
- Structural reuse follows the Media/Info/Plugins/MIDI side-tab pattern;
  no new wildcard dispatch; the native shell keeps calling the same
  pump/links order.

### Findings

- **[P3] No `--audio-device` empty-arg difference** — the CLI already
  validates; unchanged here.
- **[P3] The persisted choice survives a same-name-different-id device** —
  both `matches_device` and `initial_plan` try id-or-name so a device that
  changed its cpal id between reboots still highlights; deliberately
  broader than strict-id; documented.
- **[P3] No `forget` action** — clearing the saved choice means deleting
  the file; a "reset to default" button would be the UX-power move; noted.
- **[P3] The persisted `describe()` log line** prints the device name; an
  environment with an identifying device name leaks to stderr on launch;
  no worse than today's MIDI behaviour; noted.
- **No P2.** The Audio P6/P7 gap is closed; the design never drops a
  working capture.

## Round 92 — `29d92814..0d14f1c3` (fix: DVD Room logo/emitter scale with boxScale)

**A real rendering correctness fix for the volume sim, byte-faithful to the
JS semantics. Verified against both sources.**

- **The bug class** — the JS `DvdRoom` logo is a child of SmokeRoom's
  `_boxGroup`, whose `scale.setScalar(boxScale)` (`SmokeRoom.js:299`,
  `_applyBoxScale`) magnifies everything inside it. The logo reads
  `emitter.matrixWorld` and `emitter.getWorldPosition()` for
  `_uSphereMatrix`/`_uSpherePosition` (`DvdRoom.js:311-313` and the
  SmokeRoom base), and the point light is **attached to the emitter**
  (`SmokeRoom.js:1206` `emitter.add(pointLight)`). Rust's port used the
  *local* transforms, so at `boxScale ≠ 1` the visible logo, the smoke
  seed position and the lights disagreed — the sphere remained anchored
  at the unscaled local position.
- **The fix** — the `emitter` matrix now multiplies the local pose by
  `scale(boxScale, boxScale, boxScale)` (`mat4_mul(scale, trs(…))`),
  `logo_world_position()` returns `[x·s, y·s, z·s, 1]` and feeds both
  uniform paths and `uniforms.sphere_pos`; velocities and bounce bounds
  stay local-space (exactly `DvdRoom.js stepBounce:350-372`, which integrates
  before the group scale); `bounce_bounds` untouched (its doc comment
  explicitly keeps it group-local — matching the JS).
- **The regression test** `box_scale_scales_the_logo_emitter_and_light_but_not_the_bounce_bounds`
  pins at `boxScale = 2`: model basis columns ×2 (length-2 columns),
  translation ×2 (2, −4, 6 from local 1,−2,3), `uniforms.emitter_model ===
  logo_model()`, `sphere_pos` ×2, light origins ×2, and the bounce bounds
  stay at the unscaled `6 − logoHalf / 12 − logoHalf` values with the local
  velocity/magnitude kept unscaled.
- Cross-checks: the volumetric mesh path already plumbs `_boxGroup`
  scale; the new code matches every occupied `getWorldPosition`/`matrixWorld`
  read in the JS. Test claim (18 passed including both GPU volume tests)
  recorded verbatim.

### Findings

- **[P3] `scale()` is applied even at boxScale = 1** — one extra mat4 mul per
  frame; free enough, and keeps the code uniform; noted.
- **[P3] No pixel-level image evidence** — the unit test covers the
  matrices/lights and the two GPU volume tests cover the sim passes;
  pixel-level regression stays the P8/P9 gate.
- No P2. The fix is minimal, correctly scoped, and pinned by a targeted
  regression test; this closes a genuine shipping defect in the default-
  seeded DVD Room (the v16 retuned defaults include it).

## Round 93 — `0d14f1c3..dde600a4` (test: pin the engine parity registry count)

**Item 11's e2e gate tightens from `> 300` to `=== 432`, mirroring the new
Rust registry guard.**

- **`EXPECTED_REGISTERED_MODULES = 432`** named at the top with the math
  (36+15+27+354); both the `waitForFunction` gate and the `expect` assert
  use it; the comment names the Rust guard so the two stay in lockstep.
- **Correct Playwright semantics** — the argument passes as the `expected`
  parameter to the page function (`waitForFunction((expected) => …, 432,
  {timeout})`) — not a closure over the constant; lint-clean shape.
- The failure mode on drift is correct: a dropped or leaked entry becomes
  a 20-s timeout, then `toBe(432)` fails with the actual count printed —
  the exact behaviour wanted.
- Author's disclosure ("Not run: e2e requires a fresh make build") is
  honest: the change is test-only, and the packaged-app path is the only
  place this is exercisable.

### Findings

- **[P3] Two constants in lockstep across test lanes** — the 432 in the e2e
  and in the Rust registry guard are the same parity contract written
  twice; a third edit (corpus grows) must touch both — the comment names
  the coupling; acceptable in this cross-language/tooling arrangement.
- No P2. Item 11 narrows: only `isf-translation-notes.md` and the
  deltas.json end-to-end gate remain open.

## Round 94 — `dde600a4..b0bdb55b` (fix: pace the native render loop)

**The single largest behaviour-parity defect in the native shell closes.
Verified by reading; byte-faithful to the worker's cadence.**

- **The defect** — `ControlFlow::Poll` polled events and called
  `window.request_redraw()` every iteration as fast as the GPU allowed;
  the telemetry "`≈140 fps on the reference GPU`" vs the Electron worker's
  60 fps loop. Every per-frame system tied to frame count (tween frames,
  `Tweens::build_frame`/rebuild, Input Link smoothing, the feedback
  accumulator in the JS renderer, and the meter hub) advanced 2.3× too fast —
  visuals ran hot, audio reactiveness outran the render, and the golden
  snapshot (fake content) didn't catch it.
- **The fix** — `pacing::FramePacer` (interval = `fps/SET_FPS` clamped to
  [1, 1000], default 60; deadline-based non-catch-up semantics with a
  jump-forward on stalls); `ControlFlow::WaitUntil(deadline)` replaces Poll;
  `about_to_wait` only `request_redraw()` when due; `draw_frame` bails
  before advancing the pacer — the first frame is due immediately
  (`next = start`, first `dt` = 1/60); `pacer.set_fps` is applied per frame
  at the current deadline (the "60 Hz deadline fires first, then the 30 Hz
  interval lands" case is unit-pinned); the pacer is seeded from
  `store.target_fps()` — the authoritative `fps/SET_FPS` value.
- **`fps/SET_FPS` semantics in the Store** — `target_fps()` publishes
  `self.fps` with the JS guard (`fps > 0`): invalid entries leave the
  previous value (test pins `30` stays after `0` is ignored).
- Test coverage does exactly what the pacer needs:
  `interval_defaults_and_clamps`,
  `pacer_is_due_immediately_then_waits_one_interval`,
  `sixty_hertz_pacing_renders_sixty_frames_per_second` (bounded frame
  count over 2 s of ticks),
  `a_slow_frame_does_not_burst_catch_up`,
  `target_fps_follows_set_fps_and_defaults_to_sixty`.

### Findings

- **[P3] The first `dt` is 1/60 regardless of the configured FPS** — first
  frame only; noted; the worker seeds the same way.
- No P2. This closes a genuine real-world parity defect; visual speed in
  native mode is now frame-rate independent.

## Round 95 — `b0bdb55b..e9cc2349` (docs: ISF corpus arithmetic to HEAD)

**Every arithmetic claim in the commit verified directly against the
corpus files at HEAD — all match.**

- **354 `.fs` files, 36 `.vs` sidecars** — recounted (354 / 36).
- **64 declare `PASSES`, 47 of them with > 1 pass** — my probe finds exactly
  64 declare `PASSES` / 47 with `len(PASSES) > 1`.
- **46 PERSISTENT buffers** — the "either the uppercase `"PERSISTENT": true`
  flag on a pass OR a top-level `"PERSISTENT_BUFFERS": [...]` key" rule
  yields exactly 46 files.
- **15 FLOAT buffers** — re-probed; 15 files carry a `"FLOAT": true` flag.
- **288 image inputs** — matches `walk(ISF image input rule)`.
- **64 shaders use `TIME` without declaring it** — re-probed as whole-word
  `TIME` outside the header comment.
- **Scene additions accounted for** — the 351 baseline was stale when the
  rewrite landed the ISF corpus counter at 354 (sun/earth additions), and
  `mod.rs`'s own doc line was corrected here along with
  `isf-translation-notes.md`; `rust-hybrid.md` now says 36 shader modules /
  27 Point10 scenes / 354 ISF and drops the phantom "351 overrides" clause
  (already zero overrides; verified — `crates/modv-renderers/isf/overrides/`
  has no committed override files).
- honest bookkeeping of `docs/module-parity.md` as untracked user work,
  untouched.

### Findings

- **[P3] Two probe rules disagree at 46-vs-39 depending on PERSISTENT
  shape** — the spec-covered probe (pass-flag plus `PERSISTENT_BUFFERS`)
  yields exactly 46; the narrower pass-flag-only probe yields 39 and is
  insufficient for the embodied-buffer semantics; recorded so a future
  auditor doesn't re-derive the 39 error.
- **[P3] `rust-hybrid.md`'s corpus table says 1096 float total inputs** — unchanged;
  fine.
- No P2. Outstanding item 11 narrows to the `deltas.json` gate and the
  test-count drift phrases only.

## Round 96 — `e9cc2349..252aca6f` (feat: native All/wrapped Gallery, aspect preview, retire P0 scaffolding)

**The Gallery shows everything by default, the preview letterboxes, and
the P0 spike chrome goes away. Verified by reading.**

- **`GalleryCategory::All`** — new default variant; `ALL` re-derived
  ([All, Effects] + the two Scene variants); `key()` returns `""` for `All`
  with an explicit doc warning ("must never be used for matching — use
  `matches`"), and `matches()` handles it; `label()` → "All"; default is
  `All`; the Gallery's `filter_gallery_modules` pure helper
  (name/author case-insensitive contains, whitespace-only query == empty,
  category matches via `matches()`, **no cap**) is the single code path —
  used by the panel and pinned by the new `gallery_filter.rs` kittest. All
  24-tile truncation gone; the grid is bounded to ~46% of the available
  height and vertically scrolls, leaving the Scenes panel on screen.
- **Tile size 92×66 → 108×76** — legibility bump.
- **Preview letterbox** — `fit_preview_rect(available, [u32; 2])` scales a
  `resolution`-sized frame by `min(available.w/src.w, available.h/src.h)`,
  centers with half the leftover on each axis, guarantees 16:9 fallback on
  unusable resolution; the painter strokes the fitted rect (not the
  container); the caption shows the live aspect (`"1.33:1"`) from snapshot
  resolution.
- **P0 scaffolding retires** — `show_p0` checkbox, the status bar chip, the
  central "P0 FRAMEWORK SPIKE" chip, `probe_panel`, `RANGECONTROL
  PROBE`/`P0 CHECKLIST` panels — all removed from the visible UI. The
  `P0State` model stays in-tree (unused by the shell; used by the snapshot
  tests) — confirmed by `shell.rs` compiling.
- **Snapshot re-baselined** — reflects all-modules Gallery (432 registered,
  All default) and the retired P0 sections; size 152565→148702 B.
- New tests: `gallery_filter.rs` (205 ln — filter helper, All semantics,
  whitespace query, author match), `preview_aspect.rs` (92 ln — letterbox
  math on every aspect class), `shell_gallery.rs` (41 ln — integration
  smoke), re-baselined `shell_snapshot.rs` (+70 ln) — recorded verbatim, not
  run.

### Findings

- **[P3] `key()` returning `""` for `All` is a footgun** — an in-tree caller
  that forgot to migrate reads every module; every call site is now
  hand-audited, noted for future edits.
- **[P3] P0State is retained but now unused by the shell** — exact shape of
  the kept-for-tests pattern already used for `is_empty` (Round 78); noted.
- **No P2.** The visible P0 spike scaffolding is gone; the native shell
  resembles the reusable app layout with the All-modules Gallery.

## Round 97 — `252aca6f..85349542` (style: prettier for the notices guard)

**Formatting-only commit; no behaviour change; nothing to review at the
logic level — noted for completeness.**

- `tests/unit/third-party-notices.test.js` — `path.join(...)` call wrapped;
  1 line changed, 4 removed; the guard test re-run claim (55 passed)
  recorded verbatim.
- No P3; no P2.

## Round 98 — `85349542..39d7c1e3` (fix: `.deb` declares `gstreamer1.0-pipewire`)

**Closes the long-outstanding `.deb` gap — the engine's screen-capture
dependency is declared at last, with a regression gate.**

- **The defect** — `electron-builder.yml`'s `deb.depends` listed
  `gstreamer1.0-plugins-base|good|libav` but not `gstreamer1.0-pipewire`,
  while the hybrid supervisor and the Rust `modv-media` probe both require
  the `pipewiresrc` element (`crates/modv-media/src/video/pipeline.rs`:
  `pipewiresrc` element factories gated at :510/:525, and the screen-capture
  availability probe at :553–558 warns "install gstreamer1.0-pipewire"). On
  a fresh `.deb` install, `MODV_ENGINE=1` screen capture would fail with
  `media_plugin_missing`.
- **The fix** — `gstreamer1.0-pipewire` added to `deb.depends`; the new
  `appimage-flags.test.js` spec asserts (a) a `deb:` top-level section
  exists with `depends:`, and (b) a line matching
  `^\s+-\s+gstreamer1.0-pipewire\s*$` — so a future flag flip fails the
  unit suite.
- Author-confirmed the test is red without the YAML change (recorded
  verbatim); not run here.
- The comment in the `depends` block (header noting the supervisor's
  PipeWire expectation) was already accurate — the package entry was the
  only missing piece.

### Findings

- **[P3] The AppImage continues to not bundle `gstreamer1.0-pipewire`** —
  the AppImage is deliberately host-runtime-dependent (PipeWire is
  glibc/driver-sensitive), documented; no action; noted for anyone reading
  the packaging gap between the two artifacts.
- No P2. This closes the `.deb` screen-capture gap tracked since the
  hostility bundle.

## Round 99 — `39d7c1e3..893e891c` (docs: notices texture provenance corrected)

**Completes the notices P1 tail. All claims verified against the tree.**

- **`dvd_logo.svg` row added** — "Wikimedia Commons DVD text logo, extruded
  by the DvdRoom scene | Wikimedia Commons contributors | public domain
  (stated in `DvdRoom.js`)". Verified: the file exists, and
  `DvdRoom.js:6` states "The logo is the Wikimedia DVD textlogo (public domain,
  textures/dvd_logo.svg)" — the notices is now factually consistent with the
  in-tree attribution.
- **ambientCG attribution is now file-scoped, not blanket** — tinore suite,
  rubber002 (diff + rough), moon_04, concrete044c — all verified present with
  CC0 (ambientCG is CC0 by definition; the claim names exactly the files used).
- **Poly Haven skies named** — `stormy`, `spruit_sunrise` — both present.
- **NASA-scoped Earth imagery** — earth/earthlights/earthcloud/wilkyway_panorama
  — these are the NASA-derived files, avoiding the previous (true-at-the-time
  but now-drifted) blanket "Earth/Moon/Milky Way are all NASA" claim which
  incorrectly swept in `moon_04` (an ambientCG asset, not NASA).
- No files were relicensed; unclear assets remain explicitly not-MIT.

### Findings

- **[P3] No per-file licence line for the NASA imagery** — the notices state
  NASA origin by family without individual source links (the AKK
  "BlueMarble"/"VisibleEarth" URLs); a row per file would be stronger; noted.
- No P2. The notices are now file-consistent with the tree.

## Round 100 — `893e891c..6c5f3687` (fix: map Scene cells onto their named rows)

**The native Scenes grid drew Groups in the wrong cells with a
`performance.rowNames` list. Fixed correctly, with the exact
user-reported fixture regression-tested at all three levels.**

- **The defect** — `scene_view` enumerated `groups` by document position,
  so with `rowNames = ["WEBCAM","Background","Foreground","Effects",""]`
  and a Scene storing `[Background, Foreground, Effects]` (no WEBCAM hole),
  Background landed in the WEBCAM cell, hover asked scene_preview for the
  WEBCAM row, and the engine returned "has no renderable Group at row" on
  every hover.
- **The fix** — `scene_view` now calls
  `assign_scene_row_indexes(groups, row_names)` — the exact function the
  apply path uses — so Groups land on their named rows, unnamed/all-empty
  names keep positional mapping (identity), and rows without a Group stay
  empty (the grid shows `Empty`/`{""}` cells). `cells` is preallocated to
  `row_names.len()` only when at least one row name is non-empty — empty
  names → cells vector is grown by position; same shape as JS.
- **Hover-preview gate** — new `should_request_preview` guards the engine
  request behind `hovered && !cell.is_empty()`; empty cells never ask
  scene_preview; "has no renderable Group" spam is gone by design (and the
  unit test pins both arms).
- **Regression coverage at all three levels**: (1) the reported fixture
  exactly (Mycelium Bloom — WEBCAM row must be empty, Background at 1,
  Effects at 3); (2) store-level rowNames round-trip (hydrate preserves the
  saved list, `setRowNames` persists immediately, reload keeps it); (3) the
  `should_request_preview` gate (true/false arms).

### Findings

- **[P3] The WEBCAM row label bug was only visible in the native UI** —
  the legacy code path already honoured rowNames (Scenes.vue rows), so this
  is a straight native-UI gap closed without touching the shared engine;
  correct fix localisation.
- **[P3] No e2e** for the reported fixture — the native path has no UI
  automation yet (P8); here the integration/unit coverage is sufficient and
  pins the fixture exactly.
- No P2. A real defect with a real regression harness, fixed at the right
  layer.

## Round 101 — `6c5f3687..578b802a` (feat: skip performance-disabled Scene cells)

**A genuine native-only feature faithful to the JS's grid model, with
regression coverage at the engine, store and snapshot layers.**
Verified by reading:

- **New `performance.disabledCells` key** `{sceneId: [rowIndex]}` — stored
  sorted and deduped (`set_scene_cell_disabled`), pruned on hydrate and on
  every save (unknown Scenes, out-of-range rows, unaddressed rows dropped)
  so an older/sibling `scenes.json` keeps loading. The JS Scenes grid
  ignores the key and round-trips the document untouched — honest native-only
  extension, explicitly documented as such.
- **`scenes/setCellEnabled {sceneId,rowIndex,enabled}`** — new action:
  validates scene exists, validates the row is a real Group row
  (`get_row_group_index`), updates the flag, bumps the scene-list revision,
  emits `performance/UPDATE_PERFORMANCE`, persists immediately; returns
  `{updated: reason}` envelopes for not-found/empty-cell. Manual `applyScene`
  and cell clicks still render disabled cells (intentional; matches the
  preview/apply split design); only performance picks skip them.
- **Performance picks skip disabled cells** — `pick_next_row_scene_with_disabled`
  (in-order + random) and `resolve_row_advance_scene_with_disabled`
  (incl. the pending-prewarm rejection path) take the disabled map; a row
  whose every candidate is disabled never advances, never crossfades, never
  applies a Scene at start. Test coverage pins: start skips to first enabled
  candidate; fully-disabled row never transitions; in-order dwell advance
  skips the disabled scene-b cell to scene-c; random-order picks;
  prewarm rejection.
- **`scene_view` snapshot flag** — cells now carry `disabled` from
  `is_scene_cell_disabled(disabled_cells, scene_id, row)`; the UI reads it
  through the existing `SceneCellView` wiring (`disabled` default false
  for foreign fixtures).
- **Cross-check verified**: `false` → `set_scene_cell_disabled(disabled=true)` →
  persist → reload keeps the flag; the test walks the full round-trip.

### Findings

- **[P3] The disabled flags are native-only** — the JS grid cannot disable
  cells; a user who mixes Electron and native sessions keeps the document
  coherent (JS round-trips the unknown key) but the JS has no way to
  re-enable them. Documented; no bug.
- **[P3] `row_names.len()` gate for preallocation** — rows disabled on an
  unaddressed row are pruned (`get_row_group_index` checked), so out-of-grid
  indexes can never persist — test pins it.
- **[P3] `disabledCells` prunes on hydrate but only silently** — honestly
  logged on failure path only; correct.
- No P2. This is a well-scoped feature with correct tests at all layers.

## Round 102 — `578b802a..c8993a65` (feat: row/column controls in the Scenes grid)

**The native Scenes grid gains the legacy editing affordances: column and
row controls, drag reorder with drop targets, inline rename, and a
per-cell performance dot. Verified by reading; the committed golden
snapshot is accurate for the new layout.**

- **Column drag** — `begin_scene_drag`/`scene_drop_target`/`drop_scene`:
  dispatches `scenes/moveScene` with the **snapshot-derived** target index
  (same payload as Move left/right); drag from a column header, drop target
  is itself highlighted (`ZOrder`-free), no-target drops cancel cleanly;
  same-id drops are no-op. Unit tests pin move/self/ordering.
- **Row drag + inline rename** — `begin_row_drag`/`row_drop_target`/
  `drop_row` + `row_renaming`/`row_rename_focus`: row reorder splices the
  name out and into the target, matching `Scenes.vue`'s `onRowDrop`;
  `setRowNames` dispatch remaps live Groups (engine arm, verified since
  Round 40-class rounds); the gutter shows all rows, supports inline
  double-click rename with the same Enter/LostFocus/Escape idiom as the
  scene-cell rename; Move up/down and a context menu (Rename/move/delete)
  repeat the row splice logic.
- **Row labels gutter** (`Layers`) — sized `ROW_LABEL_WIDTH = 96`, fits
  two-line labels, scrolls both ways with the vertical grid strip; the new
  `+ Row` appends a blank name and starts the rename in-place (JS's `addRow`);
  `+ Column` dispatches `scenes/addEmptyScene` and scrolls to it.
- **Per-cell performance dot** — red = disabled, green = eligible, click to
  toggle with `scenes/setCellEnabled` **without applying the Scene** (the
  correct semantics for a performance toggle); the Scene-level dot moved to
  the column header.
- **`should_request_preview`/render flow** unchanged — empty cells still
  don't ask for previews; the grid now scrolls both directions so all
  rows/columns stay reachable.
- **Pixel-honest snapshot** — `shell.png` re-baselined (148702 → 153577 B);
  the new gutter, dots, +Row/+Column controls and header name are all
  rendered without clipping or overlap; status bar still honest (`silent`).
- New `shell_snapshot.rs` and `p0_widgets.rs` assertions live in the
  committed file; test claims verbatim, not run here.

### Findings

- **[P3] Dual drop-canceller paths** (`None => cancel_*` inside the drag
  block followed by unconditional `cancel_*`) — harmless double-clear; noted.
- **[P3] `drop_row` clamps target to `names.len() - 1`** — a drop onto an
  index beyond the end anchors to the last row (egui may hand out-of-list
  drops); acceptable.
- **[P3] `row_drop_target == Some(row)` and `row_drag == Some(row)`** —
  same-cell drop is filtered by `source == target` inside `drop_row`; fine.
- **[P3] `+ Column` unconditionally dispatches addEmptyScene without a
  confirmation** — matches the JS's one-click add; noted.
- No P2. The editing affordances are now functionally at parity with the
  legacy grid; the snapshot shows a tidy, legible layout.

## Round 103 — `c8993a65..9c084394` (feat: every Group shows its modules)

**UI bug fixed honestly: the Groups pane only rendered a Group's module
list while that Group was selected.**

- **The defect** — the collapsed-Groups read selected-only for expansion;
  every other Group looked collapsed (smoke-tested by the author: the
  Background selected, the Effects Group showed nothing).
- **The fix** — `group_module_list` factored out as a shared helper; every
  Group calls it; the editors (name/alpha/hidden/inherit/clear/pipeline/
  blend/enable-reorder) stay selected-only because they share the single
  `group_name_draft` state (can't per-Group without a draft map — correct
  scope).
- **Regression kittest** (`groups_panel.rs`) — two Groups, one selected;
  asserts both `Bands (bands-12)` and `Ball (ball-567)` render; pins both
  always-on rendering and editor selectivity.
- **Snapshot re-baselined** — few antialiased pixels moved; nothing clipped.

### Findings

- **[P3] An `empty` label only shows on selected Groups** — unselected
  empty Groups show nothing (by design); noted.
- No P2.

## Round 104 — `9c084394..12c4666e` (fix: enumerate audio devices on demand)

**The 1 Hz cpal enumeration was a real defect — killed at startup +
Refresh-button cadence only.**

- **The defect** — `refresh_audio_devices_if_due` re-ran `input_devices()`
  every second; on hosts without JACK/OSS configured, every second probed
  every backend, flooding stderr with ALSA/JACK errors and exhausting
  pthread keys (author-reported symptom).
- **The fix** — the periodic poll is gone; the device list enumerates once
  in the constructor and only on the panel's Refresh button. `last_audio_scan`
  field removed; the tick no longer calls into cpal.
- Selection correctness unaffected (the picker is the same panel).

### Findings

- **[P3] No user-feedback window for "list may be stale"** — the Refresh
  button label is honest enough; noted.
- No P2. The `refresh_audio_devices` comment now states why (documented).

## Round 105 — `12c4666e..fa163c0d` (fix: performance mode off a fully disabled grid)

**The last hole in the 578b802a feature: the fully-disabled grid no
longer resolves to the legacy whole-Scene autopilot.**

- **The defect** — `startPerformance` conflated "every candidate cell is
  disabled" with "no cells at all" and fell back to the whole-Scene path,
  applying a disabled cell through it.
- **The fix** — two probes: `has_any_candidate` (any renderable cell) vs
  `has_row_candidates` (any candidate *enabled*). No cells anywhere →
  legacy autopilot (empty Scene list). Cells exist but all disabled →
  performance mode toggles with `rows: 0` and the live stack is untouched.
- **Regression test** `start_performance_does_not_apply_a_fully_disabled_grid`
  — a grid of 3 Scenes × 2 rows all disabled; asserts `performance.rows`
  is empty and no Group was created.
- Manual apply still works unaffected; bursts of `setCellEnabled` renames
  are now coherent — the fully-disabled fixture cannot leak through the
  fallbacks.

### Findings

- **[P3] `enabled: true` with `rows: 0` on a fully disabled grid** — the
  performance flag reports on while nothing plays; honest envelope but a
  future "grid disabled" label would be clearer UX; noted.
- No P2. The disabled-cells feature is now airtight; the round closes the
  last edge found in review of the feature itself.

## Round 106 — `fa163c0d..f4495117` (docs: disabledCells in AGENTS.md)

**Docs-only, accurate; the author documents their own feature.**

- The new bullet precisely matches the implementation: `{ "<sceneId>":
  [rowIndex, ...] }` shape, sorted+deduped, prune-on-load/save, red/green
  dots, `scenes/setCellEnabled` toggle semantics, performance-pick skip,
  manual-apply unaffected, fully-disabled-row rule, JS ignoring the key.
- Cites specifically from `scene_rows.rs`/`scene_ops.rs`, which I have
  reviewed; claims match byte-for-byte with HEAD.

No P3/P2.

## Round 107 — `f4495117..6fe0cb50` (test: assert the add controls render)

**Smoke test now asserts presence; nothing else to say.**

- `p0_widgets.rs` adds the three `get_by_label` assertions for the new
  `+ Column` / `+ Row` / `Row names` buttons; covered checks on a
  committed shell render. Test-only; no behaviour change.

## Round 108 — `6fe0cb50..9e54b5e2` (feat: clarify the audio picker)

**The Audio panel becomes honest: an IN-USE banner, resolved friendly
names, a filtered device list, and an unplugged-device state. Verified by
reading and by the committed snapshot.**

- **IN-USE banner** — green dot + resolved friendly name (`active_label`),
  subtitle, and the tech line (`active_detail`: `N ch · NN kHz · id`); an
  unplugged saved device shows
  `"The selected device is not currently connected"` — honest fallback,
  consistent with Round 91's `initial_plan` preview.
- **OUTPUT MONITOR vs INPUT DEVICES split** — System audio and System
  default input get explicit labels/explanations (no raw cpal ids); the
  system default sorts first; the active row renders with a selected
  background + radio; cpal ids live in hover text only.
- **Filter** — new `is_usable_input_device`: empty/0-channel dropped;
  ALSA null/jack/oss/rate-converter/speex probes dropped by id canary
  (`null/jack/oss/lavrate/samplerate/speexrate/speex/upmix/vdownmix`);
  by name marker (`discard all, JACK, OSS, rate converter plugin, /dev/dsp`);
  duplicate id/name collapsed (first wins, preference order is enumeration
  order). 36 raw → 6 usable on the reference host (PipeWire, PulseAudio,
  default ALSA, USB CODEC, 2× HD-Audio) — author-measured, sane.
- **`sample_rate_label`** formats 0 → `unknown rate`, <1000 → Hz, integral
  kHz → `NkHz`, else `NN.N kHz`; unit-pinned.
- **Snapshot** — a dedicated `audio_panel.png` golden captures the full
  panel; the default shell snapshot unchanged (correct split). Banner,
  rows (radio/hover), Refresh footer all render cleanly; `audio: engine`
  in the status bar is the fixture's data.
- Kittest: banner assertions + row selectors + detail/subtitle/sample-rate
  unit tests + host-invariance invariant. All claimed; not run here.

### Findings

- **[P3] The filter could hide a JACK device in a JACK-configured host** —
  the man deliberately filters `jack` and `open sound system` by name; on a
  host where the user *uses* JACK, the panel drops it by design. The
  default fallback (PipeWire monitor/default input) still works; the panel
  cannot select a JACK node. Deliberate product call matching the
  engine's PipeWire-first audio posture; noted.
- **[P3] `default_sample_rate` is `0` on some probes** — handled with the
  `unknown rate` fallback; the banner shows it faithfully.
- No P2. The UX is now honestly informative about the live source and
  useful as a selector; no polling anywhere (on-demand enumeration
  preserved).

## Round 109 — `9e54b5e2..43fa4bbe` (feat: expose expression/tween/meta input views)

**The read model widens for the editor; the builders are pure.**

- **ModuleView gains** `alpha_input_id`, `composite_operation` (spec value),
  `composite_operation_input_id`; **GroupView gains** `alpha_input_id`,
  `composite_operation_input_id`; **EngineSnapshot gains** `expressions`
  (`ExpressionView { id, input_id, expression, evaluate_every_frame }`) and
  `tweens` (`HashMap<id, TweenView>`).
- **Pure extracted helpers** — `module_view`, `group_view`, `expression_views`
  (was inline in the snapshot builder; now unit-testable); behaviour preserved
  byte-for-byte (alpha/enabled/etc. map the same ways).
- **`ExpressionView`/`TweenView` round-trips** — the focused-input editor can
  now read back the expression assignment and the linked tween for any input.
- Tests pin exact payloads for both meta ids and the expression list,
  byte-wise, on engine JSON — honest.
- No functional change to live behaviour beyond exposing these; snapshot
  size grows by the expressions/tweens arrays; fine.

### Findings

- **[P3] The group Blend combo stays group-scope only** — the module Blend
  dropdown is wired in Round 110; the meta input id exists on GroupView but
  there is no link_row invocation for it in this commit alone.
- No P2.

## Round 110 — `43fa4bbe..d38045d9` (feat: port the Input Link audio/expression/tween editors)

**The focused-input editor grows full Audio/MIDI/State/Expression/Tween
sections for meta inputs too — module alpha/blend and group alpha/blend.
Verified by reading; the committed snapshot renders the new Blend dropdown.**

- **Meta-input link rows** — rows now draw for `module.alpha_input_id`,
  `module.composite_operation_input_id`, `group.alpha_input_id`,
  `group.composite_operation_input_id` — the same rows for props — so
  alpha fades and blend modes can be driven by audio/tween/expressions.
- **Module Blend dropdown** — uses the same 29-token `COMPOSITE_OPERATIONS`
  as the group combo (byte-equal; the engine's `BlendMode::from_str`
  accepts all 29 — verified by parsing every entry in both directions).
  Dispatches `modules/updateMeta { metaKey: "compositeOperation" }`.
- **Expression section** — collapsible; defaults open when an assignment
  exists; `window_exp` renders a `TextEdit::multiline` with an "invert" hint;
  Save dispatches either `expressions/create` (no id yet) or
  `expressions/update` (id present) or `expressions/remove` (expression
  cleared/changed to `value`/empty); `updateEvaluation` toggles the
  every-frame flag when id exists; `expression_cleared` is a latch that
  prevents the next snapshot sync from re-adopting a just-removed
  expression (correct guards against stale snap echo). Engine arms all
  verified (`expressions/create`/`update`/`updateEvaluation`/`remove`).
- **Tween resolution** — the tween section adopts descriptors from
  `state.tweens`; the focused editor reads the property's linked tween
  via the round-trip map (`PropView.tween`); create/update/remove flows
  already verified in Round 60/63/110 — this commit keeps them and widens
  to meta inputs.
- **State-link wiring** — `LinkEditor` payloads (`getter`/`state`/
  `mutation`) already verified in Rounds 42/63; unchanged.
- **Pure payload test coverage** (`expression_create_update_and_clear_payloads`)
  pins all three Save branches; payload ordering is update/clear stable;
  the Expression section's feature parity with `InputLinkComponents/Tween.vue`
  is complete for the editor's scope (not keyframes — palette editor stays
  P95-class).
- Snapshot re-baselined (+72 lines of model/shell churn; captures the Blend
  dropdown; the rest is shell noise-free).

### Findings

- **[P3] The Expression TextEdit accepts any string silently** — the engine's
  AST allowlist rejects at compile time (Round 76) with `Compile` errors; the
  UI does not surface those diagnostics inline (only via stderr/logs); a
  "not accepted" annotation would be better UX; noted.
- **[P3] The every-frame checkbox sticks with no live gate** — the engine
  stores it; the UI exposes it honestly; no lenience.
- **[P3] Three Model snapshots in lockstep** — `Cargo.lock` didn't move; no
  kittest for the meta-row wire yet (only payload tests + Alpha/blend unit
  and expression-section kittest). The per-input coverage is unit-level
  excellent but the UI-integration layer remains uncovered; noted.
- **No P2.** The round closes the last of the focused-input editor gaps for
  the native inspector.

## Round 111 — `d38045d9..38c63a1c` (fix: link-editor section gating)

**A same-message fixup: MIDI/Tween/OSC/Audio sections are disabled when
the editor is editing an existing non-matching source.**

- Previously the collapsible section bodies rendered regardless of a
  `editor.exists = true` link whose `source` belongs elsewhere; clicking
  would either silently no-op or, worse, leave a draft in a mixed half-set
  state where the Save button would ship a hybrid getter/mutation payload.
- Fix: each section short-circuits (audio/midi/osc via `add_enabled_ui`,
  tween likewise) unless `!editor.exists || editor.source.is_empty() ||
  source == <kind>` — exactly the correct "mixed edit" gate.
- 38 insertions / 31 deletions; one file (`input_link.rs`); no new tests;
  the payload-level tests are unaffected (`expression_actions` untouched).

### Findings

- No P3/P2. Small, safe fixup; round log rows keep both shas (d38045d9
  remains the full change; 38c63a1c supersedes it semantically).

## Round 112 — `38c63a1c..6eb80170` (docs: full Cargo.lock dependency inventory)

**The last notices leg closes — every registry crate in the lock file.**
Verified independently, not just by the author's test.**

- **The inventory is exact.** I parsed `crates/Cargo.lock` independently:
  626 `[[package]]` entries — 7 first-party `modv-*` (no `source`) and 570
  registry packages. **0 registry crates missing from the notices** — every
  one of the 570 names appears in the `## Rust engine dependencies` table.
  The commit's "570-crate inventory" claim is exact, not approximate.
- **Row integrity is enforced by a real test**, not spot checks:
  `tests/unit/third-party-notices.test.js` gained the `## Rust dependency
  graph` suite — it hand-parses `Cargo.lock` (`[[package]]` blocks,
  name/version/source), de-duplicates by name, and asserts:
  - the scanner is sane (>500 registry crates; `wgpu`/`winit`/`naga`/`tokio`
    present; workspace crates are exactly the seven `modv-*`);
  - one row per registry crate — every registry name has a row;
  - **no made-up rows** — every table entry resolves in Cargo.lock (stale
    entries fail the build);
  - rows are sorted and one-per-crate;
  - every row carries a valid **SPDX expression** (a hand-rolled
    AND/OR/parens/WITH parser fronting a curated identifier set — a real
    expression grammar check, not a regex — rejects shorthands and
    free-text);
  - every row links to a plausible upstream (known-forge host or URL
    containing the crate name; HTTPS-only);
  - **obligation licences are spelled out** — an obligations matcher
    (MPL-2.0/CC-BY/NCSA/OFL-1.1/Ubuntu-font), requires each obligation
    licence present in the graph to have an explicit bullet in the notes,
    and each bullet must name at least one crate from the table. Wrapped
    bullet lines are joined before the check.
- **The notices now carry the obligations** verbatim for: `colored`
  (MPL-2.0 file-level copyleft, header + distribution obligation),
  `epaint_default_fonts` ((MIT OR Apache-2.0) AND OFL-1.1 AND
  Ubuntu-font-1.0, attribution + forbids selling the font), and
  `libfuzzer-sys` (NCSA). Non-permissive crates are exactly those three
  obligations — the spelling is accurate for the two-week-old lockfile;
  new ones will fail the suite loudly.
- **No Rust file changed; no Cargo.lock change; the 570-crate table is
  generated from cargo metadata (the commit says regenerated via
  `cargo metadata --format-version 1 --locked --offline`) and sorted;
  canonicalisations are SPDX (MIT/Apache-2.0 → SPDX `OR` form), not
  relicensing.** The Rust-Crate-graph attribution axis is now fully
  covered.

### Findings

- **[P3] The obligation bullets for MPL/OFL/NCSA describe obligations but
  do not embed the licence texts** — like the rest of the notices, names and
  pointers rather than full text; enough for attribution; noted.
- **[P3] SPDX whitelist needs a new entry when a new SPDX identifier
  appears in the graph** — by design; loud failure with a clear message is
  the right failure mode.
- **[P3] No CI** runs this yet (no lint/test workflow in CI) — consistent with
  the rest of the suite; noted in passing.
- **No P2.** This closes the notices P-1 fully — the file-level NC-SA
  scans (Round 90), texture provenance (Round 99), and now the entire
  Cargo.lock graph.

## Round 113 — `6eb80170..cd0eacc3` (feat: mint the six Group control inputs in createGroup)

**The last genuine Group-create parity gap closes. Verified line-by-line
against `groups.js:174-197`.**

- **Exact JS minting ported** — six controls `alpha, enabled, clearing,
  inherit, compositeOperation, pipeline` (same order), each a `commit`
  input with `location: groups/UPDATE_GROUP_BY_KEY` and
  `data {groupId, key}`, dispatched **before** the `groups/ADD_GROUP`
  delta, with the id stamped on the Group as `<name>InputId`. The Rust
  deltas appear in precisely that order (verified against the machine-emitted
  `tests/fixtures/engine/deltas.json`: position 0-5 are six `ADD_INPUT`s and
  index 6 is `groups/ADD_GROUP` for the first Group, repeating).
- **"Pre-supplied ids trusted verbatim" is exact** — JS only mints for
  `undefined`; Rust skips only when the params map contains the key. A
  present-but-`null` id keeps the Group's stored `None` — *not* replaced
  with a fresh minted input. Matches the JS's `group[idKey] = null` outcome.
  (Note: Rust omits the key entirely from the serialised Group since
  `skip_serializing_if = "Option::is_none"`, while JS writes `null` — a
  benign, documented asymmetry: the reload path accepts both.)
- **`removeGroup` now cleans all six inputs** — `REMOVE_INPUT_LINK` then
  remove each; the cleanup test asserts six `REMOVE_INPUT` deltas fire;
  and the round-trip test proves create → save → restore keeps ids without
  minting duplicates.
- **Fixtures bump coherently end-to-end**: `EXPECTED_TYPES` 36→54 (18 group
  control inputs), snapshot 17→35 inputs, placeholders 7+17→7+35, supervisor
  delta assertions updated (19-delta group-prefixed sequence; delta positions
  resequenced), and the engine-mirror test asserts the six `UPDATE_GROUP_BY_KEY`
  commit inputs are commit-side with no `getLocation` — the JS mirror sees
  the same record shape.
- Byte-identical clean-up of the previously-documented workaround
  ("bind `input-a` manually to exercise the cleanup path"); the tests now
  assert the authored behaviour directly.

### Findings

- **[P3] `null` vs absent `*InputId` in the serialised Group** — noted above;
  spec-compliant with the reload reader; one deliberate deviation from JS
  serialisation that round-trips fine.
- No P2. This closes a real parity deficit: native createGroup now exposes
  the same Alpha/Blend link rows the JS worker has always minted, so
  `groups/createGroup` from either lane writes matching documents.

## Round 114 — `cd0eacc3..9e4a5a40` (test: smoke-room re-baseline on the real curl field)

**The one pre-existing failing Point10 GPU test closes — by re-baselining
an assertion that was calibrated against the broken precompute state.**

- **Why the test was red** — `smoke_room_volume_is_structured` required
  `dark_fraction > 0.15` (a mostly-dark frame, a bounded room). Since
  `cf9be174` fixed the volume curl-noise precompute to run with real
  uniforms, the closed room legitimately fills with dye and every ray
  picks up light — the failure is correct behaviour, not a regression.
- **The old threshold was against the bug** — evidence reproduced by the
  author: fire-disabled frame (intensity/density 0) → `dark_fraction = 1.000`
  (glass/bloom/clear stay near-black), confirming the frame-wide orange is
  the raymarched dye field itself; the emissive is temperature-gated
  (`fire = fireRamp * temperature^pow * uFireIntensity` →
  `density.add(0.15)` atten) exactly like `SmokeRoom.js:953`; scattering and
  the point/key lights are density-gated; the test's 1:1 aspect makes the
  sim box overflow the frame (production 16:9 keeps side bands), so no ray
  can stay dark once the field develops.
- **The new assertion** encodes the true bounded-room invariant:
  `luminance_extremes(0.05)` — the brightest 5% must exceed the dimmest 5%
  by a factor > 3.0 (measured on lavapipe: 248 vs 31; a flat sheet scores
  ~1, a dead emitter ~0/0). `assert_alpha_is_max_rgb`/`assert_translucent_layer`
  retained.
- **Reading the change as a re-baseline, not a silence-the-alarm** — the
  commit explicitly names the recalibration reason, ships the new assertion
  instead of an `#[ignore]` or a deletion, and the metric is now a ratio
  robust to absolute-exposure drift.
- Author claims lavapipe + radv agree on the new bound; not run here;
  recorded verbatim. The full Point10 GPU suite otherwise unchanged —
  788 passed/1 failed story exits, presumably (author's working tree;
  recorded, not re-run).

### Findings

- **[P3] A 1:1-frame guard** — production 16:9 keeps dark bands; the
  square test view saturates the room; nothing testable at 16:9 this round;
  fine.
- **[P3] The gradient assertion could mask a catastrophic fully-saturated
  frame** — the ratio condition catches a dead field (0/0) but a fully
  uniform white frame also passes (inf/inf). The frame's quantised 248/31
  check in the commit message mitigates; note it, no action needed.
- No P2. **The Point10 suite has no known-red tests anymore** — closing
  the last recorded failure; the GPU tier is green in intent.

## Round 115 — `9e4a5a40..11446c5a` (feat: mint data-type component sub-inputs)

**The second Group of this window: every native Input Link row that exists in
Electron now exists in the Rust engine, for component-typed props.**
Verified line-by-line against the JS.

- **Byte-faithful port of `initialiseModuleProperties`** (`modules.js:91-113`):
  a prop whose data type exposes component inputs (color → r/g/b/a,
  vec2/3/4 → 0..N) owns one sub-input per component, named `${parent}-${key}`,
  `type: "action"`, `data {moduleId, prop, path: "[key]"}`, a `getLocation`
  pointing into the parent's prop at `[key]`, location `modules/updateProp`
  — identical to the JS records.
- **Registration call-sites** — minted for `make_active_module` (with the
  Gallery exemption) and `build_live_module` (Scene apply path);
  **drop-with-parent** on `remove_active_module` (`modules.js:798-819`).
- **Shared table**: `modv_engine::inputs::data_type_input_keys` — one
  compiled-in table for the three minting sites plus the UI view builders,
  replacing `scene_cells`' private copy.
- **Scene apply-mint round-trip is faithful** to `mapModuleInputIds`
  (`scenes.js:426-433`): `${oldId}-${key}` → `${newId}-${key}`, the union of
  old+new prop types drives the union of keys — exactly the JS semantics;
  `apply_scene_mints_and_remaps_component_sub_inputs` exercises the full
  flow (fresh ids, remapped `doc-fill-r` → `{newParent}-r`, stale id absent).
- **Native Inspector** lists each existing component as a linkable row
  under its prop (`${prop}.${key}`), showing only sub-inputs that actually
  exist in the engine's registry. The goldens are unchanged (demo module
  has no color/vec props; honest).
- Fixture suite and mirror gate unchanged (scalar/tween only) — no parity
  churn.

### Findings

- **[P3] Component-minted-but-inputless modules** — explicit guard; Gallery
  module mints zero inputs (asserted); correct library hygiene.
- **[P3] Outstanding item 7 degrades to its sibling gap** — the
  component-minted half closes here; the `PropView::{label, step,
  component, enum}` side stays open; the item detail is updated to match.
- No P2.

## Round 116 — `11446c5a..e1acbf55` (test: shared lavapipe adapter)

**Test-infra SIGSEGV fix; the GPU test story stabilises for parallel
`cargo test`.**

- **The defect** — each GPU harness (ISF runtime, Point10, 2D) created a
  fresh `wgpu::Instance`/fallback adapter per test thread; parallel
  lavapipe instances SIGSEGV; `cargo test -p modv-renderers --lib` aborted
  (and previously "passed" only when serialised manually).
- **The fix** — new `#[cfg(test)]` `crate::test_gpu`: `OnceLock<Option<
  (Device, Queue)>>` + `AtomicUsize` init counter; every GPU harness now
  calls `test_gpu::gpu()` (Arc-backed clones per test), zero per-test wgpu
  instances; a counting regression `gpu_calls_share_one_adapter` asserts
  eight `gpu()` calls create at most one adapter/device pair; the
  `block_on` duplicates were removed everywhere (all delegate to the new
  shared module).
- **The claim "Full parallel lib run: 792 passed where it previously
  segfaulted"** — recorded verbatim; not re-run here.

### Findings

- **[P3] `SHARED.get_or_init(init).clone()` on every call** — the `OZ`
  guard counts device creations *actually* performed, and the clone is a
  cheap Arc bump; correct.
- No P2. This is an important test-infra fix; the 2D/ISF/Point10 GPU
  surface is now exercisable in parallel on lavapipe.

## Round 117 — `e1acbf55..75e7f116` (fix: un-premultiply preview readbacks)

**The native preview path now decodes the compositor's premultiplied RGBA
instead of flattening it — translucency is restored end-to-end.**

- **The bug it kills** — the preview readbacks ship premultiplied RGBA; the
  native upload used to force `alpha = 255`, leaving the alpha-scaled RGB as
  an opaque dark fringe over light backgrounds (the exact symptom item 1
  names: "translucent tiles render darker than the live frame"; the R-20
  finding from Round 22).
- **The fix** — a new pure `modv_ui::un_premultiply_rgba` (round-to-nearest,
  clamped, 0-α stays 0, opaque unchanged, malformed samples clamp to 255,
  never wraps) applied in `upload_preview` before
  `ColorImage::from_rgba_unmultiplied`. egui CPU textures premultiply on
  upload, so un-premultiplied input round-trips correctly.
- **The GPU path is explicitly skipped** — the live main output texture is
  registered with `egui-wgpu` as a native texture; its `One/OneMinusSrcAlpha`
  blend consumes premultiplied content directly; converting would
  double-apply.
- **Readback origin** — the helper doc names the sources: `read_main_rgba8`
  and the isolated Gallery/Scene preview renderers. All flow through this
  one upload path.
- **Unit tests** cover the restoration (exact triple), opaque-pass-through,
  zero-α pass-through, the ±2 tolerance round-trip over four samples, and
  the malformed-sample clamp.
- The progress doc gets the one-line note; the legacy `putImageData` sites
  stay.

### Findings

- **[P3] The legacy Electron `putImageData` sites are unchanged** — the
  native shell inherits the fix but the legacy app still paints premultiplied
  bytes as straight alpha at the sites named in item 1
  (`compositor.rs:2793` → `Scenes.vue`; `module_preview.rs:524,590`). Item 1
  narrows: the native axis closes here; the JS one remains.
- No P2. The Round-64-era leak is closed natively; the JS half honestly open.

## Round 118 — `75e7f116..73feb02a` (fix: marquee long Gallery names on hover)

**A genuine legibility fix; the truncation user complaint is dead.**

- **The defect** — Gallery tiles rendered names with `truncate(&module.name, 12)`,
  so long names showed as `It Is Comin~` with no recovery path.
- **The fix** — full name via `layout_no_wrap`; if it exceeds the strip, it
  marquees on hover with a ping-pong cycle (0.5 s hold at start, 28 px/s
  travel, 0.8 s hold at the end, then back). Strip clips horizontally
  against the tile border; the type label, selection fill, click/double-click
  and context menu are unchanged. The separate `gallery_name_hover: Option<(String, f64)>`
  state starts the timer on hover-enter and clears on leave, restarting
  cleanly on re-hover.
- **Pure helpers** — `marquee_offset()` (clamped, hold/travel/hold/back/reset)
  and `gallery_name_layout()` (overflow > 0 ⇒ clipped strip), both
  unit-pinned: zero overflow never moves, start hold stays at 0, linear
  travel at `speed`, end hold and back-travel, full cycle returns to 0;
  short names unchanged; `strip_width - text_width` overflow sign correct.
- No UI regressions otherwise; snapshot layout unchanged.

### Findings

- **[P3] `ui.ctx().request_repaint_after(16ms)` per hovered tile** — one
  repaint per frame while hovered; the marquee needs it; fine.
- **[P3] `truncate()` still used for the type label** — deliberate; noted.
- No P2.

## Round 119 — `73feb02a..56745632` (feat: per-row layer monitors)

**The last big native-UI feature delta: every row gutter shows a live
thumbnail of the row's Group output. Verified by reading; both snapshots
honest.**

- **Pixel source — the compositor's own Group texture** (design option (a)) —
  plain GPU-GPU copy into a per-Group snapshot; no read-back, no extra pass,
  no isolated state re-materialisation; the `scene_preview.rs` route (b)
  would have re-rendered everything; (a) is the right choice.
- **Premultiplied content** — consumed by egui-wgpu's blend directly; no
  un-premultiply step on this path (consistent with Round 117's rule).
- **`row_monitor.rs`** — pure policy: at most one row refreshes per frame;
  `stride = budget / rows` (~20/rows) keeps every row in a ~3 fps band;
  a rowless grid refreshes nothing; display policy covers live/dim (35%)
  vs NoLayer placeholder. Unit tests pin stride math, cadence, no-layer, dim.
- **Compositor side** — `refresh_row_monitor` copies the Group's rendered
  texture into the per-Group snapshot (output-sized), returns false when
  the Group never drew; `row_monitors` map keyed by Group id; prune on Group
  prune / resize / reset; GPU test proves copy + prune drops stale.
- **Scene-apply lifecycle** — fresh Group ids on apply; monitors never
  survive an apply; stale snapshots drop with the Group texture (correct).
- **No renderer disposal anywhere** — only compositor-owned GPU textures are
  copied; shared ISF vertex buffer and cached 2D/Point10 instances untouched.
- **Leak discipline** — shell frees egui native textures as soon as a row's
  Group changes; no leak path for ids that were renamed/rebuilt outside apply.
- **Goldens** — `scene_grid_row_monitor.png` and `scene_grid_row_monitor_dimmed.png`
  show both arms (rows 1/2 black live thumbnails vs Row 3 `no layer`); the
  dimmed variant visibly dims. The shell golden is re-baselined (gutter plate
  change); nothing clipped.
- The demo fixture remains honest (`audio: silent`, `BPM 0`-pref but present).

### Findings

- **[P3] Row monitors repaint at 3 fps while performance runs** — at 6
  rows × ~18-frame stride, each row's motion is stuttery-but-accurate; by
  design (budget-aware); noted.
- **[P3] No manual refresh/intensity slider** — refresh cadence is hard-coded;
  a UI knob would be the next (deliberate) product step; noted.
- No P2. The feature is wired correctly, snapshot-safe, and matches the
  JS's "no monitor" baseline plus one.

## Round 120 — `56745632..4c3af40f` (feat: performance Scene order control)

**The native bar gains the last missing Electron control — `Scene order`
(In order / Random) — wired end to end with a persisted round-trip.**

- Engine already honoured `performance.order` at every pick site and in
  `updatePerformance`; only the UI was missing.
- **Model** — `PerformanceView` gains `order` with a manual `Default`
  (`in-order`); `normalize_performance_order` collapses anything but
  `"random"` to `in-order` (unknown saved values show the behaviour
  that will actually happen); `PERFORMANCE_ORDERS`/`PERFORMANCE_ORDER_*`
  constants; label/value helpers unit-pinned.
- **UI** — `Scene order` ComboBox renders in `performance_bar` (enabled
  while performance runs, so a live change applies to the next pick);
  dispatches `scenes/updatePerformance` with the bare `{order}` payload —
  exactly the shape the engine's flat-key merge path handles.
- **Engine behaviour** — verified `updatePerformance {order:"random"}`
  round-trips through `scenes.json`: live, persisted, reload; the reloaded
  store with `Rng::seeded(7)` picks Gamma (in-order → Beta); switching the
  live value on the same dispatch path applies to the very next pick
  without a restart — the exact test pinning all three.
- Goldens re-baselined (shell + row_monitor).
- `the_performance_bar_labels_the_saved_scene_order` and
  `choosing_random_while_performance_runs_dispatches_the_order` kittest
  cover the saved-label render and the live-changed action.

### Findings

- **[P3] The toolbar overflows slightly on squeezed widths** — the bar now
  carries seven widgets and "1 Scenes" text clips against "+ Row" edge;
  cosmetic; the default-width shell golden passes; noted.
- No P2.

## Round 121 — `4c3af40f..0263f2ee` (feat: anime multi-keyframe tween sampling)

**A byte-grounded correctness port of the hardest JS tween semantic.**

- **The change** — the Rust tween builder now treats 3+ rows as one tween
  per keyframe (anime 3.2.2's keyframe semantics): each segment lasts
  `duration / N` (after the inner `duration / data.length` division, i.e.
  wall-clock segments = `duration / N / N`), the first segment interpolates
  row-0→row-0 at `duration·(1/N)/1` of the run (effectively a hold — exactly
  what anime does because it interpolates from the target's current value),
  and **easing/`steps` quantisation apply per segment**, not once across
  the run.
- **Measurement-based port** — `tools/tween-keyframe-reference.mjs`
  duplicates the worker's `buildFrames` verbatim (the `newDuration` rule,
  the one-tween-per-row path, the frame recorder) and runs it against the
  installed animejs 3.2.2 with representative 3/5-row, linear/EaseInOut
  Quad/Cubic, looping/one-shot, multi-channel, steps and BPM configs. The
  output `data/tween-keyframe-reference.json` records raw anime output plus
  the timing structure.
- **The Rust port is line-by-line against the keyframe semantics** —
  `active_keyframe_tween` reproduces anime's strictly-greater-than-seek-end
  selection with the `t => insTime < t.end)[0] || last` fallback;
  `normalize_elapsed` keeps the NaN elapsed (zero duration → complete frame
  like anime); easing/steps applied on segment-local progress only.
- **Tests compare frame 0 (reset_frame, direction reverse end point),
  intermediate frames and loop ping-pong restored legs**; existing 2-row
  reference fixture untouched; 3 new reference tests cover multi-keyframe
  frames/hold/ping-pong.
- The `Cargo.toml` refs the fixture; `x`-count asserts we have proper indices
  (`Parity` table lists parity of frame counts/durations in the reference).

### Findings

- **[P3] The fixture hard-pins anime 3.2.2** — an anime upgrade must
  re-run the generator; documented; fine.
- **[P3] The reverse-direction path frames match but cycles ≠ duration-exact**
  — loop ping-pong restored leg counts match anime's count including both
  endpoints; existing 2-row tests cover the rest of this axis; noted.
- **No P2.** This is one of the best-characterised ports in the rewrite —
  the reference fixture is the ground truth and it stays committed.

## Round 122 — `0263f2ee..e555ba04` (test: reverse-direction 3-row fixture)

**Same-shape fixup: the reference fixture gains the missing reverse phase.**

- Adds `three-row-linear-reverse-10` to the generator config and the
  fixture (anime's `reverse` direction with 3 rows — `reset()` installs
  the last keyframe, then seek applies the reversed-time adjustment twice,
  so frames 1.. sample normally), with a full `animation` echo.
- `multi_keyframe_frames_match_anime` iterates every config, so the reverse
  path is now frame-exact (±1e-9) against animejs 3.2.2 without any code
  change — the Rust `direction == "reverse"` `reset_frame` branch (sample
  at `new_duration`) already matched the contract.
- 88 new lines in the fixture (+3 config timings note), 17 in the generator;
  no code change.

### Findings

- No P3/P2. Same-message same-scope fixup round; Rust code untouched;
  fixture is strictly stronger now.

## Round 123 — `e555ba04..7202749d` (feat: free keyframe times in tweens)

**A genuine Electron-readable native extension — tween keyframe drag on
both axes.**

- **Contract** — `keyframeTimes: [number,...]` normalized 0..1 progress for
  each `data` row, one per row, strictly increasing, first 0 / last 1;
  validated at load and at every sample (`effective_keyframe_times`
  re-validates against the current data's row count because the field is
  public).
- **Behaviour** — valid times replace anime's even keyframe spacing (and
  its leading hold) with piecewise segments; the descriptor's easing/`steps`
  ease each **local** segment; invalid lists fall back silently to the
  anime multi-keyframe semantics (wrong length, wrong endpoints,
  non-monotonic, non-finite), and invalid raw values are kept verbatim in
  `TweenDescriptor::extra` so load → save does not lose them.
  `RawKeyframeTimes` distinguishes absent (`null`) from plain missing; a
  two-row `[0, 1]` sampling is bit-identical to even spacing.
- **Serialisation** — typed `keyframeTimes` present when valid; the raw
  invalid key stays in `extra`; `is_gallery`/steps/other keys remain.
- **Beyond the editor** — `create_tween` forwards `keyframeTimes` from JS
  params. Test coverage: round-trip byte parity, invalid-list fallback
  equivalence with even-spacing (exact frame assert), programmatically
  non-finite times can't panic, absent key absent on round trip, the free
  positions respect local easing, `ri`-row ordering, and the preset
  `state.tweens` carrying them back.

### Findings

- **[P3] Electron evaluates whatever times are written** — an Electron-side
  load → save → reload with an invalid list keeps the document intact; the
  JS never reads `keyframeTimes` (semantically fine).
- No P2.

## Round 124 — `7202749d..b6ae28a0` (feat: remember, reorder and restore webcams)

**Persisted Webcam selection, order and modes — mirrors `mediaStream`.**

- **Document** — `<userData>/native-media.json` `{cameras: [id,...], modes:
  {id: {width, height, fps}}}`; the ordered list is the camera prop order
  (1-based). Unplugged cameras kept (resume on replug); explicit stop
  removes the id and suppresses auto-reopen; every save is atomic tmp+rename;
  failure surfaces to `record_error`/`health.last_error`, never crashes.
- **Restore** — on first V4L enumeration, saved cameras start in saved
  order; a saved mode that no longer advertises resolves to the exact
  advertised mode, else same resolution at closest fps, else the next
  smaller resolution at max fps, else the device max; a camera with no caps
  falls back to 1280x720@30; missing caps fill 0; unsaved cameras wait
  pending in the 1 Hz poll. Fallback normalises the document.
- **Y4L 1.0x restart coverage** — `set_camera_order` reorders camera-like
  sources only (files keep their slots, no pipelines restart); the status
  indexes are rewritten to be **camera-only 1-based** (the Webcam prop
  matches, `reconcileCameraIndex` agrees; files report 0).
- **Native panel** — per-camera checkbox (run/stop and 1-based index #N
  badge), Up/Down reorder for running cameras (no pipeline restarts),
  hover hint; camera row shows `{name}` with `{N}` badge and selected
  state for running ones.
- **Kittest + unit coverage** for the check/uncheck/reorder/badges; the
  default shell golden is unchanged (correct — Media isn't its tab); panel
  counts as `3 inputs`, honest.
- The persisted `mediaStream` equivalent docs note added; window note stays
  MOC (this is a real write target, not a diagnostics-only store).

### Findings

- **[P3] `native-media.json` is a second persisted file alongside `native-audio.json`**
  — fine, mirrors the Electron `vuex-persistedstate` convention (`mediaStream`
  key is separate from anything audio).
- **[P3] No UI to reorder cameras while stopped** — reorder only for running
  cameras; by design.
- **[P3] `restore` only fires on enumeration-only devices** — restore order
  across flat screens PID duplicates (two identical webcams at different
  paths) — fine.
- No P2.

## Round 125 — `b6ae28a0..c211457e` (feat: webcam resolution and frame-rate controls)

**The Media panel renders the advertised camera modes and switches live.**

- **`CameraView` gains** `modes` (deduped/sorted) and a resolved `selected`
  mode, built in the NativeShell snapshot from `CameraInfo.caps` — the whole
  V4L2 capability surface the hybrid renderer already queries.
- **Pure helpers, heavily unit-pinned** — `unique_resolutions` (largest
  first), `fps_options` (fastest first, ignores fps 0 = "not advertised"),
  `default_camera_mode` (resolution then rate), `mode_for_resolution`
  (preferred fps when advertised, else fastest rate there), `CameraModeView`
  (pixels for ordering; labels `1920×1080`/`60 fps`). All with unit tests
  incl. empty lists, all-zero modes, dups and fps 0.
- **ComboBoxes** per resolution + frame-rate; change → `MediaAction::StartCamera`
  with the mode; shell stops/restarts the source at the chosen mode,
  re-maps Webcam slot with `set_camera_order`, rewrites `native-media.json`
  (replace semantics; the saved mode is normalised against advertised caps).
- **No fake choices for caps-less cameras** — the UI says "mode not reported"
  with hover text instead of inventing options; the fallback stays the
  documented 1280×720@30.
- **`camera`-only index consistency** — `MediaHub::start` now returns the
  camera-only 1-based Webcam index (0 for files), matching
  `MediaSourceStatus.index` and `reconcileCameraIndex`.
- The shell calls `resolve_mode` on the request and answers with an exact
  or nearest advertised mode — never an unadvertised one.

### Findings

- **[P3] `1920×1080` uses the `×` glyph** — egui default fonts carry it;
  fine; noted.
- **[P3] No live reload of the running source's mode** — needs a stop/start;
  the action does exactly that; caveat documented on hover; fine.
- No P2.

## Round 126 — `c211457e..f2361b01` (feat: convert even keyframes to explicit times)

**The curve-editor migration helper: even anime 3+ row tweens normalise to
identical free-keyframes tweens, pixel-exact.**

- **The conversion** — `with_free_keyframes()` duplicates row 0 and
  re-expresses the total duration (`duration / N` or `bpmDivision / N`),
  clamping to `duration_as_total_time` semantics; `effective_keyframes_times`
  guards validity; 2-row `[from, to]` and already-timed descriptors are
  untouched.
- **The fast path** — `evenly_spaced(times)` detects the exact grid
  (equality on every `index / (len - 1)`), and when it holds,
  `active_keyframe_tween` is used (anime's segment arithmetic) instead of
  the normalised fractions — so the conversion is bit-exact for non-BPM
  tweens, and ≤1e-12 drift for BPM (compensation of the `(60*60/bpm)*division`
  re-expression for the extra row).
- **Tests are thorough**: `free_keyframe_normalisation_preserves_wall_clock_playback`
  (3/5 rows, both durations, 60/30/144 fps, loop + no loop, frame-exact
  equality), `..._keeps_the_bpm_cycle` (120/128/174/0 BPM, frame count
  identical, drift ≤ 1e-12), `normalisation_only_rewrites_evenly_keyframed_multi_row_tweens`,
  and the even-times fast path test.
- **Edge cases** — invalid `times` sample-compatible with even spacing by
  construction (all callers route through `sample`); zero-duration/NaN
  elapsed clamped; empty list `times[0] == 0` gate; two-row short-circuits.

### Findings

- **[P3] The duplicated first row is observable in `tween.frames.len()`** —
  a normalised tween builds one more frame than the even one; the editor
  will drop that row on save. Cosmetic; the expected algebra accounts for it.
- **[P3] No `--convert-even` CLI** — conversion only via `with_free_keyframes`
  in-code; editor hooks will call it; fine.
- No P2.

## Round 127 — `f2361b01..5f832d5a` (fix: report the capture rate, not cpal's 44.1 kHz heuristic)

**Honest rate reporting — the device table now shows both the verbatim
advertised rates and the rate capture actually negotiates.**

- **The defect** — `InputDeviceInfo.default_sample_rate` read cpal's
  `default_input_config()`, whose ALSA backend hard-codes 44.1 kHz whenever
  44.1 kHz sits inside an advertised range, so every picker row claimed
  44.1 kHz even though capture chose 48 kHz whenever a range covered it
  (`choose_config` in `AudioCapture::start` already did this).
- **The fix** — new pure `capture_rate(default_rate, ranges)` (the mirror
  of the `choose_config` decision, device-free) and `device_info()` (assembles
  the row — advertised extremes plus capture rate without a device);
  `InputDeviceInfo` gains `capture_sample_rate`; `describe_devices` prints
  `capture rate: N Hz` **alongside** the verbatim `sample rates (Hz)`
  advertisement (not in place of it).
- **Round-trip honour** — the rate reported is what capture *would* open
  with (the actual is known only after the stream starts); the boundary
  cases are covered: V2 range covering 48 k → capture 48 k, exotic 8 k..96 k
  ranges with one 44.1 k-advertised device → capture 48 k, virtual ALSA
  1..=u32::MAX devices → capture 48 k, no-48k-range → falls back to cpal default,
  no default → falls back to `DEFAULT_SAMPLE_RATE`.
- **Picker unchanged** — `default_sample_rate` field and `AudioCapture::start`
  are untouched; the native UI reader changes come separately (plumbing
  is declared out of scope by the commit, a comment on the table notes).

### Findings

- **[P3] The `--list-audio-devices` output grew a `capture rate:` line** —
  fine; the table is explicit now; noted.
- No P2.

## Round 128 — `5f832d5a..e2eb0680` (feat: read tween keyframes and cycle length into the view)

**The tween read model now carries exactly what the curve editor needs.**

- **`TweenView` gains** `data`, `keyframe_times` (drops invalid lists;
  falls back to even spacing in sampling), `durationAsTotalTime`, and
  `cycle_seconds` (`effective_duration(fps?)` —  the engine's pre-render
  length at the live BPM, converting to seconds for the beat/seconds axis).
- **Descriptor decode** — `tween_view_from_json` round-trips through
  `TweenDescriptor`, so every view shows the same validated fallbacks the
  engine uses (invalid lists → even spacing, `bpmDivision`/`easing`/`loop`
  defaults from the descriptor, not hand-rolled ones).
- **Behaviour preserved** — the `state.tweens` JSON is still the source of
  truth; the builder tolerates invalid descriptors by retaining raw fields.
  BPM read via `self.store.bpm()` (the same tap-promoted field the
  meters/clock use today).

### Findings

- **[P3] `cycle_seconds` uses current BPM at snapshot build time** — a BPM
  change does not re-render until the next snapshot build (next frame); fine.
- No P2/P3 beyond note.

## Round 129 — `e2eb0680..f9500bf6` (feat: draw and edit tween curves with keyframe handles)

**The curve editor lands — drag points, interpolate display, round-trip save.**

- **Model (`tween_curve.rs`)** — `TweenCurve::from_view` normalises evenly
  keyframed multi-row tweens to explicit times (duplicate first row + even
  grid — byte-identical playback); two-row stays `[from, to]` with implicit
  `[0, 1]`. `sample_channel` linear-interpolates for display (the easing is
  playback's business); `insert_point` interpolates other channels at the
  click time; `MIN_KEYFRAME_GAP` keeps rows ordered; endpoints pinned;
  `nearest_channel` chooses which channel follows a click; `value_bounds`
  pads the value span.
- **Editor (`tween_control.rs`)** — per-channel color overlay (6 colours),
  grid lines/labels, draggable handle glyphs, right-click remove,
  click-time add, raw JSON/Inspector sync; save dispatches `tweens/createTween`
  with the descriptor, exactly Round 60's path; the first edit on an even
  tween transitions to the `with_free_keyframes` shape the engine
  plays back identically.
- **Wire** — `create_tween` forwards `keyframeTimes`; the curve restores
  explicit times on the descriptor; the two-row shorthand untouched. The
  committed `tween_curve_editor.rs` kittest drives the actual harness
  (click-to-add, drag-to-move, right-click to delete, descriptor dispatch).
- **Snapshot re-baselined/virgin** — new `tween_curve_editor.png` shows BPM
  128, two channels, time/value grid, `.96 beats · 0.48 s at 120 BPM`, the
  two-curves handles, JSON block visible — honest proof of the full flow.

### Findings

- **[P3] The editor does not render the easing curve** — by design (handle
  grid is linear in position); noted.
- **[P3] No undo** — the underlying descriptor commit is the undo boundary;
  noted.
- No P2.

## Round 130 — `f9500bf6..006c9704` (fix: curve editor cosmetics and channel-cycling)

**Same-message fixup: wrap tween rows, reposition Edit curve, cycle channel
colors, hit-test all channels.**

- `horizontal` → `horizontal_wrapped` in the tween control and curve-handle row
  (avoids clipping on squeezed widths); the Edit curve button joins the
  first-button position.
- `CHANNEL_COLORS` now cycles via `iter().cycle()` (previously truncated at 6
  colors with no way to pick more channels); `nearest_handle` drops its own
  6-channel cap so draw-loop and hit-test agree.
- New unit test pins the JSON-branch payload (`keyframeTimes` dispatch).
Goldens: only `tween_curve_editor.png` re-baselined.

### Findings

- No P3/P2. The channel cycling is a necessary-not-sufficient — over 6
  channels the repeat colours are ambiguous (ch 0 == ch 6); correct in-colour
  terms; noted.

## Round 131 — `006c9704..6fa788a8` (feat: persist resizable panel and window layout)

**Phase 1 of the layout persistence: a correct, pure, atomic document.**
Phase 2 wires it into `shell.rs`/`native.rs`/`model.rs` next.

- **Document** — `<userData>/native-layout.json`, ordered/stable;
  one writer; `0` = automatic (proportional default); `windows` keyed by
  stable window id; unusable entries dropped on sanitise/clamp, kept for the
  next launch otherwise.
- **Clamp rules** — proportionally scale below `MIN_TOTAL_WIDTH`; Groups
  clamped to `[190, avail − inspector − centre_min]`; Scenes keeps
  `MIN_SCENES_HEIGHT`; Live keeps `MIN_LIVE_HEIGHT`; the Gallery gives up
  its height first; floating windows clamped on-screen with NaN-out
  semantics. All pure; test coverage pins: default round-trip,
  partial/legacy documents (unknown keys tolerated), malformed/missing
  files, proportional scaling, section-budget arithmetic, NaN/negative
  rejection, on-screen clamping, `record_*` sanitisation, and
  `a_write_never_persists_non_finite_geometry` (write path drops non-finite
  fields before the tmp+rename).
- **Write discipline** — JSON payload through `sanitised()` (non-finite →
  default, unknown entries dropped), tmp-file + rename; flush best-effort
  (returns `Result`, callers log but never panic).
- Behaviour parity with the legacy `window-state.js` clamps; addressed
  positions across both hybrid and native regressions.

### Findings

- **[P3] Phase 2 not wired** — the document is unreachable on `make native`;
  noted, and the commit message says Phase 2 wires it in.
- No P2.

## Round 132 — `6fa788a8..e8f02b08` (fix: show the real capture rate in the picker)

**The 44.1 kHz lie is fully dead in native — rows and banners now render
capture.**

- **`AudioDeviceView.capture_sample_rate`** replaces `default_sample_rate`
  (cpal heuristic gone from the UI entirely; old field deleted).
- **Row text** prints the rate capture negotiates (`2 ch · 48 kHz`), not
  cpal's report.
- **`AudioSelectionView`** gains `capture_sample_rate` — the live feed's
  negotiated rate when capture is running, `None` when silent;
  `active_detail()` prefers it over the device row (falls back while silent)
  so the banner and picker never disagree — plus the PipeWire monitor and
  default-input banners show the rate when known.
- **BPM panel/`audio_status_text`** get `snapshot.audio.sample_rate` —
  `Some(rate)` renders `engine · 48 kHz`; the silent path says `engine
  capture` (or `silent`).
- **Tests** — mapping (heuristic report "44.1" → rows read 48), detail/
  active_detail formatting, silent fallback, kittest picker rows; audio_panel
  golden re-baselined.
- The AGENTS.md note from the same author explains the capture path: the UI
  hint is cpal's heuristic workaround, not a 44.1 kHz engine.

### Findings

- **[P3] No explicit user-facing "cpal default says X, actual is Y" note** —
  the honest number wins everywhere; the note in AGENTS.md covers that; noted.
- No P2.

## Round 133 — `e8f02b08..4e57f81b` (feat: show the live capture rate by the BPM indicator)

**Same-shape extension of the capture-rate fix — every tempo readout now
reports the negotiated rate.**

- **Status bar / menu bar / BPM window** render `BPM 128 · 48 kHz` with
  `bpm_with_rate(bpm, Option<u32>)`; `.on_hover_text` explains the analysis
  source; the session tail reads `engine · 48 kHz` / `silent`; the Tempo
  window adds `Capture 48 kHz` (or `Capture rate unavailable` when silent);
  the Info panel wraps both Tempo and Audio rows in the same rate;
  hover widgets explain them.
- **One `AudioCaptureRate` struct** — a new native.rs cache (`opened`/
  `stopped`/`get`) initialised from a newly opened feed, refreshed on
  live-switch, cleared on feed error/stop/shutdown; f32 device rate is
  clipped to whole hertz with non-positive/non-finite → `None`; the snapshot
  feeds the cache through both `audio_view` and `audio_selection_view`, so
  banner and BPM readouts never disagree.
- Tests: formatter (live/silent/fractal), `AudioFeed::new(48_000.0)` device-
  free seam test, no-rate preservation in the snapshot builder, picker/banner
  agreement kittest, status bar/tempo window/Info row kittest, golden BPM
  window added, default shell snapshot `audio_active: true` honestly set.
- The AGENTS.md cpal-heuristic note now makes sense for all three hint
  sites.

### Findings

- **[P3] The `snapshot.audio.sample_rate` and device file rows can disagree
  when no feed is active** — silent paths omit print; honest; noted.
- No P2.

## Round 134 — `4e57f81b..72baa971` (fix: satisfy clippy in the layout tests)

**Mechanical refactor; no behaviour change.**

- `field_reassign_with_default` → struct-update literals in the six
  `default()`-then-mutate fixtures; `BTreeMap::from([…])` for the
  six-floating-window test.
- No assertions or semantics changed; clippy `root` lint family clean per
  author (`make engine-lint` green).

## Round 135 — `72baa971..f359e459` (feat: persist panel, tab and window layout)

**The layout persistence phase 2 goes in: pure document, full clamp,
atomic, diffable.**

- **Document** — `<userData>/native-layout.json` now carries panel widths,
  section heights and **per-tab `TabPlacement`**: exactly one of
  `Tabs`/`Docked`/`Floating` per right-panel tab; unrecognised ids degrade
  silently to `Tabs`; a missing entry means `Tabs`; `windows` keyed by
  stable ids — unknown keys preserved in-flight, invalid values dropped.
- **`modv-ui::layout::LayoutState`** — runtime counterpart to
  `modv-app::ui_layout::LayoutSettings`; pure placement transitions;
  a tab can never be rendered twice; the active strip tab falls back to
  the next strip tab when its tab detaches; `ensure_strip_active` fixes a
  stale active selection on every frame; `TabPlacement` has an explicit
  `from_key` default-to-strip.
- **Clamp** — minimums centred on Groups (190) + Inspector (232) +
  centre (360) + 232 dock; below 1014 px every column scales proportionally;
  windows clamp on-screen; `clamp_to` restores the document for the real
  window before the first frame.
- **NativeApp wiring** — loads at startup (clamped before the first frame),
  after each `draw_frame` call `sync_layout` diffs the live UiState and
  flips `layout_dirty`; `save_layout` writes at most once per second (and
  on quit/shutdown) atomically; errors are logged once and ignored.
- **Primitives** — `LayoutSettings::from_ui`/`apply_to_ui` round-trip the
  document; `LayoutState::apply_window` applies saved bounds to a freshly
  opened floating window (position+size); `record_window_rect` records the
  outer rect after show; widths captured from the real `Panel` rect each
  frame.
- The shell still draws the previous composition — this phase lays state-
  and-diff-paths under it.

### Findings

- **[P3] No migration test for a stale `native-layout.json`** with absent
  `dock_width`/`tabs` — `apply_to_ui` unknown-key handling is pinned but
  the round-trip against a hand-authored legacy doc isn't — noted.
- No P2. Phase 2-correct.

## Round 136 — `f359e459..d2951184` (feat: resizable sections, floating windows and detachable tabs)

**The native UI is now a real layout manager — drag-splitters everywhere,
per-tab detach/float/dock, persisted user choices. Verified by reading the
full diff and the committed goldens.**

- **Section resizing** — Live Output and Gallery resize as top panels;
  Scenes fills the remainder; saved heights apply as first-show defaults;
  the rects captured after draw update `UiState.layout.live_height`/
  `gallery_height`; live values edge % are paint-over instead of alloc'd
  (so the split can't creep).
- **Per-window `egui::Window`** — palette, tempo, shortcuts, about,
  scene-timing, performance-row-names, tween_curve, input_link: all get
  default size+min-size, position restored at first show, outer-rect
  recorded on `show`; `NativeApp` debounces and writes.
- **Detachable tab panels** — every right tab (Inspector, Media, Audio,
  Info, Plugins, MIDI) has a `Detach` button; detaching floats it and
  strips move on to the next tab; the floating frame carries `Dock to
  tabs` and `Dock to area`; the Inspector body is factored so the strip
  and dock share the same widgets; the dock column has per-tab `Float` /
  `Back to tabs` and stacks multiple sections; the strip hides when empty;
  exactly one placement per tab (enforced by `TabLayout::place`).
- **Shell section capture** — Groups/Inspector widths tracked from
  `Panel::left`/`right` rects; a shrink leak in the old fixed code handled.
- **Goldens** — three new (`tab_docked`, `tab_float`, `tab_back_in_strip`)
  show all three configurations honestly; `shell.png` re-baselined for
  the new composition.

### Findings

- **[P3] The strip detaches-but-doesn't-show an empty-state hint when all
  tabs are floating** — fine, the header space is intentionally empty; not
  a footgun (strip hides when empty).
- **[P3] Nudge: detaching a tab while its window is open in another area** —
  `TabLayout` guarantees one placement; fine.
- No P2. The layout now matches Visual Studio's detach/dock/float
  semantics — first-run defaults are identical to the previous build.

## Round 137 — `d2951184..3bd065d2` (feat: drag Gallery modules into Groups)

**The Electron Gallery → Group gesture is back, with the pure/impure split
done right.**

- **Payload** — `GalleryDrag { module_name }` egui dnd payload on the
  Gallery tile (`Sense::click_and_drag`); `widget_info(Button)` keeps the
  labels addressable; the `Grab` cursor sign; double-click/context/marquee
  paths unchanged.
- **Drop targets** — every Group block (header, selected-Group controls,
  module strip) unions into one drop rect; the drop affordance paints an
  accent wash/border plus an `Add <module>` chip; `Copy` cursor while
  hovering; on release `take_payload` consumes and queues the exact action
  the double-click path queues — `ui/addModuleToGroup` with
  `{ moduleName, groupId }` — the engine still owns duplicate semantics.
- **Pure resolver** — `gallery_drop_action` unit-pinned (hovered-Group
  destination, empty-space cancels, no-payload cancels).
- **Kittest pointer drags** cover header drop, strip drop (incl.
  already-owned module), drop on non-selected Group, empty-space release,
  and double-click regression; goldens unchanged (correct).

### Findings

- **[P3] `updateMeta` on the module's own enable/disable checkbox is busy
  in a drop zone?** — no; the drop rect unions header+controls+modules, but
  no drop happens there because the dnd payload only matches on release;
  noted.
- **[P3] Drag-source affordance is a cursor only** — no permanent "drag"
  glyph on tiles; deliberate; fine.
- No P2.

## Round 138 — `3bd065d2..da724d45` (feat: build a native .deb)

**Native packaging gets a Debian artifact with real-world verification.**

- **Build** (`make native-deb` + `tools/native-deb.sh`): release `modv`
  compiled in Docker, staged under `/usr/lib/vmodv-native`, prefix-resolving
  launcher at `/usr/bin/modv` (plus `vmodv-native` alias), ISF corpus +
  Point10 textures under `/usr/share/vmodv`, desktop entry, icon,
  md5sums for the regular payload, root-owner-group permissions, `--help`
  prints at install time.
- **Control manifest** — declared `Depends`: the NEEDED libraries
  (libasound2, libpipewire-0.3-0, libgstreamer1.0-0,
  libgstreamer-plugins-base1.0-0, libglib2.0-0, libasound2-style t64
  fallbacks noted in comments), `libvulkan1` (dlopen), `libxkbcommon-x11-0`
  (dlopen X11 fallback), the four GStreamer plugin packages (base/good/libav +
  `gstreamer1.0-pipewire` — the same one Round 98 added to the Electron
  `.deb`); `Recommends: mesa-vulkan-drivers`; versioned `@INSTALLED_SIZE@`
  and `@VERSION@` templated at build time.
- **`tools/native-deb-check.sh`** — validates the .deb in-host: control
  fields (Version/Architecture/Section/Maintainer/Description/Installed-Size
  numeric > 0), every `Depends` present, `./usr/bin/modv` and
  `./usr/lib/vmodv-native/modv` both root-owned 0755, `vmodv-native` symlink,
  ISF count matches checkout-derived `find … | wc -l` (354 fallback),
  textures count (25 fallback), the launcher runs `--help` and `--version`.
  Called at the end of every `make native-deb`.
- **Ownership** — `--root-owner-group` honoured in the build.
- **Not verified here** — the Docker/Rust toolchain image was not built;
  the "prints help" check is exercised in the build script.

### Findings

- **[P3] The check runs only after a build** — a `dpkg-deb`-less environment
  fails early with the error message (intended); noted.
- **[P3] The AppImage remains .deb's sibling; no Flatpak yet** — as the
  tracker states; fine.
- No P2.

## Round 139 — `da724d45..26121571` (ci: native Rust CI jobs)

**Native-only CI lands; the Electron nightly remains the default.**

- **Workflows** — `.github/workflows/native.yml`: triggers on push/PR to
  `main`/`rust-hybrid` for `crates/**`, `packaging/native/**`,
  `tools/native-*`, Makefile/Dockerfile/native.yml paths; `workflow_dispatch`
  allowed. Three jobs: `Clippy and rustfmt` (make engine-lint), `Rust tests`
  (make engine-test — workspace serial for the shared Lavapipe; `modv-app`
  in-app `--test-threads=1`), `Native AppImage and .deb` (build both
  installers, ls/dpkg-deb check, upload `actions/upload-artifact@v4`).
- **Scope is explicit** — the Electron nightly release is unchanged, this
  workflow only publishes workflow artifacts; P9 row updated accordingly.
- **GHA cache reuse** — docker/build-push-action@v6 with type=gha scope
  per the Rust stage of the Dockerfile, matching the local `make engine-lint`.
- **Test invocation is correct** — workspace tests excluding `modv-app`
  run in parallel; `modv-app` runs serially in a follow-up step, matching
  the Round-116 lavapipe-serialization note.

### Findings

- **[P3] The workflow has no nightly schedule** — manual-trigger + push
  only; fine for a feature branch.
- **[P3] Artifacts expire after the default GHA window** — noted.
- No P2.

## Round 140 — `26121571..a331368c` (ci: package triggers cover the embedded payload)

**Same-message fixup: native CI reacts to changes to what it packages.**

- Adds `src/renderer/src/application/renderers/isf/modules/isf/**`, the
  Point10 `textures/**`, `resources/icon.png` and `LICENSE` to both push and
  pull-request triggers — packaging must rebuild when its embedded payload
  sources change.
- 8 lines added; no new jobs, no behaviour change.

### Findings

- No P3/P2 — this is the right fixup; previously a corpus shader change would
  not have triggered the installer jobs even though the AppImage/.deb content
  depends on it.

## Round 141 — `a331368c..dbc2aa76` (feat: NDI and OSC panels)

**The two remaining major Panel surfaces land natively; every dispatch
hits a real engine arm.**

- **NDI tab** — reads `NdiRuntime` through the same read accessors
  `ndi/getState` exposes; library availability + found path shown; sender
  enable/disable/name/width/height/size/fps/target-fps; sender status;
  discovery with `showLocalSources`; receivers with enable/disable/remove
  + per-receiver status; `ndi:<id>` texture registration — all backed at
  `modv-app::NdiRuntime`, the same arms at the engine dispatch table;
  1 Hz refresh while the tab is visible; immediate refresh after actions;
  `Store::poll_ndi` wired in `tick_engine` so discovery completes.
- **OSC tab** — default 0.0.0.0:3333 listener bound (logged once,
  non-fatal on failure); listener list with add/remove; latest osc.data leaves
  with a copy-to-clipboard button; addresses flattened with a bounded
  helper; 1 Hz refresh while the tab is visible.
- **Both tabs integrate** — `SidePanelTab::Ndi`/`Osc` through the normal
  strip; detachable through d2951184's mechanism; persisted keys; kittest
  goldens updated honestly.

### Findings

- **[P3] No NDI/OSC e2e** — unit + kittest coverage only, hardware NDI
  traffic is device-dependent; fine.
- No P2.

## Round 142 — `dbc2aa76..df2ade1f` (feat: bundle IBM Plex Sans and Plex Mono)

**The native UI gets a consistent typeface, with the licence story intact.**

- **Four static faces** embedded via `include_bytes!` (Plex Sans Regular /
  SemiBold, Plex Mono Regular / Medium) under
  `crates/modv-ui/assets/fonts/`; `OFL.txt` beside them.
- **`fonts.rs`** — defines the proportional/monospace families with Plex at
  the head (egui's Ubuntu-Light / Hack stay behind it for fallback glyphs),
  adds named `plex-sans-semibold`/`plex-mono-medium` `Name` families for
  heading/strong text, and `install()`s them inside `theme::apply`.
- **Unit-pinned** — every embedded face has valid bytes + magic (TTF/OTF),
  Plex leads both families with bundled fallbacks intact, named weight
  families start with their own face, font_data contains every key.
- **Notices** — the fonts section added (IBM Plex faces under OFL 1.1);
  egui default faces remain covered by the existing crate table.
- `modv-app`-side call sites keep going through `ctx.set_fonts` on every
  app boot, consistent with prior theme/apply lifecycle.

### Findings

- **[P3] The fonts are uncompressed TTFs (~760 KB added)** — static assets,
  no dynamic subsetting; fine for a desktop app; noted.
- No P2.

## Round 143 — `df2ade1f..5da737a8` (feat: neon-green theme overhaul)

**Visual parity with the shipping legacy UI skipped; native gains a fresh
serif-black neon identity.**

- Every `CornerRadius::same(2)` → `same(0)`; every rounded pill gone;
  handles squared; surface ladder rebuilt as near-black steps
  (`BACKGROUND`/`PANEL`/`PANEL_ALT`/`INPUT` strictly ordered); one accent
  (green) + semantic red/amber/blue badges; type scale 16/15/12/15/20
  (Body/Button/Small/Mono/Heading); 22 px rows, 140 px sliders, 32 px menu,
  26 px status bar; defaults Groups 270 / Inspector 340 / dock 320 (mins
  210/250/250) in shell and ui_layout; every hardcoded grey/blue in the
  Scene grid and Gallery moved to the palette; goldens re-baselined.
- The commit is big-but-shallow: pure palette/sizes/const moves; the layout
  clamp from Round 131 keeps working (new minimums cascade consistently).

### Findings

- **[P3] All goldens re-baselined for the new theme and type scale** —
  every interaction suite needs a re-baseline; the screenshot deltas are the
  point; the gallery/diameter expectations are intentional. Visual parity
  with the legacy reference image is now less bit-exact, which is the
  expressed intent.
- No P2.

## Round 144 — `5da737a8..40fdb8fb` (fix: keep input links alive across Scene applies)

**A real editor correctness bug, closed honestly — the Save after a Scene
apply no longer silently drops.**

- **The defect** — `inputs/createInputLink` used to `WARN` and drop the save
  when the named input id vanished; a Scene apply rebuilds every Group/
  Module/Input with fresh ids, so an editor opened before the apply still
  named the retired id. Whatever the user picked in the UI before applying
  was lost.
- **The fix** — a `InputIdentity` description (Module prop + sub-prop path,
  Module meta key, Group key — name-based, not id-based) is captured before
  every apply/row-apply/release; on save, if the named id vanished, resolve
  through the retired-inputs map onto the rebuilt control; the link lands on
  the fresh id. Tween links follow the new id via the parks on the link
  record (`tweens.tweens['<new id>'].value[0]`). Ambiguous duplicate-module
  matches are not retargeted (no wrong-Control binding), logged at INFO;
  genuinely-removed inputs also INFO instead of WARN.
- **Round-trip test** — seed a Scene with a bass-linked prop link,
  re-apply it, dispatch a save against the retired id, assert the link lands
  on the rebuilt input and resolves through `update_input_links` to the
  exact [0.1, 0.2] mapped value; a tween-location link also follows the
  rewrite onto the new id.
- Ambiguity safety: `find_input_by_identity` rejects partial matches;
  the retired map is bounded at 4096 entries; preset restores clear it;
  log-once semantics for retarget vs ignored.
- The commit message calls out the honest difference vs `scenes.js`: the
  legacy path also dropped Save on an editor opened against a live input
  that apply replaced; native now keeps it when a unique rebuilt match
  exists and logs at INFO otherwise. **This is a strictly better story
  than the JS.**

### Findings

- **[P3] A retired id can be bound by a future module that mints the same
  name** — extremely narrow collision window; acceptable for a best-effort
  convenience map.
- **[P3] `warn_once` messages changed class** — INFO for "replaced/ignored"
  is deliberate; users on the log file may misread fewer loud warnings;
  fine.
- No P2. This is the class of parity gap the review has been flagging
  for rounds and it is now closed.

## Round 145 — `40fdb8fb..f239ad56` (fix: docked/floating tabs and sliders have contrast)

**A visual regression is closed honestly, with real regression goldens.**

- **The defect** — the dock column painted `PANEL_ALT` and the slider rail
  (`widgets.inactive.bg_fill`) was also `PANEL_ALT`; a docked/floating tab's
  module slider rails were invisible against the background.
- **The fix** — `DOCK_FILL`/`STRIP_FILL` aliases to `PANEL`, floating-window
  `window_fill = PANEL`, and a scoped `theme::slider_visuals` that paints a
  distinct rail (`SLIDER_TRACK`, darker and greener) with a
  `FOREGROUND` handle stroke; `RangeControl` plus every `egui::Slider` call
  site (Groups alpha, module alpha, Input-link smoothing, link editor) route
  through `theme::slider()`/`slider_visuals(…)` so the rail and handle read on
  any surface; buttons keep panel-alt surfaces.
- **Regression coverage** — `a_floating_tab_keeps_its_slider_visible` and
  `a_docked_tab_keeps_its_slider_visible` show the RangeControl label and
  count `>= 2` slider roles via `accesskit::Role::Slider` (module Alpha and
  gain), plus two new goldens (`tab_docked_range.png` and
  `tab_float_range.png`); shell/tab goldens re-baselined.

### Findings

- **[P3] `.as_str()` inside the `on_hover_text` call** — an empty-string
  `input_id` tooltip on link rows is possible; unreachable today; noted.
- No P2.

## Round 146 — `f239ad56..1b7eb187` (docs: refresh progress and parity map)

**Honest refresh of both trackers; counts drift by one or two.**

- **Native full-rewrite progress** correctly marks P0 as "GO (with one
  caveat)" and P4/P6/P7/P8/P9 rows updated from actual current state —
  verified the NDI/OSC panel slot (`dbc2aa76`), NDI as PARTIAL-verified, the
  tween curve editor P3 as Partial, packaging P9 as started/native.
- **Parity map** rows moved to track the current native surface behaviour:
  432-module guard closed, Gallery drag-to-Group kittest, native file icon.
  Points to module-parity where unique.
- **The "Current test inventory as of `40fdb8fb`" section** is honest (named
  as a count-from-the-tree, not a re-run) but the numbers drift by ±1–2: I
  recount `modv-ui` = 127 unit + 91 kittest/integration = **218** (the doc
  says "216 unit + kittest/integration" and notes the f239ad56 +2), and
  `modv-media` = **256** (the doc says 257). modv-engine/app/renderers match
  exactly (476/519/794). Not dishonest; not byte-exact either.
- The "Last recorded full workspace run: 1951 passed at `6e5cf6be`" stays
  accurate (that's the number; pre-expansion).
- The `smoke_room` reference is retired as known-red (closed `9e4a5a40`) —
  credited correctly. The P9 note "Flatpak / 2.0.0 / nightly-rust /
  Electron deletion remain pending" is accurate and honest.

### Findings

- **[P3] The `#[test]` count method omits `#[rstest]`-style cases and
  `ctest`-lists if any exist** (there appear to be none) — noted.
- **[P3] 1951 stays non-re-run since `6e5cf6be`** — the CI will supersede
  this at the next run; noted.
- No P2.

## Round 147 — `1b7eb187..60b59e20` (feat: drag tabs between strip, dock and floating frame)

**The last UI gap between native and Electron snapped shut.**

- **Payload** — `TabDrag { tab }` egui payload (template for the gallery
  drag from Round 137); strip tabs and floating frames carry a drag grip
  (pop-out icon on hover/selected); plain click still selects.
- **Drop targets** — strip, empty-dock-column ("Dock here" placeholder even
  while empty), central area ("Float here" outline); release writes the
  matching placement, bound clamped on-screen from the release point;
  `floating_bounds_for_drop` is unit-pinned (worst-edge, smaller-screen);
  placement stays exclusive.
- **Gesture adjuncts** — tab re-renames to pop-out shape icon, "Detach"
  text gone, tab context menu gains Detach/Dock options.
- **Kittest coverage** — `tab_drag_drop.rs` pins strip→dock, strip→centre,
  grip→strip, cancelled drops, context-menu path; `tab_drag.rs` pins the
  payload shape, placement mapping and bounds clamping.
- Goldens re-baselined (new layouts).

### Findings

- **[P3] No explicit keyboard path for moving tabs between placements** —
  detach/dock/back-to-tabs buttons remain; fine, noted.
- No P2.

## Round 148 — `60b59e20..14700585` (fix: the strip advertises drag outline while docked/floating tabs fly)

**Same-message fix: the `Back to tabs` strip outline shows during every drag, regardless of tab placement.**

- When a drag flies and the dragged tab is docked or floating (not in the
  strip), the strip paints its thin `Back to tabs` outline even without a
  hovering pointer — a destination at a glance.
- `paint_tab_drop_zone` gained an independent `outline` axis; the central
  panel now outlines regardless of hover position; the dock column always
  outlines while a drag is in flight above it.

## Round 149 — `14700585..d35241f6` (fix: Scene cell disable toggle reliable and visible)

**Per-cell disable toggle actually works now — closes a genuine interaction bug.**

- **The defect** — the dot sent `enabled: !cell.disabled`, which is the inverse of the new state; the engine's `disabledCells` stayed empty from user interaction; the context-menu toggle had the same inversion.
- **The fix** — both paths send `enabled: cell.disabled` (new enabled-state); the dot is a real `CellEnableDot` widget in a 20×20 hit area registered after the cell so it always wins press priority; its presses never apply the Scene or start a Ctrl+drag copy; disabled cells gain a red dot/border/left-stripe/scrim/`OFF` chip; enabled cells keep the green dot and normal chrome.
- **Wire-level** — `set_cell_enabled` maps `enabled` to `set_scene_cell_disabled(..., !enabled)`; `disabledCells` persists through the normal save path and is honoured for dwell/prewarm/start/resolve picks.
- Kittest pointer tests pin exact payloads in both directions, the accessibility-role click, context-menu toggle, and draw-state helper.
- scene-grid and shell goldens re-baselined.

### Findings

- **[P3] The toggle helper is testable only through the paint loop** — the input is captured through the disabled flag on `CellEnableDot`, which is fine.
- No P2.

## Round 150 — `d35241f6..6b5d07ef` (style: pure-black frame backgrounds)

**Theme delta: green-cast near-black gone; pure #000000 frames.**

- `BACKGROUND` and `PANEL` collapse to `#000000`; `INPUT` steps to `#0A0A0A`;
  `PANEL_ALT` is neutral `#121212`; `FOREGROUND` keeps a muted greenish-white
  (`#E8F5EC`) by choice. Theme unit tests updated from strict-ordering to
  frames-are-black-and-raised-steps-lighter; goldens deferred for the next
  batch. Theme slant is coherent with the neon accent discipline (Round 143).
- No P3/P2.

## Round 151 — `6b5d07ef..df395840` (feat: native file dialogs and the File menu actions)

**The commit lands end to end — dialogs, menu, profiles, CLI aliases.**
Verified by reading at `df395840`; no P2; no P1.

- **Scope** — largest single commit in this window (2267 insertions). New:
  `media_dialogs.rs` (dialog plumbing and document detection), `media_profiles.rs`
  (project profile model), `modv-ui::file_dialog.rs` (in-app picker), the
  File menu entries, `--scene`/`--playlist` CLI aliases, `--preset` auto-detect,
  and `<userData>/native-project.json` with an atomic tmp+rename writer.
- **Classification is byte-faithful** to `preset-serializer.js`'s
  `isSceneDocument`/`isScenesPlaylist`: playlist first (typed match plus
  `scenes` array), Scene needs a non-empty `id`, a `name`, and a `groups` or
  `layers` array — verified against the JS source.
- **`scenes/loadSceneFromJson` upserts, never silently replaces the live stack** —
  verified in `scene_ops.rs:1837`: params (`scene` object or JSON string) →
  `normalize_scene_document` → replace-or-push by id → `scenes/ADD_SCENE` delta
  → best-effort persist. No `apply` unless `apply: true` is passed (the
  File menu path doesn't pass it).
- **Posture is clean**: `xdg-open`/`open`/`explorer` get the path as one
  argv item (no shell), targets can never be reached as a command name;
  every open validates `.json` extension first; `read_json` requires an
  existing regular file; hidden files stay hidden unless the user asks.
- **The modal guard actually works** — `handle_shortcuts` Ctrl+S/Ctrl+O
  shortcuts are inert while the dialog or profile modal is open; Escape
  closes all three windows; the picker draws on the last canvas.
- **Startup precedence** — `startup_document_path` is unit-pinned
  (playlist > scene > preset > none); `--scene` and `--playlist` are strict
  conveniences; every action maps through the same `open_document` path.
- **The seam is honest** — `FilePicker` is the correct test seam;
  `RecordingPicker` in the unit suite pins every menu action reaches the
  engine; `FileDialogAction::command` is pure and pinned in `file_dialog.rs`.
- **Atomic profile writes**, idempotent profile creation
  (`create_dir_all` on the profile folder, existing files preserved);
  saved profile that vanishes falls back to `default` with a WARN log —
  no recreation, no silent breakage.
- **`AddIsfFolder` is the whole win** — canonicalised path, saved in
  `native-project.json`, watcher rebuilt, modules rescanned — matches the
  Electron media-manager reload semantics.

### Findings

- **[P3] Ctrl/Cmd+Shift+S does not do what the shortcuts overlay claims** —
  the overlay lists `("Ctrl/Cmd+Shift+S", "Save preset as…")` but
  `handle_shortcuts` binds `command && shift && Key::S` to `UiCommand::SaveSnapshot`
  (writes the `--state` mirror). Saving a preset as never happens via the
  keyboard; a snapshot-write happens instead under a false label. The menu
  click path works; the overlay text does not match the wire. Stale doc +
  missing binding in one line — two-line fix (relabel the overlay and/or
  bind Shift+S to `ShowFileDialog(SavePresetAs)`).
- **[P3] Startup `--preset` makes the auto-detected file the Save target,
  regardless of document kind** — `preset_path = cli.preset.clone()` runs
  before the kind is known; the in-app Open path (lent by the
  FilePicker seam) only makes a *Preset* kind the target. So
  `--preset my-scene.json` (auto-detects a Scene, loads it) leaves
  `preset_path` pointing at the Scene document; later Ctrl+S writes the
  canonical engine snapshot (a plain preset document) over the Scene file,
  destroying its id/name/groups metadata. The fix mirrors the seam:
  set `preset_path` only when `classify_document` returns `Preset`.
- **[P3] `PROFILE_FOLDERS` over-creates two dead folders and the module
  doc misstates the JS origin** — the live JS media manager creates
  folders from the `readHandlers` ADD-mutations' keys, i.e. exactly
  `image, video, isf, palette, preset, scene, module` (7). The Rust list
  adds `plugin` and `plugin_data` (only ever mentioned in the commented-out
  block in `fs-create-profile.js`, never in the active path). The doc-line
  "The same folder set fs-create-profile.js makes" matches neither the live
  set (7) nor the commented block (7, includes plugin/plugin_data but lacks
  scene/isf). Nothing breaks — extra empty folders live under `media/` —
  but the comment is wrong and the profile-modal hint repeats it. Ask the
  author to either trim `plugin`/`plugin_data` or correct the comment to
  "the live seven plus two legacy folders".
- **[P3] The shortcut surface is untested** — no kittest covers
  `handle_shortcuts`; that is how the previous item shipped silently.
  A `file_menu.rs`-style matcher for Ctrl+Shift+S (and Ctrl+O/Ctrl+Shift+O)
  would pin it. Noted, not blocking.
- **[P3] The file-name field's Enter does nothing** — the picker confirms via
  double-click or the footer button only; desktop dialogs treat Enter=confirm.
  Cosmetic UX parity; fine to defer.
- **[P3] `list_profiles` sorts case-insensitively by `to_lowercase`** — a
  pure-ASCII call in effect; non-Latin names compare oddly, harmless.
- **[P3] The 2-second `last_profile_scan` is a fresh read of
  `media_root` each tick** — cheap (one `read_dir`), fine.

### Doc-discipline note

The commit message claims "a Scene is a preset plus timing metadata" — a
paraphrase matching `isSceneDocument`; fine. The `process count = 8 tests
in media_profiles.rs, 6 in media_dialogs.rs` matches my recount exactly.
No red claims are hidden in this commit.

## Round 152 — `df395840..e7ffecf7` (fix: align Scenes rows; pin the layer gutter)

**A layout bug class, fixed at the right layer.**

- **Root cause — `ui.put` rewinds the cursor to the widget's bottom**:
  per-cell placing used `ui.put(dot_hit, CellEnableDot {...})`, which
  advances the column's vertical cursor to the dot's bottom edge; with the
  next row starting below the dot, every row pushed the column further down.
  Chain-adds accumulated drift; per-cell differences became visible drift
  between columns.
- **Fix — one fixed pitch everywhere**: `ROW_PITCH` (CELL_SIZE.y + ROW_GAP),
  `HEADER_HEIGHT`/`HEADER_GAP` are the same constants on both sides (gutter
  and Scene column), `ui.spacing_mut().item_spacing.y = 0.0` so nothing
  improvises extra gaps, and `ui.place(...)` for the dot and the header
  TextEdit (registers without advancing).
- **Layer gutter pinned** — the row-name column is now painted on top of
  the scroll viewport (child UI with an opaque `PANEL` backing, clipped to
  the viewport, sharing the wheel offset with the columns but never the
  horizontal scroll). Columns scroll under the gutter; the gutter rows
  match the Scene rows exactly, with a visible pixel-exact backing so
  nothing shows through while the columns slide.
- **The `Layers` header** moved into the gutter (painter-based,
  `widget_info` for kittest) and the gutter reserves its width in the
  scrolled content so the first column clears it at rest.
- **Regression coverage** — `scene_grid_layout.rs` drives `egui_kittest`:
  (a) every row's pitch is identical between gutter and every Scene column,
  (b) the gutter stays pinned while sideways scrolling, (c) the gutter
  shares the vertical offset. Pixel-exactness is asserted via
  `assert_close` to sub-point tolerance.

### Findings

- **[P3] The gutter paints after the scrolled content with an opaque
  fill** — a deliberate overlap; unavoidable with egui's single-UI model;
  noted.
- No P2. The bug class was invisible to the golden-image suite because the
  drift was sub-tile per cell; the layout-pitch tests close the gap.

## Round 153 — `e7ffecf7..fa67044b` (feat: watch the project media tree)

**The ISF watcher grows into a real media-tree watcher — well built.**

- **New `media_watch.rs`** replaces `isf_watch.rs`:
  non-recursive watches on the active profile's `isf`, `image`, `video`,
  `palette` folders plus every extra user ISF folder; the extension
  allowlist matches the JS read handlers (`.fs`/`.vs`, jpg/jpeg/png/gif,
  mp4/webm/webv, json).

- **Hidden/temp/editor swap files are ignored** (`.…`, `…~`, `….tmp`) and
  only the immediate folder is considered (`isf/compiled/*` is excluded by
  construction since the JS compiled path is separate).
- **The 500 ms quiet debounce is split into a pure `Debounce` struct** so
  the coalescing policy is unit-testable without a real watcher (quiet
  window, reset-on-take, event counting). Event counts are reported for
  diagnostics.
- **ISF bursts trigger exactly one rescan/register**; other folder
  changes are logged once and explicitly do **not** register anything in
  the engine (correct — the engine doesn't consume image/video/palette
  files yet).
- **The watcher is rebuilt on profile switch and on AddIsfFolder**
  (`rebuild_user_isf_dirs` now calls `MediaWatcher::for_profile` alongside
  the dirs rebuild). Preset/scene folders deliberately unwatched — the
  shell writes those itself.
- **Tests**: allowlist for every folder, classification + temporary-name
  rejection, a debounce-policy unit test (burst + quiet window), plus a
  real-filesystem integration test with a 5-second deadline that observes
  an ISF and an image write appear as one burst. The last is the standard
  async-fs-watch discipline for notification paths.

### Findings

- **[P3] Non-recursive** — sub-folder trees (`isf/sub/…`) are not watched;
  matches the JS read-handler behavior; noted.
- **[P3] The image/video/palette events are logged but nothing consumes
  them** — intentional placeholder for the media-manager pipeline; the log
  line is honest.
- No P2.

## Round 154 — `fa67044b..7f27a34a` (fix: guard startup Save-target; logs folder; profile modal tests)

**The fix the last round asked for lands with the associated hardening.**
Round 151's second finding is closed.

- **The fix** — in `NativeApp::new`, `preset_path = cli.preset.clone()` is
  now `let mut preset_path = cli.preset.clone()` followed by the startup
  document match: `Ok(kind)` with `kind != DocumentKind::Preset` →
  `preset_path = None`. So `--preset scene.json` no longer arms Ctrl+S to
  overwrite the Scene document with a plain preset. The file-dialog seam
  path already had the correct gate (`open_document` only makes a Preset
  the target); this brings the startup path to parity.
- **The failure branch keeps the stale `preset_path`** — when the startup
  open fails (`Err(error)`), `preset_path` stays `Some(path)`: the caller
  passed `--preset bad.json`, the open was rejected, yet Ctrl+S later would
  overwrite the rejected file with a plain preset. Probably benign — the
  user may be deliberately repairing an unreadable preset by saving over
  it — but worth the same treatment as the `Ok(kind != Preset)` arm for
  symmetry (clear `preset_path` when the open fails, or log the decision).
  Noted, harmless.
- **The logs folder is created before `xdg-open`** — `OpenLogsFolder` now
  `create_dir_all`s `<userData>/logs` and only opens it on success; a
  missing folder no longer yields a spurious error from xdg-open, and the
  empty-dir case is handled cleanly.
- **The profile modal now guards an empty name** — `queue_create_profile`
  rejects a whitespace-only name with an inline error and stays open; a
  kittest (`the_profile_dialog_queues_the_typed_name`) pins the
  success path (typed name → queue `CreateProfile("live")`, modal closes).
- **Not addressed yet** — Round 151's Ctrl/Cmd+Shift+S overlay mismatch
  (overlay says "Save preset as…", key dispatches `UiCommand::SaveSnapshot`)
  is **still open at `7f27a34a`**. The menu action works; the keyboard
  mapping and the overlay are still wrong. Calling it out for the next
  batch, in priority order: relabel the overlay (or bind Shift+S to
  `ShowFileDialog(SavePresetAs)`) and remove the stale "Save engine
  snapshot" label from before this batch's precedent.

### Findings

- **[P3] On startup-open failure, `preset_path` keeps the stale
  `Some(path)`** — by accident vs intent; empty/missing file with `--preset`
  leaves the user's Ctrl+S wired to overwrite it. Noted; minor.
- **[P3] `Ok(kind != Preset)` gate discards `preset_path` without a log** —
  the INFO log line does name "opened startup Scene …" so the user can
  deduce it, but an explicit "not arming the preset target" line would be
  friendlier; noted.
- No P2. A small round that closes a small data-loss trap; correct in
  intent and execution.

## Round 155 — `7f27a34a..dad7c58e` (style: drop duplicated capture rate from BPM readouts)

**A coherence fix — the rate is reported once.**

- **The duplication** — the menu bar, status bar and Info panel all showed
  `BPM 128 · 48 kHz` while the audio indicator immediately beside them
  already reported `audio: engine · 48 kHz`. The three tempo readouts now
  render the bare `BPM {:.0}`; the rate lives on the audio indicator and in
  the Tempo window's `Capture 48 kHz` line only.
- **Call sites** — three `bpm_with_rate(...)` invocations replaced with
  `format!("BPM {:.0}", ...)`; the helper is **now dead** (defined in
  `model.rs:627`, re-exported in `lib.rs`, four unit tests, no callers) —
  the commit removed its callers but left the helper behind.
- **Tests updated, honestly** — `bpm_window.rs` renames the stale assertion
  (`the_bpm_readouts_show_the_live_capture_rate` → …“without duplicating …”)
  and updates the two expectations to bare `BPM 128`;
  `audio_panel.rs` does the same for the count (2 = menu bar + status bar).
- Goldens NOT re-baselined — explicitly deferred to the pending
  Scene-timing batch ("Golden re-baseline for the tempo window follows
  with the Scene-timing batch").

### Findings

- **[P3] `cargo test -p modv-ui --test bpm_window` is red at this HEAD** —
  `bpm_window_snapshot` draws the full shell (status bar included) into the
  captured frame, and the status bar text now reads `BPM 128` instead of
  `BPM 128 · 48 kHz`; the committed golden still shows the old line.
  `Harness::snapshot` without `UPDATE_SNAPSHOTS=1` fails on the pixel diff,
  so the suite is red between this commit and the Scene-timing batch that
  re-baselines it. The author disclosed the deferral in the message, and
  the fix is one command away; this is the deliberate-transition class,
  not a silent regression — but the discipline elsewhere in this rewrite
  (re-baseline in the same commit whenever a visible UI string moves)
  should hold here too. Asked: land the tempo-window re-baseline first in
  that next batch.
- **[P3] `bpm_with_rate` is dead code** — definition, re-export and four
  unit tests stay while their only call sites were removed. Ask: delete the
  three bits in that next batch, or assert the emptiness intentionally.
- **[P3] The Info-panel row count** in `audio_panel.rs` changed from
  `count() == 2` (was `BPM 128 · 48 kHz` ×2) to the same count of bare
  `BPM 128` — consistent; fine.
- No P2.

## Round 156 — `dad7c58e..f610d08b` (feat: phrase flips + bar timing drive Scene transitions)

**Two genuine and substantial engine features land behind one commit.**

**A — Phrase-triggered Scene advances (the legacy `shouldAdvanceRow` rule).**

- The analyser already publishes a one-frame `phrase_drop` pulse;
  `tick_scenes_with_phrase` now accepts it via `scenes/tick({phraseFlipped})`
  and runs the byte-faithful `shouldAdvanceRow` port — deadline forces the
  change, a phrase flip only advances on `random() < switchChance(dwell,
  min, max)`. Chance math, the forced/musical split, the `random` call only
  after a flip (so the non-phrase dwell stream is untouched), and the
  `NaN`-bound fallback (chance 0) were verified line-by-line against
  `scene-rows.js:405-431` and `phrase-detector.js:87-96`.
- Wired end-to-end: `pump_audio` returns the pulse, `tick_engine` forwards
  it into `scenes/tick`, the hybrid `window.rs` path does the same.
- Tests pin: flip before min never advances; mid-window advance only on a
  seeded passing roll (both sides of the line); at/after max always
  advances; `tick` with no flip never touches the RNG (byte-compare of the
  cloned RNG state); bar-unit per-row dwell windows at the live tempo (120
  BPM → 2-4 s target).

**B — `performance.timingUnit` (`"seconds" | "bars"`) with live-tempo conversion.**

- A bar is 4 beats (`BEATS_PER_BAR`); invalid/absent tempo falls back to
  `FALLBACK_BPM = 120` — the same rule the tween compiler uses for
  `TimeBpm` helpers. Stored `minSeconds`/`maxSeconds` keep their historical
  names and their unit is announced by `timingUnit`.
- **Correct stored-vs-resolved split** — `normalize_timing_config` keeps
  stored values in the active unit; `resolve_performance_timing_at` does the
  conversion to seconds at the read sites (live dwell, view). Scene
  overrides travel through the same split (`normalize_scene_timing_values`)
  so the grid never shows silently-converted numbers; the native dialog
  adds quarter-bar steps and a resolved-seconds hint.
- **`scenes/setTimingUnit`** — dispatch converts the global pair plus every
  Scene override through seconds at the live BPM, then writes the unit and
  the converted pair in the storage and re-emits the performance config.
  Test pins round-trip persistence across restarts and playlists carry
  `timingUnit` (PLAYLIST_PERFORMANCE_KEYS updated; import/export keep
  override min/max separate).
- Idempotency and validity: unit parse is tolerant (`"bars"|"bar"` → Bars;
  anything else → Seconds), the default is seconds (an older document is
  unchanged); overrides before and after a switch re-convert with the live
  tempo — no stale-seconds drift.
- A compatibility footnote is documented in the module doc and the progress
  tracker: Electron ignores `timingUnit` and would interpret bars as
  seconds; switch back to seconds before opening the project in Electron.

### Findings

- **[P3] The legacy whole-Scene autopilot (Groups with no Modules,
  `scenes.js:3940-3978`) is not implemented here** — documented and explicit
  ("no legacy phrase rule to mirror"); acceptable; noted as a divergence
  from the JS baseline only for the zero-Module Groups case.
- **[P3] `tick_scenes` keeps the default-`false` wrapper** for callers that
  don't have a phrase pulse — correct; noted.
- **[P3] Bar-timed `minSeconds`/`maxSeconds` read as raw numbers in any
  other consumer that assumes seconds** (this is the same hazard any
  `timingUnit` document-by-reference creates); documented; ok.
- No P2. This is the deepest engine delta this window and it is byte-true
  to the JS guards.

## Round 157 — `f610d08b..1901063c` (feat: Scene times in bars; snapshots re-baselined)

**Closes Round 155's red-test gap: the goldens are re-baselined here.**

- **Read model** — `SceneView.timing` is now `Some((min, max))` — the old
  view exposed only `maxSeconds`, so the override dialog opened with the
  minimum replaced by the maximum. `PerformanceView` carries `timing_unit`
  plus a renaming `min_value`/`max_value`; defaults come from
  `default_timing_values(unit)`.
- **Performance bar** — a `Timing` selector (Seconds/Bars) dispatches
  `scenes/setTimingUnit {unit}` on change; min/max `DragValue`s use
  unit-aware floors/steps/suffixes; a resolved-seconds hint renders the
  pair in current BPM; the grid's timing chip on a Scene cell shows
  `{max:.2}b` in bars instead of `{max:.0}s`.
- **Override dialog** — unit-aware inputs, resolved-seconds hint, quarter-
  bar steps, `Reset override` button (dispatches the pre-existing
  `scenes/resetSceneTiming`), save re-clamps with `clamp_timing_pair`.
- **Kittest coverage** for: the unit selector payload, the bar hint, the
  min/max pair round trip through override save, the reset action.
- **All 22 affected goldens re-baselined** (`bpm_window.png`, `shell.png`,
  `tab_*`, `audio_panel.png`, `scene_grid_*`, `tween_curve_editor.png`,
  `tab_float_range`, `tab_docked_range`, `tab_docked`, `tab_float`,
  `tab_drag_docked`) — the theme/aligned-grid deltas plus the new read model
  account for every counted image; `bpm_window.png` visibly renders bare
  `BPM 128` in the status bar with `Capture 48 kHz` in the Tempo window —
  verified by eye.
- `docs/rust-hybrid-progress.md` honestly describes the feature.

### Findings

- **[P3] The `Timing` ComboBox commits on every change** — selecting the
  other unit converts immediately, wiping any pending edit to min/max
  DragValues in the same frame; acceptable and noted.
- **[P3] `bpm_with_rate` remains dead code** — the last callers were
  removed in `dad7c58e`, the function, re-export and tests still live in
  `model.rs`/`lib.rs`; delete in the next window.
- **[P3] No unit round-trip test through the `performance bar → CmbBox →
  setTimingUnit` flow** — covered at the engine level instead (selector
  payload unit tested; `set_timing_unit` round trip pinned in the store);
  fine.
- No P2. Round 155's red-test finding is closed by this commit.

## Round 158 — `1901063c..822e7303` (style: correct the bar-unit doc comment)

**Cosmetic doc fix, accurately observed and scoped.**

- `BEATS_PER_BAR`'s doc comment read "Unger's bar-based Scene timing is
  always 4/4" — a stray proper noun belonging to nothing in the tree;
  corrected to "Bar-based Scene timing is always 4/4".

### Findings

- No P3/P2.

## Round 159 — `822e7303..dde24d04` (feat: show each performance row's live Scene cell)

**The Scenes grid finally distinguishes applied-Scene from live-row state.**

- **The problem** — performance mode advances each row independently, but
  the grid highlighted the applied Scene's whole column and the header read
  the applied Scene everywhere, implying the whole column was live after
  rows had diverged.
- **The fix** — the snapshot now carries `active_scene_rows: BTreeMap<usize, String>`,
  built from the same live Scene-stamped Groups that feed
  `scenes/SET_ACTIVE_ROWS` (ghosts excluded, rows without a Scene stamp
  excluded). The pure decision `scene_cell_highlight(…)` splits:
  `Applied` (performance off), `Live` (per row, performance on), `None`.
- **The UI** — the applied Scene column gets a dim accent border + outline +
  `APPLIED` tag in the header (never "whole column live" when performance is
  on); each row's gutter shows the row's live Scene name (or nothing); each
  cell with a matching row gets an `ACCENT_DIM` `LIVE` badge under its timing
  chip; the accessible labels distinguish `"LIVE"` vs plain — kittest pins
  both.
- **Regression coverage** — 3 pure unit tests on `scene_cell_highlight`, 4
  paint/kittest tests (accessible label assertions, gutter naming), a
  divergent-columns golden (`scene_grid_live_rows.png`), plus `evidence`
  for performance off (historical APPLIED-whole-column behaviour
  preserved).
- **Verified** by eye via the committed golden and the matching tests;
  no behavior change when performance mode is off (historical whole-column
  applied highlight is intact).

### Findings

- **[P3] `active_scene_rows` is rebuilt every frame from live Group
  stamps** — defensive and correct (every frame reflects the current row
  assignment); the cost is a small `groups` walk; noted.
- **[P3] The LIVE badge is paint-only** — it has no click handler (the
  accessible label covers the kittest assertion); acceptable.
- No P2. The grid is now semantically honest about performance mode.

## Round 160 — `dde24d04..c667b1d6` (fix: keep transparent Point10 scenes in the Foreground row)

**Two default-scenes files corrected in lockstep; genuinely honest
re-seed discipline.**

- **The bug class** — Rolling Fire, Ball of Fire, DVD Room and Smoke Room
  render background-free (composite over the layer below); placing them in
  the Background or Effects row therefore hid the layer. The commit moves
  all such hero modules into Foreground as the first module, keeping the
  emptied Background/Effects rows inert and present.
- **Nine Scenes moved, three were already correct** — the commit lists all
  twelve; the emptied rows remain in place, preserving the row structure.
- **Two lanes maintained atomically** — `crates/modv-engine/data/default-scenes.json`
  and `src/renderer/src/application/worker/store/default-scenes.js` carry
  the same definitions (I verified: 62 scenes, same names, same order).
- **DEFAULTS_VERSION 16 → 17** in both runtimes, and `Smoke Room` joins
  `REFRESHED_DEFAULT_SCENES`; `REFRESHED_DEFAULT_SCENES` rebuilds Scenes
  in place only for installs on `defaultsVersion == 16`, never touching
  custom Scenes (matched by name only and only when they were seeded).
  `getScenesToSeed` is unchanged; the in-place refresh path builds the new
  definitions and preserves `introducedVersion`.
- **Tests** — both lanes assert the Foreground-only invariant and
  first-in-row order for the four modules; `DEFAULTS_VERSION` pinned to 17;
  the refresh plan covered end-to-end. Test counts consistent with
  defaults.v17 semantics.

### Findings

- **[P3] The `.gitignore` untouched, both lanes committed** — good; previously
  only the JSON lane was tracked in sync; now both are.
- **[P3] No e2e pixel diff for the Scenes re-placement** — GPU content in
  the foreground-positioned scenes is the same but the ordering changed;
  GPU tests cover registration; pixel parity is covered by eye in the
  prior rounds' evidence. Noted.
- No P2. The commit correctly uses the in-place refresh machinery; the
  default Scene experience improves for v16 installs on next launch.

## Round 161 — `c667b1d6..026b5721` (feat: the sandboxed Boa plugin host)

**A serious, capable, opt-in plugin substrate lands. No integration wires it yet.**

- **Architecture** — `plugins.rs` (2534 ln) builds a Boa context per
  registered plugin with a minimal CommonJS shim (`module`/`exports`/
  `globalThis.modVPlugin`), deleted after source evaluation
  (`remove_load_globals`). Host installs exactly five globals: `props`
  (mutable prop object), `modV.get(path)` (read-only deep-copy through
  the injected `PluginStateReader`; unknown → undefined; no writers),
  `modV.ws(url, payload)` (ws/wss only; payload is
  `JsValue → stringify`-bounded); `log()` (debug/info/warn/error, all
  capped). Capture is `MAX_CAPTURE_DIMENSION` 1024 + `MAX_CAPTURE_BYTES`
  4 MB.
- **Safety limits** — 1 MiB source cap, 128-deep lexical nesting, 256 MB
  parse stack on a dedicated validator thread (separate from the Boa
  context), Boa `RuntimeLimits` (1M loop iterations, 32 recursion depth,
  4096 values), and catch_unwind guards every hook. Failure isolation per
  plugin (throwing hooks skipped, recorded once per (plugin,phase), init
  failure disables only that plugin). `PLUGIN_NO_HEAP_CEILING` noted and
  honest — no wall-clock budget exists in Boa 0.22; the doc says so.
- **Tests (`tests/plugins.rs`, 24 cases)** — adversarial in the right
  ways: runaway loop is stopped by the runtime budget; a top-level loop
  at load is stopped during load; `require`, `process`, `fetch`,
  `WebSocket`, `XMLHttpRequest`, `setTimeout`, `console`, `module`,
  `exports`, `modV.set`, `modV.commit` all asserted absent; transport
  failures recorded once and don't stop sibling plugins; an init failure
  disables only the failing plugin; registration order preserved; captured
  `js/module` IIFE and `module.exports` alias both load; props clamp on
  every write (Int/Float/Bool/Text/Color/Select).
- **Wired substrate, not app-level surface** — `crates/modv-engine/src/lib.rs`
  exports the module; nothing in `modv-app`, the native shell, or any
  persist flag constructs the host today. Not a claim-vs-code issue: the
  commit documents itself as the host substrate (D5/P7). No consent
  dialog, no `--enable-plugins` flag, no persistence, no audio/features
  plumbing yet. The perf-hybrid integration sits on the tracker's WS-12.

### Findings

- **[P3] No app-level wiring** — the substrate is tested, gated and safe;
  the consent flow, the persisted setting and the action dispatch
  registration don't exist yet; this commit must be followed by the wiring
  column or the D5 milestone stalls at "substrate exists". Asked: the
  next batch wires at least the persisted switch + one plugin registration
  point.
- **[P3] No wall-clock or heap ceiling** — honest and documented; an
  array-heavy plugin can still OOM the host inside the loop budget; for an
  opt-in advanced surface this is the right call.
- **[P3] 256 MB parse-stack per source validation** — per-plugin transient,
  sequential; acceptable for a desktop app; noted.
- **[P3] `PLUGIN_PERSISTENCE=false` by construction** — no paths written by
  the host today; fine.
- **No P2.** This is one of the most hardened modules in the rewrite.

## Round 162 — `026b5721..c4fc956f` (feat: gate and discover opt-in Boa plugins)

**The consent gate and the discovery pipeline land — the wiring Round 161
said was missing, except the UI.**

- **Persisted gate (`<userData>/native-plugins.json`)** — default disabled
  AND unconsented; `set_consented(true)` then `set_enabled(true)`; withdrawing
  consent disables immediately and drops every Boa context; a hand-edited
  `enabled: true` without `consented` **normalises to disabled on read** —
  the bypass path is closed by construction, not by policy.
- **Discovery** — the active profile's `plugin/` folder plus
  `<userData>/plugins`; top-level `.js` only; hidden/unreadable skipped;
  source-size cap; canonical-path de-duplication; sorted; `MAX_PLUGINS`
  cap with a diagnostic; profile switch repoints the roots via
  `set_roots`.
- **Per-plugin enable switches** — keyed by absolute path in the same JSON;
  missing entries mean on; explicit `false` disables a single plugin without
  disabling the rest; the toggle reloads only the affected plugin.
- **The IO bundle** — `features_from_store` flattens the exact live audio
  frame (tempo/kick/bands/lanes/cells/wave/sampleRate + scalar Meyda
  features); `NativeStateReader::from_store` is the read-only allow-list
  (`beats.bpm`, `beats.kick`, `fps.fps`, `meyda.bands.*`,
  `meyda.getFeature.<name>` for the scalar features, `groups.<id>.name`,
  `modules.<id>.moduleName`) — mirroring the legacy `meyda/getFeature(…)`
  surface. Unknown paths → undefined (tested). The store is never exposed
  wholesale.
- **Transport is honestly absent** — `UnsupportedTransport` rejects every
  post; the host records a `Transport` error; the module doc and the switch
  say so plainly. ModV.WS is not wired.
- **Tests (16)** — atomic round-trip + malformed files, the consent gate
  (set/withdraw), discovery rules (hidden, over-size, non-js, sort, cap,
  dedup), disabled-runtime allocates nothing, enabling gates on consent,
  partial disable re-enable, frame payload shape, throttle/clamp/clear on
  disable, reader allow-list, transport errors recorded once.

### Findings

- **[P3] No UI is wired yet** — by design and documented; the panel is the
  missing Renderer surface. Until it lands the only toggles are the settings
  file or nothing at all for a real user; this must be the very next batch.
- **[P3] No filesystem watcher for the plugin folder** — explicit
  `rescan`/`reload` only; documented; fine.
- **[P3] Discovery errors are retained in `discovery_errors`** — used by the
  future panel; currently logged; fine.
- No P2. The consent discipline is correct and, importantly, enforced
  against hand-edited files.

## Round 163 — `c4fc956f..b4481f3c` (feat: run plugin frame hooks with capture)

**Completes the substrate: hooks actually run in the native shell.**

- **`pre_frame` / `post_frame` in the frame loop** —
  `plugin_pre_frame` is called after the UI drain and before composition
  (hooks see the current frame's audio+state); `plugin_post_frame` after
  `present()` (like the NDI read-back). A disabled host does zero work and
  allocates no Boa context — verified in `plugin_pre_frame`/`post_frame`
  short-circuits on `enabled()`.
- **Capture path factored out of `read_preview`** —
  `Compositor::read_frame_at(w, h)` now hosts the shared downsample body;
  the VMP1 preview calls it at 640×360; the plugin runtime calls it at a
  plugin-declared size clamped to `MAX_CAPTURE_DIMENSION`; alpha flattened
  to opaque in both. A GPU test asserts a small frame returns exactly the
  requested size with white pixels and α=255.
- **Throttling** — capture only runs when a loaded plugin asked for one;
  at most every 50 ms (20 Hz); the size change forces a cache reset;
  `FrameCapture` clamps too defensively.
- **Shutdown** — `plugin_runtime.shutdown()` runs `shutdown` hooks and
  drops every context on quit.
- **The switch logging** — enabled plugins log loaded count at startup;
  disabled host says "plugins discovered; disabled" — informative without
  being noisy.
- The `plugins_mut()` accessor exposes enable/consent to the future panel.

### Findings

- **[P3] No UI yet** — the third commit in a row noting this; the Plugins
  panel (enable/consent/reload incl. per-plugin rows) is documented as the
  next batch; this review will block on it for the milestone.
- **[P3] `modV.ws` transport unwired** — documented; recorded.
- **[P3] `pre_frame` allocates features+state every frame while enabled** —
  cheap (two Rc clones + a ~2 KB map); fine; noted.
- **[P3] Capture read-back is synchronising** — same cost class as the NDI
  read-back and the preview; throttled to 20 Hz; correct trade-off.
- No P2. The runtime path now matches the JS plugin host's shape on the
  opt-in frame lifecycle, with the right read-back discipline.

## Round 164 — `b4481f3c..8e16e008` (feat: model the opt-in plugin panel)

**The read model for the future Plugins panel lands.**

- **`PluginView`** — the snapshot's plugin payload shape (master gate,
  consented flag, loaded count, discovered plugins, deduplicated log lines,
  requested capture size); `is_running()` = `enabled && consented`;
  `gate_reason()` renders the "not running yet" text; `summary()` renders
  "N of M plugins loaded" copy; `capture_label()` renders the largest
  requested capture side.
- **`PluginInfoView`** — one per discovered script: path/name/author/
  version, per-plugin switch/active/loaded/failed/error/source-bytes/
  capture/**props**; `title()` falls back to the script file stem; status
  dot (`Loaded`/`Disabled`/`Error`); gate reason.
- **`PluginPropView`** — every prop kind
  (Int/Float/Bool/Text/Color/Select) parsed from the runtime's serialised
  schema; `from_schema` skips malformed entries rather than failing the
  whole panel; `bounds()` widens to (value, default, 0..1) when the
  descriptor declares no floor/ceiling (Int clamps sensible step defaults);
  `number_value` rounds Ints (floated); colour payloads serialise the
  legacy `{r,g,b,a}` ratio with micro-precision and clamping.
- **`PluginPropKindView::parse`**, `PluginPropOptionView`, and the
  select/list indexer (`select_label()`) — pure and unit-pinned.
- **~30 model unit tests** cover: schema parse kinds, unknown-type skip,
  select skip, fallback bounds, int-round, colour payload round, error
  coercion fallback, summary/is_running/gate_reason/capture_label lines.
- **Nothing is wired to a renderer yet** — the panel itself (a
  `plugins_panel` replacement for the existing placeholder) still isn't in
  the tree; so `PluginView` is constructed nowhere live (dead-end safety
  used only by tests and the snapshot struct).

### Findings

- **[P3] The shipped `plugins_panel()` in `shell.rs` is stale and false** —
  it shows the old placeholder text claiming the native shell "has not
  landed the sandboxed Boa host yet" and the master switch is
  "deliberately disabled until the consent flow and restricted host API
  exist". Both statements are false since `026b5721` (host shipped),
  `c4fc956f` (consent-normalised gate + allow-listed state reader) and
  `b4481f3c` (frame hooks running). The panel misinforms the user about
  shipped capability. Two lines of copy, or the Plugins panel commit as
  the follow-through — flagging here because the user-facing string is a
  visible claim-vs-code. Open at `8e16e008`.
- **[P3] `plugins_panel()` is rendered in the tab strip** — selecting it
  shows the stale copy. The new `PluginView` model exists and is tested but
  nothing instantiates it from `NativeApp`; the snapshot carries no
  `PluginView` (native.rs not touched in this commit) — so the Plugins
  panel's true enablement is still pending.
- No P2.

## Round 165 — `8e16e008..18fcdc81` (feat: retain plugin logs and prop writes)

**The runtime becomes authoritative on writes; the panel gets its data.**

- **Bounded, deduplicated retained log window** — the `plugin: …` lines fed
  by hooks now keep the last 256 unique lines for the panel; the existing
  seen-set (4096) remains the dedupe of the file. `clear_logs()` drops the
  panel window without re-emitting already-seen lines (by construction, a
  per-frame logger cannot repopulate it); fresh distinct events still land.
- **Error forwarding tested** — a plugin whose `preProcessFrame` throws has
  its error retained in the panel log **and** surfaced in `PluginView.error`
  (`boom` finds both paths).
- **Prop writes** — `set_prop(path, prop, value)` routes through
  `PluginHost::set_prop` (schema coercion: Int rounds/clamps, Float floats,
  Text clips, color ratios, Select falls back to default on unknown), persists
  the coerced value in `native-plugins.json::props` (path+prop keyed), and
  **reapplies the saved coercions after every load/change/restart** — a
  reload, rescan or fresh process restores every control the user moved.
- **Persistence is atomic** (`json.tmp` + rename) and the document keeps
  coercions even when the plugin later renames the prop (`props` entries
  the current schema doesn't declare are skipped on apply, never deleted).
- **Snapshot discipline noted** — `native.rs` now carries
  `plugins: Default::default()`: the UI panel is still unfed (placeholder
  default in the read model), so the actual panel must come next; nothing
  consumes this yet.
- **Tests cover every branch** — log dedupe/clear/reload semantics, error
  forwarding, full prop coercion matrix (clamps, text, colour, select
  fallback), live descriptor sync, persisted values after reload and fresh
  restart, unknown plugins/props (error not panic), and schema-disabled
  writes rejected.

### Findings

- **[P3] `plugins: Default::default()` in the live snapshot** — unchanged
  claim-vs-code status: the panel is still unwired; ask: populate
  `EngineSnapshot::plugins` with a `plugin_snapshot()` builder in that next
  batch so the Plugins tab shows truth on open.
- **[P3] `apply_saved_props` re-applies only after `load_pending`** — a
  settings-file edit between runs is reapplied, not mid-frame ever (by
  design); fine.
- **[P3] `apply_saved_props` ignores saved entries for absent schema props
  silently** — the whole property stays in `native-plugins.json` forever; a
  stale prop after a plugin schema rename lingers; acceptable; noted.
- **[P3] `.remove(0)` on a 256-element Vec** — O(n) with a tiny constant; fine.
- No P2.

## Round 166 — `18fcdc81..c7a6bac1` (fix: apply only the clicked Scene row)

**The grid now honours the per-row model.**

- **The defect** — clicking a Scene cell dispatched `scenes/applyScene`,
  which rebuilds every Group in the Scene: a cell click was a whole-Scene
  apply, despite the per-row advance fantasy elsewhere.
- **The fix** — cell click dispatches `scenes/applySceneRow` with the
  `rowIndex` payload; the engine arm accepts it (verified at
  `store_ops.rs:726`: `sceneId` required, `rowIndex`/`row` accepted,
  documented null-outcome behavior) and replays only that row's layer;
  the column header and the `Apply whole Scene` menu entry keep
  `scenes/applyScene`.
- **Context menu** — a new `Apply this row` button (enabled only when the
  cell holds a Group) precedes `Apply whole Scene`.
- **Tests** — `clicking_a_cell_applies_only_that_row` pins the exact
  `applySceneRow` payload and forbids `applyScene`; `clicking_an_empty_cell_applies_nothing`
  pins no action; the existing dot-focus tests assert no `applySceneRow` as
  well.

### Findings

- **[P3] `apply_scene_row_transition` is not exercised in the engine suite
  for this dispatch** — covered in rounds 101–105; fine.
- No P2.

## Round 167 — `c7a6bac1..abd0c51e` (feat: consent-gate the Plugins panel)

**The stale placeholder is gone. The Plugins tab is now a real, safe gate.**

- **The old false copy is deleted** — the panel no longer claims the Boa
  host doesn't exist or is deliberately disabled; the new text describes
  the sandbox accurately ("no DOM, no filesystem/network globals, not a
  security boundary"; "only enable scripts you trust"; "modV.ws is not
  wired yet"). Round 164's user-facing claim-vs-code finding is **closed**
  by this commit.
- **The consent flow** — the master checkbox never runs anything directly:
  first enable opens the modal dialog; confirming queues
  `SetPluginConsent(true)` **then** `SetPluginsEnabled(true)` in that order;
  cancelling queues nothing; consenting later needs no dialog; disabling is
  immediate; **Revoke consent** withdraws the whole gate.
- **The modal's truthful wording** — "arbitrary JavaScript", "allow-listed
  read-only snapshot", "not a security boundary"; exactly the right
  disclosure.
- **The snapshot now carries live data** — `EngineSnapshot::plugins` reads
  the cached `plugin_view`, rebuilt at **1 Hz while the tab is visible**
  (and immediately after a plugin command) — never per frame; the retained
  log window is cleared on reopen; a background refresh picks up hook
  churn (a plugin may flip its own config).
- **`plugins_panel` wiring verified end-to-end** in
  `native.rs::poll_panel_views` — visible-tab gating identical to
  NDI/OSC; plugin-poll window; `plugins_seen` gate to clear logs exactly
  on reopen.
- **Kittest covers the gate matrix** — first-enable-opens-dialog, confirm
  flow (both commands queued in order), cancel-queues-nothing, gated
  re-open doesn't repeat the dialog, disabling immediate revocation,
  revoke-with-tools-off, the "why the list is greyed" explanatory line.

### Findings

- **[P3] The panel shows only the gate + a discovery count** — the
  discovered-plugin rows, the per-plugin enables, the prop controls, the
  retained log viewer and `SetPluginProp` **don't exist yet**; the read
  model from Rounds 164–165 goes unused. Incremental landing is fine, but
  the panel is currently a gate in an empty room. Asked: next batch lists
  the discovered plugins and wires one control.
- **[P3] `plugins_panel.rs` header comment references a non-existent
  `UiCommand::SetPluginProp`** — the comment says prop edits become
  `UiCommand::SetPluginProp`, but `UiCommand` only has `SetPluginsEnabled`
  and `SetPluginConsent`; the comment describes a command the crate
  doesn't have. Stale comment, same docs-claim class, in the same file that
  tests the gate — pin it when the prop command lands.
- **[P3] × No UI for `plugin_runtime.set_prop` yet** — same line item.
- No P2.

## Round 168 — `abd0c51e..be173841` (feat: retire Strobe Riot)

**The first version-keyed removal rule lands — the correct mechanism, with
byte-accurate coordination.**

- **The change** — `DEFAULT_SCENES` loses `Strobe Riot` in both lanes
  (`default-scenes.json` deleted the block; `default-scenes.js` deleted the
  scene); `REMOVED_DEFAULT_SCENES` gains it in both;
  `REFRESHED_DEFAULT_SCENES` loses it in both (it would otherwise rebuild an
  empty shell); `DEFAULTS_VERSION` 17→18 in both; e2e (`defaultScenes.spec.js`)
  and unit-test version asserts bumped to 18; the version-16-era comments
  swept to 18.
- **The migration table is populated for the first time** —
  `DEFAULT_SCENE_MIGRATIONS` ships one row: `{version: 18, removed:
  ["Strobe Riot"]}`. The mechanism was exercised with fixtures until now;
  this is the first production rule. The module doc explains why previous
  edits were refresh-only and the doc-line about "the production table is
  empty" is honestly rewritten.
- **Seeded-vs-custom semantics pinned three ways** — (a) store-level test:
  a seeded Strobe Riot disappears on upgrade; (b) store-level test: a
  custom Scene with the same name survives; (c) migration-table test: seeded
  → removed, unseeded → reported skipped. All three are the same rule,
  enforced at the right layers.
- **Sync verified** — 61/61 scenes, byte-identical order, in both lanes;
  `REMOVED`/`REFRESHED` sets agree across lanes; e2e/unit asserts agree.

### Findings

- **[P3] `the_production_path_is_a_no_op_until_a_rule_ships` test retired** —
  correctly replaced by the version-18 tests; the new tests cover both arms
  (seeded removed, unseeded skipped); noted.
- **[P3] The e2e bumps reference-only the constants** — fine.
- No P2. A disciplined retirement with no user data at risk.

## Round 169 — `be173841..1515694a` (feat: list and configure Boa plugins)

**The last layer of the plugin story lands — discovered scripts render and edit.**

- **Per-plugin rows** — status dot (green/red/grey for Loaded/Error/Disabled),
  name + version + author line, title falls back to the file stem, muted
  truncated path (full path on hover), per-plugin Enabled checkbox issuing
  `SetPluginEnabled { path, enabled }`, capture size, and the plugin's
  generated controls (preallocated rows exist only for **running** plugins
  — controls are disabled unless the host is running this plugin).
- **Prop controls** — Int/Float via `RangeControl` honouring
  min/max/step/default (default visual marker); Bool checkbox; Text
  singleline; colour `{r,g,b,a}` ratios; Select dropdown; every write emits
  `SetPluginProp { path, prop, value }`. Int rounds, floats stay floats,
  colour channels clamp micro-rounded ratios (0..1, 6dp).
- **Runtime logs section** — collapsible `Logs & errors (N)` header, default
  open when any line contains error/warn; Clear drops the panel window;
  Copy pipes joined lines into the clipboard; DANGER/WARNING/MUTED colour
  coding; empty state reads "No plugin messages."
- **New empty-state** — when discovery yields nothing, the panel points at
  the plugin folders (`<active profile>/plugin` and `<userData>/plugins`).
- **Reload/Rescan** buttons queue the `ReloadPlugins`/`RescanPlugins`
  commands; the shell applies them through `PluginRuntime` and refreshes
  the cached view **immediately** and at 1 Hz while the tab is visible.
- **The new golden `plugins_panel.png` honestly shows three states** — a loaded plugin
  with sliders/colour/checkbox props, a failing plugin ("load failed:
  unexpected token at line 4"), and a disabled plugin — plus the gate row,
  capture line, log section, and the empty discovery placeholder; the
  `plugins_panel.rs` kittest covers payloads/toggles/error paths; a native
  view-builder test pins `plugin_snapshot`.
- **`SetPluginProp`/`RescanPlugins`/`ReloadPlugins`/`ClearPluginLogs` are all
  new `UiCommand` variants**, routed in the dispatch table to
  `PluginRuntime`; `plugin_view` clones into the snapshot (as already done),
  so the read model finally shows live truth.

### Findings

- **[P3] The snapshot rebuilt every 1 Hz clones `logs` (up to 256 lines) and
  `plugins`** — fine at this size; a diff-checker would be overkill.
- **[P3] The colour channels aren't wired to the picker widget yet** — raw
  `0.100/0.800/0.400/1.000` four f32s in a row; usable but not as pretty as
  the existing ColorControl; polish noted for a follow-up.
- **[P3] Logs default-open only on error/warn** — good discipline; noted.
- **[P3] The `plugins_panel.rs` doc comment still cites the `SetPluginProp`
  command that now exists** — the Round-167 stale-comment finding is healed
  by this commit.
- **No P2.** The native shell now ships the complete D5 plugin milestone
  end-to-end: sandbox → gate → frame hooks → discovery → panel. The one
  milestone the rewrite was missing is closed.

## Round 170 — `1515694a..6d9bb058` (feat: list cameras in Webcam-index order)

**`orderEngineCameraInputs` is ported faithfully.**

- **Semantics** — running cameras lead in Webcam-module 1-based index order;
  non-running cameras follow, case-insensitively by name with id tiebreak;
  non-camera sources don't enter the list at all.
- **Sort order** — Rust sorts running by `(index, name)` and the tail
  by `(name.lowercase, name, id)` — equivalent to the JS `localeCompare`
  for the ASCII names V4L2 devices expose; divergence on mixed-case names
  is theoretically possible but cosmetic.
- **Feed** — `running_cameras` feeds the order; the UI iterates
  `media.ordered_cameras()` in place of `media.cameras`; the `#N` badges
  and reorder arrows are unchanged.
- **Tests** — three pure unit tests (running-first ordering with reversed
  names, unselected-by-name order, non-camera sources and zero-index
  exclusions) and a kittest that asserts the on-screen top-to-bottom order.

### Findings

- **[P3] JS `localeCompare` vs Rust `to_lowercase` tie-break** — documented;
  ASCII-equivalent; non-blocking.
- No P2. A correct surface, byte-ported where sensible.

## Round 171 — `6d9bb058..ee65cd55` (feat: scan and watch custom ISF folders recursively)

**The user ISF pipeline goes full-recursive — with honest limits everywhere.**

- **Watcher (`media_watch.rs`)** — `RecursiveMode::Recursive`; hidden/temp/
  backup names ignored at **every path component** (a `.cache/pack/x.fs` is
  ignored, while `artist/pack/x.fs` fires); longest matching root wins for
  nested extras; missing folders are simply unwatched (partial trees still
  work); integration tests pin nested writes, nested deletes and nested
  extra-ISF bursts.
- **Registry scan (`module_registry.rs`)** — corpus stays flat; user folders
  walk recursively (sorted per directory; depth ≤ 8; ≤ 4096 entries per
  root; symlink cycles refused outright; hidden/temporary names rejected at
  every level); duplicate-name resolution deterministic (corpus wins, then
  sorted-first path; each ignored duplicate logged once); hash-different
  registrations collapse (a root nested inside another); sorting also makes
  the entry cap deterministic.
- **Posture honest** — the warning is explicit about depth-limited/entry-
  limited skips and `.vs` siblings get picked up for nested `.fs` files.

### Findings

- **[P3] `isf/compiled` is no longer ignored** — the old non-recursive
  watcher ignored the compiled tree; under the recursive watch a recompiled
  `.fs` under `compiled/` fires a burst like any other `.fs` change. Current
  behavior: one extra (debounced) rescan per compile burst — acceptable;
  noted for the next media-watch iteration; not a correctness issue.
- **[P3] Both `scan` passes are sorted, `visited` counts entries including
  the ignored, cap's determinism** — fine by design.
- No P2. The commit is the union of the JS media-manager's recursive
  behavior and honest limits the JS never had (depth, entries, symlinks).

## Round 172 — `ee65cd55..cbdcbdae` (feat: share a monitor or window through the portal)

**The screen-capture feature lands — with the right lifetimes, the right
one-at-a-time gate, and no portal pledge.**

- **Where it comes from** — the native shell asks the XDG desktop portal
  **itself** (`modv_media::video::portal::request_screencast`): the desktop's
  compositor owns the dialog; the portal completes with a PipeWire node id;
  `pipewiresrc path=<node>` captures it as the `screen:portal` MediaHub
  source, camera-like (Webcam-indexed).
- **Ownership model is exactly right** — the portal session dies with the
  D-Bus connection, so `ScreenShare` *owns* the `PortalStream` for the whole
  capture; stop handing the handle back; the PipeWire node stays alive exactly
  as long as the capture. Without the keep-alive, dropping stops the pipeline
  immediately.
- **One request, one capture** — `request()` refuses while busy (dialog or
  live capture occupy the single screen slot); a second `ShareScreen` is
  warned and ignored; the Media panel buttons disable accordingly.
- **Stale-answer race handled** — every answer carries a monotonic id;
  `complete` only applies pending ids; stale handles are dropped (closing
  the portal session); cancel works by dropping the receiver; abandoned
  answers drained at 1 Hz.
- **Screens are never opened silently** — if the host can't capture a
  PipeWire node (no `pipewiresrc` / no socket), the panel renders
  "unavailable: <reason>" and the request is refused before the dialog
  shows; capability detected at startup and re-probed at 1 Hz via
  `screen_support()`.
- **Threading** — the blocking portal call runs on a dedicated worker
  thread (`spawn_portal_request`, the only portal touchpoint); the frame
  loop drains answers non-blockingly, at most one request outstanding; a
  thread that stops without an answer becomes a Runtime error, not a
  wedged app.
- **Error codes match the engine's `media/start-*` vocabulary** —
  cancelled/media_unavailable/media_start_failed; errors recorded and
  logged once; the panel shows them.
- **Webcam indexing** — the screen is camera-like and `start()` yields the
  implementation Webcam index; the Webcam module can select it via camera
  prop like any camera.
- **Tests** — the full state machine in `screen_share.rs` (one-slot,
  monotonic ids, stale-drop, cancel, start/fail flows) with a fake stream;
  the actual portal dialog never opens in tests.

### Findings

- **[P3] The path is honestly logged once at startup (not re-probed)** —
  `screen_support()` is called at startup to cache the reason; if the host
  gains PipeWire/GStreamer afterwards (e.g. installs `pipewiresrc`), the
  refusal reason stays stale until restart; noted.
- **[P3] No `restore_token` path used** — `request_screencast_restoring` exists
  but the request always errors on dialog rather than attempting resume; a
  restore-token upgrade could short-circuit the picker for engines that support it;
  deliberate minimal surface; noted.
- **[P3] `MAX_SCREEN_ANSWERS` bound is a safety cap only** — fine.
- No P2. The screen-capture milestone that's been pending since the media
  PR is finally closed; the remaining caveat is the portal/PipeWire being
  desktop-only by nature.

## Round 173 — `cbdcbdae..65d14072` (feat: the screen-capture controls)

**The Media panel gains the Share monitor / Share window surface.**

- **The section renders in the same place as camera rows** — a "Screen
  capture" header, an unavailable banner with the host's reason when one is
  known, and two buttons (Share monitor…/Share window…), both disabled
  while a request is pending or a capture is live.
- **One-plug flow** — Share → `MediaAction::ShareScreen { window: bool }`;
  the panel's buttons re-enable only once the portal dialog resolves or
  the capture ends.
- **Pending state** — while the dialog is open the panel shows "waiting
  for the dialog (monitor|window)" with a Cancel action;
  `MediaAction::CancelScreenShare` — the dialog itself can't be dismissed
  from the panel, only from the compositor, but the pending state is
  observable and abortable.
- **Running state** — "Running (monitor|window): screen:portal (PipeWire
  node N)"; the user stops it from the existing Stop button for the
  `screen:portal` MediaHub source (camera-like — the panel's single stop
  affordance covers it, consistent with camera rows).
- **Error** — any per-host error is shown below; the most-recent one
  sticks until the next attempt clears it.
- Cycle covered by kittest (`media_screen_capture.rs`, 247 lines) — button
  enabling states, payloads, pending/running/end-to-end rendering of
  `screen:portal`, unavailable gating, cancel path.
- The snapshot's `MediaSnapshot::screen` feeds the same read model used
  everywhere (`ScreenCaptureView`).

### Findings

- **[P3] The error stays visible until the next request clears it** — correct
  UX; noted.
- **[P3] The screen row lacks the "camera-like" badge consumers might
  expect** — the MediaHub index is opaque to this panel; noted for the next batch.
- No P2. Screen capture is feature-complete for this era's native shell.

## Round 174 — `65d14072..a20f105f` (feat: index the media library and add the Library tab)

**The biggest single commit of the night — a full third-party media index
plus a brand-new tab.**

- **`media_library.rs`** — scans the active profile's `image/`, `video/`
  and `palette/` folders recursively into a capped (200 per kind),
  deterministic index; hidden/temp names ignored at every level; depth
  capped; decode paths reuse `image` crate (`decode_file`) for image
  thumbnails (downscaled, aspect-preserved), palette JSON → colour strip
  thumbnails (16 colours max); MP4/WebM headers read for duration +
  resolution (MP4 `moov` caps at 32 MB); cache of decoded thumbnails with a
  bounded size; watcher bursts coalesce to one rescan.
- **The Library tab** — a right-strip tab between Media and Audio with
  folder switching (one per kind), folder navigation (breadcrumb/back),
  subfolder chips, rescan button, error/empty states;
  drag-and-drop/click assignments for TextureControl props
  (`LibraryAssignmentTarget`); drag image/palette thumbnails into texture
  props.
- **The read model** — `MediaLibraryView` carrying scanned items with
  thumbnails, counts, truncation flags; thumbnails uploaded at
  `NativeApp::next_frame`; caching honours a generation token and clears on
  rescan and profile switch.
- **The screenshot suite re-baselined** with the new tab visible (19
  goldens, all counted).
- **Tests** — 16 unit tests in `media_library.rs` (walk caps/depths/errors,
  palette JSON round-trips, downscale/widths, cache eviction, watcher
  bursts, profile switching); a kittest `media_library_panel.rs` (245 ln); a
  committed `library_panel.png` golden; a `SidePanelTab::Library` unit test.
- **State surface** — `library_kind`, `library_folder`,
  `library_target`, `library_drag`, `library_search`.

### Findings

- **[P3] No video poster frame** — explicitly deferred in the commit
  ("a first-frame poster is deferred"); duration/res use container headers
  only; noted.
- **[P3] Document folders deliberately not indexed** — noted as scope choice.
- **[P3] `MAX_ASSETS_PER_KIND = 200`** — truncation is reported in the UI;
  fine.
- **[P3] Thumbnails assigned by absolute path** — the `MediaAction` has no
  structured path; fine given the model.
- **[P3] The library golden shows the empty state** — honest.
- No P2. Real library workflow for images/videos/palettes is finally in the
  native shell.

## Round 175 — `a20f105f..7f4d16ce` (feat: assign library assets to texture props)

**Two assignment flows, one value builder — Byte-true to the manual path.**

- **The prop surface** — `TextureControl::draw` now returns a typed
  `TextureControlChange` (`Value(value)` | `PickFromLibrary { kind }`)
  instead of a bare Value, so the panel can distinguish "the user edited" from
  "the user wants a library ASSET". Manual edits produce the exact same
  `{ type, options: { path } }` payload as before; nothing else re-encodes it.
- **"Use from library…"** — rendered only for `image`/`video` props, arms
  `LibraryAssignmentTarget { module_id, module_name, prop_name, kind }`,
  switches to the Library tab, highlights matching tiles, assigns on the next
  click; the banner names it; the banner clears on assignment.
- **Drag-and-drop** — Library tiles are `click_and_drag` sources carrying a
  typed `MediaLibraryDrag { path, kind, name }` payload. Image/video texture
  controls accept matching drops: highlight wash + border + `Drop <name>`
  chip, `Copy` cursor, single-drop consumed on release. Kind-mismatched
  assets are **not drop targets** (a video can't replace an image prop);
  palettes can't fill texture props at all.
- **Drag drops nothing in the empty space** — the payload is consumed only when
  a matching control asks for it; manual paths unaffected.
- **Tests are adversarial in the right ways** — click-to-assign arms and
  clears, cancel keeps prop untouched, manual path still writes same
  payload, typed payload while dragging, drag/drop writes the same value,
  video→image prop refused, image→video prop coherent, palette refusal,
  empty-path refusal; the kittest path asserts the exact `texture_prop_action`
  payload equality against the manual path editor.
- **The `TextureControlChange` split is wired everywhere** — Inspector prop
  loop and the `PickFromLibrary` arm call `arm_library_target`; the old bare
  Value path is gone.

### Findings

- **[P3] No video drag image preview/ghost** — default egui DnD affordance;
  fine.
- **[P3] The banner copy "Pick images for <module> / <prop>" is honest** —
  fine.
- **[P3] `clear_library_target` clears only the target, not the folder
  memory** — cancelling a pending pick leaves `library_kind` set, so the
  next Library visit opens the same folder (arguably the right memory;
  noting for honesty).
- **No P2.** The complete feature — scan/thumbnails/tab/assignment — is
  live and coherent.

## Round 176 — `7f4d16ce..19af2dad` (feat: import loose media-tree Scenes once per profile)

**The migration hole closes — silently lost Scene documents come home.**

- **The problem** — the native shell boots `<userData>/engine-state.json`
  for the engine mirror and `<userData>/scenes.json` for the Scene grid,
  but a user who saved Scene documents under
  `<userData>/media/<profile>/scene/` (an electron-habit install) had them
  invisible to both.
- **The import** — one-time, per profile, in deterministic order; recursive
  scan of `scene/` and `preset/`, hidden/temp/symlinked-directory/oversized
  (>4 MiB) files skipped; `classify_document` routes them (preset/playlist/
  Scene); Scenes go through `scenes/loadSceneFromJson` with `persist: false`
  so we only write to scenes.json at the end; existing ids are skipped
  (scenes.json wins); a new id whose trimmed case-insensitive name collides
  keeps the existing Scene and reports the file; playlists are counted, not
  imported; malformed JSON counted; **originals never rewritten/moved/deleted**.
- **Per-profile flag** — `native-project.json` gains
  `imported_documents: { "<profile>": true }`; `documents_imported(profile)`
  before any walk; `mark_documents_imported` after; a profile first used at
  runtime imports on the switch; profiles each import exactly once.
- **The media root plumbing** — the native shell now dispatches
  `media/setMediaDirectoryPath` (startup + profile switches) and points the
  compositor's `ImageTextures`/previews at it, so library-relative image
  props resolve under `MODV_USER_DATA` as well as the default tree.
- **Tests** — mixed-tree (new/existing id, name collision, malformed,
  oversized, preset, playlist, hidden/temp, nested folder); original-file
  preservation (byte-compared); idempotent reruns; per-profile flags;
  settings round-trip; image-resolution under the root.

### Findings

- **[P3] `persist: false` + final `persist_scenes` is the correct shape** —
  a single atomic write at the end; noted.
- **[P3] The import scans `preset/` too but only reports** — presets are
  documented as "still openable through File → Open preset…", not imported
  (scenes.json only stores Scenes), noted as deliberate.
- **[P3] The flag's key space has no prune for deleted profiles** — stale
  entries accumulate in `native-project.json` forever; cosmetic; noted.
- No P2. A silent data loss wedge is closed — the kind of regression a
  user would never see coming.

## Round 177 — `19af2dad..e1985c34` (fix: documentation note for the import flag)

Docs-only same-message fixup: the module header now says that picking up
files added later requires removing the profile's entry from
`native-project.json` (id/name checks still prevent duplicate Scenes) — the
rescan story that was implicit is now explicit. No code change.

## Round 178 — `e1985c34..64f364f1` (feat: add video-file transport controls)

**Playback transport (pause/resume/seek/loop/position) lands in the media
layer — engine-side, engine-facing, honest about live sources.**

- **`transport.rs`** — a `VideoTransport` snapshot: `paused`, `playing`,
  `position_ms`, `duration_ms`, `looping`, `eos`, `seekable`; a `progress()`
  ratio helper; saturating seeks; 30 Hz-ish pollable, no callback, no new
  thread.
- **`source.rs`** — `VideoSource::transport()`, `pause()`, `resume()`,
  `set_paused(_)`, `set_looping(_)`, `seek(_)`; `.pause()` stops frame
  publication (settled test asserts bytes stop flowing); a file seek uses
  a flushing accurate `seek_simple` on the pipeline; seek while paused
  shows the prerolled frame; seek while playing resumes from the new spot;
  `stop()` clears the transport entirely (`pause`/`seek` after `stop()` are
  `Stopped` errors; `source.is_paused()` reflects the flag).
- **`hub.rs`** — per-source `transport(id)`, `transports()`, `seek_source`,
  `set_looping_source`, `set_paused_source`, used by the engine thread; all
  called from the render thread; GStreamer state changes are queued and
  return without waiting for streaming threads, so no frame-loop stalls.
- **Loop semantics** — end-of-stream latch; with `looping` (the default for
  files) the source seeks back to zero and keeps playing; without it the
  source parks on its last frame with `eos: true` until a seek/resume.
- **Live sources** — `seekable == false`; `seek_source` returns
  `VideoError::Unsupported`; `position_ms == 0`; `duration_ms == None` —
  consistent with cameras and test patterns; tested.
- **Tests** — 5 + 8 + 17 tests (transport, hub, source) incl. pause stops
  frames, live pattern behaves per contract, real-file duration/position
  polls, stop clears the transport, eos-and-park (file-perfect) is not
  yet unit-pinned against GStreamer (lavapipe gst element availability
  varies), live-pattern looping default and pause behavior pinned.
- **Not wired yet** — the engine's `MediaAction` dispatch doesn't expose
  these to the UI (`SetPause`/`Seek`/`SetLooping`, a scrubber); this is the
  bottom of the three-layer implementation; the native media panel still
  has no transport controls. Honest as a raw commitment with full coverage.

### Findings

- **[P3] No UI wiring in this commit** — callers must cross through
  `modv-media` until the m panel grows playback controls; ask: next batch
  wires `MediaAction` variants + a scrubber into the Media panel.
- **[P3] `looping` defaults to true for all sources incl. cameras** — cameras
  ignore loop anyway (no timeline to loop); noted, matches `is_seekable() == false`.
- **[P3] No file-end-of-stream unit test in the file fixture** — exists but
  skip-if-missing; noted; matches lavapipe precedent elsewhere.
- No P2. The substrate doc-wise exactly documents the panel-time to come.

## Round 179 — `64f364f1..c96601a6` (feat: edit colour palettes as swatches on tween props)

**The palette tween editor lands — byte-true read/write of the Ball pattern.**

- **Detection** — `palette_shape(data)`: 2+ rows, every row finite with 3
  (RGB) or 4 (RGBA) channels in 0..=255, at least one channel above 1.0
  (so a normalised vec3 stays a numeric curve); 4-channel rows are ratios
  when every alpha ≤ 1.0.
- **Swatch read/write** — `row_rgba` (clamped, rounded bytes, ratio→8-bit
  conversion for ratio alphas), `row_hex`, `row_color`, `rgba_hex`,
  `parse_hex` (CSS `#rgb`/`#rgba`/`#rrggbb`/`#rrggbbaa`, optional `#`),
  `add_row` (duplicate last, append), `remove_row` (floor of two rows),
  `times_after_row_change` (re-even an explicit `keyframeTimes` on
  add/remove; drop `None` and two-row shorthand), `set_row_rgba` (RGB always
  write, ratio alpha converts back).
- **UI** — the tween control row shows a read-only swatch strip (hover shows
  hex); the curve window gains the editable `Palette` strip (swatch
  colour-pick on click, right-click remove, `+` appends the last colour);
  both live alongside the channel graph with colour-coded channels (red/blue
  coast/green ramp).
- **Wire format** — palette edits rewrite the full descriptor and dispatch
  `tweens/updateTween` exactly like the numeric curve editor; undo boundaries
  uniform; `.fs` tweens/recorded keyframeTimes round-trip correctly.
- **Kittest + goldens** — `tween_palette_editor.rs` pins dispatch payloads
  (add, remove, hex edit including malformed rejected); the committed
  `tween_palette_editor.png` honestly shows the strip + the curve graph +
  the `data` JSON with the four colour rows and the `[0,0.3,0.6,1]` times.

### Findings

- **[P3] Any vec3-ish tween that happens to range 0..255 reads as a
  palette** — explicitly documented; consistent with Electron's ball shape;
  fine.
- **[P3] Palettes are 3/4-channel only** — no 2/3-channel general fallback; fine.
- **No P2.** The feature works; the parity with `PaletteControl.vue` is
  faithfully maintained.

## Round 180 — `c96601a6..a1016475` (feat: add playback-rate control to the video transport)

**The legacy `VideoControl.vue` speed slider's engine lands.**

- **`set_playback_rate`** — a flushing, accurate seek at the current position
  carrying GStreamer's seek rate (there is no separate "set rate" call);
  pipeline state untouched, so a paused file stays paused and a parked file
  keeps its latch (rate applies on replay).
- **`position_ms()` now caches the last known position** — a flushing seek
  nulls `query_position()` for a poll or two; without the cache, a fast
  scrubber or a rapid rate drag would anchor at zero. Seek targets also
  store themselves until the flush settles.
- **Range** — `MIN=0.1`, `MAX=10.0` matching the legacy slider; out-of-range
  finite clamps in; non-finite/non-positive rejected; backwards play
  refused (GStreamer can, but the legacy surface doesn't).
- **`VideoTransport` gains `playback_rate: f64`** (manual `Default` 1.0);
  `seekable == false` → `Unsupported` for live sources (checked before the
  EOS check — glitches rejected cleanly); a rapid-rate-change test pins
  `after.position_ms + 100 >= before` on an in-flight flush.
- **Tests** — real-pipeline integration tests (2x advancing faster, 0.25x
  advancing slower, paused-preservation, parked-EOS-preserved, looping
  survives rate changes, rapid rate changes keep position) all skip cleanly
  only when `videotestsrc`/`appsink` are absent; unusable rates rejected;
  clamps unit-tested.

### Findings

- **[P3] Rate change at EOS stores the rate without replaying** — the
  alternative (seeking at EOS to reapply) is worse; noted; the fixture test
  replays via `seek(0)`.
- **[P3] No UI yet** — substrate unchanged from Round 178's cadence; next
  batch presumably wires `MediaAction::SetVideoRate`.
- No P2.

## Round 181 — `a1016475..1416ee38` (feat: start and reconcile video-file sources for texture props)

**The missing pipeline gaffe — video texture props finally get real file
sources.**

- **The gap it closes** — the legacy renderer starts one hidden `<video>`
  per `video` texture prop; the native shell had no equivalent: video
  texture props rendered black forever in the native preview.
- **The design** — `video_sources::VideoSources`:
  - scans the live store for texture props of type `video` with an
    `options.path` (`from_prop` accepts the legacy `"location": "videos/video"`
    variant too);
  - one GStreamer file source per unique *absolute* path, keyed
    `video:<absolute path>` — the id the compositor already matches against a
    prop's path; two props sharing a clip share one pipeline;
  - reference-counted by `(module_id, prop_name)`; stops when the last
    reference disappears (Scene change, module removal, prop flip, profile
    switch, shutdown) — transition ghosts keep both sources alive for the
    length of the crossfade;
  - reconcile at 10 Hz — cheap; nothing restarts unless the prop changed;
  - a missing file logs once, falls back to canvas, retries on video-folder
    changes (the media watcher notifies);
  - `loop`/`paused`/`playbackrate` read from the prop; applied on start and
    live via transport commands (never a pipeline restart for those).
- **Surface area** — `MediaHub` gains transport read/control methods;
  `media/status` carries a `transport` snapshot; paused-by-transport
  reports `"paused"` through `VideoSourceState::Paused => "paused"` — health
  polling never restarts it; `Cargo.toml` adds `gstreamer` as a
  dev-dependency (the test matrix needs real pipelines); `native.rs` wires
  the reconcile at ~10 Hz, profile switches and shutdown release every
  pipeline.
- **Tests** — 20 in `video_sources.rs` (fixture + fake hub): reconciler
  start/stop/dedup/reference counting, prop flip semantics, transition-
  ghosts, missing-file once-log + retry, option changes live.

### Findings

- **[P3] `RECONCILE_INTERVAL = 100 ms`** — every drag of a scrubber fires a
  scan of the prop tree; acceptable for the tiny tree involved (single-digit
  video props per install); noted.
- **[P3] Absolute-path source id is fine on Linux** — a Windows install
  would need a prefix-invariant decision between `C:`/`c:`; forked-verbatim
  pathlib; noted.
- No P2. The video texture pathway is now a known, tested shape.

## Round 182 — `1416ee38..1b1f1af7` (feat: video transport controls in the UI)

**The user-facing half is in: the Inspector and the Media tab both get
transport controls.**

- **Inspector texture row** — the `video` texture control shows clip
  name/path, the Play/Pause button (writes both prop option AND queues the
  transport command), Restart (transport only — never prop), Speed
  0.1–10 scrubber (clamped; writes option + queues), Loop checkbox,
  position/duration labels; the whole row mirrors into prop `data` via
  `texture_prop_action` semantics (exactly the value the legacy writes).
- **Media tab Sources list** — same transport row for file sources (not
  cameras), same commands — parity.
- **Error surface** — a missing clip shows the shell's problem line instead
  of a dead transport row; `MediaSourceView::video_problems` exposed through
  the snapshot — honestly absent from the panel when the source is running.
- **Kittest coverage** — 11 tests (`video_transport.rs`): per-control payload
  assignments, command equality vs option round-trip, restart-only, loop
  both channels, clamped speed writes, missing-file branch, Media-tab parity,
  paused-source-shows-Play for a finished clip.
- **Snapshot shape** — `MediaSourceView` gains `transport` keyed by source
  id (path suffix match, boundary-safe); `video_problems` is a separate
  flattened vec — clean split.

### Findings

- **[P3] Position/duration labels are static snapshot text** — the panel
  polls at 1 Hz (or on any prop change); not animated; noted.
- **[P3] The `.fs` clip name truncates at 52 chars** — fine.
- No P2.

## Round 183 — `1b1f1af7..0d7719c5` (fix: label paused engine media sources)

**Determinism fix in the legacy renderer: paused is now labelled.**

- `MEDIA_SOURCE_STATE_LABELS.paused = "Paused"` in `engine-media.js`
  (one line) + one assertion in the normalisation test. The engine reports
  transport-paused sources as `paused` (Round 181/182 counterpart), and the
  Audio/Video panel now labels them instead of falling through to
  `Unknown`.

### Findings

- None. A one-line fix matching the new engine semantics exactly.

## Round 184 — `0d7719c5..e6ae9c0a` (feat: add one-shot MIDI learn)

**The engine-side of MIDI Learn lands, matching `MIDI.vue` byte-for-byte.**

- **`MidiLearn`** — one arm at a time (a second Learn replaces the first, as
  the JS resolves only one callback); the next message that writes
  `channelData` resolves once and disarms; a clock tick never resolves it
  (the learning callback lives in the `listenForInput` branch); a learned
  CC learns type 144 when latch rewrites it (same `data[0]` rewrite the
  Electron handler does).
- **`MidiLearnArmView`/`MidiLearnResultView`** — exported through the
  snapshot; `arm`/`last` round-trip; id counter bumps monotonically.
- **`midi/learn`** replaces any previous arm; **`midi/cancelLearn`** clears
  the arm without writing; **`resolve`** writes the legacy payload shape:
  `{ type: "midi/WRITE_DATA", location: "midi.devices['<id-name-manufacturer>'].channelData['<channel>']['<type>']", match: { type: "midi/WRITE_DATA", payload: { id, channel, type } } }`.
- **input_links** — borrowed-key path walker test handles spaces/dashes
  correctly (`dev-1-My Pad-Acme`), ids stay verbatim.
- **Store arms** — the `midi/learn` arm dispatches evidently produce the
  right delta ordering; `midi/cancelLearn` writes nothing.

### Findings

- **[P3] No explicit `input-links/createLink` dispatch here** — the UI
  announces `sync_midi_learn` which queues the legacy payload in r185;
  engine arm only produces `midi/learn` feedback.
- No P2.

## Round 185 — `e6ae9c0a..33ea2fb6` (feat: Learn and Forget MIDI input links)

**The last gap in the Inspector/Input Link editor closes.**

- **The section** — one-shot `Learn` button (available on any link type,
  MIDI-native hint text when MIDI waits); while armed, "Waiting for MIDI…"
  shows with Cancel; if the arm was moved to another input by the engine,
  the editor says so and disarms; on resolve, the exact legacy payload is
  queued and the draft clears; a `Forget` button clears an existing learned
  link without touching the draft semantics; learned MIDI devices show as
  `Last learned: <name> channel <ch> type <type>`; the typed path field is
  the manual route stays unbroken alongside.
- **Draft semantics** — `MidiLearnDraft { floor, … }`; `start_midi_learn`
  captures `floor = last_learn.id`; `sync_midi_learn` only applies a result
  *newer* than the floor (an old `last_learn` never re-arms a stale apply);
  when the arm is dropped elsewhere it disarms the draft; the "learned
  link over audio" case **releases its smoothing-id** — regression-pinned
  by `learning_over_an_audio_link_releases_its_smoothing_id`.
- **Kittest (16+ cases)** — payload equality with the legacy shape, arm-
  replace behavior, resolve ordering semantics (floor-resolved-last-write-
  wins matches the JS), audio-link smoothing release, no-MIDI warning,
  Cancel-does-nothing-until-user-picks, the row-level Forget behaviour,
  multi-Learn-replace handling, and the MIDI-section-widget logic.

### Findings

- **[P3] The Learn row is `midi/learn` driven only** — no synthetic
  Learn-via-palette command; fine.
- **[P3] `last_learn` accumulates into the snapshot forever** — one record
  high-water; fine.
- No P2.

## Round 186 — `33ea2fb6..737672ed` (feat: open the current Library folder in the file manager)

**A small, tight affordance with a closed path resolver.**

- **The Library header** — an `Open folder` button beside Rescan queues
  `UiCommand::OpenLibraryFolder { folder, relative }` with the active kind
  and breadcrumb.
- **Folder allow-list** — only `image`/`video`/`palette` are addressable;
  anything else opens the media root.
- **`resolve_library_folder`** — rejects anything absolute (`path.is_absolute()`
  or leading `/`), `..` segments, drive-letter-looking `:` segments, and
  NUL bytes; trims and tolerates `.`/empty segments; preserves the
  kind-root's path canonicality otherwise.
- **Fallback hierarchy** — missing/invalid relative → kind root →
  profile → media root; the button never silently does nothing.
- **Kittest** pins the command at root and after a subfolder navigation
  (`relative = "artist"`); the app tests pin the resolver rejections
  (`../secret`, `/etc`, `C:/Windows`, `a:b`) and the accept cases.
- The `library_panel.png` golden is re-baselined.

### Findings

- **[P3] A legitimate `:` in a POSIX filename can't be opened** — a subfolder
  with a colon fires the reject; harmless; noted.
- **[P3] TOCTOU between the `is_dir` filter and `open_folder`** — a folder
  could vanish in the gap; the file manager handles a missing path fine; noted.
- No P2.

## Round 187 — `737672ed..8281bcc5` (fix: ignore filesystem access events in the media watcher)

**The Library's watch→rescan→re-watch feedback loop closes.**

- **The defect** — reading a video/image for a Library thumbnail touches its
  access time on filesystems that record atime (ext4 by default, some tmpfs
  shapes); inotify reports it as `Access(Read)` — the media watcher took every
  event kind, so the rescan read the very file it was watching, which touched
  atime again, ad infinitum. Symptom: a steady `media folders changed: video`
  + rescan cycle on any host whose Library stayed warm.
- **The fix** — `event_kind_relevant(kind)` returns false only for
  `EventKind::Access(_)`; everything else (Create/Modify/Remove/RenameAnd
  everything the read handlers understand) flows; the Access-kind test pins
  `Access(Read)` and `Access(Close(Read))` inert, `Create`/`Modify`/`Remove`
  active.
- **The rescan still coalesces a burst** (500 ms quiet) — one
  `rescan_modules()` per burst maximum, which the loop fix needs.

### Findings

- **[P3] The filter is coarse — `Access(Close(Write))` is dropped too** —
  GStreamer's file saves in this tree don't go through plain file writes that
  surface only as `Access(Close(Write))` (they use Create/Rename or Modify/Data),
  and the legacy watcher had the same coarseness; acceptable; noted.
- No P2. This one was a real loop — the Library tab on ext4 would have been
  quietly scanning forever.

## Round 188 — `8281bcc5..979a5bd1` (fix: persist the engine snapshot on exit and periodically)

**A stale-state bug — boot restore read two-day-old files.**

- **The defect** — the native shell resolved `<userData>/engine-state.json`
  for *reading* (restore + Scenes) but `save_snapshot` only wrote when
  `--state` was explicitly passed. Normal launches therefore never persisted
  the live Groups/Modules, and every boot restored whatever the file held
  from whenever the file last happened to be written.
- **The fix** — `state_path` is kept from `native_state_path()`
  (supervisor constant spelled `<userData>/engine-state.json`); a 30-second
  quiet save fires in the frame loop for crash resilience; manual
  `SaveSnapshot` command and both quit paths save it announced; failures log
  once per shell until they recover.
- **Regression test** — seeds the default state path (no `--state`), saves,
  reloads, asserts the created Group is restored. Tests the actual bug
  rather than the plumbing.

### Findings

- **[P3] The periodic save every 30 s is a full snapshot write to disk** —
  at ~2-3 KB it's trivial; at a fat modular state (600 modules) it could
  cost ~1 MB per 30 s on a busy host; acceptable; noted.
- **[P3] No explicit `before-quit` ordering check vs layout save** —
  independent paths; fine.
- No P2. A silent data-reconstruction bug is closed.

## Round 189 — `979a5bd1..9832c9e8` (feat: write a daily native log from the launchers)

**The native shell finally writes to the same log the Electron supervisor does.**

- **`packaging/native/log-run.sh`** — a POSIX-sh preamble re-execs the body
  under bash when found; without bash, the child runs unlogged with a
  stderr warning. Under bash: resolves `MODV_USER_DATA` (or
  `$XDG_CONFIG_HOME/VmodV` / `$HOME/.config/VmodV` — matches
  `window_state::user_data_dir()`), creates `<userData>/logs`, appends to
  `vmodv-YYYY-MM-DD.log` with an ISO-timestamp + pid header
  (`%Y-%m-%dT%H:%M:%S%z`), tees the child's stderr with `tee -a` through a
  coprocess (today's stderr on the terminal stays intact), preserves the
  child's exit code, prunes older dated logs (7-day horizon + at most 6
  older + today = 7 total, matching `MAX_LOG_FILES`).
- **Launchers** — the AppImage's `AppRun` now `exec`s `log-run.sh` with the
  binary; the `.deb`'s launcher identical (mount-aware, prefix-resolving);
  both fail soft: without `log-run.sh` they run `modv` directly;
  `tools/native-appimage.sh` and `tools/native-deb.sh` install the wrapper;
  `tools/native-deb-check.sh` asserts presence, root-own/mode, and that
  `--help`/`--version`/bad-flag runs all produce a conforming log; it also
  proves stdout is never teed (the `grep -q 'ModV.*version' $LOG && fail`
  check) and the AppImage dispatch gets checked through a staged AppDir.
- **`tools/native-log-run-check.sh`** — behavioural suite with a stub
  `modv`: MODV_USER_DATA wins, child exit code round-trips, stale-past
  pruned, today's kept, closed-stderr survives, XDG fallback, no-bash
  degrade, AppRun dispatch staged AppDir, install wiring — all pass per the
  commit message (not re-run here).
- **stdout is deliberately untouched** — `--bench`/`--headless` output stays
  machine-readable; only stderr flows to the daily log (as stderr does in the
  Electron's own `modV` — identical semantics).

### Findings

- **[P3] `log-run.sh` needs `bash` at runtime for the stderr tee** — bash is
  ubiquitous on Linux; if missing the wrapper runs unlogged (documented
  fallback); noted for anyone reading the POSIX preamble.
- **[P3] `coproc` uses the `LOG_TEE` array** — depth of bash usage; correct;
  noted.
- **[P3] `native-deb-check.sh`'s `ROOT` env default undefinement** —
  `ROOT="${ROOT:-/app}"` — correct for Docker mounts; noted.
- No P2. The last logging asymmetry between the two shells is closed.

## Round 190 — `9832c9e8..8b9c40ed` (build: version the native release as 2.0.0)

**Version metadata consistency, no lying elsewhere.**

- `NATIVE_VERSION ?= 0.1.0` → `2.0.0`; the three packaging scripts get the
  same fallback (`APP_VERSION:-NATIVE_VERSION:-2.0.0`); the comment
  clarifies the crates keep their own metadata and the Electron installer
  version remains `APP_VERSION`.
- The Makefile's `NATIVE_VERSION ?=` stays CI-overridable (the nightly
  will append `-nightly.<date>`).

### Findings

- **[P3] The crates still say 0.1.0** — deliberate; crates' versions stay at
  0.1.0 for this cycle; packaging metadata is the user-facing artifact's
  label; documented in the commit.
- No P2.

## Round 191 — `8b9c40ed..613cfe40` (build: make the native shell the default dev target)

**Bold, reversible, correctly wired change of default.**

- `make dev` now runs the native egui shell; the legacy Electron UI stays
  reachable via `make dev-electron` and `MODV_UI=electron make dev`;
  `MODV_UI=native` remains an explicit alias; `ENGINE_MODE=1` (Rust engine
  inside Electron) still flips to Electron unless `MODV_UI=native` is given.
- The README/Makefile commentry updates along with the behaviour; `build`/
  `appimage` stay Electron as the `.deb`/AppImage fallback build-time
  default.

### Findings

- **[P3] The CI `make dev` instructions in AGENTS.md are now stale** — they
  describe `MODV_UI=native` as opt-in while the default is now native; a
  one-line doc touch-up is due in the author's queue (but AGENTS.md is the
  author's file, not for reviewer to edit); noted.
- **[P3] No known regression surface for `make dev` becoming the Rust
  shell** — the GUI target wins on all Linux hosts alike; Docker GUI args
  forwarded; this is the intended release gate.
- No P2.

## Round 192 — `613cfe40..d8058b89` (ci: publish native AppImage and deb in the nightly)

**The release engineering closes.**

- **New `native` job** in `nightly.yml` runs after the Electron `build`
  publishes (needs: build), on `main`, freeing runner disk first,
  `MIN_FREE_GB=8`, with `ref: main` (matches the nightly); it computes
  `NATIVE_VERSION=${BASE}-nightly.$DATE` from the Makefile, builds the Rust
  toolchain once (GHA cache reuse), builds `make appimage-native` +
  `make native-deb`, inspects them, and uploads to the same rolling
  `nightly` release with `--clobber` — exactly additive, Electron stays
  the fallback.
- **`native.yml`** — docs header updated to say the nightly publishes to
  the rolling release alongside the Electron build; per-change artifacts
  remain its job.
- **Truthful framing** — the commit message says the Electron job still
  publishes first as the fallback; if native fails, the nightly run
  fails visibly; no silent half-publishes.

### Findings

- **[P3] The native job doesn't run the test suite** — it trusts the
  per-change CI for greenness; consistent with the shape; noted.
- **[P3] `dpkg-deb -I` is informational, not asserting tool versions** — fine.
- No P2.

## Round 193 — `d8058b89..3dc44678` (docs: the native 2.0 release note)

**The release preparation note is honest, complete and self-aware.**

- **`docs/native-2.0-release.md`** — a thorough Linux-only-timeliest 2.0 prep
  doc: artifacts/version policy (independent `NATIVE_VERSION`/`APP_VERSION`),
  userData compatibility (everything shared, native-only files additive and
  Electron-ignored in both directions), ISF/Plugins notes (corpus bundled,
  hot-reload works, `grab-canvas` NOT ported, `modV.ws` NOT wired), the
  `make dev` default with the Electron escape hatch, the nightly publish
  flow, and an explicit "Residuals before editing this note into a release
  announcement" section: Electron deletion waits for sign-off; Flatpak
  unbuilt; README/AGENTS.md deliberately unchanged; both will be updated
  at sign-off.
- **`rust-full-rewrite-progress.md`** — the rewrite tracker refreshes to
  reflect reality: native is the default dev path; P7/P8/P9 rows updated
  (P9 now "prep prepared"; `nightly.yml` publishes next to Electron);
  "P1 residuals: structured daily logger, splash and native file dialogs"
  — the launcher logger (`9832c9e8`) is listed correctly as landed, with
  splash + native dialogs remaining); the constraints-honoured line now
  says "legacy JS UI remains selectable via MODV_UI=electron".
- The note flags everything honestly and names what's still missing — it is
  the author's own honest self-assessment, not an announcement.

### Findings

- **[P3] "P1 residuals: structured daily logger, splash and native file
  dialogs" still lists the logger as residual though Round 189 landed it** —
  the doc line (properly refreshed) should arguably say only splash +
  native file dialogs remain; the author will fix at the next refresh;
  noted.
- **[P3] "Flatpak is not built" repeated in two places** — by design; noted.
- No P2.

## Round 194 — `3dc44678..aa93356d` (test: guard module-registry parity)

**The 432-module contract becomes a committed manifest + a corpus-bound
test + a namespace-coherence guard.**

- **Committed manifest** — `tests/fixtures/module-parity.json` ($comment,
  counts, twoD, point10, shader, isf, 61 default scenes): 432 = 15+27+36+354
  with the legacy Gallery's expected name set, generated from the legacy
  worker's `import.meta.glob` surface (2d/modules, shader/modules, ISF
  `*.fs` stem).
- **JS guard test** (`tests/unit/module-parity.test.js`) — recomputes the
  glob exactly, compares counts and name sets, fails on drift; `UPDATE_MODULE_PARITY=1`
  regenerates; `DEFAULT_SCENES_PATH` integrity asserted (all scene module
  names resolve, 61 scenes, 62 distinct name tokens).
- **Rust guard tests** — `module_registry_matches_the_legacy_manifest`
  compares the built registry against the same manifest (name sets for twoD,
  Point10, shader, ISF);
  `every_default_scene_module_resolves_in_the_registry` checks the seeded
  default Scenes against the registry; the cosmetic "known name mismatch"
  doc note retires since the names were fixed to match the Gallery.

### Findings

- **[P3] `legacy_module_manifest()` panics if the corpus is absent** —
  fine for a guarded test; documented in the failure message; noted.
- No P2.

## Round 195 — `aa93356d..821bef8a` (test: the packaged-app smoke suite)

**`tools/native-smoke.sh` is the native e2e from the package itself.**

- **What it covers** — headless `--frames N --bench` (exit 0, one JSON
  object, sane fps/p99/RSS), windowed `--frames 20 --apply-scene`, a state
  round-trip (first run writes engine-state.json, second restores the
  Group/Module counts), the log-run.sh launch header + stdout sanity,
  SIGKILL-before-the-periodic-save recovery, and a bounded soak
  (`--soak-seconds`, fps + peak RSS reported).
- **Build-free** — when no `--appimage` is provided it assembles the same
  AppDir the `appimage-native` target produces around
  `crates/target/release/modv`; never builds anything; always uses a
  throwaway `MODV_USER_DATA`; never touches `~/.config/VmodV`; per-command
  timeout.
- **Crash recovery** — SIGKILL at 4 s into a run, restart, observe the
  restore; "starting empty" on a fresh tree is accepted; the 30 s
  periodic save is a documented upper bound.
- **The soak is deliberately bounded** — 60 s by default; no 2 h soak on
  a developer desktop; the real 2 h check stays on the release checklist.

### Findings

- **[P3] `tools/native-smoke.sh` is not wired into CI** — a deliberate
  boundary for now; ask: wire into the nightly or sign-off checklist.
- No P2.

## Round 196 — `821bef8a..36d0bab2` (docs: record the 2.0 hardware waivers)

**Honest waiver ledger — the smoke run IS landscape evidence.**

- **Evidence table added** — native 2.0.0 AppImage actual-run verified
  numbers: 600 frames, 5956 fps, p50 0.16 ms, p99 0.38 ms, 99.4 MB RSS,
  windowed 20 frames at 3498 frames @ 58.30 fps, Wayland/X11 RADV exits,
  state round-trip, SIGKILL recovery, log header, module parity guard.
- **Everything hardware-specific honestly waived here** — NDI (no host
  libndi), multi-GPU matrix (RADV only), clean-VM installer integration,
  audio content, camera enumeration, 2 h soak — each with a reason and
  the waiver scope.
- **The parity map now distinguishes coverage classes** — packaged smoke
  C, quit C/P, GL-context stayed waived, engineSkeleton P.
- **"Was: 62 scenes; now: 61"** — the Strobe Riot retirement lands in the
  count line.

### Findings

- **[P3] The evidence table mixes host-observed numbers (Great) with the
  waiver list (correct for a developer box)** — exactly the right
  shape; ask: adopt the same format for the eventual sign-off run.
- No P2.

## Round 197 — `36d0bab2..65881cd8` (fix: keep smoke profiles out of the real userData)

**Smoke-testing must never touch a real user tree — enforced now, not by
convention.**

- **The defect class** — the smoke script passed its throwaway `MODV_USER_DATA`
  through each sub-command by hand; any phase that forgot it would silently
  use the real `~/.config/VmodV` — exactly the thing a smoke test must never
  do.
- **The fix** — the script *defaults* `MODV_USER_DATA` to a temp-dir path
  before any sub-command runs; each phase now has its own dedicated tree
  (headless, windowed, crash, soak); the crash-recovery phase **copies** the
  state tree instead of relying on the shared one.
- **Split coverage** — headless → its own tree, windowed → its own, crash →
  a `cp -a` of the state tree (clean restart isolation), soak → its own.
- **No behavioural change** — the checks are unchanged; only the isolation
  story improved.

### Findings

- **[P3] `MODV_SMOKE_KEEP=1` still keeps everything** — deliberate; noted.
- No P2.

## Round 198 — `65881cd8..83d7a1a2` (feat: add the BPM tempo source to Input Links)

**The BPM tempo-source feature lands — both runtimes byte-symmetric.**

- **What changed** — BPM becomes a state link (`beats.bpm`), not a meyda
  getter. A new shared helper `createBpmInputLink(inputId)` provides the
  payload: `{ inputId, type: "state", location: "beats.bpm", source: "bpm" }`;
  the Audio panel's Input Config lists BPM under audio features.
- **Critical detail** — BPM passes through *absolute*: `min`/`max` are
  deliberately omitted because link bounds remap 0..1, and a 174 tempo would
  scale through a 0.2–0.8 range into tens of thousands; a stale saved
  ranged link's min/max are also ignored when `source === "bpm"`.
- **Both runtimes gate identically** — the JS `resolveInputLinkValue` returns
  undefined for non-finite/≤0 BPM and ignores bounds; the Rust engine's
  `resolve_input_link_value_for_source` does the same before the usual
  ranged map. At startup, without a detector estimate (`beats.bpm == 0`),
  the saved value survives instead of being overwritten with `0`.
- **UI** — the Input Config Audio Feature list shows live values for meyda
  features and the detected tempo; the Min/Max sections hide for
  non-rangeable sources (BPM); the Audio Features panel exposes the BPM row.
- **e2e fix** — the engine parity spec's full-suite flake: replaced single
  `SET_BANDS` commits with `driveBands` (re-commit + poll until predicate or
  timeout), keeping the assertion robust under GPU load; the engine mirror
  test adds a BPM-feature link fixture with the expected value.

### Findings

- **[P3] BPM links select on `source === "bpm"`** — two engines with a
  different spelling would silently diverge; the JS and Rust share the
  literal ("bpm") by convention in both files (reviewers' note, not a bug).
- No P2.

## Round 199 — `83d7a1a2..eb00dd3b` (feat: add `--start-performance`)

**Unattended-run flag, wired correctly.**

- **`--start-performance`** — a new `clap` flag on the native shell that
  dispatches `scenes/startPerformance` after the startup Scene/seed; the
  2 h smoke and the planned native e2e suite can now drive performance mode
  without a click.
- **Unit test** pins the flag's effect (true after dispatch; false before).
- `--bench-seconds` remains the natural companion (windowed deadline).

### Findings

- **[P3] `start_cli_performance` errors go to stderr, never fail** — a
  malformed start logs `WARN could not start performance mode`; correct
  behaviour for a CLI flag (don't brick the boot); noted.
- No P2.

## Round 200 — `eb00dd3b..2ddc89b4` (feat: the test control socket)

**A genuinely strong artefact — the native shell's first fully isolated control surface.**

- **Scope** — `modv --socket PATH` starts an opt-in Unix socket (alias
  `--control-socket`) speaking JSON-lines; deliberately *not* part of the
  production path (unset = nothing new happens); the Electron supervisor's
  binary-framed `modv-ipc` stays the production channel.
- **Protocol** — `ping`/`pong`, `version`, `state`, `dispatch` (action+payload
  verbatim), `screenshot` (compositor frame → PNG), `quit`, and a `save`
  added later. Every request gets exactly one correlated response with the
  exact `id` echoed (numbers/strings).
- **Safety** — the socket file is `0o600`; stale sockets replaced (their
  node can linger), live sockets refused (`AddrInUse`), regular files
  refused (never deleted); the backlog is capped (64 MB); `try_recv` is
  non-blocking per frame; quit flushes with a bounded wait; the socket
  file is removed on drop.
- **Threading** — listener on a dedicated thread, responses correlated, the
  winit loop drains once per frame — no locking hazards on the render
  thread.
- **Tests** cover stale/live/regular-path, request round-trip, and PNG
  round-trip.

### Findings

- **[P3] No `ok=true` evidence for the `state` payload size** — cap at 64 MB
  (documented); fine.
- **[P3] The socket lives in the user's temp scope only when absolute paths are passed** — safe; note.
- No P2.

## Round 201 — `2ddc89b4..8d04f165` (test: the native e2e process harness)

**The most important infrastructure move of this window.**

- The Playwright suite cannot run against the native GPU/X11/GPU-sharing
  reality; instead the author built a Rust-native harness: `NativeHarness`
  spawns the real `modv` binary with a throwaway userData; `--socket`
  serves the JSON-lines protocol; a virtual X server (Xvfb) plus lavapipe
  Vulkan provide the display and GPU when the host lacks one; `mod.rs` +
  `e2e_harness.rs` gate on `--test-threads=1`.
- **Mocking nothing** — real binary, real store, real compositor, fare (the
  proof is in the tests written on top of it).
- **Docker gets xvfb** — Dockerfile gains `xvfb xauth libx11-6 libxcursor1
  libxrandr2 libxi6 libxinerama1 libxcb-render0 libxcb-shape0 libxcb-xfixes0
  libxcb-xkb1`; the Rust image now runs the harness — but note the display
  selection: unset `DISPLAY`+`WAYLAND_DISPLAY` → **Xvfb first**; a Wayland
  fallback (`sway --headless`) exists in the harness but is only a second
  choice, so CI exercises the X11 path while this host runs Wayland+RADV —
  the two "the same tests" lanes diverge on platform realism.
- **CI** — native.yml adds a serial step `modv-app --test e2e_harness`;
  modv-app splits unit/lib tests (already serial) from the harness step.
- **`docs/native-e2e.md`** records the protocol, the harness rules, and
  the waiver semantics (Playwright stays as the legacy gate).

### Findings

- **[P3] CI headless is X11 while the target platform is Wayland** —
  the harness ranks `Xvfb` above `sway --headless` (Docker installs the
  former; the Rust image has no sway), so the "covered" bucket rides on X11
  while the shipping shell is Wayland-first under XDG. Suggested next batch:
  prefer `sway --headless` over Xvfb (reorder the two arms in
  `tests/e2e/mod.rs::TestDisplay::ensure`), install `sway`+`libwayland-*`
  in the rust image, keep xvfb as the last fallback. Leads to me: the app
  prefers wayland and winit's wayland-sys is natively exercised there.
- **[P3] Xvfb runs with default resolution** — 640×360 is the harness default; fine.
- **[P3] Lavapipe-only GPU** — the `.deb`'s Vulkan path is not CI-covered
  here; native suites run on the same lavapipe; noted.
- No P2. The harness architecture is right; the fallback order needs the
  Wayland-first tweak.

## Round 202 — `8d04f165..a2403611` (docs: document the native e2e harness)

**Correct, complete, and unusually precise.**

- `docs/native-e2e.md` explains framing, correlation, the opt-in posture,
  display/CI requirements, the harness rules (no mocks, intent parity,
  explicit waivers), and how porting specs decide bucket: process-lifecycle,
  state, store dispatch, pixel.
- **`quit` semantics** — quit goes through the real File → Quit path with a
  bounded flush; SIGKILL is a separate shape (e2e_process covers it).
- **The harness timeouts are failure bounds, not pacing** — sleeps aren't
  used; specs poll `wait_ready`/state; correct.
- **The log wrapper asserts** — stdout unchanged, stderr to the log — the
  e2e harness enforces the native-log contract while testing it.

### Findings

- **[P3] "the supervisor has no process test seam"** — acknowledged in the
  porting notes; fine.
- No P2.

## Round 203 — `a2403611..9ec29d1b` (test: port the audio tempo and system-audio coverage)

**A faithful port of the JS rhythm-detection test matrix.**

- **`make_onset_signal_with`** — the JS `makeOnsetSignal` options struct:
  jitter (per-block deviation), noise (per-frame white noise), ghosts
  (extra pulses at offsets with strength); Gaussian-like 0.06s support /
  0.025 s width — the same shape the JS test used, reviewed to match.
- **Three tempo modules** — the JS spec asserted detections against a 120
  BPM signal with ghosts; the Rust port asserts the same; the JS `tempo-
  detector.test.js` matrix is now native coverage with the same fixtures.
- **The system-audio coverage** — `AudioSelectionView` gets the contract to
  pin: PipeWire monitoring, real capture rate, silent failure keeps the
  previous feed (with an error, not silence).

### Findings

- **[P3] Noise seed `create_random(seed)`** — exact LCG replication used by
  the existing suite, reviewed once more; matches the JS lightly-mixed seed
  sequence; noted.
- No P2.

## Round 204 — `9ec29d1b..f8242f02` (fix: apply Scene documents opened like presets)

**A real parity bug exposed by porting — fixed before the port was visible downstream.**

- **The defect** — `scenes/loadSceneFromJson` defaulted `apply` to `false`,
  while the JS action defaults it to `true`; opening a Scene JSON upserted the
  document without applying it. Silent no-op for the "File → Open Scene" path.
- **The fix** — `apply` defaults to `true`; the seed/import callers pass
  `false` explicitly; direct store callers do the same; unit test pins both
  arms (apply-default applies *and* an explicit `apply:false` stays load-only).
- **Crash-safe** — existing callers that don't pass `apply` now correctly
  get apply; the Profile-media import gains `"apply": false`; the seeding
  tests all add `"apply": false"` explicitly — no accidental apply on migration.
- **Byte-for-byte JS parity** — `scenes.js:2048-2074` defaults apply to
  true; verified here too.

### Findings

- **[P3] Upsert vs apply** remain two distinct effects with overlapping inputs;
  documented in the doc line; fine.
- No P2. A silent UX break understood and fixed inside the porting window.

## Round 205 — `f8242f02..89759887` (test: port ISF categories and Point10 transparency)

**Coverage for the awkward rendering paths.**

- **`crates/modv-app/src/compositor.rs` gains** `render_frames`/canvas
  helper functions for transparency test assertions (backdrop fully
  retained when the compositor blends/reads, asserts more than X% transparent).
- **ISF category guard** — `tunnel_isf_modules_are_registered_as_3d_scenes`
  and `nc_sa_scene_isf_modules_are_registered_as_3d_scenes` verify the
  corpus-driven ISF registry registers the documented Sparkle/Hot Shower/Helix
  Eye/Traced Tunnel/etc. as 3d-scenes (matching the JS e2e IsfCategories spec).
- **Point10 transparency** — `chromatic_line_keeps_the_background_transparent`,
  `chromatic_wave_keeps_the_background_transparent`,
  `wormhole_keeps_the_background_transparent`, `magic magenta survives`
  flow through the real compositor with a 96×96 frame format, asserting
  transparent fractions > 20%, 20%, 5% respectively.
- **The fixture flavour** — lavapipe/profile or a real GPU; tests skip
  gracefully when no adapter.

### Findings

- **[P3] `canvas` fig-size 96 match** — same as the e2e; noted.
- No P2.

## Round 206 — `89759887..671dfc6c` (test: port the default-Scene content audits)

**The default-Scene invariants are now native asserts rather than e2e-only.**

- **`e2e_defaults.rs`** — the JS `defaultScenes.spec.js` logic without a
  browser: drives the seeded store with `DEFAULT_SCENES` (both lanes),
  and asserts (a) no Mesmer modules in Effects rows anywhere,
  (b) every default Scene's ISF filter effects are drawn from the registered
  3d-scenes set, (c) version-3 seeding is intact (the v3 batch keeps every
  module), (d) bass/BPM auto-links name controls as the JS does, (e) legacy
  hydration treats `defaultsSeeded` alone as a valid seed.
- **`corpus_dir()`** — env!("CARGO_MANIFEST_DIR")-relative, skip-if-absent,
  so the test never demands the whole repo.
- Run widget: `make engine-test ARGS="-p modv-app --test e2e_defaults"`.

### Findings

- **[P3] `get_default_scene_names_up_to`** stays internally versioned; fine.
- No P2.

## Round 207 — `671dfc6c..3563f3af` (feat: seed a default Group on a fresh shell)

**The native shell now matches the Electron boot semantics.**

- **`New Group` seeding** — the native e2e harness exposed that the shell
  started with zero Groups; Electron's renderer calls
  `groups/createGroup(...)` at boot (`application/index.js:446-456`). The
  native shell dispatches the same action once and only on a truly fresh
  start: `restored_snapshot == false` **and** no startup preset/Scene/
  playlist **and** no applied Scene. Restored state and documents win.
- **`window-state.json` now overrides CLI `--width`/`--height`** — the
  running window size is the truth reported through the e2e `runtime.window`
  query; CLI sizes are the first estimate only.
- **Tests** cover fresh/relaunch paths and the restore-preference ordering.

### Findings

- **[P3] The default Group name may drift** if a future default deviates
  from Electron's name; noted.
- No P2.

## Round 208 — `3563f3af..2847cbb1` (test: port the process-level native scenarios)

**The process lifecycle is now under actual test.**

- **`e2e_process.rs`** mirrors four Electron process e2e groups: engine
  skeleton (frames + snapshot + SIGKILL + relaunch — the native stand-in for
  the Electron supervisor under the new cut: the Rust supervisor has no process
  test seam so the harness does it), main window (readiness, frame count,
  640×360 screenshot), default Group (fresh start == exactly one New Group,
  no duplicate after relaunch), window-state round-trip (saved bounds beat
  CLI sizes), and Scene-like preset open (listed and applied day with
  Groups/Modules live).
- **Harness gains** — `kill_and_wait`, explicit userData reuse, temp_root,
  save command on the control socket (`"save"` → `Command::Save`).
- **`docs/native-e2e.md`** documents all of the above.

### Findings

- **[P3] `kill_and_wait` sends one SIGTERM to the SIGKILL leader** — safe;
  noted.
- No P2.

## Round 209 — `2847cbb1..988ed03e` (test: port the Scene snapshot and performance-grid coverage)

**The biggest port: the whole Scene grid/performance surface.**

- **`e2e_scenes.rs`** — 1300 ln porting the JS `performanceGrid.spec.js`,
  `sceneSnapshotsAndPerformance.spec.js`, and `gallerylessPerformance.spec.js`
  through the same store actions the worker used; the compositor is driven
  with `RowFadeInfo` records and real frame rendering at 64×64 (lavapipe);
  tests skip gracefully when no adapter is present.
- **Five themes** — row-pass crossfade (applySceneRow), Gallery-less
  performance mode, legacy shader/mode-switch group swaps, row transitions
  with a ghost canvas, preset-shape serialisation and round-trip.
- **Fixture flavour** — assert on actual `FrameInfo` + `RowFadeInfo` structs
  and compositor reads rather than on pixel mocks; skip-if-no-adapter.

### Findings

- **[P3] 64×64 lavapipe coverage only** — no GPU-path parity here; noted.
- No P2.

## Round 210 — `988ed03e..6596a55c` (docs: point the supervisor coverage note at the Electron tests)

One line: the doc refines its supervisor-coverage note to say the
supervised-engine restart budget stays with the Electron unit tests
(`engine-supervisor.test.js`, `engine-restart-budget.test.js`); the native
harness ports the intent without touching them.

### Findings

- **[P3] The supervisor is not researched here** — no action; noted.
- No P2.

## Round 211 — review follow-up (CI headless fallback order)

**A real platform-parity catch, recorded as its own round because the
commit window has moved.**

- **Finding** — the native e2e harness `TestDisplay::ensure` ranks
  `Xvfb` **above** `sway --headless`. The Dockerfile installs xvfb but not
  sway, so CI cannot reach Wayland headless. This host runs
  `WAYLAND_DISPLAY=wayland-0` and gets external (Wayland) coverage; CI gets
  X11+lavapipe — a differently-shaped winit path for a Wayland-first
  shell.
- **Ask** — prefer `sway --headless` over Xvfb, install `sway` +
  `libwayland-*` in the rust image; keep xvfb last as a fallback.
- **File class** — a one-line swap in `tests/e2e/mod.rs` + two `apt-get`
  lines in the Dockerfile. One commit. No risk surface.
- The review doc's outstanding list carries this item (12) and notes it
  dates from Round 201.

## Round 212 — `6596a55c..491c9214` (test: port the Groups pane coverage)

** ported coverage with real drag-reorder semantics.**

- **`e2e_groups.rs`** — the legacy Playwright `groups/*.spec.js` ported
  against the real native shell: clears the compile-time contract that
  modulo dispatches (`groups/UPDATE_GROUP_BY_KEY`) with the same payload
  shape the engine validates; alpha/blend/name/inherit/clearing/pipeline/
  reorder/remove/create — every control pinned exactly once per the JS
  contract (toggle both directions, drop both orders).
- **Native affordance mismatch documented honestly** — `inheritFrom`,
  tri-state "2", Backspace delete are named in the module header as not
  covered, because the native pane has no such control.
- **`apply(snapshot, action)`** mutates the snapshot the same way the engine
  would, so toggles round-trip assert real state, not stale reads.

### Findings

- **[P3] The `drag-reorder` port uses the native Move up/down instead of
  a drag gesture** — different input, same outcome; documented.
- No P2.

## Round 213 — `491c9214..a9901d41` (test: port the Scenes grid editor coverage)

**Grid coverage port — well-scoped, honest about native affordances.**

- **The port** (`e2e_scenes_grid.rs`) — hover preview + still, add empty
  Scene, clear cell (with live modules removed), live-capture back to cell,
  column rename, empty-cell create — every grid affordance in intent parity.
- **Native Path** — no in-cell editor; the "capture live row" button stands
  in for the JS in-cell edit path; documented in the test header.

### Findings

- **No P3/P2.**

## Round 214 — `a9901d41..5683e82e` (test: port the focused-input coverage)

**Input Config focused-input parity.**

- `inputConfigUpdatesWhenControlIsFocused.spec.js` ported byte-faithfully:
  clicking a prop's link row focuses that input id (`in-amount` vs
  `in-speed` pinned); the panel title follows; native affordance
  mismatch documented (each prop row *is* the focused-input editor).
- **Dual payload check** — two focused-input cases prove the editor sets
  `input_id` correctly on open and follows a different click.

### Findings

- **No P3/P2.**

## Round 215 — `5683e82e..3dbafe2a` (test: name the ported JS specs)

Docs-of-tests: every `#[test]` line notes which legacy spec it ports; the
JS/Group and grid headers each state their JS lineage; no behavioural
change.

## Round 216 — `3dbafe2a..f61c4d55` (test: drive every composite operation)

**Blend test parity.**

- `blend_mode_can_be_set` now iterates all 29 `COMPOSITE_OPERATIONS`; the
  popup scrolls the option into view; asserts each dispatches exactly once.
- The scrollable popup list shape was noticed by the previous run — now
  handled like a real user would.

### Findings

- **[P3] Any 30th operation needs a list-length bump** — the harness drags
  a scroll; noted.
- No P2.

## Round 217 — `f61c4d55..69cb78da` (feat: implement the MIDI state load action)

**Store-carried MIDI state lands — the data the restore path needs.**

- **`midi/SET_STATE` + `midi/loadPresetData` alias** — merges every top-level
  key over `state.midi` (the JS loop semantics); replaced `learning`, kept
  it absent without touching it; unknown keys survive; non-object payload
  refused with `InvalidPayload` (loud failure, not silent).
- **Tests** pin the merge behaviour, unknown-key survival,
  payload-vs-contract delta emission, refusal on non-objects.

### Findings

- **[P3] `midi/loadPresetData` is the restore-path entry** — correct; noted.
- No P2.

## Round 218 — `69cb78da..bbe8e882` (feat: restore Scene MIDI and focused input on apply)

**Scene apply restores everything an Electron Scene saves.**

- **`restore_scene_midi`** — a whole-Scene `apply` backfills the saved
  `midi` block through the new `midi/SET_STATE`; missing/non-object is
  ignored loudly (warn-once); scoped/row applies returned early — the
  JS scoped early-return.
- **`restore_scene_focused_input`** — the focused-input tail of the JS
  restore: the saved `focusedInput` is remapped through the same old→new
  input map the links/tweens use; when the Scene no longer creates that
  input the focus is cleared; the legacy `layers` shape is left alone;
  scoped applies row-restores nothing.
- **Split helpers** — `set_focused_input_values`/`clear_focused_input_values`
  factor the mutation out of the public `inputs/*` actions.
- **Tests** (e2e_scenes.rs `apply_scene_restores_the_saved_midi_state_and_focused_input`)
  pin both the restored MIDI channels and the focus remap, and the scoped
  row leaves both alone.

### Findings

- **[P3] `restoreSceneMidi` runs even when the scene has no `midi`** — `None` guards; fine.
- No P2.

## Round 219 — `bbe8e882..a78996e0` (fix: hydrate the stored performance flag like the Electron store)

**A real engine-state bug: performance state hydration was diverging from the JS.**

- **`enabled` now survives hydration** — a legacy `autopilot.enabled: true`
  really resumes performance mode after a restart, exactly like
  `sceneSnapshotsAndPerformance`'s autopilot spec. `currentSceneId` remains
  runtime-only (a live value wins); `updatePerformance` now also carries
  `enabled` into the runtime (the JS tick reads `state.performance.enabled`)
  so the two stores agree.
- **`loadScenesFromDisk` keeps the live runtime keys** — no silent
  start/stop of performance mode during a reload.
- **Tests** updated to pin the JS-specified resumed behaviour — legacy
  autopilot comes back on; the stored per-launch `currentSceneId` does not.
- **Parity** — the JS behaviour is now the engine's rule: `enabled` is
  state, not a runtime sentinel.

### Findings

- **[P3] The prior commentary "runtime state never survives" was wrong** —
  correctly rewritten with a JS-anchored rationale.
- No P2.

## Round 220 — the 2-hour native soak (review evidence; not a commit; nothing re-run)

**The last hardware-waiver-class item is closed by a real two-hour run.**

- **Where the evidence lives** — `/tmp/opencode/soak-default/` (throwaway
  userData): `monitor.log`, `rss.csv` (240 samples, 30 s cadence),
  `logs/vmodv-2026-11-18.log`, `engine-state.json`, `scenes.json`. Launched
  from the OpenCode session "Full Rust rewrite completion" (~04:24–06:47);
  backgrounded monitor pinned to the real `modv` child, not the wrapper.
- **The run** — packaged 2.0.0 AppImage binary (`dist/VmodV-Native-2.0.0-x86_64.AppImage`,
  rebuilt 04:00; sha256 `c4295ad5…`; executed from the extracted tree
  `/tmp/appimage_extracted_a451bd9eaf7cadb43b0808dcc793e642/usr/bin/modv`):
  `--apply-scene "Mycelium Bloom" --start-performance --bench-seconds 7200`.
  Startup lines from the app log: 354 ISF modules registered from the
  AppImage (0 failed), `432 registered`, 61 default Scenes, Scene applied
  (3 groups), `performance mode started from the command line:
  {"enabled":true,"rows":3}`, **audio source pipewire owns capture at
  48000 Hz** (the desktop instance was stopped for the soak; the audio was
  the operator's `mpv --no-video --volume=70 https://radio.noiseblock.live/stream`),
  output window 1920×1080, `output fullscreen on`.
- **Exit** — `monitor.log`: `start=2026-11-18T04:47:00+00:00`,
  `modv disappeared at 2026-11-18T06:47:02+00:00` (the 7200 s deadline) [checked here].
  Final log lines, in order:
  `native: frames=431816 fps=59.97 resolution=1920x1080 output=open`
  (431816 / 7200 = 59.97 fps [checked here]), `ui: INFO state snapshot
  saved to …/engine-state.json`, `ui: INFO output window closed`.
  No SIGSEGV/SIGKILL; the exit path is the graceful bench deadline.
- **RSS** — 235,436 kB → 284,468 kB (min 235,436 / max 284,468; HWM
  293,416 kB ≈ 286 MB; +47.9 MB over 1.99 h). The shape is warm-up only;
  per-15-min averages recomputed from the raw csv [checked here]:
  261.2 / 269.8 / 274.5 / 275.6 / 275.6 / 276.8 / 277.8 / 277.8 MB —
  flat ~277.8 MB for the **last 75 minutes**.
- **VRAM** — device-wide (includes the desktop compositor and other GPU
  clients; not per-process): first 3.59 GB → last 4.54 GB, min 3.43 GB,
  max 5.07 GB, mean 4.13 GB [checked here against the csv]. Author-reported
  bands through the second hour: 4.39 / 4.49 / 4.39 / 4.40 GB; my per-band
  recomputation: 4.04 / 4.07 / 4.18 / 4.08 / 4.09 GB. Both readings agree
  on the shape (warm-up to ~4.0–4.1 GB then flat); the author's GB figures
  sit ~0.3 GB high against the raw bytes — the verdict does not depend on
  which band is used.
- **What this is not** — the 04:36 sway-headless `soak-2h` attempt (pid
  2749645; killed at 04:45:51, status=143, ~9.5 min; monitor initially read
  the launcher wrapper at 1.2 MB RSS; PipeWire unavailable in that headless
  session — JACK/ALSA errors) was **not** the 2 h soak and is not evidence
  for it. Only the soak-default run above is.
- **Waiver retired** — this closes the "2 h soak — waived" line in
  `docs/native-parity-map.md` (tracked, unmodified; the author owns that
  file — see outstanding item 24). The 60 s `--soak-seconds` remains the
  `tools/native-smoke.sh` default; nothing about this run was re-executed
  or reproduced here — the numbers above are the author's, cross-checked
  arithmetically from the files the author left in /tmp.

### Findings

- **[P3] VRAM is device-wide, not per-process** — the flat-through-hour-two
  claim rests on a stable device total, which also includes the desktop;
  a genuine per-process leak below the compositor's noise floor would not
  be visible here. Noted, not disputed — RSS is per-process and is the
  stronger signal.
- **[P3] The artifacts are volatile** — `/tmp` content does not survive
  reboots; the only durable record is this file. Noted; the numbers are
  recorded verbatim above for that reason.
- No P2. The gate this closes is the one that mattered: two hours of the
  packaged shell under performance mode, real GPU, real PipeWire — no
  crash, no renderer loss, no monotonic growth.

## Round 221 — `a78996e0..18de3aa3` (feat: tri-state Group enabled, inheritFrom, Backspace, startup focus)

**The Groups pane closes its three documented gaps in one commit.**

- `GroupView` now carries the raw engine `enabled` (`GROUP_ENABLED = 1`,
  `GROUP_DRAW_TO_OUTPUT = 2`) instead of a collapsed bool; `is_drawn()` /
  `draws_to_output()` / `is_gallery()` helpers; `GALLERY_GROUP_NAME`
  constant mirrors the engine's internal Gallery Group.
- **`InheritTarget::Previous` (`-1`) / `InheritTarget::Group(uuid)`** —
  `value()` serialises the numeric `-1` or the UUID string; the selector
  lists Previous Group plus every other user Group by name (Gallery
  excluded); `groups/UPDATE_GROUP_BY_KEY` accepts both forms.
- **The Enabled control is a real tri-state checkbox** with the legacy
  `Checkbox.vue` `allow-partial` semantics — verified here against the JS
  source: alt-click always selects 2; a plain click is `Number(!n)` (so
  2 → 0, and `Checkbox.vue`'s `modelValue === 2` branch is dead code in the
  legacy too — the port is faithful, not "fixed"); `2 → partial dash`
  indicator; hover text matches the gesture.
- **Backspace removes the focused Group**, gated like `Group.vue`'s
  `@keydown.self`: ignored while a text field owns the keyboard and while
  any dialog/editor/search/about/BMP/consent surface is open; no
  confirmation dialog, matching the legacy binding.
- **`ensure_default_group_focus`** focuses the first *user* Group exactly
  once (Gallery skipped; retries while the snapshot is empty; never
  re-arms after the user clears the selection) — pins
  `defaultGroupIsFocusedByDefault.spec.js`.
- **`focused_input_title`** resolves `<owner>: <control>` from the
  snapshot (module props incl. component sub-inputs; Group alpha/blend via
  the Group name) — the legacy Input Config window title returns
  (`Ball: Amount`, `Background: Alpha`); unowned inputs keep the bare
  control label.

### Findings

- **[P3] The tri-state "2" end-to-end GPU semantics were not re-verified
  frame-by-frame** — the engine treats 2 as drawn-to-intermediate-only;
  the port asserts the read model and the dispatch, not a rendered frame
  diff. Noted.
- **[P3] Startup focus never settles while only the Gallery Group
  exists** — by design (empty-snapshot retry); noted.
- **[P3] Backspace deletes without confirmation** — parity with the legacy
  binding; a native confirm would deviate from the spec. Noted.
- No P2.

## Round 222 — `18de3aa3..afa85986` (feat: close the Groups/Scenes UI parity gaps)

**Stills, confirmation, and the `Scene N` default — the remaining grid gaps.**

- **Hover stills** — a hovered non-empty cell keeps painting its cached
  preview after the pointer leaves, per cell (the legacy grid's
  `scenes__gridPreviewCanvas--visible`, one still per rendered cell);
  `still_cells` inserted on hover, `prune_preview_stills` drops stills
  whose Scene/cell disappeared or emptied, `paints_preview` merges
  hovered + stills.
- **Clear cell confirmation** — the context-menu item no longer writes
  directly; it opens a modal (Scene name, row label resolved from
  `scene_rows` with `Row N` fallback, monospace module list, Clear/Cancel);
  the spec line for `performanceGrid.spec.js` "after confirmation" drives
  the test. Cancel leaves the saved cell untouched.
- **`+ Column` / `Add empty Scene` send `{}`** — the engine mints the
  `Scene N` default like the legacy `addEmptyScene`; the kittest fixture
  mints `Scene {n}` identically and asserts the header renders with the
  minted name.
- **`<owner>: <control>` titles land in the focused-input window** —
  `input_link.rs` passes `focused_input_title` into
  `LinkEditor::for_input`; tests assert `Input link: Ball: Amount` /
  `Ball: Speed`; the old "module prefix is not available" waiver note is
  gone.
- The cell-editor intents are restated as an explicit **waiver** in the
  test module header with the native replacements named (create cell
  Group → live edit → `captureSceneRowToScene` / `clearSceneCell`, both
  now asserted).

### Findings

- **[P3] Stills were keyed by cell address in this commit** — an edit of
  the saved cell kept painting the stale frame until the very next commit
  re-keyed by content (Round 224). The intermediate state is one commit
  wide; noted for the ledger.
- **[P3] The Clear-cell window closes on `.open(false)` via the X, and on
  Cancel; no Escape shortcut** — egui windows close on Escape by default
  only if configured; not configured here. Noted.
- No P2.

## Round 223 — `afa85986..760c3a57` (test: complete the UI port)

**The ported Groups coverage is now full and the startup-focus test runs.**

- `#[ignore]` removed from the startup-focus test; `build()` runs the
  harness twice so accesskit rects settle before kittest clicks.
- **Enabled round-trip 1→0→1→2→0→1** through the real control, including
  the alt-click partial state; asserts dispatched payloads, the snapshot
  read model, `is_drawn`/`draws_to_output`, and the indicator string.
- **Both `inheritFrom` forms** are exercised: UUID (`group-fx`) and the
  numeric `-1`; the selector hides while inheritance is off and restores
  on re-enable; the closed selector shows the engine default.
- **Backspace** removes the focused Group; the same test pins the
  text-field gate (Backspace in the name field edits text instead of
  removing the Group); the per-Group Remove button remains the pointer
  path.
- The fixture snapshot applies raw `enabled` (i64) and `inheritFrom`
  (number-vs-string) itself.

### Findings

- **[P3] `inherit_combo` finds the combo by vertical proximity to the
  `Inherit from` label** — layout-coupled; fine for a test, noted.
- **[P3] The ignore-while-dialog Backspace matrix is a single shell-level
  disjunction** — no per-dialog regression pin; the gate is one `if`.
  Tested only via the text-field case. Noted.
- No P2.

## Round 224 — `760c3a57..6bb12221` (fix: drop a hover still when the saved cell changes)

**One bug, one fix, one test — the correct shape.**

- `still_cells` re-keyed `BTreeSet<CellRef>` → `BTreeMap<CellRef,
  Vec<String>>`, storing the module names the preview showed;
  `keep_preview` snapshots them; `prune_preview_stills` now also requires
  `saved.module_names == *module_names`; `draw_scene_cell` passes the
  current cell's names.
- The e2e grid test asserts: edit row 2's saved modules → its still stops
  painting while row 1's (unchanged) still survives; then the existing
  clear/empty prune assertions still hold.

### Findings

- **[P3] Module-name comparison is order-sensitive** — re-ordering the
  same modules drops the still (re-warms on next hover); cosmetic, noted.
- No P2. Closes the Round 222 P3 (the one-commit-wide stale-frame window).

## Round 225 — `6bb12221..6c74947e` (test: finalize the Rust e2e parity manifest)

**The e2e port closes with a ledger, not an impression.**

- `tests/e2e-rust/parity.json` — **50 spec files, 147 executed tests:
  137 covered + 10 waived.** `tests/e2e-rust/README.md` documents the
  counting rules: 49 Playwright `*.spec.js` + the root `main.spec.js`
  placeholder (ten commented-out `test(` calls), 133 `test(...)`
  declarations = 129 literal + 4 parameterised templates
  (tempo 5, Chromatic 2, Imported Scenes 9, Rolling Fire/Smoke Room 2
  = 18). **Independently re-derived here against the live tree: 49
  `*.spec.js` [checked], 133 `^\s*test\(` declarations [checked], the
  four template loops present with the exact names the README lists
  [checked], 129 + 18 = 147 [checked], 137 + 10 = 147 [checked].**
- **Two guards pin the ledger from both lanes** —
  `tests/unit/e2e-parity-manifest.test.js` (JS: walks the live tree,
  re-derives the totals, refuses duplicates, empty notes, `port-*`
  statuses) and `crates/modv-app/tests/e2e_manifest.rs` (Rust:
  `include_str!`s the manifest, asserts `EXPECTED_* = 147/137/10` and
  that every `covered` reference resolves to a `fn <name>(` in the named
  file). The Rust guard survives the Electron suite's deletion.
- **The 10 waivers** each cite the Electron/Chromium dependency and, where
  one exists, the native test covering the surviving intent — the two
  `performanceGrid` cell-editor waivers are correctly scoped to the
  in-cell dialog intent and point at the native replacements that
  `18de3aa3`/`afa85986`/`760c3a57` then landed.
- **Semantic-delta notes** are recorded in the manifest (fresh Group ids
  on apply; native single-process kill-survival vs a supervised engine
  child; Scene-open applies by default) — the right level of honesty.

### Findings

- **[P3] `assert_reference_exists` is a substring check** — `fn <name>(`
  present in the file proves existence, not intent; the review passes in
  this document are the other half of that gate. Noted.
- **[P3] The JS guard dies with the Electron tree** — by design; the Rust
  guard is the durable one (its own comment says so). Consistent.
- **[P3] Neither guard is CI-enforced here** — same posture as the rest of
  the native wave; noted.
- No P2.

## Round 226 — `6c74947e..4a2a4164` (refactor: move the ISF corpus and Point10 textures to `resources/`)

**A mechanically clean relocation — with one consequence nobody can miss
now that the last commit is in.**

- **Moved** — `src/renderer/src/application/renderers/isf/modules/isf/`
  → `resources/isf/` (354 `.fs`, 36 `.vs`, 4 CRT mask PNGs; 394 files) and
  `src/renderer/src/application/point10/textures/` →
  `resources/point10/textures/` (25 files). Pure renames [checked: 0-line
  diffs on every renamed file].
- **Every reference that needed to move moved** — the Rust corpus call
  sites (`cli.rs` gains `DEV_ISF_DIR` + a `resources/isf/Bloom.fs` dev-tree
  assertion; `native.rs` drops the Electron-tree dev candidates;
  `scene_ops.rs`/`store_ops.rs`/`compositor.rs`/`module_registry.rs`/
  `e2e_defaults.rs` corpus paths), the CI path filters in both workflows,
  `THIRD_PARTY_NOTICES.md` (every moved-file path rewritten; the one
  JS-lane survivor, `GreatBallOfFire.js`, correctly keeps its `src/` path),
  `tools/native-appimage.sh`, `native-deb.sh`, `native-smoke.sh`,
  `native-deb-check.sh`, the docs (`isf-corpus-report.md`, overrides
  README), the JS guards (`module-parity`, `third-party-notices`,
  `isf-module-category`, `point10EffectsAreRegistered.spec.js`), and the
  Rust parity docs.
- **Verification here** — the counts match the commit message exactly
  (354 + 36 + 4 = 394; 25 textures) [checked]; the moved files are
  byte-identical renames [checked via `git show --stat`].

### Findings

- **[P2] The documented Electron escape hatch is no longer buildable at
  this commit — while `docs/native-2.0-release.md` still advertises it, and
  the nightly job that builds it will fail.** By measurement:
  - 10 JS scene modules under `src/renderer/src/application/point10/`
    import `../textures/*.jpg|png|svg?url` (e.g. `GlassLens.js` →
    `pcb.jpg`) [checked] — those files no longer exist, so the Electron
    renderer bundle fails to resolve them at build time.
  - `src/renderer/src/application/worker/index.worker.js`
    `import.meta.glob("../renderers/isf/modules/isf/*.fs")` [checked] now
    matches nothing — the Electron app would register **zero** ISF modules.
  - `electron-builder.yml:13` still packs
    `from: src/renderer/src/application/renderers/isf/modules/isf`
    [checked] — the `artifacts` Docker target (and therefore
    `nightly.yml`'s `build` job and `make build`) cannot produce the
    "Electron fallback" the release note lists as a shipped artifact.
  - `docs/native-2.0-release.md` still lists the Electron fallback
    AppImage/deb, `make dev-electron`, `ENGINE_MODE=1 make dev`, and says
    "A native failure leaves the Electron nightly intact" and
    "Electron tree deletion ... waits for sign-off" [checked] — all now
    false. The sign-off has not happened (deletion has not happened), so
    the tree was required to stay honest until then.
  - Verified-by-reading, not re-run: the failure mode follows from
    rolldown/vite resolution and electron-builder's missing-`from` error;
    nothing was built to confirm.
- **Disposition options** — (a) repoint the JS lane at `resources/`
  (restore the bridge until sign-off), (b) update the release note +
  nightly to stop advertising Electron (state it plainly: the fallback is
  gone pending the signed-off deletion), or (c) proceed with the
  user-signed-off Electron deletion now. Any one is honest; the current
  state is not.
- No P3-level nits beyond the docs: the commit is otherwise exemplary —
  notices, CI filters and scripts all moved together.

## Round 227 — `4a2a4164..f226729b` (test: port the remaining native guards)

**The right order: guards land before the JS tree dies.**

- **`crates/modv-app/tests/notices.rs`** (617 ln) — the Rust replacement
  for `third-party-notices.test.js`: parses `crates/Cargo.lock`, requires
  every registry package to have a NOTICE row with a plausible SPDX
  expression and an upstream link on a known forge, checks the
  copyleft/attribution bullets exist, scans `crates/` +
  `resources/{isf,point10}` for NC-SA markers and requires each file named
  in `THIRD_PARTY_NOTICES.md` with its `CREDIT` author, pins the specific
  attribution corrections (st_Ms2SD1, spherical-shader-tut, Perlin
  Transition, Helix Eye, PCB texture, Around Plus, …) and the README
  licence badge, and asserts the notices file no longer references the
  deleted Electron tree.
- **`crates/modv-app/tests/removed_content.rs`** (197 ln) — folds
  `point10-removed-effects`, `cube-removed` and `point10-disco-ball-removal`
  into one guard: removed implementation files absent (including the Rust
  Geometric Tunnel port and its SSAO pass), registry/default-Scene
  absence, no source token reintroduced; Old Movie stays pinned by its own
  test.
- `THIRD_PARTY_NOTICES.md` rows switched to Rust-only paths (the paired
  JS paths dropped; attributions unchanged).
- Verified-by-reading only (no cargo here); the count of what this
  protects: 570 crates, every NC-SA source, the notice's integrity.

### Findings

- No P2/P3 of note. The one risk — a guard that silently passes vacuously —
  is addressed by the file-count and sample-inclusion assertions
  (`sources.length >= 20`, named-file checks), matching the JS guard's
  posture.

## Round 228 — `f226729b..eaac0a68` (chore: remove the Electron application tree)

**The deletion itself. 564 files changed; 113,053 lines removed; 36 added.**

- **Gone** — `src/` (worker, renderer, main, preload), `tests/e2e`
  (Playwright) and `tests/unit` (Vitest), `public/`, `patches/`, Node
  configs (`package.json`, `yarn.lock`, `vitest.config.mjs`,
  `playwright.config.js`, `eslint.config.js`, `electron.vite.config.mjs`,
  `electron-builder.yml`, `notarize.js`), the output-window/entry HTML
  pages.
- **Kept** — `crates/`, `resources/`, `tests/e2e-rust/`, `tests/fixtures/module-parity.json`,
  `docs/`, `tools/`, `packaging/native/`, `CHANGES.md`, `THIRD_PARTY_NOTICES.md`.
- **`tests/fixtures/engine/deltas.json` + `snapshot.json` deleted** — both
  were consumed only by the deleted `engine-mirror.test.js`; the Rust
  `--emit-fixtures` path regenerates equivalents on demand into a temp dir
  (self-contained test), so no live Rust coverage depended on them.
  `docs/rust-full-rewrite.md:824` still says the fixtures are "Kept" —
  stale, see item 25.
- **The parity manifest survives** — 147-test ledger intact (the JS guard
  dies with the suite; the Rust `e2e_manifest.rs` guard is the durable one,
  exactly as designed).
- **The rollback decision is recorded in the tree** — branch
  `legacy-electron` (tag `v1.x-final`) points at `6c74947e`, the last
  commit before the `resources/` relocation. Verified here: both refs
  resolve to the same commit [checked], and that commit still holds the
  Electron tree, so the archive is buildable; the ISF corpus existed under
  `src/` there, so `legacy-electron` is a fully working 1.x fallback.
- The JS↔Rust mirror parity fixtures being gone means the *byte-level*
  delta-conformance check is now a one-way street: nothing replays JS
  store deltas into the Rust store any more (there is no JS store). The
  semantic parity guarantee rests on the pinned fixture discipline already
  recorded through Rounds 204–225.

### Findings

- **[P3] `docs/rust-full-rewrite.md:824` still claims the engine fixtures
  are kept** — stale after this commit; fold into the doc updates in flight.
- No P2 — the deletion is what the branch exists for, and the archival
  strategy (a buildable legacy branch + tag) is the correct one.

## Round 229 — `eaac0a68..4e0a7b14` (build: make the Docker toolchain Rust-only)

**The build system follows the tree.**

- **Dockerfile** — `debian:trixie-slim` base; no Node/Yarn/Electron stages;
  the release stages are `rust` (shared toolchain: GStreamer, PipeWire,
  Vulkan/lavapipe, X11+Wayland headers, Xvfb, Mesa, native build deps),
  `native-package`, `appimage`, `deb`, `artifacts`.
- **Makefile** — native-only: `build` = `clean + appimage-native +
  native-deb`; `make dev` = native release (`MODV_UI` is gone);
  `test-e2e-native` runs e2e_harness/e2e_process/e2e_scenes/e2e_defaults/
  e2e_manifest serially under the X11 path; `lint` → `engine-lint`; dead
  targets removed (`install`, `yarn`, `dev-electron`, `test-unit`,
  `docker-node-modules`, …); `docker-package` via the Dockerfile
  `artifacts` target.
- `.gitignore`/`.dockerignore` pruned to native build outputs.
- **CI environment note in the comment still holds:** X11-forwarded e2e is
  deliberate ("so an existing Wayland session does not change which Vulkan
  surface the tests exercise") — the Wayland headless preference remains
  item 12 and is now the only display-related question left.

### Findings

- No P2. **[P3]** The toolchain image still ships `xvfb` and no `sway` —
  item 12 stands; with the Electron path gone, CI's X11-first e2e harness
  is the only remaining divergence from the Wayland-first shipping target.

## Round 230 — `4e0a7b14..e25128c6` (ci: switch the nightly to native only)

**CI now publishes the native nightly and nothing else.**

- **nightly.yml** — header comment states the Electron tree is gone and is
  archived on `legacy-electron` (tag `v1.x-final`) [verified: branch and
  tag both resolve to `6c74947e`, the pre-relocation commit]. New `test`
  job: clippy + rustfmt, workspace tests (modv-app excluded), modv-app
  unit tests serial, `make test-e2e-native`; `package` job (`needs:
  test`): `make build`, publish `nightly` prerelease with
  `VmodV-Native-*` artifacts only. The Electron job is deleted, not
  paused — the doc's "a native failure leaves the Electron nightly
  intact" is now meaningfully impossible by construction.
- **native.yml** — header comment updated to native-only; path filters
  widened to `resources/**` + `tests/**`; e2e step runs `make
  test-e2e-native`; packaging runs `make build`.
- **ISSUE_TEMPLATE/PULL_REQUEST_TEMPLATE** point at the native commands
  (no `make lint`/`yarn` references survive — checked); `browserslist.yml`
  deleted (Node-specific).
- `nightly.yml` header count: `MIN_FREE_GB: 8` lowered explicitly for
  stock runners.

### Findings

- **[P3] The committed top-level docs still describe the Electron-first
  workflow at this commit** — committed `AGENTS.md` documents `make yarn`,
  `make install`, the yarn/Docker rule and Electron entry points as the
  day-to-day workflow; committed `README.md` describes the Electron build;
  `docs/native-2.0-release.md:23-24, 78-92` advertises the "Electron
  fallback AppImage/deb", `make dev-electron`, `ENGINE_MODE=1 make dev`,
  and says the Electron tree "waits for sign-off" — all three reference
  commands/artifacts that no longer exist at `e25128c6` (the Makefile
  dropped `ENGINE_MODE`, `yarn` and `dev-electron`; the tree is deleted;
  the nightly no longer builds Electron). The author's working tree
  carries the doc updates in flight (`M AGENTS.md`, `M README.md`,
  `M docs/native-2.0-release.md`, `M docs/native-parity-map.md`,
  `M docs/native-e2e.md`, `M docs/rust-full-rewrite-progress.md`,
  `M tests/e2e-rust/README.md`, new `docs/legacy-electron.md`) — recorded
  here so the committed gap is visible until those land.
- **[P3] `tools/tween-keyframe-reference.mjs`** survives the deletion with
  a header pointing at the removed toolchain (`modv-toolchain:local`,
  `make yarn ARGS="..."`) — dead weight in a Node-free tree; delete or
  archive with a corrected header.
- No P2. The Round 226 finding is resolved by this wave: the user
  confirmed removal was the plan, `eaac0a68` executed it, and the release
  docs that advertised the fallback are being rewritten in the author's
  in-flight files — the one outstanding piece is committing those docs.

## Round 231 — `e25128c6..b1ac24d5` (test: keep the ported guards clippy-clean)

Mechanical: collapsible_if collapse + rustfmt in `notices.rs`; no
behaviour change; the guards stay green in CI. No findings.

## Round 232 — `b1ac24d5..8b51280b` (docs: describe the Rust-only tree and legacy archive)

**The docs catch up to the tree — the release story is honest again.**

- **README.md** rewritten as the native app guide: Rust-only crate map
  (engine/render/media/ipc/ui/app), no-Node/Electron statement, Linux
  release target with the host Vulkan/PipeWire/ALSA/GStreamer runtime
  dependencies, Docker/make prerequisites, `make build` / `make dev` /
  `make test-e2e-native` / `make engine-lint`, and a pointer to
  `docs/legacy-electron.md` for the archive.
- **`docs/native-2.0-release.md`** now describes the Rust-only tree:
  no "Electron fallback AppImage/deb" row, no `make dev-electron`, no
  `ENGINE_MODE=1` option, Windows/macOS explicitly unsupported-native with
  the archived tree as the only reference; nightly is native-only by
  construction.
- **`docs/legacy-electron.md`** (new) — how to retrieve the archive
  (`git show legacy-electron:<path>`, `git worktree add`), plus what stays
  current in this tree.
- **`docs/native-parity-map.md`** — legacy spec rows demoted to
  archive-only with the branch/tag reference; a "Post-cleanup verification"
  section lists the native coverage layers (workspace tests,
  `make test-e2e-native`, `make engine-lint`, deb-check/smoke).
- **`docs/rust-full-rewrite-progress.md`** — records the deletion, the two
  ported guards, the archived refs and author-reported verification:
  modv-app lib **668 passed** serial, **28 e2e tests**, clean lint, both
  installers built, deb-check pass (354 ISF files, 25 textures), smoke pass.
  (All author-reported; never re-measured here.)
- **`docs/native-e2e.md` / `tests/e2e-rust/README.md`** — "the Electron
  suite lives on branch `legacy-electron`", and the JS guard pin is now
  explicitly archive-only; the Rust guard is the durable one.
- The Round 226 finding is fully closed: the committed release docs no
  longer advertise a fallback that cannot exist.

### Findings

- **[P3] `legacy-electron.md` lists `commit 4a2a4164` as "HEAD before the
  deletion commits" without warning that its Electron lane is unbuildable**
  — at that commit the ISF corpus had moved (`resources/isf`) and the
  10 Point10 JS `?url` imports fail resolution (Round 226). The
  **`legacy-electron` branch / `v1.x-final` tag at `6c74947e` is the only
  known-good Electron reference** — the doc's table should either say so
  or drop the `4a2a4164` row; anyone following the row gets a broken build.
- **[P3] The Rust-only verification numbers are author-reported and not
  re-measured** (668 lib tests, 28 e2e tests, installers, deb-check,
  smoke) — recorded verbatim, as is policy.
- **[P3] The parity map's stale "2 h soak — waived" line survives this
  rewrite** — `docs/native-parity-map.md:137-138` still claims "long
  soaks are not appropriate on a developer desktop", which the 2026-11-18
  run directly contradicts (item 24).

## Round 233 — `8b51280b..13e38c13` (chore: drop the last Electron-era paths and Node tooling)

**The tree is now fully free of the JS/Node toolchain.**

- **NDI library resolution** — `grandiose`'s vendored
  `libndi.so.5` candidates are gone; lookup is now `$MODV_NDI_LIB` →
  `/usr/lib/libndi.so.5` → `/usr/local/lib/libndi.so.5` → `libndi.so.5`
  next to the executable → `ndi/libndi.so.5` next to the executable.
  Packaged AppImage/.deb do not ship libndi; fresh machines degrade to
  "NDI unavailable" with one log line (documented behaviour).
- **`tools/tween-keyframe-reference.mjs` deleted** — the last Node script;
  its `.json` fixture keeps its anime.js 3.2.2 provenance and its generator
  metadata no longer points at the removed script.
- **`tools/`** now contains only the native packaging/smoke/log-check
  shell scripts (+ `rave-words.txt`, user-owned).
- Sweep at this commit: no `grandiose`/`node_modules`/`yarn` references
  remain in packaging, CI, Makefile, Dockerfile or tools [verified by
  reading]; remaining mentions are historical port-lineage comments in
  Rust doc headers.

### Findings

- **[P3] NDI now has no bundled distribution path at all** — a fresh
  install of the native `.deb`/AppImage reports NDI unavailable unless the
  user installs the NDI SDK or sets `$MODV_NDI_LIB`. That is the designed
  degradation, and the doc comment now states it plainly; called out for
  the release-note's "first run" guidance.
- No P2. This commit closes the toolchain-hygiene tail of the removal.

## Round 234 — `13e38c13..041fb488` (docs: pin the legacy archive refs)

`legacy-electron` and `v1.x-final` are now pinned to `6c74947e` in
`docs/legacy-electron.md`, and `4a2a4164` is labelled as this branch's
pre-deletion HEAD (with the asset relocation). The unbuildable-Electron
caveat for that row is still implicit ("includes the final asset
relocation") rather than spelled out — the buildable archive is the branch
/tag at `6c74947e`; item 25 already carries the doc-caveat ask. No new
findings.

## Round 235 — `041fb488..eca253f1` (ci: run the workspace suite serially)

`--test-threads=1` now applies to the workspace pass in `native.yml` and
`nightly.yml` — the kittest and GPU tests share one lavapipe device and
virtual display; a `tab_placement` failure observed in parallel passes
alone. Matches the modv-app serial posture. A real flake caught and pinned
by config. No findings beyond item 12 (X11 still preferred-first in CI).

## Round 236 — `eca253f1..a2fa2463` (chore: drop Electron-only icon/entitlement assets)

`resources/icon.png` is the only icon the native installers use; the
macOS entitlements plist and Windows/macOS `.ico`/`@2x.png` variants are
removed with Electron. Correct tidying of the packaging assets. No findings.

## Round 237 — `a2fa2463..d49524ad` (feat(isf): regenerate rave.exe from the updated word list)

**The Node toolchain re-enters `tools/`, but deliberately quarantined.**

- `tools/rave-reencode.mjs` + `tools/rave-wordcodec.mjs` are back
  (patch the packed txtLeaf/wordPair tables from `tools/rave-words.txt`),
  run only through `make rave-reencode` — a throwaway `node:24-slim`
  container with a `--user` mapping; the app build/tests stay Node-free.
- The Makefile documents it as dev-only asset tooling (`NODE_IMAGE`).
- `resources/isf/rave.exe.fs` regenerated (378 ln change);
  `tools/rave-words.txt` updated (the user's word list).

### Findings

- **[P3] `tools/` is no longer native-shell-only** — two Node scripts are
  tracked again (third one by intent: asset regeneration). They are
  isolated from the app by the container boundary; the claim "tools/ is
  native shell only" in tests/e2e-rust/README.md and the parity docs is
  now slightly stale. Noted, not a defect — but the docs' tool counts
  need the same edit when the author next touches them.
- **[P3] The re-added wordcodec has its own tests? No** — it has none;
  it was deleted and restored as-is. The ShaderBall lineage guards
  (`rave.exe.fs` render tests) cover the output indirectly. Noted.
- No P2.

## Round 238 — `d49524ad..2e3e1d30` (style: drop the engine subtitle from the header)

Decorative `egui + wgpu + in-process engine` line removed from the
header; four snapshots re-baselined (audio_panel, library_panel, shell,
tab_float_range). Correct noise reduction; no findings.

## Round 239 — `2e3e1d30..02d41416` (style: show the Scenes summary in the top header)

The `modV Native` title is replaced by the Scenes summary (`SCENES`,
`<n> saved`, `rows: …`) so the top row carries grid context; the
duplicated Scenes-bar header is dropped; the performance toggle stays in
the Scenes bar (later moved in 244/245). Snapshot re-baselines. No findings.

## Round 240 — `02d41416..9cdde543` (feat: move the performance bar into the top header)

**A real per-frame action bug fixed en route.**

- The performance controls (run toggle, Scene order, timing unit,
  min/max/fade, Row names, +Column/+Row) move into the top header; the
  separate bar above the grid is dropped; `draw_performance_bar` exported
  for tests that render the grid in isolation.
- **The bug** — a fresh (zeroed) performance config has `min_value = 0`,
  and a `DragValue` outside its range clamps on the first frame and
  reports `changed`, so the UI queued `scenes/updatePerformance` **every
  frame** for a default config. Fixed by clamping the displayed values
  with `.max(min_floor)` (and `fade` with `.max(0.1)`) before the widget.
- `timing_input_limits` drives the range/speed; `clamp_timing_pair` still
  guards the write path.

### Findings

- No P2. **[P3]** The per-frame-action class of bug is now exercised by
  the fixture's `build()` (two harness runs) — worth keeping the clamp
  test if the panel is ever refactored. Noted.

## Round 241 — `9cdde543..dfed308d` (style: move the Scenes summary and grid controls down)

Two-row header: run toggle/Scene order/timing unit on row 1, timing
values on their own row; `SCENES <n> saved / rows:` moved directly above
the cells next to Row names / +Column / +Row; `timing_hint` drops the
BPM suffix (the status bar already shows tempo). `draw_timing_controls`
split out alongside `draw_performance_bar`; `grid_controls` renders the
summary row. Snapshot/test updates follow. Clean layering. No findings.

## Round 242 — `dfed308d..a6938032` (feat(engine): move modules between groups by index)

**A native-only engine action, correctly shaped.**

- **`groups/MOVE_MODULE`** (alias `groups/moveModule`) — `moduleId`,
  `fromGroupId`, `toGroupId`, `toIndex` (final index after source
  removal). Source list is authoritative: remove first, then insert at
  the clamped index — same-Group and cross-Group drops share one path;
  the target dedups the id on entry (a mirror-restored Group cannot keep
  two copies).
- Unknown groups/modules warn once and no-op; incomplete payloads are
  `InvalidPayload`; an already-in-place drop emits nothing; the affected
  Groups emit the existing `groups/REPLACE_GROUP_MODULES` mirror deltas,
  source first then target.
- Unit tests pin same-group both directions, cross-group insert, and the
  already-in-place no-op.

### Findings

- **[P3] Native-only action** — no JS counterpart (by design; the legacy
  tree had no such action). Noted.
- **[P3] `toIndex` is clamped silently** — out-of-range indices clamp to
  the destination length; no warning. Matches the engine's lenient array
  semantics elsewhere (`REPLACE_GROUP_MODULES` accepts any list). Noted.
- No P2.

## Round 243 — `a6938032..3401b858` (feat(native-ui): drag modules to reorder and move groups)

**The Groups pane gains first-class drag-and-drop.**

- `ModuleDrag` in `modv-ui::module_drag` (transient pointer state, kept
  out of the engine read model): `drop_action` returns `None` for an
  in-place drop (inert), converts a gap slot to the final index (downward
  same-Group drop shifts by one), returns `groups/MOVE_MODULE`; payload
  taken exactly once, keeping `GalleryDrag` and `ModuleDrag` disjoint.
- **`insertion_index`** — Upper half of a row inserts above, lower half
  below, Group-block empty space appends after its last row; hovered half
  tinted, accent gap line, three-line grip, grab/grabbing cursor;
  click-to-select unchanged.
- The per-row `^`/`v` buttons and their swap code are removed.

### Findings

- **[P3] The drag math is pinned by unit tests, not the e2e alone** —
  `same_group_drop_in_place_is_a_noop`, `same_group_move_down_lands_one_slot_earlier_than_the_gap`,
  `same_group_move_up_uses_the_gap_index`, cross-group and end-of-list
  cases all present. Strong.
- **[P3] `from_index` snapshot at drag start** — if the snapshot reorders
  mid-drag (impossible in practice; drags are single-frame UI actions), the
  shift math could mis-fire; noted, not a defect.
- No P2.

## Round 244 — `3401b858..87d7eac8` (test(e2e): cover module drag and drop)

Real-shell coverage with raw pointer events: same-Group re-order,
cross-Group insert, empty-Group append, empty-space release queues
nothing, click-to-select survives, `^`/`v` absent, `D`/`x` present. The
fixture applies the queued `MOVE_MODULE` like the engine and asserts the
re-rendered row order. The harness's `drag_at`/`hover_at` idiom and the
`PointerGone` caveat are documented in the module header.

### Findings

- No P2/P3 of substance. The fixture's `apply_move` mirrors the engine's
  remove-then-insert exactly (source first, clamp) — parity between what
  the test drives and what the engine does is the point.

## Round 245 — `87d7eac8..20a8c805` (test: re-baseline Groups module-row snapshots)

`shell.png`, `tab_docked_range.png`, `tab_float_range.png` re-baselined
for the grip + `^`/`v` removal; no other snapshot moved. Correct
snapshot hygiene. No findings.

## Round 246 — `20a8c805..81ea1c55` (fix: insert a module dropped between two rows)

**A real hit-testing bug, fixed with the exact shape it needed.**

- The few inter-row pixels were outside both row rects, so a release
  between rows hit the Group's append target — the drop landed at the
  end instead of the gap.
- **Fix** — `module_rows_span` (union of row rects) is the drop surface;
  `insertion_index` resolves the gap; the Group append target only takes
  the pointer outside that span (header or below the last row).
- Regression test: pointer between Ball and Tunnel inserts Bands at
  index 1.

### Findings

- No P2. **[P3]** Inter-row gaps are ~2–4 px; the fix is correct, and the
  test pins it. Noted for the record.

## Round 247 — `81ea1c55..22646926` (style: make the two-bar grip the module drag handle)

The grip is now the only drag source (painted two-bar handle with its
own `Drag <module> (<id>)` accessible label, `Role::Button`, grab
cursors); the name button is plain click-to-select with `click` sense;
dragging the name carries no `ModuleDrag` and queues nothing — pinned by
a dedicated test. Accessibility-tree honesty (grip labeled, hint lines
painted, not text). Snapshot re-baseline across the three affected
images.

### Findings

- No P2. **[P3]** The name being click-only slightly reduces
  discoverability (nothing invites the drag) — the grip + cursors + e2e
  tests carry that; noted.

## Outstanding items — as of `22646926`

**Closed / re-verified this cycle** (Rounds 15–247; the detailed items are all
carried elsewhere in the doc):

- Scenes rows aligned and layer gutter pinned (`e7ffecf7`) — fixed pitch
  (`ROW_PITCH`), opaque pinned gutter sharing the wheel offset, `ui.place`
  instead of `ui.put`, three egui_kittest layout tests. Round 152.
- Native media tree watch (`fa67044b`) — `MediaWatcher` replaces the
  ISF-only watcher: active-profile isf/image/video/palette + extra ISF
  folders, allowlist matches the JS read handlers, 500 ms quiet debouce in a
  pure `Debounce`, ISF bursts trigger exactly one rescan; profile switch
  rebuilds the watch. Round 153.
- **Phrase-flip Scene transitions + bar timing (`f610d08b`)** —
  `phrase_drop` one-frame pulse forwarded through `scenes/tick` to the
  byte-faithful `shouldAdvanceRow` port (deadline forces; flip advances only
  under the chance roll); `performance.timingUnit` (`seconds` default,
  `bars`, 4/4, 120 BPM fallback) with unit-aware normalisation and a
  non-destructive `scenes/setTimingUnit` converter (globals + overrides
  through seconds at live BPM); stored values keep their unit; playlists
  carry `timingUnit`; guide declared for Electron compatibility (Round 156).
- **Transparent Point10 scenes moved to the Foreground row
  (`c667b1d6`)** — Nine Scenes re-placed; two synchronised default-scenes
  copies (JS + Rust JSON) byte-compared (62/62 in order); `DEFAULTS_VERSION`
  16→17 in both runtimes; `Smoke Room` joins the refresh list; exhaustive
  parity tests (Round 160).
- **Boa plugin gating + discovery + consent (`c4fc956f`)** — persisted
  `native-plugins.json` with master-switch-behind-consent **normalised on
  read** (hand-edited `enabled` can't bypass), active-profile plugin/ +
  `<userData>/plugins` discovery, per-plugin switches keyed by path, atomic
  writes, allow-listed read-only state reader, no UI yet (Round 162).
- **Media-tree import flag doc clarified (`e1985c34`)** — the module
  header says how files added later are picked up (delete the profile's
  flag). Round 177.
- **Video-file texture sources reconciled (`1416ee38`)** — one
  GStreamer file source per unique resolved path (`video:<abs path>`, the id
  the compositor already matches), reference-counted by (module, prop),
  10 Hz reconcile, missing-file once-log + retry on change, `loop`/`paused`/
  `playbackrate` live from the prop, ghosts kept for crossfades (Round 181).
- **Video transport controls in the UI (`1b1f1af7`)** — Inspector +
  Media tab: Play/Pause, Restart, Speed 0.1–10, Loop, position/duration;
  writes the prop option and queues `MediaAction::VideoTransport`; missing
  clip shows the shell problem instead; 11 kittest cases; **closes the
  "no UI" leg left by Rounds 178/180** (Round 182).
- **One-shot MIDI Learn (`e6ae9c0a`)** — arm-at-a-time semantics, clock
  ticks never resolve, CC→type-144 latch rewrite preserved; `midi/learn` and
  `midi/cancelLearn` dispatch; `MidiLearnArmView/ResultView` round-trip; the
  legacy payload shape; borrowed-device-key path walker test. Round 184.
- **Native release versioned 2.0.0 (`8b9c40ed`)** — `NATIVE_VERSION` =
  `2.0.0` in Makefile + scripts fallback; CI-overridable; Electron version
  stays `APP_VERSION` (Round 190).
- **`make dev` runs the native egui shell (`613cfe40`)** — Electron stays as
  `make dev-electron` / `MODV_UI=electron` / `ENGINE_MODE=1`; native is
  the default sign-off pointer; `ENGINE_MODE=1` selects Electron-with-Rust;
  explicit `MODV_UI=native` alias preserved (Round 191).
- **Module-registry parity guard (`aa93356d`)** — committed
  `tests/fixtures/module-parity.json` (432 names via recompute + corpus +
  `UPDATE_MODULE_PARITY=1`); JS guard test recomputes the glob; Rust
  manifest guards built registry vs snapshot (15+27+36+354); default-scenes
  resolve-in-registry guard; JS-side `import.meta.glob` test (Round 194).
- **Groups pane e2e (`491c9214`)** — all JS group/*.spec.js flows
  ported (toggles both directions, reorder/remove/create, JS-lineage
  annotated; inheritFrom/tri-state/Backspace honestly excluded — native has
  no such controls); snapshot-mutation round-trip. Round 212.
- **Scenes grid e2e (`a9901d41`)** — hover preview+still, add/reset column,
  clear cell, live-capture back to the cell, column rename, empty-cell
  create; native edit affordances documented. Round 213.
- **Focused input e2e (`5683e82e`)** — Input Config editor targets the
  clicked prop; follows a different click. Round 214.
- **JS-spec lineage named in test docs** — every native test names its JS
  spec; commit `3dbafe2a`. Round 215.
- **Every composite blend op driven** — 29 `COMPOSITE_OPERATIONS` exercised
  in the blend test; popup scroll-to-option. Round 216.
- **`midi/SET_STATE` + `midi/loadPresetData` implemented (`69cb78da`)** —
  top-level merge semantics, unknown keys survive, non-object payloads
  refused with `InvalidPayload`; restore-path entry is the same action.
  Round 217.
- **Scene apply restores MIDI + focused input (`bbe8e882`)** —
  `restoreSceneMidi` + `restoreSceneFocusedInput` (§ whole-Scene apply);
  scoped applies early-return; focus is remapped through the input
  old→new map or cleared when the Scene no longer creates it. Round 218.
- **`enabled` survives hydration (`a78996e0`)** — a legacy `autopilot.
  enabled: true` resumes performance mode after restart; `updatePerformance`
  carries `enabled` into the runtime; `loadScenesFromDisk` keeps the live
  runtime keys. Closes the JS divergence. Round 219.
- **The Electron tree is gone (`eaac0a68`)** — 564 files, −113,053
  lines; the privacy-first archive is `legacy-electron` + tag
  `v1.x-final` (both at `6c74947e`); the parity manifest, Rust guards and
  `resources/` survive — Round 228. The Rust-only toolchain (`4e0a7b14`,
  Round 229) and native-only nightly (`e25128c6`, Round 230) complete the
  removal; `f226729b` (Round 227) ported the last two guards first.
- **The Electron/Node tree was removed and the archive documented
  (`f226729b`..`13e38c13`)** — guards ported first (`notices.rs`,
  `removed_content.rs`), 564 files deleted, Docker toolchain and Makefile
  made Rust-only, nightly switched to native-only, docs rewritten,
  `docs/legacy-electron.md` records the archive, last Node tooling gone —
  Rounds 227–233.
- **Post-release snagging wave (`041fb488`..`22646926`)** — archive refs
  pinned; CI workspace suite serial (tab_placement flake); Electron-only
  icons dropped; `rave.exe` regenerated via quarantined container Node
  (`make rave-reencode`); header/Scenes-summary/performance-bar layout
  rework with a per-frame `updatePerformance` fix; **`groups/MOVE_MODULE`**
  + real module drag-and-drop (grip handle, inter-row drop bug fixed);
  re-baselined snapshots — Rounds 234–247.
- **The ISF corpus and Point10 textures moved to `resources/`
  (`4a2a4164`)** — 354 `.fs` + 36 `.vs` + 4 PNGs and 25 textures, pure
  renames; every Rust/script/CI/notice/JS-guard reference switched —
  Round 226. The Electron lane is unbuildable as of this commit; see the
  Round 226 finding and item 25.
- **The 2-hour native soak ran 2026-11-18** — packaged AppImage,
  `--apply-scene "Mycelium Bloom" --start-performance --bench-seconds 7200`;
  431,816 frames / 7200 s = 59.97 fps at 1920×1080; clean deadline exit at
  06:47:02; RSS plateaued at ~277.8 MB for the last 75 min (HWM 286 MB);
  device VRAM flat after warm-up — Round 220. The
  `native-parity-map.md` "2 h soak — waived" line is stale (item 24).
- **Tri-state Group enabled + inheritFrom + Backspace + startup focus
  (`18de3aa3`)** — raw 0/1/2 model, `Checkbox.vue`-parity gesture, `-1`/UUID
  inheritFrom with Gallery exclusion, Backspace delete, first-user-Group
  focus — Round 221.
- **Groups/Scenes UI parity gaps closed (`afa85986`)** — hover stills,
  Clear-cell confirmation, `{}` addEmptyScene (`Scene N`),
  `<owner>: <control>` focused-input titles — Round 222.
- **UI port completed (`760c3a57`)** — Enabled 0→1→2→0→1 round-trip,
  inheritFrom both forms, Backspace with a text-field gate, startup-focus
  test un-ignored — Round 223.
- **Hover still re-keyed by saved modules (`6bb12221`)** — an edited cell
  stops painting its stale still — Round 224.
- **e2e parity manifest finalized (`6c74947e`)** — 147 tests, 137 covered +
  10 waived; two guards, independent count re-derivation against the live
  tree — Round 225.
- **`--start-performance` flag (`eb00dd3b`)** — unattended performance
  start after seed; 2 h smoke / e2e harness can drive performance mode
  without clicks; pinned unit test (Round 199).
- **Test control socket (`2ddc89b4`)** — opt-in JSON-lines Unix socket
  (`--socket`) with mode 0o600; ping/state/dispatch/screenshot/quit/save/
  version; bounded backlog; stale-socket replacement; refuses live and
  regular files; winit drains once per frame; quit flush bounded; the e2e
  harness's transport (Round 200).
- **Native e2e process harness (`8d04f165`)** — real binary, real userData
  (throwaway), virtual X + lavapipe on CI, serial CI step; **the whole
  Playwright suite has a native stand-in**; Docker/Rust image gets xvfb +
  x11 libraries (Round 201).
- **Audio e2e port (`9ec29d1b`)** — tempo-detector matrix ported into
  `modv-media::audio::tempo` with identical fixtures; system-audio state
  contract pinned on `AudioSelectionView` (Round 203).
- **Scene-apply parity (`f8242f02`)** — `loadSceneFromJson` defaults apply
  to true (JS parity); callers that only upsert pass `false`; the file-open
  path applies the Scene and sets `currentSceneId` (Round 204).
- **ISF categories + Point10 transparency ported (`89759887`)** — corpus-
  driven ISF 3d-scenes registration asserted against the named specs;
  transparent-composition GPU tests (lavapipe or GPU, skip-if-absent) (Round 205).
- **Default-Scene audits ported (`671dfc6c`)** — e2e_defaults.rs: no
  Mesmer in Effects rows, registered ISF filters used, v3 seed intact,
  bass/BPM auto-links, defaultsSeeded-only legacy hydration (Round 206).
- **Default Group seeded (`3563f3af`)** — one New Group on a truly fresh
  start (no restored snapshot, no startup doc); restored state wins; the
  window-state.json size now overrides CLI `--width/--height` (Round 207).
- **Process-level scenarios ported (`2847cbb1`)** — engine skeleton
  (frames + snapshot + SIGKILL + relaunch), main window, default Group,
  window-state round-trip, Scene-open-like-preset; `save` command on the
  control socket; docs (Round 208).
- **Scene/performance-grid ported (`988ed03e`)** — 1300-line `e2e_scenes.rs`
  covering row fades, gallery-less performance, symlink-ish repeats, the
  JS trio of scene specs; skips without a GPU adapter (Round 209).
- Native e2e docs + supervisor-note refinement (`a2403611`, `6596a55c`) —
  `docs/native-e2e.md` records the protocol/harness/rules; supervisor
  watchdog tests point at the Electron suite (Rounds 202, 210).
- **BPM tempo as an Input Link source (`83d7a1a2`)** — the BPM feature is a
  state link on `beats.bpm`, absolute (bounds ignored), skipped until the
  detector reports a non-zero estimate; both runtimes fold the same rule;
  the Input Config Audio Feature list shows it; the engine-parity spec's
  full-suite flake replaced a single-commit with re-commit-and-poll
  (`driveBands`) (Round 198).
- **Smoke profiles kept out of the real userData (`65881cd8`)** —
  the smoke script defaults `MODV_USER_DATA` to the temp tree and every
  phase gets its own dedicated tree (headless/windowed/crash-copy/soak);
  isolation is now enforced, not convention (Round 197).
- **Packaged-app smoke suite (`821bef8a`)** — `tools/native-smoke.sh`:
  headless bench, windowed apply-scene, state round-trip, launcher log,
  SIGKILL recovery, bounded soak; build-free; throwaway `MODV_USER_DATA`;
  never the real userData (Round 195).
- **Native 2.0 hardware waivers recorded (`36d0bab2`)** — the smoke run's
  evidence table + NDI/GPU/clean-VM/audio/2h-soak waivers; parity map
  tightened; Strobe Riot count now correct at 61 (Round 196).
- **Native 2.0 release preparation documented (`3dc44678`)** —
  `docs/native-2.0-release.md` (artifacts/versions/userData/ISF/plugins/
  local-commands/CI flow + explicit residuals); `rust-full-rewrite-progress`
  refreshed to reflect the native-as-default state; honest about flatpak
  absence, grab-canvas, modV.ws, and the Electron-default retention
  (Round 193).
- **Nightly publishes native installers (`d8058b89`)** — rolling nightly now
  carries native 2.0.0 AppImage + `.deb` (`2.0.0-nightly.<date>`) alongside
  the Electron release; `needs: build`; Electron still the fallback; native.yml
  now describes nightly-publishing semantics (Round 192).
- **The launchers write the shared daily log (`9832c9e8`)** — a POSIX
  wrapper (`log-run.sh`) tees the child's stderr into
  `<userData>/logs/vmodv-YYYY-MM-DD.log`, preserving the exit code and the
  live stderr; stdout stays untouched; AppRun + `.deb` launcher both
  dispatch through it; `--help`/`--version`/bad-flag enforced by the check;
  seven-file prune + launch header; 8-case behavioural suite
  (`native-log-run-check.sh`); `--help` still stdout (Round 189).
- **Engine snapshot written on quit and every 30 s (`979a5bd1`)** —
  `save_state_snapshot(false)` keeps `<userData>/engine-state.json` warm;
  manual "Save engine snapshot" and both quit paths save the resolved path;
  one-shot failure logging; default-path round-trip test (Round 188).
- **Media watcher ignores access events (`8281bcc5`)** —
  `Access(_)` events no longer wake the handlers; the Library
  watch→rescan→re-watch atime loop on atime-journaling filesystems closes
  (Round 187).
- **Library "Open folder" (`737672ed`)** — header button opens the current
  subfolder in the file manager; kind allow-list + traversal/absolute/
  drive-letter/NUL rejections; kind-root → profile → media-root fallback
  chain; unit + resolver tests (Round 186).
- **Learn and Forget MIDI input links (`33ea2fb6`)** — Inspector gains
  one-shot Learn/Forget + a "Waiting for MIDI…" state with floor-based
  resolve ordering; a resolved Learn over an audio link releases its
  smoothing id; the `MidiLearnDraft` is drop-safe and stale-safe;
  16-case kittest (Round 185).
- **Legacy renderer labels paused engine sources (`0d7719c5`)** — the engine
  now reports `paused`; the panel renders `Paused` instead of `Unknown`
  (Round 183).
- **Video playback-rate control (`a1016475`)** — flushing rate seek at
  current position; `position_ms` now caches across flushes; 0.1–10 range with
  clamp + rejection; parked EOS files keep state; looping survives rate
  changes (Round 180).
- **Palette tween editor (`c96601a6`)** — `palette_shape` detection
  (RGB/RGBA rows over 0..=255, normalised vec3 stays numeric), swatch row
  engine (add/remove/set with preserve-alpha), CSS hex parsing, two-row floor,
  `times_after_row_change` re-even semantics; committed golden shows the full
  flow end-to-end (Round 179).
- **Video-file transport substrate (`64f364f1`)** — pause/resume/seek/loop/
  position/EOS in `modv-media`'s `VideoSource` + `VideoHub` (snapshot model,
  no callback; looping default for files; live sources unseekable; pause stops
  frame publication); full test coverage; UI scrubber is the next batch
  (Round 178).
- **One-time media-tree Scene import (`19af2dad`)** — per-profile,
  deterministic, seeded-vs-custom collapses to id-or-name collision checks;
  originals never touched; flag persisted in `native-project.json`;
  `media/setMediaDirectoryPath` + compositor `ImageTextures`/previews now
  resolve under the shell's own media root (Round 176).
- **Assign Library assets to texture props (`7f4d16ce`)** — image/video
  texture controls accept Library drops (kind-matching only, palette
  refused) or arm a from-library pick; the payload is one shared
  `MediaLibraryDrag`; kittest pins the exact prop payload vs the manual
  path editor; drag-drops enable, mismatched drops are refused (Round 175).
- **Library tab + media index (`a20f105f`)** — recursive, capped index of
  `image/`, `video/`, `palette/` with decoded thumbnails/palette strips/
  MP4-WebM metadata; a new right-strip Library tab with folder navigation,
  breadcrumb, rescan, empty/error states; TextureControl drag-assignment;
  bursts coalesce to one rescan; 19 goldens re-baselined (Round 174).
- **Screen capture through the portal (`cbdcbdae` + `65d14072`)** —
  the shell asks the XDG desktop portal itself; `pipewiresrc` captures the
  node as `screen:portal` (camera-like, Webcam-indexed); one-slot
  request/capture discipline with stale-answer drops; pending/running/error
  UI states with Cancel; unavailable gating happens **before** any dialog
  opens; capture GPU/log noise kept honest (Rounds 172–173).
- **Recursive ISF scan + watch (`ee65cd55`)** — user folders walk
  recursively with depth/entry caps and symlink-cycle refusal; corpus stays
  flat; longest-root attribution; `isf/compiled` floods now debounced
  rescans (Round 171).
- **Cameras in Webcam-index order (`6d9bb058`)** — `ordered_cameras` ports
  `orderEngineCameraInputs`: running cameras lead by index, unselected by
  name; kittest pins the row order (Round 170).
- **Plugins panel complete (`1515694a`)** — per-plugin rows (dot/enabled/
  capture/author/version/path), generated prop controls (range/checkbox/
  text/colour/select with Int rounding, ratio clamping), `Logs & errors`
  collapsible with Clear/Copy, empty-discovery pointer to the folders,
  Reload/Rescan actions, 1 Hz refresh; the committed golden documents
  all three plugin states incl. an error case (Round 169).
- **`Strobe Riot` retired (`be173841`)** — `DEFAULTS_VERSION` 17→18 in both
  lanes; first production entry in `DEFAULT_SCENE_MIGRATIONS` (version-keyed
  removal); seeded copy removed on upgrade, custom same-name Scene kept;
  REMOVED in both lanes; REFRESHED loses it; all three copies verified
  synchronised (61/61, same order; Round 168).
- **Consent-gated Plugins panel (`abd0c51e`)** — replaces the stale
  placebo; a real Advanced gate (warning, master switch, Revoke consent,
  consent modal explaining the sandbox honestly); the first enable opens
  the dialog and queues `SetPluginConsent(true)` then `SetEnabled(true)`;
  the snapshot finally feeds the read model; 1 Hz refresh while visible;
  logs cleared on reopen; **closes Round 164's stale-panel finding**; bare
  panel still lacks rows/props (Round 167).
- **Per-row cell apply (`c7a6bac1`)** — a cell click dispatches
  `scenes/applySceneRow` (was: `applyScene` for the whole Scene); empty cells
  apply nothing; the context menu distinguishes "Apply this row" from
  "Apply whole Scene"; the enable dot still applies neither (Round 166).
- **Plugin logs retained + prop writes persisted (`18fcdc81`)** — the
  runtime keeps a deduped 256-line panel window (clearable), forwards hook
  errors, coerces prop writes through the host's schema, persists them in
  `native-plugins.json`, and reapplies them after load/rescan/restart; full
  property-round-trip coverage (Round 165).
- **Plugin panel read model (`8e16e008`)** — `PluginView`/`PluginInfoView`/
  `PluginPropView` (all kinds, bounds fallback, int/float rounding, legacy
  ratio colour payloads), ~30 unit tests; the panel itself remains the
  unwired Renderer step (Round 164).
- **Plugin frame hooks run (`b4481f3c`)** — pre-hook runs before
  composition (sees current frame), post-hook after present (like NDI);
  capture read-back shared with the preview path, 20 Hz throttle, only on
  request, alpha-flattened; `Compositor::read_frame_at` factored out; GPU
  test asserts exact size + opaque alpha (Round 163).
- **The sandboxed Boa plugin host (`026b5721`)** — `plugins.rs` substrate:
  per-plugin Boa realms, CommonJS shim removed after load, five globals
  (props/modV.get/modV.ws/log), caps on source/capture/loop/recursion/stack,
  catch_unwind isolation, failures recorded once per (plugin,phase);
  24 adversarial unit tests; **nothing wires it in the app yet** (Round 161).
- **Per-row live Scene markers in the grid (`dde24d04`)** — the
  snapshot carries `active_scene_rows`; `scene_cell_highlight` splits
  Applied/Live/None; the applied column shows `APPLIED` in its header;
  rows gain a live-name line under the row label; cells show a
  `LIVE` badge; kittest + golden grid coverage (Round 159).
- Bar-unit doc comment de-proper-nouned (`822e7303`) —
  "Unger's bar-based…" → "Bar-based…". Round 158.
- **Bar-timed Scene times in the native UI (`1901063c`)** —
  `SceneView` exposes override min/max separately (was: max for both),
  `PerformanceView` carries the unit; the bar gains a unit selector +
  resolved-seconds hint; the dialog gets quarter-bar steps and a
  `Reset override`; all 22 affected goldens re-baselined; **closes Round
  155's red-test finding** (Round 157).
- BPM readouts no longer repeat the capture rate (`dad7c58e`) — menu
  bar, status bar and Info panel show the bare `BPM N`; rate stays on the
  audio indicator and the Tempo window; kittest expectations updated
  (Round 155).
- **Startup Save-target guard (`7f27a34a`)** — closes Round 151's second
  finding: `--preset` pointing at a Scene/playlist no longer arms Ctrl+S for
  a plain-preset overwrite; `<userData>/logs` is created before xdg-open;
  the profile modal rejects an empty name and closes on success. Still
  open: the Ctrl/Cmd+Shift+S overlay row (dispatches `SaveSnapshot`). Round
  154.
- **Native file dialogs and the File menu (`df395840`)** — in-app egui
  picker (no portal/GTK round-trip), six menu actions, project profiles
  mirroring the Electron media manager (`<userData>/native-project.json`,
  tmp+rename), `--scene`/`--playlist` aliases, `--preset` auto-detection,
  modal Ctrl+S/Ctrl+O guards, Escape-close, argv-safe xdg-open; three
  follow-up items logged as Round 151 findings (Ctrl+Shift+S overlay vs
  `SaveSnapshot`, startup Save-target gating, PROFILE_FOLDERS superset).
- `scenes/saveCurrentAsScene` implemented in the engine (`c06bfb1f`) —
  the Scene-grid save button in engine mode fell through to the dispatch
  wildcard and silently no-oped (the "cannot save current scene" report);
  the engine now snapshots the settled stack, persists the scene list
  atomically and returns the document for `Scenes.vue` to write
  `scene/<name>.json` (Round 28).
- Scene-grid column/list actions implemented (`aa2814a6`): `addEmptyScene`,
  `moveScene`, `setSceneEnabled`, `updateSceneName`, `updateSceneTiming`,
  `resetSceneTiming` — all six were silently dead in engine mode through the
  dispatch wildcard; registered (`store_ops.rs:448-453`), ported faithfully
  (identity-return → false on no-op; unique-name fallback;
  `ADD_SCENE` upsert mirror-safe), unit-tested, persisted atomically. The
  progress-doc claims (`7ac215bd`) were both verified accurate this round
  (Round 29).
- NC-SA ports gained linkable float inputs (`8da469e0`): Helix Eye, Traced
  Tunnel, Hot Shower — defaults byte-verified identical to the removed
  consts; shared JS/Rust corpus so both renderers pick them up (Round 30).
- Grid cell/Gallery actions implemented (`1236c374`): `clearSceneCell`,
  `captureSceneRowToScene`, `createSceneCellGroup`, `disableGalleryItemModules`,
  `focusGalleryItem` (newly named), `orderByIds` — verified branch-level, but
  **two defects flagged, not closed**: `clearSceneCell` leaves phantom ids in
  the live Group (P2, item 26) and the capture fallback can grab another
  Scene's row (P2, item 27) (Round 31).
- `scenes/copySceneCellToScene` implemented (`236a3bf9`): faithful to
  `scenes.js:3568-3735` — positional write, hole padding/trim, id-membership
  carry for inputs/links/assignments/tweens, dangling-tween drop, 5 unit
  tests (Round 31).
- `groups/duplicateModule` implemented (`457c846a`): live props/meta carried,
  fresh ids, prop/meta input ids remapped by key (superset of the JS index
  zip), links and expressions recreated on the new inputs, position + order
  tests; `beats/SET_BPM_SOURCE` and `fonts/SET_LOCAL_FONTS` land the
  remaining easy audit items; progress doc honestly updated (Round 32).
- Point10 Old Movie retired (`5ebd4a24`) — removed from JS and Rust
  catalogues (36/30 counts consistent everywhere), Boxy Arcade's Effects row
  now uses the `Sepia Tone` ISF filter in both default-Scene sources, v13
  installs refresh in place (`REFRESHED_DEFAULT_SCENES`; two-versions-behind
  not refreshed — strictly stronger arm), removal guard forbids the literal
  (Round 33).
- Three sparse Shadertoy NC-SA Scenes seed as v15 (`56f56364`) — Ember
  Storm / Prism Tunnel / Sentinel; `DEFAULTS_VERSION = 15` both languages;
  JS/Rust scene bodies semantically identical (byte-verified per field walk);
  per-module prop ranges all in-spec; linked controls all resolvable;
  v14→v15 seed lane unit-tested; e2e version pins corrected (Round 34).
- Engine-owned xdg-desktop-portal screen capture groundwork landed
  (`145005ec`) — ashpd ScreenCast backend for the engine picker, dedicated
  current-thread tokio runtime keeps the PipeWire node alive; pure unit tests
  only; app wiring still pending (`media/pick-screen`); no callers at HEAD
  (Round 37).
- `media/pick-screen` input validation (`7f501bac`) — non-monitor/window/
  both or non-string `sourceType` rejected with `invalid_params` before the
  portal thread spawns; unit test confirms no dialog is opened. Desktop
  portal capture docs recorded accurately (Round 41).
- Progress doc records NDI, tween-editor and mediaStream state accurately
  (`f5c243b6`) — new entries match verified behaviour; three pre-registered
  stale-claim sites survive unchanged (items 30/31) (Round 45).
- The three renderer and `Cube` removed from both runtimes (`f7d35db0`) —
  the last module-parity gap closed; worker glob clears; Gallery section and
  aliases removed (JS side only — the Rust ISF category normaliser keeps a
  stale "three" mapping, Round 48); e2e and guard tests updated (Round 48).
- Screen blend mode parity (`a6ca39c0`) — `BlendMode::Screen = 28` (JS menu's
  own `globalCompositeOperation`, absent from the menu) with the W3C separable
  math, Porter-Duff bounds tightened 15..=27, parse+count updated both
  crates, graph golden case, JS menu offers Screen after Multiply; the 73
  default-Scene Foreground "screen" groups no longer degrade to normal
  (Round 49).
- Point10 `Sun` and `Earth` ported to wgpu (`cbb24d8a`) — registry, kind,
  parity-verified JS↔Rust prop schemas, packaged texture assets resolve via
  `$MODV_ASSETS_DIR`; the legacy Gallery gap documented in Round 50.
- `Point10 Geometric Tunnel` retired in both runtimes (`34528ff6`) — five
  default Scenes retuned in place (v16, `REFRESHED_DEFAULT_SCENES`), SSAO pass
  deleted, guard tests updated, JS/Rust docs verified field-identical
  (Round 51).
- Scenes panel crashed on null group holes after a Ctrl+drag cell copy
  (`0fc00538`) — new `scene-groups.js` tolerant counter; panel and
  `modules/init` degrade gracefully; Ctrl+drag e2e asserts the panel
  survives; Round 52.
- Volume curl-noise precompute now runs with real uniforms (`cf9be174`) —
  the `gid >= u.grid.xyz` guard had rejected every workgroup since the sim
  spawned with a zeroed uniform block; field was zero and the four volume
  scenes rendered the radial puff instead of the turbulence. GPU regression
  test asserts non-zero (round 53).
- Clippy/fmt for Rust 1.100 (`6e5cf6be`) — `FromStr` for `BlendMode`, the
  inherent `from_str` is gone (Round 54).
- Native hover previews land as real isolated engine renders (`ea5fe63a`) —
  Gallery tiles and Scene cells poll at ≤150 ms, one preview per frame,
  bounded 32-entry LRU with dedup warnings, premultiplied→opaque flatten
  before egui; Scenes panel filled (perf bar, timing/row-name editors, rename,
  enable toggle, move left/right, empty-cell group creation, Add Scene).
  Round 57.
- Stale support claims corrected (`fe8a75ad`) — boot warning/header name only
  JavaScript plugins; the transition gap paragraph and the MIDI/OSC paragraph
  fixed; the 429/424 counts → 432 at both stale sites + module-parity
  pointer; store_ops registered-modules bound tightened (36+15+27); the
  phantom `three` ISF category aliases dropped (JS parity); stray `\n` doc
  comment fixed (Round 58).
- `--apply-scene` CLI for reproducible native traces (`590b1fe0`) — name
  or id lookup after default seeding, with info/warn messaging; bump to
  prelude seed-load flag; unit test pins name + id lookup (Round 72).
- Info and Plugins side tabs (`edd93b55`) — adapter/backend/frame/audio/
  scene summary, honest Boa plugin disclosure "not yet landed"; snapshot
  re-baselined to show both tabs (Round 73).
- Whole-scene shader transitions in native (`4f41c87c`) — capture,
  prepare, fallback-to-Fade, `SceneTransitionInfo` into the compositor; row
  crossfades stay working, shader/ISF transitions now fire in native mode
  too; ordering (capture → sync_media → prepare → render) puts the outgoing
  frame capture before media sync. Round-56 P3 gap closed. Round 71.
- **Native audio restored (`9d3c1fc4`)** — `NativeApp` opens an
  `AudioFeed` (CLI `--audio-source` > Pipewire > Input fallback), pumps it
  before Input Links and feeds `set_engine_audio`/`disable_engine_audio`
  correctly; camera enumeration cached at 1 Hz; saved window clamps to the
  primary monitor at launch; preview-error re-report cadence (every 10th);
  `add` → `lighter` blend token finally legal; Ctrl+K toggles with a dedicated
  `palette_focus` flag; Show keyboard shortcuts is a real action, not a dead
  dispatch (Round 70).
- P0 release/lavapipe evidence recorded (`71f707ad`) — 138–143 fps on the
  reference GPU, audio live, 56.9 fps at 960×540 on lavapipe (one caveat
  recorded), ~157 MB RSS; full P0 written go/no-go verdict lands in the
  progress doc (Round 74).
- First native parity map (`f6d88439`) — every legacy e2e group mapped to a
  Rust/kittest/manual/waived bucket; the remaining P8 work is named
  (kittest interactions, exact-count guard, third-party ISF, clean-VM
  checklist); accurate against HEAD; regenerated at P8 start (Round 75).
- **Expressions AST allowlist lands (`b84553ac`)** — Boa parser + validator
  thread, AST allowlist (Math/Number/Array/value methods, literals, plain
  property reads, no new/Function/eval/assign/regex/BigInt/loops/timers),
  member-name constant folding, 2048 non-whitespace / 512 AST nodes /
  32 depth structural caps, Rust-owned handle map (no hidden global).
  Outstanding item 1 ("expressions have no wall-clock budget") is closed.
  The progress doc's "a denylist rejects native allocation/loop/context
  escapes" line now under-states the landed fix (Round 76).
- Preset load/save in native mode (`0d83d639`) — `--preset <path>` loads a
  six-key preset at startup; File → Save preset writes back; round-trip
  test; Go/no-go: chosen format compatible with the hybrid engine's
  snapshot writer (Round 77).
- Registry parity guard lands (`10ac008d`) — `registered_module_count_matches_the_parity_baseline`
  asserts 36+15+27+354 = 432 from the corpus, skipping only when the corpus
  is absent; HEAD counts verified directly. The e2e `engineParity.spec.js`
  still asserts `> 300` — remaining gap in item 12 (Round 85).
- Native AppImage target lands (`40f12406`) — `make appimage-native`:
  release `modv` bundled with ISF corpus + Point10 textures, AppRun with
  `MODV_ISF_DIR`/`MODV_ASSETS_DIR`, 24 MB artifact; author-verified launch
  at 120.7 fps. **It exposed the `--bench-seconds` inheritance bug (R87).**
  Round 86.
- Native `--bench-seconds` inheritance fixed (`1d80f5f7`) — native runs
  until the window closes unless the flag is explicitly supplied; status
  bar relabels frame counter `frames total` (round 87).
- BPM now in the status bar (`323a0eb5`) (round 88).
- **DVD Room logo/emitter boxScale fix (`0d14f1c3`)** — the logo model
  now multiplies by the `_boxGroup` scale (`matrixWorld`-consistent),
  `logo_world_position` feeds the volume seed, the logo/glass point-light
  origins and both `_uSphere*` uniform paths; bounce bounds stay local
  (`stepBounce` faithful); regression test pins boxScale=2. Round 92.
- **Native render loop paced to the engine FPS (`b0bdb55b`)** —
  `FramePacer` (non-catch-up semantics, jump-forward on stalls), `WaitUntil`
  control flow, `fps/SET_FPS` wired via `Store::target_fps()` (clamped to
  [1, 1000], default 60). The Electron-native 2.3× speed defect closes;
  tween frames/smoothing/feedback run at the same wall-clock speed
  (Round 94).
- **ISF corpus arithmetic corrected to HEAD (`e9cc2349`)** — 354/36,
  64 PASSES (47 multi-pass), 46 PERSISTENT, 15 FLOAT, 288 image, 64
  undeclared-TIME — every number re-verified from the corpus by me
  (round 95).
- Groups pane always renders every Group's module list
  (`9c084394`) — shared `group_module_list` helper; per-Group editors stay
  selected-only; kittest pins both arms; snapshot re-baselined (Round 103).
- Audio device enumeration is now on-demand (`12c4666e`) — one enumeration
  at startup + the panel's Refresh button; the 1 Hz poll (which flooded
  ALSA/JACK and exhausted pthread keys on unconfigured hosts) is gone
  (Round 104).
- **Fully-disabled grid keeps performance mode inert (`fa163c0d`)** —
  `startPerformance` distinguishes "no cells at all" (legacy autopilot) from
  "all cells disabled" (no rows/no applies); regression test pins the
  fully-disabled grid. **Completes the disabled-cells feature.** Round 105.
- Gallery tiles marquee on hover (`73feb02a`) — full name, ping-pong
  cycle (0.5/28 px·s/0.8), clipped inside the strip, linear movement,
  pinned by pure helper tests (Round 118).
- **Free keyframe times in tweens (`7202749d`)** — optional
  `keyframeTimes` normalized 0..1 progress replacing anime-even spacing
  with piecewise segments (local easing/steps applied); Electron ignores
  the key; invalid lists fall back to anime semantics and keep the raw
  payload in the round-trip (Round 123).
- **tween read model widened for the curve editor (`e2eb0680`)** —
  `data`, validated `keyframe_times`, `durationAsTotalTime`, `cycle_seconds`
  via `effective_duration(bpm)`; decodes through `TweenDescriptor` so the
  views share the engine's fallback semantics (Round 128).
- **Persisted panel / tab / window layout (`f359e459`)** — the document
  becomes `dock_width` + `tabs`; `modv-ui::layout::LayoutState` runtime model
  + transitions (exactly one placement per tab); load-clamp-apply at startup;
  sync-and-write at most once per second (and on quit); defaults identical to
  the previous build (Round 135).
- **Native neon theme overhaul (`5da737a8`)** — near-black ladder,
  single green accent, every radius 0, Plex faces step up the type scale,
  interaction rows and sliders bigger, panel defaults grown; goldens
  re-baselined (Round 143).
- Tab drag: strip advertises drag outline (no hover needed) for
  docked/floating tabs (`14700585`). Round 148.
- Frame backgrounds now pure black (`6b5d07ef`) — `BACKGROUND`/`PANEL` are
  `#000000`; raised surfaces step lighter (`INPUT` → `#0A0A0A`,
  `PANEL_ALT` → neutral `#121212`); theme tests updated; goldens rebaselined
  in next batch (Round 150).
- **Scene cell disable toggle now reliable (`d35241f6`)** — toggle sends
  the new state (`enabled: cell.disabled`); `CellEnableDot` owns its
  presses in a 20×20 area; disabled cells unmistakable; kittest pins both
  directions end-to-end. Round 149.
- **Drag tabs between strip, dock and floating frame (`60b59e20`)** —
  typed `TabDrag` payload, strip grip, dock-here placeholder, float outline,
  release writes placement with bounds clamp; pure payload/placement/bounds
  pinned; goldens re-baselined (Round 147).
- Refresh of native progress + parity maps (`1b7eb187`) — P0/P5/P8 rows
  honest, packaging closed, `smoke_room` retired; all counts named
  as tree-counted not re-run (Round 146).
- **Docked/floating sliders have real contrast (`f239ad56`)** —
  `DOCK_FILL`/`STRIP_FILL` route sliders; `theme::slider_visuals` paints
  SLIDER_TRACK/handle; regression kittest pins every tab kind with a real
  RangeControl and two goldens (Round 145).
- **Input links survive Scene applies (`40fdb8fb`)** — `InputIdentity`
  capture-before-apply + `retired_inputs` retarget in `create_input_link`;
  tween locations rewrite to the rebuilt id; INFO-not-WARN for resolved or
  genuinely-missing cases; regressions pin the round-trip with the exact bass
  value assertion (Round 144).
- IBM Plex Sans/Mono bundled (`df2ade1f`) — four static faces, OFL 1.1,
  Plex-led font families with bundled egui faces kept as fallbacks, named
  weight families for strong text; unit-pinned; notices updated (Round 142).
- **NDI + OSC native panels (`dbc2aa76`)** — `NdiSnapshot`/`OscSnapshot`
  read models, engine accessors exercised as-is, 1 Hz tab-visible refresh,
  `osc/createServer`/`removeServer` + `osc.data` flattening with copy; both
  tabs detachable/persisted; goldens re-baselined (Round 141).
- Native CI payload triggers fixed (`a331368c`) — the ISF corpus, Point10
  textures, icon.png and LICENSE now trigger the native workflow too. The
  packaging job correctly rebuilds on corpus changes (Round 140).
- **Native `.deb` packaging (`da724d45`)** — `make native-deb`: package
  installed layout under /usr/bin (wrapper), /usr/lib/vmodv-native
  (binary), /usr/share/vmodv (ISF + textures); md5sums + root-owner-group;
  launcher resolves the prefix; Depends matches NEEDED + dlopen (GL,
  xkbcommon-x11); auto-runs `native-deb-check.sh` (fields, paths, corpus
  counts, `--help`/`--version`); `make native-deb-check` separately works
  (Round 138).
- **Native Rust CI (`26121571`)** — `.github/workflows/native.yml`: Clippy
  & rustfmt, workspace tests (modv-app serial for shared Lavapipe), AppImage
  + .deb build with artifact upload; native-only, nightly Electron stays the
  release default; P9 row updated (Round 139).
- **Drag Gallery modules into Groups (`3bd065d2`)** — tile payload +
  accent-chip drop target on every Group block; release queues
  `ui/addModuleToGroup` (same as double-click); pure `gallery_drop_action`;
  kittest covers all four drop cases + double-click regression; goldens
  unchanged (Round 137).
- **Resizable sections, floating windows and detachable tabs
  (`d2951184`)** — Live Output/Gallery resize as top panels; Groups/Inspector
  resize; eight floating types with ids + record; per-tab Detach → Floating
  (dock/back tabs) or Docked (header Float/Back to tabs); strip hides when
  empty; one placement per tab guaranteed; goldens tab_float/tab_docked/
  tab_back_in_strip; shell re-baselined (Round 136).
- Clippy `field_reassign_with_default` fixup (`72baa971`) — fixtures
  converted to struct-update literals; no behaviour change. Round 134.
- Live capture rate publication (`4e57f81b`) — an
  `AudioCaptureRate(Option<u32>)` cache initialised on open/refreshed on
  switch/stopped on error, threaded through `audio_view` and
  `audio_selection_view` so BPM/menu/tempo-window/Info all show the same
  negotiated rate; silent paths omit it; `bpm_with_rate`/`bpm_hover` /
  `audio_status_text` pinned by kittest; `bpm_window` golden added (Round
  133).
- Audio picker shows the capture rate (`e8f02b08`) — rows print the
  negotiated `capture_sample_rate`; `AudioSelectionView` prefers the live
  feed's rate and falls back to the device row; BPM panel reads it; the cpal
  44.1 k heuristic no longer reaches any UI path (Round 132).
- Native layout persistence landed (`6fa788a8`) — `ui_layout.rs` phase 1:
  `<userData>/native-layout.json` with Groups/Inspector widths, Live/Gallery
  heights, per-window geometry; pure clamp rules (proportional below
  `MIN_TOTAL_WIDTH`; Scenes-first budget; floating-window clamp; NaN-out);
  atomic tmp+rename write. Wiring into the shell is phase 2 (Round 131).
- Tween curve editor wrap + channel-cycling fixup (`006c9704`) —
  `horizontal_wrapped`, Edit curve button position, ≥6 channel color cycle,
  `nearest_handle` cap removed, JSON-branch test added (Round 130).
- **The tween curve editor ships (`f9500bf6`)** — `tween_curve.rs` model +
  `tween_control.rs` handles/canvas/inspector sync; normalisation on first
  edit via the committed "duplicate first row + even grid" path (Round 123/126);
  save dispatches the descriptor payload; kittest covers add/move/remove/
  round-trip; new `tween_curve_editor.png` golden renders the whole flow;
  the native Inspector finally edits tweens the way Electron does (Round 129).
- Audio table reports the real capture rate (`5f832d5a`) — new pure
  `capture_rate()`/`device_info()`; `InputDeviceInfo.capture_sample_rate`
  computed with the same rule `choose_config` applies, alongside the verbatim
  advertised-rate list and the untouched cpal heuristic; tested incl. ALSA
  virtual devices and no-48k fallbacks (Round 127).
- **Even→free keyframe conversion (`f2361b01`)** — `with_free_keyframes()`
  re-expresses anime-even 3+ row tweens as a duplicated first row + explicit
  times on the even grid; bit-exact playback (non-BPM); ≤1e-12 BPM drift;
  `evenly_spaced()` fast path identical to even sampling. Tests pin frame
  identity on 60/30/144 fps × loop × 3/5 rows (Round 126).
- **Webcam resolution / frame-rate controls (`c211457e`)** — `CameraView.modes`
  + resolved `selected`; pure-units `unique_resolutions`/`fps_options`/
  `default_camera_mode`/`mode_for_resolution`; per-camera Resolution and
  Frame rate ComboBoxes; switch = stop/start at valid mode + order restore
  + `native-media.json` rewrite; caps-less cameras say "mode not reported";
  camera-only index everywhere (Round 125).
- **Native webcam selection persistent (`b6ae28a0`)** — `<userData>/native-media.json`
  with ordered cameras + per-camera modes, mirrors Electron's `mediaStream`;
  restore prefers exact advertised mode → same resolution × nearest fps →
  smaller resolution × max fps → device max; unplugged cameras resume on
  replug; explicit stop forgets; camera-only 1-based status index; no pipelines
  restart on reorder; panel gains checkboxes, badges, reorder arrows (Round 124).
- Tween reference fixture gains the reverse-direction 3-row phase
  (`e555ba04`) — generator config + fixture entry + the reverse phase now
  covered frame-exact by `multi_keyframe_frames_match_anime`; no Rust change.
  Round 122.
- **Anime multi-keyframe tween sampling port (`0263f2ee`)** — a 3+ row
  tween now splits into one segment per keyframe with per-segment easing/
  steps; the first segment holds row 0; reference fixture regenerated from
  real animejs 3.2.2 (`tools/tween-keyframe-reference.mjs`); `frame 0`
  (incl. `direction="reverse"` reset target), segment boundaries and
  loop ping-pong match anime byte-for-byte. Round 121.
- Performance Scene-order select (`4c3af40f`) — native engine already
  honoured `performance.order` at all pick sites; the bar now exposes the
  In order / Random combo, normalising unknown values to the engine's
  effective behaviour, with a round-trip persistence test, live-change
  applies on next pick, kittest + goldens re-baselined (Round 120).
- **Per-row layer monitors in the Scenes grid (`56745632`)** — GPU-GPU copy
  of the Group's rendered texture into a per-Group snapshot, egui-wgpu
  native texture (premultiplied), pure ~3 fps refresh cadence, dimmed when
  performance is off, `no layer` placeholder otherwise; prune-on-apply
  (fresh Group ids); unit + GPU + kittest coverage, goldens pinned;
  the production frame graph is untouched (Round 119).
- **Premultiplied preview readbacks un-premultiplied (`75e7f116`)** —
  `modv_ui::un_premultiply_rgba` (round-to-nearest, clamped) applied in the
  egui upload before `ColorImage::from_rgba_unmultiplied`; GPU-side textures
  (live main output) explicitly skip the conversion (premultiplied blend
  consumes directly); the flatten-to-opaque hack is gone. Native half of
  item 1 closes (Round 117).
- Shared lavapipe adapter helper (`e1acbf55`) — every GPU test harness
  (isf/runtime, point10, two_d) delegates to `crate::test_gpu`, one adapter
  per test binary (Arc-cloned), with a counting regression; the SIGSEGV
  under parallel `cargo test` closes. Round 116.
- **Component sub-inputs minted for data-typed props (`11446c5a`)** —
  `initialiseModuleProperties` ported: `${parent}-${key}` sub-inputs for
  color (r/g/b/a) and vec2/3/4 (0..N), `type: "action"` with
  `path: "[key]"` and a getLocation pointing into the parent; make/apply/
  cleanup all round-trip with JS-parity `mapModuleInputIds` union-of-types
  remap; the Inspector lists them; the shared `data_type_input_keys` table
  kills the scene_cells duplicate. The component half of outstanding item
  7 closes (Round 115).
- **Point10 smoke-room re-baseline (`9e4a5a40`)** — the stale
  `dark_fraction > 0.15` was calibrated against the broken curl precompute;
  replaced with a bounded-glow gradient assertion (brightest 5% > 3× dimmest
  5%) encoding the JS-faithful invariant; both lavapipe and radv agree per
  the author. **The Point10 suite has no known-red GPU test anymore.**
  Round 114.
- **Group create mints the six control inputs (`cd0eacc3`)** — `commit`
  input per control bound to `groups/UPDATE_GROUP_BY_KEY` + `data {groupId,
  key}`, dispatched before `ADD_GROUP`, ids stamped on the Group
  (`<name>InputId`); pre-supplied ids trusted (JS "only mint when
  undefined"),`null` kept; round-trip keeps create→save→restore without
  duplicate minting; parity fixtures and engine-mirror bump coherently
  (36→54 deltas, 17→35 inputs). Round 113.
- Full `Cargo.lock` dependency inventory in the notices (`6eb80170`) —
  every one of the 570 registry crates listed with SPDX licence + upstream
  link (worktree re-verified: 0 missing / 0 stale / sorted unique);
  obligations spelled out for `colored` (MPL-2.0),
  `epaint_default_fonts` (OFL-1.1/Ubuntu-font), `libfuzzer-sys` (NCSA); the
  guard test enforces all of it on every unit run. **The notices P1 is
  done.** Round 112.
- AGENTS.md documents `performance.disabledCells` native-only semantics
  (`f4495117`) — byte-accurate against HEAD; updated in the author's own
  doc. Round 106.
- Expression/tween/meta input views exposed in the snapshot
  (`43fa4bbe`) — `alpha_input_id`/`composite_operation_input_id` on module
  and group views; `expressions`/`tweens` arrays; pure extracted builders
  with unit tests (Round 109).
- Link editor section gating (`38c63a1c`) — MIDI/Tween/OSC/Audio sections
  now disabled when editing an existing non-matching source (no more mixed
  half-states on Save). Round 111.
- **Input Link audio/expression/tween editors ported for meta inputs
  (`d38045d9`)** — link rows for module/group alpha and blend; module
  Blend dropdown (29 tokens, all accepted by `BlendMode::from_str`); the
  Expression section with create/update/clear payloads, every-frame toggle,
  and a cleared-latch guard; snapshot re-baselined (Blend control; Round
  110).
- Audio picker clarified (`9e54b5e2`) — IN-USE banner (green dot,
  resolved friendly name, tech line), system-default first ordering,
  radio/selected rows, cpal ids to hover, `is_usable_input_device` filter
  (ALSA backend probes + 0-channel + dupes dropped; PipeWire/PulseAudio/
  hardware kept), separate `audio_panel.png` golden; default shell snapshot
  unchanged (Round 108).
- Scenes grid smoke test asserts the add controls render (`6fe0cb50`).
  Round 107.
- Native Gallery: `All` default + flat search, no 24-tile cap, grid scroll,
  aspect-correct letterboxed preview, P0 spike scaffolding retired from the
  visible UI (`252aca6f`). Round 96.
- **engineParity e2e pins 432 (`dde600a4`)** — the `> 300` gate is gone;
  both the wait and the assert use `EXPECTED_REGISTERED_MODULES = 432`,
  in lockstep with the Rust baseline guard (item 11 narrowed further;
  Round 93).
- **`.deb` declares `gstreamer1.0-pipewire` (`39d7c1e3`)** — the engine's
  PipeWire screen-capture dependency was absent from `deb.depends`
  (`pipewiresrc` factory requirement verified at
  `modv-media::video::pipeline`); the fix adds it plus a unit-test
  regression gate (`appimage-flags.test.js`). Round 98.
- **Skip performance-disabled Scene cells (`578b802a`)** — native-only
  `performance.disabledCells` key (`{sceneId: [row]}`), pruned on
  hydrate/persist; `scenes/setCellEnabled` toggles per-cell flags (manual
  apply still renders disabled cells; only performance picks skip them);
  engine tests pin start/dwell/prewarm/in-order/random semantics;
  snapshot/store/TTL coverage round-trips the flag (Round 101).
- **Row/column controls in the Scenes grid (`c8993a65`)** — column drag
  with snapshot-derived moveScene dispatch, row drag (splice semantics
  identical to `onRowDrop`), inline row rename with the standard egui
  idiom, `+ Row`/`+ Column` buttons, `Layers`-gutter with scroll,
  per-cell performance dots (click-to-toggle, no apply). Snapshot
  re-baselined (Round 102).
- **Named-row cell mapping fix (`6c5f3687`)** — `scene_view` maps Groups
  onto their named performance rows via `assign_scene_row_indexes` (the
  same apply-time mapping); the hover-preview request is gated behind
  non-empty cells; regression tests pin the exact user fixture (Mycelium
  Bloom), the store round-trip of `performance.rowNames`, and the empty-cell
  gate. Round 100.
- Bundled texture provenance corrected (`893e891c`) — `dvd_logo.svg`
  (Wikimedia public domain, per `DvdRoom.js`), `moon_04`/`rubber002`/
  `concrete044c`/`tinore` named as ambientCG CC0, `stormy`/`spruit_sunrise`
  as Poly Haven skies; NASA claim narrowed to Earth/Milky Way; unclear
  assets remain non-MIT. Round 99.
- Notices guard reformatted by prettier (`85349542`) — formatting only.
  Round 97.
- **Notices complete (`34d04e47`)** — the Plane9/Joakim Dahl
  family (11 ports, JS + Rust twins, per-file licence variant), the
  mojovideotech CC BY-NC-SA shader family, st_Ms2SD1 (TDM CC BY-NC-SA 3.0
  explicit), the corrected spherical-shader-tut/Perlin-Transition/Helix-Eye/
  Traced-Tunnel dependency attributions, the Miguel Alba pcb.jpg texture,
  the direct Rust crate licences, and a README badge that no longer implies
  the bundle is MIT; a guard test re-scans NC-SA in the tree on every unit
  run. **The notices P1 is closed.** Round 90.
- Native audio device picker (`29d92814`) — an Audio side panel lists the
  PipeWire monitor, the default input and every cpal input (name/channels/
  rate), selection marks live state, persists to
  `<userData>/native-audio.json` atomically, explicit CLI/MODV_* still wins,
  a failed live switch keeps the previous feed and reports the error
  inline. 9 unit tests + kittest coverage (round 91).
- **Shared Scenes list loaded at startup (`9ab30445`)** — native resolves
  an absent `--state` to `<userData>/engine-state.json` (the supervisor's
  own constant), restoring `<userData>/scenes.json` in the AppImage; the
  user's Scenes (71 verified by the author) no longer vanish in native mode.
  The regression tests cover both paths. Round 89.
- Verification ledger trued up (`08254041`) — reference-GPU result now
  "138–143 fps unobstructed; 59.7 fps after window-manager resize/occlusion
  (Fifo)", lavapipe 57.5 fps with content, 422 `modv-app` tests recorded
  verbatim (matches the eight added by the MIDI/user-ISF/preset commits);
  62 seeded Scenes counted at HEAD as correct; failing
  `smoke_room_volume_is_structured` still named openly (Round 84).
- Progress doc + AGENTS.md updated for logged P6/P7 milestones
  (`11d9c5c3`) — P6/P7 marked Partial (media watcher, project files, Boa
  plugin host still not landed), P8 marked started; all bullet correspondences
  with verified tree commits — correct claims (Round 83).
- Hot-reload user ISF shaders (`19a71602`) — `IsfWatcher` watches
  `<userData>/media/isf` and each `<project>/isf` non-recursively, 500 ms
  quiet debounce, `ModuleRegistry::build_with_extra` re-parses the whole
  corpus+user set, `Compositor::set_registry` swaps atomically with failure
  caches cleared (broken shaders retry); live-verified by the author on
  this branch (round 82).
- MIDI panel finished + user ISF registration (`4522f73f`) —
  `midi/WRITE_DATA` integer serialisation for the note-on read-back path;
  `ModuleRegistry::build_with_extra` adds user ISF folders under `<userData>/media/isf`
  and `<userData>/media/<project>/isf` (corpus wins on name collision,
  `MODV_USER_ISF_DIR` override), scan at startup with the pinned
  regression test; watch/hot-reload still P6 (Round 81).
- Native MIDI input lands (`539854bb`) — 1 Hz port hot-plug, full Note On/
  CC decode with latches, 24-PPQN clock-to-BPM, the same `midi/ADD_DEVICE`/
  `midi/WRITE_DATA`/`midi/UPDATE_DEVICE` arms the engine verified at Round
  36, MIDI tab in the right pane with per-device flags, snapshot
  re-baselined. JS's channel-0-only status-byte match faithfully preserved
  (legacy latent bug, not a new regression) (Round 80).
- Progress doc verification ledger (`67f1097e`) — test counts recorded
  (modv-engine 459, modv-app 414 serial, modv-ui 25+3+1, clippy/fmt gate),
  the one pre-existing Point10 GPU failure named openly, known P5–P9 scope
  documented (Round 79).
- Clippy/fmt and unused-dependency hygiene (`0cd68d06`) — `uuid`/`libc`/
  `memmap2` removed (exactly 3 lockfile entries shrink); `modv-ipc` clean;
  Copy/cleanup/panic-safe pixel-flatten fixups applied. Round 78.
- `AGENTS.md` documents the opt-in native UI (`594ea38e`) — entry points
  verified against the tree; one honesty gap (the doc says nothing about
  the native shell having no audio feed). Same class as earlier
  claim-vs-code entropy but now benign: AGENTS.md is guidance, not a
  release claim (Round 69).
- Native shell golden snapshot (`6fd8352c`) — representative
  Groups/Scenes/Inspector/Gallery/P0-checklist snapshot with pinned engine
  content and committed `tests/snapshots/shell.png`; egui_kittest gains
  `snapshot`/`wgpu` features. Visual P0 regression net (Round 68).
- BPM window / FPS sparkline / bass-mid-treble meters
  (`e28892ff`) — BPM source combo (` beats/SET_BPM_SOURCE`), 2 s-rolling
  tap history with tap/promote, max 30–300 BPM clamp, FPS history sparkline;
  meters paint opaquely (Round 65).
- **Three Round-31/35 findings closed (`bda55128`)** —
  `clearSceneCell` empties the live Group and rewrites module order with
  `REPLACE_GROUP_MODULES`; `captureSceneRowToScene` drops the positional
  fallback and is strict-stamped; default Bass links that declare
  `smoothing` mint a `getSmoothingId` id (uuid_v4, seeded at 0) and carry
  `[feature, smoothingId, amount]` — v15's calming behaviour finally
  round-trips in the engine. All three carry regression tests (items
  25/26/27 closed; Round 66).
- **Ctrl+K command palette (`a28162d1`)** — fuzzy→substring-filtered
  command list (Scenes, palette actions, module add to selected Group,
  performance toggle, save snapshot, window commands), arrow keys select,
  Enter runs; one functional defect (Show keyboard shortcuts dispatches
  `ui/showShortcuts` into the engine catch-all and visibly does nothing);
  one keyboard race (Ctrl+K steals the Gallery search focus rather than the
  palette's). Round 67.
- **Media panel + window placement + link snapshot (`b765b5c4`)** —
  native MediaHub owns cameras/test-pattern/`poll_health`/`sync_media`
  (pre-frame, same order as the hybrid), Media tab streams the camera
  list + running sources, `window_state.rs` ports the legacy read/atomic
  replace/clamp rules with the same V4L2-sourced encoding (window
  placement is shared with the Electron app via VmodV/window-state.json),
  the Input Link snapshot is populated (closing Round 63's gap), the
  texture_sources populator lands (closing Round 61's). **Native is deaf:
  no audio feed anywhere in the native loop — the witness of the Round 63
  pattern is not audio.** Round 64.
- Focused-input link editor (`ab85acc8`) — widget/tests/field model all
  landed; dispatch grammar verified; **the commit ships the plumbing before
  the paint** — `snapshot.input_links` is never populated at this HEAD, so
  the "read the link record" branch is dead code until the in-flight
  populator commits (second instance of the Round-61 `texture_sources`
  shape). Round 63.
- TextureControl + group/module controls + shortcuts (`73a697ba`) —
  engine-id texture pickers, Group rename (double-click)/alpha/inherit/clear/
  pipeline/reorder/remove, module enable/remove/duplicate/reorder, Ctrl+S/
  Ctrl+Enter/Ctrl+K/F1/Escape/Ctrl+1..9 (Round 61).
- Native full-rewrite tracker added (`10562230`) — P0–P4 honestly marked
  done/pending with the in-flight fixes named (Round 62). **It names
  `window_state.rs` as done before the file is committed — flagged.**
- Native Inspector gains colour, vector and tween controls (`060e2bfb`) —
  ColorControl (ratio-object round-trip), VecControl (X/Y/Z/W lanes with
  padding), TweenControl (descriptor + state link, upsert, remove, easing
  combo), event buttons, palette swatches; `PropView` carries `input_id`
  (+ linked tween) so the Inspector addresses real Input Links;
  verified end to end against the engine's dispatch arms and the location
  grammar `tweens.tweens['<id>'].value[0]` (Round 60).
- **Image decoder bomb fixed (`0dd577b9`)** — header probe with
  `image::image_dimensions` runs before decoding; the giant-PNG refusal is
  regression-tested with a 71-byte header; and **`groups/removeGroup` now
  drops its Modules and their prop/meta inputs through the shared
  `release_group` teardown**, with a regression test proving no module leaks
  into the next preset (Round 59).
- Native `modv` binary (`b1b8458e`) — single-process winit+egui+engine;
  egui-wgpu texture registration (zero readback, present wired into one
  encoder, Fifo UI + AutoNoVsync output); `modv-app` → lib + `modv-engine`
  and `modv` bins (native read model, snapshot round-trip, actions
  dispatch in-process, Scenes/apply/refresh paths all shipped behind
  `MODV_UI=native`). P0 review nits (default/reset/test name) folded;
  round-trip snapshot verified; P4-tier whole-scene transitions
  unavailable in native (instant switch, in-code documented). Round 56.
- `modv-ui` P0 framework spike landed (`87ee0787`) — egui shell,
  RangeControl, Scenes-grid cell, theme ported from CSS; every engine action
  the UI emits verified against the engine dispatch table; good scope
  discipline (P0 exits unverified; dead binary until P1). Round 55.
- Progress doc records ISF scene transitions accurately (`46f5bb18`) — new
  entries match the code; the stale gap entry at :277-278 survives one
  paragraph below the new one (Round 47).
- **ISF whole-scene transitions render natively in engine mode (`f591a108`)**
  — `TransitionEffect::Isf` classifies `startImage`+`endImage`+`progress`
  documents (the same predicate as the JS `ISFParser.inferFilterType`), the
  captured outgoing frame feeds `startImage`, the incoming main composite
  feeds `endImage`, failed compiles fall back to Fade mid-transition; bounded
  4-entry LRU of compiled transitions shared with the live-module runtime.
  This closes the ISF-transitions-infeasible finding end to end (Round 46).
- `dataTypes/createType` implemented (`6c627812`) — the Input Link Tween
  editor's `{type: "tween", args}` mints an engine tween and returns the
  descriptor to link to (the previously-silent path that broke tween links in
  engine mode). `groups/SWAP`/`modules/SWAP` acknowledged as no-ops (no swap
  buffer in the engine; A/B debug button becomes silently inert — see item
  33) (Round 44).
- Engine-owned NDI output and input (`5139d8a0`) — the big one.
  `ndi_runtime.rs` owns sender/receiver threads + transport trait, FFI module
  with SAFETY-commented unsafe, BGRA send conversion, receiver frames uploaded
  to `ndi:<id>` textures feeding ISF inputs, Texture 2D's texture prop and
  the Webcam source list; the mirror parity payloads verified against the JS;
  `engine-ndi.test.js` (22 assertions) replays engine deltas through the real
  mirror store. `Texture 2D` budget is now closed end-to-end (image, module,
  group, media, NDI). Round 43.
- AudioVideo engine-guard migration complete for `mediaStream/*` selection
  mutations (`76cd1440`) — `SET_SELECTED_VIDEO_SOURCES`,
  `SET_CURRENT_AUDIO_SOURCE`, `SET_VIDEO_SETTINGS`,
  `REMOVE_SELECTED_VIDEO_SOURCE` no longer reach the engine facade in engine
  mode (previously fragile pipeline of warn-once noise); legacy double-commit
  preserved; boot-time `restoreVideoSources` path handled too (Round 42).
- Engine-mode screen capture is live (`00b82246`) — `media/pick-screen`
  end-to-end (deferred portal dialog, PipeWire node → screen source,
  PortalStream held until stop, session/restoreToken payload, graceful
  cancel/no-portal/shut-down error codes, renderer IPC + panel wiring, the
  media root for ISF texture resolution applied per frame). The engine can
  now capture the screen with no Electron mediation (Round 40).
- **ISF image inputs bind real textures (`e4cf3e26`)** — the largest
  functional gap in the engine is closed for image-input shaders: image
  assets via `ImageTextures` (decode-once LRU, path-traversal guards), live
  media sources, module outputs, other groups' canvases, server-canvas
  fallback when unresolvable; self-reference binds the canvas; the filter
  alpha mask skips when at least one image input is external (assigned
  photo/video filters no longer erased over transparent groups); previews
  resolve assigned images too. `Texture 2D` remains inert. **New P2:**
  image decoder bomb — post-decode pixel check only, no header pre-read
  (Round 38).
- Engine-owned native Output Windows (`a121eaea`) — `windows/createWindow`
  opens a native winit window + wgpu surface blitting the shared main
  texture in the same encoder as the primary present; `windows/removeWindow`
  drops it; WM-close and failed-create paths both emit REMOVE_WINDOW; View
  menu stays enabled; `window.open` eliminated from engine mode (Round 39).
  Stale boot support claim documents MIDI/OSC still wired (closed `fe8a75ad`, Round 58).
- MIDI + OSC transport wired (`70540391`) — engine handles `midi/ADD_DEVICE`/
  `UPDATE_DEVICE`/`WRITE_DATA` with the JS device record/channelData shapes
  (same key scheme, flags, types; mirror replays unchanged); Web MIDI keeps
  forwarding unchanged; the MIDI panel is enabled in engine mode; OSC gets a
  real UDP listener (`start_messages`/`dispatch_packet_messages`), drained
  into `EngineState.osc.data` per frame before Input Links; the JS
  `DELETE_RECIEVER` typo fixed (legacy OSC remove now works); boot
  unsupported message corrected (Round 36).
- v15 Shadertoy scenes toned down (`1240db0c`) — one bass-linked control
  per scene, tight range, `smoothing: 0.6` declared in both default-Scene
  sources; generator defaults restored; `Chromatic Abberation`/`Glow` dropped;
  v15 remains unreleased; JS unit tests pin ranges and smoothing ≥ 0.5
  (Round 35). **The engine drops the declared smoothing entirely — P2, see
  item 28.**
- Monitor→monitor capture kill (Round 19 P1) — per-session `stopSession`
  closure in `system-audio-pipewire.js:164-269` kills only the owning
  session; a stale `track.stop()` can no longer reach module-scope
  `stopActive()`; `pcm.close()` idempotent; unit test asserts exactly one
  IPC stop for the replacement capture (`5851595f`; Round 22).
- `backgroundThrottling: false` is now `MODV_ENGINE=1`-gated
  (`window-prefs.js:154`) — legacy installs keep default throttling;
  F/Escape gained `event.repeat` and editable-target guards
  (`output-window-controls.js:27-30,46-56`), enforced at
  `window-handler.js:131` (`5851595f`; Round 22).
- Gallery post-await repaint guard (`GalleryItem.vue:245-257`) — a pull
  issued before blur can no longer paint the blanked tile (`5851595f`;
  Round 22). The leave→re-enter residual (same pull painting the *new*
  hover session) remains and is tracked in Round 22's PARTIAL list.
- The `SET_SYSTEM_AUDIO_STATE` stray commit site was migrated in `5851595f`,
  and the facade now skips the engine commit in engine mode entirely
  (`index.js:1253-1262`) — the engine catch-all warn-once is no longer
  reachable from audio-input changes.
- Gallery/Scene Point10 hover previews now read JSON booleans via the
  shared `scene_prop_number` (`compositor.rs:3151`) — `useAgc=false` reaches
  the live preview identically to the compositor (previously AGC was stuck
  on in tiles) (`66eb4860`; Round 27).
- Smoothing-id entry no longer silently leaks; `AudioFeatures.vue` captures
  the id before clearing, and the reopen path reads `args[1]`
  (`66eb4860`; Round 27).
- Rust audio-bands gained the near-silence gate with the JS pump's
  constants/placement (`0.01` / `0.001`, both arms required) — engine-owned
  capture can no longer boost dither to a full-scale bass reading
  (`8e41ef2c`; Round 27).
- Rust engine default Scenes are on v13 (`f467242f`): `DEFAULTS_VERSION =
  13`, and the embedded `default-scenes.json` ships the JS v13 content
  (Halftone Harbor Effects row empty; MBOX `dhue = 0`, no tween);
  `ensure_default_scenes` upgrades v12 installs in place (`f467242f`;
  Round 27).
- ISF `isf_FragNormCoord` stale docs — `docs/isf-translation-notes.md` now
  describes the shipped sidecar behaviour, and the JS-side notes no longer
  claim unconditional top-left (`d9e74945`; Round 27). The two Rust-side
  re-bake sites (`mod.rs:59-60`, `translate.rs:974-976`) remain open below.
- Module alpha / enable / blend commits (`117f0f0c` + `fc8371c4`) —
  `modules/UPDATE_ACTIVE_MODULE_META` aliases land in the engine and mirror;
  tests assert alpha 0.5 halves and enable skips the draw.
- Transparent background flattened to opaque black (`117f0f0c`).
- Camera path end-to-end (`2a149f16` + `28d90158` + `a1a9057f` +
  `fc8371c4`); both UVC cameras enumerate and render; project-prefixed
  library paths re-root under `media.path`.
- Gallery stutter (`f4548375`) — viewport-only polling + volumetric poster
  caching + 1 preview/frame + 640×360@30 ring; author-measured at 120 fps
  with 1 stutter/10 s.
- PipeWire Firefox-style monitor inputs (`beed7a20`) — picker enumerates all
  monitors; argv-safe; `getDefaultPlatform()` fix restores Linux native
  capture.
- Gallery hover previews composited over live output (`2c7995e0`) —
  hover-only polling; `withOutput` blends the module layer over the live
  main frame.
- ISF `gl_FragCoord` flip for single-pass texture-free generators
  (`0f392ff9`) — 26 corpus generators upright; multi-pass buffer/feedback
  generators (9) remain mirrored (see P2.15 below).
- Vertex sidecars compile and link for 36 shaders (`d6295e2b`) — vertex
  outputs drop as sidecars' WGSL; filter image inputs mask output alpha by
  the group canvas alpha; default Scenes v13 landed on the JS side (CMYK
  Halftone removed; MBOX Light Tunnel dhue fixed); sidecars draw on the
  legacy 6-vertex quad rather than the superset triangle (`69d1bba6`), and
  `isf_FragNormCoord` is now top-left.
- Hot Shower built-in (`54b48fc9`) — in-file attribution, notices row,
  corpus 353→354 consistent, procedural texture replacement, NC-SA flags
  preserved; no `texture()` references.
- Checkbox 1/0 flags coerced (`fc8371c4`).
- Group mutation parity — `update/removeAction`, `updateGroupName`
  (`a83cdd2d`/`57174b5b`/`fc8371c4`); tween engine (R-5–R-9); Scene
  transitions (Round 4); `removeScene` completes transitions first; Jacobi
  tone mapping; R-34/R-36/R-35/R-31/R-13/R-15/R-14/R-12/R-10/R-11/R-16/R-17
  all landed.
- PipeWire mid-stream loss (R-2 residual) — latch + renderer-pump fallback.
- Camera enumeration SIGSEGV (libcamera DeviceMonitor) — direct V4L2 +
  sysfs queries; verified on packaged build.
- Input Link 10 Hz throttle swallows settled writes — fixed (`1a94b31b`).
- Corpus baseline gate (≥ 95 %) asserted instead of `translated > 0`.
- `groups/updateGroupName`; `.deb` declares GStreamer base runtime packages
  (the `gstreamer1.0-pipewire` gap remains; see P2.21).

**P1**
**P2**
1. **Hover previews composite premultiplied bytes as straight alpha** —
   Round-20 finding; `5851595f` flattened only the `withOutput` path.
   **The native shell half is closed `75e7f116` (Round 117)** —
   `upload_preview` now un-premultiplies before the egui texture upload
   and the live GPU texture is explicitly skipped (premultiplied blending
   consumes directly). The **legacy JS sites** remain: the isolated
   (plain) Gallery preview, volumetric Point10 posters
   (`module_preview.rs:524, :590`), and the **Scene hover cells** (`Scenes.vue`
   painting via `drawEngineScenePreviewFrame` over bytes from
   `compositor.rs:2793`). Fix: un-premultiply on read-back or flatten
   wherever the engine ships premultiplied bytes to `putImageData`.
2. **GPU-overlay test claim is false** — the commit message and progress
   doc claim an "alpha-flatten assertion in the GPU overlay test"; no such
   assert exists (`module_preview_composites_over_the_live_output` only
   checks base row + one corner arrives; base is already alpha 255).
   The flatten path is untested at the Rust/GPU level.
3. **Scenes.vue hover cells are inconsistent with module hover previews** —
   module side flattens (withOutput path), cell side does not; same
   live-frame composition, different look per hover surface.
4. **Post-spawn resource death is invisible** — no `system-audio:error` IPC
   exists anywhere in src/; pw-record's death after spawn only `logger.warn`s
   (`main/system-audio.js:88-93`) and a code-0 exit isn't reported at all —
   the panel shows "(active: …)" forever with silence. The same class now
   applies to the engine OSC reader thread (`Err => break`, no callback,
   no mirror event; Round 36) with the "Listening" UI going stale over a dead
   socket. Fix: add an error channel + post-spawn death handling in both
   paths (or kill the UI surface that claims health it cannot prove).
5. **`/video/*` absolute POSIX paths false-positively re-root** — clips
   stored in a genuine user folder named `/video` are re-rooted with no
   override; document in docs/, or escape (add a leading `/`-preserved marker
   or an engine flag).
6. **ISF generator orientation (texture-free multi-pass)** — 9 corpus
   multi-pass/procedural-feedback generators still render vertically mirrored
   vs the WebGL legacy (`Life.fs`, `Circle Trails.fs`, `pixel-tunnel.fs`,
   `Etch-a-Sketch.fs`, `fractal-mage-tunnel.fs`, `Random Shape Blast.fs`,
   `Color History.fs`, `lopy-frac-tunnel.fs`, `fractal-mountain-tunnel.fs`).
   Fix: select bottom-left on "no externally-supplied images/audio/IMPORTED"
   — i.e. `Buffer`-only slots count as self-supplied.
7. ~~Gallery modules lack `dataTypes` sub-inputs~~ — **closed
    `11446c5a`** (Round 115) for make/apply/cleanup with byte-faithful port.
    Gallery modules themselves never minted inputs (asserted). The remaining
    sibling gap: `PropView::{label, step, component, enum}` — `$props`
    descriptors drop these so the Inspector degrades to raw prop keys with
    empty enum dropdowns.
8. **Dispatch-wildcard class — mostly closed; remaining surface** —
    `midi/*` (Round 36), `ndi/*` (Round 43), `fonts/*` (Round 32),
    `modules/SWAP` / `groups/SWAP` acknowledged no-ops (`6c627812`),
    `dataTypes/createType` implemented (`6c627812`),
    `scenes/saveCurrentAsScene` / Scene-grid cell Gallery actions implemented,
    `windows/createWindow` implemented, `media/pick-screen` implemented,
    `mediaStream/*` migrated to mirror. Still open: `outputs/UPDATE_AUXILLARY`
    and anything else the audit sweep hasn't read, `midi/learn`,
    `scenePreview/*` (the engine *has* the preview paths; only the store-action
    wildcard is unverified) — author-asserted, needs the audit sweep. Same
    fragility remains: `groups/UPDATE_GROUP_BY_KEY` `transitionGhost` bypasses
    the fc8371c4 coercion; `UPDATE_ACTIVE_MODULE` `meta` passes verbatim;
    poster eviction is clear-all at 32 (not LRU) and doubles 1 MiB clone
    per poll. The cell capture/clear paths carry correctness bugs (see
    items 26/27).
9. **Hostility/resize robustness — still-open P2s (from Rounds 2/3/9/10)**:
    PipeWire transient mirror-handle and rate-renegotiation edge (denylist);
    R-35 apply-time unblended frame + late WARN;
    ~~`.deb` missing `gstreamer1.0-pipewire`~~ (closed `39d7c1e3`,
    Round 98 — declared in `deb.depends` + unit-regression gate);
    `should_restart`/`poll_health` untested; non-looping file park
    unpinned; ~~`removeGroup` leaks group modules~~ (closed `0dd577b9`,
    Round 59 — shared `release_group` teardown with module deltas + tween
    input-link cleanup, regression-tested end to end);
    `groups/UPDATE_GROUP` all-or-nothing fail-closed undocumented.
10. **Point10 volume re-port residuals** — production always simulates the
    JS-full grid (~432 MB; no override); transparency contracts loosened to
    a 10% "some opaque" check; RollingFire's render coverage thin; GPU tests
    still skip vacuously without adapters; no CI gates them; `uSway` still
    unbound in both rooms; `half_to_f32` subnormal decode 4x small.
11. **Docs arithmetic / gates** — the e2e `engineParity.spec.js` registry
    count is pinned to `=== 432` (`dde600a4`, Round 93); same baseline as the
    Rust registry guard (`10ac008d`, Round 85); the ISF corpus counts are
    HEAD-verified in `isf-translation-notes.md` and `rust-hybrid.md`
    (`e9cc2349`, Round 95). What remains open here: `store_ops.rs` test
    constant is `36+15+27` as of v16 (unchanged, valid, not re-run); test
    counts still vary and Vitest quotes inconsistently;
    no end-to-end gate for the new `groups/*` mutation payloads in
    `tests/fixtures/engine/deltas.json`.

**P3**
12. **Native e2e CI runs X11 while the native shell is Wayland-first** —
    `TestDisplay::ensure` ranks `Xvfb` over `sway --headless`; the Docker
    image installs the former and no sway; prefer sway + `libwayland-*` in
    the rust image (item added Round 211).
13. `Preview.vue` debug panel — `reconcileCameraIndex` is a
    warn-only mismatch and no surface reports a camera that failed to
    start (shift-order, clamped to first); document the drop of
    Chromium-opaque-id selections on upgrade (silent). The stale
    "15 fps plenty" comment was folded into the new 30 fps note at HEAD.
14. `mirror/normalise` transient divergence on every sync — modules re-add
    `enabled:1`; only the subsequent normalised dispatch fixes it; nothing
    asserts the round-trip closes (OK on current engine).
15. `engine-module-meta.js` is redundant with engine-side normalisation —
    delete or keep only as a belt (tests won't catch the glue path).
16. Author-measured numbers carried forward as unverifiable claims:
    "120 fps @1080p in every scene", "p99 23.9 ms", "~0.02 ms mean preview",
    "22 requests in 6 s", "1801 Rust tests" vs CHANGES.md verdict.
17. Group-mutation regression trade: `groups/UPDATE_GROUP` fail-closed
    diverges from JS verbatim semantics — accepted behaviour; document the
    divergence in docs/progress.
18. ISF docs inconsistency (partially fixed by `d9e74945`) — three docs on
    `isf_FragNormCoord` disagreed; the translation-notes site is now correct.
    The Rust-side re-bake risk remains: `mod.rs:59-60` and `runtime.rs`
    claim sidecars are unbound report-only / never consumed, and
    `translate.rs:974-976`'s template re-bakes that text on every corpus
    report run. Fix those sites together. Check that `rave.exe`-class
    generators have at least one orientation pixel test before the next
    corpus bump.
19. `band-features.js` now unreferenced production module — 322-line unit
    suite exists, no production caller; delete or mark deprecated.
20. Smoothing-id end-to-end coverage missing — the `getSmoothingId` path is
    restored (`66eb4860`) but no e2e or integration test exercises the
    reopen flow against a live engine; the silence gate lacks the "strong
    FFT peak ⇒ not gated" assertion on the JS side.
21. `isFullscreenExitEvent` carries no repeat/editable guard itself —
    today's sole caller is guarded at `window-handler.js:131`; any future
    helper caller reintroduces the strobe. The unit suite only tests helpers.
22. Startup fallback — `withOutput` previews before the first frame render
    isolated and unflattened (`compositor.rs:2872`); the JS paints them as
    straight alpha; cosmetic and intended but unflagged in tests.
23. Re-enter repaint residual — `GalleryItem.vue:238-260`: the guard tests
    `state.hovered`, but a pull issued before a leave→re-enter can pass the
    guard and paint the *new* hover session with the old offset (self-corrects
    visually but is a residual window the fix does not close).
24. **`docs/native-parity-map.md`'s "2 h soak — waived" line is stale** —
    the 2 h soak ran 2026-11-18 on the packaged AppImage (431,816 frames,
    59.97 fps, clean deadline exit at the 7200 s mark; RSS plateau for the
    last 75 min; see Round 220). The author owns that file; the bounded-soak
    evidence table also undersells the run. Retire the waiver and cite the
    Round 220 figures.
25. **Committed `AGENTS.md` still documents the Electron-first workflow at
    `13e38c13`** — `make yarn`, the Node/Docker rule set and Electron
    entry points (the author's working tree carries the rewrite; fold it
    in). Also `docs/rust-full-rewrite.md:824` still claims the
    `tests/fixtures/engine/` snapshot/deltas are "Kept" (deleted in
    `eaac0a68`), and `docs/legacy-electron.md` lists `4a2a4164` without
    warning that its Electron lane is unbuildable (Rounds 228–233).




**P2 (raised in Round 64)**
## Round log

| Round | Commit reviewed | What it added | Notes |
|-------|-----------------|---------------|-------|
| 1 | `e8d78f18` (worktree, as handed to the reviewer) | Entire Rust workspace, JS integration, tests, build files | Worktree deleted externally mid-review; findings re-verified against `3548101f` by reading; docs/build drift noted |
| 2 | `e8d78f18..3548101f` (delta, from `git archive`) | Tweens, Scene hover previews, engine sync wiring, Point10 batch 5a (21/25) | All P1s from round 1 re-verified still present; new tween and preview findings R-5..R-11 |
| 3 | `3548101f..c088335d` (delta) | Point10 batch 5b (25/25), `--audio-source` engine-owned capture wiring, installer plumbing | New P1 R-31 (tunnel hoop depth); R-32..R-34 parity findings; packaging gap (no libpipewire in deb depends); docs/tests coverage gaps |
| 4 | `c088335d..36672a33` (delta) | Shader whole-Scene transitions (crossfade/dissolve/pixelate/glitch/wipe/circle), progress docs, `M7 — in progress` status | No P1; new R-35..R-36 fallback-path defects; transition window glue untested; fallback-Fade alpha snap |
| 5 | `36672a33..916586f5` (fix wave, 7 commits) | Review-driven fixes across all crates + JS | R-1..R-30 re-verified; residuals listed in the Round 5 section; **R-31..R-36 NOT FIXED**; new caveat: PipeWire mid-stream death still silent; module-count docs still wrong (424→428 unfixed) |
| 6 | `916586f5..73729fdb` (delta) | Point10 textures shipped for packaged builds | Assets fix clean; `.deb` still undeclared `libpipewire-0.3-0` |
| 7 | `73729fdb..9afa21dc` (delta) | ISF “351/351” (four new translation passes, no overrides), gallery effect previews over a test card | Passes verified per-shader; caveat: v002 Light Leak validates with its rotation silently dead — “351/351” ≠ “351/351 render correctly”; gallery fix clean |
| 8 | `9afa21dc..89f8acc9` (delta) | R-31/R-35/R-36 fixes, `.deb` PipeWire dep, Boa expressions (WS-12), JS plugin host | R-31/R-36/deb verified fixed; R-35 partial; new P1 (expression evaluation budget); plugin-host P2s; WS-12 docs still contradict the shipped feature |
| 9 | `89f8acc9..418c5cc8` (delta) | GStreamer video capture core (WS-10), docs commit | No P1; GStreamer FFI/pool/escaping clean; packaged engine undeclared GStreamer deps; start timeout aborts slow prerolls; stalled sources silent; screen caps unbounded |
| 10 | `418c5cc8..e88bbd07` (delta) | Engine-owned media sources (renderer + engine), WS-10 end-to-end | **2 new P1s** (media path rooting breaks files on POSIX; TextureControl lifetime freezes file video); engine/renderer contract tight (sender validation, error codes, wire shapes all verified); MediaHub P2s (loop:false replay, order-drop on failed start, thread-blocking restart, no cap); camera-order divergence repointing Webcam props |
| 11 | `e88bbd07..28d90158` (delta) | Webcam camera index → camera-like sources (files excluded); Point10 SmokeRoom/DvdRoom re-ported to a GPU fluid sim + raymarch | Camera-index fix verified (engine side); R-28 fix verified; R-32 fix verified; **R-33/R-34 still open**; volume rewrite deleted RollingFire/GeometricTunnel GPU tests and loosened transparency contracts |
| 12 | `28d90158..270a0003` (delta) | RollingFire re-ported to the volume sim (R-34 fix); BallOfFire registered as 26th scene; docs counts | R-34 verified fixed (CPU-gated only — no GPU silent test); BallOfFire verified end-to-end; straight-alpha scene targets composite as premultiplied (pre-existing engine-wide); SmokeRoom/DvdRoom never bind `uSway`; GPU tests pass vacuously without an adapter |
| 13 | `270a0003..e916f42a` (delta) | V4L2 camera enumeration (libcamera SIGSEGV); media-path re-root; inspector-unmount fix; PipeWire mid-stream latch; expression cap/denylist; group mutation parity; R-34 silent-GPU test; Input Link mirror flush; `.deb` GStreamer | R-2 closed, camera crash closed, mirror flush closed, SmokeRoom R-33 closed (**DvdRoom NOT**); **expressions P1 NOT closed** (denylist bypass classes; no budget), **library-path bug NOT fixed** (`/default/video/*` still fails; Windows rejected); `groups/removeGroup` leaks group modules; `.deb` missing `gstreamer1.0-pipewire` |
| 14 | (user-reported) | alpha/enable dead on modules, background UX, cameras dead, stutter | Root causes identified: `modules/UPDATE_ACTIVE_MODULE_META` dropped silently by the dispatch wildcard; engine window always visible + transparent pixels; blocking full-frame preview readback stalls Fifo pacing; camera caps truncation/group-permission candidates. Author ran the current build at 17:08–17:12: `camera:/dev/video0` starts (1280×720@30); a second-source attempt fails `Device '/dev/video2' is busy` (log) |
| 15 | `e916f42a..fc8371c4` (fix wave) | Module alpha/enable commit aliasing; opaque-black present; restored webcams; project-media paths; Gallery viewport-only polling; volumetric Point10 posters; 600-frame snapshot; ring 640×360@30; mirror replays with inputs and ISF generator orientation | Author-verified 120 fps @1080p, p99 23.9 ms, 1 stutter/10 s; **3 ISF multi-pass generators orientation not fixed**; dataTypes sub-inputs still missing; group-by_keyGhost bypass; dispatch-wildcard class unfixed; docs/ counts |
| 16 | `fc8371c4` | Verification round of the user-reported defect fix wave | All P1s VERIFIED fixed (alpha/enable/background/cameras/media-paths/stutter); remaining P2s tracked in ‘Outstanding items’ |
| 17 | `f7b58951..e384269a` (delta) | fullscreen output window (F / double-click / Escape); `backgroundThrottling: false` (rAF pump survives occluded output window); GeometricTunnel surface maps (unbound textures) + base tint linear fix; docs: module-parity audit | Output-window legacy wiring carries an event.repeat hole and no isContentEditable guard; **round-17 P2** — `backgroundThrottling: false` unconditioned in legacy mode; **module-parity audit verified by spot-checks** (ISF image inputs bind group canvas; Texture 2D inert; Cube/Earth/Sun absent; transitions unusable) |
| 18 | `e384269a..0f392ff9` (delta) | ISF `gl_FragCoord` Y-flip for texture-free generators (26 corpus, incl. The Pool Rooms / st_Ms2SD1) | VERIFIED fix; **9 multi-pass generators and 42–47 textured `gl_FragCoord` docs stay top-left**; progress/`isf-translation-notes.md` claim 31 fixed; docs still claim unconditional top-left |
| 19 | `0f392ff9..beed7a20` (delta) | Firefox-style “Monitor of …” entries for every PipeWire sink (`pw-dump` discovery + `pw-record --target` selection); `.deb` gains `pipewire-bin`; `getDefaultPlatform()` fix; engine no longer sees `SET_SYSTEM_AUDIO_STATE` | **P1** monitor→monitor switch silently kills just-created capture (stale-stop wrapper reassigned `active` before the stale system streams are stopped; fresh-boot and mic↔monitor work); **P2** pw-record death after spawn is invisible (no error channel, panel shows (active) forever); one `AudioVideo.vue` `SET_SYSTEM_AUDIO_STATE` call site missed by the fix |
| 20 | `beed7a20..2c7995e0` (delta) | Gallery hover previews composite over the live output (new `withOutput:true` preview flag); hover-only polling removed ~150 preview requests/s | **Both P2s are live-app bugs** — premultiplied bytes reach putImageData as straight alpha (tiles render darker than the live frame), and the post-blur stale repaints after the user moves the pointer away |
| 21 | `2c7995e0..254c3cb7` (delta) | Helix Eye + Traced Tunnel built-in 3D Scenes; `THIRD_PARTY_NOTICES.md`; README licence note | Attribution flaw — notices claim completeness but miss 4 Shadertoy-derived NC-SA modules and the entire Plane9/Joakim Dahl family (Boxy, Earth, HollowCube, LightTunnel, MirrorCube, SoundCube, SquareTunnel, Sun, Wormhole); st_Ms2SD1 misattributed; README still proclaims bare MIT badge |
| 22 | `254c3cb7..5851595f` (delta) | review-driven fix: hover preview flattened over opaque black; post-blur repaint stopped; monitor→monitor capture-stop fixed; `SET_SYSTEM_AUDIO_STATE` delegated to engine guard; `backgroundThrottling` gated on engine mode; legacy output popup guards (`F`/Escape repeat/editable) | **Hover-preview alpha fix leaves isolated/poster/Scene-hover-cell paths unflattened**; commit message claims an alpha GPU test that doesn't exist; Windows-classic call sites in `AudioVideo.vue` still bypass the engineMode gate |
| 23 | `5851595f..d6295e2b` (delta) | ISF vertex sidecars compile+link (36 corpus); filters preserve transparent group canvas; default Scenes v13 (CMYK Halftone removed; MBOX hue fixed) | **Rust engine defaults never migrated** — `DEFAULTS_VERSION = 12` + stale JSON seeds vs JS v13; **sidecar `isf_FragNormCoord` is bottom-origin in a top-left pipeline** — group canvas samples upside-down vs legacy; Vertex Manipulator.vs impossible on the shared 3-vertex buffer; sidecar corpus gate is `>= 30` under adapter-less CI |
| 24 | `d6295e2b..54b48fc9` (delta) | Hot Shower (Kali) NC-SA 3D Scene; in-file attribution/notices row added; corpus 353→354 | **Attribution completeness unchanged** — three more NC-SA ports landed while the Plane9 family, the 4 mojovideotech NC-SA shaders and `st_Ms2SD1` go un-noticed; README MIT badge persists; stale "351" counts in progress doc |
| 25 | `54b48fc9..72c49c1d` (delta) | `meyda/getSmoothingId`+`removeSmoothingId` arms; u_bass/input links read grid means again; JS silence gate; Point10 props accept JSON booleans | **P2** gallery Point10 preview path still reads booleans via `as_f64` (AGC stuck on in tiles); **P2** Rust audio-bands has no silence gate so engine-owned captures can boost dither to full scale; UI smoothing-id callsite dispatches `null`, dropping it |
| 26 | `72c49c1d..69d1bba6` (delta) | Sidecar vertex shaders draw on the legacy 6-vertex superset quad (positions/winding byte-identical to `ISFGLProgram.bindVertices`); `isf_FragNormCoord` now emits 1-(y+1)/2 (top-left) | Both Round-23 sidecar P1s closed *correctly*; **P2** stale doc sites keep the old contract in 4 files (next `cargo test` re-bakes it); `compiled >= 30` is not 36/36; orientation gate adapter-gated |
| 27 | `69d1bba6..f467242f` (delta) | Preview props share `scene_prop_number`; smoothing-id leak in `AudioFeatures.vue`; Rust silence gate (dither/-30 dB/both-arms pinned); translation-notes sidecar section; engine default Scenes v13 (JSON byte-matches JS v13) | All four commits are the author landing review-driven fixes; **P2** two Rust-side stale doc sites remain (`mod.rs:59-60`, `translate.rs:974` template re-bake risk); smoothing-id path still has no end-to-end test |
| 28 | `f467242f..c06bfb1f` (delta) | `scenes/saveCurrentAsScene` implemented (dispatch arm + snapshot + persist + mirror delta) — the Scene-grid save button no longer silently no-ops in engine mode | Verified end-to-end by reading (dispatch wildcard → action → `save-file what="scene"` → mirror replay); transition-order swap converged; Rust persists the scene list where JS does not (strictly better); **P3** unit test never pins the `scenes/ADD_SCENE` delta; no engine e2e for the save flow; weak `is_number()` timing assertions |
| 29 | `c06bfb1f..7ac215bd` (delta; covers `aa2814a6` + `7ac215bd`) | Six Scene-grid column/list actions implemented (`addEmptyScene`/`moveScene`/`setSceneEnabled`/`updateSceneName`/`updateSceneTiming`/`resetSceneTiming`); progress doc names remaining wildcard gaps | Ports are JS-faithful branch-by-branch (identity-return → false; unique-name fallback; ADD_SCENE upsert mirror-safe); **P2** the named six cell/Gallery actions still silently dead — same wildcard class; author documents the gap honestly |
| 30 | `7ac215bd..8da469e0` (delta) | Helix Eye / Traced Tunnel / Hot Shower declare ISF float inputs (LABEL/MIN/MAX/DEFAULT) — linkable from links/MIDI/tweens/expressions | Defaults byte-verified identical to deleted consts (bit-exact at 1.0 multipliers); corpus is shared JS/Rust (no divergence); unit test asserts float typing/ranging/reference; **P3** no runtime-level "inputs bound" test |
| 31 | `8da469e0..236a3bf9` (delta; covers `1236c374` + `236a3bf9`) | Grid cell/Gallery actions implemented (clearSceneCell/captureSceneRowToScene/createSceneCellGroup/disableGalleryItemModules/focusGalleryItem/orderByIds); `copySceneCellToScene` (+760 LOC scene_cells.rs, 5 tests) | Faithful ports verified branch-level; **P2** clearSceneCell never empties live Group `modules` (JS issues REPLACE_GROUP_MODULES=[]) — next cell capture writes a corrupt cell; **P2** captureSceneRowToScene positional fallback can capture another Scene's row (JS returns null); remaining wildcard: vestigial `dataTypes/createType` |
| 32 | `236a3bf9..457c846a` (delta) | `groups/duplicateModule` (existingModule path, props/meta copy, key-remapped input ids, links+expressions carried); `beats/SET_BPM_SOURCE` source gating; `fonts/SET_LOCAL_FONTS` bare-array echo | Verified faithful to JS refs; progress doc honestly updated; **P3** in-group miss diverges (JS position 0); sub-input duplication is moot until P2 #12 lands; remaining wildcards author-asserted, audit sweep recommended |
| 33 | `457c846a..5ebd4a24` (delta) | `Point10 Old Movie` retired from JS + Rust catalogues; Boxy Arcade Effects row swapped to `Sepia Tone` ISF filter; defaults → v14 with in-place refresh | Retirement faithful and symmetric; both default-Scene sources swapped identically (`props: {}`); v13→v14 in-place refresh correct (`REFRESHED_DEFAULT_SCENES`); two-versions-behind not refreshed; **P3** progress doc headline now self-contradicts (429 vs 431 arithmetic); `store_ops.rs:4975` still asserts `37 + 15 + 18` |
| 34 | `5ebd4a24..56f56364` (delta) | Three sparse Shadertoy NC-SA Scenes seeded as v15 (`Ember Storm` Hot Shower+Bloom, `Prism Tunnel` Traced Tunnel+Bloom+Chromatic Abberation, `Sentinel` Helix Eye+Glow+Bloom); single sparse Effects group, no Background/Foreground | JS↔Rust scene bodies byte-identical (field-walk; only float/int noise); all props in-range of shader INPUTS; linked controls resolve; v14→v15 seed lane tested; **P3** progress doc not updated; e2e pins were stale at v14 (fixed now, constant-pinning suggested) |
| 35 | `56f56364..1240db0c` (delta) | v15 Scenes toned down — one bass-linked control each, tight ranges, `smoothing: 0.6` declared, generator defaults restored, Glow/Chromatic Abberation dropped | JS data and generator cures verified byte-consistent; unit tests pin ranges and smoothing ≥ 0.5 on the JSON; **P2** engine drops smoothing entirely at scene-build time (`create_bass_link` emits `[feature]`) — the calm only holds in the legacy renderer |
| 36 | `1240db0c..70540391` (delta) | MIDI/OSC transport wired — engine `midi/*` device record/channelData stores + deltas; OSC real UDP listener (start_messages/coalesced) drained per-frame into `EngineState.osc.data`; links read `osc.data`; legacy `DELETE_RECIEVER` typo fixed | End-to-end verified by reading (setup-midi unchanged; BPM via SET_BPM_SOURCE; mirror payloads byte-consistent; OSC delta replays; lodash.set shape replicated, bounded) | **P3** no live UDP test; OSC reader thread death silent (item 6 extended) |
| 37 | `70540391..145005ec` (delta) | Engine-owned xdg-desktop-portal screen capture (ashpd ScreenCast → PipeWire node) | Groundwork only — zero call sites at HEAD by design; 9 unit tests; pure-layer verified; **P3** not reachable yet (`media/pick-screen` wiring pending); progress doc not updated |
| 38 | `145005ec..e4cf3e26` (delta) | ISF image inputs bind real GPU textures — image assets (LRU, guards), live media, module outputs, self-reference fallback, external-masked alpha; previews resolve assigned images | **Closes half of outstanding P1 #2** (ISF half); `Texture 2D` still inert; **P2** image decoder bomb (no pre-decode header check) |
| 39 | `e4cf3e26..a121eaea` (delta) | Native Output Windows — menu/dispatch/native window shared present encoder; SET_WINDOWS mirror sync; WM/remove/failed-create all remove cleanly | Verified by reading (encoder blit, `is_main` event dispatch, AutoNoVsync, input focus per window); **P3** stale boot warning re-lists wired MIDI/OSC |
| 40 | `a121eaea..00b82246` (delta) | Engine-mode screen picker live — `media/pick-screen` (portal dialog on worker, deferred reply, PipeWire node → screen source, session alive until stop; 5 min IPC guard; cancel/no-portal/shutdown-safe codes); Screen Capture panel engine path; `media/setMediaDirectoryPath` wired to frame loop | End-to-end verified by reading (queue→drain→start→store→stop→drop orders); project media root flows into compositor; **P3** notices still ignore Rust-binary crate graph; `.deb` still missing `gstreamer1.0-pipewire` |
| 41 | `00b82246..7f501bac` (delta) | Docs record ISF textures/output windows/portal capture accurately; `media/pick-screen` validates `sourceType` (`invalid_params`) before portal spawn; no dialog on bad input | Input validation + test verified; docs claims byte-checked all accurate except the edited MIDI/OSC paragraph (regression-ward note); item 30 boot-warning claim persists |
| 42 | `7f501bac..76cd1440` (delta) | `mediaStream/*` selection commits skip the engine facade in engine mode (mirror-only); legacy double-commit kept | Item 6 closed for the mediaStream family — the six direct-commit sites enumerated and guarded; **P3** one crumb left (`outputs/removeWebcamOutputForDevice:821` dispatch unguarded) |
| 43 | `76cd1440..5139d8a0` (delta) | Engine-owned NDI in/out — `ndi_runtime.rs` (transport trait, BGRA send, receiver threads, drop-oldest queue, shutdown order), ISF/Media-source/Texture-2D texture binding, aux outputs, NDI panel availability states | Beyond parity: brings a whole NDI transport into the engine; unsafe isolated with SAFETY comments; **Texture 2D now samples real textures** (item 2 closed); **P3** no live e2e possible; sender readback ~8 MB/frame is new grandiose-nomic cost |
| 44 | `5139d8a0..6c627812` (delta) | `dataTypes/createType` forwards `type: "tween"` to the tween store — Input Link Tween editors now work in engine mode; `groups/SWAP`/`modules/SWAP` acknowledged no-ops | Verified end-to-end (JS chain `dataTypes → tweens/createTween` faithful; unit test pins descriptor + delta + null-branch); **P3** ABSwap.vue still enabled-but-inert; texture arm unplumbed |
| 45 | `6c627812..f5c243b6` (delta) | Progress doc entries for NDI, tween editor, mediaStream UI-plane state | Claims verified against code (`MODV_NDI_LIB` honoured; loader path + panel notice accurate); **P3 ×2** docs inconsistencies persist unchanged (items 30/31) |
| 46 | `f5c243b6..f591a108` (delta) | ISF whole-scene transitions render natively — `TransitionEffect::Isf` (startImage/endImage/progress predicate), captured→outgoing, composite→main, compile-failure falls back to Fade mid-frame; performance-mode pick includes every ISF transition | **Closes the ISF-transitions-infeasible finding**; picker and engine now agree; classification faithful to JS `ISFParser.inferFilterType`; **P3** progress doc contradicts the feature (verify-by-claim retracts needed) |
| 47 | `f591a108..46f5bb18` (delta) | Docs: ISF transition entry recorded in progress notes | Claims all verified accurate; **P3** the stale gap entry survives one paragraph below the new entry — the contradiction is now inline |
| 48 | `46f5bb18..f7d35db0` (delta) | `three` renderer and `Cube` removed from both runtimes; worker glob clears; Gallery category/aliases removed; e2e + guard tests | **Closes the last parity gap**; removal claims verified by grep; **P3** the Rust ISF category normaliser still maps `three`(→ dead side, tests pin stale) |
| 49 | `f7d35db0..a6ca39c0` (delta) | Screen blend parity — `BlendMode::Screen = 28`; Porter-Duff bounds 15..=27; graph golden RED over BLUE → 255,0,255; JS menu offered after Multiply | The 73 default-scene "screen" groups no longer degrade to normal; **P3** stray literal `\n` in blend.rs:114 doc comment (rustfmt-immune) |
| 50 | `a6ca39c0..cbb24d8a` (delta) | Point10 `Sun` and `Earth` ported to wgpu; registry 26→28; prop schemas parity-verified line-by-line; packaged assets resolve via `$MODV_ASSETS_DIR` | Both scenes Gallery-registered; no default Scene references (correct); **P3** progress-doc "429/432 (missing Sun, Earth, Cube)" line doubly stale |
| 51 | `cbb24d8a..34528ff6` (delta) | `Point10 Geometric Tunnel` retired both sides; v16 DEFAULTS_VERSION; the five Scenes retuned in place (Square/Light Tunnel/Color Cube/Wormhole/Spiral Particles), SSAO pass deleted | JS↔Rust scene documents field-walked identical; refresh semantics verified correct; REMOVED_DEFAULT_SCENE_MODULES both lanes |
| 52 | `34528ff6..a918598f` (delta; covers 4a547b16 + 0fc00538 + 2 prettier) | `scene-groups.js` (null-tolerant modules counter) + `modules/init` degrade — the Scenes panel no longer unmounts on null group holes; e2e asserts panel survives | Real crash fix with unit+e2e coverage; **P3** missing-definition still silent |
| 53 | `dd8914c2..cf9be174` (delta) | Volume curl-noise precompute now runs with real uniforms (grid dims/world/TURB_FREQUENCY stamped before dispatch; snoiseVec3 third-component domain-scale correction) | Four volume scenes restored to smoke-turbulence rather than radial puff; GPU regression test reads curl field; **P3** adapterless CI passes vacuously |
| 54 | `cf9be174..6e5cf6be` (delta) | clippy/rustfmt for Rust 1.100 (`FromStr` for `BlendMode`, inherent from_str gone; `as_chunks`; else-let chains) | 1951 workspace tests green per author; **P3** the Round-49 stray `\n` doc comment survives fmt |
| 55 | `6e5cf6be..87ee0787` (delta) | `modv-ui` P0 spike — egui shell, RangeControl (402 LOC→widget), Scenes cell (hover preview/timing/Ctrl+drag/context menu), CSS theme, every emitted action verified against the engine dispatch table | P0 exit criteria unverified (no binary yet; flags hand-set); action vocabulary alignment verified correct; **P3** dead write-only `SceneGridState.context`; kittest test name overpromises (no click performed) |
| 56 | `87ee0787..b1b8458e` (delta) | Native `modv` binary — one process winit+egui+engine; zero-readback preview texture; engine output window shares encoder; snapshot round-trip; Electron default kept | Output content correct by reading (texture registration, FrameInfo row_fades path works, egui-wgpu textures-delta contract honoured, EOF-linked release); P0 GO evidence still measured-claim territory; **P3** whole-scene shader transitions deferred to P4 (instant switch, doc'd) |
| 57 | `b1b8458e..ea5fe63a` (delta) | Native hover previews — real isolated engine renders for Gallery/Scene cells (150 ms poll, 1-request/frame cap, 32-entry LRU, premultiplied→opaque flatten), Scenes panel filled (perf bar, timing/row-name editors, rename, enable toggle, move left/right, empty-cell group create) | All actions verified against the dispatch table; LRU matches hybrid policy; flatten is the Round-38-class premultiplied fix; reuses hybrid `scene_preview` intact (no duplicated renderer) |
| 58 | `ea5fe63a..fe8a75ad` (delta) | Stale support claims corrected — boot message lists only JS plugins; transition gap paragraph + MIDI/OSC paragraph fixed; 429/424→432 both sites + module-parity pointer; store_ops bound tightened (36+15+27); ISF `three` aliases dropped; stray `\n` fixed | **Closes the stale-boot-claim/ISF-aliases/`\n` findings**; 27 transition-capable ISF shaders byte-counted at HEAD to validate the rewritten gap text (exactly 27) |
| 59 | `fe8a75ad..0dd577b9` (delta) | Image decoder bomb — header probe (`image::image_dimensions`) runs before decode; 71-byte PNG-bomb refusal regression-tested. `groups/removeGroup` drops its Modules + their prop/meta inputs through shared `release_group`; regression test end-to-end | **Closes the decoder bomb and the removeGroup module leak**; fix order JS-faithful (tween descriptor cleanup kept; deltas replay order preserved) |
| 60 | `0dd577b9..060e2bfb` (delta) | Native Inspector: colour (ratio-object round-trip), vec (X/Y/Z/W lanes), tween editor (descriptor+link, upsert, easing); `PropView` carries `input_id` + linked tween | All paths verified end to end against engine dispatch sites and the `tweens.tweens[<id>].value[0]` location grammar (pre-registered parse-match); **P3** frames model field unused; no P2 |
| 61 | `060e2bfb..73a697ba` (delta) | Native: texture picker (engine-id sources), group alpha/inherit/clear/pipeline/rename/reorder/remove, module enable/duplicate/remove/reorder, global shortcuts (Ctrl+S/Enter/K/1..9/F1/Escape) | All arms verified in dispatch table; **P3** blend combo lists `add` (no engine token — warns and no-ops); producer for texture_sources in-flight and uncommitted (unverified feature); shortcuts correct |
| 62 | `73a697ba..10562230` (delta) | `docs/rust-full-rewrite-progress.md` — P0–P4 tracker, honest about unverified P0 evidence and in-flight fixes | Doc accurate on every point I checked; **P3** names untracked `window_state.rs` as existing-today (it does not at this HEAD) — claim-vs-code class again |
| 63 | `10562230..ab85acc8` (delta) | Native focused-input link editor — `link_row`/`link_editor_window` widgets, audio/state/MIDI/OSC presets, JS `makeLink` payload shape (`state`+`source`), all dispatch arms verified | Widgets match engine dispatch; **P3** `snapshot.input_links` never populated at this HEAD — the commit advertises "reads the real link record" that is unavailable until in-flight populator commits; second instance of the pattern |
| 64 | `ab85acc8..b765b5c4` (delta) | Native Media panel (cameras + test pattern + statuses, V4L2 enum per frame), `window_state.rs` port (atomic, same JSON layout as Electron), InputLink snapshot populated, texture_sources populated | Round 63 link-snapshot gap closed; Round 61 texture-source gap closed; V4L2 enum is per-frame cost; **P2** native is deaf: no AudioFeed anywhere — Input Links always evaluate to 0; window-state clamp compiled but never called |
| 65 | `b765b5c4..e28892ff` (delta) | Tempo window (source combo, roll-over-2 s tap with source promotion), rolling FPS sparkline, bass/mid/treble meters in the status bar | JS-faithful tap promote / 2 s rolling clamp / SET_BPM_SOURCE wiring; meters paint nothing observable (**P2**, Round 64: native is deaf); **P3** tap promote leaves stale `tap` source when user stops tapping |
| 66 | `e28892ff..bda55128` (delta) | `clearSceneCell` empties live Group (phantom ids + `REPLACE_GROUP_MODULES`), `captureSceneRowToScene` drops positional fallback (stamped only), default Scene smoothing mints `getSmoothingId` (uuid_v4 seeded 0) emitting `[feature, id, amount]` | **Closes outstanding items 25, 26 and 27** (all three regression-tested round-trip); smoothing registry now matches JS `addSmoothingId` semantics |
| 67 | `bda55128..a28162d1` (delta) | Ctrl+K command palette — Scenes list, performance/Group/Scene/module actions, window commands, arrow-key select, Enter runs | Palette renders and handles input correctly; **P3** `ui/showShortcuts` dispatched into engine catch-all (no-op); **P3** Ctrl+K focus goes to Gallery search first (shared `search_focus` bug) |
| 68 | `a28162d1..6fd8352c` (delta) | Native shell golden snapshot — representative Groups/Scenes/Gallery/Inspector snapshot with committed `shell.png`; egui_kittest `snapshot`/`wgpu` features | First visual regression net; P0 all-green fixture (honest: goldens only pinned to a "healthy" state) |
| 69 | `6fd8352c..594ea38e` (delta) | `AGENTS.md` native-UI section — make native / MODV_UI=native / `modv --headless`, window-state compatibility, engine-test args | All claims verified accurate against the tree (GUI args, MODV_UI switch, headless paths, make engine-test); **P3** no mention of the deaf native shell (Round 64) |
| 70 | `594ea38e..9d3c1fc4` (delta) | Native audio restored (`AudioFeed` PipeWire→Input fallback, pump before Input Links), V4L2 cached at 1 Hz, saved window clamps to primary, preview-error re-report cadence, `lighter` token, palette focus/shortcut fixes | **Closes Round-64 deaf-shell P2** plus five R61/R64/R67 P3s (Lighter token, preview cadence, camera enumerate rate, primary-monitor clamp, palette focus race) |
| 71 | `9d3c1fc4..4f41c87c` (delta) | Native shader transitions — capture_pending_transition + prepare_scene_transition with full fallback-to-Fade (lost/pipeline/failed), evidence-based `SceneTransitionInfo` fed into compositor | **Closes the Round-56 P3 native transition gap**; ordering places capture before sync_engine/render — better than the hybrid; identical fallback discipline as Round 46 |
| 72 | `4f41c87c..590b1fe0` (delta) | `--apply-scene <name|id>` for reproducible P0 traces; seeding tests both lookup paths | Seed-ordering correct (seeds before apply); **P3** flag scoped to `modv`, accepted by `modv-engine` via shared CLI (vestigial there) |
| 73 | `590b1fe0..edd93b55` (delta) | Info (adapter/frame/audio/scenes/errors) and Plugins (Boa "not yet landed", grab-canvas disabled) side tabs; snapshot re-baselined | Info reads straight from the snapshot (no extra state); Plugins honest; **P3** `audio_active` label consistent fixture-only |
| 74 | `edd93b55..71f707ad` (delta) | Progress doc records P0 verdicts: 138–143 fps reference GPU, 56.9 fps lavapipe with Lone Chrome, ~157 MB RSS; full P0 acceptance text | Claims match verified state (audio-source note accurate; RSS unverified here); **P3** the still-stale `denylist` line at :276 now contradicts the same paragraph's new engine-green attestation |
| 75 | `71f707ad..f6d88439` (delta) | First native parity map — every legacy spec group mapped to Rust/kittest/manual/waived | Map accurate at HEAD; **P3** paths aren't test-name-specific; regenerated at P8 start |
| 76 | `f6d88439..b84553ac` (delta) | Expressions AST allowlist (the Round-13 P1) — Boa parser validator, Rust-owned handle map, 54-bypass test suite; no wall-clock budget (crate-private Boa paths documented) | **Closes the expressions P1**; **P3** progress doc's "denylist" line stale in the same file; pre-parse 2048/512/32 structural caps |
| 77 | `b84553ac..0d83d639` (delta) | Native preset load/save — `--preset <path>` at startup, File → Save preset, round-trip test | Round-trip verified end-to-end at the store level; **P3** preset write is not atomic (tmp+rename absent) — could corrupt on crash |
| 78 | `0d83d639..0cd68d06` (delta) | Clippy/fmt sweep; unused manifest deps removed (`uuid`, `libc` from modv-ipc, `memmap2`) | All claims verified against src/; **P3** `is_empty` has no live caller |
| 79 | `0cd68d06..67f1097e` (delta) | Verification ledger for the full workspace (459/414/25+3+1); the one Point10 GPU failure recorded openly; known P5–P9 scope documented | Every count consistent with HEAD; **P3** the Point10 GPU test is still red — not flagged in release-readiness checklist anywhere yet |
| 80 | `67f1097e..539854bb` (delta) | Native MIDI — midir input, 1 Hz hot-plug diff, Note On/CC decode (cc/noteOnLatch), 24-PPQN clock → BPM via `beats/setBpm`, MIDI side tab with per-device flags | Demon; dispatch verified at `midi/*` arms; BPM path mirrors JS; **P3** channel-0-only status-byte match faithful to legacy bug; snapshot re-baselined |
| 81 | `539854bb..4522f73f` (delta) | MIDI panel finished (read-model snapshot export, `midi/WRITE_DATA` integer serialisation for note-on readback, tests realigned); user ISF folders registered via `ModuleRegistry::build_with_extra` (`<userData>/media/isf` + `<project>/isf`, corpus wins collisions) | Corpus-first ordering verified; test `user_isf_dirs_find_flat_and_project_folders` pins all three cases; **P3** `MODV_USER_ISF_DIR` replaces auto-discovery; no hot-reload (P6) |
| 82 | `4522f73f..19a71602` (delta) | ISF hot-reload — `IsfWatcher` (notify, 500 ms quiet debounce, `.fs` filter) + `Compositor::set_registry` (failed caches cleared) + `Store::reload_modules`; user ISF now updates without restart | Full wire-up verified; live demo claimed by author (dropped ISF => count+1); **P3** whole-registry rescan on every save, no `.vs` trigger |
| 83 | `19a71602..11d9c5c3` (delta) | AGENTS.md native section updated (hot reload/MIDI/preset now listed as wired); progress doc marks P6/P7 Partial, P8 Started | Every claim matches HEAD (commit-level verified); **P3** P8 parity-map not refreshed for the four newest native commits |
| 84 | `11d9c5c3..08254041` (delta) | Verification ledger trued up — reference-GPU claim now "138–143 fps unobstructed; 59.7 fps post-window-manager-occlusion (Fifo)"; lavapipe 57.5 fps with content; 422 `modv-app` tests; 62 seeds counted at HEAD | Claims verified consistent; the Point10 GPU failure still named openly; **P3** no perf-regression gate on the 59.7 fps run |
| 85 | `08254041..10ac008d` (delta) | Exact registry parity guard — `registered_module_count_matches_the_parity_baseline` asserts 36+15+27+354 = 432 (corpus-bound, skip-if-absent) | HEAD counts directly verified (36/15/27/354/432 all re-checked); **P3** e2e `engineParity.spec.js` still asserts `> 300` (item 12 narrowed) |
| 86 | `10ac008d..40f12406` (delta) | Native AppImage — `make appimage-native` (AppRun, MODV_ISF_DIR/MODV_ASSETS_DIR staged at `usr/share/vmodv`) | Packaging shape correct, matches `resolve_isf_dir`; **P3** appimagetool `continuous` fetched un-pinned; NATIVE_VERSION hand-maintained |
| 87 | `40f12406..1d80f5f7` (delta) | Native runs until window closes by default (`--bench-seconds` explicit-only); status bar FPS explicit + `frames total` counter label; snapshot re-baselined | **Closes the 10-second-UI-exit bug**; unit test pins all three branches |
| 88 | `1d80f5f7..323a0eb5` (delta) | Status bar shows live BPM (`BPM {:.0}`) replacing frames counter | Snapshot re-baselined; **P3** renders `0` pre-first-BPM |
| 89 | `323a0eb5..9ab30445` (delta) | Shared Scenes list loaded at startup — absent `--state` now resolves to `<userData>/engine-state.json` (supervisor constant); `<userData>/scenes.json` hydrated; regression tests | **Fixes the AppImage/data-loss bug** (native showed defaults only); exact path constants byte-verified vs supervisor; 428 claimed |
| 90 | `9ab30445..34d04e47` (delta) | Notices closed — Plane9 NC-SA family (11 ports JS+Rust, per-file variant), mojovideotech NC-SA shaders, st_Ms2SD1 (explicit CC BY-NC-SA 3.0), Helix Eye/Traced Tunnel/Perlin corrections, pcb.jpg attribution, direct Rust crate licence list, README badge replaced; guard test scans+asserts | **Closes the notices P1** — re-scanned independently: 35 NC-SA sources all named; 11 credit-bearing files all named; 43/43 paths exist; guard runs in unit suite |
| 91 | `34d04e47..29d92814` (delta) | Native audio picker — `audio_settings.rs` (atomic tmp+rename persistence to `<userData>/native-audio.json`, precedence persisted>env>PipeWire-then-input, live-switch Keep-on-failure, 9 tests); Audio side panel + live marking + Info readout | Failed switch keeps the working feed; persisted unplugged device falls back; 1 Hz cpal re-enumerate + Refresh button |
| 92 | `29d92814..0d14f1c3` (delta) | Point10 DVD Room boxScale fix — logo model × box group scale; `logo_world_position` feeds the volume seed, `_uSphere*` uniform paths and both point-light origins; bounce stays local | Byte-faithful to `DvdRoom.js:311-313`, `SmokeRoom.js:1206` and `stepBounce:350-372`; regression test pins boxScale=2; 18 passed claimed verbatim |
| 93 | `0d14f1c3..dde600a4` (delta) | e2e `engineParity.spec.js` registry count pinned to exact 432 (`EXPECTED_REGISTERED_MODULES`), both the waitForFunction gate and the expect assert | **Closes the last loose e2e registry gate**; failure mode is timeout→(count printed in assert) — correct; not run here |
| 94 | `dde600a4..b0bdb55b` (delta) | Native loop paced to the engine FPS — `FramePacer` (non-catch-up, jump-forward on stalls), `WaitUntil` replaces Poll, `fps/SET_FPS` wired via `Store::target_fps()` | **Closes the Electron-native 2.3× visuals speed defect**; pacer semantics pinned incl. catch-up behaviour |
| 95 | `b0bdb55b..e9cc2349` (delta) | ISF corpus counts corrected to HEAD — 354/36, 64 PASSES (47 multi), 46 PERSISTENT, 15 FLOAT, 288 image, 64 undeclared TIME | Every number re-verified by my probe; `rust-hybrid.md` tables aligned; `module-parity.md` untouched |
| 96 | `e9cc2349..252aca6f` (delta) | All-modules Gallery default + flat search without the 24-tile cap, letterboxed aspect-correct preview, P0 spike scaffolding retired (P0State kept for tests) | Gallery filter is a pure tested function; snapshot re-baselined; **P3** key()-All footgun noted |
| 97 | `252aca6f..85349542` (delta) | prettier guard reformat (`path.join` folded) | Formatting-only; no behaviour change |
| 98 | `85349542..39d7c1e3` (delta) | `.deb` packaging declares `gstreamer1.0-pipewire`; regression gate in `appimage-flags.test.js` | **Closes the `.deb` screen-capture gap**; probe site `modv-media::video::pipeline` (pipewiresrc factories) byte-verified; test red-first confirmed by author |
| 99 | `39d7c1e3..893e891c` (delta) | Notices texture provenance — `dvd_logo.svg` (Wikimedia PD, DvdRoom.js-stated), ambientCG list narrowed per-file, Poly Haven skies named, NASA narrowed to Earth/Milky Way | All file references verified at HEAD; only `THIRD_PARTY_NOTICES.md` changed |
| 100 | `893e891c..6c5f3687` (delta) | Native Scenes grid cells mapped onto named rows — `assign_scene_row_indexes` reused in `scene_view`; hover-preview gate behind non-empty cells | Byte-faithful to the JS mapping rules (exact → family fallback, holes skip); regression tests pin the reported fixture + store round-trip |
| 101 | `6c5f3687..578b802a` (delta) | Performance-disabled Scene cells (native-only `performance.disabledCells`); picks skip disabled cells; manual apply unaffected | Engine/store/UI all consistent (sorted+deduped, pruned on hydrate, round-trip tested); JS untouched (key round-trips) |
| 102 | `578b802a..c8993a65` (delta) | Scenes grid row/column controls — column drag (`moveScene`), row drag (`onRowDrop`-faithful splice), inline row rename, `+ Row`/`+ Column`, Layers gutter, scroll-both-axes, per-cell performance dot | Pixel-accurate snapshot (148702→153577 B); drop-cancellers double-clear (harmless); snapshot re-baselined |
| 103 | `c8993a65..9c084394` (delta) | Groups pane shows every Group's module list; editors stay selected-only; kittest regression | Shared `group_module_list`; pixel-minimal re-baseline |
| 104 | `9c084394..12c4666e` (delta) | Audio device enumeration on-demand (startup + Refresh only); the 1 Hz poll (stdio flood + pthread-key exhaustion) removed | Correct hot-plug discipline preserved; comment documents the why |
| 105 | `12c4666e..fa163c0d` (delta) | `startPerformance` distinguishes no-cells from all-disabled; fully-disabled grid starts nothing | **Closes the disabled-cells feature's fallback hole**; regression test pins both arms |
| 106 | `fa163c0d..f4495117` (delta) | AGENTS.md records the native-only disabledCells semantics | Byte-accurate against HEAD |
| 107 | `f4495117..6fe0cb50` (delta) | Smoke test asserts `+ Column` / `+ Row` / `Row names` render | Test-only |
| 108 | `6fe0cb50..9e54b5e2` (delta) | Audio panel clarification — IN-USE banner + detail line, system-default-first ordering, cpal ids to hover, `is_usable_input_device` filter (ALSA probes/0-ch/dupes dropped) | New `audio_panel.png` golden; default snapshot untouched; **P3** JACK denied by design; `unknown rate` fallback handled |
| 109 | `9e54b5e2..43fa4bbe` (delta) | Snapshot exposes `alpha_input_id`/`composite_operation_input_id`/`composite_operation` on module+group views; `expressions`/`tweens` arrays; pure builders | Byte-perfect refactor (no behavioural change); unit payload tests |
| 110 | `43fa4bbe..d38045d9` (delta) | Meta-input Link rows (module/group alpha/blend); module Blend dropdown (29 tokens, all accepted by parser); Expression section (create/update/clear + every-frame) with cleared-latch echo guard | Only additive UI; payload tests pin all four expression branches; snapshot re-baselined for the Blend control |
| 111 | `d38045d9..38c63a1c` (delta) | Same-message fixup — link-editor MIDI/Tween/OSC/Audio sections disabled on mixed-source edit (`add_enabled_ui`) | Correct UX gate; zero behavioural change for happy path |
| 112 | `38c63a1c..6eb80170` (delta) | Notices: every registry crate (570/570) in the lock file; SPDX row integrity; obligations spelt out (`colored` MPL-2.0, `epaint_default_fonts` OFL-1.1/Ubuntu-font, `libfuzzer-sys` NCSA); guard test | **Closes the notices P1 end-to-end**; independent re-count: 0 missing, 0 stale, sorted unique |
| 113 | `6eb80170..cd0eacc3` (delta) | Groups mint the six control inputs in createGroup (`groups.js:174-197` byte-verified); pre-supplied ids trusted; removeGroup reclaims all six; round-trip heals no duplicates | **Closes the native createGroup parity gap**; fixtures bump coherently (36→54/17→35); **P3** null vs absent keys in serialised Group (skipped round-trip fine) |
| 114 | `cd0eacc3..9e4a5a40` (delta) | Point10 smoke-room re-baseline — bounded-glow gradient assertion (bright 5% > 3× dim 5%) replaces stale `dark_fraction>0.15` | **Closes the one known-red Point10 GPU test**; gradient robust to exposure drift; **P3** uniform-white frame passes ratio (partition not tested) |
| 115 | `9e4a5a40..11446c5a` (delta) | `initialiseModuleProperties` port — `${parent}-${key}` data-type sub-inputs (color r/g/b/a, vec2/3/4 0..N), `type: "action"`, `path: "[key]"`, surviving Scene apply via union-of-types remap | Component half of item 7 closes; **P3** label/step/component/enum still open; goldens honestly unchanged |
| 116 | `11446c5a..e1acbf55` (delta) | GPU tests share one adapter per binary — new `#[cfg(test)] crate::test_gpu`, counting regression, three harness delegates | **Closes the parallel SIGSEGV**; 792-passed claim asserted |
| 117 | `e1acbf55..75e7f116` (delta) | Native preview un-premultiply — `modv_ui::un_premultiply_rgba`; the `alpha=255` flatten removed; GPU live texture explicitly skipped | **Native half of item 1 closes**; legacy JS `putImageData` sites unchanged; unit coverage pinned |
| 118 | `75e7f116..73feb02a` (delta) | Gallery tiles marquee long names on hover — full name, ping-pong 0.5/28 px·s/0.8 clamp cycle; strip-clipped | Pure helpers unit-pinned; the `truncate(..., 12)` cap deleted; hover-state restart semantics |
| 119 | `73feb02a..56745632` (delta) | Per-row layer monitors — GPU-GPU copy from the Group's already-rendered texture (no read-back, no extra pass, no state rebuild); pure refresh cadence (~3 fps/row); dim vs no-layer display policy | Two new goldens honest (row live vs `no layer`); prune-on-apply; **P3** hard-coded cadence, no user knob |
| 120 | `56745632..4c3af40f` (delta) | Scene order select in performance_bar (In order / Random); engine already honoured the key at all pick sites; round-trip persist/reload test with seeded pick migration | JS's `performance.order` key confirmed in scenes.js:258-272; live change applies next pick; **P3** bar overflows on squeezed widths |
| 121 | `4c3af40f..0263f2ee` (delta) | Multi-keyframe tween port — one segment per row, per-segment easing/steps, first-row hold; regenerated from real animejs 3.2.2 via `tools/tween-keyframe-reference.mjs` | Byte-anchored reference fixture committed; reset/direction/loop edges pinned; parity with worker `buildFrames` |
| 122 | `0263f2ee..e555ba04` (delta) | Reference fixture gains reverse-direction 3-row phase; `multi_keyframe_frames_match_anime` covers it frame-exact | Same-shape fixup; no Rust change |
| 123 | `e555ba04..7202749d` (delta) | Free keyframe times in tweens (`keyframeTimes`); replaces anime's even spacing with piecewise segments; invalid lists verbatim in round trip | Native-only extension; Electron ignores the key; unit coverage covers validity matrix |
| 124 | `7202749d..b6ae28a0` (delta) | Persisted webcam selection/order/modes (`native-media.json`); restore preferred-mode fallback chain; camera-only 1-based status index; no pipeline restarts | Byte-faithful to `mediaStream`; atomic writes; unplugged-resume supported |
| 125 | `b6ae28a0..c211457e` (delta) | Webcam resolution/frame-rate ComboBoxes — pure helpers (`unique_resolutions`/`fps_options`/…), advertised modes only, stop/start re-map; `MediaHub::start` camera-only index | Advertised-modes rule honestly enforced via `resolve_mode`; "mode not reported" for caps-less devices |
| 126 | `c211457e..f2361b01` (delta) | Even→free keyframe normalisation — first-row duplicate + even-grid times; `evenly_spaced()` fast path; bit-exact frames non-BPM, ≤1e-12 BPM | Tests pin 3/5 rows × 60/30/144 fps × loop/noloop; **P3** frame count = rows+1 for the doubled row (known cost) |
| 127 | `f2361b01..5f832d5a` (delta) | Capture rate reporting — pure `capture_rate`/`device_info`, `capture_sample_rate` distinct from cpal heuristic, `describe_devices` prints both | The ALSA 44.1 k lie honest; boundary cases pinned (no-48k, virtual 1..=MAX) |
| 128 | `5f832d5a..e2eb0680` (delta) | Tween read model widens — `data`, validated `keyframe_times`, `durationAsTotalTime`, `cycle_seconds` in the snapshot | Descriptors validated at decode (Round 123 fallback semantics) |
| 129 | `e2eb0680..f9500bf6` (delta) | Curve editor ships — handles/grid/handles; normalise on first edit (duplicate first row + even grid); save via `tweens/createTween`; kittest coverage (add/move/remove/round-trip) | New `tween_curve_editor.png` golden; **P3** no undo, no easing preview |
| 130 | `f9500bf6..006c9704` (delta) | Wrap rows + reposition Edit curve; `CHANNEL_COLORS` cycles; `nearest_handle` un-capped; JSON-branch test | Cosmetic; harmonic 6-channel cap → cycle; both loops agree |
| 131 | `006c9704..6fa788a8` (delta) | Native layout persistence — phase 1: pure clamp/write machinery for `<userData>/native-layout.json` | Full clamp coverage pinned; **P3** phase-2 wiring into shell pending; atomic rename write |
| 132 | `6fa788a8..e8f02b08` (delta) | Audio picker renders the negotiated capture rate (`2 ch · 48 kHz`); old cpal 44.1 k heuristic removed from the UI entirely | Banner/picker/BPM panel consistent; golden re-baselined |
| 133 | `e8f02b08..4e57f81b` (delta) | BPM/menu/status/Info/BPM-window all report the same live negotiated rate; new `AudioCaptureRate` cache in-device; silent paths omit rate; `bpm_window.png` golden added | One formatter renders every readout; snapshot stays honest on `audio_active` |
| 134 | `4e57f81b..72baa971` (delta) | clippy clean — `field_reassign_with_default` → struct-update literals in the layout-test fixtures | Mechanical; no assertions changed |
| 135 | `72baa971..f359e459` (delta) | Layout persistence — `native-layout.json` gains `dock_width` + `tabs` (one of tabs/docked/floating per tab); `LayoutState` runtime model; load-clamp-apply + sync-diff-write | Every tab has exactly one placement; **P3** no legacy-doc migration test yet |
| 136 | `f359e459..d2951184` (delta) | Resizable sections/windows; detachable tabs (dock column, floating window); one placement per tab; goldens (tab_float/docked/back_in_strip); shell re-baselined | All eight floating window types restored by id; kittest coverage for detach/dock/back-to-tabs |
| 137 | `d2951184..3bd065d2` (delta) | Gallery → Group drag — `GalleryDrag` payload, accent-chip drop target, `ui/addModuleToGroup` on release, marquee/double-click untouched | One placement per Group; pure `gallery_drop_action` pinned; goldens unchanged |
| 138 | `3bd065d2..da724d45` (delta) | Native `.deb` — wrapper at `/usr/bin/modv`, real binary under `/usr/lib/vmodv-native`, ISF+textures at `/usr/share/vmodv`; `Depends` lists NEEDED + dlopen libs, `Recommends: mesa-vulkan-drivers` | `native-deb-check.sh` auto-runs post-install; field/path/corpus counts asserted |
| 139 | `da724d45..26121571` (delta) | Native-only CI workflow — Clippy+rustfmt, workspace tests (modv-app serial), AppImage+.deb artifacts | Electron nightly untouched; correct Lavapipe serial ordering matches Round 116 |
| 140 | `26121571..a331368c` (delta) | Native CI payload triggers extended (ISF corpus, Point10 textures, icon.png, LICENSE) | Same-message fixup; no jobs/args changed |
| 141 | `a331368c..dbc2aa76` (delta) | Native NDI + OSC panels — `NdiSnapshot`/`OscSnapshot` selectors; 1 Hz tab-visible refresh; receivers with errors; OSC listeners + osc.data leaves | Engine arms pre-existed (Rounds 42–43/50); the UI half now matches |
| 142 | `dbc2aa76..df2ade1f` (delta) | IBM Plex Sans/Mono bundled — four faces, OFL 1.1 next to them, Plex-led families with egui fallbacks, unit-pinned | **Licensing intact**: notices fonts section added; egui bundled faces still attribution-covered |
| 143 | `df2ade1f..5da737a8` (delta) | Neon-black theme overhaul — square corners/handles, single green accent, Plex scale bump, rescaled sliders/menu/status/dock gaps; all goldens re-baselined | Pure theme tickets; **P3** goldens a regression net; viewport reads fine |
| 144 | `5da737a8..40fdb8fb` (delta) | Input-link Scene-apply fix — `InputIdentity` capture-before-apply; `retired_inputs` retarget; tween location rewrite; regressions pin full round-trip | **Strictly stronger than JS** (native keeps unique rebuilt matches; ambiguity → INFO); suite pins value semantics end-to-end |
| 145 | `40fdb8fb..f239ad56` (delta) | Slider contrast — DOCK_FILL/STRIP_FILL route the rail; `theme::slider_visuals` slider look everywhere; regression kittest pins every tab kind with a real RangeControl | Two new goldens: `tab_docked_range.png`, `tab_float_range.png` |
| 146 | `f239ad56..1b7eb187` (delta) | Native progress + parity maps refreshed — P0/P5/P8 rows re-marked; smoke_room retired as red; counted `#[test]` figures replace stale ones | **P3** docs drifted by ±1–2 vs HEAD (modv-ui 218; modv-media 256 vs "257"); package/e2e claims held honestly |
| 147 | `1b7eb187..60b59e20` (delta) | Tab drag strip/dock/floating — typed `TabDrag`, grip icon, dock-here placeholder, float outline, bounded bounds on drop, exclusive placement | Kittest pointer drag + unit bounds clamp; **P3** no keyboard path |
| 148 | `60b59e20..14700585` (delta) | Same-message fixup — strip advertises drag outline while docked/floating tabs fly | Pure paint |
| 149 | `14700585..d35241f6` (delta) | Scene cell disable toggle reliable — `enabled: cell.disabled`, `CellEnableDot` 20×20 hit area owns presses | Closes the latent Round-101 toggle bug end to end |
| 150 | `d35241f6..6b5d07ef` (delta) | Frames go pure `#000000`; raised surfaces step lighter; theme tests updated | Style-only |
| 151 | `6b5d07ef..df395840` (delta) | Native file dialogs + File menu actions: in-app egui picker, six File menu actions, project profiles (`native-project.json`, tmp+rename), `--scene`/`--playlist` aliases, `--preset` auto-detect, modal Ctrl+S/Ctrl+O guards | **P3** Ctrl+Shift+S advertises "Save preset as" but dispatches `SaveSnapshot`; startup `--preset` Save target ignores document kind; PROFILE_FOLDERS over-creates dead folders; keyboard surface untested |
| 152 | `df395840..e7ffecf7` (delta) | Scene grid: fixed row pitch; pinned row-label gutter (opaque backing, shares vertical scroll); `ui.place` vs `ui.put`; pixel-aligned rows | New `scene_grid_layout.rs`: pitch equality, pinned gutter, shared vertical offset (3 kittest cases) |
| 153 | `e7ffecf7..fa67044b` (delta) | Media-tree watcher — isf/image/video/palette + extra ISF dirs, allowlist = JS read handlers; pure `Debounce`; ISF burst = one rescan; others log-only | Integration test observes one burst within 5 s |
| 154 | `fa67044b..7f27a34a` (delta) | Startup Save-target guard — `--preset` pointing at Scene/playlist no longer arms Ctrl+S; `<userData>/logs` created before xdg-open; profile modal empty-name test + kittest | **Closes Round 151 finding 2**; Round 151 finding 1 (Ctrl+Shift+S) still open |
| 155 | `7f27a34a..dad7c58e` (delta) | BPM readouts drop the duplicated `· 48 kHz` (bare `BPM N`; rate only on audio indicator + Tempo window) | **P3** `bpm_window.png` still shows the old line — suite red until the Scene-timing batch re-baselines; `bpm_with_rate` left dead |
| 156 | `dad7c58e..f610d08b` (delta) | `phrase_drop` one-frame pulse → `scenes/tick({phraseFlipped})` → `shouldAdvanceRow` port (deadline forces, flip rolls `switchChance`); `performance.timingUnit` (`seconds`→`bars`, 4/4, 120 BPM fallback) + `scenes/setTimingUnit` converter for globals and overrides | Byte-faithful to `scene-rows.js:405-431`/`phrase-detector.js:87-96`; stored-unit-retaining normalisation; RNG untouched without a flip; **P3** no legacy zero-Module autopilot |
| 157 | `f610d08b..1901063c` (delta) | Scene times editable in bars — `SceneView.timing` tuple, unit selector + resolved-seconds hint in the performance bar, quarter-bar steps, `Reset override`; 22 goldens re-baselined (status bar now `BPM 128`) | **Closes Round 155 red-test finding**; **P3** `bpm_with_rate` still dead |
| 158 | `1901063c..822e7303` (delta) | Bar-unit doc comment de-proper-nouned | Style-only |
| 159 | `822e7303..dde24d04` (delta) | Per-row live Scene markers — snapshot `active_scene_rows`, pure `scene_cell_highlight` (Applied/Live/None), LIVE badge, APPLIED header tag for per-row mode | Golden shows the divergent state; pure + kittest coverage |
| 160 | `dde24d04..c667b1d6` (delta) | Default scenes: transparent Point10 heroes moved into Foreground (9 scenes); both default-scenes copies re-synchronised; `DEFAULTS_VERSION` 16→17; refresh list extended | JS/Rust byte-compared (62/62 in order); parity tests pin in-order seeding |
| 161 | `c667b1d6..026b5721` (delta) | Sandboxed Boa plugin host — `plugins.rs` (2534 ln), 24 adversarial tests, CommonJS shim deleted post-load, five globals only, loop/recursion/source/capture caps, catch_unwind isolation | **Nothing wires it in the app yet**; tracked to the D5/WS-12 milestone |
| 162 | `026b5721..c4fc956f` (delta) | Plugin gate + discovery: `native-plugins.json`, consent-normalised master switch, active-profile + user roots, per-plugin switches, atomic writes, allow-listed reader, `UnsupportedTransport` placeholder | **P3** no UI yet; consent enforced against hand-edited files; 16 tests |
| 163 | `c4fc956f..b4481f3c` (delta) | Plugin frame hooks run: pre-hook pre-composition, post-hook post-present, capture via `read_frame_at` (factored from the preview path), 20 Hz throttle, alpha-flattened, α=255 tested | **P3** still no Plugins panel; transport placeholder; capture GPU test added |
| 164 | `b4481f3c..8e16e008` (delta) | Plugin panel model — `PluginView`/`PluginInfoView`/`PluginPropView`/`PluginPropKindView`; from_schema skip-malformed; bounds fallback; int-round; colour ratio payload | **P3** plugins_panel stale text at HEAD (claims host unshipped); nothing renders the new model yet |
| 165 | `8e16e008..18fcdc81` (delta) | Retained plugin logs (256 dedup window, `clear_logs`) + prop writes through the host schema; coerced values persisted in `native-plugins.json` and reapplied on load/rescan/restart | Full property-round-trip restart coverage; snapshot `plugins` still unfed (default placeholder) |
| 166 | `18fcdc81..c7a6bac1` (delta) | Scene grid: cell click dispatches `scenes/applySceneRow` (payload-pinned); empty cells apply nothing; menu distinguishes rows | Engine `applySceneRow` arm byte-verified; dot-click still no-op |
| 167 | `c7a6bac1..abd0c51e` (delta) | Plugins panel: consent-gated master switch, Revoke consent, dialog quotes real sandbox limits; snapshot feeds `PluginView`; 1 Hz refresh only when visible; logs cleared on reopen | **Closes Round 164 stale-panel false-copy finding**; **P3** rows/props/logs viewer still absent; **P3** test header cites absent `SetPluginProp` |
| 168 | `abd0c51e..be173841` (delta) | `Strobe Riot` retired: `DEFAULTS_VERSION` 17→18, `REMOVED_DEFAULT_SCENES`, refresh list, first production `DEFAULT_SCENE_MIGRATIONS` row; seeded-only removal | Both lanes synchronised; tests pin seeded/custom separation |
| 169 | `be173841..1515694a` (delta) | Plugins panel — per-plugin rows (dot/enabled/capture/name/author/path), generated prop controls (RangeControl/checkbox/text/colour/select with rounding+clamping), `Logs & errors` collapsible, Rescan/Reload buttons, empty-state | Committed golden documents all three states incl. an error case; closes the plugin milestone |
| 170 | `1515694a..6d9bb058` (delta) | Cameras in Webcam-index order — `ordered_cameras` ports `orderEngineCameraInputs`; kittest pins on-screen row order | **P3** JS `localeCompare` vs Rust `to_lowercase` (ASCII-equivalent) |
| 171 | `6d9bb058..ee65cd55` (delta) | Recursive ISF scanning + watching — depth ≤ 8, 4096 entries/root, symlinks refused, hidden/temp names rejected per-component, longest-root classification, corpus flat | **P3** `isf/compiled` no longer ignored (debounced); integration tests pin nested dirs |
| 172 | `ee65cd55..cbdcbdae` (delta) | Screen capture: XDG portal → `pipewiresrc` as `screen:portal` (camera-like); `ScreenShare` state-machine (one-slot, monotonic ids, stale-drop, cancel); portal session kept alive for the capture; support probed before any dialog; worker thread, 1 Hz drain | **P3** no `restore_token` path; **P3** single screen source by design |
| 173 | `cbdcbdae..65d14072` (delta) | Screen-share UI — Share monitor/window buttons (disabled while busy), pending cancel, running hint with node, unavailable reason, error surfaces | kittest covers payloads/enable states/rendering across all states |
| 174 | `65d14072..a20f105f` (delta) | Library — recursive capped index of image/video/palette with decoded thumbnails/palette JSON/MP4-WebM metadata; new Library tab (navigation, breadcrumb, rescan); TextureControl drag-assignment; 19 goldens re-baselined | **P3** no video poster (headers only); **P3** document folders not indexed; **P3** 200-per-kind cap |
| 175 | `a20f105f..7f4d16ce` (delta) | Library → texture props — matching-kind drops + "Use from library…" arming flow; typed `MediaLibraryDrag`; palette refused; kind-mismatch drop refused | kittest asserts exact prop payload equality with the manual path editor; **P3** no ghost drag preview |
| 176 | `7f4d16ce..19af2dad` (delta) | Loose `scene/`/`preset/` import once per profile — deterministic recursive walk (sorted, bounded, symlink-refusing), id-or-name collision handling (existing wins, name collides → skip + report) | Compositor `ImageTextures`/previews now use the shell media root; **P3** flag map never pruned |
| 177 | `19af2dad..e1985c34` (delta) | Import-flag doc: files added later require deleting the profile flag | Docs-only |
| 178 | `e1985c34..64f364f1` (delta) | Video transport — pause/resume/seek/loop/position/EOS in `modv-media` (`VideoTransport` snapshot); `VideoHub` transport route; 30/fps-respecting poll; live sources unseekable | **P3** no UI wiring yet — panel scrubber is the next batch |
| 179 | `64f364f1..c96601a6` (delta) | Palette tween editor — detection (RGB/RGBA, ratio alpha), swatch strip edit engine, CSS hex parse, two-row floor, re-evened `keyframeTimes`; golden + kittest | Full `PaletteControl.vue` parity; **P3** docs declare the vec3 ambiguity explicitly |
| 180 | `c96601a6..a1016475` (delta) | Video playback rate — flushing rate seek at current position; `position_ms` caches across flushes; 0.1–10 clamp; rejected rates; EOS-parked state preserved | Real-pipeline integration tests (2x, 0.25x, rapid changes, loop survives) |
| 181 | `a1016475..1416ee38` (delta) | Video-file texture sources — one pipeline per unique path, `video:<abs path>` ids, reference-counted by (module, prop), 10 Hz reconcile, missing-file once-log + retry, non-restart transport options | Ghosts stay alive through crossfades; 20 unit tests; `gst` dev-dep for real-pipeline fixtures |
| 182 | `1416ee38..1b1f1af7` (delta) | Inspector + Media tab transport — Play/Pause/Restart/Speed/Loop/position/duration; prop-option + `MediaAction::VideoTransport` dual writes; missing-clip problem line; 11 kittest cases | Position/duration shown as point-in-time snapshot (not animated) |
| 183 | `1b1f1af7..0d7719c5` (delta) | Renderer labels `paused` engine media sources (`"Paused"` in `MEDIA_SOURCE_STATE_LABELS`) | One line + one assertion |
| 184 | `0d7719c5..e6ae9c0a` (delta) | One-shot MIDI learn — one-armed learner, clock never resolves, CC→type-144 latch rewrite kept; legacy payload shape; dispatcher/read arms | Engine byte-faithful against `MIDI.vue` semantics |
| 185 | `e6ae9c0a..33ea2fb6` (delta) | Inspector MIDI Learn/Forget — floor-based resolve ordering, "Waiting for MIDI…" state, audio-link smoothing release | 16-case kittest pins payload ordering and semantics |
| 186 | `33ea2fb6..737672ed` (delta) | Library "Open folder" — kind allow-list, traversal/absolute/drive-letter/NUL rejections, kind-root → profile → media-root fallback | unit + resolver tests; **P3** `:` blocked by design |
| 187 | `737672ed..8281bcc5` (delta) | Media watcher ignores `Access` events; the Library watch/rescan/rewatch atime feedback loop closes | Pinned by unit test; **P3** coarse Access(_) filter (documented) |
| 188 | `8281bcc5..979a5bd1` (delta) | Engine snapshot persisted periodically (30 s) and on quit — default state path resolved once, reused; crash-resilient; regression test pins default-path create→save→restore | **Fixes a silent stale-state bug** (every launch boots a two-day-old file) |
| 189 | `979a5bd1..9832c9e8` (delta) | Daily native log — POSIX `log-run.sh` stderr-teed launcher; ISO header + pid; exit code and stderr preserved; fail-soft without bash; stdout untouched; AppRun + .deb dispatch both route through it; 7-file prune | **Closes the launcher logging asymmetry**; behavioural suite; **P3** bash required for the tee |
| 190 | `9832c9e8..8b9c40ed` (delta) | Native installers versioned 2.0.0 — `NATIVE_VERSION ?= 2.0.0`; nightly-appendable via CI (`-nightly.<date>`) | **P3** crates stay 0.1.0 deliberately |
| 191 | `8b9c40ed..613cfe40` (delta) | `make dev` defaults to the native egui shell; Electron retained as `make dev-electron`/`MODV_UI=electron`/`ENGINE_MODE=1` | **P3** AGENTS.md dev-entries are stale now (default flips) — author-own doc to update |
| 192 | `613cfe40..d8058b89` (delta) | Nightly publishes native AppImage + .deb (`2.0.0-nightly.<date>`) after Electron publishes; `needs: build`; `--clobber` upload | Electron remains the fallback or full fail-visible; CI only |
| 193 | `d8058b89..3dc44678` (delta) | Native 2.0 release note — artifacts/versions/userData/ISF/plugins/local-commands/CI flow + explicit residuals; `rust-full-rewrite-progress.md` refreshed | Honest flatpak absence, grab-canvas and modV.ws named; **P3** rewrite P1 residual line still names the logger |
| 194 | `3dc44678..aa93356d` (delta) | Module-registry parity manifest (`tests/fixtures/module-parity.json`, 432) + JS glob recompute guard + Rust manifest/name-set guards | Corpus-bound; **P3** corpus-absence panic in guarded test |
| 195 | `aa93356d..821bef8a` (delta) | Packaged-app smoke suite — build-free; throwaway userData; headless bench + windowed apply + state + SIGKILL recovery + bounded soak | **P3** not wired into CI (yet); never touches real userData |
| 196 | `821bef8a..36d0bab2` (delta) | 2.0 hardware waivers — NDI/multi-GPU/clean-VM/audio/2h-soak waived; smoke evidence table; 61-scenes count corrected | Honest waiver ledger attached to release artefacts |
| 197 | `36d0bab2..65881cd8` (delta) | Smoke profiles stay off the real userData — temp-tree default per phase; crash tree copied not shared | Isolation enforced, not by convention |
| 198 | `65881cd8..83d7a1a2` (delta) | BPM Input-Link source — `beats.bpm` state link, absolute (bounds ignored), skipped pre-detect; UI lists BPM under Audio features; e2e parity flake fixed via re-commit-and-poll | Mirror gate updated; **P3** token is `"bpm"` in both trees |
| 199 | `83d7a1a2..eb00dd3b` (delta) | `--start-performance` CLI flag — unattended performance start after seed | Unit test; composable with `--bench-seconds` |
| 200 | `eb00dd3b..2ddc89b4` (delta) | Control socket (`--socket`, JSON-lines) — ping/state/dispatch/screenshot/quit/save/version; 0o600; bounded backlog; stale/live/regular path handling; one response per request | Opt-in only; no production path involvement; winit drains once per frame |
| 201 | `2ddc89b4..8d04f165` (delta) | Native e2e process harness — real binary, throwaway userData; CI `--test-threads=1`; Docker installs xvfb/x11; docs (`native-e2e.md`); display ranked Xvfb→sway (host runs Wayland+RADV externally) | **The Playwright suite has a native stand-in**; **P3** CI headless rides X11 while the shell is Wayland-first |
| 202 | `8d04f165..a2403611` (delta) | `docs/native-e2e.md` — protocol, rules, porting buckets, waiver semantics | Precise, test-first port discipline |
| 203 | `a2403611..9ec29d1b` (delta) | Audio port — tempo-detector matrix ported to `modv-media::audio::tempo`; system-audio state contract on `AudioSelectionView` | Same fixtures as the JS spec |
| 204 | `9ec29d1b..f8242f02` (delta) | `loadSceneFromJson` defaults `apply:true` (JS parity); importers/seeders pass `apply:false`; Unit test pins both arms | **Fixes a silent no-op on File → Open Scene** |
| 205 | `f8242f02..89759887` (delta) | ISF 3d-scenes registry test + Point10 transparency GPU tests (lavapipe); skip-if-absent | 96×96 canvas matches e2e |
| 206 | `89759887..671dfc6c` (delta) | Default-Scene audits — no Mesmer Effects, registered ISF filters spread, v3 batch intact, bass/BPM links, defaultsSeeded-alone hydration | Drives exact JS spec logic |
| 207 | `671dfc6c..3563f3af` (delta) | Fresh shell seeds one New Group (`restored_snapshot==false` + no startup doc + no applied Scene); window-state beats CLI size | Regression-pinned |
| 208 | `3563f3af..2847cbb1` (delta) | Process-level scenarios — engine skeleton (snapshot/relaunch), main window, default-Group parity, window-state round-trip, Scene-like preset open; control socket `save` command | `docs/native-e2e.md` updated |
| 209 | `2847cbb1..988ed03e` (delta) | Scene graphics/performance port — 1300 ln `e2e_scenes.rs`; row fades, gallery-less, preset round-trip, GPU skip-guard | Direct compositor driving with `RowFadeInfo` |
| 210 | `988ed03e..6596a55c` (delta) | Supervisor-coverage note points at the Electron unit tests | Doc-only refinement |
| 211 | review-only (no commit yet) | CI headless rides X11 while the native shell is Wayland-first — prefer `sway --headless`; install `sway` + `libwayland-*` in the rust image | One-line swap + two apt-get lines; surfaced in review, no commit yet |
| 212 | `6596a55c..491c9214` (delta) | Groups pane port — full groups/*.spec.js parity: toggles/reorder/remove/create, snapshot-mutation round-trip | JS lineage names every test; inheritFrom/tri-state/Backspace honestly excluded |
| 213 | `491c9214..a9901d41` (delta) | Scenes grid port — hover/still, add/reset, clear cell, live-capture, rename, empty-cell create | Native 'edit' = live-capture button; documented |
| 214 | `a9901d41..5683e82e` (delta) | Focused input port — editor follows the clicked prop (`in-amount` vs `in-speed`) | Byte-true to inputConfigUpdates spec |
| 215 | `5683e82e..3dbafe2a` (delta) | Each native test annotated with its JS spec lineage | Docs-of-tests |
| 216 | `3dbafe2a..f61c4d55` (delta) | All 29 COMPOSITE_OPERATIONS exercised in the blend test | Popup scrolls options into view like a real user |
| 217 | `f61c4d55..69cb78da` (delta) | `midi/SET_STATE` + `midi/loadPresetData` merge semantics; unknown keys survive; non-object payloads refused | JS `midi.js:60-69` byte-true |
| 218 | `69cb78da..bbe8e882` (delta) | Scene apply restores MIDI state and focused input; remap through input id map; row applies no-op | Byte-true to `restoreSceneMidi`/`restoreSceneInputs`; scoped early-return preserved |
| 219 | `bbe8e882..a78996e0` (delta) | `enabled` survives hydration; `updatePerformance` syncs runtime flag; `loadScenesFromDisk` keeps live keys | **Closes the JS divergence** (legacy autopilot resumes performance mode after restart) |
| 220 | review-only (soak evidence; no commit) | The 2 h native soak: packaged AppImage, `--apply-scene "Mycelium Bloom" --start-performance --bench-seconds 7200`; 431,816 frames, 59.97 fps, 1920×1080; exit at the 7200 s deadline; RSS flat at ~277.8 MB for the last 75 min; device VRAM flat after warm-up | Recorded from author artifacts + OpenCode session; cross-checked arithmetically; never re-run; closes the "2 h soak — waived" line |
| 221 | `a78996e0..18de3aa3` (delta) | Tri-state Group enabled (0/1/2, `Checkbox.vue` alt-click→2), `inheritFrom` `-1`/UUID selector (Gallery excluded), Backspace delete, one-time startup Group focus, `focused_input_title` | JS gesture semantics verified against Checkbox.vue; **P3** no frame-level 2-state render check |
| 222 | `18de3aa3..afa85986` (delta) | Hover stills per cell; Clear-cell confirmation; `{}` addEmptyScene mints `Scene N`; focused-input window keeps `<owner>: <control>` | Legacy grid `performanceGrid` intent honoured |
| 223 | `afa85986..760c3a57` (delta) | Groups port completed: 1→0→1→2→0→1 round-trip, both inheritFrom forms, Backspace with text-field gate, startup-focus test un-ignored | Full groups-spec parity |
| 224 | `760c3a57..6bb12221` (delta) | Hover still re-keyed by saved `module_names`; an edited cell stops painting its stale frame | Closes Round 222's one-commit stale-frame window |
| 225 | `6bb12221..6c74947e` (delta) | e2e parity manifest finalized — 147 tests: 137 covered + 10 waived; two guards; counts re-derived independently against the live tree | Manifest survives the Electron suite's deletion |
| 226 | `6c74947e..4a2a4164` (delta) | ISF corpus + Point10 textures relocated to `resources/` — pure renames; every native/CI/script/notice/JS-guard reference switched | **P2**: Electron fallback no longer buildable; `native-2.0-release.md` still advertises it |
| 227 | `4a2a4164..f226729b` (delta) | Remaining guards ported before the JS tree dies: `notices.rs` (Cargo.lock SPDX/forge/NC-SA scan + attribution pins + no-Electron-paths guard) and `removed_content.rs` | Notices verified by reading only |
| 228 | `f226729b..eaac0a68` (delta) | **Electron tree deleted** — 564 files, −113,053 lines; archive `legacy-electron` + tag `v1.x-final` at `6c74947e` | Parity manifest survives; engine fixtures deleted (unused by Rust); **P3** `rust-full-rewrite.md:824` stale |
| 229 | `eaac0a68..4e0a7b14` (delta) | Rust-only Docker toolchain + native-only Makefile (`build` = appimage+deb; `test-e2e-native`; `lint` → clippy) | Xvfb+lavapipe still preferred over sway in CI (item 12) |
| 230 | `4e0a7b14..e25128c6` (delta) | Nightly is native-only (test → package → publish); native.yml path filters widened; browserslist.yml removed; issue/PR templates point at native commands | **P3** committed docs still Electron-first (item 25); archive refs verified |
| 231 | `e25128c6..b1ac24d5` (delta) | Ported guards made clippy-clean (collapsible_if; rustfmt) | No behaviour change |
| 232 | `b1ac24d5..8b51280b` (delta) | Docs rewritten for the Rust-only tree — README native guide, release note de-Electroned, `legacy-electron.md`, parity map archive-only + verification layers | **P3** legacy-electron.md lists broken `4a2a4164` without warning; **P3** verification numbers author-reported |
| 233 | `8b51280b..13e38c13` (delta) | Last Node tooling gone — `tween-keyframe-reference.mjs` removed; NDI resolution drops grandiose candidates; tools/ is native shell only | **P3** no bundled libndi anywhere (documented degradation) |
| 234 | `13e38c13..041fb488` (delta) | Legacy archive refs pinned (branch/tag at `6c74947e`) | Docs-only |
| 235 | `041fb488..eca253f1` (delta) | CI workspace suite serial (`--test-threads=1`; tab_placement flake) | X11 CI unchanged (item 12) |
| 236 | `eca253f1..a2fa2463` (delta) | Electron-only icon/entitlement assets dropped | Assets-only |
| 237 | `a2fa2463..d49524ad` (delta) | `rave.exe.fs` regenerated via quarantined Node container (`make rave-reencode`) | **P3** tools/ is not native-only anymore (docs tool counts stale) |
| 238 | `d49524ad..2e3e1d30` (delta) | Header subtitle dropped; 4 snapshots re-baselined | Cosmetic |
| 239 | `2e3e1d30..02d41416` (delta) | Scenes summary moves into the top header; duplicated header dropped | Cosmetic |
| 240 | `02d41416..9cdde543` (delta) | Performance bar → top header; **DragValue clamp fixes per-frame `updatePerformance` on zeroed configs** | Real bug, real fix |
| 241 | `9cdde543..dfed308d` (delta) | Two-row header; grid controls above the cells; timing hint drops redundant BPM | Cosmetic |
| 242 | `dfed308d..a6938032` (delta) | `groups/MOVE_MODULE` — clamped insert after source removal; mirror deltas source-then-target; dedup-on-move | Native-only action |
| 243 | `a6938032..3401b858` (delta) | Module drag-and-drop — ModuleDrag (in-place inert), insertion_index, gap line, ^/v buttons removed | Unit-pinned gap math |
| 244 | `3401b858..87d7eac8` (delta) | e2e drag/drop — reorder, cross-Group insert, empty-Group append, name-drag inert; fixture mirrors engine math | Strong harness coverage |
| 245 | `87d7eac8..20a8c805` (delta) | Groups module-row snapshots re-baselined | Snapshots-only |
| 246 | `20a8c805..81ea1c55` (delta) | **Fix: inter-row pixel gap is an insertion point, not an append** (`module_rows_span`) | Regression test pinned |
| 247 | `81ea1c55..22646926` (delta) | Two-bar grip is the only drag handle (accessible label); name = click-to-select; drag-name inert — test pinned | Snapshots re-baselined |
<sub>Review file maintained incrementally; earlier passes are not retracted,
only extended. Latest pass: Rounds 234–247 (post-release snagging) on top of
`22646926` — 2026-11-18. This file's own edits are
the reviewer's working state — commit or fold them at your convenience.</sub>

## Round 13 — the author's fix-wave report (recorded verbatim, with the verifier's verdicts inline)

*The block below is the author's own closing note for this round, committed
as `a3cac91f`…`e916f42a`. Ticks ✓ were re-verified by reading the code (see
“Round 13 — verification” above); ⚠ entries are disputed — do not rely on
them without the corrections there.*


- **Expressions P1 closed ⚠ NOT CLOSED** — `expr` source is capped (`MAX_EXPRESSION_SOURCE`)
  with a denylist for native allocation/loop/context escapes (`Array.from`,
  `repeat/fill/map/forEach/reduce`, `Function/eval`, `globalThis`,
  `setTimeout`, `require`, …); the compiled-function map uses an unguessable,
  non-enumerable global key; `rebuild` disposes the JS realm (no prototype
  pollution across presets); every-frame writes follow document order instead
  of `HashMap` order. Hostile sources are rejected at compile time (tested);
  ordinary maths still evaluates (≈0.7 µs). *(Cap and denylist are real,
  but the denylist is bypassable — padStart/padEnd, `.constructor.constructor`,
  regress backtracking, whitespace evasion, BigInt literals — and there is
  no wall-clock/allocation budget at all. "Hostile sources are rejected
  at compile time" is false for the same reasons. See Round 13.)*
- **Media P1s closed ⚠ PARTIAL / NOT FOR THE DEFAULT PROFILE** — media-library paths (`/video/clip.mp4`) are re-rooted
  under `media.path` (known folders only, `..` rejected) while true absolute
  paths pass through; *(only the root-level `/video/...` shape is re-rooted;
  the real default-project library shape `/default/video/clip.mp4` still
  fails end-to-end on POSIX, and fails outright on Windows. See Round 13.)* engine video sources are owned by the texture/module and
  no longer stopped when the inspector unmounts; `media/start-file` rejects
  URI schemes, relative paths and `..`; non-looping files park at their end
  instead of replaying forever; frames larger than 8192² are refused before
  the texture pool reserves them.
- **PipeWire P2 closed ✓** — the monitor stream now latches `state_changed`
  errors/disconnects into the existing error channel, so a mid-stream server
  restart falls back to the renderer pump instead of reporting silence.
- **Camera enumeration crash fixed ✓** — `media/list-cameras` crashed the
  packaged engine with SIGSEGV: GStreamer's `DeviceMonitor` instantiates the
  libcamera provider on a GStreamer 1.28 host. Enumeration now queries V4L2
  directly (`v4l` + sysfs), filters non-capturable metadata nodes, and lists
  `/dev/video0` + `/dev/video2` with their full caps. Verified in the packaged
  AppImage (`MODV_ENGINE=1`), including a live Webcam module fed by
  `/dev/video0`.
- **R-33 closed for SmokeRoom ✓ (DvdRoom NOT fixed)** — the emitter mesh/seed matrix now scales with
  `boxScale` (the JS `_boxGroup.setScalar`), matching the wind-falloff radius
  (`u.origin.w`), with a regression test.
- **Packaging ⚠ PARTIAL** — the `.deb` declares the GStreamer runtime packages
  (`libgstreamer1.0-0`, plugins base/good/libav) alongside
  `libpipewire-0.3-0`; the AppImage still needs them from the host (the legacy
  renderer does not), documented in AGENTS/progress. *(`gstreamer1.0-pipewire`
  is still missing — the screen-capture path in this same report is dead on a
  `.deb` install. See Round 13.)*
- **Input Link mirror flush ✓** — a value that settled inside the 100 ms delta
  throttle was cleared as "unchanged" before it reached the mirror, so the UI
  slider could stick at an intermediate value while the engine rendered the
  final one (reproduced by the engineParity bass link: mirror 1.37, engine
  0.63). Unchanged writes now flush the current value for module props, meta
  values and group controls, with a regression test.
- **Volumetric fidelity ✓ (with the round-12 caveats)** — SmokeRoom, DvdRoom, RollingFire and BallOfFire now
  run the real GPU volume sim + raymarch; BallOfFire is registered (26
  scenes); side-by-side captures against the legacy AppImage look structurally
  right (turbulent warm fields, raymarch strata, legible DVD wordmark, frozen
  lava core). GeometricTunnel hoop scale (R-31) and same-frame/fade transition
  fallbacks (R-35/R-36) are fixed with tests.
- **Still open (documented) ⚠ incomplete** — straight-alpha Point10/ISF targets composited
  through a premultiplied graph (black backdrop rows unaffected; needs an
  explicit composite-time premultiply decision), engine-mode screen capture
  (Electron portal node id), R-35 apply-time failure path, plugin-host P2s,
  and the media-health nits (inline restart, permanent-failure latch)
  *— also open: the default-profile media-path shape, orphaned file sources
  decoding forever, express's bypass classes (Rounds 8/13), DvdRoom `boxScale`,
  `removeGroup` leaking group modules, mirror-parity fixtures not extended
  for the new `groups/*` deltas (Round 13).*

## Round 14 — user-reported defects (background, alpha, enable, cameras, perf)

Reported by the user after `e916f42a`. All items here were analysed statically
(no build/run); GPU behaviour is verified by reading only. The other agent is
working on these now.

### [P1] Module `alpha` and `enabled` (and blend mode) silently do nothing — `docs/rust-hybrid-review.md` root-cause below

The inspector commits **mutation-name** mutations directly
(`modules/UPDATE_ACTIVE_MODULE_META`, `compositeOperation`, `enabled`,
`alpha`) via the fire-and-forget `engine/commit` facade
(`ActiveModule.vue:152,169,186`, `GalleryItem.vue:371`). The engine's
dispatch table (`store_ops.rs:dispatch`, the `match action` at
`store_ops.rs:359-460`) has **no arm for `modules/UPDATE_ACTIVE_MODULE_META`**
— it lands in the wildcard branch
(`store_ops.rs:458-462`, `if self.warned.insert(...)`) which logs a
warn-once and returns `Ok(Value::Null)` **without emitting a delta**. The
renderer never applies the mutation to its mirror either (engine-mode
`storeFacade.commit` forwards only to the engine — `engine-client.js:382`), so
the control doesn't react at all: clicking Enable does nothing, Alpha springs
back on the next mirrored state, and the Compositor keeps rendering with the
stored values.

Affected (same failure class — committed with a mutation name the engine does
not register; every one of these is today silently discarded, fire-and-forget,
zero feedback):

| Mutation | Call sites | Symptom |
|---|---|---|
| `modules/UPDATE_ACTIVE_MODULE_META` | ActiveModule.vue:152,169,186 · GalleryItem.vue:371 · Group.vue:457 | enable/alpha/blend-mode/name do nothing |
| `outputs/UPDATE_AUXILLARY` | Groups/webcam/aux output paths | output window never updates |
| `outputs/RESIZE_AUXILLARY`, `outputs/REMOVE_AUXILLARY`, `outputs/TOGGLE_DEBUG`, `outputs/SET_DEBUG_ID` | debug/output tooling | debug windows |
| `beats/SET_BPM_SOURCE` | BPM source picker | picker snaps back |
| `modules/SWAP`, `groups/SWAP` | SWAP feature | dead button |
| `midi/UPDATE_DEVICE`, `ndi/*`, `fonts/*` | MIDI/NDI config | dead panels |

The mirror-parity fixture does not contain `modules/UPDATE_ACTIVE_MODULE_META`
either, so no test would catch it.

**Fix (whichever the author prefers):** either add a dispatch alias
`"modules/UPDATE_ACTIVE_MODULE_META" -> update_meta(params)` (and the same
for the other mutation-name commits), or — better — make the renderer's
`storeFacade.commit` translate mutation names to their JS action names over
the wire (they are the same payload shapes), or explicitly route raw
mutation-name commits through a `commit` arm per mutation with an assigned
owner, failing loudly otherwise. Failing loud matters here: today every one
of these writes is invisible to both the engine state and the mirror and even
the logs only ever show one line per boot.

### [P1] `isf/*` and shader module `enabled`/`alpha` parity — check ISF modules too

`render_groups` reads `module.meta.enabled` and `module.meta.alpha`
(`compositor.rs:1410,1413`) — the engine-side contract matches the JS for
shader/2d modules since both keys live under `meta`. The failure is purely
the transport above: nobody can set them. Same root cause, same fix.

### Background not black over transparent modules — ambiguous; two candidate paths, both cheap

Every UI background is already `#000` (Preview.vue:330, GalleryItem.vue:458,
Scenes hover tiles, output-window.html:15), and the legacy worker also
cleared with `clearRect`. So the black backdrop is fine in the **panel**.
The user's report is almost certainly the **winit engine window**: it is
created visible on the desktop (titled "modV Engine", 1280×720 default; only
`--fullscreen` makes it borderless) and never hides itself
(`crates/modv-app/src/window.rs:330-347`,
`WindowAttributes::default().with_title("modV Engine")`), presenting a
framebuffer cleared with `rgba(0,0,0,0)`
(`crates/modv-render/src/frame_graph.rs:826,906`). On compositors that
composite window alpha, the desktop shows through transparent pixels ("the
transparent stuff shows my wallpaper, not black"); even without alpha blending
it is an empty, decorated, visible window on the desktop. The fix is to hide
it (`with_visible(false)` — or an explicit `--debug-window` flag for the rare
need to see it) and/or paint MAIN to opaque black
(`Clear(Color::BLACK)` with alpha 1); if the fullscreen projection path is
also in play the same opaque clear covers it.

Needs a screenshot to pick between these; the one-line fix (hide the window,
black clear) resolves either.

### Neither camera works — strongest causes, ranked; needs one log line to decide

Engine-mode camera path was fixed end-to-end this cycle (V4L2 direct
enumeration fixed the libcamera SIGSEGV), and the author reportedly saw a live
Webcam working. If both are dead for the user, check in order:

37. **Enumeration yields zero cameras** (the list is empty and the panel
   shows none — reported as "cameras don't work"): on a stock host this is
   permission-related (`v4l2` ioctl EPERM, or the user lacks the `video`
   group; the Docker/e2e path forwards the group explicitly). Capture the
   engine log lines starting with `camera`/`media` at `modV Engine` start.
38. **Capture starts but no frames** (module renders black): the camera
   pipeline exists but preroll fails — the advertised modes from direct
   V4L2 caps can produce `framerate=29/1` vs the driver's `30000/1001`
   discrete interval (bug already flagged, `camera.rs:145-149`); videorate
   should rescue it, but the caps string is still wrong at the source.
39. **Reconnect loop never converges** — transient list error reads as
   "empty list" and stops the selected cameras (already flagged:
   `media/list-cameras` failure path).

Observed evidence from the author's live run (logs only, not committed
code — `/tmp/opencode/app-cam.log` 17:08, `app-cam2.log` 17:12): an
AppImage built at 16:28 (post-`2a149f16`) logs `GStreamer ready for video
capture`, and `camera:/dev/video0` starts successfully (`index 1, 1280x720@30`,
~21 s after boot), while a second-source attempt fails with
`media_start_failed: video pipeline failed to start: Device '/dev/video2' is
busy` (GStreamer device busy, not a permission error). So at least one
camera path is confirmed alive on the current build; the busy node is
metadata-or-second-capture adjacent — note for the fix author: if the
metadata node is listed by the fixed enumerator, hide it from the
capture-list and surface "in use by another process" as a visible error.
The user-facing outcome may still be "neither camera works" if the build
they run enumerates zero cameras (permission/group) or was built before
`2a149f16`. One log excerpt will disambiguate.

Whatever resolves it here should be accompanied by: a unit test that the
`30/1`-style requested-caps are always within the discrete intervals the
device exposes, and a check in `pollStatus` that `frames == 0` with
`lastError == null` escalates to a visible error instead of silent
reconnect-forever.

### Stutter / why the rewrite can feel worse than legacy (and better in others)

The engine forwards the live output to Electron **out of band**, not through
Chromium's GPU canvas, but the way it produces that frame is a full GPU→CPU
readback in the frame path:

40. `read_preview` (`compositor.rs:2795-2823`) does a **full-resolution
   blocking readback of MAIN every preview publish** (`read_main_rgba8` →
   `read_back_texture` → `MAP_READ` buffer +
   `device.poll(PollType::wait_indefinitely())`, `compositor.rs:3005-3060`),
   then CPU nearest-neighbour resample full-res → 320×180 (~1–3 ms). The
   blocking poll drains the Fifo pipeline every publish — this is the frame
   pacing killer and the most probable cause of the visible jank even on a
   fast GPU.
41. `ENGINE_PREVIEW_FPS = 15` (Preview.vue) paints with `putImageData`; the
   browser then re-uploads the whole canvas to the GPU compositor every
   paint. Cheap-ish but redundant once (1) is fixed.
42. The **winit window presents back-to-back at the display's refresh rate**
   (Fifo, `window.rs:6,30-37`); the renderer paints at 15 fps — the two are
   unsynchronised, so the user sees the engine's vsync plus a mismatched
   Chromium paint cadence.
43. Heavy scenes genuinely cost more than legacy: the fluid-sim scenes
   (SmokeRoom/RollingFire/DvdRoom/BallOfFire) allocate 100×100×200–150×150×300
   voxel grids (~130–430 MB), and the per-frame cost includes the multi-pass
   sim and raymarch. Simple shader/2D scenes, by contrast, outrun the legacy
   canvas path (direct GPU → GPU composite), hence "better in others".

**Fix (the engine should make this cheap):** produce the 320×180 preview
**on the GPU** — render the main group to a scratch target then a single
bilinear blit to the ring-sized texture — and read back only 230 KB per
publish instead of ~8 MB at 1920×1080 (~35× less readback, and it unblocks
Fifo pipelining). Alternatively, publish on `map_async` without
`wait_indefinitely()` and let the next frame complete the mapping (drop the
frame if not ready — previews tolerate a missed publish).

Also queued for the author:
- The mirror itself receives **per-link `UPDATE_ACTIVE_MODULE_PROP` deltas up
  to 10 Hz** — fine — but the **whole-scene/group transition deltas** are
  emitted per frame for every ghost alpha (10 Hz by design) and
  `meyda/SET_BANDS` mirror @ 1 Hz metrics — ok. Main remaining sync traffic
  risk: preset restore replays hundreds of deltas synchronously at boot —
  fine.
- **`ENGINE_PREVIEW_FPS` should match the ring publish rate** (30 fps) or
  the panel should read the ring directly (it's on disk by design), removing
  the main→renderer Buffer copy per frame entirely.

### Summary of what I'd flag to the working agent

44. `modules/UPDATE_ACTIVE_MODULE_META` dropped silently — enable/alpha/blend
   dead; map the mutation or fail loud (P1).
45. First-class `background = black`: make the main composite opaque black
   or hide the engine window (P1 UX).
46. Cameras: the drop is viewable/fixable in the two steps above;
   `framerate=29/1` for 30000/1001 sources is wrong (P2).
47. Preview path: GPU resample, async map, ~35× less readback (P2 perf).
48. Docs: progress doc still claims “144/144”, “1788 tests”; the
   "un-premultiplied" comment mismatch is now a documented decision (Round 13
   note).

## Round 15 — fix wave for the user-reported defects

Verified on the packaged AppImage (`MODV_ENGINE=1`), 2026-11-12.

- **Alpha / Enable / Blend (P1) — fixed.** `modules/UPDATE_ACTIVE_MODULE_META`
  (and `modules/UPDATE_ACTIVE_MODULE`, `modules/UPDATE_ACTIVE_MODULE_PROP`,
  `inputs/UPDATE_INPUT_LINK`) now alias the action handlers and emit the
  mirror-shaped delta. Tests drive the Store through the same commits the
  Inspector uses and assert alpha 0.5 halves the composited contribution and
  `enabled: false` skips the draw. Live: engine and mirror both show the new
  values; disabling every module leaves an opaque black frame.
- **Transparent background (P1 UX) — fixed.** `present` clears the swapchain
  with `wgpu::Color::BLACK` (was #0b0b0f) and `read_preview` flattens the
  premultiplied main onto opaque black (`[0,0,0,255]`). Gallery/Scene hover
  previews keep intentional transparency.
- **Cameras (P1) — fixed.** The drop was `meta.enabled: 1` from saved Scene
  documents while the compositor requires a boolean `true`; presets and Scene
  snapshots now normalise `enabled` with JS truthiness on load, and the
  renderer re-asserts it for sync/Scene-apply modules. Camera selection derives
  its 1-based index from the engine's camera list (dedup, unavailable ids
  dropped) instead of stale Chromium ids. Live: `WEBCAM2`/`WEBCAM2-EDGEBLUR`
  render the feed with both cameras running; no index-mismatch warnings.
- **Media library paths (P1) — fixed.** Project-prefixed paths such as
  `/default/video/clip.mp4` are re-rooted under `media.path` (known media
  folder in the first two segments), while true absolute paths
  (`/tmp/clip.mp4`, `D:\clips\…`) still pass through. Regression tests pin
  both forms.
- **Engine-mode stutter (P1 perf) — fixed.** Root cause was not preview
  forwarding (the engine presents its own native window; the VMP1 ring is
  UI-only at ~0.02 ms mean): 429 Gallery tiles polled 0.25 Hz (~150
  `preview/module` requests/s), the 4-slot cache thrashed, and each
  volumetric miss rebuilt an instance allocating ~432 MB, blocking the render
  thread 18–40 ms. Tiles now poll only while intersecting the viewport
  (22 requests in 6 s measured), volumetric previews cache a static RGBA8
  poster and dispose the instance, at most one preview is answered per frame,
  snapshots save every ~5 s, and the preview ring is 640×360 at 30 fps.
  Engine at 1920×1080 with the UI open: **120 fps, p99 23.9 ms, max 31.7 ms,
  1 stutter/10 s** (was 120 fps, p99 49.4 ms, 253 stutters/10 s). Legacy
  holds 60 fps / p99 ~20 ms.

- **Gallery-added Modules (P1) — fixed.** `modules/makeActiveModule` now
  creates the per-prop Inputs and the alpha/enabled/compositeOperation meta
  Inputs, records their ids in `$props`/`meta`, and emits `inputs/ADD_INPUT`
  deltas before `ADD_ACTIVE_MODULE`. Previously only Scene snapshots did this,
  so a Gallery drop produced a Module whose Inspector was empty. Fixtures
  regenerated; the mirror-parity test now compares 17 input records.
- **Checkbox 1/0 flags (P1) — fixed.** The shared Checkbox emits numbers;
  Module Enable stored `1`, which the strict compositor boolean check skipped
  (a fresh group appeared black), and Group Clearing/Pipeline failed the typed
  decode and snapped back. `updateMeta` coerces `enabled` with JS truthiness;
  `groups/UPDATE_GROUP` coerces the known flag keys before the schema
  round-trip and echoes booleans to the mirror.
- **Texture-free ISF generators (P1 visual) — fixed.** Legacy ISF gives
  `isf_FragNormCoord` a bottom-left origin (WebGL); the engine used top-left,
  which is correct for sampling wgpu textures but rendered coordinate-built
  generators upside down (`rave.exe` posters had inverted type). Shaders with
  no texture bindings now use a bottom-left vertex shader; filters keep the
  top-left varying so input sampling stays upright. Verified live.

**Remaining, ranked:** GPU-resample the preview before read-back (or map
async) to remove the ~32 ms full-resolution read-back outlier; the X11
obscured-window wedge; engine output window/CLI size (defaults to 1280×720,
no separate output window in engine mode); `groups/removeGroup` leaves its
Modules in the store; media-health nits (inline restart, permanent-failure
latch).
