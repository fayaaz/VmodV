//! GPU tests for the post-processing chain.
//!
//! Rendered on the fallback (software / lavapipe) adapter and read back, in the
//! style of `crates/modv-render/src/graph_tests.rs`. Every test is skipped with
//! a log line when there is no adapter.

use crate::point10::FULLSCREEN_TRIANGLE_WGSL;
use crate::point10::test_support::{TestTarget, any_lit, any_visible, gpu};

use super::Composer;
use super::plane9_bloom::Plane9BloomParams;

const SIZE: u32 = 64;

/// A minimal stand-in for a scene: fills the target with one colour, optionally
/// limited to a scissored patch.
struct SolidScene {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    queue: wgpu::Queue,
    colour: [f32; 4],
    patch: Option<(u32, u32, u32, u32)>,
}

impl SolidScene {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        colour: [f32; 4],
        patch: Option<(u32, u32, u32, u32)>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point10-test-solid-scene"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{FULLSCREEN_TRIANGLE_WGSL}\n\
                     struct Solid {{ colour: vec4<f32> }}\n\
                     @group(0) @binding(0) var<uniform> solid: Solid;\n\
                     @fragment fn fs_main() -> @location(0) vec4<f32> {{ return solid.colour; }}\n"
                )
                .into(),
            ),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point10-test-solid-scene-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-test-solid-scene-uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&colour));

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("point10-test-solid-scene-bind-group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point10-test-solid-scene-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point10-test-solid-scene-pipeline"),
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
            bind_group,
            uniform_buffer,
            queue: queue.clone(),
            colour,
            patch,
        }
    }

    fn render(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        // Keep the buffer referenced for clarity (values never change).
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&self.colour));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("point10-test-solid-scene"),
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        if let Some((x, y, width, height)) = self.patch {
            pass.set_scissor_rect(x, y, width, height);
        }
        pass.draw(0..3, 0..1);
    }
}

/// Render one frame through the composer and read the destination back.
fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    composer: &mut Composer,
    scene: &SolidScene,
) -> TestTarget {
    let target = TestTarget::new(device, SIZE, SIZE);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("point10-postfx-test"),
    });
    scene.render(&mut encoder, composer.scene_view());
    composer.render(&mut encoder, &target.view);
    queue.submit(Some(encoder.finish()));
    target
}

#[test]
fn composer_renders_a_solid_colour_scene() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    let mut composer = Composer::new(&device, &queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    // Linear red; the composer's output pass applies the sRGB transfer for a
    // non-sRGB destination, so the pixel reads back as (255, 0, 0, 255).
    let scene = SolidScene::new(
        &device,
        &queue,
        Composer::scene_format(),
        [1.0, 0.0, 0.0, 1.0],
        None,
    );

    let target = render_frame(&device, &queue, &mut composer, &scene);
    let pixels = target.read(&device, &queue);
    assert!(any_lit(&pixels), "the composer produced an empty frame");

    let centre = target.pixel(&device, &queue, SIZE / 2, SIZE / 2);
    assert!(
        centre[0] > 240 && centre[1] < 8 && centre[2] < 8 && centre[3] > 240,
        "expected opaque red, got {centre:?}"
    );
}

#[test]
fn composer_preserves_transparency() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    let mut composer = Composer::new(&device, &queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    // Point10 scenes clear to transparent black; the composer must not invent
    // an opaque background (the scene composites over the layers below).
    let scene = SolidScene::new(
        &device,
        &queue,
        Composer::scene_format(),
        [0.0, 0.0, 0.0, 0.0],
        None,
    );
    let target = render_frame(&device, &queue, &mut composer, &scene);
    let pixels = target.read(&device, &queue);
    assert!(!any_visible(&pixels), "expected a fully transparent frame");
}

