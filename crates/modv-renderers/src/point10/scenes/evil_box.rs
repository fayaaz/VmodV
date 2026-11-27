//! Plane9 "Evil Box" — Rust/wgpu port of `Cube/Evil Box.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Cube/Evil Box.xml`, preset `myxelac7`
//! (Version 26) by **Joakim Dahl**, licensed **CC BY-NC-SA 4.0** — the preset's
//! own description is "Light trying to escape an evil box". Tags: `cube,vr`.
//!
//! # The graph
//!
//! ```text
//! Screen1 (0, 0, 3.38) -> (0, 0, -1), fov 45, ScaleByAspect
//!   RenderObject1 (Pos 0, Rot 451.89 / -421.757 / -236.581, Scale 1.01251)
//!     Clear1 (0 0 0 0)
//!     MeshObject1 <- Cube1 (1 1 1)  +  Shader1 (surface)
//!     Vector1 <- Expression1 (three 720-degree noise axes)
//!   RenderRect2 -> RenderToTexture1 (Format 5, 100%; CreateMipMaps inert)
//!     Shader3 (zoom 0.95, * 0.85)        additive history
//!     Blur1 (Dir 2, Width 4, Brightness 1) <- CopyTexture1 (alpha 0)
//!       StoreTexture1 (SwapDelay 0) <- RenderToTexture1.Color  [previous frame]
//! RenderRect1 -> Screen1
//!   Clear2 (0 0 0 1)
//!   Shader2 (Cube capture + PreviousLayer1)
//! ```
//!
//! Each frame draws the cube into a full-scene-size single-mip `Rgba16Float`
//! capture, adds the blurred copy of the **previous** capture (zoom `0.95`,
//! brightness `0.85`), and composites it over black. `PreviousLayer1` is
//! transparent black in standalone playback but keeps its texture binding.
//!
//! # Ported exactly
//!
//! * Expression1 (`:290-295`), verbatim:
//!
//!   ```text
//!   perm = perm + ((min(0.03, band(-1, 0, 0, 0.03)*0.3+0.001))*deltatime);
//!   t = perm + permrand*10;
//!   out1=noise2(0,t)*720;
//!   out2=noise2(1,t)*720;
//!   out3=noise2(2,t)*720;
//!   ```
//!
//!   `permrand` is the node's `RandomSeed` (`1`), the three outs drive
//!   `Vector1` -> `RenderObject1.Rot` (whole-object degrees).
//! * Shader1's Expression (`:80-82`), verbatim:
//!
//!   ```text
//!   in1.x = min(0.4,beat(0)*10);
//!   in1.y = 0.1+beat((noise1(time*5)*0.5+0.5)*0.1)*10;
//!   ```
//!
//! * Shader1 (`:43-76`): the surface is
//!   `smoothstep(0.2, 1, _turbulencefast(vec3(p*10, gTime), 3)) * gIn1.y`,
//!   times `_noisefast(vec3(p, gTime*2))*0.5+0.4`, emitted as
//!   `vec4(vec3(v*2), 1)`. `_noisefast` aliases `_noise` (`P9Glsl.js:64`), so
//!   the shared `p9_noise` helper is the faithful call.
//! * `Shader3` (`:212-235`): `texture(gTexture1, (p*0.95)*0.5+0.5) * 0.85`.
//! * `CopyTexture1` (`:256-282`): `vec4(texture(gTexture1).rgb, 0.0)` — the
//!   copied history is alpha-zero so the cube capture's own alpha survives.
//! * `Blur1` (`:243-247`): `Dir 2` (both axes), `Width 4`, `Brightness 1`.
//! * `Shader2` (`:115-142`): `c = texture(gTexture1) * si.diffuse`,
//!   `bg = texture(gTexture2)`, `oColor = mix(bg, vec4(0.0), c.a) + c`.
//! * Camera/model (`:8-24`, `:85-97`): `CAMERA_POSITION`,
//!   `CAMERA_LOOK_AT` (relative — added to the position), fov 45, near 0.1,
//!   far 1000, `ScaleByAspect`; `MODEL_SCALE = [1.01251, 1.01251, 1]` and the
//!   `INITIAL_ROTATION` seed from `Vector1`.
//! * Blend states: Shader1 `SrcBlend 1 / DstBlend 0` (One / Zero, i.e.
//!   overwrite) with depth test/write off; Shader2 `1 / 4`
//!   (One / OneMinusSrcColor); Shader3 `1 / 1` (One / One, additive). Clear1
//!   is transparent black, Clear2 opaque black. Plane9's blend enum is the
//!   `P9Scene.js:1109-1118` ground truth (`0` Zero, `1` One, `2` SrcColor,
//!   `3` DstColor, `4` OneMinusSrcColor, …).
//! * The RTT (`:149-158`): `Format 5` is a four-channel half-float target
//!   (the `P9Scene.js` format note maps the vendor float formats to
//!   `Rgba16Float`), `Width 0 / Height 0` is **100% of the render target**.
//!   `CreateMipMaps` is recorded from the XML as `RTT_MIPS` but intentionally
//!   inert: the port follows the repository's single-mip / explicit LOD 0
//!   sampling convention, and every capture sample is 1:1 at LOD 0.
//! * Props (`sceneProps.js:282-426` has no Evil Box entry): exactly the six
//!   responsiveness levers plus **Lens**.
//!
//! # Design decisions
//!
//! * **Format 5 -> `Rgba16Float`.** The vendor float formats all behave as
//!   four-channel float storage in the DLL's FBOs (the same reading the Hollow
//!   Cube port documents); the additive history needs the headroom.
//! * **`CopyTexture1.TextureSize = 1` is ignored.** The vendor JS derives the
//!   copy target from the source's own dimensions (`P9Scene.js:copyTextureTo`)
//!   and never reads `TextureSize`, so the port copies at the source size.
//! * **`RTT_MIPS` is recorded metadata, not work.** `CreateMipMaps` is kept as
//!   `RTT_MIPS` from the XML, but the capture is single-mip: every capture,
//!   composite and feedback sample is 1:1 with explicit LOD 0, so no chain is
//!   generated.
//! * **The initial rotation is a seed.** Expression1 overwrites
//!   `RenderObject1.Rot` on the first `update`, exactly like the preset.
//! * **The Lens prop magnifies the projection.** The capture is full-scene and
//!   the only content in it is the cube and its feedback trail, so scaling the
//!   projection zooms everything the preset draws; `lens == 1.0` is
//!   bit-identical to the vendor framing.
//! * **The dead inline `_fbm` RTT shader is not implemented.** `RenderRect2`'s
//!   `Render` is connected to `RenderToTexture1.Render` (`:319-320`), so the
//!   cube chain replaces the RTT's own fragment, exactly as in Sound Cube.
//! * **No external assets.** The preset samples only its own captures.
//!
//! # Replaced three.js machinery
//!
//! | three.js | Rust |
//! |----------|------|
//! | `RenderToTexture1` + `CreateMipMaps` | one single-mip `Rgba16Float` texture (`RTT_MIPS` recorded, inert) |
//! | `StoreTexture1` ping-pong | two stored captures selected by [`EvilBox::feedback_order`] |
//! | `CopyTexture1` / `Blur1` | full-screen copy + separable gaussian passes |
//! | `RenderRect2` + Shader3 additive | the same pass with a One/One blend state |
//! | `Clear2` + Shader2 to the screen | a full-screen pass into the composer's scene target |
//! | `BoxGeometry` | [`build_box`](crate::point10::scenes::cube_mesh::build_box) |
//! | `get the renderer output` | the shared [`Composer`] output pass |

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{Composer, ToneMapping};
use crate::point10::scenes::cube_mesh::{
    Mat4, MeshGpu, build_box, euler_xyz, look_at, mat4_mul, perspective, scale,
};
use crate::point10::scenes::plane9_common::{
    PLANE9_DUAL_TEXTURE_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL, Plane9Uniforms,
    create_colour_target, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::{value_noise1, value_noise2};
use crate::point10::{
    FULLSCREEN_TRIANGLE_WGSL, Point10Scene, SceneContext, SceneMeta, SceneProp, apply_lens,
    frame_dt, lens_value, plane9_scene_props,
};

// ---------------------------------------------------------------------------
// Constants (`Cube/Evil Box.xml`)
// ---------------------------------------------------------------------------

/// `Screen1.CamPos`.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 3.38];
/// `Screen1.CamLookAt` — a direction relative to `CAMERA_POSITION`
/// (`CamLookAtInWorldSpace = false`).
pub const CAMERA_LOOK_AT: [f32; 3] = [0.0, 0.0, -1.0];
/// `Screen1.CamFov`.
pub const CAMERA_FOV: f32 = 45.0;
/// `Screen1.CamNear`.
pub const CAMERA_NEAR: f32 = 0.1;
/// `Screen1.CamFar`.
pub const CAMERA_FAR: f32 = 1000.0;
/// `Screen1.ScaleByAspect` — the fov is vertical and the aspect widens it.
pub const SCALE_BY_ASPECT: bool = true;

/// `RenderObject1.Scale`.
pub const MODEL_SCALE: [f32; 3] = [1.01251, 1.01251, 1.0];
/// `RenderObject1.Rot` / `Vector1` seed — Expression1 drives it after frame 0.
pub const INITIAL_ROTATION: [f32; 3] = [451.89, -421.757, -236.581];
/// `Cube1.Size`.
pub const CUBE_SIZE: [f32; 3] = [1.0, 1.0, 1.0];

/// `Expression1.RandomSeed` (`permrand`).
pub const PERMRAND: f32 = 1.0;

/// `RenderToTexture1.Format 5` — see the four-channel half-float note above.
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// `RenderToTexture1.Width`/`Height` mode (`0` = 100% of the scene size).
pub const RTT_MODE: u32 = 0;
/// `RenderToTexture1.CreateMipMaps` — recorded from the XML but intentionally
/// inert: the capture is single-mip and every sample uses explicit LOD 0.
pub const RTT_MIPS: bool = true;

/// `Blur1.Dir` (`2` = both axes).
pub const BLUR_DIRECTION: u32 = 2;
/// `Blur1.Width`.
pub const BLUR_WIDTH: f32 = 4.0;
/// `Blur1.Brightness`.
pub const BLUR_BRIGHTNESS: f32 = 1.0;

/// `StoreTexture1.SwapDelay`.
pub const STORE_SWAP_DELAY: f32 = 0.0;
/// `CopyTexture1.CreateMipMaps`.
pub const COPY_MIPS: bool = false;
/// `PreviousLayer1.CreateMipMaps`.
pub const PREVIOUS_LAYER_MIPS: bool = false;

/// The two node clear colours.
pub const CLEAR1: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};
/// `Clear2.Color 0 0 0 1`.
pub const CLEAR2: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// One frame of Expression1 (`:290-295`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvilBoxDrive {
    /// The accumulated `perm` for the next frame.
    pub perm: f32,
    /// `t = perm + permrand*10`.
    pub t: f32,
    /// `out1..3` — whole-object rotation in degrees.
    pub rotation: [f32; 3],
}

