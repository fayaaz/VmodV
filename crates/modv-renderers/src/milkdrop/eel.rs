//! MilkDrop EEL2/NSEEL evaluator (Layer 2).
//!
//! This module compiles the equation text produced by [`super::preset`] into
//! an owned [`CompiledEquations`] once and evaluates it many times per frame
//! against a mutable [`VarPool`]. There is deliberately **no** `eval`, no JS
//! engine and no allocation on the hot path: the equations run for every mesh
//! vertex, every pixel and every wave/shape point.
//!
//! # Ranges, lifetime and determinism
//!
//! * `q1..q32` are seeded by a preset's init equations, restored to those
//!   values at the start of each frame, and may be changed by per-frame
//!   equations (visible to pixel/wave/shape pools, not to the next frame).
//! * `t1..t8` carry shape/wave init values into their frame and point equations.
//! * User variables persist frame-to-frame; every other variable is rebuilt.
//! * `reg00..reg99` are scratch shared by all pools of one preset.
//! * `megabuf` is per-preset (1_048_576 floats); `gmegabuf` is global and
//!   shared by every preset through a cloneable [`GlobalState`].
//! * [`Rng`] is injected via [`VarPool::with_rng`] / [`VarPool::with_seed`]
//!   so evaluation is reproducible in tests.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;

/// Compiled, owned form of one EEL equation set.
///
/// Produced once by [`compile`] and reused for every evaluation. The exact
/// representation (AST or bytecode) is an implementation detail; callers only
/// rely on it being `Clone` + `Send` and owning all of its data.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct CompiledEquations {
    source: String,
}

/// Typed failures produced by compiling or running EEL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EelError {
    /// A syntax problem such as unbalanced braces or a malformed statement.
    ParseError(String),
    /// A call to a function that is not in the supported function set.
    UnknownIdentifier(String),
    /// A division (or remainder) by a literal/effective zero divisor.
    DivisionByZero,
    /// A `megabuf`/`gmegabuf` index outside `0..1_048_576`.
    MegabufOutOfBounds(i64),
    /// A `while`/`loop` body exceeded the 1_048_576 iteration hard bound.
    IterationLimitExceeded,
}

impl fmt::Display for EelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EelError::ParseError(s) => write!(f, "EEL parse error: {s}"),
            EelError::UnknownIdentifier(s) => write!(f, "unknown EEL identifier: {s}"),
            EelError::DivisionByZero => write!(f, "EEL division by zero"),
            EelError::MegabufOutOfBounds(i) => write!(f, "EEL megabuf index out of bounds: {i}"),
            EelError::IterationLimitExceeded => write!(f, "EEL iteration limit exceeded"),
        }
    }
}

impl std::error::Error for EelError {}

/// The fixed size of `megabuf` (and of `gmegabuf`).
pub const MEGABUF_SIZE: usize = 1_048_576;

/// Deterministic pseudo-random generator used by `rand`.
///
/// `next_f32` returns a value in `[0.0, 1.0)` and `next_int` returns the
/// truncated integer form. Two generators built from the same seed produce
/// identical sequences.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Rng {
    seed: u64,
}

impl Rng {
    /// Build a generator from a seed.
    pub fn new(seed: u64) -> Self {
        Rng { seed }
    }

    /// Next pseudo-random `f32` in `[0.0, 1.0)`.
    pub fn next_f32(&mut self) -> f32 {
        0.0
    }

    /// Next pseudo-random integer.
    pub fn next_int(&mut self) -> i32 {
        0
    }
}

/// Shared global state: the `gmegabuf` visible to every preset.
///
/// Cloning a `GlobalState` produces another handle to the *same* buffer, which
/// is how two [`VarPool`]s (two presets) share `gmegabuf`.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct GlobalState {
    seed: u64,
}

impl GlobalState {
    /// A fresh, empty global buffer.
    pub fn new() -> Self {
        GlobalState { seed: 0 }
    }
}

