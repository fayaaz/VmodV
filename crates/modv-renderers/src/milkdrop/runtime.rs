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
//! # Render pipeline (Layer 5b)
//!
//! [`MilkRuntime::render`] encodes one command buffer with six full-screen
//! passes, in MilkDrop's order:
//!
//! 1. **warp** — the translated warp WGSL resamples the previous composited
//!    frame (`sampler_main`) at the mesh UVs carried in `uv_orig`.
//! 2. **blur1/2/3** — populate the three `GetBlur{1,2,3}` targets. These are
//!    currently plain texture copies; real blur kernels are a deliberate
//!    simplification deferred to the wave/shape overlay cycle.
//! 3. **comp** — the translated comp WGSL samples the warped frame and produces
//!    the composite colour.
//! 4. **output** — post-processing into the caller-supplied target:
//!    `gammaadj`, `invert` and `red_blue` are applied exactly, while
//!    `brighten`, `darken` and `solarize` (booleans in a `.mlk`) use a fixed
//!    small strength because their per-pixel `q` equations are not evaluated
//!    here. The exact arithmetic lives in `OUTPUT_FRAGMENT_WGSL`.
//!
//! [`MilkGpuTargets`] owns the five `Rgba8Unorm` intermediate textures and
//! reallocates them only when the size changes. Wave/shape overlays are deferred
//! to the next cycle, so the mesh UV grid is not uploaded and the translated
//! shaders read `uv_orig` from the uniform block instead; the wave/spectrum
//! arrays copied onto [`MilkInstance`] are not yet consumed by any pass.
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
use crate::milkdrop::preset::{MilkBaseVals, MilkDocument, extract_shader_body};
use crate::milkdrop::translate::{CompiledMilkShader, MilkShaderStage, translate_milk_shader};

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

// --- GPU pass shaders (Layer 5b) -------------------------------------------
//
// The warp/comp fragment stages come from `translate::translate_milk_shader`
// and already declare `@group(0) @binding(0)` (uniform) plus eight built-in
// sampler pairs at bindings 1..=16 (see `translate::BUILTIN_SAMPLERS`). A single
// full-screen triangle vertex stage drives every pass; the mesh UV grid is
// carried in the uniform block's `uv_orig` slot for this cycle (waves/shapes
// overlays remain a separate, later cycle).

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

/// Plain texture copy, used by the three blur passes. Real blur kernels are
/// intentionally deferred: these passes only need to populate the blur
/// targets so `GetBlur{1,2,3}` samplers have valid inputs.
const BLUR_FRAGMENT_WGSL: &str = r#"
@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
struct MilkFragIn { @location(0) uv: vec2<f32> }
@fragment
fn fs_main(input: MilkFragIn) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(src_texture, src_sampler, input.uv).rgb, 1.0);
}
"#;

/// Final output pass. Applies the documented subset of MilkDrop's post
/// ops in the canonical order: gamma adjust, brighten/darken, solarize,
/// invert, red/blue stereo swap. `brighten`/`darken`/`solarize` are booleans
/// in a `.mlk` (their per-pixel `q` equations are not evaluated here), so they
/// are applied with a small fixed strength — a deliberate simplification
/// documented in the module notes. gamma/invert/red_blue are exact.
const OUTPUT_FRAGMENT_WGSL: &str = r#"
struct MilkOutputUniforms {
    _pad0: vec4<f32>, _pad1: vec4<f32>, _pad2: vec4<f32>, _pad3: vec4<f32>,
    _pad4: vec4<f32>, _pad5: vec4<f32>, _pad6: vec4<f32>, _pad7: vec4<f32>,
    _pad8: vec4<f32>, _pad9: vec4<f32>, _pad10: vec4<f32>, _pad11: vec4<f32>,
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
struct MilkFragIn { @location(0) uv: vec2<f32> }
@fragment
fn fs_main(input: MilkFragIn) -> @location(0) vec4<f32> {
    var c = textureSample(src_texture, src_sampler, input.uv).rgb;
    if (params.gammaadj > 0.0 && params.gammaadj != 1.0) {
        c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / params.gammaadj));
    }
    if (params.brighten != 0.0) { c = c * (1.0 + params.brighten); }
    if (params.darken != 0.0) { c = c * (1.0 - params.darken); }
    if (params.solarize != 0.0) {
        c = select(c, vec3<f32>(1.0) - c, c > vec3<f32>(0.5));
    }
    if (params.invert != 0.0) { c = vec3<f32>(1.0) - c; }
    if (params.red_blue != 0.0) { c = vec3<f32>(c.b, c.g, c.r); }
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

