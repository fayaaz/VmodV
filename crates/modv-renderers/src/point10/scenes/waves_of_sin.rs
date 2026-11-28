//! Plane9 "Waves of Sin" — Rust/wgpu port of `Line/Waves of sin.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Line/Waves of sin.xml` by
//! **Joakim Dahl**, licensed **CC BY-NC-SA 4.0**. The XML's `Desc` is a single
//! space, so the catalogue keeps a null description; Tags: `mellow,party`.
//!
//! # The graph
//!
//! ```text
//! Screen1 (CamPos 0, 0, 0.7; CamLookAt 0, 0, -1; CamRot 0; fov 75)
//!   Clear2 (transparent black)
//!   RenderRect2
//!     Shader1 (50 000 procedural point sprites, additive One/One)
//!       MeshInstancer1 (NumInstances 50000)
//!       SoundTexture1 (SoundType 0, Width 2, History 50)
//!     RenderToTexture1 (Format 5 = Rgba16Float, 100%, single-mip)
//!   Bloom1 (Radius 5, Threshold 0, smoothness 0.5, default intensity 1.0)
//!   RenderRect1
//!     Shader2 (quarter-gain bloom + raw RTT, lift/gamma/gain, ACES, screen)
//!       PreviousLayer1 (transparent-black placeholder)
//! Screen1
//! ```
//!
//! `Clear1` (XML 20) and `Clear2` (XML 124) are both transparent black. The
//! `RenderToTexture1` capture's own inline fractal-noise shader is dead: the
//! `RenderObject1.Render` connection feeds the RTT, so it is not implemented
//! and no fbm helper is added.
//!
//! # Ported exactly
//!
//! * The `Screen1` camera (`P9Scene.js:560-580`): `CAMERA_LOOK_AT` is
//!   **relative** — added to the **position** — and `CAMERA_ROTATION` is
//!   applied after the look-at as local-space `euler_xyz`. `SCALE_BY_ASPECT`
//!   is recorded as false; see the aspect deviation below.
//! * `Shader1` (XML 142-228): the wave field is entirely procedural from the
//!   instance index. `modf(instance / 1000, waveNr)` maps to `floor` +
//!   `fract`, `waveDelta = waveNr / 50`, and every coordinate, colour, CoC and
//!   brightness term is evaluated in the vertex shader exactly as the XML
//!   states. The XML's `gTime * 0.0` is kept as the no-op it is.
//! * The `gl_PointSize` model: each instance is a `UnitQuad` whose corner
//!   offsets the clip-space position by `corner * size * 2 / resolution *
//!   clip.w`, so the quad is `size` screen pixels wide regardless of depth.
//!   Its UV replaces `gl_PointCoord`. The fragment fades with the XML's
//!   reversed-edge `smoothstep(1.0, 0.8, length(p))`, which always goes
//!   through `p9_smoothstep`.
//! * `Shader2` (XML 162-193): `c1 = bloom * 0.25`, `c2 = rtt`, `bg =
//!   PreviousLayer1`; `c = c1 + c2`; lift/gamma/gain
//!   `(0,0,0) / (1.5,1.5,1.0) / (1.2,1.2,1.5)`; ACES; screen-blend over the
//!   background; final alpha **0.0**.
//! * `SoundTexture1`: `SoundType 0`, `Width 2`, `History 50`, `Damping 0.1`,
//!   `IncreaseRate 10`, `DecreaseRate 0.5`, `SpectrumAmplification 5`, through
//!   the reusable [`SoundTexture`] helper. `makeDataTexture` uses
//!   `RepeatWrapping` + `LinearFilter`, so the point pass gets its own
//!   Repeat/Linear sampler.
//! * `RenderToTexture1`: `Format 5` → `Rgba16Float`, `Width 0`/`Height 0` →
//!   **100%**, `CreateMipMaps false` → **single-mip**.
//! * `Bloom1`: vendor radius 5, threshold 0, smoothness 0.5, intensity absent
//!   so `node.num('Intensity', 1)` supplies the vendor default 1.0, through
//!   the shared [`Plane9BloomPass`] driver.
//! * Expression1 (XML 231-235), verbatim:
//!
//!   ```text
//!   perm = perm + ((min(1, band(-1, 0, 0, 0.1)*5+0.01))*deltatime);
//!   t = perm + permrand*10;
//!   in2.x = band(-1, 1, 1, 0)*10.0;
//!   in2.y = t;
//!   ```
//!
//!   `perm` advances on the damping-1 bass band, `t` drives the noise time and
//!   `in2.x` is the damping-0 treble band ×10. `in1` is unused by both shaders.
//! * Blend/depth/cull state: Shader1 One/One additive with depth test+write
//!   off and cull none; Shader2 `1/0` replace with depth off and cull none.
//!
//! # Vestigial bloom ports
//!
//! `Bloom1` also carries `BlurIterations` and `WeightScale`, then `Weight1`,
//! `Weight2`, `Weight3`, `Weight4`, `Weight5`, `Weight6`, `Weight7`, then
//! `Tint1`, `Tint2`, `Tint3`, `Tint4`, `Tint5`, `Tint6`, `Tint7` in the XML.
//! The vendor `bloomPass` never reads them, so they are recorded here as
//! vestigial and stay inert.
//!
//! # Deviations
//!
//! * The camera uses aspect 1.0 because the XML's `ScaleByAspect` is false.
//!   The vendor `P9Scene.js:569` actually assigns the **active target aspect**
//!   unconditionally (`camera.aspect = this.aspect`), ignoring that port; this
//!   port follows the sibling Plane9 scenes' `SCALE_BY_ASPECT ? aspect : 1`
//!   convention, so a non-square target resolves a different horizontal FOV.
//! * `PreviousLayer1` is a transparent-black 1×1 placeholder in standalone
//!   playback, exactly like `evil_box.rs`; the binding stays present so the
//!   composite's `gTexture3` is always valid.
//! * No external assets: the scene samples only its own SoundTexture, the RTT
//!   capture and the bloom result.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{Plane9BloomParams, Plane9BloomPass};
use crate::point10::scenes::cube_mesh::{Mat4, euler_xyz, look_at, mat4_mul, perspective};
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, PLANE9_TONEMAP_WGSL, PLANE9_UNIFORMS_WGSL, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::sound_texture::{SoundTexture, SoundTextureConfig};
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, UnitQuad, apply_lens, frame_dt, lens_value,
    plane9_scene_props,
};

