//! HollowCube — Rust/wgpu port of
//! `src/renderer/src/application/point10/scenes/HollowCube.js`.
//!
//! The JS scene is a **WebGL** `BaseScene` (not WebGPU): the Plane9 "Hollow
//! Cube" preset (Joakim Dahl, CC BY-NC-SA 4.0) as 15 000 instanced 0.01-unit
//! cubes on the six faces of a 50x50x50 lattice. `Shader1` positions every cube
//! from its instance id (the hollow-shell mapping), rotates it by a random
//! axis-angle quaternion and pulses it outward along the lattice normal; the
//! fragment is lit with Plane9's half-Lambert/Blinn-Phong pair. A separate
//! `Expression1` rotates the whole object on three noise-driven axes. The cube
//! chain is captured into a Plane9 Format 5 (`GL_RGBA16F`) target and `Shader2`
//! composites it with Plane9 bloom through the [`Composer`].
//!
//! # Ported exactly
//!
//! * the instance placement (`HollowCube.js:137-204`): `o` from
//!   `mod(id, 50)` / `mod(id / 50, 50)` and the six face branches, `p = o/50`,
//!   `up = (p - 0.5) * 0.5`, `pulse = _cubicpulse(mod(p.x+p.y+p.z+t, 1.5),
//!   0.6, 0.4)` with `t = gIn2.x * 0.1`, the dead interior branch, the
//!   axis-angle rotation `pulse * 6 * gIn1.z`, the constant `nv` noise
//!   direction (`d = 0.0001` in the JS — see the shader note), the
//!   `up * gIn2.y` offset and the per-instance `vDiffuse` (HSV 0/0.9/0.8 times
//!   `0.1 + 0.9 * _rand(vec2(id))`);
//! * the fragment (`:207-264`): `vDiffuse * gIn1.y`, the two
//!   `_lightBlinnPhongHalfLambert` terms at `specularHardness = 10`, the
//!   `(0, 1, 0)` sky light `(0.5, 0.8, 1.0)` * 1.15 at 0.04 and the
//!   `(0, -1, 0)` ground light `(0.8, 0, 0)` / white specular at 0.025;
//! * `Shader2` (`:279-298`): `mix(bg.rgb, _tonemapACES(c2.rgb), c2.a)`, alpha
//!   `c2.a` (the cube capture's alpha). `PreviousLayer1.Color` is transparent
//!   black here: this architecture has no previous layer, and the compositor's
//!   own alpha blend over the layers below is the faithful equivalent;
//! * `Shader1`'s own Expression: a private `perm` on damping-0 bass,
//!   `perm += min(0.5, band + 0.01) * 40 * dt` (0.1 s cap), feeding `gIn2.x`.
//!   The level reads are damping-1 bass (Point10's runtime override moves the
//!   explosion from damping 2 to damping 1, `P9Overrides.js`),
//!   `in1 = (min(1.2, l*2.2)*0.6, min(0.55, l*4 + 0.4), min(1, l*1.5))`,
//!   `in2 = (perm, 1 + min(0.2, l))`;
//! * `Expression1` (`:351-361`): a second phase,
//!   `perm += min(0.15, band + 0.02) * 0.08 * dt`, then
//!   `out*.x/y/z = noise2(perm, 0|1|2) * (1 + band * 0.1) * 500` degrees of
//!   whole-object rotation;
//! * the camera (`:44-51`): `PerspectiveCamera(50, aspect, 0.01, 100)` at
//!   `(0, 0, 2.2)` looking at `(0, 0, -1)`;
//! * the bloom: Plane9 `Radius 6, Threshold 0, ThresholdSmoothness 0.5,
//!   Intensity 0.51, Antiflicker false` via [`Plane9BloomParams`];
//! * the prop binder (`sceneProps.js:312-317`): `spinScale = spinSpeed`,
//!   `explodeScale = explode`, `colorScale = color`, `bloomScale = bloom`.
//!
//! # Replaced three.js machinery
//!
//! | three.js | Rust |
//! |----------|------|
//! | `InstancedBufferGeometry` + `aInstance` (15 000 floats) | `@builtin(instance_index)` — no attribute buffer |
//! | `BoxGeometry(0.01)` | [`build_box`](crate::point10::scenes::cube_mesh::build_box) |
//! | `projectionMatrix * modelViewMatrix` (identity model) | view-projection + model-rotation uniform |
//! | `WebGLRenderTarget` cube capture | one `Rgba16Float` target (Plane9 Format 5) |
//! | `EffectComposer` + bloom pass | [`Composer`] driven by [`Plane9BloomParams`] |
//! | `ShaderPass` + `OutputPass` | the port's `fs_main` composite + [`Composer`]'s output pass |
//! | `_cubicpulse`, `_hsv2rgb`, `_rand`, `_rand3`, `_noise`, `_rotateVector`, `_axisAngleToQuaternion` | [`P9_COMMON_WGSL`] + [`P9_SHADING_WGSL`] (`p9_rand3`) |
//!
//! # Deviations
//!
//! * **`_rand3(vec2)` is local.** `P9Glsl.js:44` declares a `vec2` overload of
//!   `_rand3` that the shared WGSL subset does not carry (batch 2 only needed
//!   the scalar form). The scene keeps it as `hollow_rand3_vec2`, verbatim.
//! * **`nv` is computed per vertex, not per instance.** The JS writes
//!   `_noise(vec3(d * 50, axis, 0))` with `d = 0.0001`, which is a *constant*
//!   (the instance id never enters), so this is the same number with the same
//!   `sign(up.axis)` multiplier.
//! * **`PreviousLayer1.Color` is transparent black.** The vendor `Texture3`
//!   read is dead output; the compositor's alpha blend over the layers below
//!   carries the transparency instead (`CLEAR_ALPHA = 0`).
//! * `_tonemapACES` is not part of the shared WGSL subset yet; the scene carries
//!   the exact Plane9 formula (`P9Glsl.js:454-462`).
//! * `Renderer.setClearColor(0, 0)` for the cube capture is reproduced by the
//!   render-pass clear; the final `Shader2` alpha is the cube capture's, so
//!   empty space stays transparent.
//! * `P9Audio` is `plane9/P9Audio.js` in the JS; the port reads [`P10Audio`]
//!   over the shared [`AudioBridge`]. `Shader1`'s phase reads bass at damping
//!   lane 0 (`band(-1, 0, 0, 0.1)`); its other lanes and all of `Expression1`
//!   read damping lane 1 (`band(-1, 1, 0, fallback)`).
//!
//! [`Plane9BloomParams`]: crate::point10::postfx::Plane9BloomParams
//! [`Composer`]: crate::point10::postfx::Composer

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{Composer, Plane9BloomParams, ToneMapping};
use crate::point10::scenes::cube_mesh::{
    Mat4, MeshGpu, build_box, create_depth, depth_state, euler_xyz, look_at, mat4_mul, perspective,
    translation,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::p9_shading::P9_SHADING_WGSL;
use crate::point10::shared::random::value_noise2;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, read_scene_props,
    scene_props_with_levers,
};

// ---------------------------------------------------------------------------
// Constants (`HollowCube.js:39-69`)
// ---------------------------------------------------------------------------

/// `INSTANCE_COUNT` — `MeshInstancer1` NumInstances.
pub const INSTANCE_COUNT: u32 = 15_000;
/// `LATTICE_COUNT` — the 50x50x50 shell.
pub const LATTICE_COUNT: u32 = 50;
/// `CUBE_SIZE`.
pub const CUBE_SIZE: f32 = 0.01;

/// `CAMERA.fov`.
pub const CAMERA_FOV: f32 = 50.0;
/// `CAMERA.near`.
pub const CAMERA_NEAR: f32 = 0.01;
/// `CAMERA.far`.
pub const CAMERA_FAR: f32 = 100.0;
/// `CAMERA.position`.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 2.2];
/// `CAMERA.lookAt`.
pub const CAMERA_LOOK_AT: [f32; 3] = [0.0, 0.0, -1.0];

/// `BLOOM.Intensity` (Plane9 `Intensity`).
pub const BLOOM_STRENGTH: f32 = 0.51;
/// `BLOOM.Radius` (Plane9 `Radius 6`).
pub const BLOOM_RADIUS: f32 = 6.0;
/// `BLOOM.Threshold`.
pub const BLOOM_THRESHOLD: f32 = 0.0;
/// `BLOOM.ThresholdSmoothness`.
pub const BLOOM_SMOOTHNESS: f32 = 0.5;

/// `CLEAR_ALPHA` — Hollow Cube composites transparently over the layers below.
pub const CLEAR_ALPHA: f64 = 0.0;

/// `MAX_DT` — Expression1's own 0.1 s step cap.
pub const MAX_DT: f32 = 0.1;

/// The `Shader2` composite the cube capture is tonemapped with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HollowCubeComposite {
    /// `mix(bg.rgb, ACES(c2.rgb), c2.a)` — Plane9 Hollow Cube, Hollow Cuboid
    /// and Hollow Cube Layers.
    Mix,
    /// `c2.rgb * 0.06 + ACES(c1.rgb * c1.rgb)` — Hollow Cube Blur.
    Blur,
}

/// One preset of the `Plane9 Hollow Cube` family.
///
/// The four registered scenes share the lattice, the two lights, the cube
/// shader and the bloom chain; only the camera, the Expression1 tuning, the
/// `Bloom1` drive and the `Shader2` composite differ. Carrying them as data on
/// one [`HollowCube`] avoids a copy of the whole port per preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HollowCubeVariant {
    /// Registry name (`"Plane9 Hollow Cube"`, `"Point10 Hollow Cuboid"`, …).
    pub name: &'static str,
    /// `CAMERA.position`.
    pub camera_position: [f32; 3],
    /// `CAMERA.lookAt`.
    pub camera_look_at: [f32; 3],
    /// Whether `lookAt` is a world-space point rather than one relative to
    /// `camera_position`.
    pub camera_world_space: bool,
    /// `RenderObject1.position`.
    pub model_translation: [f32; 3],
    /// `Shader1`'s `pulse * <angle>` rotation.
    pub pulse_angle: f32,
    /// `Expression1`'s `min(1, band * <weight>)` scaling of `in1.z`.
    pub in1_z_weight: f32,
    /// `Expression1`'s `min(<min>, band + 0.02)` perm cap.
    pub perm_min: f32,
    /// `Expression1`'s `(1 + band * <gain>)` orbit amplitude gain.
    pub noise_gain: f32,
    /// `Expression1`'s degree scale for the noise orbit.
    pub orbit_scale: f32,
    /// Whether `Expression1`'s `out4` drives the `Bloom1` intensity.
    pub out4_bloom: bool,
    /// Whether `MeshInstancer` puts the lattice cell in each instance matrix.
    ///
    /// `P9Nodes.js:1867-1899` keeps identity matrices for Hollow Cube and
    /// Hollow Cuboid, but sets the instance matrix to `hollowCubePos(i)` for
    /// Layers, Streaks and Blur (the presets derived from "Hollow Cube Room").
    /// `P9Glsl.js:1057-1059` applies that matrix to the incoming box corner
    /// before the shader's own `up` lattice offset, so those three presets
    /// deliberately show a doubled, larger cloud. The port adds the cell
    /// directly to the instance position instead of rotating the translated
    /// corner first; the resulting doubled placement is intentional, the exact
    /// per-instance rotation of the extra cell is not reproduced.
    pub instance_lattice: bool,
    /// The `Shader2` composite form.
    pub composite: HollowCubeComposite,
}

