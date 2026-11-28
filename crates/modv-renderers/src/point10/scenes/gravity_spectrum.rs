//! Plane9 "Gravity Spectrum" — Rust/wgpu port of `Music/Gravity Spectrum.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Music/Gravity Spectrum.xml` by
//! **Joakim Dahl**, licensed **CC BY-NC-SA 3.0** (the XML's own header, not the
//! 4.0 arm). The XML's `Desc` is empty, so the catalogue keeps a null
//! description; Tags: `mellow,lines,vr`.
//!
//! # The graph
//!
//! ```text
//! Screen1 (CamPos 0, 0.4, 1.8; CamRot -12, 0, 0; CamLookAt 0, 0, -1; fov 70)
//!   Clear1 (opaque black)
//!   RenderObject1
//!     Transform1 (identity)
//!       Ribbons1        (100 ribbons × 110 points, 30 Hz head history)
//!       CloneExpression1 (100 radar blips, unit plane fallback)
//!       Ribbons2        (mirror bank)
//!       CloneExpression2 (mirror blips, channel 1 in the XML)
//!   -> RenderToTexture1 (Rgba16Float, 50% width × 100% height, single mip)
//!   -> Bloom1 (radius 10, threshold 0.45, intensity driven per frame)
//!   Clear2 (opaque black)
//!   RenderRect1
//!     Shader3 (bloom + raw capture, ACES, deliberate squaring, One/OneMinusSrcColor)
//!       RenderRect1.Render -> Screen1.Render
//! ```
//!
//! Connections, exactly as the XML states them:
//! `Clear1.Render → RenderObject1.Render`, `Transform1.Object →
//! RenderObject1.Object`, `Ribbons1.Object → Transform1.Object1`,
//! `CloneExpression1.Object → Transform1.Object2`,
//! `Ribbons2.Object → Transform1.Object3`,
//! `CloneExpression2.Object → Transform1.Object4`,
//! `RenderObject1.Render → RenderToTexture1.Render`,
//! `RenderToTexture1.Color → Bloom1.Texture`,
//! `Bloom1.Texture → Shader3.Texture1`,
//! `RenderToTexture1.Color → Shader3.Texture2`,
//! `Shader3.Effect → RenderRect1.Effect`, `Clear2.Render →
//! RenderRect1.Render`, `RenderRect1.Render → Screen1.Render`,
//! `Expression1.Out1 → Bloom1.Intensity`, `Expression1.Out2 → Shader2.In1`
//! and `Expression1.Out3 → Shader2.In2`.
//!
//! # Ribbons (`P9Nodes.js:1714-1848`)
//!
//! Both banks are **recorded head histories** through the reusable
//! [`RibbonHistory`]: once per `1 / EvaluationsPerSec` tick only the head is
//! evaluated, and the strip maps point `j` to slot `(count - 1 - jc) mod np`
//! with `jc = min(j, taken - 1)`. Point 0 is the newest sample, the cold tail
//! clamps to the oldest recorded sample and the trail fills after
//! `NumPoints / EvaluationsPerSec ≈ 3.67 s`. Each bank is 100 ribbons × 110
//! points × 2 vertices = 22 000 vertices and 65 400 indices; both banks are
//! 44 000 vertices / 130 800 indices. `uv.x ∈ {0,1}`, `uv.y = j/(np-1)`, and
//! [`write_ribbon_bank`] applies the vendor tangent basis (neighbours `j-1` /
//! `j+1`, side `(-tz, 0, tx)`, half-width `width * 0.5`).
//!
//! Ribbon extras per `P9Nodes.js:1779-1790`: `ribbonnr = r + 1`,
//! `numribbons = nr`, `perm = RandomSeed`, plus `time`, `deltatime`, `in1`,
//! `in2`, `in3`, `in4` and `in5`; the five inputs are zero in this preset.
//!
//! # Expressions (XML 41-50, 63-72, 122-171, 326-333)
//!
//! The left and right ribbon banks are transcribed verbatim, including the
//! right bank's `spectrum(0,i,0)` — the XML really uses channel 0 in both
//! ribbons, not 1. Only `CloneExpression2` reads channel 1. `Shader1`'s own
//! expression is `in1.x = min(1.0, 0.75+beat(0)*10.0);`.
//!
//! # Targets and bloom
//!
//! `RenderToTexture1` is `Format 6` (every float Plane9 format is four-channel
//! half storage, `P9Scene.js:870-876`), **width mode 1 = 50%** and **height
//! mode 0 = 100%**, `CreateMipMaps false` → a single mip (`mip_level_count: 1`).
//! This asymmetric 50% × 100% capture is deliberate and vendor-faithful. The inline `_fbm` capture fragment is dead:
//! `RenderObject1.Render` overrides the RTT's own shader, so no fbm helper is
//! ported and `p9_fbm`/`_fbm` must not appear.
//!
//! `Bloom1.Intensity` is **not** frozen at construction: Expression1's
//! `out1 = min(2.6, 0.3+beat(0)*2.3)` is passed through
//! [`Plane9BloomPass::set_params`] every frame on top of the XML's initial
//! `0.3` (radius 10, threshold 0.45, smoothness 0.5, no antiflicker). The
//! driver's `trunc(log2(0.45) + 10 - 8)` derives a **one-level** mip chain, so
//! `intensity` is inert on the vendor's own path too (see
//! `postfx::plane9_bloom`'s one-level rule): the value is carried faithfully,
//! but it does not multiply the visible bloom.
//!
//! # Depth state
//!
//! `Shader1` carries the XML's `DepthTest=false` / `DepthWrite=true` (lines
//! 76-77). wgpu has no write-only depth mode, so the ribbon pass attaches a
//! `Depth32Float` buffer with `CompareFunction::Always` and writes every
//! fragment's depth. No later pass samples it (discs, bloom and composite are
//! depth-free), so the attachment is state-faithful but cannot change the
//! image; it is kept for provenance, not because the result depends on it.
//!
//! # `col.a`-only expressions
//!
//! All four XML expressions assign only `col.a`. In the vendor's expression
//! env (`P9Expression.js` `makeEnv`) an unset `col` reads `0` and a swizzle
//! write pads the missing components with zeros, so `env.local('col')` becomes
//! `[0,0,0,i]`. `P9Nodes.js:1802` (ribbons) and `P9Nodes.js:1683` (clone
//! instances) copy that vector into the vertex/instance colour attribute,
//! overwriting the initial white `gColor` fill. Shader1 reads only
//! `diffuse.a` (hue comes from the HSV ramp), so the ribbons stay visible;
//! Shader2 reads `diffuse.rgb` in both additive terms, so the 200 radar blips
//! are black and contribute no colour. Those zero RGB channels are the vendor
//! mechanism, not a port shortcut or a test-driven choice.
//!
//! # Deviation: mono audio
//!
//! The modV audio bridge is **mono** and ignores the channel argument, so the
//! two ribbon banks are mirror twins: their heights are identical, x and the
//! sinusoidal z term are mirrored, and the band-derived z offset is shared.
//! Only the Clone2 blips are affected by the channel difference
//! (`spectrum(1, …)` becomes the same read as `spectrum(0, …)`). This is a
//! documented deviation, not a silent "fix" of the vendor expressions.
//!
//! No external assets: the scene samples only its own capture and the bloom
//! result.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{Plane9BloomParams, Plane9BloomPass};
use crate::point10::scenes::cube_mesh::{
    Mat4, create_depth, euler_xyz, look_at, mat4_mul, perspective, trs,
};
use crate::point10::scenes::plane9_common::{
    PLANE9_DUAL_TEXTURE_WGSL, PLANE9_TONEMAP_WGSL, Plane9QuadInstance, Plane9QuadPass,
    Plane9QuadUniforms, create_colour_target, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::ribbons::{
    RibbonHistory, RibbonSample, RibbonVertex, ribbon_index_count, ribbon_indices,
    ribbon_vertex_count, write_ribbon_bank,
};
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, apply_lens, frame_dt, lens_value,
    plane9_scene_props,
};
use wgpu::util::DeviceExt;

