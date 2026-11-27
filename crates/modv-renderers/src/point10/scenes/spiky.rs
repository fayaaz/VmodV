//! Plane9 "Spiky" — Rust/wgpu port of `Things/Spiky.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Things/Spiky.xml` by Joakim Dahl,
//! CC BY-NC-SA 4.0. A subdivided, extruded, randomized cube becomes a ball of
//! spikes, lit with Plane9's directional/roughness model and the Stormy Days
//! cubemap, captured to a Format 5 (`Rgba16Float`) target, bloomed and
//! composited over the layers below.
//!
//! # Scene graph
//!
//! ```text
//! Cube1 (1x1x1) -> Subdivide2 (1, smooth) -> SelectMesh1 (all)
//!   -> Extrude2 (2 steps, 0.88, LocalScale 0) -> Randomize1 (0.1)
//!   -> Subdivide1 (2 steps, smooth) -> TransformEx1 (2 2 1)
//!   -> MeshObject1 -> RenderObject1 (0.5 scale, Expression1 rotation)
//! ```
//!
//! The mesh chain lives in [`super::mesh_ops`]. `TransformEx1`'s `Scale 2 2 1`
//! is **not** baked into the vertex data: it is a model-matrix factor, because
//! [`SPIKY_BALL_WGSL`]'s tip mask reads the pre-transform `iPosition`
//! (`d = (length(position) - 0.5) / 0.75`, red at `d > 0.9`). Baking the scale
//! would push the whole surface past the mask and turn the ball solid red.
//!
//! # Ported exactly
//!
//! * the camera (`0 0 1`, look `0 0 -1` *relative*, FOV 70, near 0.1, far
//!   1000, `ScaleByAspect false` means the aspect still comes from the target);
//! * `Clear1`/`Clear2` transparent black;
//! * `Shader1`'s vertex displacement, varyings and lighting (including the
//!   `Specular_F_Roughness`/`_perturbNormal` pair and the two distant lights);
//! * the red tip term (`mix(c1, c2, saturate(gIn1.x))`, `c2 = red * 10` when
//!   `d > 0.9`);
//! * `Shader1`'s expression (`min(0.5, band(-1,1,0, noise1(time*0.1)*0.7)*5)`,
//!   `in1.y = beat(0)`), `Shader2`'s (`b = band(-1,0,0,0)*1`,
//!   `in1.x = min(0.01, b*b*b*5)`) and `Expression1`'s noise orbit;
//! * `RenderToTexture1` Format 5 (`Rgba16Float`) at 100% of the screen with no
//!   mips (its inline fbm shader is dead — the render chain wins);
//! * `Bloom1` `(radius 5, threshold 0, smoothness 0.5, intensity 1,
//!   antiflicker false)` — one mip, so intenser is inert;
//! * `FileTexture2` `data\Stormy days` from the bundled six-face cubemap.
//!
//! # Replaced three.js machinery
//!
//! | Plane9 | Rust |
//! | --- | --- |
//! | `RenderToTexture` + `MeshObject` | [`wgpu`] ball pass into an `Rgba16Float` target |
//! | `Bloom1` feeding `Shader2.Texture1` | a standalone [`Plane9BloomPass`] over the capture |
//! | `PreviousLayer1.Color` | transparent black; the compositor's alpha blend supplies the layer below |
//! | `PerspectiveCamera` | [`cube_mesh::perspective`]/[`cube_mesh::look_at`] |
//!
//! # Deviations
//!
//! * **`_noise(vec2)` in `Shader2`** is evaluated as `p9_noise(vec3(x, y, 0))`,
//!   since only the 3D simplex form is in the shared WGSL subset. The warp is
//!   a small screen-space offset, so this is visually equivalent.
//! * **`Shader2` applies the sRGB OETF when the destination is linear**, the
//!   same rule as [`crate::point10::postfx::OutputPass`]; an `*Srgb` target
//!   gets the hardware transfer instead (no double encode).
//! * **The mesh chain is a reconstruction.** Plane9's `P9Nodes.js` ships the
//!   mesh modifiers as passthroughs, so the spike count/taper is rebuilt from
//!   the XML ports rather than ported line for line; see [`super::mesh_ops`].
//! * **`PreviousLayer1.Color` is transparent black** (no previous-layer
//!   texture in this architecture), as `hollow_cube.rs` documents.
//!
//! [`Plane9BloomPass`]: crate::point10::postfx::Plane9BloomPass

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{Plane9BloomParams, Plane9BloomPass};
use crate::point10::scenes::cube_mesh::{
    Mat4, MeshGpu, Vertex, build_box, create_depth, look_at, mat4_mul, perspective, scale, trs,
};
use crate::point10::scenes::mesh_ops::{
    self, ExtrudeParams, IndexedMesh, RandomizeParams, SelectParams, SubdivideParams,
};
use crate::point10::scenes::plane9_common::{
    PLANE9_TONEMAP_WGSL, load_stormy_cube, plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::p9_shading::P9_SHADING_WGSL;
use crate::point10::shared::random::{value_noise1, value_noise2};
use crate::point10::{
    FULLSCREEN_TRIANGLE_WGSL, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt,
    scene_props_with_levers,
};

// ---------------------------------------------------------------------------
// XML constants (`Things/Spiky.xml`)
// ---------------------------------------------------------------------------

/// Registry name.
pub const NAME: &str = "Plane9 Spiky";

/// `Screen1.CamPos`.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, 1.0];
/// `Screen1.CamLookAt` (a direction, `CamLookAtInWorldSpace false`).
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, -1.0];
/// `Screen1.CamFov`.
pub const CAMERA_FOV: f32 = 70.0;
/// `Screen1.CamNear`.
pub const CAMERA_NEAR: f32 = 0.1;
/// `Screen1.CamFar`.
pub const CAMERA_FAR: f32 = 1000.0;

