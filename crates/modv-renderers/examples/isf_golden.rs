//! Render `Back The Way You Came.fs` and `The Pool Rooms.fs` through the real
//! ISF runtime.
//!
//! ```sh
//! # Rewrite the golden frames pinned by `src/isf/golden.rs` (run this only
//! # when the look of a shader changed deliberately; commit the result with
//! # the shader edit):
//! cargo run --release -p modv-renderers --example isf_golden -- --generate
//!
//! # Time the two shaders at a fixed size on the local adapter:
//! cargo run --release -p modv-renderers --example isf_golden -- --bench [frames]
//! ```
//!
//! Both modes pick whatever Vulkan adapter wgpu finds (hardware if present,
//! else lavapipe). The golden test skips when there is no adapter at all.

use std::path::PathBuf;
use std::time::Instant;

use modv_renderers::isf::runtime::read_texture;
use modv_renderers::isf::{IsfFrameUniforms, IsfRuntime, IsfValues, parse_isf};

/// The canonical golden frame size. Small on purpose: the shaders are
/// full-screen infinite raymarchers and these corpus GPU paths are expected to
/// run on lavapipe as well.
const GOLDEN_SIZE: [u32; 2] = [160, 90];

/// The size `--bench` measures at.
const BENCH_SIZE: [u32; 2] = [640, 360];

/// Fixed frame for both modes, so the numbers compare like with like.
const FRAME: IsfFrameUniforms = IsfFrameUniforms {
    time: 12.5,
    time_delta: 1.0 / 60.0,
    frame_index: 750,
    date: [2026.0, 11.0, 19.0, 43_200.0],
};

/// The two shaders this tool works on, as `(corpus file stem, golden file name)`.
const SHADERS: [(&str, &str); 2] = [
    ("Back The Way You Came", "back-the-way-you-came.png"),
    ("The Pool Rooms", "the-pool-rooms.png"),
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf")
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/golden")
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::Waker;
    use std::task::{Context, Poll};

    let mut cx = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn create_device() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wgpu::Instance::default();
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .map_err(|error| format!("no GPU adapter: {error}"))?;
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("modv-renderers isf_golden"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|error| format!("no GPU device: {error}"))
}

fn target_texture(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modv ISF golden target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
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
    })
}

fn render_frame(
    runtime: &mut IsfRuntime,
    compiled: &mut modv_renderers::isf::CompiledIsf,
    destination: &wgpu::Texture,
    size: [u32; 2],
) -> Vec<u8> {
    let view = destination.create_view(&wgpu::TextureViewDescriptor::default());
    runtime
        .draw(compiled, &view, size, FRAME, &IsfValues::new(), &[], &[])
        .expect("the frame draws");
    read_texture(runtime.device(), runtime.queue(), destination).expect("read the frame back")
}

fn load_and_compile(runtime: &mut IsfRuntime, stem: &str) -> modv_renderers::isf::CompiledIsf {
    let path = corpus_dir().join(format!("{stem}.fs"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let document = parse_isf(&source, None).expect("the shader parses");
    runtime
        .compile(&document, stem, wgpu::TextureFormat::Rgba8Unorm)
        .expect("the shader compiles")
}

fn generate() {
    let (device, queue) = create_device().unwrap_or_else(|error| {
        eprintln!("skipping golden generation: {error}");
        std::process::exit(1);
    });
    let mut runtime = IsfRuntime::new(device, queue, 4);
    std::fs::create_dir_all(golden_dir()).expect("the golden directory is writable");

    for (stem, file_name) in SHADERS {
        let mut compiled = load_and_compile(&mut runtime, stem);
        let destination = target_texture(runtime.device(), GOLDEN_SIZE);
        let pixels = render_frame(&mut runtime, &mut compiled, &destination, GOLDEN_SIZE);
        let path = golden_dir().join(file_name);
        image::save_buffer(
            &path,
            &pixels,
            GOLDEN_SIZE[0],
            GOLDEN_SIZE[1],
            image::ExtendedColorType::Rgba8,
        )
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        eprintln!("wrote {}", path.display());
    }
}

fn bench() {
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(30);

    let (device, queue) = create_device().unwrap_or_else(|error| {
        eprintln!("skipping the bench: {error}");
        std::process::exit(1);
    });
    let mut runtime = IsfRuntime::new(device, queue, 4);

    for (stem, _) in SHADERS {
        let mut compiled = load_and_compile(&mut runtime, stem);
        let destination = target_texture(runtime.device(), BENCH_SIZE);

        // Warm up: pipeline compilation and the first-frame cache fills
        // dominate frame one and would skew the numbers.
        for _ in 0..3 {
            let view = destination.create_view(&wgpu::TextureViewDescriptor::default());
            runtime
                .draw(
                    &mut compiled,
                    &view,
                    BENCH_SIZE,
                    FRAME,
                    &IsfValues::new(),
                    &[],
                    &[],
                )
                .expect("the warm-up frame draws");
        }
        runtime
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device flushes");

        let start = Instant::now();
        for _ in 0..frames {
            let view = destination.create_view(&wgpu::TextureViewDescriptor::default());
            runtime
                .draw(
                    &mut compiled,
                    &view,
                    BENCH_SIZE,
                    FRAME,
                    &IsfValues::new(),
                    &[],
                    &[],
                )
                .expect("the frame draws");
        }
        runtime
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device flushes");
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        let per_frame = elapsed / f64::from(frames);
        eprintln!(
            "{stem}: {per_frame:.2} ms/frame ({:.1} MHz) at {}x{} over {frames} frames",
            1000.0 / per_frame,
            BENCH_SIZE[0],
            BENCH_SIZE[1],
        );
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        None | Some("--generate") => generate(),
        Some("--bench") => bench(),
        Some(other) => {
            eprintln!("unknown mode: {other} (expected --generate or --bench)");
            std::process::exit(2);
        }
    }
}