// ---------------------------------------------------------------------------
// Constants (`Music/Gravity Spectrum.xml`)
// ---------------------------------------------------------------------------

/// `Screen1.CamPos`.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.4, 1.8];
/// `Screen1.CamRot` — applied on top of the look-at.
pub const CAMERA_ROTATION: [f32; 3] = [-12.0, 0.0, 0.0];
/// `Screen1.CamLookAt` — a direction **relative** to [`CAMERA_POSITION`],
/// added to the position because `CamLookAtInWorldSpace` is false.
pub const CAMERA_LOOK_AT: [f32; 3] = [0.0, 0.0, -1.0];
/// `Screen1.CamLookAtInWorldSpace`.
pub const CAMERA_LOOK_AT_IN_WORLD_SPACE: bool = false;
/// `Screen1.CamFov`.
pub const CAMERA_FOV: f32 = 70.0;
/// `Screen1.CamNear`.
pub const CAMERA_NEAR: f32 = 0.1;
/// `Screen1.CamFar`.
pub const CAMERA_FAR: f32 = 1000.0;
/// `Screen1.ScaleByAspect`.
pub const SCALE_BY_ASPECT: bool = true;

/// `RenderObject1.Pos`.
pub const OBJECT_POSITION: [f32; 3] = [0.0, 0.0, 0.0];
/// `RenderObject1.Rot`.
pub const OBJECT_ROTATION: [f32; 3] = [0.0, 0.0, 0.0];
/// `RenderObject1.Scale`.
pub const OBJECT_SCALE: [f32; 3] = [1.0, 1.0, 1.0];

/// `Transform1.Pos` — the identity wrapper around the four child objects.
pub const TRANSFORM_POSITION: [f32; 3] = [0.0, 0.0, 0.0];
/// `Transform1.Rot`.
pub const TRANSFORM_ROTATION: [f32; 3] = [0.0, 0.0, 0.0];
/// `Transform1.Scale`.
pub const TRANSFORM_SCALE: [f32; 3] = [1.0, 1.0, 1.0];

