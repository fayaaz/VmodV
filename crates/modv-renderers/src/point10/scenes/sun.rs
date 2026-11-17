//! Sun — Rust/wgpu port of
//! `src/renderer/src/application/point10/scenes/Sun.js`.
//!
//! The Plane9 "Sun" preset (author Joakim Dahl, CC BY-NC-SA 3.0) renders a
//! ridged-multifractal sun sphere plus an additive corona disc into a
//! render-to-texture, blurs a copy through a multi-mip gaussian pyramid,
//! combines the blurred and sharp sun, tints the result orange, then a
//! fullscreen god-ray shader accumulates the tint over 50 samples and squares
//! the result. The port keeps the shader maths verbatim and reproduces the
//! chain with explicit render targets — no three.js `EffectComposer`, no
//! `UnrealBloomPass`, `renderer.toneMapping = NoToneMapping` (`Sun.js:386`),
//! so the final frame is presented raw, with alpha derived from the ray
//! brightness so the scene composites transparently over the layer below.
//!
//! # Ported exactly
//!
//! * the `SUN` preset constants (`:27-44`): fov 45, near 0.1, far 1000,
//!   camera `[0, 0, 3.8]` looking at `[0, 0, -1]`, sphere radius 1 with 32x32
//!   segments, `RingGeometry(1, 1.1, 32, 1)`, `rays { samples: 50, density: 1,
//!   weight: 0.2, decay: 0.96 }`, `tint [0.882, 0.439, 0.024]`,
//!   `surface { floor: 0.03, gain: 1, ceiling: 0.12, damping: 2, band: 0 }`,
//!   `corona { floor: 0.1, gain: 20, ceiling: 0.5, damping: 2, band: 0 }`,
//!   `blur { width: 4, taps: 21, sigma: 1.2 }`;
//! * the two pure Expressions and the god-ray weight (`:46-68`):
//!   [`sun_surface_level`], [`sun_corona_level`], [`sun_ray_weight`] — same
//!   `Math.max(0, Number.isFinite(bass) ? bass : 0)` clamping and the same
//!   `Math.min(ceiling, b * gain + floor) * Math.max(0, scale)` shape;
//! * the camera (`:394-404`): fixed position, `lookAt(0, 0, -1)`, fov 45,
//!   `aspect = w / max(1, h)` re-applied each resize;
//! * the audio drive (`:413-440`): `bass = band(-1, 2, 0, 0.1)` for the
//!   surface, `band(-1, 2, 0, 0.05)` for the corona; `surface =
//!   sunSurfaceLevel(bass, 1)`, `corona = sunCoronaLevel(cbass, coronaScale)`;
//!   `time = this.time * surfaceSpeed` feeds both `gTime` uniforms;
//!   `gRayWeight = sunRayWeight(raysScale)` and `gBrightness = brightnessScale`;
//! * the surface fragment (`:92-107`):
//!   `_ridgedmffast(vWpos * 15 + vec3(0, 0, gTime * 0.2), 3.0)` with sunspots
//!   `smoothstep(1.0, 0.6, _noise(vWpos * 3 - vec3(0, 0, gTime * 0.2)) * 0.5 + 0.5)`
//!   and the output `vec4(vec3(v * gIn1.x), 1.0)`;
//! * the corona fragment (`:120-136`): `p = vTex * 2 - 1`,
//!   `v1 = _ridgedmffast(vec3(vTex * 50, gTime * 0.5), 5.0)`,
//!   `v2 = smoothstep(0.7, 1.0, _turbulencefast(vec3(vTex * 10, gTime * 0.2), 3.0) + 0.4)`,
//!   `v3 = smoothstep(1.0, 0.9, length(p))`, `v = v1 * v2 * v3 * gIn1.x`,
//!   output `vec4(vec3(v), 0.0)` — a DoubleSide, additive (`ONE + ONE`) disc
//!   with no depth test, exactly as the JS material;
//! * the fullscreen chain (`:457-558`):
//!   1. scene group into `sunTarget` after a transparent-black clear;
//!   2. `_blurPyramid`: copy `sunTarget` into `_mips[0]`, then four separable
//!      gaussian iterations (horizontal into the fixed half-res `_blurAux`,
//!      then vertical into `_mips[i + 1]`), with
//!      `spread = from.width / to.width` and `gTexel = spread / to.{width,height}`;
//!   3. `coronaComposite`: weighted `c1` of `_mips[0..4]` (`0.333` / `0.267` /
//!      `0.2` / `0.133` / `0.067`, alpha ignored) plus the sharp `sunTarget`;
//!      output `(c1.rrr * 3.0 + c2.rgb * 0.9, c2.a)` into `coronaTarget`;
//!   4. `tint`: `gColor1 * coronaTarget.r` into `tintTarget`;
//!   5. `screen`/god rays: 50 samples from the light position
//!      (`gIn1 = (0.5, 0.5)`, fixed — the JS never updates it), `density 1`,
//!      `weight = gRayWeight`, `decay 0.96`; adds `coronaTarget.rgb * 0.8`;
//!      `out = c * c * gBrightness`; `alpha = clamp(max(out.rgb), 0, 1)` —
//!      rendered into the destination view handed to [`Point10Scene::render`];
//! * every intermediate target is `Rgba16Float` (three's `HalfFloatType`) with
//!   no depth buffer; mips are `halfRes >> i`, min 2 px.
//!
//! # Replaced three.js machinery
//!
//! | three.js | Rust |
//! |----------|------|
//! | `SphereGeometry(1, 32, 32)` | [`build_sphere`] (CPU twin, same vertex order, uvs and pole handling) |
//! | `RingGeometry(1, 1.1, 32, 1)` | [`build_ring`] (flat annulus in the XY plane, three's uv formula) |
//! | `WebGLRenderTarget` + `rendering.setRenderTarget` | [`crate::point10::postfx`]-style explicit targets built in [`Sun::rebuild_targets`] |
//! | `PlaneGeometry(2, 2)` fullscreen quad | the shared [`crate::point10::FULLSCREEN_TRIANGLE_WGSL`] |
//! | `ShaderMaterial`s | one WGSL module per pass (`*_wgsl` functions), pipelines created once at init |
//! | `P9Audio.band(-1, damping, bandnr, nomusic)` | [`P10Audio::band`] over the shared [`AudioBridge`] |
//! | `this.time` capped by the adapter clock | [`frame_dt`] (0.1 s cap, `Sun.js` inherits it from `BaseScene`) |
//!
//! # Deviations
//!
//! * **`_ridgedmffast` is not in `P9_COMMON_WGSL`.** The WGSL for the ridged
//!   multifractal is inlined in this file as `sun_ridgedmffast`, matching the
//!   GLSL semantics exactly (`offset - abs(n)`, then `n * n`, `amp` starts at
//!   `0.5`, `prev` feedback, `2.13628142` lacunarity) — `P9Glsl.js:100-113`.
//!   It is *not* the `4 * v * v` Musgrave variant; the JS preset calls the
//!   smooth ridged noise, and so does this port. A pure-Rust twin for probe
//!   comparisons is not included: it would mean re-implementing Brian
//!   Sharpe's 3D simplex on the CPU just for this scene, and the
//!   equivalence is pinned by the WGSL validation test plus the
//!   `p9_glsl` common-block tests that the noise itself shares.
//! * **`smoothstep` with reversed edges.** The GLSL calls
//!   `smoothstep(1.0, 0.6, …)`, `smoothstep(1.0, 0.9, …)` where WGSL leaves
//!   `edge0 >= edge1` undefined; everywhere a reversed edge appears this port
//!   uses `p9_smoothstep` (GLSL's own formula), which is exact for both
//!   orders.
//! * **Blur weights.** three recomputes the 21 gaussian weights on the CPU
//!   (`exp(-0.5 * d² / sigma²)`, normalised) and uploads them in a
//!   `gW[21]` uniform. WGSL uniform arrays need a 16-byte stride, so an
//!   `array<f32, 21>` uniform would not validate; instead the weights are
//!   computed at shader build time by [`blur_weights`] (the same formula) and
//!   baked into the blur fragment as a local array literal. Same numbers,
//!   no layout games.
//! * **`gIn1` on the screen material.** The light position `(0.5, 0.5)` is
//!   a WGSL const here; the JS keeps it as an always-(0.5, 0.5) uniform
//!   (`Sun.js:369`), never updates it, and the Expression only ever writes
//!   `gRayWeight`/`gBrightness`.
//! * **`P9Audio`.** As in every ported scene, `P10Audio` over the shared
//!   `AudioBridge` replaces `plane9/P9Audio.js`; same read API, damping lanes
//!   from `audio-bands.js`.
//! * **No tone mapping.** `renderer.toneMapping = NoToneMapping` is kept:
//!   the chain presents raw (no ACES, no sRGB encode beyond what the
//!   destination format implies), exactly like the JS.
//! * **Orientation.** Fullscreen passes derive `uv` from `@builtin(position)`
//!   (`position / resolution`, v = 0 at the top row), the same convention
//!   `afterimage.wgsl` uses. The sun and ring are radially symmetric, so the
//!   flip is invisible, and the light position (0.5, 0.5) is unaffected.
//! * **Ring uvs** follow three's formula
//!   `((vertex / outerRadius + 1) / 2)` — the only effect is the corona
//!   pattern phase, which is identical to three's.
//! * **Geometry winding.** The sphere's triangles are reordered to be
//!   counter-clockwise seen from outside (the equator check in the tests
//!   pins this) and drawn with `CullMode::Back`; three's `FrontSide` sphere
//!   relies on the same convention. The ring stays `cull_mode: None`
//!   (`DoubleSide`) with additive blending.
//! * **Corner halo.** With the default god-ray weight the 50-sample
//!   accumulate over the multi-mip tint never reaches exactly zero in the
//!   far corners: measured 2–3/255 alpha at 64 px and 256 px (in this port
//!   and in the JS preset alike). With the `rays` prop at 0 the corners are
//!   exactly `(0, 0, 0, 0)`; with it at 1 they are a 2–3/255 ghost the layer
//!   compositor absorbs. The smoke test asserts both halves of that floor.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::{
    FULLSCREEN_TRIANGLE_WGSL, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt,
    read_scene_props, scene_props_with_levers,
};

