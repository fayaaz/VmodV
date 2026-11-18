//! Golden-frame regression tests for the two CC0 "liminal building" corpus
//! generators — `Back The Way You Came.fs` and `The Pool Rooms.fs`.
//!
//! These are the most expensive shaders in the corpus (full-screen infinite
//! raymarchers with analytic water on top), so they are the ones worth
//! optimising in place. The optimisation work is only safe if the image does
//! not move, so both are pinned to a fixed frame: the corpus is rendered at
//! [`GOLDEN_SIZE`] with a fixed `[`IsfFrameUniforms`]`, and the result is
//! compared against the checked-in PNGs in `data/golden/`.
//!
//! Regenerate the PNGs (only after a deliberate look change) with:
//!
//! ```sh
//! cargo run --release -p modv-renderers --example isf_golden -- --generate
//! ```
//!
//! The comparison allows a little tolerance for cross-vendor transcendental
//! precision (`sin`/`cos` differ by ulps across drivers, and a 1-ulp
//! difference can flip a sphere-march step at a silhouette). A broken camera,
//! march or shading change shifts whole regions and blows these tolerances
//! immediately; the baseline on lavapipe is 0.0 everywhere.

#![cfg(test)]

use std::path::PathBuf;

use super::runtime::read_texture;
use super::{IsfFrameUniforms, IsfRuntime, IsfValues, parse_isf};
use crate::test_gpu::gpu;

/// The canonical golden frame size. Keep in sync with `examples/isf_golden.rs`.
const GOLDEN_SIZE: [u32; 2] = [160, 90];

/// Fixed frame for the golden render. Keep in sync with `examples/isf_golden.rs`.
const FRAME: IsfFrameUniforms = IsfFrameUniforms {
    time: 12.5,
    time_delta: 1.0 / 60.0,
    frame_index: 750,
    date: [2026.0, 11.0, 19.0, 43_200.0],
};

/// The pinned shaders, as `(corpus file stem, golden PNG)`.
const SHADERS: [(&str, &[u8]); 2] = [
    (
        "Back The Way You Came",
        include_bytes!("../../data/golden/back-the-way-you-came.png"),
    ),
    (
        "The Pool Rooms",
        include_bytes!("../../data/golden/the-pool-rooms.png"),
    ),
];

/// Mean per-channel absolute difference in 0..=255.
const MAX_MEAN_ABS_DELTA: f64 = 2.0;
/// Largest allowed per-pixel delta before a pixel counts as an outlier.
const OUTLIER_DELTA: u8 = 24;
/// Largest allowed fraction of outlier pixels.
const MAX_OUTLIER_FRACTION: f64 = 0.005;

fn corpus_path(stem: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/isf")
        .join(format!("{stem}.fs"))
}

fn render_frame(stem: &str) -> (Vec<u8>, [u32; 2]) {
    let (device, queue) = gpu().expect("golden test requires a GPU adapter");
    let destination = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modv ISF golden target"),
        size: wgpu::Extent3d {
            width: GOLDEN_SIZE[0],
            height: GOLDEN_SIZE[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut runtime = IsfRuntime::new(device, queue, 4);
    let source = std::fs::read_to_string(corpus_path(stem)).expect("the corpus shader reads");
    let document = parse_isf(&source, None).expect("the shader parses");
    let mut compiled = runtime
        .compile(&document, stem, wgpu::TextureFormat::Rgba8Unorm)
        .expect("the shader compiles");

    let view = destination.create_view(&wgpu::TextureViewDescriptor::default());
    runtime
        .draw(
            &mut compiled,
            &view,
            GOLDEN_SIZE,
            FRAME,
            &IsfValues::new(),
            &[],
            &[],
        )
        .expect("the golden frame draws");
    (
        read_texture(runtime.device(), runtime.queue(), &destination).expect("read the frame back"),
        GOLDEN_SIZE,
    )
}

fn decode_golden(bytes: &[u8]) -> Vec<u8> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .expect("the golden PNG's format is guessed");
    reader
        .decode()
        .expect("the golden PNG decodes")
        .into_rgba8()
        .into_vec()
}

/// `(mean absolute delta, max delta, outlier fraction)` over the RGB channels.
fn compare(rendered: &[u8], golden: &[u8]) -> (f64, u8, f64) {
    assert_eq!(rendered.len(), golden.len(), "frame sizes must match");
    let mut sum = 0.0f64;
    let mut max_delta = 0u8;
    let mut outliers = 0usize;
    let mut pixels = 0usize;
    let (rendered, _) = rendered.as_chunks::<4>();
    let (golden, _) = golden.as_chunks::<4>();
    for (actual, expected) in rendered.iter().zip(golden) {
        pixels += 1;
        let mut worst = 0u8;
        for channel in 0..3 {
            let delta = actual[channel].abs_diff(expected[channel]);
            sum += f64::from(delta);
            worst = worst.max(delta);
        }
        max_delta = max_delta.max(worst);
        if worst > OUTLIER_DELTA {
            outliers += 1;
        }
    }
    let channels = pixels as f64 * 3.0;
    (sum / channels, max_delta, outliers as f64 / pixels as f64)
}

fn run_golden(stem: &str, golden_bytes: &[u8]) {
    if gpu().is_none() {
        eprintln!("skipping the {stem} golden test: no GPU adapter in this environment");
        return;
    }
    let (rendered, size) = render_frame(stem);
    let golden = decode_golden(golden_bytes);
    let (mean_abs, max_delta, outliers) = compare(&rendered, &golden);
    assert!(
        mean_abs <= MAX_MEAN_ABS_DELTA,
        "{stem}: the frame drifted from the golden image (mean |delta| {mean_abs:.3}/255, \
         max {max_delta}/255, outlier pixels {outliers:.3}%). If the change in the shader is \
         deliberate, regenerate the golden with `example isf_golden -- --generate`; otherwise \
         something in the march/shading/water math moved."
    );
    assert!(
        outliers <= MAX_OUTLIER_FRACTION,
        "{stem}: {outliers:.3}% of pixels differ from the golden image by more than \
         {OUTLIER_DELTA}/255 (max {max_delta}/255); the two raymarched buildings look wrong"
    );
    eprintln!("{stem}: mean |delta| {mean_abs:.3}/255, max {max_delta}/255, frame {size:?} — ok");
}

#[test]
fn back_the_way_you_came_matches_the_golden_frame() {
    let (stem, golden) = SHADERS[0];
    run_golden(stem, golden);
}

#[test]
fn the_pool_rooms_matches_the_golden_frame() {
    let (stem, golden) = SHADERS[1];
    run_golden(stem, golden);
}
