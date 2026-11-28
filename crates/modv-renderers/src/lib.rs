//! Renderer implementations for the Rust engine. See AGENTS.md.
#![forbid(unsafe_code)]

pub mod isf;
pub mod milkdrop;
pub mod point10;
pub mod shader;
pub mod two_d;
pub mod user_module;

#[cfg(test)]
mod test_gpu;
#[cfg(test)]
mod test_gpu_compute;
#[cfg(test)]
mod test_gpu_isf;
#[cfg(test)]
mod test_gpu_postfx;
#[cfg(test)]
mod test_gpu_render;