// ---------------------------------------------------------------------------
// Constants (`Sun.js:27-44`)
// ---------------------------------------------------------------------------

/// `SUN.surface` — the surface Expression bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunSurface {
    /// `SUN.surface.floor`.
    pub floor: f32,
    /// `SUN.surface.gain`.
    pub gain: f32,
    /// `SUN.surface.ceiling`.
    pub ceiling: f32,
    /// `SUN.surface.damping` — the lane the bass reads through.
    pub damping: i32,
    /// `SUN.surface.band` — `-1` channel, band 0.
    pub band: i32,
}

/// `SUN.corona` — the corona Expression bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunCorona {
    /// `SUN.corona.floor`.
    pub floor: f32,
    /// `SUN.corona.gain`.
    pub gain: f32,
    /// `SUN.corona.ceiling`.
    pub ceiling: f32,
    /// `SUN.corona.damping`.
    pub damping: i32,
    /// `SUN.corona.band`.
    pub band: i32,
}

/// `SUN.rays` — the god-ray accumulator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunRays {
    /// `SUN.rays.samples` — fixed 50, baked into the screen shader.
    pub samples: u32,
    /// `SUN.rays.density`.
    pub density: f32,
    /// `SUN.rays.weight` — the base `gRayWeight` scale.
    pub weight: f32,
    /// `SUN.rays.decay` — per-sample decay.
    pub decay: f32,
}

/// `SUN.blur` — the gaussian pyramid setup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunBlur {
    /// `SUN.blur.width` — kept for parity; the preset uses this only as a label.
    pub width: f32,
    /// `SUN.blur.taps` — 21.
    pub taps: u32,
    /// `SUN.blur.sigma`.
    pub sigma: f32,
}

/// `SUN` — the whole preset constant block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunPreset {
    /// `SUN.fov`.
    pub fov: f32,
    /// `SUN.near`.
    pub near: f32,
    /// `SUN.far`.
    pub far: f32,
    /// `SUN.cameraPosition`.
    pub camera_position: [f32; 3],
    /// `SUN.cameraLookAt`.
    pub camera_look_at: [f32; 3],
    /// `SUN.radius`.
    pub radius: f32,
    /// `SUN.segments` — sphere width/height segments.
    pub segments: u32,
    /// `SUN.ringInner`.
    pub ring_inner: f32,
    /// `SUN.ringOuter`.
    pub ring_outer: f32,
    /// `SUN.ringSegments`.
    pub ring_segments: u32,
    /// `SUN.rays`.
    pub rays: SunRays,
    /// `SUN.tint` — `gColor1` (alpha 1).
    pub tint: [f32; 3],
    /// `SUN.surface`.
    pub surface: SunSurface,
    /// `SUN.corona`.
    pub corona: SunCorona,
    /// `SUN.blur`.
    pub blur: SunBlur,
}

/// `export const SUN` (`Sun.js:27-44`) — kept as one object so callers can
/// read `SUN.rays.weight` et al. the same way the JS does.
pub const SUN: SunPreset = SunPreset {
    fov: 45.0,
    near: 0.1,
    far: 1000.0,
    camera_position: [0.0, 0.0, 3.8],
    camera_look_at: [0.0, 0.0, -1.0],
    radius: 1.0,
    segments: 32,
    ring_inner: 1.0,
    ring_outer: 1.1,
    ring_segments: 32,
    rays: SunRays {
        samples: 50,
        density: 1.0,
        weight: 0.2,
        decay: 0.96,
    },
    tint: [0.882, 0.439, 0.024],
    surface: SunSurface {
        floor: 0.03,
        gain: 1.0,
        ceiling: 0.12,
        damping: 2,
        band: 0,
    },
    corona: SunCorona {
        floor: 0.1,
        gain: 20.0,
        ceiling: 0.5,
        damping: 2,
        band: 0,
    },
    blur: SunBlur {
        width: 4.0,
        taps: 21,
        sigma: 1.2,
    },
};

// ---------------------------------------------------------------------------
// Pure expressions (`Sun.js:46-68`)
// ---------------------------------------------------------------------------

/// `sunSurfaceLevel(bass, scale = 1)`.
///
/// `Math.max(0, Number.isFinite(bass) ? bass : 0)`, then
/// `Math.min(SUN.surface.ceiling, b * SUN.surface.gain + SUN.surface.floor)`,
/// scaled by `Math.max(0, scale)`. Non-finite scales clamp to 0 (Rust's
/// `f32::max` drops NaN); an infinite scale stays infinite, matching JS.
pub fn sun_surface_level(bass: f32, scale: f32) -> f32 {
    let b = if bass.is_finite() { bass.max(0.0) } else { 0.0 };
    let level = SUN
        .surface
        .ceiling
        .min(b * SUN.surface.gain + SUN.surface.floor);

    level * scale.max(0.0)
}

/// `sunCoronaLevel(bass, scale = 1)` — same clamping, `SUN.corona` bounds.
pub fn sun_corona_level(bass: f32, scale: f32) -> f32 {
    let b = if bass.is_finite() { bass.max(0.0) } else { 0.0 };
    let level = SUN
        .corona
        .ceiling
        .min(b * SUN.corona.gain + SUN.corona.floor);

    level * scale.max(0.0)
}

/// `sunRayWeight(scale = 1)` — `SUN.rays.weight * Math.max(0, scale)`.
pub fn sun_ray_weight(scale: f32) -> f32 {
    SUN.rays.weight * scale.max(0.0)
}

/// `sunSurfaceLevel` under its JS name.
#[allow(non_snake_case)]
pub fn sunSurfaceLevel(bass: f32, scale: f32) -> f32 {
    sun_surface_level(bass, scale)
}

/// `sunCoronaLevel` under its JS name.
#[allow(non_snake_case)]
pub fn sunCoronaLevel(bass: f32, scale: f32) -> f32 {
    sun_corona_level(bass, scale)
}

/// `sunRayWeight` under its JS name.
#[allow(non_snake_case)]
pub fn sunRayWeight(scale: f32) -> f32 {
    sun_ray_weight(scale)
}

/// `blurInto`'s `spread` — `from.width / to.width`.
fn blur_spread(from_width: u32, to_width: u32) -> f32 {
    from_width as f32 / to_width.max(1) as f32
}

/// The 21 normalised gaussian weights (`Sun.js:313-320`), recomputed on the
/// CPU so the blur shader can bake them.
///
/// `d = i - (taps - 1) / 2`, `w = exp(-0.5 * d² / sigma²)`, then each weight
/// is divided by the sum.
pub fn blur_weights() -> [f32; 21] {
    let mut weights = [0.0_f32; 21];
    let mut sum = 0.0_f32;
    for (i, weight) in weights.iter_mut().enumerate() {
        let d = i as f32 - (SUN.blur.taps - 1) as f32 / 2.0;
        *weight = (-0.5 * d * d / (SUN.blur.sigma * SUN.blur.sigma)).exp();
        sum += *weight;
    }
    for weight in weights.iter_mut() {
        *weight /= sum;
    }
    weights
}

// ---------------------------------------------------------------------------
// Geometry (`Sun.js:291-301` — SphereGeometry + RingGeometry)
// ---------------------------------------------------------------------------

/// One interleaved sun vertex (`position`, `normal`, `uv`).
///
/// `v = 1` is the visual top of the texture (three's `flipY` convention);
/// nothing in this scene samples by uv, so the flip only matters for parity.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SunVertex {
    /// Position in object space.
    pub position: [f32; 3],
    /// Normal (sphere: normalised position; ring: +z).
    pub normal: [f32; 3],
    /// Face texture coordinate.
    pub uv: [f32; 2],
}

impl SunVertex {
    /// Buffer layout (`@location(0..2)`).
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 24,
                    shader_location: 2,
                },
            ],
        }
    }
}

/// `SphereGeometry(radius, widthSegments, heightSegments)`.
///
/// Same vertex order as three (rows from the top pole, columns along phi),
/// same uvs (`u` along phi, `1 - v` down the rows), normals normalised
/// positions. The pole rows are degenerate in three as well, so the outermost
/// triangles of those rows are skipped exactly as three's index builder does.
/// The returned triangles wind counter-clockwise seen from outside.
pub fn build_sphere(
    radius: f32,
    width_segments: u32,
    height_segments: u32,
) -> (Vec<SunVertex>, Vec<u32>) {
    let columns = width_segments + 1;
    let rows = height_segments + 1;
    let mut vertices = Vec::with_capacity((columns * rows) as usize);

    for iy in 0..rows {
        let v = iy as f32 / height_segments as f32;
        let theta = v * std::f32::consts::PI;
        let (sin_theta, cos_theta) = theta.sin_cos();

        for ix in 0..columns {
            let u = ix as f32 / width_segments as f32;
            let phi = u * std::f32::consts::TAU;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let position = [
                -radius * cos_phi * sin_theta,
                radius * cos_theta,
                radius * sin_phi * sin_theta,
            ];
            let length =
                (position[0] * position[0] + position[1] * position[1] + position[2] * position[2])
                    .sqrt()
                    .max(1e-9);
            vertices.push(SunVertex {
                position,
                normal: [
                    position[0] / length,
                    position[1] / length,
                    position[2] / length,
                ],
                uv: [u, 1.0 - v],
            });
        }
    }

    let mut indices = Vec::with_capacity((width_segments * height_segments * 6) as usize);
    for iy in 0..height_segments {
        for ix in 0..width_segments {
            let a = iy * columns + ix;
            let b = (iy + 1) * columns + ix;
            let c = b + 1;
            let d = a + 1;

            if iy != 0 {
                indices.extend_from_slice(&[a, b, d]);
            }
            if iy + 1 != height_segments {
                indices.extend_from_slice(&[b, c, d]);
            }
        }
    }

    (vertices, indices)
}

