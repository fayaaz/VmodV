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
//! | [`translate`] | warp/comp shader body → desktop GLSL 450 → naga → owned WGSL ([`CompiledMilkShader`]) |
//! | [`props`] | preset → module metadata ([`MilkMeta`]) and prop table ([`MilkProp`]), mirroring `isf/props.rs` |
//! | [`runtime`] | CPU runtime core (Layer 5a): compile preset → [`MilkCompiledPreset`], per-frame globals/audio/followers, [`MilkMesh`] UV grid, [`MilkFailureCache`] (device attaches in 5b) |
//!
//! # Layering
//!
//! [`preset`] *parses* a preset into plain source strings; [`eel`] compiles
//! those strings into an owned [`eel::CompiledEquations`] once and evaluates
//! them many times per frame. [`translate`] is the shader-assembly layer,
//! mirroring the `isf/{parser,props,translate,runtime}` split and reusing the
//! same naga GLSL→WGSL path.
//!
//! The parser is deliberately extension-agnostic: whether a file is named
//! `.mlk`, `.txt` or has no extension at all is a concern of scanners and
//! hot-reload watchers, never of [`preset::parse_milk`].
//!
//! # Shader runtime binding contract
//!
//! The WGSL from [`translate`] is consumed by a wgpu pipeline builder (Layer 5):
//!
//! * `@group(0) @binding(0)` — uniform buffer with the MilkDrop prelude,
//!   following WGSL uniform alignment (pad members to 16-byte boundaries;
//!   256-byte stride if dynamic offsets are needed).
//! * Textures/samplers follow in a fixed order: built-ins first, then user
//!   samplers in declaration order.
//! * Entry point `main`, returning `@location(0) vec4<f32>`; a full-screen
//!   triangle drives comp, while warp consumes the mesh's warped UVs.

#![forbid(unsafe_code)]

pub mod eel;
pub mod preset;
pub mod props;
pub mod runtime;
pub mod translate;

pub use preset::{
    MilkBaseVals, MilkDocument, MilkEquationSet, MilkParseError, MilkShader, MilkShape, MilkWave,
    extract_shader_body, parse_milk,
};

pub use eel::{
    CompiledEquations, EelError, GlobalState, Globals, Rng, VarPool, compile, run_frame, run_pixel,
};

pub use translate::{
    CompiledMilkShader, MilkShaderStage, TranslateError, translate_milk_shader, wrap_milk_shader,
};

pub use props::{MilkMeta, MilkProp, MilkPropType, meta_from_document, synthesise_props};

pub use runtime::{
    MilkAudio, MilkCompiledPreset, MilkFailureCache, MilkFrameInfo, MilkInstance, MilkMesh,
    MilkRuntimeError, MilkRuntime, begin_frame, compile_preset, evaluate_mesh, new_instance,
};