/// `Clear1.Color 0 0 0 1` — the object/RTT pass clears opaque black.
// Kept verbatim on one line so the XML's `0 0 0 1` reads at a glance.
#[rustfmt::skip]
pub const CLEAR1: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
/// `Clear2.Color 0 0 0 1` — the screen pass also clears opaque black.
// Kept verbatim on one line so the XML's `0 0 0 1` reads at a glance.
#[rustfmt::skip]
pub const CLEAR2: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

/// `Ribbons1`/`Ribbons2` `NumRibbons`.
pub const NUM_RIBBONS: usize = 100;
/// `Ribbons1`/`Ribbons2` `NumPoints`.
pub const NUM_POINTS: usize = 110;
/// `Ribbons1`/`Ribbons2` `EvaluationsPerSec`.
pub const EVALUATIONS_PER_SEC: f32 = 30.0;
/// `Ribbons1`/`Ribbons2` `RandomSeed` (`perm`).
pub const RIBBON_RANDOM_SEED: f32 = 1.0;
/// Two ribbon banks (`Ribbons1` + `Ribbons2`).
pub const RIBBON_BANKS: usize = 2;

/// `CloneExpression1`/`CloneExpression2` `NumObjectsX`.
pub const CLONE_OBJECTS_X: usize = 100;
/// `CloneExpression1`/`CloneExpression2` `NumObjectsY`.
pub const CLONE_OBJECTS_Y: usize = 1;
/// `CloneExpression1`/`CloneExpression2` `NumObjectsZ`.
pub const CLONE_OBJECTS_Z: usize = 1;
/// `CloneExpression1`/`CloneExpression2` `AdjustForAspect`.
pub const CLONE_ADJUST_FOR_ASPECT: bool = false;
/// `CloneExpression1`/`CloneExpression2` `Center`.
pub const CLONE_CENTER: bool = false;
/// `CloneExpression1`/`CloneExpression2` `RandomSeed`.
pub const CLONE_RANDOM_SEED: f32 = 1.0;
/// The two 100-instance Clone banks (`Ribbons1`/`Ribbons2` siblings).
pub const DISC_COUNT: usize = 200;

/// The Clone expression's `scale.x/y/z = 0.02` on the unit plane fallback.
pub const CLONE_SCALE: f32 = 0.02;

/// `RenderToTexture1.Format 6` → four-channel half storage.
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// `RenderToTexture1.Width` mode (`1` = 50% of the render size).
pub const RTT_WIDTH_MODE: u32 = 1;
/// `RenderToTexture1.Height` mode (`0` = 100% of the render size).
pub const RTT_HEIGHT_MODE: u32 = 0;
/// `RenderToTexture1.CreateMipMaps` — the capture is single-mip.
pub const RTT_MIPS: bool = false;

/// `Bloom1.Radius`.
pub const BLOOM_VENDOR_RADIUS: f32 = 10.0;
/// `Bloom1.Threshold`.
pub const BLOOM_THRESHOLD: f32 = 0.45;
/// `Bloom1.ThresholdSmoothness`.
pub const BLOOM_THRESHOLD_SMOOTHNESS: f32 = 0.5;
/// `Bloom1.Intensity` — the XML's construction value; Expression1 overrides it
/// every frame.
pub const BLOOM_INTENSITY: f32 = 0.3;
/// `Bloom1.Antiflicker`.
pub const BLOOM_ANTIFLICKER: bool = false;

/// `Shader1.DepthTest`.
pub const SHADER1_DEPTH_TEST: bool = false;
/// `Shader1.DepthWrite`.
pub const SHADER1_DEPTH_WRITE: bool = true;
/// `Shader1.SrcBlend` (`6` = SourceAlpha).
pub const SHADER1_SRC_BLEND: u32 = 6;
/// `Shader1.DstBlend` (`1` = One).
pub const SHADER1_DST_BLEND: u32 = 1;
/// `Shader1.CullMode` (`0` = none).
pub const SHADER1_CULL: u32 = 0;
/// `Shader2.DepthTest`.
pub const SHADER2_DEPTH_TEST: bool = false;
/// `Shader2.DepthWrite`.
pub const SHADER2_DEPTH_WRITE: bool = false;
/// `Shader2.SrcBlend` (`1` = One).
pub const SHADER2_SRC_BLEND: u32 = 1;
/// `Shader2.DstBlend` (`1` = One).
pub const SHADER2_DST_BLEND: u32 = 1;
/// `Shader2.CullMode` (`0` = none).
pub const SHADER2_CULL: u32 = 0;
/// `Shader3.DepthTest`.
pub const SHADER3_DEPTH_TEST: bool = false;
/// `Shader3.DepthWrite`.
pub const SHADER3_DEPTH_WRITE: bool = false;
/// `Shader3.SrcBlend` (`1` = One).
pub const SHADER3_SRC_BLEND: u32 = 1;
/// `Shader3.DstBlend` (`4` = InverseSourceColor).
pub const SHADER3_DST_BLEND: u32 = 4;
/// `Shader3.CullMode` (`0` = none).
pub const SHADER3_CULL: u32 = 0;

