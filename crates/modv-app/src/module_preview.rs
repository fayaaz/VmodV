//! Isolated Gallery-thumbnail rendering — the engine side of WS-14
//! ("Gallery thumbnails and previews: request a frame from Rust and blit it").
//!
//! The renderer's Gallery tiles poll
//! `window.engine.requestModulePreview({ moduleName, width, height })`, which
//! reaches the engine as a `preview/module` request. The supervisor cannot
//! render (it owns no GPU device), so it queues the request and the window loop
//! answers it through [`Compositor::render_module_preview`] after
//! `render_frame`; the reply is `{ width, height, pixels }` with `pixels` the
//! base64 of a tightly packed `width × height` RGBA8 frame.
//!
//! ## Why an isolated instance
//!
//! A thumbnail must never touch the live composition. The JS renderer keeps a
//! separate "gallery" shader/ISF context and a per-module `OffscreenCanvas`
//! (`renderers/*/index.js`, `adapter.js:296-323`); the port keeps a
//! [`ModulePreviews`] cache of standalone module instances — the same
//! arrangement as a Scene hover preview
//! ([`modv_engine::scene_preview_data::create_preview_module`]) — so a hover
//! cannot resize the live ISF pass buffers, steal a shader pipeline slot or
//! advance a live module's animation state.
//!
//! ## What one request renders
//!
//! Each registered kind takes the same route the live loop takes (see
//! [`crate::compositor`]):
//!
//! * **shader** — a slot on a dedicated [`ShaderRenderer`] (so preview slots
//!   can never collide with, or free, a live module's slot), sampling a
//!   transparent full-size input texture and writing the shader output.
//! * **2D** — a [`Canvas`] at the requested size, `init` once then
//!   `update`/`draw` per request, `submit`.
//! * **Point10** — the scene (init once), its [`AudioBridge`] and a target
//!   texture at the requested size.
//! * **ISF** — compiled once per slot through [`IsfRuntime`], drawn straight
//!   into the target.
//!
//! The 2D canvas and the Point10 scene target are then composited onto the
//! cached transparent target with the fresh module's defaults (alpha 1,
//! `normal`), which is exactly the blend the live loop applies per module.
//!
//! ISF image inputs have no group canvas to sample, so every `image` input is
//! bound to one transparent 1 × 1 dummy ([`IsfRuntime`]'s documented fallback
//! shape). A filter therefore sees a fully transparent input, which is the
//! honest "nothing underneath" thumbnail for a top-of-stack effect.
//!
//! ## Cost
//!
//! A slot is cached by `(module name, width, height)` in a small LRU
//! ([`MAX_CACHED_PREVIEWS`]), so repeated hover polls re-render only the frame:
//!
//! | kind | first request per key | later requests |
//! | --- | --- | --- |
//! | shader | WGSL pipeline creation (~ms) | full-frame draw + read-back |
//! | 2D | canvas + instance `init` | module `update`/`draw` + copy |
//! | Point10 | scene `init` (pipelines, buffers) | scene update/draw + copy |
//! | ISF | GLSL → WGSL translate + pipeline | pass(es) + read-back |
//!
//! The read-back dominates a warm request: it is a `copy_texture_to_buffer`
//! plus `Device::poll` for `width × height × 4` bytes. The shell hands the
//! whole render to [`crate::preview_worker::PreviewWorker`], which keeps at
//! most one job in flight and polls the map with [`PendingPreview`] off the
//! window thread, so even a cold hover cannot block presentation. Hover
//! previews render at 128 × 72; the Gallery tile is 108 × 76, so the lower
//! resolution is invisible at display size.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use modv_engine::engine_state::ModuleInstance;
use modv_render::wgpu;
use modv_renderers::isf::{CompiledIsf, IsfFrameUniforms, IsfRuntime};
use modv_renderers::point10::audio_bridge::{AudioBridge, ResponsivenessLevers};
use modv_renderers::point10::{Point10Scene, SceneContext};
use modv_renderers::shader::pipeline::ShaderRenderer;
use modv_renderers::two_d::{Canvas, DrawContext, ModuleContext, TwoDModule};

use crate::compositor::{
    FrameInfo, MAX_POINT10_FRAME_SECONDS, OutputTexture, PendingReadback, begin_read_back_texture,
    clear_texture, create_output_texture, encode_props, interim_band_payload, isf_values,
    isf_wall_clock_date, read_back_texture,
};
use crate::module_registry::{ModuleKind, ModuleRegistry, Point10SceneKind};
use crate::texture_sources::{ImageTextures, TextureRequest};

/// Read one Point10 scene prop for a preview.
///
/// Shares [`scene_prop_number`](crate::compositor::scene_prop_number) with the
/// live compositor: the Inspector's checkboxes (`useAgc`) send JSON booleans,
/// which `Value::as_f64` drops, so the preview silently kept the `true`
/// default while the live scene honoured the toggle.
fn preview_scene_prop(props: &Value, key: &str) -> Option<f32> {
    props
        .get(key)
        .and_then(crate::compositor::scene_prop_number)
}

/// Largest edge a preview may request (mirrors the main process'
/// `MODULE_PREVIEW_MAX_SIZE`).
pub const MAX_PREVIEW_DIMENSION: u32 = 512;

/// Number of `(module, width, height)` preview instances kept alive.
///
/// The Gallery only polls tiles inside the viewport now (~20 at once), so this
/// covers the visible set and a little scroll headroom without accumulating
/// hundreds of instances. Volumetric Point10 scenes are *not* retained (see
/// [`ModulePreviews::volumetric_frames`]).
pub const MAX_CACHED_PREVIEWS: usize = 24;

