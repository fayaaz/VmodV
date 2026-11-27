//! The Cave — Rust/wgpu port of the Plane9 "The Cave" preset.
//!
//! Source: `../Point10/public/plane9/xml/Nature/The Cave.xml` (the copy in the
//! Point10 repo this project vendors its Plane9 ports from), plus the
//! scene-specific expression overrides in `Point10/src/plane9/P9Overrides.js`.
//! The Plane9 preset is by Joakim Dahl, CC BY-NC-SA 3.0, and is itself based on
//! BoyC's Shadertoy "The Cave" (<https://www.shadertoy.com/view/MsX3RH>).
//!
//! # What the preset actually draws
//!
//! A fullscreen distance-field raymarch through a network of ten winding
//! tunnels (`Dist2D` sums ten `Distance` terms). The camera glides along the
//! main tunnel (`cama/camb/camc/camd`) looking slightly ahead, each hit is
//! lit by a light that travels the same path half a unit ahead of it (with a
//! 10-step shadow march), and the cave walls are triplanar-mapped from
//! `steel_diffuse.jpg` with a ridged-multifractal modulation.
//!
//! The Plane9 node graph is:
//!
//! ```text
//! Shader1 (raymarch + steel texture)
//!   -> RenderToTexture1 (full-res, float16)
//!   -> RenderToTexture2 (half-width, 9-tap tent upsample)
//!   -> Bloom1 (Radius 5, Threshold 0.3, Smoothness 0.5, Intensity 1)
//!   -> Shader2 (_tonemapACES(bloom + RTT2)) -> Screen1 (opaque)
//! ```
//!
//! `RenderToTexture1`'s own shader (a 256x256 FBM generator) is dead code in
//! the preset: the node has an `Effect` connection from `Shader1`, and Plane9
//! renders the incoming chain *instead* of the shader in that case
//! (`P9Scene.js:606-616`). The port keeps the live chain only.
//!
//! # Ported exactly
//!
//! * all ten tunnel terms, the 75-step primary march, 6 binary-refinement
//!   steps, 3-axis normal, triplanar blend, 10-step shadow march, the ACES-fit
//!   wall colour and the `pow(..., 100000.0)` light wisp;
//! * the audio Expression with the port's retuned gates
//!   (`P9Overrides.js:36-51`): `perm += min((0.7 + band(-1,0,0,0.1) * 0.7) *
//!   dt, 0.05)`, `in1.x = min(1 + max(band(-1,1,0,1) - 0.1, 0) * 5 + beat(0) *
//!   3, 10)` and `in1.y = perm / 6 + 291`;
//! * `RT_SCALE = 1.0` for this preset — the expensive march runs at native
//!   resolution, and `RenderToTexture2` is 50% W x 100% H
//!   (`P9Overrides.js:356-358`, `P9Scene.js:899-908`);
//! * the Narkowicz ACES fit `_tonemapACES` (`P9Glsl.js:459-467`) in Shader2,
//!   which adds the bloom result to the sharp upsample before tonemapping;
//! * the GLSL camera transform `normalize(...) * mat3(CamRight, CamUp, CamDir)`.
//!   GLSL treats a vector on the left as a row vector, so this is `Mᵀ d`, not
//!   `M d`. Using the forward transform keeps the view on `CamDir`, which
//!   drifts away from the travelling light whenever the path bends in y — a
//!   unit test pins the centre-ray alignment across the whole path.
//!
//! # Replaced Plane9 machinery
//!
//! | Plane9 | Rust |
//! |--------|------|
//! | `Shader` node + `RenderRect` | [`FULLSCREEN_TRIANGLE_WGSL`] + explicit pipelines |
//! | `RenderToTexture` 5/6 (float16) | [`crate::point10::postfx::create_target`]-style HDR targets |
//! | `Bloom` node | [`UnrealBloomPass`] (Radius 5 -> 0.85, threshold 0.3) |
//! | `_ridgedmffast` | `cave_ridgedmffast`, the same helper `sun.rs` inlines |
//! | `P9Audio.band/beat` | [`P10Audio`] over the shared [`AudioBridge`] |
//!
//! # Deviations
//!
//! * `texture()` in the raymarch becomes `textureSampleLevel(..., 0.0)`: the
//!   steel texture is sampled at base level in both cases (the preset's mip
//!   hint is unused) and explicit LOD keeps the helper valid after the
//!   march's data-dependent loop breaks.
//! * The 9-tap tent upsample reads the source with raw texel offsets, without
//!   the source mip chain; `RenderToTexture2`'s mipmaps are only read by the
//!   preset's own shader, which is not part of the live chain.
//! * OutputPass sRGB encoding is done in the composite shader when the
//!   destination is a plain UNORM format, as in every other port.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::postfx::{UnrealBloomPass, create_target};
use crate::point10::shared::assets::{ColourSpace, LoadedTexture, Wrap, load_texture};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::{
    FULLSCREEN_TRIANGLE_WGSL, HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp,
    frame_dt, lens_value, plane9_scene_props, read_scene_props,
};

