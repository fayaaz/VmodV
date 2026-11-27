//! Plane9's bloom driver as a reusable wgpu pass.
//!
//! Port of the bloom chain behind Plane9's `Bloom` post effect (driver
//! `0x1005cc60` in `Plane9Engine.dll`). It is deliberately **separate** from
//! [`super::UnrealBloomPass`]: the three.js composer scenes keep their
//! UnrealBloomPass semantics, while the Plane9 preset ports use this model.
//!
//! # Driver model
//!
//! ```text
//! mip_count = clamp(trunc(log2(threshold) + radius - 8), 1, 16)
//! prefilter = threshold > 1e-4 || antiflicker
//! knee      = (threshold - threshold*smoothness,
//!              2*threshold*smoothness,
//!              0.25 / (threshold*smoothness))
//! ```
//!
//! A `threshold <= 0` makes `log2` undefined, so the driver (and this port)
//! clamps to a single mip. The knee's third component is defined as `0` when
//! `threshold*smoothness` is `0`, keeping the curve finite.
//!
//! # Pass chain
//!
//! ```text
//! source ─▶ PASS0 prefilter (1 tap)  (or PASS1 prefilter+median, 5 taps)
//!        ─▶ PASS2 Downsample36 (13 taps, brightness-weighted 1/(1+B))
//!        ─▶ PASS3 box (4 taps)
//!        ─▶ PASS4 UpsampleCircular (8 taps, diagonals ×2, /12)
//! ```
//!
//! The downsample/box pair runs once per mip (half resolution, halving with
//! rounding and clamped to at least one pixel, like
//! [`super::UnrealBloomPass`]). The upsample walks back up:
//!
//! ```text
//! for level = mip_count - 2; level >= 0; --level:
//!     out[level] = (base[level] + upsample(out[level + 1])) * gScale
//! ```
//!
//! `gScale` is `intensity` on the final (level 0) upsample and `1` otherwise.
//!
//! # The one-level rule
//!
//! At `mip_count == 1` the upsample loop never runs: the composite is the box
//! result on its own, copied through with no multiplier. `intensity` is
//! therefore **inert** at one level — it is only referenced by the final
//! upsample uniform, which that path never records.

#![forbid(unsafe_code)]

use super::{create_target, half, linear_sampler};
use crate::point10::FULLSCREEN_TRIANGLE_WGSL;

/// The driver clamps its derived mip count to this many levels.
pub const MAX_MIP_COUNT: u32 = 16;

/// The Plane9 bloom driver's parameters.
///
/// The fields mirror the values the DLL reads: `radius` feeds the mip count,
/// `threshold`/`smoothness` build the knee, `intensity` is the final upsample
/// gain and `antiflicker` forces the 5-tap prefilter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane9BloomParams {
    radius: f32,
    threshold: f32,
    smoothness: f32,
    intensity: f32,
    antiflicker: bool,
}

impl Plane9BloomParams {
    /// Build the driver parameters, sanitising non-finite and negative
    /// inputs (the same defensive style as
    /// [`super::afterimage::AfterimagePass`]'s `damp` clamp).
    pub fn new(
        radius: f32,
        threshold: f32,
        smoothness: f32,
        intensity: f32,
        antiflicker: bool,
    ) -> Self {
        Self {
            radius: finite_or(radius, 0.0).max(0.0),
            threshold: finite_or(threshold, 0.0).max(0.0),
            smoothness: finite_or(smoothness, 0.0).clamp(0.0, 1.0),
            intensity: finite_or(intensity, 1.0).max(0.0),
            antiflicker,
        }
    }

    /// The driver's `radius`.
    pub fn radius(&self) -> f32 {
        self.radius
    }

    /// The driver's brightness `threshold`.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// The driver's knee `smoothness` (0..1).
    pub fn smoothness(&self) -> f32 {
        self.smoothness
    }

    /// The driver's final upsample gain.
    pub fn intensity(&self) -> f32 {
        self.intensity
    }

