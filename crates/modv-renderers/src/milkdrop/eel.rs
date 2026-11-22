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
// The fixed RED test suite spells out ranges as `x >= lo && x < hi` (see
// `rand_is_deterministic_and_version_dependent`), which trips clippy's
// `manual_range_contains` under `-D warnings`. Those assertions are part of
// the TDD contract and may not be edited, so the lint is allowed here; no
// production expression in this module uses that form.
#![allow(clippy::manual_range_contains)]

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Comparison epsilon: values within this distance compare as equal.
const EPS: f32 = 1e-4;

// All EEL comparisons (operators and `equal`/`above`/`below`) share one
// epsilon definition. Keeping the predicates here means the infix operators
// and their builtin-function spellings can never drift apart.

#[inline]
fn eel_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPS
}

#[inline]
fn eel_lt(a: f32, b: f32) -> bool {
    b - a > EPS
}

#[inline]
fn eel_gt(a: f32, b: f32) -> bool {
    a - b > EPS
}

#[inline]
fn eel_le(a: f32, b: f32) -> bool {
    a - b <= EPS
}

#[inline]
fn eel_ge(a: f32, b: f32) -> bool {
    b - a <= EPS
}

#[inline]
fn bool_f32(cond: bool) -> f32 {
    if cond { 1.0 } else { 0.0 }
}

/// Compiled, owned form of one EEL equation set.
///
/// Produced once by [`compile`] and reused for every evaluation. The exact
/// representation (AST or bytecode) is an implementation detail; callers only
/// rely on it being `Clone` + `Send` and owning all of its data.
#[derive(Clone, Debug, Default)]
pub struct CompiledEquations {
    program: Vec<Stmt>,
    version: u8,
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
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Build a generator from a seed.
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    /// Advance the splitmix64 state and return one 64-bit word.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Next pseudo-random `f32` in `[0.0, 1.0)` (24 bits of mantissa).
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / ((1u64 << 24) as f32)
    }

    /// Next pseudo-random integer.
    pub fn next_int(&mut self) -> i32 {
        self.next_u64() as i32
    }
}

