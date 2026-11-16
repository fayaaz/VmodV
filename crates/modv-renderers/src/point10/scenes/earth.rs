//! Earth — Rust/wgpu port of
//! `src/renderer/src/application/point10/scenes/Earth.js`.
//!
//! The Plane9 "Earth" preset (author Joakim Dahl, CC BY-NC-SA 4.0) rebuilt as
//! a standalone scene: a planet sphere (day/night/clouds), an atmosphere
//! shell, an inside-out sky sphere with procedural stars + a Milky Way
//! panorama, and a tiny 100x-bright sun, drawn into an HDR target, bloomed,
//! then ACES-tonemapped by the output pass.
//!
//! # Ported exactly
//!
//! * the constant set (`Earth.js:41-56`) as [`EARTH`] (`seed 1`, planet radius
//!   10, atmosphere radius 10.2, sky radius 150, sun distance 100, sun time
//!   scale 0.04, camera distance 34 ± 5.5, fov 60.4, near 0.1, far 200, bloom
//!   `{ strength: 0.2, radius: 0.85, threshold: 0 }`);
//! * the pure driver functions (`:58-93`), function for function:
//!   [`earth_camera`] (`earthCamera`), [`earth_sun_position`]
//!   (`earthSunPosition`), [`earth_texture_phase`] (`earthTexturePhase`) and
//!   [`earth_light_level`] (`earthLightLevel`) — all evaluated in `f64` to
//!   match the JS's doubles everywhere, including the non-finite-input
//!   behaviour (`earthLightLevel` routes non-finite `mid` to 0 and propagates
//!   a NaN `scale`, like `Number.isFinite(mid) ? mid : 0` and
//!   `Math.max(0, scale)` in `Earth.js:89-93`);
//! * `valueNoise(2, …)` / `hash2` from `plane9/P9Expression.js:5-40`, inlined
//!   as [`value_noise2`]/[`hash2`] with the same `(a|0) * 374761393 …`,
//!   `Math.imul(…, 1274126177)`, `>>> 13` / `>>> 16` bit arithmetic (JS
//!   `ToUint32` is Euclidean mod 2^32; negative inputs wrap the same way);
//! * the atmosphere-scattering vertex shaders (`buildEarthSurfaceVertex`
//!   `:99-181`, `buildEarthAtmosphereVertex` `:233-307`) verbatim:
//!   `nSamples = 2`, the `InvWaveLength`/`Scale`/`ScaleDepth` constants, the
//!   `scale()` polynomial, `getNearIntersection`, the `frontColor`/`attenuate`
//!   accumulation loop, `vCol0`/`vCol1`, `vTex = uv + vec2(gIn1.x, 0)`;
//! * the surface fragment (`buildEarthSurfaceFragment` `:186-225`): lambert
//!   NDotL + 0.01 ambient, `albedo = ground + clouds`, `vCol0 + c * vCol1`,
//!   `_brightnessSaturationContrast(c.rgb, 1.2, 2.2, 1.05)` inlined as
//!   `p9_brightness_saturation_contrast` (see Deviations), the orange city
//!   lights term (`vec3(3.0 * gIn3.x)` tint `vec3(1.0, 0.82, 0.2)`), the faint
//!   blue ambient add and the `max(-ndl, 0.0)` night gate;
//! * the atmosphere shell fragment (`buildEarthAtmosphereFragment`
//!   `:312-331`): `G = -0.95`, Rayleigh `0.75 (1 + cos²)`, Mie
//!   `1.5 (1 − g²)/(2 + g²) (1 + cos²) / pow(1 − 2 g cos + g², 1.5)`,
//!   `c = fRayleighPhase * vCol0 + fMiePhase * vCol1; c *= uAtmosphere`,
//!   `alpha = clamp(max(c.rgb))`;
//! * the sky fragment (`buildEarthSkyFragment` `:352-400`): `pp = vWorldpos * 0.01`,
//!   `cloud = smoothstep(0.6, 0.0, fbm(pp * 3.0, 3) * 0.8 + 0.5) * 0.25` (the
//!   reversed-edge `smoothstep` goes through [`p9_smoothstep`]), `stars` via
//!   `sampleTextureWithFilter` (the dFdx/dFdy-weighted 2x2 max-2-sample tap
//!   of `smoothstep(0.7, 1, noise(p * 1.5)) + smoothstep(0.8, 1, noise(p * 0.75)) * 1.5`),
//!   `c = stars * 0.2 + cloud * gColor1.rgb * 0.05 + panorama * 0.05`, alpha
//!   `clamp(max(c.rgb))`;
//! * render order via manual draw order in one pass: planet 0, sun 1, sky 2,
//!   atmosphere 3 (`:502-524`);
//! * the prop binder (`sceneProps.js:289-295`): `spinSpeed` → `spinScale`,
//!   `sunSpeed` → `sunScale`, `nightLights` → `lightsScale`, `atmosphere` →
//!   `atmosphereScale`, `brightness` → `brightnessScale`;
//! * the composer chain (`:543-553`): RenderPass (the scene target) →
//!   UnrealBloomPass (strength 0.2, radius 0.85, threshold 0) → OutputPass with
//!   `ACESFilmic` (`renderer.toneMapping = ACESFilmicToneMapping`,
//!   `:530`) and `brightnessScale` driving `toneMappingExposure` only;
//! * the sun as a `SphereGeometry(1, 32, 32)` drawn with a uniform
//!   `vec4(100, 100, 100, 1)` fragment so the bloom catches it (`:479-496`);
//! * the planet being **opaque** (`gl_FragColor.a = 1.0`, `:221-224`) — the
//!   planet must not punch a hole in the layer below;
//! * the sphere geometry (three's `SphereGeometry` index pattern
//!   `[a, b, d, b, c, d]`, outward normals) with the JS segment counts
//!   (planet/atmosphere 200×200, sky 64×64, sun 32×32).
//!
//! # Replaced three.js machinery
//!
//! | three.js | Rust |
//! |----------|------|
//! | `SphereGeometry` uvs (`uv = (u, 1 - v)`) | stored as `(u, v)` with `v = 0` at the zenith; the Rust asset loader uploads unflipped (row 0 = top = `v 0`), unlike three's `flipY = true` uploads — same equirect sampling, upright image |
//! | `FrustumCamera` / `PerspectiveCamera` | [`cube_mesh::perspective`] (wgpu clip-z) + [`cube_mesh::look_at`] |
//! | `camera.updateMatrixWorld()` + `matrixWorldInverse` | [`cube_mesh::look_at`] (the camera's view matrix) |
//! | `cameraPosition` built-in | a `camera_pos` uniform (the renderer is not ambient in wgpu) |
//! | `RenderPass` + `EffectComposer` | [`Composer`] (`scene_view` → blit → bloom → combine → output) |
//! | `modelViewMatrix` (planet/sky are untransformed) | no model matrix; the planet/sky draw at identity (the JS meshes carry no transform); the sun carries a translation-only `model` |
//! | `texture2D()` | `textureSample()` |
//! | `transparent: true` materials | [`wgpu::BlendState::ALPHA_BLENDING`] on the sky and the atmosphere; `depthWrite: false`, depth test kept |
//! | `FrontSide`/`BackSide` | `cull_mode: Some(Face::Back)` (planet, sun) / `Some(Face::Front)` (sky, atmosphere) |
//!
//! # Deviations
//!
//! * **`_brightnessSaturationContrast`** is not ported in
//!   `shared/p9_shading.rs` (no ported scene calls it), so the Earth surface
//!   fragment carries an inline `p9_brightness_saturation_contrast` with the
//!   identical arithmetic (`c * brightness`, Rec. 709-ish `0.2126/0.7152/0.0722`
//!   luminance, `mix(vec3(luma), c, saturation)`, `mix(vec3(0.5), …, contrast)`).
//! * **Sphere uvs store `v = 0` at the zenith.** three's `SphereGeometry`
//!   stores `uv.v = 1 - v` (with `flipY = true` uploads), which is upright for
//!   three and upside down for the unflipped RGBA8 uploads here; the port
//!   stores `v = iy / heightSegments` (`v 0` at the zenith). The equirect
//!   panorama, day, lights and cloud textures sample identically.
//! * **Anisotropy is not reproduced** (the loader is trilinear, anisotropy 1).
//!   Same documented deviation as AroundPlus.
//! * **`_texturePanoramic`** is not a separate helper in
//!   `shared/p9_shading.rs`; the sky calls `p9_texture_panoramic_lod(…, 0.0)`,
//!   which is the same equirect tap at LOD 0.
//! * **Segment counts are kept** (200×200 planet/atmosphere, 64×64 sky,
//!   32×32 sun) — no reduction, so no deviation to record there.
//! * **`toneMappingExposure`** (`brightnessScale`) is written to the output
//!   pass each time the prop changes; **`outputColorSpace = LinearSRGBColorSpace`
//!   (no sRGB encode)** maps to `OutputPass::set_encode_srgb(false)`: the
//!   composer otherwise enables the shader-side transfer for non-sRGB
//!   destinations (e.g. the `Rgba8Unorm` test target), which the JS does not
//!   want. With an `*Srgb` destination the transfer is hardware-side anyway.
//!   The destination format itself is the engine's choice; with
//!   `Rgba16Float`-style HDR intermediates the ACES curve is applied in
//!   `OutputPass` exactly as three's OutputPass applies it.
//! * **Planet/atmosphere model matrix is identity** (the JS meshes carry no
//!   transform; only the sun moves). The surface/atmosphere vertex shaders
//!   therefore do not multiply by a model matrix; `gl_Position` is
//!   `view_proj * vec4(position, 1)` and world == object space.
//! * **`vWnormal` stays object-space**, as in the JS (`vWnormal =
//!   normalize(normal)`), and is used as world-space because the model is
//!   identity — kept verbatim rather than re-derived.
//! * **`p9audio.band(-1, 1, 1, 1)`** maps onto [`P10Audio::band`]'s
//!   `(channel, damping, bandnr, nomusic)` signature unchanged: damping lane 1
//!   of the mid band, nomusic fallback 1.0 (only reachable without audio).
//! * **`renderOrder`** is applied by draw order in the single render pass
//!   (planet, sun, sky, atmosphere) instead of three's object sort.
//! * **GPU disposal** frees the composer targets, textures, buffers,
//!   pipelines and depth attachment by dropping `EarthGpu`; there is no JS
//!   `material.dispose()`/`geometry.dispose()` equivalent needed beyond that.
//! * **`id`/`role` labels** (`"Earth"`, `"standalone"`) are JS-adapter
//!   bookkeeping and are not part of the [`Point10Scene`] contract; not
//!   reproduced.
//! * The atmosphere shader's `min(clamp(c.rgb, 0, 100), 100)` is carried as a
//!   single two-sided `clamp(c.rgb, 0.0, 100.0)` — the same arithmetic.
//!
//! # `P9Audio`
//!
//! Like every other ported scene the port reads [`P10Audio`] over the shared
//! [`AudioBridge`] instead of `plane9/P9Audio.js`.
//!
//! # Pure-function reference values
//!
//! `tests` below pins [`earth_camera`], [`earth_sun_position`],
//! [`earth_texture_phase`], [`earth_light_level`], [`value_noise2`] and
//! [`hash2`] against constants computed by a Python transcription of the JS
//! (same integer ops in doubles); see `tests::*_match_reference`.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{Composer, ToneMapping};
use crate::point10::scenes::cube_mesh::{
    self, Mat4, Vertex, create_depth, mat4_mul, perspective, look_at,
};
use crate::point10::shared::assets::{ColourSpace, Wrap, load_texture};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::p9_shading::P9_SHADING_WGSL;
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, read_scene_props,
    scene_props_with_levers,
};