/// `Shader1` raymarch precision (`numit`).
pub const NUM_ITERATIONS: u32 = 75;
/// `Shader1` tunnel thickness (`threshold`).
pub const MARCH_THRESHOLD: f32 = 1.20;
/// `Shader1` step length (`scale`).
pub const MARCH_SCALE: f32 = 1.5;
/// `Shader1` binary refinement count.
pub const REFINE_STEPS: u32 = 6;
/// `Shader1` shadow march precision (`ShadowIT`).
pub const SHADOW_STEPS: u32 = 10;
/// `Bloom1` Radius 5, normalised the same way as Light Tunnel (5 -> 0.85).
pub const BLOOM_RADIUS: f32 = 0.85;
/// `Bloom1` Threshold.
pub const BLOOM_THRESHOLD: f32 = 0.3;
/// `Bloom1` Intensity.
pub const BLOOM_STRENGTH: f32 = 1.0;
/// `RenderToTexture2` width: Width=1 of the DLL combo map = 50%.
pub const UPSAMPLE_WIDTH_DIVISOR: u32 = 2;
/// `Shader1`'s `gTexture1`.
pub const STEEL_TEXTURE: &str = "steel_diffuse.jpg";
/// The expression's per-frame cap on the perm advance.
pub const MAX_PERM_STEP: f32 = 0.05;
/// `in1.y = perm / 6 + 291`.
pub const TIME_OFFSET: f32 = 291.0;
/// `in1.y` divisor.
pub const TIME_DIVISOR: f32 = 6.0;
/// Light gate after the `P9Overrides.js` retune.
pub const LIGHT_GATE: f32 = 0.1;
/// Light range ceiling.
pub const LIGHT_CEILING: f32 = 10.0;
/// Bass speed gate after the `P9Overrides.js` retune.
pub const SPEED_BASE: f32 = 0.7;

/// `sceneProps.js`-shaped levers, following the other fullscreen Point10
/// presets (the Plane9 XML has no modV props of its own).
const CAVE_PROPS: &[SceneProp] = &[
    SceneProp::float("scrollSpeed", "Scroll Speed", 1.0, 0.0, 4.0, 0.01),
    SceneProp::float("glow", "Glow", 1.0, 0.0, 3.0, 0.01),
    SceneProp::float("bloom", "Bloom", 1.0, 0.0, 3.0, 0.01),
];

/// One frame of the preset's audio Expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaveDrive {
    /// `perm` for the next frame.
    pub perm: f32,
    /// `in1.x`, the diffuse/wisp light level.
    pub in1_x: f32,
    /// `in1.y`, the raymarch's tunnel position.
    pub in1_y: f32,
}

/// Pure port of Expression1 with the `P9Overrides.js` retuned gates.
///
/// `speed_band = band(-1, 0, 0, 0.1)`, `light_band = band(-1, 1, 0, 1)` and
/// `beat = beat(0)`; the caller reads those from [`P10Audio`].
pub fn cave_drive(
    speed_band: f32,
    light_band: f32,
    beat: f32,
    dt: f32,
    perm: f32,
    speed_scale: f32,
    glow_scale: f32,
) -> CaveDrive {
    let finite = |value: f32, fallback: f32| {
        if value.is_finite() { value } else { fallback }
    };
    let speed_band = finite(speed_band, 0.1).max(0.0);
    let light_band = finite(light_band, 1.0);
    let beat = finite(beat, 0.0).max(0.0);
    let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let speed_scale = finite(speed_scale, 1.0).max(0.0);
    let glow_scale = finite(glow_scale, 1.0).max(0.0);
    let perm = finite(perm, 1.0);

    let advance = ((SPEED_BASE + speed_band * SPEED_BASE) * step * speed_scale).min(MAX_PERM_STEP);
    let next = perm + advance;
    let light = (1.0 + (light_band - LIGHT_GATE).max(0.0) * 5.0 + beat * 3.0).min(LIGHT_CEILING);

    CaveDrive {
        perm: next,
        in1_x: light * glow_scale,
        in1_y: next / TIME_DIVISOR + TIME_OFFSET,
    }
}