/// Pure Expression1: a damping-0 bass phase drives three `noise2` degrees axes.
pub fn evil_box_drive(band: f32, permrand: f32, dt: f32, perm: f32) -> EvilBoxDrive {
    let band = if band.is_finite() { band.max(0.0) } else { 0.0 };
    let step = if dt.is_finite() {
        dt.clamp(0.0, 0.1)
    } else {
        0.0
    };
    let base = if perm.is_finite() { perm } else { 1.0 };
    let permrand = if permrand.is_finite() {
        permrand
    } else {
        PERMRAND
    };
    let next = base + (band * 0.3 + 0.001).min(0.03) * step;
    let t = next + permrand * 10.0;

    EvilBoxDrive {
        perm: next,
        t,
        rotation: [
            value_noise2(t, 0.0) * 720.0,
            value_noise2(t, 1.0) * 720.0,
            value_noise2(t, 2.0) * 720.0,
        ],
    }
}

/// One frame of Shader1's Expression (`:80-82`).
///
/// `beat_x` is `beat(0)` (fallback 0), `beat_y` the noise-selected fallback
/// `(noise1(time*5)*0.5+0.5)*0.1`.
pub fn evil_box_surface(beat_x: f32, beat_y: f32) -> [f32; 4] {
    let x = if beat_x.is_finite() { beat_x } else { 0.0 };
    let y = if beat_y.is_finite() { beat_y } else { 0.1 };
    [(x * 10.0).min(0.4), 0.1 + y * 10.0, 0.0, 0.0]
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// A full-screen pass' uniform block: the shared Plane9 block.
type PassUniforms = Plane9Uniforms;

/// `RenderObject1` + Shader1's uniform block.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CubeUniforms {
    view_proj: Mat4,
    model: Mat4,
    time: f32,
    _pad0: f32,
    _pad1: [f32; 2],
    in1: [f32; 4],
    color1: [f32; 4],
}

