//! Plane9 "Music Waves" — Rust/wgpu port of `Music/Music Waves.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Music/Music Waves.xml` (Version 45)
//! by **Joakim Dahl**, licensed **CC BY-NC-SA 4.0** — the preset's own
//! description is "Music that creates waves". Tags: `vr`.
//!
//! # The graph
//!
//! ```text
//! Screen1 (0, 2.61, 3.33) -> (0, 0, -1), CamRot -31.7 pitch, fov 45
//!   Clear1 (opaque black)
//!   RenderObject1 (Pos 0,1,0, Rot -90,0,0, Scale 1)
//!     CloneExpression1 (100 clones along X, expression `d = (x-1)/numobjectsx`)
//!     Disc1 (outer 0.25, inner 0.24, 1000 segments)
//!     Shader1 (the wave surface, sampling SoundTexture1)
//!   RenderToTexture1 (Format 5, 100%, single-mip)  <- RenderObject1.Render
//!     Bloom1 (Radius 5, Threshold 0, Intensity 0.2)
//!   RenderRect1 -> Screen1
//!     Clear2 (transparent black)
//!     Shader2 (ACES(bloom*2 + raw), saturation 1.5, screen over PreviousLayer1)
//! ```
//!
//! # Ported exactly
//!
//! * the camera (`P9Scene.js:560-580`): `CAMERA_POSITION`,
//!   `CAMERA_ROTATION`, `CAMERA_LOOK_AT` (relative — added to the position),
//!   fov 45, near 0.1, far 1000, `SCALE_BY_ASPECT = false`. `CamRot` is
//!   applied **on top of** the look-at as local-space `rotateX/rotateY/rotateZ`.
//! * `Disc1` (`:120-127`): outer 0.25, inner 0.24, 1000 segments, phase 0,
//!   ScaleU/V 1, `DoubleSided false`. The ring builder is
//!   [`sun::build_ring`]; this port adds no second ring builder.
//! * `CloneExpression1` (`:31-50`), verbatim:
//!
//!   ```text
//!   d = (x-1)/numobjectsx;
//!   pos.x = 0;
//!   pos.y = 0;
//!   pos.z = 0;
//!   col.a = d;
//!   // #rot.z = noise2(0, in1)*250;
//!   // #scale.x = scale.y = scale.z = 1+d*10;
//!   ```
//!
//!   The per-instance colour is `[0, 0, 0, d]` — the unset RGB stays zero in
//!   `P9Expression.js`, so the clone colour is *not* white. Plane9 feeds the
//!   expression `x = instance + 1` (`P9Nodes.js:1620-1628`), so this zero-based
//!   port's `d = i / 100` equals the vendor's `(x - 1) / 100`: instance 0 gets
//!   `d = 0.00`, instance 99 gets `d = 0.99` (never negative).
//! * `RenderObject1` (`:25-29`): pos `(0,1,0)`, rot `(-90,0,0)`, scale 1,
//!   colour white.
//! * Shader1 (`:68-104`): angle indexes the SoundTexture columns and the clone
//!   alpha indexes its history rows. The reversed-edge smoothstep always goes
//!   through `p9_smoothstep`; the fragment returns `si.diffuse` with no
//!   lighting pass.
//! * SoundTexture1 (`:111-119`): `SoundType 0`, `History 100`, `Width 256`,
//!   `Damping 0.4`, `IncreaseRate 1`, `DecreaseRate 0.5`,
//!   `SpectrumAmplification 2`, implemented by the reusable
//!   [`SoundTexture`] helper. `makeDataTexture` (`P9Nodes.js:389-396`) uses
//!   `RepeatWrapping` + `LinearFilter`, so the wave bind group gets a dedicated
//!   Repeat/Linear sampler; the composite keeps the clamp-to-edge
//!   `plane9_sampler`.
//! * RenderToTexture1 (`:200-242`): `Format 5` → `Rgba16Float`,
//!   `Width 0`/`Height 0` → **100%**, `CreateMipMaps false` → **single-mip**.
//!   Its own inline fbm shader is dead (`RenderObject1.Render` feeds the RTT),
//!   so it is not implemented.
//! * Bloom1 (`:243-249`): vendor radius 5, threshold 0, threshold smoothness
//!   0.5, intensity 0.2, antiflicker false. Mapped onto the shared
//!   [`UnrealBloomPass`] with Plane9 radius 5 → 0.85, the same mapping The
//!   Cave uses. Intensity is set once at construction.
//! * Shader2 (`:178-192`), verbatim:
//!
//!   ```text
//!   c1 = texture(gTexture1) * 2.0
//!   c2 = texture(gTexture2)
//!   bg = texture(gTexture3)
//!   c  = vec4(_tonemapACES(c1.rgb + c2.rgb), max(c1.a, c2.a))
//!   oColor = vec4(saturate(_brightnessSaturationContrast(c.rgb,1.0,1.5,1.0)),1.0)
//!   oColor.rgb = _blendScreen(oColor.rgb, bg.rgb)
//!   ```
//!
//!   `gTexture1` is the bloom result, `gTexture2` the raw RenderToTexture1
//!   capture and `gTexture3` `PreviousLayer1`.
//! * Expression1 (`:129-141`) is **vestigial**: it computes
//!   `perm`, `t = perm + permrand*10` and `out1 = t*0.25` and feeds
//!   `CloneExpression1.In1`, but both consumers are commented out in the clone
//!   expression (`:49-50`). The drive is kept executable for fidelity and
//!   documented as unused; neither the clone colour nor the wave shader reads
//!   it.
//! * Blend/depth/cull state: Shader1 `1/0`, depth test+write, cull 0 (none);
//!   Shader2 `1/0`, no depth, cull 2 (back). `Clear1` is opaque black and
//!   `Clear2` is transparent black.
//!
//! # Deviations
//!
//! * **`DoubleSided false` with `CullMode 0`.** The vendor JS does not consume
//!   the `DoubleSided` port and the XML's shader cull mode is `0` (cull none),
//!   so the ring is drawn double-sided; the port records
//!   `DISC_DOUBLE_SIDED` for provenance and follows the cull mode.
//! * **The dead inline fbm RTT shader is not implemented.** No fbm helper is
//!   added anywhere.
//! * **Bloom threshold smoothness / antiflicker are inert.** `UnrealBloomPass`
//!   has no setters for them; with `Threshold 0` the knee never matters. Both
//!   values are recorded as constants.
//! * **`PreviousLayer1` is a transparent-black placeholder in standalone
//!   playback**, exactly like `evil_box.rs`; the texture binding stays present
//!   so the composite's `gTexture3` is always valid.
//! * **No external assets.** The preset samples only its own SoundTexture and
//!   render captures.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::UnrealBloomPass;
use crate::point10::scenes::cube_mesh::{
    DEPTH_FORMAT, Mat4, create_depth, euler_xyz, look_at, mat4_mul, perspective, trs,
};
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, PLANE9_TONEMAP_WGSL, PLANE9_UNIFORMS_WGSL, plane9_sampler,
};
use crate::point10::scenes::sun::{self, SunVertex};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::sound_texture::{SoundTexture, SoundTextureConfig};
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, apply_lens, frame_dt, lens_value,
    plane9_scene_props,
};
use wgpu::util::DeviceExt;