/// Per-frame global variables injected into every equation evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct Globals {
    pub time: f32,
    pub fps: f32,
    pub frame: f32,
    pub bass: f32,
    pub bass_att: f32,
    pub mid: f32,
    pub mid_att: f32,
    pub treb: f32,
    pub treb_att: f32,
    pub meshx: f32,
    pub meshy: f32,
    pub pixelsx: f32,
    pub pixelsy: f32,
    pub aspectx: f32,
    pub aspecty: f32,
}

impl Default for Globals {
    fn default() -> Self {
        Globals {
            time: 0.0,
            fps: 60.0,
            frame: 0.0,
            bass: 0.0,
            bass_att: 0.0,
            mid: 0.0,
            mid_att: 0.0,
            treb: 0.0,
            treb_att: 0.0,
            meshx: 0.0,
            meshy: 0.0,
            pixelsx: 0.0,
            pixelsy: 0.0,
            aspectx: 1.0,
            aspecty: 1.0,
        }
    }
}

/// Mutable variable pool with a stable key→slot map.
///
/// Use [`VarPool::set`] / [`VarPool::get`] to seed and inspect named variables
/// (`q1`, `t3`, `reg07`, user vars, ...). [`VarPool::snapshot_q`] records the
/// current `q1..q32` as the per-frame reset baseline captured from init
/// equations. [`VarPool::set_global_state`] attaches a shared [`GlobalState`].
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct VarPool {
    vars: HashMap<String, f32>,
    megabuf: HashMap<usize, f32>,
    global: Option<GlobalState>,
    rng: Rng,
}

impl VarPool {
    /// Empty pool with a deterministic default RNG.
    pub fn new() -> Self {
        VarPool::with_rng(Rng::new(0))
    }

    /// Pool whose `rand` is driven by `seed`.
    pub fn with_seed(seed: u64) -> Self {
        VarPool::with_rng(Rng::new(seed))
    }

    /// Pool driven by a caller-supplied [`Rng`].
    pub fn with_rng(rng: Rng) -> Self {
        VarPool {
            vars: HashMap::new(),
            megabuf: HashMap::new(),
            global: None,
            rng,
        }
    }

    /// Attach a (possibly shared) global buffer.
    pub fn set_global_state(&mut self, global: GlobalState) {
        self.global = Some(global);
    }

    /// Set a named variable.
    pub fn set(&mut self, name: &str, value: f32) {
        self.vars.insert(name.to_string(), value);
    }

    /// Read a named variable (0.0 when absent).
    pub fn get(&self, name: &str) -> f32 {
        self.vars.get(name).copied().unwrap_or(0.0)
    }

    /// Capture the current `q1..q32` as the per-frame reset baseline.
    pub fn snapshot_q(&mut self) {}

    /// Read a `megabuf` slot.
    pub fn megabuf(&self, _index: usize) -> f32 {
        0.0
    }

    /// Write a `megabuf` slot directly.
    pub fn set_megabuf(&mut self, _index: usize, _value: f32) {}

    /// Read a `gmegabuf` slot from the attached [`GlobalState`].
    pub fn gmegabuf(&self, _index: usize) -> f32 {
        0.0
    }

    /// Write a `gmegabuf` slot directly.
    pub fn set_gmegabuf(&mut self, _index: usize, _value: f32) {}
}

/// Compile EEL source (`version` is 1 or 2).
pub fn compile(src: &str, _version: u8) -> Result<CompiledEquations, EelError> {
    Ok(CompiledEquations {
        source: src.to_string(),
    })
}

/// Evaluate a compiled equation set for one frame.
pub fn run_frame(
    _eq: &CompiledEquations,
    _pool: &mut VarPool,
    _globals: &Globals,
) -> Result<(), EelError> {
    Ok(())
}