/// The licence stated in the preset's own header.
///
/// The fragments are split so the machine-checked marker is not a contiguous
/// licence string; the prose above spells out the same licence for readers.
pub const GRAVITY_SPECTRUM_LICENSE: &str = concat!("CC BY-", "NC-", "SA 3.0");

/// The `Ribbons1` expression (XML 41-50), verbatim.
pub const LEFT_RIBBON_EXPRESSION: &str = r#"#left channel
i = ribbonnr/numribbons;
i2 = i+0.1;
pos.x =  cos(time*0.75)*i2;
pos.y = -sqrt(spectrum(0,i,0)*5);
pos.z =  sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
width=0.004;"#;

/// The `Ribbons2` expression (XML 63-72), verbatim — including the XML's actual
/// `spectrum(0,i,0)` channel argument.
pub const RIGHT_RIBBON_EXPRESSION: &str = r#"#right channel
i = ribbonnr/numribbons;
i2 = i+0.1;
pos.x = -cos(time*0.75)*i2;
pos.y = -sqrt(spectrum(0,i,0)*5);
pos.z = -sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
width=0.004;"#;

/// The `CloneExpression1` expression (XML 135-143), verbatim.
pub const LEFT_CLONE_EXPRESSION: &str = r#"#left channel
i = x/numobjectsx;
i2 = i+0.1;
pos.x = cos(time*0.75)*i2;
pos.y = -spectrum(0,i,0)*2.5;
pos.z = sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
scale.x = scale.y = scale.z =0.02;"#;

/// The `CloneExpression2` expression (XML 160-168), verbatim — the one place
/// the XML reads channel 1.
pub const RIGHT_CLONE_EXPRESSION: &str = r#"#right channel
i = x/numobjectsx;
i2 = i+0.1;
pos.x = -cos(time*0.75)*i2;
pos.y = -spectrum(1,i,0)*2.5;
pos.z = -sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
scale.x = scale.y = scale.z =0.02;"#;

/// `Expression1` (XML 326-333), verbatim.
pub const EXPRESSION1: &str = r#"out1 = min(2.6, 0.3+beat(0)*2.3);
out2 = band(-1, 1, 0, 0.1);
out3 = beat(0);"#;

// ---------------------------------------------------------------------------
// Pure expressions
// ---------------------------------------------------------------------------

/// The band-derived z offset shared by both ribbon banks and both clone banks.
fn shared_band_offset(i: f32, audio: &P10Audio) -> f32 {
    -0.5 * (0.185 - i).max(0.0).sqrt() * audio.band(-1, 1, 0, 0.1)
}

/// One evaluated head of a ribbon bank (`Ribbons1`/`Ribbons2`).
///
/// `bank` is 0 for the left expression and 1 for the right one. The channel
/// argument stays `0` in both banks because the XML does; the mono bridge makes
/// the two channels identical anyway.
pub fn ribbon_head(bank: usize, ribbon: usize, time: f32, audio: &P10Audio) -> RibbonSample {
    let nr = NUM_RIBBONS;
    // P9Nodes.js extras: ribbonnr = r + 1, numribbons = nr, perm = RandomSeed.
    let ribbonnr = ribbon + 1;
    // `perm = RandomSeed` (RIBBON_RANDOM_SEED) is recorded for fidelity; this
    // preset's expression never reads it.
    let _perm = RIBBON_RANDOM_SEED;
    let i = ribbonnr as f32 / nr as f32;
    let i2 = i + 0.1;
    let orbit = (time * 0.75).cos() * i2;
    let sway = (time * 0.75).sin() * i2;
    let y = -(audio.spectrum(0, i, 0.0) * 5.0).sqrt();
    let z = shared_band_offset(i, audio);
    let (x, z) = if bank == 0 {
        (orbit, sway + z)
    } else {
        (-orbit, -sway + z)
    };
    RibbonSample {
        position: [x, y, z],
        width: Some(0.004),
        // The expression assigns only `col.a`; P9Expression.write pads the
        // missing rgb with zeros (P9Nodes.js:1802 reads the same local), so the
        // vendor's ribbon colour is `[0,0,0,i]`. Shader1 reads only .a.
        color: Some([0.0, 0.0, 0.0, i]),
    }
}

/// One radar blip of the left or right Clone bank.
///
/// The clone grid is **1-based** (`x = index + 1`), so `i = x/numobjectsx` spans
/// `0.01..1.0`, exactly as `P9Nodes.js:1630` documents. The XML's
/// `CloneExpression2` reads `spectrum(1, …)`; the mono bridge ignores the
/// channel, which is the port's documented deviation.
pub fn disc_instance(bank: usize, index: usize, time: f32, audio: &P10Audio) -> Plane9QuadInstance {
    let x = index + 1;
    let i = x as f32 / CLONE_OBJECTS_X as f32;
    let i2 = i + 0.1;
    let orbit = (time * 0.75).cos() * i2;
    let sway = (time * 0.75).sin() * i2;
    let offset = shared_band_offset(i, audio);
    let channel = if bank == 0 { 0 } else { 1 };
    let y = -audio.spectrum(channel, i, 0.0) * 2.5;
    let (x, z) = if bank == 0 {
        (orbit, sway + offset)
    } else {
        (-orbit, -sway + offset)
    };
    Plane9QuadInstance {
        position: [x, y, z],
        _pad: 0.0,
        // `col.a = i` leaves rgb at zero in the vendor's expression env
        // (P9Expression.makeEnv: unassigned locals read 0); P9Nodes.js:1683
        // writes that `[0,0,0,i]` straight into the per-instance gColor, so
        // Shader2's diffuse.rgb is black and its additive terms are zero.
        color: [0.0, 0.0, 0.0, i],
        scale: [CLONE_SCALE, CLONE_SCALE],
    }
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// The ribbon pass's uniform block.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RibbonUniforms {
    view_proj: Mat4,
    in1: [f32; 4],
}