    /// Whether the driver forces the prefilter's 5-tap median variant.
    pub fn antiflicker(&self) -> bool {
        self.antiflicker
    }

    /// `clamp(trunc(log2(threshold) + radius - 8), 1, 16)`.
    ///
    /// `threshold <= 0` has no `log2`; the driver clamps those to one level.
    pub fn mip_count(&self) -> u32 {
        if self.threshold <= 0.0 || !self.radius.is_finite() {
            return 1;
        }
        let levels = self.threshold.log2() + self.radius - 8.0;
        if !levels.is_finite() {
            return 1;
        }
        levels.trunc().clamp(1.0, MAX_MIP_COUNT as f32) as u32
    }

    /// `threshold > 1e-4 || antiflicker`.
    pub fn prefilter_enabled(&self) -> bool {
        self.threshold > 1e-4 || self.antiflicker
    }

    /// The three-component threshold knee:
    /// `(threshold - threshold*smoothness, 2*threshold*smoothness, 0.25/(threshold*smoothness))`.
    ///
    /// The third component is `0` when the product is `0` so the curve stays
    /// finite.
    pub fn threshold_knee(&self) -> (f32, f32, f32) {
        let product = self.threshold * self.smoothness;
        let scale = if product.is_finite() && product != 0.0 {
            0.25 / product
        } else {
            0.0
        };
        (self.threshold - product, 2.0 * product, scale)
    }
}

/// Whether two parameter sets need the same pipelines and mip targets.
///
/// `radius` only feeds [`Plane9BloomParams::mip_count`], and `threshold` is
/// fully described by the mip count, [`Plane9BloomParams::prefilter_enabled`]
/// and [`Plane9BloomParams::threshold_knee`]. `intensity` is handled separately
/// because it is baked into the final upsample uniform.
fn structurally_equal(a: Plane9BloomParams, b: Plane9BloomParams) -> bool {
    a.mip_count() == b.mip_count()
        && a.prefilter_enabled() == b.prefilter_enabled()
        && a.threshold_knee() == b.threshold_knee()
        && a.antiflicker() == b.antiflicker()
}

/// The uniform block all Plane9 bloom shaders share (32 bytes): two `vec4`s.
///
/// `header` and `extra` are interpreted per pass:
///
/// | pass | `header` | `extra` |
/// | --- | --- | --- |
/// | prefilter | `(inv_w, inv_h, knee_start, knee_width)` | `(knee_scale, 0, 0, 0)` |
/// | downsample/box | `(inv_w, inv_h, 0, 0)` | unused |
/// | upsample | `(inv_source_w, inv_source_h, 0, 0)` | `(scale, mode, 0, 0)` |
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Plane9Uniforms {
    header: [f32; 4],
    extra: [f32; 4],
}

/// The Plane9 bloom targets and the bind groups that feed them.
struct Plane9Targets {
    /// Prefilter output (only present when [`Plane9BloomParams::prefilter_enabled`]).
    prefilter: Option<wgpu::TextureView>,
    /// `Downsample36` output per mip.
    down: Vec<wgpu::TextureView>,
    /// Box-blur output per mip.
    blur: Vec<wgpu::TextureView>,
    /// Upsample outputs (`up[0]` is the composite handed to the composer).
    up: Vec<wgpu::TextureView>,
    /// Prefilter bind group; samples the composer's input.
    prefilter_bind: Option<wgpu::BindGroup>,
    /// Downsample bind groups; level 0 samples the input directly when the
    /// prefilter is disabled, so its entry is rebuilt by [`Plane9BloomPass::set_input`].
    down_binds: Vec<Option<wgpu::BindGroup>>,
    box_binds: Vec<wgpu::BindGroup>,
    /// Upsample bind groups in execution order (coarsest first), paired with
    /// the `up` target index they write.
    up_binds: Vec<(wgpu::BindGroup, usize)>,
}