/// A single-mip render target plus its view.
///
/// The capture (`RenderToTexture1`) uses this too: `create_colour_target` is
/// `mip_level_count: 1`, so `RTT_MIPS` stays recorded metadata with no chain.
struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Target {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32) -> Self {
        let (texture, view) = create_colour_target(device, label, width, height, RTT_FORMAT);
        Self {
            _texture: texture,
            view,
        }
    }
}

/// GPU resources built in `init` / `resize`.
struct EvilBoxGpu {
    queue: wgpu::Queue,

    // Shader1 surface capture.
    cube_pipeline: wgpu::RenderPipeline,
    cube_bind: wgpu::BindGroup,
    cube_uniforms: wgpu::Buffer,
    mesh: MeshGpu,

    // The shared full-screen pass layout (uniform, gTexture1, sampler).
    copy_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    zoom_pipeline: wgpu::RenderPipeline,
    store_pipeline: wgpu::RenderPipeline,
    copy_uniforms: wgpu::Buffer,
    blur_h_uniforms: wgpu::Buffer,
    blur_v_uniforms: wgpu::Buffer,
    zoom_uniforms: wgpu::Buffer,
    store_uniforms: wgpu::Buffer,
    /// `copy_binds[store]` reads one stored capture (`StoreTexture1` pair).
    copy_binds: [wgpu::BindGroup; 2],
    blur_h_bind: wgpu::BindGroup,
    blur_v_bind: wgpu::BindGroup,
    zoom_bind: wgpu::BindGroup,
    store_bind: wgpu::BindGroup,