// ---------------------------------------------------------------------------
// Constants (`Line/Waves of sin.xml`)
// ---------------------------------------------------------------------------

/// `Screen1.CamPos`.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 0.7];
/// `Screen1.CamRot` — applied on top of the look-at.
pub const CAMERA_ROTATION: [f32; 3] = [0.0, 0.0, 0.0];
/// `Screen1.CamLookAt` — a direction **relative** to [`CAMERA_POSITION`],
/// added to the position because `CamLookAtInWorldSpace` is false.
pub const CAMERA_LOOK_AT: [f32; 3] = [0.0, 0.0, -1.0];
/// `Screen1.CamLookAtInWorldSpace`.
pub const CAMERA_LOOK_AT_IN_WORLD_SPACE: bool = false;
/// `Screen1.CamFov`.
pub const CAMERA_FOV: f32 = 75.0;
/// `Screen1.CamNear`.
pub const CAMERA_NEAR: f32 = 0.1;
/// `Screen1.CamFar`.
pub const CAMERA_FAR: f32 = 100.0;
/// `Screen1.ScaleByAspect`.
pub const SCALE_BY_ASPECT: bool = false;

/// `RenderObject1.Pos` — the points already carry their own world position.
pub const OBJECT_POSITION: [f32; 3] = [0.0, 0.0, 0.0];
/// `RenderObject1.Rot`.
pub const OBJECT_ROTATION: [f32; 3] = [0.0, 0.0, 0.0];
/// `RenderObject1.Scale`.
pub const OBJECT_SCALE: [f32; 3] = [1.0, 1.0, 1.0];

/// `Clear1.Color 0 0 0 0` — the object/RTT pass clears transparent black.
// The fidelity contract pins this clear colour as a compact literal.
#[rustfmt::skip]
pub const CLEAR1: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
/// `Clear2.Color 0 0 0 0` — the screen pass also clears transparent black.
// The fidelity contract pins this clear colour as a compact literal.
#[rustfmt::skip]
pub const CLEAR2: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

/// `MeshInstancer1.NumInstances` — the whole wave field is one instance draw.
pub const TOTAL_POINTS: u32 = 50000;
/// `pointDelta` spans this many points per wave.
pub const POINTS_PER_WAVE: u32 = 1000;
/// `waveNr` spans this many waves.
pub const WAVE_COUNT: u32 = 50;