/// The Plane9 bloom pass.
pub struct Plane9BloomPass {
    device: wgpu::Device,
    queue: wgpu::Queue,
    params: Plane9BloomParams,
    width: u32,
    height: u32,

    sampler: wgpu::Sampler,
    single_layout: wgpu::BindGroupLayout,
    dual_layout: wgpu::BindGroupLayout,

    prefilter_pipeline: wgpu::RenderPipeline,
    prefilter_median_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    box_pipeline: wgpu::RenderPipeline,
    upsample_pipeline: wgpu::RenderPipeline,

    prefilter_uniforms: wgpu::Buffer,
    downsample_uniforms: Vec<wgpu::Buffer>,
    box_uniforms: Vec<wgpu::Buffer>,
    upsample_uniforms: Vec<wgpu::Buffer>,

    targets: Option<Plane9Targets>,
}

impl Plane9BloomPass {
    /// Build the pipelines and mip targets for `width` x `height`.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        params: Plane9BloomParams,
    ) -> Self {
        let single_layout = single_texture_layout(device);
        let dual_layout = dual_texture_layout(device);

        let prefilter_shader = shader_module(
            device,
            "point10-plane9-bloom-prefilter",
            include_str!("plane9_prefilter.wgsl"),
        );
        let downsample_shader = shader_module(
            device,
            "point10-plane9-bloom-downsample",
            include_str!("plane9_downsample.wgsl"),
        );
        let box_shader = shader_module(
            device,
            "point10-plane9-bloom-box",
            include_str!("plane9_box.wgsl"),
        );
        let upsample_shader = shader_module(
            device,
            "point10-plane9-bloom-upsample",
            include_str!("plane9_upsample.wgsl"),
        );

        let prefilter_pipeline = fullscreen_pipeline(
            device,
            "point10-plane9-bloom-prefilter-pipeline",
            &prefilter_shader,
            "fs_main",
            &single_layout,
        );
        let prefilter_median_pipeline = fullscreen_pipeline(
            device,
            "point10-plane9-bloom-prefilter-median-pipeline",
            &prefilter_shader,
            "fs_median",
            &single_layout,
        );
        let downsample_pipeline = fullscreen_pipeline(
            device,
            "point10-plane9-bloom-downsample-pipeline",
            &downsample_shader,
            "fs_main",
            &single_layout,
        );
        let box_pipeline = fullscreen_pipeline(
            device,
            "point10-plane9-bloom-box-pipeline",
            &box_shader,
            "fs_main",
            &single_layout,
        );
        let upsample_pipeline = fullscreen_pipeline(
            device,
            "point10-plane9-bloom-upsample-pipeline",
            &upsample_shader,
            "fs_main",
            &dual_layout,
        );

        let prefilter_uniforms = uniform_buffer(device, "point10-plane9-bloom-prefilter-uniforms");

        let mut pass = Self {
            device: device.clone(),
            queue: queue.clone(),
            params,
            width: width.max(1),
            height: height.max(1),
            sampler: linear_sampler(device, "point10-plane9-bloom-sampler"),
            single_layout,
            dual_layout,
            prefilter_pipeline,
            prefilter_median_pipeline,
            downsample_pipeline,
            box_pipeline,
            upsample_pipeline,
            prefilter_uniforms,
            downsample_uniforms: Vec::new(),
            box_uniforms: Vec::new(),
            upsample_uniforms: Vec::new(),
            targets: None,
        };
        pass.rebuild_targets();
        pass
    }

    /// The driver parameters this pass was built with.
    pub fn params(&self) -> Plane9BloomParams {
        self.params
    }

    /// Whether switching to `params` can reuse this pass' pipelines and mip
    /// targets. A one-level chain never applies `intensity`, so only that
    /// field may change without a rebuild there.
    pub(crate) fn can_reuse(&self, params: Plane9BloomParams) -> bool {
        structurally_equal(self.params, params)
            && (self.params.mip_count() <= 1 || self.params.intensity() == params.intensity())
    }

    /// Adopt parameters approved by [`Self::can_reuse`] without rebuilding any
    /// GPU resource.
    pub(crate) fn set_params(&mut self, params: Plane9BloomParams) {
        debug_assert!(self.can_reuse(params));
        self.params = params;
    }

    /// The composite view handed to the composer's additive combine.
    pub fn composite_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|targets| &targets.up[0])
    }

    /// Resize the mip chain. The caller must call [`Plane9BloomPass::set_input`]
    /// again afterwards, because the bind group that samples the composer's
    /// input is rebuilt with the new targets.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.rebuild_targets();
    }

    /// Point the prefilter (or, with the prefilter disabled, the first
    /// downsample) at the composer's HDR input.
    pub fn set_input(&mut self, input: &wgpu::TextureView) {
        let Some(targets) = self.targets.as_mut() else {
            return;
        };
        if targets.prefilter.is_some() {
            targets.prefilter_bind = Some(single_bind(
                &self.device,
                &self.single_layout,
                &self.prefilter_uniforms,
                input,
                &self.sampler,
                "point10-plane9-bloom-prefilter-bind-group",
            ));
        } else {
            targets.down_binds[0] = Some(single_bind(
                &self.device,
                &self.single_layout,
                &self.downsample_uniforms[0],
                input,
                &self.sampler,
                "point10-plane9-bloom-down-input-bind-group",
            ));
        }
    }

    /// Record the prefilter, the per-mip downsample/box pair and the upsample
    /// walk.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(targets) = self.targets.as_ref() else {
            return;
        };

        // PASS0/PASS1: prefilter (1 tap) or prefilter+median (5 taps).
        if let Some(bind) = targets.prefilter_bind.as_ref() {
            let Some(target) = targets.prefilter.as_ref() else {
                return;
            };
            let pipeline = if self.params.antiflicker() {
                &self.prefilter_median_pipeline
            } else {
                &self.prefilter_pipeline
            };
            let mut pass = begin_pass(encoder, "point10-plane9-bloom-prefilter", target);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }

        // PASS2: Downsample36 for every mip.
        for index in 0..targets.down.len() {
            let Some(bind) = targets.down_binds[index].as_ref() else {
                return;
            };
            let mut pass = begin_pass(
                encoder,
                "point10-plane9-bloom-downsample",
                &targets.down[index],
            );
            pass.set_pipeline(&self.downsample_pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }

        // PASS3: box blur for every mip.
        for index in 0..targets.blur.len() {
            let mut pass = begin_pass(encoder, "point10-plane9-bloom-box", &targets.blur[index]);
            pass.set_pipeline(&self.box_pipeline);
            pass.set_bind_group(0, &targets.box_binds[index], &[]);
            pass.draw(0..3, 0..1);
        }

        // PASS4: UpsampleCircular. `up_binds[0]` is the coarsest level's
        // pass, the last entry is level 0 (the one carrying `intensity`).
        for (bind, level) in &targets.up_binds {
            let mut pass = begin_pass(
                encoder,
                "point10-plane9-bloom-upsample",
                &targets.up[*level],
            );
            pass.set_pipeline(&self.upsample_pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Re-create the mip targets, uniforms and bind groups for the current
    /// size and parameters.
    fn rebuild_targets(&mut self) {
        let device = &self.device;
        let queue = &self.queue;
        let levels = self.params.mip_count().max(1);

        // Half-res first mip, then halve with rounding (clamped to 1px).
        let mut sizes = Vec::with_capacity(levels as usize);
        let mut size = (half(self.width), half(self.height));
        for _ in 0..levels {
            sizes.push(size);
            size = (half(size.0), half(size.1));
        }

        let (knee_start, knee_width, knee_scale) = self.params.threshold_knee();
        let prefilter_enabled = self.params.prefilter_enabled();

        let prefilter = if prefilter_enabled {
            Some(
                create_target(
                    device,
                    "point10-plane9-bloom-prefilter",
                    sizes[0].0,
                    sizes[0].1,
                )
                .1,
            )
        } else {
            None
        };

        queue.write_buffer(
            &self.prefilter_uniforms,
            0,
            bytemuck::bytes_of(&Plane9Uniforms {
                header: [
                    1.0 / sizes[0].0 as f32,
                    1.0 / sizes[0].1 as f32,
                    knee_start,
                    knee_width,
                ],
                extra: [knee_scale, 0.0, 0.0, 0.0],
            }),
        );

        let mut down = Vec::with_capacity(levels as usize);
        let mut blur = Vec::with_capacity(levels as usize);
        self.downsample_uniforms = Vec::with_capacity(levels as usize);
        self.box_uniforms = Vec::with_capacity(levels as usize);
        for (width, height) in sizes.iter().copied() {
            let (_, down_view) =
                create_target(device, "point10-plane9-bloom-downsample", width, height);
            let (_, blur_view) = create_target(device, "point10-plane9-bloom-box", width, height);
            down.push(down_view);
            blur.push(blur_view);

            let uniforms = Plane9Uniforms {
                header: [1.0 / width as f32, 1.0 / height as f32, 0.0, 0.0],
                extra: [0.0; 4],
            };
            let buffer = uniform_buffer(device, "point10-plane9-bloom-downsample-uniforms");
            queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&uniforms));
            self.downsample_uniforms.push(buffer);
            let buffer = uniform_buffer(device, "point10-plane9-bloom-box-uniforms");
            queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&uniforms));
            self.box_uniforms.push(buffer);
        }

        // With one mip the upsample loop never runs: `up[0]` is a plain copy
        // of the box result (mode 0), so no intensity is ever applied.
        let up_count = if levels == 1 { 1 } else { levels as usize - 1 };
        let mut up = Vec::with_capacity(up_count);
        for (width, height) in sizes.iter().take(up_count).copied() {
            up.push(create_target(device, "point10-plane9-bloom-upsample", width, height).1);
        }
        self.upsample_uniforms = (0..up_count)
            .map(|_| uniform_buffer(device, "point10-plane9-bloom-upsample-uniforms"))
            .collect();

        let mut down_binds = Vec::with_capacity(levels as usize);
        for index in 0..levels as usize {
            let source = if index == 0 {
                prefilter.as_ref()
            } else {
                Some(&blur[index - 1])
            };
            down_binds.push(source.map(|source| {
                single_bind(
                    device,
                    &self.single_layout,
                    &self.downsample_uniforms[index],
                    source,
                    &self.sampler,
                    "point10-plane9-bloom-downsample-bind-group",
                )
            }));
        }

        let mut box_binds = Vec::with_capacity(levels as usize);
        for index in 0..levels as usize {
            box_binds.push(single_bind(
                device,
                &self.single_layout,
                &self.box_uniforms[index],
                &down[index],
                &self.sampler,
                "point10-plane9-bloom-box-bind-group",
            ));
        }

        let mut up_binds = Vec::with_capacity(up_count);
        if levels == 1 {
            queue.write_buffer(
                &self.upsample_uniforms[0],
                0,
                bytemuck::bytes_of(&Plane9Uniforms {
                    header: [1.0 / sizes[0].0 as f32, 1.0 / sizes[0].1 as f32, 0.0, 0.0],
                    // mode 0: copy the box result, no gScale.
                    extra: [1.0, 0.0, 0.0, 0.0],
                }),
            );
            up_binds.push((
                dual_bind(
                    device,
                    &self.dual_layout,
                    &self.upsample_uniforms[0],
                    &blur[0],
                    &blur[0],
                    &self.sampler,
                    "point10-plane9-bloom-finalize-bind-group",
                ),
                0,
            ));
        } else {
            for level in (0..levels as usize - 1).rev() {
                let index = up_binds.len();
                let source_size = sizes[level + 1];
                let scale = if level == 0 {
                    self.params.intensity()
                } else {
                    1.0
                };
                queue.write_buffer(
                    &self.upsample_uniforms[index],
                    0,
                    bytemuck::bytes_of(&Plane9Uniforms {
                        header: [
                            1.0 / source_size.0 as f32,
                            1.0 / source_size.1 as f32,
                            0.0,
                            0.0,
                        ],
                        extra: [scale, 1.0, 0.0, 0.0],
                    }),
                );
                let coarser = if level + 1 == levels as usize - 1 {
                    &blur[levels as usize - 1]
                } else {
                    &up[level + 1]
                };
                up_binds.push((
                    dual_bind(
                        device,
                        &self.dual_layout,
                        &self.upsample_uniforms[index],
                        &blur[level],
                        coarser,
                        &self.sampler,
                        "point10-plane9-bloom-upsample-bind-group",
                    ),
                    level,
                ));
            }
        }

        self.targets = Some(Plane9Targets {
            prefilter,
            down,
            blur,
            up,
            prefilter_bind: None,
            down_binds,
            box_binds,
            up_binds,
        });
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn shader_module(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(format!("{FULLSCREEN_TRIANGLE_WGSL}\n{source}").into()),
    })
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
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

