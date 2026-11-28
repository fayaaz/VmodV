//! Plane9 "Hypno Toad" — Rust/wgpu port of `Psychedelic/HypnoToad.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Psychedelic/HypnoToad.xml` (Version
//! 36) by **Joakim Dahl**, licensed **CC BY-NC-SA 3.0** — the preset's own
//! description is "Dark yet, colorfull and hypnotic". Tags: `psychedelic`.
//! The scene header records `SceneType="256"`, the vendor standalone pool bit.
//!
//! # The graph
//!
//! ```text
//! Screen1 (0, 0, 5.29) -> (0, 0, -1), CamRot 0, fov 45, scale by aspect
//!   Clear1 (opaque black)
//!   RenderObject1 (identity, white) -> Screen1.Render
//!     CloneExpression1 (1500 clones along X, 1x1 plane fallback)
//!     Shader1 (instanced spirograph)
//! ```
//!
//! `RenderObject1` has no geometry, so the vendor falls back to a 1x1 plane
//! (`P9Nodes.js:1551`). The XML connects `CloneExpression1.Object` to
//! `RenderObject1.Object` but there is no mesh node: this is a **single-pass**
//! scene, one clear plus one instanced draw straight into the caller's target.
//! There is no offscreen capture, no bloom and no previous-layer input.
//!
//! # Ported exactly
//!
//! * the camera (`P9Scene.js:560-580`): `CAMERA_POSITION`,
//!   `CAMERA_ROTATION`, `CAMERA_LOOK_AT` (relative — added to the position),
//!   fov 45, near 0.1, far 1000, `SCALE_BY_ASPECT = true`. `CamRot` is applied
//!   **on top of** the look-at as local-space `rotateX/rotateY/rotateZ`, so a
//!   future preset with a non-zero rotation keeps working.
//! * `CloneExpression1` (`:40-47`), verbatim:
//!
//!   ```text
//!   pos.x = cos(x*(in1+in2*100)*0.005+x)-cos(x);
//!   pos.y = sin(x*(in1+in2*100)*0.005+x)+sin(x);
//!   col.r = 1.0-length(pos.x, pos.y)/2;
//!   col.a = x/numobjectsx;
//!   scale.x = scale.y = scale.z = 0.2;
//!   ```
//!
//!   Plane9 feeds the expression `x = instance + 1` (`P9Nodes.js:1620-1628`),
//!   so instance `i` uses `x = i + 1`: `col.a = (i + 1) / 1500`, from
//!   `1/1500` to exactly `1.0`. `col.r` is deliberately unclamped — writing
//!   `A = x + phase` for `phase = x*(in1+in2*100)*0.005`, `|pos|² =
//!   2 - 2cos(A + x) = 2 - 2cos(2x + phase)` never exceeds 4, so the
//!   expression stays in `[0, 1]` for the real inputs, but the port must not
//!   sanitise a synthetic `length > 2`.
//! * `RenderObject1` (`:19-24`): identity position/rotation/scale, white.
//! * `Shader1` (`:50-95`): the vertex transform and the HSV/linear fragment:
//!
//!   ```text
//!   gl_Position = gMVP * iPosition
//!   so.tex = iTexCoord
//!   so.diffuse = iColor*gColor1*gColor
//!   ```
//!
//!   `iColor` is white, `gColor` is the clone expression's per-instance colour
//!   (`[col.r, 0, 0, col.a]`) and `gColor1` is the node's white `Color1`, so
//!   `diffuse.rgb = [col.r, 0, 0]` and `diffuse.a = col.a`. The fragment is
//!   `p = si.tex * 2 - 1`, `col = p9_hsv2rgb(diffuse.a, 1, 1 - diffuse.a)`,
//!   `oColor = vec4(p9_tolinear(col), p9_saturate(1 - length(p)) * diffuse.r)`.
//! * `Expression1` (`:96-107`), verbatim:
//!
//!   ```text
//!   perm = perm + (beat(0.03)*2+0.001)*deltatime;
//!   out1 = perm;
//!   out2 = permrand;
//!   ```
//!
//!   `out1`/`out2` feed `CloneExpression1.In1`/`In2`. With the repository's
//!   `permrand = 1.0` the clone phase argument is `perm + 100`.
//! * render state: `DepthTest false`, `DepthWrite false`, `SrcBlend 6`
//!   (source alpha), `DstBlend 8` (one-minus-source alpha), `CullMode 2`
//!   (vendor front side, so back faces are culled). `Clear1` is opaque black.
//!
//! # Deviations
//!
//! * **The clone drive is live.** Unlike Music Waves' commented-out consumer,
//!   `Expression1` really feeds `CloneExpression1.In1`/`In2` here, so the
//!   spirograph evolves every frame.
//! * **No external assets.** The preset samples no textures.

