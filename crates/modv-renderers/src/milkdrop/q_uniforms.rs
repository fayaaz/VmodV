//! Shared layout of the additive `MilkQUniforms` binding.
//!
//! Real MilkDrop 2 presets read their per-frame EEL `q1..q32` variables
//! directly in warp/comp shader bodies. They live in a dedicated uniform block
//! so the canonical [`super::runtime::MilkUniforms`] block (binding 0) and all
//! of its byte offsets stay untouched. Both the GLSL wrapper emitted by
//! [`super::translate`] and the wgpu bind group built by [`super::runtime`]
//! derive their layout from the constants here — a single source of truth.

use super::runtime::MILK_RUNTIME_SAMPLER_PAIRS;

/// `@group(0)` binding of the `MilkQUniforms` block.
///
/// The main uniform occupies binding 0 and each of the
/// [`MILK_RUNTIME_SAMPLER_PAIRS`] samplers takes two bindings (texture +
/// sampler), so the first free slot is `1 + 13 * 2 = 27`. Deriving it from the
/// sampler count keeps emission and binding in lockstep if a sampler is ever
/// added or removed.
pub(crate) const MILK_Q_BINDING: u32 = 1 + (MILK_RUNTIME_SAMPLER_PAIRS as u32) * 2;

/// The per-frame EEL `q1..q32` names, in GPU byte order: `qN` lives at float
/// index `N - 1` of the block.
pub(crate) const Q_UNIFORM_MEMBERS: [&str; 32] = [
    "q1", "q2", "q3", "q4", "q5", "q6", "q7", "q8", "q9", "q10", "q11", "q12", "q13", "q14", "q15",
    "q16", "q17", "q18", "q19", "q20", "q21", "q22", "q23", "q24", "q25", "q26", "q27", "q28",
    "q29", "q30", "q31", "q32",
];

/// Byte size of the `MilkQUniforms` block: one `f32` per [`Q_UNIFORM_MEMBERS`].
pub const MILK_Q_STRIDE: usize = Q_UNIFORM_MEMBERS.len() * std::mem::size_of::<f32>();
