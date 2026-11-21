//! The opt-in sandboxed Boa plugin host (D5/P7).
//!
//! The legacy worker ran plugin objects in the renderer (`plugins.js`,
//! `plugin-host.js`) with the renderer's full privileges. The VmodV app has no
//! renderer, so this module re-creates the plugin surface in-process, behind
//! an explicit opt-in switch:
//!
//! ```ignore
//! let mut host = PluginHost::new(/* enabled = */ false);
//! host.add_source(include_str!("counter.js"))?;
//! let mut io = PluginIo::new(features, state, transport);
//! host.set_enabled(true, &mut io);   // load + init every source
//! host.pre_frame(&mut io);
//! host.post_frame(&mut io, Some(&capture))?;
//! ```
//!
//! # Loading contract
//!
//! A plugin is a **single CommonJS script**. It is evaluated in a fresh Boa
//! realm with a minimal `module`/`exports` shim:
//!
//! ```js
//! module.exports = {
//!   name: "My plugin",           // required, unique, non-empty
//!   author: "…",                 // optional
//!   version: "1.0.0",            // optional
//!   captureWidth: 7,             // optional RGBA capture request
//!   captureHeight: 7,
//!   props: {                     // optional; drives the generated UI
//!     threshold: { type: "float", default: 0.5, min: 0, max: 1, step: 0.01 },
//!     mode: { type: "select", default: "a",
//!             options: [{ label: "A", value: "a" }, …] },
//!   },
//!   init({ props, modV, log }) { … },
//!   preProcessFrame({ features, props, modV, log }) { … },
//!   postProcessFrame({ features, props, modV, log, width, height, pixels }) { … },
//!   shutdown({ props, modV, log }) { … },
//! };
//! ```
//!
//! The descriptor may also be assigned to `globalThis.modVPlugin` instead of
//! `module.exports` (handy for an IIFE bundle). `module.exports.default` is
//! accepted as a convenience for CJS-transpiled `export default`. Sources must
//! be bundled to plain script format (`npx esbuild plugin.js --bundle
//! --format=cjs`); ES module syntax is rejected.
//!
//! The two contract choices that differ from the JavaScript reference:
//!
//! * `props` is a **mutable object**. A plugin writes `props.foo = value`; the
//!   host coerces it back against the schema after every hook (clamping numeric
//!   props to `[min, max]`, rounding `int`, keeping only valid `select` values)
//!   and writes the coerced value back so the next hook observes it. Mutate the
//!   object; reassigning the `props` global is ignored by the host.
//! * `postProcessFrame` receives `width`, `height` and `pixels` (RGBA8 as a
//!   `Uint8Array`) plus a `capture` object carrying the same three fields; the
//!   fields are absent when no capture was supplied. The app reads the
//!   composited frame back at [`PluginHost::capture_config`]'s size.
//!
//! # Host API (the sandbox boundary)
//!
//! Only these globals are installed by the host; everything else is the
//! standard ECMAScript realm:
//!
//! * `props` — the mutable prop object.
//! * `modV.get(path)` — read-only, deep-copied engine state through the
//!   injected [`PluginStateReader`]. Unknown paths return `undefined`. There is
//!   deliberately **no** `modV.set`/`commit`: plugins cannot write engine
//!   state (D5 keeps the legacy "mirror-only" model). The reader is the
//!   allow-list boundary: it decides which paths exist.
//! * `modV.ws(url, payload)` — post through the injected
//!   [`PluginTransport`]. `url` must be `ws://`/`wss://`; a string payload is
//!   sent verbatim, other JSON values are serialised with the realm's
//!   `JSON.stringify`. Returns `true` when the transport accepted it. The
//!   transport owns connection reuse/close; the host only supplies `post`.
//! * `log(level, message)` — `debug`/`info`/`warn`/`error`; collected in
//!   [`PluginHost::logs`] for the app's logger. Levels other than the four
//!   named ones become `info`.
//!
//! `require`, `process`, `fetch`, `WebSocket`, `XMLHttpRequest`, timers,
//! filesystem and network access do not exist. `module`/`exports` exist only
//! during the source evaluation and are deleted afterwards.
//!
//! # Safety limits
//!
//! * Source length: [`MAX_PLUGIN_SOURCE_BYTES`] (1 MiB).
//! * A lexical nesting/unary-run cap plus a parse on a large-stack thread
//!   (Boa's parser is recursive) before evaluation. The lexical scan is
//!   conservative: bracket characters inside regex literals also count toward
//!   the nesting cap.
//! * Boa [`RuntimeLimits`]: [`PLUGIN_LOOP_LIMIT`] loop iterations per call
//!   frame, [`PLUGIN_RECURSION_LIMIT`] call depth and a small value-stack cap.
//!   A `while (true) {}` hook errors out instead of wedging the engine thread.
//! * Capture: [`MAX_CAPTURE_DIMENSION`] per side and [`MAX_CAPTURE_BYTES`]
//!   total (RGBA8), validated by [`FrameCapture::new`].
//! * Collections are capped (plugins, props, select options, logs, errors);
//!   messages are truncated.
//!
//! There is deliberately **no heap or wall-clock ceiling**: Boa 0.22 does not
//! expose a synchronous instruction budget. JavaScript plugins are opt-in
//! advanced code behind a consent dialog, not the hostile-playlist surface the
//! expression sandbox defends against; the loop, recursion and source caps
//! bound the common runaway cases (a plugin can still allocate large arrays
//! inside the loop budget).
//!
//! # Failure isolation and logging
//!
//! Loading, `init` and every frame hook are independent per plugin. A throwing
//! hook is skipped, recorded **once per `(plugin, phase)`** and the remaining
//! plugins run. A load/`init` failure disables only that plugin (its descriptor
//! stays visible to the UI as `failed`). A Rust panic escaping Boa is caught,
//! recorded and unloads that plugin's context. The host never panics on plugin
//! input.
//!
//! # Opt-in lifecycle
//!
//! A disabled host allocates **zero** Boa contexts and never evaluates plugin
//! source. [`PluginHost::set_enabled`] loads and initialises every registered
//! source; disabling runs `shutdown` and drops every context (sources and prop
//! values are kept so re-enabling restores the same plugins). The app owns the
//! persisted "enable JavaScript plugins" setting and constructs the host with
//! it; this module persists nothing.

use std::cell::RefCell;
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use boa_engine::{
    Context, JsArgs, JsObject, JsString, JsValue, NativeFunction, Source,
    ast::scope::Scope,
    gc::{Finalize, Trace},
    interner::Interner,
    object::builtins::{JsArray, JsUint8Array},
    parser::Parser,
    property::{Attribute, PropertyKey},
    vm::RuntimeLimits,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The largest width or height a plugin may declare for frame capture.
pub const MAX_CAPTURE_DIMENSION: u32 = 1024;

/// The largest number of RGBA bytes a capture may contain.
pub const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

/// The largest plugin source accepted (before a Boa context is allocated).
pub const MAX_PLUGIN_SOURCE_BYTES: usize = 1024 * 1024;

/// The largest number of registered plugin sources.
pub const MAX_PLUGINS: usize = 64;

/// The largest number of props one plugin may declare.
pub const MAX_PLUGIN_PROPS: usize = 128;

/// The largest number of options one `select` prop may declare.
pub const MAX_PROP_OPTIONS: usize = 128;

/// The largest number of `log()` lines retained between drains.
pub const MAX_LOGS: usize = 512;

/// The largest number of errors retained (deduplicated by plugin + phase).
pub const MAX_ERRORS: usize = 128;

/// Boa loop iterations allowed per plugin call frame.
///
/// This bounds every `while`/`for` loop (and every hook) without a wall-clock
/// interrupt, which Boa 0.22 does not expose on the synchronous call path. A
/// frame hook iterating a 7x7 mapping is far below it; a runaway loop errors.
pub const PLUGIN_LOOP_LIMIT: u64 = 1_000_000;

/// Boa call-depth limit inside one plugin context.
pub const PLUGIN_RECURSION_LIMIT: usize = 32;

/// Boa value-stack limit (number of values, not bytes) inside one context.
pub const PLUGIN_STACK_LIMIT: usize = 4096;

const MAX_PLUGIN_NAME_CHARS: usize = 120;
const MAX_AUTHOR_CHARS: usize = 200;
const MAX_VERSION_CHARS: usize = 64;
const MAX_PROP_NAME_CHARS: usize = 64;
const MAX_PROP_TEXT_CHARS: usize = 4096;
const MAX_OPTION_LABEL_CHARS: usize = 256;
const MAX_ERROR_MESSAGE_CHARS: usize = 512;
const MAX_LOG_MESSAGE_CHARS: usize = 4096;
const MAX_STATE_PATH_BYTES: usize = 256;
const MAX_WS_URL_BYTES: usize = 2048;
const MAX_WS_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_NESTING: usize = 128;
const PLUGIN_PARSER_STACK: usize = 256 * 1024 * 1024;
const FORBIDDEN_PROP_NAMES: &[&str] = &["__proto__", "constructor", "prototype"];

/// A stable plugin identifier, allocated by the host in registration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PluginId(u64);

impl PluginId {
    /// The numeric id.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "plugin-{}", self.0)
    }
}