/// Which bind-group layout a cached pipeline was built against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MilkLayout {
    /// The translated warp/comp shaders: uniform + 8 sampler pairs.
    WarpComp,
    /// The blur passes: one texture + one sampler.
    Pass,
    /// The output pass: uniform + one texture + one sampler.
    Output,
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
    sampler: Option<wgpu::Sampler>,
    dummy: Option<(wgpu::Texture, wgpu::TextureView)>,
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
            sampler: None,
            dummy: None,
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
        if self.vertex.is_none() {
            self.vertex = Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("modV MilkDrop vertex"),
                source: wgpu::ShaderSource::Wgsl(FULLSCREEN_VERTEX_WGSL.into()),
            }));
        }
        if self.milk_layout.is_none() {
            // Binding 0 is the uniform; bindings 1..=16 are eight
            // (texture, sampler) pairs in `BUILTIN_SAMPLERS` order.
            let mut entries = Vec::with_capacity(17);
            entries.push(Self::uniform_entry(0));
            for binding in 1..=16u32 {
                entries.push(if binding % 2 == 1 {
                    Self::texture_entry(binding)
                } else {
                    Self::sampler_entry(binding)
                });
            }
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
                    entries: &[Self::texture_entry(0), Self::sampler_entry(1)],
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
                    ],
                },
            ));
        }
    }

    /// Fetch a cached pipeline or build it once. Invalid WGSL is reported as a
    /// typed `Draw` error and remembered so it is not compiled again.
    ///
    /// Validation uses naga's `parse_str` rather than a wgpu error scope. The
    /// shader source is either produced by our own translator (syntactically
    /// valid by construction) or supplied at runtime by a caller; a parse
    /// failure catches the latter (e.g. an empty or truncated `main`) and keeps
    /// the failure path synchronous. Semantic faults that only wgpu can see are
    /// reported through the device's normal validation-error handling; adding
    /// an error scope here would require blocking on `pop_error_scope()` inside
    /// the cache mutex for no test-visible benefit in this cycle.
    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        key: String,
        source: &str,
        entry: &str,
        kind: MilkLayout,
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
        let vertex = self
            .vertex
            .clone()
            .ok_or_else(|| MilkRuntimeError::Draw("MilkDrop vertex module missing".into()))?;
        let fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("modV MilkDrop fragment"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("modV MilkDrop pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("modV MilkDrop pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
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
}

/// Byte stride of the `@group(0) @binding(0)` `MilkUniforms` block.
///
/// 256 is `wgpu::Limits::default().min_uniform_buffer_offset_alignment`, so the
/// same buffer can be used with dynamic offsets on every backend.
pub const MILK_UNIFORM_STRIDE: usize = 256;

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
/// This holds the MilkDrop scalars/vec2s described by
/// `milkdrop::translate::UNIFORM_MEMBERS` (globals, base vals, rand values and
/// derived quantities) for one frame. [`MilkUniforms::to_bytes`] always returns exactly
/// [`MILK_UNIFORM_STRIDE`] bytes so the buffer can be used with dynamic offsets.
pub struct MilkUniforms {
    bytes: Vec<u8>,
}

impl MilkUniforms {
    /// Build the uniform payload for `frame` from the live `instance` state.
    ///
    /// Offsets 0..=191 mirror the 32 scalars + 8 vec2s of
    /// `translate::UNIFORM_MEMBERS` (the layout the warp/comp WGSL expects).
    /// Offsets 192..=231 carry the base-val post-processing controls and
    /// deterministic per-frame rand values consumed by the output pass. Both
    /// live in one 256-byte slice so a single buffer serves every pass.
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

        // The eight vec2 members start at byte 128 (8-byte aligned).
        let size = [frame.width as f32, frame.height as f32];
        write_vec2(&mut bytes, 128, size);
        write_vec2(&mut bytes, 136, size);
        write_vec2(&mut bytes, 144, size);
        write_vec2(&mut bytes, 152, size);
        write_vec2(&mut bytes, 160, size);
        write_vec2(&mut bytes, 168, [0.5, 0.5]); // uv_orig (texture centre)
        write_vec2(&mut bytes, 176, [0.5, 0.5]); // rad
        write_vec2(&mut bytes, 184, [0.0, 0.0]); // ang

        // Output-pass controls (see OUTPUT_FRAGMENT_WGSL).
        write_f32(&mut bytes, 192, base.gammaadj as f32);
        write_f32(&mut bytes, 196, if base.brighten { 0.2 } else { 0.0 });
        write_f32(&mut bytes, 200, if base.darken { 0.2 } else { 0.0 });
        write_f32(&mut bytes, 204, if base.invert { 1.0 } else { 0.0 });
        write_f32(&mut bytes, 208, if base.solarize { 1.0 } else { 0.0 });
        write_f32(&mut bytes, 212, if base.red_blue { 1.0 } else { 0.0 });
        write_f32(&mut bytes, 216, rand_frame);
        write_f32(&mut bytes, 220, rand_preset);
        write_f32(&mut bytes, 224, base.brightness as f32);
        write_f32(&mut bytes, 228, base.decay as f32);

        Self { bytes }
    }

    /// The WGSL-aligned bytes to upload into `@group(0) @binding(0)`.
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

    /// Warp-pass target (previous frame resampled through the mesh UVs).
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
    /// The mesh UV grid is not uploaded in this cycle (waves/shapes are a
    /// separate pass); the translated shaders read `uv_orig` from the uniform
    /// block instead. The blur passes currently copy their source unchanged —
    /// real blur kernels are deferred until the wave/shape overlay cycle.
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
        // `mesh` and `cache_capacity` are intentionally unused this cycle: the
        // UV grid is deferred to the wave/shape overlay passes, and the cache
        // capacity is applied in `new`/`default` when the cache is created.
        let _ = (mesh, self.cache_capacity);

        targets.resize(device, frame.width.max(1), frame.height.max(1));

        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.ensure_common(device, queue);

        // Compile (or fetch from the bounded LRU) each pass pipeline. Warp and
        // comp are keyed by their WGSL so a preset swap invalidates the entry.
        let warp_key = format!("warp|{}", instance.compiled.warp.wgsl);
        let comp_key = format!("comp|{}", instance.compiled.comp.wgsl);
        let warp_pipeline = cache.pipeline(
            device,
            warp_key,
            &instance.compiled.warp.wgsl,
            "main",
            MilkLayout::WarpComp,
        )?;
        let comp_pipeline = cache.pipeline(
            device,
            comp_key,
            &instance.compiled.comp.wgsl,
            "main",
            MilkLayout::WarpComp,
        )?;
        let blur_pipeline = cache.pipeline(
            device,
            "blur|copy-v1".into(),
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

        let warp_bind = milk_bind_group(
            device,
            &milk_layout,
            &uniform_buffer,
            previous,
            blur_refs,
            &sampler,
            &dummy_view,
        );
        let comp_bind = milk_bind_group(
            device,
            &milk_layout,
            &uniform_buffer,
            &warp_view,
            blur_refs,
            &sampler,
            &dummy_view,
        );
        let output_bind = output_bind_group(
            device,
            &output_layout,
            &uniform_buffer,
            &comp_view,
            &sampler,
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("modV MilkDrop frame"),
        });

        // warp: resample `previous` through the (uniform) mesh UVs.
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop warp",
            &warp_view,
            &warp_pipeline,
            &warp_bind,
        );
        // blur1/2/3: populate the blur targets feeding GetBlur{1,2,3}. These are
        // plain copies in this cycle (real kernels are deferred).
        let blur1_bind = pass_bind_group(device, &pass_layout, previous, &sampler);
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop blur1",
            &blur_views[0],
            &blur_pipeline,
            &blur1_bind,
        );
        let blur2_bind = pass_bind_group(device, &pass_layout, &blur_views[0], &sampler);
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop blur2",
            &blur_views[1],
            &blur_pipeline,
            &blur2_bind,
        );
        let blur3_bind = pass_bind_group(device, &pass_layout, &blur_views[1], &sampler);
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop blur3",
            &blur_views[2],
            &blur_pipeline,
            &blur3_bind,
        );
        // comp: translated composite shader, sampling the warped frame.
        encode_fullscreen_pass(
            &mut encoder,
            "milkdrop comp",
            &comp_view,
            &comp_pipeline,
            &comp_bind,
        );
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
    let warp = translate_milk_shader(&warp_src.prelude, &warp_src.body, MilkShaderStage::Warp)
        .map_err(|e| fail(format!("warp shader: {e}")))?;

    let comp_src = extract_shader_body(&doc.comp);
    let comp = translate_milk_shader(&comp_src.prelude, &comp_src.body, MilkShaderStage::Comp)
        .map_err(|e| fail(format!("comp shader: {e}")))?;

    Ok(MilkCompiledPreset {
        doc: doc.clone(),
        init,
        frame,
        pixel,
        warp,
        comp,
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
    }
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

/// Bind the uniform block plus the eight built-in sampler pairs of a
/// translated warp/comp shader. Bindings follow `translate.rs`: binding 0 is
/// the uniform, `sampler_main` is `main_view`, blur samplers 1..3 are the
/// blur targets and every other built-in uses the 1×1 dummy texture.
fn milk_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    main_view: &wgpu::TextureView,
    blur_views: [&wgpu::TextureView; 3],
    sampler: &wgpu::Sampler,
    dummy_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    let mut entries = Vec::with_capacity(17);
    entries.push(uniform_bind_entry(uniform));
    let textures: [&wgpu::TextureView; 8] = [
        main_view,
        dummy_view,
        dummy_view,
        dummy_view,
        dummy_view,
        blur_views[0],
        blur_views[1],
        blur_views[2],
    ];
    for (index, view) in textures.iter().enumerate() {
        let base = (index as u32) * 2 + 1;
        entries.push(texture_bind(base, view));
        entries.push(sampler_bind(base + 1, sampler));
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modV MilkDrop shader bind group"),
        layout,
        entries: &entries,
    })
}

fn pass_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modV MilkDrop blur bind group"),
        layout,
        entries: &[texture_bind(0, view), sampler_bind(1, sampler)],
    })
}

fn output_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modV MilkDrop output bind group"),
        layout,
        entries: &[
            uniform_bind_entry(uniform),
            texture_bind(1, view),
            sampler_bind(2, sampler),
        ],
    })
}

fn encode_fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
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
}
