//! GPU-executed smoke tests for the built-in shader registry.
//!
//! `shader::modules::tests` validates that every module's WGSL parses and
//! that its metadata matches the JS. This module goes one step further: it
//! renders every registered module on the shared adapter with a deterministic
//! group texture, reads back a float framebuffer and asserts behavioural
//! invariants (finite channels, opaque alpha on the mainImage wrapper path).
//! A driver-level regression in the emitted shader or wrapper therefore fails
//! here instead of only in the app.

#![cfg(test)]

use crate::shader::modules::ShaderModule;
use crate::shader::pipeline::{ShaderAudioUniforms, ShaderRenderer};
use crate::shader::uniforms::PropSlots;

/// Small enough for lavapipe, large enough that the full-screen triangle and
/// texture sampling are exercised.
const FRAME_SIZE: u32 = 32;

/// A float target so NaN/Inf in the shader math is visible to the assertion
/// (an RGBA8 target would quietly clamp it away).
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// The frame inputs a behavioural test varies while the group texture and
/// prop defaults stay fixed.
pub(crate) struct RenderInputs {
    pub time_seconds: f32,
    pub audio: ShaderAudioUniforms,
}

impl Default for RenderInputs {
    fn default() -> Self {
        Self {
            time_seconds: 12.5,
            audio: ShaderAudioUniforms {
                kick: false,
                bass: 0.4,
                mid: 0.3,
                treble: 0.2,
            },
        }
    }
}

