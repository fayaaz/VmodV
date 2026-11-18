//! Shared software-GPU engine for this crate's tests.
//!
//! Every GPU test in `modv-app` used to create its own [`Engine`], and
//! lavapipe crashes (SIGSEGV) when several instances render in parallel
//! threads. Sharing one adapter/device across the suite — the same approach
//! `modv-renderers` uses — keeps the default parallel `cargo test` alive.
//!
//! `#[cfg(test)]` only — nothing here ships.

use std::sync::OnceLock;

use modv_render::{Engine, EngineConfig};

/// The one adapter-backed engine for this test binary, created on first use.
/// The `Engine` is intentionally leaked: the process exits with it.
static SHARED: OnceLock<Option<&'static Engine>> = OnceLock::new();

/// The shared fallback (lavapipe) engine, or `None` when no adapter exists
/// (a GPU-less machine still passes, with a log line).
pub(crate) fn gpu_engine() -> Option<&'static Engine> {
    *SHARED.get_or_init(|| {
        Engine::new(&EngineConfig {
            force_fallback_adapter: true,
        })
        .ok()
        .map(|engine| &*Box::leak(Box::new(engine)))
    })
}