// ---------------------------------------------------------------------------
// Constants (`Earth.js:41-56`)
// ---------------------------------------------------------------------------

/// The `EARTH` preset constants (`Earth.js:41-56`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EarthConstants {
    /// Expression `RandomSeed` (`seed: 1`).
    pub seed: f64,
    /// Planet sphere radius (`radius: 10`).
    pub radius: f32,
    /// Atmosphere shell outer radius (`atmosphereRadius: 10.2`); the shell is
    /// offset outward by `outer - inner` in the vertex shader.
    pub atmosphere_radius: f32,
    /// Sky sphere radius (`skyRadius: 150`).
    pub sky_radius: f32,
    /// Sun orbit radius (`sunDistance: 100`).
    pub sun_distance: f64,
    /// Sun orbit time scale (`sunTimeScale: 0.04`).
    pub sun_time_scale: f64,
    /// Camera distance from the origin (`cameraDistance: 34`).
    pub camera_distance: f64,
    /// Camera distance noise amplitude (`cameraDistanceVar: 5.5`).
    pub camera_distance_var: f64,
    /// Camera fov in degrees (`fov: 60.4`).
    pub fov: f32,
    /// Camera near plane (`near: 0.1`).
    pub near: f32,
    /// Camera far plane (`far: 200`).
    pub far: f32,
    /// Bloom pass strength (`bloom: { strength: 0.2, … }`).
    pub bloom_strength: f32,
    /// Bloom pass radius (`bloom: { radius: 0.85, … }`).
    pub bloom_radius: f32,
    /// Bloom pass threshold (`bloom: { threshold: 0 }`).
    pub bloom_threshold: f32,
}

/// `EARTH` (`Earth.js:41-56`).
pub const EARTH: EarthConstants = EarthConstants {
    seed: 1.0,
    radius: 10.0,
    atmosphere_radius: 10.2,
    sky_radius: 150.0,
    sun_distance: 100.0,
    sun_time_scale: 0.04,
    camera_distance: 34.0,
    camera_distance_var: 5.5,
    fov: 60.4,
    near: 0.1,
    far: 200.0,
    bloom_strength: 0.2,
    bloom_radius: 0.85,
    bloom_threshold: 0.0,
};

/// `InnerRadius` in the scattering shaders — the planet's radius in world
/// units (`Earth.js:116`).
pub const INNER_RADIUS: f32 = EARTH.radius;
/// `OuterRadius` in the scattering shaders (`Earth.js:117/250`).
pub const OUTER_RADIUS: f32 = EARTH.atmosphere_radius;

/// Planet sphere segments (`SphereGeometry(radius, 200, 200)`, `:499`/`:520`).
pub const PLANET_SEGMENTS: usize = 200;
/// Sky sphere segments (`SphereGeometry(skyRadius, 64, 64)`, `:513`).
pub const SKY_SEGMENTS: usize = 64;
/// Sun sphere segments (`SphereGeometry(1, 32, 32)`, `:505`).
pub const SUN_SEGMENTS: usize = 32;

/// `gColor1` from the preset (`:469`), as linear-space rgb floats plus the
/// `1.0` alpha the JS carries in the `Vector4`.
pub const G_COLOR1: [f32; 4] = [0.051, 0.133, 0.337, 1.0];

// ---------------------------------------------------------------------------
// Pure driver math (`Earth.js:5-25`, `:58-93`)
// ---------------------------------------------------------------------------

/// One camera pose from [`earth_camera`] (`Earth.js:58-76`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EarthCamera {
    /// Camera position in world units (`{ x, y, z }`).
    pub position: [f64; 3],
    /// Look-at target in world units (`{ x, y, z }`).
    pub look_at: [f64; 3],
}

const JS_UINT32_SIZE: f64 = 4294967296.0;

/// JS `ToUint32` for (already truncation-free) `f64` inputs: Euclidean
/// remainder mod 2^32. `fmod` is exact, and the add-back stays below 2^33,
/// so this is exact for every finite `h`.
fn js_to_uint32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0; // JS: ToUint32(NaN/±∞) === 0
    }
    // `rem_euclid` is Euclidean: `(x % 2^32 + 2^32) % 2^32` with exact ops.
    value.rem_euclid(JS_UINT32_SIZE) as u32
}

/// `hash2(x, y)` — the seeded integer hash behind `valueNoise`
/// (`P9Expression.js:5-12`).
///
/// `(a | 0)` is `ToInt32` (bit-identical to the low 32 bits), the `h`
/// accumulator is JS `number` arithmetic (round-to-nearest f64), and the two
/// `Math.imul`/`>>>` steps are 32-bit wrapping.
pub fn hash2(x: f64, y: f64) -> f64 {
    let a = js_to_uint32(x) as i32;
    let b = js_to_uint32(y) as i32;
    // `let h = (a | 0) * 374761393 + (b | 0) * 668265263;` — JS f64 arithmetic.
    let h = (a as f64) * 374_761_393.0 + (b as f64) * 668_265_263.0;
    let h_bits = js_to_uint32(h);
    // `Math.imul(h ^ (h >>> 13), 1274126177)`
    let mixed = h_bits ^ (h_bits >> 13);
    let mut h = mixed.wrapping_mul(1_274_126_177u32);
    // `h ^= h >> 16;`
    h ^= h >> 16;
    f64::from(h) / JS_UINT32_SIZE
}

