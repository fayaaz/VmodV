//! The combined module registry (M3 + M4): shader modules, the ported 2D
//! modules and the ported Point10 scenes.
//!
//! M2 only knew the shader registry ([`modv_renderers::shader::all_modules`]);
//! `modules/makeActiveModule` therefore turned every 2D module into an
//! "unregistered" record and the compositor logged and skipped it. M3 adds the
//! 15 hand-written 2D modules ([`modv_renderers::two_d::all_modules`]) to the
//! same lookup table so
//!
//! * `modules/makeActiveModule` produces a real record with `meta.type: "2d"`
//!   and the module's prop defaults, and
//! * the compositor can tell a shader module (pipeline slot) from a 2D module
//!   (canvas facade instance) by name.
//!
//! M4 adds the Point10 scenes ([`modv_renderers::point10::all_scenes`]). They
//! are registered as **`meta.type: "2d"`** modules under their JS wrapper name
//! (`Point10 <Scene>`, `renderers/2d/modules/Point10*.js`) so Scene documents,
//! presets and the Gallery keep working unchanged, but they render through
//! their own scene path in the compositor (see [`ModuleKind::Point10`]). The
//! Gallery `category`, `audioFeatures` and prop schema come straight from each
//! scene's [`modv_renderers::point10::Point10Scene::meta`]/`props` report.
//!
//! ## Point10 prop binding
//!
//! [`modv_renderers::point10::Point10Scene`] deliberately has no prop binder:
//! the scene schemas live in `modv-renderers` while the resolved prop values
//! live in the engine store. The vendored scenes do expose an inherent
//! `apply_props(getter)` (the `sceneProps.js` binder), so
//! [`Point10SceneKind`] wraps the concrete Point10 scenes, delegates the
//! trait, and adds [`Point10SceneKind::apply_props`]. The compositor needs
//! that wrapper to push prop changes into a live scene; the trait-object form
//! the plan asks for is [`ModuleRegistry::create_point10`].
//!
//! ## Two prop-metadata gaps in [`TwoDProp`]
//!
//! `TwoDProp` carries `name`, `prop_type`, `default`, `min`, `max`, `label`,
//! `enum_members`, `component` and `step` — everything the controls need — but
//! **not** the JS descriptor's `strict`/`abs` coercion flags, and
//! [`TwoDModuleMeta`] has no `previewWithOutput`. Rather than change
//! `modv-renderers` in M3, this file keeps the handful of JS declarations that
//! set them in [`TWO_D_PROP_FLAGS`] / [`TWO_D_PREVIEW_WITH_OUTPUT`]. Both
//! tables are exhaustive against the corpus (`grep -n "strict:\\|abs:"
//! renderers/2d/modules/*.js`) and are the intended home for the flags once
//! `TwoDProp`/`TwoDModuleMeta` grow the fields.
//!
//! ## Prop coercion
//!
//! [`ModuleRegistry::prop_schema`] maps a [`TwoDProp`] onto the same
//! [`PropSchema`] the shader path uses, so `modules/updateProp`
//! (`modules.js:481-562`) runs one coercion pipeline for both module types:
//! `type` is the JS `dataTypes` key (`TwoDProp::prop_type`), `min`/`max` come
//! from the descriptor, and `strict`/`abs` from the flags table above.
//!
//! ## Default props and `data`
//!
//! [`ModuleRegistry::default_props`] is the JS `getPropDefault`
//! (`utils/get-prop-default.js`): the descriptor's `default` verbatim, in
//! declaration order. The 2D modules keep their mutable state in the
//! [`TwoDModule`] instance (the port of the JS `data` object), so the store's
//! `data` starts empty — see the M3 report and the compositor's module docs.
//!
//! ## Registry parity
//!
//! The built-in registry covers the legacy Gallery set: 15 ported 2D modules,
//! 27 Point10 scenes and 36 shader modules, plus the VmodV-only `Video` 2D
//! module and the 354 `.fs` shaders of the ISF corpus (433 total). Names match
//! the JS registrations
//! (`X Drips`, `Texture 2D`, `Grid Stretch`, …) so presets cross over
//! unchanged. `module_registry_matches_the_legacy_manifest` compares this
//! registry against the committed snapshot generated from the legacy worker's
//! `import.meta.glob` list (`tests/fixtures/module-parity.json`), and
//! `every_default_scene_module_resolves_in_the_registry` checks the seeded
//! default Scenes against it.
//!
//! ## ISF corpus (WS-4)
//!
//! The 354 `.fs` shaders under `resources/isf`
//! are not compiled into the binary: [`ModuleRegistry::build`] scans a
//! directory at startup and registers one [`IsfDefinition`] per parsable
//! shader (name = file stem, matching the JS `getIsfModuleName`). The scan is
//! header-only ([`parse_isf`] never compiles GLSL), so startup stays fast; the
//! compositor compiles a shader the first time its module draws.
//!
//! * [`ModuleRegistry::builtin`] registers no ISF module, which keeps the
//!   existing unit tests and the headless benchmark independent of a corpus
//!   on disk.
//! * The engine resolves the corpus directory from `--isf-dir`, then
//!   `$MODV_ISF_DIR`, then a development default relative to the executable
//!   (see [`crate::cli::resolve_isf_dir`]) and hands it to
//!   [`ModuleRegistry::build`]. The VmodV packages ship the corpus as
//!   `/usr/share/vmodv/isf` (`.deb`) or `usr/share/vmodv/isf` inside the
//!   AppImage (see `tools/vmodv-appimage.sh`/`tools/vmodv-deb.sh`).
//! * A file that cannot be read or parsed is skipped with a one-line warning;
//!   the summary line reports the registered/failed counts.
//! * The built-in corpus is scanned flat. User ISF folders supplied through
//!   [`ModuleRegistry::build_with_extra`] are scanned **recursively** (sorted
//!   paths, hidden/temporary entries skipped, depth/entry caps per root); the
//!   module name stays the file stem. Duplicate stems keep the corpus first,
//!   then the first sorted path, and the ignored copies are logged once.
//! * ISF float props carry `MIN`/`MAX` but never `strict` in the JS renderer
//!   (`renderers/isf/index.js:210-257`), so writes are **not** clamped. The
//!   range is metadata for the UI controls only, exactly as in JS.
//! * A `texture` prop is a declared image input, not a shader uniform: it is
//!   registered so the UI can show it. The compositor resolves assigned image
//!   props through `texture_sources` and keeps the group canvas for unassigned
//!   inputs (the JS default at `renderers/isf/index.js:84-91`).

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use modv_engine::props::PropSchema;
use modv_renderers::isf::{
    IsfDocument, IsfMeta, IsfProp, IsfPropType, meta_from_document, parse_isf, synthesise_props,
};
use modv_renderers::milkdrop::{
    MilkMeta, MilkProp, meta_from_document as milk_meta_from_document, parse_milk,
    synthesise_props as milk_synthesise_props,
};
use modv_renderers::point10::chromatic::line::ChromaticLine;
use modv_renderers::point10::chromatic::wave::ChromaticWave;
use modv_renderers::point10::scenes::around_plus::AroundPlus;
use modv_renderers::point10::scenes::attractor_particles::AttractorParticles;
use modv_renderers::point10::scenes::boxy::Boxy;
use modv_renderers::point10::scenes::color_cube::ColorCube;
use modv_renderers::point10::scenes::dvd_room::DvdRoom;
use modv_renderers::point10::scenes::earth::Earth;
use modv_renderers::point10::scenes::glass_lens::GlassLens;
use modv_renderers::point10::scenes::hollow_cube::HollowCube;
use modv_renderers::point10::scenes::it_is_coming_for_you::ItIsComingForYou;
use modv_renderers::point10::scenes::latent_backrooms::LatentBackrooms;
use modv_renderers::point10::scenes::light_tunnel::LightTunnel;
use modv_renderers::point10::scenes::mirror_cube::MirrorCube;
use modv_renderers::point10::scenes::mycelium::Mycelium;
use modv_renderers::point10::scenes::particle_field::ParticleField;
use modv_renderers::point10::scenes::rolling_fire::{BallOfFire, RollingFire};
use modv_renderers::point10::scenes::smoke_room::SmokeRoom;
use modv_renderers::point10::scenes::sound_cube::SoundCube;
use modv_renderers::point10::scenes::spiral_particles::SpiralParticles;
use modv_renderers::point10::scenes::square_tunnel::SquareTunnel;
use modv_renderers::point10::scenes::sun::Sun;
use modv_renderers::point10::scenes::ultra_hdr::UltraHdr;
use modv_renderers::point10::scenes::water_ripple::WaterRipple;
use modv_renderers::point10::scenes::waveform::Waveform;
use modv_renderers::point10::scenes::wormhole::Wormhole;
use modv_renderers::point10::{
    Point10Scene, SceneMeta, SceneProp, ScenePropKind, all_scenes as all_point10_scenes,
};
use modv_renderers::shader::uniforms::{PropKind, ShaderProp};
use modv_renderers::shader::{ShaderModule, all_modules as all_shader_modules};
use modv_renderers::two_d::modules::default_props as two_d_default_props;
use modv_renderers::two_d::{
    TwoDModule, TwoDModuleMeta, TwoDProp, all_modules as all_two_d_modules,
};

/// Which renderer owns a registered module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    /// A `modv-renderers::shader` module (fragment WGSL, pipeline slot).
    Shader,
    /// A `modv-renderers::two_d` module (canvas facade instance).
    TwoD,
    /// A `modv-renderers::point10` scene (own target texture + composite).
    ///
    /// Registered with `meta.type: "2d"` (the JS `createPoint10Module` meta) so
    /// Scene documents and presets resolve it, but rendered through the Point10
    /// path in the compositor, never the 2D canvas facade.
    Point10,
    /// A vendored ISF shader (`modv-renderers::isf`), registered with
    /// `meta.type: "isf"` and rendered through the ISF runtime into its own
    /// target texture (`IsfRuntime::draw`), then composited onto the group.
    Isf,
    /// A MilkDrop `.mlk` preset (`modv-renderers::milkdrop`), registered with
    /// `meta.type: "milkdrop"` following the ISF arm.
    Milkdrop,
}

/// A registered 2D module: its static metadata and prop descriptors, factored
/// out of the module instance so the store can answer without building one.
#[derive(Clone, Debug, PartialEq)]
pub struct TwoDDefinition {
    /// `meta` (`name`, `author`, `version`, `category`, `audioFeatures`).
    pub meta: TwoDModuleMeta,
    /// Prop descriptors in declaration order.
    pub props: Vec<TwoDProp>,
}

/// A registered Point10 scene: the metadata and prop schema the scene reports,
/// factored out of the GPU-backed instance.
///
/// Unlike [`TwoDDefinition`] there is no separate descriptor type: the scene's
/// [`SceneMeta`]/[`SceneProp`] values *are* the JS `meta`/`props` contract
/// (`adapter.js:857-871`), so they are stored verbatim.
#[derive(Clone, Debug, PartialEq)]
pub struct Point10Definition {
    /// `meta` (`name`, `author`, `version`, `module_type`, `category`,
    /// `audio_features`).
    pub meta: SceneMeta,
    /// The scene's own schema preceded by the responsiveness levers
    /// ([`modv_renderers::point10::scene_props_with_levers`]).
    pub props: Vec<SceneProp>,
}

/// A registered ISF corpus shader: the parsed document plus the synthesised
/// `meta`/prop table the store needs.
///
/// The parsed [`IsfDocument`] is kept so the compositor can compile the shader
/// on first draw ([`crate::compositor`]) without re-reading the file. The
/// document only holds header data and the GLSL body; no GPU work happens at
/// registration.
#[derive(Clone, Debug, PartialEq)]
pub struct IsfDefinition {
    /// Module name: the `.fs` file stem (`getIsfModuleName`,
    /// `worker/index.worker.js:27`).
    pub name: String,
    /// Metadata from the ISF header (`CREDIT`/`DESCRIPTION`/`VSN`/
    /// `MODV_CATEGORY`/`ISFVSN`).
    pub meta: IsfMeta,
    /// Synthesised props in declaration order, keyed by the ISF input `NAME`.
    ///
    /// Kept as an ordered `Vec` rather than a map because `modv-app` does not
    /// depend on `indexmap`; order is the JS declaration order and is what the
    /// prop panels and the compositor iterate.
    pub props: Vec<(String, IsfProp)>,
    /// The parsed shader (header inputs/passes + the GLSL body).
    pub document: IsfDocument,
}

impl IsfDefinition {
    /// The descriptor for one of the shader's inputs, if declared.
    pub fn prop(&self, name: &str) -> Option<&IsfProp> {
        self.props
            .iter()
            .find(|(prop_name, _)| prop_name == name)
            .map(|(_, prop)| prop)
    }
}

/// A registered MilkDrop `.mlk` preset: its display name, derived metadata and
/// synthesised prop table.
#[derive(Clone, Debug, PartialEq)]
pub struct MilkdropDefinition {
    /// Module name: the `.mlk` file stem.
    pub name: String,
    /// Metadata derived with `milkdrop::meta_from_document`.
    pub meta: MilkMeta,
    /// Synthesised props (currently empty: presets are EEL-driven), keyed by
    /// prop name in declaration order — the value returned by `synthesise_props`.
    pub props: IndexMap<String, MilkProp>,
}

/// The app-side wrapper around the concrete Point10 scenes.
///
/// [`modv_renderers::point10::Point10Scene`] is the render contract and has no
/// prop binder, while every ported scene exposes an inherent
/// `apply_props(getter)` (the `sceneProps.js` binder). This enum keeps that
/// capability: it delegates every trait method to the live scene and adds
/// [`Point10SceneKind::apply_props`], which the compositor calls once per frame.
///
/// A new scene must be added here *and* to
/// [`modv_renderers::point10::all_scenes`]; `every_registered_scene_constructs`
/// fails when the two drift.
#[allow(clippy::large_enum_variant)]
pub enum Point10SceneKind {
    /// `Point10 Chromatic Line` (`chromatic/line.rs`).
    ChromaticLine(ChromaticLine),
    /// `Point10 Chromatic Wave` (`chromatic/wave.rs`).
    ChromaticWave(ChromaticWave),
    /// `Point10 Particle Field` (`scenes/particle_field.rs`).
    ParticleField(ParticleField),
    /// `Point10 Spiral Particles` (`scenes/spiral_particles.rs`).
    SpiralParticles(SpiralParticles),
    /// `Point10 Waveform` (`scenes/waveform.rs`).
    Waveform(Waveform),
    /// `Point10 Square Tunnel` (`scenes/square_tunnel.rs`).
    SquareTunnel(SquareTunnel),
    /// `Point10 Light Tunnel` (`scenes/light_tunnel.rs`).
    LightTunnel(LightTunnel),
    /// `Point10 Wormhole` (`scenes/wormhole.rs`).
    Wormhole(Wormhole),
    /// `Point10 It Is Coming For You` (`scenes/it_is_coming_for_you.rs`).
    ItIsComingForYou(ItIsComingForYou),
    /// `Point10 Water Ripple` (`scenes/water_ripple.rs`).
    WaterRipple(WaterRipple),
    /// `Point10 Glass Lens` (`scenes/glass_lens.rs`).
    GlassLens(GlassLens),
    /// `Point10 Mycelium` (`scenes/mycelium.rs`).
    Mycelium(Mycelium),
    /// `Point10 Around Plus` (`scenes/around_plus.rs`).
    AroundPlus(AroundPlus),
    /// `Point10 Ultra HDR` (`scenes/ultra_hdr.rs`).
    UltraHdr(UltraHdr),
    /// `Point10 Mirror Cube` (`scenes/mirror_cube.rs`).
    MirrorCube(MirrorCube),
    /// `Point10 Color Cube` (`scenes/color_cube.rs`).
    ColorCube(ColorCube),
    /// `Point10 Boxy` (`scenes/boxy.rs`).
    Boxy(Boxy),
    /// `Point10 Sound Cube` (`scenes/sound_cube.rs`).
    SoundCube(SoundCube),
    /// `Point10 Hollow Cube` (`scenes/hollow_cube.rs`).
    HollowCube(HollowCube),
    /// `Point10 Attractor Particles` (`scenes/attractor_particles.rs`).
    AttractorParticles(AttractorParticles),
    /// `Point10 DVD Room` (`scenes/dvd_room.rs`).
    DvdRoom(DvdRoom),
    /// `Point10 Rolling Fire` (`scenes/rolling_fire.rs`).
    RollingFire(RollingFire),
    /// `Point10 Ball Of Fire` (`scenes/rolling_fire.rs`).
    BallOfFire(BallOfFire),
    /// `Point10 Smoke Room` (`scenes/smoke_room.rs`).
    SmokeRoom(SmokeRoom),
    /// `Point10 Latent Backrooms` (`scenes/latent_backrooms.rs`).
    LatentBackrooms(LatentBackrooms),
    /// `Point10 Sun` (`scenes/sun.rs`).
    Sun(Sun),
    /// `Point10 Earth` (`scenes/earth.rs`).
    Earth(Earth),
}