/// `RingGeometry(inner, outer, thetaSegments, phiSegments)` — the flat
/// annulus in the XY plane with `z = 0`.
///
/// uvs follow three's formula: `((vertex / outer + 1) / 2)` per axis.
/// Triangles wind counter-clockwise seen from +z (the ring is drawn with
/// `cull_mode: None`, like the JS `DoubleSide`).
pub fn build_ring(
    inner: f32,
    outer: f32,
    theta_segments: u32,
    phi_segments: u32,
) -> (Vec<SunVertex>, Vec<u32>) {
    let columns = theta_segments + 1;
    let rows = phi_segments + 1;
    let mut vertices = Vec::with_capacity((columns * rows) as usize);

    for icr in 0..rows {
        let radius = inner + (outer - inner) * (icr as f32 / phi_segments as f32);

        for ict in 0..columns {
            let theta = (ict as f32 / theta_segments as f32) * std::f32::consts::TAU;
            let (sin_theta, cos_theta) = theta.sin_cos();
            let position = [radius * cos_theta, radius * sin_theta, 0.0];
            vertices.push(SunVertex {
                position,
                normal: [0.0, 0.0, 1.0],
                uv: [
                    (position[0] / outer + 1.0) * 0.5,
                    (position[1] / outer + 1.0) * 0.5,
                ],
            });
        }
    }

    let mut indices = Vec::with_capacity((theta_segments * phi_segments * 6) as usize);
    for icr in 1..rows {
        for ict in 1..columns {
            let a = (icr - 1) * columns + (ict - 1);
            let b = icr * columns + (ict - 1);
            let c = b + 1;
            let d = a + 1;
            indices.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }

    (vertices, indices)
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// `SUN_PROPS` (`sceneProps.js:153-159`).
const SUN_PROPS: &[SceneProp] = &[
    SceneProp::float("surfaceSpeed", "Surface Speed", 1.0, 0.0, 4.0, 0.01),
    SceneProp::float("brightness", "Brightness", 1.0, 0.0, 3.0, 0.01),
    SceneProp::float("corona", "Corona", 1.0, 0.0, 3.0, 0.01),
    SceneProp::float("rays", "God Rays", 1.0, 0.0, 3.0, 0.01),
];

/// The shared mesh uniform block (surface + corona materials, 96 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SunMeshUniforms {
    view_proj: [[f32; 4]; 4],
    /// `gIn1` — x carries `surface` / `corona`.
    g_in1: [f32; 4],
    /// `gTime` — `this.time * surfaceSpeed`.
    g_time: f32,
    _pad: [f32; 3],
}

/// A `vec2` resolution + padding uniform for the fullscreen passes (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SunPassUniforms {
    resolution: [f32; 2],
    _pad: [f32; 2],
}

/// The blur pass uniform block (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SunBlurUniforms {
    resolution: [f32; 2],
    /// `gDir`.
    dir: [f32; 2],
    /// `gTexel`.
    texel: [f32; 2],
    _pad: [f32; 2],
}

/// The tint pass uniform block (`gColor1`, 32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SunTintUniforms {
    resolution: [f32; 2],
    _pad: [f32; 2],
    /// `gColor1` — `(tint, 1)`.
    g_color1: [f32; 4],
}

/// The god-ray pass uniform block (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SunScreenUniforms {
    resolution: [f32; 2],
    /// `gRayWeight`.
    g_ray_weight: f32,
    /// `gBrightness`.
    g_brightness: f32,
}

/// The intermediate targets, rebuilt on size change (`Sun.js:442-508`).
struct SunTargets {
    _sun_texture: wgpu::Texture,
    sun_view: wgpu::TextureView,
    _corona_texture: wgpu::Texture,
    corona_view: wgpu::TextureView,
    _tint_texture: wgpu::Texture,
    tint_view: wgpu::TextureView,
    _blur_aux_texture: wgpu::Texture,
    blur_aux_view: wgpu::TextureView,
    mips: [(wgpu::Texture, wgpu::TextureView); 5],
    copy_bind_group: wgpu::BindGroup,
    /// One bind group per blur source: index 0-4 sample `mips[i]`, index 5
    /// samples `blur_aux`.
    blur_bind_groups: [wgpu::BindGroup; 6],
    composite_bind_group: wgpu::BindGroup,
    tint_bind_group: wgpu::BindGroup,
    screen_bind_group: wgpu::BindGroup,
    copy_uniforms: wgpu::Buffer,
    blur_uniforms: wgpu::Buffer,
    composite_uniforms: wgpu::Buffer,
    tint_uniforms: wgpu::Buffer,
    screen_uniforms: wgpu::Buffer,
}

/// GPU resources created in `init`.
struct SunGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    sampler: wgpu::Sampler,
    surface_pipeline: wgpu::RenderPipeline,
    corona_pipeline: wgpu::RenderPipeline,
    copy_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    tint_pipeline: wgpu::RenderPipeline,
    /// Fullscreen pipelines are built against the destination format because
    /// the screen pass draws straight into the caller's view.
    screen_pipeline: wgpu::RenderPipeline,
    copy_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    tint_layout: wgpu::BindGroupLayout,
    screen_layout: wgpu::BindGroupLayout,
    surface_uniforms: wgpu::Buffer,
    corona_uniforms: wgpu::Buffer,
    surface_bind_group: wgpu::BindGroup,
    corona_bind_group: wgpu::BindGroup,
    sphere_vertices: wgpu::Buffer,
    sphere_indices: wgpu::Buffer,
    sphere_index_count: u32,
    ring_vertices: wgpu::Buffer,
    ring_indices: wgpu::Buffer,
    ring_index_count: u32,
    targets: SunTargets,
}

/// Sun scene — the Plane9 "Sun" preset.
pub struct Sun {
    audio: P10Audio,
    /// `this.time` (advanced by [`frame_dt`]).
    time: f32,
    surface_speed: f32,
    brightness_scale: f32,
    corona_scale: f32,
    rays_scale: f32,
    /// `uniforms.gIn1.value.x` of the surface material — stored so tests can
    /// read the live drive without reaching into the GPU.
    surface_level: f32,
    corona_level: f32,
    ray_weight: f32,
    size: (u32, u32),
    /// Size the current [`SunTargets`] were built for, or `None` at init.
    targets_size: Option<(u32, u32)>,
    gpu: Option<SunGpu>,
}

impl Default for Sun {
    fn default() -> Self {
        Self::new()
    }
}

impl Sun {
    /// Create the scene (`Sun.js:248-260` defaults).
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            time: 0.0,
            surface_speed: 1.0,
            brightness_scale: 1.0,
            corona_scale: 1.0,
            rays_scale: 1.0,
            surface_level: 0.0,
            corona_level: 0.0,
            ray_weight: SUN.rays.weight,
            size: (1, 1),
            targets_size: None,
            gpu: None,
        }
    }

    /// Apply the resolved modV props (`sceneProps.js:296-301`).
    pub fn apply_props(&mut self, get: impl FnMut(&str) -> Option<f32>) {
        for (key, value) in read_scene_props(SUN_PROPS, get) {
            match key {
                "surfaceSpeed" => self.surface_speed = value,
                "brightness" => self.brightness_scale = value,
                "corona" => self.corona_scale = value,
                "rays" => self.rays_scale = value,
                _ => {}
            }
        }
    }

    /// `this.surfaceSpeed`.
    pub fn surface_speed(&self) -> f32 {
        self.surface_speed
    }

    /// `this.brightnessScale`.
    pub fn brightness_scale(&self) -> f32 {
        self.brightness_scale
    }

    /// `this.coronaScale`.
    pub fn corona_scale(&self) -> f32 {
        self.corona_scale
    }

    /// `this.raysScale`.
    pub fn rays_scale(&self) -> f32 {
        self.rays_scale
    }

    /// The surface Expression level driving `gIn1.x`.
    pub fn surface_level(&self) -> f32 {
        self.surface_level
    }

    /// The corona Expression level driving `gIn1.x`.
    pub fn corona_level(&self) -> f32 {
        self.corona_level
    }

    /// The god-ray weight (`gRayWeight`).
    pub fn ray_weight(&self) -> f32 {
        self.ray_weight
    }

    /// Scene time of the last `update`.
    pub fn time(&self) -> f32 {
        self.time
    }

    fn camera_matrix(&self) -> [[f32; 4]; 4] {
        use crate::point10::scenes::cube_mesh::{look_at, mat4_mul, perspective};
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let projection = perspective(SUN.fov.to_radians(), aspect, SUN.near, SUN.far);
        let view = look_at(SUN.camera_position, SUN.camera_look_at, [0.0, 1.0, 0.0]);
        mat4_mul(&projection, &view)
    }

    /// Rebuild the intermediate targets if the size changed
    /// (`Sun.js:442-508` — dispose and rebuild, min 2 px half sizes).
    fn rebuild_targets(&mut self) {
        let size = self.size;
        if self.targets_size == Some(size) {
            return;
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };

        gpu.targets = build_targets(
            &gpu.device,
            size,
            &gpu.sampler,
            &gpu.copy_layout,
            &gpu.blur_layout,
            &gpu.composite_layout,
            &gpu.tint_layout,
            &gpu.screen_layout,
        );
        let gpu = self.gpu.as_ref().expect("gpu was just rebuilt");
        write_pass_uniforms(gpu, size, self.ray_weight, self.brightness_scale);
        self.targets_size = Some(size);
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let view_proj = self.camera_matrix();
        let time = self.time * self.surface_speed;
        let surface = SunMeshUniforms {
            view_proj,
            g_in1: [self.surface_level, 0.0, 0.0, 0.0],
            g_time: time,
            _pad: [0.0; 3],
        };
        let corona = SunMeshUniforms {
            view_proj,
            g_in1: [self.corona_level, 0.0, 0.0, 0.0],
            g_time: time,
            _pad: [0.0; 3],
        };
        gpu.queue
            .write_buffer(&gpu.surface_uniforms, 0, bytemuck::bytes_of(&surface));
        gpu.queue
            .write_buffer(&gpu.corona_uniforms, 0, bytemuck::bytes_of(&corona));
        let screen = SunScreenUniforms {
            resolution: [self.size.0 as f32, self.size.1 as f32],
            g_ray_weight: self.ray_weight,
            g_brightness: self.brightness_scale,
        };
        gpu.queue
            .write_buffer(&gpu.targets.screen_uniforms, 0, bytemuck::bytes_of(&screen));
    }
}