fn smoothstep01(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// `valueNoise(2, x, y)` (`P9Expression.js:16-41`), evaluated in `f64`.
///
/// Returns values in `[-1, 1)`.
pub fn value_noise2(x: f64, y: f64) -> f64 {
    let ix = x.floor();
    let iy = y.floor();
    let fx = smoothstep01(x - ix);
    let fy = smoothstep01(y - iy);

    let mut acc = 0.0;
    for dy in 0..2i32 {
        let wy = if dy == 1 { fy } else { 1.0 - fy };
        for dx in 0..2i32 {
            let wx = if dx == 1 { fx } else { 1.0 - fx };
            acc += hash2(ix + f64::from(dx), iy + f64::from(dy)) * wx * wy;
        }
    }
    acc * 2.0 - 1.0
}

/// `earthCamera(time)` (`Earth.js:58-76`).
///
/// `t = seed * 15 + time * 0.005`; `rot = valueNoise(2, t * 0.2, 0)`;
/// `dist = cameraDistance + valueNoise(2, t * 2, 2) * cameraDistanceVar`;
/// `position = (cos(rot * PI * 4) * dist, valueNoise(2, t, 1) * 10, sin(rot * PI * 4) * dist)`;
/// `lookAt = (valueNoise(2, t, 3.42|6.73|8.45) * 5)`.
///
/// Non-finite `time` propagates NaN, exactly like the JS.
pub fn earth_camera(time: f64) -> EarthCamera {
    let t = EARTH.seed * 15.0 + time * 0.005;
    let rot = value_noise2(t * 0.2, 0.0);
    let dist = EARTH.camera_distance + value_noise2(t * 2.0, 2.0) * EARTH.camera_distance_var;
    EarthCamera {
        position: [
            (rot * std::f64::consts::PI * 4.0).cos() * dist,
            value_noise2(t, 1.0) * 10.0,
            (rot * std::f64::consts::PI * 4.0).sin() * dist,
        ],
        look_at: [
            value_noise2(t, 3.42) * 5.0,
            value_noise2(t, 6.73) * 5.0,
            value_noise2(t, 8.45) * 5.0,
        ],
    }
}

/// `earthSunPosition(time, speed = 1)` (`Earth.js:78-83`).
///
/// `t = 1 + 10 * seed + time * sunTimeScale * max(0, speed)`;
/// `(sin(-t) * sunDistance, 0, cos(-t) * sunDistance)`.
pub fn earth_sun_position(time: f64, speed: f64) -> [f64; 3] {
    let t = 1.0 + 10.0 * EARTH.seed + time * EARTH.sun_time_scale * js_max0(speed);
    [
        (-t).sin() * EARTH.sun_distance,
        0.0,
        (-t).cos() * EARTH.sun_distance,
    ]
}

/// `earthTexturePhase(time, speed = 1)` (`Earth.js:85-87`).
///
/// `seed * 10 + time * 0.002 * max(0, speed)`.
pub fn earth_texture_phase(time: f64, speed: f64) -> f64 {
    EARTH.seed * 10.0 + time * 0.002 * js_max0(speed)
}

/// `earthLightLevel(mid, scale = 1)` (`Earth.js:89-93`).
///
/// `(min(max(0, finite(mid)) * 3, 0.8) + 0.2) * max(0, scale)` — non-finite
/// `mid` reads as 0 (`Number.isFinite`), a NaN `scale` stays NaN
/// (`Math.max(0, NaN)`), a negative `scale` clamps to 0.
pub fn earth_light_level(mid: f64, scale: f64) -> f64 {
    let clamped_mid = if mid.is_finite() { mid } else { 0.0 };
    let m = js_max0(clamped_mid);
    (js_min(m * 3.0, 0.8) + 0.2) * js_max0(scale)
}

/// JS `Math.max(0, x)` — NaN propagates (`Math.max(0, NaN) === NaN`).
fn js_max0(x: f64) -> f64 {
    if x.is_nan() {
        f64::NAN
    } else if x > 0.0 {
        x
    } else {
        0.0
    }
}

/// JS `Math.min(a, b)` for finite operands.
fn js_min(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

// ---------------------------------------------------------------------------
// Sphere geometry (three's `SphereGeometry`, `:1533-1610` of the three source)
// ---------------------------------------------------------------------------

/// The sphere index pattern three's `SphereGeometry` uses
/// (`a, b, d` / `b, c, d`), wound counter-clockwise seen from outside.
pub fn build_sphere(
    radius: f32,
    width_segments: usize,
    height_segments: usize,
) -> (Vec<Vertex>, Vec<u32>) {
    let width_segments = width_segments.max(3);
    let height_segments = height_segments.max(2);
    let mut vertices = Vec::with_capacity((width_segments + 1) * (height_segments + 1));
    let mut indices = Vec::with_capacity(width_segments * height_segments * 6);

    let width1 = width_segments + 1;
    for iy in 0..=height_segments {
        let v = iy as f32 / height_segments as f32;
        let phi = v * std::f32::consts::PI;
        let (sin_phi, cos_phi) = phi.sin_cos();
        for ix in 0..=width_segments {
            let u = ix as f32 / width_segments as f32;
            let theta = u * std::f32::consts::TAU;
            let (sin_theta, cos_theta) = theta.sin_cos();

            let x = -radius * cos_theta * sin_phi;
            let y = radius * cos_phi;
            let z = radius * sin_theta * sin_phi;
            let inv = 1.0 / radius;
            vertices.push(Vertex {
                position: [x, y, z],
                normal: [x * inv, y * inv, z * inv],
                // `uv = (u, 1 - v)` in three with `flipY = true` uploads.
                // The Rust loader uploads unflipped, so store `v` directly
                // (v = 0 at the zenith) — see the module docs (Deviations).
                uv: [u, v],
            });
        }
    }

    for iy in 0..height_segments {
        for ix in 0..width_segments {
            let a = (ix + width1 * iy) as u32;
            let b = (ix + width1 * (iy + 1)) as u32;
            let c = (ix + 1 + width1 * (iy + 1)) as u32;
            let d = (ix + 1 + width1 * iy) as u32;
            indices.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }

    (vertices, indices)
}

// ---------------------------------------------------------------------------
// Props (`sceneProps.js:148-154` / `:289-295`)
// ---------------------------------------------------------------------------

/// `EARTH_PROPS` (`sceneProps.js:148-154`).
const EARTH_PROPS: &[SceneProp] = &[
    SceneProp::float("spinSpeed", "Spin Speed", 1.0, 0.0, 4.0, 0.01),
    SceneProp::float("sunSpeed", "Sun Orbit", 1.0, 0.0, 4.0, 0.01),
    SceneProp::float("nightLights", "Night Lights", 1.0, 0.0, 3.0, 0.01),
    SceneProp::float("atmosphere", "Atmosphere", 1.0, 0.0, 3.0, 0.01),
    SceneProp::float("brightness", "Brightness", 1.0, 0.0, 3.0, 0.01),
];

// ---------------------------------------------------------------------------
// Uniforms (one block per material)
// ---------------------------------------------------------------------------

/// Surface material uniforms (128 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SurfaceUniforms {
    view_proj: Mat4,
    /// `xyz` camera world position (`cameraPosition`).
    camera_pos: [f32; 4],
    /// `x` = texture phase (`gIn1.x`).
    g_in1: [f32; 4],
    /// `xyz` = sun position (`gIn2`).
    g_in2: [f32; 4],
    /// `x` = night light level (`gIn3.x`).
    g_in3: [f32; 4],
}

/// Atmosphere material uniforms (112 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct AtmosphereUniforms {
    view_proj: Mat4,
    camera_pos: [f32; 4],
    /// `xyz` = sun position (`gIn2`).
    g_in2: [f32; 4],
    /// `uAtmosphere`, three-style float uniform padded to 16 bytes.
    u_atmosphere: f32,
    _pad: [f32; 3],
}

/// Sky material uniforms (64 bytes; `gColor1` is a shader const).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniforms {
    view_proj: Mat4,
}

/// Sun material uniforms (128 bytes; the model is translation-only).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SunUniforms {
    view_proj: Mat4,
    model: Mat4,
}

/// Depth state with the test kept but writes disabled (sky, atmosphere).
fn depth_test_no_write() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: cube_mesh::DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// GPU resources built in `init`.
struct EarthGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_pipeline: wgpu::RenderPipeline,
    atmosphere_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    sun_pipeline: wgpu::RenderPipeline,
    surface_bind_group: wgpu::BindGroup,
    atmosphere_bind_group: wgpu::BindGroup,
    sky_bind_group: wgpu::BindGroup,
    sun_bind_group: wgpu::BindGroup,
    surface_uniforms: wgpu::Buffer,
    atmosphere_uniforms: wgpu::Buffer,
    sky_uniforms: wgpu::Buffer,
    sun_uniforms: wgpu::Buffer,
    // The planet buffers serve both the surface and the atmosphere pipelines.
    planet_vertices: wgpu::Buffer,
    planet_indices: wgpu::Buffer,
    planet_index_count: u32,
    sky_vertices: wgpu::Buffer,
    sky_indices: wgpu::Buffer,
    sky_index_count: u32,
    sun_vertices: wgpu::Buffer,
    sun_indices: wgpu::Buffer,
    sun_index_count: u32,
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    _earth: wgpu::Texture,
    _lights: wgpu::Texture,
    _clouds: wgpu::Texture,
    _panorama: wgpu::Texture,
    composer: Composer,
}

/// Earth scene.
pub struct Earth {
    audio: P10Audio,
    /// Scene time in seconds (`f64`, like the JS accumulator).
    time: f64,
    spin_scale: f32,
    sun_scale: f32,
    lights_scale: f32,
    atmosphere_scale: f32,
    brightness_scale: f32,
    /// Last exposure written to the output pass (`set_exposure` re-uploads its
    /// uniform, so only write on change).
    exposure: f32,
    size: (u32, u32),
    view_proj: Mat4,
    camera_pos: [f32; 3],
    sun_pos: [f32; 3],
    phase: f32,
    lights: f32,
    gpu: Option<EarthGpu>,
}

impl Default for Earth {
    fn default() -> Self {
        Self::new()
    }
}

impl Earth {
    /// Create the scene (`Earth.js:426-433` defaults).
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            time: 0.0,
            spin_scale: 1.0,
            sun_scale: 1.0,
            lights_scale: 1.0,
            atmosphere_scale: 1.0,
            brightness_scale: 1.0,
            exposure: 1.0,
            size: (1, 1),
            view_proj: cube_mesh::identity(),
            camera_pos: [0.0, 0.0, 0.0],
            sun_pos: [EARTH.sun_distance as f32, 0.0, 0.0],
            phase: EARTH.seed as f32 * 10.0,
            lights: earth_light_level(0.0, 1.0) as f32,
            gpu: None,
        }
    }

    /// Apply the resolved modV props (`sceneProps.js:289-295`).
    pub fn apply_props(&mut self, get: impl FnMut(&str) -> Option<f32>) {
        for (key, value) in read_scene_props(EARTH_PROPS, get) {
            match key {
                "spinSpeed" => self.spin_scale = value,
                "sunSpeed" => self.sun_scale = value,
                "nightLights" => self.lights_scale = value,
                "atmosphere" => self.atmosphere_scale = value,
                "brightness" => self.brightness_scale = value,
                _ => {}
            }
        }
    }

    /// Scene time in seconds.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// `gIn1.x` — the texture scroll phase.
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// `gIn3.x` — the night light level.
    pub fn lights(&self) -> f32 {
        self.lights
    }

    /// Sun world position.
    pub fn sun_position(&self) -> [f32; 3] {
        self.sun_pos
    }

    /// The camera pose for the current scene time.
    pub fn camera_pose(&self) -> EarthCamera {
        earth_camera(self.time)
    }

    /// Camera aspect ratio for the current size.
    fn aspect(&self) -> f32 {
        self.size.0 as f32 / self.size.1.max(1) as f32
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        gpu.queue.write_buffer(
            &gpu.surface_uniforms,
            0,
            bytemuck::bytes_of(&SurfaceUniforms {
                view_proj: self.view_proj,
                camera_pos: [self.camera_pos[0], self.camera_pos[1], self.camera_pos[2], 0.0],
                g_in1: [self.phase, 0.0, 0.0, 0.0],
                g_in2: [self.sun_pos[0], self.sun_pos[1], self.sun_pos[2], 0.0],
                g_in3: [self.lights, 0.0, 0.0, 0.0],
            }),
        );
        gpu.queue.write_buffer(
            &gpu.atmosphere_uniforms,
            0,
            bytemuck::bytes_of(&AtmosphereUniforms {
                view_proj: self.view_proj,
                camera_pos: [self.camera_pos[0], self.camera_pos[1], self.camera_pos[2], 0.0],
                g_in2: [self.sun_pos[0], self.sun_pos[1], self.sun_pos[2], 0.0],
                u_atmosphere: self.atmosphere_scale,
                _pad: [0.0; 3],
            }),
        );
        gpu.queue.write_buffer(
            &gpu.sky_uniforms,
            0,
            bytemuck::bytes_of(&SkyUniforms {
                view_proj: self.view_proj,
            }),
        );
        gpu.queue.write_buffer(
            &gpu.sun_uniforms,
            0,
            bytemuck::bytes_of(&SunUniforms {
                view_proj: self.view_proj,
                model: cube_mesh::translation(self.sun_pos[0], self.sun_pos[1], self.sun_pos[2]),
            }),
        );
    }
}

