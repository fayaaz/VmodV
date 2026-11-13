//! Whole-scene shader transitions (WS-8 "Whole-scene transition").
//!
//! Ports the six native ISF transitions from
//! `src/renderer/src/application/scene-transition-defs.js` (`NATIVE_TRANSITIONS`)
//! to WGSL and runs them over two full-resolution frames: the **captured**
//! outgoing Scene (`startImage`) and the incoming Scene the compositor just
//! rendered into its main texture (`endImage`). `Fade` is deliberately absent —
//! it stays the existing per-Group alpha ghost path in [`crate::performance`],
//! and `None` stays an instant switch.
//!
//! ## Shader ports
//!
//! Every effect is the same WGSL prefix (uniform block, `start`/`end` textures,
//! clamp/linear sampler, fullscreen triangle) plus one fragment entry point.
//! The maths is ported line-for-line from the ISF bodies:
//!
//! * **Three Crossfade** — `smoothstep(0, 1, progress)` mix of the two frames.
//! * **Three Dissolve** — a 32 × 18 hash grid; a cell flips to the incoming
//!   frame when its noise enters the `progress * 1.4 - 0.2` … `+0.14` band.
//! * **Three Pixelate** — both frames are point-sampled at
//!   `max(1, floor(size / 9))` cells per axis (9 px cells) and crossfaded.
//! * **Three Glitch** — 24 horizontal bands; bands whose hash exceeds 0.72
//!   shift by up to 0.08 uv, and the incoming frame's red/blue channels are
//!   displaced along `x` by the same amount while the two frames crossfade.
//! * **Three Wipe** — a hard edge: `step(uv.x, p)` selects the incoming frame
//!   for every column left of the eased progress.
//! * **Three Circle** — an aspect-corrected radial wipe: the incoming frame
//!   fills a disc of radius `smoothstep(0, 1, progress) * 1.2` with a 0.08
//!   soft edge.
//!
//! `uv` comes from `@builtin(position) / resolution`, so both inputs are
//! sampled in output orientation (the same convention [`crate::compositor`]'s
//! present blit uses) and a shader that writes the incoming frame untouched
//! looks exactly like the Fade path at progress 1.
//!
//! ## Capture and lifecycle
//!
//! The compositor's main texture after a presented frame *is* the outgoing
//! Scene, so [`crate::compositor::Compositor::capture_transition_source`]
//! copies it once into a persistent `from` texture before the first frame of
//! the incoming Scene overwrites main. The pass keeps a **single** output-sized
//! scratch target (the JS `getTransitionRenderSize` instead caps the transition
//! at 1920 × 1080 pixels; the port renders at output size, the documented
//! trade-off being fill rate for exactness) and one pipeline per effect, built
//! lazily. Resize drops the scratch (and the compositor drops the capture);
//! reset drops the whole pass.
//!
//! The shader path composites the two Scene frames itself, so the outgoing
//! Groups are **hidden** (not faded) while it runs: they stay in the state at
//! their recorded alpha but the compositor skips them until the transition
//! completes and removes them. That keeps the Fade fallback intact if the
//! capture ever disappears, without running two animations at once.
//!
//! ## ISF / custom transitions
//!
//! Besides the six natives, any registered ISF shader the registry classifies
//! as a transition (`startImage` + `endImage` image inputs plus a `progress`
//! float, [`modv_renderers::isf::IsfDocument::filter_type`]) runs natively
//! through [`modv_renderers::isf::IsfRuntime`]. That is the same module list the
//! Scenes transition picker shows (`Scenes.vue` `transitionOptions`), so a
//! corpus/gl-transitions shader the user allowed actually plays instead of
//! silently fading.
//!
//! [`TransitionEffect::from_name`] therefore classifies a name as:
//!
//! * a native effect when it is one of [`TransitionEffect::ALL`];
//! * [`TransitionEffect::Isf`] when the registry has a transition-shaped
//!   document of that name (the document travels with the effect, so the render
//!   path never has to re-read the corpus);
//! * `None` for `Fade`/`None`, an unregistered name, or a registered module
//!   that is a filter/generator rather than a transition. Excluding `Fade`
//!   first is deliberate: the corpus ships a `Fade.fs` transition, but `Fade`
//!   must keep meaning the per-Group alpha ghost path the JS and the picker
//!   use. The caller keeps the Fade alpha path for those — the JS
//!   `fallbackToFade` behaviour — and a shader that fails to compile or draw
//!   downgrades to Fade on the same frame ([`crate::window`] calls
//!   [`crate::compositor::Compositor::prepare_scene_transition`] before the
//!   first transition frame).
//!
//! The ISF branch binds the captured `from` texture as `startImage` and the
//! incoming main texture as `endImage` (both are the premultiplied RGBA8 frames
//! the natives sample, so the shader sees exactly what the JS WebGL renderer
//! gave it), sets `progress` from the transition timeline and draws into the
//! same output-sized scratch target the natives use. Corpus transitions that
//! declare extra inputs (e.g. `strength`) take their header defaults, exactly
//! like an ISF module whose props were never touched.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use modv_engine::scene_transition_pick::{FADE_TRANSITION, NONE_TRANSITION};
use modv_render::wgpu;
use modv_renderers::isf::{
    CompiledIsf, IsfDocument, IsfFrameUniforms, IsfRuntime, IsfValue, IsfValues,
};

use crate::compositor::{OutputTexture, create_output_texture};
use crate::module_registry::ModuleRegistry;