/// `RenderObject1.Scale`.
pub const RENDER_SCALE: [f32; 3] = [0.5, 0.5, 0.5];

/// `TransformEx1`'s ports.
///
/// `Scale 2 2 1` is a **model-matrix factor**, never baked into the vertex
/// data (the fragment tip mask reads the pre-transform `iPosition`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformExPorts {
    /// `Scale`.
    pub scale: [f32; 3],
    /// `Selection`.
    pub selection: u32,
    /// `InputComponent`.
    pub input_component: u32,
    /// `OutputComponent`.
    pub output_component: u32,
}

/// `TransformEx1` (`Scale 2 2 1`, `Selection 1`, `InputComponent 0`,
/// `OutputComponent 3`).
pub const TRANSFORM_EX: TransformExPorts = TransformExPorts {
    scale: [2.0, 2.0, 1.0],
    selection: 1,
    input_component: 0,
    output_component: 3,
};

/// `RenderToTexture1.Format 5` (`GL_RGBA16F`).
pub const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// `Shader1`'s expression, verbatim.
pub const SHADER1_EXPRESSION: &str =
    "in1.x = min(0.5, band(-1, 1, 0, noise1(time*0.1)*0.7)*5); in1.y = beat(0);";

/// `Shader2`'s expression, verbatim.
pub const SHADER2_EXPRESSION: &str = "b = band(-1, 0, 0, 0)*1; in1.x = min(0.01, b*b*b*5);";

/// `Expression1`, verbatim.
pub const EXPRESSION1: &str = "perm = perm+((beat(0.1)*0.1+0.001)*deltatime); out1=noise2(perm, 0)*360; out2=noise2(perm, 1)*360; out3=noise2(perm, 2)*360;";

/// `Shader1`'s `RandomSeed`.
pub const SHADER_RANDOM_SEED: f32 = 1.0;

/// The ball shader's uniform block.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SpikyUniforms {
    /// `gM` — `TransformEx1 * RenderObject1`.
    model: Mat4,
    /// `gMV`'s view half (`gMV = view * model`).
    view: Mat4,
    /// `gMVP`.
    view_proj: Mat4,
    /// `gMIT` — inverse transpose of `model`.
    normal_matrix: Mat4,
    /// `gViewPosition`.
    camera_pos: [f32; 4],
    /// `gTime`.
    time: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    /// `gIn1`.
    in1: [f32; 4],
    /// `gIn2`.
    in2: [f32; 4],
}

/// The `Shader2` composite uniform block.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniforms {
    resolution: [f32; 2],
    time: f32,
    encode_srgb: u32,
    in1: [f32; 4],
}

/// The ball pipeline's shader, uniforms and bindings in front of
/// [`SPIKY_BALL_WGSL`].
fn spiky_ball_wgsl() -> String {
    format!("{P9_COMMON_WGSL}\n{P9_SHADING_WGSL}\n{SPIKY_BALL_WGSL}")
}

/// The full-screen `Shader2` composite.
fn spiky_composite_wgsl() -> String {
    format!(
        "{FULLSCREEN_TRIANGLE_WGSL}\n{P9_COMMON_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{SPIKY_COMPOSITE_WGSL}"
    )
}

const SPIKY_BALL_WGSL: &str = r#"
struct SpikyUniforms {
  model: mat4x4<f32>,
  view: mat4x4<f32>,
  view_proj: mat4x4<f32>,
  normal_matrix: mat4x4<f32>,
  camera_pos: vec4<f32>,
  time: f32,
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
  in1: vec4<f32>,
  in2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: SpikyUniforms;
@group(0) @binding(1) var g_texture1: texture_cube<f32>;
@group(0) @binding(2) var g_sampler: sampler;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) tex: vec2<f32>,
  @location(1) wnormal: vec3<f32>,
  @location(2) viewdir: vec3<f32>,
  @location(3) wpos: vec3<f32>,
  @location(4) vpos: vec3<f32>,
  @location(5) opos: vec3<f32>,
};

@vertex
fn vs_main(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
) -> VsOut {
  var out: VsOut;
  let g_time = u.time;
  var v = vec4<f32>(position, 1.0);
  out.opos = v.xyz;

  let d = (length(position) - 0.5) / 0.75;
  // The scalar displacement broadcasts along (1,1,1), exactly like GLSL's
  // `v += d * (_noise(...) * 0.1)`.
  v = v + vec4<f32>(d * (p9_noise(position.xyz * 1.0 + vec3<f32>(0.0, 0.0, g_time * 0.5)) * 0.1));

  let wpos = (u.model * v).xyz;
  out.wpos = wpos;
  out.vpos = (u.view * v).xyz;
  out.position = u.view_proj * v;
  out.tex = uv;
  out.wnormal = normalize((u.normal_matrix * vec4<f32>(normal, 0.0)).xyz);
  out.viewdir = normalize(u.camera_pos.xyz - wpos);
  return out;
}

