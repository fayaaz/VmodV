//! MilkDrop runtime: CPU core plus the Layer 5b GPU render passes.
//!
//! This module turns a parsed [`MilkDocument`] into an owned
//! [`MilkCompiledPreset`], drives one or more [`MilkInstance`]s frame by frame,
//! and — when backed by a wgpu device — executes one frame of the MilkDrop
//! render graph. The CPU path (equation compilation/evaluation, globals/audio
//! mapping, band followers, q/reset persistence and the warped mesh UV grid) is
//! pure logic and is unit-tested **without a device**; the GPU path reuses
//! `crate::test_gpu::gpu`, `crate::isf::runtime::read_texture` and
//! `crate::isf::runtime::BoundedCache` instead of duplicating device setup.
//!
//! # Ownership and construction
//!
//! [`MilkRuntime`] is the per-compositor owner. [`MilkRuntime::default`] is
//! device-free and keeps the CPU tests self-contained; a GPU-backed runtime is
//! built with [`MilkRuntime::new`]:
//!
//! ```ignore
//! let runtime = MilkRuntime::new(Arc::new(device), Arc::new(queue), 4);
//! ```
//!
//! `new` keeps the device/queue as `Arc`s and bounds the pipeline cache to
//! `cache_capacity` entries (at least one; four matches the ISF runtime's
//! convention). The runtime also owns a [`MilkFailureCache`] for compilation
//! failures and a lazily populated, `Arc<Mutex<_>>`-wrapped GPU cache of
//! bind-group layouts, samplers, a shared full-screen vertex module and render
//! pipelines. None of this changes the CPU API below.
//!
//! # Render pipeline (Layers 5b + 5c)
//!
//! [`MilkRuntime::render`] encodes one command buffer in MilkDrop/Butterchurn
//! order:
//!
//! 1. **warp** — the translated warp WGSL resamples the previous **pre-post**
//!    composite (`sampler_main`) through the CPU-warped mesh, applying
//!    `fDecay` in the default body. Feedback is the warp+overlay image, never
//!    the filtered display output: feeding gamma/darken/solarize back makes
//!    them compound into noise (the original witchcraft bug).
//! 2. **blur1/2/3** — populate the three `GetBlur{1,2,3}` targets from the
//!    warped image with a normalized 5x5 Gaussian kernel whose radius comes
//!    from the `b1n`/`b2n`/`b3n` base vals.
//! 3. **comp** — the translated comp WGSL samples the warped frame and produces
//!    the composite colour.
//! 4. **overlays (Layer 5c)** — enabled waves/shapes are drawn onto the comp
//!    target (`LineList` or `PointList`) *before* post, exactly like
//!    `DrawCustomWaves()` → `ShowToUser()`.
//! 5. **feedback copy** — the pre-post composite (warp + overlays) is copied to
//!    the internal feedback target for the next frame.
//! 6. **post** — video echo (`echo_zoom`/`echo_alpha`/`echo_orient`), gamma,
//!    the legacy hue shader (`fShader`), then `brighten` (`sqrt(x)`), `darken`
//!    (`x^2`), `solarize` (`4*x*(1-x)`), `invert` and red/blue swap, matching
//!    Butterchurn's composite shader. This output is display-only.
//!
//! [`MilkGpuTargets`] owns the six `Rgba8Unorm` intermediate textures (warp,
//! comp, feedback, three blur levels) and reallocates them only on a size
//! change. The warp mesh's CPU-computed UV grid is uploaded every frame.
//!
//! # Overlay subset and error isolation (Layer 5c)
//!
//! Up to four enabled waves and four shapes are compiled once per preset and
//! evaluated per frame:
//!
//! * A **wave** is a polyline or point cloud over `x/y` points from its
//!   `point_eqs`; `wavecode_<i>_bUseDots` selects `PointList`, and the audio
//!   value/spectrum source follows Butterchurn's `customWaveform.js` scaling,
//!   smoothing, `sep`, `scaling` and per-point colour reset. A **shape** is a
//!   closed regular polygon whose `sides`/`cx`/`cy`/`radius`/`rot` come from
//!   the frame pool. Each vertex carries its own NDC position and clamped
//!   `r/g/b/a` colour.
//! * The built-in waveform is drawn **alongside** custom waves (MilkDrop's
//!   `DrawCustomWaves(); DrawWave(...)`), with modes 0–7 ported from
//!   Butterchurn's `basicWaveform.js`.
//! * Every section is isolated: a parse failure at compile time marks only
//!   that section failed and is logged; a runtime EEL error returns a message
//!   that the caller logs and skips for that section alone. Errors are never
//!   propagated to the preset and never panic.
//!
//! # Per-frame contract (what the tests pin down)
//!
//! 1. [`begin_frame`] runs the preset's init equations exactly once (on the
//!    first frame of an instance), then [`VarPool::snapshot_q`] records the
//!    `q1..q32` baseline. Every subsequent frame restores that baseline via
//!    [`run_frame`] before running the per-frame equations, then sets the
//!    globals (see below) and runs those equations.
//! 2. Globals are built from [`MilkFrameInfo`]:
//!    `time = time_seconds`, `frame = frame_index`, `fps`, `meshx`, `meshy`,
//!    `pixelsx/pixelsy = width/height`. Aspect is chosen so the *smaller*
//!    axis is `1.0` and the larger axis carries the ratio: for `w >= h`,
//!    `aspectx = 1.0` and `aspecty = h / w`; otherwise `aspectx = w / h` and
//!    `aspecty = 1.0`.
//! 3. Audio follows Butterchurn's relative band contract: FFT band sums
//!    (bass 20–320 Hz, mid 320–2800 Hz, treble 2800–11025 Hz) divided by slow
//!    per-instance averages, so a steady passage settles near `1.0` and
//!    transients rise above it. The fast `*_att` followers divide the same
//!    slow average. Values are clamped to `0..=16`.
//! 4. [`evaluate_mesh`] walks a `(meshx + 1) × (meshy + 1)` vertex grid. Each
//!    vertex gets `x`/`y` normalised to `-1.0..=1.0` (center at `(0, 0)`,
//!    `y` increasing upward), `rad = hypot(x, y)` and `ang = atan2(y, x)`, then
//!    runs the per-pixel equations. The returned UV is computed from
//!    `zoom`, `zoomexp`, `cx/cy`, `sx/sy`, `rot`, `dx/dy` and the four warp
//!    sine/cosine terms derived from `warp`/`warpscale`/`warpanimspeed`/`time`.
//!    The exact arithmetic is intentionally left to the implementation; the
//!    tests assert grid size, finiteness and determinism.
//!
//! # Compilation failures
//!
//! [`compile_preset`] records a failing module name in the supplied
//! [`MilkFailureCache`] (shared with the compositor, cleared on registry
//! replacement) and returns [`MilkRuntimeError::Compile`] instead of
//! panicking. A preset with an untranslatable warp or comp shader is therefore
//! skipped safely.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::isf::runtime::BoundedCache;
use crate::milkdrop::eel::{CompiledEquations, Globals, VarPool, compile, run_frame, run_pixel};
use crate::milkdrop::preset::{
    MilkBaseVals, MilkDocument, MilkShader, MilkWaveVals, extract_shader_body,
};
use crate::milkdrop::q_uniforms::{MILK_Q_BINDING, Q_UNIFORM_MEMBERS};
use crate::milkdrop::translate::{CompiledMilkShader, MilkShaderStage, translate_milk_shader};

// Re-exported here so the public path `runtime::MILK_Q_STRIDE` (and the crate
// root re-export) stays unchanged; the definition lives with the shared layout.
pub use crate::milkdrop::q_uniforms::MILK_Q_STRIDE;

/// Denominators smaller than this are treated as degenerate in the warp mesh.
const WARP_EPS: f32 = 1e-6;
/// Sampling UV used when the warp arithmetic would otherwise be non-finite.
const UV_FALLBACK: f32 = 0.5;
/// Upper bound on wave sample points evaluated per section (MilkDrop's
/// Butterchurn limit is 512 samples per custom wave).
const MAX_WAVE_SAMPLES: usize = 512;
/// Draw every Nth evaluated wave point as a line segment (keeps vertex counts small).
const WAVE_VERTEX_STRIDE: usize = 1;
/// Bytes in the single-`vec4` uniform that carries one blur level's texel radius.
const SMALL_UNIFORM_BYTES: u64 = 16;
/// Base blur radius in texels, added to a level's `b1n`/`b2n`/`b3n` strength.
const BLUR_RADIUS_BASE_TEXELS: f64 = 0.5;

/// Turn a `b1n`/`b2n`/`b3n` base-val minimum into the blur kernel's texel radius.
///
/// Strength is clamped to `0..=1`, so the radius spans `0.5..=1.5` texels. The
/// shader clamps the value again defensively before spacing the 5x5 taps.
fn blur_radius_texels(strength: f64) -> f32 {
    (BLUR_RADIUS_BASE_TEXELS + strength.clamp(0.0, 1.0)) as f32
}

// --- Deterministic built-in noise -----------------------------------------
//
// The five built-in noise samplers (`noise_lq/mq/hq` and the two `pw_noise`
// samplers) read four deterministic RGBA8 fields generated on the CPU and
// uploaded once as GPU textures. Both the noise unit tests and the render path
// share the single generator below.

/// Deterministic noise buffers for MilkDrop's built-in noise samplers.
///
/// `lq`, `mq` and `hq` are independent 256×256 RGBA8 white-noise fields at
/// increasing spatial frequency; `pw` is the preset-wave noise field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MilkNoise {
    /// Low-frequency noise (sampler_noise_lq).
    pub lq: Vec<u8>,
    /// Medium-frequency noise (sampler_noise_mq).
    pub mq: Vec<u8>,
    /// High-frequency noise (sampler_noise_hq).
    pub hq: Vec<u8>,
    /// Preset-wave noise (sampler_pw_noise_lq / sampler_pw_noise_hq).
    pub pw: Vec<u8>,
}

impl MilkNoise {
    /// Edge length (in pixels) of each square noise texture.
    pub const SIZE: u32 = 256;

    /// Generate the four deterministic noise fields.
    ///
    /// Each field is filled with bytes drawn from an independent splitmix64
    /// stream seeded with a distinct constant, so every buffer is non-uniform
    /// and the four fields are pairwise distinct. The generator is fixed and
    /// allocation-only, so two calls always produce byte-identical output.
    pub fn new() -> Self {
        Self {
            lq: Self::field(0x9E37_79B9_7F4A_7C15),
            mq: Self::field(0xBF58_476D_1CE4_E5B9),
            hq: Self::field(0x94D0_49BB_1331_11EB),
            pw: Self::field(0xD1B5_4A32_D192_ED03),
        }
    }

    /// Fill one `SIZE × SIZE` RGBA8 field from a seeded splitmix64 stream.
    fn field(seed: u64) -> Vec<u8> {
        let len = (Self::SIZE as usize) * (Self::SIZE as usize) * 4;
        let mut bytes = Vec::with_capacity(len);
        let mut state = seed;
        while bytes.len() < len {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            bytes.extend_from_slice(&z.to_le_bytes());
        }
        bytes.truncate(len);
        bytes
    }
}

impl Default for MilkNoise {
    fn default() -> Self {
        Self::new()
    }
}

/// Number of (texture, sampler) pairs the MilkDrop shader bind group binds:
/// the eight original built-ins plus the five noise samplers. Guarded against
/// `translate::BUILTIN_SAMPLERS` by a unit test.
pub(crate) const MILK_RUNTIME_SAMPLER_PAIRS: usize = 13;

// --- GPU pass shaders (Layer 5b) -------------------------------------------
//
// The warp/comp fragment stages come from `translate::translate_milk_shader`
// and declare `@group(0) @binding(0)` (uniform) plus one pair per
// `translate::BUILTIN_SAMPLERS` entry (13 samplers, bindings 1..=26). A single
// full-screen triangle vertex stage drives every pass and supplies the
// per-pixel `uv_orig` varying at `@location(0)` (waves/shapes overlays remain a
// separate, later cycle).

/// Shared full-screen-triangle vertex stage. `@location(0)` carries an
/// upright (top-left origin) UV to the fragment stages.
const FULLSCREEN_VERTEX_WGSL: &str = r#"
struct MilkVertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> MilkVertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let position = positions[index];
    var out: MilkVertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = vec2<f32>((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5);
    return out;
}
"#;

/// Warp-pass vertex stage driven by the uploaded mesh grid.
///
/// `@location(0)` is the grid's fixed NDC position, `@location(1)` the CPU
/// computed warped sampling UV; these match the two attributes declared by
/// [`mesh_vertex_layout`]. The latter is forwarded as the `@location(0)`
/// varying read by the translated warp fragment shader, so `sampler_main` is
/// sampled at each vertex's warped UV instead of the full-screen `uv_orig`.
/// Grid cells are assembled into two triangles each by
/// [`mesh_triangle_indices`], so this stage runs once per mesh vertex rather
/// than once per output pixel.
const MESH_VERTEX_WGSL: &str = r#"
struct MeshVertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex
fn vs_main(
    @location(0) grid_position: vec2<f32>,
    @location(1) warped_uv: vec2<f32>,
) -> MeshVertexOut {
    var out: MeshVertexOut;
    out.position = vec4<f32>(grid_position, 0.0, 1.0);
    out.uv = warped_uv;
    return out;
}
"#;

/// Normalized 5x5 Gaussian blur, used by the three blur passes.
///
/// The kernel is the separable binomial `[1, 4, 6, 4, 1]` applied in both axes
/// (`tap_weight` below), so it is symmetric and sums to one — a flat image is
/// unchanged. `radius.x` is the tap spacing in texels (derived from the
/// `b1n`/`b2n`/`b3n` base vals), spreading energy into neighbouring texels so
/// `GetBlur{1,2,3}` sample a softened frame rather than a copy.
/// `textureDimensions` supplies `texsize` without another uniform.
const BLUR_FRAGMENT_WGSL: &str = r#"
struct BlurUniforms {
    radius: vec4<f32>,
}
@group(0) @binding(0) var<uniform> blur: BlurUniforms;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;
struct MilkFragIn { @location(0) uv: vec2<f32> }

fn tap_weight(index: i32) -> f32 {
    if (index == -2 || index == 2) { return 1.0; }
    if (index == -1 || index == 1) { return 4.0; }
    return 6.0;
}

@fragment
fn fs_main(input: MilkFragIn) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(src_texture, 0));
    let texel = vec2<f32>(1.0) / dims;
    let radius = clamp(blur.radius.x, 0.0, 4.0);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var j = -2; j <= 2; j = j + 1) {
        for (var i = -2; i <= 2; i = i + 1) {
            let offset = vec2<f32>(f32(i), f32(j)) * radius;
            let w = tap_weight(i) * tap_weight(j);
            sum = sum + textureSample(src_texture, src_sampler, input.uv + offset * texel).rgb * w;
            total = total + w;
        }
    }
    return vec4<f32>(sum / total, 1.0);
}
"#;

/// Straight copy used to seed the internal feedback target from the caller's
/// previous frame on the first render after allocation.
const COPY_FRAGMENT_WGSL: &str = r#"
struct CopyUniforms { _pad: vec4<f32> }
@group(0) @binding(0) var<uniform> copy_params: CopyUniforms;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;
struct MilkFragIn { @location(0) uv: vec2<f32> }
@fragment
fn fs_main(input: MilkFragIn) -> @location(0) vec4<f32> {
    return textureSample(src_texture, src_sampler, input.uv);
}
"#;

/// Final composite/post pass. This is the MilkDrop 1 "show to user" chain:
/// video echo (two zoomed/oriented copies mixed by `echo_alpha`), gamma
/// brightness (the old additive redraws are a multiply), the legacy hue shader
/// (`fShader`), then the fixed `brighten`/`darken`/`solarize`/`invert` filters.
///
/// Overlays are drawn onto the composite target *before* this pass, exactly
/// like MilkDrop's `DrawCustomWaves` → `ShowToUser` order. The warp stage's
/// `decay` remains the only feedback multiplier; this pass never reads the
/// previous frame.
const POST_FRAGMENT_WGSL: &str = r#"
struct MilkPostUniforms {
    time_gamma_fshader_echo_zoom: vec4<f32>,
    echo_alpha_echo_orient_brighten_darken: vec4<f32>,
    solarize_invert_red_blue_legacy: vec4<f32>,
    rand_start: vec4<f32>,
    brightness_decay_pad: vec4<f32>,
}
@group(0) @binding(0) var<uniform> post: MilkPostUniforms;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;
@group(0) @binding(3) var prev_texture: texture_2d<f32>;
@group(0) @binding(4) var prev_sampler: sampler;
struct MilkFragIn { @location(0) uv: vec2<f32> }

fn hue_corner(index: f32) -> vec3<f32> {
    let t = post.time_gamma_fshader_echo_zoom.x;
    var c = vec3<f32>(
        0.6 + 0.3 * sin(t * 30.0 * 0.0143 + 3.0 + index * 21.0 + post.rand_start.w),
        0.6 + 0.3 * sin(t * 30.0 * 0.0107 + 1.0 + index * 13.0 + post.rand_start.y),
        0.6 + 0.3 * sin(t * 30.0 * 0.0129 + 6.0 + index * 9.0 + post.rand_start.z),
    );
    let max_shade = max(c.x, max(c.y, c.z));
    return vec3<f32>(0.5) + 0.5 * (c / max(max_shade, 1e-6));
}

fn hue_at(uv: vec2<f32>) -> vec3<f32> {
    let x = uv.x;
    let y = uv.y;
    let c0 = hue_corner(0.0);
    let c1 = hue_corner(1.0);
    let c2 = hue_corner(2.0);
    let c3 = hue_corner(3.0);
    return c0 * x * y + c1 * (1.0 - x) * y + c2 * x * (1.0 - y)
        + c3 * (1.0 - x) * (1.0 - y);
}

@fragment
fn fs_main(input: MilkFragIn) -> @location(0) vec4<f32> {
    let gamma = post.time_gamma_fshader_echo_zoom.y;
    let fshader = post.time_gamma_fshader_echo_zoom.z;
    let echo_zoom = post.time_gamma_fshader_echo_zoom.w;
    let echo_alpha = post.echo_alpha_echo_orient_brighten_darken.x;
    let echo_orient = post.echo_alpha_echo_orient_brighten_darken.y;
    let brighten = post.echo_alpha_echo_orient_brighten_darken.z;
    let darken = post.echo_alpha_echo_orient_brighten_darken.w;
    let solarize = post.solarize_invert_red_blue_legacy.x;
    let invert = post.solarize_invert_red_blue_legacy.y;
    let red_blue = post.solarize_invert_red_blue_legacy.z;
    let legacy = post.solarize_invert_red_blue_legacy.w;

    var c = textureSample(src_texture, src_sampler, input.uv).rgb;

    if (legacy > 0.5) {
        // Video echo: orientation flips bit 0 horizontally and bit 1
        // vertically; `echo_zoom` scales around the centre.
        let orient_h = fract(echo_orient * 0.5) * 2.0;
        let orient_x = select(1.0, -1.0, orient_h >= 1.0);
        let orient_y = select(1.0, -1.0, echo_orient >= 2.0);
        let zoom = max(abs(echo_zoom), 0.001);
        let uv_echo = (input.uv - vec2<f32>(0.5)) * (1.0 / zoom)
            * vec2<f32>(orient_x, orient_y) + vec2<f32>(0.5);
        let echoed = textureSample(src_texture, src_sampler, uv_echo).rgb;
        c = mix(c, echoed, clamp(echo_alpha, 0.0, 1.0));
    }

    // MilkDrop's gamma adjustment redraws the image additively `gamma` times,
    // which is a multiply in the continuous domain. The old test fixtures use
    // gammaadj == 1 for a neutral pass.
    c = c * max(gamma, 0.0);

    if (legacy > 0.5 && fshader > 0.001) {
        let hue = hue_at(input.uv);
        c = c * mix(vec3<f32>(1.0), hue, clamp(fshader, 0.0, 1.0));
    }

    if (brighten != 0.0) {
        c = sqrt(max(c, vec3<f32>(0.0)));
    }
    if (darken != 0.0) { c = c * c; }
    if (solarize != 0.0) { c = 4.0 * c * (vec3<f32>(1.0) - c); }
    if (invert != 0.0) { c = vec3<f32>(1.0) - c; }
    if (red_blue != 0.0) { c = vec3<f32>(c.b, c.g, c.r); }
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

// --- Layer 5c overlay shaders ----------------------------------------------
//
// Overlays are drawn as `LineList` primitives directly onto the composite
// target (between the comp and output passes). Each vertex carries its own
// NDC position and RGBA colour, taken from the evaluated wave/shape pools, so
// no bind groups are needed: the pipeline uses an auto layout derived from
// these two entry points. A single 1-pixel line per segment is the faithful
// subset chosen here — it keeps the pass allocation-free of textures and still
// produces visible, deterministic, equation-driven pixels.

const OVERLAY_VERTEX_WGSL: &str = r#"
struct OverlayOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}
@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> OverlayOut {
    var out: OverlayOut;
    out.position = vec4<f32>(pos, 0.0, 1.0);
    out.color = color;
    return out;
}
"#;

const OVERLAY_FRAGMENT_WGSL: &str = r#"
struct OverlayIn { @location(0) color: vec4<f32> }
@fragment
fn fs_main(input: OverlayIn) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

/// Which bind-group layout a cached pipeline was built against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MilkLayout {
    /// The translated warp/comp shaders: uniform + 13 sampler pairs.
    WarpComp,
    /// The blur passes: one texture + one sampler.
    Pass,
    /// The output pass: uniform + one texture + one sampler.
    Output,
}

/// Vertex-stage configuration shared by the cached render pipelines.
///
/// Both variants use the same bind-group layout and `Rgba8Unorm` fragment
/// target; they differ only in vertex module, primitive topology and whether a
/// vertex buffer is bound. Keeping the choice in one place lets [`MilkGpuCache`]
/// share the cache/failure bookkeeping and pipeline construction between the
/// full-screen and mesh warp passes.
#[derive(Clone, Copy)]
enum MilkVertexMode {
    /// Full-screen triangle generated from `vertex_index`; no vertex buffers.
    Fullscreen,
    /// Indexed mesh whose vertices carry a grid position plus a warped UV.
    Mesh,
}

/// The four deterministic noise fields, uploaded once and kept alive by their
/// views. The views are bound positionally at the five built-in noise sampler
/// slots in `milk_bind_group` (both preset-wave noise samplers share the `pw`
/// field). wgpu ref-counts GPU resources, so a `TextureView` keeps its parent
/// texture alive — no separate `Texture` handles need to be stored.
#[derive(Clone)]
struct MilkNoiseGpu {
    views: [wgpu::TextureView; 4],
}

/// Reusable GPU objects shared by every render of one runtime. Kept behind an
/// `Arc<Mutex<_>>` so `render(&self, ..)` can populate caches while the runtime
/// stays `Clone` + `Debug` and `Default` remains device-free.
#[derive(Clone)]
struct MilkGpuCache {
    pipelines: BoundedCache<String, wgpu::RenderPipeline>,
    milk_layout: Option<wgpu::BindGroupLayout>,
    pass_layout: Option<wgpu::BindGroupLayout>,
    output_layout: Option<wgpu::BindGroupLayout>,
    vertex: Option<wgpu::ShaderModule>,
    /// Vertex module for the mesh-driven warp pass (grid position + warped UV).
    mesh_vertex: Option<wgpu::ShaderModule>,
    sampler: Option<wgpu::Sampler>,
    dummy: Option<(wgpu::Texture, wgpu::TextureView)>,
    noise: Option<MilkNoiseGpu>,
    /// WGSL sources already known to fail validation, so a broken preset is
    /// reported as a typed [`MilkRuntimeError::Draw`] without recompiling.
    failed: HashSet<String>,
}

impl MilkGpuCache {
    fn new(capacity: usize) -> Self {
        Self {
            pipelines: BoundedCache::new(capacity),
            milk_layout: None,
            pass_layout: None,
            output_layout: None,
            vertex: None,
            mesh_vertex: None,
            sampler: None,
            dummy: None,
            noise: None,
            failed: HashSet::new(),
        }
    }

    /// Filterable 2D float texture binding visible to the fragment stage.
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

