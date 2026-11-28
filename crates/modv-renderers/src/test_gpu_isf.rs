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

use crate::isf::{IsfFrameUniforms, IsfRuntime, IsfValue, IsfValues, parse_isf};

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
    /// The shader.gallery import must not bind its four palette inputs or the
    /// `gl_FragCoord`/`RENDERSIZE` pair in a way that leaves the frame black.
    #[test]
    fn shader_gallery_wireframe_draws_a_visible_frame() {
        if crate::test_gpu::gpu().is_none() {
            eprintln!("skipping Wireframe render test: no GPU adapter in this environment");
            return;
        }
        let frame = render_corpus_shader("Wireframe").expect("Wireframe renders");
        assert_eq!(frame.len(), FRAME_SIZE as usize * FRAME_SIZE as usize * 4);
        assert!(
            frame
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..3].iter().any(|c| *c > 0.02)),
            "Wireframe rendered a black frame"
        );
    }

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
        assert_eq!(stems.len(), 403, "corpus size changed");

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

/// GPU pixel counterparts of the text-only assertions in `isf::source` and
/// `isf::runtime`.
///
/// Those tests pin the *mechanism* (a `RENDERSIZE.y - gl_FragCoord.y` rewrite,
/// a bottom-left/top-left `isf_FragNormCoord` assignment, an unrewritten `.r`
/// or `for` token). This module pins the *result*: the same shaders are parsed
/// from inline sources, compiled with [`IsfRuntime::compile`], drawn into a
/// small `Rgba32Float` target with [`IsfRuntime::draw`] and read back with
/// [`crate::isf::runtime::read_texture`]. A translator change that keeps the
/// text shape but flips the wrong channel, or silently changes a uniform
/// layout, fails here with a pixel diff instead.
#[cfg(test)]
mod pixel_behaviour {
    use super::*;

    const SIZE: u32 = 8;

    /// Both coordinate systems as colour channels, as in the runtime
    /// orientation tests: green is `isf_FragNormCoord.y`, blue is
    /// `gl_FragCoord.y / RENDERSIZE.y`. The middle-column pixels of the top and
    /// bottom rows are enough to tell a WebGL bottom-left origin from a wgpu
    /// top-left one.
    const ORIENTATION_BODY: &str = "void main() { gl_FragColor = vec4(isf_FragNormCoord, gl_FragCoord.y / RENDERSIZE.y, 1.0); }";

    const SIDECAR_PROBE: &str = r#"varying vec2 norm;
    void main() { gl_FragColor = vec4(norm, 0.0, 1.0); }"#;

    const SIDECAR_VERTEX: &str = r#"varying vec2 norm;
    void main() { isf_vertShaderInit(); norm = isf_FragNormCoord; }"#;

    // Header *entries* (not whole comments): `parse_isf` reads only the first
    // `/*{ ... }*/` block, so every probe needs exactly one merged header.
    const IMAGE_INPUT_JSON: &str = r#""INPUTS":[{"NAME":"inputImage","TYPE":"image"}]"#;
    const BUFFER_ONLY_JSON: &str = r#""PASSES":[{"TARGET":"accum","PERSISTENT":true}]"#;

    fn with_header(entries: &str, body: &str) -> String {
        format!("/*{{{entries}}}*/\n{body}")
    }

    fn with_image_and_buffer(body: &str) -> String {
        with_header(&format!("{IMAGE_INPUT_JSON},{BUFFER_ONLY_JSON}"), body)
    }