    // Shader2 composite.
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind: wgpu::BindGroup,
    composite_uniforms: wgpu::Buffer,

    // Captures.
    rtt: Target,
    stores: [Target; 2],
    copy_target: Target,
    blur_a: Target,
    blur_b: Target,
    /// `PreviousLayer1`: transparent black in standalone playback.
    _previous: Target,

    composer: Composer,
}

/// A full-screen pipeline over the given shader entry point.
fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    entry: &str,
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
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn pass_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    uniforms: &wgpu::Buffer,
    source: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point10-plane9-evil-box-pass-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
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
}

/// The scene.
pub struct EvilBox {
    audio: P10Audio,
    /// Expression1's `perm`.
    perm: f32,
    /// `permrand` — the Expression node's `RandomSeed`.
    permrand: f32,
    /// `RenderObject1.Rot` in degrees (Expression1's `out1..3`).
    rotation: [f32; 3],
    /// Shader1's Expression outs (`in1.x`, `in1.y`).
    in1: [f32; 4],
    /// `gTime` — seconds since activation.
    elapsed: f32,
    lens: f32,
    /// Which `StoreTexture1` target the next frame writes.
    feedback_current: usize,
    camera_aspect: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    gpu: Option<EvilBoxGpu>,
}

impl Default for EvilBox {
    fn default() -> Self {
        Self::new()
    }
}