    /// Filtering sampler binding visible to the fragment stage.
    fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        }
    }

    /// The single 256-byte uniform binding visible to the fragment stage.
    fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(MILK_UNIFORM_STRIDE as u64),
            },
            count: None,
        }
    }

    /// The post/legacy-composite uniform binding (five `vec4`s).
    fn post_uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(MILK_POST_UNIFORM_BYTES as u64),
            },
            count: None,
        }
    }

    /// The `MilkQUniforms` binding visible to the fragment stage.
    fn q_uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(MILK_Q_STRIDE as u64),
            },
            count: None,
        }
    }

    /// A small (one `vec4`) uniform binding, used for the blur radius.
    fn small_uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(SMALL_UNIFORM_BYTES),
            },
            count: None,
        }
    }

    /// Lazily create the device-owned objects common to every pass.
    fn ensure_common(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.sampler.is_none() {
            self.sampler = Some(device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("modV MilkDrop sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                lod_min_clamp: 0.0,
                lod_max_clamp: 0.0,
                compare: None,
                anisotropy_clamp: 1,
                border_color: None,
            }));
        }
        if self.dummy.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("modV MilkDrop dummy sampler"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &[0u8; 4],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.dummy = Some((texture, view));
        }
        if self.noise.is_none() {
            let noise = MilkNoise::new();
            let fields = [&noise.lq, &noise.mq, &noise.hq, &noise.pw];
            let labels = [
                "modV MilkDrop noise lq",
                "modV MilkDrop noise mq",
                "modV MilkDrop noise hq",
                "modV MilkDrop noise pw",
            ];
            let views = std::array::from_fn(|index| {
                create_noise_view(device, queue, fields[index], labels[index])
            });
            self.noise = Some(MilkNoiseGpu { views });
        }
        if self.vertex.is_none() {
            self.vertex = Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("modV MilkDrop vertex"),
                source: wgpu::ShaderSource::Wgsl(FULLSCREEN_VERTEX_WGSL.into()),
            }));
        }
        if self.mesh_vertex.is_none() {
            self.mesh_vertex = Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("modV MilkDrop mesh vertex"),
                source: wgpu::ShaderSource::Wgsl(MESH_VERTEX_WGSL.into()),
            }));
        }
        if self.milk_layout.is_none() {
            // Binding 0 is the uniform; the rest are `MILK_RUNTIME_SAMPLER_PAIRS`
            // (texture, sampler) pairs in `BUILTIN_SAMPLERS` order (1..=26).
            let mut entries = Vec::with_capacity(2 + MILK_RUNTIME_SAMPLER_PAIRS * 2);
            entries.push(Self::uniform_entry(0));
            for binding in 1..=(MILK_RUNTIME_SAMPLER_PAIRS as u32) * 2 {
                entries.push(if binding % 2 == 1 {
                    Self::texture_entry(binding)
                } else {
                    Self::sampler_entry(binding)
                });
            }
            // The additive q block takes the first binding after the samplers.
            entries.push(Self::q_uniform_entry(MILK_Q_BINDING));
            self.milk_layout = Some(device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("modV MilkDrop shader layout"),
                    entries: &entries,
                },
            ));
        }
        if self.pass_layout.is_none() {
            self.pass_layout = Some(device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("modV MilkDrop blur layout"),
                    entries: &[
                        Self::small_uniform_entry(0),
                        Self::texture_entry(1),
                        Self::sampler_entry(2),
                    ],
                },
            ));
        }
        if self.output_layout.is_none() {
            self.output_layout = Some(device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label: Some("modV MilkDrop output layout"),
                    entries: &[
                        Self::post_uniform_entry(0),
                        Self::texture_entry(1),
                        Self::sampler_entry(2),
                        Self::texture_entry(3),
                        Self::sampler_entry(4),
                    ],
                },
            ));
        }
    }

    /// Build (and cache) a render pipeline for one translated WGSL pass.
    ///
    /// [`Self::pipeline`] and [`Self::mesh_pipeline`] both funnel through here:
    /// cache/failure bookkeeping, bind-group layout, fragment module and target
    /// format are shared, while `vertex_mode` selects the vertex stage,
    /// primitive topology and vertex-buffer binding.
    ///
    /// Invalid WGSL is reported as a typed `Draw` error and remembered so it is
    /// not compiled again. Validation uses naga's `parse_str` rather than a wgpu
    /// error scope. The shader source is either produced by our own translator
    /// (syntactically valid by construction) or supplied at runtime by a caller;
    /// a parse failure catches the latter (e.g. an empty or truncated `main`)
    /// and keeps the failure path synchronous. Semantic faults that only wgpu
    /// can see are reported through the device's normal validation-error
    /// handling; adding an error scope here would require blocking on
    /// `pop_error_scope()` inside the cache mutex for no test-visible benefit.
    fn pipeline_impl(
        &mut self,
        device: &wgpu::Device,
        key: String,
        source: &str,
        entry: &str,
        kind: MilkLayout,
        vertex_mode: MilkVertexMode,
    ) -> Result<wgpu::RenderPipeline, MilkRuntimeError> {
        if let Some(pipeline) = self.pipelines.get(&key) {
            return Ok(pipeline.clone());
        }
        if self.failed.contains(&key) {
            return Err(MilkRuntimeError::Draw(format!(
                "cached invalid MilkDrop WGSL for {entry}"
            )));
        }
        // Cheap syntactic guard: translated WGSL is valid by construction, so
        // this only trips on caller-supplied garbage (see the method docs).
        if naga::front::wgsl::parse_str(source).is_err() {
            self.failed.insert(key);
            return Err(MilkRuntimeError::Draw(format!(
                "invalid WGSL for MilkDrop {entry} pass"
            )));
        }
        let layout = match kind {
            MilkLayout::WarpComp => self.milk_layout.clone(),
            MilkLayout::Pass => self.pass_layout.clone(),
            MilkLayout::Output => self.output_layout.clone(),
        }
        .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop bind group layout missing".into()))?;
        let (vertex, vertex_error) = match vertex_mode {
            MilkVertexMode::Fullscreen => (self.vertex.clone(), "MilkDrop vertex module missing"),
            MilkVertexMode::Mesh => (
                self.mesh_vertex.clone(),
                "MilkDrop mesh vertex module missing",
            ),
        };
        let vertex = vertex.ok_or_else(|| MilkRuntimeError::Draw(vertex_error.into()))?;
        let fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("modV MilkDrop fragment"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("modV MilkDrop pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let mesh_buffers = [Some(mesh_vertex_layout())];
        let empty_buffers: &[Option<wgpu::VertexBufferLayout<'static>>] = &[];
        let (buffers, primitive) = match vertex_mode {
            MilkVertexMode::Fullscreen => (empty_buffers, wgpu::PrimitiveState::default()),
            MilkVertexMode::Mesh => (
                &mesh_buffers[..],
                wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
            ),
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("modV MilkDrop pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers,
            },
            primitive,
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pipeline.clone());
        Ok(pipeline)
    }

    /// Fetch (or build once) a full-screen-triangle pipeline.
    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        key: String,
        source: &str,
        entry: &str,
        kind: MilkLayout,
    ) -> Result<wgpu::RenderPipeline, MilkRuntimeError> {
        self.pipeline_impl(device, key, source, entry, kind, MilkVertexMode::Fullscreen)
    }

    /// Fetch (or build once) the warp pipeline driven by the uploaded mesh.
    ///
    /// Same bind layout and translated warp fragment as [`Self::pipeline`], but
    /// the mesh vertex module supplies grid position + warped UV through an
    /// indexed triangle list, so `sampler_main` is sampled at each vertex's
    /// warped UV (see [`MESH_VERTEX_WGSL`] and [`mesh_vertex_layout`]).
    fn mesh_pipeline(
        &mut self,
        device: &wgpu::Device,
        key: String,
        source: &str,
        entry: &str,
    ) -> Result<wgpu::RenderPipeline, MilkRuntimeError> {
        self.pipeline_impl(
            device,
            key,
            source,
            entry,
            MilkLayout::WarpComp,
            MilkVertexMode::Mesh,
        )
    }

    /// Fetch (or build once) the overlay `LineList` pipeline. The only
    /// difference between variants is the blend state: `additive` reads the
    /// section's `additive` variable and uses `(src_alpha, one)`, otherwise a
    /// straight-alpha blend is used. Both share the same vertex layout and an
    /// auto-derived (empty) bind-group layout because the shaders declare no
    /// bindings.
    fn overlay_pipeline(
        &mut self,
        device: &wgpu::Device,
        additive: bool,
        dots: bool,
    ) -> Result<wgpu::RenderPipeline, MilkRuntimeError> {
        let key = format!(
            "overlay|{}|{}",
            if additive { "additive" } else { "alpha" },
            if dots { "points" } else { "lines" }
        );
        if let Some(pipeline) = self.pipelines.get(&key) {
            return Ok(pipeline.clone());
        }
        let color_blend = if additive {
            wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            }
        } else {
            wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            }
        };
        let blend = wgpu::BlendState {
            color: color_blend,
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("modV MilkDrop overlay vertex"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_VERTEX_WGSL.into()),
        });
        let fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("modV MilkDrop overlay fragment"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_FRAGMENT_WGSL.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("modV MilkDrop overlay"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(overlay_vertex_layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: if dots {
                    wgpu::PrimitiveTopology::PointList
                } else {
                    wgpu::PrimitiveTopology::LineList
                },
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pipeline.clone());
        Ok(pipeline)
    }
}

/// Audio snapshot consumed for one frame.
///
/// `wave` and `byte_frequency_data` are kept as `Vec`s (rather than fixed
/// arrays) so callers can pass whatever their capture backend produced;
/// [`begin_frame`] copies them onto the instance each frame. The canonical
/// lengths are 1024 samples each.
#[derive(Clone, Debug, Default)]
pub struct MilkAudio {
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    /// Capture sample rate, used to map FFT bins to MilkDrop's band ranges.
    pub sample_rate: f32,
    pub wave: Vec<f32>,
    pub byte_frequency_data: Vec<u8>,
}

/// All per-frame inputs to the CPU runtime.
#[derive(Clone, Debug)]
pub struct MilkFrameInfo {
    pub time_seconds: f64,
    pub frame_index: u32,
    pub fps: f32,
    /// Texture/render target width (feeds `pixelsx`).
    pub width: u32,
    /// Texture/render target height (feeds `pixelsy`).
    pub height: u32,
    pub meshx: u32,
    pub meshy: u32,
    pub audio: MilkAudio,
}

/// An owned, compiled preset, ready to be instantiated.
#[derive(Clone, Debug)]
pub struct MilkCompiledPreset {
    pub doc: MilkDocument,
    pub init: CompiledEquations,
    pub frame: CompiledEquations,
    pub pixel: CompiledEquations,
    pub warp: CompiledMilkShader,
    pub comp: CompiledMilkShader,
    /// True when the preset has no custom composite shader body, so the
    /// legacy MilkDrop 1 video-echo/hue/post chain applies.
    pub legacy_composite: bool,
    /// One compiled program per *enabled* wave section (Layer 5c). Added
    /// additively; existing fields and behaviour are unchanged.
    pub waves: Vec<WaveProgram>,
    /// One compiled program per *enabled* shape section (Layer 5c).
    pub shapes: Vec<ShapeProgram>,
}

// --- Layer 5c wave/shape overlays (RED stubs) ------------------------------
//
// These public types/functions are the surface the GREEN phase implements.
// The stubs make the overlay tests compile while leaving them failing on their
// assertions; no existing render pass is modified.

/// Persistent per-section state for one wave or shape overlay.
///
/// `pool` carries the section's user variables and the `t1..t8` values captured
/// from its init equations; `init_done` records whether init has run for the
/// current preset instance.
#[derive(Clone, Debug, Default)]
pub struct OverlayState {
    pub pool: VarPool,
    pub init_done: bool,
}

/// One compiled wave overlay: its init/frame/point EEL equation sets.
#[derive(Clone, Debug, Default)]
pub struct WaveProgram {
    pub init: CompiledEquations,
    pub frame: CompiledEquations,
    pub point: CompiledEquations,
    /// Authored per-section scalars (samples/sep/scaling/smoothing/colour).
    pub values: MilkWaveVals,
    /// From `wavecode_<i>_bUseDots`: draw points instead of a polyline.
    pub dots: bool,
}

/// One compiled shape overlay: its init/frame EEL equation sets.
#[derive(Clone, Debug, Default)]
pub struct ShapeProgram {
    pub init: CompiledEquations,
    pub frame: CompiledEquations,
}

/// One evaluated sample point of a wave overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WavePoint {
    pub x: f32,
    pub y: f32,
    pub sample: f32,
    pub value1: f32,
    pub value2: f32,
    /// RGBA written by the point equations for this sample, clamped to [0, 1].
    pub color: [f32; 4],
}

/// Per-frame evaluated state of one wave overlay.
#[derive(Clone, Debug, Default)]
pub struct WaveOverlay {
    pub pool: VarPool,
    pub points: Vec<WavePoint>,
    pub enabled: bool,
    /// Draw one point per sample instead of connecting them with lines.
    pub dots: bool,
}

/// Per-frame evaluated state of one shape overlay.
#[derive(Clone, Debug, Default)]
pub struct ShapeOverlay {
    pub pool: VarPool,
    pub enabled: bool,
}

/// Every overlay evaluated for one frame, cached on [`MilkInstance`].
#[derive(Clone, Debug, Default)]
pub struct OverlayFrame {
    pub waves: Vec<WaveOverlay>,
    pub shapes: Vec<ShapeOverlay>,
}

/// Byte stride of the `@group(0) @binding(0)` `MilkUniforms` block.
///
/// 256 is `wgpu::Limits::default().min_uniform_buffer_offset_alignment`, so the
/// same buffer can be used with dynamic offsets on every backend.
pub const MILK_UNIFORM_STRIDE: usize = 256;

/// Bytes in the post/legacy-composite uniform block (five `vec4`s).
pub const MILK_POST_UNIFORM_BYTES: usize = 80;

/// Typed failures from compiling or driving a MilkDrop preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MilkRuntimeError {
    /// Compiling the equations or translating a shader failed.
    Compile(String),
    /// Running an equation set for a frame/vertex failed.
    Evaluate(String),
    /// Building or submitting a GPU pass failed (e.g. invalid WGSL).
    Draw(String),
}

/// Names of modules whose preset failed to compile.
///
/// Device-free and shareable: the compositor owns one cache per registry and
/// clears it (via [`MilkRuntime::clear_failures`]) when the registry is
/// replaced. Interior mutability keeps the `compile_preset(&cache)` signature
/// borrow-only.
#[derive(Clone, Debug, Default)]
pub struct MilkFailureCache {
    // `Arc` keeps the handle `Clone` (and shareable with the compositor) while
    // `Mutex` gives the `&self` methods interior mutability. Poison is recovered
    // rather than propagated: a failed compilation must never abort rendering.
    failed: Arc<Mutex<HashSet<String>>>,
}

impl MilkFailureCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `name` as having failed compilation.
    pub fn mark_failed(&self, name: &str) {
        let mut failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        failed.insert(name.to_string());
    }

    /// Whether `name` is currently marked as failed.
    pub fn is_failed(&self, name: &str) -> bool {
        let failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        failed.contains(name)
    }

    /// Forget all recorded failures.
    pub fn clear(&self) {
        let mut failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        failed.clear();
    }
}

/// Serialized `MilkUniforms` payload for one frame.
///
/// This holds the MilkDrop scalars/`vec4`/`vec2` members described by
/// `milkdrop::translate::UNIFORM_MEMBERS` (globals, base vals, rand values and
/// derived quantities) for one frame. [`MilkUniforms::to_bytes`] always returns
/// exactly [`MILK_UNIFORM_STRIDE`] bytes so the buffer can be used with dynamic
/// offsets.
pub struct MilkUniforms {
    bytes: Vec<u8>,
}

impl MilkUniforms {
    /// Build the uniform payload for `frame` from the live `instance` state.
    ///
    /// Layout, in bytes into the shared 256-byte slice:
    /// * `0..=127` — the 32 `f32` scalars of `translate::UNIFORM_MEMBERS`.
    /// * `128..=143` — `texsize` as a `vec4` (width, height, 1/width, 1/height):
    ///   the real MilkDrop 2 contract, 16-byte aligned.
    /// * `144..=199` — seven `vec2` members (`texsize_fw/pw/fc/pc`, `uv_orig_u`,
    ///   `rad`, `ang`), 8-byte aligned.
    /// * `208..=247` — ten `f32` output-pass controls (post-processing flags and
    ///   deterministic rand values), starting at `OUTPUT_CONTROLS_OFFSET`. The
    ///   gap past `ang` keeps the warp/comp and output interpretations of the
    ///   buffer from overlapping; `OUTPUT_FRAGMENT_WGSL` mirrors it with 13
    ///   `vec4` pads before the controls.
    ///
    /// One slice serves every pass: warp/comp read the first 200 bytes as
    /// `MilkUniforms`, the output stage reads the same buffer as
    /// `MilkOutputUniforms`.
    pub fn from_frame(instance: &MilkInstance, frame: &MilkFrameInfo) -> Self {
        let mut bytes = vec![0u8; MILK_UNIFORM_STRIDE];
        let g = &instance.globals;
        let base = &instance.compiled.doc.base;
        let live_or = |name: &str, fallback: f64| {
            let value = instance.pool.get(name);
            if value.is_finite() {
                value
            } else {
                fallback as f32
            }
        };

        // The 32 scalars of the translated uniform block, in declaration order.
        let aspect = if g.aspecty.abs() > 1e-6 {
            g.aspectx / g.aspecty
        } else {
            1.0
        };
        // A zero pool value means "never assigned"; derive a deterministic
        // stand-in so two instances with the same seed still agree.
        let rand_frame = effective_rand(instance.pool.get("rand_frame"), 0.25, frame.frame_index);
        let rand_preset = effective_rand(instance.pool.get("rand_preset"), 0.75, frame.frame_index);
        let scalars = [
            g.time,                          // time
            g.frame,                         // frame
            g.fps,                           // fps
            0.0,                             // progress
            live_or("decay", base.decay),    // decay
            g.bass,                          // bass
            g.mid,                           // mid
            g.treb,                          // treb
            g.bass_att,                      // bass_att
            g.mid_att,                       // mid_att
            g.treb_att,                      // treb_att
            (g.bass + g.mid + g.treb) / 3.0, // vol
            rand_preset,                     // rand_preset
            rand_frame,                      // rand_frame
            aspect,                          // aspect
            0.0,                             // slow_roam_cos
            0.0,                             // slow_roam_sin
            0.0,                             // roam_cos
            0.0,                             // roam_sin
            0.0,                             // hue_shader
            1.0,                             // scale1 (no preset field; neutral)
            1.0,                             // scale2
            1.0,                             // scale3
            0.0,                             // bias1
            0.0,                             // bias2
            0.0,                             // bias3
            live_or("b1n", base.b1n),        // blur1_min
            live_or("b2n", base.b2n),        // blur2_min
            live_or("b3n", base.b3n),        // blur3_min
            1.0,                             // blur1_max
            1.0,                             // blur2_max
            1.0,                             // blur3_max
        ];
        for (index, value) in scalars.iter().enumerate() {
            write_f32(&mut bytes, index * 4, *value);
        }

        // `texsize` is a vec4 at byte 128 (16-byte aligned): x=width,
        // y=height, z=1/width, w=1/height — the real MilkDrop 2 contract.
        let width = frame.width as f32;
        let height = frame.height as f32;
        write_f32(&mut bytes, 128, width);
        write_f32(&mut bytes, 132, height);
        write_f32(&mut bytes, 136, 1.0 / width);
        write_f32(&mut bytes, 140, 1.0 / height);
        // The seven downstream vec2 members follow at byte 144 (8-byte
        // aligned): the fw/pw/fc/pc sizes, uv_orig_u, rad and ang. `ang` ends
        // at byte 199; the output controls start at `OUTPUT_CONTROLS_OFFSET`.
        let size = [width, height];
        write_vec2(&mut bytes, 144, size); // texsize_fw
        write_vec2(&mut bytes, 152, size); // texsize_pw
        write_vec2(&mut bytes, 160, size); // texsize_fc
        write_vec2(&mut bytes, 168, size); // texsize_pc
        // uv_orig_u: legacy uniform slot kept for byte-layout stability; the
        // translated shaders read `uv_orig` from the vertex varying instead.
        write_vec2(&mut bytes, 176, [0.5, 0.5]);
        write_vec2(&mut bytes, 184, [0.5, 0.5]); // rad
        write_vec2(&mut bytes, 192, [0.0, 0.0]); // ang

        Self { bytes }
    }

    /// The WGSL-aligned bytes to upload into `@group(0) @binding(0)`.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

/// Four-corner colours of MilkDrop's legacy hue shader at `time`.
///
/// This is the host-side twin of the WGSL `hue_corner` function and follows
/// Butterchurn/MilkDrop exactly: each corner channel is a sine of `time`,
/// normalised by the largest channel and mapped to `[0.5, 1.0]`.
pub fn hue_corner_colors(time: f32, rand_start: [f32; 4]) -> [[f32; 3]; 4] {
    let mut out = [[1.0f32; 3]; 4];
    for (index, corner) in out.iter_mut().enumerate() {
        let i = index as f32;
        corner[0] = 0.6 + 0.3 * (time * 30.0 * 0.0143 + 3.0 + i * 21.0 + rand_start[3]).sin();
        corner[1] = 0.6 + 0.3 * (time * 30.0 * 0.0107 + 1.0 + i * 13.0 + rand_start[1]).sin();
        corner[2] = 0.6 + 0.3 * (time * 30.0 * 0.0129 + 6.0 + i * 9.0 + rand_start[2]).sin();
        let max = corner[0].max(corner[1]).max(corner[2]).max(1e-6);
        for channel in corner.iter_mut() {
            *channel = 0.5 + 0.5 * (*channel / max);
        }
    }
    out
}

/// Bilinearly interpolate [`hue_corner_colors`] at `uv` (top-left origin,
/// `[0, 1]`), matching Butterchurn's `generateCompColors`.
pub fn hue_shader_at(uv: [f32; 2], corners: &[[f32; 3]; 4]) -> [f32; 3] {
    let x = uv[0];
    let y = uv[1];
    std::array::from_fn(|channel| {
        corners[0][channel] * x * y
            + corners[1][channel] * (1.0 - x) * y
            + corners[2][channel] * x * (1.0 - y)
            + corners[3][channel] * (1.0 - x) * (1.0 - y)
    })
}

/// Serialized post/legacy-composite uniform block for one frame.
///
/// Reads the **live** EEL values seeded and updated by [`begin_frame`], so a
/// preset whose per-frame equations write `gamma = gv;`, `echo_alpha = ...`
/// or `fshader = ...` drives the composite exactly like MilkDrop.
pub struct MilkPostUniforms {
    bytes: Vec<u8>,
}

impl MilkPostUniforms {
    pub fn from_instance(instance: &MilkInstance) -> Self {
        let mut bytes = vec![0u8; MILK_POST_UNIFORM_BYTES];
        let base = &instance.compiled.doc.base;
        let live = |name: &str, fallback: f64| {
            let value = instance.pool.get(name);
            if value.is_finite() {
                value
            } else {
                fallback as f32
            }
        };
        // `gamma` is MilkDrop 1's per-frame name; `gammaadj` is Butterchurn's.
        // Seed both in `begin_frame`, and prefer `gamma` when it is non-zero.
        let gamma_live = instance.pool.get("gamma");
        let gamma = if gamma_live != 0.0 {
            gamma_live
        } else {
            live("gammaadj", base.gammaadj)
        };
        let legacy = if instance.compiled.legacy_composite {
            1.0
        } else {
            0.0
        };
        let values = [
            instance.globals.time,
            gamma,
            live("fshader", base.shader),
            live("echo_zoom", base.echo_zoom),
            live("echo_alpha", base.echo_alpha),
            live("echo_orient", base.echo_orient as f64),
            if instance.pool.get("brighten") != 0.0 {
                1.0
            } else {
                0.0
            },
            if instance.pool.get("darken") != 0.0 {
                1.0
            } else {
                0.0
            },
            if instance.pool.get("solarize") != 0.0 {
                1.0
            } else {
                0.0
            },
            if instance.pool.get("invert") != 0.0 {
                1.0
            } else {
                0.0
            },
            if instance.pool.get("red_blue") != 0.0 {
                1.0
            } else {
                0.0
            },
            legacy,
            instance.rand_start[0],
            instance.rand_start[1],
            instance.rand_start[2],
            instance.rand_start[3],
            live("brightness", base.brightness),
            live("decay", base.decay),
            0.0,
            0.0,
        ];
        for (index, value) in values.iter().enumerate() {
            write_f32(&mut bytes, index * 4, *value);
        }
        Self { bytes }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

/// Serialized `MilkQUniforms` payload for one frame.
///
/// Real v2 presets keep their per-frame `q1..q32` values in the EEL
/// [`VarPool`]. Those are read from the live instance and packed into a
/// dedicated block (one `f32` per name in [`Q_UNIFORM_MEMBERS`]) uploaded to
/// `@group(0)` at [`MILK_Q_BINDING`] each frame. This block is independent of
/// [`MilkUniforms`], so none of its byte offsets or the sampler layout move.
pub struct MilkQUniforms {
    bytes: Vec<u8>,
}

impl MilkQUniforms {
    /// Read `q1..q32` from `instance`'s live variable pool into GPU bytes.
    pub fn from_instance(instance: &MilkInstance) -> Self {
        let mut bytes = vec![0u8; MILK_Q_STRIDE];
        for (index, name) in Q_UNIFORM_MEMBERS.iter().enumerate() {
            write_f32(
                &mut bytes,
                index * std::mem::size_of::<f32>(),
                instance.pool.get(name),
            );
        }
        Self { bytes }
    }

    /// The bytes to upload into the q binding (always [`MILK_Q_STRIDE`] bytes).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

/// Off-screen render targets owned by the MilkDrop GPU path.
///
/// The warp, composite, feedback and three blur stages render into these
/// textures; the final post pass writes into the caller-supplied module
/// target. All targets are `Rgba8Unorm` and are reallocated only when the size
/// changes.
pub struct MilkGpuTargets {
    pub width: u32,
    pub height: u32,
    warp: Option<wgpu::Texture>,
    comp: Option<wgpu::Texture>,
    /// Pre-post composite copied out for the next frame's warp input, matching
    /// Butterchurn's `prevTexture` feedback contract (the post/filters output
    /// is displayed but never fed back).
    feedback: Option<wgpu::Texture>,
    feedback_cleared: bool,
    blur: [Option<wgpu::Texture>; 3],
}

impl MilkGpuTargets {
    /// Allocate the warp/comp/blur targets at `width`×`height`.
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let mut targets = Self {
            width,
            height,
            warp: None,
            comp: None,
            feedback: None,
            feedback_cleared: false,
            blur: [None, None, None],
        };
        targets.allocate(device);
        targets
    }

    /// Allocate (or replace) all intermediate textures.
    fn allocate(&mut self, device: &wgpu::Device) {
        self.warp = Some(create_target(
            device,
            self.width,
            self.height,
            "modV MilkDrop warp",
        ));
        self.comp = Some(create_target(
            device,
            self.width,
            self.height,
            "modV MilkDrop comp",
        ));
        self.feedback = Some(create_target(
            device,
            self.width,
            self.height,
            "modV MilkDrop feedback",
        ));
        self.feedback_cleared = false;
        self.blur = [0, 1, 2].map(|index| {
            Some(create_target(
                device,
                self.width,
                self.height,
                &format!("modV MilkDrop blur{index}"),
            ))
        });
    }

    /// Reallocate only when the requested size differs from the current one.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.allocate(device);
    }

    /// Warp-pass target (previous frame resampled through the `uv_orig` varying).
    pub fn warp_texture(&self) -> Option<&wgpu::Texture> {
        self.warp.as_ref()
    }

    /// Composite-pass target.
    pub fn comp_texture(&self) -> Option<&wgpu::Texture> {
        self.comp.as_ref()
    }

    /// Pre-post composite retained as next frame's warp feedback source.
    pub fn feedback_texture(&self) -> Option<&wgpu::Texture> {
        self.feedback.as_ref()
    }

    /// Blur target `index` (0, 1 or 2) feeding `GetBlur{1,2,3}`.
    pub fn blur_texture(&self, index: usize) -> Option<&wgpu::Texture> {
        self.blur.get(index).and_then(|texture| texture.as_ref())
    }
}

/// Per-compositor owner of MilkDrop state.
///
/// Holds the shared failure cache plus optional device/queue handles and the
/// bounded pipeline/sampler caches. [`MilkRuntime::default`] stays device-free
/// so the CPU tests need no adapter; the compositor constructs one with
/// [`MilkRuntime::new`] and calls [`MilkRuntime::render`] once per module.
#[derive(Clone)]
pub struct MilkRuntime {
    pub failures: MilkFailureCache,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
    cache_capacity: usize,
    /// Device-owned pipeline/sampler/layout objects behind interior
    /// mutability, so `render(&self, ..)` can populate them.
    cache: Arc<Mutex<MilkGpuCache>>,
}

impl Default for MilkRuntime {
    fn default() -> Self {
        Self {
            failures: MilkFailureCache::new(),
            device: None,
            queue: None,
            cache_capacity: 4,
            cache: Arc::new(Mutex::new(MilkGpuCache::new(4))),
        }
    }
}

impl fmt::Debug for MilkRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MilkRuntime")
            .field("failures", &self.failures)
            .field("has_device", &self.device.is_some())
            .field("cache_capacity", &self.cache_capacity)
            .finish()
    }
}