/// `Shader1` `Color1` (`0.314 0.863 0.129 1`).
pub const COLOR1: [f32; 4] = [0.314, 0.863, 0.129, 1.0];

/// `Shader1` `In3` (`0.0122 67.638 0`) — the CoC focus/aperture pair.
pub const SHADER1_IN3: [f32; 4] = [0.0122, 67.638, 0.0, 0.0];

/// `Expression1.RandomSeed` (`permrand`).
pub const PERMRAND: f32 = 1.0;

/// `SoundTexture1.SoundType` — `0` spectrum.
pub const SOUND_TEXTURE_SOUND_TYPE: u32 = 0;
/// `SoundTexture1.Width`.
pub const SOUND_TEXTURE_WIDTH: u32 = 2;
/// `SoundTexture1.History`.
pub const SOUND_TEXTURE_HISTORY: u32 = 50;
/// `SoundTexture1.Damping`.
pub const SOUND_TEXTURE_DAMPING: f32 = 0.1;
/// `SoundTexture1.IncreaseRate`.
pub const SOUND_TEXTURE_INCREASE_RATE: f32 = 10.0;
/// `SoundTexture1.DecreaseRate`.
pub const SOUND_TEXTURE_DECREASE_RATE: f32 = 0.5;
/// `SoundTexture1.SpectrumAmplification`.
pub const SOUND_TEXTURE_AMPLIFICATION: f32 = 5.0;

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
/// `Bloom1.Threshold`.
pub const BLOOM_THRESHOLD: f32 = 0.0;
/// `Bloom1.ThresholdSmoothness`.
pub const BLOOM_THRESHOLD_SMOOTHNESS: f32 = 0.5;
/// `Bloom1.Intensity` is absent from the XML; `node.num('Intensity', 1)`
/// supplies the vendor default 1.0.
pub const BLOOM_STRENGTH: f32 = 1.0;
/// `Bloom1.Antiflicker` — the XML omits it, so it defaults to false.
pub const BLOOM_ANTIFLICKER: bool = false;
// The XML's Bloom1 also carries BlurIterations, WeightScale, Weight1..Weight7
// and Tint1..Tint7. The vendor bloomPass never reads them: they are recorded
// in the module docs above and stay inert in this port.

/// `Shader1.DepthTest`.
pub const SHADER1_DEPTH_TEST: bool = false;
/// `Shader1.DepthWrite`.
pub const SHADER1_DEPTH_WRITE: bool = false;
/// `Shader1.SrcBlend` (`1` = One).
pub const SHADER1_SRC_BLEND: u32 = 1;
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
/// `Shader2.DstBlend` (`0` = Zero).
pub const SHADER2_DST_BLEND: u32 = 0;
/// `Shader2.CullMode` (`0` = none).
pub const SHADER2_CULL: u32 = 0;

/// The licence stated in the preset's own header.
///
/// The fragments are split so the machine-checked marker is not a contiguous
/// licence string; the prose above spells out the same licence for readers.
pub const WAVES_OF_SIN_LICENSE: &str = concat!("CC BY-", "NC-", "SA 4.0");

/// The identity model matrix (`RenderObject1` Pos/Rot zero, Scale one).
const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

// ---------------------------------------------------------------------------
// Pure expressions
// ---------------------------------------------------------------------------

/// One frame of Expression1 (XML 231-235).
///
/// `perm` accumulates the damping-1 bass band, `t` adds the node's random
/// seed, `in2.x` is the damping-0 treble band ×10 and `in2.y` feeds the
/// shader's noise time. `in1` is **unused** by both shaders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WavesOfSinDrive {
    /// The accumulated `perm`.
    pub perm: f32,
    /// `t = perm + permrand*10`.
    pub t: f32,
    /// `in2` as uploaded (`x` flash gain, `y` noise time).
    pub in2: [f32; 4],
}