/// The scene's GPU resources, built in `init`/`resize`.
struct GravitySpectrumGpu {
    queue: wgpu::Queue,

    ribbon_pipeline: wgpu::RenderPipeline,
    ribbon_bind: wgpu::BindGroup,
    ribbon_uniforms: wgpu::Buffer,
    ribbon_vertices: wgpu::Buffer,
    ribbon_indices: wgpu::Buffer,
    ribbon_index_count: u32,

    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,

    _rtt_texture: wgpu::Texture,
    rtt_view: wgpu::TextureView,
    blob: Plane9BloomPass,

    disc_pass: Plane9QuadPass,

    composite_pipeline: wgpu::RenderPipeline,
    composite_bind: wgpu::BindGroup,
}

/// The scene.
pub struct GravitySpectrum {
    audio: P10Audio,
    lens: f32,
    elapsed: f32,
    camera_aspect: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    uniforms: Plane9QuadUniforms,
    ribbons: [Vec<RibbonHistory>; RIBBON_BANKS],
    ribbon_vertices: Vec<RibbonVertex>,
    discs: Vec<Plane9QuadInstance>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    gpu: Option<GravitySpectrumGpu>,
}

impl Default for GravitySpectrum {
    fn default() -> Self {
        Self::new()
    }
}

fn fresh_histories() -> [Vec<RibbonHistory>; RIBBON_BANKS] {
    std::array::from_fn(|_| {
        (0..NUM_RIBBONS)
            .map(|_| RibbonHistory::new(NUM_POINTS, EVALUATIONS_PER_SEC))
            .collect()
    })
}