/// The prop kinds the plugin descriptor accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginPropKind {
    /// A whole number.
    Int,
    /// A floating-point number.
    Float,
    /// A boolean toggle.
    Bool,
    /// Free text.
    Text,
    /// An RGBA colour whose channels are `0..=1`.
    Color,
    /// One of a fixed list of options (accepts the legacy `enum` alias).
    Select,
}

impl PluginPropKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            "text" => Some(Self::Text),
            "color" => Some(Self::Color),
            "select" | "enum" => Some(Self::Select),
            _ => None,
        }
    }

    fn neutral(self) -> Value {
        match self {
            Self::Int => Value::from(0),
            Self::Float => Value::from(0.0),
            Self::Bool => Value::Bool(false),
            Self::Text => Value::String(String::new()),
            Self::Color => color_value(0.0, 0.0, 0.0, 1.0),
            Self::Select => Value::Null,
        }
    }
}

/// One option of a [`PluginPropKind::Select`] prop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginPropOption {
    /// Human-readable label for the generated UI.
    pub label: String,
    /// Scalar value written into `props`.
    pub value: Value,
}

/// A plugin prop: the schema plus the current value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginProp {
    /// The value type, serialised as `type`.
    #[serde(rename = "type")]
    pub kind: PluginPropKind,
    /// The default resolved at load time.
    pub default: Value,
    /// The current value (kept in sync with the plugin's `props` object).
    pub value: Value,
    /// Inclusive lower bound for numeric props.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Inclusive upper bound for numeric props.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// UI step for numeric props.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// Take the absolute value of a numeric prop.
    #[serde(default)]
    pub abs: bool,
    /// Select options.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<PluginPropOption>,
}

impl PluginProp {
    /// Coerce a Rust/JSON value against this schema.
    fn coerce_json(&self, value: &Value) -> Value {
        match self.kind {
            PluginPropKind::Int => self.coerce_number(value.as_f64(), true),
            PluginPropKind::Float => self.coerce_number(value.as_f64(), false),
            PluginPropKind::Bool => Value::Bool(match value {
                Value::Bool(flag) => *flag,
                Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
                _ => self.default.as_bool().unwrap_or(false),
            }),
            PluginPropKind::Text => value
                .as_str()
                .map(|text| Value::String(truncate_chars(text, MAX_PROP_TEXT_CHARS)))
                .unwrap_or_else(|| self.default.clone()),
            PluginPropKind::Color => {
                coerce_color_json(value).unwrap_or_else(|| self.default.clone())
            }
            PluginPropKind::Select => {
                if self
                    .options
                    .iter()
                    .any(|option| select_matches(&option.value, value))
                {
                    value.clone()
                } else {
                    self.default.clone()
                }
            }
        }
    }

    /// Coerce a live JS value against this schema.
    fn coerce_js(&self, value: &JsValue, context: &mut Context) -> Result<Value, String> {
        match self.kind {
            PluginPropKind::Int => Ok(self.coerce_number(value.as_number(), true)),
            PluginPropKind::Float => Ok(self.coerce_number(value.as_number(), false)),
            PluginPropKind::Bool => Ok(Value::Bool(
                value
                    .as_boolean()
                    .or_else(|| value.as_number().map(|number| number != 0.0))
                    .unwrap_or_else(|| self.default.as_bool().unwrap_or(false)),
            )),
            PluginPropKind::Text => Ok(value
                .as_string()
                .map(|text| {
                    Value::String(truncate_chars(
                        &text.to_std_string_escaped(),
                        MAX_PROP_TEXT_CHARS,
                    ))
                })
                .unwrap_or_else(|| self.default.clone())),
            PluginPropKind::Color => {
                let Some(object) = value.as_object() else {
                    return Ok(self.default.clone());
                };
                let (default_r, default_g, default_b, default_a) = color_channels(&self.default);
                let r = color_channel(&object, "r", default_r, context)?;
                let g = color_channel(&object, "g", default_g, context)?;
                let b = color_channel(&object, "b", default_b, context)?;
                let a = color_channel(&object, "a", default_a, context)?;
                Ok(color_value(r, g, b, a))
            }
            PluginPropKind::Select => {
                for option in &self.options {
                    let candidate = JsValue::from_json(&option.value, context)
                        .map_err(|error| error.to_string())?;
                    if candidate.strict_equals(value) {
                        return Ok(option.value.clone());
                    }
                }
                Ok(self.default.clone())
            }
        }
    }

    fn coerce_number(&self, number: Option<f64>, round: bool) -> Value {
        let Some(mut number) = number.filter(|number| number.is_finite()) else {
            return self.default.clone();
        };
        if let Some(min) = self.min {
            number = number.max(min);
        }
        if let Some(max) = self.max {
            number = number.min(max);
        }
        if self.abs {
            number = number.abs();
        }
        if round {
            number = (number + 0.5).floor();
            if number >= i64::MIN as f64 && number <= i64::MAX as f64 {
                return Value::from(number as i64);
            }
            return self.default.clone();
        }
        serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or_else(|| self.default.clone())
    }

    /// Read a prop schema from the descriptor object.
    fn from_js(name: &str, value: &JsValue, context: &mut Context) -> Result<Self, String> {
        let Some(object) = value.as_object() else {
            return Err(format!("prop `{name}` must be an object"));
        };
        if object.is_callable() {
            return Err(format!("prop `{name}` must be a schema object"));
        }

        let type_value = object
            .get(JsString::from("type"), context)
            .map_err(|error| error.to_string())?;
        let type_name = type_value
            .as_string()
            .map(|name| name.to_std_string_escaped())
            .ok_or_else(|| format!("prop `{name}` type must be a string"))?;
        let kind = PluginPropKind::parse(&type_name)
            .ok_or_else(|| format!("prop `{name}` has unsupported type `{type_name}`"))?;

        let min = read_number(&object, "min", context)?;
        let max = read_number(&object, "max", context)?;
        if let (Some(min), Some(max)) = (min, max)
            && min > max
        {
            return Err(format!("prop `{name}` has min > max"));
        }
        let step = read_number(&object, "step", context)?;
        if let Some(step) = step
            && step <= 0.0
        {
            return Err(format!("prop `{name}` has a non-positive step"));
        }
        let abs = object
            .get(JsString::from("abs"), context)
            .map_err(|error| error.to_string())?
            .as_boolean()
            .unwrap_or(false);

        let options = if kind == PluginPropKind::Select {
            read_options(&object, context)?
        } else {
            Vec::new()
        };
        if kind == PluginPropKind::Select && options.is_empty() {
            return Err(format!("select prop `{name}` needs at least one option"));
        }

        let mut prop = Self {
            kind,
            default: kind.neutral(),
            value: kind.neutral(),
            min,
            max,
            step,
            abs,
            options,
        };

        let default_value = object
            .get(JsString::from("default"), context)
            .map_err(|error| error.to_string())?;
        let default = if default_value.is_undefined() || default_value.is_null() {
            if kind == PluginPropKind::Select {
                prop.options[0].value.clone()
            } else {
                kind.neutral()
            }
        } else {
            prop.coerce_js(&default_value, context)?
        };

        if kind == PluginPropKind::Select {
            prop.default = if prop
                .options
                .iter()
                .any(|option| select_matches(&option.value, &default))
            {
                default
            } else {
                prop.options[0].value.clone()
            };
        } else {
            prop.default = default;
        }
        prop.value = prop.default.clone();
        Ok(prop)
    }
}

/// The capture size the app should read back from the composited frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// Capture width in pixels.
    pub width: u32,
    /// Capture height in pixels.
    pub height: u32,
}

/// A validated RGBA8 frame capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameCapture {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl FrameCapture {
    /// Create a capture, validating the dimensions and buffer length.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, CaptureError> {
        validate_capture_shape(width, height, pixels.len())?;
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Capture width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Capture height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// RGBA8 pixels in row-major order.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Downscale to `width`x`height` with a box filter.
    pub fn downscale(&self, width: u32, height: u32) -> Result<Self, CaptureError> {
        let expected = validate_capture_shape(
            width,
            height,
            (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4),
        )?;
        let mut pixels = Vec::with_capacity(expected);
        for y in 0..height {
            let y0 = (u64::from(y) * u64::from(self.height) / u64::from(height)) as u32;
            let y1 = (((u64::from(y) + 1) * u64::from(self.height) / u64::from(height)) as u32)
                .max(y0 + 1)
                .min(self.height);
            for x in 0..width {
                let x0 = (u64::from(x) * u64::from(self.width) / u64::from(width)) as u32;
                let x1 = (((u64::from(x) + 1) * u64::from(self.width) / u64::from(width)) as u32)
                    .max(x0 + 1)
                    .min(self.width);
                let mut sums = [0u64; 4];
                let mut count = 0u64;
                for source_y in y0..y1 {
                    for source_x in x0..x1 {
                        let index =
                            ((source_y as usize * self.width as usize) + source_x as usize) * 4;
                        for channel in 0..4 {
                            sums[channel] += u64::from(self.pixels[index + channel]);
                        }
                        count += 1;
                    }
                }
                for sum in sums {
                    pixels.push((sum / count) as u8);
                }
            }
        }
        Self::new(width, height, pixels)
    }

    fn validate(&self) -> Result<(), CaptureError> {
        validate_capture_shape(self.width, self.height, self.pixels.len()).map(|_| ())
    }
}

