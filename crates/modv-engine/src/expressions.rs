//! JavaScript expression evaluation (WS-12).
//!
//! The worker store compiles a user's expression with
//! `new Function("scope", "const { value, time, inputValue, osc } = scope; return <expression>")`
//! (`store/modules/expressions.js:25-45`) and calls it on every prop/meta write
//! (`utils/apply-expression.js:4-23`). Presets store only the source text and
//! the `evaluateEveryFrame` flag, so an engine that ignores expressions silently
//! drops a user-visible part of a preset. This module is the Rust replacement
//! for that JS evaluator, backed by [Boa](https://github.com/boa-dev/boa), a
//! pure-Rust ECMAScript engine.
//!
//! # API
//!
//! ```ignore
//! let mut engine = ExpressionEngine::new();
//! let handle = engine.compile("value * 2 + Math.sin(time)")?;
//! let value = engine.evaluate(&handle, &ExpressionScope {
//!     value: &json!(0.5),
//!     time_ms: 1234.0,
//!     input_value: &json!(1.0),
//!     osc: &json!({}),
//! })?;
//! # Ok::<(), ExpressionError>(())
//! ```
//!
//! `compile` parses the expression once; every `evaluate` reuses the compiled
//! function object, so the per-write/per-frame cost is a call plus the JSON ↔
//! JS conversion, not a parse/compile. Compiled functions live in a Rust map
//! (a `JsObject` is a rooted `boa_gc` pointer), never on the JS global, so an
//! expression cannot discover, overwrite or delete another expression's
//! handle.
//!
//! # Semantics
//!
//! * The expression body has `value`, `time`, `inputValue` and `osc` in scope,
//!   exactly like the JS `new Function` body. `time` is `Date.now()` at
//!   evaluation time, supplied by the caller as `time_ms`.
//! * JSON numbers/booleans/strings/arrays/objects round-trip through
//!   [`serde_json::Value`], so the store's prop coercion sees the same shape JS
//!   would commit.
//! * JS `undefined` cannot be represented in JSON; it is returned as
//!   [`serde_json::Value::Null`] (documented deviation — the JS path would
//!   commit `undefined`, which the prop coercion then turns into `NaN`/no-op).
//!   Non-finite numbers likewise cannot be represented and become `null`.
//!
//! # Sandboxing
//!
//! `compile` validates the source with Boa's parser before it reaches the
//! realm. The AST must be a single expression made only of: the four scope
//! identifiers (plus `undefined`/`NaN`/`Infinity`), numeric/string/boolean
//! literals, array/object/template literals without methods/spread/computed
//! keys, unary/binary/conditional/parenthesised operators (no `delete`), plain
//! property reads (never `constructor`/`__proto__`/`prototype`), optional
//! property reads, and calls to `Math.*`, `Number.*`, `Array.isArray` or the
//! bounded value methods in `ALLOWED_VALUE_METHODS`.
//!
//! Everything else is unrepresentable: `new`, function/arrow/class literals,
//! assignment, update, `eval`/`Function`, `globalThis`, `this`, `import`/
//! `require`, timers, regex literals, BigInt literals, spread, and the native
//! allocation/loop helpers named by the review (`repeat`, `padStart`, `split`,
//! `join`, `map`, `forEach`, `reduce`, `fill`, …). Because the parser resolves
//! identifier escapes and whitespace first, `\u0063onstructor`, `value\n
//! .constructor` and similar token-level evasions cannot bypass the checks.
//! A 2048-byte non-whitespace cap (on top of the 4096-byte source cap) and a
//! 512-node / 32-level structural cap keep string concatenation, parser
//! recursion and nested parentheses from amplifying a large host value or
//! exhausting the engine thread's stack.
//!
//! ## Why there is no wall-clock budget
//!
//! Boa 0.22 exposes its cooperative opcode budget only on
//! `Script::evaluate_async_with_budget`; the compiled-handle path uses
//! `JsObject::call`, which has no async or budgeted variant, and
//! `Context::run_async_with_budget` is crate-private. Re-entering through a
//! per-evaluation `Script` would re-create the function object on every frame
//! and still only yields between VM opcodes, so a single native builtin call
//! or allocation would run to completion before a deadline could be observed.
//! The validator therefore makes unbounded constructs unrepresentable instead,
//! with the existing loop/recursion [`RuntimeLimits`] kept as defence in
//! depth. Residual risk: a hostile expression is bounded structurally (source
//! length, AST shape, host value size), not by a wall-clock or heap ceiling.
//!
//! # Deviation: `osc`
//!
//! The JS scope reads `store.state.osc.data`. The Rust engine has no OSC
//! receive state yet (WS-9 leaves the renderer owning OSC), so callers pass an
//! empty object; `osc` is always `{}` until that state lands. Expressions that
//! only read `value`/`time`/`inputValue` are unaffected.

use std::collections::HashMap;
use std::ops::ControlFlow;

use boa_engine::{
    Context, JsObject, JsValue, Source,
    ast::{
        Expression, Statement, StatementListItem,
        expression::{
            Call, Identifier, OptionalOperationKind,
            access::{PropertyAccess, PropertyAccessField},
            literal::{Literal, LiteralKind, PropertyDefinition, TemplateElement},
            operator::{
                Unary,
                binary::{ArithmeticOp, BinaryOp},
                unary::UnaryOp,
            },
        },
        property::PropertyName,
        scope::Scope,
        visitor::{VisitWith, Visitor},
    },
    interner::Interner,
    parser::Parser,
    vm::RuntimeLimits,
};
use serde_json::Value;
use thiserror::Error;

/// Maximum number of loop iterations one expression may perform.
///
/// Kept as defence in depth: the AST validator rejects loop statements and
/// function literals, so a saved `while (true) …` never reaches the VM. This
/// still bounds any unforeseen builtin path that loops through the VM.
const EXPRESSION_LOOP_LIMIT: u64 = 100_000;

/// Maximum JS recursion depth inside one expression (defence in depth; the
/// AST validator rejects function literals, so ordinary expressions cannot
/// recurse).
const EXPRESSION_RECURSION_LIMIT: usize = 64;

/// Maximum expression source length (a hostile playlist must not embed a
/// megabyte expression).
const MAX_EXPRESSION_SOURCE: usize = 4096;

