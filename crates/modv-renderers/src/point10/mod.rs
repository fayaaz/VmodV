//! Point10 scenes — port of `src/renderer/src/application/point10/`.
//!
//! # Layout
//!
//! ```text
//! point10/
//!   mod.rs                 this file: Point10Scene, SceneContext, prop/meta schemas,
//!                          all_scenes()
//!   audio_bridge.rs        AudioBridge (audioBridge.js) + P10Audio reader (p10Audio.js)
//!   shared/                particle_alpha (particleAlpha/particleSprite), random,
//!                          p9_glsl (the P9Glsl.js helpers batch 2 uses, as WGSL),
//!                          p9_shading (the P9Glsl.js lighting/colour half),
//!                          assets (texture loading + procedural fallbacks)
//!   chromatic/             shared ring-buffer sim + the ChromaticLine/ChromaticWave scenes
//!   chromatic_line.rs      compatibility re-export of chromatic/line.rs
//!   postfx/                RenderPass, UnrealBloomPass, ShaderPass, OutputPass, Composer
//!   volume/                the GPU fire/smoke simulation (compute passes + raymarch),
//!                          the room/audio maths and the shared HDR stage that
//!                          SmokeRoom, DVD Room, RollingFire and Ball of Fire
//!                          configure
//!   scenes/                ParticleField, SpiralParticles, Waveform, SquareTunnel,
//!                          LightTunnel, Wormhole, ItIsComingForYou, WaterRipple,
//!                          Mycelium, AroundPlus, UltraHdr, MirrorCube,
//!                          ColorCube, Boxy, SoundCube, HollowCube,
//!                          RollingFire, BallOfFire, SmokeRoom, LatentBackrooms,
//!                          TheCave
//!                          (+ the cube_mesh, particle_quad and fire_common
//!                          helpers)
//! ```
//!
//! # Batch order
//!
//! The port follows the legacy inventory order (dependency order, not
//! complexity):
//!
//! | Batch | Scenes | Why here |
//! |------:|--------|----------|
//! | — | shared helpers | `particleAlpha`, `particleSprite`, `random` have no scene dependency and three later scenes need them |
//! | — | audio bridge | every scene reads `P10Audio`; the bridge is the only audio entry point |
//! | — | postfx chain | 20 of 26 scenes use `EffectComposer` (RenderPass → UnrealBloom → ShaderPass → OutputPass) |
//! | 1 | **ChromaticWave**, ParticleField, SpiralParticles, Waveform | uses everything above, no `P9Glsl`/texture assets |
//! | 2 | **SquareTunnel**, LightTunnel, Wormhole, ItIsComingForYou | needs `plane9/P9Glsl` + `shared/textures` + `shared/random` |
//! | 3 | WaterRipple, Mycelium, AroundPlus | needs `shared/waterWave`, `shared/phraseColor`, `shared/assets` |
//! | 4 | **UltraHDR**, MirrorCube, ColorCube, Boxy, SoundCube | `P9Glsl` lighting (`shared/p9_shading`), `BoxGeometry`, `preserveBloomAlpha` |
//! | 5a | HollowCube, AttractorParticles, DvdRoom | first of the WebGPU/TSL group; DvdRoom runs the ported volume simulation |
//! | 5b | RollingFire, BallOfFire, SmokeRoom, LatentBackrooms | adds the Afterimage pass; RollingFire, Ball of Fire and SmokeRoom run the ported volume simulation |
//! | 6 | TheCave | Plane9 XML preset with its own raymarch/RTT/bloom/ACES chain |
//!
//! ChromaticLine was ported ahead of this batch (`chromatic_line.rs`, a
//! fixture-driven port with its own `SyntheticSine`); it now shares its
//! ring-buffer simulation with ChromaticWave through [`chromatic`] and its
//! update path is bridge-driven like every other scene, so [`all_scenes`]
//! registers it first, followed by the four **batch 1** scenes. `SyntheticSine`
//! stays available for tests and the headless frame harness, but it only fills
//! a `BandPayload` — the scene itself never reads it.
//!
//! # Scene contract
//!
//! `BaseScene.js` (WebGL) and `BaseSceneWebGPU.js` (WebGPU) both expose
//! `_init`, `update(dt, audioData)`, `render()`, `resize(w, h)`, `dispose()`.
//! The Rust contract is [`Point10Scene`]; the differences are deliberate:
//!
//! * `init` receives a [`SceneContext`] (device, queue, format, size) instead
//!   of a renderer/camera pair, because wgpu has no ambient renderer.
//! * `update` receives the [`AudioBridge`] rather than the adapter-prepared
//!   byte spectrum: scenes that want the byte array read
//!   [`AudioBridge::spectrum`] themselves (the JS adapter does exactly that at
//!   `adapter.js:809-816`), scenes that want bands use
//!   [`audio_bridge::P10Audio`].
//! * `render` takes a command encoder + a target view instead of using
//!   `renderer.render(group, camera)`. Every scene clears its own target to
//!   transparent black (`adapter.js:297`: `setClearColor(0x000000, 0)` with
//!   `alpha: true`), so a scene target composites over the layers below — the
//!   same "empty frame stays transparent" rule the JS scenes rely on.
//!
//! There is no camera object: the WebGL scenes are written against
//! `PerspectiveCamera(60, aspect, 0.1, 1000)` at `z = 15` looking at the
//! origin (`adapter.js:299-301`). The scenes that need a projection
//! ([`scenes::ParticleField`], [`scenes::SpiralParticles`],
//! [`scenes::Waveform`]) carry that camera as constants in their WGSL.
//! [`scenes::Wormhole`] and [`scenes::ItIsComingForYou`] build their own
//! cameras in `_init` (`Wormhole.js:216-227`,
//! `ItIsComingForYou.js:262-274`), so those two inline their own projection.
//! The two tunnels are fullscreen shaders and ignore the camera entirely.
//!
//! # `SceneContext` and the scene's own GPU resources
//!
//! [`SceneContext`] owns cheap clones of the device/queue plus the two shared
//! helpers: the full-screen-triangle vertex shader and a unit-quad
//! vertex/index buffer. Everything else (pipelines, bind groups, instance
//! buffers, textures) belongs to the scene and is created in `init` — the
//! pipeline cache lives in wgpu, so re-creating a pipeline after a scene
//! switch is cheap.
//!
//! From batch 3 the context also carries an optional `assets_dir`: the engine
//! will pass `--assets-dir` there, and scenes resolve textures through
//! [`shared::assets::AssetLoader`] (`$MODV_ASSETS_DIR`, then `assets_dir`, then
//! the repo's own texture directory).
//!
//! # Not here (later wiring)
//!
//! Instance lifecycle, eviction, the warm cache and presentation are engine
//! concerns (`adapter.js:1-120`, `warm-cache.js`). In Rust there is one device
//! and no OffscreenCanvas blit: the scene draws straight into the caller's
//! target, which removes the adapter's one-frame acquisition lag.

#![forbid(unsafe_code)]

pub mod audio_bridge;
pub mod chromatic;
pub mod chromatic_line;
pub mod postfx;
pub mod scenes;
pub mod shared;
pub mod volume;

#[cfg(test)]
mod scene_tests;
#[cfg(test)]
mod test_support;

/// HDR scene/postfx target format.
///
/// three's `EffectComposer` uses `HalfFloatType` render targets
/// (`UnrealBloomPass.js:90-125`), so the scene target and every bloom mip are
/// `Rgba16Float`. Scenes are initialised with this format by
/// [`postfx::Composer::scene_format`].
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Full-screen-triangle vertex shader, shared by every full-screen pass.
///
/// The same trick as the legacy pipeline (`ChromaticLine.js` `VERT`: raw NDC
/// from the vertices `[-1,-1, 3,-1, -1,3]`). Use it with a fragment entry that
/// has no vertex inputs, or together with [`fullscreen_uv`] for a
/// `position → uv` mapping.
pub const FULLSCREEN_TRIANGLE_WGSL: &str = "\
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
  var positions = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
  return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}
";

/// Fragment-side `uv` helper matching [`FULLSCREEN_TRIANGLE_WGSL`].
///
/// `resolution` is the target size in pixels; `position` is the
/// `@builtin(position)` value (top-left origin, like `gl_FragCoord` after the
/// WebGL y-flip the legacy shaders get for free). Returns `0..1` with v = 0 at
/// the top row, matching a `DataTexture` upload.
pub const FULLSCREEN_UV_WGSL: &str = "\
fn fullscreen_uv(position: vec2<f32>, resolution: vec2<f32>) -> vec2<f32> {
  return position / resolution;
}
";