/// Shared global state: the `gmegabuf` visible to every preset.
///
/// Cloning a `GlobalState` produces another handle to the *same* buffer, which
/// is how two [`VarPool`]s (two presets) share `gmegabuf`.
#[derive(Clone, Debug)]
pub struct GlobalState {
    buf: Arc<Mutex<Vec<f32>>>,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalState {
    /// A fresh, empty global buffer. Clones share the same storage.
    pub fn new() -> Self {
        GlobalState {
            buf: Arc::new(Mutex::new(vec![0.0; MEGABUF_SIZE])),
        }
    }

    fn read(&self, index: usize) -> f32 {
        if index >= MEGABUF_SIZE {
            return 0.0;
        }
        self.buf.lock().map(|b| b[index]).unwrap_or(0.0)
    }

    fn write(&self, index: usize, value: f32) {
        if index >= MEGABUF_SIZE {
            return;
        }
        if let Ok(mut b) = self.buf.lock() {
            b[index] = value;
        }
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

impl Globals {
    /// Inject every built-in global into `pool` before a frame runs.
    ///
    /// Kept next to the struct so the set of exposed global names has a single
    /// definition; the old form repeated fifteen `pool.set(...)` calls at the
    /// call site.
    fn inject_into(&self, pool: &mut VarPool) {
        let entries = [
            ("time", self.time),
            ("fps", self.fps),
            ("frame", self.frame),
            ("bass", self.bass),
            ("bass_att", self.bass_att),
            ("mid", self.mid),
            ("mid_att", self.mid_att),
            ("treb", self.treb),
            ("treb_att", self.treb_att),
            ("meshx", self.meshx),
            ("meshy", self.meshy),
            ("pixelsx", self.pixelsx),
            ("pixelsy", self.pixelsy),
            ("aspectx", self.aspectx),
            ("aspecty", self.aspecty),
        ];
        for (name, value) in entries {
            pool.set(name, value);
        }
    }
}

/// Mutable variable pool with a stable key→slot map.
///
/// Use [`VarPool::set`] / [`VarPool::get`] to seed and inspect named variables
/// (`q1`, `t3`, `reg07`, user vars, ...). [`VarPool::snapshot_q`] records the
/// current `q1..q32` as the per-frame reset baseline captured from init
/// equations. [`VarPool::set_global_state`] attaches a shared [`GlobalState`].
///
/// # Cheap, copy-on-write clones
///
/// `megabuf` is a fixed 1 048 576-float array (4 MiB). It is held behind an
/// [`Arc`] so cloning a pool is cheap: the first write through [`VarPool::set_megabuf`]
/// (via [`Arc::make_mut`]) forks a private copy, so clones keep independent
/// megabufs *by value* without an eager memcpy. This matters for the wave/shape
/// overlay path, which forks a frame pool once per section to evaluate point
/// equations on a scratch copy; the fork no longer copies 4 MiB per point pass.
#[derive(Clone, Debug)]
pub struct VarPool {
    vars: HashMap<String, f32>,
    q_baseline: Option<HashMap<String, f32>>,
    megabuf: Arc<Vec<f32>>,
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
            q_baseline: None,
            megabuf: Arc::new(vec![0.0; MEGABUF_SIZE]),
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
    pub fn snapshot_q(&mut self) {
        let mut snap = HashMap::with_capacity(32);
        for i in 1..=32 {
            let key = format!("q{i}");
            let value = self.vars.get(&key).copied().unwrap_or(0.0);
            snap.insert(key, value);
        }
        self.q_baseline = Some(snap);
    }

    /// Restore `q1..q32` to a previously captured baseline (if any).
    fn reset_q(&mut self) {
        if let Some(snap) = &self.q_baseline {
            for (key, value) in snap.iter() {
                self.vars.insert(key.clone(), *value);
            }
        }
    }

    /// Read a `megabuf` slot.
    pub fn megabuf(&self, index: usize) -> f32 {
        self.megabuf.get(index).copied().unwrap_or(0.0)
    }

    /// Write a `megabuf` slot directly.
    ///
    /// Forks the shared buffer on first write (copy-on-write) so a [`Clone`]d
    /// pool keeps its own megabuf values without an eager 4 MiB copy at clone
    /// time.
    pub fn set_megabuf(&mut self, index: usize, value: f32) {
        if index < MEGABUF_SIZE {
            Arc::make_mut(&mut self.megabuf)[index] = value;
        }
    }

    /// Read a `gmegabuf` slot from the attached [`GlobalState`].
    pub fn gmegabuf(&self, index: usize) -> f32 {
        self.global.as_ref().map(|g| g.read(index)).unwrap_or(0.0)
    }

    /// Write a `gmegabuf` slot directly.
    pub fn set_gmegabuf(&mut self, index: usize, value: f32) {
        if let Some(g) = &self.global {
            g.write(index, value);
        }
    }
}

impl Default for VarPool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f32),
    Ident(String),
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
    Amp,
    Pipe,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Newline,
    Eof,
}

/// Map a single operator byte to `(bare token, doubled token, second byte)`.
///
/// The doubled form is emitted only when the next byte matches `second`: this
/// covers both the compound assignments (`+=`, `-=`, `==`, ...) and the
/// doubled logical/bitwise operators (`&&`, `||`) with one table.
fn operator_tokens(c: u8) -> Option<(Tok, Tok, u8)> {
    Some(match c {
        b'=' => (Tok::Assign, Tok::Eq, b'='),
        b'!' => (Tok::Bang, Tok::Ne, b'='),
        b'<' => (Tok::Lt, Tok::Le, b'='),
        b'>' => (Tok::Gt, Tok::Ge, b'='),
        b'&' => (Tok::Amp, Tok::AndAnd, b'&'),
        b'|' => (Tok::Pipe, Tok::OrOr, b'|'),
        b'+' => (Tok::Plus, Tok::AddAssign, b'='),
        b'-' => (Tok::Minus, Tok::SubAssign, b'='),
        b'*' => (Tok::Star, Tok::MulAssign, b'='),
        b'/' => (Tok::Slash, Tok::DivAssign, b'='),
        b'%' => (Tok::Percent, Tok::ModAssign, b'='),
        _ => return None,
    })
}

/// Tokenize EEL source. Newlines are separators at top level (and inside
/// braces) but are skipped inside parentheses/brackets so expressions may be
/// wrapped across lines.
fn lex(src: &str) -> Result<Vec<Tok>, EelError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut nesting = 0i32;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' => i += 1,
            b'\n' => {
                if nesting == 0 {
                    out.push(Tok::Newline);
                }
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                if i + 1 >= bytes.len() {
                    return Err(EelError::ParseError("unterminated comment".into()));
                }
                i += 2;
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                let mut saw_dot = false;
                while i < bytes.len() {
                    let d = bytes[i];
                    if d.is_ascii_digit() {
                        i += 1;
                    } else if d == b'.' && !saw_dot {
                        saw_dot = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    let exp_start = i;
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    let digits = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i == digits {
                        i = exp_start;
                    }
                }
                let text = std::str::from_utf8(&bytes[start..i])
                    .map_err(|_| EelError::ParseError("bad number literal".into()))?;
                let value: f32 = text
                    .parse()
                    .map_err(|_| EelError::ParseError(format!("bad number literal: {text}")))?;
                out.push(Tok::Num(value));
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let text = std::str::from_utf8(&bytes[start..i])
                    .map_err(|_| EelError::ParseError("bad identifier".into()))?;
                out.push(Tok::Ident(text.to_string()));
            }
            c @ (b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'+' | b'-' | b'*' | b'/' | b'%') => {
                // `//` and `/*` were handled above, so a bare `/` is division.
                let (bare, doubled, second) =
                    operator_tokens(c).expect("operator_tokens covers every byte matched here");
                if bytes.get(i + 1) == Some(&second) {
                    out.push(doubled);
                    i += 2;
                } else {
                    out.push(bare);
                    i += 1;
                }
            }
            b'(' => {
                nesting += 1;
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                nesting -= 1;
                out.push(Tok::RParen);
                i += 1;
            }
            b'[' => {
                nesting += 1;
                out.push(Tok::LBracket);
                i += 1;
            }
            b']' => {
                nesting -= 1;
                out.push(Tok::RBracket);
                i += 1;
            }
            b'{' => {
                out.push(Tok::LBrace);
                i += 1;
            }
            b'}' => {
                out.push(Tok::RBrace);
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b';' => {
                out.push(Tok::Semi);
                i += 1;
            }
            _ => {
                return Err(EelError::ParseError(format!(
                    "unexpected character '{}'",
                    c as char
                )));
            }
        }
    }
    out.push(Tok::Eof);
    Ok(out)
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    BitAnd,
    BitOr,
    And,
    Or,
}