/// An invalid frame capture.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CaptureError {
    /// A dimension is zero.
    #[error("capture dimensions must be non-zero")]
    Empty,
    /// A dimension or the byte count exceeds the host cap.
    #[error(
        "capture {width}x{height} exceeds the {MAX_CAPTURE_DIMENSION}px / {MAX_CAPTURE_BYTES}-byte cap"
    )]
    TooLarge {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The pixel buffer length does not match the dimensions.
    #[error("capture has {actual} bytes, expected {expected} RGBA bytes")]
    WrongLength {
        /// Expected byte length.
        expected: usize,
        /// Actual byte length.
        actual: usize,
    },
}

/// The lifecycle phase an error came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginPhase {
    /// Source evaluation / descriptor parsing.
    Load,
    /// `init`.
    Init,
    /// `preProcessFrame`.
    PreFrame,
    /// `postProcessFrame`.
    PostFrame,
    /// `shutdown`.
    Shutdown,
    /// Reading a prop back from the plugin.
    Props,
    /// `modV.ws`.
    Transport,
    /// Frame capture validation.
    Capture,
    /// Host-level limit/rejection.
    Host,
}

/// A plugin failure, reported once per `(plugin, phase)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginError {
    /// The plugin id, when the failure belongs to a registered plugin.
    pub plugin_id: Option<PluginId>,
    /// The plugin name, when it was known at failure time.
    pub plugin_name: Option<String>,
    /// The lifecycle phase.
    pub phase: PluginPhase,
    /// The error message.
    pub message: String,
}

/// A `log()` level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginLogLevel {
    /// `log("debug", ...)`.
    Debug,
    /// `log("info", ...)`.
    Info,
    /// `log("warn", ...)`.
    Warn,
    /// `log("error", ...)`.
    Error,
}

impl PluginLogLevel {
    fn from_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "debug" => Self::Debug,
            "warn" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

/// One line written by a plugin through `log()`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginLog {
    /// The plugin id.
    pub plugin_id: PluginId,
    /// The plugin name, when known.
    pub plugin_name: Option<String>,
    /// The requested level.
    pub level: PluginLogLevel,
    /// The message, truncated to the host cap.
    pub message: String,
}

/// A snapshot of one registered plugin for the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    /// Stable host id.
    pub id: PluginId,
    /// Descriptor name (empty until the source loads).
    pub name: String,
    /// Descriptor author.
    pub author: String,
    /// Descriptor version.
    pub version: String,
    /// The per-plugin user switch.
    pub active: bool,
    /// Whether the plugin currently runs hooks (`host enabled && active && loaded`).
    pub enabled: bool,
    /// Whether a Boa context is loaded and `init` succeeded.
    pub loaded: bool,
    /// Whether loading/initialising failed.
    pub failed: bool,
    /// Props with their current values.
    pub props: IndexMap<String, PluginProp>,
    /// The plugin's requested capture size.
    pub capture: Option<CaptureConfig>,
}

/// Read-only engine state for `modV.get(path)`.
pub trait PluginStateReader {
    /// Return the JSON value at `path`, or `None` for an unknown path.
    fn get(&self, path: &str) -> Option<Value>;
}

/// The host-provided WebSocket transport for `modV.ws(url, payload)`.
pub trait PluginTransport {
    /// Post `payload` to `url`.
    fn post(&mut self, url: &str, payload: &str) -> Result<(), String>;
}

/// The host environment supplied for one frame/lifecycle call.
///
/// The app builds one per frame (cheap: both trait handles are `Rc` clones) and
/// updates `features` from the current audio payload before calling
/// [`PluginHost::pre_frame`].
#[derive(Clone)]
pub struct PluginIo {
    /// The audio feature payload passed to hooks.
    pub features: Value,
    /// Read-only state for `modV.get`.
    pub state: Rc<dyn PluginStateReader>,
    /// WebSocket transport for `modV.ws`.
    pub transport: Rc<RefCell<dyn PluginTransport>>,
}

impl PluginIo {
    /// Build an IO bundle.
    pub fn new(
        features: Value,
        state: Rc<dyn PluginStateReader>,
        transport: Rc<RefCell<dyn PluginTransport>>,
    ) -> Self {
        Self {
            features,
            state,
            transport,
        }
    }

    /// An IO bundle with no state and a transport that rejects posts.
    ///
    /// Used by [`PluginHost::shutdown`] when the app has already torn down its
    /// transport; hooks see `modV.get(...) === undefined` and
    /// `modV.ws(...) === false`.
    pub fn empty() -> Self {
        let transport: Rc<RefCell<dyn PluginTransport>> = Rc::new(RefCell::new(NullTransport));
        Self::new(Value::Null, Rc::new(EmptyState), transport)
    }
}

struct EmptyState;

impl PluginStateReader for EmptyState {
    fn get(&self, _path: &str) -> Option<Value> {
        None
    }
}

struct NullTransport;

impl PluginTransport for NullTransport {
    fn post(&mut self, _url: &str, _payload: &str) -> Result<(), String> {
        Err("no plugin transport installed".to_owned())
    }
}

/// The opt-in plugin host.
pub struct PluginHost {
    enabled: bool,
    next_id: u64,
    slots: Vec<PluginSlot>,
    shared: SharedHandle,
}

struct PluginSlot {
    id: PluginId,
    source: String,
    descriptor: Option<PluginDescriptor>,
    runtime: Option<PluginRuntime>,
    active: bool,
    failed: bool,
}

impl PluginSlot {
    fn loaded(&self) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(|runtime| runtime.initialized)
    }
}

#[derive(Clone)]
struct PluginDescriptor {
    name: String,
    author: String,
    version: String,
    props: IndexMap<String, PluginProp>,
    capture: Option<CaptureConfig>,
}

struct PluginRuntime {
    context: Context,
    exports: JsObject,
    props_object: JsObject,
    modv_object: JsObject,
    log_function: JsObject,
    init: Option<JsObject>,
    pre_process: Option<JsObject>,
    post_process: Option<JsObject>,
    shutdown: Option<JsObject>,
    initialized: bool,
}

/// Host state visible to native functions.
///
/// `SharedHandle` is captured by Boa native closures, so it must implement
/// `Trace`; it holds no `Gc` pointers, only Rust data (logs, errors and the
/// current `PluginIo`), hence the empty trace.
#[derive(Clone)]
struct SharedHandle(Rc<RefCell<Shared>>);

impl Finalize for SharedHandle {}

// SAFETY: `Shared` contains no `boa_gc` managed values, so there is nothing to
// trace. The `Rc` is a plain Rust allocation owned by the host and the native
// function captures.
unsafe impl Trace for SharedHandle {
    unsafe fn trace(&self, _tracer: &mut boa_engine::gc::Tracer) {}
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {
        Finalize::finalize(self);
    }
}

#[derive(Default)]
struct Shared {
    logs: Vec<PluginLog>,
    errors: Vec<PluginError>,
    error_keys: HashSet<(Option<PluginId>, PluginPhase)>,
    current: Option<CurrentPlugin>,
    io: Option<Rc<PluginIo>>,
}

#[derive(Clone)]
struct CurrentPlugin {
    id: PluginId,
    name: Option<String>,
}

/// Installs the frame/lifecycle IO for the duration of a host call.
///
/// Nested installation is a no-op so one `pre_frame` clone serves every plugin.
struct ActiveIoGuard {
    shared: SharedHandle,
    installed: bool,
}

impl ActiveIoGuard {
    fn ensure(shared: &SharedHandle, io: &PluginIo) -> Self {
        let installed = {
            let mut state = shared.0.borrow_mut();
            if state.io.is_none() {
                state.io = Some(Rc::new(io.clone()));
                true
            } else {
                false
            }
        };
        Self {
            shared: shared.clone(),
            installed,
        }
    }
}

impl Drop for ActiveIoGuard {
    fn drop(&mut self) {
        if !self.installed {
            return;
        }
        if let Ok(mut state) = self.shared.0.try_borrow_mut() {
            state.io = None;
        }
    }
}

/// Sets the plugin id/name used by `log()` and transport errors, then clears it.
struct CurrentGuard {
    shared: SharedHandle,
}

impl CurrentGuard {
    fn install(shared: &SharedHandle, id: PluginId, name: Option<String>) -> Self {
        shared.0.borrow_mut().current = Some(CurrentPlugin { id, name });
        Self {
            shared: shared.clone(),
        }
    }
}

impl Drop for CurrentGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.0.try_borrow_mut() {
            state.current = None;
        }
    }
}

/// A failed hook invocation.
enum HookFailure {
    /// A JavaScript exception (the context stays usable).
    Js(String),
    /// A Rust panic escaping Boa (the context is dropped by the caller).
    Panic(String),
}

impl HookFailure {
    fn message(self) -> String {
        match self {
            Self::Js(message) | Self::Panic(message) => message,
        }
    }
}