// vec3 Specular_F_Roughness(vec3 specularColor, float a, vec3 h, vec3 v)
fn specular_f_roughness(
  specular_color: vec3<f32>,
  a: f32,
  h: vec3<f32>,
  v: vec3<f32>,
) -> vec3<f32> {
  let t = pow(1.0 - p9_saturate(dot(v, h)), 5.0);
  return vec3<f32>(t) + (1.0 - t) * specular_color;
}

fn get_height(p: vec2<f32>) -> f32 {
  return p9_ridgedmf_fast(vec3<f32>(p.x * 15.0, p.y * 15.0, u.time * 0.1), 4u);
}

// Forward-difference screen-space derivative of the height field.
fn dHdxy_fwd(p: vec2<f32>) -> vec2<f32> {
  let d_stdx = dpdx(p);
  let d_stdy = dpdy(p);
  let hll = get_height(p);
  let dbx = get_height(p + d_stdx) - hll;
  let dby = get_height(p + d_stdy) - hll;
  return vec2<f32>(dbx, dby);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let normal0 = normalize(in.wnormal);
  let f_derivs = dHdxy_fwd(in.tex) * 0.002;
  let normal = p9_perturb_normal(in.vpos, normal0, f_derivs);
  let viewdir = normalize(in.viewdir);

  let diff_col = vec3<f32>(0.5, 0.48, 0.53) * 0.75;
  let spec_col = vec3<f32>(0.25);
  var c = vec3<f32>(0.0);

  let light_colors = array<vec3<f32>, 2>(
    vec3<f32>(0.247059, 0.474510, 0.811765),
    vec3<f32>(0.929412, 0.615686, 0.207843),
  );
  let light_positions = array<vec3<f32>, 2>(
    vec3<f32>(40.0, 50.0, -25.0),
    vec3<f32>(-40.0, 50.0, 25.0),
  );
  let roughness = 0.2;
  let albedo = diff_col;
  for (var l = 0u; l < 2u; l = l + 1u) {
    let light_dir = normalize(light_positions[l] - in.wpos);
    c = c + light_colors[l] * p9_light_directional(
      albedo,
      vec3<f32>(0.03),
      normal,
      roughness,
      light_colors[l] * 1.0,
      light_dir,
      viewdir,
    );
  }

  let mip_index = 0.2 * 0.2 * 8.0;
  let env_color = textureSampleLevel(g_texture1, g_sampler, reflect(-viewdir, normal), mip_index).rgb;
  let env_fresnel = specular_f_roughness(vec3<f32>(0.03), roughness * roughness, normal, viewdir);
  c = c + env_fresnel * env_color * 1.0;

  // Red lights along the spike tips.
  let d = (length(in.opos) - 0.5) / 0.75;
  var c1 = vec4<f32>(c, 1.0);
  var c2 = c1;
  if (d > 0.9) {
    c2 = vec4<f32>(1.0, 0.0, 0.0, 1.0) * 10.0;
  }
  return clamp(mix(c1, c2, p9_saturate(u.in1.x)), vec4<f32>(0.0), vec4<f32>(1.0));
}
"#;

const SPIKY_COMPOSITE_WGSL: &str = r#"
struct CompositeUniforms {
  resolution: vec2<f32>,
  time: f32,
  encode_srgb: u32,
  in1: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: CompositeUniforms;
@group(0) @binding(1) var g_texture1: texture_2d<f32>;
@group(0) @binding(2) var g_sampler: sampler;
@group(0) @binding(3) var g_texture2: texture_2d<f32>;

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let t = vec2<f32>(
    tex.x + p9_noise(vec3<f32>(u.time, tex.y * u.resolution.y * 0.25, 0.0)) * u.in1.x * 0.1,
    tex.y,
  );
  let c1 = textureSampleLevel(g_texture1, g_sampler, t, 0.0);
  let c2 = textureSampleLevel(g_texture2, g_sampler, t, 0.0);
  // `PreviousLayer1.Color` has no texture here: transparent black, and the
  // compositor's alpha blend supplies the layer below.
  let bg = vec4<f32>(0.0);
  let linear = vec4<f32>(mix(bg.rgb, p9_tonemap_aces(c2.rgb * 0.5), c2.a) + p9_tonemap_aces(c1.rgb * 0.75), 1.0);
  // `WebGLRenderer` writes to an sRGB canvas, so the shader-side encode is
  // only needed for a linear destination (the same rule as `OutputPass`).
  let srgb = select(
    1.055 * pow(linear.rgb, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055),
    linear.rgb * 12.92,
    linear.rgb <= vec3<f32>(0.0031308),
  );
  return vec4<f32>(select(linear.rgb, srgb, u.encode_srgb != 0u), 1.0);
}
"#;