impl Point10SceneKind {
    /// Whether `name` is one of the four raymarched volume scenes.
    ///
    /// Their instances allocate ~432 MB of 3D textures, so Gallery thumbnails
    /// must not keep them alive per tile. Name-based so the preview path can
    /// classify without building a scene.
    pub fn is_volumetric_name(name: &str) -> bool {
        matches!(
            name,
            "Point10 DVD Room"
                | "Point10 Rolling Fire"
                | "Point10 Ball Of Fire"
                | "Point10 Smoke Room"
        )
    }

    /// Build a fresh scene for a registered wrapper name.
    ///
    /// `None` when `name` is not a ported scene — the compositor then skips the
    /// module exactly like an unregistered 2D module.
    pub fn create(name: &str) -> Option<Self> {
        Some(match name {
            "Point10 Chromatic Line" => Self::ChromaticLine(ChromaticLine::new()),
            "Point10 Chromatic Wave" => Self::ChromaticWave(ChromaticWave::new()),
            "Point10 Particle Field" => Self::ParticleField(ParticleField::new()),
            "Point10 Spiral Particles" => Self::SpiralParticles(SpiralParticles::new()),
            "Point10 Waveform" => Self::Waveform(Waveform::new()),
            "Point10 Square Tunnel" => Self::SquareTunnel(SquareTunnel::new()),
            "Point10 Light Tunnel" => Self::LightTunnel(LightTunnel::new()),
            "Point10 Wormhole" => Self::Wormhole(Wormhole::new()),
            "Point10 It Is Coming For You" => Self::ItIsComingForYou(ItIsComingForYou::new()),
            "Point10 Water Ripple" => Self::WaterRipple(WaterRipple::new()),
            "Point10 Glass Lens" => Self::GlassLens(GlassLens::new()),
            "Point10 Mycelium" => Self::Mycelium(Mycelium::new()),
            "Point10 Around Plus" => Self::AroundPlus(AroundPlus::new()),
            "Point10 Ultra HDR" => Self::UltraHdr(UltraHdr::new()),
            "Point10 Mirror Cube" => Self::MirrorCube(MirrorCube::new()),
            "Point10 Color Cube" => Self::ColorCube(ColorCube::new()),
            "Point10 Boxy" => Self::Boxy(Boxy::new()),
            "Point10 Sound Cube" => Self::SoundCube(SoundCube::new()),
            "Point10 Hollow Cube" => Self::HollowCube(HollowCube::new()),
            "Point10 Attractor Particles" => Self::AttractorParticles(AttractorParticles::new()),
            "Point10 DVD Room" => Self::DvdRoom(DvdRoom::new()),
            "Point10 Rolling Fire" => Self::RollingFire(RollingFire::new()),
            "Point10 Ball Of Fire" => Self::BallOfFire(BallOfFire::new()),
            "Point10 Smoke Room" => Self::SmokeRoom(SmokeRoom::new()),
            "Point10 Latent Backrooms" => Self::LatentBackrooms(LatentBackrooms::new()),
            "Point10 Sun" => Self::Sun(Sun::new()),
            "Point10 Earth" => Self::Earth(Earth::new()),
            _ => return None,
        })
    }

    /// Push the resolved modV props into the scene.
    ///
    /// The getter is the engine's prop bag (`modules.active[id].props`): each
    /// schema entry falls back to its default and clamps to its range, exactly
    /// like `sceneProps.js: readSceneProps` does once per frame
    /// (`adapter.js:797-800`).
    pub fn apply_props(&mut self, get: &mut dyn FnMut(&str) -> Option<f32>) {
        match self {
            Self::ChromaticLine(scene) => scene.apply_props(|key| get(key)),
            Self::BallOfFire(scene) => scene.apply_props(|key| get(key)),
            Self::RollingFire(scene) => scene.apply_props(|key| get(key)),
            Self::SmokeRoom(scene) => scene.apply_props(|key| get(key)),
            Self::LatentBackrooms(scene) => scene.apply_props(|key| get(key)),
            Self::Sun(scene) => scene.apply_props(|key| get(key)),
            Self::Earth(scene) => scene.apply_props(|key| get(key)),
            Self::HollowCube(scene) => scene.apply_props(|key| get(key)),
            Self::AttractorParticles(scene) => scene.apply_props(|key| get(key)),
            Self::DvdRoom(scene) => scene.apply_props(|key| get(key)),
            Self::UltraHdr(scene) => scene.apply_props(|key| get(key)),
            Self::MirrorCube(scene) => scene.apply_props(|key| get(key)),
            Self::ColorCube(scene) => scene.apply_props(|key| get(key)),
            Self::Boxy(scene) => scene.apply_props(|key| get(key)),
            Self::SoundCube(scene) => scene.apply_props(|key| get(key)),
            Self::WaterRipple(scene) => scene.apply_props(|key| get(key)),
            Self::GlassLens(scene) => scene.apply_props(|key| get(key)),
            Self::Mycelium(scene) => scene.apply_props(|key| get(key)),
            Self::AroundPlus(scene) => scene.apply_props(|key| get(key)),
            Self::SquareTunnel(scene) => scene.apply_props(|key| get(key)),
            Self::LightTunnel(scene) => scene.apply_props(|key| get(key)),
            Self::Wormhole(scene) => scene.apply_props(|key| get(key)),
            Self::ItIsComingForYou(scene) => scene.apply_props(|key| get(key)),
            Self::ChromaticWave(scene) => scene.apply_props(|key| get(key)),
            Self::ParticleField(scene) => scene.apply_props(|key| get(key)),
            Self::SpiralParticles(scene) => scene.apply_props(|key| get(key)),
            Self::Waveform(scene) => scene.apply_props(|key| get(key)),
        }
    }
}

/// Every trait method delegates to the concrete scene.
impl Point10Scene for Point10SceneKind {
    fn name(&self) -> &'static str {
        match self {
            Self::ChromaticLine(scene) => scene.name(),
            Self::BallOfFire(scene) => scene.name(),
            Self::RollingFire(scene) => scene.name(),
            Self::SmokeRoom(scene) => scene.name(),
            Self::LatentBackrooms(scene) => scene.name(),
            Self::Sun(scene) => scene.name(),
            Self::Earth(scene) => scene.name(),
            Self::HollowCube(scene) => scene.name(),
            Self::AttractorParticles(scene) => scene.name(),
            Self::DvdRoom(scene) => scene.name(),
            Self::UltraHdr(scene) => scene.name(),
            Self::MirrorCube(scene) => scene.name(),
            Self::ColorCube(scene) => scene.name(),
            Self::Boxy(scene) => scene.name(),
            Self::SoundCube(scene) => scene.name(),
            Self::WaterRipple(scene) => scene.name(),
            Self::GlassLens(scene) => scene.name(),
            Self::Mycelium(scene) => scene.name(),
            Self::AroundPlus(scene) => scene.name(),
            Self::SquareTunnel(scene) => scene.name(),
            Self::LightTunnel(scene) => scene.name(),
            Self::Wormhole(scene) => scene.name(),
            Self::ItIsComingForYou(scene) => scene.name(),
            Self::ChromaticWave(scene) => scene.name(),
            Self::ParticleField(scene) => scene.name(),
            Self::SpiralParticles(scene) => scene.name(),
            Self::Waveform(scene) => scene.name(),
        }
    }

    fn init(&mut self, ctx: &mut modv_renderers::point10::SceneContext) {
        match self {
            Self::ChromaticLine(scene) => scene.init(ctx),
            Self::BallOfFire(scene) => scene.init(ctx),
            Self::RollingFire(scene) => scene.init(ctx),
            Self::SmokeRoom(scene) => scene.init(ctx),
            Self::LatentBackrooms(scene) => scene.init(ctx),
            Self::Sun(scene) => scene.init(ctx),
            Self::Earth(scene) => scene.init(ctx),
            Self::HollowCube(scene) => scene.init(ctx),
            Self::AttractorParticles(scene) => scene.init(ctx),
            Self::DvdRoom(scene) => scene.init(ctx),
            Self::UltraHdr(scene) => scene.init(ctx),
            Self::MirrorCube(scene) => scene.init(ctx),
            Self::ColorCube(scene) => scene.init(ctx),
            Self::Boxy(scene) => scene.init(ctx),
            Self::SoundCube(scene) => scene.init(ctx),
            Self::WaterRipple(scene) => scene.init(ctx),
            Self::GlassLens(scene) => scene.init(ctx),
            Self::Mycelium(scene) => scene.init(ctx),
            Self::AroundPlus(scene) => scene.init(ctx),
            Self::SquareTunnel(scene) => scene.init(ctx),
            Self::LightTunnel(scene) => scene.init(ctx),
            Self::Wormhole(scene) => scene.init(ctx),
            Self::ItIsComingForYou(scene) => scene.init(ctx),
            Self::ChromaticWave(scene) => scene.init(ctx),
            Self::ParticleField(scene) => scene.init(ctx),
            Self::SpiralParticles(scene) => scene.init(ctx),
            Self::Waveform(scene) => scene.init(ctx),
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        match self {
            Self::ChromaticLine(scene) => scene.resize(width, height),
            Self::BallOfFire(scene) => scene.resize(width, height),
            Self::RollingFire(scene) => scene.resize(width, height),
            Self::SmokeRoom(scene) => scene.resize(width, height),
            Self::LatentBackrooms(scene) => scene.resize(width, height),
            Self::Sun(scene) => scene.resize(width, height),
            Self::Earth(scene) => scene.resize(width, height),
            Self::HollowCube(scene) => scene.resize(width, height),
            Self::AttractorParticles(scene) => scene.resize(width, height),
            Self::DvdRoom(scene) => scene.resize(width, height),
            Self::UltraHdr(scene) => scene.resize(width, height),
            Self::MirrorCube(scene) => scene.resize(width, height),
            Self::ColorCube(scene) => scene.resize(width, height),
            Self::Boxy(scene) => scene.resize(width, height),
            Self::SoundCube(scene) => scene.resize(width, height),
            Self::WaterRipple(scene) => scene.resize(width, height),
            Self::GlassLens(scene) => scene.resize(width, height),
            Self::Mycelium(scene) => scene.resize(width, height),
            Self::AroundPlus(scene) => scene.resize(width, height),
            Self::SquareTunnel(scene) => scene.resize(width, height),
            Self::LightTunnel(scene) => scene.resize(width, height),
            Self::Wormhole(scene) => scene.resize(width, height),
            Self::ItIsComingForYou(scene) => scene.resize(width, height),
            Self::ChromaticWave(scene) => scene.resize(width, height),
            Self::ParticleField(scene) => scene.resize(width, height),
            Self::SpiralParticles(scene) => scene.resize(width, height),
            Self::Waveform(scene) => scene.resize(width, height),
        }
    }

    fn update(&mut self, dt: f32, audio: &modv_renderers::point10::audio_bridge::AudioBridge) {
        match self {
            Self::ChromaticLine(scene) => scene.update(dt, audio),
            Self::BallOfFire(scene) => scene.update(dt, audio),
            Self::RollingFire(scene) => scene.update(dt, audio),
            Self::SmokeRoom(scene) => scene.update(dt, audio),
            Self::LatentBackrooms(scene) => scene.update(dt, audio),
            Self::Sun(scene) => scene.update(dt, audio),
            Self::Earth(scene) => scene.update(dt, audio),
            Self::HollowCube(scene) => scene.update(dt, audio),
            Self::AttractorParticles(scene) => scene.update(dt, audio),
            Self::DvdRoom(scene) => scene.update(dt, audio),
            Self::UltraHdr(scene) => scene.update(dt, audio),
            Self::MirrorCube(scene) => scene.update(dt, audio),
            Self::ColorCube(scene) => scene.update(dt, audio),
            Self::Boxy(scene) => scene.update(dt, audio),
            Self::SoundCube(scene) => scene.update(dt, audio),
            Self::WaterRipple(scene) => scene.update(dt, audio),
            Self::GlassLens(scene) => scene.update(dt, audio),
            Self::Mycelium(scene) => scene.update(dt, audio),
            Self::AroundPlus(scene) => scene.update(dt, audio),
            Self::SquareTunnel(scene) => scene.update(dt, audio),
            Self::LightTunnel(scene) => scene.update(dt, audio),
            Self::Wormhole(scene) => scene.update(dt, audio),
            Self::ItIsComingForYou(scene) => scene.update(dt, audio),
            Self::ChromaticWave(scene) => scene.update(dt, audio),
            Self::ParticleField(scene) => scene.update(dt, audio),
            Self::SpiralParticles(scene) => scene.update(dt, audio),
            Self::Waveform(scene) => scene.update(dt, audio),
        }
    }

    fn render(
        &mut self,
        encoder: &mut modv_render::wgpu::CommandEncoder,
        target: &modv_render::wgpu::TextureView,
    ) {
        match self {
            Self::ChromaticLine(scene) => scene.render(encoder, target),
            Self::BallOfFire(scene) => scene.render(encoder, target),
            Self::RollingFire(scene) => scene.render(encoder, target),
            Self::SmokeRoom(scene) => scene.render(encoder, target),
            Self::LatentBackrooms(scene) => scene.render(encoder, target),
            Self::Sun(scene) => scene.render(encoder, target),
            Self::Earth(scene) => scene.render(encoder, target),
            Self::HollowCube(scene) => scene.render(encoder, target),
            Self::AttractorParticles(scene) => scene.render(encoder, target),
            Self::DvdRoom(scene) => scene.render(encoder, target),
            Self::UltraHdr(scene) => scene.render(encoder, target),
            Self::MirrorCube(scene) => scene.render(encoder, target),
            Self::ColorCube(scene) => scene.render(encoder, target),
            Self::Boxy(scene) => scene.render(encoder, target),
            Self::SoundCube(scene) => scene.render(encoder, target),
            Self::WaterRipple(scene) => scene.render(encoder, target),
            Self::GlassLens(scene) => scene.render(encoder, target),
            Self::Mycelium(scene) => scene.render(encoder, target),
            Self::AroundPlus(scene) => scene.render(encoder, target),
            Self::SquareTunnel(scene) => scene.render(encoder, target),
            Self::LightTunnel(scene) => scene.render(encoder, target),
            Self::Wormhole(scene) => scene.render(encoder, target),
            Self::ItIsComingForYou(scene) => scene.render(encoder, target),
            Self::ChromaticWave(scene) => scene.render(encoder, target),
            Self::ParticleField(scene) => scene.render(encoder, target),
            Self::SpiralParticles(scene) => scene.render(encoder, target),
            Self::Waveform(scene) => scene.render(encoder, target),
        }
    }

    fn dispose(&mut self) {
        match self {
            Self::ChromaticLine(scene) => scene.dispose(),
            Self::BallOfFire(scene) => scene.dispose(),
            Self::RollingFire(scene) => scene.dispose(),
            Self::SmokeRoom(scene) => scene.dispose(),
            Self::LatentBackrooms(scene) => scene.dispose(),
            Self::Sun(scene) => scene.dispose(),
            Self::Earth(scene) => scene.dispose(),
            Self::HollowCube(scene) => scene.dispose(),
            Self::AttractorParticles(scene) => scene.dispose(),
            Self::DvdRoom(scene) => scene.dispose(),
            Self::UltraHdr(scene) => scene.dispose(),
            Self::MirrorCube(scene) => scene.dispose(),
            Self::ColorCube(scene) => scene.dispose(),
            Self::Boxy(scene) => scene.dispose(),
            Self::SoundCube(scene) => scene.dispose(),
            Self::WaterRipple(scene) => scene.dispose(),
            Self::GlassLens(scene) => scene.dispose(),
            Self::Mycelium(scene) => scene.dispose(),
            Self::AroundPlus(scene) => scene.dispose(),
            Self::SquareTunnel(scene) => scene.dispose(),
            Self::LightTunnel(scene) => scene.dispose(),
            Self::Wormhole(scene) => scene.dispose(),
            Self::ItIsComingForYou(scene) => scene.dispose(),
            Self::ChromaticWave(scene) => scene.dispose(),
            Self::ParticleField(scene) => scene.dispose(),
            Self::SpiralParticles(scene) => scene.dispose(),
            Self::Waveform(scene) => scene.dispose(),
        }
    }

    fn props(&self) -> Vec<SceneProp> {
        match self {
            Self::ChromaticLine(scene) => scene.props(),
            Self::BallOfFire(scene) => scene.props(),
            Self::RollingFire(scene) => scene.props(),
            Self::SmokeRoom(scene) => scene.props(),
            Self::LatentBackrooms(scene) => scene.props(),
            Self::Sun(scene) => scene.props(),
            Self::Earth(scene) => scene.props(),
            Self::HollowCube(scene) => scene.props(),
            Self::AttractorParticles(scene) => scene.props(),
            Self::DvdRoom(scene) => scene.props(),
            Self::UltraHdr(scene) => scene.props(),
            Self::MirrorCube(scene) => scene.props(),
            Self::ColorCube(scene) => scene.props(),
            Self::Boxy(scene) => scene.props(),
            Self::SoundCube(scene) => scene.props(),
            Self::WaterRipple(scene) => scene.props(),
            Self::GlassLens(scene) => scene.props(),
            Self::Mycelium(scene) => scene.props(),
            Self::AroundPlus(scene) => scene.props(),
            Self::SquareTunnel(scene) => scene.props(),
            Self::LightTunnel(scene) => scene.props(),
            Self::Wormhole(scene) => scene.props(),
            Self::ItIsComingForYou(scene) => scene.props(),
            Self::ChromaticWave(scene) => scene.props(),
            Self::ParticleField(scene) => scene.props(),
            Self::SpiralParticles(scene) => scene.props(),
            Self::Waveform(scene) => scene.props(),
        }
    }

    fn meta(&self) -> SceneMeta {
        match self {
            Self::ChromaticLine(scene) => scene.meta(),
            Self::BallOfFire(scene) => scene.meta(),
            Self::RollingFire(scene) => scene.meta(),
            Self::SmokeRoom(scene) => scene.meta(),
            Self::LatentBackrooms(scene) => scene.meta(),
            Self::Sun(scene) => scene.meta(),
            Self::Earth(scene) => scene.meta(),
            Self::HollowCube(scene) => scene.meta(),
            Self::AttractorParticles(scene) => scene.meta(),
            Self::DvdRoom(scene) => scene.meta(),
            Self::UltraHdr(scene) => scene.meta(),
            Self::MirrorCube(scene) => scene.meta(),
            Self::ColorCube(scene) => scene.meta(),
            Self::Boxy(scene) => scene.meta(),
            Self::SoundCube(scene) => scene.meta(),
            Self::WaterRipple(scene) => scene.meta(),
            Self::GlassLens(scene) => scene.meta(),
            Self::Mycelium(scene) => scene.meta(),
            Self::AroundPlus(scene) => scene.meta(),
            Self::SquareTunnel(scene) => scene.meta(),
            Self::LightTunnel(scene) => scene.meta(),
            Self::Wormhole(scene) => scene.meta(),
            Self::ItIsComingForYou(scene) => scene.meta(),
            Self::ChromaticWave(scene) => scene.meta(),
            Self::ParticleField(scene) => scene.meta(),
            Self::SpiralParticles(scene) => scene.meta(),
            Self::Waveform(scene) => scene.meta(),
        }
    }
}