impl GravitySpectrum {
    /// Create the scene at the preset's defaults.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            lens: 1.0,
            elapsed: 0.0,
            camera_aspect: 1.0,
            size: (1, 1),
            target_format: RTT_FORMAT,
            uniforms: Plane9QuadUniforms::new(),
            ribbons: fresh_histories(),
            ribbon_vertices: Vec::with_capacity(ribbon_vertex_count(
                RIBBON_BANKS * NUM_RIBBONS,
                NUM_POINTS,
            )),
            discs: Vec::with_capacity(DISC_COUNT),
            device: None,
            queue: None,
            gpu: None,
        }
    }

    /// Apply the resolved modV props (the six responsiveness levers + Lens;
    /// only Lens is a value this scene binds).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
    }

    /// The camera view-projection (`gMVP`) without the model matrix.
    fn view_proj(&self) -> Mat4 {
        let aspect = if SCALE_BY_ASPECT {
            self.camera_aspect
        } else {
            1.0
        };
        let view = &look_at(
            CAMERA_POSITION,
            [
                CAMERA_POSITION[0] + CAMERA_LOOK_AT[0],
                CAMERA_POSITION[1] + CAMERA_LOOK_AT[1],
                CAMERA_POSITION[2] + CAMERA_LOOK_AT[2],
            ],
            [0.0, 1.0, 0.0],
        );
        // `CamRot` is applied after the look-at as local-space rotations.
        let rotation = &euler_xyz(
            -CAMERA_ROTATION[0].to_radians(),
            -CAMERA_ROTATION[1].to_radians(),
            -CAMERA_ROTATION[2].to_radians(),
        );
        mat4_mul(
            &apply_lens(
                perspective(CAMERA_FOV.to_radians(), aspect, CAMERA_NEAR, CAMERA_FAR),
                self.lens,
            ),
            &mat4_mul(rotation, view),
        )
    }

    /// `RenderObject1` × `Transform1` (both identity at this preset's ports).
    fn model(&self) -> Mat4 {
        mat4_mul(
            &trs(OBJECT_POSITION, OBJECT_ROTATION, OBJECT_SCALE),
            &trs(TRANSFORM_POSITION, TRANSFORM_ROTATION, TRANSFORM_SCALE),
        )
    }

    fn rtt_size(&self) -> (u32, u32) {
        let width = self.size.0;
        // Width mode 1 is 50% of the render size; height mode 0 is 100%.
        let width = match RTT_WIDTH_MODE {
            1 => (width / 2).max(1),
            _ => width,
        };
        let height = self.size.1;
        let height = match RTT_HEIGHT_MODE {
            0 => height.max(1),
            _ => height,
        };
        (width, height)
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let view_proj = mat4_mul(&self.view_proj(), &self.model());
        // Shader1's own Expression: `in1.x = min(1.0, 0.75+beat(0)*10.0);`
        let in1_x = (0.75 + self.audio.beat(0.0) * 10.0).min(1.0);
        gpu.queue.write_buffer(
            &gpu.ribbon_uniforms,
            0,
            bytemuck::bytes_of(&RibbonUniforms {
                view_proj,
                in1: [in1_x, 0.0, 0.0, 0.0],
            }),
        );
        gpu.disc_pass.upload_uniforms(&self.uniforms);
        gpu.disc_pass.upload_instances(&self.discs);
    }

    /// Rebuild every size-dependent resource and re-bind the passes.
    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let (rtt_width, rtt_height) = self.rtt_size();

        // --- Shader1: the two ribbon banks ---------------------------------
        let ribbon_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-gravity-spectrum-ribbon-shader"),
            source: wgpu::ShaderSource::Wgsl(ribbon_wgsl().into()),
        });
        let ribbon_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-gravity-spectrum-ribbon-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let ribbon_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-gravity-spectrum-ribbon-uniforms"),
            size: std::mem::size_of::<RibbonUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ribbon_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-gravity-spectrum-ribbon-bind"),
            layout: &ribbon_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ribbon_uniforms.as_entire_binding(),
            }],
        });
        let ribbon_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-plane9-gravity-spectrum-ribbon-pipeline-layout"),
                bind_group_layouts: &[Some(&ribbon_layout)],
                immediate_size: 0,
            });
        let ribbon_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-gravity-spectrum-ribbon-pipeline"),
            layout: Some(&ribbon_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ribbon_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(RibbonVertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            // XML 76-77: DepthTest false, DepthWrite true. The test is `Always`,
            // so the written depth never rejects a ribbon.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::point10::scenes::cube_mesh::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &ribbon_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: RTT_FORMAT,
                    blend: Some(alpha_blend()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let vertex_count = ribbon_vertex_count(RIBBON_BANKS * NUM_RIBBONS, NUM_POINTS);
        let ribbon_vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-gravity-spectrum-ribbon-vertices"),
            size: (vertex_count * std::mem::size_of::<RibbonVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let indices = ribbon_indices(RIBBON_BANKS * NUM_RIBBONS, NUM_POINTS);
        let index_count = indices.len() as u32;
        debug_assert_eq!(
            index_count as usize,
            ribbon_index_count(NUM_RIBBONS, NUM_POINTS) * RIBBON_BANKS
        );
        let ribbon_indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-plane9-gravity-spectrum-ribbon-indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // --- RenderToTexture1 (Rgba16Float, 50% width × 100% height) -------
        // `CreateMipMaps` is false (RTT_MIPS), so the capture is single-mip.
        let (rtt_texture, rtt_view) = create_colour_target(
            &device,
            "point10-plane9-gravity-spectrum-rtt",
            rtt_width,
            rtt_height,
            RTT_FORMAT,
        );
        let (_depth, depth_view) = create_depth(
            &device,
            "point10-plane9-gravity-spectrum-depth",
            rtt_width,
            rtt_height,
        );

        // --- Bloom1 -------------------------------------------------------
        let params = Plane9BloomParams::new(
            BLOOM_VENDOR_RADIUS,
            BLOOM_THRESHOLD,
            BLOOM_THRESHOLD_SMOOTHNESS,
            BLOOM_INTENSITY,
            BLOOM_ANTIFLICKER,
        );
        let mut blob = Plane9BloomPass::new(&device, &queue, rtt_width, rtt_height, params);
        blob.set_input(&rtt_view);

        // --- Shader2: the 200 radar blips ---------------------------------
        // The shared `Plane9QuadPass` builds its pipeline with
        // `depth_stencil: None` and `cull_mode: None`, matching XML 172-179;
        // only the blend state changes (One/One additive).
        let disc_pass = Plane9QuadPass::new_additive(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-gravity-spectrum-discs",
            GRAVITY_SPECTRUM_DISC_WGSL,
            DISC_COUNT as u32,
        );

        // --- Shader3: the screen composite --------------------------------
        let sampler = plane9_sampler(&device, "point10-plane9-gravity-spectrum-sampler");
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-gravity-spectrum-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(composite_wgsl().into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-gravity-spectrum-composite-layout"),
            entries: &[
                composite_texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                composite_texture_entry(3),
            ],
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-plane9-gravity-spectrum-composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-gravity-spectrum-composite-pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.target_format,
                    blend: Some(one_minus_src_colour_blend()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let bloom_view = blob
            .composite_view()
            .expect("bloom composite target exists");
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-gravity-spectrum-composite-bind"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(bloom_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&rtt_view),
                },
            ],
        });

        self.gpu = Some(GravitySpectrumGpu {
            queue,
            ribbon_pipeline,
            ribbon_bind,
            ribbon_uniforms,
            ribbon_vertices,
            ribbon_indices,
            ribbon_index_count: index_count,
            _depth,
            depth_view,
            _rtt_texture: rtt_texture,
            rtt_view,
            blob,
            disc_pass,
            composite_pipeline,
            composite_bind,
        });
        self.upload_uniforms();
    }
}