#[derive(Clone, Copy)]
enum HookKind {
    Pre,
    Post,
}

impl HookKind {
    fn phase(self) -> PluginPhase {
        match self {
            Self::Pre => PluginPhase::PreFrame,
            Self::Post => PluginPhase::PostFrame,
        }
    }
}

impl PluginHost {
    /// Create a host. A disabled host allocates no Boa contexts.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            next_id: 0,
            slots: Vec::new(),
            shared: SharedHandle(Rc::new(RefCell::new(Shared::default()))),
        }
    }

    /// Whether the host switch is on.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Register a plugin source.
    ///
    /// No Boa context is allocated here: the source is stored and loaded by
    /// [`Self::load_pending`] (or by enabling the host) once an IO is
    /// available. [`Self::add_source_with_io`] is the convenience for adding a
    /// source to an already-enabled host.
    pub fn add_source(&mut self, source: impl Into<String>) -> Result<PluginId, PluginError> {
        let source = source.into();
        if self.slots.len() >= MAX_PLUGINS {
            return Err(self.host_error(
                PluginPhase::Host,
                format!("at most {MAX_PLUGINS} plugins may be registered"),
            ));
        }
        if source.len() > MAX_PLUGIN_SOURCE_BYTES {
            return Err(self.host_error(
                PluginPhase::Load,
                format!("plugin source exceeds {MAX_PLUGIN_SOURCE_BYTES} bytes"),
            ));
        }

        let id = PluginId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        self.slots.push(PluginSlot {
            id,
            source,
            descriptor: None,
            runtime: None,
            active: true,
            failed: false,
        });
        Ok(id)
    }

    /// Register a source and, when the host is enabled, load it immediately.
    pub fn add_source_with_io(
        &mut self,
        source: impl Into<String>,
        io: &mut PluginIo,
    ) -> Result<PluginId, PluginError> {
        let id = self.add_source(source)?;
        if self.enabled {
            self.load_pending(io);
        }
        Ok(id)
    }

    /// Enable or disable the host.
    ///
    /// Enabling loads and initialises every active source that is not already
    /// running (failed sources are retried). Disabling runs `shutdown`, drops
    /// every Boa context and keeps the registered sources for a later enable.
    pub fn set_enabled(&mut self, enabled: bool, io: &mut PluginIo) {
        if enabled {
            self.enabled = true;
            let retry: Vec<PluginId> = self
                .slots
                .iter()
                .filter(|slot| slot.active && !slot.loaded())
                .map(|slot| slot.id)
                .collect();
            for id in retry {
                clear_errors_for(&self.shared, id);
            }
            for slot in &mut self.slots {
                if slot.active && !slot.loaded() {
                    slot.failed = false;
                }
            }
            self.load_pending(io);
        } else {
            self.disable(io);
        }
    }

    /// Load and initialise every pending/active source.
    ///
    /// Returns how many plugins became loaded. Does nothing while the host is
    /// disabled.
    pub fn load_pending(&mut self, io: &mut PluginIo) -> usize {
        if !self.enabled {
            return 0;
        }
        let mut loaded = 0;
        for index in 0..self.slots.len() {
            let slot = &self.slots[index];
            if !slot.active || slot.failed || slot.runtime.is_some() {
                continue;
            }
            if self.load_slot(index, io).is_ok() {
                loaded += 1;
            }
        }
        loaded
    }

    /// Enable or disable a single plugin.
    ///
    /// Disabling runs its `shutdown` and drops its context (the descriptor and
    /// prop values stay available to the UI). Enabling loads/initialises it
    /// when the host is on, otherwise it stays pending until the host enables.
    pub fn set_plugin_enabled(
        &mut self,
        id: PluginId,
        enabled: bool,
        io: &mut PluginIo,
    ) -> Result<(), PluginError> {
        let Some(index) = self.index_of(id) else {
            return Err(self.host_error(PluginPhase::Host, format!("unknown plugin `{id}`")));
        };
        self.slots[index].active = enabled;
        if enabled {
            clear_errors_for(&self.shared, id);
            self.slots[index].failed = false;
            if self.enabled && !self.slots[index].loaded() {
                self.load_slot(index, io)?;
            }
        } else {
            self.unload_slot(index, io);
        }
        Ok(())
    }

    /// Drop a plugin and its context.
    pub fn remove(&mut self, id: PluginId, io: &mut PluginIo) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.unload_slot(index, io);
        self.slots.remove(index);
        true
    }

    /// Re-evaluate and re-initialise one plugin.
    pub fn reload(&mut self, id: PluginId, io: &mut PluginIo) -> Result<(), PluginError> {
        let Some(index) = self.index_of(id) else {
            return Err(self.host_error(PluginPhase::Host, format!("unknown plugin `{id}`")));
        };
        self.unload_slot(index, io);
        clear_errors_for(&self.shared, id);
        self.slots[index].failed = false;
        if self.enabled && self.slots[index].active {
            self.load_slot(index, io)?;
        }
        Ok(())
    }

    /// Run every enabled `preProcessFrame` hook in registration order.
    pub fn pre_frame(&mut self, io: &mut PluginIo) {
        if !self.enabled {
            return;
        }
        let _io_guard = ActiveIoGuard::ensure(&self.shared, io);
        for index in 0..self.slots.len() {
            if !self.slots[index].active || !self.slots[index].loaded() {
                continue;
            }
            self.call_slot_hook(index, HookKind::Pre, io, None);
        }
    }

    /// Run every enabled `postProcessFrame` hook in registration order.
    ///
    /// The supplied capture is validated once; each plugin that declared a
    /// capture size receives its own downscaled copy. An invalid capture
    /// returns [`CaptureError`] without running any hook.
    pub fn post_frame(
        &mut self,
        io: &mut PluginIo,
        capture: Option<&FrameCapture>,
    ) -> Result<(), CaptureError> {
        if let Some(capture) = capture
            && let Err(error) = capture.validate()
        {
            push_error(
                &self.shared,
                None,
                None,
                PluginPhase::Capture,
                error.to_string(),
            );
            return Err(error);
        }
        if !self.enabled {
            return Ok(());
        }
        let _io_guard = ActiveIoGuard::ensure(&self.shared, io);
        for index in 0..self.slots.len() {
            if !self.slots[index].active || !self.slots[index].loaded() {
                continue;
            }
            let has_post = self.slots[index]
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.post_process.is_some());
            if !has_post {
                continue;
            }
            let requested = self.slots[index]
                .descriptor
                .as_ref()
                .and_then(|descriptor| descriptor.capture);
            let resized = match (capture, requested) {
                (Some(capture), Some(config))
                    if capture.width != config.width || capture.height != config.height =>
                {
                    match capture.downscale(config.width, config.height) {
                        Ok(capture) => Some(capture),
                        Err(error) => {
                            push_error(
                                &self.shared,
                                None,
                                None,
                                PluginPhase::Capture,
                                error.to_string(),
                            );
                            return Err(error);
                        }
                    }
                }
                _ => None,
            };
            let capture = resized.as_ref().or(capture);
            self.call_slot_hook(index, HookKind::Post, io, capture);
        }
        Ok(())
    }

    /// Plugin snapshots in registration order.
    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.slots
            .iter()
            .map(|slot| {
                let descriptor = slot.descriptor.as_ref();
                PluginInfo {
                    id: slot.id,
                    name: descriptor.map(|data| data.name.clone()).unwrap_or_default(),
                    author: descriptor
                        .map(|data| data.author.clone())
                        .unwrap_or_default(),
                    version: descriptor
                        .map(|data| data.version.clone())
                        .unwrap_or_default(),
                    active: slot.active,
                    enabled: self.enabled && slot.active && slot.loaded(),
                    loaded: slot.loaded(),
                    failed: slot.failed,
                    props: descriptor
                        .map(|data| data.props.clone())
                        .unwrap_or_default(),
                    capture: descriptor.and_then(|data| data.capture),
                }
            })
            .collect()
    }

    /// How many plugins have a live context.
    pub fn loaded_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.loaded()).count()
    }

    /// Collected failures (deduplicated by plugin + phase).
    pub fn errors(&self) -> Vec<PluginError> {
        self.shared.0.borrow().errors.clone()
    }

    /// Collected `log()` lines, without draining them.
    pub fn logs(&self) -> Vec<PluginLog> {
        self.shared.0.borrow().logs.clone()
    }

    /// Take the collected `log()` lines for forwarding to the app logger.
    pub fn drain_logs(&mut self) -> Vec<PluginLog> {
        std::mem::take(&mut self.shared.0.borrow_mut().logs)
    }

    /// Drop collected errors (for example after the UI acknowledged them).
    pub fn clear_errors(&mut self) {
        let mut shared = self.shared.0.borrow_mut();
        shared.errors.clear();
        shared.error_keys.clear();
    }

    /// The largest capture requested by a running plugin, or `None`.
    ///
    /// The app reads one frame at this size and [`Self::post_frame`] gives each
    /// plugin its own declared size (box-downscaled) from it.
    pub fn capture_config(&self) -> Option<CaptureConfig> {
        if !self.enabled {
            return None;
        }
        let mut requested: Option<CaptureConfig> = None;
        for slot in &self.slots {
            if !slot.active || !slot.loaded() {
                continue;
            }
            let has_post = slot
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.post_process.is_some());
            if !has_post {
                continue;
            }
            let Some(capture) = slot.descriptor.as_ref().and_then(|data| data.capture) else {
                continue;
            };
            requested = Some(match requested {
                Some(current) => CaptureConfig {
                    width: current.width.max(capture.width),
                    height: current.height.max(capture.height),
                },
                None => capture,
            });
        }
        requested
    }

    /// Set a prop value, returning the coerced value.
    ///
    /// Works while the host/plugin is disabled (the value is used the next time
    /// the plugin loads) and updates the live `props` object when one exists.
    pub fn set_prop(
        &mut self,
        id: PluginId,
        prop: &str,
        value: Value,
    ) -> Result<Value, PluginError> {
        let shared = self.shared.clone();
        let Some(index) = self.index_of(id) else {
            return Err(push_error(
                &shared,
                None,
                None,
                PluginPhase::Props,
                format!("unknown plugin `{id}`"),
            ));
        };
        let slot = &mut self.slots[index];
        let plugin_name = slot.descriptor.as_ref().map(|data| data.name.clone());
        let Some(descriptor) = slot.descriptor.as_mut() else {
            return Err(push_error(
                &shared,
                Some(id),
                plugin_name,
                PluginPhase::Props,
                format!("plugin `{id}` has not loaded a prop schema yet"),
            ));
        };
        let Some(schema) = descriptor.props.get_mut(prop) else {
            return Err(push_error(
                &shared,
                Some(id),
                Some(descriptor.name.clone()),
                PluginPhase::Props,
                format!("plugin `{}` has no prop `{prop}`", descriptor.name),
            ));
        };
        let coerced = schema.coerce_json(&value);
        schema.value = coerced.clone();

        if let Some(runtime) = slot.runtime.as_mut() {
            let value = match JsValue::from_json(&coerced, &mut runtime.context) {
                Ok(value) => value,
                Err(error) => {
                    return Err(push_error(
                        &shared,
                        Some(id),
                        Some(descriptor.name.clone()),
                        PluginPhase::Props,
                        format!("could not convert prop `{prop}`: {error}"),
                    ));
                }
            };
            if let Err(error) = runtime.props_object.create_data_property(
                JsString::from(prop),
                value,
                &mut runtime.context,
            ) {
                return Err(push_error(
                    &shared,
                    Some(id),
                    Some(descriptor.name.clone()),
                    PluginPhase::Props,
                    format!("could not write prop `{prop}`: {error}"),
                ));
            }
        }
        Ok(coerced)
    }

    /// Run `shutdown` and drop every context without a frame IO.
    ///
    /// Used on app quit. Shutdown hooks can log; `modV.get`/`modV.ws` are
    /// unavailable unless the app calls [`Self::set_enabled(false, io)`]
    /// instead.
    pub fn shutdown(&mut self) {
        let mut io = PluginIo::empty();
        self.disable(&mut io);
    }

    fn disable(&mut self, io: &mut PluginIo) {
        for index in 0..self.slots.len() {
            self.unload_slot(index, io);
        }
        self.enabled = false;
    }

    fn index_of(&self, id: PluginId) -> Option<usize> {
        self.slots.iter().position(|slot| slot.id == id)
    }

    fn host_error(&self, phase: PluginPhase, message: impl Into<String>) -> PluginError {
        push_error(&self.shared, None, None, phase, message)
    }

    fn load_slot(&mut self, index: usize, io: &mut PluginIo) -> Result<(), PluginError> {
        let shared = self.shared.clone();
        let id = self.slots[index].id;
        let source = self.slots[index].source.clone();
        let previous = self.slots[index].descriptor.clone();

        let (mut runtime, mut descriptor) =
            match PluginRuntime::load(id, &source, &shared, previous.as_ref(), io) {
                Ok(loaded) => loaded,
                Err(message) => {
                    let name = previous.as_ref().map(|data| data.name.clone());
                    self.slots[index].failed = true;
                    return Err(push_error(
                        &shared,
                        Some(id),
                        name,
                        PluginPhase::Load,
                        message,
                    ));
                }
            };

        if let Some(existing) = self.duplicate_name(&descriptor.name, index) {
            self.slots[index].failed = true;
            return Err(push_error(
                &shared,
                Some(id),
                Some(descriptor.name.clone()),
                PluginPhase::Load,
                format!(
                    "duplicate plugin name `{}` (already used by `{existing}`)",
                    descriptor.name
                ),
            ));
        }

        if let Some(hook) = runtime.init.clone()
            && let Err(failure) =
                invoke_hook(&mut runtime, hook, &mut descriptor, id, &shared, io, None)
        {
            let message = failure.message();
            self.slots[index].descriptor = Some(descriptor);
            self.slots[index].failed = true;
            let name = self.slots[index]
                .descriptor
                .as_ref()
                .map(|data| data.name.clone());
            return Err(push_error(
                &shared,
                Some(id),
                name,
                PluginPhase::Init,
                message,
            ));
        }

        runtime.initialized = true;
        self.slots[index].descriptor = Some(descriptor);
        self.slots[index].runtime = Some(runtime);
        self.slots[index].failed = false;
        Ok(())
    }

    fn duplicate_name(&self, name: &str, except: usize) -> Option<String> {
        self.slots.iter().enumerate().find_map(|(index, slot)| {
            if index == except {
                return None;
            }
            slot.descriptor
                .as_ref()
                .filter(|descriptor| descriptor.name.eq_ignore_ascii_case(name))
                .map(|descriptor| descriptor.name.clone())
        })
    }

    fn unload_slot(&mut self, index: usize, io: &mut PluginIo) {
        let shared = self.shared.clone();
        let id = self.slots[index].id;
        let slot = &mut self.slots[index];
        let Some(mut runtime) = slot.runtime.take() else {
            return;
        };
        let Some(descriptor) = slot.descriptor.as_mut() else {
            return;
        };
        if runtime.initialized
            && let Some(hook) = runtime.shutdown.clone()
            && let Err(failure) = invoke_hook(&mut runtime, hook, descriptor, id, &shared, io, None)
        {
            push_error(
                &shared,
                Some(id),
                Some(descriptor.name.clone()),
                PluginPhase::Shutdown,
                failure.message(),
            );
        }
        runtime.initialized = false;
    }

    fn call_slot_hook(
        &mut self,
        index: usize,
        kind: HookKind,
        io: &mut PluginIo,
        capture: Option<&FrameCapture>,
    ) {
        let shared = self.shared.clone();
        let id = self.slots[index].id;
        let slot = &mut self.slots[index];
        let Some(mut runtime) = slot.runtime.take() else {
            return;
        };
        let Some(descriptor) = slot.descriptor.as_mut() else {
            slot.runtime = Some(runtime);
            return;
        };
        let hook = match kind {
            HookKind::Pre => runtime.pre_process.clone(),
            HookKind::Post => runtime.post_process.clone(),
        };
        let Some(hook) = hook else {
            slot.runtime = Some(runtime);
            return;
        };
        let name = descriptor.name.clone();
        match invoke_hook(&mut runtime, hook, descriptor, id, &shared, io, capture) {
            Ok(()) => {
                slot.runtime = Some(runtime);
            }
            Err(HookFailure::Js(message)) => {
                push_error(&shared, Some(id), Some(name), kind.phase(), message);
                slot.runtime = Some(runtime);
            }
            Err(HookFailure::Panic(message)) => {
                push_error(&shared, Some(id), Some(name), kind.phase(), message);
                // A panic can leave Boa's internal RefCells borrowed; drop the
                // context rather than reusing a poisoned realm.
                slot.failed = true;
            }
        }
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl PluginRuntime {
    fn load(
        id: PluginId,
        source: &str,
        shared: &SharedHandle,
        previous: Option<&PluginDescriptor>,
        io: &mut PluginIo,
    ) -> Result<(Self, PluginDescriptor), String> {
        check_source(source)?;
        validate_source_parse(source)?;

        let mut context = Context::default();
        let mut limits = RuntimeLimits::default();
        limits.set_loop_iteration_limit(PLUGIN_LOOP_LIMIT);
        limits.set_recursion_limit(PLUGIN_RECURSION_LIMIT);
        limits.set_stack_size_limit(PLUGIN_STACK_LIMIT);
        context.set_runtime_limits(limits);

        let props_object = JsObject::with_object_proto(context.intrinsics());
        let modv_object = make_modv_object(&mut context, shared)?;
        let log_function = make_log_function(&mut context, shared)?;
        context
            .register_global_property(
                JsString::from("props"),
                props_object.clone(),
                Attribute::all(),
            )
            .map_err(|error| error.to_string())?;
        context
            .register_global_property(
                JsString::from("modV"),
                modv_object.clone(),
                Attribute::all(),
            )
            .map_err(|error| error.to_string())?;
        context
            .register_global_property(
                JsString::from("log"),
                log_function.clone(),
                Attribute::all(),
            )
            .map_err(|error| error.to_string())?;

        let exports_object = JsObject::with_object_proto(context.intrinsics());
        let module_object = JsObject::with_object_proto(context.intrinsics());
        module_object
            .create_data_property(
                JsString::from("exports"),
                exports_object.clone(),
                &mut context,
            )
            .map_err(|error| error.to_string())?;
        context
            .register_global_property(
                JsString::from("module"),
                module_object.clone(),
                Attribute::all(),
            )
            .map_err(|error| error.to_string())?;
        context
            .register_global_property(JsString::from("exports"), exports_object, Attribute::all())
            .map_err(|error| error.to_string())?;

        {
            let _io_guard = ActiveIoGuard::ensure(shared, io);
            let _current = CurrentGuard::install(shared, id, None);
            let evaluated = catch_unwind(AssertUnwindSafe(|| {
                context.eval(Source::from_bytes(source.as_bytes()))
            }));
            match evaluated {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    return Err(format!("plugin evaluation failed: {error}"));
                }
                Err(payload) => {
                    return Err(format!(
                        "plugin evaluation panicked: {}",
                        panic_message(&*payload)
                    ));
                }
            }
        }

        let parsed = catch_unwind(AssertUnwindSafe(|| {
            let exports = resolve_exports(&module_object, &mut context)?;
            remove_load_globals(&mut context);
            let mut descriptor = parse_descriptor(&exports, &mut context)?;

            if let Some(previous) = previous {
                for (name, prop) in &mut descriptor.props {
                    if let Some(old) = previous.props.get(name) {
                        prop.value = prop.coerce_json(&old.value);
                    }
                }
            }

            for (name, prop) in &descriptor.props {
                let value = JsValue::from_json(&prop.value, &mut context)
                    .map_err(|error| format!("could not build prop `{name}`: {error}"))?;
                props_object
                    .create_data_property(JsString::from(name.as_str()), value, &mut context)
                    .map_err(|error| format!("could not install prop `{name}`: {error}"))?;
            }

            let init = read_hook(&exports, "init", &mut context)?;
            let pre_process = read_hook(&exports, "preProcessFrame", &mut context)?;
            let post_process = read_hook(&exports, "postProcessFrame", &mut context)?;
            let shutdown = read_hook(&exports, "shutdown", &mut context)?;
            Ok::<_, String>((
                exports,
                descriptor,
                init,
                pre_process,
                post_process,
                shutdown,
            ))
        }));
        let (exports, descriptor, init, pre_process, post_process, shutdown) = match parsed {
            Ok(Ok(parsed)) => parsed,
            Ok(Err(error)) => return Err(error),
            Err(payload) => {
                return Err(format!(
                    "plugin descriptor panicked: {}",
                    panic_message(&*payload)
                ));
            }
        };

        Ok((
            Self {
                context,
                exports,
                props_object,
                modv_object,
                log_function,
                init,
                pre_process,
                post_process,
                shutdown,
                initialized: false,
            },
            descriptor,
        ))
    }
}