impl std::fmt::Debug for Point10SceneKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Point10SceneKind")
            .field(&self.name())
            .finish()
    }
}

/// The JS 2D props whose descriptor sets `strict: true` (clamp to
/// `[min, max]`). Exhaustive against `renderers/2d/modules/*.js`.
const TWO_D_PROP_FLAGS: &[(&str, &str, bool, bool)] = &[
    // (module, prop, strict, abs)
    ("Ball", "amount", true, false),              // Ball.js:21
    ("Ball", "size", false, true),                // Ball.js:46
    ("Ball", "intensity", false, true),           // Ball.js:56
    ("Line", "linesToShow", true, false),         // Line.js:14
    ("Line", "spacing", false, true),             // Line.js:22
    ("Line", "lineWidth", false, true),           // Line.js:30
    ("Line", "speed", false, true),               // Line.js:54
    ("Concentrics", "strokeWeight", true, false), // Concentrics.js:50
    ("Polygon", "strokeWeight", true, false),     // Polygon.js:36
    ("Text", "strokeSize", false, true),          // Text.js:55
    ("Waveform", "strokeWeight", false, true),    // Waveform.js:19
    ("X Drips", "maxDripHeight", false, true),    // X-Drips.js:28
    ("Video", "start", true, false),              // Video start trim clamps to 0..1
    ("Video", "finish", true, false),             // Video finish trim clamps to 0..1
];

/// The JS 2D modules whose `meta` sets `previewWithOutput: true`.
const TWO_D_PREVIEW_WITH_OUTPUT: &[&str] = &[
    "Grid Stretch", // GridStretch.js:7
    "Pixelate",     // Pixelate.js:9
];

/// All built-in registries, with the metadata the store and the compositor
/// need.
pub struct ModuleRegistry {
    shader: Vec<ShaderModule>,
    two_d: Vec<TwoDDefinition>,
    point10: Vec<Point10Definition>,
    isf: Vec<IsfDefinition>,
    milkdrop: Vec<MilkdropDefinition>,
}