/// One frame of the ported expressions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpikyDrive {
    /// Shader1's `gIn1`.
    pub shader_in1: [f32; 4],
    /// Shader2's `gIn1`.
    pub composite_in1: [f32; 4],
    /// `Expression1`'s `perm` for the next frame.
    pub perm: f32,
    /// `Expression1`'s `out1..3` in degrees.
    pub rotation: [f32; 3],
}

/// Pure port of Shader1/Shader2/Expression1.
///
/// `band` is the Plane9 `band(channel, damping, bandnr, nomusic)` reader and
/// `beat` the `beat(fallback)` reader, so the pure function can be tested
/// without an [`AudioBridge`].
pub fn spiky_drive(
    band: impl Fn(i32, i32, i32, f32) -> f32,
    beat: impl Fn(f32) -> f32,
    time: f32,
    dt: f32,
    perm: f32,
) -> SpikyDrive {
    let time = if time.is_finite() { time } else { 0.0 };
    let step = frame_dt(dt);
    let base = if perm.is_finite() { perm } else { 1.0 };

    let shader_x = (band(-1, 1, 0, value_noise1(time * 0.1) * 0.7) * 5.0).min(0.5);
    let shader_y = beat(0.0);

    let b = band(-1, 0, 0, 0.0) * 1.0;
    let composite_x = (b * b * b * 5.0).min(0.01);

    let next = base + (beat(0.1) * 0.1 + 0.001) * step;
    let rotation = [
        value_noise2(next, 0.0) * 360.0,
        value_noise2(next, 1.0) * 360.0,
        value_noise2(next, 2.0) * 360.0,
    ];

    SpikyDrive {
        shader_in1: [shader_x, shader_y, 0.0, 0.0],
        composite_in1: [composite_x, 0.0, 0.0, 0.0],
        perm: next,
        rotation,
    }
}

// ---------------------------------------------------------------------------
// GPU resources
// ---------------------------------------------------------------------------

struct CaptureTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct SpikyGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    ball_pipeline: wgpu::RenderPipeline,
    ball_bind_group: wgpu::BindGroup,
    ball_uniforms: wgpu::Buffer,
    capture: CaptureTarget,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    bloom: Plane9BloomPass,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_group: wgpu::BindGroup,
    composite_uniforms: wgpu::Buffer,
    composite_layout: wgpu::BindGroupLayout,
    composite_sampler: wgpu::Sampler,
    mesh: MeshGpu,
    _cube: wgpu::Texture,
    _cube_view: wgpu::TextureView,
    _cube_sampler: wgpu::Sampler,
}

/// The `Plane9 Spiky` scene.
pub struct Spiky {
    audio: P10Audio,
    elapsed: f32,
    perm: f32,
    rotation: [f32; 3],
    shader_in1: [f32; 4],
    composite_in1: [f32; 4],
    size: (u32, u32),
    aspect: f32,
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    assets_dir: Option<std::path::PathBuf>,
    gpu: Option<SpikyGpu>,
}

impl Default for Spiky {
    fn default() -> Self {
        Self::new()
    }
}