impl Point10Scene for Earth {
    fn name(&self) -> &'static str {
        "Point10 Earth"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
                let device = ctx.device.clone();
        let queue = ctx.queue.clone();
        self.size = (ctx.width, ctx.height);

        let (planet_mesh, planet_count) = {
            let (vertices, indices) =
                build_sphere(EARTH.radius, PLANET_SEGMENTS, PLANET_SEGMENTS);
            (vertex_and_index_buffers(&device, "point10-earth-planet", &vertices, &indices), indices.len() as u32)
        };
        let (sky_vertices, sky_indices) =
            build_sphere(EARTH.sky_radius, SKY_SEGMENTS, SKY_SEGMENTS);
        let (sky_vertices_buf, sky_indices_buf) =
            vertex_and_index_buffers(&device, "point10-earth-sky", &sky_vertices, &sky_indices);
        let (sun_vertices, sun_indices) = build_sphere(1.0, SUN_SEGMENTS, SUN_SEGMENTS);
        let (sun_vertices_buf, sun_indices_buf) =
            vertex_and_index_buffers(&device, "point10-earth-sun", &sun_vertices, &sun_indices);
        let sky_index_count = sky_indices.len() as u32;
        let sun_index_count = sun_indices.len() as u32;
        let (planet_vertices, planet_indices) = planet_mesh;
        let planet_index_count = planet_count;

        let loader = ctx.asset_loader();
        // `configureTexture` sets RepeatWrapping on all four (`:414-422`);
        // the panorama also wraps in longitude for the equirect lookup.
        let earth = load_texture(&device, &queue, &loader, "earth.jpg", ColourSpace::Srgb, Wrap::Repeat);
        let lights = load_texture(&device, &queue, &loader, "earthlights.jpg", ColourSpace::Srgb, Wrap::Repeat);
        let clouds = load_texture(&device, &queue, &loader, "earthcloud.jpg", ColourSpace::Srgb, Wrap::Repeat);
        let panorama =
            load_texture(&device, &queue, &loader, "wilkyway_panorama.jpg", ColourSpace::Srgb, Wrap::Repeat);

        let surface_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-earth-surface-shader"),
            source: wgpu::ShaderSource::Wgsl(surface_wgsl().into()),
        });
        let atmosphere_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-earth-atmosphere-shader"),
            source: wgpu::ShaderSource::Wgsl(atmosphere_wgsl().into()),
        });
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-earth-sky-shader"),
            source: wgpu::ShaderSource::Wgsl(sky_wgsl().into()),
        });
        let sun_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-earth-sun-shader"),
            source: wgpu::ShaderSource::Wgsl(sun_wgsl().into()),
        });

        let uniform_layout_entry = || wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
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

        let surface_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-earth-surface-layout"),
            entries: &[
                uniform_layout_entry(),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                sampler_entry(4),
            ],
        });
        let atmosphere_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-earth-atmosphere-layout"),
            entries: &[uniform_layout_entry()],
        });
        let sky_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-earth-sky-layout"),
            entries: &[uniform_layout_entry(), texture_entry(1), sampler_entry(2)],
        });
        let sun_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-earth-sun-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                visibility: wgpu::ShaderStages::VERTEX,
                ..uniform_layout_entry()
            }],
        });

        let make_uniforms = |label: &str, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let surface_uniforms = make_uniforms(
            "point10-earth-surface-uniforms",
            std::mem::size_of::<SurfaceUniforms>(),
        );
        let atmosphere_uniforms = make_uniforms(
            "point10-earth-atmosphere-uniforms",
            std::mem::size_of::<AtmosphereUniforms>(),
        );
        let sky_uniforms = make_uniforms("point10-earth-sky-uniforms", std::mem::size_of::<SkyUniforms>());
        let sun_uniforms = make_uniforms("point10-earth-sun-uniforms", std::mem::size_of::<SunUniforms>());

        let surface_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-earth-surface-bind-group"),
            layout: &surface_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: surface_uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&earth.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&lights.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&clouds.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&earth.sampler) },
            ],
        });
        let atmosphere_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-earth-atmosphere-bind-group"),
            layout: &atmosphere_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: atmosphere_uniforms.as_entire_binding() }],
        });
        let sky_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-earth-sky-bind-group"),
            layout: &sky_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sky_uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&panorama.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&panorama.sampler) },
            ],
        });
        let sun_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-earth-sun-bind-group"),
            layout: &sun_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: sun_uniforms.as_entire_binding() }],
        });

        let pipeline_layout = |label: &str, layout: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            })
        };

        let planet_layout = pipeline_layout("point10-earth-planet-pipeline-layout", &surface_layout);
        let atmosphere_pipeline_layout =
            pipeline_layout("point10-earth-atmosphere-pipeline-layout", &atmosphere_layout);
        let sky_pipeline_layout = pipeline_layout("point10-earth-sky-pipeline-layout", &sky_layout);
        let sun_pipeline_layout = pipeline_layout("point10-earth-sun-pipeline-layout", &sun_layout);

        let make_pipeline = |label: &str,
                             shader: &wgpu::ShaderModule,
                             layout: &wgpu::PipelineLayout,
                             vs_entry: &str,
                             fs_entry: &str,
                             cull_mode: Option<wgpu::Face>,
                             depth_stencil: wgpu::DepthStencilState,
                             blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(vs_entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(Vertex::layout())],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(depth_stencil),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(fs_entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: crate::point10::HDR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let opaque_depth = cube_mesh::depth_state();
        let transparent_depth = depth_test_no_write();
        // `FrontSide` (three) → cull the back faces; `BackSide` → cull the front faces.
        let surface_pipeline = make_pipeline(
            "point10-earth-surface-pipeline",
            &surface_shader,
            &planet_layout,
            "vs_main",
            "fs_main",
            Some(wgpu::Face::Back),
            opaque_depth.clone(),
            None,
        );
        let sun_pipeline = make_pipeline(
            "point10-earth-sun-pipeline",
            &sun_shader,
            &sun_pipeline_layout,
            "vs_main",
            "fs_main",
            Some(wgpu::Face::Back),
            opaque_depth,
            None,
        );
        let sky_pipeline = make_pipeline(
            "point10-earth-sky-pipeline",
            &sky_shader,
            &sky_pipeline_layout,
            "vs_main",
            "fs_main",
            Some(wgpu::Face::Front),
            transparent_depth.clone(),
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let atmosphere_pipeline = make_pipeline(
            "point10-earth-atmosphere-pipeline",
            &atmosphere_shader,
            &atmosphere_pipeline_layout,
            "vs_main",
            "fs_main",
            Some(wgpu::Face::Front),
            transparent_depth,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );

        let (depth, depth_view) = create_depth(
            &device,
            "point10-earth-depth",
            self.size.0,
            self.size.1,
        );

        // `RenderPass → UnrealBloomPass(0.2, 0.85, 0) → OutputPass` with ACES
        // (`Earth.js:530/547-553`) and no sRGB transfer
        // (`outputColorSpace = LinearSRGBColorSpace`, `:534`).
        let mut composer =
            Composer::new(&device, &queue, self.size.0, self.size.1, ctx.target_format);
        composer.enable_bloom(EARTH.bloom_strength, EARTH.bloom_radius, EARTH.bloom_threshold);
        composer.set_encode_srgb(false);
        composer
            .output_mut()
            .set_tone_mapping(ToneMapping::AcesFilmic);
        composer.output_mut().set_exposure(self.brightness_scale);

        self.gpu = Some(EarthGpu {
            device,
            queue,
            surface_pipeline,
            atmosphere_pipeline,
            sky_pipeline,
            sun_pipeline,
            surface_bind_group,
            atmosphere_bind_group,
            sky_bind_group,
            sun_bind_group,
            surface_uniforms,
            atmosphere_uniforms,
            sky_uniforms,
            sun_uniforms,
            planet_vertices,
            planet_indices,
            planet_index_count,
            sky_vertices: sky_vertices_buf,
            sky_indices: sky_indices_buf,
            sky_index_count,
            sun_vertices: sun_vertices_buf,
            sun_indices: sun_indices_buf,
            sun_index_count,
            depth,
            depth_view,
            _earth: earth.texture,
            _lights: lights.texture,
            _clouds: clouds.texture,
            _panorama: panorama.texture,
            composer,
        });
        self.exposure = self.brightness_scale;
        self.upload_uniforms();
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width.max(1), height.max(1));
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.composer.resize(self.size.0, self.size.1);
            let (depth, depth_view) = create_depth(
                &gpu.device,
                "point10-earth-depth",
                self.size.0,
                self.size.1,
            );
            gpu.depth = depth;
            gpu.depth_view = depth_view;
        }
        self.upload_uniforms();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.time += f64::from(dt);
        self.audio.update(dt, audio);

        let pose = earth_camera(self.time);
        let sun = earth_sun_position(self.time, f64::from(self.sun_scale));
        self.phase = earth_texture_phase(self.time, f64::from(self.spin_scale)) as f32;
        // `earthLightLevel(p9audio.band(-1, 1, 1, 1), lightsScale)` (`:603-606`).
        let mid = self.audio.band(-1, 1, 1, 1.0);
        self.lights = earth_light_level(f64::from(mid), f64::from(self.lights_scale)) as f32;

        let cam = [pose.position[0] as f32, pose.position[1] as f32, pose.position[2] as f32];
        let look = [pose.look_at[0] as f32, pose.look_at[1] as f32, pose.look_at[2] as f32];
        self.camera_pos = cam;
        self.sun_pos = [sun[0] as f32, sun[1] as f32, sun[2] as f32];

        // `camera.lookAt(...)` + `matrixWorldInverse` (`:609-617`).
        let aspect = self.aspect();
        let projection =
            perspective(EARTH.fov.to_radians(), aspect, EARTH.near, EARTH.far);
        let view = look_at(cam, look, [0.0, 1.0, 0.0]);
        self.view_proj = mat4_mul(&projection, &view);

        // `renderer.toneMappingExposure = brightnessScale` (`:627-628`).
        if let Some(gpu) = self.gpu.as_mut() {
            if self.exposure != self.brightness_scale {
                gpu.composer.output_mut().set_exposure(self.brightness_scale);
                self.exposure = self.brightness_scale;
            }
        }
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };

        {
            // `setClearColor(0x000000, 0)` with `alpha: true` (`:529`):
            // the frame starts transparent.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-earth"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: gpu.composer.scene_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
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

            // Draw order instead of three's `renderOrder` sort:
            // planet (0) → sun (1) → sky (2) → atmosphere (3).
            pass.set_pipeline(&gpu.surface_pipeline);
            pass.set_bind_group(0, &gpu.surface_bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.planet_vertices.slice(..));
            pass.set_index_buffer(gpu.planet_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.planet_index_count, 0, 0..1);

            pass.set_pipeline(&gpu.sun_pipeline);
            pass.set_bind_group(0, &gpu.sun_bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.sun_vertices.slice(..));
            pass.set_index_buffer(gpu.sun_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.sun_index_count, 0, 0..1);

            pass.set_pipeline(&gpu.sky_pipeline);
            pass.set_bind_group(0, &gpu.sky_bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.sky_vertices.slice(..));
            pass.set_index_buffer(gpu.sky_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.sky_index_count, 0, 0..1);

            pass.set_pipeline(&gpu.atmosphere_pipeline);
            pass.set_bind_group(0, &gpu.atmosphere_bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.planet_vertices.slice(..));
            pass.set_index_buffer(gpu.planet_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.planet_index_count, 0, 0..1);
        }

        gpu.composer.render(encoder, target);
    }

    fn dispose(&mut self) {
        self.gpu = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(EARTH_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        // `Point10Earth.js` sets no category; 3D scenes default to `3d-scenes`.
        SceneMeta::for_scene("Point10 Earth", "3d-scenes")
    }
}