impl ModuleRegistry {
    /// The 36 built-in shader modules, the 16 2D modules (15 ported plus the
    /// VmodV-only `Video`) and the ported Point10 scenes, with no ISF corpus.
    ///
    /// Used by unit tests and the headless benchmark, which must not depend on
    /// files outside the process. The engine path calls [`Self::build`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn builtin() -> Self {
        Self::build(None)
    }

    /// Build the registry, scanning `isf_dir` for the vendored ISF corpus.
    ///
    /// `None` (or a missing directory) registers no ISF module. Every `*.fs`
    /// is parsed with its optional `*.vs` sidecar; unreadable or unparsable
    /// files are skipped with a one-line warning and counted in the summary.
    pub fn build(isf_dir: Option<&Path>) -> Self {
        Self::build_with_extra(isf_dir, &[])
    }

    /// Build the registry with the corpus plus user ISF directories.
    ///
    /// The corpus wins on a name collision; user folders are scanned
    /// recursively (subfolders allowed), sorted by path, and duplicate stems
    /// across folders keep the first sorted path. Every ignored duplicate is
    /// reported once at startup. This is the VmodV app's third-party ISF path
    /// (the media watcher calls it again when a `.fs` file changes).
    pub fn build_with_extra(isf_dir: Option<&Path>, extra_dirs: &[PathBuf]) -> Self {
        Self::build_with_milkdrop(isf_dir, extra_dirs, &[])
    }

    /// Build the registry with the ISF corpus plus user MilkDrop directories.
    ///
    /// `milkdrop_dirs` are scanned recursively by
    /// [`collect_milkdrop_definitions`]: case-insensitive `.mlk` files, sorted
    /// paths, hidden/temporary and symlink entries skipped, and the first
    /// sorted path winning a duplicate file stem. The walk shares the user-ISF
    /// caps [`USER_ISF_MAX_DEPTH`] and [`USER_ISF_MAX_ENTRIES`].
    pub fn build_with_milkdrop(
        isf_dir: Option<&Path>,
        extra_isf_dirs: &[PathBuf],
        milkdrop_dirs: &[PathBuf],
    ) -> Self {
        let shader = all_shader_modules();
        // `all_modules()` builds one default instance per module (all cheap,
        // no GPU work) so the descriptors can be copied out once.
        let two_d = all_two_d_modules()
            .into_iter()
            .map(|module| TwoDDefinition {
                meta: module.meta(),
                props: module.props(),
            })
            .collect();
        // Point10 scenes report their own meta/schema; no GPU work happens on
        // construction (pipelines and textures are built in `init`).
        let point10 = all_point10_scenes()
            .iter()
            .map(|scene| Point10Definition {
                meta: scene.meta(),
                props: scene.props(),
            })
            .collect();
        let (isf, duplicates) = collect_isf_definitions(isf_dir, extra_isf_dirs);
        for duplicate in &duplicates {
            let ignored: Vec<String> = duplicate
                .ignored
                .iter()
                .map(|path| path.display().to_string())
                .collect();
            eprintln!(
                "engine: WARN duplicate ISF module name `{}`: keeping {}; ignoring {}",
                duplicate.name,
                duplicate.kept.display(),
                ignored.join(", ")
            );
        }
        let milkdrop = collect_milkdrop_definitions(milkdrop_dirs);
        Self {
            shader,
            two_d,
            point10,
            isf,
            milkdrop,
        }
    }

    /// The shader modules, in Gallery registration order.
    pub fn shader_modules(&self) -> &[ShaderModule] {
        &self.shader
    }

    /// The 2D module definitions, in registration order.
    pub fn two_d_definitions(&self) -> &[TwoDDefinition] {
        &self.two_d
    }

    /// The Point10 scene definitions, in `all_scenes()` order.
    pub fn point10_definitions(&self) -> &[Point10Definition] {
        &self.point10
    }

    /// The ISF corpus definitions, in file-name order.
    pub fn isf_definitions(&self) -> &[IsfDefinition] {
        &self.isf
    }

    /// Look up a shader module by its registration name (`$moduleName`).
    pub fn shader_module(&self, name: &str) -> Option<&ShaderModule> {
        self.shader.iter().find(|module| module.name == name)
    }

    /// Look up a 2D module definition by its registration name.
    pub fn two_d_definition(&self, name: &str) -> Option<&TwoDDefinition> {
        self.two_d.iter().find(|module| module.meta.name == name)
    }

    /// Look up a Point10 scene definition by its wrapper module name
    /// (`"Point10 Chromatic Wave"`).
    pub fn point10_definition(&self, name: &str) -> Option<&Point10Definition> {
        self.point10.iter().find(|scene| scene.meta.name == name)
    }

    /// Look up an ISF definition by the shader's file stem (`"Bloom"`).
    pub fn isf_definition(&self, name: &str) -> Option<&IsfDefinition> {
        self.isf.iter().find(|definition| definition.name == name)
    }

    /// The MilkDrop preset definitions, in file-name order.
    pub fn milkdrop_definitions(&self) -> &[MilkdropDefinition] {
        &self.milkdrop
    }

    /// Look up a MilkDrop definition by the preset's file stem.
    pub fn milkdrop_definition(&self, name: &str) -> Option<&MilkdropDefinition> {
        self.milkdrop
            .iter()
            .find(|definition| definition.name == name)
    }

    /// Which renderer owns `name`, or `None` when it is not registered.
    pub fn kind(&self, name: &str) -> Option<ModuleKind> {
        if self
            .shader_modules()
            .iter()
            .any(|module| module.name == name)
        {
            Some(ModuleKind::Shader)
        } else if self
            .two_d_definitions()
            .iter()
            .any(|module| module.meta.name == name)
        {
            Some(ModuleKind::TwoD)
        } else if self
            .point10_definitions()
            .iter()
            .any(|scene| scene.meta.name == name)
        {
            Some(ModuleKind::Point10)
        } else if self
            .isf_definitions()
            .iter()
            .any(|definition| definition.name == name)
        {
            Some(ModuleKind::Isf)
        } else if self
            .milkdrop_definitions()
            .iter()
            .any(|definition| definition.name == name)
        {
            Some(ModuleKind::Milkdrop)
        } else {
            None
        }
    }

    /// The registry-authored `meta` fields of a known module, or `None` when
    /// the name is not registered.
    ///
    /// This is the JS spread of `moduleDefinition.meta`
    /// (`modules.js:247-249`): `name`, `author`, `version`, `type`, plus
    /// `category`, `audioFeatures` and `previewWithOutput` when declared. The
    /// JS defaults (`alpha`, `enabled`, `compositeOperation`), the caller's
    /// `moduleMeta` and `isGallery` are applied by the store afterwards.
    pub fn meta_fields(&self, name: &str) -> Option<Map<String, Value>> {
        match self.kind(name)? {
            ModuleKind::Shader => {
                let definition = self.shader_module(name)?;
                let mut meta = Map::new();
                meta.insert("name".to_string(), json!(definition.name));
                meta.insert("author".to_string(), json!(definition.meta_author));
                meta.insert("version".to_string(), json!(definition.meta_version));
                meta.insert("type".to_string(), json!("shader"));
                if let Some(category) = definition.category {
                    meta.insert("category".to_string(), json!(category));
                }
                if !definition.audio_features.is_empty() {
                    meta.insert(
                        "audioFeatures".to_string(),
                        json!(definition.audio_features),
                    );
                }
                meta.insert(
                    "previewWithOutput".to_string(),
                    json!(definition.preview_with_output),
                );
                Some(meta)
            }
            ModuleKind::TwoD => {
                let definition = self.two_d_definition(name)?;
                let mut meta = Map::new();
                meta.insert("name".to_string(), json!(definition.meta.name));
                // The JS `meta` object only carries `author`/`version` when the
                // module declares them (`Text.js` declares neither); a spread
                // therefore leaves the keys absent rather than empty.
                if !definition.meta.author.is_empty() {
                    meta.insert("author".to_string(), json!(definition.meta.author));
                }
                if !definition.meta.version.is_empty() {
                    meta.insert("version".to_string(), json!(definition.meta.version));
                }
                meta.insert("type".to_string(), json!("2d"));
                if let Some(category) = definition.meta.category {
                    meta.insert("category".to_string(), json!(category));
                }
                if !definition.meta.audio_features.is_empty() {
                    meta.insert(
                        "audioFeatures".to_string(),
                        json!(definition.meta.audio_features),
                    );
                }
                if TWO_D_PREVIEW_WITH_OUTPUT.contains(&definition.meta.name) {
                    meta.insert("previewWithOutput".to_string(), json!(true));
                }
                Some(meta)
            }
            ModuleKind::Point10 => {
                let definition = self.point10_definition(name)?;
                // `createPoint10Module` always spreads all five meta fields
                // (`adapter.js:857-866`), so unlike the 2D path none of them is
                // optional here.
                let mut meta = Map::new();
                meta.insert("name".to_string(), json!(definition.meta.name));
                meta.insert("author".to_string(), json!(definition.meta.author));
                meta.insert("version".to_string(), json!(definition.meta.version));
                meta.insert("type".to_string(), json!(definition.meta.module_type));
                if !definition.meta.category.is_empty() {
                    meta.insert("category".to_string(), json!(definition.meta.category));
                }
                if !definition.meta.audio_features.is_empty() {
                    meta.insert(
                        "audioFeatures".to_string(),
                        json!(definition.meta.audio_features),
                    );
                }
                Some(meta)
            }
            ModuleKind::Isf => {
                let definition = self.isf_definition(name)?;
                // `setupModule` (`renderers/isf/index.js:184-192`) overwrites
                // the registration-time `author`/`version` with `CREDIT`/`VSN`
                // and adds `description`/`category`/`isfVersion`. A missing
                // header field stays absent, exactly like the JS `undefined`
                // keys that `JSON.stringify` drops.
                let mut meta = Map::new();
                meta.insert("name".to_string(), json!(definition.name));
                if let Some(author) = &definition.meta.author {
                    meta.insert("author".to_string(), json!(author));
                }
                if let Some(version) = &definition.meta.version {
                    meta.insert("version".to_string(), json!(version));
                }
                meta.insert("type".to_string(), json!("isf"));
                if let Some(description) = &definition.meta.description {
                    meta.insert("description".to_string(), json!(description));
                }
                if let Some(category) = &definition.meta.category {
                    meta.insert("category".to_string(), json!(category));
                }
                meta.insert("isfVersion".to_string(), json!(definition.meta.isf_version));
                if definition.meta.preview_with_output {
                    meta.insert("previewWithOutput".to_string(), json!(true));
                }
                Some(meta)
            }
            ModuleKind::Milkdrop => {
                let definition = self.milkdrop_definition(name)?;
                let mut meta = Map::new();
                meta.insert("name".to_string(), json!(definition.name));
                meta.insert("type".to_string(), json!("milkdrop"));
                meta.insert("category".to_string(), json!(definition.meta.category));
                meta.insert("author".to_string(), json!(definition.meta.author));
                meta.insert("version".to_string(), json!(definition.meta.version));
                meta.insert(
                    "description".to_string(),
                    json!(definition.meta.description),
                );
                meta.insert(
                    "previewWithOutput".to_string(),
                    json!(definition.meta.preview_with_output),
                );
                Some(meta)
            }
        }
    }

    /// The coercion schema for one of a module's props.
    ///
    /// Shader props never set `strict` (`renderers/shader/modules/*.js`), so
    /// their schemas keep `strict: false`; 2D props take `type`/`min`/`max`
    /// from [`TwoDProp`] and `strict`/`abs` from [`TWO_D_PROP_FLAGS`]; ISF props
    /// take every field from the synthesised [`IsfProp`] and are never strict
    /// (see [`schema_for_isf`]); Point10 props take both from the scene's
    /// [`SceneProp`] schema.
    pub fn prop_schema(&self, module_name: &str, prop: &str) -> Option<PropSchema> {
        if let Some(module) = self.shader_module(module_name) {
            let declared = module.props.iter().find(|declared| declared.name == prop)?;
            return Some(schema_for_shader(declared));
        }

        if let Some(definition) = self.two_d_definition(module_name) {
            let declared = definition
                .props
                .iter()
                .find(|declared| declared.name == prop)?;
            return Some(schema_for_two_d(definition.meta.name, declared));
        }

        if let Some(definition) = self.isf_definition(module_name) {
            // A shader that declares its own `quality` input keeps it; only
            // modules without a declared prop get the synthetic one.
            if let Some(prop) = definition.prop(prop) {
                return Some(schema_for_isf(prop));
            }
            return (prop == QUALITY_PROP).then(quality_schema);
        }

        if prop == QUALITY_PROP {
            // Only ISF/Point10 report the synthetic prop; anything else
            // returns None like any other unknown prop name.
            return self
                .point10_definition(module_name)
                .is_some()
                .then(quality_schema);
        }
        let definition = self.point10_definition(module_name)?;
        let declared = definition
            .props
            .iter()
            .find(|declared| declared.name == prop)?;
        Some(schema_for_point10(declared))
    }

    /// The default prop values for a module, as the `props` object of a new
    /// active module (`getPropDefault`, `utils/get-prop-default.js`).
    /// The `state.registered` map the UI mirror needs in engine mode.
    ///
    /// The legacy worker registers every module definition locally
    /// (`modules.js` `ADD_REGISTERED_MODULE`), so the Gallery and the
    /// drag/preview surfaces can read `registered[name].meta`. In engine mode
    /// no worker exists; `state/sync` ships this map once per connection and
    /// the renderer replays it into the mirror. Names resolve with the same
    /// precedence as [`ModuleRegistry::kind`] (shader → 2D → Point10 → ISF), so
    /// a corpus shader that shares a built-in name does not shadow it.
    ///
    /// Only `meta` and the prop descriptors are included — the JS definition
    /// object also carries lifecycle functions, which the mirror never calls.
    pub fn registered_modules(&self) -> Value {
        let mut names: Vec<&str> = Vec::new();
        for module in &self.shader {
            names.push(module.name);
        }
        for definition in &self.two_d {
            names.push(definition.meta.name);
        }
        for definition in &self.point10 {
            names.push(definition.meta.name);
        }
        for definition in &self.isf {
            names.push(&definition.name);
        }
        for definition in &self.milkdrop {
            names.push(&definition.name);
        }
        names.sort_unstable();
        names.dedup();

        let mut registered = Map::new();
        for name in names {
            let Some(meta) = self.meta_fields(name) else {
                continue;
            };
            let props = self
                .prop_names(name)
                .into_iter()
                .filter_map(|prop| {
                    let schema = self.prop_schema(name, &prop)?;
                    Some((prop, schema_json(&schema)))
                })
                .collect::<Map<String, Value>>();
            registered.insert(name.to_string(), json!({ "meta": meta, "props": props }));
        }
        Value::Object(registered)
    }

    /// Prop names declared by a registered module, in declaration order.
    fn prop_names(&self, module_name: &str) -> Vec<String> {
        if let Some(module) = self.shader_module(module_name) {
            return module
                .props
                .iter()
                .map(|prop| prop.name.to_string())
                .collect();
        }
        if let Some(definition) = self.two_d_definition(module_name) {
            return definition
                .props
                .iter()
                .map(|prop| prop.name.to_string())
                .collect();
        }
        if let Some(definition) = self.point10_definition(module_name) {
            return definition
                .props
                .iter()
                .map(|prop| prop.name.to_string())
                .chain(std::iter::once(QUALITY_PROP.to_string()))
                .collect();
        }
        if let Some(definition) = self.isf_definition(module_name) {
            return definition
                .props
                .iter()
                .map(|(name, _)| name.clone())
                .chain(std::iter::once(QUALITY_PROP.to_string()))
                .collect();
        }
        if let Some(_definition) = self.milkdrop_definition(module_name) {
            // MilkDrop presets expose no format-specific props yet.
            return Vec::new();
        }
        Vec::new()
    }

    pub fn default_props(&self, module_name: &str) -> Option<Value> {
        if let Some(module) = self.shader_module(module_name) {
            let mut props = Map::new();
            for prop in &module.props {
                props.insert(prop.name.to_string(), default_shader_prop_value(prop));
            }
            return Some(Value::Object(props));
        }

        if let Some(definition) = self.two_d_definition(module_name) {
            return Some(two_d_default_props(&definition.props));
        }

        if let Some(definition) = self.isf_definition(module_name) {
            let mut props = Map::new();
            for (name, prop) in &definition.props {
                props.insert(name.clone(), default_isf_prop_value(prop));
            }
            if !props.contains_key(QUALITY_PROP) {
                props.insert(QUALITY_PROP.to_string(), quality_default());
            }
            return Some(Value::Object(props));
        }

        if let Some(_definition) = self.milkdrop_definition(module_name) {
            // No format-specific props yet; caller-style defaults are added by
            // the store/preview builder exactly as they are for ISF modules.
            return Some(Value::Object(Map::new()));
        }

        let definition = self.point10_definition(module_name)?;
        let mut props = Map::new();
        for prop in &definition.props {
            props.insert(prop.name.to_string(), default_scene_prop_value(prop));
        }
        if !props.contains_key(QUALITY_PROP) {
            props.insert(QUALITY_PROP.to_string(), quality_default());
        }
        Some(Value::Object(props))
    }

    /// Build a fresh 2D module instance for `name`.
    ///
    /// `TwoDModule` is not `Clone` and its state is per instance, so the
    /// compositor asks for a new one when a module first draws. `all_modules()`
    /// constructs the 16 default structs (no GPU work) and the match is
    /// returned; activation/first draw is rare enough that this does not
    /// matter.
    pub fn create_two_d(&self, name: &str) -> Option<Box<dyn TwoDModule>> {
        all_two_d_modules()
            .into_iter()
            .find(|module| module.meta().name == name)
    }

    /// Build a fresh Point10 scene for `name`.
    ///
    /// Returns `None` when `name` is not a registered scene, matching
    /// [`ModuleRegistry::create_two_d`]. The concrete wrapper is returned (not
    /// a trait object) so the compositor can call
    /// [`Point10SceneKind::apply_props`]; it also implements [`Point10Scene`],
    /// so [`ModuleRegistry::create_point10`] hands out the trait-object form.
    pub fn create_point10_instance(&self, name: &str) -> Option<Point10SceneKind> {
        self.point10_definition(name)?;
        Point10SceneKind::create(name)
    }

    /// Build a fresh Point10 scene as the render-contract trait object.
    ///
    /// This is the `Box<dyn Point10Scene>` form of
    /// [`ModuleRegistry::create_point10_instance`]. The compositor itself uses
    /// the concrete wrapper because the trait has no prop binder (see the
    /// module docs); this form exists for generic consumers.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn create_point10(&self, name: &str) -> Option<Box<dyn Point10Scene>> {
        self.create_point10_instance(name)
            .map(|scene| Box::new(scene) as Box<dyn Point10Scene>)
    }
}

/// The `dataTypes` key for a shader prop kind.
fn prop_kind_name(kind: PropKind) -> &'static str {
    match kind {
        PropKind::Float => "float",
        PropKind::Int => "int",
        PropKind::Bool => "bool",
        PropKind::Enum => "enum",
        PropKind::Vec2 => "vec2",
        PropKind::Vec3 => "vec3",
        PropKind::Vec4 => "vec4",
        PropKind::Color => "color",
    }
}

/// The synthetic `quality` prop: every ISF module and Point10 scene reports
/// it, regardless of what the header declares. The inspector renders it as
/// a dropdown; the compositor reads it to pick a render target size.
const QUALITY_PROP: &str = "quality";

fn quality_members() -> Vec<modv_engine::props::EnumMember> {
    [
        ("Global (follow the Quality setting)", "global"),
        ("Full (100%)", "full"),
        ("Balanced (75%)", "balanced"),
        ("Performance (50%)", "performance"),
    ]
    .into_iter()
    .map(|(label, value)| modv_engine::props::EnumMember {
        label: label.to_string(),
        value: Value::String(value.to_string()),
    })
    .collect()
}

/// The synthetic Quality prop descriptor shared by ISF and Point10 modules.
fn quality_schema() -> PropSchema {
    PropSchema {
        type_: "enum".to_string(),
        min: None,
        max: None,
        strict: false,
        abs: false,
        default: Some(Value::String("global".to_string())),
        enum_members: Some(quality_members()),
    }
}

/// The Quality prop's default value.
fn quality_default() -> Value {
    Value::String("global".to_string())
}