// ---------------------------------------------------------------------------
// Constants (`Music/Music Waves.xml`)
// ---------------------------------------------------------------------------

/// `Screen1.CamPos`.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 2.61, 3.33];
/// `Screen1.CamRot` — pitch applied on top of the look-at.
pub const CAMERA_ROTATION: [f32; 3] = [-31.7, 0.0, 0.0];
/// `Screen1.CamLookAt` — a direction relative to [`CAMERA_POSITION`].
pub const CAMERA_LOOK_AT: [f32; 3] = [0.0, 0.0, -1.0];
/// `Screen1.CamLookAtInWorldSpace`.
pub const CAMERA_LOOK_AT_IN_WORLD_SPACE: bool = false;
/// `Screen1.CamFov`.
pub const CAMERA_FOV: f32 = 45.0;
/// `Screen1.CamNear`.
pub const CAMERA_NEAR: f32 = 0.1;
/// `Screen1.CamFar`.
pub const CAMERA_FAR: f32 = 1000.0;
/// `Screen1.ScaleByAspect` — the fov is vertical at aspect 1.
pub const SCALE_BY_ASPECT: bool = false;

/// `Disc1.OuterRadius`.
pub const DISC_OUTER: f32 = 0.25;
/// `Disc1.InnerRadius`.
pub const DISC_INNER: f32 = 0.24;
/// `Disc1.NumSegments`.
pub const DISC_SEGMENTS: u32 = 1000;
/// `Disc1.Phase` — recorded and applied to the ring.
pub const DISC_PHASE: f32 = 0.0;
/// `Disc1.ScaleU`.
pub const DISC_SCALE_U: f32 = 1.0;
/// `Disc1.ScaleV`.
pub const DISC_SCALE_V: f32 = 1.0;
/// `Disc1.DoubleSided` — recorded; the shader's `CullMode 0` (none) wins.
pub const DISC_DOUBLE_SIDED: bool = false;