/// GLSL's `Position(z, a, b, c, d)` — the centre of one tunnel slice.
pub fn tunnel_position(z: f32, a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> [f32; 2] {
    let mut out = [0.0; 2];
    for i in 0..2 {
        out[i] = (z * a[i]).sin() * b[i] + (z * c[i]).cos() * d[i];
    }
    out
}

/// Camera path constants (`cama/camb/camc/camd` in Shader1). The CPU twins
/// below mirror the WGSL so the camera/light alignment can be tested without
/// a device.
#[cfg(test)]
const CAMA: [f32; 2] = [-2.6943, 3.0483];
#[cfg(test)]
const CAMB: [f32; 2] = [0.2516, 0.1749];
#[cfg(test)]
const CAMC: [f32; 2] = [-3.7902, 2.4478];
#[cfg(test)]
const CAMD: [f32; 2] = [0.0865, -0.1664];

#[cfg(test)]
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
fn normalise3(v: [f32; 3]) -> [f32; 3] {
    let length = dot3(v, v).sqrt();
    if length > 0.0 {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// The camera basis at tunnel position `t` (`CamDir`, `CamRight`, `CamUp`).
#[cfg(test)]
fn cave_camera_basis(t: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let pos = tunnel_position(t, CAMA, CAMB, CAMC, CAMD);
    let ahead = tunnel_position(t + 0.05, CAMA, CAMB, CAMC, CAMD);
    let forward = normalise3([ahead[0] - pos[0], -ahead[1] + pos[1], 0.1]);
    let right = normalise3(cross3(forward, [0.0, 1.0, 0.0]));
    let up = normalise3(cross3(right, forward));
    (forward, right, up)
}

/// The ray at the centre pixel, matching the shader's `d * mat3(R, U, F)`.
///
/// GLSL treats a vector on the left of a matrix as a row vector, so
/// `d * mat3(CamRight, CamUp, CamDir)` is `Mᵀ d`, *not* `M d`. Using `M d`
/// keeps the view pointed along `CamDir`, which drifts away from the light
/// whenever the tunnel path bends in y.
#[cfg(test)]
fn cave_centre_ray(t: f32) -> [f32; 3] {
    let (forward, right, up) = cave_camera_basis(t);
    [right[2], up[2], forward[2]]
}

/// `gIn1`-shaped uniform block shared by the raymarch shader (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CaveUniforms {
    in1: [f32; 4],
    resolution: [f32; 2],
    _pad: [f32; 2],
}

/// `RenderToTexture2`'s 9-tap tent upsample uniforms (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct UpsampleUniforms {
    /// `1 / source.width`, `1 / source.height`.
    inv_size: [f32; 2],
    /// Destination size.
    resolution: [f32; 2],
}

/// `Shader2` composite uniforms (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniforms {
    /// Destination size.
    resolution: [f32; 2],
    /// Shader-side sRGB encode for plain UNORM destinations.
    srgb_encode: u32,
    /// Shared Plane9 Lens magnification applied to the composite UV.
    ///
    /// The raymarch's distance fog and light falloff are non-linear, so
    /// changing the ray/FOV by `lens` moved the lit area by ~6x/1.44x at
    /// lens 2/0.5 instead of the intended 4x/0.25x. Zooming the composed
    /// image measures 4.02x/0.246x, matching the lens² area contract.
    lens: f32,
}

/// GPU resources, rebuilt on resize (the bind groups hold texture views).
struct CaveGpu {
    queue: wgpu::Queue,
    cave_pipeline: wgpu::RenderPipeline,
    cave_bind: wgpu::BindGroup,
    cave_uniforms: wgpu::Buffer,
    upsample_pipeline: wgpu::RenderPipeline,
    upsample_bind: wgpu::BindGroup,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind: wgpu::BindGroup,
    composite_uniforms: wgpu::Buffer,
    _cave_texture: wgpu::Texture,
    cave_view: wgpu::TextureView,
    _up_texture: wgpu::Texture,
    up_view: wgpu::TextureView,
    bloom: UnrealBloomPass,
}

/// `Plane9 The Cave` scene.
pub struct TheCave {
    audio: P10Audio,
    perm: f32,
    speed_scale: f32,
    glow_scale: f32,
    bloom_scale: f32,
    lens: f32,
    in1: [f32; 4],
    size: (u32, u32),
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    target_format: wgpu::TextureFormat,
    assets_dir: Option<PathBuf>,
    gpu: Option<CaveGpu>,
}

impl Default for TheCave {
    fn default() -> Self {
        Self::new()
    }
}

