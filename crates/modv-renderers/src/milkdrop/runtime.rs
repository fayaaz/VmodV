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
//! [`MilkRuntime::render`] encodes one command buffer with six full-screen
//! passes plus an optional overlay pass, in MilkDrop's order:
//!
//! 1. **warp** — the translated warp WGSL resamples the previous composited
//!    frame (`sampler_main`) at the per-pixel `uv_orig` varying emitted by the
//!    mesh vertex stage. With `decay >= 1` there is no prior-frame feedback, so
//!    the warp stage binds a neutral (black) texture instead.
//! 2. **blur1/2/3** — populate the three `GetBlur{1,2,3}` targets with a
//!    normalized 5x5 Gaussian kernel (separable `[1,4,6,4,1]` weights) whose
//!    radius comes from the `b1n`/`b2n`/`b3n` base vals. Each level blurs the
//!    previous one, so energy spreads into neighbouring texels.
//! 3. **comp** — the translated comp WGSL samples the warped frame and produces
//!    the composite colour.
//! 4. **overlays (Layer 5c)** — when at least one wave/shape was evaluated,
//!    [`evaluate_overlays`]' results are drawn as `LineList` primitives onto
//!    the comp target, between comp and output. When there are no overlays this
//!    pass is skipped entirely, so the six-pass path is byte-for-byte unchanged.
//! 5. **output** — feedback blending (`mix(previous, current, decay)` for
//!    `0 < decay < 1`; `decay == 1` and the `decay == 0` derived-Default sentinel
//!    are passthrough) followed by post-processing into the caller-supplied
//!    target. `gammaadj`, `invert`, `red_blue`, `brighten` (`1-(1-x)^2`),
//!    `darken` (`x^2`) and `solarize` (`2*x*(1-x)`) follow MilkDrop 2's
//!    `milkdropfs.cpp`. The exact arithmetic lives in `OUTPUT_FRAGMENT_WGSL`.
//!
//! [`MilkGpuTargets`] owns the five `Rgba8Unorm` intermediate textures and
//! reallocates them only when the size changes. No mesh UV grid is uploaded:
//! the full-screen vertex stage emits an upright `uv_orig` at `@location(0)`
//! and both translated stages read it as an interpolated varying (the uniform
//! block keeps a same-offset `uv_orig_u` slot only for byte-layout stability).
//! The audio wave copied onto [`MilkInstance`] feeds the wave overlay's
//! per-point equations (see below); the spectrum bytes are not yet consumed by
//! any pass.
//!
//! # Overlay subset and error isolation (Layer 5c)
//!
//! Up to four enabled waves and four shapes are compiled once per preset and
//! evaluated per frame. This is the *primitive subset* of MilkDrop's overlay
//! engine, chosen to stay deterministic and allocation-light on the GPU path:
//!
//! * A **wave** is a `LineList` polyline over `x/y` points from its `point_eqs`;
//!   every [`WAVE_VERTEX_STRIDE`]th point is kept. A **shape** is a closed
//!   regular polygon whose `sides`/`cx`/`cy`/`radius`/`rot` come from the frame
//!   pool. Each vertex carries its own NDC position and clamped `r/g/b/a`
//!   colour; there are no textures or bind groups.
//! * Solid 1-pixel lines are drawn (no thick/dotted/textured waves). The
//!   `additive` variable selects one of two cached blend states. Per-wave
//!   variables seeded for compatibility but not consumed by this subset
//!   (`border_*`, `thick`, `usedots`, `spectrum`, the secondary colour, ...)
//!   are still written to the pool so preset equations can read them.
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
//! 3. Audio is mapped as `bass = audio.bass * 2.0`, `mid = audio.mid * 2.0`,
//!    `treb = audio.treble * 2.0`. The `*_att` followers are per-instance and
//!    persist across frames: `att += (value - att) * 0.2`, starting at `0.0`.
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
use crate::milkdrop::preset::{MilkBaseVals, MilkDocument, MilkShader, extract_shader_body};
use crate::milkdrop::q_uniforms::{MILK_Q_BINDING, Q_UNIFORM_MEMBERS};
use crate::milkdrop::translate::{CompiledMilkShader, MilkShaderStage, translate_milk_shader};

