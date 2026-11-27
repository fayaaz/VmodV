//! Helpers shared by the ten Plane9 XML presets imported in this batch.
//!
//! The presets in this batch are small, self-contained Plane9 graphs:
//!
//! * a full-screen `Shader` node evaluated over `si.tex` (Colors, Gradient,
//!   Xor, Tv Noise, Though The Noise), sometimes with a `RenderToTexture`
//!   capture and a second `Shader` pass (Noise Warp, Rorschach, Truchet
//!   Tentacles);
//! * a `CloneExpression` of the default 1x1 plane driven by a per-instance
//!   `Expression` (Blob Fade, Circle Crossover).
//!
//! Rather than re-deriving the same uniform block, bind-group layouts and
//! pipeline descriptors ten times, this module carries the two pipelines the
//! batch needs:
//!
//! | Helper | Scene graph equivalent |
//! | --- | --- |
//! | [`Plane9Pass`] | `Shader` + `RenderRect` (a full-screen triangle) |
//! | [`Plane9QuadPass`] | `RenderObject` + `CloneExpression` (instanced 1x1 planes) |
//!
//! The shader contract mirrors the XML's `VERTEXOUTPUT`/`iColor`/`gColor`
//! plumbing as closely as a Rust port can: `gIn1..gIn3`, `gColor1`,
//! `gColor2` and `gTexture1` keep their Plane9 names, and the vertex stage
//! writes `si.diffuse = iColor * gColor1 * gColor`.
//!
//! # Concatenation order
//!
//! A scene composes its shader as:
//!
//! ```text
//! P9_COMMON_WGSL + P9_SHADING_WGSL + [PLANE9_TEXTURE_WGSL] + PLANE9_BATCH_WGSL + scene
//! ```
//!
//! [`Plane9Pass::new`] prepends [`FULLSCREEN_TRIANGLE_WGSL`] and
//! [`PLANE9_UNIFORMS_WGSL`] itself, so a scene only supplies the fragment
//! entry (and the texture declaration when it samples a capture).
//!
//! # GLSL → WGSL notes
//!
//! * `_rand(vec2)` is `p9_rand2` from [`crate::point10::shared::p9_glsl`];
//!   `_rand(vec3)` and `_rand4(vec2)` live here because only this batch's
//!   `_voronoi`/`_noisegradientfast` use them.
//! * `_voronoi` is the `jitter = 0.9`, `type = 0` (Euclidean) overload the
//!   presets call; the `out` parameters become a struct.
//! * `_brightnessSaturationContrast` and `_noisegradientfast` are ported
//!   verbatim (same constants, same operation order).
//! * The presets clear to a colour from their `Clear` node; [`Plane9Pass::draw`]
//!   takes that colour so a scene owns its clear with no host help.
//!
//! # Texture coordinates and depth
//!
//! Full-screen fragments derive `tex = @builtin(position).xy / resolution`,
//! so v = 0 is the **top** row. This is the repository-wide convention: every
//! texture is uploaded unflipped (row 0 = top), so a capture and the pass that
//! samples it stay aligned. Plane9's `si.tex` comes from three's
//! `PlaneGeometry` UVs and has v = 1 at the top instead. For the radial and
//! noise-driven patterns in this batch the difference only rotates a random
//! field; a future directional gradient needs a shared UV helper rather than
//! per-scene `1.0 - tex.y` patches.
//!
//! [`Plane9QuadPass`] has no depth attachment. Blob Fade's clones are exactly
//! coplanar (z = 0), so submission order matches Plane9's `LessEqual` depth
//! test. Circle Crossover's z spread is only `-y * 0.001`; its instances are
//! submitted near-to-far, so overlapping circles composite far-to-near here
//! (Plane9's depth test keeps the near row). The clone shaders write opaque
//! alpha with no blending in both paths.

#![forbid(unsafe_code)]

use bytemuck::{Pod, Zeroable};

use crate::point10::scenes::cube_mesh::{Mat4, look_at, mat4_mul, perspective};
use crate::point10::shared::assets::{AssetLoader, DecodedImage, mip_chain};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::{FULLSCREEN_TRIANGLE_WGSL, UnitQuad};

