//! MilkDrop preset renderer — parser (Layer 1) and EEL evaluator (Layer 2).
//!
//! Native replacement for the `.mlk` preset loading that used to happen in
//! the archived JavaScript tree. Like [`crate::isf`], all format logic lives
//! in `modv-renderers` so it can be shared by the engine, the compositor and
//! the headless binary without pulling in a UI or GPU dependency.
//!
//! | Module | Responsibility |
//! |--------|----------------|
//! | [`preset`] | `.mlk` text → [`MilkDocument`] (base vals, EEL equation text, wave/shape sections, warp/composite shader text) |
//! | [`eel`] | EEL2/NSEEL equation text → owned compiled form, evaluated against a mutable variable pool (no `eval`/JS) |
//!
//! # Layering
//!
//! [`preset`] *parses* a preset into plain source strings; [`eel`] compiles
//! those strings into an owned [`eel::CompiledEquations`] once and evaluates
//! them many times per frame. Shader assembly is a further, separate layer,
//! mirroring the `isf/{parser,props,translate,runtime}` split.
//!
//! The parser is deliberately extension-agnostic: whether a file is named
//! `.mlk`, `.txt` or has no extension at all is a concern of scanners and
//! hot-reload watchers, never of [`preset::parse_milk`].

#![forbid(unsafe_code)]

pub mod eel;
pub mod preset;

pub use preset::{
    MilkBaseVals, MilkDocument, MilkEquationSet, MilkParseError, MilkShader, MilkShape, MilkWave,
    extract_shader_body, parse_milk,
};

pub use eel::{
    CompiledEquations, EelError, Globals, GlobalState, Rng, VarPool, compile, run_frame, run_pixel,
};