/// The internal texture format every transition target uses (the compositor's
/// group/main format).
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// One whole-scene transition the engine can run (`scene-transition-defs.js:19-125`
/// for the natives, the ISF corpus for [`Self::Isf`]).
///
/// `Fade` and `None` are intentionally not variants: the first is the alpha
/// ghost path, the second an instant switch.
#[derive(Clone, Debug, PartialEq)]
pub enum TransitionEffect {
    /// `Three Crossfade`.
    Crossfade,
    /// `Three Dissolve`.
    Dissolve,
    /// `Three Pixelate`.
    Pixelate,
    /// `Three Glitch`.
    Glitch,
    /// `Three Wipe`.
    Wipe,
    /// `Three Circle`.
    Circle,
    /// A registered ISF shader transition (`startImage` + `endImage` +
    /// `progress`), run through the ISF runtime.
    Isf {
        /// The module name (the `.fs` file stem, `getIsfModuleName`).
        name: String,
        /// The parsed document from the registry, carried with the effect so a
        /// running transition never re-reads the corpus. Shared so the effect
        /// can be cloned per frame (the store caches one per transition).
        document: Arc<IsfDocument>,
    },
}

impl TransitionEffect {
    /// Number of native shader effects (one pipeline each).
    pub const COUNT: usize = 6;

    /// Every native effect, in `NATIVE_TRANSITION_NAMES` order
    /// (`scene-transition-defs.js:123-125`).
    pub const ALL: [Self; Self::COUNT] = [
        Self::Crossfade,
        Self::Dissolve,
        Self::Pixelate,
        Self::Glitch,
        Self::Wipe,
        Self::Circle,
    ];

    /// The native effect's index in [`Self::ALL`] (pipeline slot).
    ///
    /// `None` for [`Self::Isf`], which owns a compiled ISF pipeline instead.
    pub fn index(&self) -> Option<usize> {
        match self {
            Self::Crossfade => Some(0),
            Self::Dissolve => Some(1),
            Self::Pixelate => Some(2),
            Self::Glitch => Some(3),
            Self::Wipe => Some(4),
            Self::Circle => Some(5),
            Self::Isf { .. } => None,
        }
    }

    /// `true` when the effect is one of the six natives (`index` is `Some`).
    #[cfg(test)]
    pub fn is_native(&self) -> bool {
        self.index().is_some()
    }

    /// The ISF/native name (`scene-transition-defs.js:21`, `:31`, …).
    pub fn name(&self) -> &str {
        match self {
            Self::Crossfade => "Three Crossfade",
            Self::Dissolve => "Three Dissolve",
            Self::Pixelate => "Three Pixelate",
            Self::Glitch => "Three Glitch",
            Self::Wipe => "Three Wipe",
            Self::Circle => "Three Circle",
            Self::Isf { name, .. } => name,
        }
    }

    /// Map a native transition name to its effect.
    pub fn native_from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|effect| effect.name() == name)
            .cloned()
    }

    /// Map a picked transition name to a runnable effect.
    ///
    /// * one of the six native names → the matching native effect;
    /// * a name whose registered ISF document is transition-shaped
    ///   ([`IsfDocument::filter_type`] `== "transition"`) → [`Self::Isf`]
    ///   carrying the document;
    /// * `Fade`, `None`, an unknown name or a registered non-transition module
    ///   → `None`. `Fade` is checked before the corpus lookup, so a corpus
    ///   `Fade.fs` cannot hijack the alpha ghost path. The caller keeps Fade
    ///   for those, which is the JS `fallbackToFade` path (never a sudden
    ///   `None` switch).
    ///
    /// Names are matched exactly, like the JS `Map`/`meta.name` lookups.
    pub fn from_name(name: &str, registry: &ModuleRegistry) -> Option<Self> {
        if name == FADE_TRANSITION || name == NONE_TRANSITION {
            return None;
        }
        if let Some(native) = Self::native_from_name(name) {
            return Some(native);
        }
        let definition = registry.isf_definition(name)?;
        if definition.document.filter_type() != "transition" {
            return None;
        }
        Some(Self::Isf {
            name: name.to_string(),
            document: Arc::new(definition.document.clone()),
        })
    }
}

/// A whole-scene shader transition handed to the compositor for one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneTransitionInfo {
    /// The shader to run: one of the six natives, or a registered ISF
    /// transition ([`TransitionEffect::Isf`]).
    pub effect: TransitionEffect,
    /// Timeline position in `[0, 1]` (`transition.progress()`), un-eased in
    /// exactly the same way the Fade path is: every effect applies its own
    /// `smoothstep` where the ISF body did.
    pub progress: f64,
    /// Outgoing Scene Group ids. The compositor hides them while the shader
    /// owns the frame; the captured `from` texture stands in for them.
    pub from_groups: Vec<String>,
}