#[test]
fn bloom_spreads_energy_to_neighbours() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    // A 4x4 white patch at the centre; everything else is transparent black.
    let patch = (30, 30, 4, 4);
    let scene = SolidScene::new(
        &device,
        &queue,
        Composer::scene_format(),
        [1.0, 1.0, 1.0, 1.0],
        Some(patch),
    );

    let neighbour = (SIZE / 2 + 6, SIZE / 2);

    let mut plain = Composer::new(&device, &queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    let without = render_frame(&device, &queue, &mut plain, &scene);
    let without_pixel = without.pixel(&device, &queue, neighbour.0, neighbour.1);

    let mut bloomed = Composer::new(&device, &queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    bloomed.enable_bloom(1.0, 0.5, 0.0);
    let with = render_frame(&device, &queue, &mut bloomed, &scene);
    let with_pixel = with.pixel(&device, &queue, neighbour.0, neighbour.1);

    assert!(
        without_pixel[3] == 0 || without_pixel[0] == 0,
        "the un-bloomed neighbour should be empty, got {without_pixel:?}"
    );
    assert!(
        with_pixel[0] > 8,
        "bloom should light the neighbour: {with_pixel:?} vs {without_pixel:?}"
    );
    // The patch itself must stay bright.
    let centre = with.pixel(&device, &queue, SIZE / 2, SIZE / 2);
    assert!(
        centre[0] > 240,
        "the bloom must not darken the source: {centre:?}"
    );
}

/// `preserveBloomAlpha` (Boxy.js:73-86, used by Wormhole): the bloom still
/// lights the neighbours, but it must not add into the frame's alpha.
#[test]
fn preserve_bloom_alpha_keeps_the_destination_alpha() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    // A 4x4 white patch with a transparent frame: the same setup as
    // `bloom_spreads_energy_to_neighbours`.
    let patch = (30, 30, 4, 4);
    let scene = SolidScene::new(
        &device,
        &queue,
        Composer::scene_format(),
        [1.0, 1.0, 1.0, 1.0],
        Some(patch),
    );
    let neighbour = (SIZE / 2 + 6, SIZE / 2);

    let mut additive = Composer::new(&device, &queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    additive.enable_bloom(1.0, 0.5, 0.0);
    let additive_frame = render_frame(&device, &queue, &mut additive, &scene);
    let additive_pixel = additive_frame.pixel(&device, &queue, neighbour.0, neighbour.1);

    let mut preserved = Composer::new(&device, &queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    preserved.enable_bloom(1.0, 0.5, 0.0);
    preserved.set_preserve_bloom_alpha(true);
    assert!(preserved.preserve_bloom_alpha());
    let preserved_frame = render_frame(&device, &queue, &mut preserved, &scene);
    let preserved_pixel = preserved_frame.pixel(&device, &queue, neighbour.0, neighbour.1);

    // Both add the bloom colour...
    assert!(
        additive_pixel[0] > 8 && preserved_pixel[0] > 8,
        "bloom should light the neighbour in both modes: {additive_pixel:?} / {preserved_pixel:?}"
    );
    // ...but only the default mode adds it into the alpha.
    assert!(
        additive_pixel[3] > 0,
        "the additive combine is expected to raise alpha: {additive_pixel:?}"
    );
    assert_eq!(
        preserved_pixel[3], 0,
        "preserveBloomAlpha must keep the transparent background: {preserved_pixel:?}"
    );
}

#[test]
fn composer_resize_rebuilds_the_chain() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    let mut composer = Composer::new(&device, &queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    composer.enable_bloom(1.0, 0.5, 0.2);
    composer.resize(SIZE * 2, SIZE * 2);
    assert_eq!(composer.size(), (SIZE * 2, SIZE * 2));

    let scene = SolidScene::new(
        &device,
        &queue,
        Composer::scene_format(),
        [0.2, 0.4, 0.6, 1.0],
        None,
    );
    let target = TestTarget::new(&device, SIZE * 2, SIZE * 2);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("point10-postfx-resize-test"),
    });
    scene.render(&mut encoder, composer.scene_view());
    composer.render(&mut encoder, &target.view);
    queue.submit(Some(encoder.finish()));

    let pixel = target.pixel(&device, &queue, SIZE, SIZE);
    assert!(
        pixel[3] > 240,
        "resized composer lost its output: {pixel:?}"
    );
    assert!(
        pixel[0] > 60 && pixel[1] > 90 && pixel[2] > 130,
        "expected a bright blue-grey, got {pixel:?}"
    );
}

/// RED-phase oracle for the driver's prefilter gate.
fn expected_plane9_prefilter(threshold: f32, antiflicker: bool) -> bool {
    threshold > 1e-4 || antiflicker
}

/// RED-phase oracle for the driver's threshold-knee curve. The zero-product
/// branch deliberately avoids evaluating `0.25 / 0.0`.
fn expected_plane9_knee(threshold: f32, smoothness: f32) -> (f32, f32, f32) {
    let product = threshold * smoothness;
    (
        threshold - product,
        2.0 * product,
        if product == 0.0 { 0.0 } else { 0.25 / product },
    )
}