impl Spiky {
    /// Create the scene at 1x1.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            elapsed: 0.0,
            perm: SHADER_RANDOM_SEED,
            rotation: [0.0; 3],
            shader_in1: [0.0; 4],
            composite_in1: [0.0; 4],
            size: (1, 1),
            aspect: 1.0,
            target_format: wgpu::TextureFormat::Rgba8Unorm,
            device: None,
            queue: None,
            assets_dir: None,
            gpu: None,
        }
    }

    /// `Expression1`'s `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// `Expression1`'s `out1..3` in degrees.
    pub fn rotation(&self) -> [f32; 3] {
        self.rotation
    }

    /// Shader1's `gIn1`.
    pub fn shader_in1(&self) -> [f32; 4] {
        self.shader_in1
    }

    /// Shader2's `gIn1`.
    pub fn composite_in1(&self) -> [f32; 4] {
        self.composite_in1
    }

    /// `RenderObject1`'s rotation over `TransformEx1`'s scale.
    pub fn model(&self) -> Mat4 {
        let render = trs([0.0, 0.0, 0.0], self.rotation_radians(), RENDER_SCALE);
        let transform_ex = scale(
            TRANSFORM_EX.scale[0],
            TRANSFORM_EX.scale[1],
            TRANSFORM_EX.scale[2],
        );
        mat4_mul(&render, &transform_ex)
    }

    /// Spiky's `sceneProps.js` entry has no scene-specific controls, so this
    /// only satisfies the registry's prop binder.
    pub fn apply_props(&mut self, _props: impl FnMut(&str) -> Option<f32>) {}

    fn rotation_radians(&self) -> [f32; 3] {
        [
            self.rotation[0].to_radians(),
            self.rotation[1].to_radians(),
            self.rotation[2].to_radians(),
        ]
    }

    fn view(&self) -> Mat4 {
        let target = [
            CAMERA_POSITION[0] + CAMERA_LOOK[0],
            CAMERA_POSITION[1] + CAMERA_LOOK[1],
            CAMERA_POSITION[2] + CAMERA_LOOK[2],
        ];
        look_at(CAMERA_POSITION, target, [0.0, 1.0, 0.0])
    }

    fn projection(&self) -> Mat4 {
        perspective(
            CAMERA_FOV.to_radians(),
            self.aspect,
            CAMERA_NEAR,
            CAMERA_FAR,
        )
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let model = self.model();
        let view = self.view();
        gpu.queue.write_buffer(
            &gpu.ball_uniforms,
            0,
            bytemuck::bytes_of(&SpikyUniforms {
                model,
                view,
                view_proj: mat4_mul(&mat4_mul(&self.projection(), &view), &model),
                normal_matrix: inverse_transpose(&model),
                camera_pos: [
                    CAMERA_POSITION[0],
                    CAMERA_POSITION[1],
                    CAMERA_POSITION[2],
                    1.0,
                ],
                time: self.elapsed,
                _pad0: 0.0,
                _pad1: 0.0,
                _pad2: 0.0,
                in1: self.shader_in1,
                in2: [0.0; 4],
            }),
        );
        gpu.queue.write_buffer(
            &gpu.composite_uniforms,
            0,
            bytemuck::bytes_of(&CompositeUniforms {
                resolution: [self.size.0 as f32, self.size.1 as f32],
                time: self.elapsed,
                encode_srgb: u32::from(!self.target_format.is_srgb()),
                in1: self.composite_in1,
            }),
        );
    }

    fn rebuild_targets(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        gpu.capture = create_capture(
            &gpu.device,
            "point10-plane9-spiky-capture",
            self.size.0,
            self.size.1,
        );
        let (depth, depth_view) = create_depth(
            &gpu.device,
            "point10-plane9-spiky-depth",
            self.size.0,
            self.size.1,
        );
        gpu._depth = depth;
        gpu.depth_view = depth_view;
        gpu.bloom = Plane9BloomPass::new(
            &gpu.device,
            &gpu.queue,
            self.size.0,
            self.size.1,
            bloom_params(),
        );
        gpu.bloom.set_input(&gpu.capture.view);
        gpu.composite_bind_group = create_composite_bind_group(&gpu.device, gpu);
        self.upload_uniforms();
    }
}

impl Point10Scene for Spiky {
    fn name(&self) -> &'static str {
        NAME
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        let device = ctx.device.clone();
        let queue = ctx.queue.clone();
        self.size = (ctx.width, ctx.height);
        self.aspect = ctx.aspect();
        self.target_format = ctx.target_format;
        self.device = Some(device.clone());
        self.queue = Some(queue.clone());
        self.assets_dir = ctx.assets_dir.clone();

        let mesh = MeshGpu::new(
            &device,
            "point10-plane9-spiky-mesh",
            &build_spiky_mesh().to_render_mesh(),
        );

        let ball_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-spiky-ball-shader"),
            source: wgpu::ShaderSource::Wgsl(spiky_ball_wgsl().into()),
        });
        let ball_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-spiky-ball-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler_entry(2),
            ],
        });
        let ball_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-spiky-ball-uniforms"),
            size: std::mem::size_of::<SpikyUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (cube, cube_view, cube_sampler) =
            load_stormy_cube(&device, &queue, self.assets_dir.as_deref()).unwrap_or_else(|| {
                crate::point10::scenes::plane9_common::fallback_cube(&device, &queue)
            });

        let ball_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-spiky-ball-bind-group"),
            layout: &ball_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ball_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cube_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&cube_sampler),
                },
            ],
        });
        let ball_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-plane9-spiky-ball-pipeline-layout"),
            bind_group_layouts: &[Some(&ball_layout)],
            immediate_size: 0,
        });
        let ball_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-plane9-spiky-ball-pipeline"),
            layout: Some(&ball_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ball_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Plane9 `CullMode 2` = `CullMode::Back` (`wgpu::Face::Back`).
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(spiky_depth_state()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &ball_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: CAPTURE_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // `Shader2`: bloom (gTexture1) + capture (gTexture2) -> the screen.
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-plane9-spiky-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(spiky_composite_wgsl().into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-plane9-spiky-composite-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
                texture_entry(3),
            ],
        });
        let composite_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-plane9-spiky-composite-uniforms"),
            size: std::mem::size_of::<CompositeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_sampler = plane9_sampler(&device, "point10-plane9-spiky-composite-sampler");
        let composite_pipeline = ctx.fullscreen_pipeline(
            "point10-plane9-spiky-composite-pipeline",
            &composite_shader,
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-plane9-spiky-composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            }),
            None,
        );

        let capture = create_capture(
            &device,
            "point10-plane9-spiky-capture",
            self.size.0,
            self.size.1,
        );
        let (depth, depth_view) = create_depth(
            &device,
            "point10-plane9-spiky-depth",
            self.size.0,
            self.size.1,
        );
        let mut bloom =
            Plane9BloomPass::new(&device, &queue, self.size.0, self.size.1, bloom_params());
        bloom.set_input(&capture.view);
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-plane9-spiky-composite-bind-group"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: composite_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        bloom.composite_view().expect("one-mip bloom view"),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&composite_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&capture.view),
                },
            ],
        });

        self.gpu = Some(SpikyGpu {
            device,
            queue,
            ball_pipeline,
            ball_bind_group,
            ball_uniforms,
            capture,
            _depth: depth,
            depth_view,
            bloom,
            composite_pipeline,
            composite_bind_group,
            composite_uniforms,
            composite_layout,
            composite_sampler,
            mesh,
            _cube: cube,
            _cube_view: cube_view,
            _cube_sampler: cube_sampler,
        });
        self.upload_uniforms();
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width.max(1), height.max(1));
        self.aspect = self.size.0 as f32 / self.size.1 as f32;
        self.rebuild_targets();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;
        let drive = spiky_drive(
            |channel, damping, band, nomusic| self.audio.band(channel, damping, band, nomusic),
            |fallback| self.audio.beat(fallback),
            self.elapsed,
            dt,
            self.perm,
        );
        self.perm = drive.perm;
        self.rotation = drive.rotation;
        self.shader_in1 = drive.shader_in1;
        self.composite_in1 = drive.composite_in1;
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };

        // `RenderObject1` -> `RenderToTexture1` (transparent clear, depth test).
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-spiky-ball"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.capture.view,
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
            pass.set_pipeline(&gpu.ball_pipeline);
            pass.set_bind_group(0, &gpu.ball_bind_group, &[]);
            gpu.mesh.draw(&mut pass);
        }

        // `Bloom1` reads the capture; `Shader2` samples its one-mip composite.
        gpu.bloom.encode(encoder);

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-plane9-spiky-composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(&gpu.composite_pipeline);
            pass.set_bind_group(0, &gpu.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn dispose(&mut self) {
        self.gpu = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        // Spiky's `sceneProps.js` entry has no scene-specific controls.
        scene_props_with_levers(&[])
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene(NAME, "3d-scenes")
    }
}