impl TheCave {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            speed_scale: 1.0,
            glow_scale: 1.0,
            bloom_scale: 1.0,
            lens: 1.0,
            in1: [0.0; 4],
            size: (1, 1),
            device: None,
            queue: None,
            target_format: HDR_FORMAT,
            assets_dir: None,
            gpu: None,
        }
    }

    /// Bind the modV props (`sceneProps.js` equivalent).
    pub fn apply_props(&mut self, mut props: impl FnMut(&str) -> Option<f32>) {
        self.lens = lens_value(props("lens"));
        for (key, value) in read_scene_props(CAVE_PROPS, &mut props) {
            match key {
                "scrollSpeed" => self.speed_scale = value,
                "glow" => self.glow_scale = value,
                "bloom" => self.bloom_scale = value,
                _ => {}
            }
        }
        if let Some(gpu) = &self.gpu {
            gpu.queue.write_buffer(
                &gpu.composite_uniforms,
                0,
                bytemuck::bytes_of(&CompositeUniforms {
                    resolution: [self.size.0.max(1) as f32, self.size.1.max(1) as f32],
                    srgb_encode: u32::from(!self.target_format.is_srgb()),
                    lens: self.lens,
                }),
            );
        }
    }

    /// Current scroll phase.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// The `gIn1` uniform as last uploaded.
    pub fn in1(&self) -> [f32; 4] {
        self.in1
    }

    /// The live bloom strength, or `None` before `init`.
    pub fn bloom_strength(&self) -> Option<f32> {
        self.gpu.as_ref().map(|gpu| gpu.bloom.strength())
    }

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        self.gpu = None;

        let (width, height) = self.size;
        let up_width = (width / UPSAMPLE_WIDTH_DIVISOR).max(1);
        let up_height = height.max(1);

        let steel: LoadedTexture = {
            let loader = match &self.assets_dir {
                Some(dir) => crate::point10::shared::assets::AssetLoader::for_assets_dir(Some(dir)),
                None => crate::point10::shared::assets::AssetLoader::new(),
            };
            load_texture(
                &device,
                &queue,
                &loader,
                STEEL_TEXTURE,
                ColourSpace::Srgb,
                Wrap::Repeat,
            )
        };

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("point10-the-cave-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // --- Shader1: the raymarch, into a full-res HDR target -------------
        let cave_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-the-cave-raymarch"),
            source: wgpu::ShaderSource::Wgsl(cave_wgsl().into()),
        });
        let cave_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-the-cave-raymarch-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let cave_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-the-cave-raymarch-uniforms"),
            size: std::mem::size_of::<CaveUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-the-cave-raymarch-pipeline-layout"),
            bind_group_layouts: &[Some(&cave_layout)],
            immediate_size: 0,
        });
        let cave_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-the-cave-raymarch-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &cave_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &cave_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let (_cave_texture, cave_view) =
            create_target(&device, "point10-the-cave-capture", width, height);
        let cave_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-the-cave-raymarch-bind-group"),
            layout: &cave_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cave_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&steel.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&steel.sampler),
                },
            ],
        });

        // --- RenderToTexture2: 9-tap tent upsample ------------------------
        let up_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-the-cave-upsample"),
            source: wgpu::ShaderSource::Wgsl(upsample_wgsl().into()),
        });
        let upsample_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-the-cave-upsample-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let upsample_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-the-cave-upsample-uniforms"),
            size: std::mem::size_of::<UpsampleUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &upsample_uniforms,
            0,
            bytemuck::bytes_of(&UpsampleUniforms {
                inv_size: [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
                resolution: [up_width as f32, up_height as f32],
            }),
        );
        let up_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-the-cave-upsample-pipeline-layout"),
            bind_group_layouts: &[Some(&upsample_layout)],
            immediate_size: 0,
        });
        let upsample_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-the-cave-upsample-pipeline"),
            layout: Some(&up_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &up_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &up_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let (_up_texture, up_view) =
            create_target(&device, "point10-the-cave-upsample", up_width, up_height);
        let upsample_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-the-cave-upsample-bind-group"),
            layout: &upsample_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: upsample_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cave_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // --- Bloom1, over the upsampled capture ---------------------------
        let mut bloom = UnrealBloomPass::new(&device, &queue, up_width, up_height);
        bloom.set_strength(BLOOM_STRENGTH * self.bloom_scale);
        bloom.set_radius(BLOOM_RADIUS);
        bloom.set_threshold(BLOOM_THRESHOLD);
        bloom.set_input(&up_view);

        // --- Shader2: ACES(bloom + sharp), straight to the destination ----
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-the-cave-composite"),
            source: wgpu::ShaderSource::Wgsl(composite_wgsl().into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-the-cave-composite-layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                texture_entry(2),
                sampler_entry(3),
            ],
        });
        let composite_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-the-cave-composite-uniforms"),
            size: std::mem::size_of::<CompositeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &composite_uniforms,
            0,
            bytemuck::bytes_of(&CompositeUniforms {
                resolution: [width.max(1) as f32, height.max(1) as f32],
                srgb_encode: u32::from(!self.target_format.is_srgb()),
                lens: self.lens,
            }),
        );
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("point10-the-cave-composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-the-cave-composite-pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
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

        // The bloom composite target is recreated on resize, so this bind
        // group is rebuilt along with the rest of the GPU state.
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-the-cave-composite-bind-group"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: composite_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        bloom
                            .composite_view()
                            .expect("bloom composite target exists"),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&up_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        self.gpu = Some(CaveGpu {
            queue,
            cave_pipeline,
            cave_bind,
            cave_uniforms,
            upsample_pipeline,
            upsample_bind,
            composite_pipeline,
            composite_bind,
            composite_uniforms,
            _cave_texture,
            cave_view,
            _up_texture,
            up_view,
            bloom,
        });
    }

    fn upload_uniforms(&self) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        gpu.queue.write_buffer(
            &gpu.cave_uniforms,
            0,
            bytemuck::bytes_of(&CaveUniforms {
                in1: self.in1,
                resolution: [self.size.0.max(1) as f32, self.size.1.max(1) as f32],
                _pad: [0.0; 2],
            }),
        );
    }
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

/// The cave raymarch (`Shader1`) with the shared Plane9 helpers in front.
fn cave_wgsl() -> String {
    format!("{FULLSCREEN_TRIANGLE_WGSL}\n{P9_COMMON_WGSL}\n{CAVE_WGSL}")
}

const CAVE_WGSL: &str = r#"
struct CaveUniforms {
  in1: vec4<f32>,
  resolution: vec2<f32>,
  _pad: vec2<f32>,
}
@group(0) @binding(0) var<uniform> u: CaveUniforms;
@group(0) @binding(1) var steel: texture_2d<f32>;
@group(0) @binding(2) var steel_sampler: sampler;

const CAMA = vec2<f32>(-2.6943, 3.0483);
const CAMB = vec2<f32>(0.2516, 0.1749);
const CAMC = vec2<f32>(-3.7902, 2.4478);
const CAMD = vec2<f32>(0.0865, -0.1664);

const LIGHTA = vec2<f32>(1.4301, 4.0985);
const LIGHTB = vec2<f32>(-0.1276, 0.2347);
const LIGHTC = vec2<f32>(-2.2655, 1.5066);
const LIGHTD = vec2<f32>(-0.1284, 0.0731);