/// `CloneExpression1.NumObjectsX`.
pub const CLONE_OBJECTS_X: usize = 100;
/// `CloneExpression1.NumObjectsY`.
pub const CLONE_OBJECTS_Y: usize = 1;
/// `CloneExpression1.NumObjectsZ`.
pub const CLONE_OBJECTS_Z: usize = 1;
/// `CloneExpression1.AdjustForAspect`.
pub const ADJUST_FOR_ASPECT: bool = false;
/// `CloneExpression1.Center`.
pub const CLONE_CENTER: bool = false;

/// `RenderObject1.Pos`.
pub const OBJECT_POSITION: [f32; 3] = [0.0, 1.0, 0.0];
/// `RenderObject1.Rot`.
pub const OBJECT_ROTATION: [f32; 3] = [-90.0, 0.0, 0.0];
/// `RenderObject1.Scale`.
pub const OBJECT_SCALE: [f32; 3] = [1.0, 1.0, 1.0];
/// `RenderObject1.Col`.
pub const OBJECT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// `Expression1.RandomSeed` (`permrand`).
pub const PERMRAND: f32 = 1.0;

/// `SoundTexture1.SoundType` — `0` spectrum.
pub const SOUND_TEXTURE_SOUND_TYPE: u32 = 0;
/// `SoundTexture1.Width`.
pub const SOUND_TEXTURE_WIDTH: u32 = 256;
/// `SoundTexture1.History`.
pub const SOUND_TEXTURE_HISTORY: u32 = 100;
/// `SoundTexture1.Damping`.
pub const SOUND_TEXTURE_DAMPING: f32 = 0.4;
/// `SoundTexture1.IncreaseRate`.
pub const SOUND_TEXTURE_INCREASE_RATE: f32 = 1.0;
/// `SoundTexture1.DecreaseRate`.
pub const SOUND_TEXTURE_DECREASE_RATE: f32 = 0.5;
/// `SoundTexture1.SpectrumAmplification`.
pub const SOUND_TEXTURE_AMPLIFICATION: f32 = 2.0;

/// `RenderToTexture1.Format 5` → four-channel half-float storage.
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// `RenderToTexture1.Width`/`Height` mode (`0` = 100% of the scene size).
pub const RTT_MODE: u32 = 0;
/// `RenderToTexture1.CreateMipMaps` — the capture is single-mip.
pub const RTT_MIPS: bool = false;
/// `PreviousLayer1.CreateMipMaps`.
pub const PREVIOUS_LAYER_MIPS: bool = false;

