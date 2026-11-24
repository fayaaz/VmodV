//! Engine-owned Output Windows — the VmodV replacement for Electron's
//! `window.open("./output-window.html")` popups.
//!
//! [`WindowRegistry`] is the store-side truth the UI mirror sees (`windows`
//! Vuex module: `ADD_WINDOW` / `REMOVE_WINDOW`). The winit loop owns the actual
//! windows: `windows/createWindow` records the spec here and queues a
//! [`WindowCommand::Open`], which the frame loop drains and applies. Keeping the
//! registry free of winit/GPU objects means the action semantics (defaults,
//! clamping, duplicate ids, delta payloads) are plain unit tests, and window
//! work happens between frames instead of inside a control-socket request.
//!
//! Closing is symmetric: `windows/removeWindow` drops the spec and queues
//! [`WindowCommand::Close`]. A window the window manager closed is removed with
//! [`WindowRegistry::forget`] instead (no Close command — it is already gone)
//! and the store emits `windows/REMOVE_WINDOW` itself.

use serde_json::{Map, Value, json};

/// Title/default used when the action does not name the window.
pub const DEFAULT_TITLE: &str = "VmodV Output";

/// The legacy Output Window opens at 300 × 300 (`windows.js:8-9`).
pub const DEFAULT_WIDTH: u32 = 300;
/// See [`DEFAULT_WIDTH`].
pub const DEFAULT_HEIGHT: u32 = 300;

/// Smallest accepted window edge: below this the surface/texture round trip is
/// not worth a swapchain, and a 0×0 surface is invalid in wgpu.
pub const MIN_DIMENSION: u32 = 64;

/// Largest accepted window edge. A bogus request (or a corrupt preset) must not
/// ask for a swapchain larger than any current display.
pub const MAX_DIMENSION: u32 = 8192;

/// Longest title kept from a request, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// One engine-owned Output Window.
///
/// `x`/`y` are `None` until the caller explicitly asks for a placement; the
/// window manager then picks the spot, matching `window.open` without `left`/
/// `top`. The mirror payload still reports `0`/`0` because the legacy window
/// object defaults to that (`windows.js:12-13`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSpec {
    /// The `windows` Vuex-module key (a UUID the action returns).
    pub id: String,
    /// Window-manager title.
    pub title: String,
    /// Logical (CSS-pixel) width; the surface uses physical pixels.
    pub width: u32,
    /// Logical (CSS-pixel) height.
    pub height: u32,
    /// Explicit logical x position, if any.
    pub x: Option<i32>,
    /// Explicit logical y position, if any.
    pub y: Option<i32>,
    /// Monitor index for fullscreen (`available_monitors()` order), if any.
    pub monitor: Option<usize>,
    /// Whether the window should open borderless-fullscreen.
    pub fullscreen: bool,
}

/// The raw, unvalidated fields of a `windows/createWindow` request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewWindow {
    /// Requested title (`name` or `title`).
    pub title: Option<String>,
    /// Requested logical width; negative/zero means "use the default".
    pub width: Option<i64>,
    /// Requested logical height; negative/zero means "use the default".
    pub height: Option<i64>,
    /// Requested logical x position.
    pub x: Option<i32>,
    /// Requested logical y position.
    pub y: Option<i32>,
    /// Requested fullscreen monitor index.
    pub monitor: Option<usize>,
    /// Whether to open fullscreen.
    pub fullscreen: bool,
}