// _ridgedmffast, inlined the same way sun.rs inlines it (P9Glsl.js:100-113).
fn cave_ridgedmffast(p0: vec3<f32>, octaves: u32) -> f32 {
  let lacunarity = 2.13628142;
  let gain = 0.5;
  let offset = 1.0;
  var sum = 0.0;
  var amp_sum = 0.0;
  var freq = 1.0;
  var amp = 0.5;
  var prev = 1.0;
  for (var i = 0u; i < octaves; i = i + 1u) {
    var n = offset - abs(p9_noise(p0 * freq));
    n = n * n;
    sum = sum + n * amp * prev;
    amp_sum = amp_sum + amp;
    prev = n;
    freq = freq * lacunarity;
    amp = amp * gain;
  }
  return sum / amp_sum;
}

// Position(z, a, b, c, d).
fn cave_position(z: f32, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>) -> vec2<f32> {
  return sin(z * a) * b + cos(z * c) * d;
}

// Distance(p, a, b, c, d, e, r) — one tunnel slice.
fn cave_distance(p: vec3<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>, e: vec2<f32>, r: f32) -> f32 {
  let pos = cave_position(p.z, a, b, c, d);
  let radius = max(5.0, r + sin(p.z * e.x) * e.y) / 10000.0;
  let delta = p.xy - pos;
  return radius / dot(delta, delta);
}

// Dist2D — the ten tunnels, verbatim from the preset.
fn cave_dist_2d(pos: vec3<f32>) -> f32 {
  var d = 0.0;
  d = d + cave_distance(pos, CAMA, CAMB, CAMC, CAMD, vec2<f32>(2.1913, 15.4634), 70.0);
  d = d + cave_distance(pos, LIGHTA, LIGHTB, LIGHTC, LIGHTD, vec2<f32>(0.3814, 12.7206), 17.0590);
  d = d + cave_distance(pos, vec2<f32>(2.7377, -1.2462), vec2<f32>(-0.1914, -0.2339), vec2<f32>(-1.3698, -0.6855), vec2<f32>(0.1049, -0.1347), vec2<f32>(-1.1157, 13.6200), 27.3718);
  d = d + cave_distance(pos, vec2<f32>(-2.3815, 0.2382), vec2<f32>(-0.1528, -0.1475), vec2<f32>(0.9996, -2.1459), vec2<f32>(-0.0566, -0.0854), vec2<f32>(0.3287, 12.1713), 21.8130);
  d = d + cave_distance(pos, vec2<f32>(-2.7424, 4.8901), vec2<f32>(-0.1257, 0.2561), vec2<f32>(-0.4138, 2.6706), vec2<f32>(-0.1355, 0.1648), vec2<f32>(2.8162, 14.8847), 32.2235);
  d = d + cave_distance(pos, vec2<f32>(-2.2158, 4.5260), vec2<f32>(0.2834, 0.2319), vec2<f32>(4.2578, -2.5997), vec2<f32>(-0.0391, -0.2070), vec2<f32>(2.2086, 13.0546), 30.9920);
  d = d + cave_distance(pos, vec2<f32>(0.9824, 4.4131), vec2<f32>(0.2281, -0.2955), vec2<f32>(-0.6033, 0.4780), vec2<f32>(-0.1544, 0.1360), vec2<f32>(3.2020, 12.2138), 29.1169);
  d = d + cave_distance(pos, vec2<f32>(1.2733, -2.4752), vec2<f32>(-0.2821, -0.1180), vec2<f32>(3.4862, -0.7046), vec2<f32>(0.0224, 0.2024), vec2<f32>(-2.2714, 9.7317), 6.3008);
  d = d + cave_distance(pos, vec2<f32>(2.6860, 2.3608), vec2<f32>(-0.1486, 0.2376), vec2<f32>(2.0568, 1.5440), vec2<f32>(0.0367, 0.1594), vec2<f32>(-2.0396, 10.2225), 25.5348);
  d = d + cave_distance(pos, vec2<f32>(0.5009, 0.9612), vec2<f32>(0.1818, -0.1669), vec2<f32>(0.0698, -2.0880), vec2<f32>(0.1424, 0.1063), vec2<f32>(1.7980, 11.2733), 35.7880);
  return d;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let t = u.in1.y;

  // Camera looking ahead along the main tunnel.
  let ahead = cave_position(t + 0.05, CAMA, CAMB, CAMC, CAMD);
  var pos = vec3<f32>(cave_position(t, CAMA, CAMB, CAMC, CAMD), t);
  let origin = pos;
  let cam_dir = normalize(vec3<f32>(ahead.x - pos.x, -ahead.y + pos.y, 0.1));
  let cam_right = normalize(cross(cam_dir, vec3<f32>(0.0, 1.0, 0.0)));
  let cam_up = normalize(cross(cam_right, cam_dir));

  // PlaneGeometry's vUv has v = 0 at the bottom; fragment position has it at
  // the top, so flip y to keep the tunnel upright.
  let uv01 = position.xy / u.resolution;
  let uv = vec2<f32>(uv01.x * 2.0 - 1.0, 1.0 - uv01.y * 2.0);
  let aspect = u.resolution.x / u.resolution.y;
  let dir = normalize(vec3<f32>(uv * vec2<f32>(aspect, 1.0), 1.0));
  // GLSL `normalize(...) * mat3(CamRight, CamUp, CamDir)` is Mᵀ * d (a vector
  // on the left is a row vector). Using M * d here makes the view drift away
  // from the travelling light whenever the tunnel bends in y.
  let ray = vec3<f32>(dot(dir, cam_right), dot(dir, cam_up), dot(dir, cam_dir));

  // First hit: 75 steps, then six bisection steps.
  var fade = 0.0;
  var pos_m1 = pos;
  for (var i = 0u; i < 75u; i = i + 1u) {
    if (cave_dist_2d(pos) < 1.20) {
      fade = 1.0 - f32(i) / 75.0;
      break;
    }
    pos_m1 = pos;
    pos = pos + ray / 75.0 * 1.5;
  }
  for (var i = 0u; i < 6u; i = i + 1u) {
    let middle = (pos_m1 + pos) * 0.5;
    if (cave_dist_2d(middle) < 1.20) {
      pos = middle;
    } else {
      pos_m1 = middle;
    }
  }

  // Normal, triplanar blend vector and travelling light.
  let n = normalize(vec3<f32>(
    cave_dist_2d(pos + vec3<f32>(0.01, 0.0, 0.0)) - cave_dist_2d(pos - vec3<f32>(0.01, 0.0, 0.0)),
    cave_dist_2d(pos + vec3<f32>(0.0, 0.01, 0.0)) - cave_dist_2d(pos - vec3<f32>(0.0, 0.01, 0.0)),
    cave_dist_2d(pos + vec3<f32>(0.0, 0.0, 0.01)) - cave_dist_2d(pos - vec3<f32>(0.0, 0.0, 0.01))));
  let tpn = normalize(max(vec3<f32>(0.0), (abs(n) - vec3<f32>(0.2)) * 7.0)) * 0.5;
  let light_pos = vec3<f32>(cave_position(t + 0.5, CAMA, CAMB, CAMC, CAMD), t + 0.5);
  let light_dir = light_pos - pos;
  var lit = 1.0;
  for (var i = 1u; i < 10u; i = i + 1u) {
    if (cave_dist_2d(pos + light_dir * (f32(i) / 10.0)) < 1.20) {
      lit = 0.0;
      break;
    }
  }

  let tuv = pos * vec3<f32>(6.0, 6.0, 3.0);
  let diffuse = max(0.0, dot(n, normalize(light_dir)));
  let diff = vec4<f32>(diffuse * 0.1 * lit * u.in1.x) + vec4<f32>(0.075);

  let wisp = pow(dot(normalize(pos - origin), normalize(light_pos - origin)), 100000.0) * u.in1.x * 2.0;
  var w = wisp;
  if (length(pos - origin) < length(light_pos - origin)) {
    w = 0.0;
  }

  var col = textureSampleLevel(steel, steel_sampler, tuv.yz, 0.0) * tpn.x
          + textureSampleLevel(steel, steel_sampler, tuv.xz, 0.0) * tpn.y
          + textureSampleLevel(steel, steel_sampler, tuv.xy, 0.0) * tpn.z;
  col = col * (0.25 + (cave_ridgedmffast(tuv * vec3<f32>(2.0, 30.0, 2.0), 3u) * 0.5 + 0.5) * 0.75);

  return col * diff * min(1.0, fade * 10.0) + vec4<f32>(w, w, w, 0.0);
}
"#;