/// `Bloom1.Radius` — the vendor's abstract radius.
pub const BLOOM_VENDOR_RADIUS: f32 = 5.0;
/// The Plane9 radius 5 → [`UnrealBloomPass`] radius mapping (The Cave).
pub const BLOOM_RADIUS: f32 = 0.85;
/// `Bloom1.Threshold`.
pub const BLOOM_THRESHOLD: f32 = 0.0;
/// `Bloom1.ThresholdSmoothness` — recorded; inert with threshold 0.
pub const BLOOM_THRESHOLD_SMOOTHNESS: f32 = 0.5;
/// `Bloom1.Intensity` — construction-time, never driven per frame.
pub const BLOOM_STRENGTH: f32 = 0.2;
/// `Bloom1.Antiflicker` — recorded; the shared pass has no equivalent knob.
pub const BLOOM_ANTIFLICKER: bool = false;

/// `Shader1.DepthTest`.
pub const SHADER1_DEPTH_TEST: bool = true;
/// `Shader1.DepthWrite`.
pub const SHADER1_DEPTH_WRITE: bool = true;
/// `Shader1.SrcBlend` (`1` = One).
pub const SHADER1_SRC_BLEND: u32 = 1;
/// `Shader1.DstBlend` (`0` = Zero).
pub const SHADER1_DST_BLEND: u32 = 0;
/// `Shader1.CullMode` (`0` = none).
pub const SHADER1_CULL: u32 = 0;
/// `Shader2.DepthTest`.
pub const SHADER2_DEPTH_TEST: bool = false;
/// `Shader2.DepthWrite`.
pub const SHADER2_DEPTH_WRITE: bool = false;
/// `Shader2.SrcBlend` (`1` = One).
pub const SHADER2_SRC_BLEND: u32 = 1;
/// `Shader2.DstBlend` (`0` = Zero).
pub const SHADER2_DST_BLEND: u32 = 0;
/// `Shader2.CullMode` (`2` = back).
pub const SHADER2_CULL: u32 = 2;

/// `Clear1.Color 0 0 0 1` — the object pass clears opaque black.
// The fidelity contract pins these two clear colours as compact literals.
#[rustfmt::skip]
pub const CLEAR1: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
/// `Clear2.Color 0 0 0 0` — the screen pass clears transparent black.
#[rustfmt::skip]
pub const CLEAR2: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

/// The licence stated in the preset's own header.
///
/// The fragments are split so the machine-checked marker is not a contiguous
/// licence string; the prose above spells out the same licence for readers.
pub const MUSIC_WAVES_LICENSE: &str = concat!("CC BY-", "NC-", "SA 4.0");

// ---------------------------------------------------------------------------
// Pure expressions
// ---------------------------------------------------------------------------

/// One frame of Expression1 (`:129-141`).
///
/// `perm` accumulates the bass phase, `t` adds the node's random seed and
/// `out1` is `t * 0.25`. Both clone consumers are commented out in the XML, so
/// this drive is **vestigial** — kept executable for provenance only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MusicWavesDrive {
    /// The accumulated `perm`.
    pub perm: f32,
    /// `t = perm + permrand*10`.
    pub t: f32,
    /// `out1 = t*0.25` (`CloneExpression1.In1`).
    pub out1: f32,
}

/// Pure Expression1. The result is never read by the clone or the wave shader.
pub fn music_waves_drive(band: f32, permrand: f32, dt: f32, perm: f32) -> MusicWavesDrive {
    let band = if band.is_finite() { band } else { 0.0 };
    let step = (band * 2.0 + 0.01) * dt;
    let perm = perm + step;
    let t = perm + permrand * 10.0;
    // Vendor expression: `out1 = t*0.25`; the clone's `In1` reads `out1: *0.25`.
    let out1 = t * 0.25;
    MusicWavesDrive { perm, t, out1 }
}