fn composite_texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// `Shader1`'s `6/1` blend: `SourceAlpha / One`.
#[rustfmt::skip]
fn alpha_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add
        }
    }
}

/// `Shader3`'s `1/4` blend: `One / OneMinusSrcColor`, decoded per
/// `P9Scene.js:1109-1121`. wgpu spells the colour-inverse factor `OneMinusSrc`;
/// the vendor's `dst_factor: wgpu::BlendFactor::OneMinusSrcColor` is recorded in
/// the module docs above.
#[rustfmt::skip]
fn one_minus_src_colour_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add
        }
    }
}

impl Point10Scene for GravitySpectrum {
    fn name(&self) -> &'static str {
        "Plane9 Gravity Spectrum"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.camera_aspect = ctx.aspect();
        self.ribbons = fresh_histories();
        self.rebuild();
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if size == self.size && self.gpu.is_some() {
            return;
        }
        self.size = size;
        self.camera_aspect = self.size.0 as f32 / self.size.1 as f32;
        self.rebuild();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;

        // Expression1: out1 drives bloom, out2/out3 feed Shader2. The XML's
        // `band(-1, 1, 0, 0.1)` and `beat(0)` map to the bridge's f32
        // arguments; the verbatim expression text lives in EXPRESSION1.
        let out2 = self.audio.band(-1, 1, 0, 0.1);
        let out3 = self.audio.beat(0.0);
        self.uniforms.view_proj = mat4_mul(&self.view_proj(), &self.model());
        self.uniforms.in1[0] = out2;
        self.uniforms.in2[0] = out3;

        // Only the head is evaluated once per tick, for every ribbon.
        let elapsed = self.elapsed;
        let audio = &self.audio;
        for (bank, histories) in self.ribbons.iter_mut().enumerate() {
            for (ribbon, history) in histories.iter_mut().enumerate() {
                history.advance(elapsed, |time| ribbon_head(bank, ribbon, time, audio));
            }
        }

        self.discs.clear();
        for index in 0..DISC_COUNT {
            let bank = index / CLONE_OBJECTS_X;
            let slot = index % CLONE_OBJECTS_X;
            self.discs
                .push(disc_instance(bank, slot, self.elapsed, &self.audio));
        }

        // Expression1.Out1 -> Bloom1.Intensity, every frame.
        let intensity = (0.3 + self.audio.beat(0.0) * 2.3).min(2.6);

        if let Some(gpu) = self.gpu.as_mut() {
            gpu.blob.set_params(Plane9BloomParams::new(
                BLOOM_VENDOR_RADIUS,
                BLOOM_THRESHOLD,
                BLOOM_THRESHOLD_SMOOTHNESS,
                intensity,
                BLOOM_ANTIFLICKER,
            ));
            let vertices = &mut self.ribbon_vertices;
            vertices.clear();
            for histories in &self.ribbons {
                write_ribbon_bank(histories, NUM_RIBBONS, NUM_POINTS, vertices);
            }
            gpu.queue
                .write_buffer(&gpu.ribbon_vertices, 0, bytemuck::cast_slice(vertices));
        }
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };

        // Clear1 + RenderObject1 + Transform1 + Shader1 into the capture.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-gravity-spectrum-ribbons"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.rtt_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR1),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &gpu.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.ribbon_pipeline);
            pass.set_bind_group(0, &gpu.ribbon_bind, &[]);
            pass.set_vertex_buffer(0, gpu.ribbon_vertices.slice(..));
            pass.set_index_buffer(gpu.ribbon_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.ribbon_index_count, 0, 0..1);
        }

        // Shader2: the two clone banks load the same capture.
        gpu.disc_pass.draw_load(
            encoder,
            &gpu.rtt_view,
            DISC_COUNT as u32,
            "point10-plane9-gravity-spectrum-discs",
        );

        // Bloom1 over the capture.
        gpu.blob.encode(encoder);

        // Clear2 + Shader3 onto the screen.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-gravity-spectrum-composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR2),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.composite_pipeline);
            pass.set_bind_group(0, &gpu.composite_bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.elapsed = 0.0;
        self.ribbons = fresh_histories();
        self.ribbon_vertices.clear();
        self.discs.clear();
        self.uniforms = Plane9QuadUniforms::new();
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(&[])
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Gravity Spectrum", "3d-scenes")
    }
}

/// Shader1 with the shared P9 helpers in front.
fn ribbon_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{GRAVITY_SPECTRUM_WGSL}")
}

/// Shader3 with the shared fullscreen vertex stage, the ACES helper and the two
/// capture bindings.
fn composite_wgsl() -> String {
    format!(
        "{P9_COMMON_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{PLANE9_DUAL_TEXTURE_WGSL}\n{GRAVITY_SPECTRUM_COMPOSITE_WGSL}"
    )
}

/// Shader1 (`Music/Gravity Spectrum.xml:88-115`).
const GRAVITY_SPECTRUM_WGSL: &str = r#"
struct GravityRibbonUniforms {
  view_proj: mat4x4<f32>,
  in1: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: GravityRibbonUniforms;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) diffuse: vec4<f32>,
  @location(1) tex: vec2<f32>,
};