// Re-exported here so the public path `runtime::MILK_Q_STRIDE` (and the crate
// root re-export) stays unchanged; the definition lives with the shared layout.
pub use crate::milkdrop::q_uniforms::MILK_Q_STRIDE;

/// MilkDrop scales each raw audio band by two before exposing it to EEL.
const AUDIO_GAIN: f32 = 2.0;
/// Per-frame interpolation rate for the `*_att` band follower variables.
const BAND_FOLLOWER_RATE: f32 = 0.2;
/// Warp displacement is `warp * warpscale * WARP_AMOUNT_SCALE`.
const WARP_AMOUNT_SCALE: f32 = 0.01;
/// Denominators smaller than this are treated as degenerate in the warp mesh.
const WARP_EPS: f32 = 1e-6;
/// Sampling UV used when the warp arithmetic would otherwise be non-finite.
const UV_FALLBACK: f32 = 0.5;
/// Upper bound on wave sample points evaluated per section (EEL iteration cap).
const MAX_WAVE_SAMPLES: usize = 1024;
/// Sample count used when the frame carries no audio wave data.
const DEFAULT_WAVE_SAMPLES: usize = 512;
/// Draw every Nth evaluated wave point as a line segment (keeps vertex counts small).
const WAVE_VERTEX_STRIDE: usize = 4;
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

/// Final output pass. Blends `previous` into the composited frame for a
/// fractional `decay` in `(0, 1)` (`mix(previous, current, decay)`), then
/// applies MilkDrop 2's post ops in order: gamma adjust, brighten, darken,
/// solarize, invert and red/blue stereo swap. `brighten`/`darken`/`solarize`
/// are booleans in a `.mlk` (their per-pixel `q` equations are not evaluated
/// here), so the faithful fixed filters from `milkdropfs.cpp` are used:
/// `brighten = 1-(1-x)^2`, `darken = x^2`, `solarize = 2*x*(1-x)`.
const OUTPUT_FRAGMENT_WGSL: &str = r#"
struct MilkOutputUniforms {
    _pad0: vec4<f32>, _pad1: vec4<f32>, _pad2: vec4<f32>, _pad3: vec4<f32>,
    _pad4: vec4<f32>, _pad5: vec4<f32>, _pad6: vec4<f32>, _pad7: vec4<f32>,
    _pad8: vec4<f32>, _pad9: vec4<f32>, _pad10: vec4<f32>, _pad11: vec4<f32>,
    _pad12: vec4<f32>, // keep the controls clear of the ang slot (bytes 192..=199)
    gammaadj: f32,
    brighten: f32,
    darken: f32,
    invert: f32,
    solarize: f32,
    red_blue: f32,
    rand_frame: f32,
    rand_preset: f32,
    brightness: f32,
    decay: f32,
}
@group(0) @binding(0) var<uniform> params: MilkOutputUniforms;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;
@group(0) @binding(3) var prev_texture: texture_2d<f32>;
@group(0) @binding(4) var prev_sampler: sampler;
struct MilkFragIn { @location(0) uv: vec2<f32> }
@fragment
fn fs_main(input: MilkFragIn) -> @location(0) vec4<f32> {
    var c = textureSample(src_texture, src_sampler, input.uv).rgb;
    // MilkDrop feedback: for a fractional decay in (0, 1) the previous
    // composited frame bleeds into the current one via `mix`. decay == 1 is the
    // no-feedback case; decay == 0 is the derived-Default "unset" sentinel used
    // by test fixtures and is treated as passthrough too. Sample `previous`
    // unconditionally because `textureSample` needs uniform control flow.
    let previous = textureSample(prev_texture, prev_sampler, input.uv).rgb;
    if (params.decay > 0.0 && params.decay < 1.0) {
        c = mix(previous, c, clamp(params.decay, 0.0, 1.0));
    }
    if (params.gammaadj > 0.0 && params.gammaadj != 1.0) {
        c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / params.gammaadj));
    }
    // Faithful MilkDrop 2 filters (milkdropfs.cpp): brighten = 1-(1-x)^2,
    // darken = x^2 and solarize = 2*x*(1-x). The flags are booleans in a .mlk.
    if (params.brighten != 0.0) {
        c = vec3<f32>(1.0) - (vec3<f32>(1.0) - c) * (vec3<f32>(1.0) - c);
    }
    if (params.darken != 0.0) { c = c * c; }
    if (params.solarize != 0.0) { c = 2.0 * c * (vec3<f32>(1.0) - c); }
    if (params.invert != 0.0) { c = vec3<f32>(1.0) - c; }
    if (params.red_blue != 0.0) { c = vec3<f32>(c.b, c.g, c.r); }
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
                        Self::uniform_entry(0),
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
    ) -> Result<wgpu::RenderPipeline, MilkRuntimeError> {
        let key = if additive {
            "overlay|additive".to_string()
        } else {
            "overlay|alpha".to_string()
        };
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
                topology: wgpu::PrimitiveTopology::LineList,
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
}