#[derive(Clone, Copy, Debug)]
enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Clone, Debug)]
enum Expr {
    Num(f32),
    Var(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(AssignOp, String, Box<Expr>),
    Index {
        global: bool,
        index: Box<Expr>,
    },
    IndexAssign {
        global: bool,
        index: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
    },
    Call {
        func: Func,
        args: Vec<Expr>,
    },
}

#[derive(Clone, Debug)]
enum Stmt {
    Expr(Expr),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Loop {
        count: Expr,
        body: Vec<Stmt>,
    },
}

/// Every builtin EEL function, resolved once at compile time.
///
/// This enum is the single source of truth for both the supported name set
/// (compile-time validation) and runtime dispatch, replacing two parallel
/// string `match`es that could drift apart. It also lets the evaluator branch
/// on a `Copy` tag instead of re-matching a `String` for every call on the hot
/// path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Func {
    Int,
    Floor,
    Abs,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Exp,
    Sqr,
    Sqrt,
    Log,
    Log10,
    Sign,
    Rand,
    Bnot,
    Atan2,
    Pow,
    Min,
    Max,
    Sigmoid,
    Bor,
    Band,
    Equal,
    Above,
    Below,
    If,
    Exec2,
    Exec3,
}

impl Func {
    /// Look up a builtin by its EEL source spelling.
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "int" => Func::Int,
            "floor" => Func::Floor,
            "abs" => Func::Abs,
            "sin" => Func::Sin,
            "cos" => Func::Cos,
            "tan" => Func::Tan,
            "asin" => Func::Asin,
            "acos" => Func::Acos,
            "atan" => Func::Atan,
            "exp" => Func::Exp,
            "sqr" => Func::Sqr,
            "sqrt" => Func::Sqrt,
            "log" => Func::Log,
            "log10" => Func::Log10,
            "sign" => Func::Sign,
            "rand" => Func::Rand,
            "bnot" => Func::Bnot,
            "atan2" => Func::Atan2,
            "pow" => Func::Pow,
            "min" => Func::Min,
            "max" => Func::Max,
            "sigmoid" => Func::Sigmoid,
            "bor" => Func::Bor,
            "band" => Func::Band,
            "equal" => Func::Equal,
            "above" => Func::Above,
            "below" => Func::Below,
            "if" => Func::If,
            "exec2" => Func::Exec2,
            "exec3" => Func::Exec3,
            _ => return None,
        })
    }

    /// Minimum number of arguments this builtin accepts.
    fn min_args(self) -> usize {
        match self {
            Func::Int
            | Func::Floor
            | Func::Abs
            | Func::Sin
            | Func::Cos
            | Func::Tan
            | Func::Asin
            | Func::Acos
            | Func::Atan
            | Func::Exp
            | Func::Sqr
            | Func::Sqrt
            | Func::Log
            | Func::Log10
            | Func::Sign
            | Func::Rand
            | Func::Bnot
            | Func::Sigmoid => 1,
            Func::Atan2
            | Func::Pow
            | Func::Bor
            | Func::Band
            | Func::Equal
            | Func::Above
            | Func::Below
            | Func::Min
            | Func::Max
            | Func::Exec2 => 2,
            Func::If | Func::Exec3 => 3,
        }
    }
}