impl EvilBox {
    /// Create the scene at the preset's defaults.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            permrand: 1.0,
            rotation: INITIAL_ROTATION,
            in1: [0.0, 0.1, 0.0, 0.0],
            elapsed: 0.0,
            lens: 1.0,
            feedback_current: 0,
            camera_aspect: 1.0,
            size: (1, 1),
            target_format: crate::point10::HDR_FORMAT,
            device: None,
            queue: None,
            gpu: None,
        }
    }

    /// Apply the resolved modV props (no scene props; the six levers + Lens
    /// are read by the engine's schema and Lens is the only value to bind).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
    }

    /// `RenderObject1.Rot` in degrees.
    pub fn rotation(&self) -> [f32; 3] {
        self.rotation
    }

    /// Expression1's `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// Shader1's Expression outs.
    pub fn in1(&self) -> [f32; 4] {
        self.in1
    }

    /// The `StoreTexture1` ping-pong order `(previous, current)`.
    pub fn feedback_order(&self) -> (usize, usize) {
        (1 - self.feedback_current, self.feedback_current)
    }

    /// `_swapFeedback` — the frame just stored becomes the next source.
    pub fn swap_feedback(&mut self) {
        self.feedback_current = 1 - self.feedback_current;
    }

    fn view_proj(&self) -> Mat4 {
        let aspect = if SCALE_BY_ASPECT {
            self.camera_aspect
        } else {
            1.0
        };
        mat4_mul(
            &apply_lens(
                perspective(CAMERA_FOV.to_radians(), aspect, CAMERA_NEAR, CAMERA_FAR),
                self.lens,
            ),
            &look_at(
                CAMERA_POSITION,
                [
                    CAMERA_POSITION[0] + CAMERA_LOOK_AT[0],
                    CAMERA_POSITION[1] + CAMERA_LOOK_AT[1],
                    CAMERA_POSITION[2] + CAMERA_LOOK_AT[2],
                ],
                [0.0, 1.0, 0.0],
            ),
        )
    }

    fn model(&self) -> Mat4 {
        mat4_mul(
            &euler_xyz(
                self.rotation[0].to_radians(),
                self.rotation[1].to_radians(),
                self.rotation[2].to_radians(),
            ),
            &scale(MODEL_SCALE[0], MODEL_SCALE[1], MODEL_SCALE[2]),
        )
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        gpu.queue.write_buffer(
            &gpu.cube_uniforms,
            0,
            bytemuck::bytes_of(&CubeUniforms {
                view_proj: self.view_proj(),
                model: self.model(),
                time: self.elapsed,
                _pad0: 0.0,
                _pad1: [0.0; 2],
                in1: self.in1,
                color1: [1.0; 4],
            }),
        );

        let resolution = [self.size.0 as f32, self.size.1 as f32];
        let mut plain = Plane9Uniforms::new(self.size.0, self.size.1);
        plain.time = self.elapsed;
        gpu.queue
            .write_buffer(&gpu.copy_uniforms, 0, bytemuck::bytes_of(&plain));
        gpu.queue
            .write_buffer(&gpu.store_uniforms, 0, bytemuck::bytes_of(&plain));
        gpu.queue
            .write_buffer(&gpu.zoom_uniforms, 0, bytemuck::bytes_of(&plain));

        for (buffer, direction) in [
            (&gpu.blur_h_uniforms, [1.0, 0.0]),
            (&gpu.blur_v_uniforms, [0.0, 1.0]),
        ] {
            let mut blur = Plane9Uniforms::new(self.size.0, self.size.1);
            blur.time = self.elapsed;
            blur.in1 = [direction[0], direction[1], 0.0, 0.0];
            blur.in2 = [BLUR_WIDTH, BLUR_BRIGHTNESS, 0.0, 0.0];
            gpu.queue.write_buffer(buffer, 0, bytemuck::bytes_of(&blur));
        }

        let mut composite = Plane9Uniforms::new(self.size.0, self.size.1);
        composite.time = self.elapsed;
        composite.resolution = resolution;
        gpu.queue
            .write_buffer(&gpu.composite_uniforms, 0, bytemuck::bytes_of(&composite));
    }

    /// Recreate every size-dependent resource and re-bind the passes.
    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let (width, height) = self.size;

        // --- Shader1: the cube surface -----------------------------------
        let cube_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-evil-box-cube-shader"),
            source: wgpu::ShaderSource::Wgsl(cube_wgsl().into()),
        });
        let cube_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-evil-box-cube-layout"),
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
        });
        let cube_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-evil-box-cube-uniforms"),
            size: std::mem::size_of::<CubeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cube_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-evil-box-cube-bind"),
            layout: &cube_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: cube_uniforms.as_entire_binding(),
            }],
        });
        let cube_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-plane9-evil-box-cube-pipeline-layout"),
            bind_group_layouts: &[Some(&cube_layout)],
            immediate_size: 0,
        });
        // Shader1: SrcBlend 1 / DstBlend 0 (One / Zero = overwrite), depth
        // test/write off, CullMode 2 (three's FrontSide).
        let cube_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-evil-box-cube-pipeline"),
            layout: Some(&cube_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &cube_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(crate::point10::scenes::cube_mesh::Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &cube_shader,
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

        // --- the full-screen feedback/composite passes --------------------
        let pass_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-evil-box-pass-shader"),
            source: wgpu::ShaderSource::Wgsl(pass_wgsl().into()),
        });
        let pass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-evil-box-pass-layout"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let make_uniforms = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<PassUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let copy_uniforms = make_uniforms("point10-plane9-evil-box-copy-uniforms");
        let blur_h_uniforms = make_uniforms("point10-plane9-evil-box-blur-h-uniforms");
        let blur_v_uniforms = make_uniforms("point10-plane9-evil-box-blur-v-uniforms");
        let zoom_uniforms = make_uniforms("point10-plane9-evil-box-zoom-uniforms");
        let store_uniforms = make_uniforms("point10-plane9-evil-box-store-uniforms");

        let copy_pipeline = fullscreen_pipeline(
            &device,
            "point10-plane9-evil-box-copy-pipeline",
            &pass_shader,
            &pass_layout,
            RTT_FORMAT,
            "fs_copy",
            None,
        );
        let blur_pipeline = fullscreen_pipeline(
            &device,
            "point10-plane9-evil-box-blur-pipeline",
            &pass_shader,
            &pass_layout,
            RTT_FORMAT,
            "fs_blur",
            None,
        );
        // Shader3: SrcBlend 1 / DstBlend 1 (One / One, additive).
        let additive = wgpu::BlendState {
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
        };
        let zoom_pipeline = fullscreen_pipeline(
            &device,
            "point10-plane9-evil-box-zoom-pipeline",
            &pass_shader,
            &pass_layout,
            RTT_FORMAT,
            "fs_zoom",
            Some(additive),
        );
        let store_pipeline = fullscreen_pipeline(
            &device,
            "point10-plane9-evil-box-store-pipeline",
            &pass_shader,
            &pass_layout,
            RTT_FORMAT,
            "fs_store",
            None,
        );

        // --- Shader2: the two-texture composite --------------------------
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-evil-box-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(composite_wgsl().into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-evil-box-composite-layout"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let composite_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-evil-box-composite-uniforms"),
            size: std::mem::size_of::<PassUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Shader2: SrcBlend 1 / DstBlend 4 (One / OneMinusSrcColor).
        let composite_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let composite_pipeline = fullscreen_pipeline(
            &device,
            "point10-plane9-evil-box-composite-pipeline",
            &composite_shader,
            &composite_layout,
            crate::point10::HDR_FORMAT,
            "fs_main",
            Some(composite_blend),
        );

        // --- captures (`RenderToTexture1`, `StoreTexture1`, `CopyTexture1`,
        // `Blur1`, `PreviousLayer1`) -------------------------------------
        let rtt = Target::new(&device, "point10-plane9-evil-box-rtt", width, height);
        let stores = [
            Target::new(&device, "point10-plane9-evil-box-store-a", width, height),
            Target::new(&device, "point10-plane9-evil-box-store-b", width, height),
        ];
        let copy_target = Target::new(&device, "point10-plane9-evil-box-copy", width, height);
        let blur_a = Target::new(&device, "point10-plane9-evil-box-blur-a", width, height);
        let blur_b = Target::new(&device, "point10-plane9-evil-box-blur-b", width, height);
        let previous = Target::new(&device, "point10-plane9-evil-box-previous-layer", 1, 1);

        let sampler = plane9_sampler(&device, "point10-plane9-evil-box-sampler");
        let copy_binds = [
            pass_bind(
                &device,
                &pass_layout,
                &sampler,
                &copy_uniforms,
                &stores[0].view,
            ),
            pass_bind(
                &device,
                &pass_layout,
                &sampler,
                &copy_uniforms,
                &stores[1].view,
            ),
        ];
        let blur_h_bind = pass_bind(
            &device,
            &pass_layout,
            &sampler,
            &blur_h_uniforms,
            &copy_target.view,
        );
        let blur_v_bind = pass_bind(
            &device,
            &pass_layout,
            &sampler,
            &blur_v_uniforms,
            &blur_a.view,
        );
        let zoom_bind = pass_bind(
            &device,
            &pass_layout,
            &sampler,
            &zoom_uniforms,
            &blur_b.view,
        );
        let store_bind = pass_bind(&device, &pass_layout, &sampler, &store_uniforms, &rtt.view);

        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-evil-box-composite-bind"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: composite_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&rtt.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&previous.view),
                },
            ],
        });

        let mesh = MeshGpu::new(
            &device,
            "point10-plane9-evil-box-mesh",
            &build_box(1.0, 1.0, 1.0),
        );

        let mut composer = Composer::new(&device, &queue, width, height, self.target_format);
        composer.output_mut().set_tone_mapping(ToneMapping::None);

        self.gpu = Some(EvilBoxGpu {
            queue,
            cube_pipeline,
            cube_bind,
            cube_uniforms,
            mesh,
            copy_pipeline,
            blur_pipeline,
            zoom_pipeline,
            store_pipeline,
            copy_uniforms,
            blur_h_uniforms,
            blur_v_uniforms,
            zoom_uniforms,
            store_uniforms,
            copy_binds,
            blur_h_bind,
            blur_v_bind,
            zoom_bind,
            store_bind,
            composite_pipeline,
            composite_bind,
            composite_uniforms,
            rtt,
            stores,
            copy_target,
            blur_a,
            blur_b,
            _previous: previous,
            composer,
        });
        self.upload_uniforms();
    }
}