/// Create the five mips, the aux, the fullscreen targets and the matching
/// bind groups for `size`.
#[allow(clippy::too_many_arguments)]
fn build_targets(
    device: &wgpu::Device,
    size: (u32, u32),
    sampler: &wgpu::Sampler,
    copy_layout: &wgpu::BindGroupLayout,
    blur_layout: &wgpu::BindGroupLayout,
    composite_layout: &wgpu::BindGroupLayout,
    tint_layout: &wgpu::BindGroupLayout,
    screen_layout: &wgpu::BindGroupLayout,
) -> SunTargets {
    use crate::point10::postfx::create_target;

    let half_w = (size.0 >> 1).max(2);
    let half_h = (size.1 >> 1).max(2);

    let (sun_texture, sun_view) = create_target(device, "point10-sun-target", size.0, size.1);
    let (corona_texture, corona_view) =
        create_target(device, "point10-sun-corona-target", size.0, size.1);
    let (tint_texture, tint_view) =
        create_target(device, "point10-sun-tint-target", size.0, size.1);
    let (blur_aux_texture, blur_aux_view) =
        create_target(device, "point10-sun-blur-aux", half_w, half_h);

    let mut mips = Vec::with_capacity(5);
    for i in 0..5_u32 {
        let w = (half_w >> i).max(2);
        let h = (half_h >> i).max(2);
        mips.push(create_target(device, &format!("point10-sun-mip-{i}"), w, h));
    }
    let mips: [(wgpu::Texture, wgpu::TextureView); 5] =
        mips.try_into().expect("five mips were pushed");

    let copy_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("point10-sun-copy-uniforms"),
        size: std::mem::size_of::<SunPassUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let blur_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("point10-sun-blur-uniforms"),
        size: std::mem::size_of::<SunBlurUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let composite_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("point10-sun-composite-uniforms"),
        size: std::mem::size_of::<SunPassUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let tint_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("point10-sun-tint-uniforms"),
        size: std::mem::size_of::<SunTintUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let screen_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("point10-sun-screen-uniforms"),
        size: std::mem::size_of::<SunScreenUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let copy_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point10-sun-copy-bind-group"),
        layout: copy_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: copy_uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&sun_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    let blur_sources: [&wgpu::TextureView; 6] = [
        &mips[0].1,
        &mips[1].1,
        &mips[2].1,
        &mips[3].1,
        &mips[4].1,
        &blur_aux_view,
    ];
    let blur_bind_groups: [wgpu::BindGroup; 6] = blur_sources.map(|source| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-sun-blur-bind-group"),
            layout: blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: blur_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    });
    let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point10-sun-composite-bind-group"),
        layout: composite_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: composite_uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&sun_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&mips[0].1),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&mips[1].1),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&mips[2].1),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&mips[3].1),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&mips[4].1),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    let tint_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point10-sun-tint-bind-group"),
        layout: tint_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: tint_uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&corona_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point10-sun-screen-bind-group"),
        layout: screen_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&tint_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&corona_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });

    SunTargets {
        _sun_texture: sun_texture,
        sun_view,
        _corona_texture: corona_texture,
        corona_view,
        _tint_texture: tint_texture,
        tint_view,
        _blur_aux_texture: blur_aux_texture,
        blur_aux_view,
        mips,
        copy_bind_group,
        blur_bind_groups,
        composite_bind_group,
        tint_bind_group,
        screen_bind_group,
        copy_uniforms,
        blur_uniforms,
        composite_uniforms,
        tint_uniforms,
        screen_uniforms,
    }
}

/// Write the resolution-sized uniforms for the current targets
/// (`Sun.js:511-514` — the fullscreen passes cover the destination
/// 1:1, so `uv = position / resolution`).
fn write_pass_uniforms(gpu: &SunGpu, size: (u32, u32), ray_weight: f32, brightness: f32) {
    let half = [(size.0 >> 1).max(2) as f32, (size.1 >> 1).max(2) as f32];
    let full = [size.0 as f32, size.1 as f32];
    let write = |buffer: &wgpu::Buffer, value: SunPassUniforms| {
        gpu.queue
            .write_buffer(buffer, 0, bytemuck::bytes_of(&value));
    };
    // Copy pass renders into `mips[0]` (half res).
    write(
        &gpu.targets.copy_uniforms,
        SunPassUniforms {
            resolution: half,
            _pad: [0.0; 2],
        },
    );
    write(
        &gpu.targets.composite_uniforms,
        SunPassUniforms {
            resolution: full,
            _pad: [0.0; 2],
        },
    );
    let tint = SunTintUniforms {
        resolution: full,
        _pad: [0.0; 2],
        g_color1: [SUN.tint[0], SUN.tint[1], SUN.tint[2], 1.0],
    };
    gpu.queue
        .write_buffer(&gpu.targets.tint_uniforms, 0, bytemuck::bytes_of(&tint));
    let screen = SunScreenUniforms {
        resolution: full,
        g_ray_weight: ray_weight,
        g_brightness: brightness,
    };
    gpu.queue
        .write_buffer(&gpu.targets.screen_uniforms, 0, bytemuck::bytes_of(&screen));
}

impl Point10Scene for Sun {
    fn name(&self) -> &'static str {
        "Point10 Sun"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        use wgpu::util::DeviceExt;

        let device = ctx.device.clone();
        let queue = ctx.queue.clone();
        self.size = (ctx.width.max(1), ctx.height.max(1));
        let size = self.size;

        let (sphere_vertices, sphere_indices) =
            build_sphere(SUN.radius, SUN.segments, SUN.segments);
        let (ring_vertices, ring_indices) =
            build_ring(SUN.ring_inner, SUN.ring_outer, SUN.ring_segments, 1);