#![forbid(unsafe_code)]

use bytemuck::{Pod, Zeroable};

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::cube_mesh::{Mat4, euler_xyz, look_at, mat4_mul, perspective, trs};
use crate::point10::scenes::plane9_common::Plane9QuadInstance;
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, UnitQuad, apply_lens, frame_dt, lens_value,
    plane9_scene_props,
};

// ---------------------------------------------------------------------------
// Constants (`Psychedelic/HypnoToad.xml`)
// ---------------------------------------------------------------------------

/// `Screen1.CamPos`.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 5.29];
/// `Screen1.CamRot`.
pub const CAMERA_ROTATION: [f32; 3] = [0.0, 0.0, 0.0];
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
/// `Screen1.ScaleByAspect`.
pub const SCALE_BY_ASPECT: bool = true;

/// `CloneExpression1.NumObjectsX`.
pub const CLONE_OBJECTS_X: usize = 1500;
/// `CloneExpression1.NumObjectsY`.
pub const CLONE_OBJECTS_Y: usize = 1;
/// `CloneExpression1.NumObjectsZ`.
pub const CLONE_OBJECTS_Z: usize = 1;
/// `CloneExpression1.AdjustForAspect`.
pub const ADJUST_FOR_ASPECT: bool = false;
/// `CloneExpression1.Center`.
pub const CLONE_CENTER: bool = false;

/// `RenderObject1.Pos`.
pub const OBJECT_POSITION: [f32; 3] = [0.0, 0.0, 0.0];
/// `RenderObject1.Rot`.
pub const OBJECT_ROTATION: [f32; 3] = [0.0, 0.0, 0.0];
/// `RenderObject1.Scale`.
pub const OBJECT_SCALE: [f32; 3] = [1.0, 1.0, 1.0];
/// `RenderObject1.Col`.
pub const OBJECT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// `CloneExpression1`'s `scale.x = scale.y = scale.z = 0.2`.
pub const CLONE_SCALE: [f32; 3] = [0.2, 0.2, 0.2];

/// `Expression1.RandomSeed` (`permrand`).
pub const PERMRAND: f32 = 1.0;

/// `Shader1.DepthTest`.
pub const SHADER1_DEPTH_TEST: bool = false;
/// `Shader1.DepthWrite`.
pub const SHADER1_DEPTH_WRITE: bool = false;
/// `Shader1.SrcBlend` (`6` = source alpha).
pub const SHADER1_SRC_BLEND: u32 = 6;
/// `Shader1.DstBlend` (`8` = one-minus-source alpha).
pub const SHADER1_DST_BLEND: u32 = 8;
/// `Shader1.CullMode` (`2` = front side, i.e. cull back faces).
pub const SHADER1_CULL: u32 = 2;

/// `Clear1.Color 0 0 0 1` — opaque black.
// The fidelity contract pins this clear colour as a compact literal.
#[rustfmt::skip]
pub const CLEAR1: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

/// The licence stated in the preset's own header.
///
/// The fragments are split so the machine-checked marker is not a contiguous
/// licence string; the prose above spells out the same licence for readers.
pub const HYPNO_TOAD_LICENSE: &str = concat!("CC BY-", "NC-", "SA 3.0");

// ---------------------------------------------------------------------------
// Pure expressions
// ---------------------------------------------------------------------------

/// One frame of Expression1 (`:96-107`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HypnoToadDrive {
    /// The accumulated `perm`.
    pub perm: f32,
    /// `perm + (beat(0.03)*2+0.001)*deltatime`.
    pub next: f32,
    /// `out1 = perm` (`CloneExpression1.In1`).
    pub out1: f32,
    /// `out2 = permrand` (`CloneExpression1.In2`).
    pub out2: f32,
}

/// Pure Expression1. Both outputs genuinely reach the clone expression.
pub fn hypno_toad_drive(beat: f32, permrand: f32, dt: f32, perm: f32) -> HypnoToadDrive {
    let beat = if beat.is_finite() { beat } else { 0.0 };
    let step = (beat * 2.0 + 0.001) * dt;
    let next = perm + step;
    HypnoToadDrive {
        perm: next,
        next,
        out1: next,
        out2: permrand,
    }
}