/// The Plane9 uniform block shared by every preset's `Shader` node.
///
/// Plane9 pushes `gIn1..gIn3` (the node's evaluated `In1..In3` ports),
/// `gColor1`/`gColor2` (the node's colour ports), `gTime`, and the drawing
/// buffer size. `resolution` is `vec2` so the emboss texel step
/// (`gTexture1Size.zw`) can be derived as `1 / resolution` for a full-size
/// capture.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Plane9Uniforms {
    /// Destination size in pixels (`gResolution`).
    pub resolution: [f32; 2],
    /// Seconds since the scene was activated (`gTime`).
    pub time: f32,
    /// Standard-layout padding.
    pub _pad0: f32,
    /// `gIn1`.
    pub in1: [f32; 4],
    /// `gIn2`.
    pub in2: [f32; 4],
    /// `gIn3`.
    pub in3: [f32; 4],
    /// `gColor1`.
    pub color1: [f32; 4],
    /// `gColor2`.
    pub color2: [f32; 4],
}

impl Plane9Uniforms {
    /// A uniform block at `width` × `height` with white colour ports.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            resolution: [width.max(1) as f32, height.max(1) as f32],
            time: 0.0,
            _pad0: 0.0,
            in1: [0.0; 4],
            in2: [0.0; 4],
            in3: [0.0; 4],
            color1: [1.0; 4],
            color2: [1.0; 4],
        }
    }
}