/// Maximum number of AST expression nodes in one source.
///
/// The source cap alone still permits ~2,000 operands; a hostile string
/// concatenation could amplify a large host value. 512 nodes is far above any
/// ordinary preset expression and bounds that amplification.
const MAX_EXPRESSION_NODES: usize = 512;

/// Stack size for the throw-away validation thread.
///
/// Boa's parser is recursive and has no recursion limit of its own; the
/// 4096-byte source cap still allows thousands of nested operators (tens of
/// KiB of stack per level in a debug build). Parsing happens on a large-stack
/// thread so a hostile source cannot overflow the engine's main thread, and
/// the AST depth cap then rejects it before the wrapper is evaluated.
const EXPRESSION_VALIDATOR_STACK: usize = 256 * 1024 * 1024;

/// Maximum non-whitespace bytes before the source is parsed.
///
/// Boa's parser is recursive and has no recursion limit; a 4096-byte run of
/// `!` or `(` would overflow even a 256 MiB stack in a debug build. 2048 is
/// still far above any normal preset expression (the AST node budget is
/// tighter in practice) and bounds parser recursion so the validator thread
/// can always finish. The 4096-byte source cap still applies to the whole
/// source, including whitespace and comments.
const MAX_EXPRESSION_NON_WHITESPACE: usize = 2048;

/// Maximum AST nesting depth. Rejected at compile time so neither the
/// validator nor Boa's compiler recurses through a pathological
/// parenthesised chain. Valid expressions are evaluated on the engine's main
/// thread, so this must stay well below that thread's stack budget.
const MAX_EXPRESSION_DEPTH: usize = 32;

/// Names that may appear as bare identifiers in an expression.
///
/// The four scope names are the wrapper parameters (`value`, `time`,
/// `inputValue`, `osc`); the rest are side-effect-free scalar intrinsics that
/// presets commonly use. Everything else (including `globalThis`, `eval`,
/// `Function`, `require`, timers and `Object`/`JSON`) is unrepresentable.
const ALLOWED_IDENTIFIERS: &[&str] = &[
    "value",
    "time",
    "inputValue",
    "osc",
    "undefined",
    "NaN",
    "Infinity",
    "Math",
    "Number",
    "String",
    "Boolean",
    "Array",
    "parseInt",
    "parseFloat",
    "isFinite",
    "isNaN",
];

/// Bare-global functions an expression may call.
const ALLOWED_FUNCTION_CALLS: &[&str] = &[
    "Number",
    "String",
    "Boolean",
    "parseInt",
    "parseFloat",
    "isFinite",
    "isNaN",
];

/// `Math` methods an expression may call. Constants are plain property reads
/// and are not restricted by this list.
const ALLOWED_MATH_METHODS: &[&str] = &[
    "abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt", "ceil", "clz32",
    "cos", "cosh", "exp", "expm1", "floor", "fround", "hypot", "imul", "log", "log10", "log1p",
    "log2", "max", "min", "pow", "random", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh",
    "trunc",
];

/// `Number` static helpers an expression may call.
const ALLOWED_NUMBER_METHODS: &[&str] = &[
    "isFinite",
    "isInteger",
    "isNaN",
    "isSafeInteger",
    "parseFloat",
    "parseInt",
];

/// `Array` static helpers an expression may call.
///
/// `isArray` is a pure predicate. The constructor and every allocating static
/// (`from`, `of`) stay unreachable because they are not on this list.
const ALLOWED_ARRAY_METHODS: &[&str] = &["isArray"];

/// Methods callable on plain JSON values (numbers, strings, arrays, objects).
///
/// Deliberately excludes every native loop/allocation helper named by the
/// review (`repeat`, `padStart`, `padEnd`, `split`, `join`, `reverse`,
/// `concat`, `fill`, `map`, `forEach`, `reduce`, …). `slice`/`substring`/
/// `indexOf` are bounded by the already-converted host value, not an
/// expression-controlled length.
const ALLOWED_VALUE_METHODS: &[&str] = &[
    "toFixed",
    "toPrecision",
    "toString",
    "charAt",
    "charCodeAt",
    "codePointAt",
    "indexOf",
    "lastIndexOf",
    "startsWith",
    "endsWith",
    "includes",
    "slice",
    "substring",
    "trim",
    "trimStart",
    "trimEnd",
    "toLowerCase",
    "toUpperCase",
];

/// Property names that expose the function/prototype machinery.
const FORBIDDEN_MEMBERS: &[&str] = &[
    "constructor",
    "__proto__",
    "prototype",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// Object-literal keys that install prototype or coercion hooks.
///
/// Host JSON objects can only carry non-callable data for these keys, but an
/// expression could otherwise put an allowed intrinsic function in a
/// `valueOf`/`toString` slot and have a later coercion invoke it.
const FORBIDDEN_OBJECT_KEYS: &[&str] = &["__proto__", "valueOf", "toString"];

/// An error from compiling or evaluating an expression.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ExpressionError {
    /// The source was rejected by the expression validator or did not compile
    /// (syntax error / bad wrapper).
    #[error("could not compile expression: {0}")]
    Compile(String),
    /// The expression compiled but threw (or returned a value that cannot be
    /// converted to JSON) when evaluated.
    #[error("expression threw: {0}")]
    Runtime(String),
}

/// A compiled expression, reusable for many evaluations.
///
/// The handle keeps the source text for diagnostics and a key identifying the
/// function object inside the [`ExpressionEngine`] that compiled it. A handle
/// is only valid for the engine that produced it; evaluating it on another
/// engine yields an error rather than the wrong function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionHandle {
    key: String,
    source: String,
}

impl ExpressionHandle {
    /// The expression source this handle was compiled from.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// The variables an expression sees, matching the JS `applyExpression` scope.
#[derive(Clone, Copy, Debug)]
pub struct ExpressionScope<'a> {
    /// The value being written (`value` in JS).
    pub value: &'a Value,
    /// Wall-clock time in milliseconds (`Date.now()` in JS).
    pub time_ms: f64,
    /// The current value of the input's `getLocation` (`inputValue` in JS).
    pub input_value: &'a Value,
    /// The OSC receive data (`osc` in JS); empty until the engine owns OSC.
    pub osc: &'a Value,
}