/// Rendered frames cached for the volumetric Point10 scenes.
///
/// A volumetric instance owns eight 150x150x300 `Rgba16Float` textures
/// (~432 MB). Rebuilding one per Gallery poll was the dominant engine-mode
/// stutter (18–40 ms per miss while the 4-slot cache thrashed). A thumbnail is
/// static in the Gallery, so the first frame is cached here and the temporary
/// instance is disposed immediately.
pub const MAX_VOLUMETRIC_FRAMES: usize = 32;

/// Translated ISF pipelines kept by the preview runtime's bounded LRU.
const ISF_PIPELINE_CACHE: usize = 4;

/// Format of every preview target (the group/main format).
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Clamp one requested preview edge into `1..=MAX_PREVIEW_DIMENSION`.
pub fn clamp_preview_dimension(value: u32) -> u32 {
    value.clamp(1, MAX_PREVIEW_DIMENSION)
}

/// Fill an RGBA8 texture with the preview test card.
///
/// Effects need *something* to filter: the legacy Gallery renders a filter
/// over "nothing" and shows a blank tile. Thumbnails instead feed a colourful
/// UV gradient + checker grid (with a white border), so fisheye/CRT/bloom-style
/// effects visibly transform it. Rows are padded to the 256-byte
/// `COPY_BYTES_PER_ROW_ALIGNMENT`.
fn write_preview_pattern(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) {
    let _ = device;
    let row_bytes = (width * 4) as usize;
    let stride = row_bytes.div_ceil(256) * 256;
    let mut data = vec![0u8; stride * height as usize];
    for y in 0..height {
        for x in 0..width {
            let u = x as f32 / width.max(1) as f32;
            let v = y as f32 / height.max(1) as f32;
            let checker = ((x / 16) + (y / 16)) % 2 == 0;
            let border = x < 2 || y < 2 || x + 2 >= width || y + 2 >= height;
            let (r, g, b) = if border {
                (255, 255, 255)
            } else {
                (
                    (u * 255.0) as u8,
                    (v * 255.0) as u8,
                    if checker { 230 } else { 48 },
                )
            };
            let offset = y as usize * stride + x as usize * 4;
            data[offset..offset + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(stride as u32),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Fill the two 512 × 2 ISF audio textures with a synthetic frame so
/// audio-reactive shaders animate in a thumbnail (a silent live frame would
/// leave them black). Row 0 and 1 are identical (mono), like the live pair.
fn write_preview_audio(queue: &wgpu::Queue, wave: &wgpu::Texture, fft: &wgpu::Texture) {
    const WIDTH: u32 = 512;
    let mut wave_data = Vec::with_capacity((WIDTH * 2 * 4) as usize);
    let mut fft_data = Vec::with_capacity((WIDTH * 2 * 4) as usize);
    for x in 0..WIDTH {
        let sample = ((x as f32 / 16.0).sin() * 0.5 + 0.5) * 255.0;
        let spectrum = (255.0 * (-(x as f32) / 90.0).exp()).clamp(0.0, 255.0);
        wave_data.extend_from_slice(&[sample as u8, sample as u8, sample as u8, 255]);
        fft_data.extend_from_slice(&[spectrum as u8, spectrum as u8, spectrum as u8, 255]);
    }
    for data in [&wave_data, &fft_data] {
        // Two identical rows.
        let mut rows = Vec::with_capacity(data.len() * 2);
        rows.extend_from_slice(data);
        rows.extend_from_slice(data);
        let texture = if std::ptr::eq(data, &wave_data) {
            wave
        } else {
            fft
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rows,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(2),
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: 2,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// Why a preview request could not be answered.
///
/// Both preview kinds share this type: `preview/module` produces
/// [`PreviewError::ModuleNotFound`]/[`PreviewError::Render`], and
/// `preview/scene` adds [`PreviewError::SceneNotFound`]/
/// [`PreviewError::SceneRowEmpty`]. The renderer treats every code as "no
/// preview yet", keeps the last frame and retries on a short backoff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewError {
    /// The module name is not registered with the engine (the renderer backs
    /// off when it sees this).
    ModuleNotFound(String),
    /// The module is registered but its isolated frame could not be rendered.
    Render(String),
    /// No Scene with that id is in the store (it may have been removed or
    /// renamed since the UI last synced).
    SceneNotFound(String),
    /// The Scene exists but the requested row is out of range, holes, or has
    /// no Module the engine can render.
    SceneRowEmpty {
        /// The Scene the request referenced.
        scene_id: String,
        /// The requested grid row.
        row: usize,
    },
}

impl PreviewError {
    /// The machine-readable code sent in the IPC error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ModuleNotFound(_) => "module_not_found",
            Self::Render(_) => "preview_failed",
            Self::SceneNotFound(_) => "scene_not_found",
            Self::SceneRowEmpty { .. } => "scene_row_empty",
        }
    }

    /// The human-readable message sent in the IPC error.
    pub fn message(&self) -> String {
        match self {
            Self::ModuleNotFound(name) => {
                format!("module \"{name}\" is not registered with the engine")
            }
            Self::Render(detail) => detail.clone(),
            Self::SceneNotFound(id) => format!("no Scene with id \"{id}\" is in the engine store"),
            Self::SceneRowEmpty { scene_id, row } => {
                format!("Scene \"{scene_id}\" has no renderable Group at index {row}")
            }
        }
    }
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleNotFound(name) => write!(f, "module \"{name}\" is not registered"),
            Self::Render(detail) => write!(f, "{detail}"),
            Self::SceneNotFound(id) => write!(f, "no Scene with id \"{id}\""),
            Self::SceneRowEmpty { scene_id, row } => {
                write!(
                    f,
                    "Scene \"{scene_id}\" has no renderable Group at index {row}"
                )
            }
        }
    }
}

impl std::error::Error for PreviewError {}

/// Base64-encode bytes with the standard RFC 4648 alphabet and `=` padding.
///
/// The engine has no base64 dependency and this is the only place it is
/// needed: `preview/module` carries the RGBA8 frame as text because the control
/// socket is JSON. Padded output is what the renderer's `atob`-based decoder
/// expects.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// One cached preview instance and its target.
struct PreviewSlot {
    /// Registry name of the module (`$moduleName`).
    name: String,
    /// Target width the slot was created for.
    width: u32,
    /// Target height the slot was created for.
    height: u32,
    /// The standalone instance: its registry meta and default props are what
    /// the renderer reads, exactly like a live module.
    module: ModuleInstance,
    /// Per-kind renderer state.
    state: PreviewState,
    /// The transparent `width × height` target the finished layer lands in.
    target: OutputTexture,
    /// The current live output, scaled to the slot size (the hover preview's
    /// background). Cleared when there is no live frame.
    base: OutputTexture,
    /// The module's own layer, composited over `base` into `target`.
    scratch: OutputTexture,
    /// Frames drawn through this slot (the per-module frame counter).
    frames: u64,
}

/// Per-kind isolated renderer state.
#[allow(clippy::large_enum_variant)]
enum PreviewState {
    /// A shader pipeline slot on the dedicated renderer plus the transparent
    /// input the shader samples as `u_modVCanvas`/`iChannel0`.
    Shader {
        /// Slot index in [`ModulePreviews::shader`].
        pipeline: usize,
        /// Full-size transparent input (the empty "group canvas").
        input: OutputTexture,
    },
    /// A 2D module instance and the canvas it draws into.
    TwoD {
        /// The module implementation (owns `update`/`draw` state).
        module: Box<dyn TwoDModule>,
        /// Requested-size canvas; its texture is composited onto the target.
        canvas: Canvas,
        /// `init` runs once, on the first request.
        initialised: bool,
    },
    /// A Point10 scene, its bridge and its target texture.
    Point10 {
        /// The concrete scene wrapper (needs the inherent prop binder).
        scene: Point10SceneKind,
        /// Device/queue clones plus the shared quad, sized to the request.
        context: SceneContext,
        /// Per-instance audio bridge (lever state is per module in the JS).
        bridge: AudioBridge,
        /// The scene's own render target, composited onto the slot target.
        scratch: OutputTexture,
        /// Clock of the previous request, for the scene's `dt`.
        last_time: Option<f32>,
    },
    /// A compiled ISF shader; it draws straight into the slot target.
    Isf {
        /// The compiled passes, uniforms and buffers.
        compiled: CompiledIsf,
        /// Full-size test card bound to every `image` input.
        input: OutputTexture,
        /// Synthetic `audio`/`audioFFT` pair when the shader declares them.
        audio: Option<(OutputTexture, OutputTexture)>,
        /// Clock of the previous request, for `TIMEDELTA`.
        last_time: Option<f32>,
    },
}

impl PreviewSlot {
    /// Whether this slot serves `name` at the requested size.
    fn matches(&self, name: &str, width: u32, height: u32) -> bool {
        self.name == name && self.width == width && self.height == height
    }

    /// Release everything the instance owns.
    ///
    /// The shader slot, the 2D instance and the Point10 scene all own GPU
    /// resources that must be freed explicitly (a dropped handle alone does
    /// not free a pipeline slot, and the scenes/modules are API-disposable).
    fn dispose(&mut self, shader: &mut ShaderRenderer) {
        match &mut self.state {
            PreviewState::Shader { pipeline, .. } => {
                shader.remove_slot(*pipeline);
            }
            PreviewState::TwoD { module, .. } => module.dispose(),
            PreviewState::Point10 { scene, .. } => scene.dispose(),
            PreviewState::Isf { .. } => {}
        }
    }
}

/// The isolated thumbnail renderer, owned by the compositor.
/// A preview whose pixels are on their way back from the GPU.
///
/// The read-back is started when the preview renders and drained with
/// [`Self::try_finish`] on later frames, so a hover never blocks the output
/// frame on a `map`/`poll`.
pub struct PendingPreview {
    readback: Option<PendingReadback>,
    ready: Option<Vec<u8>>,
    /// Composite over the live output (opaque alpha) instead of a thumbnail.
    flatten: bool,
}

impl PendingPreview {
    pub(crate) fn reading(readback: PendingReadback, flatten: bool) -> Self {
        Self {
            readback: Some(readback),
            ready: None,
            flatten,
        }
    }

    pub(crate) fn ready(pixels: Vec<u8>, flatten: bool) -> Self {
        Self {
            readback: None,
            ready: Some(pixels),
            flatten,
        }
    }

    /// Drive the map callback once. `Ok(None)` means the pixels are not ready
    /// yet; call again later.
    pub fn try_finish(&mut self) -> Result<Option<Vec<u8>>, PreviewError> {
        if let Some(pixels) = self.ready.take() {
            return Ok(Some(self.finish(pixels)));
        }
        let Some(readback) = self.readback.as_mut() else {
            return Ok(None);
        };
        let Some(pixels) = readback.try_finish().map_err(PreviewError::Render)? else {
            return Ok(None);
        };
        self.readback = None;
        Ok(Some(self.finish(pixels)))
    }

    /// Block until the preview is complete (tests and synchronous callers).
    pub fn wait(mut self) -> Result<Vec<u8>, PreviewError> {
        if let Some(pixels) = self.ready.take() {
            return Ok(self.finish(pixels));
        }
        let pixels = self
            .readback
            .take()
            .ok_or_else(|| PreviewError::Render("preview has no read-back".to_string()))?
            .wait()
            .map_err(PreviewError::Render)?;
        Ok(self.finish(pixels))
    }

    fn finish(&self, mut pixels: Vec<u8>) -> Vec<u8> {
        if self.flatten {
            // The main frame is premultiplied and presented over opaque black;
            // a hover preview is flattened the same way so a translucent tile
            // does not darken every coloured pixel.
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel[3] = 255;
            }
        }
        pixels
    }
}

pub struct ModulePreviews {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Dedicated pipeline-slot owner, so preview shaders never share indices
    /// with the live renderer.
    shader: ShaderRenderer,
    /// Translated ISF pipeline cache for preview shaders.
    isf_runtime: IsfRuntime,
    /// Repeat/Linear sampler, matching the live group sampler
    /// (`renderers/shader/index.js:271-280`).
    sampler: wgpu::Sampler,
    /// Scaled copy of the live output into a slot's `base` texture.
    scale_pipeline: wgpu::RenderPipeline,
    scale_layout: wgpu::BindGroupLayout,
    /// Draws a slot's module layer over its `base` into `target`.
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_layout: wgpu::BindGroupLayout,
    /// Cache, most recently used first.
    slots: Vec<PreviewSlot>,
    /// Static thumbnails for the volumetric Point10 scenes, keyed by
    /// `(module name, width, height)`. Their instances are never retained.
    volumetric_frames: HashMap<(String, u32, u32), Vec<u8>>,
    /// Decoded image assets for ISF image inputs. A preview has no group
    /// canvas, media source or module output, so an assigned image is the only
    /// texture prop a thumbnail can honour.
    images: ImageTextures,
}

impl ModulePreviews {
    /// Build the isolated renderer (one extra shader renderer and one ISF
    /// runtime; the per-slot test cards are output-sized).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = ShaderRenderer::new(device.clone(), queue.clone(), TARGET_FORMAT);
        let isf_runtime = IsfRuntime::new(device.clone(), queue.clone(), ISF_PIPELINE_CACHE);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("modv-preview-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });
        let (scale_pipeline, scale_layout) = create_scale_pipeline(device);
        let (overlay_pipeline, overlay_layout) = create_overlay_pipeline(device);

        Self {
            device: device.clone(),
            queue: queue.clone(),
            shader,
            isf_runtime,
            sampler,
            scale_pipeline,
            scale_layout,
            overlay_pipeline,
            overlay_layout,
            slots: Vec::new(),
            volumetric_frames: HashMap::new(),
            images: ImageTextures::new(),
        }
    }

    /// Point the preview image registry at the project media directory.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_media_root(&mut self, root: impl Into<std::path::PathBuf>) {
        self.images.set_media_root(root);
    }

    /// Drop every cached instance, pipeline and target.
    pub fn reset(&mut self) {
        let shader = &mut self.shader;
        for mut slot in self.slots.drain(..) {
            slot.dispose(shader);
        }
        // Translated pipelines are keyed by shader source and never expire;
        // a restored state can reference different modules, so rebuild the
        // runtime exactly like the compositor's live one.
        self.isf_runtime =
            IsfRuntime::new(self.device.clone(), self.queue.clone(), ISF_PIPELINE_CACHE);
        self.volumetric_frames.clear();
    }

    /// Number of cached `(module, size)` instances (test/diagnostic use).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn cached_slots(&self) -> usize {
        self.slots.len()
    }

    /// Render one frame of `module_name` at `width × height` and return
    /// tightly packed RGBA8 pixels.
    ///
    /// The size is clamped with [`clamp_preview_dimension`], so an out-of-range
    /// request never allocates or reads back more than
    /// [`MAX_PREVIEW_DIMENSION`]².
    pub fn render(
        &mut self,
        registry: &ModuleRegistry,
        module_name: &str,
        width: u32,
        height: u32,
        frame: &FrameInfo,
        base: Option<&wgpu::TextureView>,
    ) -> Result<Vec<u8>, PreviewError> {
        self.render_pending(registry, module_name, width, height, frame, base)?
            .wait()
    }

    /// Render one frame into the slot's GPU texture and return its view.
    ///
    /// The browser shell registers this view with egui-wgpu instead of
    /// reading the pixels back: the native read-back path polls the device
    /// and stalls the browser's GL backend.
    pub fn render_view(
        &mut self,
        registry: &ModuleRegistry,
        module_name: &str,
        width: u32,
        height: u32,
        frame: &FrameInfo,
        base: Option<&wgpu::TextureView>,
    ) -> Result<wgpu::TextureView, PreviewError> {
        let width = clamp_preview_dimension(width);
        let height = clamp_preview_dimension(height);

        if registry.kind(module_name).is_none() {
            return Err(PreviewError::ModuleNotFound(module_name.to_string()));
        }
        if matches!(registry.kind(module_name), Some(ModuleKind::Point10))
            && Point10SceneKind::is_volumetric_name(module_name)
        {
            return Err(PreviewError::Render(
                "volumetric Point10 previews need the native read-back path".to_string(),
            ));
        }

        let index = match self
            .slots
            .iter()
            .position(|slot| slot.matches(module_name, width, height))
        {
            Some(index) => {
                if index > 0 {
                    let slot = self.slots.remove(index);
                    self.slots.insert(0, slot);
                }
                0
            }
            None => self.create_slot(registry, module_name, width, height)?,
        };
        self.draw_slot(registry, index, frame, base)?;
        Ok(self.slots[index].target.view.clone())
    }

    /// Render one frame and start its read-back without waiting for the map.
    pub fn render_pending(
        &mut self,
        registry: &ModuleRegistry,
        module_name: &str,
        width: u32,
        height: u32,
        frame: &FrameInfo,
        base: Option<&wgpu::TextureView>,
    ) -> Result<PendingPreview, PreviewError> {
        let width = clamp_preview_dimension(width);
        let height = clamp_preview_dimension(height);
        let flatten = base.is_some();

        if registry.kind(module_name).is_none() {
            return Err(PreviewError::ModuleNotFound(module_name.to_string()));
        }

        let volumetric = matches!(registry.kind(module_name), Some(ModuleKind::Point10))
            && Point10SceneKind::is_volumetric_name(module_name);
        if volumetric {
            // A cached volumetric thumbnail is a static poster frame; the
            // first one is expensive but later polls are a map lookup.
            let pixels =
                self.render_volumetric_frame(registry, module_name, width, height, frame)?;
            return Ok(PendingPreview::ready(pixels, false));
        }

        let index = match self
            .slots
            .iter()
            .position(|slot| slot.matches(module_name, width, height))
        {
            Some(index) => {
                if index > 0 {
                    let slot = self.slots.remove(index);
                    self.slots.insert(0, slot);
                }
                0
            }
            None => self.create_slot(registry, module_name, width, height)?,
        };

        self.draw_slot(registry, index, frame, base)?;

        let slot = &self.slots[index];
        let readback = begin_read_back_texture(
            &self.device,
            &self.queue,
            &slot.target.texture,
            width,
            height,
        );
        Ok(PendingPreview::reading(readback, flatten))
    }

    /// Render (once) and cache a volumetric Point10 thumbnail.
    ///
    /// The temporary slot is disposed before returning, so the 432 MB volume
    /// textures never stay resident per Gallery tile. Repeated polls are a map
    /// lookup; the thumbnail is intentionally static, like a poster frame.
    fn render_volumetric_frame(
        &mut self,
        registry: &ModuleRegistry,
        module_name: &str,
        width: u32,
        height: u32,
        frame: &FrameInfo,
    ) -> Result<Vec<u8>, PreviewError> {
        let key = (module_name.to_string(), width, height);
        if let Some(pixels) = self.volumetric_frames.get(&key) {
            return Ok(pixels.clone());
        }

        let index = self.create_slot(registry, module_name, width, height)?;
        // The volumetric thumbnail is a cached static poster, so it stays
        // isolated from the moving live output.
        self.draw_slot(registry, index, frame, None)?;
        let pixels = read_back_texture(
            &self.device,
            &self.queue,
            &self.slots[index].target.texture,
            width,
            height,
        )
        .map_err(PreviewError::Render)?;

        // Drop the huge instance immediately; only the RGBA8 frame is kept.
        if let Some(position) = self
            .slots
            .iter()
            .position(|slot| slot.matches(module_name, width, height))
        {
            let mut slot = self.slots.remove(position);
            slot.dispose(&mut self.shader);
        }

        if self.volumetric_frames.len() >= MAX_VOLUMETRIC_FRAMES {
            self.volumetric_frames.clear();
        }
        self.volumetric_frames.insert(key, pixels.clone());
        Ok(pixels)
    }

    /// Build (and front-insert) a slot for `module_name`, evicting the least
    /// recently used entry when the cache is full.
    fn create_slot(
        &mut self,
        registry: &ModuleRegistry,
        module_name: &str,
        width: u32,
        height: u32,
    ) -> Result<usize, PreviewError> {
        if self.slots.len() >= MAX_CACHED_PREVIEWS
            && let Some(mut oldest) = self.slots.pop()
        {
            oldest.dispose(&mut self.shader);
        }

        let module = build_preview_module(registry, module_name)?;
        let kind = registry
            .kind(module_name)
            .ok_or_else(|| PreviewError::ModuleNotFound(module_name.to_string()))?;

        let target = create_output_texture(
            &self.device,
            &format!("modv-preview-target-{module_name}"),
            width,
            height,
        );
        let base = create_output_texture(
            &self.device,
            &format!("modv-preview-base-{module_name}"),
            width,
            height,
        );
        let scratch = create_output_texture(
            &self.device,
            &format!("modv-preview-scratch-{module_name}"),
            width,
            height,
        );
        for texture in [&target, &base, &scratch] {
            clear_texture(
                &self.device,
                &self.queue,
                &texture.texture,
                "preview-target-clear",
            );
        }

        let state = match kind {
            ModuleKind::Shader => {
                let definition = registry.shader_module(module_name).ok_or_else(|| {
                    PreviewError::Render(format!(
                        "shader module \"{module_name}\" is not registered"
                    ))
                })?;
                // `false`: this is a dedicated renderer with no live module,
                // so the live/gallery slot markers are left alone.
                let pipeline = self
                    .shader
                    .setup_module(definition, TARGET_FORMAT, false)
                    .map_err(|error| PreviewError::Render(error.to_string()))?;
                let input = create_output_texture(
                    &self.device,
                    &format!("modv-preview-input-{module_name}"),
                    width,
                    height,
                );
                write_preview_pattern(&self.device, &self.queue, &input.texture, width, height);
                PreviewState::Shader { pipeline, input }
            }
            ModuleKind::TwoD => {
                let instance = registry.create_two_d(module_name).ok_or_else(|| {
                    PreviewError::Render(format!(
                        "2D module \"{module_name}\" has no implementation"
                    ))
                })?;
                let canvas = Canvas::new(&self.device, &self.queue, width, height);
                PreviewState::TwoD {
                    module: instance,
                    canvas,
                    initialised: false,
                }
            }
            ModuleKind::Point10 => {
                let mut scene = registry
                    .create_point10_instance(module_name)
                    .ok_or_else(|| {
                        PreviewError::Render(format!(
                            "Point10 scene \"{module_name}\" has no implementation"
                        ))
                    })?;
                let mut context = SceneContext::new(&self.device, &self.queue, TARGET_FORMAT);
                context.resize(width, height);
                scene.init(&mut context);
                let scratch = create_output_texture(
                    &self.device,
                    &format!("modv-preview-scene-{module_name}"),
                    width,
                    height,
                );
                clear_texture(
                    &self.device,
                    &self.queue,
                    &scratch.texture,
                    "preview-scene-clear",
                );
                PreviewState::Point10 {
                    scene,
                    context,
                    bridge: AudioBridge::new(),
                    scratch,
                    last_time: None,
                }
            }
            ModuleKind::Isf => {
                let definition = registry.isf_definition(module_name).ok_or_else(|| {
                    PreviewError::Render(format!("ISF shader \"{module_name}\" is not registered"))
                })?;
                let compiled = self
                    .isf_runtime
                    .compile(&definition.document, &definition.name, TARGET_FORMAT)
                    .map_err(|error| PreviewError::Render(error.to_string()))?;
                let input = create_output_texture(
                    &self.device,
                    &format!("modv-preview-isf-input-{module_name}"),
                    width,
                    height,
                );
                write_preview_pattern(&self.device, &self.queue, &input.texture, width, height);
                let needs_audio = definition
                    .document
                    .inputs
                    .iter()
                    .any(|input| input.type_ == "audio" || input.type_ == "audioFFT");
                let audio = needs_audio.then(|| {
                    let wave = create_output_texture(
                        &self.device,
                        &format!("modv-preview-isf-wave-{module_name}"),
                        512,
                        2,
                    );
                    let fft = create_output_texture(
                        &self.device,
                        &format!("modv-preview-isf-fft-{module_name}"),
                        512,
                        2,
                    );
                    write_preview_audio(&self.queue, &wave.texture, &fft.texture);
                    (wave, fft)
                });
                PreviewState::Isf {
                    compiled,
                    input,
                    audio,
                    last_time: None,
                }
            }
            // RED stub: adding `ModuleKind::Milkdrop` makes this match
            // exhaustive again. GREEN wires a real MilkDrop preview state.
            ModuleKind::Milkdrop => {
                return Err(PreviewError::Render(format!(
                    "MilkDrop preview is not implemented yet (\"{module_name}\")"
                )));
            }
        };

        self.slots.insert(
            0,
            PreviewSlot {
                name: module_name.to_string(),
                width,
                height,
                module,
                state,
                target,
                base,
                scratch,
                frames: 0,
            },
        );
        Ok(0)
    }

    /// Draw one frame of the cached slot into its target.
    fn draw_slot(
        &mut self,
        registry: &ModuleRegistry,
        index: usize,
        frame: &FrameInfo,
        base: Option<&wgpu::TextureView>,
    ) -> Result<(), PreviewError> {
        let Self {
            device,
            queue,
            shader,
            isf_runtime,
            sampler,
            scale_pipeline,
            scale_layout,
            overlay_pipeline,
            overlay_layout,
            slots,
            images,
            ..
        } = self;

        let slot = &mut slots[index];
        let PreviewSlot {
            module,
            width,
            height,
            state,
            target,
            base: slot_base,
            scratch: slot_scratch,
            frames,
            ..
        } = slot;
        let (width, height) = (*width, *height);
        *frames += 1;
        let frame_index = *frames;
        let audio = &frame.audio;
        let props = &module.props;
        let module_name = module.module_name.as_str();

        // Scale the current live output into this slot's base, then seed the
        // target with it. The module layer is drawn into `scratch` and blended
        // over the base at the end, so a hover preview shows the new module on
        // top of what is already playing.
        if let Some(base_view) = base {
            run_scale(
                device,
                queue,
                scale_pipeline,
                scale_layout,
                sampler,
                base_view,
                &slot_base.view,
            );
            run_scale(
                device,
                queue,
                scale_pipeline,
                scale_layout,
                sampler,
                &slot_base.view,
                &target.view,
            );
        } else {
            clear_texture(device, queue, &slot_base.texture, "preview-base-clear");
            clear_texture(device, queue, &target.texture, "preview-target-clear");
        }

        // The module layer's texture; every arm writes into `scratch` (or its
        // own canvas) and it is composited over the base after the match.
        let module_texture: wgpu::Texture = match state {
            PreviewState::Shader { pipeline, input } => {
                let definition = registry.shader_module(module_name).ok_or_else(|| {
                    PreviewError::Render(format!(
                        "shader module \"{module_name}\" is not registered"
                    ))
                })?;
                let params = encode_props(definition, props);
                if let Some(base_view) = base {
                    // A filter previews over the live frame instead of the
                    // synthetic test card. Generators ignore the input.
                    run_scale(
                        device,
                        queue,
                        scale_pipeline,
                        scale_layout,
                        sampler,
                        base_view,
                        &input.view,
                    );
                } else {
                    write_preview_pattern(device, queue, &input.texture, width, height);
                }
                shader
                    .render(
                        *pipeline,
                        &input.view,
                        sampler,
                        // No FFT texture feed yet (WS-9/M6): the renderer's
                        // 1 × 1 zero dummy is bound and `u_fftResolution` is 1.
                        None,
                        None,
                        frame.shader_audio(),
                        frame.time_seconds,
                        frame_index as i32,
                        frame.date,
                        [width as f32, height as f32, 1.0],
                        1.0,
                        &params,
                        &slot_scratch.view,
                    )
                    .map_err(|error| PreviewError::Render(error.to_string()))?;
                slot_scratch.texture.clone()
            }
            PreviewState::TwoD {
                module: instance,
                canvas,
                initialised,
            } => {
                // The JS `delta` argument is the `requestAnimationFrame`
                // timestamp, i.e. a monotonic millisecond clock
                // (`renderers/2d/index.js:24`).
                let delta_ms = frame.time_seconds * 1000.0;

                if !*initialised {
                    *initialised = true;
                    let mut ctx = ModuleContext {
                        canvas_width: width,
                        canvas_height: height,
                        delta: delta_ms,
                        audio,
                        frame: 0,
                        props,
                        device,
                        queue,
                        pixel_scale: 1.0,
                    };
                    instance.init(&mut ctx);
                }

                let mut ctx = ModuleContext {
                    canvas_width: width,
                    canvas_height: height,
                    delta: delta_ms,
                    audio,
                    frame: frame_index,
                    props,
                    device,
                    queue,
                    pixel_scale: 1.0,
                };
                instance.update(&mut ctx);

                canvas.begin_frame();
                // A thumbnail starts empty every time; the canvas otherwise
                // keeps its previous frame (a live module's deliberate
                // self-sampling behaviour).
                canvas.clear_all();
                {
                    let mut draw = DrawContext {
                        canvas,
                        props,
                        audio,
                        delta: delta_ms,
                        frame: frame_index,
                        device,
                        queue,
                        // No media input for a thumbnail; `Webcam`/`Texture2D`
                        // draw nothing, exactly like a live module with no
                        // source.
                        input_texture: None,
                        input_size: None,
                        is_gallery: true,
                        pixel_scale: 1.0,
                    };
                    instance.draw(&mut draw);
                }
                canvas.submit();
                canvas.texture().clone()
            }
            PreviewState::Point10 {
                scene,
                context,
                bridge,
                scratch,
                last_time,
            } => {
                context.resize(width, height);

                // `adapter.js:797-804`: scene props, then the bridge levers.
                let mut get = |key: &str| preview_scene_prop(props, key);
                scene.apply_props(&mut get);
                let levers = ResponsivenessLevers::from_getter(|key| get(key));
                let payload = interim_band_payload(audio);

                // `timing.js: createFrameClock` — first frame `dt = 0`, then
                // clamped elapsed seconds.
                let dt = match *last_time {
                    Some(previous) if (frame.time_seconds - previous).is_finite() => {
                        (frame.time_seconds - previous).clamp(0.0, MAX_POINT10_FRAME_SECONDS)
                    }
                    _ => 0.0,
                };
                *last_time = Some(frame.time_seconds);

                bridge.update(dt, Some(&payload), &levers);
                scene.update(dt, bridge);

                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("modv-preview-point10"),
                });
                scene.render(&mut encoder, &scratch.view);
                queue.submit(Some(encoder.finish()));

                scratch.texture.clone()
            }
            PreviewState::Isf {
                compiled,
                input,
                audio: preview_audio,
                last_time,
            } => {
                let definition = registry.isf_definition(module_name).ok_or_else(|| {
                    PreviewError::Render(format!("ISF shader \"{module_name}\" is not registered"))
                })?;

                let time_delta = match *last_time {
                    Some(previous) if (frame.time_seconds - previous).is_finite() => {
                        (frame.time_seconds - previous).max(0.0)
                    }
                    _ => 0.0,
                };
                *last_time = Some(frame.time_seconds);

                // Hover previews filter the live frame. Without one (no main
                // texture yet) fall back to the test card so the tile is not
                // blank.
                if let Some(base_view) = base {
                    run_scale(
                        device,
                        queue,
                        scale_pipeline,
                        scale_layout,
                        sampler,
                        base_view,
                        &input.view,
                    );
                } else {
                    write_preview_pattern(device, queue, &input.texture, width, height);
                }
                let image_views: Vec<(&str, wgpu::TextureView)> = definition
                    .document
                    .inputs
                    .iter()
                    .filter(|input| input.type_ == "image")
                    .map(|decl| {
                        // An assigned image asset previews as itself; every
                        // other texture prop has no live source inside an
                        // isolated thumbnail, so it keeps the live frame (or
                        // the test card) that seeded `input`.
                        let assigned = props
                            .get(&decl.name)
                            .and_then(|value| TextureRequest::from_value(Some(value)))
                            .and_then(|request| request.first_image_path().map(str::to_string))
                            .and_then(|path| {
                                images.get(device, queue, &path, Some((width, height))).ok()
                            })
                            .map(|image| image.view);
                        (
                            decl.name.as_str(),
                            assigned.unwrap_or_else(|| input.view.clone()),
                        )
                    })
                    .collect();
                let image_inputs: Vec<(&str, &wgpu::TextureView)> = image_views
                    .iter()
                    .map(|(name, view)| (*name, view))
                    .collect();
                let values = isf_values(&definition.document, props);
                let uniforms = IsfFrameUniforms {
                    time: frame.time_seconds,
                    time_delta,
                    frame_index: frame_index as i32,
                    date: isf_wall_clock_date(),
                };

                isf_runtime
                    .draw(
                        compiled,
                        &slot_scratch.view,
                        [width, height],
                        uniforms,
                        &values,
                        &image_inputs,
                        // Audio-reactive filters get a synthetic spectrum so
                        // a thumbnail animates even when the live frame is
                        // silent.
                        &preview_audio
                            .as_ref()
                            .map(|(wave, fft)| vec![("audio", &wave.view), ("audioFFT", &fft.view)])
                            .unwrap_or_default(),
                    )
                    .map_err(|error| PreviewError::Render(error.to_string()))?;
                slot_scratch.texture.clone()
            }
        };

        // Composite the module layer over the live-output base.
        let module_view = module_texture.create_view(&wgpu::TextureViewDescriptor::default());
        run_overlay(
            device,
            queue,
            overlay_pipeline,
            overlay_layout,
            sampler,
            &module_view,
            &target.view,
        );

        Ok(())
    }
}