impl MilkRuntime {
    /// Create a runtime backed by `device`/`queue`, with caches bounded to
    /// `cache_capacity` entries (4 matches the ISF runtime's convention).
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, cache_capacity: usize) -> Self {
        let capacity = cache_capacity.max(1);
        Self {
            failures: MilkFailureCache::new(),
            device: Some(device),
            queue: Some(queue),
            cache_capacity: capacity,
            cache: Arc::new(Mutex::new(MilkGpuCache::new(capacity))),
        }
    }

    /// Forget every cached compilation failure (called from
    /// `Compositor::set_registry` when the registry is replaced).
    pub fn clear_failures(&self) {
        self.failures.clear();
    }

    /// Execute one frame of the warp → blur → comp → overlays → post core into
    /// `output`.
    ///
    /// `previous` seeds the internal feedback texture on the first render after
    /// allocation (or a resize). Subsequent frames feed the runtime's own
    /// pre-post composite back into the warp; the filtered `output` is never
    /// sampled again. `targets` owns the intermediate warp/comp/feedback/blur
    /// textures. A malformed shader yields [`MilkRuntimeError::Draw`] rather
    /// than a panic.
    ///
    /// `mesh`'s warped UV grid is uploaded for the warp pass, which draws the
    /// indexed mesh so `sampler_main` is sampled at each vertex's warped UV.
    /// The blur passes apply a real 5x5 Gaussian kernel whose radius is driven
    /// by the `b1n`/`b2n`/`b3n` base vals.
    pub fn render(
        &self,
        instance: &mut MilkInstance,
        frame: &MilkFrameInfo,
        mesh: &MilkMesh,
        previous: &wgpu::TextureView,
        targets: &mut MilkGpuTargets,
        output: &wgpu::TextureView,
    ) -> Result<(), MilkRuntimeError> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop runtime has no wgpu device".into()))?;
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop runtime has no wgpu queue".into()))?;
        // The cache capacity is applied in `new`/`default` when the cache is
        // created; the mesh itself is uploaded once per frame below.
        let _ = self.cache_capacity;
        let gpu_mesh = MilkMeshGpu::new(device, mesh);

        targets.resize(device, frame.width.max(1), frame.height.max(1));

        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.ensure_common(device, queue);

        // Compile (or fetch from the bounded LRU) each pass pipeline. Warp and
        // comp are keyed by their WGSL so a preset swap invalidates the entry.
        let warp_key = format!("wmesh|{}", instance.compiled.warp.wgsl);
        let comp_key = format!("comp|{}", instance.compiled.comp.wgsl);
        let warp_pipeline =
            cache.mesh_pipeline(device, warp_key, &instance.compiled.warp.wgsl, "main")?;
        let comp_pipeline = cache.pipeline(
            device,
            comp_key,
            &instance.compiled.comp.wgsl,
            "main",
            MilkLayout::WarpComp,
        )?;
        let blur_pipeline = cache.pipeline(
            device,
            "blur|gaussian5x5-v1".into(),
            BLUR_FRAGMENT_WGSL,
            "fs_main",
            MilkLayout::Pass,
        )?;
        let copy_pipeline = cache.pipeline(
            device,
            "copy|v1".into(),
            COPY_FRAGMENT_WGSL,
            "fs_main",
            MilkLayout::Pass,
        )?;
        let post_pipeline = cache.pipeline(
            device,
            "post|v2".into(),
            POST_FRAGMENT_WGSL,
            "fs_main",
            MilkLayout::Output,
        )?;

        // One 256-byte uniform slice serves the warp/comp/blur-translated
        // passes; the post stage has its own compact 80-byte block.
        let uniform_bytes = MilkUniforms::from_frame(instance, frame).to_bytes();
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modV MilkDrop uniform buffer"),
            size: MILK_UNIFORM_STRIDE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, &uniform_bytes);
        let post_bytes = MilkPostUniforms::from_instance(instance).to_bytes();
        let post_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modV MilkDrop post uniform buffer"),
            size: MILK_POST_UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&post_uniform_buffer, 0, &post_bytes);
        // The q block carries the instance's live `q1..q32` values to
        // `@group(0)` at `MILK_Q_BINDING`. Kept separate from the main uniform
        // so none of its byte offsets or the sampler bindings change.
        let q_bytes = MilkQUniforms::from_instance(instance).to_bytes();
        let q_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modV MilkDrop q uniform buffer"),
            size: MILK_Q_STRIDE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&q_uniform_buffer, 0, &q_bytes);
        // One small uniform per blur level carries the radius (in texels)
        // derived from the `b1n`/`b2n`/`b3n` base-val minima. The kernel is a
        // normalized 5x5 Gaussian in `BLUR_FRAGMENT_WGSL`, so a non-zero
        // minimum produces a real blur that spills into neighbouring texels.
        let base = &instance.compiled.doc.base;
        let live_or = |name: &str, fallback: f64| {
            let value = instance.pool.get(name);
            if value.is_finite() {
                value as f64
            } else {
                fallback
            }
        };
        let blur_strengths = [
            live_or("b1n", base.b1n),
            live_or("b2n", base.b2n),
            live_or("b3n", base.b3n),
        ];
        let blur_uniforms: Vec<wgpu::Buffer> = blur_strengths
            .iter()
            .map(|&strength| {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("modV MilkDrop blur radius"),
                    size: SMALL_UNIFORM_BYTES,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let mut data = [0u8; SMALL_UNIFORM_BYTES as usize];
                write_f32(&mut data, 0, blur_radius_texels(strength));
                queue.write_buffer(&buffer, 0, &data);
                buffer
            })
            .collect();

        let warp_view = targets
            .warp_texture()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop warp target missing".into()))?
            .create_view(&wgpu::TextureViewDescriptor::default());
        let comp_view = targets
            .comp_texture()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop comp target missing".into()))?
            .create_view(&wgpu::TextureViewDescriptor::default());
        let feedback_view = targets
            .feedback_texture()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop feedback target missing".into()))?
            .create_view(&wgpu::TextureViewDescriptor::default());
        let blur_views: Vec<wgpu::TextureView> = (0..3)
            .map(|index| {
                targets
                    .blur_texture(index)
                    .ok_or_else(|| {
                        MilkRuntimeError::Draw(format!("MilkDrop blur{index} target missing"))
                    })
                    .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()))
            })
            .collect::<Result<_, _>>()?;
        let blur_refs: [&wgpu::TextureView; 3] = [&blur_views[0], &blur_views[1], &blur_views[2]];

        let sampler = cache
            .sampler
            .clone()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop sampler missing".into()))?;
        let dummy_view = cache
            .dummy
            .as_ref()
            .map(|(_, view)| view.clone())
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop dummy texture missing".into()))?;
        let noise_views = cache
            .noise
            .as_ref()
            .map(|noise| noise.views.clone())
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop noise textures missing".into()))?;
        let noise_refs: [&wgpu::TextureView; 4] = [
            &noise_views[0],
            &noise_views[1],
            &noise_views[2],
            &noise_views[3],
        ];
        let milk_layout = cache
            .milk_layout
            .clone()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop shader layout missing".into()))?;
        let pass_layout = cache
            .pass_layout
            .clone()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop blur layout missing".into()))?;
        let output_layout = cache
            .output_layout
            .clone()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop output layout missing".into()))?;

        // Butterchurn feeds the *pre-post* composite (warp + overlays) back
        // into the next warp. The post-processed image is display-only; using
        // it as feedback makes gamma/darken/solarize compound into noise.
        let warp_main = &feedback_view;
        let warp_views = MilkSamplerViews {
            main: warp_main,
            dummy: &dummy_view,
            blur: blur_refs,
            noise: noise_refs,
        };
        let comp_views = MilkSamplerViews {
            main: &warp_view,
            dummy: &dummy_view,
            blur: blur_refs,
            noise: noise_refs,
        };
        let warp_bind = milk_bind_group(
            device,
            &milk_layout,
            &uniform_buffer,
            &q_uniform_buffer,
            &sampler,
            warp_views,
        );
        let comp_bind = milk_bind_group(
            device,
            &milk_layout,
            &uniform_buffer,
            &q_uniform_buffer,
            &sampler,
            comp_views,
        );
        let output_bind = output_bind_group(
            device,
            &output_layout,
            &post_uniform_buffer,
            &comp_view,
            previous,
            &sampler,
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("modV MilkDrop frame"),
        });

        // Freshly allocated feedback is seeded from the caller's previous
        // frame once, then the runtime retains its own pre-post composite.
        if !targets.feedback_cleared {
            let seed_bind =
                pass_bind_group(device, &pass_layout, &blur_uniforms[0], previous, &sampler);
            encode_fullscreen_pass(
                &mut encoder,
                "milkdrop feedback seed",
                &feedback_view,
                &copy_pipeline,
                &seed_bind,
            );
            targets.feedback_cleared = true;
        }

        // warp: resample `previous` at each mesh vertex's warped UV, drawn as
        // an indexed triangle list so `zoom`/`rot`/`warp` base vals take effect.
        encode_mesh_pass(
            &mut encoder,
            "milkdrop warp",
            &warp_view,
            &warp_pipeline,
            &warp_bind,
            &gpu_mesh,
        );
        // blur1/2/3: populate the blur targets feeding GetBlur{1,2,3} with a
        // real Gaussian kernel whose radius comes from b1n/b2n/b3n. Each level
        // blurs the previous one (blur1 starts from the warped frame), so the
        // three radii accumulate down the chain.
        let blur_sources: [&wgpu::TextureView; 3] = [&warp_view, &blur_views[0], &blur_views[1]];
        for (index, source) in blur_sources.iter().enumerate() {
            let bind = pass_bind_group(
                device,
                &pass_layout,
                &blur_uniforms[index],
                source,
                &sampler,
            );
            encode_fullscreen_pass(
                &mut encoder,
                &format!("milkdrop blur{}", index + 1),
                &blur_views[index],
                &blur_pipeline,
                &bind,
            );
        }
        // comp: translated composite shader, sampling the warped frame.
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop comp",
            &comp_view,
            &comp_pipeline,
            &comp_bind,
        );
        // Layer 5c: evaluate overlays and draw them onto the composite target
        // *before* the post/legacy composite, exactly like MilkDrop's
        // `DrawCustomWaves()` → `ShowToUser()` order. Drawing after the filters
        // would keep raw wave colours and break the feedback rings created when
        // the filters re-shape warped intermediate values.
        let _ = evaluate_overlays(instance, frame);
        if !instance.overlay_frame.waves.is_empty() || !instance.overlay_frame.shapes.is_empty() {
            let alpha_pipeline = cache.overlay_pipeline(device, false, false)?;
            let additive_pipeline = cache.overlay_pipeline(device, true, false)?;
            let alpha_dots = cache.overlay_pipeline(device, false, true)?;
            let additive_dots = cache.overlay_pipeline(device, true, true)?;
            encode_overlay_passes(
                device,
                queue,
                &mut encoder,
                &comp_view,
                &instance.overlay_frame,
                &alpha_pipeline,
                &additive_pipeline,
                &alpha_dots,
                &additive_dots,
            );
        }
        // Retain the pre-post composite (warp + overlays) as next frame's
        // feedback source, before the echo/gamma/filter pass runs.
        if let (Some(comp), Some(feedback)) = (targets.comp_texture(), targets.feedback_texture()) {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: comp,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: feedback,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: targets.width.max(1),
                    height: targets.height.max(1),
                    depth_or_array_layers: 1,
                },
            );
        }
        // post: echo + gamma + legacy hue + fixed filters into the caller's
        // target. This output is display-only; feedback comes from the copy
        // above.
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop post",
            output,
            &post_pipeline,
            &output_bind,
        );

        queue.submit(Some(encoder.finish()));
        Ok(())
    }
}

/// Default identity shader body for presets with no `shader_body` at all
/// (notably v1 presets, which predate per-stage HLSL). Sampling the previous
/// frame at the mesh UV makes warp and comp pass-through stages.
/// MilkDrop's default warp multiplies the previous frame by `fDecay`; 1 keeps
/// the whole frame, 0 discards it. The identity composite then just passes that
/// warped frame through, so the two default stages must not both apply decay.
const DEFAULT_WARP_BODY: &str =
    "float3 ret = tex2D(sampler_main, uv_orig).rgb * (decay > 0.0 ? decay : 1.0);\n";
const DEFAULT_COMP_BODY: &str = "float3 ret = tex2D(sampler_main, uv_orig).rgb;\n";

/// The body to translate for one stage: the preset's own body verbatim, or the
/// identity body when it is empty. v1 presets carry no `shader_body`, so without
/// this substitution `ret` would be undeclared and the translated shader would
/// fail naga validation. Sampling the previous frame keeps such stages as
/// pass-through instead.
fn effective_shader_body(shader: &MilkShader, stage: MilkShaderStage) -> &str {
    if shader.body.trim().is_empty() {
        match stage {
            MilkShaderStage::Warp => DEFAULT_WARP_BODY,
            MilkShaderStage::Comp => DEFAULT_COMP_BODY,
        }
    } else {
        &shader.body
    }
}

/// Compile a parsed preset into an owned handle.
///
/// On a shader/equation error the offending `name` is recorded in `failures`
/// and [`MilkRuntimeError::Compile`] is returned; this never panics.
pub fn compile_preset(
    name: &str,
    doc: &MilkDocument,
    failures: &MilkFailureCache,
) -> Result<MilkCompiledPreset, MilkRuntimeError> {
    // One failure path for every stage: record the module, then return a typed
    // error. Closures keep the borrow-only cache signature intact.
    let fail = |message: String| {
        failures.mark_failed(name);
        MilkRuntimeError::Compile(message)
    };

    let init =
        compile(&doc.init_eqs, doc.version).map_err(|e| fail(format!("init equations: {e}")))?;
    let frame =
        compile(&doc.frame_eqs, doc.version).map_err(|e| fail(format!("frame equations: {e}")))?;
    let pixel =
        compile(&doc.pixel_eqs, doc.version).map_err(|e| fail(format!("pixel equations: {e}")))?;

    let warp_src = extract_shader_body(&doc.warp);
    let warp = translate_milk_shader(
        &warp_src.prelude,
        effective_shader_body(&warp_src, MilkShaderStage::Warp),
        MilkShaderStage::Warp,
    )
    .map_err(|e| fail(format!("warp shader: {e}")))?;

    let comp_src = extract_shader_body(&doc.comp);
    let legacy_composite = comp_src.body.trim().is_empty();
    let comp = translate_milk_shader(
        &comp_src.prelude,
        effective_shader_body(&comp_src, MilkShaderStage::Comp),
        MilkShaderStage::Comp,
    )
    .map_err(|e| fail(format!("comp shader: {e}")))?;

    // Layer 5c: compile one program per enabled wave/shape section. A malformed
    // section is recorded and skipped individually so one broken overlay never
    // fails the whole preset. Section order is preserved (index order).
    let version = doc.version;
    let mut waves = Vec::new();
    for (index, wave) in doc.waves.iter().enumerate() {
        if !wave.enabled {
            continue;
        }
        let init_wave = compile(&wave.init_eqs, version).ok();
        let frame_wave = compile(&wave.frame_eqs, version).ok();
        let point_wave = compile(&wave.point_eqs, version).ok();
        if let (Some(init), Some(frame), Some(point)) = (init_wave, frame_wave, point_wave) {
            waves.push(WaveProgram {
                init,
                frame,
                point,
                values: wave.values.clone(),
                dots: wave.base.wave_dots,
            });
        } else {
            failures.mark_failed(&format!("{name}/wave{index}"));
            eprintln!("MilkDrop {name}: skipping malformed wave {index}");
        }
    }
    let mut shapes = Vec::new();
    for (index, shape) in doc.shapes.iter().enumerate() {
        if !shape.enabled {
            continue;
        }
        let init_shape = compile(&shape.init_eqs, version).ok();
        let frame_shape = compile(&shape.frame_eqs, version).ok();
        if let (Some(init), Some(frame)) = (init_shape, frame_shape) {
            shapes.push(ShapeProgram { init, frame });
        } else {
            failures.mark_failed(&format!("{name}/shape{index}"));
            eprintln!("MilkDrop {name}: skipping malformed shape {index}");
        }
    }

    Ok(MilkCompiledPreset {
        doc: doc.clone(),
        init,
        frame,
        pixel,
        warp,
        comp,
        legacy_composite,
        waves,
        shapes,
    })
}

/// One active module instance: compiled equations, a persistent variable pool
/// (which also holds the seeded RNG), the latest globals, persistent band
/// followers and the copied wave/spectrum data.
#[derive(Clone, Debug)]
pub struct MilkInstance {
    pub compiled: MilkCompiledPreset,
    pub pool: VarPool,
    pub globals: Globals,
    /// `[bass, mid, treble]` exponential followers, persistent across frames.
    pub band_att: [f32; 3],
    /// Slow per-band average used to make `bass`/`mid`/`treb` *relative*
    /// (Butterchurn's `longAvg`). The fast [`Self::band_att`] follower feeds
    /// `bass_att`/`mid_att`/`treb_att`.
    pub band_long_att: [f32; 3],
    /// Current FFT band sums (Butterchurn's `imm`) for this frame.
    pub band_imm: [f32; 3],
    /// Inspector input-prop overrides, applied after the per-frame equations
    /// so a user control wins over authored animation. Keys are EEL variable
    /// names from [`crate::milkdrop::MILKDROP_LOOK_PROP_NAMES`].
    pub overrides: Vec<(&'static str, f32)>,
    pub frame_index: u32,
    /// Whether the init equations have run for this preset.
    pub init_done: bool,
    /// Four deterministic per-preset phases for the legacy hue shader
    /// (`rand_start` in MilkDrop/Butterchurn).
    pub rand_start: [f32; 4],
    pub wave: Vec<f32>,
    pub byte_frequency_data: Vec<u8>,
    /// Persistent per-section overlay state (Layer 5c). Indexed in enabled-wave
    /// / enabled-shape order, matching `compiled.waves` / `compiled.shapes`.
    pub wave_states: Vec<OverlayState>,
    pub shape_states: Vec<OverlayState>,
    /// Overlays evaluated for the current frame (Layer 5c), cached here by
    /// [`evaluate_overlays`] so the GPU pass can consume them.
    pub overlay_frame: OverlayFrame,
}

/// Build an instance for `compiled` whose `rand` draws are reproducible via
/// `seed`.
pub fn new_instance(compiled: MilkCompiledPreset, seed: u64) -> MilkInstance {
    // `rand_start` is independent of the EEL RNG stream: derive it from the
    // same seed with a distinct mix so determinism and preset-to-preset variety
    // both hold.
    let mut rand_rng = crate::milkdrop::eel::Rng::new(
        seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0x1234_5678_9ABC_DEF0),
    );
    MilkInstance {
        compiled,
        pool: VarPool::with_seed(seed),
        globals: Globals::default(),
        band_att: [0.0; 3],
        band_long_att: [0.0; 3],
        band_imm: [0.0; 3],
        overrides: Vec::new(),
        frame_index: 0,
        init_done: false,
        rand_start: std::array::from_fn(|_| rand_rng.next_f32()),
        wave: Vec::new(),
        byte_frequency_data: Vec::new(),
        wave_states: Vec::new(),
        shape_states: Vec::new(),
        overlay_frame: OverlayFrame::default(),
    }
}

/// MilkDrop's built-in waveform modes 0..=7.
///
/// This is a direct port of Butterchurn's `basicWaveform.js` (which follows
/// `DrawWave` in MilkDrop). The preset's `nWaveMode` selects the mode and the
/// per-frame variables `wave_x/y/scale/smoothing/mystery/a` drive it. The
/// built-in wave is drawn **alongside** custom waves, not only when there are
/// no `wavecode_*` sections.
///
/// Returns one overlay per disjoint polyline (mode 7 draws separate left/right
/// lines). All overlays share the same colour/additive pool.
fn evaluate_builtin_waves(instance: &MilkInstance, audio_wave: &[f32]) -> Vec<WaveOverlay> {
    let pool = &instance.pool;
    let max_samples = audio_wave.len().min(512);
    if max_samples < 2 {
        return Vec::new();
    }
    let source = &audio_wave[audio_wave.len() - max_samples..];

    let scale = pool.get("wave_scale");
    let smooth = pool.get("wave_smoothing").clamp(0.0, 0.98);
    let smooth2 = scale * (1.0 - smooth);
    let mut wave = Vec::with_capacity(max_samples);
    wave.push(source[0] * scale);
    for index in 1..max_samples {
        wave.push(source[index] * smooth2 + wave[index - 1] * smooth);
    }

    let mut alpha = pool.get("wave_a");
    if pool.get("modwavealphabyvolume") > 0.5 {
        let start = pool.get("modwavealphastart");
        let end = pool.get("modwavealphaend");
        if (end - start).abs() > 1e-6 {
            let vol = (instance.globals.bass + instance.globals.mid + instance.globals.treb) / 3.0;
            alpha *= (vol - start) / (end - start);
        }
    }
    alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.001 {
        return Vec::new();
    }

    let color = [
        pool.get("wave_r").clamp(0.0, 1.0),
        pool.get("wave_g").clamp(0.0, 1.0),
        pool.get("wave_b").clamp(0.0, 1.0),
        alpha,
    ];
    let mut overlay_pool = VarPool::new();
    overlay_pool.set("r", color[0]);
    overlay_pool.set("g", color[1]);
    overlay_pool.set("b", color[2]);
    overlay_pool.set("a", color[3]);
    overlay_pool.set(
        "additive",
        if pool.get("additivewave") > 0.5 {
            1.0
        } else {
            0.0
        },
    );
    let dots = pool.get("wave_dots") > 0.5;
    let rt = instance.globals.aspectx;
    let up = instance.globals.aspecty;
    let width = instance.globals.pixelsx.max(1.0);
    let mode = (pool.get("wave_mode").floor().max(0.0) as i64 % 8) as usize;
    let pos_x = pool.get("wave_x") * 2.0 - 1.0;
    let pos_y = pool.get("wave_y") * 2.0 - 1.0;
    let time = instance.globals.time;
    let param = pool.get("wave_mystery");
    let len = wave.len();
    let (wave_l, wave_r) = (&wave, &wave);

    let point = |x: f32, y: f32, sample: f32| WavePoint {
        x,
        y,
        sample,
        value1: 0.0,
        value2: 0.0,
        color,
    };
    let build = |points: Vec<WavePoint>, dots: bool| WaveOverlay {
        pool: overlay_pool.clone(),
        points,
        enabled: true,
        dots,
    };

    match mode {
        0 => {
            // Circle / spiral.
            let num = (len / 2 + 1).max(2);
            let inv = 1.0 / (num - 1) as f32;
            let offset = (len - num) / 2;
            let mut p = Vec::with_capacity(num);
            for i in 0..num - 1 {
                let mut rad = 0.5 + 0.4 * wave_r[i + offset] + param;
                let ang = i as f32 * inv * std::f32::consts::TAU + time * 0.2;
                if i < num / 10 {
                    let mix = i as f32 / (num as f32 * 0.1);
                    let mix = 0.5 - 0.5 * (mix * std::f32::consts::PI).cos();
                    let rad2 = 0.5 + 0.4 * wave_r[i + num + offset] + param;
                    rad = (1.0 - mix) * rad2 + rad * mix;
                }
                p.push(point(
                    rad * ang.cos() * up + pos_x,
                    rad * ang.sin() * rt + pos_y,
                    i as f32 * inv,
                ));
            }
            if let Some(first) = p.first().copied() {
                p.push(point(first.x, first.y, 1.0));
            }
            vec![build(p, dots)]
        }
        1 => {
            let mut alpha_extra = alpha * 1.25;
            alpha_extra = alpha_extra.clamp(0.0, 1.0);
            let mut overlay_pool = overlay_pool.clone();
            overlay_pool.set("a", alpha_extra);
            let mut p = Vec::with_capacity(len / 2);
            for i in 0..len / 2 {
                let rad = 0.53 + 0.43 * wave_r[i] + param;
                let ang = wave_l[(i + 32) % len] * 0.5 * std::f32::consts::PI + time * 2.3;
                p.push(point(
                    rad * ang.cos() * up + pos_x,
                    rad * ang.sin() * rt + pos_y,
                    i as f32 / (len / 2).max(1) as f32,
                ));
            }
            vec![WaveOverlay {
                pool: overlay_pool,
                points: p,
                enabled: true,
                dots,
            }]
        }
        2 | 3 => {
            let mut overlay_pool = overlay_pool.clone();
            let mut a = alpha
                * if width < 1024.0 {
                    0.09
                } else if width < 2048.0 {
                    0.11
                } else {
                    0.13
                };
            if mode == 3 {
                a = (a * 1.3 * instance.globals.treb * instance.globals.treb).clamp(0.0, 1.0);
            }
            overlay_pool.set("a", a.clamp(0.0, 1.0));
            let mut p = Vec::with_capacity(len);
            for i in 0..len {
                p.push(point(
                    wave_r[i] * up + pos_x,
                    wave_l[(i + 32) % len] * rt + pos_y,
                    i as f32 / len as f32,
                ));
            }
            vec![WaveOverlay {
                pool: overlay_pool,
                points: p,
                enabled: true,
                dots,
            }]
        }
        4 => {
            let num = len.min((width / 3.0) as usize).max(2);
            let inv = 1.0 / num as f32;
            let offset = (len - num) / 2;
            let w1 = 0.45 + 0.5 * (param * 0.5 + 0.5);
            let w2 = 1.0 - w1;
            let mut positions: Vec<[f32; 2]> = Vec::with_capacity(num);
            for i in 0..num {
                let mut x =
                    2.0 * i as f32 * inv + (pos_x - 1.0) + wave_r[(i + 25 + offset) % len] * 0.44;
                let mut y = wave_l[i + offset] * 0.47 + pos_y;
                if i > 1 {
                    x = x * w2 + w1 * (positions[i - 1][0] * 2.0 - positions[i - 2][0]);
                    y = y * w2 + w1 * (positions[i - 1][1] * 2.0 - positions[i - 2][1]);
                }
                positions.push([x, y]);
            }
            let points = positions
                .iter()
                .enumerate()
                .map(|(i, p)| point(p[0], p[1], i as f32 / num as f32))
                .collect();
            vec![build(points, dots)]
        }
        5 => {
            let mut overlay_pool = overlay_pool.clone();
            let a = alpha
                * if width < 1024.0 {
                    0.09
                } else if width < 2048.0 {
                    0.11
                } else {
                    0.13
                };
            overlay_pool.set("a", a.clamp(0.0, 1.0));
            let cos_rot = (time * 0.3).cos();
            let sin_rot = (time * 0.3).sin();
            let mut p = Vec::with_capacity(len);
            for i in 0..len {
                let ioff = (i + 32) % len;
                let x0 = wave_r[i] * wave_l[ioff] + wave_l[i] * wave_r[ioff];
                let y0 = wave_r[i] * wave_r[i] - wave_l[ioff] * wave_l[ioff];
                p.push(point(
                    (x0 * cos_rot - y0 * sin_rot) * (up + pos_x),
                    (x0 * sin_rot + y0 * cos_rot) * (rt + pos_y),
                    i as f32 / len as f32,
                ));
            }
            vec![WaveOverlay {
                pool: overlay_pool,
                points: p,
                enabled: true,
                dots,
            }]
        }
        6 | 7 => {
            let mut num = (len / 2).max(2);
            num = num.min((width / 3.0) as usize).max(2);
            let offset = (len - num) / 2;
            let ang = std::f32::consts::FRAC_PI_2 * param;
            let mut dx = ang.cos();
            let mut dy = ang.sin();
            let mut edge_x = [
                pos_x * (ang + std::f32::consts::FRAC_PI_2).cos() - dx * 3.0,
                pos_x * (ang + std::f32::consts::FRAC_PI_2).cos() + dx * 3.0,
            ];
            let mut edge_y = [
                pos_x * (ang + std::f32::consts::FRAC_PI_2).sin() - dy * 3.0,
                pos_x * (ang + std::f32::consts::FRAC_PI_2).sin() + dy * 3.0,
            ];
            for i in 0..2 {
                for j in 0..4 {
                    let (clip, value) = match j {
                        0 => (edge_x[i] > 1.1, 1.1),
                        1 => (edge_x[i] < -1.1, -1.1),
                        2 => (edge_y[i] > 1.1, 1.1),
                        _ => (edge_y[i] < -1.1, -1.1),
                    };
                    if clip {
                        let other = 1 - i;
                        let (a, b) = if j < 2 {
                            (edge_x[other], edge_x[i])
                        } else {
                            (edge_y[other], edge_y[i])
                        };
                        let t = (value - a) / (b - a);
                        let dxi = edge_x[i] - edge_x[other];
                        let dyi = edge_y[i] - edge_y[other];
                        edge_x[i] = edge_x[other] + dxi * t;
                        edge_y[i] = edge_y[other] + dyi * t;
                    }
                }
            }
            dx = (edge_x[1] - edge_x[0]) / num as f32;
            dy = (edge_y[1] - edge_y[0]) / num as f32;
            let ang2 = dy.atan2(dx);
            let perp_dx = (ang2 + std::f32::consts::FRAC_PI_2).cos();
            let perp_dy = (ang2 + std::f32::consts::FRAC_PI_2).sin();
            let sep = if mode == 7 {
                (pos_y * 0.5 + 0.5).powi(2)
            } else {
                0.0
            };
            let make_line = |samples: &[f32], sign: f32| {
                let mut p = Vec::with_capacity(num);
                for i in 0..num {
                    let offset_amount = 0.25 * samples[(i + offset).min(len - 1)] + sign * sep;
                    p.push(point(
                        edge_x[0] + dx * i as f32 + perp_dx * offset_amount,
                        edge_y[0] + dy * i as f32 + perp_dy * offset_amount,
                        i as f32 / num as f32,
                    ));
                }
                p
            };
            if mode == 7 {
                vec![
                    build(make_line(wave_l, 1.0), dots),
                    build(make_line(wave_r, -1.0), dots),
                ]
            } else {
                vec![build(make_line(wave_l, 0.0), dots)]
            }
        }
        _ => Vec::new(),
    }
}

