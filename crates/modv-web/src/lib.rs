//! Browser shell for VmodV.
//!
//! `state` is the platform-neutral slice of the engine the browser owns;
//! the wasm-only `app` module wraps it in eframe/egui and the real compositor.

#![forbid(unsafe_code)]

pub mod audio;
pub mod state;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod mic;

#[cfg(target_arch = "wasm32")]
pub use app::start;
#[cfg(target_arch = "wasm32")]
pub use mic::enable_microphone;