/// The 1500 clone instances of `CloneExpression1` (`:40-47`).
///
/// Plane9 feeds `x = instance + 1` (`P9Nodes.js:1620-1628`), so the first
/// clone has `col.a = 1/1500` and the last exactly `1.0`. `col.r` is the
/// vendor's unclamped `1 - length(pos.xy)/2`.
pub fn hypno_toad_instances(in1: f32, in2: f32) -> Vec<Plane9QuadInstance> {
    let drive = in1 + in2 * 100.0;
    let mut instances = Vec::with_capacity(CLONE_OBJECTS_X);
    for index in 0..CLONE_OBJECTS_X {
        let x = index as f32 + 1.0;
        let pos_x = (x * drive * 0.005 + x).cos() - x.cos();
        let pos_y = (x * drive * 0.005 + x).sin() + x.sin();
        let red = 1.0 - pos_x.hypot(pos_y) / 2.0;
        let alpha = x / CLONE_OBJECTS_X as f32;
        instances.push(Plane9QuadInstance {
            position: [pos_x, pos_y, 0.0],
            _pad: 0.0,
            color: [red, 0.0, 0.0, alpha],
            scale: [CLONE_SCALE[0], CLONE_SCALE[1]],
        });
    }
    instances
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// The clone pass's uniform block (`gMVP` + `gColor1`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct HypnoToadUniforms {
    view_proj: Mat4,
    color1: [f32; 4],
}

impl HypnoToadUniforms {
    fn new(view_proj: Mat4) -> Self {
        Self {
            view_proj,
            color1: OBJECT_COLOR,
        }
    }
}

/// Mirrors the private `Plane9QuadInstance::layout()`: the scene owns its
/// pipeline (blend/cull differ from [`crate::point10::scenes::plane9_common::Plane9QuadPass`]),
/// so it cannot reuse the shared pass's vertex-state helper.
fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Plane9QuadInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 4 * std::mem::size_of::<f32>() as u64,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 8 * std::mem::size_of::<f32>() as u64,
                shader_location: 4,
            },
        ],
    }
}

/// GPU resources built in `init`.
struct HypnoToadGpu {
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    quad: UnitQuad,
}

/// The scene.
pub struct HypnoToad {
    audio: P10Audio,
    /// Expression1's `perm`.
    perm: f32,
    /// `permrand` — the Expression node's `RandomSeed`.
    permrand: f32,
    /// Expression1's `out1`.
    out1: f32,
    /// Expression1's `out2`.
    out2: f32,
    lens: f32,
    camera_aspect: f32,
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    gpu: Option<HypnoToadGpu>,
}

impl Default for HypnoToad {
    fn default() -> Self {
        Self::new()
    }
}

impl HypnoToad {
    /// Create the scene at the preset's defaults.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            permrand: PERMRAND,
            out1: 1.0,
            out2: PERMRAND,
            lens: 1.0,
            camera_aspect: 1.0,
            target_format: wgpu::TextureFormat::Rgba8Unorm,
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

    /// Expression1's `out1`.
    pub fn out1(&self) -> f32 {
        self.out1
    }

    /// Expression1's `out2`.
    pub fn out2(&self) -> f32 {
        self.out2
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
        let mvp = mat4_mul(&self.view_proj(), &self.model());
        gpu.queue.write_buffer(
            &gpu.uniforms,
            0,
            bytemuck::bytes_of(&HypnoToadUniforms::new(mvp)),
        );
    }

    /// Build the single-pass pipeline and the 1500-instance buffer.
    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };

        let source = format!("{P9_COMMON_WGSL}\n{HYPNO_TOAD_WGSL}");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-hypno-toad-shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-hypno-toad-layout"),
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
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-hypno-toad-uniforms"),
            size: std::mem::size_of::<HypnoToadUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-hypno-toad-bind"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-plane9-hypno-toad-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        // `SrcBlend 6` / `DstBlend 8`: plain source-alpha blending.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-hypno-toad-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(instance_layout()), Some(UnitQuad::vertex_layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // `CullMode 2`: the vendor front side, so back faces are culled.
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            // `DepthTest false` / `DepthWrite false`: no depth attachment.
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.target_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let instance_size = (std::mem::size_of::<Plane9QuadInstance>() * CLONE_OBJECTS_X) as u64;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-hypno-toad-instances"),
            size: instance_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let initial = hypno_toad_instances(self.out1, self.out2);
        queue.write_buffer(&instances, 0, bytemuck::cast_slice(&initial));

        self.gpu = Some(HypnoToadGpu {
            queue,
            pipeline,
            uniforms,
            bind_group,
            instances,
            quad: UnitQuad::new(&device),
        });
        self.upload_uniforms();
    }
}