/// Render one built-in module to an RGBA32F framebuffer and return the pixels
/// as `r, g, b, a` floats in row-major order.
pub(crate) fn render_module(module: &ShaderModule, inputs: RenderInputs) -> Option<Vec<f32>> {
    let (device, queue) = crate::test_gpu::gpu()?;
    let size = wgpu::Extent3d {
        width: FRAME_SIZE,
        height: FRAME_SIZE,
        depth_or_array_layers: 1,
    };

    // Deterministic group texture: a gradient covering the whole domain, so
    // UV-dependent modules do not all sample one flat colour.
    let mut pixels = Vec::with_capacity((FRAME_SIZE * FRAME_SIZE * 4) as usize);
    for y in 0..FRAME_SIZE {
        for x in 0..FRAME_SIZE {
            pixels.extend_from_slice(&[
                ((x * 255) / FRAME_SIZE) as u8,
                ((y * 255) / FRAME_SIZE) as u8,
                128,
                255,
            ]);
        }
    }
    let group = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modV shader test group"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &group,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(FRAME_SIZE * 4),
            rows_per_image: Some(FRAME_SIZE),
        },
        size,
    );
    let group_view = group.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("modV shader test sampler"),
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
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modV shader test target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let mut renderer = ShaderRenderer::new(device.clone(), queue.clone(), TARGET_FORMAT);
    let slot = renderer
        .setup_module(module, TARGET_FORMAT, false)
        .expect("built-in module must build a pipeline");
    let params = PropSlots {
        floats: module
            .props
            .iter()
            .map(|prop| prop.default as f32)
            .collect(),
    };
    renderer
        .render(
            slot,
            &group_view,
            &sampler,
            None,
            None,
            inputs.audio,
            inputs.time_seconds,
            750,
            [2026.0, 11.0, 19.0, 43_200.0],
            [FRAME_SIZE as f32, FRAME_SIZE as f32, 1.0],
            1.0,
            &params,
            &target_view,
        )
        .expect("built-in module must render");

    let bytes = crate::isf::runtime::read_texture(&device, &queue, &target)
        .expect("float framebuffer reads back");
    Some(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many framebuffer channels changed by a visible amount between two
    /// renders. A handful would be dithering; a full row means the shader
    /// actually repainted for the new input.
    fn changed_channels(left: &[f32], right: &[f32]) -> usize {
        assert_eq!(left.len(), right.len(), "frames must have the same size");
        left.iter()
            .zip(right)
            .filter(|(a, b)| (*a - *b).abs() > 1e-4)
            .count()
    }

    #[test]
    fn builtin_modules_respond_to_time_and_audio() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping shader behaviour test: no GPU adapter in this environment");
            return;
        }
        let modules = crate::shader::all_modules();
        let find = |name: &str| {
            modules
                .iter()
                .find(|module| module.name.as_ref() == name)
                .unwrap_or_else(|| panic!("registry is missing {name}"))
        };

        // Plasma's four sine fields are driven by u_time (milliseconds,
        // divided by the default u_timeScale prop of 100): a different clock
        // must repaint the whole frame, with audio held constant.
        let plasma = find("Plasma");
        let plasma_early = render_module(
            plasma,
            RenderInputs {
                time_seconds: 12.5,
                ..RenderInputs::default()
            },
        )
        .expect("the shared test adapter is available");
        let plasma_late = render_module(
            plasma,
            RenderInputs {
                time_seconds: 87.0,
                ..RenderInputs::default()
            },
        )
        .expect("the shared test adapter is available");

        // Bands draws three rings at radii u_bass/u_mid/u_treble, so changing
        // only the audio bands at a fixed clock must move them.
        let bands = find("Bands");
        let quiet = ShaderAudioUniforms {
            kick: false,
            bass: 0.05,
            mid: 0.1,
            treble: 0.2,
        };
        let loud = ShaderAudioUniforms {
            kick: true,
            bass: 0.85,
            mid: 0.65,
            treble: 0.45,
        };
        let bands_quiet = render_module(
            bands,
            RenderInputs {
                time_seconds: 12.5,
                audio: quiet,
            },
        )
        .expect("the shared test adapter is available");
        let bands_loud = render_module(
            bands,
            RenderInputs {
                time_seconds: 12.5,
                audio: loud,
            },
        )
        .expect("the shared test adapter is available");

        for (label, frame) in [
            ("Plasma at 12.5s", &plasma_early),
            ("Plasma at 87s", &plasma_late),
            ("Bands with quiet audio", &bands_quiet),
            ("Bands with loud audio", &bands_loud),
        ] {
            assert_eq!(
                frame.len(),
                (FRAME_SIZE * FRAME_SIZE * 4) as usize,
                "{label}: wrong framebuffer size"
            );
            assert!(
                frame.iter().all(|channel| channel.is_finite()),
                "{label}: non-finite channel in the rendered frame"
            );
        }
        assert!(
            changed_channels(&plasma_early, &plasma_late) > FRAME_SIZE as usize,
            "Plasma ignored the changed time input"
        );
        assert!(
            changed_channels(&bands_quiet, &bands_loud) > FRAME_SIZE as usize,
            "Bands ignored the changed audio-band inputs"
        );
    }

    #[test]
    fn all_builtin_modules_render_finite_opaque_frames() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping shader render test: no GPU adapter in this environment");
            return;
        }
        let modules = crate::shader::all_modules();
        assert_eq!(
            modules.len(),
            33,
            "the registry size this test sweeps changed"
        );
        let direct = ["Bands", "Fisheye", "MattiasCRT", "Plasma", "Wobble"];
        for module in modules {
            let frame = render_module(&module, RenderInputs::default())
                .expect("the shared test adapter is available");
            assert_eq!(
                frame.len(),
                (FRAME_SIZE * FRAME_SIZE * 4) as usize,
                "{}: wrong framebuffer size",
                module.name
            );
            for (index, channel) in frame.iter().enumerate() {
                assert!(
                    channel.is_finite(),
                    "{}: non-finite channel {index} in the rendered frame",
                    module.name
                );
            }
            if !direct.contains(&module.name.as_ref()) {
                for (pixel, rgba) in frame.as_chunks::<4>().0.iter().enumerate() {
                    assert_eq!(
                        rgba[3], 1.0,
                        "{}: mainImage wrapper did not force opaque alpha at pixel {pixel}",
                        module.name
                    );
                }
            }
        }
    }
}