/// Validate `source` against the expression allowlist on a large-stack thread.
///
/// The parse and the AST drop both recurse in Boa/`boa_ast`; running them here
/// keeps a pathological source from overflowing the engine's main thread.
fn validate_expression(source: &str) -> Result<(), String> {
    let significant = source
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .count();
    if significant > MAX_EXPRESSION_NON_WHITESPACE {
        return Err(format!(
            "expression has more than {MAX_EXPRESSION_NON_WHITESPACE} non-whitespace bytes"
        ));
    }
    let source = source.to_string();
    let handle = std::thread::Builder::new()
        .name("modv-expression-validator".to_string())
        .stack_size(EXPRESSION_VALIDATOR_STACK)
        .spawn(move || validate_expression_inner(&source))
        .map_err(|error| format!("could not start expression validator: {error}"))?;
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err("expression validator panicked".to_string()),
    }
}

/// Parse and validate the source; runs on the validator thread.
///
/// The source is parsed exactly as the wrapper evaluates it — inside the
/// parentheses of `return ( … )` — with Boa's own parser, so identifier
/// escapes (`\u0063onstructor`), whitespace and comment tricks cannot evade the
/// structural checks. The resulting AST must be a single expression, and every
/// node in it must be on the allowlist below.
fn validate_expression_inner(source: &str) -> Result<(), String> {
    let wrapped = format!("(\n{source}\n)");
    let mut interner = Interner::default();
    let mut parser = Parser::new(Source::from_bytes(wrapped.as_bytes()));
    let script = parser
        .parse_script(&Scope::new_global(), &mut interner)
        .map_err(|error| format!("invalid expression syntax: {error}"))?;

    let [StatementListItem::Statement(statement)] = script.statements().statements() else {
        return Err("expression must be a single expression".to_string());
    };
    let Statement::Expression(expression) = statement.as_ref() else {
        return Err("expression must be a single expression".to_string());
    };

    let mut validator = AstValidator {
        interner: &interner,
        error: None,
        nodes: 0,
        depth: 0,
    };
    let flow = validator.visit_expression(expression);
    match validator.error {
        Some(error) => Err(error),
        None if flow.is_break() => Err("expression uses an unsupported construct".to_string()),
        None => Ok(()),
    }
}

/// Strip redundant parentheses so `(Math).sin(…)` and `Math.sin(…)` validate
/// the same way.
fn unparenthesize(mut expression: &Expression) -> &Expression {
    while let Expression::Parenthesized(parenthesized) = expression {
        expression = parenthesized.expression();
    }
    expression
}

/// The AST allowlist visitor.
///
/// Any node type not explicitly allowed here is rejected. Calls and member
/// accesses are validated against the function/method allowlists so a value's
/// `constructor` can never be reached, and a dynamic computed field can be
/// read but never called (there is no way to turn it into `Function(...)`).
struct AstValidator<'a> {
    interner: &'a Interner,
    error: Option<String>,
    nodes: usize,
    depth: usize,
}

impl<'a> AstValidator<'a> {
    fn name(&self, identifier: Identifier) -> String {
        self.interner.resolve_expect(identifier.sym()).to_string()
    }

    fn reject(&mut self, reason: impl Into<String>) -> ControlFlow<()> {
        if self.error.is_none() {
            self.error = Some(reason.into());
        }
        ControlFlow::Break(())
    }

    /// Resolve a statically-nameable field. `None` means a dynamic computed key.
    fn static_field(&self, field: &PropertyAccessField) -> Option<String> {
        match field {
            PropertyAccessField::Const(identifier) => Some(self.name(*identifier)),
            PropertyAccessField::Expr(expression) => self.static_key(expression),
        }
    }

    /// Constant-fold a computed key made only of literals (and `+`/templates)
    /// so `value["con" + "structor"]` is checked like `value.constructor`.
    /// Anything involving a runtime value returns `None`; such a key can still
    /// be read, but never called.
    fn static_key(&self, expression: &Expression) -> Option<String> {
        match unparenthesize(expression) {
            Expression::Literal(literal) => match literal.kind() {
                LiteralKind::String(sym) => Some(self.interner.resolve_expect(*sym).to_string()),
                LiteralKind::Num(number) => Some(number.to_string()),
                LiteralKind::Int(number) => Some(number.to_string()),
                _ => None,
            },
            Expression::TemplateLiteral(template) => {
                let mut name = String::new();
                for element in template.elements() {
                    match element {
                        TemplateElement::String(sym) => {
                            name.push_str(&self.interner.resolve_expect(*sym).to_string());
                        }
                        TemplateElement::Expr(expression) => {
                            name.push_str(&self.static_key(expression)?);
                        }
                    }
                }
                Some(name)
            }
            Expression::Binary(binary)
                if matches!(binary.op(), BinaryOp::Arithmetic(ArithmeticOp::Add)) =>
            {
                let left = self.static_key(binary.lhs())?;
                let right = self.static_key(binary.rhs())?;
                Some(format!("{left}{right}"))
            }
            _ => None,
        }
    }

    /// Check that a call target is on the function/method allowlist.
    fn check_callee(&self, callee: &Expression) -> Result<(), String> {
        match unparenthesize(callee) {
            Expression::Identifier(identifier) => {
                let name = self.name(*identifier);
                if ALLOWED_FUNCTION_CALLS.contains(&name.as_str()) {
                    Ok(())
                } else {
                    Err(format!("calling `{name}` is not supported in expressions"))
                }
            }
            Expression::PropertyAccess(PropertyAccess::Simple(access)) => {
                let method = self.static_field(access.field()).ok_or_else(|| {
                    "calls through computed member names are not supported in expressions"
                        .to_string()
                })?;

                if let Expression::Identifier(identifier) = unparenthesize(access.target()) {
                    let target = self.name(*identifier);
                    let allowed = match target.as_str() {
                        "Math" => ALLOWED_MATH_METHODS,
                        "Number" => ALLOWED_NUMBER_METHODS,
                        "Array" => ALLOWED_ARRAY_METHODS,
                        _ => &[],
                    };
                    if allowed.contains(&method.as_str()) {
                        return Ok(());
                    }
                    if matches!(target.as_str(), "Math" | "Number" | "Array") {
                        return Err(format!(
                            "`{target}.{method}` is not an allowed expression function"
                        ));
                    }
                }

                if ALLOWED_VALUE_METHODS.contains(&method.as_str()) {
                    Ok(())
                } else {
                    Err(format!("method `{method}` is not supported in expressions"))
                }
            }
            _ => Err("expression calls are limited to Math, Number and value methods".to_string()),
        }
    }