/// Build the isolated [`ModuleInstance`] a preview renders.
///
/// This is the engine's `makeActiveModule` shape without touching the store:
/// the registry supplies `meta` (`meta_fields`) and the prop defaults
/// (`default_props`), the caller-style defaults fill in `alpha`/`enabled`/
/// `compositeOperation`, and the module is flagged `isGallery` like the JS
/// gallery context.
fn build_preview_module(
    registry: &ModuleRegistry,
    module_name: &str,
) -> Result<ModuleInstance, PreviewError> {
    let mut meta = registry.meta_fields(module_name).unwrap_or_default();
    meta.insert("alpha".to_string(), json!(1.0));
    meta.insert("enabled".to_string(), json!(true));
    meta.insert("compositeOperation".to_string(), json!("normal"));
    meta.insert("isGallery".to_string(), json!(true));

    let props = registry.default_props(module_name).ok_or_else(|| {
        PreviewError::Render(format!(
            "module \"{module_name}\" has no prop defaults in the registry"
        ))
    })?;

    Ok(ModuleInstance {
        id: format!("preview:{module_name}"),
        module_name: module_name.to_string(),
        meta: Value::Object(meta),
        props,
        data: json!({}),
        dollar_props: json!({}),
        status: Vec::new(),
        extra: Map::new(),
    })
}