/// Evaluate every enabled wave/shape overlay for `frame`.
///
/// Contract:
///
/// * Compilation happens once in [`compile_preset`] — one [`WaveProgram`] /
///   [`ShapeProgram`] per enabled section, with malformed sections skipped and
///   recorded in [`MilkFailureCache`] so a single broken overlay never fails
///   the whole preset.
/// * Per frame, each section's init equations run once (capturing `t1..t8` and
///   the `q1..q32` baseline into the persistent [`OverlayState`]). The runtime
///   seeds the per-wave-frame variables (`samples`, `scaling`, `smoothing`,
///   `sep`, `additive`, `usedots`, `spectrum`, `thick`, `r/g/b/a`) and the
///   per-shape-frame variables (`r/g/b/a`, `r2/g2/b2/a2`, `border_*`, `sides`,
///   `instance`, `num_inst`, `tex_zoom`, `tex_ang`, ...) before running the
///   frame equations. These defaults live in the tables above and any value a
///   preset's equations assign afterwards wins.
/// * Wave `point_eqs` are then evaluated per sample point (bounded to
///   [`MAX_WAVE_SAMPLES`]) against the section's live pool with
///   `x/y/sample/value1/value2`, producing [`WavePoint`]s in full-screen NDC.
/// * The result is cached on `instance.overlay_frame` and returned by value so
///   [`MilkRuntime::render`] can draw it. Each section is evaluated behind its
///   own `Result`: on error only that section is logged and skipped (no panic,
///   no propagation), so one broken overlay cannot disable its neighbours.
///
pub fn evaluate_overlays(
    instance: &mut MilkInstance,
    frame: &MilkFrameInfo,
) -> Result<OverlayFrame, MilkRuntimeError> {
    // `globals` is cloned so the compiled programs and the per-section states
    // (disjoint fields of `instance`) can be borrowed independently below.
    let globals = instance.globals.clone();
    let wave_count = instance.compiled.waves.len();
    let shape_count = instance.compiled.shapes.len();
    if instance.wave_states.len() < wave_count {
        instance
            .wave_states
            .resize_with(wave_count, OverlayState::default);
    }
    if instance.shape_states.len() < shape_count {
        instance
            .shape_states
            .resize_with(shape_count, OverlayState::default);
    }

    let mut result = OverlayFrame::default();

    for index in 0..wave_count {
        match evaluate_one_wave(
            &instance.compiled.waves[index],
            &mut instance.wave_states[index],
            &globals,
            &instance.pool,
            &frame.audio.wave,
            &frame.audio.byte_frequency_data,
        ) {
            Ok(overlay) => result.waves.push(overlay),
            Err(message) => eprintln!("MilkDrop: skipping wave {index}: {message}"),
        }
    }
    // The built-in waveform is drawn in addition to any custom waves, exactly
    // like MilkDrop's `DrawCustomWaves(); DrawWave(...)`. Its own `wave_a`
    // gate is checked inside `evaluate_builtin_waves`.
    result
        .waves
        .extend(evaluate_builtin_waves(instance, &frame.audio.wave));
    for index in 0..shape_count {
        match evaluate_one_shape(
            &instance.compiled.shapes[index],
            &mut instance.shape_states[index],
            &globals,
        ) {
            Ok(overlay) => result.shapes.push(overlay),
            Err(message) => eprintln!("MilkDrop: skipping shape {index}: {message}"),
        }
    }

    // Cache the freshly evaluated frame so the GPU pass can consume it.
    instance.overlay_frame = result.clone();
    Ok(result)
}

/// Write a table of runtime-owned defaults into `pool`. The section's frame
/// equations run afterwards, so any value the preset assigns wins.
fn seed_vars(pool: &mut VarPool, vars: &[(&str, f32)]) {
    for (name, value) in vars {
        pool.set(name, *value);
    }
}

/// Colour variables shared by every overlay kind. The GPU pass reads these
/// from the evaluated frame pool, so both wave and shape paths seed the same
/// deterministic default opaque-white value.
const OVERLAY_COLOR_VARS: &[(&str, f32)] = &[("r", 1.0), ("g", 1.0), ("b", 1.0), ("a", 1.0)];

/// Extra per-shape-frame variables, on top of [`OVERLAY_COLOR_VARS`]. The
/// secondary colour and `border_*` are seeded for preset compatibility even
/// though this subset only draws the primary-colour outline.
const SHAPE_FRAME_VARS: &[(&str, f32)] = &[
    ("r2", 1.0),
    ("g2", 1.0),
    ("b2", 1.0),
    ("a2", 1.0),
    ("border_r", 0.0),
    ("border_g", 0.0),
    ("border_b", 0.0),
    ("border_a", 0.0),
    ("sides", 4.0),
    ("instance", 0.0),
    ("num_inst", 1.0),
    ("tex_zoom", 1.0),
    ("tex_ang", 0.0),
    ("cx", 0.0),
    ("cy", 0.0),
    ("rot", 0.0),
    ("radius", 0.5),
];

/// Seed the runtime-owned variables a wave frame equation can read before it
/// runs. These mirror MilkDrop's per-wave-frame variables; any value the
/// preset assigns afterwards wins.
fn seed_wave_frame_vars(pool: &mut VarPool, values: &MilkWaveVals, dots: bool) {
    seed_vars(
        pool,
        &[
            ("samples", values.samples as f32),
            ("sep", values.sep as f32),
            ("scaling", values.scaling as f32),
            ("smoothing", values.smoothing as f32),
            ("spectrum", if values.spectrum { 1.0 } else { 0.0 }),
            ("usedots", if dots { 1.0 } else { 0.0 }),
            ("thick", if values.draw_thick { 1.0 } else { 0.0 }),
            ("additive", if values.additive { 1.0 } else { 0.0 }),
            ("r", values.color[0] as f32),
            ("g", values.color[1] as f32),
            ("b", values.color[2] as f32),
            ("a", values.color[3] as f32),
        ],
    );
}

/// Seed the runtime-owned variables a shape frame equation can read.
fn seed_shape_frame_vars(pool: &mut VarPool) {
    seed_vars(pool, OVERLAY_COLOR_VARS);
    seed_vars(pool, SHAPE_FRAME_VARS);
}

/// Run a section's init equations once, capturing the `q1..q32` baseline and
/// marking the state initialised. Shared by wave and shape sections.
fn ensure_overlay_init(
    init: &CompiledEquations,
    state: &mut OverlayState,
    globals: &Globals,
) -> Result<(), String> {
    if state.init_done {
        return Ok(());
    }
    run_frame(init, &mut state.pool, globals).map_err(|e| format!("init: {e}"))?;
    state.pool.snapshot_q();
    state.init_done = true;
    Ok(())
}

/// Run one wave section: init once, seed + run frame equations, then evaluate
/// the point equations for each sample of the smoothed audio/spectrum source.
/// Any EEL failure is returned as a message so the caller can isolate this
/// section.
fn evaluate_one_wave(
    program: &WaveProgram,
    state: &mut OverlayState,
    globals: &Globals,
    frame_pool: &VarPool,
    audio_wave: &[f32],
    byte_frequency_data: &[u8],
) -> Result<WaveOverlay, String> {
    ensure_overlay_init(&program.init, state, globals)?;

    // MilkDrop shares the live q values between the frame and overlay
    // equations. Seed this section's q baseline from the frame pool so the
    // wave's own frame equations (and, through them, its point equations) see
    // `q1..q32` written by the preset's top-level `per_frame` equations.
    state.pool.copy_q_from(frame_pool);
    state.pool.snapshot_q();

    seed_wave_frame_vars(&mut state.pool, &program.values, program.dots);
    run_frame(&program.frame, &mut state.pool, globals).map_err(|e| format!("frame: {e}"))?;

    // MilkDrop evaluates frame and point equations against the same pool, so
    // user variables written per point (`mx`, `ma`, ...) persist into the next
    // frame. Several community presets random-walk those variables to build
    // their shapes; forking the pool here would freeze every wave at frame 1.
    let source_max = audio_wave.len().min(MAX_WAVE_SAMPLES);
    if source_max == 0 {
        return Ok(WaveOverlay {
            pool: state.pool.clone(),
            points: Vec::new(),
            enabled: true,
            dots: program.dots,
        });
    }
    let audio = &audio_wave[audio_wave.len() - source_max..];
    let spectrum = state.pool.get("spectrum") != 0.0;
    let mut authored_samples = state.pool.get("samples").floor().max(0.0) as usize;
    authored_samples = authored_samples.min(source_max);
    let sep = state.pool.get("sep").floor().max(0.0) as usize;
    authored_samples = authored_samples.saturating_sub(sep);
    if !(authored_samples >= 2 || (program.dots && authored_samples >= 1)) {
        return Ok(WaveOverlay {
            pool: state.pool.clone(),
            points: Vec::new(),
            enabled: true,
            dots: program.dots,
        });
    }

    let scaling = state.pool.get("scaling");
    let smoothing = state.pool.get("smoothing").clamp(0.0, 0.98);
    let wave_scale = frame_pool.get("wave_scale");
    let scale = (if spectrum { 0.15 } else { 0.004 * 128.0 }) * scaling * wave_scale;

    let spectrum_values: Vec<f32> = byte_frequency_data
        .iter()
        .take(source_max)
        .map(|byte| *byte as f32 / 255.0)
        .collect();
    let (left_source, right_source): (&[f32], &[f32]) = if spectrum {
        (&spectrum_values, &spectrum_values)
    } else {
        (audio, audio)
    };
    let (j0, j1, t) = if spectrum {
        (
            0usize,
            0usize,
            (source_max.saturating_sub(sep)).max(1) as f32 / authored_samples as f32,
        )
    } else {
        (
            ((source_max as f32 - authored_samples as f32) / 2.0 - sep as f32 / 2.0)
                .floor()
                .max(0.0) as usize,
            ((source_max as f32 - authored_samples as f32) / 2.0 + sep as f32 / 2.0)
                .floor()
                .max(0.0) as usize,
            1.0,
        )
    };

    let mix1 = (smoothing * 0.98).max(0.0).powf(0.5);
    let mix2 = 1.0 - mix1;
    let mut points_l = vec![0.0; authored_samples];
    let mut points_r = vec![0.0; authored_samples];
    points_l[0] = left_source.get(j0).copied().unwrap_or(0.0);
    points_r[0] = right_source.get(j1).copied().unwrap_or(0.0);
    for j in 1..authored_samples {
        let il = ((j as f32 * t) as usize + j0).min(source_max - 1);
        let ir = ((j as f32 * t) as usize + j1).min(source_max - 1);
        points_l[j] = left_source[il] * mix2 + points_l[j - 1] * mix1;
        points_r[j] = right_source[ir] * mix2 + points_r[j - 1] * mix1;
    }
    for j in (0..authored_samples.saturating_sub(1)).rev() {
        points_l[j] = points_l[j] * mix2 + points_l[j + 1] * mix1;
        points_r[j] = points_r[j] * mix2 + points_r[j + 1] * mix1;
    }
    for j in 0..authored_samples {
        points_l[j] *= scale;
        points_r[j] *= scale;
    }

    // Butterchurn/MilkDrop custom-wave coordinates span the full output: the
    // final image places `y=0` at the bottom and `y=1` at the top. Do not
    // apply the warp aspect ratio here; doing so pushes most of the [0,1]
    // range off-screen on a 16:9 frame.
    let frame_r = state.pool.get("r");
    let frame_g = state.pool.get("g");
    let frame_b = state.pool.get("b");
    let frame_a = state.pool.get("a");

    let mut points = Vec::with_capacity(authored_samples);
    for index in 0..authored_samples {
        let value1 = points_l[index];
        let value2 = points_r[index];
        let sample = if authored_samples > 1 {
            index as f32 / (authored_samples - 1) as f32
        } else {
            0.0
        };
        state.pool.set("sample", sample);
        state.pool.set("value1", value1);
        state.pool.set("value2", value2);
        state.pool.set("x", 0.5 + value1);
        state.pool.set("y", 0.5 + value2);
        state.pool.set("r", frame_r);
        state.pool.set("g", frame_g);
        state.pool.set("b", frame_b);
        state.pool.set("a", frame_a);
        run_pixel(
            &program.point,
            &mut state.pool,
            0.5 + value1,
            0.5 + value2,
            0.0,
            0.0,
        )
        .map_err(|e| format!("point {index}: {e}"))?;
        points.push(WavePoint {
            x: state.pool.get("x") * 2.0 - 1.0,
            y: state.pool.get("y") * 2.0 - 1.0,
            sample,
            value1: state.pool.get("value1"),
            value2: state.pool.get("value2"),
            color: overlay_color(&state.pool),
        });
    }

    Ok(WaveOverlay {
        pool: state.pool.clone(),
        points,
        enabled: true,
        dots: program.dots,
    })
}

/// Run one shape section: init once, then seed + run its frame equations.
fn evaluate_one_shape(
    program: &ShapeProgram,
    state: &mut OverlayState,
    globals: &Globals,
) -> Result<ShapeOverlay, String> {
    ensure_overlay_init(&program.init, state, globals)?;

    seed_shape_frame_vars(&mut state.pool);
    run_frame(&program.frame, &mut state.pool, globals).map_err(|e| format!("frame: {e}"))?;

    Ok(ShapeOverlay {
        pool: state.pool.clone(),
        enabled: true,
    })
}

/// Seed every base-valued per-frame variable from the preset before the
/// per-frame equations run.
///
/// MilkDrop assigns the base vals to their frame variables at the start of each
/// frame; equations then read and overwrite them. That matters for
/// self-referential code (`wave_r = wave_r + 0.1;`) and for presets that write
/// `gamma`, `decay` or `echo_alpha` from EEL: without the seed those variables
/// start at zero and the shader uniforms see the wrong value. The variable
/// names here are the frame-level names, which differ from the base-val keys
/// (`fGammaAdj` → `gamma`, `nWaveMode` → `wave_mode`, ...).
fn seed_frame_base_vals(pool: &mut VarPool, base: &MilkBaseVals) {
    let bool_val = |value: bool| if value { 1.0 } else { 0.0 };
    seed_vars(
        pool,
        &[
            ("decay", base.decay as f32),
            ("gamma", base.gammaadj as f32),
            ("gammaadj", base.gammaadj as f32),
            ("fshader", base.shader as f32),
            ("zoom", base.zoom as f32),
            ("zoomexp", base.zoomexp as f32),
            ("warp", base.warp as f32),
            ("rot", base.rot as f32),
            ("cx", base.cx as f32),
            ("cy", base.cy as f32),
            ("dx", base.dx as f32),
            ("dy", base.dy as f32),
            ("sx", base.sx as f32),
            ("sy", base.sy as f32),
            ("echo_zoom", base.echo_zoom as f32),
            ("echo_alpha", base.echo_alpha as f32),
            ("echo_orient", base.echo_orient as f32),
            ("wave_mode", base.wave_mode as f32),
            ("old_wave_mode", base.wave_mode as f32),
            ("wave_r", base.wave_r as f32),
            ("wave_g", base.wave_g as f32),
            ("wave_b", base.wave_b as f32),
            ("wave_a", base.wave_a as f32),
            ("wave_x", base.wave_x as f32),
            ("wave_y", base.wave_y as f32),
            ("wave_scale", base.wave_scale as f32),
            ("wave_smoothing", base.wave_smoothing as f32),
            ("wave_mystery", base.wave_mystery as f32),
            ("modwavealphastart", base.modwavealphastart as f32),
            ("modwavealphaend", base.modwavealphaend as f32),
            ("additivewave", bool_val(base.additivewave)),
            ("wave_dots", bool_val(base.wave_dots)),
            ("wave_thick", bool_val(base.wave_thick)),
            ("wave_brighten", bool_val(base.wave_brighten)),
            ("wrap", bool_val(base.wrap)),
            ("darken_center", bool_val(base.darken_center)),
            ("red_blue", bool_val(base.red_blue)),
            ("brighten", bool_val(base.brighten)),
            ("darken", bool_val(base.darken)),
            ("solarize", bool_val(base.solarize)),
            ("invert", bool_val(base.invert)),
            ("warpanimspeed", base.warpanimspeed as f32),
            ("warpscale", base.warpscale as f32),
            ("mv_x", base.mv_x as f32),
            ("mv_y", base.mv_y as f32),
            ("mv_a", base.mv_a as f32),
            ("mv_r", base.mv_r as f32),
            ("mv_g", base.mv_g as f32),
            ("mv_b", base.mv_b as f32),
            ("mv_len", base.mv_len as f32),
            ("ob_size", base.ob_size as f32),
            ("ob_a", base.ob_a as f32),
            ("ib_size", base.ib_size as f32),
            ("ib_a", base.ib_a as f32),
            ("b1n", base.b1n as f32),
            ("b2n", base.b2n as f32),
            ("b3n", base.b3n as f32),
            ("brightness", base.brightness as f32),
        ],
    );
}

/// Advance `instance` to `frame`: run init once, build globals/audio, update
/// the band followers and run the per-frame equations.
pub fn begin_frame(
    instance: &mut MilkInstance,
    frame: &MilkFrameInfo,
) -> Result<(), MilkRuntimeError> {
    // (1) Init equations run exactly once and capture the q1..q32 baseline.
    // `compiled`, `pool` and `globals` are disjoint fields, so they can be
    // borrowed independently without cloning the compiled equations.
    if !instance.init_done {
        run_frame(
            &instance.compiled.init,
            &mut instance.pool,
            &Globals::default(),
        )
        .map_err(|e| MilkRuntimeError::Evaluate(format!("init equations: {e}")))?;
        instance.pool.snapshot_q();
        instance.init_done = true;
    }

    // (2) Rebuild the fifteen globals from this frame's geometry and audio.
    // Butterchurn's `bass`/`mid`/`treb` are FFT band sums divided by slow
    // per-band averages, so a steady passage settles near 1.0 and transients
    // rise above it. Derive the sums from the byte spectrum (the same source
    // Butterchurn's `AudioLevels` uses) rather than the engine's dB-mapped
    // features, which suppressed mid/treble leakage and changed preset walks.
    let audio = &frame.audio;
    let sample_rate = if audio.sample_rate.is_finite() && audio.sample_rate > 0.0 {
        audio.sample_rate
    } else {
        48_000.0
    };
    let bins = audio.byte_frequency_data.len().max(1);
    let bucket_hz = sample_rate / (bins as f32 * 2.0);
    let bin_index =
        |hz: f32| (((hz / bucket_hz).round() as isize) - 1).clamp(0, bins as isize - 1) as usize;
    let starts = [bin_index(20.0), bin_index(320.0), bin_index(2800.0)];
    let stops = [bin_index(320.0), bin_index(2800.0), bin_index(11025.0)];
    let mut imm = [0.0f32; 3];
    if audio.byte_frequency_data.iter().any(|byte| *byte != 0) {
        for (band, (start, stop)) in starts.iter().zip(stops).enumerate() {
            let end = stop.max(start + 1).min(bins);
            let start = (*start).min(end);
            imm[band] = audio.byte_frequency_data[start..end]
                .iter()
                .map(|byte| *byte as f32 / 255.0)
                .sum();
        }
    } else {
        // Fallback for callers that provide only the engine's band features.
        imm = [
            audio.bass.max(0.0) * 8.0,
            audio.mid.max(0.0) * 32.0,
            audio.treble.max(0.0) * 64.0,
        ];
    }
    let first_frame = instance.frame_index == 0;
    let avg_rate = 0.2f32.powf(30.0 / frame.fps.max(1.0));
    let long_rate = 0.992f32.powf(30.0 / frame.fps.max(1.0));
    for ((avg, long), raw) in instance
        .band_att
        .iter_mut()
        .zip(instance.band_long_att.iter_mut())
        .zip(imm)
    {
        if first_frame {
            *avg = raw;
            *long = raw.max(1e-3);
        } else {
            *avg += (raw - *avg) * avg_rate;
            *long += (raw - *long) * (1.0 - long_rate);
        }
    }
    instance.band_imm = imm;
    let relative = |value: f32, long: f32| {
        if long < 0.001 {
            1.0
        } else {
            (value / long).clamp(0.0, 16.0)
        }
    };
    let values = [
        relative(imm[0], instance.band_long_att[0]),
        relative(imm[1], instance.band_long_att[1]),
        relative(imm[2], instance.band_long_att[2]),
    ];
    let relative_att = [
        relative(instance.band_att[0], instance.band_long_att[0]),
        relative(instance.band_att[1], instance.band_long_att[1]),
        relative(instance.band_att[2], instance.band_long_att[2]),
    ];

    let (aspectx, aspecty) = if frame.width >= frame.height {
        (1.0, frame.height as f32 / frame.width as f32)
    } else {
        (frame.width as f32 / frame.height as f32, 1.0)
    };

    instance.globals = Globals {
        time: frame.time_seconds as f32,
        frame: frame.frame_index as f32,
        fps: frame.fps,
        meshx: frame.meshx as f32,
        meshy: frame.meshy as f32,
        pixelsx: frame.width as f32,
        pixelsy: frame.height as f32,
        aspectx,
        aspecty,
        bass: values[0],
        bass_att: relative_att[0],
        mid: values[1],
        mid_att: relative_att[1],
        treb: values[2],
        treb_att: relative_att[2],
    };
    instance.frame_index = frame.frame_index;
    instance.wave = frame.audio.wave.clone();
    instance.byte_frequency_data = frame.audio.byte_frequency_data.clone();

    // Seed the frame-level variables from the base vals before the per-frame
    // equations run (see `seed_frame_base_vals` for why this ordering matters).
    seed_frame_base_vals(&mut instance.pool, &instance.compiled.doc.base);

    // (3) Per-frame equations: `run_frame` restores the captured q baseline
    // before injecting the globals, so q changes never leak into the next frame.
    run_frame(
        &instance.compiled.frame,
        &mut instance.pool,
        &instance.globals,
    )
    .map_err(|e| MilkRuntimeError::Evaluate(format!("frame equations: {e}")))?;

    // Inspector input props win over the preset's own animation. They are
    // applied after the equations (but before mesh/overlay/uniform evaluation)
    // so a control can override `decay`, `zoom`, echo, etc. without fighting
    // the authored EEL.
    for &(name, value) in &instance.overrides {
        if value.is_finite() {
            instance.pool.set(name, value);
        }
    }
    Ok(())
}

/// The warped mesh produced for one frame.
#[derive(Clone, Debug, Default)]
pub struct MilkMesh {
    /// One `[u, v]` per vertex, row-major over `width`×`height`.
    pub uvs: Vec<[f32; 2]>,
    /// `meshx + 1`.
    pub width: u32,
    /// `meshy + 1`.
    pub height: u32,
}

/// Resolve one warp parameter from the live EEL pool, falling back to the
/// preset's base-val default only when the pool value is non-finite.
///
/// [`begin_frame`] seeds every frame-level variable from the base vals before
/// the equations run and then applies Inspector overrides, so `0.0` is a real
/// authored/UI value (e.g. disabling `warp` or `rot`), not "unset".
fn warp_param(pool: &VarPool, name: &str, base: f64) -> f32 {
    let live = pool.get(name);
    if live.is_finite() { live } else { base as f32 }
}

/// `value` when it is finite and non-degenerate, otherwise `fallback`.
fn nonzero_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value.abs() > WARP_EPS {
        value
    } else {
        fallback
    }
}

/// `value` when finite, otherwise `fallback` (the texture centre for UVs).
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

/// Map a mesh vertex's `[-1, 1]` position to a warped sampling UV.
///
/// This mirrors Butterchurn/MilkDrop's `default_warp` (`renderer.js`): the
/// aspect-corrected radial zoom exponent, per-axis stretch, animated warp
/// displacement, rotation about the screen centre, and the `dx/dy` offset.
/// All parameters come from the live EEL pool (per-frame overrides) with the
/// preset's base vals as defaults. Degenerate values are guarded so a fixture
/// still yields finite UVs.
fn warped_uv(
    pool: &VarPool,
    base: &MilkBaseVals,
    x: f32,
    y: f32,
    time: f32,
    aspect_x: f32,
    aspect_y: f32,
) -> [f32; 2] {
    let mut zoom = nonzero_or(warp_param(pool, "zoom", base.zoom), 1.0);
    let zoomexp = warp_param(pool, "zoomexp", base.zoomexp);
    let rot = warp_param(pool, "rot", base.rot);
    let warp = warp_param(pool, "warp", base.warp);
    let cx = warp_param(pool, "cx", base.cx);
    let cy = warp_param(pool, "cy", base.cy);
    let dx = warp_param(pool, "dx", base.dx);
    let dy = warp_param(pool, "dy", base.dy);
    let sx = nonzero_or(warp_param(pool, "sx", base.sx), 1.0);
    let sy = nonzero_or(warp_param(pool, "sy", base.sy), 1.0);
    let warpscale = nonzero_or(warp_param(pool, "warpscale", base.warpscale), 1.0);
    let warpanimspeed = warp_param(pool, "warpanimspeed", base.warpanimspeed);

    if zoom.abs() < WARP_EPS {
        zoom = WARP_EPS;
    }
    let rad = (x * x * aspect_x * aspect_x + y * y * aspect_y * aspect_y)
        .max(0.0)
        .sqrt();
    let zoom_exp = zoomexp.powf(rad * 2.0 - 1.0);
    let zoom_value = zoom.powf(zoom_exp);
    let zoom_inv = if zoom_value.abs() < WARP_EPS {
        1.0 / WARP_EPS
    } else {
        1.0 / zoom_value
    };
    let mut u = x * 0.5 * aspect_x * zoom_inv + 0.5;
    let mut v = -y * 0.5 * aspect_y * zoom_inv + 0.5;
    u = (u - cx) / sx + cx;
    v = (v - cy) / sy + cy;

    if warp.abs() > 1e-9 {
        let warp_time = time * warpanimspeed;
        let warp_scale_inv = 1.0 / warpscale;
        let warp_f0 = 11.68 + 4.0 * (warp_time * 1.413 + 10.0).cos();
        let warp_f1 = 8.77 + 3.0 * (warp_time * 1.113 + 7.0).cos();
        let warp_f2 = 10.54 + 3.0 * (warp_time * 1.233 + 3.0).cos();
        let warp_f3 = 11.49 + 4.0 * (warp_time * 0.933 + 5.0).cos();
        u += warp
            * 0.0035
            * (warp_time * 0.333 + warp_scale_inv * (x * warp_f0 - y * warp_f3)).sin();
        v += warp
            * 0.0035
            * (warp_time * 0.375 - warp_scale_inv * (x * warp_f2 + y * warp_f1)).cos();
        u += warp
            * 0.0035
            * (warp_time * 0.753 - warp_scale_inv * (x * warp_f1 - y * warp_f2)).cos();
        v += warp
            * 0.0035
            * (warp_time * 0.825 + warp_scale_inv * (x * warp_f0 + y * warp_f3)).sin();
    }

    let u2 = u - cx;
    let v2 = v - cy;
    let (sin_rot, cos_rot) = rot.sin_cos();
    u = u2 * cos_rot - v2 * sin_rot + cx - dx;
    v = u2 * sin_rot + v2 * cos_rot + cy - dy;
    // Undo the aspect scaling so the full-screen mesh covers the whole texture.
    u = (u - 0.5) / aspect_x.max(1e-6) + 0.5;
    v = (v - 0.5) / aspect_y.max(1e-6) + 0.5;
    [finite_or(u, UV_FALLBACK), finite_or(v, UV_FALLBACK)]
}