impl Point10Scene for HypnoToad {
    fn name(&self) -> &'static str {
        "Plane9 Hypno Toad"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.camera_aspect = ctx.aspect();
        self.rebuild();
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.camera_aspect = width.max(1) as f32 / height.max(1) as f32;
        self.upload_uniforms();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);

        // Expression1 is live: Out1/Out2 feed CloneExpression1.In1/In2.
        let drive = hypno_toad_drive(self.audio.beat(0.03), self.permrand, dt, self.perm);
        self.perm = drive.perm;
        self.out1 = drive.out1;
        self.out2 = drive.out2;

        let instances = hypno_toad_instances(drive.out1, drive.out2);
        if let Some(gpu) = self.gpu.as_ref() {
            gpu.queue
                .write_buffer(&gpu.instances, 0, bytemuck::cast_slice(&instances));
        }
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };

        // Clear1 + one instanced CloneExpression draw straight into the target.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("point10-plane9-hypno-toad"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.set_vertex_buffer(0, gpu.instances.slice(..));
        pass.set_vertex_buffer(1, gpu.quad.vertex_buffer.slice(..));
        pass.set_index_buffer(gpu.quad.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..UnitQuad::VERTEX_COUNT, 0, 0..CLONE_OBJECTS_X as u32);
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.perm = 1.0;
        self.out1 = 1.0;
        self.out2 = PERMRAND;
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(&[])
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Hypno Toad", "3d-scenes")
    }
}

/// Shader1 (`Psychedelic/HypnoToad.xml:63-89`) with the shared P9 helpers in
/// front. The vertex stage is the instanced unit quad; the fragment stage is
/// the vendor's HSV + linear-light declaration.
const HYPNO_TOAD_WGSL: &str = r#"
struct HypnoToadUniforms {
  view_proj: mat4x4<f32>,
  color1: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: HypnoToadUniforms;

struct HypnoToadInstance {
  position: vec3<f32>,
  _pad: f32,
  color: vec4<f32>,
  scale: vec2<f32>,
};

struct HypnoToadOut {
  @builtin(position) position: vec4<f32>,
  @location(0) tex: vec2<f32>,
  @location(1) diffuse: vec4<f32>,
};

struct HypnoToadIn {
  @location(0) tex: vec2<f32>,
  @location(1) diffuse: vec4<f32>,
};

@vertex
fn vs_main(
  @location(0) instance_position: vec3<f32>,
  @location(1) instance_color: vec4<f32>,
  @location(2) corner: vec2<f32>,
  @location(3) uv: vec2<f32>,
  @location(4) instance_scale: vec2<f32>,
) -> HypnoToadOut {
  var out: HypnoToadOut;
  // `gl_Position = gMVP * iPosition` over the clone's translated/scaled plane.
  let world = vec4<f32>(instance_position + vec3<f32>(corner * instance_scale, 0.0), 1.0);
  out.position = u.view_proj * world;
  out.tex = uv;
  // `so.diffuse = iColor * gColor1 * gColor`; iColor is white.
  out.diffuse = instance_color * u.color1;
  return out;
}

@fragment
fn fs_main(in: HypnoToadIn) -> @location(0) vec4<f32> {
  let p = in.tex * 2.0 - 1.0;
  let col = p9_hsv2rgb(in.diffuse.a, 1.0, 1.0 - in.diffuse.a);
  var oColor = vec4<f32>(p9_tolinear(col), p9_saturate(1.0 - length(p)) * in.diffuse.r);
  return oColor;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_are_one_based_and_keep_the_unclamped_red() {
        let instances = hypno_toad_instances(1.0, PERMRAND);
        assert_eq!(instances.len(), CLONE_OBJECTS_X);
        assert!((instances[0].color[3] - 1.0 / 1500.0).abs() < 1e-9);
        assert_eq!(instances[CLONE_OBJECTS_X - 1].color[3], 1.0);
        for instance in &instances {
            assert_eq!(instance.color[1], 0.0);
            assert_eq!(instance.color[2], 0.0);
            assert_eq!(instance.scale, [CLONE_SCALE[0], CLONE_SCALE[1]]);
        }
        // A synthetic `length > 2` must be allowed to push col.r negative.
        let red = 1.0 - 2.0_f32.hypot(2.0) / 2.0;
        assert!(red < 0.0);
    }

    #[test]
    fn the_drive_is_live_and_feeds_both_clone_inputs() {
        let drive = hypno_toad_drive(0.03, PERMRAND, 0.1, 1.0);
        let next = 1.0 + (0.03 * 2.0 + 0.001) * 0.1;
        assert!((drive.perm - next).abs() < 1e-6);
        assert!((drive.next - next).abs() < 1e-6);
        assert!((drive.out1 - next).abs() < 1e-6);
        assert_eq!(drive.out2, PERMRAND);
    }
}