/// `"Plane9 Hollow Cube"` — the original `HollowCube.js` preset.
pub const PLANE9_HOLLOW_CUBE: HollowCubeVariant = HollowCubeVariant {
    name: "Plane9 Hollow Cube",
    camera_position: [0.0, 0.0, 2.2],
    camera_look_at: [0.0, 0.0, -1.0],
    camera_world_space: false,
    model_translation: [0.0, 0.0, 0.0],
    pulse_angle: 6.0,
    in1_z_weight: 1.5,
    perm_min: 0.15,
    noise_gain: 0.1,
    orbit_scale: 500.0,
    out4_bloom: false,
    instance_lattice: false,
    composite: HollowCubeComposite::Mix,
};

/// `"Point10 Hollow Cuboid"` — a closer camera and a wider pulse.
pub const POINT10_HOLLOW_CUBOID: HollowCubeVariant = HollowCubeVariant {
    name: "Point10 Hollow Cuboid",
    camera_position: [0.0, 0.0, 1.8],
    camera_look_at: [0.0, 0.0, -1.0],
    camera_world_space: false,
    model_translation: [0.0, 0.0, 0.0],
    pulse_angle: 10.0,
    in1_z_weight: 3.0,
    perm_min: 0.25,
    noise_gain: 0.2,
    orbit_scale: 720.0,
    out4_bloom: true,
    instance_lattice: false,
    composite: HollowCubeComposite::Mix,
};

/// `"Point10 Hollow Cube Layers"` — the camera sits inside the cloud.
pub const POINT10_HOLLOW_CUBE_LAYERS: HollowCubeVariant = HollowCubeVariant {
    name: "Point10 Hollow Cube Layers",
    camera_position: [0.0, 0.0, 0.0],
    camera_look_at: [1.0, 0.0, 0.0],
    camera_world_space: true,
    model_translation: [-0.5, -0.5, -0.5],
    pulse_angle: 10.0,
    in1_z_weight: 3.0,
    perm_min: 0.25,
    noise_gain: 0.2,
    orbit_scale: 720.0,
    out4_bloom: true,
    instance_lattice: true,
    composite: HollowCubeComposite::Mix,
};

/// `"Point10 Hollow Cube Blur"` — the inside view with the blurred composite.
pub const POINT10_HOLLOW_CUBE_BLUR: HollowCubeVariant = HollowCubeVariant {
    name: "Point10 Hollow Cube Blur",
    camera_position: [0.0, 0.0, 0.0],
    camera_look_at: [1.0, 0.0, 0.0],
    camera_world_space: true,
    model_translation: [-0.5, -0.5, -0.5],
    pulse_angle: 10.0,
    in1_z_weight: 3.0,
    perm_min: 0.25,
    noise_gain: 0.2,
    orbit_scale: 720.0,
    out4_bloom: true,
    instance_lattice: true,
    composite: HollowCubeComposite::Blur,
};

/// `makePerm(seed = 1)`.
pub fn make_perm(seed: f32) -> f32 {
    if seed.is_finite() { seed } else { 1.0 }
}

/// One frame of `Expression1` (`HollowCube.js:351-361`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HollowCubeDrive {
    /// `Expression1`'s `perm` for the next frame.
    pub perm: f32,
    /// `out1..3` — whole-object rotation in degrees (X/Y/Z).
    pub orbit: [f32; 3],
    /// `gIn1` (`x` explosion, `y` colour, `z` rotation).
    pub in1: [f32; 3],
    /// The Expression1 phase (`x`) and the cube scale (`y`).
    ///
    /// The uploaded `gIn2.x` is instead [`HollowCube::shader_perm`]: the
    /// vendor's `Shader1` advances its own phase on damping-0 bass. Keeping
    /// both fields makes the two Expression nodes explicit rather than sharing
    /// one phase across the shader and the object rotation.
    pub in2: [f32; 3],
}

/// `bandValue`: `Math.max(0, Number.isFinite(value) ? value : 0)`.
fn band_value(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// Pure `Expression1`.
///
/// `perm_band` is the damping-1 bass read that advances the phase,
/// `level_band` the damping-1 bass read that drives explosion/colour/rotation/
/// size, and `orbit_band` the damping-1 read with the `0.15` fallback that
/// scales the noise orbit.
pub fn hollow_cube_drive(
    perm_band: f32,
    level_band: f32,
    orbit_band: f32,
    dt: f32,
    perm: f32,
) -> HollowCubeDrive {
    let p = band_value(perm_band);
    let l = band_value(level_band);
    let orbit = band_value(orbit_band);
    let step = band_value(dt).min(MAX_DT);
    let base = if perm.is_finite() { perm } else { 1.0 };
    let next = base + (p + 0.02).min(0.15) * 0.08 * step;
    let amplitude = 1.0 + orbit * 0.1;

    HollowCubeDrive {
        perm: next,
        orbit: [
            value_noise2(next, 0.0) * amplitude * 500.0,
            value_noise2(next, 1.0) * amplitude * 500.0,
            value_noise2(next, 2.0) * amplitude * 500.0,
        ],
        in1: [
            (l * 2.2).min(1.2) * 0.6,
            (l * 4.0 + 0.4).min(0.55),
            (l * 1.5).min(1.0),
        ],
        in2: [next, 1.0 + l.min(0.2), 0.0],
    }
}

/// Pure `Expression1`, tuned by one [`HollowCubeVariant`].
///
/// Same maths as [`hollow_cube_drive`], with the variant's perm cap, orbit
/// gain/scale and `in1.z` weight. The Plane9 variant reproduces that function
/// exactly. The two implementations are kept side by side because the fidelity
/// contract pins the original function's literal constants in source.
fn hollow_cube_variant_drive(
    variant: &HollowCubeVariant,
    perm_band: f32,
    level_band: f32,
    orbit_band: f32,
    dt: f32,
    perm: f32,
) -> HollowCubeDrive {
    let p = band_value(perm_band);
    let l = band_value(level_band);
    let orbit = band_value(orbit_band);
    let step = band_value(dt).min(MAX_DT);
    let base = if perm.is_finite() { perm } else { 1.0 };
    let next = base + (p + 0.02).min(variant.perm_min) * 0.08 * step;
    let amplitude = 1.0 + orbit * variant.noise_gain;

    HollowCubeDrive {
        perm: next,
        orbit: [
            value_noise2(next, 0.0) * amplitude * variant.orbit_scale,
            value_noise2(next, 1.0) * amplitude * variant.orbit_scale,
            value_noise2(next, 2.0) * amplitude * variant.orbit_scale,
        ],
        in1: [
            (l * 2.2).min(1.2) * 0.6,
            (l * 4.0 + 0.4).min(0.55),
            (l * variant.in1_z_weight).min(1.0),
        ],
        in2: [next, 1.0 + l.min(0.2), 0.0],
    }
}

/// `Expression1`'s `out4` — the Plane9 `Bloom1` intensity driver.
///
/// `out4 = min(0.9, 0.25 + band(-1, 1, 0, 0.1) * 0.75)`. At one bloom mip the
/// Plane9 driver ignores `intensity`, so this is faithful but visually inert;
/// it is wired anyway.
fn out4_bloom_intensity(band: impl FnOnce(i32, i32, i32, f32) -> f32) -> f32 {
    (0.25 + band(-1, 1, 0, 0.1) * 0.75).min(0.9)
}

/// `Shader1`'s own Expression phase, the vendor damping-0 `gIn2.x` creep.
///
/// This is deliberately separate from [`hollow_cube_drive`]: the two
/// Expression nodes in the preset carry independent `perm` values, and only
/// `Expression1`'s phase drives the whole-object rotation.
fn shader_perm_step(perm: f32, band: f32, dt: f32) -> f32 {
    let base = if perm.is_finite() { perm } else { 1.0 };
    base + (band_value(band) + 0.01).min(0.5) * 40.0 * dt.min(MAX_DT)
}

/// `MeshInstancer`'s hollow-cube mapping: instance id -> one of the six faces
/// of an `n x n x n` cell grid, normalised to `0..1` (`HollowCube.js:104-125`).
pub fn hollow_cube_pos(index: u32, count: u32) -> [f32; 3] {
    let count = count.max(1);
    let n = ((count as f32 / 6.0).sqrt().round() as u32).max(1);
    let per_face = n * n;
    let face = index / per_face;
    let j = index % per_face;
    let gx = j % n;
    let gy = j / n;
    let o = match face {
        0 => [0, gx, gy],
        1 => [n, gx, gy],
        2 => [gx, 0, gy],
        3 => [gx, n, gy],
        4 => [gx, gy, 0],
        _ => [gx, gy, n],
    };
    let n = n as f32;
    [o[0] as f32 / n, o[1] as f32 / n, o[2] as f32 / n]
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// `HOLLOW_CUBE_PROPS` (`sceneProps.js:172-177`).
const HOLLOW_CUBE_PROPS: &[SceneProp] = &[
    SceneProp::float("spinSpeed", "Spin Speed", 1.0, 0.0, 4.0, 0.01),
    SceneProp::float("explode", "Explosion", 1.0, 0.0, 3.0, 0.01),
    SceneProp::float("color", "Color", 1.0, 0.0, 3.0, 0.01),
    SceneProp::float("bloom", "Bloom", 1.0, 0.0, 3.0, 0.01),
];

/// The cube shader's uniform block (176 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CubeUniforms {
    view_proj: Mat4,
    /// `RenderObject1.Rot` — the Expression1 whole-object rotation.
    model: Mat4,
    camera_pos: [f32; 4],
    /// `gIn1`: explosion, colour, rotation.
    in1: [f32; 4],
    /// `gIn2`: phase, cube scale.
    in2: [f32; 4],
}

/// The `Shader2` pass' uniform block (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniforms {
    /// Target size, for `position.xy / resolution`.
    resolution: [f32; 2],
    _pad: [f32; 2],
}

/// A render target plus its view.
struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Target {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            _texture: texture,
            view,
        }
    }
}

/// GPU resources built in `init`.
struct HollowCubeGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cube_pipeline: wgpu::RenderPipeline,
    cube_bind_group: wgpu::BindGroup,
    cube_uniforms: wgpu::Buffer,
    cube_target: Target,
    _cube_depth: wgpu::Texture,
    cube_depth_view: wgpu::TextureView,
    /// `composite_bind` samples the cube target.
    composite_pipeline: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    composite_sampler: wgpu::Sampler,
    composite_bind_group: wgpu::BindGroup,
    composite_uniforms: wgpu::Buffer,
    mesh: MeshGpu,
    composer: Composer,
}

/// HollowCube scene.
pub struct HollowCube {
    variant: HollowCubeVariant,
    audio: P10Audio,
    /// `Expression1`'s `perm` — the orbit phase.
    perm: f32,
    /// `Shader1`'s own `perm` — the vendor damping-0 phase (`gIn2.x`).
    shader_perm: f32,
    /// `Expression1`'s `out1..3`: whole-object rotation in degrees.
    orbit: [f32; 3],
    /// `gIn1` after the prop scales.
    in1: [f32; 4],
    /// `gIn2`.
    in2: [f32; 4],
    spin_scale: f32,
    explode_scale: f32,
    color_scale: f32,
    bloom_scale: f32,
    camera_aspect: f32,
    size: (u32, u32),
    gpu: Option<HollowCubeGpu>,
}

impl Default for HollowCube {
    fn default() -> Self {
        Self::new()
    }
}