/// Evaluate the per-pixel equations and warped UV grid for `frame`.
///
/// The caller must have called [`begin_frame`] for the same frame first.
pub fn evaluate_mesh(
    instance: &mut MilkInstance,
    frame: &MilkFrameInfo,
) -> Result<MilkMesh, MilkRuntimeError> {
    let width = frame.meshx + 1;
    let height = frame.meshy + 1;
    let time = instance.globals.time;

    let mut uvs = Vec::with_capacity((width as usize).saturating_mul(height as usize));
    for row in 0..height {
        let y = if height > 1 {
            1.0 - 2.0 * (row as f32 / (height - 1) as f32)
        } else {
            0.0
        };
        for col in 0..width {
            let x = if width > 1 {
                2.0 * (col as f32 / (width - 1) as f32) - 1.0
            } else {
                0.0
            };
            let rad = x.hypot(y);
            let ang = y.atan2(x);
            // `compiled` and `pool` are disjoint fields: borrow the equations
            // instead of cloning them per vertex.
            run_pixel(&instance.compiled.pixel, &mut instance.pool, x, y, rad, ang)
                .map_err(|e| MilkRuntimeError::Evaluate(format!("pixel equations: {e}")))?;
            uvs.push(warped_uv(
                &instance.pool,
                &instance.compiled.doc.base,
                x,
                y,
                time,
                instance.globals.aspectx,
                instance.globals.aspecty,
            ));
        }
    }

    Ok(MilkMesh { uvs, width, height })
}

// --- Mesh UV upload ---------------------------------------------------------
//
// The CPU-computed warped UV grid is uploaded once per frame as a vertex
// buffer, alongside the grid's fixed NDC positions, plus an index buffer
// covering every cell with two triangles. The warp pass draws that indexed
// mesh so the translated warp shader samples `sampler_main` at each vertex's
// warped UV instead of the full-screen `uv_orig` varying.

/// GPU-side storage for a [`MilkMesh`]'s warped UV grid and its triangle
/// indices, consumed by the warp pass.
#[derive(Clone, Debug, Default)]
pub struct MilkMeshGpu {
    /// Number of UV columns (`meshx + 1`).
    pub width: u32,
    /// Number of UV rows (`meshy + 1`).
    pub height: u32,
    /// Number of vertices in the UV buffer (`width * height`).
    pub vertex_count: usize,
    /// Number of indices in the triangle buffer (`meshx * meshy * 6`).
    pub index_count: usize,
    /// Interleaved `[grid x, grid y, warped u, warped v]` per vertex.
    vertex_buffer: Option<wgpu::Buffer>,
    /// Uint32 triangle-list indices for the grid.
    index_buffer: Option<wgpu::Buffer>,
}

impl MilkMeshGpu {
    /// Upload `mesh`'s warped UV grid and triangle indices to `device`.
    ///
    /// Called once per frame: the UVs are time-varying (zoom/rot/warp and the
    /// per-frame variables feeding [`warped_uv`]), so caching the vertex buffer
    /// across frames would reuse stale samples. The index buffer is rebuilt
    /// alongside it for simplicity — its contents depend only on the grid
    /// dimensions and are small compared with the vertices.
    pub fn new(device: &wgpu::Device, mesh: &MilkMesh) -> Self {
        use wgpu::util::DeviceExt;

        let width = mesh.width;
        let height = mesh.height;
        let vertex_count = (width as usize).saturating_mul(height as usize);
        let indices = mesh_triangle_indices(width, height);
        let index_count = indices.len();

        // One vertex is two f32 for the fixed NDC grid position and two f32
        // for the warped sampling UV (see `MESH_VERTEX_WGSL`).
        let mut vertices = Vec::with_capacity(vertex_count * 4);
        for row in 0..height {
            let y = if height > 1 {
                1.0 - 2.0 * (row as f32 / (height - 1) as f32)
            } else {
                0.0
            };
            for col in 0..width {
                let x = if width > 1 {
                    2.0 * (col as f32 / (width - 1) as f32) - 1.0
                } else {
                    0.0
                };
                // Fall back to the old full-screen UV when the CPU mesh is
                // shorter than the declared grid (defensive; `evaluate_mesh`
                // always fills it exactly).
                let uv = mesh
                    .uvs
                    .get(row as usize * width as usize + col as usize)
                    .copied()
                    .unwrap_or([x * 0.5 + 0.5, y * 0.5 + 0.5]);
                vertices.extend_from_slice(&[x, y, uv[0], uv[1]]);
            }
        }
        let vertex_bytes: Vec<u8> = vertices.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let index_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_ne_bytes()).collect();

        let vertex_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("modV MilkDrop mesh vertices"),
                contents: &vertex_bytes,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }),
        );
        let index_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("modV MilkDrop mesh indices"),
                contents: &index_bytes,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            }),
        );

        Self {
            width,
            height,
            vertex_count,
            index_count,
            vertex_buffer,
            index_buffer,
        }
    }
}

/// Build the triangle-list indices covering a `width`×`height` UV grid.
///
/// One cell per quad, traversed row-major, with two triangles per cell. Cell
/// `(row, col)` has corners `a=base` (top-left), `b=base+1` (top-right),
/// `c=base+width` (bottom-left), `d=base+width+1` (bottom-right) where
/// `base = row * width + col`; the two triangles share the `a–d` diagonal.
pub(crate) fn mesh_triangle_indices(width: u32, height: u32) -> Vec<u32> {
    if width < 2 || height < 2 {
        return Vec::new();
    }
    let cells = (width - 1) as usize * (height - 1) as usize;
    let mut indices = Vec::with_capacity(cells * 6);
    for row in 0..height - 1 {
        for col in 0..width - 1 {
            let base = row * width + col;
            let a = base;
            let b = base + 1;
            let c = base + width;
            let d = base + width + 1;
            // Two triangles covering the whole quad: (a, b, d) and (a, d, c).
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    indices
}

/// Vertex layout for the interleaved `[grid x, grid y, warped u, warped v]`
/// mesh buffer consumed by [`MESH_VERTEX_WGSL`].
///
/// Four `f32`s per vertex give a 16-byte stride: the NDC grid position occupies
/// bytes `0..8` (`shader_location(0)`) and the warped sampling UV bytes `8..16`
/// (`shader_location(1)`). The layout is `&'static` because the attribute slice
/// is a compile-time constant, so it can be borrowed by every mesh pipeline.
fn mesh_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 16,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 8,
                shader_location: 1,
            },
        ],
    }
}

// --- GPU helpers -----------------------------------------------------------

/// Use the EEL value when the pool has assigned it, otherwise derive a stable
/// per-frame stand-in so rand-driven output remains deterministic.
fn effective_rand(value: f32, salt: f32, frame_index: u32) -> f32 {
    if value == 0.0 {
        ((frame_index as f32 + 1.0) * 0.618_034 + salt).fract()
    } else {
        value
    }
}

fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
}

fn write_vec2(bytes: &mut [u8], offset: usize, value: [f32; 2]) {
    write_f32(bytes, offset, value[0]);
    write_f32(bytes, offset + 4, value[1]);
}

/// An off-screen MilkDrop target: `Rgba8Unorm`, renderable, sampleable and
/// readable back for tests/previews.
fn create_target(device: &wgpu::Device, width: u32, height: u32, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Upload one deterministic RGBA8 noise field and return its sampleable view.
///
/// The texture handle is dropped here: wgpu ref-counts GPU resources, so the
/// returned `TextureView` keeps the parent texture alive for as long as the
/// view (and any bind groups created from it) exist.
fn create_noise_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bytes: &[u8],
    label: &str,
) -> wgpu::TextureView {
    let size = MilkNoise::SIZE;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size * 4),
            rows_per_image: Some(size),
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Uniform-buffer entry at binding 0, shared by the warp/comp and output
/// passes. The whole 256-byte stride is bound so one buffer serves both.
fn uniform_bind_entry(buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer,
            offset: 0,
            size: wgpu::BufferSize::new(MILK_UNIFORM_STRIDE as u64),
        }),
    }
}

/// Uniform-buffer entry for the `MilkQUniforms` block.
fn q_uniform_bind_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer,
            offset: 0,
            size: wgpu::BufferSize::new(MILK_Q_STRIDE as u64),
        }),
    }
}

fn texture_bind(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn sampler_bind(binding: u32, sampler: &wgpu::Sampler) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::Sampler(sampler),
    }
}

/// Texture views bound to the built-in sampler slots of a warp/comp shader.
#[derive(Clone, Copy)]
struct MilkSamplerViews<'a> {
    main: &'a wgpu::TextureView,
    dummy: &'a wgpu::TextureView,
    blur: [&'a wgpu::TextureView; 3],
    noise: [&'a wgpu::TextureView; 4],
}

/// Bind the uniform block plus every built-in sampler pair of a translated
/// warp/comp shader, in `translate::BUILTIN_SAMPLERS` order. Binding 0 is the
/// uniform; `sampler_main` is `main_view`, blur samplers 1..3 are the blur
/// targets, the five noise samplers use the deterministic noise textures and
/// the remaining built-ins use the 1×1 dummy texture. The q block is bound at
/// [`MILK_Q_BINDING`], immediately after the sampler pairs.
fn milk_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    q_uniform: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    views: MilkSamplerViews<'_>,
) -> wgpu::BindGroup {
    let mut entries = Vec::with_capacity(2 + MILK_RUNTIME_SAMPLER_PAIRS * 2);
    entries.push(uniform_bind_entry(uniform));
    let textures: [&wgpu::TextureView; MILK_RUNTIME_SAMPLER_PAIRS] = [
        views.main,
        views.dummy,
        views.dummy,
        views.dummy,
        views.dummy,
        views.blur[0],
        views.blur[1],
        views.blur[2],
        views.noise[0],
        views.noise[1],
        views.noise[2],
        views.noise[3],
        views.noise[3],
    ];
    for (index, view) in textures.iter().enumerate() {
        let base = (index as u32) * 2 + 1;
        entries.push(texture_bind(base, view));
        entries.push(sampler_bind(base + 1, sampler));
    }
    entries.push(q_uniform_bind_entry(MILK_Q_BINDING, q_uniform));
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modV MilkDrop shader bind group"),
        layout,
        entries: &entries,
    })
}

fn small_uniform_bind(buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer,
            offset: 0,
            size: wgpu::BufferSize::new(SMALL_UNIFORM_BYTES),
        }),
    }
}

fn pass_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modV MilkDrop blur bind group"),
        layout,
        entries: &[
            small_uniform_bind(uniform),
            texture_bind(1, view),
            sampler_bind(2, sampler),
        ],
    })
}

fn output_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    view: &wgpu::TextureView,
    previous_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let uniform_entry = wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: uniform,
            offset: 0,
            size: wgpu::BufferSize::new(MILK_POST_UNIFORM_BYTES as u64),
        }),
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modV MilkDrop output bind group"),
        layout,
        entries: &[
            uniform_entry,
            texture_bind(1, view),
            sampler_bind(2, sampler),
            texture_bind(3, previous_view),
            sampler_bind(4, sampler),
        ],
    })
}

/// Begin a pass that clears a single colour target to transparent black.
///
/// Shared by the full-screen and mesh warp passes: both replace the target's
/// contents completely, unlike the overlay pass which loads what is already
/// there. The returned pass borrows `encoder` for its whole lifetime.
fn begin_clear_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
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
    })
}

fn encode_fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut pass = begin_clear_pass(encoder, label, view);
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Draw the uploaded mesh as an indexed triangle list into `view`.
///
/// The vertex buffer supplies the grid position and per-vertex warped UV (see
/// [`MESH_VERTEX_WGSL`] and [`mesh_vertex_layout`]); the index buffer from
/// [`mesh_triangle_indices`] covers each grid cell with two triangles. When no
/// buffers were uploaded (e.g. a default-constructed [`MilkMeshGpu`]) the pass
/// falls back to the full-screen triangle so the warp target stays covered.
fn encode_mesh_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    mesh: &MilkMeshGpu,
) {
    let mut pass = begin_clear_pass(encoder, label, view);
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    if let (Some(vertex_buffer), Some(index_buffer)) = (&mesh.vertex_buffer, &mesh.index_buffer) {
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count as u32, 0, 0..1);
    } else {
        pass.draw(0..3, 0..1);
    }
}

// --- Layer 5c overlay GPU helpers ------------------------------------------

/// `LineList` vertex layout: NDC `vec2` position followed by `vec4` RGBA.
fn overlay_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 24,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 8,
                shader_location: 1,
            },
        ],
    }
}

/// Pack a slice of floats into native-endian bytes without `bytemuck`/`unsafe`.
fn f32_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

/// Read a colour from a section's frame pool, clamped to `[0, 1]`.
fn overlay_color(pool: &VarPool) -> [f32; 4] {
    [
        pool.get("r").clamp(0.0, 1.0),
        pool.get("g").clamp(0.0, 1.0),
        pool.get("b").clamp(0.0, 1.0),
        pool.get("a").clamp(0.0, 1.0),
    ]
}

/// Build `LineList` vertices (x, y, r, g, b, a) for one wave: a polyline over a
/// strided subset of its evaluated sample points. Empty when fewer than two
/// points remain after striding.
fn wave_overlay_vertices(wave: &WaveOverlay) -> Vec<f32> {
    let mut out = Vec::new();
    let mut previous: Option<([f32; 2], [f32; 4])> = None;
    let mut emitted = 0usize;
    for point in wave.points.iter().step_by(WAVE_VERTEX_STRIDE) {
        let current = (
            [point.x.clamp(-1.0, 1.0), point.y.clamp(-1.0, 1.0)],
            point.color,
        );
        if let Some(([sx, sy], sc)) = previous {
            out.extend_from_slice(&[sx, sy, sc[0], sc[1], sc[2], sc[3]]);
            out.extend_from_slice(&[
                current.0[0],
                current.0[1],
                current.1[0],
                current.1[1],
                current.1[2],
                current.1[3],
            ]);
            emitted += 1;
        }
        previous = Some(current);
    }
    if emitted == 0 {
        out.clear();
    }
    out
}

/// Build `PointList` vertices for a `bUseDots` wave: one vertex per sample.
/// Dotted presets rely on this because their points can be closer than one
/// pixel, which makes the line-list path rasterise nothing at all.
fn wave_overlay_point_vertices(wave: &WaveOverlay) -> Vec<f32> {
    let mut out = Vec::with_capacity(wave.points.len() * 6);
    for point in &wave.points {
        let color = point.color;
        out.extend_from_slice(&[
            point.x.clamp(-1.0, 1.0),
            point.y.clamp(-1.0, 1.0),
            color[0],
            color[1],
            color[2],
            color[3],
        ]);
    }
    out
}

/// Build `LineList` vertices for one shape: a closed regular polygon whose
/// side count, centre, radius and rotation come from the frame pool. `r2/g2/b2`
/// are seeded but the outline uses the primary colour in this subset.
fn shape_overlay_vertices(shape: &ShapeOverlay) -> Vec<f32> {
    let sides = shape.pool.get("sides").round().clamp(3.0, 64.0) as usize;
    let color = overlay_color(&shape.pool);
    let cx = shape.pool.get("cx");
    let cy = shape.pool.get("cy");
    let radius = shape.pool.get("radius");
    let rot = shape.pool.get("rot");
    let ring: Vec<[f32; 2]> = (0..sides)
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / sides as f32 + rot;
            [cx + radius * angle.cos(), cy + radius * angle.sin()]
        })
        .collect();
    let mut out = Vec::with_capacity(sides * 12);
    for index in 0..sides {
        let a = ring[index];
        let b = ring[(index + 1) % sides];
        out.extend_from_slice(&[a[0], a[1], color[0], color[1], color[2], color[3]]);
        out.extend_from_slice(&[b[0], b[1], color[0], color[1], color[2], color[3]]);
    }
    out
}

