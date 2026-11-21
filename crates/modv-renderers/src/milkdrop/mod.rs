//! MilkDrop preset renderer — parser layer (Layer 1).
//!
//! Native replacement for the `.mlk` preset loading that used to happen in
//! the archived JavaScript tree. Like [`crate::isf`], all format logic lives
//! in `modv-renderers` so it can be shared by the engine, the compositor and
//! the headless binary without pulling in a UI or GPU dependency.
//!
//! | Module | Responsibility |
//! |--------|----------------|
//! | [`preset`] | `.mlk` text → [`MilkDocument`] (base vals, EEL equation text, wave/shape sections, warp/composite shader text) |
//!
//! # Layering
//!
//! This module only *parses*. EEL equation compilation and shader assembly
//! are separate layers, mirroring the `isf/{parser,props,translate,runtime}`
//! split: a preset is reshaped into plain source strings here, and later
//! stages decide how those strings become executable code.
//!
//! The parser is deliberately extension-agnostic: whether a file is named
//! `.mlk`, `.txt` or has no extension at all is a concern of scanners and
//! hot-reload watchers, never of [`preset::parse_milk`].

#![forbid(unsafe_code)]

pub mod preset;

pub use preset::{
    MilkBaseVals, MilkDocument, MilkEquationSet, MilkParseError, MilkShader, MilkShape, MilkWave,
    extract_shader_body, parse_milk,
};
