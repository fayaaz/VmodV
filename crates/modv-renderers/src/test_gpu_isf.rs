//! GPU-executed smoke coverage for the vendored ISF corpus.
//!
//! The translation baseline pins which corpus shaders compile to WGSL. This
//! test goes one step further: every shader in that baseline is actually
//! compiled into a wgpu pipeline, drawn for one fixed frame on the shared
//! software adapter and read back as float pixels. A translator change that
//! keeps the text checks happy but produces NaN/Inf or a driver reject fails
//! here instead.
//!
//! A handful of upstream shaders write a non-finite channel *by construction*
//! at this fixed frame (their own GLSL takes sqrt/pow of a negative value,
//! divides by zero, or overflows `exp`). They are enumerated in
//! [`EXPECTED_NON_FINITE`] with the reason in each case. Those shaders are
//! still compiled, drawn and read back — only the pixel-finiteness assertion
//! is waived for them. Every other corpus shader is checked strictly, so a
//! translator regression that starts producing NaN in a previously finite
//! shader still fails this test.

#![cfg(test)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::isf::{IsfFrameUniforms, IsfRuntime, IsfValues, parse_isf};

/// Fixed frame — the same clock as the golden tests, so a shader's behaviour
/// is comparable between the two suites.
const FRAME: IsfFrameUniforms = IsfFrameUniforms {
    time: 12.5,
    time_delta: 1.0 / 60.0,
    frame_index: 750,
    date: [2026.0, 11.0, 19.0, 43_200.0],
};

const FRAME_SIZE: u32 = 16;
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// Corpus shaders whose upstream GLSL provably emits at least one non-finite
/// channel at the fixed frame and declared-default inputs used by this test.
///
/// The runtime supplies every declared default (an empty `IsfValues` falls
/// back to the shader's own defaults) and an opaque gradient texture for every
/// image input, so these are *not* missing-input artefacts. Nor are they
/// translation bugs: the listed expressions are undefined or overflow in GLSL
/// too, and each shader is translated unchanged (no WGSL override). The list
/// is deliberately small and name-specific — do not use it to blanket-skip
/// shaders with texture inputs.
const EXPECTED_NON_FINITE: &[(&str, &str)] = &[
    (
        "Boxinator",
        "default gamma = -0.3: sqrt(max(vec4(...), 0.0) + gamma) takes the square root of a negative RGB lane",
    ),
    (
        "Cant Find My Way Out",
        "default uSat = 1.54 oversaturates col before pow(col, 0.4545), leaving negative colour lanes",
    ),
    (
        "Dirty Lens",
        "fbm value noise yields negative f; pow(f, 5.0 - brightnessCurve) has a negative base (WGSL pow of a negative is NaN)",
    ),
    (
        "FractilianParabolicCircleInversion",
        "Parabola() takes pow of 4*m*(1-m), which is negative once m > 1; CircleInversion also divides by dot(vOP, vOP)",
    ),
    (
        "Smudged Lens",
        "fbm value noise yields negative f; pow(f, 5.0 - brightnessCurve = 1.0) has a negative base",
    ),
    (
        "diatribes -water cavern mod",
        "tanh is (exp(x)-exp(-x))/(exp(x)+exp(-x)) and default intensity = 300 overflows exp to Inf, giving Inf/Inf",
    ),
    (
        "evvvvil frankenslug",
        "the blue base colour (0.1) minus radial/vertical darkening goes negative before pow(co, 0.65); pow(1+dot(no,rd), 4.0) can also take a negative base",
    ),
    (
        "neon psychedelic lydianlights mod",
        "d = 0.04/d with d = 0.5*sin(...) crosses zero (Inf/NaN), and gamma() applies pow to possibly negative colour lanes",
    ),
];

/// Index of the first non-finite channel, or `None` when every channel is
/// finite. Kept as a helper so the finiteness contract can be unit-tested
/// independently of the GPU.
fn first_non_finite(frame: &[f32]) -> Option<usize> {
    frame.iter().position(|channel| !channel.is_finite())
}