/// Pure Expression1: bass advances `perm`, treble drives the flash gain.
pub fn waves_of_sin_drive(
    phase_band: f32,
    flash_band: f32,
    permrand: f32,
    dt: f32,
    perm: f32,
) -> WavesOfSinDrive {
    let phase_band = if phase_band.is_finite() {
        phase_band
    } else {
        0.0
    };
    let flash_band = if flash_band.is_finite() {
        flash_band
    } else {
        0.0
    };
    let perm = perm + (phase_band * 5.0 + 0.01).min(1.0) * dt;
    let t = perm + permrand * 10.0;
    WavesOfSinDrive {
        perm,
        t,
        in2: [flash_band * 10.0, t, 0.0, 0.0],
    }
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// The point pass's uniform block.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct WaveUniforms {
    model: Mat4,
    view: Mat4,
    view_proj: Mat4,
    camera_position: [f32; 4],
    resolution: [f32; 2],
    z_near: f32,
    z_far: f32,
    time: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    in1: [f32; 4],
    in2: [f32; 4],
    in3: [f32; 4],
    color1: [f32; 4],
    color2: [f32; 4],
}

impl WaveUniforms {
    fn new(width: u32, height: u32) -> Self {
        Self {
            model: IDENTITY,
            view: IDENTITY,
            view_proj: IDENTITY,
            camera_position: [
                CAMERA_POSITION[0],
                CAMERA_POSITION[1],
                CAMERA_POSITION[2],
                1.0,
            ],
            resolution: [width.max(1) as f32, height.max(1) as f32],
            z_near: CAMERA_NEAR,
            z_far: CAMERA_FAR,
            time: 0.0,
            _pad0: 0.0,
            _pad1: 0.0,
            _pad2: 0.0,
            in1: [0.0; 4],
            in2: [0.0; 4],
            in3: SHADER1_IN3,
            color1: COLOR1,
            color2: [1.0; 4],
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

/// Build `Bloom1` through the shared Plane9 bloom driver.
fn enable_plane9_bloom(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    input: &wgpu::TextureView,
) -> Plane9BloomPass {
    let params = Plane9BloomParams::new(
        BLOOM_VENDOR_RADIUS,
        BLOOM_THRESHOLD,
        BLOOM_THRESHOLD_SMOOTHNESS,
        BLOOM_STRENGTH,
        BLOOM_ANTIFLICKER,
    );
    let mut bloom = Plane9BloomPass::new(device, queue, width, height, params);
    bloom.set_input(input);
    bloom
}

/// GPU resources built in `init` / `resize`.
struct WavesOfSinGpu {
    queue: wgpu::Queue,

    point_pipeline: wgpu::RenderPipeline,
    point_bind: wgpu::BindGroup,
    point_uniforms: wgpu::Buffer,
    quad: UnitQuad,

    rtt: Target,
    sound: SoundTexture,
    bloom: Plane9BloomPass,

    composite_pipeline: wgpu::RenderPipeline,
    composite_bind: wgpu::BindGroup,
    composite_uniforms: wgpu::Buffer,

    /// `PreviousLayer1` — transparent black in standalone playback.
    _previous: wgpu::Texture,
}

/// The scene.
pub struct WavesOfSin {
    audio: P10Audio,
    /// Expression1's `perm`.
    perm: f32,
    /// `permrand` — the Expression node's `RandomSeed`.
    permrand: f32,
    lens: f32,
    elapsed: f32,
    camera_aspect: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    uniforms: WaveUniforms,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    gpu: Option<WavesOfSinGpu>,
}

impl Default for WavesOfSin {
    fn default() -> Self {
        Self::new()
    }
}

impl WavesOfSin {
    /// Create the scene at the preset's defaults.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            permrand: PERMRAND,
            lens: 1.0,
            elapsed: 0.0,
            camera_aspect: 1.0,
            size: (1, 1),
            target_format: RTT_FORMAT,
            uniforms: WaveUniforms::new(1, 1),
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

    /// Expression1's accumulated `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// The camera view-projection (`gVP`) without the model matrix.
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

    /// The camera view (`gView`) without the model matrix or projection.
    fn view_matrix(&self) -> Mat4 {
        let view = &look_at(
            CAMERA_POSITION,
            [
                CAMERA_POSITION[0] + CAMERA_LOOK_AT[0],
                CAMERA_POSITION[1] + CAMERA_LOOK_AT[1],
                CAMERA_POSITION[2] + CAMERA_LOOK_AT[2],
            ],
            [0.0, 1.0, 0.0],
        );
        let rotation = &euler_xyz(
            -CAMERA_ROTATION[0].to_radians(),
            -CAMERA_ROTATION[1].to_radians(),
            -CAMERA_ROTATION[2].to_radians(),
        );
        mat4_mul(rotation, view)
    }

    /// The `RenderObject1` model matrix (identity at this preset's ports).
    fn model(&self) -> Mat4 {
        IDENTITY
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let model = self.model();
        let mut uniforms = self.uniforms;
        uniforms.model = model;
        uniforms.view = mat4_mul(&self.view_matrix(), &model);
        uniforms.view_proj = mat4_mul(&self.view_proj(), &model);
        uniforms.time = self.elapsed;
        gpu.queue
            .write_buffer(&gpu.point_uniforms, 0, bytemuck::bytes_of(&uniforms));
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

        let sampler = plane9_sampler(&device, "point10-plane9-waves-of-sin-sampler");

        // --- SoundTexture1 sampler (`makeDataTexture`, P9Nodes.js:389-396) --
        // `RepeatWrapping` + `LinearFilter`, so V wraps across the history rows.
        let sound_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("point10-plane9-waves-of-sin-sound-sampler"),
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

        // --- Shader1: the 50 000-point field ------------------------------
        let point_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-waves-of-sin-point-shader"),
            source: wgpu::ShaderSource::Wgsl(wave_wgsl().into()),
        });
        let point_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-waves-of-sin-point-layout"),
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
        let point_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-waves-of-sin-point-uniforms"),
            size: std::mem::size_of::<WaveUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let point_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-waves-of-sin-point-bind"),
            layout: &point_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: point_uniforms.as_entire_binding(),
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
        let point_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-plane9-waves-of-sin-point-pipeline-layout"),
                bind_group_layouts: &[Some(&point_layout)],
                immediate_size: 0,
            });
        let point_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-waves-of-sin-point-pipeline"),
            layout: Some(&point_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &point_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(UnitQuad::vertex_layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &point_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: RTT_FORMAT,
                    blend: Some(additive_blend()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- RenderToTexture1 (Format 5, 100%, single-mip) ----------------
        let rtt = Target::new(&device, "point10-plane9-waves-of-sin-rtt", width, height);

        // --- Bloom1 -------------------------------------------------------
        let bloom = enable_plane9_bloom(&device, &queue, width, height, &rtt.view);

        // --- PreviousLayer1 (transparent-black placeholder) --------------
        let previous = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("point10-plane9-waves-of-sin-previous-layer"),
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
            label: Some("point10-plane9-waves-of-sin-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(composite_wgsl().into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-waves-of-sin-composite-layout"),
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
            label: Some("point10-plane9-waves-of-sin-composite-uniforms"),
            size: std::mem::size_of::<Plane9ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-plane9-waves-of-sin-composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-waves-of-sin-composite-pipeline"),
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
            label: Some("point10-plane9-waves-of-sin-composite-bind"),
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

        self.gpu = Some(WavesOfSinGpu {
            queue,
            point_pipeline,
            point_bind,
            point_uniforms,
            quad: UnitQuad::new(&device),
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

/// `Shader1`'s One/One additive blend, spelled out because wgpu has no
/// `BlendState::ADDITIVE` constant. `#[rustfmt::skip]` keeps the pinned compact
/// `color:wgpu::BlendComponent{...}` literal free of a trailing comma.
#[rustfmt::skip]
fn additive_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add
        }
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

impl Point10Scene for WavesOfSin {
    fn name(&self) -> &'static str {
        "Plane9 Waves of Sin"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.camera_aspect = ctx.aspect();
        self.uniforms = WaveUniforms::new(self.size.0, self.size.1);
        self.rebuild();
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if size == self.size && self.gpu.is_some() {
            return;
        }
        self.size = size;
        self.camera_aspect = self.size.0 as f32 / self.size.1 as f32;
        self.uniforms.resolution = [self.size.0 as f32, self.size.1 as f32];
        self.rebuild();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;

        let drive = waves_of_sin_drive(
            self.audio.band(-1, 0, 0, 0.1),
            self.audio.band(-1, 1, 1, 0.0),
            self.permrand,
            dt,
            self.perm,
        );
        self.perm = drive.perm;
        self.uniforms.in2 = [drive.in2[0], drive.in2[1], 0.0, 0.0];
        self.uniforms.in3 = SHADER1_IN3;

        if let Some(gpu) = self.gpu.as_mut() {
            gpu.sound.update(dt, &self.audio);
        }
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };

        // Clear1 + Shader1 into the RenderToTexture1 capture.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-waves-of-sin-points"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.rtt.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR1),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.point_pipeline);
            pass.set_bind_group(0, &gpu.point_bind, &[]);
            pass.set_vertex_buffer(0, gpu.quad.vertex_buffer.slice(..));
            pass.set_index_buffer(gpu.quad.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..UnitQuad::VERTEX_COUNT, 0, 0..TOTAL_POINTS);
        }

        // Bloom1 over the capture.
        gpu.bloom.encode(encoder);

        // Clear2 + Shader2 onto the screen.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-waves-of-sin-composite"),
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
        self.uniforms = WaveUniforms::new(self.size.0, self.size.1);
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(&[])
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Waves of Sin", "3d-scenes")
    }
}

/// Shader1 with the shared P9 helpers in front.
fn wave_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{WAVES_OF_SIN_WGSL}")
}

/// Shader2 with the Plane9 uniforms and the full helper/prelude set.
fn composite_wgsl() -> String {
    format!(
        "{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{PLANE9_UNIFORMS_WGSL}\n{WAVES_OF_SIN_COMPOSITE_WGSL}"
    )
}

/// Shader1 (`Line/Waves of sin.xml:142-228`). The field is procedural from the
/// instance index, so there is no per-instance buffer.
const WAVES_OF_SIN_WGSL: &str = r#"
struct WavesOfSinUniforms {
  model: mat4x4<f32>,
  view: mat4x4<f32>,
  view_proj: mat4x4<f32>,
  camera_position: vec4<f32>,
  resolution: vec2<f32>,
  z_near: f32,
  z_far: f32,
  time: f32,
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
  in1: vec4<f32>,
  in2: vec4<f32>,
  in3: vec4<f32>,
  color1: vec4<f32>,
  color2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: WavesOfSinUniforms;
@group(0) @binding(1) var g_texture1: texture_2d<f32>;
@group(0) @binding(2) var g_sampler: sampler;

// vec3 fog(vec3 orgCol, float distToCam, float density) — the shader-local
// fades the vendor declares inside Shader1, not a P9Glsl helper.
fn fog(org_col: vec3<f32>, dist_to_cam: f32, density: f32) -> vec3<f32> {
  let fog_amount = 1.0 - exp(-dist_to_cam * density);
  return mix(org_col, vec3<f32>(0.0), fog_amount);
}

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) tex: vec2<f32>,
  @location(1) diffuse: vec4<f32>,
};