fn invoke_hook(
    runtime: &mut PluginRuntime,
    hook: JsObject,
    descriptor: &mut PluginDescriptor,
    id: PluginId,
    shared: &SharedHandle,
    io: &mut PluginIo,
    capture: Option<&FrameCapture>,
) -> Result<(), HookFailure> {
    let args = build_hook_args(runtime, io, capture)
        .map_err(|error| HookFailure::Js(format!("could not build hook arguments: {error}")))?;
    let this = JsValue::from(runtime.exports.clone());
    let _io_guard = ActiveIoGuard::ensure(shared, io);
    let _current = CurrentGuard::install(shared, id, Some(descriptor.name.clone()));
    let called = catch_unwind(AssertUnwindSafe(|| {
        hook.call(&this, &[JsValue::from(args)], &mut runtime.context)
    }));
    let result = match called {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(HookFailure::Js(format!("plugin hook threw: {error}"))),
        Err(payload) => Err(HookFailure::Panic(format!(
            "plugin hook panicked: {}",
            panic_message(&*payload)
        ))),
    };

    // Prop writes made before a throw still belong to the plugin's state.
    // Getters/setters on the props object are plugin code, so this call is
    // fallible too and must not unwind into the engine.
    match catch_unwind(AssertUnwindSafe(|| sync_props(runtime, descriptor))) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            push_error(
                shared,
                Some(id),
                Some(descriptor.name.clone()),
                PluginPhase::Props,
                error,
            );
        }
        Err(payload) => {
            return Err(HookFailure::Panic(format!(
                "plugin prop sync panicked: {}",
                panic_message(&*payload)
            )));
        }
    }

    result
}