    fn visit_expression_inner(&mut self, node: &'a Expression) -> ControlFlow<()> {
        match node {
            Expression::This(_) => self.reject("`this` is not available in expressions"),
            Expression::Identifier(identifier) => self.visit_identifier(identifier),
            Expression::Literal(literal) => self.visit_literal(literal),
            Expression::RegExpLiteral(_) => {
                self.reject("regular expressions are not supported in expressions")
            }
            Expression::ArrayLiteral(array) => array.visit_with(self),
            Expression::ObjectLiteral(object) => object.visit_with(self),
            Expression::Spread(_) => self.reject("spread syntax is not supported in expressions"),
            Expression::FunctionExpression(_)
            | Expression::ArrowFunction(_)
            | Expression::AsyncArrowFunction(_)
            | Expression::GeneratorExpression(_)
            | Expression::AsyncFunctionExpression(_)
            | Expression::AsyncGeneratorExpression(_) => {
                self.reject("function definitions are not supported in expressions")
            }
            Expression::ClassExpression(_) => {
                self.reject("class definitions are not supported in expressions")
            }
            Expression::TemplateLiteral(template) => template.visit_with(self),
            Expression::PropertyAccess(access) => match access {
                PropertyAccess::Simple(_) => access.visit_with(self),
                PropertyAccess::Private(_) | PropertyAccess::Super(_) => {
                    self.reject("private and super property access are not supported")
                }
            },
            Expression::New(_) => self.reject("`new` is not supported in expressions"),
            Expression::Call(call) => self.visit_call(call),
            Expression::SuperCall(_) => self.reject("`super` is not supported in expressions"),
            Expression::ImportCall(_) => self.reject("`import()` is not supported in expressions"),
            Expression::Optional(optional) => optional.visit_with(self),
            Expression::TaggedTemplate(_) => {
                self.reject("tagged templates are not supported in expressions")
            }
            Expression::NewTarget(_) | Expression::ImportMeta(_) => {
                self.reject("module syntax is not supported in expressions")
            }
            Expression::Assign(_) => self.reject("assignments are not supported in expressions"),
            Expression::Unary(unary) => self.visit_unary(unary),
            Expression::Update(_) => {
                self.reject("increment/decrement are not supported in expressions")
            }
            Expression::Binary(binary) => binary.visit_with(self),
            Expression::BinaryInPrivate(_) => {
                self.reject("private operators are not supported in expressions")
            }
            Expression::Conditional(conditional) => conditional.visit_with(self),
            Expression::Await(_) | Expression::Yield(_) => {
                self.reject("await/yield are not supported in expressions")
            }
            Expression::Parenthesized(parenthesized) => parenthesized.visit_with(self),
        }
    }
}

impl<'a> Visitor<'a> for AstValidator<'a> {
    type BreakTy = ();

    fn visit_expression(&mut self, node: &'a Expression) -> ControlFlow<()> {
        self.nodes += 1;
        if self.nodes > MAX_EXPRESSION_NODES {
            return self.reject(format!(
                "expression has more than {MAX_EXPRESSION_NODES} AST nodes"
            ));
        }
        if self.depth >= MAX_EXPRESSION_DEPTH {
            return self.reject(format!(
                "expression is nested deeper than {MAX_EXPRESSION_DEPTH} levels"
            ));
        }
        self.depth += 1;
        let flow = self.visit_expression_inner(node);
        self.depth -= 1;
        flow
    }

    fn visit_identifier(&mut self, node: &'a Identifier) -> ControlFlow<()> {
        let name = self.name(*node);
        if ALLOWED_IDENTIFIERS.contains(&name.as_str()) {
            ControlFlow::Continue(())
        } else {
            self.reject(format!(
                "identifier `{name}` is not in the expression scope"
            ))
        }
    }

    fn visit_literal(&mut self, node: &'a Literal) -> ControlFlow<()> {
        if matches!(node.kind(), LiteralKind::BigInt(_)) {
            return self.reject("BigInt literals are not supported in expressions");
        }
        ControlFlow::Continue(())
    }

    fn visit_call(&mut self, node: &'a Call) -> ControlFlow<()> {
        if let Err(error) = self.check_callee(node.function()) {
            return self.reject(error);
        }
        node.visit_with(self)
    }

    fn visit_unary(&mut self, node: &'a Unary) -> ControlFlow<()> {
        if matches!(node.op(), UnaryOp::Delete) {
            return self.reject("`delete` is not supported in expressions");
        }
        node.visit_with(self)
    }

    fn visit_property_access_field(&mut self, node: &'a PropertyAccessField) -> ControlFlow<()> {
        if let Some(name) = self.static_field(node)
            && FORBIDDEN_MEMBERS.contains(&name.as_str())
        {
            return self.reject(format!(
                "access to `{name}` is not supported in expressions"
            ));
        }
        node.visit_with(self)
    }

    fn visit_property_name(&mut self, node: &'a PropertyName) -> ControlFlow<()> {
        match node {
            PropertyName::Literal(_) => ControlFlow::Continue(()),
            PropertyName::Computed(_) => {
                self.reject("computed object keys are not supported in expressions")
            }
        }
    }

    fn visit_property_definition(&mut self, node: &'a PropertyDefinition) -> ControlFlow<()> {
        match node {
            PropertyDefinition::Property(name, _) => {
                if let PropertyName::Literal(identifier) = name {
                    let key = self.name(*identifier);
                    if FORBIDDEN_OBJECT_KEYS.contains(&key.as_str()) {
                        return self.reject(format!(
                            "object key `{key}` is not supported in expressions"
                        ));
                    }
                }
                node.visit_with(self)
            }
            PropertyDefinition::MethodDefinition(_) => {
                self.reject("object methods/getters/setters are not supported in expressions")
            }
            PropertyDefinition::SpreadObject(_) => {
                self.reject("object spread is not supported in expressions")
            }
            PropertyDefinition::CoverInitializedName(_, _) => {
                self.reject("object patterns are not supported in expressions")
            }
            _ => node.visit_with(self),
        }
    }

    fn visit_optional_operation_kind(
        &mut self,
        node: &'a OptionalOperationKind,
    ) -> ControlFlow<()> {
        match node {
            OptionalOperationKind::SimplePropertyAccess { field } => {
                self.visit_property_access_field(field)
            }
            OptionalOperationKind::PrivatePropertyAccess { .. } => {
                self.reject("private property access is not supported in expressions")
            }
            OptionalOperationKind::Call { .. } => {
                self.reject("optional calls are not supported in expressions")
            }
        }
    }
}