/// Clamp a transition progress to the `0..=1` the shaders expect.
///
/// A non-finite value becomes 0, matching the JS
/// `Math.min(1, Math.max(0, progress))` guards on a `NaN` input.
pub fn clamp_progress(progress: f64) -> f32 {
    if progress.is_finite() {
        progress.clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// The uniform block shared by every effect (`resolution`, `progress`).
///
/// 16 bytes because a `vec2<f32>` followed by an `f32` is padded to a 16-byte
/// block; the explicit `_pad` makes the layout obvious instead of implicit.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct TransitionUniforms {
    /// Output size in pixels (`RENDERSIZE`).
    pub resolution: [f32; 2],
    /// Clamped transition progress.
    pub progress: f32,
    /// Unused padding (keeps the block at 16 bytes).
    pub _pad: f32,
}

impl TransitionUniforms {
    /// The 16-byte little-endian payload (no `bytemuck` dependency here).
    pub fn to_bytes(self) -> [u8; 16] {
        let mut bytes = [0_u8; 16];
        bytes[0..4].copy_from_slice(&self.resolution[0].to_ne_bytes());
        bytes[4..8].copy_from_slice(&self.resolution[1].to_ne_bytes());
        bytes[8..12].copy_from_slice(&self.progress.to_ne_bytes());
        bytes[12..16].copy_from_slice(&self._pad.to_ne_bytes());
        bytes
    }
}

/// The declarations, vertex stage and helpers every effect shares.
const TRANSITION_PREFIX: &str = r#"// modV whole-scene transition — WGSL port of the native ISF transitions.
struct TransitionUniforms {
  resolution: vec2<f32>,
  progress: f32,
  _pad: f32,
};

@group(0) @binding(0) var<uniform> uniforms: TransitionUniforms;
@group(0) @binding(1) var start_texture: texture_2d<f32>;
@group(0) @binding(2) var end_texture: texture_2d<f32>;
@group(0) @binding(3) var transition_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
  var positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

// `@builtin(position)` is in framebuffer space (y down), so dividing by the
// output size samples both inputs in output orientation.
fn transition_uv(position: vec4<f32>) -> vec2<f32> {
  return position.xy / uniforms.resolution;
}

fn hash2(p: vec2<f32>) -> f32 {
  return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
"#;

/// `Three Crossfade` (`scene-transition-defs.js:20-30`).
const CROSSFADE_BODY: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = transition_uv(position);
  let p = smoothstep(0.0, 1.0, clamp(uniforms.progress, 0.0, 1.0));
  let outgoing = textureSample(start_texture, transition_sampler, uv);
  let incoming = textureSample(end_texture, transition_sampler, uv);
  return mix(outgoing, incoming, p);
}
"#;

/// `Three Dissolve` (`scene-transition-defs.js:32-50`).
const DISSOLVE_BODY: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = transition_uv(position);
  let cell = floor(uv * vec2<f32>(32.0, 18.0));
  let noise = hash2(cell);
  let edge = clamp(uniforms.progress, 0.0, 1.0) * 1.4 - 0.2;
  let incoming = smoothstep(edge, edge + 0.14, noise);
  let outgoing = textureSample(start_texture, transition_sampler, uv);
  let new_frame = textureSample(end_texture, transition_sampler, uv);
  return mix(outgoing, new_frame, 1.0 - incoming);
}
"#;

/// `Three Pixelate` (`scene-transition-defs.js:51-69`).
const PIXELATE_BODY: &str = r#"
fn pixel_cells(uv: vec2<f32>, size: vec2<f32>) -> vec2<f32> {
  let count = max(vec2<f32>(1.0, 1.0), floor(size / 9.0));
  return (floor(uv * count) + 0.5) / count;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = transition_uv(position);
  let from_uv = pixel_cells(uv, vec2<f32>(textureDimensions(start_texture)));
  let to_uv = pixel_cells(uv, vec2<f32>(textureDimensions(end_texture)));
  let p = smoothstep(0.0, 1.0, clamp(uniforms.progress, 0.0, 1.0));
  let outgoing = textureSample(start_texture, transition_sampler, from_uv);
  let incoming = textureSample(end_texture, transition_sampler, to_uv);
  return mix(outgoing, incoming, p);
}
"#;

/// `Three Glitch` (`scene-transition-defs.js:70-92`).
const GLITCH_BODY: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let p = clamp(uniforms.progress, 0.0, 1.0);
  let base_uv = transition_uv(position);
  let band = floor(base_uv.y * 24.0);
  let seed = floor(uniforms.progress * 12.0);
  let amount = step(0.72, hash2(vec2<f32>(band, seed))) *
    (1.0 - abs(p * 2.0 - 1.0)) * 0.08;
  let uv = vec2<f32>(
    base_uv.x + (hash2(vec2<f32>(band, seed + 1.0)) - 0.5) * amount,
    base_uv.y,
  );

  let outgoing = textureSample(start_texture, transition_sampler, uv);
  let incoming = textureSample(end_texture, transition_sampler, uv);
  let red = textureSample(
    end_texture,
    transition_sampler,
    vec2<f32>(uv.x + amount, uv.y),
  ).r;
  let blue = textureSample(
    end_texture,
    transition_sampler,
    vec2<f32>(uv.x - amount, uv.y),
  ).b;
  let displaced = vec4<f32>(red, incoming.g, blue, incoming.a);
  return mix(outgoing, displaced, smoothstep(0.0, 1.0, p));
}
"#;

/// `Three Wipe` (`scene-transition-defs.js:93-105`).
const WIPE_BODY: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = transition_uv(position);
  let p = smoothstep(0.0, 1.0, clamp(uniforms.progress, 0.0, 1.0));
  let incoming = step(uv.x, p);
  let outgoing = textureSample(start_texture, transition_sampler, uv);
  let new_frame = textureSample(end_texture, transition_sampler, uv);
  return mix(outgoing, new_frame, incoming);
}
"#;