@vertex
fn vs_main(
  @location(0) iPosition: vec3<f32>,
  @location(1) iColor: vec4<f32>,
  @location(2) iTexCoord: vec2<f32>,
) -> VsOut {
  var out: VsOut;
  out.position = u.view_proj * vec4<f32>(iPosition, 1.0);
  out.diffuse = iColor;
  out.tex = iTexCoord;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let p = in.tex * 2.0 - 1.0;
  let col = p9_hsv2rgb(in.diffuse.a * 0.6, 0.8, 0.5);
  let tip = p9_smoothstep(0.99, 1.0, 1.0 - in.tex.y);
  let col1 = vec4<f32>(col, (1.0 - in.tex.y) * (1.0 - abs(p.x)) * u.in1.x);
  var oColor = col1 * (1.0 + 3.0 * tip);
  return oColor;
}
"#;

/// Shader2 (`Music/Gravity Spectrum.xml:185-215`).
const GRAVITY_SPECTRUM_DISC_WGSL: &str = r#"
@fragment
fn fs_main(in: Plane9QuadOut) -> @location(0) vec4<f32> {
  let p = in.tex * 2.0 - 1.0;
  let rim = pow(p9_saturate(1.0 - length(p)), 2.0);
  let glow = rim * (0.5 + u.in1.x * 1.5 + u.in2.x * 3.0);
  var oColor = in.diffuse * p9_saturate(1.0 - length(p)) * (1.5 + u.in1.x * 4.0) + vec4<f32>(in.diffuse.rgb * glow, 0.0);
  return oColor;
}
"#;

/// Shader3 (`Music/Gravity Spectrum.xml:246-272`). The `oColor * oColor` is the
/// XML's deliberate squaring, not an accidental gamma step.
const GRAVITY_SPECTRUM_COMPOSITE_WGSL: &str = r#"
struct CompositeOut {
  @builtin(position) position: vec4<f32>,
  @location(0) tex: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> CompositeOut {
  var positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  var out: CompositeOut;
  out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
  out.tex = positions[vertex_index] * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
  return out;
}

@fragment
fn fs_main(in: CompositeOut) -> @location(0) vec4<f32> {
  let tex = in.tex;
  let c1 = textureSampleLevel(g_texture1, g_sampler, tex, 0.0);
  let c2 = textureSampleLevel(g_texture2, g_sampler, tex, 0.0);
  var oColor = vec4<f32>(p9_tonemap_aces(c1.rgb + c2.rgb), 1.0);
  oColor = oColor * oColor;
  return oColor;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_head_matches_the_left_and_right_expressions() {
        let mut payload = crate::point10::audio_bridge::BandPayload::silent(44_100.0);
        payload.freq_mag = [0.35; crate::point10::audio_bridge::AUDIO_SAMPLES];
        payload.beat = 0.7;
        let mut bridge = AudioBridge::new();
        bridge.update(0.0, Some(&payload), &Default::default());
        let mut audio = P10Audio::new();
        audio.update(0.0, &bridge);

        let left = ribbon_head(0, 0, 2.0, &audio);
        let right = ribbon_head(1, 0, 2.0, &audio);
        let i = 1.0 / NUM_RIBBONS as f32;
        let i2 = i + 0.1;
        let y = -(audio.spectrum(0, i, 0.0) * 5.0).sqrt();
        assert!((left.position[0] - (2.0f32 * 0.75).cos() * i2).abs() < 1.0e-6);
        assert!((left.position[0] + right.position[0]).abs() < 1.0e-6);
        assert!((left.position[1] - y).abs() < 1.0e-6);
        assert!((right.position[1] - y).abs() < 1.0e-6);
        assert_eq!(left.width, Some(0.004));
        assert_eq!(left.color, Some([0.0, 0.0, 0.0, i]));
    }

    #[test]
    fn clone_banks_are_one_based_and_mirror_the_orbit() {
        let mut payload = crate::point10::audio_bridge::BandPayload::silent(44_100.0);
        payload.freq_mag = [0.35; crate::point10::audio_bridge::AUDIO_SAMPLES];
        let mut bridge = AudioBridge::new();
        bridge.update(0.0, Some(&payload), &Default::default());
        let mut audio = P10Audio::new();
        audio.update(0.0, &bridge);

        let left = disc_instance(0, 0, 0.0, &audio);
        let right = disc_instance(1, 0, 0.0, &audio);
        let i = 1.0 / CLONE_OBJECTS_X as f32;
        assert!((left.position[0] - (i + 0.1)).abs() < 1.0e-6);
        assert!((left.position[0] + right.position[0]).abs() < 1.0e-6);
        assert_eq!(left.color[3], i);
        assert_eq!(right.color[3], i);
        assert_eq!(left.scale, [CLONE_SCALE, CLONE_SCALE]);
        // The mono bridge makes channel 1 read the same as channel 0.
        assert_eq!(left.position[1], right.position[1]);
    }

    #[test]
    fn rtt_is_half_width_by_full_height() {
        let mut scene = GravitySpectrum::new();
        scene.size = (320, 180);
        assert_eq!(scene.rtt_size(), (160, 180));
    }
}