        let sphere_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-sun-sphere-vertices"),
            contents: bytemuck::cast_slice(&sphere_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let sphere_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-sun-sphere-indices"),
            contents: bytemuck::cast_slice(&sphere_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ring_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-sun-ring-vertices"),
            contents: bytemuck::cast_slice(&ring_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ring_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-sun-ring-indices"),
            contents: bytemuck::cast_slice(&ring_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let sampler = crate::point10::postfx::linear_sampler(&device, "point10-sun-sampler");

        let mesh_bind_group_layout = |label: &str| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
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
            })
        };
        let surface_layout = mesh_bind_group_layout("point10-sun-surface-layout");
        let corona_layout = mesh_bind_group_layout("point10-sun-corona-layout");

        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let uniform_entry = |binding: u32, stage: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: stage,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        let copy_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-sun-copy-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-sun-blur-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-sun-composite-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                texture_entry(5),
                texture_entry(6),
                sampler_entry(7),
            ],
        });
        let tint_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-sun-tint-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-sun-screen-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                texture_entry(2),
                sampler_entry(3),
            ],
        });

        let surface_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-sun-surface-uniforms"),
            size: std::mem::size_of::<SunMeshUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let corona_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-sun-corona-uniforms"),
            size: std::mem::size_of::<SunMeshUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let surface_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-sun-surface-bind-group"),
            layout: &surface_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: surface_uniforms.as_entire_binding(),
            }],
        });
        let corona_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-sun-corona-bind-group"),
            layout: &corona_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: corona_uniforms.as_entire_binding(),
            }],
        });

        let surface_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-sun-surface-shader"),
            source: wgpu::ShaderSource::Wgsl(surface_wgsl().into()),
        });
        let corona_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-sun-corona-shader"),
            source: wgpu::ShaderSource::Wgsl(corona_wgsl().into()),
        });

        let surface_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-sun-surface-pipeline-layout"),
                bind_group_layouts: &[Some(&surface_layout)],
                immediate_size: 0,
            });
        let corona_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-sun-corona-pipeline-layout"),
                bind_group_layouts: &[Some(&corona_layout)],
                immediate_size: 0,
            });

        let surface_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-sun-surface-pipeline"),
            layout: Some(&surface_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &surface_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(SunVertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // three's `FrontSide` sphere: outward CCW triangles are built
                // in `build_sphere` and back faces are culled here.
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &surface_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::point10::HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let corona_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-sun-corona-pipeline"),
            layout: Some(&corona_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &corona_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(SunVertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // The JS material is `DoubleSide`.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &corona_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::point10::HDR_FORMAT,
                    // `blendSrc: OneFactor, blendDst: OneFactor` (both colour
                    // and alpha), `Sun.js:282-288`.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Fullscreen passes (`Sun.js:372-380`) --------------------------
        let copy_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-sun-copy-shader"),
            source: wgpu::ShaderSource::Wgsl(copy_wgsl().into()),
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-sun-blur-shader"),
            source: wgpu::ShaderSource::Wgsl(blur_wgsl().into()),
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-sun-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(composite_wgsl().into()),
        });
        let tint_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-sun-tint-shader"),
            source: wgpu::ShaderSource::Wgsl(tint_wgsl().into()),
        });
        let screen_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-sun-screen-shader"),
            source: wgpu::ShaderSource::Wgsl(screen_wgsl().into()),
        });

        let pass_pipeline_layout = |label: &str, layout: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            })
        };
        let copy_pipeline = ctx.fullscreen_pipeline_with_format(
            "point10-sun-copy-pipeline",
            &copy_shader,
            &pass_pipeline_layout("point10-sun-copy-pipeline-layout", &copy_layout),
            crate::point10::HDR_FORMAT,
            None,
        );
        let blur_pipeline = ctx.fullscreen_pipeline_with_format(
            "point10-sun-blur-pipeline",
            &blur_shader,
            &pass_pipeline_layout("point10-sun-blur-pipeline-layout", &blur_layout),
            crate::point10::HDR_FORMAT,
            None,
        );
        let composite_pipeline = ctx.fullscreen_pipeline_with_format(
            "point10-sun-composite-pipeline",
            &composite_shader,
            &pass_pipeline_layout("point10-sun-composite-pipeline-layout", &composite_layout),
            crate::point10::HDR_FORMAT,
            None,
        );
        let tint_pipeline = ctx.fullscreen_pipeline_with_format(
            "point10-sun-tint-pipeline",
            &tint_shader,
            &pass_pipeline_layout("point10-sun-tint-pipeline-layout", &tint_layout),
            crate::point10::HDR_FORMAT,
            None,
        );
        // The screen pass presents into the caller's destination view, so it
        // is built against the destination format (like the tunnels).
        let screen_pipeline = ctx.fullscreen_pipeline_with_format(
            "point10-sun-screen-pipeline",
            &screen_shader,
            &pass_pipeline_layout("point10-sun-screen-pipeline-layout", &screen_layout),
            ctx.target_format,
            None,
        );

        let sphere_index_count = sphere_indices.len() as u32;
        let ring_index_count = ring_indices.len() as u32;

        let targets = build_targets(
            &device,
            size,
            &sampler,
            &copy_layout,
            &blur_layout,
            &composite_layout,
            &tint_layout,
            &screen_layout,
        );
        let gpu = SunGpu {
            device: device.clone(),
            queue: queue.clone(),
            sampler,
            surface_pipeline,
            corona_pipeline,
            copy_pipeline,
            blur_pipeline,
            composite_pipeline,
            tint_pipeline,
            screen_pipeline,
            copy_layout,
            blur_layout,
            composite_layout,
            tint_layout,
            screen_layout,
            surface_uniforms,
            corona_uniforms,
            surface_bind_group,
            corona_bind_group,
            sphere_vertices: sphere_vertex_buffer,
            sphere_indices: sphere_index_buffer,
            sphere_index_count,
            ring_vertices: ring_vertex_buffer,
            ring_indices: ring_index_buffer,
            ring_index_count,
            targets,
        };
        self.gpu = Some(gpu);
        self.targets_size = Some(size);
        if let Some(gpu) = self.gpu.as_ref() {
            write_pass_uniforms(gpu, size, self.ray_weight, self.brightness_scale);
        }
        self.upload_uniforms();
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width.max(1), height.max(1));
        self.rebuild_targets();
        self.upload_uniforms();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.time += dt;
        self.audio.update(dt, audio);

        let bass = self
            .audio
            .band(-1, SUN.surface.damping, SUN.surface.band, 0.1);
        // `Sun.js:423`: `sunSurfaceLevel(bass, 1)` — no brightness lever here.
        self.surface_level = sun_surface_level(bass, 1.0);

        let corona_bass = self
            .audio
            .band(-1, SUN.corona.damping, SUN.corona.band, 0.05);
        self.corona_level = sun_corona_level(corona_bass, self.corona_scale);

        self.ray_weight = sun_ray_weight(self.rays_scale);
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        self.rebuild_targets();
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };

        // 1. Scene group into `sunTarget` (`Sun.js:549-553`).
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-sun-scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.targets.sun_view,
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
            pass.set_pipeline(&gpu.surface_pipeline);
            pass.set_bind_group(0, &gpu.surface_bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.sphere_vertices.slice(..));
            pass.set_index_buffer(gpu.sphere_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.sphere_index_count, 0, 0..1);
            pass.set_pipeline(&gpu.corona_pipeline);
            pass.set_bind_group(0, &gpu.corona_bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.ring_vertices.slice(..));
            pass.set_index_buffer(gpu.ring_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.ring_index_count, 0, 0..1);
        }

        // 2. `_blurPyramid` (`Sun.js:516-535`) — deliberately not optimised.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-sun-copy"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.targets.mips[0].1,
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
            pass.set_pipeline(&gpu.copy_pipeline);
            pass.set_bind_group(0, &gpu.targets.copy_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        let half = [(self.size.0 >> 1).max(2), (self.size.1 >> 1).max(2)];
        for i in 0..4_u32 {
            let from_index = i;
            let to_index = i + 1;
            let from_width = (half[0] >> from_index).max(2);
            let to_width = (half[0] >> to_index).max(2);
            let to_height = (half[1] >> to_index).max(2);
            let spread = blur_spread(from_width, to_width);

            // Horizontal into the fixed half-res aux.
            let horizontal = SunBlurUniforms {
                resolution: [half[0] as f32, half[1] as f32],
                dir: [1.0, 0.0],
                texel: [spread / to_width as f32, 0.0],
                _pad: [0.0; 2],
            };
            gpu.queue.write_buffer(
                &gpu.targets.blur_uniforms,
                0,
                bytemuck::bytes_of(&horizontal),
            );
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("point10-sun-blur-horizontal"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &gpu.targets.blur_aux_view,
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
                pass.set_pipeline(&gpu.blur_pipeline);
                pass.set_bind_group(0, &gpu.targets.blur_bind_groups[from_index as usize], &[]);
                pass.draw(0..3, 0..1);
            }

            // Vertical into `mips[i + 1]`.
            let vertical = SunBlurUniforms {
                resolution: [to_width as f32, to_height as f32],
                dir: [0.0, 1.0],
                texel: [0.0, spread / to_height as f32],
                _pad: [0.0; 2],
            };
            gpu.queue
                .write_buffer(&gpu.targets.blur_uniforms, 0, bytemuck::bytes_of(&vertical));
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("point10-sun-blur-vertical"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &gpu.targets.mips[to_index as usize].1,
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
                pass.set_pipeline(&gpu.blur_pipeline);
                // Vertical pass samples the aux (`blur_bind_groups[5]`).
                pass.set_bind_group(0, &gpu.targets.blur_bind_groups[5], &[]);
                pass.draw(0..3, 0..1);
            }
        }

        // 3. `coronaComposite` into `coronaTarget` (`Sun.js:555`).
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-sun-composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.targets.corona_view,
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
            pass.set_bind_group(0, &gpu.targets.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // 4. `tint` into `tintTarget` (`Sun.js:556`).
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-sun-tint"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.targets.tint_view,
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
            pass.set_pipeline(&gpu.tint_pipeline);
            pass.set_bind_group(0, &gpu.targets.tint_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // 5. The god-ray screen pass presents into the destination view
        //    (`Sun.js:557` — `_pass(this.screenMaterial, null)`).
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-sun-screen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(&gpu.screen_pipeline);
            pass.set_bind_group(0, &gpu.targets.screen_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.targets_size = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(SUN_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_scene("Point10 Sun", "3d-scenes")
    }
}

// ---------------------------------------------------------------------------
// WGSL
// ---------------------------------------------------------------------------

/// `_ridgedmffast(vec3 p, float octaves)` (`P9Glsl.js:100-113`), inlined here
/// because `P9_COMMON_WGSL` does not carry it.
///
/// `lacunarity = 2.13628142`, `gain = 0.5`, `offset = 1.0`, `amp` starts at
/// `0.5`, and each octave accumulates `n * amp * prev` where `n` is the
/// squared inverted absolute noise. This is the *smooth* ridged noise the
/// preset calls — not the `4 * v * v` Musgrave variant.
const SUN_RIDGEDWGSL: &str = r#"
fn sun_ridgedmffast(p0: vec3<f32>, octaves: u32) -> f32 {
  let lacunarity = 2.13628142;
  let gain = 0.5;
  let offset = 1.0;
  var sum = 0.0;
  var amp_sum = 0.0;
  var freq = 1.0;
  var amp = 0.5;
  var prev = 1.0;
  for (var i = 0u; i < octaves; i = i + 1u) {
    var n = offset - abs(p9_noise(p0 * freq));
    n = n * n;
    sum = sum + n * amp * prev;
    amp_sum = amp_sum + amp;
    prev = n;
    freq = freq * lacunarity;
    amp = amp * gain;
  }
  return sum / amp_sum;
}
"#;

/// The mesh uniform block shared by the surface and corona materials.
const MESH_UNIFORMS_WGSL: &str = r#"
// WGSL packs scalars tightly but pads structs to the largest member's
// alignment, so the time vector is written as four scalars (96 bytes total,
// matching `SunMeshUniforms` on the CPU side).
struct SunMeshUniforms {
  view_proj: mat4x4<f32>,
  g_in1: vec4<f32>,
  g_time: f32,
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
}
@group(0) @binding(0) var<uniform> u: SunMeshUniforms;
"#;

/// The fullscreen pass uniform block (`uv = position / resolution`).
const PASS_UNIFORMS_WGSL: &str = r#"
struct SunPassUniforms {
  resolution: vec2<f32>,
  _pad: vec2<f32>,
}
@group(0) @binding(0) var<uniform> u: SunPassUniforms;
"#;

/// Surface shader source (`Sun.js:79-107`).
fn surface_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{SUN_RIDGEDWGSL}\n{MESH_UNIFORMS_WGSL}\n{SURFACE_WGSL}")
}

const SURFACE_WGSL: &str = r#"
struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) world: vec3<f32>,
}

@vertex
fn vs_main(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
) -> VsOut {
  var out: VsOut;
  // The group has no transform (`this.group.add`, group at the origin), so
  // world = position — `vWpos = position` (`Sun.js:85`).
  out.position = u.view_proj * vec4<f32>(position, 1.0);
  out.world = position;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  // `v = _ridgedmffast(vWpos*15.0 + vec3(0.0, 0.0, gTime*0.2), 3.0)` (`:101`).
  var v = sun_ridgedmffast(in.world * 15.0 + vec3<f32>(0.0, 0.0, u.g_time * 0.2), 3u);
  // Sunspots — `smoothstep(1.0, 0.6, _noise(vWpos*3.0 - vec3(0.0, 0.0, gTime*0.2))*0.5+0.5)` (`:103`).
  v *= p9_smoothstep(
    1.0,
    0.6,
    p9_noise(in.world * 3.0 - vec3<f32>(0.0, 0.0, u.g_time * 0.2)) * 0.5 + 0.5,
  );
  // `gl_FragColor = vec4(vec3(v*gIn1.x), 1.0)` (`:104`).
  return vec4<f32>(vec3<f32>(v * u.g_in1.x), 1.0);
}
"#;

/// Corona shader source (`Sun.js:109-136`).
fn corona_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{SUN_RIDGEDWGSL}\n{MESH_UNIFORMS_WGSL}\n{CORONA_WGSL}")
}