/// `timing.js: MAX_FRAME_SECONDS` — the Point10 adapter's frame clock caps a
/// frame at 0.1 s ("a tab-suspend catch-up cannot jump the pattern").
pub const MAX_FRAME_SECONDS: f32 = 0.1;

/// The thirty-four Plane9 (Joakim Dahl) preset ports, under their current
/// canonical `Plane9 <Name>` registry names.
///
/// Twelve were originally registered as `Point10 <Name>`. Saved Scenes and
/// presets still carry those old strings, so the legacy aliases stay
/// resolvable through [`canonical_scene_name`] — this list is the single
/// source of truth for both directions and for the Gallery's `plane9` kind.
/// The remaining ten were imported later and never had a `Point10 ` name; the
/// alias lookup accepts their `Point10 ` spelling too, for uniform handling.
pub const PLANE9_SCENE_NAMES: &[&str] = &[
    "Plane9 Boxy",
    "Plane9 Blob Fade",
    "Plane9 Circle Crossover",
    "Plane9 Color Cube",
    "Plane9 Colors",
    "Plane9 Earth",
    "Plane9 Gradient",
    "Plane9 Hollow Cube",
    "Plane9 It Is Coming For You",
    "Plane9 Light Tunnel",
    "Plane9 Mirror Cube",
    "Plane9 Noise Warp",
    "Plane9 Rorschach",
    "Plane9 Sound Cube",
    "Plane9 Square Tunnel",
    "Plane9 Sun",
    "Plane9 The Cave",
    "Plane9 Though The Noise",
    "Plane9 Truchet Tentacles",
    "Plane9 Tv Noise",
    "Plane9 Wormhole",
    "Plane9 Xor",
    // The second XML batch (RTT/feedback/raymarch presets).
    "Plane9 Abstract Clouds",
    "Plane9 Magic Smoke Box",
    "Plane9 Color Blob",
    "Plane9 Stargate Event Horizon",
    "Plane9 Color Beams",
    "Plane9 Mirror Tunnel",
    "Plane9 Cloud Flight",
    // The fourth XML batch (mesh-modifier preset).
    "Plane9 Spiky",
    // The fifth XML batch (feedback cube preset).
    "Plane9 Evil Box",
    // The sixth XML batch (instanced wave preset).
    "Plane9 Music Waves",
    // The seventh XML batch (single-pass instanced spirograph).
    "Plane9 Hypno Toad",
    // The eighth XML batch (reaction-diffusion feedback preset).
    "Plane9 Cell Culture",
];

/// The prefix of the scenes that kept their `Point10 ` registry name, and of
/// the historical aliases of the renamed Plane9 ports and effects (which now
/// register under [`PLANE9_PREFIX`]).
pub const POINT10_PREFIX: &str = "Point10 ";

/// The prefix of the renamed Plane9 ports.
pub const PLANE9_PREFIX: &str = "Plane9 ";

/// The forty-nine Plane9 (Joakim Dahl) shader effects and ISF transitions that
/// were audited as Plane9 originals and moved from the historical `Point10 `
/// prefix to their canonical `Plane9 <Name>` registry names.
///
/// Saved Scenes and presets still carry the old strings, so the legacy
/// `Point10 <Name>` aliases stay resolvable through
/// [`canonical_effect_name`] — this list is the single source of truth for both
/// directions and for the Gallery's `plane9` kind. The five effects without a
/// Plane9 preset (`Pixelation`, `RGB Shift`, `Digital Glitch`, `Vignette`,
/// `Bloom`) are deliberately absent and keep their `Point10 ` names.
pub const PLANE9_EFFECT_NAMES: &[&str] = &[
    // Shader effects (`shader/modules/point10_effects.rs`).
    "Plane9 Gray",
    "Plane9 Invert",
    "Plane9 Posterization",
    "Plane9 Film Grain",
    "Plane9 BlackAndWhite",
    "Plane9 Gray Stripes",
    "Plane9 Invert Gray",
    "Plane9 Crosshatching",
    "Plane9 Blocks",
    "Plane9 3d Blocks",
    "Plane9 Bayer 8 Matrix Dithering BW",
    "Plane9 Bayer Matrix Dithering Color",
    "Plane9 Fuzzy Pixels",
    "Plane9 Fractured",
    "Plane9 Bathroom Window",
    "Plane9 Contrast",
    "Plane9 Color Distort",
    "Plane9 Crt",
    "Plane9 Cubic Lens Distortion",
    "Plane9 Hexagons",
    "Plane9 Limit Hue",
    "Plane9 Embos Merge",
    // ISF transitions (`resources/isf/Plane9 *.fs`).
    "Plane9 Banded Swirl",
    "Plane9 Blinds",
    "Plane9 Block",
    "Plane9 Circle Reveal",
    "Plane9 Circle Stretch",
    "Plane9 Clock Swipe",
    "Plane9 Close Horizontal",
    "Plane9 Crossfade",
    "Plane9 Crumble",
    "Plane9 Disolve",
    "Plane9 Dropfade",
    "Plane9 Erode",
    "Plane9 Line Reveal",
    "Plane9 Lines",
    "Plane9 Pixelate",
    "Plane9 Pour",
    "Plane9 Radial Blur",
    "Plane9 Random Circle",
    "Plane9 Ripple",
    "Plane9 Shockwave",
    "Plane9 Shrink",
    "Plane9 SlideIn",
    "Plane9 Smooth Swirl Grid",
    "Plane9 Swirl",
    "Plane9 Swirl Grid",
    "Plane9 Water",
    "Plane9 Wave",
];

/// Author credit for the thirty-four Plane9 preset ports.
pub const PLANE9_AUTHOR: &str = "Plane9 (Joakim Dahl)";

/// Resolve `name` against one canonical roster, rewriting the legacy
/// `Point10 <Name>` prefix to `Plane9 <Name>`.
///
/// Both public resolvers share this walk so the scenes and effects tables
/// cannot diverge in how aliases are matched.
fn canonical_alias_in<'a>(name: &str, canonical_names: &'a [&'a str]) -> Option<&'a str> {
    let suffix = name.strip_prefix(POINT10_PREFIX)?;
    canonical_names.iter().copied().find(|canonical| {
        canonical
            .strip_prefix(PLANE9_PREFIX)
            .is_some_and(|canonical_suffix| canonical_suffix == suffix)
    })
}

/// Resolve a historical `Point10 <Name>` registry reference to its canonical
/// `Plane9 <Name>` name.
///
/// Returns `None` when `name` is not one of the Plane9 roster (either already
/// canonical, or a Point10 scene that kept its name).
pub fn canonical_scene_name(name: &str) -> Option<&'static str> {
    canonical_alias_in(name, PLANE9_SCENE_NAMES)
}

/// The canonical registry name for `name`, resolving legacy aliases.
///
/// Non-aliases are returned unchanged.
pub fn resolve_scene_name(name: &str) -> &str {
    canonical_scene_name(name).unwrap_or(name)
}

/// Resolve a historical `Point10 <Name>` reference to a renamed Plane9 shader
/// effect or ISF transition, returning its canonical `Plane9 <Name>` name.
///
/// Returns `None` when `name` is not one of the audited effects (including the
/// five effects that kept their `Point10 ` names).
pub fn canonical_effect_name(name: &str) -> Option<&'static str> {
    canonical_alias_in(name, PLANE9_EFFECT_NAMES)
}

/// The canonical registry name for a shader effect or ISF transition,
/// resolving legacy aliases.
///
/// Non-aliases are returned unchanged.
pub fn resolve_effect_name(name: &str) -> &str {
    canonical_effect_name(name).unwrap_or(name)
}

/// The authoritative Gallery renderer kind for a registry module named `name`.
///
/// The thirty-four Plane9 preset ports and the forty-nine renamed Plane9
/// effects share the Point10 runtime renderer but are surfaced to the Gallery
/// as their own kind (`plane9`, matched as free text); every other module keeps
/// `default_kind` (its engine renderer kind). Using this helper in both snapshot
/// builders keeps the roster in one place so the desktop and wasm Galleries
/// cannot diverge.
pub fn gallery_kind(name: &str, default_kind: &str) -> String {
    if PLANE9_SCENE_NAMES.contains(&name) || PLANE9_EFFECT_NAMES.contains(&name) {
        "plane9".to_string()
    } else {
        default_kind.to_string()
    }
}