fn build_hook_args(
    runtime: &mut PluginRuntime,
    io: &PluginIo,
    capture: Option<&FrameCapture>,
) -> Result<JsObject, String> {
    let context = &mut runtime.context;
    let args = JsObject::with_object_proto(context.intrinsics());
    args.set(
        JsString::from("props"),
        runtime.props_object.clone(),
        true,
        context,
    )
    .map_err(|error| error.to_string())?;
    args.set(
        JsString::from("modV"),
        runtime.modv_object.clone(),
        true,
        context,
    )
    .map_err(|error| error.to_string())?;
    args.set(
        JsString::from("log"),
        runtime.log_function.clone(),
        true,
        context,
    )
    .map_err(|error| error.to_string())?;
    let features = JsValue::from_json(&io.features, context).map_err(|error| error.to_string())?;
    args.set(JsString::from("features"), features, true, context)
        .map_err(|error| error.to_string())?;

    if let Some(capture) = capture {
        let width = JsValue::new(f64::from(capture.width));
        let height = JsValue::new(f64::from(capture.height));
        let pixels = JsUint8Array::from_iter(capture.pixels.iter().copied(), context)
            .map_err(|error| error.to_string())?;
        let pixels_value = JsValue::from(pixels);
        args.set(JsString::from("width"), width.clone(), true, context)
            .map_err(|error| error.to_string())?;
        args.set(JsString::from("height"), height.clone(), true, context)
            .map_err(|error| error.to_string())?;
        args.set(
            JsString::from("pixels"),
            pixels_value.clone(),
            true,
            context,
        )
        .map_err(|error| error.to_string())?;

        let capture_object = JsObject::with_object_proto(context.intrinsics());
        capture_object
            .set(JsString::from("width"), width, true, context)
            .map_err(|error| error.to_string())?;
        capture_object
            .set(JsString::from("height"), height, true, context)
            .map_err(|error| error.to_string())?;
        capture_object
            .set(JsString::from("pixels"), pixels_value, true, context)
            .map_err(|error| error.to_string())?;
        args.set(JsString::from("capture"), capture_object, true, context)
            .map_err(|error| error.to_string())?;
    }

    Ok(args)
}