const CORONA_WGSL: &str = r#"
struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
) -> VsOut {
  var out: VsOut;
  out.position = u.view_proj * vec4<f32>(position, 1.0);
  out.uv = uv;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  // `vec2 p = vTex*vec2(2.0)-vec2(1.0)` (`Sun.js:128`).
  let p = in.uv * 2.0 - vec2<f32>(1.0);
  // `float v1 = _ridgedmffast(vec3(vTex*50.0, gTime*0.5), 5.0)` (`:129`).
  let v1 = sun_ridgedmffast(vec3<f32>(in.uv * 50.0, u.g_time * 0.5), 5u);
  // `float v2 = smoothstep(0.7, 1.0, _turbulencefast(vec3(vTex*10.0, gTime*0.2), 3.0)+0.4)` (`:130`).
  let v2 = p9_smoothstep(
    0.7,
    1.0,
    p9_turbulence(vec3<f32>(in.uv * 10.0, u.g_time * 0.2), 3u) + 0.4,
  );
  // `float v3 = smoothstep(1.0, 0.9, length(p))` (`:131`) — reversed edges.
  let v3 = p9_smoothstep(1.0, 0.9, length(p));
  // `float v = v1*v2*v3*gIn1.x` (`:132`).
  let v = v1 * v2 * v3 * u.g_in1.x;
  // `gl_FragColor = vec4(vec3(v), 0.0)` (`:133`) — the additive ring adds no alpha.
  return vec4<f32>(vec3<f32>(v), 0.0);
}
"#;

/// Copy pass source (`Sun.js:157-166`) — `_mips[0]` downsample.
fn copy_wgsl() -> String {
    format!("{FULLSCREEN_TRIANGLE_WGSL}\n{PASS_UNIFORMS_WGSL}\n{COPY_WGSL}")
}

const COPY_WGSL: &str = r#"
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = position.xy / u.resolution;
  return textureSample(src_texture, src_sampler, uv);
}
"#;

/// Blur pass source (`Sun.js:138-155`) — separable gaussian, 21 taps.
///
/// The weights are baked in as a local array literal because WGSL uniform
/// arrays need a 16-byte stride; the numbers come from [`blur_weights`] (the
/// JS's `exp(-0.5 * d*d / sigma^2)`, normalised).
fn blur_wgsl() -> String {
    let weights = blur_weights();
    let literal = weights
        .iter()
        .map(|weight| format!("{weight:e}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{FULLSCREEN_TRIANGLE_WGSL}\nstruct SunBlurUniforms {{\n  resolution: vec2<f32>,\n  dir: vec2<f32>,\n  texel: vec2<f32>,\n  _pad: vec2<f32>,\n}}\n@group(0) @binding(0) var<uniform> u: SunBlurUniforms;\n@group(0) @binding(1) var src_texture: texture_2d<f32>;\n@group(0) @binding(2) var src_sampler: sampler;\n\n@fragment\nfn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{\n  let weights = array<f32, 21>({literal});\n  let uv = position.xy / u.resolution;\n  var c = vec3<f32>(0.0);\n  for (var i = 0; i < 21; i = i + 1) {{\n    let offset = u.dir * u.texel * f32(i - 10);\n    c += textureSample(src_texture, src_sampler, uv + offset).rgb * weights[i];\n  }}\n  return vec4<f32>(c, 1.0);\n}}\n"
    )
}

/// `coronaComposite` source (`Sun.js:168-190`).
fn composite_wgsl() -> String {
    format!("{FULLSCREEN_TRIANGLE_WGSL}\n{PASS_UNIFORMS_WGSL}\n{COMPOSITE_WGSL}")
}

const COMPOSITE_WGSL: &str = r#"
@group(0) @binding(1) var sun_target: texture_2d<f32>;
@group(0) @binding(2) var mip0: texture_2d<f32>;
@group(0) @binding(3) var mip1: texture_2d<f32>;
@group(0) @binding(4) var mip2: texture_2d<f32>;
@group(0) @binding(5) var mip3: texture_2d<f32>;
@group(0) @binding(6) var mip4: texture_2d<f32>;
@group(0) @binding(7) var pass_sampler: sampler;

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = position.xy / u.resolution;
  var c1 = textureSample(mip0, pass_sampler, uv) * 0.333;
  c1 += textureSample(mip1, pass_sampler, uv) * 0.267;
  c1 += textureSample(mip2, pass_sampler, uv) * 0.2;
  c1 += textureSample(mip3, pass_sampler, uv) * 0.133;
  c1 += textureSample(mip4, pass_sampler, uv) * 0.067;
  let c2 = textureSample(sun_target, pass_sampler, uv);
  // `vec4(c1.rrr*3.0+c2.rgb*0.9, c2.a)` (`:187`).
  return vec4<f32>(c1.rrr * 3.0 + c2.rgb * 0.9, c2.a);
}
"#;

/// `tint` source (`Sun.js:192-202`).
fn tint_wgsl() -> String {
    format!("{FULLSCREEN_TRIANGLE_WGSL}\n{TINT_WGSL}")
}

const TINT_WGSL: &str = r#"
struct SunTintUniforms {
  resolution: vec2<f32>,
  _pad: vec2<f32>,
  g_color1: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: SunTintUniforms;
@group(0) @binding(1) var corona_texture: texture_2d<f32>;
@group(0) @binding(2) var corona_sampler: sampler;

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = position.xy / u.resolution;
  // `gl_FragColor = gColor1*texture2D(gTexture1, vUv).r` (`:199`).
  return u.g_color1 * textureSample(corona_texture, corona_sampler, uv).r;
}
"#;