/// Rewrite every legacy `Point10 <Name>` alias string in a JSON document to its
/// canonical `Plane9 <Name>` name.
///
/// Covers both the thirty-four Plane9 preset scenes and the forty-nine audited
/// Plane9 shader effects/ISF transitions. Walks arrays and objects so nested
/// Scene cells, Groups and module records are migrated; every other value is
/// left untouched. Idempotent: canonical names never match the legacy prefix.
pub fn rename_legacy_scene_names(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(canonical) =
                canonical_scene_name(text).or_else(|| canonical_effect_name(text))
            {
                *text = canonical.to_string();
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rename_legacy_scene_names(item);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                rename_legacy_scene_names(value);
            }
        }
        _ => {}
    }
}

/// Clamp a frame delta the way the Point10 adapter's frame clock does
/// (`timing.js: createFrameClock`).
///
/// The JS clock also reports **0** on the first frame after a scene is
/// activated; the engine owns that clock, so the Rust scenes only need the cap
/// and the non-finite guard. Batch-2 scenes each carried their own copy of
/// this; batch 3 shares it.
pub fn frame_dt(dt: f32) -> f32 {
    if dt.is_finite() {
        dt.clamp(0.0, MAX_FRAME_SECONDS)
    } else {
        0.0
    }
}

/// One Point10 scene, ported from the `BaseScene` class contract.
pub trait Point10Scene {
    /// Registry name — the wrapper module's `createPoint10Module({ name })`
    /// string (`renderers/2d/modules/Point10*.js`), e.g.
    /// `"Point10 Chromatic Wave"`.
    fn name(&self) -> &'static str;

    /// Build the scene's GPU resources. Called once per activation (and again
    /// after a [`Point10Scene::dispose`]).
    fn init(&mut self, ctx: &mut SceneContext);

    /// Target size changed. Scenes that depend on resolution re-create their
    /// size-dependent resources here.
    fn resize(&mut self, width: u32, height: u32);

    /// Advance one frame. The queue is not passed (the trait keeps the
    /// update/render split of `BaseScene`), so scenes clone the queue in
    /// `init` and upload uniforms/buffers here.
    fn update(&mut self, dt: f32, audio: &audio_bridge::AudioBridge);

    /// Record the scene into `target`, clearing it to transparent black first.
    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView);

    /// Release GPU resources. Idempotent; the scene can be re-initialised.
    fn dispose(&mut self);

    /// The scene's modV prop schema: its `sceneProps.js` entry preceded by
    /// `RESPONSIVENESS_PROPS`, which the adapter adds to every Point10 module
    /// (`adapter.js:868-871`: `props: { ...RESPONSIVENESS_PROPS, ...sceneProps }`).
    ///
    /// Returned by value because the JS spreads two objects; use
    /// [`scene_props_with_levers`] to build it.
    fn props(&self) -> Vec<SceneProp> {
        Vec::new()
    }

    /// The Gallery meta (`adapter.js:857-866`).
    fn meta(&self) -> SceneMeta {
        SceneMeta::DEFAULT
    }
}

/// A modV prop definition, matching the `sceneProps.js` shape exactly
/// (`{ label, type, min, max, step, default }`) so saved presets keep working.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneProp {
    /// Prop key (`props.<name>`).
    pub name: &'static str,
    /// UI label.
    pub label: &'static str,
    /// Prop type.
    pub kind: ScenePropKind,
    /// Default value (booleans use 1.0 / 0.0).
    pub default: f32,
    /// Minimum for numeric props (ignored for booleans).
    pub min: f32,
    /// Maximum for numeric props (ignored for booleans).
    pub max: f32,
    /// Control step for numeric props.
    pub step: f32,
}

/// The prop types the batch-1 scenes declare.
///
/// `sceneProps.js` only uses `float` and `bool` for these four scenes;
/// `readSceneProps` treats every other type as numeric, so the enum grows when
/// a later scene needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenePropKind {
    /// `type: "float"`.
    Float,
    /// `type: "bool"` — read as `value != false`.
    Bool,
}

impl SceneProp {
    /// A `float` prop (`sceneProps.js` helper `float(label, default, min, max, step = 0.01)`).
    pub const fn float(
        name: &'static str,
        label: &'static str,
        default: f32,
        min: f32,
        max: f32,
        step: f32,
    ) -> Self {
        Self {
            name,
            label,
            kind: ScenePropKind::Float,
            default,
            min,
            max,
            step,
        }
    }

    /// A `bool` prop (`props.js` `useAgc` shape).
    pub const fn boolean(name: &'static str, label: &'static str, default: bool) -> Self {
        Self {
            name,
            label,
            kind: ScenePropKind::Bool,
            default: if default { 1.0 } else { 0.0 },
            min: 0.0,
            max: 1.0,
            step: 1.0,
        }
    }

    /// Resolve this prop against an incoming value.
    ///
    /// `sceneProps.js: readSceneProps` (lines 221-241): booleans are
    /// `props[key] !== false`; numbers fall back to the default when the
    /// incoming value is missing/non-finite and clamp into `[min, max]`.
    pub fn read(&self, value: Option<f32>) -> f32 {
        match self.kind {
            ScenePropKind::Bool => {
                if value.unwrap_or(1.0) != 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            ScenePropKind::Float => {
                let value = match value {
                    Some(value) if value.is_finite() => value,
                    _ => self.default,
                };
                value.clamp(self.min, self.max)
            }
        }
    }
}

/// A scene's own schema with the responsiveness levers in front of it — the
/// exact prop list `createPoint10Module` publishes (`adapter.js:868-871`).
pub fn scene_props_with_levers(schema: &[SceneProp]) -> Vec<SceneProp> {
    let mut props = Vec::with_capacity(audio_bridge::RESPONSIVENESS_PROPS.len() + schema.len());
    props.extend_from_slice(audio_bridge::RESPONSIVENESS_PROPS);
    props.extend_from_slice(schema);
    props
}

/// The user-facing Lens magnification prop the Plane9 camera scenes publish.
///
/// `2.0` doubles the rendered size, `0.5` halves it and `1.0` is the vendor
/// framing. Only the Plane9 scenes declare it (see [`plane9_scene_props`]); the
/// Point10-authored scenes must not gain a dead control.
pub const LENS_PROP: SceneProp = SceneProp::float("lens", "Lens", 1.0, 0.25, 4.0, 0.01);

/// Resolve an incoming Lens value exactly like [`SceneProp::read`]:
/// non-finite falls back to `1.0`, then clamps into `[0.25, 4.0]`.
pub fn lens_value(value: Option<f32>) -> f32 {
    LENS_PROP.read(value)
}

/// A Plane9 scene's prop list: the responsiveness levers, then Lens, then the
/// scene's own schema.
///
/// This is [`scene_props_with_levers`] plus Lens for the Plane9 presets only;
/// the global helper stays Lens-free so the Point10-authored scenes keep their
/// exact schemas.
pub fn plane9_scene_props(schema: &[SceneProp]) -> Vec<SceneProp> {
    let mut props = Vec::with_capacity(audio_bridge::RESPONSIVENESS_PROPS.len() + 1 + schema.len());
    props.extend_from_slice(audio_bridge::RESPONSIVENESS_PROPS);
    props.push(LENS_PROP);
    props.extend_from_slice(schema);
    props
}

/// Apply Lens magnification to an existing projection matrix.
///
/// Only the x/y scale entries change, so the clip-space depth rows are
/// untouched. A `lens` of `1.0` multiplies by exactly `1.0`, which preserves
/// every element bit-for-bit.
pub fn apply_lens(mut projection: scenes::cube_mesh::Mat4, lens: f32) -> scenes::cube_mesh::Mat4 {
    projection[0][0] *= lens;
    projection[1][1] *= lens;
    projection
}

/// Resolve a whole schema against a prop getter.
///
/// Returns `(key, value)` pairs in schema order — the order the binder in
/// `sceneProps.js: SCENE_PROP_CONFIGS` reads them.
pub fn read_scene_props(
    schema: &'static [SceneProp],
    mut get: impl FnMut(&str) -> Option<f32>,
) -> Vec<(&'static str, f32)> {
    schema
        .iter()
        .map(|prop| (prop.name, prop.read(get(prop.name))))
        .collect()
}

/// The Gallery meta of a Point10 module (`adapter.js:857-866`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneMeta {
    /// Display + registration name, e.g. `"Point10 Particle Field"` or
    /// `"Plane9 Light Tunnel"`.
    pub name: &'static str,
    /// Module author: `"Point10"`, or [`PLANE9_AUTHOR`] for the thirty-four
    /// Plane9 preset ports.
    pub author: &'static str,
    /// Module version (`"1.0.0"`).
    pub version: &'static str,
    /// Module type — `"2d"` for every Point10 scene.
    pub module_type: &'static str,
    /// Gallery category (`"3d-scenes"` by default; the Chromatic scenes and
    /// fullscreen 2D scenes override it to `"2d-scenes"`).
    pub category: &'static str,
    /// Requested audio features (`["bass", "mid", "treble"]`).
    pub audio_features: &'static [&'static str],
}