/// Upload one sphere buffers as `u32` triangles.
fn vertex_and_index_buffers(
    device: &wgpu::Device,
    label: &str,
    vertices: &[Vertex],
    indices: &[u32],
) -> (wgpu::Buffer, wgpu::Buffer) {
    use wgpu::util::DeviceExt;

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{label}-vertices")),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{label}-indices")),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    (vertex_buffer, index_buffer)
}

// ---------------------------------------------------------------------------
// WGSL
// ---------------------------------------------------------------------------

/// The scattering helpers shared by the surface and atmosphere vertex
/// shaders (`Earth.js:123-133` and `:254-264` — identical in both).
const SCATTERING_WGSL: &str = r#"
const N_SAMPLES: u32 = 2u;
const F_SAMPLES: f32 = 2.0;
const INV_WAVE_LENGTH: vec3<f32> = vec3<f32>(5.602, 9.478, 19.646);
const INNER_RADIUS: f32 = 10.0;   // EARTH.radius (planet, world units)
const OUTER_RADIUS: f32 = 10.2;   // EARTH.atmosphereRadius
const KR_E_SUN: f32 = 0.02;
const KM_E_SUN: f32 = 0.02;
const KR_4_PI: f32 = 0.031416;
const KM_4_PI: f32 = 0.012566;
const ATMO_SCALE: f32 = 1.0 / (10.2 - 10.0);
const SCALE_DEPTH: f32 = 0.25;
const INV_SCALE_DEPTH: f32 = 1.0 / 0.25;
const SCALE_OVER_SCALE_DEPTH: f32 = ATMO_SCALE / SCALE_DEPTH;

fn atmo_scale(fCos: f32) -> f32 {
  let x = 1.0 - fCos;
  return SCALE_DEPTH * exp(-0.00287 + x * (0.459 + x * (3.83 + x * (-6.80 + x * 5.25))));
}

fn get_near_intersection(v3Pos: vec3<f32>, v3Ray: vec3<f32>, fDistance2: f32, fRadius2: f32) -> f32 {
  let B = 2.0 * dot(v3Pos, v3Ray);
  let C = fDistance2 - fRadius2;
  let fDet = max(0.0, B * B - 4.0 * C);
  return 0.5 * (-B - sqrt(fDet));
}
"#;

/// `_brightnessSaturationContrast` from `P9Glsl.js:474-479`, inlined because
/// `shared/p9_shading.rs` does not port it. Identical arithmetic.
const BRIGHTNESS_CONTRAST_WGSL: &str = r#"
fn p9_brightness_saturation_contrast(c: vec3<f32>, brighness: f32, saturation: f32, contrast: f32) -> vec3<f32> {
  let brighnessCol = c * brighness;
  let luminance = dot(brighnessCol, vec3<f32>(0.2126, 0.7152, 0.0722));
  let saturationCol = mix(vec3<f32>(luminance), brighnessCol, vec3<f32>(saturation));
  return mix(vec3<f32>(0.5), saturationCol, vec3<f32>(contrast));
}
"#;

const SURFACE_WGSL: &str = r#"
struct SurfaceUniforms {
  view_proj: mat4x4<f32>,
  camera_pos: vec4<f32>,
  // x = texture phase (gIn1.x)
  g_in1: vec4<f32>,
  // xyz = sun position (gIn2)
  g_in2: vec4<f32>,
  // x = night light level (gIn3.x)
  g_in3: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: SurfaceUniforms;
@group(0) @binding(1) var g_texture1: texture_2d<f32>;
@group(0) @binding(2) var g_texture2: texture_2d<f32>;
@group(0) @binding(3) var g_texture3: texture_2d<f32>;
@group(0) @binding(4) var g_sampler: sampler;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) vWpos: vec3<f32>,
  @location(1) vTex: vec2<f32>,
  @location(2) vWnormal: vec3<f32>,
  @location(3) vCol0: vec4<f32>,
  @location(4) vCol1: vec4<f32>,
}

@vertex
fn vs_main(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
) -> VsOut {
  let CamPos = u.camera_pos.xyz;
  let CamHeight = length(CamPos);
  var posWS = position;
  let ray0 = posWS - CamPos;
  // `posWS = normalize(posWS)` (`Earth.js:140`)
  posWS = normalize(posWS);
  let far0 = length(ray0);
  let ray = ray0 / far0;
  let near = get_near_intersection(CamPos, ray, CamHeight * CamHeight, OUTER_RADIUS * OUTER_RADIUS);
  let rayStart = CamPos + ray * near;
  let far = far0 - near;

  let lightDir = normalize(u.g_in2.xyz - posWS);

  let depth = exp((INNER_RADIUS - OUTER_RADIUS) * INV_SCALE_DEPTH);
  let camAngle = dot(-ray, posWS);
  let camScale = atmo_scale(camAngle);
  let lightAngle = dot(lightDir, posWS);
  let lightScale = atmo_scale(lightAngle);
  let camOffset = depth * camScale;
  let temp = lightScale + camScale;

  let sampleLength = far / F_SAMPLES;
  let scaledLength = sampleLength * ATMO_SCALE;
  let sampleRay = ray * sampleLength;
  var samplePoint = rayStart + sampleRay * 0.5;

  var frontColor = vec3<f32>(0.0);
  var attenuate = vec3<f32>(1.0);

  for (var i = 0u; i < N_SAMPLES; i = i + 1u) {
    let height = length(samplePoint);
    let depth2 = exp(SCALE_OVER_SCALE_DEPTH * (INNER_RADIUS - height));
    let scatter = (depth2 * temp) - camOffset;
    attenuate = exp(-scatter * ((INV_WAVE_LENGTH * KR_4_PI) + vec3<f32>(KM_4_PI)));
    frontColor = frontColor + attenuate * (depth2 * scaledLength);
    samplePoint += sampleRay;
  }

  var out: VsOut;
  out.position = u.view_proj * vec4<f32>(position, 1.0);
  out.vWpos = posWS;
  // `vWnormal = normalize(normal)` — object space, kept verbatim (the model
  // matrix is identity, so object == world).
  out.vWnormal = normalize(normal);
  out.vCol0 = vec4<f32>(
    frontColor * (INV_WAVE_LENGTH * KR_E_SUN + vec3<f32>(KM_E_SUN)) * vec3<f32>(u.g_in3.x),
    1.0,
  );
  out.vCol1 = vec4<f32>(attenuate + vec3<f32>(0.05), 1.0);
  out.vTex = uv + vec2<f32>(u.g_in1.x, 0.0);
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let lightDir = normalize(u.g_in2.xyz - in.vWpos);
  let normalWS = normalize(in.vWnormal);

  let ground = textureSample(g_texture1, g_sampler, in.vTex);
  let clouds = textureSample(g_texture3, g_sampler, in.vTex);
  let groundGrey = dot(vec3<f32>(0.222, 0.707, 0.071), ground.rgb);

  let ambient = vec3<f32>(1.0, 1.0, 1.0) * 0.01;
  let ndl = dot(normalWS, lightDir);
  let ndlL = max(ndl, 0.0);
  let diffuse = vec3<f32>(ndlL);

  // `albedo = ground + clouds` (`Earth.js:211`)
  let albedo = ground + clouds;
  var c = vec4<f32>((diffuse + ambient) * albedo.rgb, 1.0);
  c = vec4<f32>((in.vCol0 + c * in.vCol1).rgb, max(-ndl, 0.1));
  let csc = max(
    vec3<f32>(0.0),
    p9_brightness_saturation_contrast(c.rgb, 1.2, 2.2, 1.05),
  );

  // The dark side: city lights tinted orange, plus a faint blue ambient.
  var lights = textureSample(g_texture2, g_sampler, in.vTex)
    * vec4<f32>(3.0 * u.g_in3.x)
    * vec4<f32>(1.0, 0.82, 0.2, 1.0);
  lights = lights + (vec4<f32>(groundGrey) + clouds * 0.125) * vec4<f32>(0.01, 0.01, 0.2, 1.0);
  lights = lights * max(-ndl, 0.0);

  // Plane9 draws the planet opaque; the adapter composites the scene canvas,
  // so the planet keeps alpha 1.0 (`Earth.js:221-224`).
  return vec4<f32>(csc + lights.rgb, 1.0);
}
"#;