/// God-ray screen source (`Sun.js:204-240`).
///
/// The light position `gIn1 = (0.5, 0.5, 0, 0)` is a const: the JS never
/// updates `screenMaterial.uniforms.gIn1` (the Expression only writes
/// `gRayWeight`/`gBrightness`), so there is nothing to bind.
fn screen_wgsl() -> String {
    format!(
        "{FULLSCREEN_TRIANGLE_WGSL}\nstruct SunScreenUniforms {{\n  resolution: vec2<f32>,\n  g_ray_weight: f32,\n  g_brightness: f32,\n}}\n@group(0) @binding(0) var<uniform> u: SunScreenUniforms;\n@group(0) @binding(1) var tint_texture: texture_2d<f32>;\n@group(0) @binding(2) var corona_texture: texture_2d<f32>;\n@group(0) @binding(3) var ray_sampler: sampler;\n\nconst RAY_LIGHT_POS: vec2<f32> = vec2<f32>(0.5, 0.5);\nconst RAY_DENSITY: f32 = 1.0;\nconst RAY_DECAY: f32 = 0.96;\nconst RAY_NUM_SAMPLES: u32 = 50u;\n\n@fragment\nfn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{\n  let uv = position.xy / u.resolution;\n  var tex_pos = uv;\n  var tex_delta = tex_pos - RAY_LIGHT_POS;\n  tex_delta *= (1.0 / f32(RAY_NUM_SAMPLES)) * RAY_DENSITY;\n\n  var c = textureSample(tint_texture, ray_sampler, tex_pos).rgb;\n  var illumination_decay = 1.0;\n  for (var i = 0u; i < RAY_NUM_SAMPLES; i = i + 1u) {{\n    tex_pos -= tex_delta;\n    let s = textureSample(tint_texture, ray_sampler, tex_pos).rgb;\n    c += s * illumination_decay * u.g_ray_weight;\n    illumination_decay *= RAY_DECAY;\n  }}\n\n  let src = textureSample(corona_texture, ray_sampler, uv);\n  c += src.rgb * 0.8;\n\n  // `out = c*c*gBrightness` with `NoToneMapping` — the final pass presents raw.\n  let out_color = c * c * u.g_brightness;\n  return vec4<f32>(\n    out_color,\n    clamp(max(max(out_color.r, out_color.g), out_color.b), 0.0, 1.0),\n  );\n}}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::ScenePropKind;
    use crate::point10::audio_bridge::{AUDIO_SAMPLES, BandPayload};
    use crate::point10::test_support::{TestTarget, any_lit, gpu};
    use crate::point10::{MAX_FRAME_SECONDS, audio_bridge::AudioBridge};

    // Port of tests/unit/point10-sun.test.js.

    #[test]
    fn preset_constants_match_the_js() {
        assert_eq!(SUN.fov, 45.0);
        assert_eq!(SUN.near, 0.1);
        assert_eq!(SUN.far, 1000.0);
        assert_eq!(SUN.camera_position, [0.0, 0.0, 3.8]);
        assert_eq!(SUN.camera_look_at, [0.0, 0.0, -1.0]);
        assert_eq!(SUN.radius, 1.0);
        assert_eq!(SUN.segments, 32);
        assert_eq!(SUN.ring_inner, 1.0);
        assert_eq!(SUN.ring_outer, 1.1);
        assert_eq!(SUN.ring_segments, 32);

        assert_eq!(SUN.rays.samples, 50);
        assert_eq!(SUN.rays.density, 1.0);
        assert_eq!(SUN.rays.weight, 0.2);
        assert_eq!(SUN.rays.decay, 0.96);

        assert_eq!(SUN.tint, [0.882, 0.439, 0.024]);
        assert_eq!(SUN.surface.floor, 0.03);
        assert_eq!(SUN.surface.gain, 1.0);
        assert_eq!(SUN.surface.ceiling, 0.12);
        assert_eq!(SUN.surface.damping, 2);
        assert_eq!(SUN.surface.band, 0);
        assert_eq!(SUN.corona.floor, 0.1);
        assert_eq!(SUN.corona.gain, 20.0);
        assert_eq!(SUN.corona.ceiling, 0.5);
        assert_eq!(SUN.corona.damping, 2);
        assert_eq!(SUN.corona.band, 0);
        assert_eq!(SUN.blur.taps, 21);
        assert_eq!(SUN.blur.sigma, 1.2);
        assert_eq!(SUN.blur.width, 4.0);
    }

    #[test]
    fn surface_level_keeps_the_preset_floor_gain_and_ceiling() {
        assert!((sun_surface_level(0.0, 1.0) - 0.03).abs() < 1e-6);
        assert!((sun_surface_level(0.05, 1.0) - 0.08).abs() < 1e-6);
        assert_eq!(sun_surface_level(1.0, 1.0), 0.12);
        assert!((sun_surface_level(-1.0, 1.0) - 0.03).abs() < 1e-6);
    }

    #[test]
    fn surface_level_scales_with_the_module_input() {
        assert!((sun_surface_level(0.05, 2.0) - 0.16).abs() < 1e-6);
        assert_eq!(sun_surface_level(1.0, 0.0), 0.0);
        assert_eq!(sun_surface_level(1.0, -1.0), 0.0);
    }

    #[test]
    fn surface_level_sanitises_non_finite_input() {
        assert!((sun_surface_level(f32::NAN, 1.0) - 0.03).abs() < 1e-6);
        // Infinity is not finite, so it clamps like the JS `Number.isFinite`.
        assert!((sun_surface_level(f32::INFINITY, 1.0) - 0.03).abs() < 1e-6);
        assert!((sun_surface_level(f32::NEG_INFINITY, 1.0) - 0.03).abs() < 1e-6);
        // Non-finite scale clamps to 0 (Rust's f32::max drops NaN).
        assert_eq!(sun_surface_level(0.05, f32::NAN), 0.0);
        assert!(sun_surface_level(0.05, f32::INFINITY).is_infinite());
    }

    #[test]
    fn corona_level_keeps_the_preset_floor_gain_and_ceiling() {
        assert!((sun_corona_level(0.0, 1.0) - 0.1).abs() < 1e-6);
        assert!((sun_corona_level(0.01, 1.0) - 0.3).abs() < 1e-6);
        assert_eq!(sun_corona_level(1.0, 1.0), 0.5);
        assert!((sun_corona_level(-1.0, 1.0) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn corona_level_scales_with_the_module_input() {
        assert!((sun_corona_level(0.01, 2.0) - 0.6).abs() < 1e-6);
        assert_eq!(sun_corona_level(1.0, 0.0), 0.0);
    }

    #[test]
    fn corona_level_sanitises_non_finite_input() {
        assert!((sun_corona_level(f32::NAN, 1.0) - 0.1).abs() < 1e-6);
        // Infinity is not finite: falls back like the JS `Number.isFinite`.
        assert!((sun_corona_level(f32::INFINITY, 1.0) - 0.1).abs() < 1e-6);
        assert!((sun_corona_level(f32::NEG_INFINITY, 1.0) - 0.1).abs() < 1e-6);
        assert_eq!(sun_corona_level(0.01, f32::NAN), 0.0);
    }

    #[test]
    fn ray_weight_scales_the_preset_weight() {
        assert_eq!(sun_ray_weight(1.0), 0.2);
        assert!((sun_ray_weight(2.0) - 0.4).abs() < 1e-6);
        assert_eq!(sun_ray_weight(0.0), 0.0);
        assert_eq!(sun_ray_weight(-1.0), 0.0);
        assert_eq!(sun_ray_weight(f32::NAN), 0.0);
    }

    #[test]
    fn js_named_aliases_match_the_pure_functions() {
        assert_eq!(sunSurfaceLevel(0.05, 2.0), sun_surface_level(0.05, 2.0));
        assert_eq!(sunCoronaLevel(0.01, 2.0), sun_corona_level(0.01, 2.0));
        assert_eq!(sunRayWeight(2.0), sun_ray_weight(2.0));
    }

    #[test]
    fn blur_weights_are_normalised_and_symmetric() {
        let weights = blur_weights();
        let sum: f32 = weights.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "weights sum to {sum}");

        for i in 0..10 {
            assert!((weights[i] - weights[20 - i]).abs() < 1e-7);
        }

        // The JS computes `exp(-0.5 * d*d / sigma^2)` for d = i - 10 and then
        // normalises by the sum.
        let raw: Vec<f32> = (0..21)
            .map(|i| {
                let d = i as f32 - 10.0;
                (-0.5 * d * d / (SUN.blur.sigma * SUN.blur.sigma)).exp()
            })
            .collect();
        let raw_sum: f32 = raw.iter().sum();
        for (i, weight) in weights.iter().enumerate() {
            assert!((weight - raw[i] / raw_sum).abs() < 1e-7);
        }
    }

    #[test]
    fn scene_props_match_the_js_schema_in_order() {
        let scene = Sun::new();
        let props = scene.props();
        let own = &props[props.len() - 4..];

        assert_eq!(
            own.iter().map(|prop| prop.name).collect::<Vec<_>>(),
            ["surfaceSpeed", "brightness", "corona", "rays"]
        );
        assert_eq!(
            own.iter().map(|prop| prop.label).collect::<Vec<_>>(),
            ["Surface Speed", "Brightness", "Corona", "God Rays"]
        );

        let expected = [
            (1.0, 0.0, 4.0),
            (1.0, 0.0, 3.0),
            (1.0, 0.0, 3.0),
            (1.0, 0.0, 3.0),
        ];
        for (prop, &(default, min, max)) in own.iter().zip(expected.iter()) {
            assert_eq!(prop.kind, ScenePropKind::Float);
            assert_eq!(prop.default, default);
            assert_eq!(prop.min, min);
            assert_eq!(prop.max, max);
            assert_eq!(prop.step, 0.01);
        }

        // The responsiveness levers precede the scene schema.
        assert_eq!(props[0].name, "gain");
        assert_eq!(
            props.len(),
            crate::point10::audio_bridge::RESPONSIVENESS_PROPS.len() + 4
        );
    }

    #[test]
    fn meta_matches_the_js_registry() {
        let scene = Sun::new();
        assert_eq!(scene.meta().name, "Point10 Sun");
        assert_eq!(scene.meta().category, "3d-scenes");
        assert_eq!(scene.meta().audio_features, &["bass", "mid", "treble"]);
        assert_eq!(scene.name(), "Point10 Sun");
    }

    #[test]
    fn apply_props_binds_the_js_names() {
        let mut scene = Sun::new();
        scene.apply_props(|key| match key {
            "surfaceSpeed" => Some(2.5),
            "brightness" => Some(0.5),
            "corona" => Some(1.5),
            "rays" => Some(3.0),
            _ => None,
        });

        assert_eq!(scene.surface_speed(), 2.5);
        assert_eq!(scene.brightness_scale(), 0.5);
        assert_eq!(scene.corona_scale(), 1.5);
        assert_eq!(scene.rays_scale(), 3.0);
    }

    #[test]
    fn apply_props_clamps_into_the_schema_range() {
        let mut scene = Sun::new();
        scene.apply_props(|key| match key {
            "surfaceSpeed" => Some(9.0),
            "brightness" => Some(9.0),
            "corona" => Some(-2.0),
            "rays" => Some(9.0),
            _ => None,
        });

        assert_eq!(scene.surface_speed(), 4.0);
        assert_eq!(scene.brightness_scale(), 3.0);
        assert_eq!(scene.corona_scale(), 0.0);
        assert_eq!(scene.rays_scale(), 3.0);
    }

    #[test]
    fn apply_props_falls_back_to_defaults_for_missing_or_non_finite_values() {
        let mut scene = Sun::new();
        scene.apply_props(|key| match key {
            "surfaceSpeed" => Some(f32::NAN),
            "brightness" => None,
            "corona" => Some(f32::INFINITY),
            "rays" => None,
            _ => None,
        });

        assert_eq!(scene.surface_speed(), 1.0);
        assert_eq!(scene.brightness_scale(), 1.0);
        // Infinity is non-finite, so the schema read falls back to the default.
        assert_eq!(scene.corona_scale(), 1.0);
        assert_eq!(scene.rays_scale(), 1.0);
    }

    #[test]
    fn sphere_normals_point_outward() {
        let (vertices, indices) = build_sphere(1.0, 32, 32);
        assert_eq!(vertices.len(), 33 * 33);
        assert!(!indices.is_empty());
        assert!(indices.len() % 3 == 0);
        assert!(
            indices
                .iter()
                .all(|index| (*index as usize) < vertices.len())
        );

        for vertex in &vertices {
            let length = (vertex.normal[0] * vertex.normal[0]
                + vertex.normal[1] * vertex.normal[1]
                + vertex.normal[2] * vertex.normal[2])
                .sqrt();
            assert!((length - 1.0).abs() < 1e-4, "normal {vertex:?} not unit");

            let position_length = (vertex.position[0] * vertex.position[0]
                + vertex.position[1] * vertex.position[1]
                + vertex.position[2] * vertex.position[2])
                .sqrt();
            // Normals are normalised positions (radius 1 ⇒ same value).
            assert!((length - position_length).abs() < 1e-4);
        }

        // Every non-degenerate triangle's geometric normal agrees with the
        // vertex normals (outward winding).
        for triangle in indices.as_chunks::<3>().0 {
            let [a, b, c] = [
                vertices[triangle[0] as usize],
                vertices[triangle[1] as usize],
                vertices[triangle[2] as usize],
            ];
            let ax = b.position[0] - a.position[0];
            let ay = b.position[1] - a.position[1];
            let az = b.position[2] - a.position[2];
            let bx = c.position[0] - a.position[0];
            let by = c.position[1] - a.position[1];
            let bz = c.position[2] - a.position[2];
            let cross = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];
            let cross_length =
                (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            assert!(cross_length > 0.0, "degenerate triangle {triangle:?}");

            let shaded = [
                a.normal[0] + b.normal[0] + c.normal[0],
                a.normal[1] + b.normal[1] + c.normal[1],
                a.normal[2] + b.normal[2] + c.normal[2],
            ];
            let shaded_length =
                (shaded[0] * shaded[0] + shaded[1] * shaded[1] + shaded[2] * shaded[2]).sqrt();
            let dot = (cross[0] * shaded[0] + cross[1] * shaded[1] + cross[2] * shaded[2])
                / (cross_length * shaded_length);
            assert!(dot > 0.9, "triangle {triangle:?} winds inward (dot {dot})");
        }
    }

    #[test]
    fn ring_is_planar_with_the_outer_radius_uvs() {
        let (vertices, indices) = build_ring(SUN.ring_inner, SUN.ring_outer, SUN.ring_segments, 1);
        assert_eq!(vertices.len(), 2 * (SUN.ring_segments as usize + 1));
        assert_eq!(indices.len(), SUN.ring_segments as usize * 6);
        assert!(
            indices
                .iter()
                .all(|index| (*index as usize) < vertices.len())
        );

        for vertex in &vertices {
            assert_eq!(vertex.position[2], 0.0, "ring not planar: {vertex:?}");
            assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);

            let radius = (vertex.position[0] * vertex.position[0]
                + vertex.position[1] * vertex.position[1])
                .sqrt();
            assert!((SUN.ring_inner - 1e-5..=SUN.ring_outer + 1e-5).contains(&radius));

            // three's uv formula maps [-outer, +outer] onto [0, 1].
            assert!((0.0..=1.0).contains(&vertex.uv[0]));
            assert!((0.0..=1.0).contains(&vertex.uv[1]));
        }

        // The inner edge sits at `inner`, the outer at `outer`.
        let inner_radius = vertices[..(SUN.ring_segments as usize + 1)]
            .iter()
            .map(|vertex| {
                (vertex.position[0] * vertex.position[0] + vertex.position[1] * vertex.position[1])
                    .sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        assert!((inner_radius - SUN.ring_inner).abs() < 1e-6);
    }

    #[test]
    fn uniforms_have_the_wgsl_layout_sizes() {
        assert_eq!(std::mem::size_of::<SunMeshUniforms>(), 96);
        assert_eq!(std::mem::size_of::<SunPassUniforms>(), 16);
        assert_eq!(std::mem::size_of::<SunBlurUniforms>(), 32);
        assert_eq!(std::mem::size_of::<SunTintUniforms>(), 32);
        assert_eq!(std::mem::size_of::<SunScreenUniforms>(), 16);
    }

    #[test]
    fn update_caps_the_frame_delta() {
        let mut scene = Sun::new();
        scene.update(4.0, &AudioBridge::new());
        assert!((scene.time() - MAX_FRAME_SECONDS).abs() < 1e-6);
    }

    #[test]
    fn update_scales_time_by_the_surface_speed() {
        let mut scene = Sun::new();
        scene.apply_props(|key| (key == "surfaceSpeed").then_some(2.0));
        scene.update(0.05, &AudioBridge::new());
        // `time` accumulates the raw dt; the shader sees `time * surfaceSpeed`.
        assert!((scene.time() - 0.05).abs() < 1e-6);
    }

    #[test]
    fn update_drives_the_levels_from_the_bands() {
        let mut scene = Sun::new();
        let bridge = AudioBridge::new();
        scene.update(1.0 / 60.0, &bridge);
        // Silence: the `band` nomusic fallbacks dominate — the surface reads
        // `band(-1, 2, 0, 0.1)` → `min(0.12, 0.1 + 0.03) = 0.12`, the corona
        // `band(-1, 2, 0, 0.05)` → `min(0.5, 1.1) = 0.5` (`Sun.js:417-427`).
        assert_eq!(scene.surface_level(), 0.12);
        assert_eq!(scene.corona_level(), 0.5);
        assert_eq!(scene.ray_weight(), SUN.rays.weight);

        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.6; 12];
        payload.beat = 0.8;
        payload.freq_mag = [0.4; AUDIO_SAMPLES];
        payload.byte_frequency_data = vec![200; 1024];
        let mut bridge = AudioBridge::new();
        for _ in 0..10 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
        }

        let mut scene = Sun::new();
        for _ in 0..10 {
            scene.update(1.0 / 60.0, &bridge);
        }

        // Both Expressions sit at their ceilings once the lanes settle.
        assert_eq!(scene.surface_level(), SUN.surface.ceiling);
        assert_eq!(scene.corona_level(), SUN.corona.ceiling);
    }

    #[test]
    fn all_shaders_parse_and_validate() {
        for (name, source) in [
            ("surface", surface_wgsl()),
            ("corona", corona_wgsl()),
            ("copy", copy_wgsl()),
            ("blur", blur_wgsl()),
            ("composite", composite_wgsl()),
            ("tint", tint_wgsl()),
            ("screen", screen_wgsl()),
        ] {
            let module = match naga::front::wgsl::parse_str(&source) {
                Ok(module) => module,
                Err(error) => panic!("{name} shader failed to parse: {error:?}"),
            };
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{name} shader must validate: {error:?}"));
        }
    }

    #[test]
    fn shaders_keep_the_ported_maths() {
        let surface = surface_wgsl();
        for needle in [
            "fn sun_ridgedmffast(p0: vec3<f32>, octaves: u32) -> f32",
            "sun_ridgedmffast(in.world * 15.0 + vec3<f32>(0.0, 0.0, u.g_time * 0.2), 3u)",
            "p9_smoothstep(\n    1.0,\n    0.6,",
            "p9_noise(in.world * 3.0 - vec3<f32>(0.0, 0.0, u.g_time * 0.2)) * 0.5 + 0.5",
            "vec3<f32>(v * u.g_in1.x), 1.0",
        ] {
            assert!(surface.contains(needle), "surface shader lost {needle}");
        }

        let corona = corona_wgsl();
        for needle in [
            "let p = in.uv * 2.0 - vec2<f32>(1.0);",
            "sun_ridgedmffast(vec3<f32>(in.uv * 50.0, u.g_time * 0.5), 5u)",
            "p9_turbulence(vec3<f32>(in.uv * 10.0, u.g_time * 0.2), 3u) + 0.4",
            "p9_smoothstep(1.0, 0.9, length(p))",
            "let v = v1 * v2 * v3 * u.g_in1.x;",
        ] {
            assert!(corona.contains(needle), "corona shader lost {needle}");
        }

        let composite = composite_wgsl();
        for needle in [
            "textureSample(mip0, pass_sampler, uv) * 0.333",
            "textureSample(mip1, pass_sampler, uv) * 0.267",
            "textureSample(mip2, pass_sampler, uv) * 0.2",
            "textureSample(mip3, pass_sampler, uv) * 0.133",
            "textureSample(mip4, pass_sampler, uv) * 0.067",
            "c1.rrr * 3.0 + c2.rgb * 0.9, c2.a",
        ] {
            assert!(composite.contains(needle), "composite shader lost {needle}");
        }

        let tint = tint_wgsl();
        assert!(tint.contains("u.g_color1 * textureSample(corona_texture, corona_sampler, uv).r"));

        let screen = screen_wgsl();
        for needle in [
            "const RAY_NUM_SAMPLES: u32 = 50u;",
            "const RAY_LIGHT_POS: vec2<f32> = vec2<f32>(0.5, 0.5);",
            "const RAY_DECAY: f32 = 0.96;",
            "illumination_decay *= RAY_DECAY;",
            "c += s * illumination_decay * u.g_ray_weight;",
            "c += src.rgb * 0.8;",
            "let out_color = c * c * u.g_brightness;",
        ] {
            assert!(screen.contains(needle), "screen shader lost {needle}");
        }

        let blur = blur_wgsl();
        assert!(blur.contains("array<f32, 21>("));
        assert!(blur.contains("f32(i - 10)"));
    }

    #[test]
    fn tint_colour_is_the_js_orange_bytes() {
        // The JS gColor1 is the raw sRGB triple — the preset writes the
        // bytes verbatim, so no decode happens here.
        assert_eq!(SUN.tint, [0.882, 0.439, 0.024]);
    }

    /// A loud payload for the smoke test (mirrors `scene_tests::loud_payload`).
    fn loud_payload() -> BandPayload {
        let mut payload = BandPayload::silent(44_100.0);
        payload.byte_frequency_data = vec![200; 1024];
        payload.wave = [0.5; AUDIO_SAMPLES];
        payload.beat = 0.8;
        payload.lanes = [0.6; 12];
        payload.cells = [0.6; 16];
        payload.freq_mag = [0.4; AUDIO_SAMPLES];
        payload
    }

    #[test]
    fn renders_two_frames_and_keeps_the_corners_transparent() {
        const SIZE: u32 = 64;
        let Some((device, queue)) = gpu() else {
            eprintln!("skipping sun GPU smoke test: no fallback adapter");
            return;
        };

        // A frame with the god rays off: the corners must present exactly
        // transparent black, which is the compositing contract with the
        // layer below.
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(SIZE, SIZE);
        let mut scene = Sun::new();
        scene.apply_props(|key| (key == "rays").then_some(0.0));
        scene.init(&mut ctx);
        scene.resize(SIZE, SIZE);

        let payload = loud_payload();
        let mut bridge = AudioBridge::new();
        let target = TestTarget::new(&device, SIZE, SIZE);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-sun-smoke"),
        });

        // Two frames: the second proves per-frame state survives the first.
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));

        let pixels = target.read(&device, &queue);
        for (x, y) in [(0, 0), (SIZE - 1, 0), (0, SIZE - 1), (SIZE - 1, SIZE - 1)] {
            let index = ((y * SIZE + x) * 4) as usize;
            assert_eq!(
                pixels[index..index + 4],
                [0, 0, 0, 0],
                "corner ({x}, {y}) must stay transparent with rays off: {:?}",
                &pixels[index..index + 4]
            );
        }

        let centre = (((SIZE / 2) * SIZE + SIZE / 2) * 4) as usize;
        let rgb: u32 =
            pixels[centre] as u32 + pixels[centre + 1] as u32 + pixels[centre + 2] as u32;
        assert!(
            rgb > 0,
            "centre pixel is black: {:?}",
            &pixels[centre..centre + 4]
        );
        assert!(any_lit(&pixels), "expected some lit pixels");

        // A frame with the default god-ray weight: the preset's 50-sample
        // accumulate over the blurred tint never reaches exactly zero in the
        // far corners (measured 2-3/255 in Rgba8, in the JS preset too), so
        // the compositing contract here is a bound, not an equality.
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(SIZE, SIZE);
        let mut scene = Sun::new();
        scene.init(&mut ctx);
        scene.resize(SIZE, SIZE);

        let mut bridge = AudioBridge::new();
        let target = TestTarget::new(&device, SIZE, SIZE);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-sun-smoke-rays"),
        });
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));

        let pixels = target.read(&device, &queue);
        for (x, y) in [(0, 0), (SIZE - 1, 0), (0, SIZE - 1), (SIZE - 1, SIZE - 1)] {
            let index = ((y * SIZE + x) * 4) as usize;
            assert!(
                pixels[index + 3] <= 4,
                "corner ({x}, {y}) halo exceeded the preset floor: {:?}",
                &pixels[index..index + 4]
            );
        }

        let centre = (((SIZE / 2) * SIZE + SIZE / 2) * 4) as usize;
        let rgb: u32 =
            pixels[centre] as u32 + pixels[centre + 1] as u32 + pixels[centre + 2] as u32;
        assert!(
            rgb > 0,
            "centre pixel is black: {:?}",
            &pixels[centre..centre + 4]
        );
    }
}