impl SceneMeta {
    /// Default meta before a scene overrides it (`adapter.js:857-866`).
    pub const DEFAULT: Self = Self {
        name: "",
        author: "Point10",
        version: "1.0.0",
        module_type: "2d",
        category: "3d-scenes",
        audio_features: &["bass", "mid", "treble"],
    };

    /// Meta for a registered scene name with an explicit Gallery category.
    pub const fn for_scene(name: &'static str, category: &'static str) -> Self {
        Self {
            name,
            category,
            ..Self::DEFAULT
        }
    }

    /// Meta for one of the thirty-four Plane9 (Joakim Dahl) preset ports.
    pub const fn for_plane9_scene(name: &'static str, category: &'static str) -> Self {
        Self {
            name,
            author: PLANE9_AUTHOR,
            category,
            ..Self::DEFAULT
        }
    }
}

/// The shared helper resources a scene may use.
///
/// The device and queue are clones (wgpu handles are reference-counted), so a
/// scene can keep them past `init`. `target_format` is the format of the view
/// [`Point10Scene::render`] receives — [`HDR_FORMAT`] when the scene runs
/// behind [`postfx::Composer`].
pub struct SceneContext {
    /// wgpu device.
    pub device: wgpu::Device,
    /// wgpu queue.
    pub queue: wgpu::Queue,
    /// Format of the render target the scene draws into.
    pub target_format: wgpu::TextureFormat,
    /// Target width in pixels.
    pub width: u32,
    /// Target height in pixels.
    pub height: u32,
    /// Shared unit quad (corner + uv), used by the instanced particle scenes.
    pub quad: UnitQuad,
    /// Directory the engine passes for scene textures (the planned
    /// `--assets-dir`). `None` in the tests and until `modv-app` wires it; the
    /// loader then falls back to `$MODV_ASSETS_DIR` and the repo's texture
    /// directory.
    pub assets_dir: Option<std::path::PathBuf>,
}

impl SceneContext {
    /// Build a context at 1x1; call [`SceneContext::resize`] before the first
    /// frame.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            target_format,
            width: 1,
            height: 1,
            quad: UnitQuad::new(device),
            assets_dir: None,
        }
    }

    /// Set the engine-provided asset directory (builder form, so `new` stays
    /// source-compatible for the engine).
    pub fn with_assets_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.assets_dir = Some(dir.into());
        self
    }

    /// The asset loader for this context (`$MODV_ASSETS_DIR`, then
    /// [`SceneContext::assets_dir`], then the dev fallback).
    pub fn asset_loader(&self) -> shared::assets::AssetLoader {
        shared::assets::AssetLoader::for_assets_dir(self.assets_dir.as_deref())
    }

    /// Update the logical target size.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    /// Target aspect ratio (`width / height`), matching the legacy
    /// `PerspectiveCamera` aspect.
    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }

    /// Build a full-screen-triangle pipeline from `shader` (which must declare
    /// [`FULLSCREEN_TRIANGLE_WGSL`]'s `vs_main` and its own `fs_main`).
    pub fn fullscreen_pipeline(
        &self,
        label: &str,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        blend: Option<wgpu::BlendState>,
    ) -> wgpu::RenderPipeline {
        self.fullscreen_pipeline_with_format(label, shader, layout, self.target_format, blend)
    }

    /// [`SceneContext::fullscreen_pipeline`] for an explicit target format.
    ///
    /// The composer-owning scenes draw into the composer's HDR scene target
    /// ([`postfx::Composer::scene_format`]), which is not the engine's
    /// destination format, so they cannot use the context's `target_format`
    /// for their scene pipeline.
    pub fn fullscreen_pipeline_with_format(
        &self,
        label: &str,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
        blend: Option<wgpu::BlendState>,
    ) -> wgpu::RenderPipeline {
        let device = &self.device;
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}

/// A unit quad in clip space with the matching `uv`.
///
/// Corner `(x, y)` is in `[-0.5, 0.5]` with **y up** (clip space), `uv` is
/// `0..1` with **v = 0 at the top**, so a `DataTexture`-style sprite upload
/// (row 0 = top) samples the right way up.
pub struct UnitQuad {
    /// Interleaved `[x, y, u, v]` vertices.
    pub vertex_buffer: wgpu::Buffer,
    /// Two triangles (`0,1,2, 0,2,3`).
    pub index_buffer: wgpu::Buffer,
}

impl UnitQuad {
    /// Vertex count of one quad.
    pub const VERTEX_COUNT: u32 = 6;
    /// Corners, `[x, y, u, v]` — top-left, bottom-left, bottom-right,
    /// top-right (counter-clockwise in y-up clip space).
    pub const VERTICES: [[f32; 4]; 4] = [
        [-0.5, 0.5, 0.0, 0.0],
        [-0.5, -0.5, 0.0, 1.0],
        [0.5, -0.5, 1.0, 1.0],
        [0.5, 0.5, 1.0, 0.0],
    ];
    /// Index order.
    pub const INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

    /// Upload the shared quad buffers.
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::util::DeviceExt;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-unit-quad-vertices"),
            contents: bytemuck::cast_slice(&Self::VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-unit-quad-indices"),
            contents: bytemuck::cast_slice(&Self::INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            vertex_buffer,
            index_buffer,
        }
    }

    /// Vertex layout (`@location(2) corner: vec2<f32>`, `@location(3) uv: vec2<f32>`).
    ///
    /// Locations 0 and 1 are reserved for the per-instance position and
    /// colour of the particle scenes.
    pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: 4 * std::mem::size_of::<f32>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 2 * std::mem::size_of::<f32>() as u64,
                    shader_location: 3,
                },
            ],
        }
    }

    /// Bind and draw with an instanced draw call.
    pub fn draw_instanced<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, instances: u32) {
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..Self::VERTEX_COUNT, 0, 0..instances);
    }
}

