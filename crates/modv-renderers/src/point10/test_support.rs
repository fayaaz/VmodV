//! Headless test harness for the Point10 renderer.
//!
//! Same pattern as `crates/modv-render/src/graph_tests.rs` and
//! `crates/modv-renderers/src/two_d/tests.rs`: request the **fallback**
//! (software/lavapipe) adapter, and skip every GPU test with a log line when
//! there is none, so a GPU-less CI machine still compiles and runs the pure
//! unit tests.
//!
//! `#[cfg(test)]` only — nothing here ships.

#![cfg(test)]
#![forbid(unsafe_code)]

/// Whitespace-insensitive substring test. Compacts BOTH sides, so a needle
/// containing spaces inside string literals still matches. Never compare a raw
/// needle against a compacted source — see the
/// `no_contract_compacts_only_one_side` guard.
pub(crate) fn compact_contains(source: &str, needle: &str) -> bool {
    compact(source).contains(&compact(needle))
}

/// The single whitespace-compaction implementation shared by test contracts.
pub(crate) fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// Count whitespace-insensitive, non-overlapping substring matches.
pub(crate) fn compact_match_count(source: &str, needle: &str) -> usize {
    compact(source).matches(&compact(needle)).count()
}

/// Whitespace-insensitive suffix test for contracts that pin shader endings.
pub(crate) fn compact_ends_with(source: &str, suffix: &str) -> bool {
    compact(source).ends_with(&compact(suffix))
}

/// Minimal `std`-only `block_on`, same approach as `modv-render/src/device.rs`.
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll};

    let mut cx = Context::from_waker(std::task::Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// A software-adapter device, or `None` (with a log line) when unavailable.
///
/// Delegates to the crate-wide shared adapter ([`crate::test_gpu`]): one
/// instance/device per test binary, cloned per test. Parallel per-thread
/// instances crash lavapipe.
pub(crate) fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    crate::test_gpu::gpu()
}

/// An 8-bit UNORM render target plus its view, for read-back.
pub(crate) struct TestTarget {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl TestTarget {
    /// Create an `Rgba8Unorm` target (the composer applies the sRGB transfer
    /// itself for non-sRGB formats).
    pub(crate) fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("point10-test-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width,
            height,
        }
    }

    /// Read the target back as tightly packed RGBA8 (`width * height * 4`).
    pub(crate) fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let unpadded = self.width as u64 * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;
        let padded = unpadded.div_ceil(align) * align;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point10-test-readback"),
            size: padded * self.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-test-readback"),
        });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll must succeed");
        receiver
            .recv()
            .expect("map callback must run")
            .expect("mapping must succeed");

        let mapped = slice.get_mapped_range().expect("buffer must be mapped");
        let mut out = Vec::with_capacity((unpadded * self.height as u64) as usize);
        for row in 0..self.height as u64 {
            let start = (row * padded) as usize;
            let end = start + unpadded as usize;
            out.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        buffer.unmap();
        out
    }

    /// The RGBA pixel at `(x, y)`.
    pub(crate) fn pixel(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
    ) -> [u8; 4] {
        let pixels = self.read(device, queue);
        let index = ((y * self.width + x) * 4) as usize;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    }
}

/// True when any pixel has non-zero alpha — a Point10 scene's "something was
/// drawn" check (scenes clear to transparent black).
pub(crate) fn any_visible(pixels: &[u8]) -> bool {
    pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0)
}

/// True when any pixel carries visible RGB and alpha.
pub(crate) fn any_lit(pixels: &[u8]) -> bool {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[3] > 0 && (pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) > 0)
}

/// Rec. 709 luma of a bloom-enabled frame and its direct-only counterpart.
///
/// Used by the Plane9 bloom migration contracts in the three.js-ported scenes,
/// which compare the same 64x64 render with and without the composer's additive
/// combine. The thresholds in [`check_bloom_brightness`] are calibrated against
/// the vendor driver's one-mip bloom.
#[derive(Debug)]
pub(crate) struct BloomBrightness {
    pub(crate) mean: f32,
    pub(crate) direct_mean: f32,
    pub(crate) dark_count: usize,
    pub(crate) dark_mean: f32,
    pub(crate) dark_direct_mean: f32,
    pub(crate) gained_fraction: f32,
}

/// Measure the bloom halo over pixels that were dark in the direct-only frame.
pub(crate) fn bloom_brightness(with_bloom: &[u8], direct_only: &[u8]) -> BloomBrightness {
    assert_eq!(with_bloom.len(), direct_only.len());
    let luma = |pixels: &[u8]| {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| {
                (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                    / 255.0
            })
            .collect::<Vec<_>>()
    };
    let lit = luma(with_bloom);
    let direct = luma(direct_only);
    let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len() as f32;
    let dark = lit
        .iter()
        .zip(&direct)
        .filter(|(_, direct)| **direct <= 1.0 / 255.0)
        .collect::<Vec<_>>();
    BloomBrightness {
        mean: mean(&lit),
        direct_mean: mean(&direct),
        dark_count: dark.len(),
        dark_mean: dark.iter().map(|(lit, _)| **lit).sum::<f32>() / dark.len().max(1) as f32,
        dark_direct_mean: dark.iter().map(|(_, direct)| **direct).sum::<f32>()
            / dark.len().max(1) as f32,
        gained_fraction: dark.iter().filter(|(lit, _)| **lit > 1.0 / 255.0).count() as f32
            / dark.len().max(1) as f32,
    }
}