/// Evaluate a compiled equation set for one pixel/point.
pub fn run_pixel(
    _eq: &CompiledEquations,
    _pool: &mut VarPool,
    _x: f32,
    _y: f32,
    _rad: f32,
    _ang: f32,
) -> Result<(), EelError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! approx {
        ($got:expr, $want:expr) => {
            approx!($got, $want, "")
        };
        ($got:expr, $want:expr, $msg:expr) => {{
            let got = $got;
            let want = $want;
            assert!(
                (got - want).abs() < 1e-3,
                "expected {want}, got {got} (delta {}) -- {}",
                (got - want).abs(),
                $msg
            );
        }};
    }

    /// Compile `out = <expr>;` (v2) and run it on a fresh seeded pool.
    fn eval(expr: &str) -> f32 {
        eval_v(expr, 2)
    }

    fn eval_v(expr: &str, version: u8) -> f32 {
        let eq = compile(&format!("out = {expr};"), version).expect("expression compiles");
        let mut pool = VarPool::with_seed(0xC0FFEE);
        run_frame(&eq, &mut pool, &Globals::default()).expect("expression runs");
        pool.get("out")
    }

    /// Run an arbitrary statement list on a fresh seeded pool.
    fn run_src(src: &str) -> VarPool {
        run_src_v(src, 2)
    }

    fn run_src_v(src: &str, version: u8) -> VarPool {
        let eq = compile(src, version).expect("source compiles");
        let mut pool = VarPool::with_seed(0xC0FFEE);
        run_frame(&eq, &mut pool, &Globals::default()).expect("source runs");
        pool
    }

    #[test]
    fn basic_math_and_trig_functions_match_known_values() {
        approx!(eval("int(3.9)"), 3.0);
        approx!(eval("int(-3.9)"), -3.0);
        approx!(eval("floor(-3.9)"), -4.0);
        approx!(eval("floor(3.9)"), 3.0);
        approx!(eval("abs(-3.5)"), 3.5);
        approx!(eval("sin(0.5)"), 0.5_f32.sin());
        approx!(eval("cos(0.5)"), 0.5_f32.cos());
        approx!(eval("tan(0.5)"), 0.5_f32.tan());
        approx!(eval("asin(0.5)"), 0.5_f32.asin());
        approx!(eval("acos(0.5)"), 0.5_f32.acos());
        approx!(eval("atan(0.5)"), 0.5_f32.atan());
        approx!(eval("exp(1.0)"), 1.0_f32.exp());
        approx!(eval("sqr(3.0)"), 9.0);
        approx!(eval("sqrt(9.0)"), 3.0);
        approx!(eval("log(10.0)"), 10.0_f32.ln()); // natural log
        approx!(eval("log10(100.0)"), 2.0); // base-10 log
        approx!(eval("sign(-5.0)"), -1.0);
        approx!(eval("sign(0.0)"), 0.0);
        approx!(eval("sign(3.0)"), 1.0);
        approx!(eval("pow(2.0, 3.0)"), 8.0);
        approx!(eval("sigmoid(0.0)"), 0.5);
        approx!(eval("sigmoid(2.0)"), 1.0 / (1.0 + (-2.0_f32).exp()));
    }

    #[test]
    fn bitwise_and_exec_functions_match_known_values() {
        approx!(eval("bnot(0.0)"), -1.0);
        approx!(eval("bnot(5.0)"), -6.0);
        approx!(eval("bor(1.0, 2.0)"), 3.0);
        approx!(eval("bor(4.0, 3.0)"), 7.0);
        approx!(eval("band(6.0, 3.0)"), 2.0);
        approx!(eval("band(7.0, 4.0)"), 4.0);
        approx!(eval("atan2(1.0, 1.0)"), std::f32::consts::FRAC_PI_4);
        approx!(eval("min(2.0, 5.0)"), 2.0);
        approx!(eval("min(5.0, 2.0)"), 2.0);
        approx!(eval("max(2.0, 5.0)"), 5.0);

        let p = run_src("exec2(reg0 = 3, reg1 = 4);");
        approx!(p.get("reg0"), 3.0);
        approx!(p.get("reg1"), 4.0);
        let p = run_src("exec3(reg0 = 5, reg1 = 6, reg2 = 7);");
        approx!(p.get("reg0"), 5.0);
        approx!(p.get("reg1"), 6.0);
        approx!(p.get("reg2"), 7.0);
    }

    #[test]
    fn comparisons_and_equal_use_an_epsilon() {
        approx!(eval("equal(1.0, 1.00005)"), 1.0);
        approx!(eval("equal(1.0, 1.01)"), 0.0);
        approx!(eval("above(2.0, 1.0)"), 1.0);
        approx!(eval("below(1.0, 2.0)"), 1.0);
        approx!(eval("above(1.0, 1.00005)"), 0.0);
        approx!(eval("2.0 == 2.00005"), 1.0);
        approx!(eval("1.0 != 1.01"), 1.0);
        approx!(eval("1.0 < 1.00005"), 0.0); // equal within epsilon
        approx!(eval("2.0 > 1.0"), 1.0);
        approx!(eval("2.0 <= 2.00005"), 1.0);
        approx!(eval("1.0 >= 2.0"), 0.0);
    }

    #[test]
    fn rand_is_deterministic_and_version_dependent() {
        // v1: integer rand in [0, bound); v2: float rand in [0, bound).
        let v1 = compile("out = rand(100);", 1).expect("v1 compiles");
        let v2 = compile("out = rand(100);", 2).expect("v2 compiles");

        let run = |eq: &CompiledEquations, seed: u64| {
            let mut pool = VarPool::with_seed(seed);
            run_frame(eq, &mut pool, &Globals::default()).expect("runs");
            pool.get("out")
        };

        let a1 = run(&v1, 7);
        let b1 = run(&v1, 8);
        let a1_again = run(&v1, 7);
        assert_eq!(a1.fract(), 0.0, "v1 rand must be integral");
        assert!(a1 >= 0.0 && a1 < 100.0, "v1 rand in range, got {a1}");
        assert_eq!(a1, a1_again, "same seed is deterministic");
        assert_ne!(a1, b1, "different seeds differ");

        let a2 = run(&v2, 7);
        let b2 = run(&v2, 8);
        assert!(a2 >= 0.0 && a2 < 100.0, "v2 rand in range, got {a2}");
        assert_ne!(a1, a2, "v1 is integer, v2 is float for the same seed");
        assert_ne!(a2, b2, "different seeds differ for v2");

        // The injectable RNG itself is deterministic and seed-sensitive.
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        let mut r3 = Rng::new(43);
        assert_eq!(r1.next_f32(), r2.next_f32());
        assert_ne!(r1.next_f32(), r3.next_f32());
        let mut r1 = Rng::new(42);
        assert_eq!(r1.next_int(), Rng::new(42).next_int());
    }

    #[test]
    fn q_resets_each_frame_while_user_vars_persist() {
        let eq = compile("q1 = q1 + 1; warm = warm + 1;", 2).expect("compiles");
        let mut pool = VarPool::with_seed(1);
        pool.set("q1", 5.0); // value captured from init equations
        pool.snapshot_q();

        run_frame(&eq, &mut pool, &Globals::default()).expect("frame 1");
        run_frame(&eq, &mut pool, &Globals::default()).expect("frame 2");

        approx!(pool.get("q1"), 6.0); // reset to 5 at each frame start, then +1
        approx!(pool.get("warm"), 2.0); // user var carries across frames
    }

    #[test]
    fn t_values_carry_from_init_into_frame_and_point_and_regs_are_scratch() {
        let eq = compile("out = t1 + reg00 * 2; reg01 = reg00 + 1;", 2).expect("compiles");
        let mut pool = VarPool::with_seed(1);
        pool.set("t1", 3.0);
        pool.set("reg00", 7.0);

        run_frame(&eq, &mut pool, &Globals::default()).expect("frame");
        approx!(pool.get("t1"), 3.0, "t values are not reset each frame");
        approx!(pool.get("out"), 17.0, "t1 + reg00 * 2 == 17");
        approx!(pool.get("reg01"), 8.0, "registers are shared scratch");
    }

    #[test]
    fn megabuf_reads_writes_and_index_bounds() {
        // Writes performed by equations land in the pool's megabuf.
        let write = compile("megabuf[3] = 9;", 2).expect("compiles");
        let mut pool = VarPool::with_seed(1);
        run_frame(&write, &mut pool, &Globals::default()).expect("runs");
        approx!(pool.megabuf(3), 9.0);

        // Reads from a directly-seeded slot are visible to equations.
        pool.set_megabuf(4, 2.5);
        let read = compile("out = megabuf[4];", 2).expect("compiles");
        run_frame(&read, &mut pool, &Globals::default()).expect("runs");
        approx!(pool.get("out"), 2.5);

        // Indices outside 0..MEGABUF_SIZE are typed errors, never panics.
        let hi = compile("megabuf[1048576] = 1;", 2).expect("compiles");
        let res = run_frame(&hi, &mut VarPool::new(), &Globals::default());
        assert!(matches!(res, Err(EelError::MegabufOutOfBounds(1048576))));

        let lo = compile("out = megabuf[-1];", 2).expect("compiles");
        let res = run_frame(&lo, &mut VarPool::new(), &Globals::default());
        assert!(matches!(res, Err(EelError::MegabufOutOfBounds(-1))));
    }

    #[test]
    fn gmegabuf_is_shared_across_pools() {
        let global = GlobalState::new();
        let mut p1 = VarPool::with_seed(1);
        p1.set_global_state(global.clone());
        let mut p2 = VarPool::with_seed(2);
        p2.set_global_state(global);

        let write = compile("gmegabuf[10] = 77;", 2).expect("compiles");
        run_frame(&write, &mut p1, &Globals::default()).expect("p1 writes");
        approx!(p2.gmegabuf(10), 77.0, "p2 sees p1's global write");

        let read = compile("out = gmegabuf[10];", 2).expect("compiles");
        run_frame(&read, &mut p2, &Globals::default()).expect("p2 reads");
        approx!(p2.get("out"), 77.0);

        approx!(p1.megabuf(10), 0.0, "megabuf is not gmegabuf");
    }

    #[test]
    fn if_while_and_loop_control_flow_evaluates_correctly() {
        approx!(eval("if(1, 5, 9)"), 5.0);
        approx!(eval("if(0, 5, 9)"), 9.0);

        let p = run_src("x = 0; if (1) { x = 3; } else { x = 9; } out = x;");
        approx!(p.get("out"), 3.0);
        let p = run_src("x = 0; if (0) { x = 3; } else { x = 9; } out = x;");
        approx!(p.get("out"), 9.0);

        let p = run_src("i = 0; while (i < 5) { i = i + 1; } out = i;");
        approx!(p.get("out"), 5.0);

        let p = run_src("s = 0; loop(4) { s = s + 1; } out = s;");
        approx!(p.get("out"), 4.0);
    }

    #[test]
    fn loops_and_while_over_the_hard_bound_return_a_typed_error() {
        let loop_eq = compile("loop(1048577) {}", 2).expect("compiles");
        let res = run_frame(&loop_eq, &mut VarPool::new(), &Globals::default());
        assert!(matches!(res, Err(EelError::IterationLimitExceeded)));

        let while_eq = compile("i = 0; while (1) { i = i + 1; }", 2).expect("compiles");
        let res = run_frame(&while_eq, &mut VarPool::new(), &Globals::default());
        assert!(matches!(res, Err(EelError::IterationLimitExceeded)));
    }

    #[test]
    fn hostile_input_returns_typed_errors_without_panicking() {
        // Unbalanced braces are a parse error.
        let res = compile("if (1) { x = 2;", 2);
        assert!(matches!(res, Err(EelError::ParseError(_))));

        // Unknown function calls are a typed identifier error.
        let res = compile("out = frobnicate(1);", 2);
        assert!(matches!(res, Err(EelError::UnknownIdentifier(_))));

        // Division by zero is a runtime typed error, not a panic/NaN.
        let div = compile("out = 5 / 0;", 2).expect("compiles");
        let res = run_frame(&div, &mut VarPool::new(), &Globals::default());
        assert!(matches!(res, Err(EelError::DivisionByZero)));

        let rem = compile("out = 5 % 0;", 2).expect("compiles");
        let res = run_frame(&rem, &mut VarPool::new(), &Globals::default());
        assert!(matches!(res, Err(EelError::DivisionByZero)));

        // No input, however malformed, may panic the compiler.
        let panic_free = std::panic::catch_unwind(|| compile("}}} $$$ ((( &&&", 2));
        assert!(panic_free.is_ok(), "compile must not panic on hostile input");
    }

    #[test]
    fn globals_are_injected_into_every_run() {
        let eq = compile(
            "sum = time + frame + bass + bass_att + mid + mid_att + treb + treb_att + meshx + meshy + pixelsx + pixelsy + aspectx + aspecty;",
            2,
        )
        .expect("compiles");
        let globals = Globals {
            time: 1.0,
            frame: 2.0,
            bass: 3.0,
            bass_att: 4.0,
            mid: 5.0,
            mid_att: 6.0,
            treb: 7.0,
            treb_att: 8.0,
            meshx: 9.0,
            meshy: 10.0,
            pixelsx: 11.0,
            pixelsy: 12.0,
            aspectx: 13.0,
            aspecty: 14.0,
            fps: 60.0,
        };
        let mut pool = VarPool::with_seed(1);
        run_frame(&eq, &mut pool, &globals).expect("runs");
        approx!(pool.get("sum"), 105.0, "sum of all injected globals");
    }

    #[test]
    fn run_pixel_injects_pixel_variables_and_keeps_frame_pool() {
        let eq = compile("out = x + y + rad + ang + q1;", 2).expect("compiles");
        let mut pool = VarPool::with_seed(1);
        pool.set("q1", 2.0);
        run_pixel(&eq, &mut pool, 1.0, 2.0, 3.0, 4.0).expect("runs");
        approx!(pool.get("out"), 12.0, "1 + 2 + 3 + 4 + q1(2)");
    }

    #[test]
    fn deterministic_output_for_a_real_preset_per_frame_fixture() {
        // Representative per-frame equations from a MilkDrop v2 preset: a q
        // accumulator, a trig-derived q, a persistent user var, megabuf state
        // and a random draw. With a fixed seed the result is reproducible.
        const FIXTURE: &str = "\
q1 = q1 + 0.5;
q2 = sin(1.0) * 2.0;
acc = acc + q1;
megabuf[10] = megabuf[10] + 1.0;
j = rand(1000);
out = q1 + q2 + acc + megabuf[10];
";
        let eq = compile(FIXTURE, 2).expect("fixture compiles");

        let run_once = || {
            let mut pool = VarPool::with_seed(0xBEEF);
            pool.set("q1", 1.0);
            pool.snapshot_q();
            run_frame(&eq, &mut pool, &Globals::default()).expect("fixture runs");
            pool
        };

        let a = run_once();
        let b = run_once();
        approx!(a.get("q1"), 1.5);
        approx!(a.get("q2"), 2.0 * 1.0_f32.sin());
        approx!(a.get("acc"), 1.5, "persistent user var seeded at 0");
        approx!(a.megabuf(10), 1.0);
        approx!(a.get("out"), 1.5 + 2.0 * 1.0_f32.sin() + 1.5 + 1.0);
        approx!(a.get("j"), b.get("j"), "rand is deterministic with a seed");
        approx!(a.get("out"), b.get("out"), "whole fixture is deterministic");
    }
}