/// `Three Circle` (`scene-transition-defs.js:106-120`).
const CIRCLE_BODY: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = transition_uv(position);
  let aspect = uniforms.resolution.x / max(uniforms.resolution.y, 1.0);
  let centred = (uv - vec2<f32>(0.5, 0.5)) * vec2<f32>(aspect, 1.0);
  let radius = smoothstep(0.0, 1.0, clamp(uniforms.progress, 0.0, 1.0)) * 1.2;
  let incoming = 1.0 - smoothstep(radius - 0.08, radius + 0.08, length(centred));
  let outgoing = textureSample(start_texture, transition_sampler, uv);
  let new_frame = textureSample(end_texture, transition_sampler, uv);
  return mix(outgoing, new_frame, incoming);
}
"#;

/// The complete WGSL program for one native effect, or `None` for an ISF
/// transition (which the ISF runtime compiles from its document instead).
pub(crate) fn shader_source(effect: &TransitionEffect) -> Option<String> {
    let body = match effect {
        TransitionEffect::Crossfade => CROSSFADE_BODY,
        TransitionEffect::Dissolve => DISSOLVE_BODY,
        TransitionEffect::Pixelate => PIXELATE_BODY,
        TransitionEffect::Glitch => GLITCH_BODY,
        TransitionEffect::Wipe => WIPE_BODY,
        TransitionEffect::Circle => CIRCLE_BODY,
        TransitionEffect::Isf { .. } => return None,
    };
    Some(format!("{TRANSITION_PREFIX}{body}"))
}

/// How many ISF transitions keep a compiled pipeline at a time.
///
/// Performance mode can try every transition in the allowlist, so the cache is
/// bounded like the JS `MAX_CACHED_RENDERERS` (4) instead of growing with every
/// shader the user tries.
const MAX_ISF_TRANSITIONS: usize = 4;

/// One compiled ISF transition.
struct IsfTransition {
    compiled: CompiledIsf,
}

/// The GPU side of whole-scene shader transitions: one pipeline per native
/// effect, a compiled pipeline per ISF transition, a shared uniform buffer and a
/// single output-sized scratch target.
pub(crate) struct SceneTransitionPass {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    /// The ISF runtime that compiles and draws registered ISF transitions,
    /// against the same [`OUTPUT_FORMAT`] the natives write.
    isf_runtime: IsfRuntime,
    pipelines: [Option<wgpu::RenderPipeline>; TransitionEffect::COUNT],
    /// Native effects whose pipeline could not be built; never retried.
    failed: [bool; TransitionEffect::COUNT],
    /// Compiled ISF transitions keyed by module name, with first-use order for
    /// eviction.
    isf: HashMap<String, IsfTransition>,
    isf_order: Vec<String>,
    /// ISF transitions whose compile/draw failed; never retried.
    failed_isf: HashSet<String>,
    scratch: Option<OutputTexture>,
    /// Size of the current scratch target and uniform `resolution`.
    size: (u32, u32),
}