/// Every Point10 scene implemented on the shared [`Point10Scene`] contract.
///
/// Order is the WS-7 port order, so the Gallery list is stable:
/// `ChromaticLine, ChromaticWave, ParticleField, SpiralParticles, Waveform`
/// (ChromaticLine was ported ahead of batch 1; the rest are batch 1), then the
/// batch-2 scenes `SquareTunnel, LightTunnel, Wormhole, ItIsComingForYou`, the
/// batch-3 scenes `WaterRipple, Mycelium, AroundPlus` (inventory
/// order 10, 11, 17, 18), the batch-4 scenes `UltraHDR, MirrorCube,
/// ColorCube, Boxy, SoundCube` (inventory order 12, 14, 15, 16, 19) and the
/// batch-5a scenes `HollowCube, AttractorParticles, DvdRoom`, and the batch-5b
/// scenes `RollingFire, SmokeRoom, LatentBackrooms`, plus
/// `BallOfFire` (the frozen RollingFire variant), `Sun`, `Earth` and `TheCave`
/// (the Plane9 scene batch) plus Music Waves, Hypno Toad and Cell Culture — the
/// complete 52-scene set.
/// Glass Lens is not registered: its upstream CodePen does not state a licence.
pub fn all_scenes() -> Vec<Box<dyn Point10Scene>> {
    vec![
        Box::new(chromatic::line::ChromaticLine::new()),
        Box::new(scenes::chromatic_wave::ChromaticWave::new()),
        Box::new(scenes::particle_field::ParticleField::new()),
        Box::new(scenes::spiral_particles::SpiralParticles::new()),
        Box::new(scenes::waveform::Waveform::new()),
        Box::new(scenes::square_tunnel::SquareTunnel::new()),
        Box::new(scenes::light_tunnel::LightTunnel::new()),
        Box::new(scenes::wormhole::Wormhole::new()),
        Box::new(scenes::it_is_coming_for_you::ItIsComingForYou::new()),
        Box::new(scenes::water_ripple::WaterRipple::new()),
        Box::new(scenes::mycelium::Mycelium::new()),
        Box::new(scenes::around_plus::AroundPlus::new()),
        Box::new(scenes::ultra_hdr::UltraHdr::new()),
        Box::new(scenes::mirror_cube::MirrorCube::new()),
        Box::new(scenes::color_cube::ColorCube::new()),
        Box::new(scenes::boxy::Boxy::new()),
        Box::new(scenes::sound_cube::SoundCube::new()),
        Box::new(scenes::hollow_cube::HollowCube::new()),
        Box::new(scenes::hollow_cube::HollowCube::new_with_variant(
            scenes::hollow_cube::POINT10_HOLLOW_CUBOID,
        )),
        Box::new(scenes::hollow_cube::HollowCube::new_with_variant(
            scenes::hollow_cube::POINT10_HOLLOW_CUBE_LAYERS,
        )),
        Box::new(scenes::hollow_cube::HollowCube::new_with_variant(
            scenes::hollow_cube::POINT10_HOLLOW_CUBE_BLUR,
        )),
        Box::new(scenes::attractor_particles::AttractorParticles::new()),
        Box::new(scenes::dvd_room::DvdRoom::new()),
        Box::new(scenes::rolling_fire::RollingFire::new()),
        Box::new(scenes::rolling_fire::BallOfFire::new()),
        Box::new(scenes::smoke_room::SmokeRoom::new()),
        Box::new(scenes::latent_backrooms::LatentBackrooms::new()),
        Box::new(scenes::sun::Sun::new()),
        Box::new(scenes::earth::Earth::new()),
        Box::new(scenes::the_cave::TheCave::new()),
        // The second Plane9 XML batch (Simple presets), in the import order the
        // RED contract names: Abstract, DemoScene, Fractals, Other, Psychedelic.
        Box::new(scenes::colors::Colors::new()),
        Box::new(scenes::gradient::Gradient::new()),
        Box::new(scenes::noise_warp::NoiseWarp::new()),
        Box::new(scenes::rorschach::Rorschach::new()),
        Box::new(scenes::xor::Xor::new()),
        Box::new(scenes::truchet_tentacles::TruchetTentacles::new()),
        Box::new(scenes::blob_fade::BlobFade::new()),
        Box::new(scenes::though_the_noise::ThoughTheNoise::new()),
        Box::new(scenes::tv_noise::TvNoise::new()),
        Box::new(scenes::circle_crossover::CircleCrossover::new()),
        // The third Plane9 XML batch (RTT/feedback/raymarch presets), in the
        // order the RED contract names.
        Box::new(scenes::abstract_clouds::AbstractClouds::new()),
        Box::new(scenes::magic_smoke_box::MagicSmokeBox::new()),
        Box::new(scenes::color_blob::ColorBlob::new()),
        Box::new(scenes::stargate_event_horizon::StargateEventHorizon::new()),
        Box::new(scenes::color_beams::ColorBeams::new()),
        Box::new(scenes::mirror_tunnel::MirrorTunnel::new()),
        Box::new(scenes::cloud_flight::CloudFlight::new()),
        // The fourth Plane9 XML batch (mesh-modifier preset).
        Box::new(scenes::spiky::Spiky::new()),
        // The fifth Plane9 XML batch (feedback cube preset).
        Box::new(scenes::evil_box::EvilBox::new()),
        // The sixth Plane9 XML batch (instanced wave preset).
        Box::new(scenes::music_waves::MusicWaves::new()),
        // The seventh Plane9 XML batch (single-pass instanced spirograph).
        Box::new(scenes::hypno_toad::HypnoToad::new()),
        // The eighth Plane9 XML batch (reaction-diffusion feedback preset).
        Box::new(scenes::cell_culture::CellCulture::new()),
    ]
}

#[cfg(test)]
mod tests {
    // The fidelity contract accumulates messages from `!(float < float)`
    // checks and compares the audio-feature slice directly.
    #![allow(clippy::neg_cmp_op_on_partial_ord)]
    #![allow(clippy::op_ref)]

    use super::*;

    #[test]
    fn registry_has_the_port_order_scenes() {
        let scenes = all_scenes();
        assert_eq!(scenes.len(), 52);

        assert_eq!(scenes[0].name(), "Point10 Chromatic Line");
        assert_eq!(scenes[1].name(), "Point10 Chromatic Wave");
        assert_eq!(scenes[2].name(), "Point10 Particle Field");
        assert_eq!(scenes[3].name(), "Point10 Spiral Particles");
        assert_eq!(scenes[4].name(), "Point10 Waveform");
        // Batch 2, in the WS-7 inventory order. These are Plane9 ports and
        // use the `Plane9 ` prefix (the `Point10 ` names stay aliases).
        assert_eq!(scenes[5].name(), "Plane9 Square Tunnel");
        assert_eq!(scenes[6].name(), "Plane9 Light Tunnel");
        assert_eq!(scenes[7].name(), "Plane9 Wormhole");
        assert_eq!(scenes[8].name(), "Plane9 It Is Coming For You");
        // Batch 3. Glass Lens was removed: its upstream CodePen does not
        // state a licence, so the scene is not redistributed.
        assert_eq!(scenes[9].name(), "Point10 Water Ripple");
        assert_eq!(scenes[10].name(), "Point10 Mycelium");
        assert_eq!(scenes[11].name(), "Point10 Around Plus");
        // Batch 4, in the WS-7 inventory order (12, 14, 15, 16, 19).
        assert_eq!(scenes[12].name(), "Point10 Ultra HDR");
        assert_eq!(scenes[13].name(), "Plane9 Mirror Cube");
        assert_eq!(scenes[14].name(), "Plane9 Color Cube");
        assert_eq!(scenes[15].name(), "Plane9 Boxy");
        assert_eq!(scenes[16].name(), "Plane9 Sound Cube");
        // Batch 5a.
        assert_eq!(scenes[17].name(), "Plane9 Hollow Cube");
        assert_eq!(scenes[18].name(), "Point10 Hollow Cuboid");
        assert_eq!(scenes[19].name(), "Point10 Hollow Cube Layers");
        assert_eq!(scenes[20].name(), "Point10 Hollow Cube Blur");
        assert_eq!(scenes[21].name(), "Point10 Attractor Particles");
        assert_eq!(scenes[22].name(), "Point10 DVD Room");
        // Batch 5b — the last four of the 25 vendored scenes, plus the
        // Ball of Fire wrapper (the frozen RollingFire subclass).
        assert_eq!(scenes[23].name(), "Point10 Rolling Fire");
        assert_eq!(scenes[24].name(), "Point10 Ball Of Fire");
        assert_eq!(scenes[25].name(), "Point10 Smoke Room");
        assert_eq!(scenes[26].name(), "Point10 Latent Backrooms");
        assert_eq!(scenes[27].name(), "Plane9 Sun");
        assert_eq!(scenes[28].name(), "Plane9 Earth");
        assert_eq!(scenes[29].name(), "Plane9 The Cave");
        // The second Plane9 XML batch.
        assert_eq!(scenes[30].name(), "Plane9 Colors");
        assert_eq!(scenes[31].name(), "Plane9 Gradient");
        assert_eq!(scenes[32].name(), "Plane9 Noise Warp");
        assert_eq!(scenes[33].name(), "Plane9 Rorschach");
        assert_eq!(scenes[34].name(), "Plane9 Xor");
        assert_eq!(scenes[35].name(), "Plane9 Truchet Tentacles");
        assert_eq!(scenes[36].name(), "Plane9 Blob Fade");
        assert_eq!(scenes[37].name(), "Plane9 Though The Noise");
        assert_eq!(scenes[38].name(), "Plane9 Tv Noise");
        assert_eq!(scenes[39].name(), "Plane9 Circle Crossover");
        // The third Plane9 XML batch.
        assert_eq!(scenes[40].name(), "Plane9 Abstract Clouds");
        assert_eq!(scenes[41].name(), "Plane9 Magic Smoke Box");
        assert_eq!(scenes[42].name(), "Plane9 Color Blob");
        assert_eq!(scenes[43].name(), "Plane9 Stargate Event Horizon");
        assert_eq!(scenes[44].name(), "Plane9 Color Beams");
        assert_eq!(scenes[45].name(), "Plane9 Mirror Tunnel");
        assert_eq!(scenes[46].name(), "Plane9 Cloud Flight");
        assert_eq!(scenes[47].name(), "Plane9 Spiky");
        assert_eq!(scenes[48].name(), "Plane9 Evil Box");
        assert_eq!(scenes[49].name(), "Plane9 Music Waves");
        assert_eq!(scenes[50].name(), "Plane9 Hypno Toad");
        assert_eq!(scenes[51].name(), "Plane9 Cell Culture");
        assert!(
            scenes
                .iter()
                .all(|scene| scene.name() != "Point10 Glass Lens"),
            "Glass Lens must not be registered: its upstream licence is unstated"
        );
    }