/// Resolve a call's source name to a builtin and validate its argument count.
fn check_function(name: &str, argc: usize) -> Result<Func, EelError> {
    let func =
        Func::from_name(name).ok_or_else(|| EelError::UnknownIdentifier(name.to_string()))?;
    let min_args = func.min_args();
    if argc < min_args {
        return Err(EelError::ParseError(format!(
            "{name} expects at least {min_args} argument(s)"
        )));
    }
    Ok(func)
}

// ---------------------------------------------------------------------------
// Recursive-descent parser
// ---------------------------------------------------------------------------

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn new(toks: Vec<Tok>) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        self.toks.get(self.pos).unwrap_or(&Tok::Eof)
    }

    fn peek2(&self) -> &Tok {
        self.toks.get(self.pos + 1).unwrap_or(&Tok::Eof)
    }

    fn is_ident(&self, name: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == name)
    }

    fn skip_seps(&mut self) {
        while matches!(self.peek(), Tok::Semi | Tok::Newline) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, want: &Tok) -> Result<(), EelError> {
        if self.peek() == want {
            self.pos += 1;
            Ok(())
        } else {
            Err(EelError::ParseError(format!(
                "expected {want:?}, found {:?}",
                self.peek()
            )))
        }
    }

    fn parse_program(&mut self) -> Result<Vec<Stmt>, EelError> {
        let mut stmts = Vec::new();
        loop {
            self.skip_seps();
            if matches!(self.peek(), Tok::Eof) {
                break;
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, EelError> {
        self.expect(&Tok::LBrace)?;
        let mut stmts = Vec::new();
        loop {
            self.skip_seps();
            match self.peek() {
                Tok::RBrace => {
                    self.pos += 1;
                    break;
                }
                Tok::Eof => return Err(EelError::ParseError("unbalanced braces".into())),
                _ => stmts.push(self.parse_stmt()?),
            }
        }
        Ok(stmts)
    }

    /// Parse a control-flow body: either a braced `{ ... }` block or, as real
    /// MilkDrop 2 presets commonly write it, a single brace-less statement.
    /// Both forms yield a `Vec<Stmt>` so the evaluator can reuse `exec_block`.
    fn parse_body(&mut self) -> Result<Vec<Stmt>, EelError> {
        self.skip_seps();
        if matches!(self.peek(), Tok::LBrace) {
            self.parse_block()
        } else {
            Ok(vec![self.parse_stmt()?])
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, EelError> {
        if self.is_ident("if") && matches!(self.peek2(), Tok::LParen) {
            self.pos += 2; // consume `if` `(`
            let cond = self.parse_expr()?;
            self.expect(&Tok::RParen)?;
            let then_body = self.parse_body()?;
            // A brace-less body leaves its trailing `;` unconsumed, so skip
            // separators before deciding whether an `else` follows. This also
            // binds a dangling `else` to the nearest (innermost) `if`.
            self.skip_seps();
            let else_body = if self.is_ident("else") {
                self.pos += 1;
                self.parse_body()?
            } else {
                Vec::new()
            };
            return Ok(Stmt::If {
                cond,
                then_body,
                else_body,
            });
        }
        if self.is_ident("while") && matches!(self.peek2(), Tok::LParen) {
            self.pos += 2;
            let cond = self.parse_expr()?;
            self.expect(&Tok::RParen)?;
            let body = self.parse_body()?;
            return Ok(Stmt::While { cond, body });
        }
        if self.is_ident("loop") && matches!(self.peek2(), Tok::LParen) {
            self.pos += 2;
            let count = self.parse_expr()?;
            self.expect(&Tok::RParen)?;
            let body = self.parse_body()?;
            return Ok(Stmt::Loop { count, body });
        }
        Ok(Stmt::Expr(self.parse_expr()?))
    }

    fn parse_expr(&mut self) -> Result<Expr, EelError> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<Expr, EelError> {
        let left = self.parse_or()?;
        let op = match self.peek() {
            Tok::Assign => Some(AssignOp::Set),
            Tok::AddAssign => Some(AssignOp::Add),
            Tok::SubAssign => Some(AssignOp::Sub),
            Tok::MulAssign => Some(AssignOp::Mul),
            Tok::DivAssign => Some(AssignOp::Div),
            Tok::ModAssign => Some(AssignOp::Mod),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let value = Box::new(self.parse_assign()?);
            match left {
                Expr::Var(name) => Ok(Expr::Assign(op, name, value)),
                Expr::Index { global, index } => Ok(Expr::IndexAssign {
                    global,
                    index,
                    op,
                    value,
                }),
                _ => Err(EelError::ParseError("invalid assignment target".into())),
            }
        } else {
            Ok(left)
        }
    }

    fn parse_or(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_and()?;
        while matches!(self.peek(), Tok::OrOr) {
            self.pos += 1;
            let r = self.parse_and()?;
            e = Expr::Binary(BinOp::Or, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_and(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_bitor()?;
        while matches!(self.peek(), Tok::AndAnd) {
            self.pos += 1;
            let r = self.parse_bitor()?;
            e = Expr::Binary(BinOp::And, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_bitor(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_bitand()?;
        while matches!(self.peek(), Tok::Pipe) {
            self.pos += 1;
            let r = self.parse_bitand()?;
            e = Expr::Binary(BinOp::BitOr, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_bitand(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_equality()?;
        while matches!(self.peek(), Tok::Amp) {
            self.pos += 1;
            let r = self.parse_equality()?;
            e = Expr::Binary(BinOp::BitAnd, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_equality(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_comparison()?;
        loop {
            let op = match self.peek() {
                Tok::Eq => BinOp::Eq,
                Tok::Ne => BinOp::Ne,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_comparison()?;
            e = Expr::Binary(op, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_comparison(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Tok::Lt => BinOp::Lt,
                Tok::Gt => BinOp::Gt,
                Tok::Le => BinOp::Le,
                Tok::Ge => BinOp::Ge,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_add()?;
            e = Expr::Binary(op, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_add(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_mul()?;
            e = Expr::Binary(op, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_mul(&mut self) -> Result<Expr, EelError> {
        let mut e = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_unary()?;
            e = Expr::Binary(op, Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_unary(&mut self) -> Result<Expr, EelError> {
        match self.peek() {
            Tok::Minus => {
                self.pos += 1;
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.parse_unary()?)))
            }
            Tok::Bang => {
                self.pos += 1;
                Ok(Expr::Unary(UnOp::Not, Box::new(self.parse_unary()?)))
            }
            Tok::Plus => {
                self.pos += 1;
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, EelError> {
        match self.peek().clone() {
            Tok::Num(v) => {
                self.pos += 1;
                Ok(Expr::Num(v))
            }
            Tok::LParen => {
                self.pos += 1;
                let e = self.parse_expr()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Tok::Ident(name) => {
                self.pos += 1;
                if matches!(self.peek(), Tok::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Tok::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek(), Tok::Comma) {
                                self.pos += 1;
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect(&Tok::RParen)?;
                    let func = check_function(&name, args.len())?;
                    Ok(Expr::Call { func, args })
                } else if matches!(self.peek(), Tok::LBracket) {
                    self.pos += 1;
                    let index = Box::new(self.parse_expr()?);
                    self.expect(&Tok::RBracket)?;
                    if name == "megabuf" || name == "gmegabuf" {
                        Ok(Expr::Index {
                            global: name == "gmegabuf",
                            index,
                        })
                    } else {
                        Err(EelError::ParseError(format!(
                            "cannot index identifier {name}"
                        )))
                    }
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(EelError::ParseError(format!("unexpected token {other:?}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

const ITERATION_LIMIT: u64 = MEGABUF_SIZE as u64;

struct EvalCtx<'a> {
    pool: &'a mut VarPool,
    version: u8,
    iterations: u64,
}

impl EvalCtx<'_> {
    fn bump(&mut self) -> Result<(), EelError> {
        if self.iterations >= ITERATION_LIMIT {
            Err(EelError::IterationLimitExceeded)
        } else {
            self.iterations += 1;
            Ok(())
        }
    }
}

/// Convert an evaluated index to a checked buffer slot.
fn checked_index(value: f32) -> Result<usize, EelError> {
    let i = value as i64;
    if i < 0 || i >= MEGABUF_SIZE as i64 {
        Err(EelError::MegabufOutOfBounds(i))
    } else {
        Ok(i as usize)
    }
}

#[inline]
fn checked_div(a: f32, b: f32) -> Result<f32, EelError> {
    if b == 0.0 {
        Err(EelError::DivisionByZero)
    } else {
        Ok(a / b)
    }
}

#[inline]
fn checked_rem(a: f32, b: f32) -> Result<f32, EelError> {
    if b == 0.0 {
        Err(EelError::DivisionByZero)
    } else {
        Ok(a % b)
    }
}

fn apply_assign(op: AssignOp, cur: f32, rhs: f32) -> Result<f32, EelError> {
    match op {
        AssignOp::Set => Ok(rhs),
        AssignOp::Add => Ok(cur + rhs),
        AssignOp::Sub => Ok(cur - rhs),
        AssignOp::Mul => Ok(cur * rhs),
        AssignOp::Div => checked_div(cur, rhs),
        AssignOp::Mod => checked_rem(cur, rhs),
    }
}

fn eval_expr(expr: &Expr, ctx: &mut EvalCtx<'_>) -> Result<f32, EelError> {
    match expr {
        Expr::Num(v) => Ok(*v),
        Expr::Var(name) => Ok(ctx.pool.get(name)),
        Expr::Unary(op, e) => {
            let v = eval_expr(e, ctx)?;
            match op {
                UnOp::Neg => Ok(-v),
                UnOp::Not => Ok(bool_f32(v == 0.0)),
            }
        }
        Expr::Binary(op, l, r) => {
            if matches!(op, BinOp::And | BinOp::Or) {
                let a = eval_expr(l, ctx)?;
                match op {
                    BinOp::And if a == 0.0 => return Ok(0.0),
                    BinOp::Or if a != 0.0 => return Ok(1.0),
                    _ => {}
                }
                let b = eval_expr(r, ctx)?;
                return Ok(bool_f32(b != 0.0));
            }
            let a = eval_expr(l, ctx)?;
            let b = eval_expr(r, ctx)?;
            match op {
                BinOp::Add => Ok(a + b),
                BinOp::Sub => Ok(a - b),
                BinOp::Mul => Ok(a * b),
                BinOp::Div => checked_div(a, b),
                BinOp::Mod => checked_rem(a, b),
                BinOp::Eq => Ok(bool_f32(eel_eq(a, b))),
                BinOp::Ne => Ok(bool_f32(!eel_eq(a, b))),
                BinOp::Lt => Ok(bool_f32(eel_lt(a, b))),
                BinOp::Gt => Ok(bool_f32(eel_gt(a, b))),
                BinOp::Le => Ok(bool_f32(eel_le(a, b))),
                BinOp::Ge => Ok(bool_f32(eel_ge(a, b))),
                BinOp::BitAnd => Ok(((a as i32) & (b as i32)) as f32),
                BinOp::BitOr => Ok(((a as i32) | (b as i32)) as f32),
                BinOp::And | BinOp::Or => unreachable!("handled above"),
            }
        }
        Expr::Assign(op, name, value) => {
            let rhs = eval_expr(value, ctx)?;
            let new = apply_assign(*op, ctx.pool.get(name), rhs)?;
            ctx.pool.set(name, new);
            Ok(new)
        }
        Expr::Index { global, index } => {
            let idx = checked_index(eval_expr(index, ctx)?)?;
            if *global {
                Ok(ctx.pool.gmegabuf(idx))
            } else {
                Ok(ctx.pool.megabuf(idx))
            }
        }
        Expr::IndexAssign {
            global,
            index,
            op,
            value,
        } => {
            let idx = checked_index(eval_expr(index, ctx)?)?;
            let rhs = eval_expr(value, ctx)?;
            let cur = if *global {
                ctx.pool.gmegabuf(idx)
            } else {
                ctx.pool.megabuf(idx)
            };
            let new = apply_assign(*op, cur, rhs)?;
            if *global {
                ctx.pool.set_gmegabuf(idx, new);
            } else {
                ctx.pool.set_megabuf(idx, new);
            }
            Ok(new)
        }
        Expr::Call { func, args } => eval_call(*func, args, ctx),
    }
}

fn eval_call(func: Func, args: &[Expr], ctx: &mut EvalCtx<'_>) -> Result<f32, EelError> {
    if func == Func::If {
        // Ternary form: evaluate only the selected branch.
        let cond = eval_expr(&args[0], ctx)?;
        return if cond != 0.0 {
            eval_expr(&args[1], ctx)
        } else {
            eval_expr(&args[2], ctx)
        };
    }
    // Keep eager evaluation of every argument: `exec2`/`exec3` rely on the
    // side effects of their arguments, and EEL evaluates extra arguments too.
    let mut v = [0.0f32; 8];
    for (i, arg) in args.iter().enumerate() {
        v[i.min(7)] = eval_expr(arg, ctx)?;
    }
    let a = v[0];
    let b = v[1];
    Ok(match func {
        Func::Int => a.trunc(),
        Func::Floor => a.floor(),
        Func::Abs => a.abs(),
        Func::Sin => a.sin(),
        Func::Cos => a.cos(),
        Func::Tan => a.tan(),
        Func::Asin => a.asin(),
        Func::Acos => a.acos(),
        Func::Atan => a.atan(),
        Func::Exp => a.exp(),
        Func::Sqr => a * a,
        Func::Sqrt => a.sqrt(),
        Func::Log => a.ln(),
        Func::Log10 => a.log10(),
        Func::Sign => {
            if a > 0.0 {
                1.0
            } else if a < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        Func::Rand => {
            let r = ctx.pool.rng.next_f32();
            if ctx.version >= 2 {
                r * a
            } else {
                (r * a) as i32 as f32
            }
        }
        Func::Bnot => (!(a as i32)) as f32,
        Func::Atan2 => a.atan2(b),
        Func::Pow => a.powf(b),
        Func::Min => a.min(b),
        Func::Max => a.max(b),
        Func::Sigmoid => 1.0 / (1.0 + (-a).exp()),
        Func::Bor => ((a as i32) | (b as i32)) as f32,
        Func::Band => ((a as i32) & (b as i32)) as f32,
        Func::Equal => bool_f32(eel_eq(a, b)),
        Func::Above => bool_f32(eel_gt(a, b)),
        Func::Below => bool_f32(eel_lt(a, b)),
        Func::If => unreachable!("ternary if is handled before argument evaluation"),
        Func::Exec2 => v[1],
        Func::Exec3 => v[2],
    })
}

fn exec_block(stmts: &[Stmt], ctx: &mut EvalCtx<'_>) -> Result<(), EelError> {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) => {
                eval_expr(e, ctx)?;
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                if eval_expr(cond, ctx)? != 0.0 {
                    exec_block(then_body, ctx)?;
                } else {
                    exec_block(else_body, ctx)?;
                }
            }
            Stmt::While { cond, body } => loop {
                if eval_expr(cond, ctx)? == 0.0 {
                    break;
                }
                ctx.bump()?;
                exec_block(body, ctx)?;
            },
            Stmt::Loop { count, body } => {
                let n = eval_expr(count, ctx)? as i64;
                if n < 0 {
                    continue;
                }
                if n > ITERATION_LIMIT as i64 {
                    return Err(EelError::IterationLimitExceeded);
                }
                for _ in 0..n {
                    ctx.bump()?;
                    exec_block(body, ctx)?;
                }
            }
        }
    }
    Ok(())
}

/// Compile EEL source (`version` is 1 or 2).
pub fn compile(src: &str, version: u8) -> Result<CompiledEquations, EelError> {
    let toks = lex(src)?;
    let mut parser = Parser::new(toks);
    let program = parser.parse_program()?;
    Ok(CompiledEquations { program, version })
}

/// Evaluate a compiled equation set for one frame.
pub fn run_frame(
    eq: &CompiledEquations,
    pool: &mut VarPool,
    globals: &Globals,
) -> Result<(), EelError> {
    pool.reset_q();
    globals.inject_into(pool);
    let mut ctx = EvalCtx {
        pool,
        version: eq.version,
        iterations: 0,
    };
    exec_block(&eq.program, &mut ctx)
}

/// Evaluate a compiled equation set for one pixel/point.
pub fn run_pixel(
    eq: &CompiledEquations,
    pool: &mut VarPool,
    x: f32,
    y: f32,
    rad: f32,
    ang: f32,
) -> Result<(), EelError> {
    pool.reset_q();
    pool.set("x", x);
    pool.set("y", y);
    pool.set("rad", rad);
    pool.set("ang", ang);
    let mut ctx = EvalCtx {
        pool,
        version: eq.version,
        iterations: 0,
    };
    exec_block(&eq.program, &mut ctx)
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
        assert!(
            panic_free.is_ok(),
            "compile must not panic on hostile input"
        );
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

    // ------------------------------------------------------------------
    // MilkDrop 2 EEL2 control-flow compatibility (RED)
    // ------------------------------------------------------------------

    /// Run an arbitrary statement list on a seeded pool with explicit globals.
    fn run_src_with_globals(src: &str, globals: Globals) -> VarPool {
        let eq = compile(src, 2).expect("source compiles");
        let mut pool = VarPool::with_seed(0xC0FFEE);
        run_frame(&eq, &mut pool, &globals).expect("source runs");
        pool
    }

    #[test]
    fn braceless_if_and_else_select_the_correct_branch() {
        // Real v2 presets omit braces around a single-statement if/else body.
        let src = "x = 0; if (bass > 1.0) x = 2; else x = 3;";
        let hi = Globals {
            bass: 1.2,
            ..Globals::default()
        };
        let lo = Globals {
            bass: 0.5,
            ..Globals::default()
        };
        let p_hi = run_src_with_globals(src, hi);
        let p_lo = run_src_with_globals(src, lo);
        approx!(p_hi.get("x"), 2.0, "then branch when bass=1.2");
        approx!(p_lo.get("x"), 3.0, "else branch when bass=0.5");
    }

    #[test]
    fn braceless_if_statement_can_be_nested_as_the_then_body() {
        let src = "x = 0; if (a) if (b) x = 7; else x = 8;";
        let eq = compile(src, 2).expect("nested braceless if compiles");
        let run = |a: f32, b: f32| {
            let mut pool = VarPool::with_seed(1);
            pool.set("a", a);
            pool.set("b", b);
            run_frame(&eq, &mut pool, &Globals::default()).expect("runs");
            pool.get("x")
        };
        approx!(run(1.0, 1.0), 7.0, "inner then");
        approx!(run(1.0, 0.0), 8.0, "inner else");
        approx!(run(0.0, 1.0), 0.0, "outer false leaves x seeded");
    }

    #[test]
    fn braceless_else_if_chain_selects_the_correct_branch() {
        let eq =
            compile("if (a) x=1; else if (b) x=2; else x=3;", 2).expect("else-if chain compiles");
        let run = |a: f32, b: f32| {
            let mut pool = VarPool::with_seed(1);
            pool.set("a", a);
            pool.set("b", b);
            run_frame(&eq, &mut pool, &Globals::default()).expect("runs");
            pool.get("x")
        };
        approx!(run(1.0, 0.0), 1.0, "first branch");
        approx!(run(0.0, 1.0), 2.0, "else-if branch");
        approx!(run(1.0, 1.0), 1.0, "first branch wins");
        approx!(run(0.0, 0.0), 3.0, "final else branch");
    }

    #[test]
    fn braceless_while_and_loop_bodies_iterate_and_keep_the_hard_bound() {
        let p = run_src("i=0; while(i<3) i=i+1;");
        approx!(p.get("i"), 3.0, "brace-less while body");

        let p = run_src("x=0; loop(3) x=x+1;");
        approx!(p.get("x"), 3.0, "brace-less loop body");

        // A brace-less infinite loop still returns a typed error, never panics.
        let eq = compile("i=0; while(1) i=i+1;", 2).expect("compiles");
        let res = run_frame(&eq, &mut VarPool::new(), &Globals::default());
        assert!(matches!(res, Err(EelError::IterationLimitExceeded)));
    }

    #[test]
    fn c_style_block_comments_span_lines_without_breaking_statements() {
        let src = "\
x = 1; /* a block
comment spanning
multiple lines */
y = 2;
// a line comment still works
z = x + y; /* trailing block comment */
";
        let p = run_src(src);
        approx!(p.get("x"), 1.0);
        approx!(p.get("y"), 2.0);
        approx!(p.get("z"), 3.0, "comments do not merge statements");
    }

    #[test]
    fn realistic_v2_per_frame_fixture_compiles_and_is_deterministic() {
        const PER_FRAME: &str = "\
q1 = q1 + 0.1; q2 = q2*0.9 + 0.1*bass;
q3 = if(above(bass, 0.5), if(above(mid, 0.3), pow(bass, 2.0), bass), q3*0.95);
q4 = min(max(q4 + exp(-1.0*sqr(frame/50.0)), 0.0), 10.0);
if (above(bass, 0.5)) q5 = 1.0; else q5 = 0.0;
megabuf[5] = min(max(megabuf[5] + 0.01, 0.0), 1.0);
r = rand(1.0);
out = q1 + q2 + q3 + q4 + q5 + megabuf[5];
";
        let eq = compile(PER_FRAME, 2).expect("real v2 per_frame fixture compiles");
        let globals = Globals {
            bass: 0.8,
            mid: 0.4,
            ..Globals::default()
        };
        let run_once = || {
            let mut pool = VarPool::with_seed(0xFEED);
            pool.snapshot_q();
            run_frame(&eq, &mut pool, &globals).expect("fixture runs");
            pool
        };

        let a = run_once();
        let b = run_once();
        approx!(a.get("q1"), 0.1, "q accumulator");
        approx!(
            a.get("q3"),
            0.8_f32.powf(2.0),
            "nested if(above(...), pow(...))"
        );
        approx!(a.get("q5"), 1.0, "brace-less if driven by above()");
        approx!(a.megabuf(5), 0.01, "min/max clamped megabuf write");
        assert!(a.get("out").is_finite(), "outputs must be finite");
        approx!(a.get("out"), b.get("out"), "seeded run is deterministic");
        approx!(a.get("r"), b.get("r"), "seeded rand is deterministic");
    }

    #[test]
    fn malformed_braceless_if_stays_a_typed_parse_error_without_panicking() {
        // An empty body (`;`) is not a statement: even with brace-less support
        // this must remain a typed ParseError, never a panic.
        let res = std::panic::catch_unwind(|| compile("if (x>0) ;", 2));
        assert!(
            res.is_ok(),
            "compile must not panic on malformed braceless if"
        );
        assert!(
            matches!(res.expect("no panic"), Err(EelError::ParseError(_))),
            "expected a typed ParseError"
        );

        let res = std::panic::catch_unwind(|| compile("while (x<1) ;", 2));
        assert!(
            res.is_ok(),
            "compile must not panic on malformed braceless while"
        );
        assert!(matches!(
            res.expect("no panic"),
            Err(EelError::ParseError(_))
        ));
    }
}