fn single_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("point10-plane9-bloom-single-layout"),
        entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2)],
    })
}

fn dual_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("point10-plane9-bloom-dual-layout"),
        entries: &[
            uniform_entry(0),
            texture_entry(1),
            texture_entry(2),
            sampler_entry(3),
        ],
    })
}

fn uniform_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<Plane9Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn single_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    source: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
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

#[allow(clippy::too_many_arguments)]
fn dual_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    base: &wgpu::TextureView,
    blur: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(base),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(blur),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    entry_point: &str,
    layout: &wgpu::BindGroupLayout,
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
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry_point),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: crate::point10::HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &'static str,
    target: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_changes_that_reuse_the_pass() {
        let base = Plane9BloomParams::new(6.0, 0.0, 0.5, 0.51, false);
        // The Three.js ports only animate intensity (and a radius that still
        // resolves to one mip): neither changes the target structure.
        assert!(structurally_equal(
            base,
            Plane9BloomParams::new(6.0, 0.0, 0.5, 2.0, false)
        ));
        assert!(structurally_equal(
            base,
            Plane9BloomParams::new(7.0, 0.0, 0.5, 0.51, false)
        ));
        // Threshold, knee and antiflicker change the pass chain itself.
        assert!(!structurally_equal(
            base,
            Plane9BloomParams::new(6.0, 0.1, 0.5, 0.51, false)
        ));
        assert!(!structurally_equal(
            base,
            Plane9BloomParams::new(40.0, 1.0, 0.5, 0.51, false)
        ));
        assert!(!structurally_equal(
            base,
            Plane9BloomParams::new(6.0, 0.0, 0.5, 0.51, true)
        ));
        let knee = Plane9BloomParams::new(6.0, 1.0, 0.5, 0.51, false);
        assert!(!structurally_equal(
            knee,
            Plane9BloomParams::new(6.0, 1.0, 0.2, 0.51, false)
        ));
    }

    #[test]
    fn wgsl_parses_and_validates() {
        for source in [
            include_str!("plane9_prefilter.wgsl"),
            include_str!("plane9_downsample.wgsl"),
            include_str!("plane9_box.wgsl"),
            include_str!("plane9_upsample.wgsl"),
        ] {
            let full = format!("{}\n{source}", crate::point10::FULLSCREEN_TRIANGLE_WGSL);
            let module = match naga::front::wgsl::parse_str(&full) {
                Ok(module) => module,
                Err(error) => panic!("Plane9 bloom WGSL failed to parse: {error}\n{source}"),
            };
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Plane9 bloom WGSL must validate");
        }
    }
}