/// Composite `source` over the (cleared) preview `target` with a fresh
/// module's defaults: alpha 1, `normal`.
///
/// Uses the frame graph's composite pass so canvas blend math lives in exactly
/// one place (`modv_render::frame_graph`); the temporary registrations are
/// removed afterwards so evicted slots never keep a texture alive.
/// Full-screen-triangle vertex shader shared by the scale and overlay passes.
///
/// `uv` is top-left origin (v = 0 top), matching the engine's textures.
const PREVIEW_QUAD_WGSL: &str = r#"
struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
  var positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  let position = positions[index];
  var out: VsOut;
  out.position = vec4<f32>(position, 0.0, 1.0);
  out.uv = vec2<f32>((position.x + 1.0) * 0.5, 1.0 - (position.y + 1.0) * 0.5);
  return out;
}
"#;

const SCALE_BODY: &str = r#"
@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  return textureSample(source_texture, source_sampler, in.uv);
}
"#;

const OVERLAY_BODY: &str = r#"
@group(0) @binding(0) var module_texture: texture_2d<f32>;
@group(0) @binding(1) var surface_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  // The target already holds the live output; the blend state composites this
  // premultiplied module layer over it.
  return textureSample(module_texture, surface_sampler, in.uv);
}
"#;

/// Scale the live output to a preview-sized texture (`linear`, one sample).
fn create_scale_pipeline(device: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("modv-preview-scale-shader"),
        source: wgpu::ShaderSource::Wgsl([PREVIEW_QUAD_WGSL, SCALE_BODY].concat().into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("modv-preview-scale-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline = create_quad_pipeline(
        device,
        "modv-preview-scale-pipeline",
        &shader,
        &layout,
        None,
    );
    (pipeline, layout)
}

/// The overlay pass: draw the module layer with premultiplied source-over onto
/// a target that already contains the live output.
fn create_overlay_pipeline(device: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("modv-preview-overlay-shader"),
        source: wgpu::ShaderSource::Wgsl([PREVIEW_QUAD_WGSL, OVERLAY_BODY].concat().into()),
    });
    let texture = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("modv-preview-overlay-layout"),
        entries: &[
            texture(0),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline = create_quad_pipeline(
        device,
        "modv-preview-overlay-pipeline",
        &shader,
        &layout,
        Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        }),
    );
    (pipeline, layout)
}