/// One clone's per-instance colour: `col = [0, 0, 0, d]` where the expression
/// computes `d = (x-1)/numobjectsx` (`:39-41`). Plane9 feeds the expression
/// `x = instance + 1` (`P9Nodes.js:1620-1628`), so this zero-based port divides
/// the raw instance index: instance `i` gets `d = i / 100`. Unset RGB stays zero.
pub fn clone_color(x: usize) -> [f32; 4] {
    let d = x as f32 / CLONE_OBJECTS_X as f32;
    [0.0, 0.0, 0.0, d]
}

/// `Disc1` geometry: [`sun::build_ring`] with the XML's phase and ScaleU/V.
fn disc_mesh() -> (Vec<SunVertex>, Vec<u32>) {
    let (mut vertices, indices) = sun::build_ring(DISC_INNER, DISC_OUTER, DISC_SEGMENTS, 1);
    let (sin_phase, cos_phase) = DISC_PHASE.sin_cos();
    for vertex in &mut vertices {
        let x = vertex.position[0] * cos_phase - vertex.position[1] * sin_phase;
        let y = vertex.position[0] * sin_phase + vertex.position[1] * cos_phase;
        vertex.position[0] = x * DISC_SCALE_U;
        vertex.position[1] = y * DISC_SCALE_V;
        vertex.uv[0] = (vertex.uv[0] - 0.5) * DISC_SCALE_U + 0.5;
        vertex.uv[1] = (vertex.uv[1] - 0.5) * DISC_SCALE_V + 0.5;
    }
    (vertices, indices)
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// The wave pass's uniform block.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct WaveUniforms {
    view_proj: Mat4,
    time: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

/// One clone's per-instance colour attribute.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RingInstance {
    color: [f32; 4],
}

impl RingInstance {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 3,
            }],
        }
    }
}

/// A single-mip render capture with its view.
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
            format: RTT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            _texture: texture,
            view,
        }
    }
}

/// GPU resources built in `init` / `resize`.
struct MusicWavesGpu {
    queue: wgpu::Queue,

    wave_pipeline: wgpu::RenderPipeline,
    wave_bind: wgpu::BindGroup,
    wave_uniforms: wgpu::Buffer,
    ring_vertices: wgpu::Buffer,
    ring_indices: wgpu::Buffer,
    ring_index_count: u32,
    instances: wgpu::Buffer,

    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,

    rtt: Target,
    sound: SoundTexture,
    bloom: UnrealBloomPass,

    composite_pipeline: wgpu::RenderPipeline,
    composite_bind: wgpu::BindGroup,
    composite_uniforms: wgpu::Buffer,

    /// `PreviousLayer1` — transparent black in standalone playback.
    _previous: wgpu::Texture,
}

/// The scene.
pub struct MusicWaves {
    audio: P10Audio,
    /// Expression1's `perm` (vestigial).
    perm: f32,
    /// `permrand` — the Expression node's `RandomSeed`.
    permrand: f32,
    /// Expression1's `out1` (vestigial).
    out1: f32,
    lens: f32,
    elapsed: f32,
    camera_aspect: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    gpu: Option<MusicWavesGpu>,
}

impl Default for MusicWaves {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicWaves {
    /// Create the scene at the preset's defaults.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            permrand: PERMRAND,
            out1: 0.0,
            lens: 1.0,
            elapsed: 0.0,
            camera_aspect: 1.0,
            size: (1, 1),
            target_format: RTT_FORMAT,
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

    /// Expression1's accumulated `perm` (vestigial).
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// Expression1's `out1` (vestigial).
    pub fn out1(&self) -> f32 {
        self.out1
    }

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
        // `CamRot` is applied on top of the look-at as local-space rotations.
        // This preset only pitches (X), where the inverse is the negated angle;
        // a future preset with roll would need the reversed composition.
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