/// Render one corpus shader by file stem to an RGBA32F framebuffer and return
/// its pixels as `r, g, b, a` floats in row-major order.
pub(crate) fn render_corpus_shader(stem: &str) -> Result<Vec<f32>, String> {
    let (device, queue) = crate::test_gpu::gpu().ok_or("no GPU adapter in this environment")?;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/isf")
        .join(format!("{stem}.fs"));
    let source =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let document = parse_isf(&source, None).map_err(|error| error.to_string())?;

    // Give every image/audio input a deterministic, *opaque* gradient. The
    // runtime binds a transparent 1x1 dummy when a name is missing, which
    // makes shaders that divide by luminance produce NaN — a harness artefact,
    // not a shader regression.
    let mut input_pixels = Vec::with_capacity((FRAME_SIZE * FRAME_SIZE * 4) as usize);
    for y in 0..FRAME_SIZE {
        for x in 0..FRAME_SIZE {
            input_pixels.extend_from_slice(&[
                ((x * 17 + 32) & 0xff) as u8,
                ((y * 17 + 64) & 0xff) as u8,
                160,
                255,
            ]);
        }
    }
    let input = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modV ISF corpus smoke input"),
        size: wgpu::Extent3d {
            width: FRAME_SIZE,
            height: FRAME_SIZE,
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
            texture: &input,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &input_pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(FRAME_SIZE * 4),
            rows_per_image: Some(FRAME_SIZE),
        },
        wgpu::Extent3d {
            width: FRAME_SIZE,
            height: FRAME_SIZE,
            depth_or_array_layers: 1,
        },
    );
    let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
    let texture_inputs: Vec<(&str, &wgpu::TextureView)> = document
        .inputs
        .iter()
        .filter(|input| crate::isf::source::is_texture_type(&input.type_))
        .map(|input| (input.name.as_str(), &input_view))
        .collect();

    let mut runtime = IsfRuntime::new(device.clone(), queue.clone(), 4);
    let mut compiled = runtime
        .compile(&document, stem, TARGET_FORMAT)
        .map_err(|error| format!("{error:?}"))?;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modV ISF corpus smoke target"),
        size: wgpu::Extent3d {
            width: FRAME_SIZE,
            height: FRAME_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    runtime
        .draw(
            &mut compiled,
            &view,
            [FRAME_SIZE, FRAME_SIZE],
            FRAME,
            &IsfValues::new(),
            &texture_inputs,
            &texture_inputs,
        )
        .map_err(|error| format!("{error:?}"))?;
    let bytes = crate::isf::runtime::read_texture(&device, &queue, &target)?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translated_stems() -> Vec<String> {
        let baseline: serde_json::Value =
            serde_json::from_str(include_str!("../data/isf-corpus-baseline.json"))
                .expect("corpus baseline parses");
        baseline["translated"]
            .as_array()
            .expect("baseline has a translated list")
            .iter()
            .map(|name| name.as_str().expect("shader name is a string").to_string())
            .collect()
    }

    /// The finiteness check itself must not silently become a no-op.
    #[test]
    fn finiteness_check_catches_nan_and_infinity() {
        assert_eq!(first_non_finite(&[0.0, 1.0, -1.0, 0.5]), None);
        assert_eq!(first_non_finite(&[0.0, f32::NAN, 1.0]), Some(1));
        assert_eq!(first_non_finite(&[f32::INFINITY]), Some(0));
        assert_eq!(first_non_finite(&[0.0, f32::NEG_INFINITY]), Some(1));
    }

    #[test]
    fn every_translated_corpus_shader_draws_a_finite_frame() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping ISF corpus render test: no GPU adapter in this environment");
            return;
        }
        let stems = translated_stems();
        assert_eq!(stems.len(), 402, "corpus size changed");

        // Keep the exception list honest: small, and only naming shaders that
        // actually exist in the baseline. A stale/typo'd entry or an attempt
        // to skip a large part of the corpus fails here.
        let expected: BTreeSet<&str> = EXPECTED_NON_FINITE.iter().map(|(stem, _)| *stem).collect();
        assert!(
            expected.len() <= 12,
            "expected-non-finite allowlist grew to {} entries; re-check each shader before extending it",
            expected.len()
        );
        let present: BTreeSet<&str> = stems.iter().map(String::as_str).collect();
        assert!(
            expected.is_subset(&present),
            "stale expected-non-finite entries: {:?}",
            expected.difference(&present).collect::<Vec<_>>()
        );

        for stem in stems {
            let frame =
                render_corpus_shader(&stem).unwrap_or_else(|error| panic!("{stem}: {error}"));
            assert_eq!(frame.len(), 16 * 16 * 4, "{stem}: wrong framebuffer size");

            // Allowlisted shaders were still compiled, drawn and read back
            // above, so pipeline/driver regressions remain covered. Only the
            // per-pixel finiteness assertion is waived for their documented
            // upstream singularities.
            if expected.contains(stem.as_str()) {
                continue;
            }
            if let Some(index) = first_non_finite(&frame) {
                panic!("{stem}: non-finite channel {index} ({})", frame[index]);
            }
        }
    }
}