impl HollowCube {
    /// Create the original `Plane9 Hollow Cube` scene (`HollowCube.js:334-384`
    /// defaults).
    pub fn new() -> Self {
        Self::new_with_variant(PLANE9_HOLLOW_CUBE)
    }

    /// Create one of the registered `Plane9 Hollow Cube` family presets.
    pub fn new_with_variant(variant: HollowCubeVariant) -> Self {
        Self {
            variant,
            audio: P10Audio::new(),
            perm: make_perm(1.0),
            shader_perm: make_perm(1.0),
            orbit: [0.0; 3],
            in1: [0.0, 1.1, 0.0, 0.0],
            in2: [0.0, 1.0, 0.0, 0.0],
            spin_scale: 1.0,
            explode_scale: 1.0,
            color_scale: 1.0,
            bloom_scale: 1.0,
            camera_aspect: 1.0,
            size: (1, 1),
            gpu: None,
        }
    }

    /// Apply the resolved modV props (`sceneProps.js:312-317`).
    pub fn apply_props(&mut self, props: impl FnMut(&str) -> Option<f32>) {
        for (key, value) in read_scene_props(HOLLOW_CUBE_PROPS, props) {
            match key {
                "spinSpeed" => self.spin_scale = value,
                "explode" => self.explode_scale = value,
                "color" => self.color_scale = value,
                "bloom" if (self.bloom_scale - value).abs() > f32::EPSILON => {
                    self.bloom_scale = value;
                    let params = self.plane9_bloom_params();
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.composer.enable_plane9_bloom(params);
                    }
                }
                _ => {}
            }
        }
    }

    /// The preset this scene was built from.
    pub fn variant(&self) -> &HollowCubeVariant {
        &self.variant
    }

    /// `Expression1`'s `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// `Shader1`'s own `perm` (`gIn2.x`).
    pub fn shader_perm(&self) -> f32 {
        self.shader_perm
    }

    /// `Expression1`'s `out1..3` in degrees.
    pub fn orbit(&self) -> [f32; 3] {
        self.orbit
    }

    /// `gIn1` after the prop scales.
    pub fn in1(&self) -> [f32; 4] {
        self.in1
    }

    /// `gIn2` as last uploaded.
    pub fn in2(&self) -> [f32; 4] {
        self.in2
    }

    /// The destination alpha the scene clears to.
    pub fn clear_alpha(&self) -> f64 {
        CLEAR_ALPHA
    }

    /// Plane9's `Bloom1` node with the `bloom` prop folded into its intensity.
    ///
    /// `Plane9BloomParams` with `mip_count == 1` never applies `intensity`, so
    /// the prop is preserved for parity but has no visual effect. The sibling
    /// presets drive that intensity from `Expression1`'s `out4`.
    fn plane9_bloom_params(&self) -> Plane9BloomParams {
        let intensity = if self.variant.out4_bloom {
            let out4 = out4_bloom_intensity(|c, d, b, n| self.audio.band(c, d, b, n));
            out4 * self.bloom_scale
        } else {
            BLOOM_STRENGTH * self.bloom_scale
        };
        Plane9BloomParams::new(
            BLOOM_RADIUS,
            BLOOM_THRESHOLD,
            BLOOM_SMOOTHNESS,
            intensity,
            false,
        )
    }

    /// `RenderObject1`'s `Pos`/`Rot` as a model matrix.
    ///
    /// Matrices here are column-major and applied right-to-left, so this is
    /// `R * T`: first centre the instance lattice with the preset's `Pos`
    /// (Layers/Blur use `-0.5 -0.5 -0.5`, which moves the `[0,1]` shell onto
    /// the origin), then apply Expression1's rotation about that pivot. That
    /// keeps the inside presets' camera at the centre of the spinning shell.
    /// This is a deliberate deviation from three.js, which composes the
    /// wrapper as `T * R * S` (`P9Scene.js:778-780`): with the port's post-offset
    /// lattice, applying `Pos` after the rotation swings the shell away from
    /// the world-origin camera. At a zero `Pos` the two orders coincide and
    /// this method is rotation-only, exactly as before.
    fn model(&self) -> Mat4 {
        let rotation = euler_xyz(
            self.orbit[0].to_radians(),
            self.orbit[1].to_radians(),
            self.orbit[2].to_radians(),
        );
        let position = self.variant.model_translation;
        let offset = translation(position[0], position[1], position[2]);
        mat4_mul(&rotation, &offset)
    }

    fn view_proj(&self) -> Mat4 {
        let position = self.variant.camera_position;
        let look_at_target = if self.variant.camera_world_space {
            self.variant.camera_look_at
        } else {
            [
                position[0] + self.variant.camera_look_at[0],
                position[1] + self.variant.camera_look_at[1],
                position[2] + self.variant.camera_look_at[2],
            ]
        };
        mat4_mul(
            &perspective(
                CAMERA_FOV.to_radians(),
                self.camera_aspect,
                CAMERA_NEAR,
                CAMERA_FAR,
            ),
            &look_at(position, look_at_target, [0.0, 1.0, 0.0]),
        )
    }

    fn upload_cube_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        gpu.queue.write_buffer(
            &gpu.cube_uniforms,
            0,
            bytemuck::bytes_of(&CubeUniforms {
                view_proj: self.view_proj(),
                model: self.model(),
                camera_pos: [
                    self.variant.camera_position[0],
                    self.variant.camera_position[1],
                    self.variant.camera_position[2],
                    1.0,
                ],
                in1: self.in1,
                in2: self.in2,
            }),
        );
        gpu.queue.write_buffer(
            &gpu.composite_uniforms,
            0,
            bytemuck::bytes_of(&CompositeUniforms {
                resolution: [self.size.0 as f32, self.size.1 as f32],
                _pad: [0.0; 2],
            }),
        );
    }

    /// Re-create the size-dependent targets and re-bind the passes.
    fn rebuild_targets(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        gpu.cube_target = Target::new(
            &gpu.device,
            "point10-hollow-cube-capture",
            self.size.0,
            self.size.1,
        );
        let (depth, depth_view) = create_depth(
            &gpu.device,
            "point10-hollow-cube-depth",
            self.size.0,
            self.size.1,
        );
        gpu._cube_depth = depth;
        gpu.cube_depth_view = depth_view;
        gpu.composer.resize(self.size.0, self.size.1);
        gpu.composite_bind_group = create_composite_bind_group(&gpu.device, gpu);
        self.upload_cube_uniforms();
    }
}

impl Point10Scene for HollowCube {
    fn name(&self) -> &'static str {
        self.variant.name
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        let device = ctx.device.clone();
        let queue = ctx.queue.clone();
        self.size = (ctx.width, ctx.height);
        self.camera_aspect = ctx.aspect();

        let mesh = MeshGpu::new(
            &device,
            "point10-hollow-cube-mesh",
            &build_box(CUBE_SIZE, CUBE_SIZE, CUBE_SIZE),
        );

        // --- cube capture --------------------------------------------------
        let cube_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-hollow-cube-shader"),
            source: wgpu::ShaderSource::Wgsl(hollow_cube_cube_wgsl_for(&self.variant).into()),
        });
        let cube_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-hollow-cube-layout"),
            entries: &[uniform_entry(
                0,
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            )],
        });
        let cube_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-hollow-cube-uniforms"),
            size: std::mem::size_of::<CubeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cube_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-hollow-cube-bind-group"),
            layout: &cube_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: cube_uniforms.as_entire_binding(),
            }],
        });
        let cube_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-hollow-cube-pipeline-layout"),
            bind_group_layouts: &[Some(&cube_layout)],
            immediate_size: 0,
        });
        let cube_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-hollow-cube-pipeline"),
            layout: Some(&cube_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &cube_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(crate::point10::scenes::cube_mesh::Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Shader1 is `DoubleSide`.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &cube_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Shader2 composite --------------------------------------------
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-hollow-cube-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(
                hollow_cube_composite_wgsl_for(self.variant.composite).into(),
            ),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-hollow-cube-composite-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let composite_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-hollow-cube-composite-uniforms"),
            size: std::mem::size_of::<CompositeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("point10-hollow-cube-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let composite_pipeline = ctx.fullscreen_pipeline_with_format(
            "point10-hollow-cube-composite-pipeline",
            &composite_shader,
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-hollow-cube-composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            }),
            HDR_FORMAT,
            None,
        );

        let cube_target = Target::new(
            &device,
            "point10-hollow-cube-capture",
            self.size.0,
            self.size.1,
        );
        let (cube_depth, cube_depth_view) = create_depth(
            &device,
            "point10-hollow-cube-depth",
            self.size.0,
            self.size.1,
        );

        // `composer = EffectComposer(renderer); addPass(_finalPass); addPass(OutputPass)`.
        // The `Bloom1` node is Plane9's driver, applied by the composer to the
        // Shader2 output (the faithful replacement for the vendor bloom pass).
        let mut composer =
            Composer::new(&device, &queue, self.size.0, self.size.1, ctx.target_format);
        composer.output_mut().set_tone_mapping(ToneMapping::None);
        composer.enable_plane9_bloom(self.plane9_bloom_params());

        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-hollow-cube-composite-bind-group"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: composite_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cube_target.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        self.gpu = Some(HollowCubeGpu {
            device,
            queue,
            cube_pipeline,
            cube_bind_group,
            cube_uniforms,
            cube_target,
            _cube_depth: cube_depth,
            cube_depth_view,
            composite_pipeline,
            composite_layout,
            composite_sampler: sampler,
            composite_bind_group,
            composite_uniforms,
            mesh,
            composer,
        });
        self.upload_cube_uniforms();
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width.max(1), height.max(1));
        self.camera_aspect = self.size.0 as f32 / self.size.1 as f32;
        self.rebuild_targets();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);

        // `Shader1`'s own Expression keeps the vendor damping-0 phase.
        self.shader_perm = shader_perm_step(self.shader_perm, self.audio.band(-1, 0, 0, 0.1), dt);

        let drive = hollow_cube_variant_drive(
            &self.variant,
            self.audio.band(-1, 1, 0, 0.1),
            self.audio.band(-1, 1, 0, 0.1),
            self.audio.band(-1, 1, 0, 0.15),
            dt,
            self.perm,
        );
        self.perm = drive.perm;
        self.orbit = drive.orbit;
        // `material.uniforms.gIn1.value.set(drive.in1.* * *Scale)`
        self.in1 = [
            drive.in1[0] * self.explode_scale,
            drive.in1[1] * self.color_scale,
            drive.in1[2] * self.spin_scale,
            0.0,
        ];
        // `drive.in2.x` is the Expression1 phase (the pure-function contract);
        // the shader's `gIn2.x` takes Shader1's own damping-0 phase instead.
        self.in2 = [self.shader_perm, drive.in2[1], 0.0, 0.0];

        // The sibling presets' `Expression1` drives `Bloom1`'s intensity every
        // frame. `enable_plane9_bloom` only rewrites the uniforms when the
        // pass shape is unchanged.
        if self.variant.out4_bloom {
            let params = self.plane9_bloom_params();
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.composer.enable_plane9_bloom(params);
            }
        }

        self.upload_cube_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };

        // Cube capture: clear transparent, depth clear, 15 000 instances.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-hollow-cube-capture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.cube_target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &gpu.cube_depth_view,
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
            pass.set_pipeline(&gpu.cube_pipeline);
            pass.set_bind_group(0, &gpu.cube_bind_group, &[]);
            gpu.mesh.draw_instanced(&mut pass, INSTANCE_COUNT);
        }

        // Shader2 → the composer's scene target, then the Plane9 bloom and
        // OutputPass.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-hollow-cube-composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: gpu.composer.scene_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.composite_pipeline);
            pass.set_bind_group(0, &gpu.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        gpu.composer.render(encoder, target);
    }

    fn dispose(&mut self) {
        self.gpu = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(HOLLOW_CUBE_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        // `Point10HollowCube.js` sets no category.
        if self.variant.name.starts_with(crate::point10::PLANE9_PREFIX) {
            SceneMeta::for_plane9_scene(self.variant.name, "3d-scenes")
        } else {
            SceneMeta::for_scene(self.variant.name, "3d-scenes")
        }
    }
}