    #[test]
    fn registry_names_are_unique() {
        let scenes = all_scenes();
        let mut names: Vec<&str> = scenes.iter().map(|scene| scene.name()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    #[test]
    fn registry_meta_matches_the_adapter_defaults() {
        for scene in all_scenes() {
            let meta = scene.meta();
            assert_eq!(meta.name, scene.name());
            let expected_author = if scene.name().starts_with(PLANE9_PREFIX) {
                PLANE9_AUTHOR
            } else {
                "Point10"
            };
            assert_eq!(meta.author, expected_author, "{}", scene.name());
            assert_eq!(meta.version, "1.0.0");
            assert_eq!(meta.module_type, "2d");
            assert_eq!(meta.audio_features, &["bass", "mid", "treble"]);
        }
    }

    #[test]
    fn plane9_alias_table_matches_the_registered_ports() {
        let canonical_names: Vec<&str> = all_scenes()
            .iter()
            .map(|scene| scene.name())
            .filter(|name| name.starts_with(PLANE9_PREFIX))
            .collect();
        assert_eq!(canonical_names.len(), PLANE9_SCENE_NAMES.len());
        for canonical in PLANE9_SCENE_NAMES {
            assert!(
                canonical_names.contains(canonical),
                "no registered scene for {canonical}"
            );
            let legacy = canonical.replacen(PLANE9_PREFIX, POINT10_PREFIX, 1);
            assert_eq!(canonical_scene_name(&legacy), Some(*canonical));
            assert_eq!(resolve_scene_name(&legacy), *canonical);
            assert_eq!(canonical_scene_name(canonical), None);
            assert_eq!(resolve_scene_name(canonical), *canonical);
        }
        // Scenes that kept their `Point10 ` name (including the other Plane9
        // port, Around Plus) are never aliases.
        for name in [
            "Point10 Around Plus",
            "Point10 Particle Field",
            "Plane9 Unknown",
        ] {
            assert_eq!(canonical_scene_name(name), None);
            assert_eq!(resolve_scene_name(name), name);
        }
    }

    #[test]
    fn rename_legacy_scene_names_rewrites_only_aliases_and_is_idempotent() {
        let mut document = serde_json::json!({
            "$moduleName": "Point10 Light Tunnel",
            "meta": {"name": "Point10 Earth", "author": "Plane9 / Joakim Dahl"},
            "groups": [{"modules": ["Point10 Boxy", "Point10 Around Plus"]}],
            "cells": [["Plane9 Sun", "Point10 Sun"]],
            "count": 2,
            "enabled": true,
            "empty": null,
            "Point10 Boxy": "a key is not an alias",
        });
        let expected = serde_json::json!({
            "$moduleName": "Plane9 Light Tunnel",
            "meta": {"name": "Plane9 Earth", "author": "Plane9 / Joakim Dahl"},
            "groups": [{"modules": ["Plane9 Boxy", "Point10 Around Plus"]}],
            "cells": [["Plane9 Sun", "Plane9 Sun"]],
            "count": 2,
            "enabled": true,
            "empty": null,
            "Point10 Boxy": "a key is not an alias",
        });
        rename_legacy_scene_names(&mut document);
        assert_eq!(document, expected);
        let once = serde_json::to_string(&document).expect("JSON serialises");
        rename_legacy_scene_names(&mut document);
        assert_eq!(document, expected);
        assert_eq!(
            serde_json::to_string(&document).expect("JSON serialises"),
            once
        );
    }

    #[test]
    fn every_scene_declares_the_responsiveness_levers() {
        for scene in all_scenes() {
            let props = scene.props();
            for levers in audio_bridge::RESPONSIVENESS_PROPS {
                assert!(
                    props.iter().any(|prop| prop.name == levers.name),
                    "{} is missing {}",
                    scene.name(),
                    levers.name
                );
            }
        }
    }

    #[test]
    fn prop_read_clamps_and_defaults() {
        let distortion = SceneProp::float("distortion", "Chroma", 0.05, 0.0, 0.5, 0.001);
        assert_eq!(distortion.read(None), 0.05);
        assert_eq!(distortion.read(Some(f32::NAN)), 0.05);
        assert_eq!(distortion.read(Some(-2.0)), 0.0);
        assert_eq!(distortion.read(Some(9.0)), 0.5);
        assert_eq!(distortion.read(Some(0.25)), 0.25);

        let use_agc = SceneProp::boolean("useAgc", "Auto Gain (AGC)", true);
        // `props.useAgc !== false`
        assert_eq!(use_agc.read(None), 1.0);
        assert_eq!(use_agc.read(Some(1.0)), 1.0);
        assert_eq!(use_agc.read(Some(0.0)), 0.0);
    }

    #[test]
    fn read_scene_props_keeps_schema_order() {
        const SCHEMA: &[SceneProp] = &[
            SceneProp::float("height", "Height", 1.0, 0.0, 4.0, 0.01),
            SceneProp::float("speed", "Wave Speed", 1.0, 0.0, 4.0, 0.01),
        ];
        let values = read_scene_props(SCHEMA, |key| (key == "speed").then_some(3.0));
        assert_eq!(values, vec![("height", 1.0), ("speed", 3.0)]);
    }

    #[test]
    fn frame_dt_matches_the_adapter_clock() {
        assert_eq!(frame_dt(1.0 / 60.0), 1.0 / 60.0);
        assert_eq!(frame_dt(0.0), 0.0);
        assert_eq!(frame_dt(5.0), MAX_FRAME_SECONDS);
        assert_eq!(frame_dt(-1.0), 0.0);
        assert_eq!(frame_dt(f32::NAN), 0.0);
        assert_eq!(frame_dt(f32::INFINITY), 0.0);
    }

    #[test]
    fn unit_quad_is_wound_counter_clockwise() {
        // wgpu's default front face is CCW in y-up clip space; the quad must
        // not rely on culling being off.
        let vertices = UnitQuad::VERTICES;
        let a = (vertices[0][0], vertices[0][1]);
        let b = (vertices[1][0], vertices[1][1]);
        let c = (vertices[2][0], vertices[2][1]);
        let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        assert!(cross > 0.0, "expected CCW winding, cross = {cross}");
    }

    /// RED contract for `Things/Spiky.xml`. This deliberately resolves the
    /// not-yet-existing scene through `all_scenes()` and reads not-yet-existing
    /// source files at runtime, so it compiles against today's API and fails on
    /// the missing behaviour rather than on missing Rust symbols.
    #[test]
    fn plane9_spiky_red_contract() {
        use std::collections::BTreeSet;
        use std::fs;
        use std::path::{Path, PathBuf};

        use crate::point10::audio_bridge::{AudioBridge, BandPayload};
        use crate::point10::postfx::Plane9BloomParams;
        use crate::point10::scenes::cube_mesh::build_box;
        use crate::point10::scenes::plane9_common::PLANE9_BATCH_WGSL;
        use crate::point10::scenes::spiky::build_spiky_mesh;
        use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
        use crate::point10::shared::p9_shading::P9_SHADING_WGSL;
        use crate::point10::test_support::{TestTarget, any_lit, gpu};

        fn find_source(root: &Path, filename_fragment: &str) -> Option<PathBuf> {
            let mut pending = vec![root.to_path_buf()];
            while let Some(dir) = pending.pop() {
                for entry in fs::read_dir(dir).ok()? {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    if path.is_dir() {
                        pending.push(path);
                    } else if path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.to_ascii_lowercase().contains(filename_fragment))
                    {
                        return Some(path);
                    }
                }
            }
            None
        }

        fn compact(source: &str) -> String {
            source.split_whitespace().collect::<Vec<_>>().join(" ")
        }

        let mut failures = Vec::new();
        macro_rules! check {
            ($condition:expr, $($arg:tt)+) => {
                if !$condition {
                    failures.push(format!($($arg)+));
                }
            };
        }

        // D — exactly one new canonical scene, with the standard Plane9 meta.
        let mut scenes = all_scenes();
        let names: Vec<&str> = scenes.iter().map(|scene| scene.name()).collect();
        let unique_names: BTreeSet<&str> = names.iter().copied().collect();
        check!(
            scenes.len() == 52,
            "roster: expected 52 scenes, got {}",
            scenes.len()
        );
        check!(
            unique_names.len() == names.len(),
            "roster: duplicate names in {names:?}"
        );
        check!(
            PLANE9_SCENE_NAMES.len() == 34,
            "Plane9 alias roster: expected 34 names, got {}",
            PLANE9_SCENE_NAMES.len()
        );
        let spiky_matches: Vec<_> = scenes
            .iter()
            .filter(|scene| scene.name() == "Plane9 Spiky")
            .collect();
        check!(
            spiky_matches.len() == 1,
            "Plane9 Spiky: expected exactly one registration, got {}",
            spiky_matches.len()
        );
        if let Some(scene) = spiky_matches.first() {
            let meta = scene.meta();
            check!(meta.name == scene.name(), "Spiky: meta/name mismatch");
            check!(
                meta.author == "Plane9 (Joakim Dahl)",
                "Spiky: author is {:?}",
                meta.author
            );
            check!(
                meta.version == "1.0.0",
                "Spiky: version is {:?}",
                meta.version
            );
            check!(
                meta.module_type == "2d",
                "Spiky: type is {:?}",
                meta.module_type
            );
            check!(
                meta.audio_features == &["bass", "mid", "treble"],
                "Spiky: audio features are {:?}",
                meta.audio_features
            );
        }
        check!(
            PLANE9_SCENE_NAMES.contains(&"Plane9 Spiky"),
            "Plane9 Spiky: missing from the alias/Gallery roster"
        );
        check!(
            canonical_scene_name("Point10 Spiky") == Some("Plane9 Spiky"),
            "Point10 Spiky: legacy alias does not resolve"
        );
        check!(
            gallery_kind("Plane9 Spiky", "point10") == "plane9",
            "Plane9 Spiky: Gallery kind is not plane9"
        );
        let spiky_index = scenes
            .iter()
            .position(|scene| scene.name() == "Plane9 Spiky");

        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.join("../..");
        let point10_root = manifest_dir.join("src/point10");

        // D — committed parity and generated catalogue data retain 52 Point10
        // scenes and total 503 after three shader effects were retired. The
        // catalogue is checked as data.
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(repo_root.join("tests/fixtures/module-parity.json"))
                .expect("read module-parity.json"),
        )
        .expect("parse module-parity.json");
        check!(
            manifest["counts"]["point10"].as_u64() == Some(52),
            "manifest: point10 count is {:?}, expected 52",
            manifest["counts"]["point10"]
        );
        check!(
            manifest["counts"]["total"].as_u64() == Some(503),
            "manifest: total is {:?}, expected 503",
            manifest["counts"]["total"]
        );
        let manifest_names: Vec<&str> = manifest["point10"]
            .as_array()
            .expect("manifest point10 array")
            .iter()
            .map(|name| name.as_str().expect("manifest module name"))
            .collect();
        check!(
            manifest_names.len() == 52,
            "manifest: expected 52 Point10 names, got {}",
            manifest_names.len()
        );
        check!(
            manifest_names
                .iter()
                .filter(|name| **name == "Plane9 Spiky")
                .count()
                == 1,
            "manifest: Plane9 Spiky must appear exactly once"
        );

        let catalogue: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(repo_root.join("docs/modules/modules.json"))
                .expect("read docs/modules/modules.json"),
        )
        .expect("parse docs/modules/modules.json");
        check!(
            catalogue["counts"]["point10"].as_u64() == Some(52),
            "catalogue: point10 count is {:?}, expected 52",
            catalogue["counts"]["point10"]
        );
        check!(
            catalogue["counts"]["total"].as_u64() == Some(503),
            "catalogue: total is {:?}, expected 503",
            catalogue["counts"]["total"]
        );
        let catalogue_modules = catalogue["modules"].as_array().expect("catalogue modules");
        let catalogue_spiky: Vec<_> = catalogue_modules
            .iter()
            .filter(|module| module["name"] == "Plane9 Spiky")
            .collect();
        check!(
            catalogue_spiky.len() == 1,
            "catalogue: Plane9 Spiky must appear exactly once, got {}",
            catalogue_spiky.len()
        );
        if let Some(module) = catalogue_spiky.first() {
            check!(
                module["kind"] == "point10"
                    && module["author"] == "Plane9 (Joakim Dahl)"
                    && module["version"] == "1.0.0"
                    && module["audioFeatures"] == serde_json::json!(["bass", "mid", "treble"]),
                "catalogue: Plane9 Spiky metadata is {:?}",
                module
            );
        }