    /// Compile one inline shader, draw it for a fixed frame and return its
    /// pixels as tightly packed `r, g, b, a` floats in row-major order.
    ///
    /// `image_input` binds a real opaque texture (the same shape as the
    /// runtime's `solid_input`) when the shader declares an external image.
    fn draw_probe(
        source: &str,
        vertex: Option<&str>,
        name: &str,
        values: &IsfValues,
        image_input: Option<(&str, [u8; 4])>,
    ) -> Option<Vec<f32>> {
        let (device, queue) = crate::test_gpu::gpu()?;
        let document =
            parse_isf(source, vertex).unwrap_or_else(|error| panic!("{name} parses: {error}"));

        // Declared before the view so it outlives the borrowed bindings below.
        let input_texture = image_input.map(|(_, rgba)| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("modV ISF pixel-behaviour input"),
                size: wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
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
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba.repeat((SIZE * SIZE) as usize),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: Some(SIZE),
                },
                wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
            );
            texture
        });
        let input_view = input_texture
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        let bindings: Vec<(&str, &wgpu::TextureView)> = image_input
            .into_iter()
            .zip(input_view.iter())
            .map(|((input_name, _), view)| (input_name, view))
            .collect();

        let mut runtime = IsfRuntime::new(device.clone(), queue.clone(), 4);
        let mut compiled = runtime
            .compile(&document, name, TARGET_FORMAT)
            .unwrap_or_else(|error| panic!("{name} compiles: {error:?}"));
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("modV ISF pixel-behaviour target"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        runtime
            .draw(
                &mut compiled,
                &view,
                [SIZE, SIZE],
                FRAME,
                values,
                &bindings,
                &bindings,
            )
            .unwrap_or_else(|error| panic!("{name} draws: {error:?}"));
        let bytes =
            crate::isf::runtime::read_texture(&device, &queue, &target).expect("pixel read-back");
        Some(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect(),
        )
    }

    /// (top green, bottom green, top blue, bottom blue) at the middle column.
    fn orientation_rows(pixels: &[f32]) -> (f32, f32, f32, f32) {
        let stride = SIZE as usize * 4;
        let column = (SIZE as usize / 2) * 4;
        let top = column;
        let bottom = pixels.len() - stride + column;
        (
            pixels[top + 1],
            pixels[bottom + 1],
            pixels[top + 2],
            pixels[bottom + 2],
        )
    }

    fn assert_bottom_left(pixels: &[f32], label: &str) {
        let (top_green, bottom_green, top_blue, bottom_blue) = orientation_rows(pixels);
        assert!(
            top_green > 0.8 && bottom_green < 0.2,
            "{label}: isf_FragNormCoord.y must be 1 at the top row like WebGL \
             (top {top_green}, bottom {bottom_green})"
        );
        assert!(
            top_blue > 0.8 && bottom_blue < 0.2,
            "{label}: gl_FragCoord.y / RENDERSIZE.y must be 1 at the top row like WebGL \
             (top {top_blue}, bottom {bottom_blue})"
        );
    }

    fn assert_top_left(pixels: &[f32], label: &str) {
        let (top_green, bottom_green, top_blue, bottom_blue) = orientation_rows(pixels);
        assert!(
            top_green < 0.2 && bottom_green > 0.8,
            "{label}: isf_FragNormCoord.y must start at the top row like wgpu \
             (top {top_green}, bottom {bottom_green})"
        );
        assert!(
            top_blue < 0.2 && bottom_blue > 0.8,
            "{label}: gl_FragCoord.y / RENDERSIZE.y must start at the top row like wgpu \
             (top {top_blue}, bottom {bottom_blue})"
        );
    }

    fn assert_uniform_pixels(pixels: &[f32], expected: [f32; 4], label: &str) {
        assert_eq!(
            pixels.len(),
            (SIZE * SIZE) as usize * 4,
            "{label}: wrong framebuffer size"
        );
        for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            for channel in 0..4 {
                assert!(
                    (pixel[channel] - expected[channel]).abs() <= 1e-5,
                    "{label}: pixel {index} channel {channel} = {}, expected {}",
                    pixel[channel],
                    expected[channel]
                );
            }
        }
    }

    /// `texture_free_shaders_flip_gl_fragcoord_to_bottom_left` in `isf::source`
    /// and `texture_free_generators_use_the_legacy_bottom_left_coordinates` in
    /// `isf::runtime`, checked as pixels rather than source text.
    #[test]
    fn texture_free_generator_pixels_use_the_bottom_left_origin() {
        let Some(pixels) = draw_probe(
            ORIENTATION_BODY,
            None,
            "gpu-pixel texture-free orientation",
            &IsfValues::new(),
            None,
        ) else {
            eprintln!("skipping ISF pixel orientation test: no adapter");
            return;
        };
        assert_bottom_left(&pixels, "texture-free generator");
    }

    /// `textured_shaders_keep_the_top_left_fragcoord` in `isf::source`: one
    /// external image input is enough to switch both coordinate systems to the
    /// wgpu top-left origin so input sampling stays upright.
    #[test]
    fn textured_shader_pixels_keep_the_top_left_origin() {
        let source = with_header(IMAGE_INPUT_JSON, ORIENTATION_BODY);
        let Some(pixels) = draw_probe(
            &source,
            None,
            "gpu-pixel textured orientation",
            &IsfValues::new(),
            Some(("inputImage", [255, 0, 0, 255])),
        ) else {
            eprintln!("skipping ISF pixel orientation test: no adapter");
            return;
        };
        assert_top_left(&pixels, "textured generator");
    }

    /// `self_contained_buffer_only_shaders_flip_gl_fragcoord_to_bottom_left`
    /// (`isf::source`) and `self_contained_buffer_generator_uses_the_legacy_
    /// bottom_left_coordinates` (`isf::runtime`).
    #[test]
    fn self_contained_buffer_pixels_use_the_bottom_left_origin() {
        let source = with_header(BUFFER_ONLY_JSON, ORIENTATION_BODY);
        let Some(pixels) = draw_probe(
            &source,
            None,
            "gpu-pixel buffer-only orientation",
            &IsfValues::new(),
            None,
        ) else {
            eprintln!("skipping ISF pixel orientation test: no adapter");
            return;
        };
        assert_bottom_left(&pixels, "self-contained buffer generator");
    }

    /// `externally_supplied_multipass_shaders_keep_the_top_left_fragcoord`
    /// (`isf::source`) and `externally_supplied_multipass_generator_keeps_the_
    /// wgpu_top_left_coordinates` (`isf::runtime`): a buffer alone does not
    /// trigger the WebGL flip, an external image does.
    #[test]
    fn externally_supplied_multipass_pixels_keep_the_top_left_origin() {
        let source = with_image_and_buffer(ORIENTATION_BODY);
        let Some(pixels) = draw_probe(
            &source,
            None,
            "gpu-pixel external multipass orientation",
            &IsfValues::new(),
            Some(("inputImage", [255, 0, 0, 255])),
        ) else {
            eprintln!("skipping ISF pixel orientation test: no adapter");
            return;
        };
        assert_top_left(&pixels, "externally supplied multipass generator");
    }

    /// `self_contained_sidecar_generator_emits_the_bottom_left_norm_varying`
    /// (`isf::source`) and `self_contained_sidecar_generator_uses_the_legacy_
    /// bottom_left_coordinates` (`isf::runtime`), through an actual `.vs`.
    #[test]
    fn self_contained_sidecar_norm_pixels_use_the_bottom_left_origin() {
        let source = with_header(BUFFER_ONLY_JSON, SIDECAR_PROBE);
        let Some(pixels) = draw_probe(
            &source,
            Some(SIDECAR_VERTEX),
            "gpu-pixel sidecar bottom-left",
            &IsfValues::new(),
            None,
        ) else {
            eprintln!("skipping ISF pixel orientation test: no adapter");
            return;
        };
        let (top_green, bottom_green, _, _) = orientation_rows(&pixels);
        assert!(
            top_green > 0.8 && bottom_green < 0.2,
            "a self-contained sidecar's norm.y must be 1 at the top \
             (top {top_green}, bottom {bottom_green})"
        );
    }

    /// `externally_supplied_sidecar_generator_keeps_the_top_left_norm_varying`
    /// in `isf::source`: the sidecar selector follows the same buffer-only
    /// rule as the fragment stage.
    #[test]
    fn externally_supplied_sidecar_norm_pixels_keep_the_top_left_origin() {
        let source = with_image_and_buffer(SIDECAR_PROBE);
        let Some(pixels) = draw_probe(
            &source,
            Some(SIDECAR_VERTEX),
            "gpu-pixel sidecar top-left",
            &IsfValues::new(),
            Some(("inputImage", [255, 0, 0, 255])),
        ) else {
            eprintln!("skipping ISF pixel orientation test: no adapter");
            return;
        };
        let (top_green, bottom_green, _, _) = orientation_rows(&pixels);
        assert!(
            top_green < 0.2 && bottom_green > 0.8,
            "an externally supplied sidecar's norm.y must start at the top \
             (top {top_green}, bottom {bottom_green})"
        );
    }

    /// `a_short_bool_input_name_does_not_rewrite_swizzles` in `isf::source`.
    /// A macro-based bool rewrite would corrupt `pixel.r` into a call and fail
    /// to translate; a uniform-layout bug would give the wrong red channel.
    /// Both draws are checked pixel-for-pixel.
    #[test]
    fn bool_input_named_r_does_not_corrupt_swizzles_in_pixels() {
        let shader = r#"/*{ "INPUTS": [ { "NAME": "r", "TYPE": "bool" } ] }*/
        void main() {
            vec4 pixel = vec4(0.2, 0.4, 0.6, 1.0);
            if (r) { pixel.r = 1.0; }
            gl_FragColor = pixel;
        }"#;

        let mut off = IsfValues::new();
        off.insert("r".into(), IsfValue::Bool(false));
        let Some(pixels) = draw_probe(shader, None, "gpu-pixel bool r off", &off, None) else {
            eprintln!("skipping ISF input-rewrite test: no adapter");
            return;
        };
        assert_uniform_pixels(&pixels, [0.2, 0.4, 0.6, 1.0], "bool r = false");

        let mut on = IsfValues::new();
        on.insert("r".into(), IsfValue::Bool(true));
        let Some(pixels) = draw_probe(shader, None, "gpu-pixel bool r on", &on, None) else {
            eprintln!("skipping ISF input-rewrite test: no adapter");
            return;
        };
        assert_uniform_pixels(&pixels, [1.0, 0.4, 0.6, 1.0], "bool r = true");
    }

    /// `statement_keyword_inputs_keep_control_flow_intact` in `isf::source`:
    /// inputs named `return`, `for` and `if` must be renamed at declaration
    /// only, leaving the keywords to drive the loop and early return. The
    /// expected red is exactly 3 (the loop adds 1.0 three times) in every
    /// pixel, so a broken control flow cannot pass by accident.
    #[test]
    fn statement_keyword_inputs_keep_control_flow_in_pixels() {
        let shader = r#"/*{ "INPUTS": [
            { "NAME": "return", "TYPE": "float" },
            { "NAME": "for", "TYPE": "float" },
            { "NAME": "if", "TYPE": "event" }
        ] }*/
        void main() {
            float total = 0.0;
            for (int i = 0; i < 3; i++) { total += 1.0; }
            if (total > 0.0) { gl_FragColor = vec4(total, 0.0, 0.0, 1.0); return; }
            gl_FragColor = vec4(0.0);
        }"#;
        let Some(pixels) = draw_probe(
            shader,
            None,
            "gpu-pixel reserved-word inputs",
            &IsfValues::new(),
            None,
        ) else {
            eprintln!("skipping ISF input-rewrite test: no adapter");
            return;
        };
        assert_uniform_pixels(&pixels, [3.0, 0.0, 0.0, 1.0], "reserved-word control flow");
    }
}