/// Encode one render pass that draws all evaluated overlays onto `comp_view`.
/// Each overlay gets a small vertex buffer uploaded from the CPU results; the
/// pipeline (alpha vs additive) is chosen from its `additive` variable.
#[allow(clippy::too_many_arguments)]
fn encode_overlay_passes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    comp_view: &wgpu::TextureView,
    overlays: &OverlayFrame,
    alpha_pipeline: &wgpu::RenderPipeline,
    additive_pipeline: &wgpu::RenderPipeline,
    alpha_dots: &wgpu::RenderPipeline,
    additive_dots: &wgpu::RenderPipeline,
) {
    let mut draws: Vec<(Vec<f32>, bool, bool)> = Vec::new();
    for wave in &overlays.waves {
        if !wave.enabled {
            continue;
        }
        let vertices = if wave.dots {
            wave_overlay_point_vertices(wave)
        } else {
            wave_overlay_vertices(wave)
        };
        if !vertices.is_empty() {
            draws.push((vertices, wave.pool.get("additive") > 0.5, wave.dots));
        }
    }
    for shape in &overlays.shapes {
        if !shape.enabled {
            continue;
        }
        let vertices = shape_overlay_vertices(shape);
        if !vertices.is_empty() {
            draws.push((vertices, false, false));
        }
    }
    if draws.is_empty() {
        return;
    }

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("modV MilkDrop overlays"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: comp_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    for (vertices, additive, dots) in draws {
        let bytes = f32_bytes(&vertices);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modV MilkDrop overlay vertices"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, &bytes);
        pass.set_pipeline(match (additive, dots) {
            (false, false) => alpha_pipeline,
            (true, false) => additive_pipeline,
            (false, true) => alpha_dots,
            (true, true) => additive_dots,
        });
        pass.set_vertex_buffer(0, buffer.slice(..));
        let vertex_count = (vertices.len() / 6) as u32;
        pass.draw(0..vertex_count, 0..1);
    }
}

// ---------------------------------------------------------------------------
// RED tests — pure CPU, no GPU device required.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! approx {
        ($got:expr, $want:expr) => {
            approx!($got, $want, "")
        };
        ($got:expr, $want:expr, $msg:expr) => {{
            let got = $got;
            let want = $want;
            assert!(
                (got - want).abs() < 1e-4,
                "expected {want}, got {got} (delta {}) -- {}",
                (got - want).abs(),
                $msg
            );
        }};
    }

    const VALID_WARP: &str = "shader_body {\n\
float2 uv = uv_orig + float2(roam_cos, roam_sin) * 0.01;\n\
float3 ret = tex2D(sampler_main, uv).rgb;\n\
}\n";

    const VALID_COMP: &str = "shader_body {\n\
float3 ret = GetMain(uv_orig).rgb;\n\
}\n";

    const MALFORMED_WARP: &str = "shader_body {\n\
float2 uv = ;\n\
this is not valid GLSL @@@\n\
ret = uv;\n\
}\n";

    fn doc_with(init: &str, frame: &str, pixel: &str, warp: &str, comp: &str) -> MilkDocument {
        let mut doc = MilkDocument {
            version: 2,
            name: "fixture".to_string(),
            init_eqs: init.to_string(),
            frame_eqs: frame.to_string(),
            pixel_eqs: pixel.to_string(),
            warp: warp.to_string(),
            comp: comp.to_string(),
            ..Default::default()
        };
        // Most fixtures pin a custom wave; silence the built-in waveform so
        // its extra overlay cannot mask the section under test.
        doc.base.wave_a = 0.0;
        doc
    }

    fn frame(bass: f32, mid: f32, treble: f32, index: u32, time: f64) -> MilkFrameInfo {
        MilkFrameInfo {
            time_seconds: time,
            frame_index: index,
            fps: 60.0,
            width: 640,
            height: 360,
            meshx: 8,
            meshy: 6,
            audio: MilkAudio {
                bass,
                mid,
                treble,
                sample_rate: 48_000.0,
                wave: vec![0.0; 1024],
                byte_frequency_data: vec![128; 1024],
            },
        }
    }

    #[test]
    fn inspector_overrides_win_over_frame_equations() {
        let doc = doc_with(
            "",
            "decay = 0.25; gamma = 1.5; wave_a = 0.1;",
            "",
            VALID_WARP,
            VALID_COMP,
        );
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("overrides", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 1);
        instance.overrides = vec![("decay", 0.9), ("gamma", 2.0), ("wave_a", 0.4)];
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        approx!(instance.pool.get("decay"), 0.9, "decay override wins");
        approx!(instance.pool.get("gamma"), 2.0, "gamma override wins");
        approx!(instance.pool.get("wave_a"), 0.4, "wave alpha override wins");
    }

    #[test]
    fn uniform_payload_serializes_the_live_decay() {
        let doc = doc_with("", "decay = 0.37;", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("live-decay", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 1);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");
        let bytes = MilkUniforms::from_frame(&instance, &f).to_bytes();
        // `decay` is the fifth scalar in the translated uniform block.
        let decay = f32::from_ne_bytes(bytes[16..20].try_into().unwrap());
        approx!(decay, 0.37, "per-frame decay reaches the warp uniform");
    }

    #[test]
    fn full_preset_two_frames_q_reset_band_followers_and_determinism() {
        let doc = doc_with(
            "q1 = 3; q2 = 7;",
            "seen_q1 = q1; q1 = q1 + 1; j = rand(1000);",
            "px = x + y + rad + ang;",
            VALID_WARP,
            VALID_COMP,
        );
        let cache = MilkFailureCache::new();
        let compiled_a = compile_preset("fixture/a", &doc, &cache).expect("fixture compiles");
        let compiled_b = compile_preset("fixture/b", &doc, &cache).expect("fixture compiles");
        assert!(
            !cache.is_failed("fixture/a"),
            "valid preset must not be cached failed"
        );

        let mut a = new_instance(compiled_a, 0xFEED);
        let mut b = new_instance(compiled_b, 0xFEED);

        let f1 = frame(0.5, 0.25, 1.0, 1, 0.016);
        let f2 = frame(0.5, 0.25, 1.0, 2, 0.032);

        begin_frame(&mut a, &f1).expect("frame 1 on a");
        begin_frame(&mut a, &f2).expect("frame 2 on a");
        begin_frame(&mut b, &f1).expect("frame 1 on b");
        begin_frame(&mut b, &f2).expect("frame 2 on b");

        // (1) q reset: baseline from init is q1=3; each frame adds 1, so after
        // two frames q1 is 4 (NOT 5), and the frame equation saw baseline 3.
        approx!(
            a.pool.get("q1"),
            4.0,
            "q1 restored to init baseline each frame"
        );
        approx!(
            a.pool.get("seen_q1"),
            3.0,
            "per-frame equations see baseline q1"
        );

        // (2) band followers: relative levels sit at 1.0 for a steady input.
        approx!(a.globals.bass_att, 1.0, "bass_att is relative");
        approx!(a.globals.mid_att, 1.0, "mid_att is relative");
        approx!(a.globals.treb_att, 1.0, "treb_att is relative");

        // (3) determinism: two fresh instances with the same seed agree.
        assert_eq!(a.globals, b.globals, "globals identical for equal seeds");
        approx!(a.pool.get("q1"), b.pool.get("q1"), "q1 identical");
        approx!(a.pool.get("j"), b.pool.get("j"), "seeded rand identical");
        let ma = evaluate_mesh(&mut a, &f2).expect("mesh a");
        let mb = evaluate_mesh(&mut b, &f2).expect("mesh b");
        assert_eq!(ma.uvs, mb.uvs, "UV grid identical for equal seeds");
    }

    #[test]
    fn globals_audio_mapping_and_aspect() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("map", &doc, &cache).expect("compiles");

        let mut f = frame(0.5, 0.25, 1.0, 42, 2.5);
        f.fps = 30.0;
        f.width = 1920;
        f.height = 1080;
        f.meshx = 16;
        f.meshy = 9;
        for (i, sample) in f.audio.wave.iter_mut().enumerate() {
            *sample = i as f32 * 0.5;
        }
        for (i, byte) in f.audio.byte_frequency_data.iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }

        let mut instance = new_instance(compiled.clone(), 7);
        begin_frame(&mut instance, &f).expect("landscape frame");

        approx!(instance.globals.time, 2.5, "time_seconds -> time");
        approx!(instance.globals.frame, 42.0, "frame_index -> frame");
        approx!(instance.globals.fps, 30.0);
        approx!(instance.globals.meshx, 16.0);
        approx!(instance.globals.meshy, 9.0);
        approx!(instance.globals.pixelsx, 1920.0);
        approx!(instance.globals.pixelsy, 1080.0);
        approx!(instance.globals.bass, 1.0, "relative bass is ~1");
        approx!(instance.globals.mid, 1.0, "relative mid is ~1");
        approx!(instance.globals.treb, 1.0, "relative treble is ~1");
        approx!(instance.globals.bass_att, 1.0, "first-frame bass follower");
        approx!(instance.globals.mid_att, 1.0, "first-frame mid follower");
        approx!(
            instance.globals.treb_att,
            1.0,
            "first-frame treble follower"
        );
        approx!(instance.globals.aspectx, 1.0, "landscape aspectx");
        approx!(
            instance.globals.aspecty,
            1080.0 / 1920.0,
            "landscape aspecty"
        );
        assert_eq!(instance.frame_index, 42);
        assert_eq!(instance.wave, f.audio.wave, "wave data copied to instance");
        assert_eq!(
            instance.byte_frequency_data, f.audio.byte_frequency_data,
            "spectrum data copied to instance"
        );

        // Portrait: the smaller axis becomes 1.0, the wider axis carries ratio.
        let mut fp = f.clone();
        fp.width = 600;
        fp.height = 800;
        let mut portrait = new_instance(compiled, 8);
        begin_frame(&mut portrait, &fp).expect("portrait frame");
        approx!(portrait.globals.aspectx, 600.0 / 800.0, "portrait aspectx");
        approx!(portrait.globals.aspecty, 1.0, "portrait aspecty");
    }

    #[test]
    fn canonical_warp_mesh_covers_the_full_texture() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("warp-full", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 1);
        let mut f = frame(0.0, 0.0, 0.0, 1, 0.0);
        f.width = 640;
        f.height = 360;
        f.meshx = 8;
        f.meshy = 6;
        begin_frame(&mut instance, &f).expect("begin_frame");
        let mesh = evaluate_mesh(&mut instance, &f).expect("mesh");
        let umin = mesh
            .uvs
            .iter()
            .map(|uv| uv[0])
            .fold(f32::INFINITY, f32::min);
        let umax = mesh
            .uvs
            .iter()
            .map(|uv| uv[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let vmin = mesh
            .uvs
            .iter()
            .map(|uv| uv[1])
            .fold(f32::INFINITY, f32::min);
        let vmax = mesh
            .uvs
            .iter()
            .map(|uv| uv[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (umin - 0.0).abs() < 0.02 && (umax - 1.0).abs() < 0.02,
            "landscape warp must span the full texture width, got u=[{umin},{umax}]"
        );
        assert!(
            (vmin - 0.0).abs() < 0.02 && (vmax - 1.0).abs() < 0.02,
            "landscape warp must span the full texture height, got v=[{vmin},{vmax}]"
        );
    }

    #[test]
    fn canonical_warp_centre_samples_the_texture_centre() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("warp-centre", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 1);
        let f = frame(0.0, 0.0, 0.0, 1, 0.0);
        begin_frame(&mut instance, &f).expect("begin_frame");
        let mesh = evaluate_mesh(&mut instance, &f).expect("mesh");
        let centre = mesh.uvs[mesh.uvs.len() / 2];
        assert!(
            (centre[0] - 0.5).abs() < 0.01 && (centre[1] - 0.5).abs() < 0.01,
            "cx=cy=0.5 must sample the texture centre, got {centre:?}"
        );
    }

    #[test]
    fn relative_band_transients_rise_above_one() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("relative-bands", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 1);

        let mut steady = frame(0.5, 0.4, 0.3, 1, 0.016);
        steady.audio.byte_frequency_data.fill(100);
        begin_frame(&mut instance, &steady).expect("steady frame");
        approx!(instance.globals.bass, 1.0, "first frame is the reference");

        let mut transient = frame(1.0, 0.4, 0.3, 2, 0.032);
        transient.audio.byte_frequency_data.fill(200);
        begin_frame(&mut instance, &transient).expect("transient frame");
        assert!(
            instance.globals.bass > 1.5,
            "a doubled FFT band must read above 1.0 relative to the slow average, got {}",
            instance.globals.bass
        );
        assert!(
            instance.globals.mid > 1.5,
            "the same spectral rise reaches mid too, got {}",
            instance.globals.mid
        );
    }

    #[test]
    fn mesh_uv_grid_length_and_finite() {
        let doc = doc_with("", "", "px = x + y + rad + ang;", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("mesh", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 0x1234);

        let mut f = frame(0.2, 0.3, 0.4, 3, 1.0);
        f.meshx = 4;
        f.meshy = 3;
        f.width = 320;
        f.height = 240;

        begin_frame(&mut instance, &f).expect("frame");
        let mesh = evaluate_mesh(&mut instance, &f).expect("mesh");

        assert_eq!(mesh.width, 5, "width = meshx + 1");
        assert_eq!(mesh.height, 4, "height = meshy + 1");
        assert_eq!(
            mesh.uvs.len(),
            ((f.meshx + 1) * (f.meshy + 1)) as usize,
            "one UV per grid vertex"
        );
        for (i, uv) in mesh.uvs.iter().enumerate() {
            assert!(uv[0].is_finite(), "u[{i}] finite, got {}", uv[0]);
            assert!(uv[1].is_finite(), "v[{i}] finite, got {}", uv[1]);
        }
    }

    #[test]
    fn mesh_triangle_indices_cover_every_cell_and_stay_in_range() {
        let width = 4u32;
        let height = 3u32;
        let indices = mesh_triangle_indices(width, height);
        assert_eq!(
            indices.len(),
            ((width - 1) * (height - 1) * 6) as usize,
            "two triangles (six indices) per grid cell"
        );
        let vertex_count = width * height;
        assert!(
            indices.iter().all(|&index| index < vertex_count),
            "every index must address a vertex of the {width}x{height} grid, got {indices:?}"
        );

        for cell_row in 0..height - 1 {
            for cell_col in 0..width - 1 {
                let cell = (cell_row * (width - 1) + cell_col) as usize;
                let base = cell_row * width + cell_col;
                let corners = [base, base + 1, base + width, base + width + 1];
                let tri = &indices[cell * 6..cell * 6 + 6];
                assert!(
                    tri.iter().all(|&index| corners.contains(&index)),
                    "cell ({cell_row},{cell_col}) triangle {tri:?} must use only its four corners {corners:?}"
                );
                for &corner in &corners {
                    assert!(
                        tri.contains(&corner),
                        "cell ({cell_row},{cell_col}) must cover corner {corner}, got {tri:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn failure_cache_mark_query_clear() {
        let cache = MilkFailureCache::new();
        assert!(!cache.is_failed("mod/a"), "fresh cache is empty");
        cache.mark_failed("mod/a");
        assert!(cache.is_failed("mod/a"), "marked module is failed");
        assert!(!cache.is_failed("mod/b"), "other modules unaffected");

        let runtime = MilkRuntime::default();
        assert!(
            !runtime.failures.is_failed("mod/a"),
            "runtime owns its own cache"
        );

        cache.clear();
        assert!(!cache.is_failed("mod/a"), "clear forgets failures");
    }

    #[test]
    fn shader_compilation_valid_and_malformed() {
        let cache = MilkFailureCache::new();

        let good = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let compiled = compile_preset("good", &good, &cache).expect("valid shaders compile");
        assert_eq!(compiled.warp.stage, MilkShaderStage::Warp);
        assert_eq!(compiled.comp.stage, MilkShaderStage::Comp);
        assert!(!compiled.warp.wgsl.is_empty(), "valid warp yields WGSL");
        assert!(!compiled.comp.wgsl.is_empty(), "valid comp yields WGSL");

        let bad = doc_with("", "", "", MALFORMED_WARP, VALID_COMP);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compile_preset("broken", &bad, &cache)
        }));
        let result = result.expect("malformed shader must not panic");
        assert!(result.is_err(), "malformed shader returns an error");
        assert!(
            cache.is_failed("broken"),
            "malformed shader marks the module failed"
        );
    }

    /// A v1 preset has no `MILKDROP_PRESET_VERSION` line and therefore no
    /// `warp_N`/`comp_N` `shader_body` strings at all. `extract_shader_body("")`
    /// yields an empty prelude/body, and a wrapper with no `ret` declaration
    /// makes naga fail with "Unknown variable: ret". Such presets must instead
    /// compile with default identity warp/comp shaders.
    #[test]
    fn compile_preset_accepts_v1_preset_without_shader_bodies() {
        let doc = MilkDocument {
            version: 1,
            name: "v1-no-shaders".to_string(),
            frame_eqs: "x = x + 0.01;".to_string(),
            warp: String::new(),
            comp: String::new(),
            ..Default::default()
        };
        let cache = MilkFailureCache::new();
        let result = compile_preset("v1-no-shaders", &doc, &cache);
        assert!(
            result.is_ok(),
            "a v1 preset with no shader bodies must compile to default identity shaders, got {result:?}"
        );
        assert!(
            !cache.is_failed("v1-no-shaders"),
            "a bodyless v1 preset must not be recorded as a compilation failure"
        );

        let compiled = result.expect("checked is_ok above");
        for (label, shader) in [("warp", &compiled.warp), ("comp", &compiled.comp)] {
            assert!(
                !shader.wgsl.is_empty(),
                "{label}: a default identity shader must generate non-empty WGSL"
            );
            assert!(
                shader.wgsl.contains("textureSample"),
                "{label}: a default identity shader must sample the previous frame, got {:?}",
                shader.wgsl
            );
        }
    }

    // --- Layer 5c: wave/shape overlay CPU tests ----------------------------

    #[test]
    fn wave_init_frame_and_point_equations_share_t_vars() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].init_eqs = "t1 = 7;".to_string();
        // `samples` is a runtime-seeded per-wave-frame var; `r` must reflect it.
        doc.waves[0].frame_eqs = "r = t1 + samples;".to_string();
        // `t1` carried from init must be visible to the point equations.
        doc.waves[0].point_eqs = "value1 = t1; sample = 0.5;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-vars", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x11);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        assert_eq!(overlays.waves.len(), 1, "one enabled wave is evaluated");
        let wave = &overlays.waves[0];
        assert!(wave.enabled, "the enabled wave is marked enabled");
        approx!(
            wave.pool.get("t1"),
            7.0,
            "t1 from init carries into the frame pool"
        );
        let samples = wave.pool.get("samples");
        assert!(samples != 0.0, "samples is seeded as a per-wave-frame var");
        approx!(wave.pool.get("r"), 7.0 + samples, "r = t1 + samples");
        assert!(
            !wave.points.is_empty(),
            "point equations produce sample points"
        );
        approx!(
            wave.points[0].value1,
            7.0,
            "t1 carries into point equations"
        );
    }

    /// MilkDrop wave point equations write normalised screen coordinates
    /// (`x=0` left, `y=0` top), while the GPU overlay pass consumes NDC. A
    /// custom wave that writes `(0, 0)` must land on the top-left corner.
    #[test]
    fn custom_wave_normalised_xy_are_mapped_to_ndc() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].point_eqs = "x = 0; y = 0;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-xy", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x55);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        let point = &overlays.waves[0].points[0];
        approx!(point.x, -1.0, "normalised x=0 is the left edge");
        approx!(point.y, -1.0, "normalised y=0 is the bottom edge");
    }

    #[test]
    fn custom_wave_y_spans_the_full_screen_height() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].values.samples = 2;
        doc.waves[0].values.smoothing = 0.0;
        doc.waves[0].point_eqs = "x = 0.5; y = sample;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-y-full", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x88);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");
        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        let points = &overlays.waves[0].points;
        approx!(points[0].y, -1.0, "y=0 is the bottom edge");
        approx!(points[1].y, 1.0, "y=1 is the top edge");
    }

    /// `q1..q32` written by the preset's top-level per-frame equations must be
    /// visible to `wave_0_per_pointN`. Real presets such as shifter's
    /// "tumbling cubes (ripples)" drive their whole 3D projection from q1/q2;
    /// losing them collapses every sample onto one point and draws nothing.
    #[test]
    fn wave_point_equations_see_q_values_from_the_frame() {
        let mut doc = doc_with("", "q1 = 7;", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].point_eqs = "x = q1 / 10; y = 0.5;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-q", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x44);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        assert!(!overlays.waves.is_empty(), "wave is evaluated");
        approx!(
            overlays.waves[0].points[0].x,
            0.4,
            "per-point equations must see q1 from the frame equations (0.7 normalised = 0.4 NDC)"
        );
    }

    /// Real presets (for example `shifter - tumbling cubes (ripples).milk`)
    /// compute `r/g/b` inside `wave_0_per_pointN`, so each sample needs its own
    /// colour. Using one frame-level colour for the whole polyline renders the
    /// wave monochrome instead of the preset's rainbow.
    #[test]
    fn wave_per_point_colours_reach_the_vertex_buffer() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].point_eqs = "r = sample; g = 1 - sample; b = 0.25; a = 0.5;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-colours", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x33);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        let wave = &overlays.waves[0];
        assert!(wave.points.len() >= 2, "point equations produce samples");

        let verts = wave_overlay_vertices(wave);
        assert_eq!(
            verts.len(),
            (wave.points.len() - 1) * 12,
            "every evaluated sample must contribute a line segment"
        );
        assert_ne!(
            &verts[2..6],
            &verts[verts.len() - 4..],
            "vertex colours must follow the per-point equations, got {verts:?}"
        );
    }

    /// MilkDrop has a single variable pool: user variables written by
    /// `wave_0_per_pointN` survive into the next frame. Presets such as
    /// "witchcraft (i'm melting)" random-walk `mx/my` in their point equations
    /// and rely on that walk accumulating across frames to draw a ring.
    #[test]
    fn wave_point_variables_persist_across_frames() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].point_eqs = "mx = mx + 1; x = 0.5; y = 0.5;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-persist", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x66);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);

        begin_frame(&mut instance, &f).expect("begin_frame");
        let first = evaluate_overlays(&mut instance, &f).expect("evaluate");
        let after_first = first.waves[0].pool.get("mx");
        assert!(
            after_first > 0.0,
            "point equations must write the live pool"
        );

        begin_frame(&mut instance, &f).expect("begin_frame");
        let second = evaluate_overlays(&mut instance, &f).expect("evaluate");
        approx!(
            second.waves[0].pool.get("mx"),
            after_first * 2.0,
            "point variables must carry into the next frame"
        );
    }

    #[test]
    fn dotted_wave_emits_one_vertex_per_sample() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].base.wave_dots = true;
        doc.waves[0].point_eqs = "x = sample; y = 0.5;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-dots", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x77);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        let wave = &overlays.waves[0];
        assert!(wave.dots, "the section's bUseDots flag reaches the overlay");
        assert_eq!(
            wave_overlay_point_vertices(wave).len(),
            wave.points.len() * 6,
            "dots mode draws one RGBA vertex per sample"
        );
    }

    #[test]
    fn shape_init_and_frame_equations_share_t_vars() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.shapes[0].enabled = true;
        doc.shapes[0].init_eqs = "t1 = 5;".to_string();
        doc.shapes[0].frame_eqs = "sides = t1 + 3;".to_string();

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("shape-vars", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x22);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        assert_eq!(overlays.shapes.len(), 1, "one enabled shape is evaluated");
        let shape = &overlays.shapes[0];
        assert!(shape.enabled, "the enabled shape is marked enabled");
        approx!(
            shape.pool.get("t1"),
            5.0,
            "t1 from init carries into the frame pool"
        );
        approx!(shape.pool.get("sides"), 8.0, "sides = t1 + 3");
    }

    #[test]
    fn disabled_overlays_do_not_run_and_active_counts_match() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        // Default wave/shape sections are all disabled.
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("all-disabled", &doc, &cache).expect("fixture compiles");
        assert!(
            compiled.waves.is_empty(),
            "no enabled waves -> no wave programs"
        );
        assert!(
            compiled.shapes.is_empty(),
            "no enabled shapes -> no shape programs"
        );

        let mut instance = new_instance(compiled, 0x33);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");
        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        assert!(
            overlays.waves.is_empty(),
            "disabled waves leave no pool side effects"
        );
        assert!(
            overlays.shapes.is_empty(),
            "disabled shapes leave no pool side effects"
        );

        // Enable one wave (index 1) and one shape (index 2): the active lists
        // must grow to match the enabled counts, and only those sections run.
        doc.waves[1].enabled = true;
        doc.waves[1].frame_eqs = "r = 1;".to_string();
        doc.shapes[2].enabled = true;
        doc.shapes[2].frame_eqs = "sides = 4;".to_string();
        let compiled2 = compile_preset("some-enabled", &doc, &cache).expect("fixture compiles");
        assert_eq!(
            compiled2.waves.len(),
            1,
            "active wave list length matches enabled count"
        );
        assert_eq!(
            compiled2.shapes.len(),
            1,
            "active shape list length matches enabled count"
        );

        let mut instance2 = new_instance(compiled2, 0x34);
        begin_frame(&mut instance2, &f).expect("begin_frame");
        let overlays2 = evaluate_overlays(&mut instance2, &f).expect("evaluate_overlays");
        assert_eq!(overlays2.waves.len(), 1, "only the enabled wave runs");
        assert_eq!(overlays2.shapes.len(), 1, "only the enabled shape runs");
    }

    #[test]
    fn malformed_wave_eel_is_skipped_without_panicking_and_other_waves_run() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].init_eqs = "if (1) { x = 2;".to_string(); // unbalanced braces -> parse error
        doc.waves[1].enabled = true;
        doc.waves[1].init_eqs = "t2 = 9;".to_string();
        doc.waves[1].frame_eqs = "g = t2 + 1;".to_string();

        let cache = MilkFailureCache::new();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compile_preset("wave-errors", &doc, &cache)
        }));
        let compiled = outcome
            .expect("malformed wave EEL must not panic")
            .expect("a broken wave must not fail the whole preset");
        assert_eq!(
            compiled.waves.len(),
            1,
            "the malformed wave is skipped; the valid wave remains"
        );

        let mut instance = new_instance(compiled, 0x44);
        let f = frame(0.1, 0.2, 0.3, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");
        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        assert_eq!(overlays.waves.len(), 1, "only the valid wave is evaluated");
        approx!(
            overlays.waves[0].pool.get("g"),
            10.0,
            "the valid wave still evaluates"
        );
    }

    // --- Built-in waveform for v1 presets (RED) ----------------------------

    /// Build a v1-style document: no `shader_body` strings, no `wavecode_*`
    /// sections. `wave_a`/`wave_scale` are base vals; `wave_r/g/b/x/y` are
    /// per-frame EEL variables in a real MilkDrop 1 preset.
    fn v1_builtin_wave_doc() -> MilkDocument {
        let mut doc = MilkDocument {
            version: 1,
            name: "v1-builtin-wave".to_string(),
            frame_eqs: "wave_r = 1; wave_g = 0; wave_b = 0; wave_x = 0.5; wave_y = 0.5;"
                .to_string(),
            warp: String::new(),
            comp: String::new(),
            ..Default::default()
        };
        doc.base.wave_a = 2.0;
        doc.base.wave_scale = 0.5;
        doc
    }

    /// A non-flat 1024-sample sine so a sample-driven wave is observable.
    fn sine_audio_frame() -> MilkFrameInfo {
        let mut f = frame(0.5, 0.25, 0.75, 1, 0.016);
        for (index, sample) in f.audio.wave.iter_mut().enumerate() {
            *sample = (index as f32 * std::f32::consts::TAU / 64.0).sin();
        }
        f
    }

    /// `begin_frame` must seed the frame-level wave variables from `doc.base`
    /// *before* the per-frame equations run. The self-referential equation
    /// `wave_r = wave_r + 0.1;` therefore accumulates from the configured base
    /// (0.5 -> 0.6), not from the pool's zero default.
    #[test]
    fn begin_frame_seeds_wave_vars_from_base() {
        let mut doc = doc_with("", "wave_r = wave_r + 0.1;", "", "", "");
        doc.version = 1;
        doc.base.wave_r = 0.5;
        doc.base.wave_g = 0.25;
        doc.base.wave_b = 0.75;
        doc.base.wave_x = 0.3;
        doc.base.wave_y = 0.7;

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("seed-wave-base", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 7);
        let f = frame(0.0, 0.0, 0.0, 1, 0.016);

        begin_frame(&mut instance, &f).expect("begin_frame");

        for (name, expected) in [
            ("wave_r", 0.6), // 0.5 base + 0.1 self-referential increment
            ("wave_g", 0.25),
            ("wave_b", 0.75),
            ("wave_x", 0.3),
            ("wave_y", 0.7),
        ] {
            let got = instance.pool.get(name);
            assert!(
                (got - expected).abs() < 1e-6,
                "{name} must be seeded from the base val (expected {expected}), got {got}"
            );
        }
    }

    #[test]
    fn builtin_wave_is_generated_when_no_custom_waves_and_wave_a_positive() {
        let cache = MilkFailureCache::new();
        let doc = v1_builtin_wave_doc();
        assert!(
            doc.active_waves().is_empty(),
            "this fixture must have no custom wave sections"
        );
        let f = sine_audio_frame();

        let mut enabled = new_instance(
            compile_preset("v1-builtin-on", &doc, &cache).expect("fixture compiles"),
            0xBEEF,
        );
        begin_frame(&mut enabled, &f).expect("begin_frame");
        let overlays = evaluate_overlays(&mut enabled, &f).expect("evaluate_overlays");

        assert!(
            !overlays.waves.is_empty(),
            "wave_a > 0 with no custom waves must synthesise a built-in wave overlay"
        );
        let wave = &overlays.waves[0];
        assert!(
            wave.enabled,
            "the synthesised built-in wave must be enabled"
        );
        assert!(
            !wave.points.is_empty(),
            "the built-in wave must contribute at least one sample point"
        );
        for (index, point) in wave.points.iter().enumerate() {
            assert!(
                point.x.is_finite() && point.y.is_finite() && point.sample.is_finite(),
                "built-in wave point {index} must have finite x/y/sample, got {point:?}"
            );
        }

        // Colour comes from wave_r/g/b/a (alpha clamped to [0, 1]).
        approx!(wave.pool.get("r"), 1.0, "wave_r -> red");
        approx!(wave.pool.get("g"), 0.0, "wave_g -> green");
        approx!(wave.pool.get("b"), 0.0, "wave_b -> blue");
        let alpha = wave.pool.get("a");
        assert!(
            alpha.is_finite() && (0.0..=1.0).contains(&alpha),
            "wave_a = 2.0 must be clamped into [0, 1], got {alpha}"
        );

        // Deterministic for identical audio/frame and seed.
        let mut again = new_instance(
            compile_preset("v1-builtin-again", &doc, &cache).expect("fixture compiles"),
            0xBEEF,
        );
        begin_frame(&mut again, &f).expect("begin_frame");
        let again_overlays = evaluate_overlays(&mut again, &f).expect("evaluate_overlays");
        assert_eq!(
            again_overlays.waves.len(),
            overlays.waves.len(),
            "built-in wave count must be deterministic"
        );
        assert_eq!(
            again_overlays.waves[0].points, overlays.waves[0].points,
            "built-in wave points must be identical for identical audio/frame"
        );

        // wave_a == 0 must produce no built-in wave at all.
        let mut zero_doc = doc;
        zero_doc.base.wave_a = 0.0;
        let mut zero = new_instance(
            compile_preset("v1-builtin-off", &zero_doc, &cache).expect("fixture compiles"),
            0xBEEF,
        );
        begin_frame(&mut zero, &f).expect("begin_frame");
        let zero_overlays = evaluate_overlays(&mut zero, &f).expect("evaluate_overlays");
        assert!(
            zero_overlays.waves.is_empty(),
            "wave_a == 0 must produce no built-in wave, got {} wave(s)",
            zero_overlays.waves.len()
        );
    }

    // --- Legacy hue shader and post uniforms --------------------------------

    #[test]
    fn hue_corner_colours_match_the_milkdrop_shade_field() {
        let rand_start = [0.1, 0.2, 0.3, 0.4];
        let time = 1.75;
        let corners = hue_corner_colors(time, rand_start);
        for (index, corner) in corners.iter().enumerate() {
            let max = corner[0].max(corner[1]).max(corner[2]);
            approx!(max, 1.0, "corner {index} is normalised");
            for channel in corner {
                assert!(
                    (0.5..=1.0).contains(channel),
                    "corner {index} channel {channel} must be in [0.5, 1]"
                );
            }
        }

        // Bilinear weights: (1,1) is corner 0, (0,1) corner 1, (1,0) corner 2,
        // (0,0) corner 3.
        approx!(hue_shader_at([1.0, 1.0], &corners)[1], corners[0][1]);
        approx!(hue_shader_at([0.0, 1.0], &corners)[1], corners[1][1]);
        approx!(hue_shader_at([1.0, 0.0], &corners)[1], corners[2][1]);
        approx!(hue_shader_at([0.0, 0.0], &corners)[1], corners[3][1]);
    }

    #[test]
    fn post_uniforms_read_live_echo_gamma_hue_and_flags() {
        let mut doc = MilkDocument {
            version: 1,
            name: "post-live".to_string(),
            ..Default::default()
        };
        doc.base.echo_zoom = 0.5;
        doc.base.echo_alpha = 0.25;
        doc.base.echo_orient = 3;
        doc.base.gammaadj = 2.0;
        doc.base.shader = 0.5;
        doc.base.darken = true;
        doc.base.solarize = true;

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("post-live", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 0x5EED);
        let f = frame(0.1, 0.2, 0.3, 1, 0.5);
        begin_frame(&mut instance, &f).expect("begin_frame");

        // Per-frame EEL overrides must win over the base vals.
        instance.pool.set("gamma", 3.25);
        instance.pool.set("echo_alpha", 0.75);

        let bytes = MilkPostUniforms::from_instance(&instance).to_bytes();
        let read =
            |offset: usize| f32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
        approx!(read(0), 0.5, "time");
        approx!(read(4), 3.25, "live gamma overrides fGammaAdj");
        approx!(read(8), 0.5, "fshader");
        approx!(read(12), 0.5, "echo_zoom");
        approx!(read(16), 0.75, "live echo_alpha");
        approx!(read(20), 3.0, "echo_orient");
        approx!(read(24), 0.0, "brighten");
        approx!(read(28), 1.0, "darken");
        approx!(read(32), 1.0, "solarize");
        approx!(read(44), 1.0, "legacy composite flag");
        approx!(read(48), instance.rand_start[0], "rand_start[0]");
        approx!(read(60), instance.rand_start[3], "rand_start[3]");
    }

    #[test]
    fn legacy_composite_flag_tracks_whether_a_comp_body_exists() {
        let cache = MilkFailureCache::new();
        let legacy = doc_with("", "", "", VALID_WARP, "");
        let compiled = compile_preset("legacy", &legacy, &cache).expect("compiles");
        assert!(compiled.legacy_composite, "empty comp uses legacy post");

        let custom = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let compiled = compile_preset("custom", &custom, &cache).expect("compiles");
        assert!(
            !compiled.legacy_composite,
            "custom comp owns post-processing"
        );
    }

    #[test]
    fn custom_wave_seeds_audio_value_and_xy_like_milkdrop() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.waves[0].enabled = true;
        doc.waves[0].values.samples = 4;
        doc.waves[0].values.smoothing = 0.0;
        doc.waves[0].values.scaling = 1.0;
        doc.base.wave_scale = 1.0;

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("wave-audio", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x99);

        let mut f = frame(0.0, 0.0, 0.0, 1, 0.016);
        f.width = 640;
        f.height = 640;
        // The runtime consumes the most recent 512 samples and centres four of
        // them at source indices 254..257, i.e. absolute indices 766..769.
        for (index, sample) in f.audio.wave.iter_mut().enumerate().skip(766).take(4) {
            *sample = (index as f32 - 766.0 + 1.0) / 4.0;
        }
        begin_frame(&mut instance, &f).expect("begin_frame");
        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        let wave = &overlays.waves[0];
        assert_eq!(wave.points.len(), 4, "samples comes from the wave section");

        for (index, point) in wave.points.iter().enumerate() {
            let expected_value = (index as f32 + 1.0) / 4.0 * (0.004 * 128.0);
            approx!(
                point.value1,
                expected_value,
                "value1 is the scaled, smoothed audio sample at {index}"
            );
            approx!(
                point.x,
                expected_value * 2.0,
                "x starts at 0.5 + value1 (square frame)"
            );
            approx!(
                point.y,
                expected_value * 2.0,
                "y starts at 0.5 + value2 (mono right = left)"
            );
            approx!(point.sample, index as f32 / 3.0, "sample normalised 0..1");
        }
    }

    #[test]
    fn builtin_mode7_draws_two_separated_stereo_lines() {
        let mut doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        doc.version = 1;
        doc.base.wave_mode = 7;
        doc.base.wave_a = 1.0;
        doc.base.wave_mystery = 0.0;
        doc.base.wave_r = 1.0;
        doc.base.wave_g = 0.0;
        doc.base.wave_b = 0.0;
        doc.base.wave_x = 0.5;
        doc.base.wave_y = 0.5;
        doc.base.wave_scale = 1.0;

        let cache = MilkFailureCache::new();
        let compiled = compile_preset("mode7", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0x7);
        let f = sine_audio_frame();
        begin_frame(&mut instance, &f).expect("begin_frame");
        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");

        assert_eq!(
            overlays.waves.len(),
            2,
            "mode 7 draws a left and a right line"
        );
        let mean_y = |wave: &WaveOverlay| {
            wave.points.iter().map(|point| point.y).sum::<f32>() / wave.points.len() as f32
        };
        assert!(
            mean_y(&overlays.waves[0]) > mean_y(&overlays.waves[1]),
            "the two stereo lines must be separated by the wave position"
        );
        for wave in &overlays.waves {
            approx!(wave.pool.get("r"), 1.0);
            approx!(wave.pool.get("g"), 0.0);
            approx!(wave.pool.get("a"), 1.0);
        }
    }

    #[test]
    fn rand_start_is_deterministic_and_varied_by_seed() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("rand-start", &doc, &cache).expect("compiles");
        let a = new_instance(compiled.clone(), 42);
        let b = new_instance(compiled.clone(), 42);
        let c = new_instance(compiled, 43);
        assert_eq!(a.rand_start, b.rand_start, "same seed is deterministic");
        assert_ne!(a.rand_start, c.rand_start, "different seeds vary the hue");
    }

    // --- Built-in noise samplers (RED) -------------------------------------

    #[test]
    fn milk_noise_is_deterministic_non_uniform_and_distinct_per_frequency() {
        let a = MilkNoise::new();
        let b = MilkNoise::new();

        let expected_len = (MilkNoise::SIZE as usize) * (MilkNoise::SIZE as usize) * 4;
        for (label, bytes) in [("lq", &a.lq), ("mq", &a.mq), ("hq", &a.hq), ("pw", &a.pw)] {
            assert_eq!(
                bytes.len(),
                expected_len,
                "{label} noise must be a {}x{} RGBA8 field",
                MilkNoise::SIZE,
                MilkNoise::SIZE
            );
            let distinct = bytes.iter().copied().collect::<HashSet<_>>();
            assert!(
                distinct.len() > 1,
                "{label} noise must be non-uniform (multiple distinct byte values), got {distinct:?}"
            );
        }

        assert_eq!(a, b, "two generations of the noise must be byte-identical");
        assert_ne!(a.lq, a.mq, "LQ and MQ noise must differ");
        assert_ne!(a.mq, a.hq, "MQ and HQ noise must differ");
        assert_ne!(a.lq, a.hq, "LQ and HQ noise must differ");
        assert_ne!(a.lq, a.pw, "LQ and preset-wave noise must differ");
    }

    /// MilkDrop's `texsize` is a vec4 — x=width, y=height, z=1/width,
    /// w=1/height. The runtime uniform payload must serialize all four values
    /// at texsize's declared offset (byte 128: the first vector member after
    /// the 32 f32 scalars), while the total buffer stays 256 bytes.
    #[test]
    fn uniform_payload_exposes_texsize_as_vec4() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("texsize-vec4", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 7);
        let mut f = frame(0.5, 0.25, 1.0, 1, 0.016);
        f.width = 1920;
        f.height = 1080;
        begin_frame(&mut instance, &f).expect("begin_frame");

        let bytes = MilkUniforms::from_frame(&instance, &f).to_bytes();
        assert_eq!(
            bytes.len(),
            MILK_UNIFORM_STRIDE,
            "the payload must stay 256 bytes"
        );

        // texsize is the first member after the 32 f32 scalars = byte 128.
        const TEXSIZE_OFFSET: usize = 32 * 4;
        let read =
            |i: usize| f32::from_ne_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let texsize = [
            read(TEXSIZE_OFFSET),
            read(TEXSIZE_OFFSET + 4),
            read(TEXSIZE_OFFSET + 8),
            read(TEXSIZE_OFFSET + 12),
        ];
        let expected = [1920.0_f32, 1080.0_f32, 1.0 / 1920.0_f32, 1.0 / 1080.0_f32];
        for (index, (got, want)) in texsize.iter().zip(expected.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "texsize[{index}] must be {want}, got {got} (decoded texsize {texsize:?})"
            );
        }
    }

    /// Real v2 presets keep the per-frame `q1..q32` values in the EEL `VarPool`.
    /// They must be serialized into a dedicated 128-byte GPU payload (32 f32s)
    /// so the translated shader's q uniforms can be uploaded each frame. This
    /// block is separate from the 256-byte `MilkUniforms` buffer so none of the
    /// existing byte offsets or the sampler binding layout change.
    #[test]
    fn q_values_from_pool_are_serialized_to_gpu_bytes() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("q-uniform-bytes", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 7);
        let f = frame(0.5, 0.25, 1.0, 1, 0.016);
        begin_frame(&mut instance, &f).expect("begin_frame");

        // Set AFTER begin_frame so the q-baseline restore cannot overwrite them.
        instance.pool.set("q4", 3.5);
        instance.pool.set("q1", 1.25);

        let bytes = MilkQUniforms::from_instance(&instance).to_bytes();
        assert_eq!(
            bytes.len(),
            32 * 4,
            "the q uniform block must hold exactly 32 f32 values (128 bytes)"
        );

        // Slot i holds q(i+1): q1 is the first f32, q4 is the fourth.
        let read = |index: usize| {
            f32::from_ne_bytes([
                bytes[index * 4],
                bytes[index * 4 + 1],
                bytes[index * 4 + 2],
                bytes[index * 4 + 3],
            ])
        };
        approx!(read(0), 1.25, "q1 is serialized at the first f32 slot");
        approx!(read(3), 3.5, "q4 is serialized at the fourth f32 slot");
    }
}