/// Rebuild the composite bind group after [`Target`]s are recreated
/// (`resize`), because wgpu bind groups hold the texture views they were made
/// from.
fn create_composite_bind_group(device: &wgpu::Device, gpu: &HollowCubeGpu) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point10-hollow-cube-composite-bind-group"),
        layout: &gpu.composite_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: gpu.composite_uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&gpu.cube_target.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&gpu.composite_sampler),
            },
        ],
    })
}

/// A uniform binding entry.
fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A sampled-texture binding entry.
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

/// A filtering-sampler binding entry.
fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// `buildHollowCubeVertexShader` + `buildHollowCubeFragmentShader` with the
/// Plane9 helpers in front.
fn hollow_cube_cube_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{P9_SHADING_WGSL}\n{HOLLOW_CUBE_CUBE_WGSL}")
}

/// The cube shader for one variant.
///
/// The port's canonical `HOLLOW_CUBE_CUBE_WGSL` carries the vendor `pulse * 6`
/// rotation; the sibling presets only widen that angle.
fn hollow_cube_cube_wgsl_for(variant: &HollowCubeVariant) -> String {
    let mut program = hollow_cube_cube_wgsl();
    if variant.pulse_angle != 6.0 {
        program = program.replace(
            "pulse * 6.0 * u.in1.z",
            &format!("pulse * {:.1} * u.in1.z", variant.pulse_angle),
        );
    }
    if variant.instance_lattice {
        // The Layers/Blur presets' `MeshInstancer` matrices translate each
        // instance onto its lattice cell; identity matrices are the other
        // presets' default.
        program = program.replace(
            "  v = v + up * u.in2.y * xx;",
            "  v = v + up * u.in2.y * xx;\n  v = v + p;",
        );
    }
    program
}

/// `buildHollowCubeShader2Fragment` (the alpha-mix composite).
fn hollow_cube_composite_wgsl() -> String {
    format!(
        "{}\n{P9_ACES_WGSL}\n{HOLLOW_CUBE_COMPOSITE_WGSL}\n{HOLLOW_CUBE_COMPOSITE_MIX_BODY}",
        crate::point10::FULLSCREEN_TRIANGLE_WGSL
    )
}

/// `Shader2` for one composite variant.
fn hollow_cube_composite_wgsl_for(composite: HollowCubeComposite) -> String {
    match composite {
        HollowCubeComposite::Mix => hollow_cube_composite_wgsl(),
        HollowCubeComposite::Blur => format!(
            "{}\n{P9_ACES_WGSL}\n{HOLLOW_CUBE_COMPOSITE_WGSL}\n{HOLLOW_CUBE_COMPOSITE_BLUR_BODY}",
            crate::point10::FULLSCREEN_TRIANGLE_WGSL
        ),
    }
}

/// `_tonemapACES` (`P9Glsl.js:454-462`), verbatim.
const P9_ACES_WGSL: &str = r#"
fn p9_saturate3(color: vec3<f32>) -> vec3<f32> {
  return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn p9_tonemap_aces(color: vec3<f32>) -> vec3<f32> {
  let a = 2.51;
  let b = 0.03;
  let c = 2.43;
  let d = 0.59;
  let e = 0.14;
  return p9_saturate3((color * (a * color + b)) / (color * (c * color + d) + e));
}
"#;

const HOLLOW_CUBE_CUBE_WGSL: &str = r#"
struct CubeUniforms {
  view_proj: mat4x4<f32>,
  // RenderObject1.Rot — the Expression1 whole-object rotation.
  model: mat4x4<f32>,
  camera_pos: vec4<f32>,
  // x explosion, y colour, z rotation
  in1: vec4<f32>,
  // x phase, y cube scale
  in2: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: CubeUniforms;

// `vec3 _rand3(vec2 p)` (`P9Glsl.js:44`) — the vec2 overload is not in the
// shared subset yet.
fn hollow_rand3_vec2(p0: vec2<f32>) -> vec3<f32> {
  var p = fract(p0 * vec2<f32>(5.3987, 5.4421));
  p = p + vec2<f32>(dot(p.yx, p.xy + vec2<f32>(21.5351, 14.3137)));
  return fract(vec3<f32>(
    p.x * p.y * 95.4307,
    p.x * p.y * 97.5901,
    p.x * p.y * 93.8369,
  ));
}

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) diffuse: vec4<f32>,
  @location(1) w_normal: vec3<f32>,
  @location(2) w_pos: vec3<f32>,
  @location(3) view_dir: vec3<f32>,
}

@vertex
fn vs_main(
  @builtin(instance_index) instance: u32,
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
) -> VsOut {
  var out: VsOut;
  let instance_id = f32(instance);
  let d = 0.0001;
  let dt = instance_id / 15000.0;
  let obj_count = 50.0;
  let t = u.in2.x * 0.1;

  // `attr float aInstance` is `@builtin(instance_index)` here.
  let index = vec2<f32>(
    p9_mod(instance_id, obj_count),
    p9_mod(instance_id / obj_count, obj_count),
  );

  var o = vec3<f32>(0.0);
  if (instance_id < obj_count * obj_count) {
    o = vec3<f32>(0.0, index.x, index.y);
  } else if (instance_id < obj_count * obj_count * 2.0) {
    o = vec3<f32>(obj_count, index.x, index.y);
  } else if (instance_id < obj_count * obj_count * 3.0) {
    o = vec3<f32>(index.x, 0.0, index.y);
  } else if (instance_id < obj_count * obj_count * 4.0) {
    o = vec3<f32>(index.x, obj_count, index.y);
  } else if (instance_id < obj_count * obj_count * 5.0) {
    o = vec3<f32>(index.x, index.y, 0.0);
  } else {
    o = vec3<f32>(index.x, index.y, obj_count);
  }

  let p = o / 50.0;
  let up = (p - vec3<f32>(0.5)) * 0.5;
  var pulse = p9_cubicpulse(p9_mod(p.x + p.y + p.z + t, 1.5), 0.6, 0.4);
  var xx = 1.0;

  // The JS interior test never fires (every instance lies on the shell); kept
  // so the shader reads like the preset.
  if (o.x >= 1.0 && o.x < obj_count - 1.0 &&
      o.y >= 1.0 && o.y < obj_count - 1.0 &&
      o.z >= 1.0 && o.z < obj_count - 1.0) {
    pulse = 0.0;
    xx = 0.0;
  }

  let rotation = vec4<f32>(
    normalize(hollow_rand3_vec2(vec2<f32>(instance_id))),
    dt + u.in2.x,
  );
  let q_rotation = p9_axis_angle_to_quaternion(rotation.xyz, pulse * 6.0 * u.in1.z);
  var v = p9_rotate_vector(vec4<f32>(position, 1.0).xyz, q_rotation);

  // `nv` is a constant: `d * 50.0` never depends on the instance.
  var nv = vec3<f32>(
    (p9_noise(vec3<f32>(d * 50.0, 0.0, 0.0)) * 0.5 + 0.5) * sign(up.x),
    (p9_noise(vec3<f32>(d * 50.0, 1.0, 0.0)) * 0.5 + 0.5) * sign(up.y),
    (p9_noise(vec3<f32>(d * 50.0, 2.0, 0.0)) * 0.5 + 0.5) * sign(up.z),
  );

  v = v + nv * pulse * u.in1.x;
  v = v + up * u.in2.y * xx;

  // `gMVP * v` with `gM = RenderObject1` carrying the Expression1 rotation.
  let local = vec4<f32>(v, 1.0);
  let world = u.model * local;
  out.position = u.view_proj * world;
  out.diffuse = vec4<f32>(p9_hsv2rgb(0.0, 0.9, 0.8), 1.0)
    * (0.1 + 0.9 * p9_rand2(vec2<f32>(instance_id)));
  let rotated_normal = p9_rotate_vector(normal, q_rotation);
  out.w_normal = normalize((u.model * vec4<f32>(rotated_normal, 0.0)).xyz);
  out.w_pos = world.xyz;
  out.view_dir = u.camera_pos.xyz - world.xyz;
  return out;
}

fn light_half_lambert(normal: vec3<f32>, light_dir: vec3<f32>, diffuse: vec3<f32>) -> vec3<f32> {
  return (dot(light_dir, normal) * 0.5 + 0.5) * diffuse;
}

fn light_blinn_phong_half_lambert(
  normal: vec3<f32>,
  light_dir: vec3<f32>,
  view_dir: vec3<f32>,
  diffuse: vec3<f32>,
  specular: vec3<f32>,
  specular_hardness: f32,
) -> vec3<f32> {
  let half_vec = normalize(light_dir + view_dir);
  let spec = pow(p9_saturate(dot(normal, half_vec)), specular_hardness)
    * ((specular_hardness + 4.0) / 8.0);
  return light_half_lambert(normal, light_dir, diffuse) + specular * spec;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let normal = normalize(in.w_normal);
  let view_dir = normalize(in.view_dir);
  let specular_hardness = 10.0;

  var c = in.diffuse.rgb * u.in1.y;

  var light_dir = normalize(vec3<f32>(0.0, 1.0, 0.0) - in.w_pos);
  c = c + light_blinn_phong_half_lambert(
    normal,
    light_dir,
    view_dir,
    vec3<f32>(0.5, 0.8, 1.0),
    vec3<f32>(0.5, 0.8, 1.0) * 1.15,
    specular_hardness,
  ) * 0.04;

  light_dir = normalize(vec3<f32>(0.0, -1.0, 0.0) - in.w_pos);
  c = c + light_blinn_phong_half_lambert(
    normal,
    light_dir,
    view_dir,
    vec3<f32>(0.8, 0.0, 0.0),
    vec3<f32>(1.0, 1.0, 1.0) * 0.15,
    specular_hardness,
  ) * 0.025;

  return vec4<f32>(max(vec3<f32>(0.0), c), 1.0);
}
"#;

const HOLLOW_CUBE_COMPOSITE_WGSL: &str = r#"
struct CompositeUniforms {
  resolution: vec2<f32>,
  _pad2: vec2<f32>,
}
@group(0) @binding(0) var<uniform> u: CompositeUniforms;
@group(0) @binding(1) var t_cube: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = position.xy / u.resolution;
  let c2 = textureSample(t_cube, source_sampler, uv);
  // `PreviousLayer1.Color` has no equivalent here: the compositor alpha-blends
  // over the layers below, so the vendor's third texture is transparent black.
  let bg = vec4<f32>(0.0);
"#;

/// `Shader2`'s alpha-mix composite (Plane9 Hollow Cube, Hollow Cuboid,
/// Hollow Cube Layers).
const HOLLOW_CUBE_COMPOSITE_MIX_BODY: &str = r#"
  let col = mix(bg.rgb, p9_tonemap_aces(c2.rgb), c2.a);
  return vec4<f32>(col, c2.a);
}
"#;