impl NewWindow {
    /// Decode the action payload.
    ///
    /// Both `name` (the action's field) and `title` (the Vuex window record's
    /// field, which `windows/ADD_WINDOW` carries) are accepted, so a replayed
    /// ADD_WINDOW payload creates the same window.
    pub fn from_params(params: &Map<String, Value>) -> Self {
        let title = params
            .get("name")
            .or_else(|| params.get("title"))
            .and_then(Value::as_str)
            .map(str::to_string);
        Self {
            title,
            width: params.get("width").and_then(Value::as_i64),
            height: params.get("height").and_then(Value::as_i64),
            x: params
                .get("x")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok()),
            y: params
                .get("y")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok()),
            monitor: params
                .get("monitor")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
            fullscreen: params
                .get("fullscreen")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

impl WindowSpec {
    /// Validate a request into a spec: default title, clamped size.
    pub fn from_request(id: String, request: NewWindow) -> Self {
        Self {
            id,
            title: request
                .title
                .as_deref()
                .map(sanitise_title)
                .unwrap_or_else(|| DEFAULT_TITLE.to_string()),
            width: clamp_dimension(request.width, DEFAULT_WIDTH),
            height: clamp_dimension(request.height, DEFAULT_HEIGHT),
            x: request.x,
            y: request.y,
            monitor: request.monitor,
            fullscreen: request.fullscreen,
        }
    }

    /// The mirror payload for `windows/ADD_WINDOW` — the exact object the JS
    /// action builds (`windows.js:7-17`), so `state.windows[id]` looks the same
    /// in both execution modes.
    pub fn to_mirror_json(&self) -> Value {
        json!({
            "id": self.id,
            "width": self.width,
            "height": self.height,
            "title": self.title,
            "pixelRatio": 1,
            "x": self.x.unwrap_or(0),
            "y": self.y.unwrap_or(0),
            "fullscreen": self.fullscreen,
            "backgroundColor": "#000",
            "outputId": "",
        })
    }
}

/// Clamp a requested logical edge.
///
/// Missing or non-positive values keep the legacy default (a 0×0 swapchain is
/// invalid), while out-of-range values are clamped instead of rejected so a
/// stale preset still opens a usable window.
pub fn clamp_dimension(requested: Option<i64>, default: u32) -> u32 {
    match requested {
        Some(value) if value > 0 => {
            value.clamp(i64::from(MIN_DIMENSION), i64::from(MAX_DIMENSION)) as u32
        }
        _ => default,
    }
}

/// Trim, cap and default a requested title.
fn sanitise_title(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return DEFAULT_TITLE.to_string();
    }
    trimmed.chars().take(MAX_TITLE_CHARS).collect()
}

/// A pending native-window operation for the event loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowCommand {
    /// Create (and start presenting to) the window.
    Open(WindowSpec),
    /// Destroy the window and drop its surface.
    Close(String),
}

/// Why a window could not be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowError {
    /// A window with this id is already open.
    DuplicateId {
        /// The id the caller asked for.
        id: String,
    },
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateId { id } => write!(f, "an Output Window with id {id} is already open"),
        }
    }
}

impl std::error::Error for WindowError {}

/// The store-side registry of engine-owned Output Windows.
#[derive(Debug, Default)]
pub struct WindowRegistry {
    /// Insertion-ordered specs (the mirror is a keyed map, but a stable order
    /// keeps `state/sync` and the logs deterministic).
    specs: Vec<WindowSpec>,
    /// Operations the event loop has not applied yet.
    commands: Vec<WindowCommand>,
}

impl WindowRegistry {
    /// Register `spec` and queue its native creation.
    ///
    /// A duplicate id is rejected without touching the registry or the queue:
    /// the UI would otherwise lose track of which native window an entry maps
    /// to.
    pub fn create(&mut self, spec: WindowSpec) -> Result<(), WindowError> {
        if self.contains(&spec.id) {
            return Err(WindowError::DuplicateId { id: spec.id });
        }
        self.commands.push(WindowCommand::Open(spec.clone()));
        self.specs.push(spec);
        Ok(())
    }