// ---------------------------------------------------------------------------
// RED tests — GPU render passes (Layer 5b). Device acquisition, read-back and
// skip-on-no-adapter follow `crate::isf::golden` / `crate::test_gpu`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod gpu_tests {
    use std::sync::Arc;

    use super::*;
    use crate::isf::runtime::read_texture;
    use crate::test_gpu::gpu;

    const WIDTH: u32 = 160;
    const HEIGHT: u32 = 90;

    /// Identity warp: sample the previous frame at the mesh's own UV.
    const IDENTITY_WARP: &str = "shader_body {\n\
        float3 ret = tex2D(sampler_main, uv_orig).rgb;\n\
        }\n";

    fn doc_with_comp(comp_body: &str) -> MilkDocument {
        let mut doc = MilkDocument {
            version: 2,
            name: "gpu-fixture".to_string(),
            warp: IDENTITY_WARP.to_string(),
            comp: format!("shader_body {{\n{comp_body}\n}}\n"),
            ..Default::default()
        };
        // Wave-specific fixtures opt the built-in waveform back in explicitly.
        doc.base.wave_a = 0.0;
        doc
    }

    /// Neutralise the output pass so the comp colour reaches the target
    /// unchanged (gammaadj 1.0, every post-op disabled).
    fn identity_output(doc: &mut MilkDocument) {
        doc.base.gammaadj = 1.0;
        doc.base.invert = false;
        doc.base.brighten = false;
        doc.base.darken = false;
        doc.base.solarize = false;
        doc.base.red_blue = false;
    }

    fn frame() -> MilkFrameInfo {
        MilkFrameInfo {
            time_seconds: 1.25,
            frame_index: 3,
            fps: 60.0,
            width: WIDTH,
            height: HEIGHT,
            meshx: 4,
            meshy: 4,
            audio: MilkAudio {
                bass: 0.5,
                mid: 0.25,
                treble: 0.75,
                sample_rate: 48_000.0,
                wave: vec![0.0; 1024],
                byte_frequency_data: vec![128; 1024],
            },
        }
    }

    fn test_texture(device: &wgpu::Device, width: u32, height: u32, label: &str) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Clear a target to a known colour without drawing anything.
    fn clear_to(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        rgba: [f64; 4],
    ) {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("milkdrop RED clear"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("milkdrop RED clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: rgba[0],
                            g: rgba[1],
                            b: rgba[2],
                            a: rgba[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        queue.submit(Some(encoder.finish()));
    }

    /// A black RGBA8 texture carrying one opaque white rectangular patch at its
    /// centre. Fed in as the `previous` frame so a blur pass has a localized
    /// energy source whose spilled neighbours can be measured after read-back.
    fn patch_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        label: &str,
    ) -> wgpu::Texture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = (width as usize * 4).div_ceil(align) * align;
        let mut bytes = vec![0u8; padded * height as usize];
        let cx = width / 2;
        let cy = height / 2;
        for row in 0..height as usize {
            for col in 0..width as usize {
                bytes[row * padded + col * 4 + 3] = 255;
            }
        }
        for row in cy.saturating_sub(2)..=(cy + 2).min(height - 1) {
            for col in cx.saturating_sub(3)..=(cx + 3).min(width - 1) {
                let offset = row as usize * padded + col as usize * 4;
                bytes[offset] = 255;
                bytes[offset + 1] = 255;
                bytes[offset + 2] = 255;
                bytes[offset + 3] = 255;
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        texture
    }

    /// Every pixel's RGB channels must land within `tolerance` of `expected`.
    fn assert_pixels_near(pixels: &[u8], expected: [u8; 3], tolerance: u8, context: &str) {
        let (chunks, _) = pixels.as_chunks::<4>();
        assert!(!chunks.is_empty(), "{context}: no pixels were read back");
        for (index, pixel) in chunks.iter().enumerate() {
            for channel in 0..3 {
                let delta = pixel[channel].abs_diff(expected[channel]);
                assert!(
                    delta <= tolerance,
                    "{context}: pixel {index} channel {channel} = {}, expected {} (±{tolerance}), delta {delta}",
                    pixel[channel],
                    expected[channel]
                );
            }
        }
    }

    /// Prepare the compiled instance, frame, mesh and textures for a render test.
    fn render_fixture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        runtime: &MilkRuntime,
        doc: MilkDocument,
    ) -> (
        MilkInstance,
        MilkFrameInfo,
        MilkMesh,
        wgpu::Texture,
        wgpu::TextureView,
        wgpu::Texture,
        wgpu::TextureView,
        MilkGpuTargets,
    ) {
        let name = doc.name.clone();
        let compiled = compile_preset(&name, &doc, &runtime.failures).expect("fixture compiles");
        let mut instance = new_instance(compiled, 7);
        let frame = frame();
        begin_frame(&mut instance, &frame).expect("begin_frame");
        let mesh = evaluate_mesh(&mut instance, &frame).expect("evaluate_mesh");

        let previous = test_texture(device, WIDTH, HEIGHT, "milkdrop RED previous");
        clear_to(device, queue, &previous, [0.0, 0.0, 0.0, 1.0]);
        let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());
        let output = test_texture(device, WIDTH, HEIGHT, "milkdrop RED output");
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let targets = MilkGpuTargets::new(device, WIDTH, HEIGHT);

        (
            instance,
            frame,
            mesh,
            previous,
            previous_view,
            output,
            output_view,
            targets,
        )
    }

    #[test]
    fn mesh_gpu_buffer_reports_grid_dimensions_and_counts() {
        let (device, _queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping mesh_gpu_buffer_reports_grid_dimensions_and_counts: no GPU adapter in this environment"
                );
                return;
            }
        };
        let doc = doc_with_comp("float3 ret = float3(0.2, 0.4, 0.6);");
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("mesh-gpu", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 7);
        let f = frame();
        begin_frame(&mut instance, &f).expect("begin_frame");
        let mesh = evaluate_mesh(&mut instance, &f).expect("evaluate_mesh");

        let gpu_mesh = MilkMeshGpu::new(&device, &mesh);
        assert_eq!(
            gpu_mesh.width, mesh.width,
            "GPU mesh width matches the UV grid width"
        );
        assert_eq!(
            gpu_mesh.height, mesh.height,
            "GPU mesh height matches the UV grid height"
        );
        assert_eq!(
            gpu_mesh.vertex_count,
            mesh.uvs.len(),
            "vertex_count == (meshx + 1) * (meshy + 1)"
        );
        assert_eq!(
            gpu_mesh.index_count,
            ((mesh.width - 1) * (mesh.height - 1) * 6) as usize,
            "index_count == meshx * meshy * 6 (two triangles per grid cell)"
        );
    }

    #[test]
    fn gpu_warp_uses_mesh_uvs_zoom_changes_warp_target() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_warp_uses_mesh_uvs: no GPU adapter in this environment");
                return;
            }
        };

        // A non-uniform previous frame (a 2D RGB ramp). Sampling it at
        // different mesh UVs yields measurably different pixels, so a
        // mesh-driven warp must react to `zoom`. COPY_DST is required so the
        // gradient can be uploaded; COPY_SRC so read-back can inspect it.
        fn gradient_texture(
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            width: u32,
            height: u32,
        ) -> wgpu::Texture {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("milkdrop RED gradient previous"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
            let padded = (width as usize * 4).div_ceil(align) * align;
            let mut bytes = vec![0u8; padded * height as usize];
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let offset = y * padded + x * 4;
                    bytes[offset] = (x * 255 / (width as usize - 1)) as u8;
                    bytes[offset + 1] = (y * 255 / (height as usize - 1)) as u8;
                    bytes[offset + 2] = 128;
                    bytes[offset + 3] = 255;
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            texture
        }

        // Solid-colour comp (so the composite itself is irrelevant) + a warp
        // that samples sampler_main. The observable signal is the warp target:
        // a real mesh-UV upload changes what the warp pass samples as zoom moves.
        let render_warp = |zoom: f64| -> Vec<u8> {
            let mut doc = doc_with_comp("float3 ret = float3(0.2, 0.4, 0.6);");
            doc.name = format!("warp-zoom-{zoom}");
            identity_output(&mut doc);
            doc.base.zoom = zoom;
            doc.base.zoomexp = 1.0; // zoom_factor = zoom^1 = zoom
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                _previous,
                _previous_view,
                _output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc);
            let previous = gradient_texture(&device, &queue, WIDTH, HEIGHT);
            let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("render");
            let warp = targets.warp_texture().expect("warp target exists");
            read_texture(&device, &queue, warp).expect("read warp target back")
        };

        let zoom1_a = render_warp(1.0);
        let zoom1_b = render_warp(1.0);
        let zoom2_a = render_warp(2.0);
        let zoom2_b = render_warp(2.0);

        assert_eq!(
            zoom1_a.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 warp target must be read back"
        );
        assert_eq!(
            zoom1_a, zoom1_b,
            "two renders at zoom=1.0 must be byte-identical"
        );
        assert_eq!(
            zoom2_a, zoom2_b,
            "two renders at zoom=2.0 must be byte-identical"
        );
        assert_ne!(
            zoom1_a, zoom2_a,
            "mesh UVs must drive warp sampling: zoom=2.0 must change the warp target vs zoom=1.0"
        );
    }

    #[test]
    fn uniform_payload_has_declared_stride_and_serializes_frame_values() {
        assert_eq!(
            MILK_UNIFORM_STRIDE % 256,
            0,
            "the uniform stride is 256-byte aligned"
        );
        let doc = doc_with_comp("float3 ret = float3(0.2, 0.4, 0.6);");
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("uniform-layout", &doc, &cache).expect("fixture compiles");
        let mut instance = new_instance(compiled, 0xC0FFEE);
        let frame = frame();
        begin_frame(&mut instance, &frame).expect("begin_frame");

        let uniforms = MilkUniforms::from_frame(&instance, &frame);
        let bytes = uniforms.to_bytes();
        assert_eq!(
            bytes.len(),
            MILK_UNIFORM_STRIDE,
            "serialized uniform buffer has the declared stride"
        );
        assert!(
            bytes.iter().any(|&byte| byte != 0),
            "the uniform payload must serialize Globals + base vals + rand, not stay zeroed"
        );
    }

    #[test]
    fn gpu_comp_solid_color_matches_embedded_rgba() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_comp_solid_color: no GPU adapter in this environment");
                return;
            }
        };
        let mut doc = doc_with_comp("float3 ret = float3(0.2, 0.4, 0.6);");
        doc.name = "solid-comp".to_string();
        identity_output(&mut doc);
        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let (mut instance, frame, mesh, _previous, previous_view, output, output_view, mut targets) =
            render_fixture(&device, &queue, &runtime, doc);

        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &output_view,
            )
            .expect("solid comp renders");

        let pixels = read_texture(&device, &queue, &output).expect("read the output back");
        // 0.2/0.4/0.6 in Rgba8Unorm -> 51/102/153.
        assert_pixels_near(&pixels, [51, 102, 153], 6, "solid comp");
    }

    /// Real v2 preset shaders assign to bare `ret` without declaring it. The
    /// wrapper must inject a local `vec3 ret` so such a comp shader compiles
    /// and renders the expected solid colour, deterministically.
    #[test]
    fn gpu_bare_ret_assignment_renders() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_bare_ret_assignment_renders: no GPU adapter in this environment"
                );
                return;
            }
        };

        let render_once = || -> Vec<u8> {
            let mut doc = doc_with_comp("ret = vec3(0.2, 0.4, 0.6);");
            doc.name = "bare-ret-comp".to_string();
            identity_output(&mut doc);
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                _previous,
                previous_view,
                output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc);
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("a comp shader assigning bare `ret` must compile and render");
            read_texture(&device, &queue, &output).expect("read the output back")
        };

        let first = render_once();
        let second = render_once();

        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        // 0.2/0.4/0.6 in Rgba8Unorm -> 51/102/153.
        assert_pixels_near(&first, [51, 102, 153], 8, "bare-ret comp");
        assert_eq!(
            first, second,
            "two renders of the same bare-ret comp must be byte-identical"
        );
    }

    #[test]
    fn gpu_warp_passthrough_samples_previous_frame() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_warp_passthrough: no GPU adapter in this environment");
                return;
            }
        };
        let mut doc = doc_with_comp("float3 ret = tex2D(sampler_main, uv_orig).rgb;");
        doc.name = "warp-passthrough".to_string();
        identity_output(&mut doc);
        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let (mut instance, frame, mesh, previous, _previous_view, output, output_view, mut targets) =
            render_fixture(&device, &queue, &runtime, doc);
        // A known solid previous frame: 0.25/0.5/0.75 -> 64/128/191. With the
        // identity warp and passthrough comp, that colour reaches the output.
        clear_to(&device, &queue, &previous, [0.25, 0.5, 0.75, 1.0]);
        let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());

        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &output_view,
            )
            .expect("warp passthrough renders");

        let pixels = read_texture(&device, &queue, &output).expect("read the output back");
        assert_pixels_near(&pixels, [64, 128, 191], 8, "warp passthrough");
        assert!(
            pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[0] != 0),
            "the passthrough output must not stay black"
        );
    }

    #[test]
    fn gpu_blur_targets_allocated_and_invert_output_changes_pixels() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_blur_invert: no GPU adapter in this environment");
                return;
            }
        };
        let mut doc = doc_with_comp("float3 ret = float3(0.2, 0.4, 0.6);");
        doc.name = "blur-invert".to_string();
        identity_output(&mut doc);
        doc.base.invert = true;
        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let (mut instance, frame, mesh, _previous, previous_view, output, output_view, mut targets) =
            render_fixture(&device, &queue, &runtime, doc);

        // The three blur targets exist at the frame's dimensions before rendering.
        for index in 0..3 {
            let size = targets
                .blur_texture(index)
                .map(|texture| (texture.width(), texture.height()));
            assert_eq!(
                size,
                Some((WIDTH, HEIGHT)),
                "blur target {index} allocated at {WIDTH}x{HEIGHT}"
            );
        }

        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &output_view,
            )
            .expect("invert output renders");

        let pixels = read_texture(&device, &queue, &output).expect("read the output back");
        // invert(0.2, 0.4, 0.6) = (0.8, 0.6, 0.4) -> 204/153/102.
        assert_pixels_near(&pixels, [204, 153, 102], 6, "invert output");
    }

    #[test]
    fn gpu_targets_resize_reallocates_textures() {
        let (device, _) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_targets_resize: no GPU adapter in this environment");
                return;
            }
        };
        let mut targets = MilkGpuTargets::new(&device, 160, 90);
        assert_eq!((targets.width, targets.height), (160, 90));
        assert_eq!(
            targets
                .warp_texture()
                .map(|texture| (texture.width(), texture.height())),
            Some((160, 90)),
            "warp target allocated at the initial size"
        );
        assert_eq!(
            targets
                .comp_texture()
                .map(|texture| (texture.width(), texture.height())),
            Some((160, 90)),
            "comp target allocated at the initial size"
        );

        targets.resize(&device, 320, 180);
        assert_eq!((targets.width, targets.height), (320, 180));
        assert_eq!(
            targets
                .warp_texture()
                .map(|texture| (texture.width(), texture.height())),
            Some((320, 180)),
            "warp target reallocated on resize"
        );
        assert_eq!(
            targets
                .comp_texture()
                .map(|texture| (texture.width(), texture.height())),
            Some((320, 180)),
            "comp target reallocated on resize"
        );
    }

    #[test]
    fn gpu_malformed_shader_is_typed_error_and_clear_failures_resets() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_malformed_shader: no GPU adapter in this environment");
                return;
            }
        };
        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);

        // (a) A malformed comp shader is cached as failed, never panics at compile.
        let bad = doc_with_comp("float3 ret = ;\nthis is not valid GLSL @@@");
        assert!(
            compile_preset("broken/shader", &bad, &runtime.failures).is_err(),
            "malformed comp shader fails compilation"
        );
        assert!(
            runtime.failures.is_failed("broken/shader"),
            "failed module is recorded in the failure cache"
        );

        // (b) clear_failures() forgets it.
        runtime.clear_failures();
        assert!(
            !runtime.failures.is_failed("broken/shader"),
            "clear_failures resets recorded failures"
        );

        // (c) Invalid WGSL at draw time is a typed error, not a panic.
        let mut doc = doc_with_comp("float3 ret = float3(0.2, 0.4, 0.6);");
        doc.name = "render-fixture".to_string();
        identity_output(&mut doc);
        let (
            mut instance,
            frame,
            mesh,
            _previous,
            previous_view,
            _output,
            output_view,
            mut targets,
        ) = render_fixture(&device, &queue, &runtime, doc);
        instance.compiled.comp.wgsl = "this is not valid WGSL @@@".to_string();

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &output_view,
            )
        }));
        assert!(outcome.is_ok(), "render must not panic on malformed WGSL");
        let result = outcome.expect("no panic");
        assert!(
            matches!(&result, Err(MilkRuntimeError::Draw(_))),
            "malformed WGSL returns MilkRuntimeError::Draw, got {result:?}"
        );
    }

    // --- Layer 5c: GPU overlay golden tests -------------------------------

    /// Compile, evaluate overlays, render and read back one preset.
    fn render_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        runtime: &MilkRuntime,
        doc: MilkDocument,
    ) -> Vec<u8> {
        let (mut instance, frame, mesh, _previous, previous_view, output, output_view, mut targets) =
            render_fixture(device, queue, runtime, doc);
        // GREEN caches the evaluated overlays on `instance` for the GPU pass.
        let _ = evaluate_overlays(&mut instance, &frame).expect("evaluate_overlays");
        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &output_view,
            )
            .expect("render");
        read_texture(device, queue, &output).expect("read back")
    }

    /// Count pixels that are bright in all three channels.
    fn bright_count(pixels: &[u8]) -> usize {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] > 128 && pixel[1] > 128 && pixel[2] > 128)
            .count()
    }

    /// Like `render_pixels`, but accepts a caller-supplied audio frame so a
    /// non-flat 1024-sample wave can drive the built-in waveform.
    fn render_pixels_with_frame(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        runtime: &MilkRuntime,
        doc: MilkDocument,
        frame: MilkFrameInfo,
    ) -> Vec<u8> {
        let name = doc.name.clone();
        let compiled = compile_preset(&name, &doc, &runtime.failures).expect("fixture compiles");
        let mut instance = new_instance(compiled, 7);
        begin_frame(&mut instance, &frame).expect("begin_frame");
        let mesh = evaluate_mesh(&mut instance, &frame).expect("evaluate_mesh");

        let previous = test_texture(device, WIDTH, HEIGHT, "builtin-wave RED previous");
        clear_to(device, queue, &previous, [0.0, 0.0, 0.0, 1.0]);
        let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());
        let output = test_texture(device, WIDTH, HEIGHT, "builtin-wave RED output");
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let mut targets = MilkGpuTargets::new(device, WIDTH, HEIGHT);

        let _ = evaluate_overlays(&mut instance, &frame).expect("evaluate_overlays");
        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &output_view,
            )
            .expect("render");
        read_texture(device, queue, &output).expect("read back")
    }

    /// A v1-style GPU fixture: no wavecode sections, black comp, identity
    /// output and a red built-in wave configured through base vals + frame EEL.
    fn builtin_wave_gpu_doc() -> MilkDocument {
        let mut doc = doc_with_comp("float3 ret = vec3(0.0);");
        doc.version = 1;
        doc.name = "v1-builtin-wave-gpu".to_string();
        doc.frame_eqs =
            "wave_r = 1; wave_g = 0; wave_b = 0; wave_x = 0.5; wave_y = 0.5;".to_string();
        doc.base.wave_a = 2.0;
        doc.base.wave_scale = 0.5;
        identity_output(&mut doc);
        doc
    }

    fn sine_gpu_frame() -> MilkFrameInfo {
        let mut f = frame();
        for (index, sample) in f.audio.wave.iter_mut().enumerate() {
            *sample = (index as f32 * std::f32::consts::TAU / 64.0).sin();
        }
        f
    }

    fn red_dominant_count(pixels: &[u8]) -> usize {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 40 && p[0] > p[1].saturating_mul(2) && p[0] > p[2].saturating_mul(2))
            .count()
    }

    fn green_dominant_count(pixels: &[u8]) -> usize {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[1] > 40 && p[1] > p[0].saturating_mul(2) && p[1] > p[2].saturating_mul(2))
            .count()
    }

    /// MilkDrop draws waves onto the composite *before* the post chain, so the
    /// fixed darken+solarize filters apply to them as well. A white wave under
    /// darken+solarize maps to black; the same wave without those filters stays
    /// bright. This pins the `DrawCustomWaves` → `ShowToUser` ordering.
    #[test]
    fn gpu_waves_are_drawn_before_comp_post_processing() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => return,
        };
        let make_doc = |name: &str, filtered: bool| {
            let mut doc = doc_with_comp("float3 ret = float3(0.0, 0.0, 0.0);");
            doc.name = name.to_string();
            identity_output(&mut doc);
            if filtered {
                doc.base.darken = true;
                doc.base.solarize = true;
            }
            doc.waves[0].enabled = true;
            doc.waves[0].frame_eqs = "r = 1; g = 1; b = 1; a = 1;".to_string();
            doc.waves[0].point_eqs = "x = sample; y = 0.5;".to_string();
            doc
        };

        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let plain = render_pixels(
            &device,
            &queue,
            &runtime,
            make_doc("wave-before-post", false),
        );
        let filtered = render_pixels(
            &device,
            &queue,
            &runtime,
            make_doc("wave-before-post-dark", true),
        );
        assert!(
            bright_count(&plain) > 0,
            "the white wave must be visible without filters"
        );
        assert_eq!(
            bright_count(&filtered),
            0,
            "darken+solarize must be applied to the wave (white -> black)"
        );
    }

    #[test]
    fn gpu_wave_overlay_enabled_changes_pixels_vs_disabled() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_wave_overlay: no GPU adapter in this environment");
                return;
            }
        };
        // A near-black composite background; the wave's frame equations set a
        // solid bright colour that the overlay pass must paint onto the comp.
        let mut off = doc_with_comp("float3 ret = float3(0.05, 0.05, 0.05);");
        off.name = "wave-off".to_string();
        identity_output(&mut off);
        let mut on = off.clone();
        on.name = "wave-on".to_string();
        on.waves[0].enabled = true;
        on.waves[0].frame_eqs = "r = 1; g = 1; b = 1; a = 1;".to_string();
        on.waves[0].point_eqs = "x = sample; y = 0.5;".to_string();

        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let off_pixels = render_pixels(&device, &queue, &runtime, off);
        let on_pixels = render_pixels(&device, &queue, &runtime, on);
        assert_ne!(
            off_pixels, on_pixels,
            "enabling a bright wave must change the rendered pixels"
        );
        assert!(
            bright_count(&on_pixels) > bright_count(&off_pixels),
            "the enabled wave must produce more bright pixels (on={}, off={})",
            bright_count(&on_pixels),
            bright_count(&off_pixels)
        );
    }

    /// A v1 preset with no custom waves must paint visible, red-dominant
    /// pixels from its built-in waveform, deterministically. A control doc
    /// with `wave_a == 0` must leave the black comp untouched.
    #[test]
    fn gpu_builtin_wave_renders_coloured_pixels() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_builtin_wave_renders_coloured_pixels: no GPU adapter in this environment"
                );
                return;
            }
        };

        let render_once = || {
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            render_pixels_with_frame(
                &device,
                &queue,
                &runtime,
                builtin_wave_gpu_doc(),
                sine_gpu_frame(),
            )
        };

        let first = render_once();
        let second = render_once();
        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        assert!(
            red_dominant_count(&first) > 0,
            "the built-in red wave must paint red-dominant pixels, got {} (first 8 RGBA: {:?})",
            red_dominant_count(&first),
            first.as_chunks::<4>().0.iter().take(8).collect::<Vec<_>>()
        );
        assert_eq!(
            first, second,
            "two renders of the same built-in wave must be byte-identical"
        );

        let mut off = builtin_wave_gpu_doc();
        off.base.wave_a = 0.0;
        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let off_pixels = render_pixels_with_frame(&device, &queue, &runtime, off, sine_gpu_frame());
        assert_pixels_near(&off_pixels, [0, 0, 0], 4, "wave_a == 0 control");
    }

    /// A built-in wave whose only frame equation is a self-reference must keep
    /// the colour seeded from `doc.base` instead of starting from zero and
    /// clamping to black. `wave_r = 0.6` over a black comp with identity output
    /// paints a peak red channel of ~153 (0.6 * 255), not 0.
    #[test]
    fn gpu_builtin_wave_uses_wave_base_color() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_builtin_wave_uses_wave_base_color: no GPU adapter in this environment"
                );
                return;
            }
        };

        let mut doc = doc_with_comp("float3 ret = vec3(0.0);");
        doc.version = 1;
        doc.name = "v1-builtin-wave-base-color".to_string();
        doc.frame_eqs = "wave_r = wave_r + 0.0;".to_string();
        doc.base.wave_a = 1.0;
        doc.base.wave_r = 0.6;
        doc.base.wave_g = 0.0;
        doc.base.wave_b = 0.0;
        identity_output(&mut doc);
        assert!(doc.active_waves().is_empty(), "fixture has no custom waves");

        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let pixels = render_pixels_with_frame(&device, &queue, &runtime, doc, sine_gpu_frame());

        assert_eq!(
            pixels.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        let max_red = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .max()
            .unwrap_or(0);
        assert!(
            (145..=161).contains(&max_red),
            "built-in wave red channel must be ~153 (0.6 * 255) from the seeded base, \
             got peak red {max_red} (first 8 RGBA: {:?})",
            pixels.as_chunks::<4>().0.iter().take(8).collect::<Vec<_>>()
        );
    }

    /// The built-in waveform is drawn alongside enabled custom waves, exactly
    /// like MilkDrop's `DrawCustomWaves(); DrawWave(...)`. The custom wave is
    /// green and the built-in red, so both colours must be present.
    #[test]
    fn gpu_custom_and_builtin_waves_are_both_drawn() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_custom_and_builtin_waves_are_both_drawn: no GPU adapter in this environment"
                );
                return;
            }
        };

        let make_doc = || {
            let mut doc = builtin_wave_gpu_doc();
            doc.name = "custom-and-builtin".to_string();
            doc.base.wave_mode = 7;
            doc.waves[0].enabled = true;
            doc.waves[0].frame_eqs = "r = 0; g = 1; b = 0; a = 1;".to_string();
            doc.waves[0].point_eqs = "y = value1;".to_string();
            doc
        };

        let f = sine_gpu_frame();
        let cache = MilkFailureCache::new();
        let doc = make_doc();
        let mut instance = new_instance(
            compile_preset("custom-and-builtin", &doc, &cache).expect("fixture compiles"),
            7,
        );
        begin_frame(&mut instance, &f).expect("begin_frame");
        let overlays = evaluate_overlays(&mut instance, &f).expect("evaluate_overlays");
        assert_eq!(
            overlays.waves.len(),
            3,
            "one custom wave plus the two built-in mode-7 lines must all be evaluated"
        );

        // Pixel-level guard: green from the custom wave and red from the
        // built-in wave must both be visible.
        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let pixels = render_pixels_with_frame(&device, &queue, &runtime, make_doc(), f);
        assert!(
            green_dominant_count(&pixels) > 0,
            "the custom green wave must be visible"
        );
        assert!(
            red_dominant_count(&pixels) > 0,
            "the built-in red waveform must be visible alongside the custom wave"
        );
    }

    #[test]
    fn gpu_shape_overlay_enabled_changes_pixels_vs_disabled() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_shape_overlay: no GPU adapter in this environment");
                return;
            }
        };
        let mut off = doc_with_comp("float3 ret = float3(0.05, 0.05, 0.05);");
        off.name = "shape-off".to_string();
        identity_output(&mut off);
        let mut on = off.clone();
        on.name = "shape-on".to_string();
        on.shapes[0].enabled = true;
        on.shapes[0].frame_eqs =
            "r = 1; g = 1; b = 1; a = 1; r2 = 1; g2 = 1; b2 = 1; a2 = 1; sides = 6;".to_string();

        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let off_pixels = render_pixels(&device, &queue, &runtime, off);
        let on_pixels = render_pixels(&device, &queue, &runtime, on);
        assert_ne!(
            off_pixels, on_pixels,
            "enabling a bright shape must change the rendered pixels"
        );
        assert!(
            bright_count(&on_pixels) > bright_count(&off_pixels),
            "the enabled shape must produce more bright pixels (on={}, off={})",
            bright_count(&on_pixels),
            bright_count(&off_pixels)
        );
    }

    #[test]
    fn gpu_noise_sampler_comp_renders_varying_deterministic_pixels() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_noise_sampler_comp: no GPU adapter in this environment");
                return;
            }
        };
        let mut doc = doc_with_comp("float3 ret = tex2D(sampler_noise_lq, uv_orig).rgb;");
        doc.name = "noise-comp".to_string();
        identity_output(&mut doc);

        // Translation must accept the built-in noise sampler name and bind a
        // real texture to it (assert here so RED fails on an assertion, not on
        // render_fixture's internal expect).
        let cache = MilkFailureCache::new();
        let compiled = compile_preset(&doc.name, &doc, &cache);
        assert!(
            compiled.is_ok(),
            "comp shader sampling sampler_noise_lq must translate, got {compiled:?}"
        );

        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let render_once = || {
            let (
                mut instance,
                frame,
                mesh,
                _previous,
                previous_view,
                output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc.clone());
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("noise comp renders");
            read_texture(&device, &queue, &output).expect("read the output back")
        };

        let first = render_once();
        let second = render_once();
        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 RGBA target must be read back"
        );
        let distinct = first
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<HashSet<_>>();
        assert!(
            distinct.len() > 1,
            "sampling a noise texture must produce varying (non-flat) pixels, got {distinct:?}"
        );
        assert_eq!(
            first, second,
            "two renders of the same noise comp must be byte-identical"
        );
    }

    // --- MilkDrop 2 output fidelity (RED) ----------------------------------

    /// A real blur level must move energy outside the source footprint, not just
    /// copy the previous frame into `GetBlur1`. The fixture asks for a blur by
    /// setting `b1n`/`b2n`/`b3n` and having the comp sample `GetBlur1`, seeds a
    /// single white patch in `previous`, then reads blur target 0 back directly.
    #[test]
    fn gpu_blur_spreads_energy_from_bright_patch() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_blur_spreads_energy_from_bright_patch: no GPU adapter");
                return;
            }
        };

        let make_doc = || {
            let mut doc = doc_with_comp("float3 ret = GetBlur1(uv_orig).rgb;");
            doc.name = "blur-spread".to_string();
            identity_output(&mut doc);
            // Non-zero blur minima request real blur levels from the runtime.
            doc.base.b1n = 0.5;
            doc.base.b2n = 0.5;
            doc.base.b3n = 0.5;
            doc
        };

        let render_blur = || -> Vec<u8> {
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                _previous,
                _previous_view,
                _output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, make_doc());
            let previous = patch_texture(&device, &queue, WIDTH, HEIGHT, "milkdrop blur patch");
            let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("blur render");
            let blur = targets.blur_texture(0).expect("blur1 target exists");
            read_texture(&device, &queue, blur).expect("read blur1 target back")
        };

        let first = render_blur();
        let second = render_blur();
        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 blur target must be read back"
        );
        assert_eq!(
            first, second,
            "two identical blur renders must be byte-identical"
        );

        let cx = WIDTH as usize / 2;
        let cy = HEIGHT as usize / 2;
        let at = |pixels: &[u8], x: usize, y: usize| -> [u8; 3] {
            let offset = (y * WIDTH as usize + x) * 4;
            [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
        };

        // Sanity: the source patch centre is still bright.
        let centre = at(&first, cx, cy);
        assert!(
            centre[0] > 128 && centre[1] > 128 && centre[2] > 128,
            "the seed patch centre must survive the blur, got {centre:?}"
        );
        // The patch spans columns cx-3..=cx+3; a texel immediately outside it is
        // black for a plain copy but must receive energy from a real blur.
        let outside_x = cx + 4;
        let outside = at(&first, outside_x, cy);
        assert!(
            outside[0] > 0 || outside[1] > 0 || outside[2] > 0,
            "blur1 must spread energy outside the seed patch: pixel ({outside_x}, {cy}) is {outside:?} \
             (a copy-only blur target leaves it black)"
        );
    }

    /// MilkDrop 2's classic composite filters are nonlinear, not fixed-strength
    /// matches Butterchurn's composite shader: brighten = sqrt(x),
    /// darken = x^2 and solarize = 4*x*(1-x). For a solid 0.25 comp these are
    /// 0.5, 0.0625 and 0.75 -> 128 / 16 / 191 in RGBA8. Each flag must move the pixels and land on the faithful
    /// value; all flags off stays at 0.25 (64).
    #[test]
    fn gpu_post_processing_flags_use_faithful_filters() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_post_processing_flags_use_faithful_filters: no GPU adapter"
                );
                return;
            }
        };

        let render_flag = |name: &str, set: &dyn Fn(&mut MilkDocument)| -> Vec<u8> {
            let mut doc = doc_with_comp("float3 ret = float3(0.25, 0.25, 0.25);");
            doc.name = name.to_string();
            identity_output(&mut doc);
            set(&mut doc);
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            render_pixels(&device, &queue, &runtime, doc)
        };

        let baseline = render_flag("post-baseline", &|_| {});
        let brighten = render_flag("post-brighten", &|doc| doc.base.brighten = true);
        let darken = render_flag("post-darken", &|doc| doc.base.darken = true);
        let solarize = render_flag("post-solarize", &|doc| doc.base.solarize = true);

        assert_pixels_near(&baseline, [64, 64, 64], 8, "all post flags off");
        // Check the faithful values first (concise failures), then that each
        // flag moves the pixels relative to the all-off baseline.
        assert_pixels_near(&brighten, [128, 128, 128], 8, "brighten (sqrt)");
        assert_pixels_near(&darken, [16, 16, 16], 8, "darken (x^2)");
        assert_pixels_near(&solarize, [191, 191, 191], 8, "solarize (4*x*(1-x))");
        assert_ne!(brighten, baseline, "brighten must change the output pixels");
        assert_ne!(darken, baseline, "darken must change the output pixels");
        assert_ne!(solarize, baseline, "solarize must change the output pixels");
    }

    /// Guard the post-ops that are already exact: `red_blue` swaps R/B and
    /// `gammaadj > 1` must not darken any channel. (invert is covered by
    /// `gpu_blur_targets_allocated_and_invert_output_changes_pixels`.)
    #[test]
    fn gpu_red_blue_and_gamma_post_processing_remain_exact() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_red_blue_and_gamma_post_processing_remain_exact: no GPU adapter"
                );
                return;
            }
        };

        let render_flag = |name: &str, set: &dyn Fn(&mut MilkDocument)| -> Vec<u8> {
            let mut doc = doc_with_comp("float3 ret = float3(0.2, 0.4, 0.6);");
            doc.name = name.to_string();
            identity_output(&mut doc);
            set(&mut doc);
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            render_pixels(&device, &queue, &runtime, doc)
        };

        let baseline = render_flag("post-exact-base", &|_| {});
        assert_pixels_near(&baseline, [51, 102, 153], 8, "post baseline comp");
        let red_blue = render_flag("post-exact-redblue", &|doc| doc.base.red_blue = true);
        assert_pixels_near(&red_blue, [153, 102, 51], 8, "red_blue swaps R and B");
        let gamma = render_flag("post-exact-gamma", &|doc| doc.base.gammaadj = 2.0);
        assert_ne!(gamma, baseline, "gammaadj must change the output pixels");
        let (base_pixels, _) = baseline.as_chunks::<4>();
        let (gamma_pixels, _) = gamma.as_chunks::<4>();
        for (index, (before, after)) in base_pixels.iter().zip(gamma_pixels).enumerate() {
            for channel in 0..3 {
                assert!(
                    after[channel] >= before[channel],
                    "gammaadj > 1 must not darken channel {channel} at pixel {index}: \
                     {} -> {}",
                    before[channel],
                    after[channel]
                );
            }
        }
    }

    /// Feedback must be the pre-post composite, not the displayed output.
    ///
    /// Butterchurn's `prevTexture` is the warped+overlay image; the comp/gamma/
    /// filter pass writes only to the screen. Feeding the filtered output back
    /// makes gamma/darken/solarize compound into noise (the original
    /// witchcraft bug). With a solid 0.25 comp and `invert`, both frame 1 and
    /// frame 2 must show 0.75: if the 0.75 output were fed back, frame 2 would
    /// invert it back to 0.25.
    #[test]
    fn gpu_feedback_uses_the_pre_post_composite() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => return,
        };
        let mut doc = doc_with_comp("float3 ret = float3(0.25, 0.25, 0.25);");
        doc.name = "feedback-prepost".to_string();
        identity_output(&mut doc);
        doc.base.invert = true;
        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let (
            mut instance,
            frame,
            mesh,
            _previous,
            previous_view,
            _output,
            _output_view,
            mut targets,
        ) = render_fixture(&device, &queue, &runtime, doc);

        let first = test_texture(&device, WIDTH, HEIGHT, "feedback frame 1");
        let first_view = first.create_view(&wgpu::TextureViewDescriptor::default());
        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &first_view,
            )
            .expect("frame 1 renders");
        let second = test_texture(&device, WIDTH, HEIGHT, "feedback frame 2");
        let second_view = second.create_view(&wgpu::TextureViewDescriptor::default());
        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &second_view,
            )
            .expect("frame 2 renders");

        let first_pixels = read_texture(&device, &queue, &first).expect("read frame 1");
        let second_pixels = read_texture(&device, &queue, &second).expect("read frame 2");
        assert_pixels_near(&first_pixels, [191, 191, 191], 8, "frame 1 inverted comp");
        assert_pixels_near(
            &second_pixels,
            [191, 191, 191],
            8,
            "frame 2 must feed back the pre-post comp, not the inverted output",
        );
    }

    /// MilkDrop's `fDecay` is the fraction of the previous frame that survives:
    /// 0.98 means long trails, 0.1 means almost none. The default warp body
    /// applies it as `previous * fDecay`, so a higher value is brighter.
    #[test]
    fn gpu_higher_decay_keeps_more_of_the_previous_frame() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => return,
        };

        let render = |name: &str, decay: f64| -> Vec<u8> {
            let mut doc = MilkDocument {
                name: name.to_string(),
                ..Default::default()
            };
            identity_output(&mut doc);
            doc.base.decay = decay;
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                previous_texture,
                _previous_view,
                output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc);
            clear_to(&device, &queue, &previous_texture, [1.0, 1.0, 1.0, 1.0]);
            let previous_view =
                previous_texture.create_view(&wgpu::TextureViewDescriptor::default());
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("decay render");
            read_texture(&device, &queue, &output).expect("read output back")
        };

        let high = render("decay-high", 0.9);
        let low = render("decay-low", 0.1);
        assert!(
            high[0] > low[0],
            "fDecay=0.9 must retain more of the bright previous frame than 0.1: {} vs {}",
            high[0],
            low[0]
        );
    }

    /// With a default warp/comp, the previous frame reaches the output scaled
    /// by `fDecay`; rendering the same inputs twice must be byte-identical.
    #[test]
    fn gpu_decay_feedback_depends_on_previous_frame() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => return,
        };

        let render = |name: &str, decay: f64, previous: [f64; 4]| -> Vec<u8> {
            let mut doc = MilkDocument {
                name: name.to_string(),
                ..Default::default()
            };
            identity_output(&mut doc);
            doc.base.decay = decay;
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                previous_texture,
                _previous_view,
                output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc);
            clear_to(&device, &queue, &previous_texture, previous);
            let previous_view =
                previous_texture.create_view(&wgpu::TextureViewDescriptor::default());
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("decay render");
            read_texture(&device, &queue, &output).expect("read output back")
        };

        let feedback = render("decay-on", 0.5, [0.9, 0.9, 0.9, 1.0]);
        let dark_previous = render("decay-dark", 0.5, [0.0, 0.0, 0.0, 1.0]);
        assert_ne!(
            feedback, dark_previous,
            "a bright previous frame must change the output when decay > 0"
        );
        assert_eq!(
            feedback,
            render("decay-on-repeat", 0.5, [0.9, 0.9, 0.9, 1.0]),
            "feedback renders must be byte-identical for the same inputs"
        );
    }

    /// A real v1 preset has no warp/comp shader lines. It must still render a
    /// non-black, non-uniform result through the default identity shaders, and
    /// two renders of the same preset must be byte-identical.
    #[test]
    fn gpu_v1_preset_without_shaders_renders_non_black() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_v1_preset_without_shaders_renders_non_black: no GPU adapter in this environment"
                );
                return;
            }
        };

        // A non-uniform, non-black previous frame: a horizontal/vertical RGB
        // ramp. With decay < 1 the warp stage binds it as `sampler_main`, so an
        // identity warp + comp must let it show through into the output.
        let previous = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("milkdrop RED v1 previous"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = (WIDTH as usize * 4).div_ceil(align) * align;
        let mut bytes = vec![0u8; padded * HEIGHT as usize];
        for y in 0..HEIGHT as usize {
            for x in 0..WIDTH as usize {
                let offset = y * padded + x * 4;
                bytes[offset] = (x * 255 / (WIDTH as usize - 1)) as u8;
                bytes[offset + 1] = (y * 255 / (HEIGHT as usize - 1)) as u8;
                bytes[offset + 2] = 128;
                bytes[offset + 3] = 255;
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &previous,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(HEIGHT),
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());

        let render_once = || -> Vec<u8> {
            let mut doc = MilkDocument {
                version: 1,
                name: "v1-gpu-no-shaders".to_string(),
                frame_eqs: "x = x + 0.01;".to_string(),
                warp: String::new(),
                comp: String::new(),
                ..Default::default()
            };
            identity_output(&mut doc);
            // decay < 1 makes the warp pass bind `previous` (the gradient)
            // instead of the neutral dummy texture.
            doc.base.decay = 0.5;
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (mut instance, frame, mesh, _prev, _prev_view, output, output_view, mut targets) =
                render_fixture(&device, &queue, &runtime, doc);
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("a v1 preset without shader bodies must render");
            read_texture(&device, &queue, &output).expect("read output back")
        };

        let first = render_once();
        let second = render_once();

        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        assert!(
            first.iter().any(|&channel| channel != 0),
            "the output of a bodyless v1 preset must not be entirely black"
        );
        let distinct = first
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<std::collections::HashSet<_>>();
        assert!(
            distinct.len() > 1,
            "the output must be non-uniform (the previous frame must show through), got {distinct:?}"
        );
        assert_eq!(
            first, second,
            "two renders of the same bodyless v1 preset must be byte-identical"
        );
    }

    /// A real v2 comp body that references `uv` directly and reads `texsize.w`
    /// (1/height) must compile, render a non-black 160x90 output, and be
    /// deterministic across two identical renders. Skips without an adapter.
    #[test]
    fn gpu_milkdrop_v2_contract_shader_renders() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_milkdrop_v2_contract_shader_renders: no GPU adapter in this environment"
                );
                return;
            }
        };

        let render_once = || -> Vec<u8> {
            let mut doc = doc_with_comp("float3 ret = vec3(uv.x, texsize.w * 100.0, 0.2);");
            doc.name = "v2-contract".to_string();
            identity_output(&mut doc);
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                _previous,
                previous_view,
                output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc);
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("a v2 contract comp shader must compile and render");
            read_texture(&device, &queue, &output).expect("read output back")
        };

        let first = render_once();
        let second = render_once();

        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        assert!(
            first.iter().any(|&channel| channel != 0),
            "the v2 contract output must not be entirely black"
        );
        assert_eq!(
            first, second,
            "two renders of the same v2 contract shader must be byte-identical"
        );
    }

    /// Real v2 comp shaders read EEL `q` variables directly
    /// (`float3 ret = vec3(q4, 0.0, 0.0);`). The runtime must translate the
    /// shader AND upload the instance's current q value each frame so the
    /// sampled colour reflects the pool value.
    #[test]
    fn gpu_shader_using_q_variables_renders_correct_value() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_shader_using_q_variables_renders_correct_value: no GPU adapter in this environment"
                );
                return;
            }
        };

        let render_once = || -> Vec<u8> {
            let mut doc = doc_with_comp("float3 ret = vec3(q4, 0.0, 0.0);");
            doc.name = "q-uniform-comp".to_string();
            identity_output(&mut doc);
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                _previous,
                previous_view,
                output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc);
            // The instance's live pool value must reach the shader this frame.
            instance.pool.set("q4", 0.75);
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("a comp shader reading q4 must compile and render");
            read_texture(&device, &queue, &output).expect("read output back")
        };

        let first = render_once();
        let second = render_once();

        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        // q4 = 0.75 -> 0.75 * 255 = 191.25 -> 191 in Rgba8Unorm.
        assert_pixels_near(&first, [191, 0, 0], 8, "comp reading q4");
        assert_eq!(
            first, second,
            "two renders with q4 = 0.75 must be byte-identical"
        );
    }

    /// A real v2 comp body assigning `GetMain` directly to a `vec3` must
    /// compile, render a non-black 160x90 output from a non-black previous
    /// frame, and be deterministic. This is the GPU counterpart of the
    /// `vec3 b1 = GetBlur1(uvs);` pattern that currently fails WGSL validation
    /// with `InvalidStoreTypes`. Skips without an adapter.
    #[test]
    fn gpu_get_sampler_direct_vec3_assignment_renders() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_get_sampler_direct_vec3_assignment_renders: no GPU adapter in this environment"
                );
                return;
            }
        };

        let render_once = || -> Vec<u8> {
            let mut doc = doc_with_comp("vec3 c = GetMain(uv_orig); float3 ret = c;");
            doc.name = "getmain-vec3-comp".to_string();
            identity_output(&mut doc);
            // decay < 1 makes the warp pass resample `previous` into
            // `sampler_main`, which the comp's `GetMain` reads.
            doc.base.decay = 0.5;
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            let (
                mut instance,
                frame,
                mesh,
                previous,
                _previous_view,
                output,
                output_view,
                mut targets,
            ) = render_fixture(&device, &queue, &runtime, doc);
            // A non-black previous frame so the directly-assigned vec3 is
            // non-black too.
            clear_to(&device, &queue, &previous, [0.25, 0.5, 0.75, 1.0]);
            let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());
            runtime
                .render(
                    &mut instance,
                    &frame,
                    &mesh,
                    &previous_view,
                    &mut targets,
                    &output_view,
                )
                .expect("a comp assigning GetMain to vec3 must compile and render");
            read_texture(&device, &queue, &output).expect("read output back")
        };

        let first = render_once();
        let second = render_once();

        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        assert!(
            first.iter().any(|&channel| channel != 0),
            "the GetMain vec3 output must not be entirely black"
        );
        assert_eq!(
            first, second,
            "two renders of the same GetMain vec3 shader must be byte-identical"
        );
    }

    // --- Layer 5c: render() must evaluate overlays itself (RED) -------------

    /// Render a preset with the same ordinary flow the compositor uses in
    /// production: `begin_frame → evaluate_mesh → render`, with NO explicit
    /// `evaluate_overlays()` call. `explicit_evaluate` toggles whether the
    /// caller additionally evaluates overlays first; `render()` itself must
    /// behave identically either way once the wiring fix lands.
    fn render_pixels_overlay_wiring(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        runtime: &MilkRuntime,
        doc: MilkDocument,
        frame: MilkFrameInfo,
        explicit_evaluate: bool,
    ) -> Vec<u8> {
        let name = doc.name.clone();
        let compiled = compile_preset(&name, &doc, &runtime.failures).expect("fixture compiles");
        let mut instance = new_instance(compiled, 7);
        begin_frame(&mut instance, &frame).expect("begin_frame");
        let mesh = evaluate_mesh(&mut instance, &frame).expect("evaluate_mesh");

        let previous = test_texture(device, WIDTH, HEIGHT, "milkdrop overlay-wiring previous");
        clear_to(device, queue, &previous, [0.0, 0.0, 0.0, 1.0]);
        let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());
        let output = test_texture(device, WIDTH, HEIGHT, "milkdrop overlay-wiring output");
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let mut targets = MilkGpuTargets::new(device, WIDTH, HEIGHT);

        if explicit_evaluate {
            let _ = evaluate_overlays(&mut instance, &frame).expect("evaluate_overlays");
        }
        // Deliberately no evaluate_overlays() on the false path: render() is
        // the only remaining opportunity for overlays to reach the framebuffer.
        runtime
            .render(
                &mut instance,
                &frame,
                &mesh,
                &previous_view,
                &mut targets,
                &output_view,
            )
            .expect("render");
        read_texture(device, queue, &output).expect("read back")
    }

    /// One enabled custom wave whose frame equations paint solid green, over a
    /// black comp with identity output.
    fn custom_green_wave_gpu_doc() -> MilkDocument {
        let mut doc = doc_with_comp("float3 ret = vec3(0.0);");
        doc.name = "custom-green-wave-overlay-wiring".to_string();
        identity_output(&mut doc);
        doc.waves[0].enabled = true;
        doc.waves[0].frame_eqs = "r = 0; g = 1; b = 0; a = 1;".to_string();
        doc.waves[0].point_eqs = "y = value1;".to_string();
        doc
    }

    /// A v1 preset's built-in wave must reach the framebuffer through a plain
    /// `render()` call: no explicit `evaluate_overlays()` from the caller. The
    /// output must contain red-dominant wave pixels and be deterministic.
    #[test]
    fn gpu_render_evaluates_builtin_wave_without_explicit_call() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_render_evaluates_builtin_wave_without_explicit_call: no GPU adapter in this environment"
                );
                return;
            }
        };

        let render_once = || {
            let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
            render_pixels_overlay_wiring(
                &device,
                &queue,
                &runtime,
                builtin_wave_gpu_doc(),
                sine_gpu_frame(),
                false,
            )
        };

        let first = render_once();
        assert_eq!(
            first.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        assert!(
            red_dominant_count(&first) > 0,
            "render() alone must evaluate and draw the built-in red wave without an explicit evaluate_overlays() call (got {} red-dominant pixels; first 8 RGBA: {:?})",
            red_dominant_count(&first),
            first.as_chunks::<4>().0.iter().take(8).collect::<Vec<_>>()
        );
        let second = render_once();
        assert_eq!(
            first, second,
            "two renders of the same built-in wave via render() alone must be byte-identical"
        );
    }

    /// A custom wave section must likewise reach the framebuffer through a
    /// plain `render()` call with no explicit `evaluate_overlays()`.
    #[test]
    fn gpu_render_evaluates_custom_wave_without_explicit_call() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_render_evaluates_custom_wave_without_explicit_call: no GPU adapter in this environment"
                );
                return;
            }
        };

        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let pixels = render_pixels_overlay_wiring(
            &device,
            &queue,
            &runtime,
            custom_green_wave_gpu_doc(),
            sine_gpu_frame(),
            false,
        );
        assert_eq!(
            pixels.len(),
            (WIDTH * HEIGHT * 4) as usize,
            "a full 160x90 output must be read back"
        );
        assert!(
            green_dominant_count(&pixels) > 0,
            "render() alone must evaluate and draw the custom green wave without an explicit evaluate_overlays() call (got {} green-dominant pixels; first 8 RGBA: {:?})",
            green_dominant_count(&pixels),
            pixels.as_chunks::<4>().0.iter().take(8).collect::<Vec<_>>()
        );
    }

    /// Evaluating overlays explicitly before `render()` must be harmless: the
    /// resulting framebuffer must be byte-identical to letting `render()` do
    /// the evaluation itself (no missing or doubled overlay geometry).
    #[test]
    fn gpu_explicit_evaluate_then_render_is_not_double_drawn() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!(
                    "skipping gpu_explicit_evaluate_then_render_is_not_double_drawn: no GPU adapter in this environment"
                );
                return;
            }
        };

        let runtime = MilkRuntime::new(Arc::new(device.clone()), Arc::new(queue.clone()), 4);
        let with_explicit = render_pixels_overlay_wiring(
            &device,
            &queue,
            &runtime,
            builtin_wave_gpu_doc(),
            sine_gpu_frame(),
            true,
        );
        let without_explicit = render_pixels_overlay_wiring(
            &device,
            &queue,
            &runtime,
            builtin_wave_gpu_doc(),
            sine_gpu_frame(),
            false,
        );

        // Sanity: the explicitly evaluated path really does draw the wave, so
        // the equality below is about idempotence rather than two flat frames.
        assert!(
            red_dominant_count(&with_explicit) > 0,
            "the explicitly evaluated control must draw the built-in red wave"
        );
        assert_eq!(
            with_explicit, without_explicit,
            "explicit evaluate_overlays() + render() must be byte-identical to render() alone (no double-draw)"
        );
    }
}