/// Accumulate the bloom-halo assertions into `failures` for one labelled frame.
pub(crate) fn check_bloom_brightness(
    failures: &mut Vec<String>,
    label: &str,
    brightness: &BloomBrightness,
) {
    use std::fmt::Write;

    let mut detail = String::new();
    writeln!(
        detail,
        "{label}: mean={:.6} direct_mean={:.6} dark_count={} dark_mean={:.6} dark_direct_mean={:.6} gained_fraction={:.3}",
        brightness.mean,
        brightness.direct_mean,
        brightness.dark_count,
        brightness.dark_mean,
        brightness.dark_direct_mean,
        brightness.gained_fraction
    )
    .expect("write to String");
    eprintln!("{detail}");
    if brightness.dark_count < 256 {
        failures.push(format!("{detail}dark-pixel mask is vacuous (<256 pixels)"));
    }
    if brightness.dark_mean <= 0.003 {
        failures.push(format!(
            "{detail}Plane9 bloom did not brighten dark pixels above 0.003"
        ));
    }
    if brightness.dark_mean <= brightness.dark_direct_mean + 0.002 {
        failures.push(format!(
            "{detail}dark pixels gained less than 0.002 mean luma"
        ));
    }
    if brightness.gained_fraction <= 0.1 {
        failures.push(format!(
            "{detail}at least 10% of dark pixels must gain bloom"
        ));
    }
    if brightness.mean <= brightness.direct_mean * 1.05 {
        failures.push(format!(
            "{detail}bloom-enabled frame must be at least 5% brighter overall"
        ));
    }
}

/// Return semicolon-delimited statements that compact one side and then call
/// `.contains(` on it.
///
/// This is deliberately a pragmatic line/statement heuristic: it recognises
/// direct `compact(...).contains(...)` chains, locals assigned from `compact`
/// or an explicit whitespace filter, and manual `is_whitespace()` compaction.
/// It can miss aliases hidden behind macros or unrelated helper names, so it is
/// a recurrence guard rather than a Rust parser.
fn unsafe_compacted_contains(source: &str) -> Vec<(usize, String)> {
    let mut compacted_locals = std::collections::BTreeSet::new();
    for line in source.lines() {
        let Some((lhs, rhs)) = line.split_once('=') else {
            continue;
        };
        if (rhs.contains("compact(") || rhs.contains("is_whitespace()"))
            && let Some(identifier) = lhs.split_whitespace().next_back()
        {
            compacted_locals.insert(identifier.to_string());
        }
    }

    let mut failures = Vec::new();
    let mut statement = String::new();
    let mut statement_line = 1usize;
    for (index, line) in source.lines().enumerate() {
        if statement.is_empty() {
            statement_line = index + 1;
        }
        statement.push_str(line);
        statement.push(' ');
        if !line.contains(';') {
            continue;
        }
        let has_contains = statement.contains(".contains(");
        let has_compaction = statement.contains("compact(")
            || statement.contains("is_whitespace()")
            || statement
                .split(|character: char| !(character.is_alphanumeric() || character == '_'))
                .any(|token| compacted_locals.contains(token));
        if has_contains && has_compaction {
            failures.push((statement_line, statement.trim().to_string()));
        }
        statement.clear();
    }
    failures
}

#[test]
fn no_contract_compacts_only_one_side() {
    use std::path::Path;

    let scenes_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/point10/scenes");
    let mut contract_paths = Vec::new();
    for entry in std::fs::read_dir(&scenes_dir).expect("read Point10 scenes directory") {
        let path = entry.expect("read scene directory entry").path();
        let is_contract = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("contract") && name.ends_with(".rs"));
        if is_contract {
            contract_paths.push(path);
        }
    }
    contract_paths.sort();
    assert!(
        contract_paths.len() >= 3,
        "contract compaction guard found only {} contract files in {}; the scan must not be vacuous",
        contract_paths.len(),
        scenes_dir.display()
    );

    // Prove the detector is exercised in both supported bad shapes and does
    // not flag the safe helper that compacts both sides.
    let local_bad =
        "let compact_source = compact(&source);\ncompact_source.contains(\"1x1 plane\");";
    let direct_bad = "compact(&source).contains(\"1x1 plane\");";
    let safe = "compact_contains(source, \"1x1 plane\");";
    assert_eq!(
        unsafe_compacted_contains(local_bad).len(),
        1,
        "guard must detect a compacted local followed by contains"
    );
    assert_eq!(
        unsafe_compacted_contains(direct_bad).len(),
        1,
        "guard must detect a direct compact(...).contains(...) chain"
    );
    assert!(
        unsafe_compacted_contains(safe).is_empty(),
        "guard must accept compact_contains"
    );

    let mut failures = Vec::new();
    for path in contract_paths {
        let source = std::fs::read_to_string(&path).expect("read Plane9 contract");
        for (line, statement) in unsafe_compacted_contains(&source) {
            failures.push(format!(
                "{}:{line}: {statement}\n  use compact_contains(source, needle): it compacts both sides, so a needle containing spaces can match",
                path.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "Plane9 contracts still compact only one side:\n{}",
        failures.join("\n")
    );
}