    /// Drop `id` and queue its native window for closing.
    ///
    /// Returns `false` (and queues nothing) when the id is unknown, so a stale
    /// remove from the UI is a no-op.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(index) = self.specs.iter().position(|spec| spec.id == id) else {
            return false;
        };
        self.specs.remove(index);
        self.commands.push(WindowCommand::Close(id.to_string()));
        true
    }

    /// Drop `id` without queueing a Close command — used when the window is
    /// already being destroyed (the window manager closed it, or creation
    /// failed).
    pub fn forget(&mut self, id: &str) -> Option<WindowSpec> {
        let index = self.specs.iter().position(|spec| spec.id == id)?;
        Some(self.specs.remove(index))
    }

    /// Take every queued native-window operation, in issue order.
    pub fn take_commands(&mut self) -> Vec<WindowCommand> {
        std::mem::take(&mut self.commands)
    }

    /// The spec for `id`.
    pub fn spec(&self, id: &str) -> Option<&WindowSpec> {
        self.specs.iter().find(|spec| spec.id == id)
    }

    /// Whether `id` is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.spec(id).is_some()
    }

    /// The registered specs, in creation order.
    pub fn specs(&self) -> &[WindowSpec] {
        &self.specs
    }

    /// The specs as the mirror-shaped `windows` array.
    ///
    /// Sent in `state/sync` so a late-joining UI (or a mirror rebuilt after an
    /// engine restart) lists the windows the engine actually has open.
    pub fn specs_json(&self) -> Value {
        Value::Array(
            self.specs()
                .iter()
                .map(WindowSpec::to_mirror_json)
                .collect(),
        )
    }

    /// Number of registered windows.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether no window is registered.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn spec(id: &str) -> WindowSpec {
        WindowSpec::from_request(id.to_string(), NewWindow::default())
    }

    #[test]
    fn defaults_match_the_legacy_output_window() {
        let spec = spec("w1");
        assert_eq!(spec.title, DEFAULT_TITLE);
        assert_eq!(spec.width, DEFAULT_WIDTH);
        assert_eq!(spec.height, DEFAULT_HEIGHT);
        assert_eq!(spec.x, None);
        assert_eq!(spec.y, None);
        assert_eq!(spec.monitor, None);
        assert!(!spec.fullscreen);

        let mirror = spec.to_mirror_json();
        assert_eq!(mirror["id"], json!("w1"));
        assert_eq!(mirror["width"], json!(300));
        assert_eq!(mirror["height"], json!(300));
        assert_eq!(mirror["title"], json!("VmodV Output"));
        assert_eq!(mirror["pixelRatio"], json!(1));
        assert_eq!(mirror["x"], json!(0));
        assert_eq!(mirror["y"], json!(0));
        assert_eq!(mirror["fullscreen"], json!(false));
        assert_eq!(mirror["backgroundColor"], json!("#000"));
        assert_eq!(mirror["outputId"], json!(""));
    }

    #[test]
    fn requested_fields_are_parsed_from_action_or_mirror_payloads() {
        let request = NewWindow::from_params(&params(json!({
            "id": "w2",
            "name": "Left Screen",
            "width": 1920,
            "height": 1080,
            "x": -1200,
            "y": 24,
            "monitor": 1,
            "fullscreen": true,
        })));
        let spec = WindowSpec::from_request("w2".to_string(), request);
        assert_eq!(spec.title, "Left Screen");
        assert_eq!((spec.width, spec.height), (1920, 1080));
        assert_eq!((spec.x, spec.y), (Some(-1200), Some(24)));
        assert_eq!(spec.monitor, Some(1));
        assert!(spec.fullscreen);

        // `windows/ADD_WINDOW` carries `title` instead of `name`.
        let replayed = WindowSpec::from_request(
            "w3".to_string(),
            NewWindow::from_params(&params(json!({
                "title": "Replayed",
                "width": 640,
                "height": 360,
            }))),
        );
        assert_eq!(replayed.title, "Replayed");
        assert_eq!((replayed.width, replayed.height), (640, 360));
    }

    #[test]
    fn sizes_are_clamped_and_defaulted() {
        assert_eq!(clamp_dimension(None, 300), 300);
        assert_eq!(clamp_dimension(Some(0), 300), 300);
        assert_eq!(clamp_dimension(Some(-640), 300), 300);
        assert_eq!(clamp_dimension(Some(1), 300), MIN_DIMENSION);
        assert_eq!(
            clamp_dimension(Some(i64::from(MIN_DIMENSION)), 300),
            MIN_DIMENSION
        );
        assert_eq!(
            clamp_dimension(Some(i64::from(MAX_DIMENSION) + 1), 300),
            MAX_DIMENSION
        );
        assert_eq!(clamp_dimension(Some(i64::MAX), 300), MAX_DIMENSION);

        let spec = WindowSpec::from_request(
            "w".to_string(),
            NewWindow::from_params(&params(json!({"width": 10, "height": 99999}))),
        );
        assert_eq!((spec.width, spec.height), (MIN_DIMENSION, MAX_DIMENSION));
    }

    #[test]
    fn titles_are_trimmed_capped_and_defaulted() {
        let long = "x".repeat(MAX_TITLE_CHARS + 50);
        let request = NewWindow::from_params(&params(json!({"name": format!("  {long}  ")})));
        let spec = WindowSpec::from_request("w".to_string(), request);
        assert_eq!(spec.title.chars().count(), MAX_TITLE_CHARS);

        let blank = WindowSpec::from_request(
            "w".to_string(),
            NewWindow::from_params(&params(json!({"name": "   "}))),
        );
        assert_eq!(blank.title, DEFAULT_TITLE);
    }

    #[test]
    fn create_queues_one_open_command_per_window() {
        let mut registry = WindowRegistry::default();
        registry.create(spec("a")).expect("first create");
        registry.create(spec("b")).expect("second create");

        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
        assert_eq!(
            registry
                .specs()
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(
            registry.take_commands(),
            vec![
                WindowCommand::Open(spec("a")),
                WindowCommand::Open(spec("b")),
            ]
        );
        // Draining is one-shot: the loop applies each operation exactly once.
        assert!(registry.take_commands().is_empty());
    }

    #[test]
    fn duplicate_ids_are_rejected_without_side_effects() {
        let mut registry = WindowRegistry::default();
        registry.create(spec("dup")).expect("first create");
        let _ = registry.take_commands();

        let error = registry.create(spec("dup")).expect_err("duplicate id");
        assert_eq!(
            error,
            WindowError::DuplicateId {
                id: "dup".to_string()
            }
        );
        assert_eq!(registry.len(), 1);
        assert!(registry.take_commands().is_empty());
        assert!(error.to_string().contains("dup"));
    }

    #[test]
    fn remove_queues_close_and_unknown_remove_is_a_noop() {
        let mut registry = WindowRegistry::default();
        registry.create(spec("a")).expect("create");
        registry.create(spec("b")).expect("create");
        let _ = registry.take_commands();

        assert!(registry.remove("a"));
        assert_eq!(
            registry.take_commands(),
            vec![WindowCommand::Close("a".to_string())]
        );
        assert!(!registry.contains("a"));
        assert!(registry.contains("b"));

        assert!(!registry.remove("a"));
        assert!(registry.take_commands().is_empty());
    }

    #[test]
    fn forget_drops_the_spec_without_a_close_command() {
        let mut registry = WindowRegistry::default();
        registry.create(spec("native")).expect("create");
        let _ = registry.take_commands();

        let removed = registry.forget("native").expect("spec was registered");
        assert_eq!(removed.id, "native");
        assert!(registry.is_empty());
        assert!(
            registry.take_commands().is_empty(),
            "a window the WM closed must not be closed a second time"
        );
        assert!(registry.forget("native").is_none());
    }

    #[test]
    fn specs_json_is_the_mirror_array_in_creation_order() {
        let mut registry = WindowRegistry::default();
        registry.create(spec("first")).expect("create");
        registry
            .create(WindowSpec::from_request(
                "second".to_string(),
                NewWindow::from_params(&params(json!({"name": "B", "width": 1280}))),
            ))
            .expect("create");

        let value = registry.specs_json();
        let entries = value.as_array().expect("array");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["id"], json!("first"));
        assert_eq!(entries[0]["width"], json!(300));
        assert_eq!(entries[1]["id"], json!("second"));
        assert_eq!(entries[1]["title"], json!("B"));
        assert_eq!(entries[1]["width"], json!(1280));
    }
}