/// `RenderToTexture2`'s `UpsampleFilter` (the preset's 9-tap tent).
fn upsample_wgsl() -> String {
    format!("{FULLSCREEN_TRIANGLE_WGSL}\n{UPSAMPLE_WGSL}")
}

const UPSAMPLE_WGSL: &str = r#"
struct UpsampleUniforms {
  inv_size: vec2<f32>,
  resolution: vec2<f32>,
}
@group(0) @binding(0) var<uniform> u: UpsampleUniforms;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

fn tap(uv: vec2<f32>) -> vec3<f32> {
  return textureSampleLevel(source, source_sampler, uv, 0.0).rgb;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = position.xy / u.resolution;
  let tx = u.inv_size.x;
  let ty = u.inv_size.y;
  var s = tap(uv - vec2<f32>(tx, ty));
  s = s + tap(uv - vec2<f32>(0.0, ty)) * 2.0;
  s = s + tap(uv + vec2<f32>(tx, -ty));
  s = s + tap(uv - vec2<f32>(tx, 0.0)) * 2.0;
  s = s + tap(uv) * 4.0;
  s = s + tap(uv + vec2<f32>(tx, 0.0)) * 2.0;
  s = s + tap(uv + vec2<f32>(tx, -ty));
  s = s + tap(uv + vec2<f32>(0.0, ty)) * 2.0;
  s = s + tap(uv + vec2<f32>(tx, ty));
  return vec4<f32>(s * (1.0 / 16.0), 1.0);
}
"#;

/// `Shader2`: `_tonemapACES(c1.rgb + c2.rgb), 1.0`.
fn composite_wgsl() -> String {
    format!("{FULLSCREEN_TRIANGLE_WGSL}\n{COMPOSITE_WGSL}")
}

const COMPOSITE_WGSL: &str = r#"
struct CompositeUniforms {
  resolution: vec2<f32>,
  srgb_encode: u32,
  lens: f32,
}
@group(0) @binding(0) var<uniform> u: CompositeUniforms;
@group(0) @binding(1) var bloom_tex: texture_2d<f32>;
@group(0) @binding(2) var sharp_tex: texture_2d<f32>;
@group(0) @binding(3) var linear_sampler: sampler;