        // D — provenance row and the UI's exact 4.0 licence label mapping.
        let notices = fs::read_to_string(repo_root.join("THIRD_PARTY_NOTICES.md"))
            .expect("read THIRD_PARTY_NOTICES.md");
        let notice_rows: Vec<_> = notices
            .lines()
            .filter(|line| line.starts_with("| Spiky |"))
            .collect();
        match notice_rows.as_slice() {
            [row]
                if row.contains("CC BY-NC-SA 4.0")
                    && row.to_ascii_lowercase().contains("spiky") => {}
            [row] => check!(false, "notices: wrong Spiky provenance row {row:?}"),
            [] => check!(false, "notices: missing Spiky provenance row"),
            _ => check!(
                false,
                "notices: duplicate Spiky provenance rows {notice_rows:?}"
            ),
        }
        let ui_model = fs::read_to_string(repo_root.join("crates/modv-ui/src/model.rs"))
            .expect("read modv-ui model.rs");
        let v4_arm = ui_model
            .split("// Plane9 ports whose own headers state 4.0.")
            .nth(1)
            .and_then(|arm| arm.split("// Point10 variants").next())
            .unwrap_or_default();
        check!(
            v4_arm.contains("\"Spiky\""),
            "UI licence map: Spiky is missing from the exact CC BY-NC-SA 4.0 arm"
        );
        check!(
            compact(&ui_model).contains("checked.len(), 52,"),
            "UI licence provenance test: expected count must move from 51 to 52"
        );

        // A/B — the existing box primitive is only the 24-vertex/12-triangle
        // starting point. It cannot itself supply the >1.175 spike radius; the
        // reusable mesh_ops module and Spiky chain must exist alongside it.
        let cube = build_box(1.0, 1.0, 1.0);
        check!(cube.vertices.len() == 24, "Cube1: expected 24 vertices");
        check!(cube.indices.len() == 36, "Cube1: expected 12 triangles");
        let cube_max_radius = cube
            .vertices
            .iter()
            .map(|vertex| {
                vertex.position[0]
                    .hypot(vertex.position[1])
                    .hypot(vertex.position[2])
            })
            .fold(0.0_f32, f32::max);
        check!(
            (cube_max_radius - (3.0_f32 / 4.0).sqrt()).abs() < 1e-6,
            "Cube1: unexpected local radius {cube_max_radius}"
        );
        check!(
            cube_max_radius < 1.175,
            "plain cube cannot be the spike mesh: radius {cube_max_radius} must stay below 1.175"
        );