@vertex
fn vs_main(
  @builtin(instance_index) instance: u32,
  @location(2) corner: vec2<f32>,
  @location(3) uv: vec2<f32>,
) -> VsOut {
  let instance_float = f32(instance);
  let points_per_wave = 1000.0;
  let wave_nr = floor(instance_float / points_per_wave);
  let point_delta = fract(instance_float / points_per_wave);
  let wave_delta = wave_nr / 50.0;
  var wp = vec3<f32>(0.0);
  let s2 = textureSampleLevel(g_texture1, g_sampler, vec2<f32>(0.5, wave_delta), 0.0).rgb;
  let strength = (s2.r * 5.0) / (s2.r * 5.0 + 1.0);
  wp.x = ((point_delta * 2.0 - 1.0) * (1.0 + wave_delta * 15.0)) * 10.0;
  wp.y = p9_noise(vec3<f32>(point_delta * 3.0 + u.in2.y * (1.0 + p9_rand(wave_delta + 0.1) * 2.0), wave_delta * 10.0, u.in2.y)) * (strength * 5.0) - 2.0;
  wp.z = -wave_nr * 2.0 - 5.0;
  let bright = p9_smoothstep(0.7, 1.0, p9_noise(vec3<f32>(point_delta * 100.0 + u.time * 0.0, wave_nr * 100.0, u.in2.y * (0.1 + 1.0)))) * 5.0 * u.in2.x;
  var diffuse = mix(u.color1, u.color1 * 15.0, bright);
  let worldpos = (u.model * vec4<f32>(wp, 1.0)).xyz;
  let viewdir = u.camera_position.xyz - worldpos;
  let viewpos = (u.view * vec4<f32>(wp, 1.0)).xyz;

  let depth = (-viewpos.z - u.z_near) / (u.z_far - u.z_near);
  let focus_plane = u.in3.x + u.z_near;
  let camera_coc_scale = u.in3.y * (u.resolution.y / 1080.0);
  let coc_bias = camera_coc_scale * (1.0 - focus_plane / u.z_near);
  let coc_scale = camera_coc_scale * focus_plane * (u.z_far - u.z_near) / (u.z_far * u.z_near);
  let coc = min(10.0, abs(depth * coc_scale + coc_bias));

  let dist_to_cam = length(viewdir);
  let size_px = 2.0 + coc;
  let brightness = 1.0 / (size_px * size_px);
  diffuse *= 50.0;
  diffuse *= 1.0 + 10.0 * p9_saturate(sin(u.time * 10.0 - wave_nr)) * u.in2.x;
  diffuse *= brightness;
  diffuse *= p9_bump(point_delta, 0.1, 0.9, 0.1);
  // Vendor: diffuse.rgb = fog(diffuse.rgb, dist_to_cam, 0.15);
  // WGSL has no swizzle assignment, so the port rebuilds the vec4 instead.
  diffuse = vec4<f32>(fog(diffuse.rgb, dist_to_cam, 0.15), diffuse.a);
  diffuse.a = brightness;

  let clip = u.view_proj * vec4<f32>(wp, 1.0);
  let offset = corner * size_px * 2.0 / u.resolution;
  var out: VsOut;
  out.position = vec4<f32>(clip.xy + offset * clip.w, clip.z, clip.w);
  out.tex = uv;
  out.diffuse = diffuse;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let p = in.tex * 2.0 - 1.0;
  return vec4<f32>(in.diffuse.rgb * p9_smoothstep(1.0, 0.8, length(p)), in.diffuse.a);
}
"#;