impl Point10Scene for EvilBox {
    fn name(&self) -> &'static str {
        "Plane9 Evil Box"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.camera_aspect = ctx.aspect();
        self.feedback_current = 0;
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

        // Expression1: the damping-0 bass phase drives the three noise axes.
        let drive = evil_box_drive(
            self.audio.band(-1, 0, 0, 0.03),
            self.permrand,
            dt,
            self.perm,
        );
        self.perm = drive.perm;
        self.rotation = drive.rotation;

        // Shader1's Expression: `beat(0)` and the noise-selected fallback.
        let time = self.elapsed;
        let beat_x = self.audio.beat(0.0);
        let beat_y = self
            .audio
            .beat((value_noise1(time * 5.0) * 0.5 + 0.5) * 0.1);
        self.in1 = evil_box_surface(beat_x, beat_y);

        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let (previous, current) = self.feedback_order();
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };

        // `StoreTexture1` + `CopyTexture1`: copy last frame's capture with the
        // vendor's forced zero alpha (`:277`).
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-evil-box-copy-texture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.copy_target.view,
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
            pass.set_bind_group(0, &gpu.copy_binds[previous], &[]);
            pass.draw(0..3, 0..1);
        }

        // `Blur1` (Dir 2 = both axes; Width 4, Brightness 1).
        for (bind, view, label) in [
            (
                &gpu.blur_h_bind,
                &gpu.blur_a.view,
                "point10-plane9-evil-box-blur-h",
            ),
            (
                &gpu.blur_v_bind,
                &gpu.blur_b.view,
                "point10-plane9-evil-box-blur-v",
            ),
        ] {
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
            pass.set_pipeline(&gpu.blur_pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }

        // `Clear1` + `RenderObject1`: the cube surface into the capture.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-evil-box-cube"),
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
            pass.set_pipeline(&gpu.cube_pipeline);
            pass.set_bind_group(0, &gpu.cube_bind, &[]);
            gpu.mesh.draw(&mut pass);
        }

        // `Shader3` additively layers the blurred, zoomed history.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-evil-box-feedback"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.rtt.view,
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
            pass.set_pipeline(&gpu.zoom_pipeline);
            pass.set_bind_group(0, &gpu.zoom_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        // `Clear2` + `Shader2`: the capture over opaque black.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-evil-box-composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: gpu.composer.scene_view(),
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

        // `StoreTexture1`: keep this frame's capture for the next frame.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-evil-box-store-texture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.stores[current].view,
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
            pass.set_pipeline(&gpu.store_pipeline);
            pass.set_bind_group(0, &gpu.store_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        // The capture just stored becomes the next frame's history source
        // (`_swapFeedback`).
        self.feedback_current = 1 - self.feedback_current;

        if let Some(gpu) = self.gpu.as_mut() {
            gpu.composer.render(encoder, target);
        }
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.feedback_current = 0;
        self.elapsed = 0.0;
        self.perm = 1.0;
        self.rotation = INITIAL_ROTATION;
        self.in1 = [0.0, 0.1, 0.0, 0.0];
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(&[])
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Evil Box", "3d-scenes")
    }
}