// _tonemapACES (P9Glsl.js:459-467, the Narkowicz fit).
fn cave_aces(color: vec3<f32>) -> vec3<f32> {
  let a = 2.51;
  let b = 0.03;
  let c = 2.43;
  let d = 0.59;
  let e = 0.14;
  return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn srgb_transfer_oetf(value: vec3<f32>) -> vec3<f32> {
  let low = value * 12.92;
  let high = pow(value, vec3<f32>(0.41666)) * 1.055 - vec3<f32>(0.055);
  return select(high, low, value <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  var uv = position.xy / u.resolution;
  // Lens magnifies the composed frame around its centre. A ray/FOV zoom was
  // rejected here: the tunnel's distance fog and light falloff are non-linear,
  // so it did not produce the contract's lens² lit-area scaling. `lens == 1.0`
  // skips the remap so the vendor framing stays bit-identical.
  if (u.lens != 1.0) {
    uv = (uv - vec2<f32>(0.5)) / u.lens + vec2<f32>(0.5);
  }
  let c1 = textureSampleLevel(bloom_tex, linear_sampler, uv, 0.0).rgb;
  let c2 = textureSampleLevel(sharp_tex, linear_sampler, uv, 0.0).rgb;
  var color = cave_aces(c1 + c2);
  if (u.srgb_encode != 0u) {
    color = srgb_transfer_oetf(color);
  }
  return vec4<f32>(color, 1.0);
}
"#;

impl Point10Scene for TheCave {
    fn name(&self) -> &'static str {
        "Plane9 The Cave"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.assets_dir = ctx.assets_dir.clone();
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.rebuild();
        self.upload_uniforms();
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if self.size == size {
            return;
        }
        self.size = size;
        self.rebuild();
        self.upload_uniforms();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        self.audio.update(frame_dt(dt), audio);

        let drive = cave_drive(
            self.audio.band(-1, 0, 0, 0.1),
            self.audio.band(-1, 1, 0, 1.0),
            self.audio.beat(0.0),
            dt,
            self.perm,
            self.speed_scale,
            self.glow_scale,
        );
        self.perm = drive.perm;
        self.in1 = [drive.in1_x, drive.in1_y, 0.0, 0.0];
        self.upload_uniforms();

        if let Some(gpu) = &mut self.gpu {
            gpu.bloom.set_strength(BLOOM_STRENGTH * self.bloom_scale);
        }
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &self.gpu else {
            return;
        };

        // Shader1 into the full-res capture.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-the-cave-raymarch"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.cave_view,
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
            pass.set_pipeline(&gpu.cave_pipeline);
            pass.set_bind_group(0, &gpu.cave_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        // RenderToTexture2: 9-tap tent downsample/upsample.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-the-cave-upsample"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.up_view,
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
            pass.set_pipeline(&gpu.upsample_pipeline);
            pass.set_bind_group(0, &gpu.upsample_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        // Bloom1, then Shader2 straight into the destination.
        gpu.bloom.encode(encoder);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("point10-the-cave-composite"),
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
            pass.set_bind_group(0, &gpu.composite_bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        plane9_scene_props(CAVE_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 The Cave", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::{AUDIO_SAMPLES, BandPayload};
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn silence_follows_the_nomusic_defaults() {
        // band(-1,0,0,0.1) -> 0.1, band(-1,1,0,1) -> 1, beat(0) -> 0.
        let drive = cave_drive(0.1, 1.0, 0.0, 1.0 / 60.0, 1.0, 1.0, 1.0);
        let expected_perm = 1.0 + (0.7 + 0.1 * 0.7) / 60.0;
        assert!((drive.perm - expected_perm).abs() < 1e-5, "{drive:?}");
        assert!((drive.in1_x - 5.5).abs() < 1e-5, "{drive:?}");
        assert!((drive.in1_y - (expected_perm / 6.0 + 291.0)).abs() < 1e-4);
    }

    #[test]
    fn perm_step_and_light_are_capped() {
        let drive = cave_drive(10.0, 10.0, 10.0, 10.0, 0.0, 4.0, 3.0);
        assert_eq!(drive.perm, MAX_PERM_STEP);
        assert_eq!(drive.in1_x, LIGHT_CEILING * 3.0);
    }

    #[test]
    fn non_finite_inputs_fall_back_to_the_expression_defaults() {
        let drive = cave_drive(
            f32::NAN,
            f32::INFINITY,
            f32::NAN,
            f32::NAN,
            f32::NAN,
            f32::NAN,
            f32::NAN,
        );
        assert_eq!(drive.perm, 1.0);
        assert_eq!(drive.in1_x, 5.5);
        assert!(drive.in1_y.is_finite());
    }

    #[test]
    fn tunnel_position_matches_the_glsl_at_zero() {
        // sin(0)*b + cos(0)*d == d.
        let a = [1.0, 2.0];
        let b = [3.0, 4.0];
        let c = [5.0, 6.0];
        let d = [7.0, 8.0];
        assert_eq!(tunnel_position(0.0, a, b, c, d), d);
    }

    #[test]
    fn centre_ray_tracks_the_travelling_light() {
        // The light sits half a tunnel-unit ahead of the camera. When the path
        // bends in y the two must stay aligned — this is the
        // `pow(..., 100000.0)` wisp the preset uses to draw the light source.
        for t in [291.0, 300.0, 350.0, 400.0, 500.0, 800.0] {
            let pos = tunnel_position(t, CAMA, CAMB, CAMC, CAMD);
            let light_pos = tunnel_position(t + 0.5, CAMA, CAMB, CAMC, CAMD);
            let light = normalise3([light_pos[0] - pos[0], light_pos[1] - pos[1], 0.5]);
            let alignment = dot3(cave_centre_ray(t), light);
            assert!(
                alignment > 0.9,
                "t={t}: centre ray lost the light (alignment {alignment:.3})"
            );
        }
    }

    #[test]
    fn shader_uses_the_glsl_transpose_camera_transform() {
        let cave = cave_wgsl();
        assert!(
            cave.contains(
                "let ray = vec3<f32>(dot(dir, cam_right), dot(dir, cam_up), dot(dir, cam_dir));"
            ),
            "the raymarch must use GLSL's v*M (transpose) camera transform"
        );
        assert!(
            !cave.contains("cam_right * dir.x"),
            "the forward M*v transform drifts away from the travelling light"
        );
    }

    #[test]
    fn all_shaders_parse_and_validate() {
        for (name, source) in [
            ("raymarch", cave_wgsl()),
            ("upsample", upsample_wgsl()),
            ("composite", composite_wgsl()),
        ] {
            let module = match naga::front::wgsl::parse_str(&source) {
                Ok(module) => module,
                Err(error) => panic!("{name} shader failed to parse: {error:?}"),
            };
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{name} shader must validate: {error:?}"));
        }
    }

    #[test]
    fn shaders_keep_the_ported_constants() {
        let cave = cave_wgsl();
        for needle in [
            "const CAMA = vec2<f32>(-2.6943, 3.0483);",
            "for (var i = 0u; i < 75u; i = i + 1u)",
            "cave_dist_2d(middle) < 1.20",
            "pow(dot(normalize(pos - origin), normalize(light_pos - origin)), 100000.0)",
            "vec3<f32>(2.0, 30.0, 2.0), 3u",
        ] {
            assert!(cave.contains(needle), "raymarch shader lost {needle}");
        }
        let composite = composite_wgsl();
        assert!(composite.contains("let a = 2.51;"));
        assert!(composite.contains("cave_aces(c1 + c2)"));
    }

    #[test]
    fn props_bind_and_clamp() {
        let mut scene = TheCave::new();
        scene.apply_props(|key| match key {
            "scrollSpeed" => Some(9.0),
            "glow" => Some(2.5),
            "bloom" => Some(-1.0),
            _ => None,
        });
        assert_eq!(scene.speed_scale, 4.0);
        assert_eq!(scene.glow_scale, 2.5);
        assert_eq!(scene.bloom_scale, 0.0);
    }

    #[test]
    fn meta_and_props_match_the_adapter_defaults() {
        let scene = TheCave::new();
        assert_eq!(scene.name(), "Plane9 The Cave");
        let meta = scene.meta();
        assert_eq!(meta.name, scene.name());
        assert_eq!(meta.category, "3d-scenes");
        assert_eq!(meta.author, "Plane9 (Joakim Dahl)");
        let props = scene.props();
        for key in ["scrollSpeed", "glow", "bloom"] {
            assert!(props.iter().any(|prop| prop.name == key), "missing {key}");
        }
        for lever in crate::point10::audio_bridge::RESPONSIVENESS_PROPS {
            assert!(props.iter().any(|prop| prop.name == lever.name));
        }
    }

    /// A loud payload for the GPU smoke test (mirrors `scene_tests::loud_payload`).
    fn loud_payload() -> BandPayload {
        let mut payload = BandPayload::silent(44_100.0);
        payload.byte_frequency_data = vec![200; 1024];
        payload.wave = [0.5; AUDIO_SAMPLES];
        payload.beat = 0.8;
        payload.lanes = [0.6; 12];
        payload.cells = [0.6; 16];
        payload.freq_mag = [0.4; AUDIO_SAMPLES];
        payload
    }

    #[test]
    fn renders_two_frames_through_its_own_chain() {
        const SIZE: u32 = 64;
        let Some((device, queue)) = gpu() else {
            eprintln!("skipping The Cave GPU smoke test: no fallback adapter");
            return;
        };

        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(SIZE, SIZE);
        let mut scene = TheCave::new();
        scene.init(&mut ctx);
        scene.resize(SIZE, SIZE);

        let payload = loud_payload();
        let mut bridge = AudioBridge::new();
        let target = TestTarget::new(&device, SIZE, SIZE);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-the-cave-smoke"),
        });
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));

        let pixels = target.read(&device, &queue);
        assert!(
            any_lit(&pixels),
            "expected lit cave pixels, got {:?}",
            &pixels[..16]
        );
        // Shader2 writes alpha 1 over the whole frame: this preset is opaque.
        for y in 0..SIZE {
            for x in 0..SIZE {
                let index = ((y * SIZE + x) * 4 + 3) as usize;
                assert_eq!(pixels[index], 255, "pixel ({x}, {y}) must be opaque");
            }
        }
        assert!(
            scene.perm() > 1.0,
            "expression must advance the scroll phase"
        );
    }
}