/// The Blur preset's composite (`Hollow Cube Blur.xml`, Shader2).
///
/// The vendor reads `c1 = bloom * 2` and `c2 = RenderToTexture1.Color`, then
/// emits `c2.rgb * 0.06 + ACES(c1 * c1)`. In this port `c1` is the transparent
/// `PreviousLayer1` proxy, so that in-shader term is zero, but the bloom is
/// supplied once by [`Composer`]'s additive `base + bloom` combine after this
/// pass. The 6% base therefore enters the combine before the bloom, matching
/// the vendor's `dim base + bloom` order; see the GPU halo regression test.
const HOLLOW_CUBE_COMPOSITE_BLUR_BODY: &str = r#"
  let c1 = bg;
  let col = c2.rgb * 0.06 + p9_tonemap_aces(c1.rgb * c1.rgb);
  return vec4<f32>(col, c2.a);
}
"#;

#[cfg(test)]
mod tests {
    // The fidelity contract accumulates messages from `!(float < float)`
    // checks; the negation is the readable form here.
    #![allow(clippy::neg_cmp_op_on_partial_ord)]
    // The sibling contract compares `meta.audio_features == &[...]` directly.
    #![allow(clippy::op_ref)]

    use super::*;
    use crate::point10::all_scenes;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::postfx::Plane9BloomParams;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    // Port of tests/unit/point10-hollow-cube.test.js.

    #[test]
    fn perm_starts_at_the_preset_seed_and_falls_back() {
        assert_eq!(make_perm(1.0), 1.0);
        assert_eq!(make_perm(4.0), 4.0);
        assert_eq!(make_perm(f32::NAN), 1.0);
    }

    #[test]
    fn drive_accumulates_perm_from_the_bass_band_and_the_creep() {
        let quiet = hollow_cube_drive(0.0, 0.0, 0.0, 0.1, 1.0);
        let loud = hollow_cube_drive(0.1, 0.0, 0.0, 0.1, 0.0);
        let louder = hollow_cube_drive(5.0, 0.0, 0.0, 0.1, 0.0);

        assert!((quiet.perm - (1.0 + 0.02 * 0.08 * 0.1)).abs() < 1e-6);
        assert!((loud.perm - (0.1 + 0.02) * 0.08 * 0.1).abs() < 1e-6);
        // min(0.15, band + 0.02) caps the advance at 0.15 * 0.08 per second.
        assert!((louder.perm - 0.15 * 0.08 * 0.1).abs() < 1e-6);
    }

    #[test]
    fn drive_maps_the_damped_bass_lanes_to_in1_and_in2() {
        let drive = hollow_cube_drive(0.0, 0.1, 0.5, 0.1, 0.0);

        // The explosion now reads the damping-1 level lane like colour/rotation.
        assert!((drive.in1[0] - (0.1_f32 * 2.2).min(1.2) * 0.6).abs() < 1e-6);
        assert!((drive.in1[1] - (0.1_f32 * 4.0 + 0.4).min(0.55)).abs() < 1e-6);
        assert!((drive.in1[2] - (0.1_f32 * 1.5).min(1.0)).abs() < 1e-6);
        assert!((drive.in2[0] - drive.perm).abs() < 1e-6);
        assert!((drive.in2[1] - (1.0 + 0.1_f32.min(0.2))).abs() < 1e-6);
        // The third lane scales the noise orbit.
        let amplitude = 1.0 + 0.5 * 0.1;
        assert!((drive.orbit[0] - value_noise2(drive.perm, 0.0) * amplitude * 500.0).abs() < 1e-4);
        assert!((drive.orbit[1] - value_noise2(drive.perm, 1.0) * amplitude * 500.0).abs() < 1e-4);
        assert!((drive.orbit[2] - value_noise2(drive.perm, 2.0) * amplitude * 500.0).abs() < 1e-4);
    }

    #[test]
    fn drive_clamps_the_colour_size_and_rotation_terms() {
        let drive = hollow_cube_drive(10.0, 10.0, 10.0, 0.1, 0.0);

        assert!((drive.in1[0] - 0.72).abs() < 1e-6);
        assert!((drive.in1[1] - 0.55).abs() < 1e-6);
        assert!((drive.in1[2] - 1.0).abs() < 1e-6);
        assert!((drive.in2[1] - 1.2).abs() < 1e-6);
    }

    #[test]
    fn drive_sanitises_negative_non_finite_and_oversized_steps() {
        let negative = hollow_cube_drive(-1.0, -2.0, -3.0, -1.0, f32::NAN);
        let bad = hollow_cube_drive(f32::NAN, f32::NAN, f32::NAN, f32::NAN, f32::NAN);
        let long_frame = hollow_cube_drive(1.0, 1.0, 1.0, 10.0, 0.0);

        assert!((negative.perm - 1.0).abs() < 1e-6);
        assert_eq!(negative.in1, [0.0, 0.4, 0.0]);
        // A silent damping-1 lane leaves the orbit at the noise term's 1.0 gain.
        assert!((negative.orbit[0] - value_noise2(1.0, 0.0) * 500.0).abs() < 1e-4);
        assert!((bad.perm - 1.0).abs() < 1e-6);
        // P9 deltatime is clamped, so a suspended tab cannot skip the creep.
        assert!((long_frame.perm - 0.15 * 0.08 * 0.1).abs() < 1e-6);
    }

    #[test]
    fn lattice_is_the_presets_15000_instances_on_six_faces() {
        assert_eq!(INSTANCE_COUNT, 15_000);
        assert_eq!(LATTICE_COUNT, 50);
        assert_eq!(CUBE_SIZE, 0.01);

        let per_face = LATTICE_COUNT * LATTICE_COUNT;
        assert_eq!(hollow_cube_pos(0, INSTANCE_COUNT), [0.0, 0.0, 0.0]);
        assert_eq!(hollow_cube_pos(1, INSTANCE_COUNT), [0.0, 1.0 / 50.0, 0.0]);
        assert_eq!(hollow_cube_pos(per_face, INSTANCE_COUNT), [1.0, 0.0, 0.0]);
        assert_eq!(
            hollow_cube_pos(per_face * 4, INSTANCE_COUNT),
            [0.0, 0.0, 0.0]
        );
        assert_eq!(
            hollow_cube_pos(INSTANCE_COUNT - 1, INSTANCE_COUNT),
            [49.0 / 50.0, 49.0 / 50.0, 1.0]
        );
    }

    #[test]
    fn lattice_stays_on_the_shell() {
        for index in (0..INSTANCE_COUNT).step_by(137) {
            let pos = hollow_cube_pos(index, INSTANCE_COUNT);
            assert!(
                pos.iter().any(|value| *value == 0.0 || *value == 1.0),
                "instance {index} left the shell: {pos:?}"
            );
        }
    }

    #[test]
    fn clears_to_a_fully_transparent_background() {
        assert_eq!(CLEAR_ALPHA, 0.0);
        assert_eq!(HollowCube::new().clear_alpha(), 0.0);
    }