/// Shader1 with the Plane9 helpers (`_noise`/`_turbulencefast`/`smoothstep`).
fn cube_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{EVIL_BOX_CUBE_WGSL}")
}

/// The copy / blur / zoom / store passes over the shared Plane9 block.
fn pass_wgsl() -> String {
    format!(
        "{FULLSCREEN_TRIANGLE_WGSL}\n{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{EVIL_BOX_PASS_WGSL}"
    )
}

/// Shader2 with the two-texture Plane9 declarations.
fn composite_wgsl() -> String {
    format!(
        "{FULLSCREEN_TRIANGLE_WGSL}\n{PLANE9_UNIFORMS_WGSL}\n{PLANE9_DUAL_TEXTURE_WGSL}\n{EVIL_BOX_COMPOSITE_WGSL}"
    )
}

/// Shader1 (`Cube/Evil Box.xml:43-76`).
const EVIL_BOX_CUBE_WGSL: &str = r#"
struct CubeUniforms {
  view_proj: mat4x4<f32>,
  model: mat4x4<f32>,
  time: f32,
  _pad0: f32,
  _pad1: vec2<f32>,
  in1: vec4<f32>,
  color1: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: CubeUniforms;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) tex: vec2<f32>,
  @location(1) diffuse: vec4<f32>,
};