/// WGSL declaration of [`Plane9Uniforms`] (`@binding(0)`).
pub const PLANE9_UNIFORMS_WGSL: &str = r#"
struct Plane9Uniforms {
  resolution: vec2<f32>,
  time: f32,
  _pad0: f32,
  in1: vec4<f32>,
  in2: vec4<f32>,
  in3: vec4<f32>,
  color1: vec4<f32>,
  color2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Plane9Uniforms;
"#;

/// Declarations for a 2D `gTexture1` (`@binding(1)`/`@binding(2)`).
pub const PLANE9_TEXTURE_WGSL: &str = r#"
@group(0) @binding(1) var g_texture1: texture_2d<f32>;
@group(0) @binding(2) var g_sampler: sampler;
"#;

/// Declarations for a cube `gTexture1` (`@binding(1)`/`@binding(2)`).
pub const PLANE9_CUBE_WGSL: &str = r#"
@group(0) @binding(1) var g_texture1: texture_cube<f32>;
@group(0) @binding(2) var g_sampler: sampler;
"#;

/// The P9Glsl helpers this batch needs that the earlier ports did not.
pub const PLANE9_BATCH_WGSL: &str = r#"
// vec4 _rand4(vec2 p) — the four-lane hash _perm() is built on.
fn p9_rand4(p0: vec2<f32>) -> vec4<f32> {
  var p = fract(p0 * vec2<f32>(5.3987, 5.4421));
  p = p + vec2<f32>(dot(p.yx, p.xy + vec2<f32>(21.5351, 14.3137)));
  let a = p.x * p.y;
  return fract(vec4<f32>(a * 95.4307, a * 97.5901, a * 93.8369, a * 91.6931));
}

// vec4 _perm(float pos) { return _rand4(vec2(pos, pos*1.1737)); }
fn p9_perm4(pos: f32) -> vec4<f32> {
  return p9_rand4(vec2<f32>(pos, pos * 1.1737));
}

// The four outs of `_voronoi(vec3, out f1, out pos1, out f2, out pos2)`.
struct P9Voronoi3 {
  f1: f32,
  pos1: vec3<f32>,
  f2: f32,
  pos2: vec3<f32>,
}

// _voronoi(position, f1, pos1, f2, pos2, jitter = 0.9, type = 0)
fn p9_voronoi3(position: vec3<f32>) -> P9Voronoi3 {
  let jitter = 0.9;
  let cur_cell = floor(position) + vec3<f32>(0.5);
  var result: P9Voronoi3;
  result.f1 = 100000.0;
  result.f2 = 100000.0;
  result.pos1 = vec3<f32>(0.0);
  result.pos2 = vec3<f32>(0.0);
  for (var x = -1.0; x <= 1.0; x = x + 1.0) {
    for (var y = -1.0; y <= 1.0; y = y + 1.0) {
      for (var z = -1.0; z <= 1.0; z = z + 1.0) {
        let test_cell = cur_cell + vec3<f32>(x, y, z);
        let random_uvw = test_cell * vec3<f32>(1.171, 1.073, 1.127);
        let lane = p9_rand4(random_uvw.xy).r;
        let cell_noise = p9_perm4(lane + random_uvw.z).xyz;
        let pos = test_cell + vec3<f32>(jitter) * (cell_noise - vec3<f32>(0.5));
        let offset = pos - position;
        let dist = dot(offset, offset);
        if (dist < result.f1) {
          result.f2 = result.f1;
          result.pos2 = result.pos1;
          result.f1 = dist;
          result.pos1 = pos;
        } else if (dist < result.f2) {
          result.f2 = dist;
          result.pos2 = pos;
        }
      }
    }
  }
  result.f1 = sqrt(result.f1);
  result.f2 = sqrt(result.f2);
  return result;
}

// vec3 _noisegradientfast(vec3 p, float d)
fn p9_noise_gradient3(p: vec3<f32>, d: f32) -> vec3<f32> {
  let f0 = p9_noise(p);
  let fx = p9_noise(p + vec3<f32>(d, 0.0, 0.0));
  let fy = p9_noise(p + vec3<f32>(0.0, d, 0.0));
  let fz = p9_noise(p + vec3<f32>(0.0, 0.0, d));
  return vec3<f32>(fx - f0, fy - f0, fz - f0) / d;
}

// vec3 _brightnessSaturationContrast(vec3 c, float brightness, float saturation, float contrast)
fn p9_brightness_saturation_contrast(
  c: vec3<f32>,
  brightness: f32,
  saturation: f32,
  contrast: f32,
) -> vec3<f32> {
  let brightness_col = c * brightness;
  let luminance = dot(brightness_col, vec3<f32>(0.2126, 0.7152, 0.0722));
  let saturation_col = mix(vec3<f32>(luminance), brightness_col, saturation);
  return mix(vec3<f32>(0.5), saturation_col, contrast);
}

// GLSL `mod()`: the result takes the sign of the divisor.
fn p9_mod_f(x: f32, y: f32) -> f32 {
  return x - y * floor(x / y);
}
"#;

/// One texture a [`Plane9Pass`] samples as `gTexture1`.
pub enum Plane9TextureBinding<'a> {
    /// A 2D render target (`RenderToTexture`'s `Color` port).
    D2 {
        /// Texture view (`TEXTURE_BINDING`).
        view: &'a wgpu::TextureView,
        /// Filtering sampler.
        sampler: &'a wgpu::Sampler,
    },
    /// A cube texture (`FileTexture`'s Stormy Days cubemap).
    Cube {
        /// Cube texture view.
        view: &'a wgpu::TextureView,
        /// Filtering sampler.
        sampler: &'a wgpu::Sampler,
    },
}

/// A full-screen `Shader` + `RenderRect` pass.
pub struct Plane9Pass {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    queue: wgpu::Queue,
}