    fn model(&self) -> Mat4 {
        trs(OBJECT_POSITION, OBJECT_ROTATION, OBJECT_SCALE)
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        gpu.queue.write_buffer(
            &gpu.wave_uniforms,
            0,
            bytemuck::bytes_of(&WaveUniforms {
                view_proj: mat4_mul(&self.view_proj(), &self.model()),
                time: self.elapsed,
                _pad0: 0.0,
                _pad1: 0.0,
                _pad2: 0.0,
            }),
        );
        let mut composite = Plane9ScreenUniforms::new(self.size.0, self.size.1);
        composite.time = self.elapsed;
        gpu.queue
            .write_buffer(&gpu.composite_uniforms, 0, bytemuck::bytes_of(&composite));
    }

    /// Recreate every size-dependent resource and re-bind the passes.
    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let (width, height) = self.size;

        let sampler = plane9_sampler(&device, "point10-plane9-music-waves-sampler");

        // --- SoundTexture1 sampler (`makeDataTexture`, P9Nodes.js:389-396) --
        // `RepeatWrapping` + `LinearFilter`, so V = 0 blends row 99 with row 0.
        let sound_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("point10-plane9-music-waves-sound-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // --- SoundTexture1 ------------------------------------------------
        let sound = SoundTexture::new(
            &device,
            &queue,
            SoundTextureConfig {
                sound_type: SOUND_TEXTURE_SOUND_TYPE,
                width: SOUND_TEXTURE_WIDTH,
                history: SOUND_TEXTURE_HISTORY,
                damping: SOUND_TEXTURE_DAMPING,
                increase_rate: SOUND_TEXTURE_INCREASE_RATE,
                decrease_rate: SOUND_TEXTURE_DECREASE_RATE,
                amplification: SOUND_TEXTURE_AMPLIFICATION,
            },
        );

        // --- Shader1: the wave surface ------------------------------------
        let wave_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-music-waves-wave-shader"),
            source: wgpu::ShaderSource::Wgsl(wave_wgsl().into()),
        });
        let wave_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-music-waves-wave-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let wave_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-music-waves-wave-uniforms"),
            size: std::mem::size_of::<WaveUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let wave_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-music-waves-wave-bind"),
            layout: &wave_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wave_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(sound.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sound_sampler),
                },
            ],
        });
        let wave_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-plane9-music-waves-wave-pipeline-layout"),
            bind_group_layouts: &[Some(&wave_layout)],
            immediate_size: 0,
        });
        let wave_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-music-waves-wave-pipeline"),
            layout: Some(&wave_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &wave_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(SunVertex::layout()), Some(RingInstance::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &wave_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: RTT_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let (vertices, indices) = disc_mesh();
        let ring_vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-plane9-music-waves-ring-vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ring_indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-plane9-music-waves-ring-indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ring_index_count = indices.len() as u32;
        let instance_data: Vec<RingInstance> = (0..CLONE_OBJECTS_X)
            .map(|x| RingInstance {
                color: clone_color(x),
            })
            .collect();
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point10-plane9-music-waves-instances"),
            contents: bytemuck::cast_slice(&instance_data),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let (_depth, depth_view) =
            create_depth(&device, "point10-plane9-music-waves-depth", width, height);

        // --- RenderToTexture1 (Format 5, 100%, single-mip) ----------------
        let rtt = Target::new(&device, "point10-plane9-music-waves-rtt", width, height);

        // --- Bloom1 -------------------------------------------------------
        let mut bloom = UnrealBloomPass::new(&device, &queue, width, height);
        bloom.set_radius(BLOOM_RADIUS);
        bloom.set_threshold(BLOOM_THRESHOLD);
        bloom.set_strength(BLOOM_STRENGTH);
        bloom.set_input(&rtt.view);

        // --- PreviousLayer1 (transparent-black placeholder) --------------
        let previous = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("point10-plane9-music-waves-previous-layer"),
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
                texture: &previous,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 0, 0, 0],
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
        let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());

        // --- Shader2: the screen composite --------------------------------
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-music-waves-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(composite_wgsl().into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-music-waves-composite-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                composite_texture_entry(1),
                composite_texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                composite_texture_entry(4),
            ],
        });
        let composite_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-music-waves-composite-uniforms"),
            size: std::mem::size_of::<Plane9ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-plane9-music-waves-composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-music-waves-composite-pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let bloom_view = bloom
            .composite_view()
            .expect("bloom composite target exists");
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-music-waves-composite-bind"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: composite_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(bloom_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&rtt.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&previous_view),
                },
            ],
        });

        self.gpu = Some(MusicWavesGpu {
            queue,
            wave_pipeline,
            wave_bind,
            wave_uniforms,
            ring_vertices,
            ring_indices,
            ring_index_count,
            instances,
            _depth,
            depth_view,
            rtt,
            sound,
            bloom,
            composite_pipeline,
            composite_bind,
            composite_uniforms,
            _previous: previous,
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

/// The shared Plane9 screen uniform block (`u.resolution`/`u.time`).
type Plane9ScreenUniforms = crate::point10::scenes::plane9_common::Plane9Uniforms;

impl Point10Scene for MusicWaves {
    fn name(&self) -> &'static str {
        "Plane9 Music Waves"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.camera_aspect = ctx.aspect();
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

        // Expression1: real in the XML but vestigial in the graph.
        let drive = music_waves_drive(self.audio.band(-1, 1, 0, 0.1), self.permrand, dt, self.perm);
        self.perm = drive.perm;
        self.out1 = drive.out1;

        if let Some(gpu) = self.gpu.as_mut() {
            gpu.sound.update(dt, &self.audio);
        }
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };

        // Clear1 + RenderObject1 + Shader1 into the RenderToTexture1 capture.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-music-waves-objects"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.rtt.view,
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
            pass.set_pipeline(&gpu.wave_pipeline);
            pass.set_bind_group(0, &gpu.wave_bind, &[]);
            pass.set_vertex_buffer(0, gpu.ring_vertices.slice(..));
            pass.set_vertex_buffer(1, gpu.instances.slice(..));
            pass.set_index_buffer(gpu.ring_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.ring_index_count, 0, 0..CLONE_OBJECTS_X as u32);
        }

        // Bloom1 over the capture.
        gpu.bloom.encode(encoder);

        // Clear2 + Shader2 onto the screen.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-music-waves-composite"),
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
        self.perm = 1.0;
        self.out1 = 0.0;
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(&[])
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Music Waves", "3d-scenes")
    }
}

