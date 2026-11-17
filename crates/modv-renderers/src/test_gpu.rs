//! Shared software-GPU adapter for this crate's tests.
//!
//! Every GPU test in `modv-renderers` renders on the fallback
//! (software/lavapipe) adapter and skips with a log line when there is none,
//! so a GPU-less machine still compiles and runs the pure unit tests.
//!
//! The adapter and device are created **once per test binary** and shared by
//! every test: `wgpu::Device`/`wgpu::Queue` are `Clone` (Arc-backed) and
//! thread-safe, so each test clones the pair and submits its own work.
//! Creating a fresh `wgpu::Instance` in every parallel test thread used to
//! SIGSEGV lavapipe, which aborted a default (parallel) `cargo test` run.
//!
//! `#[cfg(test)]` only — nothing here ships.

#![cfg(test)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Number of adapter/device initialisations actually performed by [`gpu`].
/// Tests assert on this to keep the harness single-adapter.
static INITIALISATIONS: AtomicUsize = AtomicUsize::new(0);

/// The one adapter-backed device pair for this test binary.
static SHARED: OnceLock<Option<(wgpu::Device, wgpu::Queue)>> = OnceLock::new();

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

/// Initialise the shared fallback adapter and device, counting the attempt.
fn init() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .map_err(|error| eprintln!("skipping GPU test: no adapter ({error})"))
    .ok()?;
    let pair = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("modv-renderers-test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|error| eprintln!("skipping GPU test: no device ({error})"))
    .ok()?;
    INITIALISATIONS.fetch_add(1, Ordering::Relaxed);
    Some(pair)
}

/// The shared software-adapter device and queue, or `None` (with a log line)
/// when no adapter is available.
pub(crate) fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    SHARED.get_or_init(init).clone()
}

/// How many times [`gpu`] has really created an adapter/device pair.
pub(crate) fn gpu_initialisations() -> usize {
    INITIALISATIONS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: parallel test threads must share one adapter. A fresh
    /// `wgpu::Instance` per call used to SIGSEGV lavapipe under `cargo
    /// test`'s default thread pool, aborting the whole binary.
    #[test]
    fn gpu_calls_share_one_adapter() {
        let before = gpu_initialisations();
        let mut available = 0;
        for _ in 0..8 {
            if gpu().is_some() {
                available += 1;
            }
        }
        if available == 0 {
            eprintln!("skipping: no GPU adapter in this environment");
            return;
        }
        let created = gpu_initialisations() - before;
        assert!(
            created <= 1,
            "each gpu() call initialised its own adapter ({created} created); \
             parallel tests would crash lavapipe"
        );
    }
}