fn sync_props(
    runtime: &mut PluginRuntime,
    descriptor: &mut PluginDescriptor,
) -> Result<(), String> {
    for (name, prop) in &mut descriptor.props {
        let value = runtime
            .props_object
            .get(JsString::from(name.as_str()), &mut runtime.context)
            .map_err(|error| error.to_string())?;
        if !value.is_undefined() {
            let coerced = prop.coerce_js(&value, &mut runtime.context)?;
            prop.value = coerced;
        }
        let value = JsValue::from_json(&prop.value, &mut runtime.context)
            .map_err(|error| error.to_string())?;
        runtime
            .props_object
            .create_data_property(JsString::from(name.as_str()), value, &mut runtime.context)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn resolve_exports(module_object: &JsObject, context: &mut Context) -> Result<JsObject, String> {
    let module_exports = module_object
        .get(JsString::from("exports"), context)
        .map_err(|error| error.to_string())?;
    let Some(mut exports) = module_exports.as_object() else {
        return Err("plugin must set `module.exports` to an object".to_owned());
    };
    if exports.is_callable() {
        return Err("plugin must export an object, not a function".to_owned());
    }

    let has_name = exports
        .has_own_property(JsString::from("name"), context)
        .map_err(|error| error.to_string())?;
    if !has_name {
        let default = exports
            .get(JsString::from("default"), context)
            .map_err(|error| error.to_string())?;
        if let Some(default_object) = default.as_object() {
            exports = default_object;
        } else {
            let global = context
                .global_object()
                .get(JsString::from("modVPlugin"), context)
                .map_err(|error| error.to_string())?;
            if let Some(global_object) = global.as_object() {
                exports = global_object;
            }
        }
    }
    if exports.is_callable() {
        return Err("plugin must export an object, not a function".to_owned());
    }
    Ok(exports)
}

fn remove_load_globals(context: &mut Context) {
    let global = context.global_object();
    for name in ["module", "exports", "modVPlugin"] {
        let _ = global
            .delete_property_or_throw(JsString::from(name), context)
            .ok();
    }
}

fn parse_descriptor(exports: &JsObject, context: &mut Context) -> Result<PluginDescriptor, String> {
    let name_value = exports
        .get(JsString::from("name"), context)
        .map_err(|error| error.to_string())?;
    let name = name_value
        .as_string()
        .map(|name| name.to_std_string_escaped())
        .ok_or_else(|| "plugin must export a string `name`".to_owned())?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err("plugin name must not be empty".to_owned());
    }
    if name.chars().count() > MAX_PLUGIN_NAME_CHARS {
        return Err(format!(
            "plugin name is longer than {MAX_PLUGIN_NAME_CHARS} characters"
        ));
    }

    let author =
        read_optional_string(exports, "author", context, MAX_AUTHOR_CHARS)?.unwrap_or_default();
    let version =
        read_optional_string(exports, "version", context, MAX_VERSION_CHARS)?.unwrap_or_default();
    let props = read_props(exports, context)?;
    let width = read_capture_dim(exports, "captureWidth", context)?;
    let height = read_capture_dim(exports, "captureHeight", context)?;
    let capture = match (width, height) {
        (None, None) => None,
        (Some(width), None) => Some(CaptureConfig {
            width,
            height: width,
        }),
        (None, Some(height)) => Some(CaptureConfig {
            width: height,
            height,
        }),
        (Some(width), Some(height)) => Some(CaptureConfig { width, height }),
    };

    Ok(PluginDescriptor {
        name,
        author,
        version,
        props,
        capture,
    })
}

fn read_props(
    exports: &JsObject,
    context: &mut Context,
) -> Result<IndexMap<String, PluginProp>, String> {
    let value = exports
        .get(JsString::from("props"), context)
        .map_err(|error| error.to_string())?;
    if value.is_undefined() || value.is_null() {
        return Ok(IndexMap::new());
    }
    let Some(object) = value.as_object() else {
        return Err("plugin `props` must be an object".to_owned());
    };
    if object.is_callable() {
        return Err("plugin `props` must be an object".to_owned());
    }
    let keys = object
        .own_property_keys(context)
        .map_err(|error| error.to_string())?;
    if keys.len() > MAX_PLUGIN_PROPS {
        return Err(format!(
            "plugin declares more than {MAX_PLUGIN_PROPS} props"
        ));
    }

    let mut props = IndexMap::new();
    for key in keys {
        let name = match key {
            PropertyKey::String(name) => name.to_std_string_escaped(),
            PropertyKey::Index(index) => index.get().to_string(),
            PropertyKey::Symbol(_) => continue,
        };
        if name.is_empty() || name.chars().count() > MAX_PROP_NAME_CHARS {
            return Err(format!(
                "prop names must be 1..={MAX_PROP_NAME_CHARS} characters"
            ));
        }
        if FORBIDDEN_PROP_NAMES.contains(&name.as_str()) {
            return Err(format!("prop name `{name}` is reserved"));
        }
        let value = object
            .get(JsString::from(name.as_str()), context)
            .map_err(|error| error.to_string())?;
        props.insert(name.clone(), PluginProp::from_js(&name, &value, context)?);
    }
    Ok(props)
}

fn read_options(object: &JsObject, context: &mut Context) -> Result<Vec<PluginPropOption>, String> {
    let mut value = object
        .get(JsString::from("options"), context)
        .map_err(|error| error.to_string())?;
    if value.is_undefined() {
        value = object
            .get(JsString::from("enum"), context)
            .map_err(|error| error.to_string())?;
    }
    if value.is_undefined() || value.is_null() {
        return Ok(Vec::new());
    }
    let Some(array_object) = value.as_object() else {
        return Err("select `options` must be an array".to_owned());
    };
    let array = JsArray::from_object(array_object).map_err(|error| error.to_string())?;
    let length = array.length(context).map_err(|error| error.to_string())?;
    if length > MAX_PROP_OPTIONS as u64 {
        return Err(format!(
            "select props may declare at most {MAX_PROP_OPTIONS} options"
        ));
    }
    let mut options = Vec::with_capacity(length as usize);
    for index in 0..length {
        let item = array
            .get(index as u32, context)
            .map_err(|error| error.to_string())?;
        let (label, value) = if let Some(option) = item.as_object() {
            let value = option
                .get(JsString::from("value"), context)
                .map_err(|error| error.to_string())?;
            let value = scalar_json(&value)
                .ok_or_else(|| "select option `value` must be a scalar".to_owned())?;
            let label = option
                .get(JsString::from("label"), context)
                .map_err(|error| error.to_string())?
                .as_string()
                .map(|label| truncate_chars(&label.to_std_string_escaped(), MAX_OPTION_LABEL_CHARS))
                .unwrap_or_else(|| scalar_label(&value));
            (label, value)
        } else {
            let value =
                scalar_json(&item).ok_or_else(|| "select option must be a scalar".to_owned())?;
            (scalar_label(&value), value)
        };
        options.push(PluginPropOption { label, value });
    }
    Ok(options)
}

fn read_hook(
    exports: &JsObject,
    name: &str,
    context: &mut Context,
) -> Result<Option<JsObject>, String> {
    let value = exports
        .get(JsString::from(name), context)
        .map_err(|error| error.to_string())?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    value
        .as_callable()
        .map(Some)
        .ok_or_else(|| format!("plugin `{name}` must be a function"))
}

fn read_optional_string(
    object: &JsObject,
    name: &str,
    context: &mut Context,
    max_chars: usize,
) -> Result<Option<String>, String> {
    let value = object
        .get(JsString::from(name), context)
        .map_err(|error| error.to_string())?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    value
        .as_string()
        .map(|value| Some(truncate_chars(&value.to_std_string_escaped(), max_chars)))
        .ok_or_else(|| format!("plugin `{name}` must be a string"))
}

fn read_number(
    object: &JsObject,
    name: &str,
    context: &mut Context,
) -> Result<Option<f64>, String> {
    let value = object
        .get(JsString::from(name), context)
        .map_err(|error| error.to_string())?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    value
        .as_number()
        .filter(|number| number.is_finite())
        .map(Some)
        .ok_or_else(|| format!("prop `{name}` must be a finite number"))
}

fn read_capture_dim(
    exports: &JsObject,
    name: &str,
    context: &mut Context,
) -> Result<Option<u32>, String> {
    let value = exports
        .get(JsString::from(name), context)
        .map_err(|error| error.to_string())?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    let Some(number) = value.as_number() else {
        return Err(format!("plugin `{name}` must be a number"));
    };
    if !number.is_finite()
        || number.fract() != 0.0
        || number < 1.0
        || number > f64::from(MAX_CAPTURE_DIMENSION)
    {
        return Err(format!(
            "plugin `{name}` must be an integer in 1..={MAX_CAPTURE_DIMENSION}"
        ));
    }
    Ok(Some(number as u32))
}

fn make_modv_object(context: &mut Context, shared: &SharedHandle) -> Result<JsObject, String> {
    let get_function = NativeFunction::from_copy_closure_with_captures(
        |_this, args, shared, context| modv_get(args, shared, context),
        shared.clone(),
    )
    .to_js_function(context.realm());
    let ws_function = NativeFunction::from_copy_closure_with_captures(
        |_this, args, shared, context| modv_ws(args, shared, context),
        shared.clone(),
    )
    .to_js_function(context.realm());

    let object = JsObject::with_object_proto(context.intrinsics());
    object
        .set(JsString::from("get"), get_function, true, context)
        .map_err(|error| error.to_string())?;
    object
        .set(JsString::from("ws"), ws_function, true, context)
        .map_err(|error| error.to_string())?;
    Ok(object)
}

fn make_log_function(context: &mut Context, shared: &SharedHandle) -> Result<JsObject, String> {
    let function = NativeFunction::from_copy_closure_with_captures(
        |_this, args, shared, context| {
            let level = args
                .get_or_undefined(0)
                .to_string(context)?
                .to_std_string_escaped();
            let message = args
                .get_or_undefined(1)
                .to_string(context)?
                .to_std_string_escaped();
            push_log(shared, PluginLogLevel::from_name(&level), message);
            Ok(JsValue::undefined())
        },
        shared.clone(),
    )
    .to_js_function(context.realm());
    Ok(function.into())
}

fn modv_get(
    args: &[JsValue],
    shared: &SharedHandle,
    context: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(path) = args.first().and_then(|value| value.as_string()) else {
        return Ok(JsValue::undefined());
    };
    let path = path.to_std_string_escaped();
    if path.is_empty() || path.len() > MAX_STATE_PATH_BYTES {
        return Ok(JsValue::undefined());
    }
    let io = shared.0.borrow().io.clone();
    let Some(io) = io else {
        return Ok(JsValue::undefined());
    };
    match io.state.get(&path) {
        Some(value) => JsValue::from_json(&value, context),
        None => Ok(JsValue::undefined()),
    }
}

fn modv_ws(
    args: &[JsValue],
    shared: &SharedHandle,
    context: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(url) = args.first().and_then(|value| value.as_string()) else {
        return Ok(JsValue::new(false));
    };
    let url = url.to_std_string_escaped();
    let lower = url.to_ascii_lowercase();
    if url.is_empty()
        || url.len() > MAX_WS_URL_BYTES
        || !(lower.starts_with("ws://") || lower.starts_with("wss://"))
    {
        return Ok(JsValue::new(false));
    }

    let payload = args.get(1).cloned().unwrap_or_else(JsValue::undefined);
    let Some(payload) = stringify_payload(&payload, context)? else {
        return Ok(JsValue::new(false));
    };
    if payload.len() > MAX_WS_PAYLOAD_BYTES {
        return Ok(JsValue::new(false));
    }

    let io = shared.0.borrow().io.clone();
    let Some(io) = io else {
        return Ok(JsValue::new(false));
    };
    match io.transport.borrow_mut().post(&url, &payload) {
        Ok(()) => Ok(JsValue::new(true)),
        Err(error) => {
            record_error(
                shared,
                PluginPhase::Transport,
                format!("modV.ws({url}) failed: {error}"),
            );
            Ok(JsValue::new(false))
        }
    }
}

fn stringify_payload(
    value: &JsValue,
    context: &mut Context,
) -> boa_engine::JsResult<Option<String>> {
    if let Some(string) = value.as_string() {
        return Ok(Some(string.to_std_string_escaped()));
    }
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    let json = context
        .global_object()
        .get(JsString::from("JSON"), context)?;
    let Some(json_object) = json.as_object() else {
        return Ok(None);
    };
    let stringify = json_object.get(JsString::from("stringify"), context)?;
    let Some(callable) = stringify.as_callable() else {
        return Ok(None);
    };
    let result = callable.call(&json, std::slice::from_ref(value), context)?;
    Ok(result
        .as_string()
        .map(|string| string.to_std_string_escaped()))
}

fn push_log(shared: &SharedHandle, level: PluginLogLevel, message: String) {
    let current = shared.0.borrow().current.clone();
    let Some(current) = current else {
        return;
    };
    let mut state = shared.0.borrow_mut();
    push_log_locked(
        &mut state,
        current.id,
        current.name,
        level,
        truncate_chars(&message, MAX_LOG_MESSAGE_CHARS),
    );
}

fn push_log_locked(
    shared: &mut Shared,
    plugin_id: PluginId,
    plugin_name: Option<String>,
    level: PluginLogLevel,
    message: String,
) {
    if shared.logs.len() >= MAX_LOGS {
        return;
    }
    shared.logs.push(PluginLog {
        plugin_id,
        plugin_name,
        level,
        message,
    });
}

fn push_error(
    shared: &SharedHandle,
    plugin_id: Option<PluginId>,
    plugin_name: Option<String>,
    phase: PluginPhase,
    message: impl Into<String>,
) -> PluginError {
    let message = truncate_chars(&message.into(), MAX_ERROR_MESSAGE_CHARS);
    let error = PluginError {
        plugin_id,
        plugin_name: plugin_name.clone(),
        phase,
        message: message.clone(),
    };
    let mut state = shared.0.borrow_mut();
    if !state.error_keys.insert((plugin_id, phase)) {
        return error;
    }
    if state.errors.len() < MAX_ERRORS {
        state.errors.push(error.clone());
    }
    if let Some(plugin_id) = plugin_id {
        push_log_locked(
            &mut state,
            plugin_id,
            plugin_name,
            PluginLogLevel::Error,
            message,
        );
    }
    error
}

fn record_error(shared: &SharedHandle, phase: PluginPhase, message: impl Into<String>) {
    let current = shared.0.borrow().current.clone();
    match current {
        Some(current) => {
            push_error(shared, Some(current.id), current.name, phase, message);
        }
        None => {
            push_error(shared, None, None, phase, message);
        }
    }
}

fn clear_errors_for(shared: &SharedHandle, id: PluginId) {
    let mut state = shared.0.borrow_mut();
    state.errors.retain(|error| error.plugin_id != Some(id));
    state
        .error_keys
        .retain(|(plugin_id, _)| *plugin_id != Some(id));
}

fn validate_capture_shape(width: u32, height: u32, byte_len: usize) -> Result<usize, CaptureError> {
    if width == 0 || height == 0 {
        return Err(CaptureError::Empty);
    }
    if width > MAX_CAPTURE_DIMENSION || height > MAX_CAPTURE_DIMENSION {
        return Err(CaptureError::TooLarge { width, height });
    }
    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if expected > MAX_CAPTURE_BYTES {
        return Err(CaptureError::TooLarge { width, height });
    }
    if byte_len != expected {
        return Err(CaptureError::WrongLength {
            expected,
            actual: byte_len,
        });
    }
    Ok(expected)
}

fn check_source(source: &str) -> Result<(), String> {
    if source.len() > MAX_PLUGIN_SOURCE_BYTES {
        return Err(format!(
            "plugin source exceeds {MAX_PLUGIN_SOURCE_BYTES} bytes"
        ));
    }

    let mut depth = 0usize;
    let mut max_depth = 0usize;
    let mut unary_run = 0usize;
    let mut quote: Option<char> = None;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut chars = source.chars().peekable();

    while let Some(character) = chars.next() {
        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_comment {
            if character == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block_comment = false;
            }
            continue;
        }
        if let Some(active) = quote {
            if character == '\\' {
                chars.next();
                continue;
            }
            if character == active {
                quote = None;
            }
            continue;
        }

        match character {
            '/' => {
                if chars.peek() == Some(&'/') {
                    chars.next();
                    line_comment = true;
                } else if chars.peek() == Some(&'*') {
                    chars.next();
                    block_comment = true;
                } else {
                    unary_run = 0;
                }
            }
            '\'' | '"' | '`' => {
                quote = Some(character);
                unary_run = 0;
            }
            '(' | '[' | '{' => {
                depth += 1;
                max_depth = max_depth.max(depth);
                unary_run = 0;
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                unary_run = 0;
            }
            '!' | '~' | '+' | '-' => {
                unary_run += 1;
                if unary_run > MAX_SOURCE_NESTING {
                    return Err(format!(
                        "plugin source has a unary chain longer than {MAX_SOURCE_NESTING}"
                    ));
                }
            }
            _ => {
                unary_run = 0;
            }
        }

        if max_depth > MAX_SOURCE_NESTING {
            return Err(format!(
                "plugin source is nested deeper than {MAX_SOURCE_NESTING} levels"
            ));
        }
    }

    Ok(())
}