/// Per-frame evaluated state of one wave overlay.
#[derive(Clone, Debug, Default)]
pub struct WaveOverlay {
    pub pool: VarPool,
    pub points: Vec<WavePoint>,
    pub enabled: bool,
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

/// Byte offset of the first output-pass control scalar in the shared uniform
/// buffer. This sits past byte 199 so it cannot overlap the `ang` vec2 slot of
/// the warp/comp `MilkUniforms` layout (bytes 192..=199). It is mirrored by the
/// 13 `vec4` pads that precede the controls in `OUTPUT_FRAGMENT_WGSL`'s
/// `MilkOutputUniforms` struct.
const OUTPUT_CONTROLS_OFFSET: usize = 208;

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
            base.decay as f32,               // decay
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
            base.b1n as f32,                 // blur1_min
            base.b2n as f32,                 // blur2_min
            base.b3n as f32,                 // blur3_min
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

        // Output-pass controls (see OUTPUT_FRAGMENT_WGSL). Kept past the `ang`
        // vec2 so the warp/comp and output layouts never share a byte.
        let output_controls = [
            base.gammaadj as f32,
            if base.brighten { 1.0 } else { 0.0 },
            if base.darken { 1.0 } else { 0.0 },
            if base.invert { 1.0 } else { 0.0 },
            if base.solarize { 1.0 } else { 0.0 }, // faithful 2*x*(1-x)
            if base.red_blue { 1.0 } else { 0.0 },
            rand_frame,
            rand_preset,
            base.brightness as f32,
            base.decay as f32,
        ];
        for (index, value) in output_controls.iter().enumerate() {
            write_f32(&mut bytes, OUTPUT_CONTROLS_OFFSET + index * 4, *value);
        }