/// `(differing pixel count, first difference, largest channel delta)`.
type PixelDifferences = (usize, Option<(usize, [u8; 4], [u8; 4])>, u8);

fn pixel_differences(left: &[u8], right: &[u8]) -> PixelDifferences {
    if left.len() != right.len() {
        return (usize::MAX, None, u8::MAX);
    }
    let mut count = 0;
    let mut first = None;
    let mut max_delta = 0;
    for (index, (left, right)) in left
        .as_chunks::<4>()
        .0
        .iter()
        .zip(right.as_chunks::<4>().0.iter())
        .enumerate()
    {
        max_delta = max_delta.max(
            left.iter()
                .zip(right.iter())
                .map(|(left, right)| left.abs_diff(*right))
                .max()
                .unwrap_or(0),
        );
        if left != right {
            count += 1;
            if first.is_none() {
                first = Some((
                    index,
                    [left[0], left[1], left[2], left[3]],
                    [right[0], right[1], right[2], right[3]],
                ));
            }
        }
    }
    (count, first, max_delta)
}

fn max_rgb_channel(pixels: &[u8]) -> u8 {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| &pixel[..3])
        .copied()
        .max()
        .unwrap_or(0)
}

fn pixel_at(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * width + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

fn has_halo_outside(pixels: &[u8], width: u32, patch: (u32, u32, u32, u32)) -> bool {
    let (patch_x, patch_y, patch_width, patch_height) = patch;
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .any(|(index, pixel)| {
            let x = index as u32 % width;
            let y = index as u32 / width;
            let in_patch = x >= patch_x
                && x < patch_x + patch_width
                && y >= patch_y
                && y < patch_y + patch_height;
            !in_patch
                && pixel[..3]
                    .iter()
                    .any(|&channel| channel > 0 && channel < 255)
        })
}

/// RED-phase adapter: the composer's Plane9 bloom entry point shares the
/// additive combine with the Unreal pass but keeps the driver's own
/// parameters.
fn render_bloom_candidate(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &SolidScene,
    radius: f32,
    threshold: f32,
    smoothness: f32,
    intensity: f32,
) -> Vec<u8> {
    let mut composer = Composer::new(device, queue, SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm);
    composer.enable_plane9_bloom(Plane9BloomParams::new(
        radius, threshold, smoothness, intensity, false,
    ));
    let target = render_frame(device, queue, &mut composer, scene);
    target.read(device, queue)
}

#[test]
fn plane9_bloom_matches_the_disassembled_driver() {
    let mut failures = Vec::new();

    // A. The driver derives the mip count from radius and threshold
    // (`0x1005cc60`); a non-positive threshold clamps to a single mip.
    for (radius, threshold, expected) in [
        (6.0, 0.0, 1),
        (5.0, 0.0, 1),
        (5.0, 1.0, 1),
        (6.0, 0.3, 1),
        (7.0, 10.0, 2),
        (8.0, 1.0, 1),
        (9.0, 1.0, 1),
        (9.0, 3.0, 2),
        (20.0, 1.0, 12),
        (40.0, 1000.0, 16),
    ] {
        let actual = Plane9BloomParams::new(radius, threshold, 0.5, 1.0, false).mip_count();
        if actual != expected {
            failures.push(format!(
                "mip_count({radius}, {threshold}) = {actual}, expected {expected}"
            ));
        }
    }

    // B. The prefilter runs when the threshold is above 1e-4, or when
    // antiflicker is set.
    for (threshold, antiflicker) in [
        (0.0, false),
        (1e-5, false),
        (1e-4, false),
        (1e-3, false),
        (0.0, true),
    ] {
        let actual =
            Plane9BloomParams::new(6.0, threshold, 0.5, 1.0, antiflicker).prefilter_enabled();
        let expected = expected_plane9_prefilter(threshold, antiflicker);
        if actual != expected {
            failures.push(format!(
                "prefilter({threshold}, {antiflicker}) = {actual}, expected {expected}"
            ));
        }
    }

    // C. The knee curve is (threshold - threshold*smoothness,
    // 2*threshold*smoothness, 0.25/(threshold*smoothness)).
    for (threshold, smoothness) in [(1.0, 0.5), (0.0, 0.5), (1.0, 0.0), (0.0, 0.0)] {
        let expected = expected_plane9_knee(threshold, smoothness);
        let actual =
            Plane9BloomParams::new(6.0, threshold, smoothness, 1.0, false).threshold_knee();
        if !actual.2.is_finite() || !expected.2.is_finite() {
            failures.push(format!(
                "threshold_knee({threshold}, {smoothness}) produced a non-finite curve: {actual:?}"
            ));
        }
        if (threshold, smoothness) == (1.0, 0.5) && actual != expected {
            failures.push(format!(
                "threshold_knee({threshold}, {smoothness}) = {actual:?}, expected {expected:?}"
            ));
        }
    }

    let Some((device, queue)) = gpu() else {
        assert!(
            failures.is_empty(),
            "Plane9 bloom pure-logic mismatches (GPU checks skipped):\n{}",
            failures.join("\n")
        );
        return;
    };

    // D + F. Radius 6 / Threshold 0 resolves to one level. Intensity must be
    // inert there, while the shipped Hollow Cube-like settings still produce
    // a visible, non-uniform halo. The dim source keeps clipping out of the
    // comparison.
    let patch = (30, 30, 4, 4);
    let scene = SolidScene::new(
        &device,
        &queue,
        Composer::scene_format(),
        [0.1, 0.1, 0.1, 1.0],
        Some(patch),
    );
    let low_intensity = render_bloom_candidate(&device, &queue, &scene, 6.0, 0.0, 0.5, 0.1);
    let high_intensity = render_bloom_candidate(&device, &queue, &scene, 6.0, 0.0, 0.5, 10.0);
    let (different, first, max_delta) = pixel_differences(&low_intensity, &high_intensity);
    if different != 0 {
        failures.push(format!(
            "one-level intensity was not inert: {different} pixels differ; max channel delta {max_delta}; max RGB low/high {}/{}; first {first:?}",
            max_rgb_channel(&low_intensity),
            max_rgb_channel(&high_intensity),
        ));
    }

    let halo = render_bloom_candidate(&device, &queue, &scene, 6.0, 0.0, 0.5, 0.51);
    if !any_lit(&halo) {
        failures.push("one-level bloom produced an empty frame".to_owned());
    }
    if halo
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[..] == halo[..4])
    {
        failures.push("one-level bloom produced a uniform frame".to_owned());
    }
    if !has_halo_outside(&halo, SIZE, patch) {
        failures.push(
            "one-level bloom did not spread a non-saturated halo outside the source patch"
                .to_owned(),
        );
    }
    let halo_max_rgb = max_rgb_channel(&halo);
    if halo_max_rgb == 255 {
        failures.push("one-level halo frame saturated to 255".to_owned());
    }

    // E. Radius 40 / Threshold 0.1 still resolves to a deep 16-level chain.
    // A dim source exactly at the threshold exposes today's fixed 0.01 knee:
    // UnrealBloomPass discards it, so the two intensities incorrectly remain
    // identical without any channel clipping.
    let deep_patch = (28, 28, 8, 8);
    let deep_scene = SolidScene::new(
        &device,
        &queue,
        Composer::scene_format(),
        [0.1, 0.1, 0.1, 1.0],
        Some(deep_patch),
    );
    let deep_low = render_bloom_candidate(&device, &queue, &deep_scene, 40.0, 0.1, 0.5, 0.1);
    let deep_high = render_bloom_candidate(&device, &queue, &deep_scene, 40.0, 0.1, 0.5, 10.0);
    let (deep_different, deep_first, deep_max_delta) = pixel_differences(&deep_low, &deep_high);
    if deep_different <= 32 {
        failures.push(format!(
            "deep-chain intensity did not produce a substantive difference: {deep_different} pixels differ (need > 32); max channel delta {deep_max_delta}; max RGB low/high {}/{}; centre low {:?}, high {:?}; first difference {deep_first:?}",
            max_rgb_channel(&deep_low),
            max_rgb_channel(&deep_high),
            pixel_at(&deep_low, SIZE, SIZE / 2, SIZE / 2),
            pixel_at(&deep_high, SIZE, SIZE / 2, SIZE / 2),
        ));
    }

    assert!(
        failures.is_empty(),
        "Plane9 bloom mismatches:\n{}",
        failures.join("\n")
    );
}