fn bloom_params() -> Plane9BloomParams {
    Plane9BloomParams::new(5.0, 0.0, 0.5, 1.0, false)
}

fn create_capture(device: &wgpu::Device, label: &str, width: u32, height: u32) -> CaptureTarget {
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
        format: CAPTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    CaptureTarget {
        _texture: texture,
        view,
    }
}

/// `Shader1`'s depth state (`DepthTest`/`DepthWrite` true, `Less`).
fn spiky_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: crate::point10::scenes::cube_mesh::DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

fn create_composite_bind_group(device: &wgpu::Device, gpu: &SpikyGpu) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point10-plane9-spiky-composite-bind-group"),
        layout: &gpu.composite_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: gpu.composite_uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(
                    gpu.bloom.composite_view().expect("one-mip bloom view"),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&gpu.composite_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&gpu.capture.view),
            },
        ],
    })
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

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

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// The Spiky modifier chain (everything before `TransformEx1`).
///
/// Kept here, with the XML port values spelled out, so the chain is reviewable
/// next to the node it came from. `TransformEx1` is deliberately not applied:
/// its `2 2 1` belongs to the model matrix (see [`Spiky::model`]).
pub fn build_spiky_mesh() -> IndexedMesh {
    let cube = mesh_ops::weld(&build_box(1.0, 1.0, 1.0));
    // Subdivide2: NumSteps 1, Smootness 1.
    let mesh = mesh_ops::subdivide(
        &cube,
        SubdivideParams {
            num_steps: 1,
            smoothness: 1.0,
        },
    );
    // SelectMesh1: SelectionType 4 (all), Logic 1, Invert false, Output 1.
    let mesh = mesh_ops::select_faces(
        &mesh,
        SelectParams {
            selection_type: 4,
            logic: 1,
            invert: false,
            output: 1,
            random_factor: 1.0,
            random_seed: 1,
        },
    );
    // Extrude2: NumSteps 2, Amount 0.88, Direction 0, LocalScale 0.
    let mesh = mesh_ops::extrude(
        &mesh,
        ExtrudeParams {
            num_steps: 2,
            amount: 0.88,
            direction: 0,
            local_scale: 0.0,
        },
    );
    // Randomize1: Selection 1, Direction 0, Amount 0.1, RandomSeed 1.
    let mesh = mesh_ops::randomize(
        &mesh,
        RandomizeParams {
            amount: 0.1,
            direction: 0,
            random_seed: 1,
        },
    );
    // Subdivide1: NumSteps 2, Smootness 1.
    mesh_ops::subdivide(
        &mesh,
        SubdivideParams {
            num_steps: 2,
            smoothness: 1.0,
        },
    )
}

/// `TransformEx1`'s `InputComponent`/`OutputComponent` selectors are inert for
/// the mesh path; [`TRANSFORM_EX`] records them next to their node.
pub const TRANSFORM_EX_INPUT_COMPONENT: u32 = TRANSFORM_EX.input_component;
/// See [`TRANSFORM_EX_INPUT_COMPONENT`].
pub const TRANSFORM_EX_OUTPUT_COMPONENT: u32 = TRANSFORM_EX.output_component;

// ---------------------------------------------------------------------------
// Matrix helpers
// ---------------------------------------------------------------------------

fn identity() -> Mat4 {
    let mut matrix = [[0.0_f32; 4]; 4];
    for (index, column) in matrix.iter_mut().enumerate() {
        column[index] = 1.0;
    }
    matrix
}