fn validate_source_parse(source: &str) -> Result<(), String> {
    let source = source.to_owned();
    let handle = std::thread::Builder::new()
        .name("modv-plugin-validator".to_owned())
        .stack_size(PLUGIN_PARSER_STACK)
        .spawn(move || {
            let mut interner = Interner::default();
            let mut parser = Parser::new(Source::from_bytes(source.as_bytes()));
            parser
                .parse_script(&Scope::new_global(), &mut interner)
                .map(|_| ())
                .map_err(|error| format!("invalid plugin syntax: {error}"))
        })
        .map_err(|error| format!("could not start the plugin validator: {error}"))?;
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err("plugin validator panicked".to_owned()),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let mut truncated: String = value.chars().take(max.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

fn scalar_json(value: &JsValue) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    if let Some(boolean) = value.as_boolean() {
        return Some(Value::Bool(boolean));
    }
    if let Some(number) = value.as_number() {
        return serde_json::Number::from_f64(number).map(Value::Number);
    }
    value
        .as_string()
        .map(|string| Value::String(truncate_chars(&string.to_std_string_escaped(), 256)))
}

fn scalar_label(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(string) => string.clone(),
        _ => String::new(),
    }
}

/// Compare a select option with a value, treating `1` and `1.0` as the same
/// JavaScript number (the UI may carry either JSON representation).
fn select_matches(option: &Value, value: &Value) -> bool {
    if option == value {
        return true;
    }
    match (option.as_f64(), value.as_f64()) {
        (Some(option), Some(value)) => option == value,
        _ => false,
    }
}

fn color_channel(
    object: &JsObject,
    channel: &str,
    fallback: f64,
    context: &mut Context,
) -> Result<f64, String> {
    let value = object
        .get(JsString::from(channel), context)
        .map_err(|error| error.to_string())?;
    Ok(value.as_number().unwrap_or(fallback))
}

fn color_channels(value: &Value) -> (f64, f64, f64, f64) {
    let object = value.as_object();
    let channel = |name: &str, fallback: f64| {
        object
            .and_then(|object| object.get(name))
            .and_then(Value::as_f64)
            .unwrap_or(fallback)
    };
    (
        channel("r", 0.0),
        channel("g", 0.0),
        channel("b", 0.0),
        channel("a", 1.0),
    )
}

fn coerce_color_json(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let (default_r, default_g, default_b, default_a) = (0.0, 0.0, 0.0, 1.0);
    let channel =
        |name: &str, fallback: f64| object.get(name).and_then(Value::as_f64).unwrap_or(fallback);
    Some(color_value(
        channel("r", default_r),
        channel("g", default_g),
        channel("b", default_b),
        channel("a", default_a),
    ))
}

fn color_value(r: f64, g: f64, b: f64, a: f64) -> Value {
    let channel = |value: f64| {
        serde_json::Number::from_f64(value.clamp(0.0, 1.0))
            .map(Value::Number)
            .unwrap_or(Value::from(0.0))
    };
    let mut object = serde_json::Map::new();
    object.insert("r".to_owned(), channel(r));
    object.insert("g".to_owned(), channel(g));
    object.insert("b".to_owned(), channel(b));
    object.insert("a".to_owned(), channel(a));
    Value::Object(object)
}
