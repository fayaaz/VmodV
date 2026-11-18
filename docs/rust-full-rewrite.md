# Rust full rewrite plan — modV Native

Status: proposal / follow-on to [rust-hybrid.md](./rust-hybrid.md)
Branch: `full-rewrite` (not started)
Last revised: 2026-11-17

This document is the porting specification for the **last step** of the
migration: deleting Electron, Chromium and Node entirely and shipping modV as
a single native Rust application. It follows on from
[rust-hybrid.md](./rust-hybrid.md), which remains the source of truth for the
engine work (WS-1 … WS-15) and whose M0–M7 milestones are assumed here.
Module-level known-differences are tracked in
[module-parity.md](./module-parity.md) and inherited deliberately (see
[Inherited module-parity gaps](#inherited-module-parity-gaps)).
Nothing in this document is actionable until the hybrid gates in
[Preconditions](#preconditions-hard-gates) are met.

Line numbers refer to this repository on the `rust-hybrid` branch (2026-11-16);
they are anchors, not contracts.

---

## Decisions at a glance

| ID  | Decision                                                                                                  | Where                                    |
| --- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| D1  | One **native single-process** app (winit + wgpu + egui); no Electron, Chromium, Node or web tech anywhere | [Target architecture](#target-architecture) |
| D2  | Engine links into the app **in-process** (library, not socket); the `modv-engine` headless binary survives | [Process model](#process-model)        |
| D3  | UI toolkit: **egui** (egui-winit/egui-wgpu/egui_dock or egui_tiles), menus via `muda`, files via `rfd`    | [Framework](#framework-decision)         |
| D4  | **One shared `wgpu::Device`/`Queue`** for UI and engine; previews are egui native textures, no readbacks  | [Sharing the GPU](#sharing-the-gpu)      |
| D5  | **JS plugins**: keep them, but sandboxed and opt-in — one embedded **Boa** instance (already vendored for expressions) serves both expressions and plugins | [JS plugins](#decision-js-plugins-js-modules-and-expressions) |
| D6  | Expressions (`new Function`) — **already ported to Boa in the engine** (WS-12); the native app reuses it — mandatory for preset compatibility | [Expressions](#expressions-are-done--the-native-app-reuses-them) |
| D7  | User-authored **JS 2D modules** (media `module/` folder) are **dropped**; ISF is the third-party format   | [User JS modules](#user-authored-js-modules-are-dropped) |
| D8  | MIDI moves fully into Rust (`midir` + polled hot-plug); the Web MIDI plane is deleted                      | [P7 — MIDI](#p7--midi-and-plugins-expressions-already-done) |
| D9  | Testing: Rust unit/integration + lavapipe goldens + **`egui_kittest`**; Playwright e2e retired (kept runnable one release) | [Testing](#testing-strategy)               |
| D10 | Packaging: Rust-only Docker build; AppImage + `.deb`; nightly releases from the native app; Electron tree deleted | [Packaging](#packaging-and-release)        |

---

## Goal

Ship modV as a native Rust desktop application ("modV Native"): a Rust UI,
a Rust render core (already built), Rust media capture, zero JavaScript at
runtime — with **unchanged** on-disk formats, user data, presets, Scenes and
playlists.

## Why

The hybrid migration removes the GPU from Chromium, but the app still pays for
an Electron shell it no longer needs for rendering:

- Chromium is a permanent ~150–250 MB RSS floor, a multi-process lifecycle to
  supervise, and a compositor stack between the user and the output.
- The UI plane is the **last** place JavaScript remains: ~62k LOC of
  Vue/JS — the entire `src/renderer`, `src/main` (5.2k), `src/preload` (0.2k),
  plus the Vitest/Playwright harness around it — every line of it existing to
  display state the engine already owns.
- The preview ring (`VMP1`), the `modv-ipc` framing, the supervisor heartbeat
  and the Vuex mirror-replay layer are all **glue between two halves of one
  program**. A single process deletes the whole class of bugs they create
  (split-brain state, snapshot drift, lost deltas, preview staleness).
- Native menus, dialogs and window management behave better than Electron's
  wrappers on Wayland, and the app gains a path to Flatpak with the
  `xdg-desktop-portal` PipeWire screen-capture flow used for camera/screen
  capture.

## Non-goals

- Rewriting any engine subsystem — `crates/` is reused as-is (≈147k LOC
  including tests, measured 2026-11-16). This plan changes **how it is linked
  and driven**, not how it works.
- Changing preset/Scene/playlist JSON formats or media-directory layout. Hard
  requirement, same as the hybrid.
- Redesigning the UI. The panels, workflows and defaults stay; this is a port.
- Windows/macOS as first-class targets. Linux first (as the project is today);
  the codebase stays portable (winit/egui/cpal/wgpu) so ports are possible
  later, but they are not scheduled here.
- Mobile, web or embedded targets.
- Keeping a JavaScript escape hatch "just in case" beyond the two in-tree
  Boa sandboxes (expressions, plugins). There is no Node in the shipped
  product.

---

## Target architecture

```text
┌───────────────────────────────────────────────────────────────┐
│ modv (single native process)                                   │
│                                                                │
│  egui UI  ── in-process calls ──▶  engine libs                  │
│  (main window)                     (modv-engine, -render,       │
│   ├─ Groups / Modules panel         -renderers, -media)         │
│   ├─ Gallery (texture previews)     ├─ scene/performance state │
│   ├─ Scenes grid                    ├─ compositor → output      │
│   ├─ Input config                   │  windows (winit+wgpu)     │
│   └─ Status bar / meters            └─ audio analysis/capture   │
│                                                                │
│  Output windows: winit windows created by the same event loop,  │
│  presented by the engine compositor (as today, modv-app/window) │
└───────────────────────────────────────────────────────────────┘
        (optional)  modv-engine headless binary
                    kiosk / LED-wall / CI: same engine, no UI,
                    driven over the modv-ipc control socket
```

### Process model

The hybrid's process split existed for two reasons: GPU-crash isolation and a
clean lifecycle. Both survive in weaker but sufficient form in-process:

- `wgpu` calls go through `Device::on_uncaptured_error` and explicit error
  scopes; a validation error becomes a logged, recoverable event, not a
  segfault. `catch_unwind` around the per-frame tick converts engine panics
  into a contained error path (frame dropped, error reported to the UI).
- The engine already persists `engine-state.json` snapshots; the app flushes a
  snapshot on every clean exit **and** on panic, so a restart restores the
  Scene/performance state (same mechanism the hybrid supervisor used).
- A hard process abort (driver SIGSEGV) still takes the UI down — accepted.
  Deployments that need OS-level crash isolation run the `modv-engine`
  headless binary under a supervisor (a ~200-line Rust re-implementation of
  `src/main/engine-supervisor.js` alongside `modv-ipc`); this is documented
  as the kiosk/LED-wall mode, **not** the default.

The default build **deletes**: the VMP1 preview ring, the SHM slot reader,
`modv-ipc` client code in the app, `src/main/engine-*.js`, and the Vuex
mirror-replay layer. The `state/delta` record format survives only inside the
headless binary's control socket.

### Framework decision

The UI is ~59 Vue components (13.3k LOC) + ~1.7k LOC of menus, dialogs,
sub-pages and ui-store behind a golden-layout docking workspace. Requirements:
webgpu/wgpu integration (previews as textures), immediate or retained mode
with fast iteration, docking, menus, text input, dark theme, Linux/Wayland
first-class, MIT-compatible license.

| Candidate         | Verdict             | Reason                                                                                                        |
| ----------------- | ------------------- | ------------------------------------------------------------------------------------------------------------- |
| **egui**          | **Selected (D3)**   | Immediate-mode, wgpu-native, MIT/Apache, mature on Wayland, trivial interop with engine textures, `egui_dock`/`egui_tiles` for docking, `egui_kittest` for tests. Fits a dense control-surface app repainted at audio-meter rate. |
| iced              | Rejected           | Elm-style architecture is a worse fit for this app's shared mutable model + per-frame meters; docking is immature; widget set for dense property panels is thinner. |
| slint             | Rejected           | GPLv3-or-commercial licensing (modV is MIT); .slint DSL is a second language to maintain; weakest wgpu story of the three. |
| dioxus (native)   | Rejected           | Web-mental-model UI without the web; smaller ecosystem for desktop-dense tooling. |
| Keep Electron     | Rejected (that is the status quo this plan removes) | Devtools are the only argument. |

egui's weak spots to manage explicitly: immediate-mode focus handling for
many simultaneous text/float fields (mitigate with a focus-request queue
pattern), clipping inside scroll areas with many custom painters (mitigate
with layered `Ui` and explicit clip rects), and font fallback for symbols the
theme uses (test on a stock system, ship bundled Inter + IAW Mono converted
from woff2 — see risks).

### Sharing the GPU

One `wgpu::Instance`, one `wgpu::Device`, one `wgpu::Queue`, owned by the app.
Each window (main UI, each output) is a `winit::Window` with its own
`wgpu::Surface`. egui renders the main window into its surface; the engine
compositor renders outputs into theirs. Previews (Gallery tiles, hover
previews, the main panel) live as GPU textures in the engine; they are
registered with egui-wgpu as native textures, so the preview path has **no
CPU readback, no SHM ring, no IPC**. Frame pacing: egui drives the main
window's `about_to_wait`; outputs present on `Fifo` with the engine's existing
frame-scheduling; UI repaints are requested only when deltas arrive or meters
need them (`egui::Context::request_repaint_after`).

Version alignment between egui-wgpu's `wgpu` and the engine's `wgpu` is a hard
build constraint: both come from the same workspace `Cargo.lock`, and P0
verifies naga versions match (egui-wgpu and `modv-renderers` both depend on
naga). If egui forces a semver bump, the engine follows in the same commit.

### Workspace layout after the rewrite

```text
crates/
  modv-engine/      unchanged (state, scenes, presets)
  modv-render/      unchanged (graph, pool, blend)
  modv-renderers/   unchanged (+ the in-process Boa plugin host, see D5)
  modv-media/       unchanged (+ midir MIDI client)
  modv-ui/          NEW: egui app — shell, menus, docking, panels, widgets, theme
  modv-app/         becomes the `modv` binary: winit loop + modv-ui + engine libs;
                    keeps `modv-engine` headless mode via --headless
  modv-ipc/         kept only for the headless binary's control socket
```

`modv-ui` internally: `shell/` (event loop, windows), `theme.rs`, `dock.rs`,
`panels/` (one module per dock panel), `widgets/` (prop-driven control set),
`state.rs` (engine handle + UI-local state), `menus.rs`, `dialogs.rs`,
`plugins_panel.rs`.

---

## Decision: JS plugins, JS modules and expressions

The hybrid plan (WS-12) kept expressions and JS plugins in the "UI plane"
because Electron still existed to run them. This rewrite deletes the UI plane,
so the decision can no longer be deferred. Three distinct JS surfaces are
involved; they get three different answers.

### The hybrid's current answer, and what changes here

The repo has already moved on from the hybrid plan's deferral:

- **Expressions are evaluated in the engine** (WS-12 as adapted): a Boa
  (`boa_engine` 0.22, `default-features = false`, pure Rust, no C toolchain)
  sandbox in `crates/modv-engine/src/expressions.rs` +
  `crates/modv-app/src/expressions.rs`, applied at every JS `applyExpression`
  site. The scope is exactly the JS `new Function` scope (`value`, `time`,
  `inputValue`, `osc` with `osc` currently fixed to `{}` — the engine has no
  OSC *receive* state yet), with loop/recursion/source limits and a denylist
  of native-escape tokens for hostile playlists. Non-finite results become
  `null`.
- **JS plugins run in a renderer-side host** in engine mode
  (`src/renderer/src/application/plugin-host.js`): hooks
  (`preProcessFrame`/`postProcessFrame`) execute in the renderer loop, one
  frame behind the engine's compositor; plugins receive **no canvas** (`grab-
  canvas` is marked unsupported); plugin store modules register on the mirror
  only; unknown mutation types are applied mirror-side and logged.

The full rewrite deletes the renderer, so the host has to be re-created
in-process, or plugins dropped. Both decisions stay (D5/D6).

### Expressions are done — the native app reuses them

Saved presets contain expressions compiled with `new Function("scope", …)`
where `scope = { value, time, inputValue, osc }`
(`src/renderer/src/application/worker/store/modules/expressions.js`). Any plan
that loses them breaks preset compatibility, which is a hard requirement.

**Decision (D6): keep the in-engine Boa evaluator** —
[`crates/modv-engine/src/expressions.rs`](../crates/modv-engine/src/expressions.rs)
is exactly the hermetic sandbox this plan originally specified, already
landed:

- Globals exposed: exactly `{ value, time, inputValue, osc, Math }`; no
  `require`, no `import`, no timers, no I/O.
- Execution is bounded (`RuntimeLimits`, loop-iteration and recursion caps, a
  token denylist for allocation/native-escape); a runaway `while (true)`
  errors instead of hanging the engine thread.
- Results are coerced and `NaN`/`Infinity` cannot escape into the store
  (serialised as `null`, the same NaN-rejection class of bug the input-link
  guard in `worker/input-link-value.js` closes).
- The per-evaluation cost is a call plus JSON↔JS conversion (compiled
  function objects are cached); there is no IPC and no per-frame parse.
- Known deviation: `osc` is `{}` until the engine owns OSC receive state
  (OSC send exists in `modv-media`; receive binding into expression scope is
  open work, tracked as an engine item, not a UI one).

Remaining work is parity-hardening, not a port: extend the fixture corpus
(real expressions extracted from saved presets) against the legacy `eval`
results, and gate on `evaluateEveryFrame` semantics.

### JS plugins are kept — sandboxed and opt-in

The only built-in plugin is `grab-canvas` (186 LOC): it copies the composited
frame into a tiny mapping canvas and pushes RGB arrays to a WebSocket for
Frame-fixture mapping (luminave-style). The plugin API
(`src/renderer/src/application/worker/store/modules/plugins.js`) supports
`props`, `init`/`shutdown`, `preProcessFrame`/`postProcessFrame`, a Vuex
store module, and Vue UI components. Note that plugins have **no** disk
drop-in today: the `eval`'d path in `media.js` loads user Canvas2D
**modules**, not plugins; the only plugin install path is `modV.use(plugin)`
with a plugin object already in memory — `grab-canvas` is the only plugin
and it is bundled at build time. D5 proposes adding an opt-in disk scan, it
is not restoring one.

In engine mode today, `application/plugin-host.js` keeps plugins working by
running the hooks in the renderer loop and forwarding their writes — hooks
are one frame behind the compositor, plugins get **no canvas**, and
`grab-canvas` is marked `supportsEngine: false` for exactly that reason. The
native app has no renderer to hide this in, so the host must be re-created
properly (or plugins dropped).

**Decision (D5): one Boa instance dedicated to plugins, off by
default, with a restricted host API.** Not a JS engine requirement — a
power-user switch with an explicit consent flow. Boa is already vendored
(`boa_engine` 0.22, default-features off) and runs the expression sandbox, so
the D5 host shares one engine dependency, one build, one set of JS semantics
to document. (An earlier draft of this plan chose `rquickjs`; Boa landed in
the tree first, is pure Rust and keeps the Docker toolchain C-free, so the
plan standardises on it. The layering, the opt-in consent flow and the host
API are unchanged.)

| Surface                                   | Kept? | Notes                                                        |
| ----------------------------------------- | ----- | ------------------------------------------------------------ |
| Plugin `props` + `init`/`shutdown`        | Yes   | Prop schema drives an auto-generated settings UI (the same schema-driven approach as Module controls — see P3). No hand-written UI per plugin. UI differences driven by plugin `store` modules are *not* portable (see below). |
| `preProcessFrame` / `postProcessFrame`    | Yes   | Receives a downscaled RGBA buffer (mapping size, e.g. 7×7 by default — plugin declares `captureWidth/captureHeight`) read back from the final target on demand. Readback is opt-in per enabled plugin and off the hot path otherwise. Hooks run at the frame boundary, not inside the compositor, so they are one pass from the pixels the Output presents — better placement than the engine-mode renderer host, which is a full frame behind. |
| `store` (plugins registered Vuex modules) | **No**| No Vuex exists. Plugins get a narrow `modV.get(path)` read-only accessor over allow-listed engine state (props of enabled modules, beat, bands) — the same mirror-only model the engine-mode `plugin-host.js` uses, but single-process. |
| Vue UI components (`galleryTabComponent`, `controlPanelComponent`, `install(app,…)`) | **No**| No Vue exists. A plugin is configured entirely through its generated `props` panel. |
| WebSocket / network                       | Yes   | Host-provided `modV.ws.connect/send/close` only (no arbitrary sockets, no HTTP) — exactly what grab-canvas needs. |
| `log`                                     | Yes   | Forwarded to the shared daily log with a plugin tag.         |
| `require` / npm / ES modules / file read  | **No**| Single-file plain JS only. Plugins using npm deps must ship a pre-bundled file (docs: `npx esbuild plugin.js --bundle`). |
| Plugin disk drop-in (new, opt-in)     | Yes, opt-in | Settings toggle **“Enable JavaScript plugins (advanced)”**, off by default; first-enable consent dialog; `notify`-watched `<media>/<project>/plugin/*.js`, loaded into the sandbox. No disk plugin API exists today — this is proposed, not a regression fix. |

Security posture is strictly better than today (today's plugin runs `eval`
with full renderer privileges, including network, DOM and IPC) and better than
the engine-mode stopgap (`plugin-host.js` runs unbounded JS in the renderer).
The manifest stays JSON-ish: same `name`/`props` shape; `plugins` list
persisted in the engine state so an enabled plugin survives restart. The
renderer-side `plugin-host.js` is deleted with the renderer at P9; it remains
the behavioural reference for the in-process host (hook order, prop defaults,
store facade, the "log once per unknown mutation" policy).

`grab-canvas` is reimplemented against this API and shipped bundled, **off by
default**. The user does not use it; the switch exists because the ecosystem
(largely luminave/LED-mapping users) does.

### Why not the alternatives

| Alternative                                  | Verdict    | Why                                                                 |
| -------------------------------------------- | ---------- | ------------------------------------------------------------------- |
| Drop plugins entirely                        | Rejected   | Leaves the LED-mapping use case with no path (legacy build dies). Cheap to keep once Boa exists for expressions. |
| WASM-only plugin ABI (wasmtime)              | Kept as P10, not v1 | Right long-term sandbox, but zero ecosystem, heavy authoring lift, and grab-canvas would be a rewrite nobody has asked for. WASM is the post-parity hard-sandbox for *untrusted, performance-critical* modules. |
| `rquickjs` (QuickJS) instead of Boa          | Rejected   | Smaller and ES2020-exact, but adds a second JS engine to the product and a C toolchain to the Rust Docker build to replace a working one. Keep one engine, Boa, already vendored. |
| Node/Electron kept just for plugins          | Rejected   | The entire point of this plan.                                        |

### User-authored JS modules are dropped

The media `module/` folder (webpack-compiled Canvas2D JS modules, `eval`'d
into the worker) is **not ported** (D7). Rationale:

- They are the riskiest runtime surface in the product (arbitrary JS with a
  full Canvas2D API) and the media-manager side of them depends on
  webpack + npm at runtime (`src/main/media-manager/read-handlers/module.js`),
  which is a supply-chain and maintenance liability.
- Their role — community extensions — is already served better by ISF: 351
  shaders ship, users drop `.fs` files into the ISF folder and they appear in
  the Gallery with synthesised props, no recompilation, no eval. ISF is the
  third-party module format of modV Native.
- Usage is **zero on the maintainer's machine** (checked: both
  `~/.config/VmodV/media/default/module/` and `…/module/compiled/` are
  empty) — the maintainer does not use them, and the hybrid already lists
  them as an M7 documented gap.

**Migration path (documented, not tooling):** most Canvas2D module ports to
ISF mechanically (draw calls → fragment shader; feedback → ISF `PASSES` +
`PERSISTENT` buffers; audio bands → uniform-driven math). Modules that cannot
express themselves as shaders can be rewritten against the P10 WASM ABI when
it lands, or run in the last Electron build.

### The built-in JS 2D corpus is unaffected — except two scenes

For completeness on what "dropped" means for the built-in tree:

- The **15 hand-written 2D modules** (`Ball`, `Bar`, `Buffer`, `Concentrics`,
  `Counter`, `GridStretch`, `Line`, `Pixelate`, `Polygon`, `Smear`, `Text`,
  `Texture2d`, `Waveform`, `Webcam`, `X-Drips`) ship in the engine already —
  ported in M3. Their `.js` sources are deleted at P9, nothing else changes.
- The **27 `Point10*.js` wrappers** are 8-line shims; the engine registers the
  underlying scenes natively (incl. `BallOfFire`; 27 Point10 scene classes
  after Geometric Tunnel was retired in both runtimes, `DEFAULTS_VERSION` 16).
  Deleted bookkeeping at P9.
- **`point10/scenes/Earth.js` and `Sun.js` are no longer missing** — both
  were ported to the engine and registered on 2026-11-16 (`cbb24d8a`,
  `point10/scenes/earth.rs` / `sun.rs`, 3d-scenes category, matching prop
  schemas). The built-in Gallery is now identical between the runtimes.
  Meanwhile `Cube` and the `three` renderer were removed from the legacy
  build too (`f7d35db0`), and `Point10 Geometric Tunnel` was retired in
  both runtimes with the five affected default Scenes retuned in place
  (`DEFAULTS_VERSION` 16, JS and Rust) — so even the "unported scene"
  category no longer exists.
  (These are the module-parity items 1, 2 and the companion removal; see
  [module-parity.md](./module-parity.md), which is now stale —
  it audits HEAD `a13dacd8`.)

---

## Preconditions (hard gates)

The full rewrite does not start — not even P0 — until:

1. **Hybrid M6 media is engine-owned** — cameras (V4L2, incl. non-native
   sizes), video files (uridecodebin), PipeWire portal screen nodes and test
   patterns all run in the engine through `MediaHub` (validated on real
   hardware). This gate is **met**, with one carve-out: in engine mode the UI
   screen picker is still gated because Electron does not expose the portal
   PipeWire node id; the engine path itself is implemented and tested. The
   native app has no Electron, so the same portal flow runs in-process via
   `ashpd` (see P4) — the carve-out disappears, it is not waived.
2. **Hybrid M7 exit criteria met**: engine is the default path, the e2e suite
   is green against the packaged build. The old "7 pending ISF overrides"
   item is resolved — the corpus now translates with zero overrides, and the
   corpus gate asserts a checked-in baseline (100 %,
   `docs/isf-corpus-report.md`). Screen-capture UI parity completes here
   (gate 1's carve-out).
3. **Module-parity triage** (from [module-parity.md](./module-parity.md)):
   the built-in-set delta is **closed** (Earth/Sun ported; `Cube` and
   Geometric Tunnel removed both sides); the inert-module gaps are closed
   (texture registry + ISF transitions bind real frames); third-party ISF
   registration and the regression guard are scheduled (P6 and P8). The audit
   doc — refreshed 2026-11-17 — see [module-parity.md](./module-parity.md).
4. **P0 framework spike signs off** (below).
5. **This document is signed off**, specifically D5/D6 (Boa plugins/expressions
   decisions) — they affect preset compatibility and are expensive to reverse.

## Inherited module-parity gaps

[module-parity.md](./module-parity.md) (originally audited at HEAD `a13dacd8`,
2026-11-12; refreshed 2026-11-17 at HEAD `dd8914c2`) quantified the
engine↔legacy Gallery delta. Almost all of it closed in the days after the
audit; the remainder are scheduled here rather than rediscovered during P4/P8.

| module-parity item | Status | Where |
| ------------------ | ------ | ----- |
| 1 — Earth / Sun    | **Landed 2026-11-16** (`cbb24d8a`) — ported on the UltraHDR pattern and registered | closed |
| 2 — `Cube`         | **Landed 2026-11-16** — `three` renderer and `Cube` removed from *both* runtimes (`f7d35db0`) | closed |
| 3 — Texture registry | **Landed 2026-11-13** (`e4cf3e26`, `modv-app/src/texture_sources.rs`): image pool (LRU by path+size, `image` crate decode), media-library paths under the compositor media root, live surfaces by id (`isf:`/`two-d:`/`point10:` module outputs, `group:` canvases, `camera:`/`video:`/`screen:` sources), unresolved/inherit/null keep the group canvas, self-reference falls back. ISF image inputs bind assigned textures; `Texture 2D` draws its `texture` prop through the same `TextureRequest` path | closed — this is the workstream this plan originally called **M8** |
| 4 — ISF transitions | **Landed 2026-11-13** (`f591a108`, `scene_transition.rs` + `performance.rs`): captured from/to frames bind as `startImage`/`endImage` on ISF shaders registered for transitions — the six natives already used that path, the 27 corpus transitions joined them | closed |
| 5 — Third-party ISF | **Open.** No `modules/registerModule` arm for user `.fs` drops (translate/register pipeline exists; only watch + index plumbing is missing) | **P6** |
| 6 — Regression guard | **Open.** A test asserting the JS glob-extracted registry equals the Rust registry + an exact e2e count assertion (`engineParity` asserts `>300` today) | **P8** |
| 7 — Hygiene | **Partial.** Geometric Tunnel retired both sides; doc counts still inconsistent (424/428/429/431/432) | **P8** |

So the engine-vs-legacy delta for the **built-in** module set is now zero —
the native port inherits a parity problem only for third-party ISF (5) and
the guard (6). Nothing here blocks P0–P2.

## Migration map and milestones

```text
hybrid:  M0 spike → M1 skeleton → M2 shader/ISF → M3 2D → M4 Point10
         → M5 scenes/perf → M6 media (engine-owned cameras/files/screen)
         → M7 parity/release (GATE: engine default)
           [post-audit M4/M6 work landed: Earth/Sun port, texture registry,
            ISF transitions, three-renderer/Cube removal, Geometric Tunnel
            retirement, defaults v16 — the native port inherits no engine
            porting work; see "Inherited module-parity gaps"]
native:  P0 framework spike → P1 shell → P2 engine in-process → P3 widgets
         → P4 panels → P5 docking/menus/shortcuts → P6 media manager
         → P7 MIDI + plugins (expressions already done) → P8 parity & hardening
         → P9 packaging & release → P10 (post-parity) WASM plugin ABI
```

Rules (identical to the hybrid): test-first; the legacy Electron build remains
the shipping product until P9; nothing deletes until parity is signed off;
`main` keeps nightly Electron releases until then. The `full-rewrite` branch is
where all work lands.

### P0 — Framework spike (go/no-go)

**Scope.** A `modv-ui` skeleton opening one winit window via egui-winit/
egui-wgpu sharing the engine's `wgpu` device; three probes:

1. The hardest widget — the **Scenes grid cell** (from `Scenes.vue`: hover
   previews, Ctrl+drag copy, context menu, timing badges) as an egui widget.
2. The highest-churn widget — **RangeControl** (402 LOC: drag, scrub, typed
   input, expression/tween badges) as an egui widget.
3. An **output window** presented by the engine compositor alongside the egui
   main window, with a live Gallery tile fed by a registered egui native
   texture (proves D4 end-to-end, zero readbacks).

**Exit.** 60 fps main window with two animated panels on lavapipe **and** on
the reference GPU; no readbacks in a frame trace; texture registration API
works with the pinned egui-wgpu version; binary RSS ≤ Electron hybrid build;
written go/no-go. If egui fails, P0 is repeated with iced (timebox 1 week);
if both fail, the plan stops and Electron stays.

### P1 — App shell

**Scope.** `modv` binary: winit event loop hosting main window + output
windows; `muda` menu bar (File/Edit/Scene/View/Help — port the command set
from `src/main/menu-bar.js`, 296 LOC); `rfd` file dialogs; tracing logger
with daily rotation (replacing `src/main/logger.js` + renderer forwarding);
window-state persistence (port `src/main/window-state.js`, 537 LOC — display
id, bounds, maximised/fullscreen, debounced flush); shutdown coordinator
(port `src/main/shutdown.js` — snapshot flush on exit and on panic hook);
single-instance lock; splash screen (trivial egui window, shown while state
loads); color-picker entry (becomes an in-app egui window); app icon; theme
ported from the CSS (`iaw.css`, `golden-layout_theme.css`, `raster.css`) to
`theme.rs` (egui `Visuals` + `FontDefinitions`); bundled Inter (woff2 →
converted embeddable format) and IAW Mono.

**Exit.** Native app opens, paints theme, saves/restores window placement,
menus work or are stubbed with tracking issues, quits cleanly with no orphan
processes. No engine wiring yet — the window is chrome only.

### P2 — Engine in-process + state plane

**Scope.** Delete the socket between UI and engine in the app (keep
`modv-ipc` for `--headless`); `modv-app` starts engine libs directly.
Reimplement `window.modV.store.commit/dispatch` semantics in-process:
`EngineHandle` (a struct with channels) exposes `commit`/`dispatch`/
`subscribe`; the mirror-replay layer (`application/index.js` engine client +
the renderer mirror) disappears — panels read engine state directly and
subscribe to the in-process delta stream for invalidation (list invalidation,
Gallery registration changes, Scene list changes). `application/plugin-host.js`
(the engine-mode JS plugin host) is deleted with the renderer; its contract is
the reference for the in-process host in P7. UI-local state (focus,
dialogs, search, info view, UI-only groups/modules) ports from
`ui-store` (389 LOC) to plain Rust structs in `modv-ui::state`. `modV.store`
call sites in panels become `handle.dispatch(...).await`.

**Exit.** A panel renders live engine state (Groups list) updated by deltas;
`dispatch` resolves with action results; engine snapshot round-trips through
panic-restart; zero references to `state/delta` outside the headless binary;
headless `--headless` still passes its IPC tests.

### P3 — Controls (the prop-driven widget set)

**Scope.** Port the Auto-Generated control layer. `Control.vue` (281)
dispatches on prop type; the same table drives egui:

| Prop type | Widget | Source |
| --------- | ------ | ------ |
| `float` / `int` | `RangeControl` (slider + scrub + typed field + min/max) | `Controls/RangeControl.vue` (402) |
| `bool` | checkbox | `inputs/Checkbox.vue` |
| `text` | plain text field | inline in `Control.vue` |
| `enum` | dropdown | `inputs/Select.vue` |
| `event` | momentary button | `inputs/Button.vue` (inline in `Control.vue`) |
| `color` | swatch + egui color picker window | `Controls/ColorControl.vue` (298) |
| `vec2` | drag fields | `Controls/Vec2DControl.vue` (163) + `Controls/Vec2DXY.vue` (283) |
| `vec3` / `vec4` | X/Y/Z[/W] drag fields | `Controls/Vec3Control.vue` (203), `Controls/Vec4Control.vue` (244) |
| `texture` | source picker (image/video/canvas/group) | `Controls/TextureControl.vue` (353) (+ `Controls/VideoControl.vue` (108)) |
| `palette` | palette editor + tween gradient | `Controls/PaletteControl.vue` (267) |
| `tween` | the 1064-LOC `TweenControl.vue` port (easing picker, BPM division, loop/alternate, steps) | hardest widget in this phase |
| `font` | font registry picker | `Controls/FontControl.vue` (207) |

`ActiveModule` (the draggable module card in a Group, 333 LOC: drag handle,
focus/keyboard-delete, status badges, context menu) and
`ModuleControl`/`ModuleInspector` (the auto-generated property panel wrapper
+ focus/pin behaviour, 92 + 99 LOC) come along in the same phase — panels
need them to show anything.

**Exit.** Golden preset renders full property panels for one shader module,
one ISF module, one Point10 module and one 2D module; editing any prop type
and seeing the module react within one frame; palette tweens advance;
egui_kittest snapshots for every widget type; prop coercion parity with the
engine's `props::coerce` (already tested there). Texture props already
resolve through the hybrid's texture registry (image pool LRU, live module
outputs / group canvases / `camera:`/`video:`/`screen:` sources; unresolved
values keep the group canvas) — no decorative-prop caveat remains.

### P4 — Panels

**Scope.** Port the dockable panels, largest first.

| Panel | Source | LOC | Notes |
| ----- | ------ | --: | ----- |
| Scenes grid | `Scenes.vue` | 2212 | The biggest single port; cells, rows, row-name editor, hover previews via engine `preview/scene`, Ctrl+drag copy, playlist import/export, transition picker, context menus. Hover preview calls the engine directly (no deferral IPC). |
| Gallery | `Gallery.vue` (332), `GalleryItem.vue` (473) | ~800 | Tiles poll `preview/module` only while visible and intersecting the viewport (engine-mode behaviour — port it); search; category sections; drag-to-Group. |
| Groups / Modules | `Groups.vue` (115), `Group.vue` (780), `ModuleControl.vue` (92), `ModuleInspector.vue` (99), `ABSwap.vue` (16), `TooltipDisplay.vue` (16) | ~1.2k | Layer list, visibility toggles, alpha, blend, inherit, drag-reorder, per-group context menu; drag-and-drop via egui DnD. |
| Input Config | `InputConfig.vue` (226) + `InputLinkComponents/*` (5 files, ~550) | ~800 | Input list, add/assign, link editor (audio features / state / MIDI / OSC), expression badge. |
| Audio/Video Device Config | `InputDeviceConfig/AudioVideo.vue` | 749 | No new engine work — the engine enumerates V4L2 cameras with caps directly (`v4l` crate) and the `media/list-cameras`/`start-camera`/`start-file`/`start-screen`/`stop-source`/`status` actions already drive sources (engine-mode UI does exactly this today). P4 is a panel UI over those actions. |
| Screen capture | `InputDeviceConfig/ScreenCapture.vue` | 359 | Engine `pipewiresrc` path implemented + tested against a live portal PipeWire node; the UI is gated today only because Electron does not expose the node id — in the native app `ashpd` requests the node in-process and the gate disappears. Delete `ScreenPickerDialog.vue`, the Electron portal IPC and the gating code. |
| MIDI | `InputDeviceConfig/MIDI.vue` (142) | ~150 | MIDI sources, MIDI-learn assignments (needs P7 midir work first). |
| BPM | `InputDeviceConfig/BPM.vue` (119) | ~120 | BPM source selection (audio/MIDI), manual tap. |
| NDI | `InputDeviceConfig/NDI.vue` | 246 | Engine NDI already exists; panel is list + add/remove receiver/sender. |
| OSC | OSC panel | ~150 | Add/remove server, address→prop binding view. |
| Info View | `InfoView.vue` | ~50 | Markdown-ish info + keyboard shortcuts list. |
| Search | `Search.vue` | 300 | Fuzzy command search (Ctrl+K) over modules/actions. |
| Status bar | `StatusBar/*` (5 files) | ~400 | FPS graph, frame time, resolution, BPM, engine health. |
| Plugins panel | `Plugins.vue`, `PluginControl.vue` | ~200 | Lists enabled JS plugins (D5); auto-generated prop panel; enable/disable. |
| Error watcher | `ErrorWatcher.vue` | ~50 | Engine error events → toast list. |
| AB Swap | `ABSwap.vue` | ~20 | A/B preset compare. |

**Exit.** Every panel functional against the engine; a 30-minute hands-on
session finds no P1-severity usability defects vs the Electron build;
kittest covers panel smoke (render, interact, assert dispatch).

### P5 — Docking, layout, menus, shortcuts

**Scope.** Replace golden-layout with `egui_dock` (or `egui_tiles`, decided by
P0): same default arrangement as `App.vue`'s `layoutConfig` (Groups+Scenes
stack | Inspector; InfoView | Gallery | stack(Input Config | AVD Config |
Screen | MIDI | BPM | …)); layout persistence to `window-state.json`-adjacent
`layout.json` (old golden-layout state detected → reset to default, one-time
notice); drag-dock/undock; double-click header collapse; popout output
window toggle; full keyboard shortcut map (Ctrl+S save preset, Ctrl+Enter
Scene apply, number keys row select, etc. — from `menus` + docs); context
menus (right-click): active module (duplicate/remove/move), group, scene
cell, gallery item, BPM panel, column headers — port all six from
`menus/context/*.js` (255 LOC total).

**Exit.** Layout round-trips through restart; shortcuts documented and tested;
context menus match parity checklist.

### P6 — Media manager in Rust

**Scope.** Replace `src/main/media-manager/**` (~1.4k LOC) — but note it
shrinks dramatically: the engine already loads ISF (`--isf-dir`), images and
runtime media sources (`MediaHub`: cameras, files, screen nodes, test
patterns). Remaining work: `notify`-based watcher over the project media
directory; read/save handlers (preset, scene, palette, isf passthrough);
project profile creation (`fs-create-profile.js`, 80 LOC); folder rescan →
engine registration events; "open in app" for preset/scene files (argv);
media permission checks (`check-media-permission.js` — becomes trivial, native
file APIs). **Deleted entirely**: the webpack/npm `module/` handler (D7),
ISF corpus handling (engine), images/videos read (engine), the
`eval(text).default` third-party-JS path (D7).

**Inherited work that lands here (module-parity item 5)**: a user ISF
directory scan + watch/reload — the `.fs`→translate→register pipeline all
exists; only the watch + index plumbing and an engine `modules/registerModule`
arm for dropped `.fs` files are missing. This is what makes "drop a `.fs`
into the media ISF folder and it appears in the Gallery" true, and is the
load-bearing piece of D7's claim that ISF is the third-party format. (The
image-pool half this plan once lumped into M8 already landed —
`texture_sources.rs`, 2026-11-13 — only the watch/registerModule arm
remains.)

**Exit.** Drop a `.fs` file into the media ISF folder → appears in Gallery
without restart; save preset round-trips; watcher ignores its own writes;
unit tests for the watcher debounce + profile creation.

### P7 — MIDI and plugins (expressions already done)

**Scope.**

- **MIDI (D8).** `midir` client in `modv-media`: enumeration by port name,
  hot-plug by polling `ports()` at 1 Hz and diffing (midir has no
  notifications); identity synthesised as `${port_name}-${running_id}`;
  manufacturer unavailable — documented, existing learn mappings migrate if
  `${id}-${name}` still matches by name. MIDI-learn flow moves into the
  Input Config panel; CC→0..1 normalisation stays in the engine path; clock
  (248) → BPM already lives in the engine (WS-11). The `setup-midi.js` and
  Web MIDI bridge are deleted; the engine becomes the single owner of all
  MIDI.
- **Expressions (done).** The Boa evaluator already ships in the engine
  (`crates/modv-engine/src/expressions.rs`); P7 keeps the expression badge
  and extends the fixture corpus (real expressions from saved presets
  replayed against legacy results). No port work. `osc` in expression scope
  needs the engine's OSC receive state wired in when it exists (currently
  `{}`), an engine item.
- **Plugins (D5).** The restricted Boa host, **off by default** behind
  "Enable JavaScript plugins" in Settings with a first-enable consent
  dialog; the host API (`props`, `modV.get`, pre/post-frame downscaled RGBA
  capture, `modV.ws`, `log`); auto-generated config UI from `props`;
  `grab-canvas` ported and bundled (off by default). `plugin-host.js`'s
  contract (hooks at the frame boundary, mirror-only plugin stores, log-once
  unknown-mutation policy) is the reference for the in-process replacement.

**Exit.** MIDI CC drives a prop with the same latency as the legacy build on
a fixed trace; hot-plug appears ≤ 1.5 s; the expression corpus fixtures pass
on the engine; grab-canvas drives luminave from the in-process WebSocket
path; plugins disabled ⇒ zero Boa plugin allocation.

### P8 — Parity and hardening

**Scope.** Test parity (below); **module-parity closure** — remaining item
5 (third-party ISF registration via P6), item 6 (the registry-equality guard
test), item 7 (hygiene: `GreatBallOfFire.js`, counts normalised, stale doc
comments). Items 1–4 are already landed: Earth/Sun ported (`cbb24d8a`), `Cube`
removed (`f7d35db0`), the texture registry binds assigned ISF image inputs and
`Texture 2D` (`e4cf3e26`), ISF transitions render (`f591a108`); performance
harness re-run against the Electron baseline (same scenes/fixture, same
session, p50/p90/p99, CPU, RSS); memory-leak hunt over 2 h performance-mode soak; crash-recovery
drills (SIGKILL mid-frame → restart → state restored ≤ 5 s); Wayland + X11
pass on labwc/KWin/Mutter; NDI out/in; OSC; system audio (PipeWire)
end-to-end; log review; `make lint`-equivalent (clippy `-D warnings`);
AGENTS.md rewrite (draft); user documentation (migration guide, plugin guide,
ISF guide).

**Exit.** Parity checklist (Appendix B) signed off; performance targets from
rust-hybrid.md still met or improved; no P1 bugs; the 144 e2e scenarios
mapped (Appendix B) or explicitly waived with rationale.

### P9 — Packaging and release

**Scope.** Docker build becomes Rust-only (drop Node/yarn/electron stages;
keep lavapipe + the Rust toolchain); AppImage + `.deb` via the existing
multi-stage build, emitting `modv`; bundled assets: ISF corpus + docs + icons
ship beside the binary (as today via `resources/isf`); the AppImage keeps
hosting PipeWire/GStreamer from the host (audio capture and engine video
already depend on it — the `.deb` declares both, the AppImage documents both);
Flatpak manifest (GStreamer ships in the freedesktop runtime; PipeWire via
portal); nightly workflow switched to Rust; release workflow publishes the
rolling nightly; version scheme: the first native release ships as **2.0.0**;
`README.md`, `docs/` and AGENTS.md rewritten; the Electron tree
(`src/`, `electron.vite.config.mjs`, `package.json`, `node_modules`,
`patches/`, `tests/e2e`, `tests/unit`, Playwright/Vitest configs) **deleted** —
including `plugin-host.js` and `install-plugin.js`;
last Electron build archived as `legacy-electron` branch tagged
`v1.x-final`.

**Exit.** Clean-machine check (fresh VM, no Node, no Chromium): download
AppImage → launch → audio plays → a Scene applies → quits cleanly; `.deb`
same; nightly CI green; docs merged.

### P10 — (post-parity) WASM plugin ABI

**Scope.** Optional, scheduled only if third-party demand materialises.
`wasmtime` component-model ABI: `pre_frame`/`post_frame`/`on_event`, host
API (downscaled frame, props, ws/osc, log), deny-by-default imports, same
auto-generated props UI and consent flow. Not a v1 blocker; see JS-plugins
section.

---

## Testing strategy

The Playwright suite (144 specs, ~9.4k LOC) cannot survive
(D9). Replacement matrix:

| Layer              | Tool                                   | Coverage anchor                                        |
| ------------------ | -------------------------------------- | ------------------------------------------------------ |
| Engine logic       | `cargo test` (existing, ~1950 tests)   | unchanged — the Rust suite **is** the reference now    |
| Golden images      | lavapipe goldens (existing harness)    | unchanged; per-module/scene goldens already exist      |
| UI widgets         | `egui_kittest` snapshot + interaction  | every control type in P3; panel smoke tests in P4      |
| UI ↔ engine        | in-process integration tests           | dispatch/commit/subscribe, snapshot restore, deltas    |
| End-to-end smoke   | `modv --headless` + scripting CLI      | scene apply, performance mode tick, NDI/OSC round-trip |
| Real-GPU/manual    | release checklist                      | Wayland/X11 × compositors matrix                       |
| Regression net     | keep the legacy suite on `legacy-electron` for one release cycle | run it against the last Electron build; file parity bugs into P8 |

Each of the 144 e2e specs is mapped in Appendix B to a Rust test, a
`egui_kittest` interaction, a manual check, or an explicit waiver. The Vitest
unit suite (~15k LOC, 1165 tests) is deleted at P9; the pure-logic reference
implementations it backed are already ported and Rust-tested by the hybrid
work.

---

## Packaging and release

- **Linux**: AppImage (primary, as today) + `.deb`; optional Flatpak in P9.
  Bundled: binary, ISF corpus, icons, docs. Runtime deps unchanged:
  `libpipewire-0.3`, the host GStreamer runtime (engine video capture and
  screen PipeWire nodes already require it — `.deb` declares it, AppImage
  documents it), `libvulkan` (or lavapipe via Flatpak/mesa).
- **CI/nightly**: `.github/workflows/nightly.yml` switches to
  `cargo build --release` + `cargo test` + lavapipe + packaging; matrix:
  stable toolchain only (edition 2024). No Node except where unavoidable
  (none).
- **Sizing**: the native binary + assets ≈ 25–40 MB (vs ~300 MB AppImage
  today) — an explicit release-note item.
- **Migration for users**: none — same `~/.config/VmodV`, same JSON, same
  media tree. The `window-state.json` is reused; `layout.json` is new
  (old layout reset, announced in release notes). Plugins see D5/P7 consent.
- **Version**: first native release is **2.0.0**. `MODV_ENGINE` /
  `MODV_LEGACY_RENDERER` env vars disappear; `MODV_AUDIO_SOURCE` survives on
  the `--headless` binary only.

---

## Data compatibility and migration

| Artifact                                    | Fate in modV Native                       |
| ------------------------------------------- | ----------------------------------------- |
| Presets / Scenes / playlists (JSON)         | Unchanged; round-trip tests keep passing   |
| `scenes.json`, `seededDefaultScenes`        | Unchanged; seeding already ported (M5)     |
| Media directory (`<media>/<project>/…`)     | Unchanged                                 |
| `<userData>/logs/*.log`                     | Unchanged path; format via shared formatter |
| `window-state.json`                         | Read + migrated in P1                      |
| golden-layout layout state                  | Detected, reset to default, logged once    |
| Plugin enablement set                       | New `plugins.json` (off by default)        |
| MIDI device mappings                        | Name-keyed; manufacturer field dropped     |

---

## Risks

| ID  | Risk                                                          | L   | I   | Mitigation |
| --- | ------------------------------------------------------------- | --- | --- | ---------- |
| R1  | egui cannot express a dense desktop panel UI at this size     | Med | High| P0 probes exactly this (Scenes cell, RangeControl, Texture previews) on both lavapipe and the reference GPU; iced fallback; Electron remains until P9 |
| R2  | egui-wgpu ↔ engine wgpu/naga version skew                     | Med | High| Single workspace lock; P0 gate; engine follows egui's bump in the same commit |
| R3  | Loss of Playwright e2e as a safety net                        | Med | Med | egui_kittest + in-process integration tests; legacy branch runnable one release; explicit waiver list in Appendix B |
| R4  | Single-process GPU crash takes the app down                   | Low | High| `on_uncaptured_error` + error scopes + `catch_unwind` on the frame tick; snapshot restore on relaunch; headless binary documented for isolation-sensitive deployments |
| R5  | Boa expression semantics diverge from saved preset semantics | Low | High| Corpus replay tests against the legacy `eval` results (the evaluator already ships with loop/recursion/source limits); `NaN`/`Infinity` rejection identical to today; `osc` scope gap documented until engine OSC receive lands |
| R6  | Boa binary/constraint creep (build time, size)                | Low | Med | Boa 0.22 is already a vendored dependency (expressions ship in the engine); measure bundle impact in P7; budget ~5 MB; pure-Rust build keeps the Docker toolchain C-free |
| R7  | MIDI identity change breaks saved learn mappings              | Med | Low | Name-keyed mapping with documented migration; remap prompt in UI |
| R8  | Fonts: IAW Mono/Inter woff2 → egui needs ttf/otf; glyph drift | Med | Low | Convert with subsetting at build time; golden-image the Text module; fall back to Inter only |
| R9  | Scope (again) — this plan is larger than the hybrid           | High| Med | Same rule: hybrid ships and stays default until P9; milestones independent; P0 is a real gate |
| R10 | Docking behaviour mismatch (split/tab/popout) vs golden-layout| Med | Med | egui_dock vs egui_tiles bake-off in P0; P5 dedicated phase; layout reset is an acceptable loss |
| R11 | Portal UX regression (screen picker) vs Chromium picker       | Med | Med | Native ashpd picker UI or portal sysfs restore dialog; dogfood on GNOME/KDE |
| R12 | AppImage/Flatpak vs PipeWire/screen-capture sandboxing        | Med | Med | Flatpak portal route is the clean path; AppImage keeps host PipeWire (as today); NDI in Flatpak documented |
| R13 | NDI FFI and libpipewire runtime deps on clean machines        | Low | Med | `extraResources`-equivalent bundling of libndi as today; `.deb` depends on distro packages; release-checklist on a clean VM |
| R14 | Maintenance of two apps during the transition                  | High| Low | The Electron tree is **not** modified on `full-rewrite` (engine libs only); rebase cost is the engine's, and only for P2's in-process refactor |

## Success criteria (branch `full-rewrite`)

1. `modv` opens a native main window on Wayland (and X11), presents outputs
   fullscreen, and quits cleanly with no orphan processes.
2. Every default Scene applies and renders; performance mode runs 10 minutes
   without leaks; hover previews work; Gallery previews render.
3. Presets/Scenes round-trip; the Gallery group is preserved; playlists
   import/export; a fixture with expressions renders identical prop values to
   the legacy build.
4. module-parity items 5–7 closed or explicitly waived: third-party ISF
   registration works via P6; the registry-equality guard test (JS glob ≡
   engine registry) is green; the doc-count and `GreatBallOfFire.js` hygiene
   items land. (Items 1–4 are closed already — Earth/Sun ported, `Cube`
   removed, texture registry and ISF transitions bound.)
5. p50/p90/p99 frame time at 1080p ≤ hybrid baseline; CPU/output frame and
   RSS reduced ≥ 25 % vs the Electron build; preview path has no readbacks.
6. The full parity checklist passes; the AppImage and `.deb` run on a clean
   machine (with the documented PipeWire/GStreamer host deps); nightly CI
   produces a rolling pre-release from the native app.

## Open questions

| #   | Question                                                              | Recommendation | Decide by |
| --- | --------------------------------------------------------------------- | -------------- | --------- |
| 1   | Keep an optional two-process mode (UI ⇄ engine over `modv-ipc`) as a supported configuration? | No — one default, one code path; the headless binary covers kiosk/CI. Revisit only if a real crash-isolation deployment appears. | P2 |
| 2   | `egui_dock` vs `egui_tiles` for docking                              | Bake-off in P0 on the Scenes/Inspector split; `egui_dock` preferred for its familiar tab/split model | P0 |
| 3   | Flatpak as the recommended install (vs AppImage)?                     | Recommend Flatpak for desktop users (portal-native PipeWire), keep AppImage for parity with today's install path | P9 |
| 4   | Ship Windows/macOS builds once Linux is green?                        | Out of scope here; re-plan after 2.0.0; winit/egui/wgpu keep the door open | post-2.0 |
| 5   | Should the JS plugin API include a limited `fetch`-like HTTP verb (not just WebSocket)? | No for v1 — WebSocket only; add only on measured demand | P7 |
| 6   | Keep the splash screen, or drop it (engine loads fast)?               | Keep a minimal one; init is not free (ISF corpus + state parse) and a blank window is worse | P1 |
| 7   | Rename the app/binary (`modv` vs `vmodv` vs `modv-native`)?           | `modv`; userData dir stays `VmodV`; package name `vmodv-native` only in packaging metadata | P9 |
| 8   | ~~Port or waive `Point10 Earth`/`Point10 Sun`?~~ **Resolved 2026-11-16**: both ported and registered (`cbb24d8a`); `Cube`/`three` removed from both runtimes (`f7d35db0`); built-in Gallery sets are now identical | closed |
| 9   | ~~Texture registry as its own milestone (M8)?~~ **Resolved 2026-11-13**: landed as `modv-app/src/texture_sources.rs` (image pool LRU + live sources + assigned ISF image inputs + `Texture 2D`) — no separate gate remains | closed |

---

## Appendix A — what gets deleted, and what is kept

### Deleted at P9 (after parity)

| Area                                     | LOC (approx) | Notes |
| ---------------------------------------- | -----------: | ----- |
| `src/renderer/src/application/**`        | ~46,600      | worker, renderers, Point10, setup-media, tempo/audio, system audio, `plugin-host.js` + `install-plugin.js`/`use.js` (the engine-mode JS plugin host — replaced or waived in P7); excludes the ISF `.fs` corpus, relocated — see Kept/converted. (The `three` renderer and `Cube` module were removed from this tree already.) |
| `src/renderer/src/components/**`         | 13,371       | 59 Vue components (incl. scoped styles) |
| `src/renderer/src/{menus,subpages,util,ui-store}` + `App.vue`, `main.js`, `v3-gl-ext.es.js` | ~1,750 | menus (255 LOC of context menus), splash, color-picker, theme glue, golden-layout build |
| `src/main/**` + `src/preload/**`         | ~5,400       | window-prefs, menu-bar, shutdown, window-state, media-manager (non-engine parts live in Rust), engine-supervisor/protocol/ipc/preview-read/log-forwarder/restart-budget |
| `src/renderer/src/css/**`                | ~1,400       | theme → `theme.rs`                             |
| `tests/unit/**` (Vitest)                 | ~15.5k       | 1165 tests; superseded by Rust tests (hybrid) — retained until parity sign-off |
| `tests/e2e/**` (Playwright)              | 9,428         | mapped in Appendix B; kept on `legacy-electron` one release |
| `electron.vite.config.mjs`, `playwright.config.js`, `vitest.config.mjs`, `eslint.config.js`, `patches/`, `package.json`, lockfiles | — | golden-layout/hapticjs/smooth-dnd patches die with the Vue stack |
| Docker Node stages, yarn toolchain       | —            | Dockerfile becomes Rust-only (kept stages: toolchain, engine, package, artifacts) |

### Kept / converted

| Area                                  | Fate                                            |
| ------------------------------------- | ----------------------------------------------- |
| ISF corpus `src/renderer/src/application/renderers/isf/modules/isf/*.{fs,vs}` (387 files) | **Relocated, not deleted**: moved to `resources/isf/` in the repo; `electron-builder.yml` already ships it as the `--isf-dir` runtime directory, and the native app bundles the same path |
| `crates/**` (~147k LOC)               | Reused; `modv-app` becomes the `modv` binary; `--headless` retains engine-only mode |
| `tests/fixtures/engine/*.json`        | Kept; replayed by Rust mirror tests (already the case) |
| `point10/scenes/Earth.js`, `Sun.js`   | **Ported** — now live as `crates/modv-renderers/src/point10/scenes/earth.rs` / `sun.rs` (2026-11-16); the JS files die with the legacy tree as planned |
| `src/renderer/src/assets/fonts/**`    | Converted (woff2 → otf/ttf, subsetting) → bundled in `modv-ui` |
| `src/renderer/src/assets/graphics/**` | App icon/logo — kept                             |
| Media-manager JSON/profile semantics  | Reimplemented in `modv-ui`/`modv-media` (P6)     |
| `docs/**`, `LICENSE` (MIT)            | Kept; docs updated at P9                          |
| `~/.config/VmodV` (userData)          | Untouched                                       |

### New code estimate

| Crate/area        | Estimate     | Notes                                             |
| ----------------- | -----------: | ------------------------------------------------- |
| `modv-ui`         | ~22–30k LOC  | 59 components → immediate-mode Rust, denser LOC but linear; TweenControl and Scenes.vue dominate |
| `modv-media` add  | ~1.5k        | midir client + polling hot-plug                   |
| menus/dialogs/window-state/logger/shutdown | ~2k (ports) | mechanical                                     |
| Boa plugin host (in-process)        | ~1.5k    | + fixtures; expressions already done in-engine     |
| Tests (kittest + integration)       | ~12–15k  | replaces 24k LOC of JS tests                       |
| **Total new**     | **~40–50k LOC** | ≈ 40 % of the hybrid effort; the engine is already done |

---

## Appendix B — e2e parity mapping (144 specs)

Spec groups in `tests/e2e/spec/` and their native replacement:

| Spec (group)                          | Replacement |
| -------------------------------------- | ----------- |
| `main/quitCleansUp.spec.js`, `main/windowState.spec.js` | Rust integration test: spawn `modv --headless`, SIGTERM/SIGKILL, assert snapshot restore + no orphan; `windowState` ports with P1's window-state rewrite — kittest/manual checklist item for the packaged app |
| `packaging/linuxBundleIsTrimmed.spec.js` | CI gate: assert Flatpak/AppImage file listing; no Chromium/Node artifacts in the bundle |
| `presets/generatesAPresetWithExpectedKeys.spec.js` | Rust round-trip tests (exist since M2): assert exactly the six keys; Gallery group preserved |
| `scenes/performanceCellTiming.spec.js`, `scenes/performanceTiming.spec.js`, `scenes/performanceGrid.spec.js`, `scenes/performanceTransitions.spec.js`, `scenes/gallerylessPerformance.spec.js` | In-process integration tests driving `EngineHandle::dispatch("scenes/…")` + tick assertions (the underlying logic is Rust-tested since M5); plus `egui_kittest` on the Scenes panel |
| `scenes/defaultScenes.spec.js`, `scenes/sceneSnapshotsAndPerformance.spec.js`, `scenes/captureLiveState.spec.js` | seeding/refresh logic already Rust-tested (M5); kittest for the capture button and default-seed boot path |
| `scenes/hoverPreviewIsolation.spec.js`, `scenes/hoverPreviewUsesGalleryContexts.spec.js` | lavapipe golden tests + in-process preview tests (an isolated compositor renders the hovered cell) — no egui needed |
| `point10/*.spec.js` (8 specs)         | lavapipe goldens per scene/transparency case (existing harness) + compositor integration tests; `point10WebGPU.spec.js` needs the lavapipe/Vulkan path the current CI already uses |
| `groups/*.spec.js`, `general/*.spec.js`, `inputLinks/*.spec.js`, `general/fullScreenGlContextsAreNotAntialiased.spec.js` | mostly engine-behaviour assertions → covered by Rust engine tests (groups/state/input links since M2); the UI-facing remainder → kittest interactions on Groups/Module panels; the GL-context spec disappears (there is no GL in the native app) |
| `audio/systemAudio.spec.js`, `audio/tempoDetector.spec.js` | Rust fixture tests (exist since M6); system-audio end-to-end moves to the P8 manual checklist (host PipeWire) |
| `engine/engineSkeleton.spec.js`, `engine/engineParity.spec.js` | These assert the hybrid's Electron↔engine boundary. The native app deletes that boundary, so the specs assert things that no longer exist; their meaningful content (preview frames publish, kill-survival/restart, ISF corpus translates with no overrides, the registered-module count (`engineParity` asserts `>300` — module-parity item 6 wants an exact assertion), audio-link min/max, transparency, output pixels) is re-covered by in-process integration tests + headless CLI checks |
| `general/searchGalleryAndAddModuleToGroup.spec.js`, `general/galleryItemPreviewRendersModule.spec.js`, `general/galleryItemPreviewsInheritTheComposition.spec.js`, `general/galleryGroupInheritsOnlyWhenHovered.spec.js`, `general/isfGalleryCategories.spec.js` | kittest + in-process preview tests; gallery share/inheritance semantics already Rust-tested (compositor) |

Every spec lands in exactly one bucket; anything not portable gets a named
waiver with a reason (expected count: small — most specs assert engine
behaviour, which is already Rust-tested). The mapping table is regenerated at
P8 start and kept as `docs/native-parity-map.md`.

## Appendix C — glossary

| Term | Meaning |
| ---- | ------- |
| modV Native | The single-process Rust application produced by this plan |
| Module parity | The engine↔legacy Gallery audit in `docs/module-parity.md` — counts registrations, meta/prop mismatches and inert modules, ordered by fix |
| M8 | The former inherited texture-registry milestone — **landed on the hybrid branch** (`e4cf3e26`): image pool + decode + texture-prop binding for ISF image inputs and `Texture 2D` |
| Headless binary | `modv --headless` / `modv-engine` — engine without UI, same workspace, driven over `modv-ipc` |
| Engine handle | The in-process `EngineHandle` abstraction replacing the socket in the app |
| Frame Spigot | The D5 host API exposed to opt-in JS plugins (downscaled RGBA frame + `modV.ws`) |
| Panel | A dockable egui region replacing a golden-layout component |
| Parity | Behavioural equivalence with the Electron build, proven by Appendix B + manual checklist — not pixel-identical UI |
