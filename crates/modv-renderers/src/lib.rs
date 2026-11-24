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