        Self { bytes }
    }

    /// The WGSL-aligned bytes to upload into `@group(0) @binding(0)`.
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
/// The warp, composite and three blur passes render into these textures; the
/// final output pass writes into the caller-supplied module target. All targets
/// are `Rgba8Unorm` and are reallocated only when the size changes.
pub struct MilkGpuTargets {
    pub width: u32,
    pub height: u32,
    warp: Option<wgpu::Texture>,
    comp: Option<wgpu::Texture>,
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
            blur: [None, None, None],
        };
        targets.allocate(device);
        targets
    }

    /// Allocate (or replace) all five intermediate textures.
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

    /// Execute one frame of the warp → blur → comp → output core into `output`.
    ///
    /// `previous` is the last composited frame (bound as `sampler_main` for the
    /// warp pass) and `targets` owns the intermediate warp/comp/blur textures.
    /// A malformed shader yields [`MilkRuntimeError::Draw`] rather than a panic.
    ///
    /// `mesh`'s warped UV grid is uploaded for the warp pass, which draws the
    /// indexed mesh so `sampler_main` is sampled at each vertex's warped UV.
    /// The blur passes apply a real 5x5 Gaussian kernel whose radius is driven
    /// by the `b1n`/`b2n`/`b3n` base vals; the output pass blends the previous
    /// frame when `decay < 1`.
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
        let output_pipeline = cache.pipeline(
            device,
            "output|v1".into(),
            OUTPUT_FRAGMENT_WGSL,
            "fs_main",
            MilkLayout::Output,
        )?;

        // One 256-byte uniform slice serves every pass.
        let uniform_bytes = MilkUniforms::from_frame(instance, frame).to_bytes();
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modV MilkDrop uniform buffer"),
            size: MILK_UNIFORM_STRIDE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, &uniform_bytes);
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
        let blur_strengths = [
            instance.compiled.doc.base.b1n,
            instance.compiled.doc.base.b2n,
            instance.compiled.doc.base.b3n,
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

        // With decay >= 1 there is no prior-frame feedback: the warp stage has
        // no previous composited frame to resample, so bind the neutral 1x1
        // texture instead. This keeps the composite independent of `previous`
        // in that mode (and leaves the decay=0/0<decay<1 paths unchanged).
        let warp_main = if instance.compiled.doc.base.decay >= 1.0 {
            &dummy_view
        } else {
            previous
        };
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
            &uniform_buffer,
            &comp_view,
            previous,
            &sampler,
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("modV MilkDrop frame"),
        });

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
        // blurs the previous one (blur1 starts from `previous`), so the three
        // radii accumulate down the chain.
        let blur_sources: [&wgpu::TextureView; 3] = [previous, &blur_views[0], &blur_views[1]];
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
        // Layer 5c: draw wave/shape overlay primitives onto the comp target so
        // the output pass (which samples comp_view) sees them. Best-effort:
        // when no overlays were evaluated the existing six-pass path is exact.
        if !instance.overlay_frame.waves.is_empty() || !instance.overlay_frame.shapes.is_empty() {
            let alpha_pipeline = cache.overlay_pipeline(device, false)?;
            let additive_pipeline = cache.overlay_pipeline(device, true)?;
            encode_overlay_passes(
                device,
                queue,
                &mut encoder,
                &comp_view,
                &instance.overlay_frame,
                &alpha_pipeline,
                &additive_pipeline,
            );
        }
        // output: post-processing into the caller-supplied target.
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop output",
            output,
            &output_pipeline,
            &output_bind,
        );

        queue.submit(Some(encoder.finish()));
        Ok(())
    }
}

/// Default identity shader body for presets with no `shader_body` at all
/// (notably v1 presets, which predate per-stage HLSL). Sampling the previous
/// frame at the mesh UV makes warp and comp pass-through stages.
const DEFAULT_SHADER_BODY: &str = "float3 ret = tex2D(sampler_main, uv_orig).rgb;\n";