/// A reusable Boa context plus the compiled function objects.
pub struct ExpressionEngine {
    context: Context,
    /// Compiled functions keyed by handle id, owned directly by Rust.
    ///
    /// A `JsObject` is a rooted `boa_gc` pointer: keeping it in this map keeps
    /// it alive across collections. Nothing is registered on the JS global, so
    /// expression code cannot discover, overwrite or delete another handle.
    functions: HashMap<String, JsObject>,
    next_key: u64,
}

impl Default for ExpressionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpressionEngine {
    /// A fresh evaluator with `Math`, `JSON`, `String`, `Array`, … available.
    pub fn new() -> Self {
        let mut context = Context::default();
        let mut limits = RuntimeLimits::default();
        limits.set_loop_iteration_limit(EXPRESSION_LOOP_LIMIT);
        limits.set_recursion_limit(EXPRESSION_RECURSION_LIMIT);
        context.set_runtime_limits(limits);

        Self {
            context,
            functions: HashMap::new(),
            next_key: 0,
        }
    }

    /// Replace the JS realm and drop every compiled function.
    ///
    /// Called when a preset/Scene replaces the expression assignments: a
    /// long-lived realm lets one expression's prototype pollution affect every
    /// later expression, and recompilation is cheap relative to a preset load.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Compile `source` into a reusable handle.
    ///
    /// The wrapper mirrors the JS `new Function` body: the four scope names
    /// are parameters and the source is the returned expression. A syntax error
    /// is reported as [`ExpressionError::Compile`].
    pub fn compile(&mut self, source: &str) -> Result<ExpressionHandle, ExpressionError> {
        if source.len() > MAX_EXPRESSION_SOURCE {
            return Err(ExpressionError::Compile(format!(
                "expression is longer than {MAX_EXPRESSION_SOURCE} bytes"
            )));
        }
        validate_expression(source).map_err(ExpressionError::Compile)?;
        // The newlines matter: a trailing `// comment` in the user's source
        // would otherwise swallow the closing parenthesis.
        let wrapper =
            format!("(function(value, time, inputValue, osc) {{\n  return (\n{source}\n  );\n}})");

        let value = self
            .context
            .eval(Source::from_bytes(wrapper.as_str()))
            .map_err(|error| ExpressionError::Compile(error.to_string()))?;

        let function = value.as_callable().ok_or_else(|| {
            ExpressionError::Compile("expression did not evaluate to a function".to_string())
        })?;

        let key = format!("e{}", self.next_key);
        self.next_key = self.next_key.wrapping_add(1);
        self.functions.insert(key.clone(), function);

        Ok(ExpressionHandle {
            key,
            source: source.to_string(),
        })
    }

    /// Drop a compiled function from the engine.
    ///
    /// Handles live for the engine's lifetime otherwise; the store releases a
    /// handle when an expression is edited or removed so repeated edits do not
    /// accumulate function objects in the JS heap.
    pub fn release(&mut self, handle: &ExpressionHandle) {
        self.functions.remove(&handle.key);
    }

    /// Evaluate a compiled expression with `scope`.    ///
    /// `undefined` and non-finite results become [`Value::Null`]; see the
    /// module docs.
    pub fn evaluate(
        &mut self,
        handle: &ExpressionHandle,
        scope: &ExpressionScope<'_>,
    ) -> Result<Value, ExpressionError> {
        let function = self.functions.get(&handle.key).cloned().ok_or_else(|| {
            ExpressionError::Runtime(format!(
                "compiled expression '{}' is missing from this engine",
                handle.source
            ))
        })?;

        let value = JsValue::from_json(scope.value, &mut self.context)
            .map_err(|error| ExpressionError::Runtime(error.to_string()))?;
        let input_value = JsValue::from_json(scope.input_value, &mut self.context)
            .map_err(|error| ExpressionError::Runtime(error.to_string()))?;
        let osc = JsValue::from_json(scope.osc, &mut self.context)
            .map_err(|error| ExpressionError::Runtime(error.to_string()))?;
        let args = [value, JsValue::new(scope.time_ms), input_value, osc];

        let result = function
            .call(&JsValue::undefined(), &args, &mut self.context)
            .map_err(|error| ExpressionError::Runtime(error.to_string()))?;

        let json = result
            .to_json(&mut self.context)
            .map_err(|error| ExpressionError::Runtime(error.to_string()))?;

        Ok(json.map(normalize_json).unwrap_or(Value::Null))
    }
}