@vertex
fn vs_main(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
) -> VsOut {
  var out: VsOut;
  out.tex = uv;
  // `so.diffuse = iColor * gColor1 * gColor` — all white in this preset.
  out.diffuse = u.color1;
  out.position = u.view_proj * (u.model * vec4<f32>(position, 1.0));
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let p = in.tex*2.0-1.0;
  var v = (p9_smoothstep(0.2, 1.0, p9_turbulence(vec3<f32>(p*10.0, u.time), 3u)))*u.in1.y;
  v = v * (p9_noise(vec3<f32>(p, u.time*2.0))*0.5+0.4);
  return vec4<f32>(vec3<f32>(v*2.0), 1.0);
}
"#;

/// Shader3 + `CopyTexture1` + `Blur1` + `StoreTexture1`.
const EVIL_BOX_PASS_WGSL: &str = r#"
// `CopyTexture1` (`:273-278`): copy RGB and force alpha zero.
@fragment
fn fs_copy(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  return vec4<f32>(textureSampleLevel(g_texture1, g_sampler, tex, 0.0).rgb, 0.0);
}

// `Blur1` (`:243-247`): a separable gaussian; `Dir 2` runs one pass per axis.
@fragment
fn fs_blur(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let step = u.in1.xy / u.resolution;
  let width2 = u.in2.x*u.in2.x;
  var sum = vec4<f32>(0.0);
  var total = 0.0;
  for (var i = -6; i <= 6; i = i + 1) {
    let fi = f32(i);
    let weight = exp(-0.5 * fi * fi / width2);
    sum = sum + textureSampleLevel(g_texture1, g_sampler, tex + step * fi, 0.0) * weight;
    total = total + weight;
  }
  return vec4<f32>((sum / total).rgb * u.in2.y, 1.0);
}

// Shader3 (`:229-235`): the previous capture, zoomed and dimmed.
@fragment
fn fs_zoom(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * 2.0 - 1.0;
  return textureSampleLevel(g_texture1, g_sampler, (p*0.95)*0.5+0.5, 0.0) * 0.85;
}

// `StoreTexture1` (`:298-300`): keep the capture as-is.
@fragment
fn fs_store(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  return textureSampleLevel(g_texture1, g_sampler, tex, 0.0);
}
"#;

/// Shader2 (`:115-142`): the capture over `PreviousLayer1`.
const EVIL_BOX_COMPOSITE_WGSL: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  // `si.diffuse = iColor * gColor1 * gColor`, all white in this preset.
  let diffuse = u.color1;
  let c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * diffuse;
  let bg = textureSampleLevel(g_texture2, g_sampler, tex, 0.0);
  return mix(bg, vec4<f32>(0.0), c.a) + c;
}
"#;