const ATMOSPHERE_WGSL: &str = r#"
struct AtmosphereUniforms {
  view_proj: mat4x4<f32>,
  camera_pos: vec4<f32>,
  // xyz = sun position (gIn2)
  g_in2: vec4<f32>,
  u_atmosphere: f32,
  // Three scalar pads so the layout matches the Rust block byte for byte.
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
}
@group(0) @binding(0) var<uniform> u: AtmosphereUniforms;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) vWpos: vec3<f32>,
  @location(1) vCol0: vec4<f32>,
  @location(2) vCol1: vec4<f32>,
  @location(3) vTex: vec3<f32>,
}

@vertex
fn vs_main(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
) -> VsOut {
  // `pos = position + (normal * (OuterRadius - InnerRadius))` (`Earth.js:267`)
  let pos = position + normal * (OUTER_RADIUS - INNER_RADIUS);
  let CamPos = u.camera_pos.xyz;
  let posWS = pos;
  let CamHeight = length(CamPos);
  let ray0 = posWS - CamPos;
  let far0 = length(ray0);
  let ray = ray0 / far0;

  let lightDir = normalize(u.g_in2.xyz - posWS);
  let near = get_near_intersection(CamPos, ray, CamHeight * CamHeight, OUTER_RADIUS * OUTER_RADIUS);
  let rayStart = CamPos + ray * near;
  let far = far0 - near;

  let startAngle = dot(ray, rayStart) / OUTER_RADIUS;
  let startDepth = exp(-INV_SCALE_DEPTH);
  let startOffset = startDepth * atmo_scale(startAngle);

  let sampleLength = far / F_SAMPLES;
  let scaledLength = sampleLength * ATMO_SCALE;
  let sampleRay = ray * sampleLength;
  var samplePoint = rayStart + sampleRay * 0.5;

  var frontColor = vec3<f32>(0.0);
  for (var i = 0u; i < N_SAMPLES; i = i + 1u) {
    let height = length(samplePoint);
    let depth = exp(SCALE_OVER_SCALE_DEPTH * (INNER_RADIUS - height));
    let lightAngle = dot(lightDir, samplePoint) / height;
    let cameraAngle = dot(ray, samplePoint) / height;
    let scatter = startOffset + depth * (atmo_scale(lightAngle) - atmo_scale(cameraAngle));
    let attenuate = exp(-scatter * ((INV_WAVE_LENGTH * KR_4_PI) + vec3<f32>(KM_4_PI)));
    frontColor = frontColor + attenuate * (depth * scaledLength);
    samplePoint += sampleRay;
  }

  var out: VsOut;
  out.position = u.view_proj * vec4<f32>(pos, 1.0);
  out.vWpos = posWS;
  out.vCol0 = vec4<f32>(frontColor * (INV_WAVE_LENGTH * KR_E_SUN), 1.0);
  out.vCol1 = vec4<f32>(frontColor * vec3<f32>(KM_E_SUN), 1.0);
  out.vTex = CamPos - posWS;
  return out;
}

const G: f32 = -0.95;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let lightDir = normalize(u.g_in2.xyz - in.vWpos);
  let fCos = dot(lightDir, in.vTex) / length(in.vTex);
  let fRayleighPhase = 0.75 * (1.0 + fCos * fCos);
  let fMiePhase = 1.5
    * ((1.0 - (G * G)) / (2.0 + (G * G)))
    * (1.0 + fCos * fCos)
    / pow(1.0 + (G * G) - 2.0 * G * fCos, 1.5);
  var c = fRayleighPhase * in.vCol0 + fMiePhase * in.vCol1;
  c = c * vec4<f32>(u.u_atmosphere);
  // `gl_FragColor = vec4(clamp(c.rgb, 0.0, 100.0), …)` (`Earth.js:329`); the
  // JS's `min(clamp(c.rgb, 0.0, 100.0), 100.0)` collapses to this clamp.
  return vec4<f32>(
    clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(100.0)),
    clamp(max(c.r, max(c.g, c.b)), 0.0, 1.0),
  );
}
"#;

const SKY_WGSL: &str = r#"
struct SkyUniforms {
  view_proj: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> u: SkyUniforms;
@group(0) @binding(1) var g_texture1: texture_2d<f32>;
@group(0) @binding(2) var g_sampler: sampler;

// `gColor1 = (0.051, 0.133, 0.337, 1)` (`Earth.js:469`)
const G_COLOR1: vec4<f32> = vec4<f32>(0.051, 0.133, 0.337, 1.0);
const MAX_SAMPLES: u32 = 2u;

fn sampleTexture(p: vec3<f32>) -> f32 {
  var stars = smoothstep(0.7, 1.0, p9_noise(p * 1.5));
  stars = stars + smoothstep(0.8, 1.0, p9_noise(p * 0.75)) * 1.5;
  return stars;
}

fn sampleTextureWithFilter(uvw: vec3<f32>, detail: f32) -> vec3<f32> {
  let ddx_uvw = uvw + dpdx(uvw);
  let ddy_uvw = uvw + dpdy(uvw);

  let sx = 1u + u32(clamp(detail * length(ddx_uvw - uvw), 0.0, f32(MAX_SAMPLES - 1u)));
  let sy = 1u + u32(clamp(detail * length(ddy_uvw - uvw), 0.0, f32(MAX_SAMPLES - 1u)));
  var no = vec3<f32>(0.0);
  for (var j = 0u; j < MAX_SAMPLES; j = j + 1u) {
    for (var i = 0u; i < MAX_SAMPLES; i = i + 1u) {
      if (j < sy && i < sx) {
        let st = vec2<f32>(f32(i), f32(j)) / vec2<f32>(f32(sx), f32(sy));
        no = no
          + vec3<f32>(sampleTexture(uvw + st.x * (ddx_uvw - uvw) + st.y * (ddy_uvw - uvw)));
      }
    }
  }
  return no / f32(sx * sy);
}

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) vWorldpos: vec3<f32>,
  @location(1) vWnormal: vec3<f32>,
}

@vertex
fn vs_main(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
) -> VsOut {
  var out: VsOut;
  out.position = u.view_proj * vec4<f32>(position, 1.0);
  // `vWorldpos = position` — object space (identity model, so also world).
  out.vWorldpos = position;
  out.vWnormal = normalize(normal);
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let pp = in.vWorldpos * 0.01;
  // Reversed smoothstep edges: `smoothstep(0.6, 0.0, …)` (`Earth.js:388`).
  let cloud = p9_smoothstep(0.6, 0.0, p9_fbm(pp * 3.0, 3u) * 0.8 + 0.5) * 0.25;

  var stars = sampleTextureWithFilter(in.vWorldpos, 8.0).x;
  stars = stars * clamp(1.0 - cloud, 0.0, 1.0);

  var c = vec3<f32>(stars * 0.2) + cloud * G_COLOR1.rgb * 0.05;
  // `_texturePanoramic(…)` — the equirect tap at LOD 0
  // (`p9_texture_panoramic_lod` with lod 0.0; see the module docs).
  c = c + p9_texture_panoramic_lod(g_texture1, g_sampler, normalize(in.vWnormal), 0.0).rgb * 0.05;

  return vec4<f32>(c, clamp(max(c.r, max(c.g, c.b)), 0.0, 1.0));
}
"#;

const SUN_WGSL: &str = r#"
struct SunUniforms {
  view_proj: mat4x4<f32>,
  model: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> u: SunUniforms;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
  return u.view_proj * u.model * vec4<f32>(position, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
  // `vec4(100.0, 100.0, 100.0, 1.0)` so the bloom catches the sun (`Earth.js:489-491`).
  return vec4<f32>(100.0, 100.0, 100.0, 1.0);
}
"#;

fn surface_wgsl() -> String {
    format!("{SCATTERING_WGSL}\n{BRIGHTNESS_CONTRAST_WGSL}\n{SURFACE_WGSL}")
}

fn atmosphere_wgsl() -> String {
    format!("{SCATTERING_WGSL}\n{ATMOSPHERE_WGSL}")
}

fn sky_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{P9_SHADING_WGSL}\n{SKY_WGSL}")
}