/// Serialise a [`PropSchema`] the way the JS prop descriptors are shaped.
fn schema_json(schema: &PropSchema) -> Value {
    let mut map = Map::new();
    map.insert("type".to_string(), json!(schema.type_));
    if let Some(default) = &schema.default {
        map.insert("default".to_string(), default.clone());
    }
    if let Some(members) = &schema.enum_members {
        map.insert(
            "options".to_string(),
            Value::Array(
                members
                    .iter()
                    .map(|member| {
                        json!({
                            "label": member.label,
                            "value": member.value,
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(min) = schema.min {
        map.insert("min".to_string(), json!(min));
    }
    if let Some(max) = schema.max {
        map.insert("max".to_string(), json!(max));
    }
    if schema.strict {
        map.insert("strict".to_string(), json!(true));
    }
    if schema.abs {
        map.insert("abs".to_string(), json!(true));
    }
    Value::Object(map)
}

/// Build a [`PropSchema`] from a shader prop declaration.
fn schema_for_shader(prop: &ShaderProp) -> PropSchema {
    PropSchema {
        type_: prop_kind_name(prop.kind).to_string(),
        min: Some(prop.min),
        max: Some(prop.max),
        // JS shader props never set `strict`.
        strict: false,
        abs: false,
        default: Some(default_shader_prop_value(prop)),
        enum_members: None,
    }
}

/// Build a [`PropSchema`] from a 2D prop descriptor.
fn schema_for_two_d(module_name: &str, prop: &TwoDProp) -> PropSchema {
    let (strict, abs) = two_d_prop_flags(module_name, prop.name);
    PropSchema {
        type_: prop.prop_type.to_string(),
        min: prop.min,
        max: prop.max,
        strict,
        abs,
        default: Some(prop.default.clone()),
        enum_members: prop.enum_members.as_ref().map(|members| {
            members
                .iter()
                .map(|member| modv_engine::props::EnumMember {
                    label: member.label.to_string(),
                    value: Value::String(member.value.to_string()),
                })
                .collect()
        }),
    }
}

/// The `(strict, abs)` coercion flags of a 2D prop, from [`TWO_D_PROP_FLAGS`].
fn two_d_prop_flags(module_name: &str, prop: &str) -> (bool, bool) {
    TWO_D_PROP_FLAGS
        .iter()
        .find(|(module, name, ..)| *module == module_name && *name == prop)
        .map(|(_, _, strict, abs)| (*strict, *abs))
        .unwrap_or((false, false))
}

/// Build a [`PropSchema`] from a Point10 scene prop declaration.
///
/// The JS scene schemas always declare `min`/`max` and clamp on read
/// (`sceneProps.js: readSceneProps`), so the engine marks the numeric props
/// `strict`: an out-of-range `modules/updateProp` (a hand-edited preset, an
/// input link) is stored already clamped instead of only being clamped when the
/// scene reads it. Booleans are `type: "bool"` and go through the plain
/// `!= false` read, exactly like `props.useAgc`.
fn schema_for_point10(prop: &SceneProp) -> PropSchema {
    PropSchema {
        type_: scene_prop_kind_name(prop.kind).to_string(),
        min: Some(f64::from(prop.min)),
        max: Some(f64::from(prop.max)),
        strict: prop.kind == ScenePropKind::Float,
        abs: false,
        default: Some(default_scene_prop_value(prop)),
        enum_members: None,
    }
}

/// The `dataTypes` key for a Point10 prop kind.
fn scene_prop_kind_name(kind: ScenePropKind) -> &'static str {
    match kind {
        ScenePropKind::Float => "float",
        ScenePropKind::Bool => "bool",
    }
}

/// The JSON default for a Point10 prop declaration.
///
/// `SceneProp` stores the default as an `f32`; going through the shortest
/// round-trip decimal keeps the JSON readable and identical to the JS literals
/// (`0.05` stays `0.05`, not `0.05000000074505806`). Whole numbers are written
/// as floats (`1.0`) so every float prop has the same JSON number shape as the
/// shader and 2D default paths.
fn default_scene_prop_value(prop: &SceneProp) -> Value {
    match prop.kind {
        ScenePropKind::Bool => json!(prop.default != 0.0),
        ScenePropKind::Float => {
            let text = prop.default.to_string();
            let text = if text.contains(['.', 'e', 'E']) {
                text
            } else {
                format!("{text}.0")
            };
            serde_json::from_str(&text).unwrap_or_else(|_| json!(f64::from(prop.default)))
        }
    }
}

/// The JSON default for a shader prop declaration.
///
/// `ShaderProp` carries one `f64` default (every in-tree prop is a scalar
/// float); vector and colour kinds fan that number out to their lanes so the
/// shape is still a valid value for the type. `bool`/`int`/`enum` are cast to
/// their canonical scalar form, mirroring `dataTypes.js`.
fn default_shader_prop_value(prop: &ShaderProp) -> Value {
    match prop.kind {
        PropKind::Float => json!(prop.default),
        PropKind::Int | PropKind::Enum => json!(prop.default.round() as i64),
        PropKind::Bool => json!(prop.default != 0.0),
        PropKind::Vec2 => json!([prop.default, prop.default]),
        PropKind::Vec3 => json!([prop.default, prop.default, prop.default]),
        PropKind::Vec4 => json!([prop.default, prop.default, prop.default, prop.default]),
        PropKind::Color => json!({
            "r": prop.default,
            "g": prop.default,
            "b": prop.default,
            "a": prop.default,
        }),
    }
}

/// Build a [`PropSchema`] from a synthesised ISF prop.
///
/// `renderers/isf/index.js:210-275` never sets `strict` for ISF inputs, even
/// when the header declares `MIN`/`MAX`, so the schema keeps the bounds as
/// control metadata but does **not** clamp writes. That is the JS behaviour:
/// `modules/updateProp` only clamps `strict` props, and the shader receives
/// out-of-range values unchanged.
fn schema_for_isf(prop: &IsfProp) -> PropSchema {
    PropSchema {
        type_: prop.type_.as_str().to_string(),
        min: prop.min,
        max: prop.max,
        strict: false,
        abs: false,
        default: Some(default_isf_prop_value(prop)),
        enum_members: prop.enum_members.as_ref().map(|members| {
            members
                .iter()
                .map(|member| modv_engine::props::EnumMember {
                    label: if member.label.is_empty() {
                        member.value.to_string()
                    } else {
                        member.label.clone()
                    },
                    value: member.value.clone(),
                })
                .collect()
        }),
    }
}

/// The JSON default for an ISF prop, mirroring `getPropDefault`
/// (`utils/get-prop-default.js:10-19`) and the synthesiser.
///
/// * `float` always has a default (the header `DEFAULT`, else `0.0`).
/// * `bool` always has one (`Boolean(DEFAULT)`).
/// * `enum` takes the value of its `selected` member. The JS returns
///   `undefined` when no member matches the header `DEFAULT`; this port falls
///   back to the first member so a control always has a sensible value. (The
///   runtime still uses the document `DEFAULT` for the uniform, so the
///   rendered result is unchanged.)
/// * `point2D` always has one (the synthesiser substitutes `[0, 0]`).
/// * `color` only has one when the header declares `DEFAULT`.
/// * `image`/`event` have no default in JS; they map to `null`.
fn default_isf_prop_value(prop: &IsfProp) -> Value {
    match prop.type_ {
        IsfPropType::Float => prop.default.clone().unwrap_or_else(|| json!(0.0)),
        IsfPropType::Bool => prop.default.clone().unwrap_or(json!(false)),
        IsfPropType::Enum => prop
            .enum_members
            .as_ref()
            .and_then(|members| {
                members
                    .iter()
                    .find(|member| member.selected)
                    .or_else(|| members.first())
            })
            .map(|member| member.value.clone())
            .unwrap_or(Value::Null),
        IsfPropType::Vec4 | IsfPropType::Vec2 => prop.default.clone().unwrap_or(Value::Null),
        IsfPropType::Texture | IsfPropType::Event => Value::Null,
    }
}

/// How many folders below a user ISF root are scanned (the root is depth 0).
const USER_ISF_MAX_DEPTH: usize = 8;

/// Upper bound on directory entries visited under one user ISF root.
const USER_ISF_MAX_ENTRIES: usize = 4096;

/// Limits for one module-directory walk, shared by the ISF and MilkDrop
/// scanners.
#[derive(Clone, Copy, Debug)]
struct IsfScanLimits {
    recursive: bool,
    max_depth: usize,
    max_entries: usize,
}

impl IsfScanLimits {
    /// The built-in corpus is flat and deterministic at its current size.
    const CORPUS: Self = Self {
        recursive: false,
        max_depth: 0,
        max_entries: usize::MAX,
    };

    /// User folders may be organised in subfolders, with a bound on runaway
    /// trees.
    const USER: Self = Self {
        recursive: true,
        max_depth: USER_ISF_MAX_DEPTH,
        max_entries: USER_ISF_MAX_ENTRIES,
    };
}

/// A module name claimed by more than one user ISF file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DuplicateIsfName {
    name: String,
    kept: PathBuf,
    ignored: Vec<PathBuf>,
}

/// One parsed shader together with the file it came from (used to report which
/// path won a duplicate-name collision).
#[derive(Clone, Debug)]
struct ScannedIsf {
    path: PathBuf,
    definition: IsfDefinition,
}

/// One sorted, bounded walk of a user module directory.
///
/// `extension` selects which files are collected (case-insensitively) and
/// `label` names the module kind in warning messages. The walker is shared by
/// the ISF (`.fs`) and MilkDrop (`.mlk`) scanners so both honour identical
/// hidden/temporary/symlink skips and the depth/entry caps carried by
/// [`IsfScanLimits`] (the user caps reuse [`USER_ISF_MAX_DEPTH`] and
/// [`USER_ISF_MAX_ENTRIES`]).
struct ModuleFileScan {
    limits: IsfScanLimits,
    extension: &'static str,
    label: &'static str,
    files: Vec<PathBuf>,
    visited: usize,
    depth_limited: bool,
    entry_limited: bool,
}

impl ModuleFileScan {
    /// The ISF scanner: case-insensitive `.fs`, labelled "ISF" in warnings.
    fn collect(root: &Path, limits: IsfScanLimits) -> Self {
        Self::collect_with_extension(root, limits, "fs", "ISF")
    }

    /// A generic scanner for the given extension and warning label.
    fn collect_with_extension(
        root: &Path,
        limits: IsfScanLimits,
        extension: &'static str,
        label: &'static str,
    ) -> Self {
        let mut scan = Self {
            limits,
            extension,
            label,
            files: Vec::new(),
            visited: 0,
            depth_limited: false,
            entry_limited: false,
        };
        scan.walk(root, 0);
        // Sort by full path so registration order is stable even when the
        // platform returns directory entries in hash order.
        scan.files.sort();
        if scan.depth_limited {
            eprintln!(
                "engine: WARN {} scan depth limit ({}) reached under {}; deeper folders were skipped",
                scan.label,
                limits.max_depth,
                root.display()
            );
        }
        if scan.entry_limited {
            eprintln!(
                "engine: WARN {} scan entry limit ({}) reached under {}; remaining files were skipped",
                scan.label,
                limits.max_entries,
                root.display()
            );
        }
        scan
    }

    fn walk(&mut self, dir: &Path, depth: usize) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                let suffix = if depth == 0 {
                    format!("; no {} modules registered from it", self.label)
                } else {
                    "; skipping that folder".to_owned()
                };
                eprintln!(
                    "engine: WARN {} directory {} is not readable: {error}{suffix}",
                    self.label,
                    dir.display()
                );
                return;
            }
        };

        // Sorting each directory makes the entry cap deterministic as well as
        // the final file order.
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            if self.visited >= self.limits.max_entries {
                self.entry_limited = true;
                return;
            }
            self.visited += 1;

            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if crate::media_watch::is_ignored_media_name(name) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                if !self.limits.recursive {
                    continue;
                }
                if depth < self.limits.max_depth {
                    self.walk(&path, depth + 1);
                } else {
                    self.depth_limited = true;
                }
                continue;
            }
            // Symlinks are deliberately not followed: a link cycle must not
            // turn a user module folder into an unbounded walk.
            if !file_type.is_file() {
                continue;
            }
            if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case(self.extension))
            {
                self.files.push(path);
            }
        }
    }
}

/// The `*.fs` files under `dir`, sorted by path and bounded by `limits`.
fn scan_isf_paths(dir: &Path, limits: IsfScanLimits) -> Vec<PathBuf> {
    ModuleFileScan::collect(dir, limits).files
}

/// Scan the corpus and every extra user directory, then resolve duplicate
/// module names deterministically.
///
/// The built-in corpus is scanned first and flat, so it keeps winning against
/// a user shader with the same stem (existing Scene references never change
/// meaning). Extra directories are sorted before scanning; within a directory
/// the walk is sorted too, so the **first sorted path** wins a duplicate. The
/// ignored copies are collected so the caller can report them once.
fn collect_isf_definitions(
    isf_dir: Option<&Path>,
    extra_dirs: &[PathBuf],
) -> (Vec<IsfDefinition>, Vec<DuplicateIsfName>) {
    let mut scanned = Vec::new();
    if let Some(dir) = isf_dir {
        scanned.extend(scan_isf_directory(dir, IsfScanLimits::CORPUS));
    }
    let mut extras: Vec<&Path> = extra_dirs.iter().map(PathBuf::as_path).collect();
    extras.sort();
    extras.dedup();
    for dir in extras {
        scanned.extend(scan_isf_directory(dir, IsfScanLimits::USER));
    }

    let mut kept: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    let mut seen_paths = std::collections::HashSet::new();
    let mut duplicate_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut definitions = Vec::new();
    let mut duplicates: Vec<DuplicateIsfName> = Vec::new();
    for entry in scanned {
        // An extra root nested inside another watched root reaches the same
        // file twice; that is not a name collision.
        if !seen_paths.insert(entry.path.clone()) {
            continue;
        }
        let name = entry.definition.name.clone();
        if let Some(first) = kept.get(&name) {
            if let Some(&index) = duplicate_index.get(&name) {
                duplicates[index].ignored.push(entry.path);
            } else {
                let index = duplicates.len();
                duplicates.push(DuplicateIsfName {
                    name: name.clone(),
                    kept: first.clone(),
                    ignored: vec![entry.path],
                });
                duplicate_index.insert(name, index);
            }
            continue;
        }
        kept.insert(name, entry.path.clone());
        definitions.push(entry.definition);
    }
    (definitions, duplicates)
}

/// Parse the `*.fs` files found by [`scan_isf_paths`] into definitions.
fn scan_isf_directory(dir: &Path, limits: IsfScanLimits) -> Vec<ScannedIsf> {
    let files = scan_isf_paths(dir, limits);

    let mut definitions = Vec::with_capacity(files.len());
    let mut failed = 0usize;
    for path in files {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            eprintln!(
                "engine: WARN skipping ISF shader with a non-UTF-8 file name: {}",
                path.display()
            );
            failed += 1;
            continue;
        };
        let name = stem.to_string();
        let fragment = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!(
                    "engine: WARN skipping ISF shader {} (read failed: {error})",
                    path.display()
                );
                failed += 1;
                continue;
            }
        };
        let vertex_path = path.with_extension("vs");
        let vertex = if vertex_path.is_file() {
            match fs::read_to_string(&vertex_path) {
                Ok(source) => Some(source),
                Err(error) => {
                    eprintln!(
                        "engine: WARN skipping ISF shader {} (vertex sidecar read failed: {error})",
                        path.display()
                    );
                    failed += 1;
                    continue;
                }
            }
        } else {
            None
        };

        match parse_isf(&fragment, vertex.as_deref()) {
            Ok(document) => {
                let meta = meta_from_document(&document);
                let props = synthesise_props(&document).into_iter().collect();
                definitions.push(ScannedIsf {
                    path,
                    definition: IsfDefinition {
                        name,
                        meta,
                        props,
                        document,
                    },
                });
            }
            Err(error) => {
                eprintln!(
                    "engine: WARN skipping ISF shader {} (parse failed: {error})",
                    path.display()
                );
                failed += 1;
            }
        }
    }

    eprintln!(
        "engine: INFO registered {} ISF module(s) from {} ({} failed)",
        definitions.len(),
        dir.display(),
        failed
    );
    definitions
}

/// The `.mlk` files under `dir`, sorted by path and bounded by `limits`.
fn scan_milk_paths(dir: &Path, limits: IsfScanLimits) -> Vec<PathBuf> {
    ModuleFileScan::collect_with_extension(dir, limits, "mlk", "MilkDrop").files
}