impl SceneTransitionPass {
    /// Build the pass against the compositor's device and queue.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("modv-scene-transition-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // Clamp like the ISF image inputs: a glitch offset that leaves the
        // frame samples the edge texel instead of wrapping to the far side.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("modv-scene-transition-sampler"),
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
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modv-scene-transition-uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            device: device.clone(),
            queue: queue.clone(),
            layout,
            sampler,
            uniforms,
            isf_runtime: IsfRuntime::new(device.clone(), queue.clone(), MAX_ISF_TRANSITIONS),
            pipelines: Default::default(),
            failed: [false; TransitionEffect::COUNT],
            isf: HashMap::new(),
            isf_order: Vec::new(),
            failed_isf: HashSet::new(),
            scratch: None,
            size: (0, 0),
        }
    }

    /// Ensure the scratch target matches `width × height`, dropping it when the
    /// output resized.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        if self.size == (width, height) && self.scratch.is_some() {
            return;
        }
        self.size = (width, height);
        self.scratch = (width > 0 && height > 0).then(|| {
            create_output_texture(&self.device, "modv-scene-transition-target", width, height)
        });
    }

    /// The scratch view the compositor hands to [`Self::encode`] as its
    /// destination.
    pub(crate) fn target_view(&self) -> Option<wgpu::TextureView> {
        self.scratch.as_ref().map(|target| target.view.clone())
    }

    /// The scratch texture, so the caller can copy the result back into main.
    pub(crate) fn target_texture(&self) -> Option<wgpu::Texture> {
        self.scratch.as_ref().map(|target| target.texture.clone())
    }

    /// Drop the scratch target (resize, transition end).
    pub(crate) fn drop_scratch(&mut self) {
        self.scratch = None;
        self.size = (0, 0);
    }

    /// Pre-build `effect` before the first transition frame so a compile
    /// failure downgrades to Fade on the same frame instead of showing one
    /// unblended frame. Native effects build their WGSL pipeline here; an ISF
    /// transition compiles its document through the ISF runtime.
    pub(crate) fn prepare(&mut self, effect: &TransitionEffect) -> bool {
        match effect {
            TransitionEffect::Isf { name, document } => self.ensure_isf(name, document),
            native => self.ensure_pipeline(native),
        }
    }

    /// Run one effect into `destination`.
    ///
    /// Both inputs must be the current output size. `time` feeds the ISF frame
    /// uniforms (`TIME`); native effects ignore it.
    ///
    /// Returns `false` when the effect cannot run (pipeline compile failure,
    /// ISF compile/draw error), so the caller can fall back to Fade.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        effect: &TransitionEffect,
        from: &wgpu::TextureView,
        to: &wgpu::TextureView,
        progress: f64,
        time: f32,
        destination: &wgpu::TextureView,
    ) -> bool {
        let (width, height) = self.size;
        if width == 0 || height == 0 {
            return false;
        }
        if let TransitionEffect::Isf { name, document } = effect {
            return self.encode_isf(name, document, from, to, progress, time, destination);
        }
        if !self.ensure_pipeline(effect) {
            return false;
        }
        let Some(index) = effect.index() else {
            return false;
        };
        let Some(pipeline) = &self.pipelines[index] else {
            return false;
        };

        let uniforms = TransitionUniforms {
            resolution: [width as f32, height as f32],
            progress: clamp_progress(progress),
            _pad: 0.0,
        };
        self.queue
            .write_buffer(&self.uniforms, 0, &uniforms.to_bytes());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("modv-scene-transition-bind-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(from),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(to),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("modv-scene-transition"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: destination,
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
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
        true
    }

    /// Run a registered ISF transition (`renderers/isf/index.js` contract).
    ///
    /// The captured outgoing frame binds as `startImage`, the incoming main
    /// texture as `endImage` and the timeline position as `progress`, both in
    /// the same premultiplied RGBA8 convention the natives sample. The ISF
    /// runtime owns its pass scheduling and submits its own command buffer;
    /// the caller's copy of the scratch target into main runs after it on the
    /// same queue.
    #[allow(clippy::too_many_arguments)]
    fn encode_isf(
        &mut self,
        name: &str,
        document: &IsfDocument,
        from: &wgpu::TextureView,
        to: &wgpu::TextureView,
        progress: f64,
        time: f32,
        destination: &wgpu::TextureView,
    ) -> bool {
        if !self.ensure_isf(name, document) {
            return false;
        }
        let (width, height) = self.size;
        // ISF clears the destination itself when the final pass is implicit
        // with `CLEAR` (the default) or when a trailing buffer is blitted; only
        // a `CLEAR: false` final pass would otherwise blend onto the previous
        // transition's scratch contents.
        let needs_clear = self.isf.get(name).is_some_and(|entry| {
            entry.compiled.last_target.is_none()
                && entry.compiled.passes.last().is_some_and(|pass| !pass.clear)
        });
        if needs_clear {
            self.clear_target(destination);
        }

        let uniforms = IsfFrameUniforms {
            time,
            time_delta: 0.0,
            frame_index: 0,
            date: [0.0; 4],
        };
        let mut values = IsfValues::new();
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };
        values.insert("progress".to_string(), IsfValue::Float(progress));
        let images = [("startImage", from), ("endImage", to)];

        let Some(entry) = self.isf.get_mut(name) else {
            return false;
        };
        let result = self.isf_runtime.draw(
            &mut entry.compiled,
            destination,
            [width, height],
            uniforms,
            &values,
            &images,
            &[],
        );
        match result {
            Ok(()) => true,
            Err(error) => {
                eprintln!("engine: WARN ISF transition '{name}' failed to draw: {error}");
                self.isf.remove(name);
                self.isf_order.retain(|candidate| candidate != name);
                self.failed_isf.insert(name.to_string());
                false
            }
        }
    }

    /// Clear the scratch target before an ISF transition draws into it.
    fn clear_target(&self, destination: &wgpu::TextureView) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("modv-scene-transition-clear"),
            });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("modv-scene-transition-clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: destination,
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
        self.queue.submit(Some(encoder.finish()));
    }

    /// Compile an ISF transition on first use; remember a failure.
    ///
    /// The compiled pipeline is kept (bounded to [`MAX_ISF_TRANSITIONS`]) so a
    /// running transition does not recompile every frame, matching the JS
    /// renderer cache.
    fn ensure_isf(&mut self, name: &str, document: &IsfDocument) -> bool {
        if self.isf.contains_key(name) {
            if let Some(position) = self.isf_order.iter().position(|entry| entry == name) {
                let entry = self.isf_order.remove(position);
                self.isf_order.push(entry);
            }
            return true;
        }
        if self.failed_isf.contains(name) {
            return false;
        }
        match self.isf_runtime.compile(document, name, OUTPUT_FORMAT) {
            Ok(compiled) => {
                self.isf
                    .insert(name.to_string(), IsfTransition { compiled });
                self.isf_order.push(name.to_string());
                while self.isf_order.len() > MAX_ISF_TRANSITIONS {
                    let evicted = self.isf_order.remove(0);
                    self.isf.remove(&evicted);
                }
                true
            }
            Err(error) => {
                eprintln!("engine: WARN ISF transition '{name}' failed to compile: {error}");
                self.failed_isf.insert(name.to_string());
                false
            }
        }
    }

    /// Build the effect's pipeline on first use; remember a failure.
    fn ensure_pipeline(&mut self, effect: &TransitionEffect) -> bool {
        let Some(index) = effect.index() else {
            return false;
        };
        if self.pipelines[index].is_some() {
            return true;
        }
        if self.failed[index] {
            return false;
        }

        let Some(source) = shader_source(effect) else {
            return false;
        };
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("modv-scene-transition-shader"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("modv-scene-transition-pipeline-layout"),
                bind_group_layouts: &[Some(&self.layout)],
                immediate_size: 0,
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("modv-scene-transition-pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: OUTPUT_FORMAT,
                        // The shader produces the finished frame, including
                        // the mixed alpha; writing replaces the scratch.
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        self.pipelines[index] = Some(pipeline);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::read_back_texture;
    use modv_engine::scene_transition_pick::NATIVE_TRANSITION_NAMES;
    use modv_render::wgpu::naga;
    use modv_render::{Engine, EngineConfig};

    const WIDTH: u32 = 128;
    const HEIGHT: u32 = 72;

    /// A temp ISF corpus with one transition-shaped shader and one filter.
    ///
    /// Returns the directory (kept alive for the test) and a registry built
    /// from it, so classification runs the real corpus scan + parser.
    fn isf_registry(label: &str) -> (std::path::PathBuf, ModuleRegistry) {
        let dir = std::env::temp_dir().join(format!(
            "modv-scene-transition-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp ISF directory");
        std::fs::write(
            dir.join("Transition Probe.fs"),
            r#"/*{
  "ISFVSN": "2",
  "INPUTS": [
    { "NAME": "startImage", "TYPE": "image" },
    { "NAME": "endImage", "TYPE": "image" },
    { "NAME": "progress", "TYPE": "float", "MIN": 0, "MAX": 1, "DEFAULT": 0 }
  ]
}*/
void main() {
  gl_FragColor = mix(
    IMG_NORM_PIXEL(startImage, isf_FragNormCoord),
    IMG_NORM_PIXEL(endImage, isf_FragNormCoord),
    progress
  );
}"#,
        )
        .expect("transition shader written");
        // The corpus ships a `Fade.fs`; `Fade` must keep meaning the alpha
        // ghost path even when a transition-shaped module has that name.
        std::fs::write(
            dir.join("Fade.fs"),
            r#"/*{
  "ISFVSN": "2",
  "INPUTS": [
    { "NAME": "startImage", "TYPE": "image" },
    { "NAME": "endImage", "TYPE": "image" },
    { "NAME": "progress", "TYPE": "float", "MIN": 0, "MAX": 1, "DEFAULT": 0 }
  ]
}*/
void main() {
  gl_FragColor = mix(
    IMG_NORM_PIXEL(startImage, isf_FragNormCoord),
    IMG_NORM_PIXEL(endImage, isf_FragNormCoord),
    progress
  );
}"#,
        )
        .expect("fade shader written");
        // A filter: transition-shaped *except* for the missing `progress`, and
        // a generator with no image inputs at all.
        std::fs::write(
            dir.join("Filter Probe.fs"),
            r#"/*{
  "ISFVSN": "2",
  "INPUTS": [ { "NAME": "inputImage", "TYPE": "image" } ]
}*/
void main() {
  gl_FragColor = IMG_NORM_PIXEL(inputImage, isf_FragNormCoord);
}"#,
        )
        .expect("filter shader written");
        std::fs::write(
            dir.join("Generator Probe.fs"),
            r#"/*{ "ISFVSN": "2", "INPUTS": [] }*/
void main() {
  gl_FragColor = vec4(1.0);
}"#,
        )
        .expect("generator shader written");
        let registry = ModuleRegistry::build(Some(&dir));
        (dir, registry)
    }

    #[test]
    fn uniform_block_is_sixteen_bytes() {
        assert_eq!(std::mem::size_of::<TransitionUniforms>(), 16);
        let bytes = TransitionUniforms {
            resolution: [128.0, 72.0],
            progress: 0.5,
            _pad: 0.0,
        }
        .to_bytes();
        assert_eq!(bytes.len(), 16);
        let first = f32::from_ne_bytes(bytes[0..4].try_into().unwrap());
        let second = f32::from_ne_bytes(bytes[4..8].try_into().unwrap());
        let progress = f32::from_ne_bytes(bytes[8..12].try_into().unwrap());
        assert_eq!((first, second, progress), (128.0, 72.0, 0.5));
    }

    #[test]
    fn progress_is_clamped_to_the_unit_interval() {
        assert_eq!(clamp_progress(-3.0), 0.0);
        assert_eq!(clamp_progress(0.25), 0.25);
        assert_eq!(clamp_progress(4.0), 1.0);
        assert_eq!(clamp_progress(f64::NAN), 0.0);
        assert_eq!(clamp_progress(f64::INFINITY), 0.0);
    }

    #[test]
    fn effect_names_map_to_the_defs_module_and_fade_stays_out() {
        let registry = ModuleRegistry::builtin();
        assert_eq!(TransitionEffect::ALL.len(), NATIVE_TRANSITION_NAMES.len());
        for (effect, name) in TransitionEffect::ALL.iter().zip(NATIVE_TRANSITION_NAMES) {
            assert_eq!(effect.name(), *name);
            assert_eq!(
                TransitionEffect::from_name(name, &registry),
                Some(effect.clone())
            );
        }
        assert_eq!(
            TransitionEffect::from_name(FADE_TRANSITION, &registry),
            None
        );
        assert_eq!(
            TransitionEffect::from_name(NONE_TRANSITION, &registry),
            None
        );
        assert_eq!(
            TransitionEffect::from_name("E2E Missing Transition", &registry),
            None
        );
        // Names are matched exactly, like the JS `Map` lookup.
        assert_eq!(TransitionEffect::from_name("three wipe", &registry), None);
    }

    #[test]
    fn transition_shaped_isf_registry_entries_resolve_to_an_isf_effect() {
        let (dir, registry) = isf_registry("classify");
        let effect = TransitionEffect::from_name("Transition Probe", &registry)
            .expect("a startImage/endImage/progress shader is a transition");
        match &effect {
            TransitionEffect::Isf { name, document } => {
                assert_eq!(name, "Transition Probe");
                assert_eq!(document.filter_type(), "transition");
                assert!(document.inputs.iter().any(|input| input.name == "progress"));
            }
            other => panic!("expected an ISF effect, got {other:?}"),
        }
        assert_eq!(effect.name(), "Transition Probe");
        assert!(!effect.is_native());
        assert_eq!(effect.index(), None);
        assert_eq!(shader_source(&effect), None);

        // A registered filter or generator is *not* a transition: the caller
        // keeps the Fade alpha path instead of running it over two frames.
        assert_eq!(TransitionEffect::from_name("Filter Probe", &registry), None);
        assert_eq!(
            TransitionEffect::from_name("Generator Probe", &registry),
            None
        );
        // The corpus ships `Fade.fs`, but `Fade` stays the alpha ghost path:
        // the reserved names are checked before the registry.
        assert!(registry.isf_definition("Fade").is_some());
        assert_eq!(
            TransitionEffect::from_name(FADE_TRANSITION, &registry),
            None
        );
        // And an unregistered name has no document to run.
        assert_eq!(TransitionEffect::from_name("Nope", &registry), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_effect_program_parses_and_validates() {
        for effect in TransitionEffect::ALL {
            let source = shader_source(&effect).expect("natives have WGSL sources");
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| {
                panic!(
                    "{effect:?} does not parse:\n{}",
                    error.emit_to_string(&source)
                )
            });
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{effect:?} is not valid: {error:?}"));
        }
    }

    fn try_engine() -> Option<Engine> {
        match Engine::new(&EngineConfig {
            force_fallback_adapter: true,
        }) {
            Ok(engine) => Some(engine),
            Err(error) => {
                eprintln!("skipping scene-transition pixel test: no adapter ({error})");
                None
            }
        }
    }

    fn solid_frame(engine: &Engine, label: &str, pixel: [u8; 4]) -> wgpu::Texture {
        let texture = engine.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut data = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
        for chunk in data.as_chunks_mut::<4>().0 {
            *chunk = pixel;
        }
        engine.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        texture
    }

    fn run_effect(
        engine: &Engine,
        pass: &mut SceneTransitionPass,
        effect: &TransitionEffect,
        from: &wgpu::TextureView,
        to: &wgpu::TextureView,
        progress: f64,
    ) -> Vec<u8> {
        pass.resize(WIDTH, HEIGHT);
        let destination = pass.target_view().expect("scratch target exists");
        let mut encoder = engine
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene-transition-test"),
            });
        assert!(
            pass.encode(&mut encoder, effect, from, to, progress, 0.0, &destination),
            "{effect:?} encodes"
        );
        engine.queue.submit(Some(encoder.finish()));
        let target = pass.target_texture().expect("scratch target exists");
        read_back_texture(&engine.device, &engine.queue, &target, WIDTH, HEIGHT)
            .expect("transition target reads back")
    }

    /// Mean of one channel over an optional column range.
    fn mean_channel(pixels: &[u8], channel: usize, columns: Option<(u32, u32)>) -> f64 {
        let range = columns.unwrap_or((0, WIDTH));
        let mut total = 0.0;
        let mut count = 0_u32;
        for y in 0..HEIGHT {
            for x in range.0..range.1 {
                let index = ((y * WIDTH + x) * 4) as usize + channel;
                total += f64::from(pixels[index]);
                count += 1;
            }
        }
        total / f64::from(count)
    }

    fn assert_dominant(pixels: &[u8], red: bool, context: &str) {
        let r = mean_channel(pixels, 0, None);
        let b = mean_channel(pixels, 2, None);
        if red {
            assert!(
                r > 200.0 && b < 45.0,
                "{context}: expected a red frame (r={r:.1}, b={b:.1})"
            );
        } else {
            assert!(
                b > 200.0 && r < 45.0,
                "{context}: expected a blue frame (r={r:.1}, b={b:.1})"
            );
        }
    }

    #[test]
    fn each_effect_moves_from_red_to_blue_through_a_mix() {
        let Some(engine) = try_engine() else {
            return;
        };
        let mut pass = SceneTransitionPass::new(&engine.device, &engine.queue);
        let red = solid_frame(&engine, "transition-red", [255, 0, 0, 255]);
        let blue = solid_frame(&engine, "transition-blue", [0, 0, 255, 255]);
        let red_view = red.create_view(&wgpu::TextureViewDescriptor::default());
        let blue_view = blue.create_view(&wgpu::TextureViewDescriptor::default());

        for effect in TransitionEffect::ALL {
            let effect = &effect;
            let start = run_effect(&engine, &mut pass, effect, &red_view, &blue_view, 0.0);
            assert_dominant(&start, true, &format!("{effect:?} at progress 0"));

            let end = run_effect(&engine, &mut pass, effect, &red_view, &blue_view, 1.0);
            assert_dominant(&end, false, &format!("{effect:?} at progress 1"));

            let middle = run_effect(&engine, &mut pass, effect, &red_view, &blue_view, 0.5);
            let r = mean_channel(&middle, 0, None);
            let b = mean_channel(&middle, 2, None);
            if *effect == TransitionEffect::Crossfade
                || *effect == TransitionEffect::Pixelate
                || *effect == TransitionEffect::Circle
            {
                // Smooth blends: both frames are present in bulk.
                assert!(
                    (100.0..=156.0).contains(&r) && (100.0..=156.0).contains(&b),
                    "{effect:?} at 0.5 mixes evenly (r={r:.1}, b={b:.1})"
                );
            } else {
                // Dissolve/glitch pick per-cell or per-band: both frames cover
                // a meaningful share even though the split is not even.
                assert!(
                    r > 30.0 && b > 30.0,
                    "{effect:?} at 0.5 shows both frames (r={r:.1}, b={b:.1})"
                );
            }
        }
    }

    #[test]
    fn isf_transition_effect_moves_from_red_to_blue_through_a_mix() {
        let Some(engine) = try_engine() else {
            return;
        };
        let (dir, registry) = isf_registry("render");
        let effect = TransitionEffect::from_name("Transition Probe", &registry)
            .expect("a transition-shaped ISF shader resolves");
        let mut pass = SceneTransitionPass::new(&engine.device, &engine.queue);
        assert!(
            pass.prepare(&effect),
            "the ISF transition compiles before the first frame"
        );

        let red = solid_frame(&engine, "transition-red", [255, 0, 0, 255]);
        let blue = solid_frame(&engine, "transition-blue", [0, 0, 255, 255]);
        let red_view = red.create_view(&wgpu::TextureViewDescriptor::default());
        let blue_view = blue.create_view(&wgpu::TextureViewDescriptor::default());

        let start = run_effect(&engine, &mut pass, &effect, &red_view, &blue_view, 0.0);
        assert_dominant(&start, true, "ISF transition at progress 0");

        let end = run_effect(&engine, &mut pass, &effect, &red_view, &blue_view, 1.0);
        assert_dominant(&end, false, "ISF transition at progress 1");

        let middle = run_effect(&engine, &mut pass, &effect, &red_view, &blue_view, 0.5);
        let r = mean_channel(&middle, 0, None);
        let b = mean_channel(&middle, 2, None);
        assert!(
            (100.0..=156.0).contains(&r) && (100.0..=156.0).contains(&b),
            "the ISF crossfade mixes both frames evenly (r={r:.1}, b={b:.1})"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_uncompilable_isf_transition_fails_prepare_so_the_caller_can_fade() {
        let Some(engine) = try_engine() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!(
            "modv-scene-transition-broken-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp ISF directory");
        std::fs::write(
            dir.join("Broken Transition.fs"),
            r#"/*{
  "ISFVSN": "2",
  "INPUTS": [
    { "NAME": "startImage", "TYPE": "image" },
    { "NAME": "endImage", "TYPE": "image" },
    { "NAME": "progress", "TYPE": "float", "MIN": 0, "MAX": 1, "DEFAULT": 0 }
  ]
}*/
void main() {
  gl_FragColor = this_is_not_glsl(startImage, endImage);
}"#,
        )
        .expect("broken shader written");
        let registry = ModuleRegistry::build(Some(&dir));
        let effect = TransitionEffect::from_name("Broken Transition", &registry)
            .expect("the document is transition-shaped even though the body is broken");
        let mut pass = SceneTransitionPass::new(&engine.device, &engine.queue);
        assert!(
            !pass.prepare(&effect),
            "a shader that cannot compile must report failure so the store falls back to Fade"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wipe_splits_hard_at_the_eased_half_way_point() {
        let Some(engine) = try_engine() else {
            return;
        };
        let mut pass = SceneTransitionPass::new(&engine.device, &engine.queue);
        let red = solid_frame(&engine, "transition-red", [255, 0, 0, 255]);
        let blue = solid_frame(&engine, "transition-blue", [0, 0, 255, 255]);
        let red_view = red.create_view(&wgpu::TextureViewDescriptor::default());
        let blue_view = blue.create_view(&wgpu::TextureViewDescriptor::default());

        let middle = run_effect(
            &engine,
            &mut pass,
            &TransitionEffect::Wipe,
            &red_view,
            &blue_view,
            0.5,
        );
        let left_blue = mean_channel(&middle, 2, Some((0, WIDTH / 2)));
        let left_red = mean_channel(&middle, 0, Some((0, WIDTH / 2)));
        let right_red = mean_channel(&middle, 0, Some((WIDTH / 2, WIDTH)));
        let right_blue = mean_channel(&middle, 2, Some((WIDTH / 2, WIDTH)));
        assert!(
            left_blue > 250.0 && left_red < 5.0,
            "left half is the incoming frame (r={left_red:.1}, b={left_blue:.1})"
        );
        assert!(
            right_red > 250.0 && right_blue < 5.0,
            "right half is the outgoing frame (r={right_red:.1}, b={right_blue:.1})"
        );
    }

    #[test]
    fn circle_reveals_the_incoming_frame_from_the_centre() {
        let Some(engine) = try_engine() else {
            return;
        };
        let mut pass = SceneTransitionPass::new(&engine.device, &engine.queue);
        let red = solid_frame(&engine, "transition-red", [255, 0, 0, 255]);
        let blue = solid_frame(&engine, "transition-blue", [0, 0, 255, 255]);
        let red_view = red.create_view(&wgpu::TextureViewDescriptor::default());
        let blue_view = blue.create_view(&wgpu::TextureViewDescriptor::default());

        let middle = run_effect(
            &engine,
            &mut pass,
            &TransitionEffect::Circle,
            &red_view,
            &blue_view,
            0.5,
        );
        let centre = ((HEIGHT / 2 * WIDTH + WIDTH / 2) * 4) as usize;
        let corner = 0_usize;
        assert!(
            middle[centre + 2] > 200 && middle[centre] < 40,
            "the disc centre shows the incoming frame ({:?})",
            &middle[centre..centre + 4]
        );
        assert!(
            middle[corner] > 200 && middle[corner + 2] < 40,
            "the corner still shows the outgoing frame ({:?})",
            &middle[corner..corner + 4]
        );
    }
}