        let mesh_ops_path = find_source(&point10_root, "mesh_ops");
        let mesh_ops_source = mesh_ops_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok());
        check!(
            mesh_ops_source.is_some(),
            "mesh_ops: reusable CPU mesh-modifier module is missing"
        );
        if let Some(source) = &mesh_ops_source {
            let source = source.to_ascii_lowercase();
            for operation in [
                "subdivide",
                "select",
                "extrude",
                "randomize",
                "local_scale",
                "invert",
            ] {
                check!(
                    source.contains(operation),
                    "mesh_ops: missing {operation} support required by the Spiky chain"
                );
            }
        }

        let spiky_path = find_source(&point10_root, "spiky");
        let spiky_source = spiky_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok());
        check!(
            spiky_source.is_some(),
            "Plane9 Spiky scene source is missing"
        );
        if let Some(source) = &spiky_source {
            let source = compact(source);
            let lower = source.to_ascii_lowercase();
            check!(
                lower.contains("mesh_ops"),
                "Spiky: scene does not use mesh_ops"
            );

            // E — camera, render state, targets and bloom pinned to the XML.
            for needle in [
                "[0.0, 0.0, 1.0]",
                "[0.0, 0.0, -1.0]",
                "70.0",
                "0.1",
                "1000.0",
                "[0.5, 0.5, 0.5]",
                "Rgba16Float",
                "mip_level_count: 1",
                "Plane9BloomParams::new(5.0, 0.0, 0.5, 1.0, false)",
                "build_box(1.0, 1.0, 1.0)",
                "load_stormy_cube",
                "depth_write_enabled: Some(true)",
                "CompareFunction::Less",
                "CullMode::Back",
            ] {
                check!(source.contains(needle), "Spiky constants: lost {needle}");
            }
            for needle in [
                "0.88",
                "local_scale: 0.0",
                "amount: 0.1",
                "random_seed: 1",
                "num_steps: 2",
                "smoothness: 1.0",
                "selection_type: 4",
                "logic: 1",
                "invert: false",
                "output: 1",
                "random_factor: 1.0",
                "direction: 0",
                "input_component: 0",
                "output_component: 3",
                "[2.0, 2.0, 1.0]",
            ] {
                check!(
                    lower.contains(&needle.to_ascii_lowercase()),
                    "Spiky mesh chain: lost {needle}"
                );
            }

            // B/C/E — local positions feed the tip distance; 2,2,1 remains a
            // model transform. The scalar noise displacement broadcasts on XYZ.
            for needle in [
                "opos",
                "vpos",
                "wpos",
                "wnormal",
                "viewdir",
                "(length(position) - 0.5) / 0.75",
                "d * (p9_noise",
                "g_time * 0.5",
                "u.model",
                "[2.0, 2.0, 1.0]",
                "p9_perturb_normal",
                "dHdxy_fwd",
                "* 0.002",
                "vec3<f32>(0.5, 0.48, 0.53) * 0.75",
                "vec3<f32>(0.247059, 0.474510, 0.811765)",
                "vec3<f32>(0.929412, 0.615686, 0.207843)",
                "vec3<f32>(40.0, 50.0, -25.0)",
                "vec3<f32>(-40.0, 50.0, 25.0)",
                "0.2 * 0.2 * 8.0",
                "vec3<f32>(0.03)",
                "> 0.9",
                "vec4<f32>(1.0, 0.0, 0.0, 1.0) * 10.0",
                "let bg = vec4<f32>(0.0);",
                "p9_tonemap_aces(c2.rgb * 0.5)",
                "p9_tonemap_aces(c1.rgb * 0.75)",
                "vec4<f32>(mix(bg.rgb",
            ] {
                check!(source.contains(needle), "Spiky shader: lost {needle}");
            }
            check!(
                !lower.contains("p9_fbm") && !lower.contains("_fbm("),
                "Spiky: dead RenderToTexture1 inline fbm shader must not be implemented"
            );

            // E — the three vendor expressions are retained verbatim.
            for expression in [
                "in1.x = min(0.5, band(-1, 1, 0, noise1(time*0.1)*0.7)*5);",
                "in1.y = beat(0);",
                "b = band(-1, 0, 0, 0)*1;",
                "in1.x = min(0.01, b*b*b*5);",
                "perm = perm+((beat(0.1)*0.1+0.001)*deltatime);",
                "out1=noise2(perm, 0)*360;",
                "out2=noise2(perm, 1)*360;",
                "out3=noise2(perm, 2)*360;",
            ] {
                check!(
                    source.contains(&compact(expression)),
                    "Spiky expression: lost {expression}"
                );
            }
        }

        // C — shared helpers exist and a composed fragment calling both parses
        // and validates with naga. The live scene initialisation below compiles
        // the full production composition when the scene is present.
        let shared = format!("{P9_COMMON_WGSL}\n{P9_SHADING_WGSL}\n{PLANE9_BATCH_WGSL}");
        let has_ridged = shared.contains("fn p9_ridgedmf_fast(");
        let has_perturb = shared.contains("fn p9_perturb_normal(");
        check!(has_ridged, "shared WGSL: p9_ridgedmf_fast is missing");
        check!(has_perturb, "shared WGSL: p9_perturb_normal is missing");
        if has_ridged && has_perturb {
            let program = format!(
                "{shared}\n@fragment\nfn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{\n  let p = vec3<f32>(position.xy * 0.01, 0.5);\n  let h = p9_ridgedmf_fast(p, 4u);\n  let n = p9_perturb_normal(p, vec3<f32>(0.0, 0.0, 1.0), vec2<f32>(h, 0.0));\n  return vec4<f32>(n, 1.0);\n}}\n"
            );
            match naga::front::wgsl::parse_str(&program) {
                Ok(module) => {
                    if let Err(error) = naga::valid::Validator::new(
                        naga::valid::ValidationFlags::all(),
                        naga::valid::Capabilities::all(),
                    )
                    .validate(&module)
                    {
                        check!(false, "shared Spiky WGSL failed validation: {error}");
                    }
                }
                Err(error) => check!(false, "shared Spiky WGSL failed to parse: {error}"),
            }
        }

        // Keep the bloom construction itself honest even before the scene
        // constants can be referenced directly.
        let bloom = Plane9BloomParams::new(5.0, 0.0, 0.5, 1.0, false);
        check!(
            (
                bloom.radius(),
                bloom.threshold(),
                bloom.smoothness(),
                bloom.intensity(),
                bloom.antiflicker(),
                bloom.mip_count(),
            ) == (5.0, 0.0, 0.5, 1.0, false, 1),
            "Plane9 bloom helper no longer matches (5,0,0.5,1,false)"
        );

        // F — 64x64 two-frame smoke plus a driven-frame render and a geometric
        // tip-band check. A present-but-silent payload makes band(-1,1,0,...)
        // exactly zero; the driven frame uses damping-1 bass and dt=0 so
        // geometry/rotation stay put.
        if let (Some(index), Some((device, queue))) = (spiky_index, gpu()) {
            let mut scene = scenes.remove(index);
            let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            ctx.resize(64, 64);
            scene.init(&mut ctx);
            scene.resize(64, 64);
            let mut payload = BandPayload::silent(44_100.0);
            let mut bridge = AudioBridge::new();
            let target = TestTarget::new(&device, 64, 64);
            for frame in 0..2 {
                bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
                scene.update(1.0 / 60.0, &bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-plane9-spiky-red-smoke"),
                });
                scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                let pixels = target.read(&device, &queue);
                check!(
                    any_lit(&pixels),
                    "Spiky GPU smoke frame {frame} is uniformly black"
                );
            }

            payload.lanes[3] = 0.2; // damping-1 bass -> min(0.5, 0.2*5) == 0.5
            bridge.update(0.0, Some(&payload), &Default::default());
            scene.update(0.0, &bridge);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("point10-plane9-spiky-red-tips"),
            });
            scene.render(&mut encoder, &target.view);
            queue.submit(Some(encoder.finish()));
            let high = target.read(&device, &queue);
            check!(any_lit(&high), "Spiky: driven frame is uniformly black");
            // The Stormy Days cubemap is a warm sunset, so the silent frame
            // legitimately has red-dominant grazing rims: "red pixel means
            // beat" is not a fidelity property. With the vendor `Amount 0.88`
            // the extreme apexes also fall outside this 64x64 camera frustum,
            // so no rasterised tip is required here. What *is* contractually
            // true is the mechanism the fragment uses: the geometry owns
            // vertices in the `d > 0.9` tip band and the driven `in1.x`
            // selects how much red those vertices receive (checked by the
            // scene's own unit test).
            let mesh = build_spiky_mesh();
            let tip_vertices = mesh
                .positions
                .iter()
                .filter(|p| (p[0].hypot(p[1]).hypot(p[2]) - 0.5) / 0.75 > 0.9)
                .count();
            check!(
                tip_vertices > 0,
                "Spiky: vendor mesh has no vertices in the fragment's d > 0.9 tip band"
            );
            scene.dispose();
        } else if spiky_index.is_none() {
            check!(
                false,
                "Spiky GPU smoke: cannot run until the scene is registered"
            );
        } else {
            eprintln!("skipping Plane9 Spiky GPU smoke: no fallback adapter");
        }

        assert!(
            failures.is_empty(),
            "Plane9 Spiky RED contract failed ({} checks):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