fn create_quad_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: TARGET_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Copy + scale `source` into `target` (both the slot's size after scaling).
fn run_scale(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    source: &wgpu::TextureView,
    target: &wgpu::TextureView,
) {
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modv-preview-scale-bind-group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("modv-preview-scale"),
    });
    {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("modv-preview-scale-pass"),
            color_attachments: &color_attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
}

/// Composite `module` over the live-output target without clearing it.
#[allow(clippy::too_many_arguments)]
fn run_overlay(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    module: &wgpu::TextureView,
    target: &wgpu::TextureView,
) {
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modv-preview-overlay-bind-group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(module),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("modv-preview-overlay"),
    });
    {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("modv-preview-overlay-pass"),
            color_attachments: &color_attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ready_preview_finishes_once_and_flattens_hover_alpha() {
        let mut preview = PendingPreview::ready(vec![10, 20, 30, 7], true);
        assert_eq!(
            preview.try_finish().expect("ready preview"),
            Some(vec![10, 20, 30, 255])
        );
        assert!(
            preview.try_finish().expect("finished preview").is_none(),
            "a finished preview does not yield pixels twice"
        );

        let mut thumbnail = PendingPreview::ready(vec![10, 20, 30, 7], false);
        assert_eq!(
            thumbnail.try_finish().expect("ready thumbnail"),
            Some(vec![10, 20, 30, 7]),
            "an isolated thumbnail keeps its alpha"
        );
    }

    /// RFC 4648 §10 test vectors plus a binary case with high bytes.
    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_encodes_an_rgba_pixel() {
        // Transparent black, then opaque white: the two shapes the renderer
        // decodes back into `ImageData`.
        assert_eq!(base64_encode(&[0, 0, 0, 0]), "AAAAAA==");
        assert_eq!(base64_encode(&[255, 255, 255, 255]), "/////w==");
        // The renderer's own fixture: 2×2 red/green/blue/white.
        let pixels = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        assert_eq!(base64_encode(&pixels), "/wAA/wD/AP8AAP///////w==");
    }

    #[test]
    fn preview_scene_props_read_boolean_levers() {
        let props = json!({ "useAgc": false, "gain": 1.5, "missing": null });
        assert_eq!(preview_scene_prop(&props, "useAgc"), Some(0.0));
        assert_eq!(preview_scene_prop(&props, "gain"), Some(1.5));
        assert_eq!(preview_scene_prop(&props, "missing"), None);
    }

    #[test]
    fn preview_dimensions_clamp_to_one_and_the_maximum() {
        assert_eq!(clamp_preview_dimension(0), 1);
        assert_eq!(clamp_preview_dimension(1), 1);
        assert_eq!(clamp_preview_dimension(128), 128);
        assert_eq!(clamp_preview_dimension(MAX_PREVIEW_DIMENSION), 512);
        assert_eq!(clamp_preview_dimension(513), 512);
        assert_eq!(clamp_preview_dimension(u32::MAX), 512);
    }
}