/// The body to translate for one stage: the preset's own body verbatim, or the
/// identity body when it is empty. v1 presets carry no `shader_body`, so without
/// this substitution `ret` would be undeclared and the translated shader would
/// fail naga validation. Sampling the previous frame keeps such stages as
/// pass-through instead.
fn effective_shader_body(shader: &MilkShader) -> &str {
    if shader.body.trim().is_empty() {
        DEFAULT_SHADER_BODY
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
        effective_shader_body(&warp_src),
        MilkShaderStage::Warp,
    )
    .map_err(|e| fail(format!("warp shader: {e}")))?;

    let comp_src = extract_shader_body(&doc.comp);
    let comp = translate_milk_shader(
        &comp_src.prelude,
        effective_shader_body(&comp_src),
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
            waves.push(WaveProgram { init, frame, point });
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
    pub frame_index: u32,
    /// Whether the init equations have run for this preset.
    pub init_done: bool,
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
    MilkInstance {
        compiled,
        pool: VarPool::with_seed(seed),
        globals: Globals::default(),
        band_att: [0.0; 3],
        frame_index: 0,
        init_done: false,
        wave: Vec::new(),
        byte_frequency_data: Vec::new(),
        wave_states: Vec::new(),
        shape_states: Vec::new(),
        overlay_frame: OverlayFrame::default(),
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
///   [`MAX_WAVE_SAMPLES`]) with `x/y/sample/value1/value2`, producing
///   [`WavePoint`]s on a copy-on-write fork of the frame pool.
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

    let samples = if frame.audio.wave.is_empty() {
        DEFAULT_WAVE_SAMPLES
    } else {
        frame.audio.wave.len().min(MAX_WAVE_SAMPLES)
    };

    let mut result = OverlayFrame::default();

    for index in 0..wave_count {
        match evaluate_one_wave(
            &instance.compiled.waves[index],
            &mut instance.wave_states[index],
            &globals,
            &frame.audio.wave,
            samples,
        ) {
            Ok(overlay) => result.waves.push(overlay),
            Err(message) => eprintln!("MilkDrop: skipping wave {index}: {message}"),
        }
    }
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

/// Extra per-wave-frame variables. `samples` is dynamic (derived from the
/// audio wave length) and is seeded separately by [`seed_wave_frame_vars`].
const WAVE_FRAME_VARS: &[(&str, f32)] = &[
    ("scaling", 1.0),
    ("smoothing", 0.0),
    ("sep", 0.0),
    ("additive", 0.0),
    ("usedots", 0.0),
    ("spectrum", 0.0),
    ("thick", 1.0),
];

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
/// runs. These mirror the subset of MilkDrop's per-wave-frame variables that a
/// preset's `frame_eqs` may reference; any value the preset assigns afterwards
/// wins.
fn seed_wave_frame_vars(pool: &mut VarPool, samples: usize) {
    seed_vars(pool, OVERLAY_COLOR_VARS);
    seed_vars(pool, WAVE_FRAME_VARS);
    pool.set("samples", samples as f32);
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
/// the point equations for each bounded sample. Any EEL failure is returned as
/// a message so the caller can isolate this section.
fn evaluate_one_wave(
    program: &WaveProgram,
    state: &mut OverlayState,
    globals: &Globals,
    audio_wave: &[f32],
    samples: usize,
) -> Result<WaveOverlay, String> {
    ensure_overlay_init(&program.init, state, globals)?;

    seed_wave_frame_vars(&mut state.pool, samples);
    run_frame(&program.frame, &mut state.pool, globals).map_err(|e| format!("frame: {e}"))?;

    // Point equations run on a fork so the frame-level pool (r/g/b/a, t1..t8,
    // samples, ...) survives unchanged for both the returned overlay and the
    // GPU colour pass. `VarPool`'s megabuf is copy-on-write, so this fork (and
    // the returned-overlay clone below) only copies the 4 MiB buffer if a
    // point equation actually writes to it — typically zero copies per frame.
    let mut point_pool = state.pool.clone();
    let mut points = Vec::with_capacity(samples);
    for index in 0..samples {
        let t = if samples > 1 {
            index as f32 / (samples - 1) as f32
        } else {
            0.5
        };
        let x = t * 2.0 - 1.0;
        let audio_value = audio_wave.get(index).copied().unwrap_or(0.0);
        point_pool.set("sample", t);
        point_pool.set("value1", audio_value);
        point_pool.set("value2", 0.0);
        run_pixel(&program.point, &mut point_pool, x, audio_value, 0.0, 0.0)
            .map_err(|e| format!("point {index}: {e}"))?;
        points.push(WavePoint {
            x: point_pool.get("x"),
            y: point_pool.get("y"),
            sample: point_pool.get("sample"),
            value1: point_pool.get("value1"),
            value2: point_pool.get("value2"),
        });
    }

    Ok(WaveOverlay {
        pool: state.pool.clone(),
        points,
        enabled: true,
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
    let audio = &frame.audio;
    let values = [
        audio.bass * AUDIO_GAIN,
        audio.mid * AUDIO_GAIN,
        audio.treble * AUDIO_GAIN,
    ];
    for (att, value) in instance.band_att.iter_mut().zip(values) {
        *att += (value - *att) * BAND_FOLLOWER_RATE;
    }

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
        bass_att: instance.band_att[0],
        mid: values[1],
        mid_att: instance.band_att[1],
        treb: values[2],
        treb_att: instance.band_att[2],
    };
    instance.frame_index = frame.frame_index;
    instance.wave = frame.audio.wave.clone();
    instance.byte_frequency_data = frame.audio.byte_frequency_data.clone();

    // (3) Per-frame equations: `run_frame` restores the captured q baseline
    // before injecting the globals, so q changes never leak into the next frame.
    run_frame(
        &instance.compiled.frame,
        &mut instance.pool,
        &instance.globals,
    )
    .map_err(|e| MilkRuntimeError::Evaluate(format!("frame equations: {e}")))?;
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
/// preset's base-val default.
///
/// Per-frame equations may override any of these variables at runtime. The
/// pool is not seeded with the base vals, so a `0.0` read means "untouched"
/// and the base value wins. For a hand-written fixture whose base vals are all
/// zero this leaves the parameter at zero (the warp math below guards it),
/// keeping the result finite and deterministic.
fn warp_param(pool: &VarPool, name: &str, base: f64) -> f32 {
    let live = pool.get(name);
    if live == 0.0 { base as f32 } else { live }
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
/// This mirrors MilkDrop's warp mesh: warp displacement, rotation, zoom (with
/// exponent), centre/offset (`cx`/`cy`/`dx`/`dy`) and per-axis stretch
/// (`sx`/`sy`). Parameter values come from the EEL pool (per-frame overrides)
/// with the preset's base vals as defaults. Every divisor is guarded so a
/// hand-written fixture with all-zero base vals still yields finite,
/// deterministic UVs; a non-finite result falls back to the texture centre.
fn warped_uv(pool: &VarPool, base: &MilkBaseVals, x: f32, y: f32, time: f32) -> [f32; 2] {
    let warp = warp_param(pool, "warp", base.warp);
    let warp_anim = warp_param(pool, "warpanimspeed", base.warpanimspeed);
    let warp_scale = warp_param(pool, "warpscale", base.warpscale);
    let t = time * warp_anim;
    let amount = warp * warp_scale * WARP_AMOUNT_SCALE;
    let wx = x + amount * (t.sin() + (2.0 * t).sin());
    let wy = y + amount * (t.cos() + (2.0 * t).cos());

    let zoom = nonzero_or(warp_param(pool, "zoom", base.zoom), 1.0);
    let zoomexp = warp_param(pool, "zoomexp", base.zoomexp);
    let zoom_factor = nonzero_or(zoom.powf(zoomexp), 1.0);

    let sx = nonzero_or(warp_param(pool, "sx", base.sx), 1.0);
    let sy = nonzero_or(warp_param(pool, "sy", base.sy), 1.0);
    let (sin_rot, cos_rot) = warp_param(pool, "rot", base.rot).sin_cos();
    let rx = wx * cos_rot - wy * sin_rot;
    let ry = wx * sin_rot + wy * cos_rot;

    let cx = warp_param(pool, "cx", base.cx);
    let cy = warp_param(pool, "cy", base.cy);
    let dx = warp_param(pool, "dx", base.dx);
    let dy = warp_param(pool, "dy", base.dy);
    let u = (rx / (sx * zoom_factor) + cx + dx) * 0.5 + 0.5;
    let v = (ry / (sy * zoom_factor) + cy + dy) * 0.5 + 0.5;
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
            | wgpu::TextureUsages::COPY_SRC,
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
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modV MilkDrop output bind group"),
        layout,
        entries: &[
            uniform_bind_entry(uniform),
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
    let color = overlay_color(&wave.pool);
    let mut previous: Option<[f32; 2]> = None;
    let mut emitted = 0usize;
    for point in wave.points.iter().step_by(WAVE_VERTEX_STRIDE) {
        let current = [point.x.clamp(-1.0, 1.0), point.y.clamp(-1.0, 1.0)];
        if let Some(start) = previous {
            out.extend_from_slice(&[start[0], start[1], color[0], color[1], color[2], color[3]]);
            out.extend_from_slice(&[
                current[0], current[1], color[0], color[1], color[2], color[3],
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
fn encode_overlay_passes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    comp_view: &wgpu::TextureView,
    overlays: &OverlayFrame,
    alpha_pipeline: &wgpu::RenderPipeline,
    additive_pipeline: &wgpu::RenderPipeline,
) {
    let mut draws: Vec<(Vec<f32>, bool)> = Vec::new();
    for wave in &overlays.waves {
        if !wave.enabled {
            continue;
        }
        let vertices = wave_overlay_vertices(wave);
        if !vertices.is_empty() {
            draws.push((vertices, wave.pool.get("additive") > 0.5));
        }
    }
    for shape in &overlays.shapes {
        if !shape.enabled {
            continue;
        }
        let vertices = shape_overlay_vertices(shape);
        if !vertices.is_empty() {
            draws.push((vertices, false));
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
    for (vertices, additive) in draws {
        let bytes = f32_bytes(&vertices);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modV MilkDrop overlay vertices"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, &bytes);
        pass.set_pipeline(if additive {
            additive_pipeline
        } else {
            alpha_pipeline
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
        MilkDocument {
            version: 2,
            name: "fixture".to_string(),
            init_eqs: init.to_string(),
            frame_eqs: frame.to_string(),
            pixel_eqs: pixel.to_string(),
            warp: warp.to_string(),
            comp: comp.to_string(),
            ..Default::default()
        }
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
                wave: vec![0.0; 1024],
                byte_frequency_data: vec![128; 1024],
            },
        }
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

        // (2) band followers: value = audio*2, att += (value - att)*0.2.
        // frame1: 0.2 / 0.1 / 0.4 ; frame2: 0.36 / 0.18 / 0.72.
        approx!(a.globals.bass_att, 0.36, "bass_att across two frames");
        approx!(a.globals.mid_att, 0.18, "mid_att across two frames");
        approx!(a.globals.treb_att, 0.72, "treb_att across two frames");

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
        approx!(instance.globals.bass, 1.0, "audio.bass * 2.0");
        approx!(instance.globals.mid, 0.5, "audio.mid * 2.0");
        approx!(instance.globals.treb, 2.0, "audio.treble * 2.0");
        approx!(instance.globals.bass_att, 0.2, "first-frame bass follower");
        approx!(instance.globals.mid_att, 0.1, "first-frame mid follower");
        approx!(
            instance.globals.treb_att,
            0.4,
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
        MilkDocument {
            version: 2,
            name: "gpu-fixture".to_string(),
            warp: IDENTITY_WARP.to_string(),
            comp: format!("shader_body {{\n{comp_body}\n}}\n"),
            ..Default::default()
        }
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
    /// multiplies (milkdropfs.cpp): brighten = 1-(1-x)^2, darken = x^2 and
    /// solarize = 2*x*(1-x). For a solid 0.25 comp these are 0.4375, 0.0625 and
    /// 0.375 -> 112 / 16 / 96 in RGBA8, whereas a fixed 0.2 multiplier yields
    /// 77 / 51 / 64. Each flag must move the pixels and land on the faithful
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
        assert_pixels_near(&brighten, [112, 112, 112], 8, "brighten (1-(1-x)^2)");
        assert_pixels_near(&darken, [16, 16, 16], 8, "darken (x^2)");
        assert_pixels_near(&solarize, [96, 96, 96], 8, "solarize (2*x*(1-x))");
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

    /// With `decay < 1` a non-zero previous frame must influence the output even
    /// when the comp shader is fixed; `decay == 1` must ignore it. Rendering the
    /// same setup twice must be byte-identical. The comp samples `sampler_main`
    /// and adds a constant so the signal is observable whether the faithful
    /// implementation applies feedback in the warp or in the output pass.
    #[test]
    fn gpu_decay_feedback_depends_on_previous_frame() {
        let (device, queue) = match gpu() {
            Some(pair) => pair,
            None => {
                eprintln!("skipping gpu_decay_feedback_depends_on_previous_frame: no GPU adapter");
                return;
            }
        };

        let render = |name: &str, decay: f64, previous: [f64; 4]| -> Vec<u8> {
            let mut doc =
                doc_with_comp("float3 ret = tex2D(sampler_main, uv_orig).rgb * 0.5 + 0.25;");
            doc.name = name.to_string();
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
        let no_feedback = render("decay-off", 1.0, [0.9, 0.9, 0.9, 1.0]);
        let dark_previous = render("decay-dark", 0.5, [0.0, 0.0, 0.0, 1.0]);
        let no_feedback_dark = render("decay-off-dark", 1.0, [0.0, 0.0, 0.0, 1.0]);

        let first_rgb = |pixels: &[u8]| -> [u8; 3] { [pixels[0], pixels[1], pixels[2]] };
        assert_ne!(
            first_rgb(&feedback),
            first_rgb(&no_feedback),
            "decay < 1 must feed the previous frame into the output: first pixel {:?} vs {:?}",
            first_rgb(&feedback),
            first_rgb(&no_feedback)
        );
        assert_ne!(
            feedback, no_feedback,
            "every pixel: decay < 1 must feed the previous frame into the output"
        );
        assert_ne!(
            feedback, dark_previous,
            "with decay < 1 a bright previous frame must change the output"
        );
        assert_eq!(
            no_feedback, no_feedback_dark,
            "decay == 1 must make the output independent of the previous frame"
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
}