/// Collect MilkDrop presets from user directories.
///
/// Roots are sorted and deduplicated before walking. Each root is scanned with
/// the same bounded walker as user ISF folders (see [`ModuleFileScan`]):
/// case-insensitive `.mlk` matches, sorted paths, hidden/temporary entries and
/// symlinks skipped, recursion capped by [`USER_ISF_MAX_DEPTH`] and
/// [`USER_ISF_MAX_ENTRIES`]. The first sorted path wins a duplicate file stem;
/// later duplicates are logged and skipped. Each preset is parsed header-only
/// with [`parse_milk`]; an unreadable or unparsable file is likewise logged and
/// skipped. The module name is the file stem, exactly like the ISF arm.
fn collect_milkdrop_definitions(user_dirs: &[PathBuf]) -> Vec<MilkdropDefinition> {
    let mut dirs: Vec<&Path> = user_dirs.iter().map(PathBuf::as_path).collect();
    dirs.sort_unstable();
    dirs.dedup();

    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut definitions = Vec::new();
    for dir in dirs {
        for path in scan_milk_paths(dir, IsfScanLimits::USER) {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                eprintln!(
                    "engine: WARN skipping MilkDrop preset with a non-UTF-8 file name: {}",
                    path.display()
                );
                continue;
            };
            let name = stem.to_string();
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) => {
                    eprintln!(
                        "engine: WARN skipping MilkDrop preset {} (read failed: {error})",
                        path.display()
                    );
                    continue;
                }
            };
            let document = match parse_milk(&content, &name) {
                Ok(document) => document,
                Err(error) => {
                    eprintln!(
                        "engine: WARN skipping MilkDrop preset {} (parse failed: {error})",
                        path.display()
                    );
                    continue;
                }
            };
            if !seen_names.insert(name.clone()) {
                eprintln!(
                    "engine: WARN duplicate MilkDrop preset name `{name}`: keeping the first; ignoring {}",
                    path.display()
                );
                continue;
            }
            let meta = milk_meta_from_document(&document);
            let props = milk_synthesise_props(&document);
            definitions.push(MilkdropDefinition { name, meta, props });
        }
    }
    definitions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_knows_both_module_kinds() {
        let registry = ModuleRegistry::builtin();
        assert_eq!(registry.kind("Bands"), Some(ModuleKind::Shader));
        assert_eq!(registry.kind("Ball"), Some(ModuleKind::TwoD));
        assert_eq!(registry.kind("Not A Module"), None);
        assert_eq!(registry.shader_modules().len(), 36);
        assert_eq!(registry.two_d_definitions().len(), 16);
    }

    #[test]
    fn video_module_is_registered_with_start_and_finish_trims() {
        let registry = ModuleRegistry::builtin();
        assert_eq!(registry.kind("Video"), Some(ModuleKind::TwoD));
        let defaults = registry
            .default_props("Video")
            .expect("Video has prop defaults");
        for key in [
            "texture",
            "cropTop",
            "cropRight",
            "cropBottom",
            "cropLeft",
            "scale",
            "offsetX",
            "offsetY",
            "constrain",
        ] {
            assert!(
                defaults.get(key).is_some(),
                "Video is missing Texture 2D's `{key}` prop"
            );
        }
        assert_eq!(
            defaults["start"],
            json!(0.0),
            "start defaults to the beginning"
        );
        assert_eq!(defaults["finish"], json!(1.0), "finish defaults to the end");
        assert!(
            defaults.get("progress").is_none(),
            "the progress input must be removed"
        );
        for prop in ["start", "finish"] {
            let schema = registry
                .prop_schema("Video", prop)
                .unwrap_or_else(|| panic!("{prop} schema"));
            assert_eq!(schema.type_, "float");
            assert_eq!(schema.min, Some(0.0));
            assert_eq!(schema.max, Some(1.0));
        }
    }

    #[test]
    fn two_d_meta_fields_match_the_js_descriptors() {
        let registry = ModuleRegistry::builtin();
        let ball = registry.meta_fields("Ball").expect("Ball is registered");
        assert_eq!(ball["name"], json!("Ball"));
        assert_eq!(ball["author"], json!("2xAA"));
        assert_eq!(ball["version"], json!("1.0.1"));
        assert_eq!(ball["type"], json!("2d"));
        assert_eq!(ball["audioFeatures"], json!(["zcr", "rms"]));
        assert!(ball.get("category").is_none());
        assert!(ball.get("previewWithOutput").is_none());

        // Text declares neither author nor version (`Text.js`), so the spread
        // leaves them out.
        let text = registry.meta_fields("Text").expect("Text is registered");
        assert!(text.get("author").is_none());
        assert!(text.get("version").is_none());

        // Effects-category modules carry `category`; the two JS descriptors
        // that set `previewWithOutput` keep it.
        let pixelate = registry
            .meta_fields("Pixelate")
            .expect("Pixelate is registered");
        assert_eq!(pixelate["category"], json!("effects"));
        assert_eq!(pixelate["previewWithOutput"], json!(true));
        assert!(
            registry
                .meta_fields("Smear")
                .unwrap()
                .get("previewWithOutput")
                .is_none()
        );
    }

    #[test]
    fn two_d_default_props_are_the_descriptor_defaults() {
        let registry = ModuleRegistry::builtin();
        let props = registry
            .default_props("Ball")
            .expect("Ball has prop defaults");
        assert_eq!(props["amount"], json!(10));
        assert_eq!(props["speed"], json!(2.0));
        assert_eq!(props["wrap"], json!(false));
        // The tween default is the raw palette control object, as in JS.
        assert!(props["color"]["data"].is_array());
        assert_eq!(props["color"]["duration"], json!(10000));
    }

    #[test]
    fn two_d_prop_schema_carries_the_js_coercion_flags() {
        let registry = ModuleRegistry::builtin();

        // Polygon.js:29-37 — `strict: true` clamps, `type: "int"` rounds.
        let stroke = registry
            .prop_schema("Polygon", "strokeWeight")
            .expect("Polygon declares strokeWeight");
        assert_eq!(stroke.type_, "int");
        assert_eq!(stroke.min, Some(1.0));
        assert_eq!(stroke.max, Some(20.0));
        assert!(stroke.strict);

        // Ball.js:38-48 — `size` is bounded but takes `abs`, not `strict`.
        let size = registry
            .prop_schema("Ball", "size")
            .expect("Ball declares size");
        assert!(!size.strict);
        assert!(size.abs);

        // A bounded prop with no flags stays unclamped.
        let speed = registry
            .prop_schema("Ball", "speed")
            .expect("Ball declares speed");
        assert!(!speed.strict && !speed.abs);
        assert_eq!(speed.max, Some(20.0));

        assert!(registry.prop_schema("Ball", "nope").is_none());
        assert!(registry.prop_schema("Not A Module", "amount").is_none());
    }

    #[test]
    fn create_two_d_builds_the_named_instance() {
        let registry = ModuleRegistry::builtin();
        let module = registry.create_two_d("Smear").expect("Smear is registered");
        assert_eq!(module.meta().name, "Smear");
        assert!(registry.create_two_d("Bands").is_none());
    }

    // ————————————————— Point10 scenes (M4) —————————————————

    #[test]
    fn registered_modules_cover_every_kind_and_match_meta_fields() {
        let registry = ModuleRegistry::builtin();
        let registered = registry.registered_modules();
        let map = registered.as_object().expect("registered is an object");

        for name in ["Bands", "Ball", "Point10 Chromatic Line"] {
            let entry = map.get(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(
                entry["meta"],
                json!(registry.meta_fields(name).expect("meta fields")),
                "{name} meta must match meta_fields"
            );
            assert!(
                entry["props"]
                    .as_object()
                    .is_some_and(|props| !props.is_empty()),
                "{name} must expose prop descriptors"
            );
        }

        // Built-in shader + 2D + Point10 definitions, all unique.
        assert_eq!(map.len(), 36 + 16 + 27, "built-in shader + 2D + Point10");
        assert_eq!(map["Bands"]["props"]["u_ringWidth"]["type"], json!("float"));
        assert_eq!(map["Bands"]["props"]["u_ringWidth"]["min"], json!(0.01));
    }

    #[test]
    fn registry_knows_the_point10_scenes() {
        let registry = ModuleRegistry::builtin();
        assert_eq!(
            registry.kind("Point10 Chromatic Line"),
            Some(ModuleKind::Point10)
        );
        assert_eq!(
            registry.kind("Point10 Chromatic Wave"),
            Some(ModuleKind::Point10)
        );
        assert_eq!(registry.kind("Point10 Waveform"), Some(ModuleKind::Point10));
        // Point10 names are their own kind, not 2D modules.
        assert_eq!(registry.kind("Ball"), Some(ModuleKind::TwoD));
        assert_eq!(registry.kind("Point10 Not A Scene"), None);
        assert_eq!(registry.point10_definitions().len(), 27);
    }

    #[test]
    fn point10_definitions_match_the_wrapper_modules() {
        let registry = ModuleRegistry::builtin();
        let names: Vec<&str> = registry
            .point10_definitions()
            .iter()
            .map(|definition| definition.meta.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "Point10 Chromatic Line",
                "Point10 Chromatic Wave",
                "Point10 Particle Field",
                "Point10 Spiral Particles",
                "Point10 Waveform",
                "Point10 Square Tunnel",
                "Point10 Light Tunnel",
                "Point10 Wormhole",
                "Point10 It Is Coming For You",
                "Point10 Water Ripple",
                "Point10 Glass Lens",
                "Point10 Mycelium",
                "Point10 Around Plus",
                "Point10 Ultra HDR",
                "Point10 Mirror Cube",
                "Point10 Color Cube",
                "Point10 Boxy",
                "Point10 Sound Cube",
                "Point10 Hollow Cube",
                "Point10 Attractor Particles",
                "Point10 DVD Room",
                "Point10 Rolling Fire",
                "Point10 Ball Of Fire",
                "Point10 Smoke Room",
                "Point10 Latent Backrooms",
                "Point10 Sun",
                "Point10 Earth",
            ]
        );

        for definition in registry.point10_definitions() {
            assert_eq!(definition.meta.author, "Point10");
            assert_eq!(definition.meta.version, "1.0.0");
            // The JS wrappers register as `2d` modules so Scene documents and
            // presets keep resolving them (`adapter.js:857-866`).
            assert_eq!(definition.meta.module_type, "2d");
            assert_eq!(definition.meta.audio_features, &["bass", "mid", "treble"]);
            assert!(!definition.props.is_empty());

            // The levers come first and the scene schema follows
            // (`adapter.js:868-871`).
            let levers: Vec<&str> = definition.props[..6].iter().map(|prop| prop.name).collect();
            assert_eq!(
                levers,
                vec![
                    "gain",
                    "smoothing",
                    "bassGain",
                    "midGain",
                    "trebleGain",
                    "useAgc"
                ]
            );
        }

        // Categories mirror the legacy JS metadata and are independent of
        // render scale: Chromatic Line, Chromatic Wave and Waveform are filed
        // under 2D Scenes, while Particle Field, Spiral Particles, Sun and
        // Earth stay under 3D Scenes, as do the heavy ones.
        let category = |name: &str| {
            registry
                .point10_definition(name)
                .expect("registered")
                .meta
                .category
        };
        assert_eq!(category("Point10 Chromatic Line"), "2d-scenes");
        assert_eq!(category("Point10 Chromatic Wave"), "2d-scenes");
        assert_eq!(category("Point10 Waveform"), "2d-scenes");
        assert_eq!(category("Point10 Particle Field"), "3d-scenes");
        assert_eq!(category("Point10 Spiral Particles"), "3d-scenes");
        assert_eq!(category("Point10 Sun"), "3d-scenes");
        assert_eq!(category("Point10 Earth"), "3d-scenes");
    }

    #[test]
    fn point10_meta_fields_match_the_js_wrapper() {
        let registry = ModuleRegistry::builtin();
        let meta = registry
            .meta_fields("Point10 Chromatic Wave")
            .expect("registered");
        assert_eq!(meta["name"], json!("Point10 Chromatic Wave"));
        assert_eq!(meta["author"], json!("Point10"));
        assert_eq!(meta["version"], json!("1.0.0"));
        assert_eq!(meta["type"], json!("2d"));
        assert_eq!(meta["category"], json!("2d-scenes"));
        assert_eq!(meta["audioFeatures"], json!(["bass", "mid", "treble"]));

        let waveform = registry
            .meta_fields("Point10 Waveform")
            .expect("registered");
        assert_eq!(waveform["category"], json!("2d-scenes"));
    }

    #[test]
    fn point10_default_props_are_the_scene_defaults() {
        let registry = ModuleRegistry::builtin();

        // `sceneProps.js: CHROMATIC_LINE_PROPS` + the levers.
        let props = registry
            .default_props("Point10 Chromatic Wave")
            .expect("registered");
        assert_eq!(props["gain"], json!(1.0));
        assert_eq!(props["smoothing"], json!(0.0));
        assert_eq!(props["bassGain"], json!(1.0));
        assert_eq!(props["useAgc"], json!(true));
        assert_eq!(props["distortion"], json!(0.05));
        assert_eq!(props["amplitude"], json!(1.0));
        assert_eq!(props["glow"], json!(1.0));

        let waveform = registry
            .default_props("Point10 Waveform")
            .expect("registered");
        assert_eq!(waveform["height"], json!(1.0));
        assert_eq!(waveform["speed"], json!(1.0));
        assert_eq!(waveform["opacity"], json!(1.0));

        // Every registered scene has a default for every declared prop.
        for definition in registry.point10_definitions() {
            let props = registry
                .default_props(definition.meta.name)
                .expect("defaults");
            for prop in &definition.props {
                assert!(
                    props.get(prop.name).is_some(),
                    "{} is missing a default for {}",
                    definition.meta.name,
                    prop.name
                );
            }
        }
    }

    #[test]
    fn point10_prop_schema_clamps_like_the_scene_binder() {
        let registry = ModuleRegistry::builtin();

        let gain = registry
            .prop_schema("Point10 Chromatic Wave", "gain")
            .expect("gain is declared");
        assert_eq!(gain.type_, "float");
        assert_eq!(gain.min, Some(0.0));
        assert_eq!(gain.max, Some(4.0));
        assert!(gain.strict);

        let use_agc = registry
            .prop_schema("Point10 Waveform", "useAgc")
            .expect("useAgc is declared");
        assert_eq!(use_agc.type_, "bool");
        assert!(!use_agc.strict);
        assert_eq!(use_agc.default, Some(json!(true)));

        assert!(
            registry
                .prop_schema("Point10 Waveform", "notAProp")
                .is_none()
        );
    }

    #[test]
    fn create_point10_builds_the_named_scene() {
        let registry = ModuleRegistry::builtin();

        let scene = registry
            .create_point10_instance("Point10 Waveform")
            .expect("Waveform is registered");
        assert_eq!(scene.name(), "Point10 Waveform");

        let boxed = registry
            .create_point10("Point10 Waveform")
            .expect("Waveform is registered");
        assert_eq!(boxed.name(), "Point10 Waveform");

        assert!(registry.create_point10("Bands").is_none());
        assert!(registry.create_point10_instance("Not A Scene").is_none());
    }

    #[test]
    fn every_registered_scene_constructs() {
        let registry = ModuleRegistry::builtin();
        for definition in registry.point10_definitions() {
            assert!(
                registry.create_point10(definition.meta.name).is_some(),
                "{} is registered but has no constructor in Point10SceneKind",
                definition.meta.name
            );
        }
    }

    // ————————————————— ISF corpus (WS-4) —————————————————

    /// Write one inline ISF shader into a fresh temporary directory and return
    /// that directory.
    fn write_inline_isf(source: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("modv-isf-registry-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp corpus directory");
        std::fs::write(dir.join("Inline Test.fs"), source).expect("inline shader written");
        dir
    }

    /// Every input kind the synthesiser handles, in declaration order.
    const INLINE_ISF: &str = r#"/*{
        "CREDIT": "by test",
        "DESCRIPTION": "inline corpus entry",
        "VSN": "2.1",
        "ISFVSN": "2",
        "MODV_CATEGORY": "3D Scenes",
        "INPUTS": [
            { "NAME": "gain", "TYPE": "float", "MIN": 0.0, "MAX": 2.0, "DEFAULT": 0.75 },
            { "NAME": "flip", "TYPE": "bool", "DEFAULT": true },
            { "NAME": "mode", "LABEL": "ignored", "TYPE": "long", "DEFAULT": 2,
              "VALUES": [0, 2, 5], "LABELS": ["zero", "two", "five"] },
            { "NAME": "tint", "TYPE": "color", "DEFAULT": [0.5, 0.25, 0.0, 1.0] },
            { "NAME": "centre", "TYPE": "point2D", "DEFAULT": [0.25, 0.75] },
            { "NAME": "inputImage", "TYPE": "image" },
            { "NAME": "restart", "TYPE": "event" }
        ]
    }*/
    void main() { gl_FragColor = vec4(0.0); }
    "#;

    #[test]
    fn isf_props_match_the_javascript_synthesis() {
        let dir = write_inline_isf(INLINE_ISF);
        let registry = ModuleRegistry::build(Some(&dir));
        assert_eq!(registry.kind("Inline Test"), Some(ModuleKind::Isf));

        let props = registry
            .default_props("Inline Test")
            .expect("the shader declares props");
        assert_eq!(props["gain"], json!(0.75));
        assert_eq!(props["flip"], json!(true));
        assert_eq!(props["mode"], json!(2), "the selected enum member");
        assert_eq!(props["tint"], json!([0.5, 0.25, 0.0, 1.0]));
        assert_eq!(props["centre"], json!([0.25, 0.75]));
        assert_eq!(props["inputImage"], json!(null));
        assert_eq!(props["restart"], json!(null));

        let definition = registry
            .isf_definition("Inline Test")
            .expect("registered definition");
        let kinds: Vec<&str> = definition
            .props
            .iter()
            .map(|(_, prop)| prop.type_.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec!["float", "bool", "enum", "vec4", "vec2", "texture", "event"]
        );
        assert_eq!(definition.props[0].1.step, Some(0.01));
        assert_eq!(definition.props[2].1.label, "mode");

        // `setupModule` metadata, including the image-input preview flag.
        assert_eq!(definition.meta.author.as_deref(), Some("by test"));
        assert_eq!(
            definition.meta.description.as_deref(),
            Some("inline corpus entry")
        );
        assert_eq!(definition.meta.version.as_deref(), Some("2.1"));
        assert_eq!(definition.meta.category.as_deref(), Some("3d-scenes"));
        assert!(definition.meta.preview_with_output);
        assert_eq!(definition.meta.isf_version, 2.0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn isf_meta_fields_map_to_the_js_module_meta() {
        let dir = write_inline_isf(INLINE_ISF);
        let registry = ModuleRegistry::build(Some(&dir));
        let meta = registry.meta_fields("Inline Test").expect("registered");
        assert_eq!(meta["name"], json!("Inline Test"));
        assert_eq!(meta["author"], json!("by test"));
        assert_eq!(meta["description"], json!("inline corpus entry"));
        assert_eq!(meta["version"], json!("2.1"));
        assert_eq!(meta["type"], json!("isf"));
        assert_eq!(meta["category"], json!("3d-scenes"));
        assert_eq!(meta["isfVersion"], json!(2.0));
        assert_eq!(meta["previewWithOutput"], json!(true));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn isf_prop_schemas_keep_bounds_but_do_not_clamp() {
        let dir = write_inline_isf(INLINE_ISF);
        let registry = ModuleRegistry::build(Some(&dir));

        let gain = registry.prop_schema("Inline Test", "gain").expect("float");
        assert_eq!(gain.type_, "float");
        assert_eq!(gain.min, Some(0.0));
        assert_eq!(gain.max, Some(2.0));
        assert!(!gain.strict, "ISF synthesis never sets strict");
        assert!(!gain.abs);

        // The store's coercion pipeline must leave an out-of-range write alone
        // (the JS renderer only clamps `strict` props).
        let coerced = modv_engine::props::coerce_prop_value(&gain, &json!(99.0));
        assert_eq!(coerced, json!(99.0));

        let mode = registry.prop_schema("Inline Test", "mode").expect("enum");
        assert_eq!(mode.type_, "enum");
        assert_eq!(mode.default, Some(json!(2)));

        let image = registry
            .prop_schema("Inline Test", "inputImage")
            .expect("texture");
        assert_eq!(image.type_, "texture");

        let event = registry
            .prop_schema("Inline Test", "restart")
            .expect("event");
        assert_eq!(event.type_, "event");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The vendored ISF corpus directory, or `None` when this checkout does
    /// not have it (the test then skips).
    fn corpus_dir() -> Option<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
        dir.is_dir().then_some(dir)
    }

    /// The e2e gallery-category contract (`isfGalleryCategories.spec.js:1-30`):
    /// every named corpus entry is registered as an ISF module whose meta
    /// category is `3d-scenes`.
    fn assert_isf_modules_are_3d_scenes(names: &[&str]) {
        let Some(corpus) = corpus_dir() else {
            eprintln!("skipping ISF category test: the corpus is missing");
            return;
        };
        let registry = ModuleRegistry::build(Some(&corpus));

        for name in names {
            assert_eq!(
                registry.kind(name),
                Some(ModuleKind::Isf),
                "{name} should be registered from the corpus"
            );
            let meta = registry
                .meta_fields(name)
                .unwrap_or_else(|| panic!("{name} is registered but has no meta"));
            assert_eq!(meta["type"], json!("isf"), "{name}");
            assert_eq!(meta["category"], json!("3d-scenes"), "{name}");
        }
    }

    #[test]
    fn tunnel_isf_modules_are_registered_as_3d_scenes() {
        assert_isf_modules_are_3d_scenes(&[
            "digital-crystal-tunnel",
            "pixel-tunnel",
            "fractal-mage-tunnel",
            "lopy-frac-tunnel",
            "fractal-mountain-tunnel",
        ]);
    }

    #[test]
    fn nc_sa_scene_isf_modules_are_registered_as_3d_scenes() {
        assert_isf_modules_are_3d_scenes(&["Helix Eye", "Traced Tunnel", "Hot Shower"]);
    }

    #[test]
    fn isf_corpus_directory_is_scanned_into_definitions() {
        let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
        if !corpus.is_dir() {
            eprintln!("skipping ISF corpus test: {} is missing", corpus.display());
            return;
        }

        let registry = ModuleRegistry::build(Some(&corpus));
        let definitions = registry.isf_definitions();
        let file_count = std::fs::read_dir(&corpus)
            .expect("corpus directory reads")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("fs"))
            })
            .count();
        assert!(
            definitions.len() >= 340,
            "expected at least 340 parsable shaders, got {}",
            definitions.len()
        );
        assert!(
            definitions.len() <= file_count,
            "registered more definitions ({}) than .fs files ({file_count})",
            definitions.len()
        );

        for name in [
            "Color Invert",
            "Bloom",
            "badtv",
            "Circular Screen",
            "Posterize",
            "Vignette",
            "ASCII Art",
        ] {
            assert_eq!(
                registry.kind(name),
                Some(ModuleKind::Isf),
                "{name} should be registered from the corpus"
            );
        }

        // A known filter exposes its image input and float props.
        let bloom = registry.isf_definition("Bloom").expect("Bloom registered");
        assert!(bloom.prop("inputImage").is_some());
        assert!(bloom.prop("blurAmount").is_some());
        assert!(bloom.meta.preview_with_output);
        assert!(bloom.document.passes.len() > 1, "Bloom is multi-pass");
    }

    fn registry_temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp registry dir");
        dir
    }

    fn write_shader(path: &Path, name: &str, description: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("shader parent");
        }
        std::fs::write(
            path,
            format!(
                r#"/*{{
  "NAME": "{name}",
  "DESCRIPTION": "{description}",
  "CREDIT": "modV",
  "ISFVSN": "2",
  "CATEGORIES": ["Effects"],
  "INPUTS": []
}}*/
void main() {{ gl_FragColor = vec4(1.0); }}
"#
            ),
        )
        .expect("write test shader");
    }

    #[test]
    fn extra_isf_directories_are_scanned_recursively() {
        let dir = registry_temp_dir("modv-isf-nested");
        write_shader(
            &dir.join("artist/pack/Deep Shader.fs"),
            "Deep Shader",
            "nested in two folders",
        );
        write_shader(
            &dir.join("artist/Top Shader.fs"),
            "Top Shader",
            "directly in a subfolder",
        );
        std::fs::write(
            dir.join("artist/pack/Deep Shader.vs"),
            "void main() { gl_Position = vec4(0.0); }\n",
        )
        .expect("nested vertex sidecar");

        let registry = ModuleRegistry::build_with_extra(None, std::slice::from_ref(&dir));

        assert_eq!(registry.kind("Deep Shader"), Some(ModuleKind::Isf));
        assert_eq!(registry.kind("Top Shader"), Some(ModuleKind::Isf));
        let deep = registry
            .isf_definition("Deep Shader")
            .expect("nested shader registered");
        assert!(
            deep.document.vertex_source.is_some(),
            "a nested .fs picks up its sibling .vs"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn extra_isf_scan_ignores_hidden_and_temporary_entries() {
        let dir = registry_temp_dir("modv-isf-ignored");
        write_shader(
            &dir.join("artist/Visible Shader.fs"),
            "Visible Shader",
            "kept",
        );
        write_shader(
            &dir.join("artist/.hidden/Hidden Folder Shader.fs"),
            "Hidden Folder Shader",
            "hidden directory",
        );
        write_shader(
            &dir.join("artist/.Hidden Shader.fs"),
            ".Hidden Shader",
            "hidden file",
        );
        std::fs::write(dir.join("artist/Backup Shader.fs~"), "backup").expect("editor backup");
        std::fs::write(dir.join("artist/Swap Shader.fs.tmp"), "swap").expect("editor swap");
        std::fs::write(dir.join("artist/Swap Shader.fs.part"), "part").expect("partial write");

        let registry = ModuleRegistry::build_with_extra(None, std::slice::from_ref(&dir));
        let names: Vec<&str> = registry
            .isf_definitions()
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();

        assert_eq!(names, vec!["Visible Shader"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_user_isf_stems_keep_the_first_sorted_directory() {
        let root = registry_temp_dir("modv-isf-duplicate");
        let first = root.join("a-pack");
        let second = root.join("b-pack");
        write_shader(
            &first.join("Shared Shader.fs"),
            "Shared Shader",
            "from a-pack",
        );
        write_shader(
            &second.join("Shared Shader.fs"),
            "Shared Shader",
            "from b-pack",
        );

        // The extra directories are supplied in reverse order on purpose: the
        // scan sorts them so the result does not depend on caller order.
        let registry = ModuleRegistry::build_with_extra(None, &[second, first]);

        let definition = registry
            .isf_definition("Shared Shader")
            .expect("duplicate name still registers once");
        assert_eq!(definition.meta.description.as_deref(), Some("from a-pack"));
        assert_eq!(
            registry
                .isf_definitions()
                .iter()
                .filter(|definition| definition.name == "Shared Shader")
                .count(),
            1
        );

        let (definitions, duplicates) =
            collect_isf_definitions(None, &[root.join("b-pack"), root.join("a-pack")]);
        assert_eq!(definitions.len(), 1);
        assert_eq!(duplicates.len(), 1, "the duplicate is reported");
        assert_eq!(duplicates[0].name, "Shared Shader");
        assert_eq!(duplicates[0].kept, root.join("a-pack/Shared Shader.fs"));
        assert_eq!(
            duplicates[0].ignored,
            vec![root.join("b-pack/Shared Shader.fs")]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overlapping_user_isf_roots_are_not_reported_as_duplicates() {
        let root = registry_temp_dir("modv-isf-overlap");
        write_shader(
            &root.join("pack/Only Shader.fs"),
            "Only Shader",
            "one file, two roots",
        );

        let (definitions, duplicates) =
            collect_isf_definitions(None, &[root.clone(), root.join("pack")]);

        assert_eq!(definitions.len(), 1);
        assert!(
            duplicates.is_empty(),
            "a file reached through two roots is not a duplicate"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn user_isf_scan_enforces_depth_and_entry_caps() {
        let dir = registry_temp_dir("modv-isf-caps");

        // Default depth cap: a folder at depth `USER_ISF_MAX_DEPTH` is
        // scanned (the root is depth 0), one below it is not.
        let mut deep = dir.join("a");
        for _ in 1..USER_ISF_MAX_DEPTH {
            deep.push("d");
        }
        write_shader(&deep.join("At Limit.fs"), "At Limit", "at the depth cap");
        deep.push("one-more");
        write_shader(&deep.join("Too Deep.fs"), "Too Deep", "below the depth cap");

        let found = scan_isf_paths(&dir, IsfScanLimits::USER);
        let names: Vec<String> = found
            .iter()
            .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()))
            .map(str::to_string)
            .collect();
        assert!(names.iter().any(|name| name == "At Limit"));
        assert!(
            !names.iter().any(|name| name == "Too Deep"),
            "folders below the depth cap are not scanned"
        );

        // Entry cap: entries are visited in sorted order, so the first files
        // win and the scan stops before reading the rest of the tree.
        let entries = registry_temp_dir("modv-isf-entries");
        for index in 0..5 {
            write_shader(
                &entries.join(format!("Shader {index}.fs")),
                &format!("Shader {index}"),
                "entry cap",
            );
        }
        let limits = IsfScanLimits {
            recursive: true,
            max_depth: USER_ISF_MAX_DEPTH,
            max_entries: 2,
        };
        let capped = scan_isf_paths(&entries, limits);
        let capped_names: Vec<String> = capped
            .iter()
            .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()))
            .map(str::to_string)
            .collect();
        assert_eq!(capped_names, vec!["Shader 0", "Shader 1"]);

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(entries);
    }

    #[test]
    fn extra_isf_directories_register_new_shaders() {
        let dir = std::env::temp_dir().join(format!(
            "modv-extra-isf-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("create extra ISF dir");
        std::fs::write(
            dir.join("VmodV Test Shader.fs"),
            r#"/*{
  "NAME": "VmodV Test Shader",
  "DESCRIPTION": "VmodV registration test",
  "CREDIT": "modV",
  "ISFVSN": "2",
  "CATEGORIES": ["Effects"],
  "INPUTS": []
}*/
void main() { gl_FragColor = vec4(1.0); }
"#,
        )
        .expect("write test shader");
        let registry = ModuleRegistry::build_with_extra(None, std::slice::from_ref(&dir));
        assert!(
            registry
                .isf_definitions()
                .iter()
                .any(|definition| definition.name == "VmodV Test Shader"),
            "extra ISF directory was not registered"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    // ————————————————— MilkDrop presets (integration 1) —————————————————

    /// A minimal parsable `.mlk` preset. The title uses the `Name - Author`
    /// convention so `meta_from_document` recovers the author; the file stem
    /// is the module name (the same contract ISF uses).
    fn write_milk_preset(path: &Path, name: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("milk preset parent");
        }
        std::fs::write(path, format!("[preset00]\ntitle={name} - Test Author\n"))
            .expect("write test milk preset");
    }

    #[test]
    fn milkdrop_preset_is_discovered_as_a_module() {
        let dir = registry_temp_dir("modv-milk-registry");
        write_milk_preset(&dir.join("Preset Name.mlk"), "Preset Name");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));

        assert_eq!(registry.kind("Preset Name"), Some(ModuleKind::Milkdrop));
        assert!(
            registry.milkdrop_definition("Preset Name").is_some(),
            "the preset has a MilkDrop definition"
        );
        assert_eq!(registry.milkdrop_definitions().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn milkdrop_scan_is_case_insensitive_and_extension_scoped() {
        let dir = registry_temp_dir("modv-milk-extension");
        write_milk_preset(&dir.join("Upper Preset.MLK"), "Upper Preset");
        write_milk_preset(&dir.join("Mixed Preset.Mlk"), "Mixed Preset");
        write_milk_preset(&dir.join("Not A Preset.txt"), "Not A Preset");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));

        assert_eq!(registry.kind("Upper Preset"), Some(ModuleKind::Milkdrop));
        assert_eq!(registry.kind("Mixed Preset"), Some(ModuleKind::Milkdrop));
        assert_eq!(
            registry.kind("Not A Preset"),
            None,
            "non-.mlk is not scanned"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn milkdrop_meta_fields_map_to_the_js_module_meta() {
        let dir = registry_temp_dir("modv-milk-meta");
        write_milk_preset(&dir.join("Preset Name.mlk"), "Preset Name");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));
        let meta = registry.meta_fields("Preset Name").expect("registered");

        assert_eq!(meta["name"], json!("Preset Name"));
        assert_eq!(meta["type"], json!("milkdrop"));
        assert_eq!(meta["category"], json!("effects"));
        assert_eq!(meta["author"], json!("Test Author"));
        assert_eq!(meta["version"], json!(1));
        assert_eq!(meta["previewWithOutput"], json!(false));
        assert_eq!(meta["description"], json!(""));

        // Caller-style defaults are not registry meta fields, exactly like the
        // ISF arm (see `build_preview_module` in `module_preview.rs`).
        assert!(meta.get("alpha").is_none());
        assert!(meta.get("enabled").is_none());
        assert!(meta.get("compositeOperation").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn milkdrop_format_props_are_empty() {
        let dir = registry_temp_dir("modv-milk-props");
        write_milk_preset(&dir.join("Preset Name.mlk"), "Preset Name");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));

        assert_eq!(registry.prop_names("Preset Name"), Vec::<String>::new());
        assert!(registry.prop_schema("Preset Name", "anything").is_none());
        assert!(registry.prop_schema("Preset Name", QUALITY_PROP).is_none());
        assert_eq!(
            registry
                .default_props("Preset Name")
                .expect("empty prop defaults still resolve"),
            json!({})
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn milkdrop_scan_ignores_hidden_temp_and_symlink_entries() {
        let dir = registry_temp_dir("modv-milk-ignored");
        write_milk_preset(&dir.join("Visible.mlk"), "Visible");
        write_milk_preset(&dir.join(".Hidden.mlk"), "Hidden");
        write_milk_preset(&dir.join(".hidden-dir/Nested.mlk"), "Nested");
        write_milk_preset(&dir.join("Backup.mlk~"), "Backup");
        write_milk_preset(&dir.join("Swap.mlk.tmp"), "Swap");
        std::os::unix::fs::symlink(dir.join("Visible.mlk"), dir.join("Link.mlk"))
            .expect("symlinked preset");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));
        let names: Vec<&str> = registry
            .milkdrop_definitions()
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();

        assert_eq!(names, vec!["Visible"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn milkdrop_scan_respects_the_shared_depth_cap() {
        let dir = registry_temp_dir("modv-milk-depth");
        let mut deep = dir.join("a");
        for _ in 1..USER_ISF_MAX_DEPTH {
            deep.push("d");
        }
        write_milk_preset(&deep.join("At Limit.mlk"), "At Limit");
        write_milk_preset(&deep.join("one-more/Too Deep.mlk"), "Too Deep");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));

        assert!(registry.milkdrop_definition("At Limit").is_some());
        assert!(
            registry.milkdrop_definition("Too Deep").is_none(),
            "folders below USER_ISF_MAX_DEPTH are not scanned"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn milkdrop_duplicate_stems_keep_the_first_sorted_path() {
        let root = registry_temp_dir("modv-milk-duplicate");
        std::fs::create_dir_all(root.join("a-pack")).expect("a-pack");
        std::fs::create_dir_all(root.join("b-pack")).expect("b-pack");
        std::fs::write(
            root.join("a-pack/Shared.mlk"),
            "[preset00]\ntitle=Shared - From A\n",
        )
        .expect("a-pack preset");
        std::fs::write(
            root.join("b-pack/Shared.mlk"),
            "[preset00]\ntitle=Shared - From B\n",
        )
        .expect("b-pack preset");

        // Supplied in reverse order on purpose: the scan sorts the roots.
        let registry = ModuleRegistry::build_with_milkdrop(
            None,
            &[],
            &[root.join("b-pack"), root.join("a-pack")],
        );

        let definition = registry
            .milkdrop_definition("Shared")
            .expect("duplicate stem still registers once");
        assert_eq!(definition.meta.author, "From A");
        assert_eq!(registry.milkdrop_definitions().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn milkdrop_malformed_preset_is_skipped_without_panic() {
        let dir = registry_temp_dir("modv-milk-malformed");
        std::fs::write(dir.join("Broken.mlk"), "not a preset\nfoo=1\n").expect("malformed preset");
        write_milk_preset(&dir.join("Good.mlk"), "Good");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));

        assert!(registry.milkdrop_definition("Broken").is_none());
        assert!(registry.milkdrop_definition("Good").is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn registered_modules_includes_the_milkdrop_bucket() {
        let dir = registry_temp_dir("modv-milk-registered");
        write_milk_preset(&dir.join("Preset Name.mlk"), "Preset Name");

        let registry = ModuleRegistry::build_with_milkdrop(None, &[], std::slice::from_ref(&dir));
        let registered = registry.registered_modules();
        let map = registered.as_object().expect("registered is an object");

        let entry = map.get("Preset Name").expect("MilkDrop module registered");
        assert_eq!(
            entry["meta"],
            json!(registry.meta_fields("Preset Name").expect("meta fields"))
        );
        assert_eq!(entry["props"], json!({}), "no format-specific props yet");

        // Existing kinds are still registered alongside the new bucket.
        for name in ["Bands", "Ball", "Point10 Chromatic Line"] {
            assert!(map.contains_key(name), "{name} is missing");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The committed registry snapshot generated from the legacy worker's
    /// `import.meta.glob` registrations. See `tests/unit/module-parity.test.js`
    /// for the generator/regenerator and the JS-side half of this guard.
    fn legacy_module_manifest() -> Value {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/module-parity.json");
        let text = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "could not read the legacy module manifest {}: {error}",
                path.display()
            )
        });
        serde_json::from_str(&text).unwrap_or_else(|error| {
            panic!(
                "could not parse the legacy module manifest {}: {error}",
                path.display()
            )
        })
    }

    fn manifest_names(manifest: &Value, group: &str) -> Vec<String> {
        manifest[group]
            .as_array()
            .unwrap_or_else(|| panic!("manifest group {group:?} is not an array"))
            .iter()
            .map(|name| {
                name.as_str()
                    .unwrap_or_else(|| panic!("manifest group {group:?} has a non-string"))
                    .to_string()
            })
            .collect()
    }

    /// Assert set equality (and no duplicate registrations) and print the
    /// symmetric difference on failure instead of two 354-entry lists.
    fn assert_same_module_names(group: &str, actual: Vec<String>, expected: Vec<String>) {
        use std::collections::BTreeSet;

        let actual_set: BTreeSet<String> = actual.iter().cloned().collect();
        let expected_set: BTreeSet<String> = expected.into_iter().collect();
        let missing: Vec<&String> = expected_set.difference(&actual_set).collect();
        let stale: Vec<&String> = actual_set.difference(&expected_set).collect();

        assert!(
            missing.is_empty() && stale.is_empty(),
            "{group} registry drifted from tests/fixtures/module-parity.json\n\
             missing from the Rust registry: {missing:?}\n\
             registered in Rust but not legacy: {stale:?}"
        );
        assert_eq!(
            actual_set.len(),
            actual.len(),
            "{group} registry registers a name more than once"
        );
    }

    /// Exact bucket counts from the legacy manifest. Kept under the original
    /// guard name because `docs/rust-hybrid-review.md` and the e2e parity spec
    /// reference it; the name-set check lives in
    /// [`module_registry_matches_the_legacy_manifest`].
    #[test]
    fn registered_module_count_matches_the_parity_baseline() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
        assert!(
            corpus.is_dir(),
            "ISF corpus missing at {}; the parity guard needs the source tree",
            corpus.display()
        );
        let manifest = legacy_module_manifest();
        let registry = ModuleRegistry::build(Some(&corpus));

        assert_eq!(
            registry.two_d_definitions().len(),
            manifest["counts"]["twoD"].as_u64().unwrap_or(0) as usize,
            "2D modules"
        );
        assert_eq!(
            registry.point10_definitions().len(),
            manifest["counts"]["point10"].as_u64().unwrap_or(0) as usize,
            "Point10 scenes"
        );
        assert_eq!(
            registry.shader_modules().len(),
            manifest["counts"]["shader"].as_u64().unwrap_or(0) as usize,
            "shader modules"
        );
        assert_eq!(
            registry.isf_definitions().len(),
            manifest["counts"]["isf"].as_u64().unwrap_or(0) as usize,
            "ISF corpus"
        );
        let milkdrop_count = manifest["counts"]["milkdrop"]
            .as_u64()
            .expect("milkdrop count in the parity fixture") as usize;
        assert_eq!(
            registry.milkdrop_definitions().len(),
            milkdrop_count,
            "MilkDrop presets"
        );
        assert_eq!(
            registry
                .registered_modules()
                .as_object()
                .map_or(0, serde_json::Map::len),
            36 + 16 + 27 + 354 + milkdrop_count,
            "registered module total (module-parity guard)"
        );
    }

    #[test]
    fn module_registry_matches_the_legacy_manifest() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
        assert!(
            corpus.is_dir(),
            "ISF corpus missing at {}; the parity guard needs the source tree",
            corpus.display()
        );
        let manifest = legacy_module_manifest();
        let registry = ModuleRegistry::build(Some(&corpus));

        let two_d: Vec<String> = registry
            .two_d_definitions()
            .iter()
            .map(|definition| definition.meta.name.to_string())
            .collect();
        let point10: Vec<String> = registry
            .point10_definitions()
            .iter()
            .map(|definition| definition.meta.name.to_string())
            .collect();
        let shader: Vec<String> = registry
            .shader_modules()
            .iter()
            .map(|module| module.name.to_string())
            .collect();
        let isf: Vec<String> = registry
            .isf_definitions()
            .iter()
            .map(|definition| definition.name.clone())
            .collect();
        let milkdrop: Vec<String> = registry
            .milkdrop_definitions()
            .iter()
            .map(|definition| definition.name.clone())
            .collect();

        assert_same_module_names("2D", two_d, manifest_names(&manifest, "twoD"));
        assert_same_module_names("Point10", point10, manifest_names(&manifest, "point10"));
        assert_same_module_names("shader", shader, manifest_names(&manifest, "shader"));
        assert_same_module_names("ISF", isf, manifest_names(&manifest, "isf"));
        assert_same_module_names("MilkDrop", milkdrop, manifest_names(&manifest, "milkdrop"));

        let milkdrop_count = manifest["counts"]["milkdrop"]
            .as_u64()
            .expect("milkdrop count in the parity fixture") as usize;
        assert_eq!(
            registry
                .registered_modules()
                .as_object()
                .map_or(0, serde_json::Map::len),
            36 + 16 + 27 + 354 + milkdrop_count,
            "registered module total (module-parity guard)"
        );
        assert_eq!(
            manifest["counts"]["total"].as_u64(),
            Some(433),
            "manifest total"
        );
    }

    #[test]
    fn every_default_scene_module_resolves_in_the_registry() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
        let manifest = legacy_module_manifest();
        let registry = ModuleRegistry::build(Some(&corpus));

        // The legacy manifest is the authority: a default Scene naming a
        // module legacy cannot register would render differently in engine
        // mode even if the Rust registry happens to know it.
        let legacy: std::collections::HashSet<String> =
            ["twoD", "point10", "shader", "isf", "milkdrop"]
                .iter()
                .flat_map(|group| manifest_names(&manifest, group))
                .collect();

        let mut unresolved = Vec::new();
        let mut not_in_legacy = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for scene in modv_engine::default_scenes::default_scenes() {
            for name in scene.module_names() {
                names.insert(name.to_string());
                if registry.kind(name).is_none() {
                    unresolved.push(format!("{}: {name}", scene.name));
                }
                if !legacy.contains(name) {
                    not_in_legacy.push(format!("{}: {name}", scene.name));
                }
            }
        }

        assert!(
            unresolved.is_empty(),
            "default Scenes reference unregistered modules: {unresolved:?}"
        );
        assert!(
            not_in_legacy.is_empty(),
            "default Scenes reference modules absent from the legacy registry: {not_in_legacy:?}"
        );
        assert!(
            !names.is_empty(),
            "default-Scene resolution guard found no modules to check"
        );
    }
}

#[cfg(test)]
mod quality_prop_tests {
    use super::*;

    fn inline_isf(source: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "modv-quality-inline-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("create inline isf dir");
        std::fs::write(dir.join("Quality Test.fs"), source).expect("write inline isf");
        dir
    }

    const SIMPLE_ISF: &str = r#"/*{
  "NAME": "Quality Test",
  "CREDIT": "test",
  "INPUTS": []
}*/
void main() { gl_FragColor = vec4(1.0); }
"#;

    #[test]
    fn an_isf_module_reports_the_quality_prop_last() {
        let dir = inline_isf(SIMPLE_ISF);
        let registry = ModuleRegistry::build(Some(&dir));
        let names: Vec<&str> = registry
            .prop_names("Quality Test")
            .into_iter()
            .map(|name| Box::leak(name.into_boxed_str()) as &str)
            .collect();
        assert_eq!(names.last(), Some(&"quality"));
        let defaults = registry.default_props("Quality Test").expect("registered");
        assert_eq!(defaults["quality"], json!("global"));
        let schema = registry
            .prop_schema("Quality Test", "quality")
            .expect("quality prop");
        assert_eq!(schema.type_, "enum");
        assert_eq!(schema.default, Some(json!("global")));
        let members = schema.enum_members.as_ref().expect("members");
        assert_eq!(
            members
                .iter()
                .map(|member| member.value.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["global", "full", "balanced", "performance"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_point10_scene_reports_the_quality_prop_last() {
        let registry = ModuleRegistry::builtin();
        let names: Vec<&str> = registry
            .prop_names("Point10 Smoke Room")
            .into_iter()
            .map(|name| Box::leak(name.into_boxed_str()) as &str)
            .collect();
        assert_eq!(names.last(), Some(&"quality"));
        let defaults = registry
            .default_props("Point10 Smoke Room")
            .expect("registered");
        assert_eq!(defaults["quality"], json!("global"));
        let schema = registry
            .prop_schema("Point10 Smoke Room", "quality")
            .expect("quality prop");
        assert_eq!(schema.type_, "enum");
        assert_eq!(schema.default, Some(json!("global")));
        let members = schema.enum_members.as_ref().expect("members");
        assert_eq!(
            members
                .iter()
                .map(|member| member.value.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["global", "full", "balanced", "performance"]
        );
    }
}