impl Plane9Pass {
    /// Build a full-screen pipeline.
    ///
    /// `fragment` is the scene's own `fs_main` (and any helpers it needs); the
    /// vertex stage, uniform block and optional texture declarations are
    /// prepended here. `texture` selects the `texture_2d` or `texture_cube`
    /// flavour and adds the matching bindings.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        label: &str,
        fragment: &str,
        texture: Option<Plane9TextureBinding<'_>>,
    ) -> Self {
        let mut source = String::from(FULLSCREEN_TRIANGLE_WGSL);
        source.push_str(PLANE9_UNIFORMS_WGSL);
        match &texture {
            Some(Plane9TextureBinding::D2 { .. }) => source.push_str(PLANE9_TEXTURE_WGSL),
            Some(Plane9TextureBinding::Cube { .. }) => source.push_str(PLANE9_CUBE_WGSL),
            None => {}
        }
        source.push_str(fragment);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        if let Some(texture) = &texture {
            let view_dimension = match texture {
                Plane9TextureBinding::D2 { .. } => wgpu::TextureViewDimension::D2,
                Plane9TextureBinding::Cube { .. } => wgpu::TextureViewDimension::Cube,
            };
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension,
                    multisampled: false,
                },
                count: None,
            });
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &entries,
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<Plane9Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut bind_entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        }];
        match &texture {
            Some(Plane9TextureBinding::D2 { view, sampler })
            | Some(Plane9TextureBinding::Cube { view, sampler }) => {
                bind_entries.push(wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                });
                bind_entries.push(wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                });
            }
            None => {}
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &layout,
            entries: &bind_entries,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            uniforms,
            bind_group,
            queue: queue.clone(),
        }
    }

    /// Upload this frame's uniform block.
    pub fn upload(&self, uniforms: &Plane9Uniforms) {
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    /// Draw the pass into `target`, clearing it to `clear` first.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clear: wgpu::Color,
        label: &str,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The per-instance data of one cloned 1x1 plane.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Plane9QuadInstance {
    /// `pos` from the `CloneExpression` (RenderObject's `Pos` is zero here).
    pub position: [f32; 3],
    /// Standard-layout padding.
    pub _pad: f32,
    /// `col` from the clone `Expression` (`gColor`).
    pub color: [f32; 4],
}

impl Plane9QuadInstance {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
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
            ],
        }
    }
}

/// The `CloneExpression` uniform block (view-projection + Plane9 ports).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Plane9QuadUniforms {
    /// `gMVP` for the (identity) RenderObject wrapper.
    pub view_proj: Mat4,
    /// `gIn1`.
    pub in1: [f32; 4],
    /// `gIn2`.
    pub in2: [f32; 4],
    /// `gIn3`.
    pub in3: [f32; 4],
    /// `gColor1`.
    pub color1: [f32; 4],
    /// `gColor2`.
    pub color2: [f32; 4],
}

impl Plane9QuadUniforms {
    /// A uniform block with an identity view-projection and white colours.
    pub fn new() -> Self {
        Self {
            view_proj: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            in1: [0.0; 4],
            in2: [0.0; 4],
            in3: [0.0; 4],
            color1: [1.0; 4],
            color2: [1.0; 4],
        }
    }
}