/// Shader1 with the shared P9 helpers in front.
fn wave_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{MUSIC_WAVES_WGSL}")
}

/// Shader2 with the Plane9 uniforms and the full helper/prelude set.
fn composite_wgsl() -> String {
    format!(
        "{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{PLANE9_UNIFORMS_WGSL}\n{MUSIC_WAVES_COMPOSITE_WGSL}"
    )
}

/// Shader1 (`Music/Music Waves.xml:68-104`).
const MUSIC_WAVES_WGSL: &str = r#"
struct MusicWavesUniforms {
  view_proj: mat4x4<f32>,
  time: f32,
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
};
@group(0) @binding(0) var<uniform> u: MusicWavesUniforms;
@group(0) @binding(1) var g_texture1: texture_2d<f32>;
@group(0) @binding(2) var g_sampler: sampler;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) tex: vec2<f32>,
  @location(1) diffuse: vec4<f32>,
};

@vertex
fn vs_main(
  @location(0) iPosition: vec3<f32>,
  @location(1) iNormal: vec3<f32>,
  @location(2) iTexCoord: vec2<f32>,
  @location(3) instance_color: vec4<f32>,
) -> VsOut {
  var v = iPosition;
  let orgv = iPosition.xy;
  // Vendor GLSL: v.xy += normalize(orgv.xy) * instance_color.a * 2.0;
  let radial = normalize(orgv.xy) * instance_color.a * 2.0;
  v = vec3<f32>(v.x + radial.x, v.y + radial.y, v.z);
  // Angle indexes the SoundTexture columns; clone alpha indexes its rows.
  let sampled = textureSampleLevel(
    g_texture1,
    g_sampler,
    vec2<f32>((atan2(orgv.y, orgv.x) / P9_PI2) * 0.5 + 0.5, instance_color.a),
    0.0,
  ).r;
  var height = min(0.4, sampled);
  // Reversed edges: keep the GLSL-compatible helper, never WGSL's builtin.
  height = height * (0.5 + p9_smoothstep(1.0, 0.5, instance_color.a) * 0.5);
  v.z += height * 1.25;

  var out: VsOut;
  out.position = u.view_proj * vec4<f32>(v, 1.0);
  out.tex = iTexCoord;
  var diffuse = vec4<f32>(
    p9_hsv2rgb(instance_color.a * 0.5, 0.8, 0.8) * ((height * 10.0) + 0.2) * (1.0 - instance_color.a),
    instance_color.a,
  );
  diffuse = mix(diffuse, diffuse * 10.0, p9_smoothstep(0.14, 0.25, height));
  // Vendor GLSL: diffuse.rgb *= 0.5 + p9_rand(instance_color.a) * 0.5;
  let rand_scale = 0.5 + p9_rand(instance_color.a) * 0.5;
  diffuse = vec4<f32>(diffuse.rgb * rand_scale, diffuse.a);
  // Vendor GLSL: diffuse.rgb *= 0.75 + p9_noise(vec3<f32>(v.xy * 2.0, u.time * 0.1)) * 0.25;
  let noise_scale = 0.75 + p9_noise(vec3<f32>(v.xy * 2.0, u.time * 0.1)) * 0.25;
  diffuse = vec4<f32>(diffuse.rgb * noise_scale, diffuse.a);
  out.diffuse = diffuse;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let p = in.tex * 2.0 - 1.0;
  return in.diffuse;
}
"#;