/// Normalise a JS result to the JSON `JSON.stringify` would write.
///
/// JavaScript has one number type; Boa keeps `3` and `3.0` distinguishable in
/// its JSON conversion (an integral `Float64` becomes a `serde_json` float).
/// `JSON.stringify(3.0)` is `"3"`, and the worker's preset documents write
/// whole numbers without a fractional part, so integral values are turned into
/// JSON integers here. This keeps engine-written props byte-comparable with the
/// JS store's output. Floats stay floats; non-finite values already became
/// `null` in `to_json`.
fn normalize_json(value: Value) -> Value {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(number) if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0 => {
                Value::from(number as i64)
            }
            _ => Value::Number(number),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_json).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, normalize_json(value)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn evaluate(source: &str, value: Value, time_ms: f64, input: Value, osc: Value) -> Value {
        let mut engine = ExpressionEngine::new();
        let handle = engine.compile(source).expect("expression compiles");
        engine
            .evaluate(
                &handle,
                &ExpressionScope {
                    value: &value,
                    time_ms,
                    input_value: &input,
                    osc: &osc,
                },
            )
            .expect("expression evaluates")
    }

    fn eval(source: &str, value: Value) -> Value {
        evaluate(source, value, 0.0, json!(0), json!({}))
    }

    #[test]
    fn arithmetic_and_math_work() {
        assert_eq!(eval("value * 2 + 1", json!(3)), json!(7));
        assert_eq!(eval("Math.round(value)", json!(2.6)), json!(3));
        assert_eq!(eval("Math.sin(0)", json!(0)), json!(0));
    }

    #[test]
    fn ternary_and_booleans_work() {
        assert_eq!(eval("value > 0.5 ? true : false", json!(0.75)), json!(true));
        assert_eq!(
            eval("value > 0.5 ? true : false", json!(0.25)),
            json!(false)
        );
        assert_eq!(eval("!value", json!(false)), json!(true));
    }

    #[test]
    fn strings_numbers_arrays_and_objects_round_trip() {
        assert_eq!(eval("'a' + value", json!("b")), json!("ab"));
        assert_eq!(
            eval("[value, value + 1, 'x']", json!(2)),
            json!([2, 3, "x"])
        );
        assert_eq!(
            eval("({ min: value, max: value * 10 })", json!(0.2)),
            json!({"min": 0.2, "max": 2})
        );
    }

    #[test]
    fn input_value_and_osc_are_in_scope() {
        assert_eq!(
            evaluate("inputValue + value", json!(2), 0.0, json!(40), json!({})),
            json!(42)
        );
        assert_eq!(
            evaluate(
                "osc.alpha === undefined ? value : osc.alpha",
                json!(1),
                0.0,
                json!(0),
                json!({"alpha": 7})
            ),
            json!(7)
        );
        assert_eq!(
            evaluate("osc", json!(1), 0.0, json!(0), json!({})),
            json!({})
        );
    }

    #[test]
    fn time_is_in_scope() {
        assert_eq!(
            evaluate("time * 2", json!(0), 21.5, json!(0), json!({})),
            json!(43)
        );
        assert_eq!(
            evaluate(
                "Math.floor(time / 1000)",
                json!(0),
                2500.0,
                json!(0),
                json!({})
            ),
            json!(2)
        );
    }

    #[test]
    fn missing_properties_are_undefined_and_become_null() {
        assert_eq!(eval("inputValue.missing", json!(0)), Value::Null);
        assert_eq!(eval("value", json!(null)), Value::Null);
        // `undefined` has no JSON representation; the engine maps it to null.
        assert_eq!(eval("undefined", json!(0)), Value::Null);
    }

    #[test]
    fn syntax_error_is_reported_as_compile_error() {
        let mut engine = ExpressionEngine::new();
        let error = engine.compile("value +").expect_err("invalid syntax");
        assert!(matches!(error, ExpressionError::Compile(_)));
    }

    #[test]
    fn runtime_throw_is_reported() {
        let mut engine = ExpressionEngine::new();
        let handle = engine
            .compile("inputValue.missing.deeper")
            .expect("compiles");
        let error = engine
            .evaluate(
                &handle,
                &ExpressionScope {
                    value: &json!(0),
                    time_ms: 0.0,
                    input_value: &Value::Null,
                    osc: &json!({}),
                },
            )
            .expect_err("throws at runtime");
        assert!(matches!(error, ExpressionError::Runtime(_)));
    }

    #[test]
    fn handles_are_reusable_and_independent() {
        let mut engine = ExpressionEngine::new();
        let double = engine.compile("value * 2").expect("compiles");
        let plus_one = engine.compile("value + 1").expect("compiles");
        let mut check = |handle: &ExpressionHandle, value: Value| {
            engine
                .evaluate(
                    handle,
                    &ExpressionScope {
                        value: &value,
                        time_ms: 0.0,
                        input_value: &Value::Null,
                        osc: &Value::Null,
                    },
                )
                .unwrap()
        };
        assert_eq!(check(&double, json!(2)), json!(4));
        assert_eq!(check(&plus_one, json!(2)), json!(3));
        assert_eq!(check(&double, json!(5)), json!(10));
    }

    #[test]
    fn node_reference_semantics_match() {
        // Expected values produced by Node 24 running the worker's wrapper:
        //   new Function("scope", "const {value,time,inputValue,osc}=scope; return <expr>")
        // and `JSON.stringify(result)` (undefined printed as typeof).
        let cases: &[(&str, Value, Value)] = &[
            ("value * 2 + 1", json!(3), json!(7)),
            ("value > 0.5 ? true : false", json!(0.75), json!(true)),
            ("!value", json!(false), json!(true)),
            ("'a' + value", json!("b"), json!("ab")),
            ("Math.round(value)", json!(2.6), json!(3)),
            ("[value, value + 1, 'x']", json!(2), json!([2, 3, "x"])),
            (
                "({ min: value, max: value * 10 })",
                json!(0.2),
                json!({"min": 0.2, "max": 2}),
            ),
            // Node reports `undefined`; the engine's JSON boundary maps it to
            // null (see the module docs).
            ("undefined", json!(0), Value::Null),
            ("inputValue.missing", json!(0), Value::Null),
        ];

        for (source, value, expected) in cases {
            assert_eq!(&eval(source, value.clone()), expected, "{source}");
        }
    }

    #[test]
    fn released_handle_cannot_be_evaluated() {
        let mut engine = ExpressionEngine::new();
        let handle = engine.compile("value * 2").expect("compiles");
        engine.release(&handle);
        let error = engine
            .evaluate(
                &handle,
                &ExpressionScope {
                    value: &json!(1),
                    time_ms: 0.0,
                    input_value: &Value::Null,
                    osc: &Value::Null,
                },
            )
            .expect_err("released handle is gone");
        assert!(matches!(error, ExpressionError::Runtime(_)));
    }

    #[test]
    fn trailing_line_comment_does_not_break_the_wrapper() {
        assert_eq!(eval("value + 1 // maths", json!(1)), json!(2));
    }

    #[test]
    fn runaway_loops_and_allocations_are_rejected_at_compile_time() {
        let mut engine = ExpressionEngine::new();
        for source in [
            "(() => { while (true) {} })()",
            "Array.from({ length: 1000000000 })",
            "'x'.repeat(1000000000)",
            "value.map((x) => x)",
        ] {
            let error = engine
                .compile(source)
                .expect_err("hostile expressions are rejected");
            assert!(matches!(error, ExpressionError::Compile(_)), "{source}");
        }

        // Ordinary maths still compiles and runs.
        let handle = engine.compile("value * 2 + Math.sin(time / 1000)").unwrap();
        let value = engine
            .evaluate(
                &handle,
                &ExpressionScope {
                    value: &json!(2),
                    time_ms: 0.0,
                    input_value: &Value::Null,
                    osc: &json!({}),
                },
            )
            .expect("safe expression evaluates");
        assert_eq!(value, json!(4));
    }

    #[test]
    fn review_bypass_strings_are_rejected_at_compile_time() {
        let mut engine = ExpressionEngine::new();
        let sources = [
            // Round 13: constructor escape through a freshly compiled function.
            "({}).constructor.constructor(\"return 1\")()",
            "({}).constructor",
            "({})['constructor']",
            "({})[\"con\" + \"structor\"](\"return 1\")",
            "value.constructor",
            "value.__proto__",
            "value.prototype",
            "value['constructor']",
            "value[\"__proto__\"]",
            "value[\"con\" + \"structor\"]",
            "value[`constructor`]",
            "value[`con${\"structor\"}`]",
            "value?.constructor",
            "value?.[\"constructor\"]",
            "({})[\"con\" + \"structor\"]",
            "value\n.constructor",
            "value\u{0063}onstructor",
            "Math.constructor('return 1')()",
            // Native allocation methods named by the review.
            "padStart(1e9)",
            "'x'.padStart(1e9)",
            "'x'.padEnd(1e9)",
            "'x'.repeat(1e9)",
            "\"x\".repeat(1e9)",
            "value.split('')",
            "value.join('')",
            "value.reverse()",
            "value.concat('x')",
            "value.map((x) => x)",
            "value.fill(0)",
            "Array.from({ length: 1e9 })",
            "new Array(1e9)",
            // BigInt literals bypass the old `BigInt` token check.
            "2n ** 99999999n",
            // Regex backtracking is native and unbudgeted.
            "/^(a+)+$/.test('aaaaaaaaaaaaaaaaaaaaaaaa!')",
            // Globals, dynamic code and timers.
            "globalThis",
            "this",
            "eval('1')",
            "Function('return 1')()",
            "new Function('return 1')",
            "import('fs')",
            "require('fs')",
            "setTimeout(() => {}, 0)",
            "setInterval(() => {}, 0)",
            "Object.keys(value)",
            "JSON.stringify(value)",
        ];

        for source in sources {
            let error = engine.compile(source).expect_err(source);
            assert!(
                matches!(error, ExpressionError::Compile(_)),
                "{source}: {error:?}"
            );
        }
    }

    #[test]
    fn normal_preset_expressions_still_evaluate() {
        // The review's canonical normal expression.
        assert_eq!(
            evaluate(
                "Math.sin(time) * 0.5 + value",
                json!(0.25),
                std::f64::consts::FRAC_PI_2,
                json!(0),
                json!({})
            ),
            json!(0.75)
        );
        assert_eq!(
            evaluate(
                "Math.sin(time) * 0.5 + value",
                json!(0.25),
                0.0,
                json!(0),
                json!({})
            ),
            json!(0.25)
        );

        assert_eq!(eval("value > 0.5 ? 1 : 0", json!(0.75)), json!(1));
        assert_eq!(
            eval("Math.max(0, Math.min(1, value))", json!(1.5)),
            json!(1)
        );
        assert_eq!(eval("Math.PI > 3 ? 1 : 0", json!(0)), json!(1));
        assert_eq!(
            eval("Number.isFinite(value) ? value : 0", json!(2)),
            json!(2)
        );
        assert_eq!(eval("value.toFixed(2)", json!(0.25)), json!("0.25"));
        assert_eq!(eval("'  x  '.trim().toUpperCase()", json!(0)), json!("X"));
        assert_eq!(eval("[value, value * 2][1]", json!(3)), json!(6));
        assert_eq!(
            eval("({ min: value, max: value + 1 }).max", json!(3)),
            json!(4)
        );
        assert_eq!(eval("`v=${value}`", json!(3)), json!("v=3"));
        assert_eq!(eval("value.length", json!([1, 2, 3])), json!(3));
        assert_eq!(eval("value[1]", json!([1, 2, 3])), json!(2));
        assert_eq!(
            evaluate(
                "osc.alpha === undefined ? value : osc.alpha",
                json!(1),
                0.0,
                json!(0),
                json!({"alpha": 7})
            ),
            json!(7)
        );
        assert_eq!(
            evaluate("osc?.alpha ?? value", json!(1), 0.0, json!(0), json!({})),
            json!(1)
        );
        assert_eq!(
            evaluate("inputValue + value", json!(2), 0.0, json!(40), json!({})),
            json!(42)
        );
    }

    #[test]
    fn dynamic_property_reads_are_allowed_but_not_calls() {
        // Indexing with a runtime value is a normal preset shape (`value[i]`).
        assert_eq!(
            evaluate(
                "value[inputValue]",
                json!([10, 20]),
                0.0,
                json!(1),
                json!({})
            ),
            json!(20)
        );

        // The same dynamic member must not become a call target, otherwise a
        // key could resolve to `constructor` at runtime.
        let mut engine = ExpressionEngine::new();
        for source in ["value[inputValue](1)", "value[time](1)"] {
            let error = engine.compile(source).expect_err(source);
            assert!(
                matches!(error, ExpressionError::Compile(_)),
                "{source}: {error:?}"
            );
        }
    }

    #[test]
    fn structural_budget_rejects_huge_or_deep_expressions() {
        fn balanced_sum(count: usize) -> String {
            match count {
                0 => "0".to_string(),
                1 => "value".to_string(),
                n => {
                    let left = balanced_sum(n / 2);
                    let right = balanced_sum(n - n / 2);
                    format!("({left} + {right})")
                }
            }
        }

        let mut engine = ExpressionEngine::new();

        // 200 operands fit both the source and pre-parse byte caps but exceed
        // the 512-node AST budget (~598 nodes), which bounds
        // string-concatenation growth. The balanced shape keeps the nesting
        // depth below its own limit.
        let long = balanced_sum(200);
        let error = engine.compile(&long).expect_err("node budget");
        assert!(error.to_string().contains("AST nodes"), "{error}");

        // 80 nested parentheses are tiny in bytes but deeper than the limit.
        let deep = format!("{}value{}", "(".repeat(80), ")".repeat(80));
        let error = engine.compile(&deep).expect_err("depth budget");
        assert!(error.to_string().contains("nested deeper"), "{error}");
    }

    #[test]
    fn pathological_nesting_is_rejected_without_overflowing() {
        let mut engine = ExpressionEngine::new();

        // Boa's parser has no recursion limit; these are at the source cap and
        // are refused by the pre-parse complexity check.
        let unary = format!("{}value", "!".repeat(4000));
        let error = engine.compile(&unary).expect_err("deep unary chain");
        assert!(
            error.to_string().contains("non-whitespace bytes"),
            "{error}"
        );

        let parens = format!("{}value{}", "(".repeat(2000), ")".repeat(2000));
        let error = engine.compile(&parens).expect_err("deep parentheses");
        assert!(
            error.to_string().contains("non-whitespace bytes"),
            "{error}"
        );

        // These fit the pre-parse budget, so the large-stack validator thread
        // must parse them; the AST depth cap then rejects them cleanly.
        let deep = format!("{}value{}", "(".repeat(1021), ")".repeat(1021));
        let error = engine.compile(&deep).expect_err("depth budget");
        assert!(error.to_string().contains("nested deeper"), "{error}");

        let long_unary = format!("{}value", "!".repeat(2040));
        let error = engine.compile(&long_unary).expect_err("deep unary chain");
        assert!(error.to_string().contains("nested deeper"), "{error}");
    }

    #[test]
    fn loops_are_unrepresentable_and_cannot_wedge_evaluation() {
        let mut engine = ExpressionEngine::new();
        for source in [
            "while (true) {}",
            "for (;;) {}",
            "do {} while (true)",
            "(() => { while (true) {} })()",
            "(function () { while (true) {} })()",
        ] {
            let error = engine.compile(source).expect_err(source);
            assert!(
                matches!(error, ExpressionError::Compile(_)),
                "{source}: {error:?}"
            );
        }

        // Rejecting the loop forms must not disturb ordinary evaluation.
        assert_eq!(eval("value + 1", json!(1)), json!(2));
    }

    #[test]
    fn non_finite_results_stay_rejected() {
        assert_eq!(eval("Infinity", json!(0)), Value::Null);
        assert_eq!(eval("NaN", json!(0)), Value::Null);
        assert_eq!(eval("1 / 0", json!(0)), Value::Null);
        assert_eq!(eval("Math.sqrt(-1)", json!(0)), Value::Null);
    }

    #[test]
    fn expressions_cannot_pollute_the_shared_realm() {
        let mut engine = ExpressionEngine::new();
        for source in [
            "Array.prototype.push = value",
            "Math.sin = value",
            "Object.prototype.polluted = 1",
            "value.__proto__.polluted = 1",
            "delete Math.sin",
            "value.x++",
            "({ valueOf: Math.sin })",
            "({ toString: Math.sin })",
            "({ __proto__: value })",
        ] {
            let error = engine.compile(source).expect_err(source);
            assert!(
                matches!(error, ExpressionError::Compile(_)),
                "{source}: {error:?}"
            );
        }

        // The realm is still usable after the rejected attempts.
        assert_eq!(eval("Math.sin(0) + value", json!(2)), json!(2));
    }

    #[test]
    fn compiled_handles_survive_gc() {
        let mut engine = ExpressionEngine::new();
        let handle = engine.compile("value * 2 + 1").expect("compiles");

        // A `JsObject` held only by the Rust map is still a GC root.
        boa_engine::gc::force_collect();

        let value = engine
            .evaluate(
                &handle,
                &ExpressionScope {
                    value: &json!(3),
                    time_ms: 0.0,
                    input_value: &Value::Null,
                    osc: &json!({}),
                },
            )
            .expect("handle survives a collection");
        assert_eq!(value, json!(7));
    }

    #[test]
    fn compiled_functions_are_not_reachable_from_expressions() {
        let mut engine = ExpressionEngine::new();
        let double = engine.compile("value * 2").expect("compiles");
        let plus_one = engine.compile("value + 1").expect("compiles");

        // The compiled function map is owned by Rust, not registered on the
        // realm, so no hidden global can be discovered or enumerated.
        let keys = engine
            .context
            .global_object()
            .own_property_keys(&mut engine.context)
            .expect("global keys are readable");
        let leaked: Vec<String> = keys
            .iter()
            .map(|key| format!("{key:?}"))
            .filter(|key| key.contains("__modv"))
            .collect();
        assert!(
            leaked.is_empty(),
            "compiled expressions leaked into the realm: {leaked:?}"
        );

        // Every route to the old registry, including overwrite/delete, is
        // rejected before evaluation.
        for source in [
            "__modvExpressionFunctions",
            "__modvExpressionFunctions.e0",
            "__modv_expr_0",
            "globalThis.__modvExpressionFunctions",
            "delete globalThis.__modvExpressionFunctions",
            "__modvExpressionFunctions = null",
        ] {
            let error = engine.compile(source).expect_err(source);
            assert!(
                matches!(error, ExpressionError::Compile(_)),
                "{source}: {error:?}"
            );
        }

        let evaluate_handle = |engine: &mut ExpressionEngine, handle: &ExpressionHandle| {
            engine
                .evaluate(
                    handle,
                    &ExpressionScope {
                        value: &json!(3),
                        time_ms: 0.0,
                        input_value: &Value::Null,
                        osc: &json!({}),
                    },
                )
                .expect("handle still evaluates")
        };
        assert_eq!(evaluate_handle(&mut engine, &double), json!(6));
        assert_eq!(evaluate_handle(&mut engine, &plus_one), json!(4));
    }
}