impl Default for Plane9QuadUniforms {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) const PLANE9_QUAD_UNIFORMS_WGSL: &str = r#"
struct Plane9QuadUniforms {
  view_proj: mat4x4<f32>,
  in1: vec4<f32>,
  in2: vec4<f32>,
  in3: vec4<f32>,
  color1: vec4<f32>,
  color2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Plane9QuadUniforms;

struct Plane9QuadInstance {
  position: vec3<f32>,
  _pad: f32,
  color: vec4<f32>,
};

struct Plane9QuadOut {
  @builtin(position) position: vec4<f32>,
  @location(0) tex: vec2<f32>,
  @location(1) diffuse: vec4<f32>,
};

@vertex
fn vs_main(
  @location(0) instance_position: vec3<f32>,
  @location(1) instance_color: vec4<f32>,
  @location(2) corner: vec2<f32>,
  @location(3) uv: vec2<f32>,
) -> Plane9QuadOut {
  var out: Plane9QuadOut;
  // three's `_p9iPos = (instanceMatrix * vec4(position, 1.0)).xyz`: the clone
  // matrix is a pure translation, so the world position is `pos + corner`.
  let world = vec4<f32>(instance_position + vec3<f32>(corner, 0.0), 1.0);
  out.position = u.view_proj * world;
  out.tex = uv;
  // `so.diffuse = iColor * gColor1 * gColor`; iColor is the white vertex colour.
  out.diffuse = instance_color * u.color1;
  return out;
}
"#;

/// An instanced 1x1-plane pass for a `CloneExpression`.
pub struct Plane9QuadPass {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: u32,
    queue: wgpu::Queue,
    quad: UnitQuad,
}

impl Plane9QuadPass {
    /// Build the pipeline with room for `capacity` instances.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        label: &str,
        fragment: &str,
        capacity: u32,
    ) -> Self {
        let source = format!("{P9_COMMON_WGSL}\n{PLANE9_QUAD_UNIFORMS_WGSL}\n{fragment}",);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
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
            label: Some(label),
            size: std::mem::size_of::<Plane9QuadUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let instance_size =
            std::mem::size_of::<Plane9QuadInstance>() as u64 * capacity.max(1) as u64;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: instance_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    Some(Plane9QuadInstance::layout()),
                    Some(UnitQuad::vertex_layout()),
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            uniforms,
            bind_group,
            instances,
            capacity,
            queue: queue.clone(),
            quad: UnitQuad::new(device),
        }
    }

    /// Upload the uniform block.
    pub fn upload_uniforms(&self, uniforms: &Plane9QuadUniforms) {
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    /// Upload the per-instance data (length must not exceed `capacity`).
    pub fn upload_instances(&self, instances: &[Plane9QuadInstance]) {
        let instances = &instances[..instances.len().min(self.capacity as usize)];
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(instances));
    }

    /// Draw every instance into `target`, clearing it first.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clear: wgpu::Color,
        count: u32,
        label: &str,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        // Slot 0 is the per-instance buffer; slot 1 is the shared unit quad.
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.set_vertex_buffer(1, self.quad.vertex_buffer.slice(..));
        pass.set_index_buffer(self.quad.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..UnitQuad::VERTEX_COUNT, 0, 0..count);
    }
}

/// Build a colour render target with the given size and usage.
pub fn create_colour_target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
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
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// A clamp-to-edge linear sampler, matching three's render-target sampler.
pub fn plane9_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    })
}

/// Plane9's `PerspectiveCamera` view-projection for one preset.
///
/// `look` is the XML's `CamLookAt`, which (with `CamLookAtInWorldSpace
/// false`) is a direction added to `position`.
pub fn plane9_camera(
    aspect: f32,
    position: [f32; 3],
    look: [f32; 3],
    fov_degrees: f32,
    near: f32,
    far: f32,
) -> Mat4 {
    let target = [
        position[0] + look[0],
        position[1] + look[1],
        position[2] + look[2],
    ];
    mat4_mul(
        &perspective(fov_degrees.to_radians(), aspect, near, far),
        &look_at(position, target, [0.0, 1.0, 0.0]),
    )
}

/// The six Stormy Days cubemap faces, in wgpu's cube-layer order.
///
/// wgpu's `TextureViewDimension::Cube` expects layers +X, -X, +Y, -Y, +Z, -Z;
/// Plane9's `data\Stormy days` uses the same naming as the bundled files.
pub const STORMY_FACES: [&str; 6] = [
    "stormy/posx.jpg",
    "stormy/negx.jpg",
    "stormy/posy.jpg",
    "stormy/negy.jpg",
    "stormy/posz.jpg",
    "stormy/negz.jpg",
];

