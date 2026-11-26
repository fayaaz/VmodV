//! Camera/screen/file capture, codecs, NDI, audio analysis. See AGENTS.md.

pub mod audio;

#[cfg(not(target_arch = "wasm32"))]
pub mod midi;
#[cfg(not(target_arch = "wasm32"))]
pub mod ndi;
#[cfg(not(target_arch = "wasm32"))]
pub mod osc;
#[cfg(not(target_arch = "wasm32"))]
pub mod video;