fn sun_wgsl() -> String {
    SUN_WGSL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::ScenePropKind;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    // Reference values: transcribed hash2/valueNoise/earthCamera/… into a
    // Python script (same f64 arithmetic and integer ops as the JS) and
    // evaluated there; see the module docs. Every `*_match_reference` test
    // asserts against those numbers.

    const SIZE: u32 = 64;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-6,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_close3(actual: [f64; 3], expected: [f64; 3]) {
        for axis in 0..3 {
            assert_close(actual[axis], expected[axis]);
        }
    }

    // ————— pure driver math —————

    #[test]
    fn hash2_matches_the_python_transcription() {
        assert_close(hash2(0.0, 0.0), 0.0);
        assert_close(hash2(1.0, 1.0), 0.663851173594594);
        assert_close(hash2(-1.0, -1.0), 0.17023825296200812);
        assert_close(hash2(15.0, 1.0), 0.560477223712951);
        assert_close(hash2(-30.0, 2.0), 0.5684565480332822);
        assert_close(hash2(255.0, 255.0), 0.22460817615501583);
    }

    #[test]
    fn value_noise2_matches_the_python_transcription() {
        assert_close(value_noise2(0.0, 0.0), -1.0);
        assert_close(value_noise2(1.5, 2.5), 0.399201721069403);
        assert_close(value_noise2(-3.25, 7.125), -0.830114451250779);
        assert_close(value_noise2(11.75, -0.5), 0.44290043580986094);
        assert_close(value_noise2(100.25, 55.5), 0.22112068567366805);
        assert_close(value_noise2(-1000.125, 333.75), -0.5497661101039739);
        assert_close(value_noise2(0.999, 0.001), 0.01624680889784713);
        assert_close(value_noise2(42.42, 1e-6), 0.6102492528363903);
    }

    #[test]
    fn value_noise2_is_deterministic_and_bounded() {
        for probe in [(3.5, 7.25), (-11.0, 2.0), (0.0, 0.0), (123.4, -56.7)] {
            assert_eq!(value_noise2(probe.0, probe.1), value_noise2(probe.0, probe.1));
        }
        // Value noise maps into [-1, 1): assert with a grid sweep.
        let mut x = -12.0;
        while x < 12.0 {
            let mut y = -8.0;
            while y < 8.0 {
                let value = value_noise2(x, y);
                assert!((-1.0..=1.0).contains(&value), "value_noise2({x}, {y}) = {value}");
                y += 0.37;
            }
            x += 0.29;
        }
    }

    #[test]
    fn earth_camera_matches_the_python_transcription() {
        let pose = earth_camera(0.0);
        assert_close3(
            pose.position,
            [-18.60034364614435, 1.209544474259019, 27.37204525106031],
        );
        assert_close3(
            pose.look_at,
            [3.245493504160568, -1.313484321667599, 3.438593483242439],
        );

        let pose = earth_camera(10.0);
        assert_close3(
            pose.position,
            [-18.582245278678663, 1.1925088708626563, 27.479668860306326],
        );
        assert_close3(
            pose.look_at,
            [3.243055103254627, -1.3226410252119358, 3.415004736897104],
        );

        let pose = earth_camera(61.8);
        assert_close3(
            pose.position,
            [-17.1744261291714, 0.6751299778632647, 30.48847688643713],
        );
        assert_close3(
            pose.look_at,
            [3.1689997773556167, -1.6007342064786434, 2.698603962152667],
        );

        let pose = earth_camera(123.456);
        assert_close3(
            pose.position,
            [-9.90272660075646, -0.3711097940142305, 34.88919408564],
        );
        assert_close3(
            pose.look_at,
            [3.019245644894346, -2.163092181390287, 1.249903612595965],
        );

        let pose = earth_camera(1000.0);
        assert_close3(
            pose.position,
            [19.655748425345603, -9.591975277289748, 20.96116803941726],
        );
        assert_close3(
            pose.look_at,
            [-0.5261626050267365, -0.8679425862630485, 0.2583726483309845],
        );
    }

    #[test]
    fn earth_camera_propagates_non_finite_time() {
        let pose = earth_camera(f64::NAN);
        assert!(pose.position.iter().all(|value| value.is_nan()));
        assert!(pose.look_at.iter().all(|value| value.is_nan()));
    }

    #[test]
    fn earth_sun_position_matches_the_python_transcription() {
        assert_close3(
            earth_sun_position(0.0, 1.0),
            [99.99902065507035, 0.0, 0.4425697988050785],
        );
        assert_close3(
            earth_sun_position(10.0, 1.0),
            [91.93285256646757, 0.0, 39.34908663478909],
        );
        assert_close3(
            earth_sun_position(123.456, 2.0),
            [-89.77648527120984, 0.0, -44.047504950317474],
        );
        // Speed clamps to 0 (`Math.max(0, speed)`), like the JS.
        assert_close3(
            earth_sun_position(50.0, 0.0),
            [99.99902065507035, 0.0, 0.4425697988050785],
        );
        assert_close3(
            earth_sun_position(10.0, -3.0),
            [99.99902065507035, 0.0, 0.4425697988050785],
        );
        // `Math.max(0, NaN) === NaN`.
        assert!(earth_sun_position(10.0, f64::NAN).iter().all(|v| v.is_nan() || *v == 0.0));
        // NaN time propagates.
        assert!(earth_sun_position(f64::NAN, 1.0)
            .iter()
            .all(|v| v.is_nan() || *v == 0.0));
    }

    #[test]
    fn earth_texture_phase_matches_the_python_transcription() {
        assert_close(earth_texture_phase(0.0, 1.0), 10.0);
        assert_close(earth_texture_phase(10.0, 1.0), 10.02);
        assert_close(earth_texture_phase(123.456, 2.5), 10.617280000000001);
        // Negative speed clamps to 0.
        assert_close(earth_texture_phase(10.0, -1.0), 10.0);
    }

    #[test]
    fn earth_light_level_matches_the_python_transcription() {
        assert_close(earth_light_level(0.0, 1.0), 0.2);
        assert_close(earth_light_level(0.1, 1.0), 0.5);
        assert_close(earth_light_level(0.3, 1.0), 1.0);
        assert_close(earth_light_level(1.0, 1.0), 1.0);
        assert_close(earth_light_level(0.5, 2.0), 2.0);
        assert_close(earth_light_level(0.5, 0.0), 0.0);
        // Negative mid clamps to 0.
        assert_close(earth_light_level(-0.5, 1.0), 0.2);
        // Non-finite mid reads as 0 (`Number.isFinite`).
        assert_close(earth_light_level(f64::NAN, 1.0), 0.2);
        assert_close(earth_light_level(f64::INFINITY, 1.0), 0.2);
        assert_close(earth_light_level(f64::NEG_INFINITY, 2.0), 0.4);
        // `Math.max(0, NaN) === NaN`.
        assert!(earth_light_level(0.5, f64::NAN).is_nan());
    }

    #[test]
    fn preset_constants_match_the_js() {
        assert_eq!(EARTH.seed, 1.0);
        assert_eq!(EARTH.radius, 10.0);
        assert_eq!(EARTH.atmosphere_radius, 10.2);
        assert_eq!(EARTH.sky_radius, 150.0);
        assert_eq!(EARTH.sun_distance, 100.0);
        assert_eq!(EARTH.sun_time_scale, 0.04);
        assert_eq!(EARTH.camera_distance, 34.0);
        assert_eq!(EARTH.camera_distance_var, 5.5);
        assert_eq!(EARTH.fov, 60.4);
        assert_eq!(EARTH.near, 0.1);
        assert_eq!(EARTH.far, 200.0);
        assert_eq!(EARTH.bloom_strength, 0.2);
        assert_eq!(EARTH.bloom_radius, 0.85);
        assert_eq!(EARTH.bloom_threshold, 0.0);
        assert_eq!(INNER_RADIUS, 10.0);
        assert_eq!(OUTER_RADIUS, EARTH.atmosphere_radius);
        assert_eq!(G_COLOR1, [0.051, 0.133, 0.337, 1.0]);
        assert_eq!(PLANET_SEGMENTS, 200);
        assert_eq!(SKY_SEGMENTS, 64);
        assert_eq!(SUN_SEGMENTS, 32);
    }

    // ————— UV sphere mesh —————

    #[test]
    fn sphere_normals_point_outward_and_match_the_winding() {
        let (vertices, indices) = build_sphere(10.0, 16, 12);
        for vertex in &vertices {
            let outward = dot3(vertex.normal, vertex.position);
            assert!(outward > 0.0, "inward normal at {vertex:?}");
            assert!((length3(vertex.normal) - 1.0).abs() < 1e-6);
        }
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] = [
                vertices[triangle[0] as usize],
                vertices[triangle[1] as usize],
                vertices[triangle[2] as usize],
            ];
            let raw = cross3(sub3(b.position, a.position), sub3(c.position, a.position));
            // The pole cap triangles are degenerate: `sin(pi)` is f32 dust,
            // not exactly 0, so the two pole vertices differ by ~1e-6 and the
            // cross product is meaningless. three emits them too (they rasterise
            // to zero pixels); skip anything whose area is dust-sized.
            if length3(raw) < 1e-3 {
                continue;
            }
            let geometric = normalise3(raw);
            let shaded = normalise3(add3(add3(a.normal, b.normal), c.normal));
            assert!(
                dot3(geometric, shaded) > 0.9,
                "triangle {triangle:?} winds inward: {geometric:?} vs {shaded:?}"
            );
        }
    }

    #[test]
    fn sphere_has_poles_and_in_range_indices() {
        let (vertices, indices) = build_sphere(10.0, PLANET_SEGMENTS, PLANET_SEGMENTS);
        // The zenith and the nadir are exactly ±radius.
        assert!(vertices.iter().any(|v| (v.position[1] - 10.0).abs() < 1e-6));
        assert!(vertices.iter().any(|v| (v.position[1] + 10.0).abs() < 1e-6));
        let count = PLANET_SEGMENTS + 1;
        assert_eq!(vertices.len(), count * count);
        assert_eq!(indices.len(), PLANET_SEGMENTS * PLANET_SEGMENTS * 6);
        assert!(indices.iter().all(|index| (*index as usize) < vertices.len()));
        // The top row maps v = 0 (the zenith) in this port's upload space.
        assert_eq!(vertices[0].uv, [0.0, 0.0]);
        assert!((vertices[count * count - 1].uv[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sphere_uv_rows_are_wrapped_unflipped() {
        let (vertices, _) = build_sphere(1.0, 4, 4);
        let row = 5;
        assert!((vertices[0].uv[1]).abs() < 1e-6);
        assert!((vertices[row - 1].uv[1]).abs() < 1e-6);
        assert!((vertices[row].uv[1] - 0.25).abs() < 1e-6);
        assert!((vertices[vertices.len() - 1].uv[1] - 1.0).abs() < 1e-6);
    }

    // ————— prop schema and binder —————

    #[test]
    fn prop_schema_order_names_defaults_and_ranges() {
        let scene = Earth::new();
        let props = scene.props();
        let names: Vec<&str> = props.iter().map(|prop| prop.name).collect();
        assert_eq!(
            &names[..6],
            &["gain", "smoothing", "bassGain", "midGain", "trebleGain", "useAgc"]
        );
        assert_eq!(
            &names[6..],
            &["spinSpeed", "sunSpeed", "nightLights", "atmosphere", "brightness"]
        );
        let earth_props = &props[6..];
        assert_eq!(
            earth_props
                .iter()
                .map(|p| (p.default, p.min, p.max, p.step, p.kind))
                .collect::<Vec<_>>(),
            vec![
                (1.0, 0.0, 4.0, 0.01, ScenePropKind::Float),
                (1.0, 0.0, 4.0, 0.01, ScenePropKind::Float),
                (1.0, 0.0, 3.0, 0.01, ScenePropKind::Float),
                (1.0, 0.0, 3.0, 0.01, ScenePropKind::Float),
                (1.0, 0.0, 3.0, 0.01, ScenePropKind::Float),
            ]
        );
        assert_eq!(
            earth_props
                .iter()
                .map(|p| p.label)
                .collect::<Vec<_>>(),
            ["Spin Speed", "Sun Orbit", "Night Lights", "Atmosphere", "Brightness"]
        );
    }

    #[test]
    fn apply_props_maps_the_js_binder() {
        let mut scene = Earth::new();
        scene.apply_props(|key| match key {
            "spinSpeed" => Some(2.0),
            "sunSpeed" => Some(0.5),
            "nightLights" => Some(3.0),
            "atmosphere" => Some(2.5),
            "brightness" => Some(1.5),
            _ => None,
        });
        assert_eq!(scene.spin_scale, 2.0);
        assert_eq!(scene.sun_scale, 0.5);
        assert_eq!(scene.lights_scale, 3.0);
        assert_eq!(scene.atmosphere_scale, 2.5);
        assert_eq!(scene.brightness_scale, 1.5);
    }

    #[test]
    fn apply_props_clamps_and_defaults() {
        let mut scene = Earth::new();
        scene.apply_props(|key| (key == "spinSpeed").then_some(f32::NAN));
        assert_eq!(scene.spin_scale, 1.0);
        scene.apply_props(|key| (key == "spinSpeed").then_some(9.0));
        assert_eq!(scene.spin_scale, 4.0);
        scene.apply_props(|key| (key == "nightLights").then_some(9.0));
        assert_eq!(scene.lights_scale, 3.0);
        scene.apply_props(|key| (key == "nightLights").then_some(-1.0));
        assert_eq!(scene.lights_scale, 0.0);
    }

    #[test]
    fn meta_and_name_match_the_registry() {
        let scene = Earth::new();
        assert_eq!(scene.name(), "Point10 Earth");
        assert_eq!(scene.meta().name, "Point10 Earth");
        assert_eq!(scene.meta().category, "3d-scenes");
        assert_eq!(scene.meta().author, "Point10");
        assert_eq!(scene.meta().version, "1.0.0");
        assert_eq!(scene.meta().module_type, "2d");
        assert_eq!(scene.meta().audio_features, &["bass", "mid", "treble"]);
    }

    // ————— uniforms and lifecycle —————

    #[test]
    fn uniforms_are_tightly_sized() {
        assert_eq!(std::mem::size_of::<SurfaceUniforms>(), 128);
        assert_eq!(std::mem::size_of::<AtmosphereUniforms>(), 112);
        assert_eq!(std::mem::size_of::<SkyUniforms>(), 64);
        assert_eq!(std::mem::size_of::<SunUniforms>(), 128);
    }

    #[test]
    fn time_advances_and_caps_at_a_tenth_of_a_second() {
        let mut scene = Earth::new();
        scene.update(4.0, &AudioBridge::new());
        // frame_dt caps at 0.1 (f32), carried into f64.
        assert!((scene.time() - 0.1).abs() < 1e-6);
        scene.update(0.016, &AudioBridge::new());
        assert!((scene.time() - (0.1f64 + f64::from(0.016f32))).abs() < 1e-6);
    }

    #[test]
    fn update_drives_the_sun_phase_and_lights() {
        let mut scene = Earth::new();
        scene.apply_props(|key| match key {
            "spinSpeed" => Some(2.0),
            "sunSpeed" => Some(2.0),
            "nightLights" => Some(2.0),
            _ => None,
        });
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.6; 12];
        payload.byte_frequency_data = vec![200; 1024];
        payload.freq_mag = [0.4; crate::point10::audio_bridge::AUDIO_SAMPLES];
        payload.wave = [0.5; crate::point10::audio_bridge::AUDIO_SAMPLES];
        payload.beat = 0.8;
        let mut bridge = AudioBridge::new();
        bridge.update(0.1, Some(&payload), &Default::default());

        scene.update(1.0 / 60.0, &bridge);
        assert!(scene.phase() > 10.0);
        assert!(scene.lights() > 0.2, "lights must respond to input {0}", scene.lights());
        let sun = scene.sun_position();
        assert_eq!(sun[1], 0.0);
        assert!((sun[0].hypot(sun[2]) - EARTH.sun_distance as f32).abs() < 1e-3);
    }

    // ————— shaders —————

    fn validate_wgsl(name: &str, program: &str) {
        let module = match naga::front::wgsl::parse_str(program) {
            Ok(module) => module,
            Err(error) => panic!("{name} WGSL failed to parse: {error}"),
        };
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{name} WGSL must validate: {error:?}"));
    }

    #[test]
    fn wgsl_parses_and_validates() {
        validate_wgsl("surface", &surface_wgsl());
        validate_wgsl("atmosphere", &atmosphere_wgsl());
        validate_wgsl("sky", &sky_wgsl());
        validate_wgsl("sun", &sun_wgsl());
    }

    #[test]
    fn shader_keeps_the_ported_maths() {
        let surface = surface_wgsl();
        for needle in [
            "const N_SAMPLES: u32 = 2u;",
            "const INV_WAVE_LENGTH: vec3<f32> = vec3<f32>(5.602, 9.478, 19.646);",
            "fn get_near_intersection(",
            "for (var i = 0u; i < N_SAMPLES; i = i + 1u) {",
            "let camAngle = dot(-ray, posWS);",
            "frontColor = frontColor + attenuate * (depth2 * scaledLength);",
            "out.vCol0 = vec4<f32>(",
            "out.vTex = uv + vec2<f32>(u.g_in1.x, 0.0);",
            "p9_brightness_saturation_contrast(c.rgb, 1.2, 2.2, 1.05)",
            "* vec4<f32>(3.0 * u.g_in3.x)",
            "vec4<f32>(1.0, 0.82, 0.2, 1.0)",
            "return vec4<f32>(csc + lights.rgb, 1.0);",
        ] {
            assert!(surface.contains(needle), "surface shader lost {needle}");
        }

        let atmosphere = atmosphere_wgsl();
        for needle in [
            "let pos = position + normal * (OUTER_RADIUS - INNER_RADIUS);",
            "let startDepth = exp(-INV_SCALE_DEPTH);",
            "let startOffset = startDepth * atmo_scale(startAngle);",
            "let scatter = startOffset + depth * (atmo_scale(lightAngle) - atmo_scale(cameraAngle));",
            "const G: f32 = -0.95;",
            "let fRayleighPhase = 0.75 * (1.0 + fCos * fCos);",
            "pow(1.0 + (G * G) - 2.0 * G * fCos, 1.5)",
            "c = c * vec4<f32>(u.u_atmosphere);",
        ] {
            assert!(atmosphere.contains(needle), "atmosphere shader lost {needle}");
        }

        let sky = sky_wgsl();
        for needle in [
            "fn sampleTextureWithFilter(uvw: vec3<f32>, detail: f32) -> vec3<f32> {",
            "let ddx_uvw = uvw + dpdx(uvw);",
            "let ddy_uvw = uvw + dpdy(uvw);",
            "p9_smoothstep(0.6, 0.0, p9_fbm(pp * 3.0, 3u) * 0.8 + 0.5) * 0.25",
            "p9_texture_panoramic_lod(g_texture1, g_sampler, normalize(in.vWnormal), 0.0)",
            "const G_COLOR1: vec4<f32> = vec4<f32>(0.051, 0.133, 0.337, 1.0);",
        ] {
            assert!(sky.contains(needle), "sky shader lost {needle}");
        }

        let sun = sun_wgsl();
        assert!(sun.contains("vec4<f32>(100.0, 100.0, 100.0, 1.0)"));
    }

    // ————— GPU smoke test (copy of `scene_tests.rs`'s owned-chain helper) —————

    fn loud_payload() -> BandPayload {
        let mut payload = BandPayload::silent(44_100.0);
        payload.byte_frequency_data = vec![200; 1024];
        payload.wave = [0.5; crate::point10::audio_bridge::AUDIO_SAMPLES];
        payload.beat = 0.8;
        payload.lanes = [0.6; 12];
        payload.cells = [0.6; 16];
        payload.freq_mag = [0.4; crate::point10::audio_bridge::AUDIO_SAMPLES];
        payload
    }

    /// [`render_owned_chain_two_frames`] — Earth owns its
    /// `RenderPass → UnrealBloomPass → OutputPass` chain, so the target is the
    /// destination the engine would hand over (`Rgba8Unorm` here).
    fn render_owned_chain_two_frames(scene: &mut Earth, size: u32) -> Option<Vec<u8>> {
        let (device, queue) = gpu()?;

        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(size, size);
        scene.init(&mut ctx);
        scene.resize(size, size);

        let mut bridge = AudioBridge::new();
        let payload = loud_payload();
        let target = TestTarget::new(&device, size, size);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-earth-owned-chain-smoke"),
        });

        // Two frames: the second proves per-frame state (uniforms, uploads)
        // survives the first render.
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));

        Some(target.read(&device, &queue))
    }

    #[test]
    fn earth_renders_two_frames_through_its_composer() {
        let mut scene = Earth::new();
        let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
            return;
        };
        assert!(
            any_lit(&pixels),
            "Earth produced an empty frame at {SIZE}x{SIZE}"
        );
        // The planet is opaque (`gl_FragColor.a = 1.0`): the centre pixel —
        // the planet disc — must be fully opaque and non-black with a loud
        // payload driving the night lights.
        let centre = ((SIZE as usize / 2) * SIZE as usize + SIZE as usize / 2) * 4;
        assert_eq!(pixels[centre + 3], 255, "centre pixel must be opaque");
        assert!(
            pixels[centre..centre + 3].iter().any(|&channel| channel > 0),
            "centre pixel must not be black"
        );
        // Everything outside the planet is the sky — an alpha ≤ 1 blended
        // layer, never fully opaque. Check the corners and the edge
        // midpoints (alpha-sane).
        let size = SIZE as usize;
        for &(x, y) in &[
            (0usize, 0usize),
            (size - 1, 0),
            (0, size - 1),
            (size - 1, size - 1),
            (size / 2, 0),
            (size / 2, size - 1),
            (0, size / 2),
            (size - 1, size / 2),
        ] {
            let index = (y * size + x) * 4;
            assert!(
                pixels[index + 3] < 255,
                "pixel ({x}, {y}) outside the planet must not be fully opaque"
            );
        }
    }

    fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn length3(v: [f32; 3]) -> f32 {
        dot3(v, v).sqrt()
    }

    fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }

    fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn normalise3(v: [f32; 3]) -> [f32; 3] {
        let len = length3(v).max(1e-9);
        [v[0] / len, v[1] / len, v[2] / len]
    }
}