/// Load the bundled Stormy Days cubemap, with mips.
///
/// Returns `None` when a face is missing, in which case the caller keeps a
/// procedural fallback. The faces are uploaded unflipped (row 0 = top), the
/// same convention as every other asset here.
pub fn load_stormy_cube(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    assets_dir: Option<&std::path::Path>,
) -> Option<(wgpu::Texture, wgpu::TextureView, wgpu::Sampler)> {
    let loader = AssetLoader::for_assets_dir(assets_dir);
    let mut faces: Vec<DecodedImage> = Vec::with_capacity(6);
    for name in STORMY_FACES {
        faces.push(loader.load(name)?);
    }

    let size = faces.iter().map(|face| face.width.max(face.height)).max()?;
    let mip_level_count = (size as f32).log2().floor().max(0.0) as u32 + 1;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("point10-plane9-stormy-cube"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (layer, face) in faces.iter().enumerate() {
        for (level, (width, height, rgba)) in mip_chain(face.width, face.height, &face.rgba)
            .iter()
            .enumerate()
        {
            let (data, bytes_per_row) = padded_rows(*width, *height, rgba);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(*height),
                },
                wgpu::Extent3d {
                    width: *width,
                    height: *height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("point10-plane9-stormy-cube-view"),
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("point10-plane9-stormy-cube-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    Some((texture, view, sampler))
}

/// A 1x1 opaque-grey cubemap stand-in for a build without the Stormy Days
/// faces, so `Though The Noise` keeps its bind-group shape.
pub fn fallback_cube(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::Sampler) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("point10-plane9-fallback-cube"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
        &[96, 96, 96, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("point10-plane9-fallback-cube-view"),
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    });
    let sampler = plane9_sampler(device, "point10-plane9-fallback-cube-sampler");
    (texture, view, sampler)
}

/// Pad an RGBA8 mip's rows to WebGPU's 256-byte copy alignment.
fn padded_rows(width: u32, height: u32, rgba: &[u8]) -> (Vec<u8>, u32) {
    let unpadded = width as usize * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let padded = unpadded.div_ceil(align) * align;
    if padded == unpadded {
        return (rgba.to_vec(), unpadded as u32);
    }
    let mut data = vec![0_u8; padded * height as usize];
    for row in 0..height as usize {
        let src = &rgba[row * unpadded..(row + 1) * unpadded];
        let dst = &mut data[row * padded..row * padded + unpadded];
        dst.copy_from_slice(src);
    }
    (data, padded as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_layout_is_std140_compatible() {
        assert_eq!(std::mem::size_of::<Plane9Uniforms>(), 96);
        assert_eq!(std::mem::size_of::<Plane9QuadInstance>(), 32);
        // `mat4x4` (64) + five `vec4` (80).
        assert_eq!(std::mem::size_of::<Plane9QuadUniforms>(), 144);
    }

    #[test]
    fn plane9_camera_looks_down_negative_z() {
        let matrix = plane9_camera(1.0, [0.0, 0.0, 8.3], [0.0, 0.0, -1.0], 45.0, 0.1, 1000.0);
        // The origin is in front of the camera: clip w > 0 and z inside 0..w.
        let clip = crate::point10::scenes::cube_mesh::transform_point(&matrix, [0.0, 0.0, 0.0]);
        assert!(clip[3] > 0.0, "{clip:?}");
        assert!(clip[2] >= 0.0 && clip[2] <= clip[3], "{clip:?}");
    }

    #[test]
    fn stormy_faces_match_the_wgpu_cube_layer_order() {
        assert_eq!(
            STORMY_FACES,
            [
                "stormy/posx.jpg",
                "stormy/negx.jpg",
                "stormy/posy.jpg",
                "stormy/negy.jpg",
                "stormy/posz.jpg",
                "stormy/negz.jpg",
            ]
        );
    }

    #[test]
    fn batch_wgsl_keeps_the_ported_formulas() {
        for needle in [
            "fn p9_rand4(p0: vec2<f32>) -> vec4<f32>",
            "p0 * vec2<f32>(5.3987, 5.4421)",
            "a * 95.4307, a * 97.5901, a * 93.8369, a * 91.6931",
            "fn p9_voronoi3(position: vec3<f32>) -> P9Voronoi3",
            "let jitter = 0.9;",
            "vec3<f32>(1.171, 1.073, 1.127)",
            "result.f1 = sqrt(result.f1);",
            "fn p9_noise_gradient3(p: vec3<f32>, d: f32) -> vec3<f32>",
            "fn p9_brightness_saturation_contrast(",
            "vec3<f32>(0.2126, 0.7152, 0.0722)",
            "fn p9_mod_f(x: f32, y: f32) -> f32",
        ] {
            assert!(PLANE9_BATCH_WGSL.contains(needle), "lost {needle}");
        }
    }
}
