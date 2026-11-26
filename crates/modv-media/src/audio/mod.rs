//! Audio analysis and transport. See AGENTS.md.

pub mod analyser;
pub mod bands;
pub mod beat_detektor;
pub mod features;
pub mod meyda;
pub mod phrase;
pub mod tempo;

#[cfg(not(target_arch = "wasm32"))]
pub mod capture;
#[cfg(not(target_arch = "wasm32"))]
pub mod engine;
#[cfg(not(target_arch = "wasm32"))]
pub mod midi_clock;
#[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
pub mod pipewire;