    #[test]
    fn preset_constants_match_the_js() {
        assert_eq!(CAMERA_FOV, 50.0);
        assert_eq!((CAMERA_NEAR, CAMERA_FAR), (0.01, 100.0));
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, 2.2]);
        assert_eq!(CAMERA_LOOK_AT, [0.0, 0.0, -1.0]);
        assert_eq!(
            (BLOOM_STRENGTH, BLOOM_RADIUS, BLOOM_THRESHOLD),
            (0.51, 6.0, 0.0)
        );
        assert_eq!(BLOOM_SMOOTHNESS, 0.5);
        assert_eq!(MAX_DT, 0.1);
    }

    #[test]
    fn update_consumes_the_props_and_the_bass_lanes() {
        let mut scene = HollowCube::new();
        scene.apply_props(|key| match key {
            "spinSpeed" => Some(2.0),
            "explode" => Some(0.5),
            "color" => Some(1.5),
            "bloom" => Some(3.0),
            _ => None,
        });

        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes[0] = 0.1; // damping 0, bass
        payload.lanes[3] = 0.2; // damping 1, bass
        payload.lanes[6] = 0.3; // damping 2, bass
        payload.byte_frequency_data = vec![0; 1024];
        payload.freq_mag = [0.4; crate::point10::audio_bridge::AUDIO_SAMPLES];
        let mut bridge = AudioBridge::new();
        bridge.update(0.1, Some(&payload), &Default::default());

        scene.update(0.1, &bridge);

        // `in1 = (min(1.2, 0.2*2.2)*0.6 * 0.5, min(0.55, 0.2*4+0.4) * 1.5,
        //         min(1, 0.2*1.5) * 2)` — the explosion reads damping 1 too.
        assert!((scene.in1()[0] - (0.2_f32 * 2.2).min(1.2) * 0.6 * 0.5).abs() < 1e-5);
        // `min(0.55, 0.2 * 4 + 0.4)` clamps *before* the colour prop scales it.
        assert!((scene.in1()[1] - 0.55 * 1.5).abs() < 1e-5);
        assert!((scene.in1()[2] - 0.3 * 2.0).abs() < 1e-5);
        // `gIn2.x` is Shader1's own damping-0 phase; the Expression1 phase is
        // `scene.perm()`.
        assert!((scene.in2()[0] - scene.shader_perm()).abs() < 1e-6);
        assert!((scene.shader_perm() - (1.0 + 0.11 * 40.0 * 0.1)).abs() < 1e-5);
        assert!((scene.perm() - (1.0 + 0.15 * 0.08 * 0.1)).abs() < 1e-6);
        assert!((scene.in2()[1] - 1.2).abs() < 1e-6);
    }

    #[test]
    fn time_is_capped_where_the_expression_caps_it() {
        let mut scene = HollowCube::new();
        scene.update(4.0, &AudioBridge::new());
        // `frame_dt` caps at 0.1 s; with no audio the reads are the 0.1
        // `nomusic` fallback, so `perm += min(0.15, 0.12) * 0.08 * 0.1`.
        assert!((scene.perm() - (1.0 + 0.12 * 0.08 * 0.1)).abs() < 1e-6);
        // Shader1's damping-0 phase keeps the vendor formula.
        assert!((scene.shader_perm() - (1.0 + 0.11 * 40.0 * 0.1)).abs() < 1e-5);
    }

    #[test]
    fn uniform_blocks_match_the_documented_sizes() {
        assert_eq!(std::mem::size_of::<CubeUniforms>(), 176);
        assert_eq!(std::mem::size_of::<CompositeUniforms>(), 16);
    }

    #[test]
    fn wgsl_parses_and_validates() {
        for (label, program) in [
            ("cube", hollow_cube_cube_wgsl()),
            ("composite", hollow_cube_composite_wgsl()),
        ] {
            let module = match naga::front::wgsl::parse_str(&program) {
                Ok(module) => module,
                Err(error) => panic!("{label} WGSL failed to parse: {error}"),
            };
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{label} WGSL must validate: {error}"));
        }
    }

    #[test]
    fn cube_shader_keeps_the_preset_placement_and_lighting() {
        let program = hollow_cube_cube_wgsl();
        for needle in [
            "@builtin(instance_index) instance: u32",
            "let instance_id = f32(instance);",
            "p9_mod(instance_id, obj_count)",
            "p9_mod(instance_id / obj_count, obj_count)",
            "p9_cubicpulse(p9_mod(p.x + p.y + p.z + t, 1.5), 0.6, 0.4)",
            "p9_axis_angle_to_quaternion(rotation.xyz, pulse * 6.0 * u.in1.z)",
            "sign(up.x)",
            "u.in2.y * xx",
            "0.1 + 0.9 * p9_rand2(vec2<f32>(instance_id))",
            "let specular_hardness = 10.0;",
            "vec3<f32>(0.5, 0.8, 1.0) * 1.15",
            "vec3<f32>(0.8, 0.0, 0.0)",
        ] {
            assert!(program.contains(needle), "cube shader lost {needle}");
        }
        assert!(!program.contains("gl_InstanceID"));
    }

    #[test]
    fn composite_shader_keeps_the_preset_maths() {
        let composite = hollow_cube_composite_wgsl();
        for needle in [
            "let bg = vec4<f32>(0.0);",
            "mix(bg.rgb, p9_tonemap_aces(c2.rgb), c2.a)",
            "return vec4<f32>(col, c2.a);",
        ] {
            assert!(composite.contains(needle), "composite lost {needle}");
        }
        assert!(!composite.contains("t_fbm"));
        assert!(!composite.contains("p9_fbm"));
    }

    #[test]
    fn plane9_hollow_cube_fidelity_contract() {
        let mut failures = Vec::new();
        macro_rules! check {
            ($condition:expr, $($arg:tt)+) => {
                if !$condition {
                    failures.push(format!($($arg)+));
                }
            };
        }

        // Expression1 reads bass on damping lane 1. Changing only lanes 0/2
        // must not move the explosion, and `perm` uses the documented creep.
        let mut scene = HollowCube::new();
        scene.apply_props(|key| match key {
            "spinSpeed" => Some(0.0),
            "explode" => Some(1.0),
            "color" => Some(1.0),
            "bloom" => Some(1.0),
            _ => None,
        });
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes[0] = 0.05;
        payload.lanes[3] = 0.2;
        payload.lanes[6] = 0.1;
        let mut bridge = AudioBridge::new();
        bridge.update(0.1, Some(&payload), &Default::default());
        scene.update(0.1, &bridge);

        let expected_explosion = (0.2_f32 * 2.2).min(1.2) * 0.6;
        let expected_perm = 1.0 + (0.2_f32 + 0.02).min(0.15) * 0.08 * 0.1;
        check!(
            (scene.in1()[0] - expected_explosion).abs() < 1e-6,
            "explosion must read damping-1 bass: expected {expected_explosion}, got {}",
            scene.in1()[0]
        );
        check!(
            (scene.perm() - expected_perm).abs() < 1e-6,
            "Expression1 perm must use damping-1 bass and the 0.08 creep: expected {expected_perm}, got {}",
            scene.perm()
        );

        payload.lanes[0] = 0.1;
        payload.lanes[6] = 0.9;
        bridge.update(0.1, Some(&payload), &Default::default());
        scene.update(0.1, &bridge);
        check!(
            (scene.in1()[0] - expected_explosion).abs() < 1e-6,
            "explosion responded to damping-0/2 instead of damping-1: expected {expected_explosion}, got {}",
            scene.in1()[0]
        );

        let drive = hollow_cube_drive(0.1, 0.2, 0.3, 0.1, 1.0);
        let documented_perm = 1.0 + (0.1_f32 + 0.02).min(0.15) * 0.08 * 0.1;
        check!(
            (drive.perm - documented_perm).abs() < 1e-6,
            "drive must implement min(0.15, band + 0.02) * 0.08 * dt: expected {documented_perm}, got {}",
            drive.perm
        );
        check!(
            (drive.in2[0] - drive.perm).abs() < 1e-6,
            "gIn2.x must carry the Expression1 perm"
        );

        // There is no public rotation output on HollowCubeDrive yet, so pin
        // the missing Expression1 orbit as a production-source contract.
        let production = include_str!("hollow_cube.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production source precedes the test module");
        let compact = production.split_whitespace().collect::<Vec<_>>().join(" ");
        check!(
            compact.matches("value_noise2(").count() >= 3,
            "Expression1 must derive all three orbit axes with value_noise2"
        );
        let noise_axes: Vec<&str> = production
            .match_indices("value_noise2(")
            .map(|(start, _)| {
                let args_start = start + "value_noise2(".len();
                let args_end = args_start
                    + production[args_start..]
                        .find(')')
                        .expect("value_noise2 call must close");
                production[args_start..args_end]
                    .split(',')
                    .nth(1)
                    .expect("value_noise2 call must have a y coordinate")
                    .trim()
            })
            .collect();
        for coordinate in ["0.0", "1.0", "2.0"] {
            check!(
                noise_axes.contains(&coordinate),
                "Expression1 must derive orbit coordinate {coordinate} with value_noise2"
            );
        }
        check!(
            compact.matches("* 500.0").count() >= 3,
            "Expression1 orbit must be expressed in degrees (noise2 * 500)"
        );
        let rust_logic = production
            .split("const HOLLOW_CUBE_CUBE_WGSL")
            .next()
            .expect("cube WGSL follows the Rust implementation");
        check!(
            rust_logic.contains("band(-1, 1, 0, 0.15)")
                && rust_logic.contains("1.0 +")
                && rust_logic.contains("* 0.1"),
            "Expression1 orbit amplitude must read damping-1 bass with the 0.15 fallback and apply its 0.1 gain"
        );

        // PreviousLayer1.Color is absent in this architecture: Texture3 is
        // transparent black, not the dead vendor fbm render target.
        check!(
            !compact.contains("fbm_pipeline"),
            "dead fbm pass must be removed"
        );
        check!(
            !compact.contains("fbm_target"),
            "dead fbm target must be removed"
        );
        check!(
            !compact.contains("FbmUniforms"),
            "dead fbm uniforms must be removed"
        );
        check!(
            !compact.contains("hollow_cube_fbm_wgsl"),
            "dead fbm shader must be removed"
        );
        check!(
            !compact.contains("texture_entry(3)"),
            "Texture3 sampler must be removed"
        );
        let composite = hollow_cube_composite_wgsl();
        check!(
            !composite.contains("t_fbm"),
            "composite must not sample fbm"
        );
        check!(
            !composite.contains("p9_fbm"),
            "composite must not contain fbm noise"
        );
        check!(
            !composite.contains("@binding(3)"),
            "composite must not bind Texture3"
        );
        check!(
            composite.contains("let bg = vec4<f32>(0.0);"),
            "PreviousLayer1.Color must be transparent black"
        );
        check!(
            composite.contains("return vec4<f32>(col, c2.a);"),
            "composite alpha must remain the cube capture alpha"
        );
        check!(
            !production.contains("RG16F"),
            "Plane9 Format 5 is RGBA16F, not RG16F"
        );

        // Vendor Bloom: Radius 6, Threshold 0, Smoothness 0.5, Intensity 0.51,
        // Antiflicker false. Threshold zero collapses the driver to one mip.
        let params = Plane9BloomParams::new(6.0, 0.0, 0.5, 0.51, false);
        check!(
            (
                params.radius(),
                params.threshold(),
                params.smoothness(),
                params.intensity()
            ) == (6.0, 0.0, 0.5, 0.51),
            "Plane9 bloom params must match the vendor values"
        );
        check!(
            !params.antiflicker(),
            "Plane9 bloom antiflicker must be false"
        );
        check!(params.mip_count() == 1, "Plane9 bloom must use one mip");
        check!(
            !params.prefilter_enabled(),
            "zero threshold must skip prefilter"
        );
        check!(
            params.threshold_knee() == (0.0, 0.0, 0.0),
            "zero-threshold bloom knee must stay finite and zero"
        );
        check!(
            (
                BLOOM_RADIUS,
                BLOOM_THRESHOLD,
                BLOOM_SMOOTHNESS,
                BLOOM_STRENGTH
            ) == (6.0, 0.0, 0.5, 0.51),
            "scene bloom constants must use Plane9 values, got radius {}",
            BLOOM_RADIUS
        );
        check!(
            compact.contains("Plane9BloomParams::new(") && compact.contains("enable_plane9_bloom("),
            "scene must wire Plane9BloomParams through Composer"
        );
        check!(
            !compact.contains("UnrealBloomPass::new("),
            "Hollow Cube must not use the three.js UnrealBloomPass model"
        );

        // Headless 64x64, two-frame smoke test. The same initialisation also
        // proves the scene's live composer received the scaled Plane9 params.
        if let Some((device, queue)) = gpu() {
            let mut gpu_scene = HollowCube::new();
            gpu_scene.apply_props(|key| (key == "bloom").then_some(2.0));
            let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            ctx.resize(64, 64);
            gpu_scene.init(&mut ctx);
            gpu_scene.resize(64, 64);

            if let Some(gpu) = gpu_scene.gpu.as_ref() {
                check!(
                    gpu.composer.plane9_bloom().is_some(),
                    "scene composer has no Plane9 bloom pass"
                );
                if let Some(bloom) = gpu.composer.plane9_bloom() {
                    check!(
                        bloom.params() == Plane9BloomParams::new(6.0, 0.0, 0.5, 0.51 * 2.0, false),
                        "live scene bloom params must include the bloom prop scale"
                    );
                }
            }

            let mut smoke_payload = BandPayload::silent(44_100.0);
            smoke_payload.lanes = [0.6; 12];
            let mut smoke_bridge = AudioBridge::new();
            let target = TestTarget::new(&device, 64, 64);
            let mut first = None;
            for frame in 0..2 {
                smoke_bridge.update(1.0 / 60.0, Some(&smoke_payload), &Default::default());
                gpu_scene.update(1.0 / 60.0, &smoke_bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-hollow-cube-fidelity-smoke"),
                });
                gpu_scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                let pixels = target.read(&device, &queue);
                check!(
                    any_lit(&pixels),
                    "64x64 Hollow Cube smoke frame {frame} is empty"
                );
                check!(
                    pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] < 255),
                    "64x64 Hollow Cube smoke frame {frame} must composite transparently"
                );
                if frame == 0 {
                    first = Some(pixels);
                }
            }
            check!(
                first.is_some_and(|pixels| pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] == 0)),
                "64x64 Hollow Cube must retain a fully transparent background"
            );

            // Isolate Expression1's whole-object orbit: zero the Shader1
            // explosion/colour/spin props and hold dt at zero. Both level reads
            // saturate cube size, so only the noise orbit amplitude may change.
            gpu_scene.apply_props(|key| match key {
                "spinSpeed" | "explode" | "color" => Some(0.0),
                _ => None,
            });
            let mut orbit_payload = BandPayload::silent(44_100.0);
            orbit_payload.lanes[0] = 0.4;
            let mut orbit_frames = Vec::new();
            for level in [0.2, 1.0] {
                orbit_payload.lanes[3] = level;
                smoke_bridge.update(0.0, Some(&orbit_payload), &Default::default());
                gpu_scene.update(0.0, &smoke_bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-hollow-cube-orbit-smoke"),
                });
                gpu_scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                orbit_frames.push(target.read(&device, &queue));
            }
            check!(
                orbit_frames[0] != orbit_frames[1],
                "Expression1 noise orbit must rotate the whole cloud when its damping-1 amplitude changes"
            );
        } else {
            eprintln!("skipping Hollow Cube GPU smoke: no fallback adapter");
        }

        assert!(
            failures.is_empty(),
            "Plane9 Hollow Cube fidelity failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn hollow_cube_sibling_presets_contract() {
        const NEW_NAMES: [&str; 3] = [
            "Point10 Hollow Cuboid",
            "Point10 Hollow Cube Layers",
            "Point10 Hollow Cube Blur",
        ];
        const STREAKS: &str = "Point10 Hollow Cube Streaks";

        let mut failures = Vec::new();
        macro_rules! check {
            ($condition:expr, $($arg:tt)+) => {
                if !$condition {
                    failures.push(format!($($arg)+));
                }
            };
        }

        // A — the canonical roster gains exactly these three Point10 scenes.
        // Keep this set-based: the implementation is free to choose a stable
        // insertion order without weakening the registry contract.
        let scenes = all_scenes();
        let names: Vec<&str> = scenes.iter().map(|scene| scene.name()).collect();
        let mut expected_names = vec![
            "Point10 Chromatic Line",
            "Point10 Chromatic Wave",
            "Point10 Particle Field",
            "Point10 Spiral Particles",
            "Point10 Waveform",
            "Plane9 Square Tunnel",
            "Plane9 Light Tunnel",
            "Plane9 Wormhole",
            "Plane9 It Is Coming For You",
            "Point10 Water Ripple",
            "Point10 Mycelium",
            "Point10 Around Plus",
            "Point10 Ultra HDR",
            "Plane9 Mirror Cube",
            "Plane9 Color Cube",
            "Plane9 Boxy",
            "Plane9 Sound Cube",
            "Plane9 Hollow Cube",
            "Point10 Attractor Particles",
            "Point10 DVD Room",
            "Point10 Rolling Fire",
            "Point10 Ball Of Fire",
            "Point10 Smoke Room",
            "Point10 Latent Backrooms",
            "Plane9 Sun",
            "Plane9 Earth",
            "Plane9 The Cave",
            "Plane9 Blob Fade",
            "Plane9 Circle Crossover",
            "Plane9 Colors",
            "Plane9 Gradient",
            "Plane9 Noise Warp",
            "Plane9 Rorschach",
            "Plane9 Though The Noise",
            "Plane9 Truchet Tentacles",
            "Plane9 Tv Noise",
            "Plane9 Xor",
            "Plane9 Abstract Clouds",
            "Plane9 Magic Smoke Box",
            "Plane9 Color Blob",
            "Plane9 Stargate Event Horizon",
            "Plane9 Color Beams",
            "Plane9 Mirror Tunnel",
            "Plane9 Cloud Flight",
            "Plane9 Spiky",
        ];
        expected_names.extend_from_slice(&NEW_NAMES);
        expected_names.sort_unstable();
        let mut actual_names = names.clone();
        actual_names.sort_unstable();
        check!(
            actual_names == expected_names,
            "A: Point10 roster must gain exactly {NEW_NAMES:?}; missing {:?}, unexpected {:?}",
            expected_names
                .iter()
                .filter(|name| !actual_names.contains(name))
                .collect::<Vec<_>>(),
            actual_names
                .iter()
                .filter(|name| !expected_names.contains(name))
                .collect::<Vec<_>>()
        );
        check!(
            scenes.len() == 48,
            "A: expected 48 Point10 scenes, got {}",
            scenes.len()
        );
        let mut unique_names = names.clone();
        unique_names.sort_unstable();
        unique_names.dedup();
        check!(
            unique_names.len() == names.len(),
            "A: Point10 registry names must be unique"
        );
        check!(
            !names.contains(&STREAKS),
            "A: Hollow Cube Streaks is a duplicate and must not be registered"
        );

        for name in NEW_NAMES {
            let matches: Vec<_> = scenes.iter().filter(|scene| scene.name() == name).collect();
            check!(
                matches.len() == 1,
                "A: {name} must be registered exactly once, found {}",
                matches.len()
            );
            if let Some(scene) = matches.first() {
                let meta = scene.meta();
                check!(meta.name == scene.name(), "A: {name} meta/name mismatch");
                check!(meta.author == "Point10", "A: {name} author must be Point10");
                check!(meta.version == "1.0.0", "A: {name} version must be 1.0.0");
                check!(meta.module_type == "2d", "A: {name} type must be 2d");
                check!(
                    meta.audio_features == &["bass", "mid", "treble"],
                    "A: {name} must request bass/mid/treble"
                );
            }
        }
        for scene in &scenes {
            check!(
                scene.meta().name == scene.name(),
                "A: {} has mismatched meta/name",
                scene.name()
            );
        }

        // G — the committed parity baseline and generated catalogue gain the
        // same three canonical names and counts. These are intentionally data
        // contracts so the RED run names the fixture/doc work explicitly.
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/module-parity.json"
        ))
        .expect("module-parity.json must parse");
        check!(
            manifest["counts"]["point10"].as_u64() == Some(48),
            "G: module-parity point10 count must be 48, got {:?}",
            manifest["counts"]["point10"]
        );
        check!(
            manifest["counts"]["total"].as_u64() == Some(502),
            "G: module-parity total must be 502, got {:?}",
            manifest["counts"]["total"]
        );
        let manifest_names: Vec<&str> = manifest["point10"]
            .as_array()
            .expect("module-parity point10 array")
            .iter()
            .map(|name| name.as_str().expect("point10 name"))
            .collect();
        check!(
            manifest_names.len() == 48,
            "G: module-parity point10 array must contain 48 names, got {}",
            manifest_names.len()
        );
        for name in NEW_NAMES {
            check!(
                manifest_names.contains(&name),
                "G: module-parity is missing {name}"
            );
        }
        check!(
            !manifest_names.contains(&STREAKS),
            "G: module-parity must not gain Hollow Cube Streaks"
        );

        let docs: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../docs/modules/modules.json"))
                .expect("generated module catalogue must parse");
        check!(
            docs["counts"]["point10"].as_u64() == Some(48),
            "G: generated catalogue point10 count must be 48, got {:?}",
            docs["counts"]["point10"]
        );
        check!(
            docs["counts"]["total"].as_u64() == Some(502),
            "G: generated catalogue total must be 502, got {:?}",
            docs["counts"]["total"]
        );
        let doc_point10: Vec<&str> = docs["modules"]
            .as_array()
            .expect("generated catalogue modules array")
            .iter()
            .filter(|module| module["kind"] == "point10")
            .map(|module| module["name"].as_str().expect("module name"))
            .collect();
        let mut expected_docs = expected_names.clone();
        let mut actual_docs = doc_point10.clone();
        expected_docs.sort_unstable();
        actual_docs.sort_unstable();
        check!(
            actual_docs == expected_docs,
            "G: generated catalogue Point10 roster differs: missing {:?}, unexpected {:?}",
            expected_docs
                .iter()
                .filter(|name| !actual_docs.contains(name))
                .collect::<Vec<_>>(),
            actual_docs
                .iter()
                .filter(|name| !expected_docs.contains(name))
                .collect::<Vec<_>>()
        );

        // C — pin the existing, unparameterised Hollow Cube through today's
        // public constants/drive API and its current production source. These
        // checks pass before GREEN and guard against changing the original.
        let production = include_str!("hollow_cube.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production source precedes the test module");
        check!(
            CAMERA_POSITION == [0.0, 0.0, 2.2],
            "C: Hollow Cube camera position changed"
        );
        check!(
            CAMERA_LOOK_AT == [0.0, 0.0, -1.0],
            "C: Hollow Cube camera look-at changed"
        );
        let original = HollowCube::new();
        check!(
            original.in1() == [0.0, 1.1, 0.0, 0.0],
            "C: Hollow Cube default In1 changed"
        );
        check!(
            production.contains("fn model(&self) -> Mat4") && production.contains("euler_xyz("),
            "C: Hollow Cube model must remain rotation-only at translation zero"
        );
        let original_drive = hollow_cube_drive(0.0, 0.2, 0.5, 0.0, 1.0);
        check!(
            (original_drive.in1[2] - 0.3).abs() < 1e-6,
            "C: Hollow Cube In1.z weight must remain 1.5"
        );
        let capped_drive = hollow_cube_drive(1.0, 0.0, 0.0, 0.1, 1.0);
        check!(
            (capped_drive.perm - (1.0 + 0.15 * 0.08 * 0.1)).abs() < 1e-6,
            "C: Hollow Cube Expression1 perm minimum must remain 0.15"
        );
        let expected_orbit = value_noise2(original_drive.perm, 0.0) * (1.0 + 0.5 * 0.1) * 500.0;
        check!(
            (original_drive.orbit[0] - expected_orbit).abs() < 1e-4,
            "C: Hollow Cube noise gain/orbit scale must remain 0.1/500"
        );
        check!(
            BLOOM_STRENGTH == 0.51,
            "C: Hollow Cube must retain its fixed 0.51 bloom intensity (no out4 driver)"
        );
        check!(
            production.contains("pulse * 6.0 * u.in1.z"),
            "C: Hollow Cube pulse angle must remain 6.0"
        );
        check!(
            production.contains("mix(bg.rgb, p9_tonemap_aces(c2.rgb), c2.a)"),
            "C: Hollow Cube must retain the alpha-mix composite"
        );

        // B/D/E — `HollowCubeVariant` does not exist on today's API, so direct
        // field assertions would not compile in RED. Pin the proposed pure-data
        // seam and each variant literal in production source instead; the test
        // module itself is excluded so these strings cannot satisfy the check.
        check!(
            production.contains("struct HollowCubeVariant"),
            "B: variant parameters need a plain HollowCubeVariant data struct"
        );

        fn variant_block<'a>(source: &'a str, name: &str) -> Option<&'a str> {
            let needle = format!("\"{name}\"");
            source.match_indices(&needle).find_map(|(start, _)| {
                let tail = &source[start..];
                let open_relative = tail.find('{')?;
                if tail[..open_relative].len() > 256 {
                    return None;
                }
                let open = start + open_relative;
                let mut depth = 0usize;
                for (offset, character) in source[open..].char_indices() {
                    match character {
                        '{' => depth += 1,
                        '}' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                let block = &source[open..open + offset + 1];
                                return block.contains("camera_position").then_some(block);
                            }
                        }
                        _ => {}
                    }
                }
                None
            })
        }

        fn field_value<'a>(block: &'a str, field: &str) -> Option<&'a str> {
            let marker = format!("{field}:");
            let start = block.find(&marker)? + marker.len();
            let tail = &block[start..];
            let mut square = 0usize;
            let mut paren = 0usize;
            let mut brace_depth = 0usize;
            for (index, character) in tail.char_indices() {
                match character {
                    '[' => square += 1,
                    ']' => square = square.saturating_sub(1),
                    '(' => paren += 1,
                    ')' => paren = paren.saturating_sub(1),
                    '{' => brace_depth += 1,
                    '}' => brace_depth = brace_depth.saturating_sub(1),
                    ',' if square == 0 && paren == 0 && brace_depth == 0 => {
                        return Some(tail[..index].trim());
                    }
                    _ => {}
                }
            }
            Some(tail.trim())
        }

        fn function_body<'a>(source: &'a str, marker: &str) -> Option<&'a str> {
            let start = source.find(marker)?;
            let open_relative = source[start..].find('{')?;
            let open = start + open_relative;
            let mut depth = 0usize;
            for (offset, character) in source[open..].char_indices() {
                match character {
                    '{' => depth += 1,
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return Some(&source[open..open + offset + 1]);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let variants = [
            (
                "Plane9 Hollow Cube",
                "[0.0, 0.0, 2.2]",
                "[0.0, 0.0, -1.0]",
                "false",
                "[0.0, 0.0, 0.0]",
                "6.0",
                "1.5",
                "0.15",
                "0.1",
                "500.0",
                "false",
                "mix",
            ),
            (
                "Point10 Hollow Cuboid",
                "[0.0, 0.0, 1.8]",
                "[0.0, 0.0, -1.0]",
                "false",
                "[0.0, 0.0, 0.0]",
                "10.0",
                "3.0",
                "0.25",
                "0.2",
                "720.0",
                "true",
                "mix",
            ),
            (
                "Point10 Hollow Cube Layers",
                "[0.0, 0.0, 0.0]",
                "[1.0, 0.0, 0.0]",
                "true",
                "[-0.5, -0.5, -0.5]",
                "10.0",
                "3.0",
                "0.25",
                "0.2",
                "720.0",
                "true",
                "mix",
            ),
            (
                "Point10 Hollow Cube Blur",
                "[0.0, 0.0, 0.0]",
                "[1.0, 0.0, 0.0]",
                "true",
                "[-0.5, -0.5, -0.5]",
                "10.0",
                "3.0",
                "0.25",
                "0.2",
                "720.0",
                "true",
                "blur",
            ),
        ];
        for (
            name,
            camera,
            look_at,
            world_space,
            translation,
            pulse,
            weight,
            perm_min,
            gain,
            scale,
            out4,
            composite,
        ) in variants
        {
            let Some(block) = variant_block(production, name) else {
                failures.push(format!("B: no HollowCubeVariant literal for {name}"));
                continue;
            };
            let compact = block.split_whitespace().collect::<Vec<_>>().join(" ");
            for (field, expected) in [
                ("camera_position", camera),
                ("camera_look_at", look_at),
                ("model_translation", translation),
            ] {
                check!(
                    compact.contains(&format!("{field}: {expected}")),
                    "B: {name} {field} must be {expected}, got {:?}",
                    field_value(block, field)
                );
            }
            for (field, expected) in [
                ("camera_world_space", world_space),
                ("pulse_angle", pulse),
                ("in1_z_weight", weight),
                ("perm_min", perm_min),
                ("noise_gain", gain),
                ("orbit_scale", scale),
                ("out4_bloom", out4),
            ] {
                check!(
                    field_value(block, field) == Some(expected),
                    "B: {name} {field} must be {expected}, got {:?}",
                    field_value(block, field)
                );
            }
            let composite_value = field_value(block, "composite")
                .unwrap_or_default()
                .to_ascii_lowercase();
            check!(
                composite_value.contains(composite),
                "E: {name} composite selector must select {composite}, got {composite_value:?}"
            );
        }

        let compact_production: String =
            production.chars().filter(|c| !c.is_whitespace()).collect();
        let out4_math = "min(0.9,0.25+band(-1,1,0,0.1)*0.75)";
        let rust_out4_math = "(0.25+band(-1,1,0,0.1)*0.75).min(0.9)";
        check!(
            compact_production.contains(out4_math) || compact_production.contains(rust_out4_math),
            "D: out4 must compute min(0.9, 0.25 + band(-1,1,0,0.1)*0.75)"
        );
        check!(
            function_body(production, "fn plane9_bloom_params").is_some_and(|body| {
                body.contains("out4") && body.contains("Plane9BloomParams::new(")
            }),
            "D: out4 must feed the Plane9 bloom intensity parameter"
        );
        check!(
            compact_production.contains("c2.rgb*0.06+p9_tonemap_aces(c1.rgb*c1.rgb)"),
            "E: Blur must composite c2.rgb*0.06 + ACES(c1*c1)"
        );
        check!(
            compact_production.contains("mix(bg.rgb,p9_tonemap_aces(c2.rgb),c2.a)"),
            "E: Hollow Cube/Cuboid/Layers must retain mix(bg, ACES(c2), c2.a)"
        );

        // G — provenance is cross-crate. The renderer crate cannot call
        // modv_ui::model::module_license, so assert the existing source/notice
        // convention here; the UI's direct licence test is GREEN follow-up work.
        let notices = include_str!("../../../../../THIRD_PARTY_NOTICES.md");
        let model_source = include_str!("../../../../modv-ui/src/model.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("model production source precedes its tests");
        let attributed_v4 = concat!("CC BY-NC-", "SA 4.0 (Point10 /");
        for name in NEW_NAMES {
            let suffix = name.strip_prefix("Point10 ").expect("Point10 prefix");
            let row = notices
                .lines()
                .find(|line| line.starts_with(&format!("| {suffix} |")));
            check!(
                row.is_some_and(|row| {
                    row.contains("CC BY-NC-SA 4.0")
                        && row.contains("Point10")
                        && row.contains("Dahl")
                }),
                "G: notices need a CC BY-NC-SA 4.0 Point10/Dahl provenance row for {suffix}"
            );
            check!(
                model_source.contains(&format!("\"{suffix}\""))
                    && model_source.contains(attributed_v4)
                    && model_source.contains("Dahl; see THIRD_PARTY_NOTICES.md)"),
                "G: module_license must resolve {name} to CC BY-NC-SA 4.0 attributed to Point10/Dahl"
            );
        }

        // F — every registered sibling gets the real 64x64, two-frame,
        // fallback-adapter smoke. Missing scenes are already failures above;
        // once present, an empty/black inside-view for Layers or Blur fails here.
        let mut siblings: Vec<Box<dyn Point10Scene>> = scenes
            .into_iter()
            .filter(|scene| NEW_NAMES.contains(&scene.name()))
            .collect();
        check!(
            siblings.len() == NEW_NAMES.len(),
            "F: expected three sibling scenes for GPU smoke, found {}",
            siblings.len()
        );
        if !siblings.is_empty() {
            if let Some((device, queue)) = gpu() {
                for mut scene in siblings.drain(..) {
                    let name = scene.name();
                    let mut ctx =
                        SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
                    ctx.resize(64, 64);
                    scene.init(&mut ctx);
                    scene.resize(64, 64);

                    let mut payload = BandPayload::silent(44_100.0);
                    payload.lanes = [0.6; 12];
                    let mut bridge = AudioBridge::new();
                    let target = TestTarget::new(&device, 64, 64);
                    for frame in 0..2 {
                        bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
                        scene.update(1.0 / 60.0, &bridge);
                        let mut encoder =
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("point10-hollow-cube-sibling-smoke"),
                            });
                        scene.render(&mut encoder, &target.view);
                        queue.submit(Some(encoder.finish()));
                        let pixels = target.read(&device, &queue);
                        check!(
                            pixels.len() == 64 * 64 * 4,
                            "F: {name} frame {frame} returned {} bytes",
                            pixels.len()
                        );
                        check!(
                            any_lit(&pixels),
                            "F: {name} 64x64 frame {frame} is empty or uniformly black"
                        );
                    }
                }
            } else {
                failures.push(
                    "F: a headless fallback adapter is required for sibling GPU smoke".to_string(),
                );
            }
        }

        assert!(
            failures.is_empty(),
            "Hollow Cube sibling preset contract failed ({} checks):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn meta_and_props_match_the_js_registry() {
        let scene = HollowCube::new();
        assert_eq!(scene.meta().name, "Plane9 Hollow Cube");
        assert_eq!(scene.meta().category, "3d-scenes");
        let names: Vec<&str> = scene.props().iter().map(|prop| prop.name).collect();
        assert_eq!(
            &names[names.len() - 4..],
            &["spinSpeed", "explode", "color", "bloom"]
        );
        assert_eq!(names[0], "gain");
    }

    /// Rec. 709 luma of an RGBA8 read-back, normalised to 0..1.
    fn luma(pixels: &[u8]) -> Vec<f32> {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
            .collect()
    }

    struct BloomHalo {
        name: &'static str,
        mean: f32,
        direct_mean: f32,
        outside_count: usize,
        outside_mean: f32,
        gained_fraction: f32,
    }

    /// Render two frames, then re-render the same uniforms with the composer's
    /// additive Plane9 combine disabled so the direct composite can be used as
    /// a silhouette mask. Blur's in-shader `c1` is the transparent previous
    /// layer proxy (`c1*c1 == 0`), so the halo below comes entirely from the
    /// composer's `base + bloom` combine — the vendor's bloom term in this
    /// architecture. The 6% base is applied *before* that combine, matching
    /// the XML's `c2.rgb*0.06 + ACES(c1*c1)` order.
    fn bloom_halo(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        variant: HollowCubeVariant,
    ) -> BloomHalo {
        let mut scene = HollowCube::new_with_variant(variant);
        let mut ctx = SceneContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        scene.resize(64, 64);
        let target = TestTarget::new(device, 64, 64);
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.6; 12];
        let mut bridge = AudioBridge::new();
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("point10-hollow-cube-halo"),
            });
            scene.render(&mut encoder, &target.view);
            queue.submit(Some(encoder.finish()));
        }
        let with_bloom = target.read(device, queue);

        // No `update` here: only the composer's bloom is removed, so the scene
        // uniforms and geometry are identical to the frame above.
        scene.gpu.as_mut().unwrap().composer.disable_plane9_bloom();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-hollow-cube-no-bloom"),
        });
        scene.render(&mut encoder, &target.view);
        queue.submit(Some(encoder.finish()));
        let direct_only = target.read(device, queue);

        let lit = luma(&with_bloom);
        let direct = luma(&direct_only);
        let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len() as f32;
        let outside: Vec<f32> = lit
            .iter()
            .zip(&direct)
            .filter(|(_, d)| **d <= 1.0 / 255.0)
            .map(|(v, _)| *v)
            .collect();
        let outside_count = outside.len();
        let outside_mean = mean(&outside);
        let gained_fraction = outside.iter().filter(|v| **v > 1.0 / 255.0).count() as f32
            / outside_count.max(1) as f32;
        BloomHalo {
            name: variant.name,
            mean: mean(&lit),
            direct_mean: mean(&direct),
            outside_count,
            outside_mean,
            gained_fraction,
        }
    }

    #[test]
    fn plane9_bloom_combine_supplies_the_halo_for_blur_and_cuboid() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let base = bloom_halo(&device, &queue, PLANE9_HOLLOW_CUBE);
        let cuboid = bloom_halo(&device, &queue, POINT10_HOLLOW_CUBOID);
        let blur = bloom_halo(&device, &queue, POINT10_HOLLOW_CUBE_BLUR);

        for halo in [&cuboid, &blur] {
            assert!(
                halo.outside_count > 1000,
                "{}: silhouette mask is vacuous ({} pixels)",
                halo.name,
                halo.outside_count
            );
            assert!(
                halo.outside_mean > 0.003,
                "{}: no halo outside the drawn geometry ({:.6})",
                halo.name,
                halo.outside_mean
            );
            assert!(
                halo.gained_fraction > 0.4,
                "{}: only {:.1}% of dark pixels gained bloom",
                halo.name,
                halo.gained_fraction * 100.0
            );
            assert!(
                halo.mean > halo.direct_mean * 1.3,
                "{}: additive bloom did not brighten the frame ({:.6} vs {:.6})",
                halo.name,
                halo.mean,
                halo.direct_mean
            );
        }
        // The Blur preset scales the base down to 6%; the composer's bloom
        // must still lift the final frame materially above that dim base.
        assert!(
            blur.mean > base.mean * 0.06,
            "Blur lost the composer's bloom: {:.6} <= 6% of {:.6}",
            blur.mean,
            base.mean
        );
    }
}