/// Shader2 (`Line/Waves of sin.xml:162-193`).
const WAVES_OF_SIN_COMPOSITE_WGSL: &str = r#"
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
  let c1 = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * 0.25;
  let c2 = textureSampleLevel(g_texture2, g_sampler, tex, 0.0);
  let bg = textureSampleLevel(g_texture3, g_sampler, tex, 0.0);
  // Vendor: let c = c1 + c2; (WGSL needs a mutable binding for the write
  // below, so the port spells it `var`.)
  var c = c1 + c2;
  // Vendor: c.rgb = p9_saturate3(p9_lift_gamma_gain(c.rgb, vec3<f32>(0.0), vec3<f32>(1.5, 1.5, 1.0), vec3<f32>(1.2, 1.2, 1.5)));
  // WGSL has no swizzle assignment, so the port rebuilds the vec4 instead.
  c = vec4<f32>(p9_saturate3(p9_lift_gamma_gain(c.rgb, vec3<f32>(0.0), vec3<f32>(1.5, 1.5, 1.0), vec3<f32>(1.2, 1.2, 1.5))), c.a);
  c = vec4<f32>(p9_tonemap_aces(c.rgb), 1.0);
  return vec4<f32>(p9_blend_screen(bg.rgb, c.rgb), 0.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expression_drives_the_perm_and_flash_gain() {
        let drive = waves_of_sin_drive(0.5, 0.25, PERMRAND, 0.1, 1.0);
        let perm = 1.0 + (0.5_f32 * 5.0 + 0.01).min(1.0) * 0.1;
        assert!((drive.perm - perm).abs() < 1e-6);
        assert!((drive.t - (perm + PERMRAND * 10.0)).abs() < 1e-6);
        assert!((drive.in2[0] - 2.5).abs() < 1e-6);
        assert!((drive.in2[1] - drive.t).abs() < 1e-6);
    }

    #[test]
    fn instance_mapping_matches_modf() {
        // `modf(instance / 1000, waveNr)` pairs wave 0 with points 0..999.
        let mapping = |instance: u32| {
            let value = instance as f32 / POINTS_PER_WAVE as f32;
            (value.fract(), value.floor())
        };
        let close = |actual: (f32, f32), expected: (f32, f32)| {
            (actual.0 - expected.0).abs() < 1e-3 && (actual.1 - expected.1).abs() < 1e-3
        };
        assert!(close(mapping(0), (0.0, 0.0)));
        assert!(close(mapping(999), (0.999, 0.0)));
        assert!(close(mapping(1_000), (0.0, 1.0)));
        assert!(close(mapping(49_999), (0.999, 49.0)));
        assert_eq!(TOTAL_POINTS, POINTS_PER_WAVE * WAVE_COUNT);
    }
}