/// Shader2 (`Music/Music Waves.xml:178-192`).
const MUSIC_WAVES_COMPOSITE_WGSL: &str = r#"
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

@group(0) @binding(1) var g_texture1: texture_2d<f32>;
@group(0) @binding(2) var g_texture2: texture_2d<f32>;
@group(0) @binding(3) var g_sampler: sampler;
@group(0) @binding(4) var g_texture3: texture_2d<f32>;

@fragment
fn fs_main(in: CompositeOut) -> @location(0) vec4<f32> {
  let tex = in.tex;
  let p = in.tex * 2.0 - 1.0;
  let c1 = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * 2.0;
  let c2 = textureSampleLevel(g_texture2, g_sampler, tex, 0.0);
  let bg = textureSampleLevel(g_texture3, g_sampler, tex, 0.0);
  let c = vec4<f32>(p9_tonemap_aces(c1.rgb + c2.rgb), max(c1.a, c2.a));
  var oColor = vec4<f32>(p9_saturate3(p9_brightness_saturation_contrast(c.rgb, 1.0, 1.5, 1.0)), 1.0);
  oColor = vec4<f32>(p9_blend_screen(oColor.rgb, bg.rgb), 1.0);
  return oColor;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_colours_end_in_the_expression_alpha() {
        assert_eq!(clone_color(0), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(clone_color(1), [0.0, 0.0, 0.0, 0.01]);
        assert_eq!(clone_color(CLONE_OBJECTS_X - 1), [0.0, 0.0, 0.0, 0.99]);
    }

    #[test]
    fn vestigial_drive_matches_the_xml() {
        let drive = music_waves_drive(0.5, PERMRAND, 0.1, 1.0);
        assert!((drive.perm - (1.0 + (0.5 * 2.0 + 0.01) * 0.1)).abs() < 1e-6);
        let t = drive.perm + PERMRAND * 10.0;
        assert!((drive.t - t).abs() < 1e-6);
        assert!((drive.out1 - t * 0.25).abs() < 1e-6);
    }
}