/// Inverse transpose of the model matrix's upper 3x3 (translation dropped).
fn inverse_transpose(matrix: &Mat4) -> Mat4 {
    let c0 = [matrix[0][0], matrix[0][1], matrix[0][2]];
    let c1 = [matrix[1][0], matrix[1][1], matrix[1][2]];
    let c2 = [matrix[2][0], matrix[2][1], matrix[2][2]];
    let det = dot3(c0, cross3(c1, c2));
    if det.abs() < 1e-9 {
        return identity();
    }
    let x = scale3(cross3(c1, c2), 1.0 / det);
    let y = scale3(cross3(c2, c0), 1.0 / det);
    let z = scale3(cross3(c0, c1), 1.0 / det);
    [
        [x[0], x[1], x[2], 0.0],
        [y[0], y[1], y[2], 0.0],
        [z[0], z[1], z[2], 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn scale3(a: [f32; 3], factor: f32) -> [f32; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn scene_metadata_matches_the_plane9_contract() {
        let scene = Spiky::new();
        assert_eq!(scene.name(), "Plane9 Spiky");
        let meta = scene.meta();
        assert_eq!(meta.author, "Plane9 (Joakim Dahl)");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.module_type, "2d");
        assert_eq!(meta.audio_features, &["bass", "mid", "treble"]);
        assert_eq!(meta.category, "3d-scenes");
        // The levers are still in front of an empty scene schema.
        let props = scene.props();
        assert_eq!(props.len(), 6);
        assert_eq!(props[0].name, "gain");
    }

    #[test]
    fn xml_constants_are_pinned() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, 1.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, -1.0]);
        assert_eq!((CAMERA_FOV, CAMERA_NEAR, CAMERA_FAR), (70.0, 0.1, 1000.0));
        assert_eq!(RENDER_SCALE, [0.5, 0.5, 0.5]);
        assert_eq!(TRANSFORM_EX.scale, [2.0, 2.0, 1.0]);
        assert_eq!(TRANSFORM_EX.selection, 1);
        assert_eq!(
            bloom_params(),
            Plane9BloomParams::new(5.0, 0.0, 0.5, 1.0, false)
        );
        assert_eq!(bloom_params().mip_count(), 1);
        assert_eq!(TRANSFORM_EX_INPUT_COMPONENT, 0);
        assert_eq!(TRANSFORM_EX_OUTPUT_COMPONENT, 3);
    }

    #[test]
    fn the_transform_ex_scale_stays_out_of_the_vertex_data() {
        // The mesh must keep the local band the tip mask reads...
        let mesh = build_spiky_mesh();
        let (min, max) = mesh.radius_range();
        // Signed Randomize jitter reaches 0.5 - 0.1 on the inner corners.
        assert!((0.45..0.6).contains(&min), "local min {min}");
        assert!((1.175..1.35).contains(&max), "local max {max}");

        // ...and the 2,2,1 belongs to the model matrix.
        let mut scene = Spiky::new();
        scene.rotation = [0.0; 3];
        let model = scene.model();
        let world = crate::point10::scenes::cube_mesh::transform_point(&model, [max, 0.0, 0.0]);
        assert!((world[0] - max * 2.0 * 0.5).abs() < 1e-4, "{world:?}");
        let z = crate::point10::scenes::cube_mesh::transform_point(&model, [0.0, 0.0, max]);
        assert!((z[2] - max * 1.0 * 0.5).abs() < 1e-4, "{z:?}");
    }

    #[test]
    fn drive_implements_the_three_vendor_expressions() {
        // Shader1: min(0.5, band(-1,1,0,·)*5) and beat(0).
        let drive = spiky_drive(
            |_, damping, band, _| {
                if (damping, band) == (1, 0) { 0.2 } else { 0.0 }
            },
            |fallback| if fallback == 0.0 { 0.25 } else { 0.0 },
            0.0,
            0.0,
            1.0,
        );
        assert!((drive.shader_in1[0] - 0.5).abs() < 1e-6);
        assert!((drive.shader_in1[1] - 0.25).abs() < 1e-6);

        // Shader2: b = band(-1,0,0,0); min(0.01, b³·5).
        let b = 0.5_f32;
        let expected = (b * b * b * 5.0).min(0.01);
        let drive = spiky_drive(
            |_, damping, band, _| {
                if (damping, band) == (0, 0) { b } else { 0.0 }
            },
            |_| 0.0,
            0.0,
            0.0,
            1.0,
        );
        assert!((drive.composite_in1[0] - expected).abs() < 1e-6);

        // Expression1: perm += (beat(0.1)*0.1 + 0.001)*dt, noise2*360.
        let next = 1.0 + (0.5_f32 * 0.1 + 0.001) * 0.1;
        let drive = spiky_drive(|_, _, _, _| 0.0, |_| 0.5, 0.0, 0.1, 1.0);
        assert!((drive.perm - next).abs() < 1e-6);
        assert!((drive.rotation[0] - value_noise2(next, 0.0) * 360.0).abs() < 1e-4);
        assert!((drive.rotation[1] - value_noise2(next, 1.0) * 360.0).abs() < 1e-4);
        assert!((drive.rotation[2] - value_noise2(next, 2.0) * 360.0).abs() < 1e-4);
    }

    #[test]
    fn wgsl_parses_and_validates() {
        for (label, program) in [
            ("ball", spiky_ball_wgsl()),
            ("composite", spiky_composite_wgsl()),
        ] {
            let module = match naga::front::wgsl::parse_str(&program) {
                Ok(module) => module,
                Err(error) => panic!("{label} WGSL failed to parse: {error}"),
            };
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{label} WGSL must validate: {error}"));
        }
    }

    #[test]
    fn shader_keeps_the_preset_maths() {
        let ball = spiky_ball_wgsl();
        for needle in [
            "out.opos = v.xyz;",
            "let d = (length(position) - 0.5) / 0.75;",
            "vec4<f32>(d * (p9_noise",
            "g_time * 0.5",
            "out.vpos = (u.view * v).xyz;",
            "out.position = u.view_proj * v;",
            "out.wnormal = normalize((u.normal_matrix * vec4<f32>(normal, 0.0)).xyz);",
            "out.viewdir = normalize(u.camera_pos.xyz - wpos);",
            "p9_perturb_normal(in.vpos, normal0, f_derivs)",
            "let f_derivs = dHdxy_fwd(in.tex) * 0.002;",
            "vec3<f32>(0.5, 0.48, 0.53) * 0.75",
            "vec3<f32>(0.247059, 0.474510, 0.811765)",
            "vec3<f32>(0.929412, 0.615686, 0.207843)",
            "vec3<f32>(40.0, 50.0, -25.0)",
            "vec3<f32>(-40.0, 50.0, 25.0)",
            "0.2 * 0.2 * 8.0",
            "vec3<f32>(0.03)",
            "if (d > 0.9)",
            "vec4<f32>(1.0, 0.0, 0.0, 1.0) * 10.0",
            "p9_saturate(u.in1.x)",
        ] {
            assert!(ball.contains(needle), "ball shader lost {needle}");
        }

        let composite = spiky_composite_wgsl();
        for needle in [
            "let bg = vec4<f32>(0.0);",
            "p9_tonemap_aces(c2.rgb * 0.5)",
            "p9_tonemap_aces(c1.rgb * 0.75)",
            "vec4<f32>(",
            "mix(bg.rgb",
        ] {
            assert!(composite.contains(needle), "composite lost {needle}");
        }
        assert!(!ball.contains("gl_FragCoord"));
        assert!(!composite.contains("gl_FragCoord"));
    }

    #[test]
    fn shared_shading_helpers_are_exposed() {
        assert!(P9_SHADING_WGSL.contains("fn p9_ridgedmf_fast("));
        assert!(P9_SHADING_WGSL.contains("fn p9_perturb_normal("));
        assert!(P9_SHADING_WGSL.contains("2.13628142"));
    }

    #[test]
    fn gpu_smoke_and_audio_driven_tip_colour() {
        let Some((device, queue)) = gpu() else {
            eprintln!("skipping Spiky GPU smoke: no fallback adapter");
            return;
        };
        let mut scene = Spiky::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        scene.resize(64, 64);

        let target = TestTarget::new(&device, 64, 64);
        let mut payload = BandPayload::silent(44_100.0);
        let mut bridge = AudioBridge::new();
        for frame in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("point10-plane9-spiky-smoke"),
            });
            scene.render(&mut encoder, &target.view);
            queue.submit(Some(encoder.finish()));
            let pixels = target.read(&device, &queue);
            assert!(any_lit(&pixels), "frame {frame} is uniformly black");
        }

        payload.lanes[3] = 0.2; // damping-1 bass -> min(0.5, 0.2 * 5)
        bridge.update(0.0, Some(&payload), &Default::default());
        scene.update(0.0, &bridge);
        assert!(
            (scene.shader_in1()[0] - 0.5).abs() < 1e-6,
            "damping-1 bass must drive Shader1's in1.x"
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-plane9-spiky-tips"),
        });
        scene.render(&mut encoder, &target.view);
        queue.submit(Some(encoder.finish()));
        let high = target.read(&device, &queue);
        assert!(any_lit(&high), "driven frame is uniformly black");

        // The Stormy Days cubemap is a warm sunset, so the silent frame
        // legitimately has red-dominant grazing rims: comparing raw pixel
        // colour against an arbitrary "red means beat" threshold would be a
        // false test. And the vendor `Amount 0.88` leaves the extreme apexes
        // outside this 64x64 camera frustum, so no rasterised tip need exist
        // here at all. Assert the vendor-faithful *inputs* to the tip mix:
        // the geometry owns vertices in the fragment's `d > 0.9` band and
        // the driven `in1.x` above selects how much red they receive.
        let mesh = build_spiky_mesh();
        let tip_vertices = mesh
            .positions
            .iter()
            .filter(|p| (p[0].hypot(p[1]).hypot(p[2]) - 0.5) / 0.75 > 0.9)
            .count();
        assert!(
            tip_vertices > 0,
            "the vendor mesh has no vertices in the fragment's d > 0.9 tip band"
        );
        scene.dispose();
    }
}
