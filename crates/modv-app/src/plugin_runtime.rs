//! The VmodV shell's opt-in sandboxed Boa plugin runtime.
//!
//! [`modv_engine::plugins`] owns the sandbox and the hook contract
//! (`preProcessFrame`/`postProcessFrame`, `modV.get`, `modV.ws`, `log`,
//! RGBA capture). This module owns everything the shell needs around it:
//!
//! * the persisted settings/consent gate (`<userData>/vmodv-plugins.json`,
//!   atomic `.json.tmp` + rename like `vmodv-audio.json`), with the master
//!   `enabled` switch normalised to `false` until the user has consented;
//! * plugin discovery: the active project profile's `plugin/` folder plus
//!   `<userData>/plugins`, sorted by path, de-duplicated and capped at
//!   [`MAX_PLUGINS`] scripts of at most [`MAX_PLUGIN_SOURCE_BYTES`] each;
//! * per-plugin enable switches keyed by absolute path;
//! * user prop values keyed by path + prop name, reapplied after every load so
//!   a reload/restart does not reset a control the user moved;
//! * the per-frame IO bundle: the current audio feature payload, a small
//!   **allow-listed** read-only state reader and a transport placeholder;
//! * throttled frame capture: at most [`CAPTURE_INTERVAL`], only when a
//!   running plugin requested `captureWidth`/`captureHeight`;
//! * `plugin: …` log forwarding (deduplicated) and `shutdown` on quit.
//!
//! The VmodV Plugins panel calls the public methods here (`set_enabled`,
//! `set_consented`, `set_plugin_enabled`, `set_prop`, `reload`, `rescan`,
//! `plugins`, `logs`) and treats the runtime as authoritative: the UI read
//! model is rebuilt from it, never written back.
//!
//! # Residuals
//!
//! * `modV.ws` is **not** wired: [`UnsupportedTransport`] rejects every post
//!   and the host records a `Transport` error per plugin (the message says the
//!   transport is not wired in the VmodV shell yet). A correct WebSocket
//!   client can replace it without changing the plugin API.
//! * The legacy Electron `grab-canvas` bundle is not shipped; plugins that
//!   import it need an external bundle (the host accepts a single
//!   CommonJS script).
//! * Discovery is explicit (`rescan`); there is no filesystem watcher yet.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use modv_engine::plugins::{
    CaptureConfig, FrameCapture, MAX_CAPTURE_DIMENSION, MAX_PLUGIN_SOURCE_BYTES, MAX_PLUGINS,
    PluginError, PluginHost, PluginId, PluginIo, PluginLogLevel, PluginPhase, PluginStateReader,
    PluginTransport,
};
use modv_renderers::two_d::AudioFeatures;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::store_ops::Store;

/// The persisted plugin settings inside the shared userData directory.
pub const PLUGIN_SETTINGS_FILENAME: &str = "vmodv-plugins.json";

/// The shortest interval between two capture read-backs (20 Hz).
///
/// A frame read-back is a synchronised full-resolution read plus a CPU
/// downsample, so it only runs when a running plugin asked for a capture and
/// never faster than this. Between reads [`postProcessFrame`] receives the
/// previous capture (at most 50 ms old) rather than no capture at all.
pub const CAPTURE_INTERVAL: Duration = Duration::from_millis(50);

/// The upper bound of retained deduplicated log lines before the set resets.
const MAX_LOGGED_LINES: usize = 4096;

/// How many deduplicated `plugin: …` lines the Plugins panel keeps.
///
/// This is the UI retention list, separate from [`MAX_LOGGED_LINES`] (the
/// dedupe seen-set): the panel only needs a recent window, and a plugin that
/// logs every frame must not grow the read model without bound.
const MAX_RETAINED_LOGS: usize = 256;

/// The persisted "enable JavaScript plugins" gate.
///
/// Defaults are disabled **and** unconsented. A document that says
/// `enabled: true` without `consented: true` is normalised to disabled on
/// read ([`Self::normalise`]): the switch can only be on behind the consent
/// dialog, and a hand-edited file must not bypass it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSettings {
    /// The master switch.
    #[serde(default)]
    pub enabled: bool,
    /// Whether the user accepted the plugin sandbox's consent notice.
    #[serde(default)]
    pub consented: bool,
    /// Per-plugin switches keyed by absolute script path. A missing path is
    /// enabled; an explicit `false` disables it.
    #[serde(default)]
    pub plugins: BTreeMap<String, bool>,
    /// User prop values keyed by absolute script path, then prop name.
    ///
    /// The runtime reapplies these after every load/rescan so a control the
    /// user moved is not reset by a reload or a restart. Values are coerced by
    /// the host against the plugin's schema before they are stored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, BTreeMap<String, Value>>,
}

impl PluginSettings {
    /// Whether the host may load plugins (the master switch **and** consent).
    pub fn allowed(&self) -> bool {
        self.enabled && self.consented
    }

    /// Normalise an in-memory document: no consent means no plugin contexts.
    pub fn normalise(&mut self) {
        if !self.consented {
            self.enabled = false;
        }
    }

    /// Whether the script at `path` is switched on (missing means on).
    pub fn is_plugin_enabled(&self, path: &str) -> bool {
        self.plugins.get(path).copied().unwrap_or(true)
    }

    /// Store one per-plugin switch.
    pub fn set_plugin_enabled(&mut self, path: &str, enabled: bool) {
        self.plugins.insert(path.to_string(), enabled);
    }

    /// Read the document, treating a missing file as "nothing saved".
    ///
    /// The returned document is already normalised. A malformed file is an
    /// error so the shell can warn and fall back to the disabled default.
    pub fn read_from(path: &Path) -> Result<Option<Self>, String> {
        let path = crate::settings_compat::read_path(path);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("could not read {}: {error}", path.display()));
            }
        };
        let mut settings: Self = serde_json::from_str(&text)
            .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))?;
        settings.normalise();
        Ok(Some(settings))
    }

    /// Write the document atomically, creating the userData directory.
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let temporary = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("could not serialise plugin settings: {error}"))?;
        std::fs::write(&temporary, text)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(())
    }
}

/// One discovered plugin script.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DiscoveredSource {
    path: PathBuf,
    source: String,
}

/// Discovery output: readable sources plus skipped-file messages.
#[derive(Clone, Debug, Default)]
struct Discovery {
    sources: Vec<DiscoveredSource>,
    errors: Vec<String>,
}

/// Discover plugin scripts under `roots`, in deterministic path order.
///
/// Only top-level `*.js` files are considered (the Electron media manager's
/// `plugin/` folder layout). Hidden files, unreadable files and sources over
/// [`MAX_PLUGIN_SOURCE_BYTES`] are skipped with a message; the remaining list
/// is truncated to [`MAX_PLUGINS`]. Canonical paths de-duplicate overlapping
/// roots, so passing the same folder twice yields one entry per file.
fn discover(roots: &[PathBuf]) -> Discovery {
    let mut errors = Vec::new();
    let mut paths: BTreeSet<PathBuf> = BTreeSet::new();

    for root in roots {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                errors.push(format!("could not read {}: {error}", root.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(format!(
                        "could not read an entry in {}: {error}",
                        root.display()
                    ));
                    continue;
                }
            };
            let path = entry.path();
            if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_none_or(|extension| !extension.eq_ignore_ascii_case("js"))
            {
                continue;
            }
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    errors.push(format!("could not inspect {}: {error}", path.display()));
                    continue;
                }
            };
            if !metadata.is_file() {
                continue;
            }
            if metadata.len() > MAX_PLUGIN_SOURCE_BYTES as u64 {
                errors.push(format!(
                    "{}: source exceeds {MAX_PLUGIN_SOURCE_BYTES} bytes",
                    path.display()
                ));
                continue;
            }
            paths.insert(path.canonicalize().unwrap_or(path));
        }
    }

    let mut paths: Vec<PathBuf> = paths.into_iter().collect();
    if paths.len() > MAX_PLUGINS {
        errors.push(format!(
            "found {} plugin scripts; only the first {MAX_PLUGINS} are loaded",
            paths.len()
        ));
        paths.truncate(MAX_PLUGINS);
    }

    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        match std::fs::read_to_string(&path) {
            Ok(source) => sources.push(DiscoveredSource { path, source }),
            Err(error) => errors.push(format!("could not read {}: {error}", path.display())),
        }
    }
    Discovery { sources, errors }
}

/// One registered source and the id the host assigned it.
#[derive(Clone, Debug)]
struct RegisteredPlugin {
    path: PathBuf,
    key: String,
    source: String,
    id: PluginId,
}

/// The plugin list entry the future Plugins panel renders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    /// Absolute script path (the settings key).
    pub path: String,
    /// Descriptor name (empty until the source loads).
    pub name: String,
    /// Descriptor author.
    pub author: String,
    /// Descriptor version.
    pub version: String,
    /// The per-plugin user switch.
    pub enabled: bool,
    /// The host slot's active flag (the switch, after the host saw it).
    pub active: bool,
    /// Whether a Boa context is loaded and `init` succeeded.
    pub loaded: bool,
    /// Whether loading/initialising failed.
    pub failed: bool,
    /// The plugin's requested capture size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<CaptureConfig>,
    /// Props with their current values (the host's serialised schema).
    pub props: Value,
    /// The first recorded error for this plugin, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The source file's size in bytes.
    pub source_bytes: usize,
}

/// The host transport placeholder for `modV.ws`.
///
/// The VmodV shell has no WebSocket client yet, so every post is rejected;
/// the host records one `Transport` error per plugin and `modV.ws` returns
/// `false`. This is documented in the module docs and the task's residuals.
struct UnsupportedTransport;

impl PluginTransport for UnsupportedTransport {
    fn post(&mut self, _url: &str, _payload: &str) -> Result<(), String> {
        Err("modV.ws transport is not wired in the VmodV shell yet".to_string())
    }
}

/// A state reader that knows nothing (before the first frame).
struct EmptyStateReader;

impl PluginStateReader for EmptyStateReader {
    fn get(&self, _path: &str) -> Option<Value> {
        None
    }
}

/// The throttled capture cache.
#[derive(Debug, Default)]
struct PluginCapture {
    /// The capture size the cache was filled for.
    request: Option<CaptureConfig>,
    /// When a read-back was last attempted (successful or not).
    last_read: Option<Instant>,
    /// The latest valid capture (at most [`CAPTURE_INTERVAL`] old).
    frame: Option<FrameCapture>,
}

impl PluginCapture {
    fn clear(&mut self) {
        self.request = None;
        self.last_read = None;
        self.frame = None;
    }

    fn frame(&self) -> Option<&FrameCapture> {
        self.frame.as_ref()
    }

    /// The read-back size when one is due, or `None` when the cache is still
    /// fresh or no plugin asked for a capture.
    ///
    /// The dimensions are clamped to [`MAX_CAPTURE_DIMENSION`] defensively;
    /// the host already rejects larger descriptor values.
    fn due(&mut self, request: Option<CaptureConfig>, now: Instant) -> Option<(u32, u32)> {
        let Some(request) = request else {
            self.clear();
            return None;
        };
        let request = CaptureConfig {
            width: request.width.clamp(1, MAX_CAPTURE_DIMENSION),
            height: request.height.clamp(1, MAX_CAPTURE_DIMENSION),
        };
        if self.request != Some(request) {
            // A different running plugin set changes the read-back size; a
            // frame of the old size must not be handed to the new request.
            self.request = Some(request);
            self.frame = None;
            self.last_read = None;
        }
        if self
            .last_read
            .is_some_and(|last| now.saturating_duration_since(last) < CAPTURE_INTERVAL)
        {
            return None;
        }
        Some((request.width, request.height))
    }

    fn store(&mut self, frame: FrameCapture, now: Instant) {
        self.frame = Some(frame);
        self.last_read = Some(now);
    }

    /// Record a failed/empty read so the failure is throttled too.
    fn attempted(&mut self, now: Instant) {
        self.last_read = Some(now);
    }
}

/// The app-owned plugin runtime.
///
/// Construct with [`Self::load`], supply the frame inputs with
/// [`Self::set_frame_inputs`] and call [`Self::start`] once at startup. The
/// frame loop then calls [`Self::pre_frame`] before rendering and
/// [`Self::post_frame`] after present, with [`Self::capture_frame`] in
/// between when a capture is due.
pub struct PluginRuntime {
    settings: PluginSettings,
    settings_path: PathBuf,
    roots: Vec<PathBuf>,
    host: PluginHost,
    registered: Vec<RegisteredPlugin>,
    discovery_errors: Vec<String>,
    features: Value,
    state: Rc<dyn PluginStateReader>,
    transport: Rc<RefCell<dyn PluginTransport>>,
    capture: PluginCapture,
    logs_seen: HashSet<String>,
    /// Deduplicated `plugin: …` lines retained for the Plugins panel.
    logs: Vec<String>,
}

impl PluginRuntime {
    /// Load the persisted settings and build a disabled host.
    ///
    /// Nothing is discovered or evaluated here; [`Self::start`] does that (so
    /// the caller can supply the real state bundle first). A malformed settings
    /// file is warned about and treated as the disabled default.
    pub fn load(settings_path: PathBuf, roots: Vec<PathBuf>) -> Self {
        let settings = match PluginSettings::read_from(&settings_path) {
            Ok(Some(settings)) => settings,
            Ok(None) => PluginSettings::default(),
            Err(error) => {
                eprintln!("ui: WARN ignoring the saved plugin settings: {error}");
                PluginSettings::default()
            }
        };
        Self {
            settings,
            settings_path,
            roots,
            host: PluginHost::new(false),
            registered: Vec::new(),
            discovery_errors: Vec::new(),
            features: Value::Null,
            state: Rc::new(EmptyStateReader),
            transport: Rc::new(RefCell::new(UnsupportedTransport)),
            capture: PluginCapture::default(),
            logs_seen: HashSet::new(),
            logs: Vec::new(),
        }
    }

    /// Discover plugin scripts and apply the persisted opt-in.
    ///
    /// Discovery runs even while disabled so the future Plugins panel can list
    /// the available scripts; no Boa context is allocated unless the settings
    /// say `enabled && consented`.
    pub fn start(&mut self) {
        self.rescan_sources();
        self.apply_enabled();
        self.flush_logs();
    }

    /// The current settings document (normalised).
    pub fn settings(&self) -> &PluginSettings {
        &self.settings
    }

    /// Whether the master switch and consent are both on.
    pub fn enabled(&self) -> bool {
        self.settings.allowed()
    }

    /// Whether the user accepted the plugin consent notice.
    pub fn consented(&self) -> bool {
        self.settings.consented
    }

    /// How many plugins currently hold a Boa context.
    pub fn loaded_count(&self) -> usize {
        self.host.loaded_count()
    }

    /// The host's recorded errors (deduplicated by plugin + phase).
    pub fn errors(&self) -> Vec<PluginError> {
        self.host.errors()
    }

    /// Deduplicated `plugin: …` lines retained for the Plugins panel.
    ///
    /// The same lines go to the daily log file through `eprintln!`; the panel
    /// renders this retained window and can clear it on reopen.
    pub fn logs(&self) -> &[String] {
        &self.logs
    }

    /// Drop the retained panel lines. The dedupe set is kept, so a line that
    /// already reached the file is not re-emitted by the next hook.
    pub fn clear_logs(&mut self) {
        self.logs.clear();
    }

    /// Write one plugin prop through the host and persist the coerced value.
    ///
    /// The error strings are the host's (`unknown plugin`, `no prop`,
    /// `not loaded a prop schema yet`); the host has already recorded them.
    pub fn set_prop(&mut self, path: &str, prop: &str, value: Value) -> Result<Value, String> {
        let Some(entry) = self.registered.iter().find(|entry| entry.key == path) else {
            return Err(format!("unknown plugin script {path}"));
        };
        let id = entry.id;
        let coerced = match self.host.set_prop(id, prop, value) {
            Ok(value) => value,
            Err(error) => {
                self.flush_logs();
                return Err(error.message);
            }
        };
        self.settings
            .props
            .entry(path.to_string())
            .or_default()
            .insert(prop.to_string(), coerced.clone());
        if let Err(error) = self.persist() {
            self.log_line(format!("plugin: {error}"));
            self.flush_logs();
            return Err(error);
        }
        Ok(coerced)
    }

    /// One entry per discovered script, sorted by path.
    pub fn plugins(&self) -> Vec<PluginView> {
        let host_plugins = self.host.plugins();
        let errors = self.host.errors();
        self.registered
            .iter()
            .map(|entry| {
                let info = host_plugins.iter().find(|plugin| plugin.id == entry.id);
                let error = errors
                    .iter()
                    .find(|error| error.plugin_id == Some(entry.id))
                    .map(|error| error.message.clone());
                PluginView {
                    path: entry.key.clone(),
                    name: info.map(|info| info.name.clone()).unwrap_or_default(),
                    author: info.map(|info| info.author.clone()).unwrap_or_default(),
                    version: info.map(|info| info.version.clone()).unwrap_or_default(),
                    enabled: self.settings.is_plugin_enabled(&entry.key),
                    active: info.is_some_and(|info| info.active),
                    loaded: info.is_some_and(|info| info.loaded),
                    failed: info.is_some_and(|info| info.failed),
                    capture: info.and_then(|info| info.capture),
                    props: info
                        .and_then(|info| serde_json::to_value(&info.props).ok())
                        .unwrap_or(Value::Null),
                    error,
                    source_bytes: entry.source.len(),
                }
            })
            .collect()
    }

    /// Discovery warnings from the last scan (skipped/unreadable files, caps).
    pub fn discovery_errors(&self) -> &[String] {
        &self.discovery_errors
    }

    /// Supply the per-frame IO bundle: the audio features and read-only state.
    ///
    /// Called before `pre_frame` each frame while plugins are enabled. The
    /// state reader is a snapshot; the runtime never holds the live store.
    pub fn set_frame_inputs(&mut self, features: Value, state: Rc<dyn PluginStateReader>) {
        self.features = features;
        self.state = state;
    }

    /// Enable or disable the master switch.
    ///
    /// Enabling requires consent. Disabling runs every `shutdown` hook and
    /// drops the Boa contexts; the registered sources and prop values stay for
    /// a later enable.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if enabled && !self.settings.consented {
            return Err(
                "JavaScript plugins need the consent switch before they can be enabled".to_string(),
            );
        }
        self.settings.enabled = enabled;
        if enabled {
            // Pick up scripts added since startup before loading anything.
            self.rescan_sources();
        }
        self.apply_enabled();
        self.persist()
    }

    /// Record (or withdraw) the user's consent.
    ///
    /// Withdrawing consent also disables the master switch and drops contexts.
    /// Granting consent alone does not enable plugins.
    pub fn set_consented(&mut self, consented: bool) -> Result<(), String> {
        self.settings.consented = consented;
        if !consented {
            self.settings.enabled = false;
        }
        self.apply_enabled();
        self.persist()
    }

    /// Enable or disable one plugin by absolute script path.
    ///
    /// Persisted even when the script is not currently discovered, so a
    /// switch made before a rescan still applies once the file appears.
    pub fn set_plugin_enabled(&mut self, path: &str, enabled: bool) -> Result<(), String> {
        self.settings.set_plugin_enabled(path, enabled);
        if let Some(entry) = self.registered.iter().find(|entry| entry.key == path) {
            let id = entry.id;
            let mut io = self.io();
            if let Err(error) = self.host.set_plugin_enabled(id, enabled, &mut io) {
                self.log_line(format!("plugin: {}", error.message));
            }
            self.apply_saved_props();
            self.flush_logs();
        }
        self.capture.clear();
        self.persist()
    }

    /// Re-read discovery and re-evaluate every registered plugin.
    ///
    /// Unlike [`Self::rescan`], this forces each enabled plugin through load +
    /// `init` again, which is what a "Reload" button needs after editing a
    /// script without changing its file set.
    pub fn reload(&mut self) {
        self.rescan_sources();
        let ids: Vec<PluginId> = self.registered.iter().map(|entry| entry.id).collect();
        let mut io = self.io();
        for id in ids {
            if let Err(error) = self.host.reload(id, &mut io) {
                self.log_line(format!("plugin: {}", error.message));
            }
        }
        self.apply_saved_props();
        self.capture.clear();
        self.flush_logs();
    }

    /// Re-read the plugin folders. New scripts are added, removed scripts are
    /// dropped and changed sources are reloaded; unchanged scripts keep their
    /// contexts.
    pub fn rescan(&mut self) {
        self.rescan_sources();
        self.flush_logs();
    }

    /// Replace the discovery roots and rescan.
    ///
    /// The VmodV shell calls this after a project-profile switch so the new
    /// profile's `plugin/` folder is picked up without a restart.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>) {
        self.roots = roots;
        self.rescan();
    }

    /// Run every enabled `preProcessFrame` hook.
    pub fn pre_frame(&mut self) {
        if !self.host.enabled() {
            return;
        }
        {
            let mut io = self.io();
            self.host.pre_frame(&mut io);
        }
        self.flush_logs();
    }

    /// Run every enabled `postProcessFrame` hook.
    ///
    /// The last capture (if any) is passed to the host, which validates it and
    /// box-downscales it to each plugin's declared size.
    pub fn post_frame(&mut self) {
        if !self.host.enabled() {
            self.capture.clear();
            return;
        }
        {
            let mut io = self.io();
            let capture = self.capture.frame();
            let result = self.host.post_frame(&mut io, capture);
            if let Err(error) = result {
                self.log_line(format!("plugin: capture: {error}"));
            }
        }
        self.flush_logs();
    }

    /// The largest capture a running plugin asked for, or `None`.
    ///
    /// The frame loop only calls [`Self::capture_frame`] when this is `Some`,
    /// so a composition with no capture-requesting plugin never pays for a
    /// read-back.
    pub fn capture_config(&self) -> Option<CaptureConfig> {
        self.host.capture_config()
    }

    /// Read one capture through `read`, at most once per [`CAPTURE_INTERVAL`].
    ///
    /// `read(width, height)` must return tightly packed RGBA8 bytes (or `None`
    /// when the frame is not available). The result is validated by
    /// [`FrameCapture::new`], so a plugin can never receive a full-resolution
    /// frame or one outside the host caps. Returns whether a new capture was
    /// stored.
    pub fn capture_frame(
        &mut self,
        now: Instant,
        mut read: impl FnMut(u32, u32) -> Option<Vec<u8>>,
    ) -> bool {
        if !self.host.enabled() {
            self.capture.clear();
            return false;
        }
        let request = self.host.capture_config();
        let Some((width, height)) = self.capture.due(request, now) else {
            return false;
        };
        let Some(pixels) = read(width, height) else {
            self.capture.attempted(now);
            return false;
        };
        match FrameCapture::new(width, height, pixels) {
            Ok(frame) => {
                self.capture.store(frame, now);
                true
            }
            Err(error) => {
                self.capture.attempted(now);
                self.log_line(format!("plugin: capture {width}x{height} failed: {error}"));
                false
            }
        }
    }

    /// Run every `shutdown` hook and drop every Boa context.
    ///
    /// Called on quit. The persisted `enabled`/`consented` settings are kept,
    /// so a restart restores the same opt-in.
    pub fn shutdown(&mut self) {
        self.host.shutdown();
        self.capture.clear();
        self.flush_logs();
    }

    fn rescan_sources(&mut self) {
        let discovery = discover(&self.roots);
        self.discovery_errors = discovery.errors;
        for error in self.discovery_errors.clone() {
            self.log_line(format!("plugin: {error}"));
        }
        self.sync_sources(discovery.sources);
    }

    /// Reconcile the host registrations with the discovered sources.
    ///
    /// The host has no "remove everything" call, so a changed file set is
    /// rebuilt: contexts are shut down and the sources are registered again in
    /// sorted path order. An unchanged set is a no-op, which keeps a rescan
    /// from restarting every plugin on every menu refresh.
    fn sync_sources(&mut self, sources: Vec<DiscoveredSource>) {
        let unchanged = sources.len() == self.registered.len()
            && sources
                .iter()
                .zip(&self.registered)
                .all(|(source, entry)| source.path == entry.path && source.source == entry.source);
        if unchanged {
            return;
        }

        let mut io = self.io();
        let previous = std::mem::take(&mut self.registered);
        for entry in previous {
            self.host.remove(entry.id, &mut io);
        }
        self.capture.clear();

        for source in sources {
            let key = source.path.to_string_lossy().into_owned();
            match self.host.add_source(source.source.clone()) {
                Ok(id) => {
                    if !self.settings.is_plugin_enabled(&key)
                        && let Err(error) = self.host.set_plugin_enabled(id, false, &mut io)
                    {
                        self.log_line(format!("plugin: {}", error.message));
                    }
                    self.registered.push(RegisteredPlugin {
                        path: source.path,
                        key,
                        source: source.source,
                        id,
                    });
                }
                Err(error) => {
                    self.discovery_errors.push(format!(
                        "{}: {}",
                        source.path.display(),
                        error.message
                    ));
                }
            }
        }
        if self.host.enabled() {
            self.host.load_pending(&mut io);
            self.apply_saved_props();
        }
    }

    /// Reapply the persisted prop values to every loaded plugin.
    ///
    /// The host has no persisted-prop store of its own, so a rescan/reload or a
    /// fresh process would otherwise reset every control to the descriptor
    /// default. Props the current schema does not declare are skipped (a
    /// plugin update may rename one) and stay in the settings document.
    fn apply_saved_props(&mut self) {
        if self.settings.props.is_empty() {
            return;
        }
        let mut assignments = Vec::new();
        for info in self.host.plugins() {
            let Some(entry) = self.registered.iter().find(|entry| entry.id == info.id) else {
                continue;
            };
            let Some(saved) = self.settings.props.get(&entry.key) else {
                continue;
            };
            for (prop, value) in saved {
                if info.props.contains_key(prop) {
                    assignments.push((info.id, prop.clone(), value.clone()));
                }
            }
        }
        for (id, prop, value) in assignments {
            if let Err(error) = self.host.set_prop(id, &prop, value) {
                self.log_line(format!("plugin: {}", error.message));
            }
        }
    }

    /// Apply the master gate to the host.
    fn apply_enabled(&mut self) {
        let allowed = self.settings.allowed();
        {
            let mut io = self.io();
            self.host.set_enabled(allowed, &mut io);
            if allowed {
                self.host.load_pending(&mut io);
            } else {
                self.capture.clear();
            }
        }
        if allowed {
            self.apply_saved_props();
        }
        self.flush_logs();
    }

    fn persist(&self) -> Result<(), String> {
        self.settings.write_to(&self.settings_path)
    }

    fn io(&self) -> PluginIo {
        PluginIo::new(
            self.features.clone(),
            Rc::clone(&self.state),
            Rc::clone(&self.transport),
        )
    }

    /// Drain the host's `log()` lines and host-level errors as `plugin: …`
    /// lines, deduplicated so a hook that logs every frame cannot flood the
    /// daily log file.
    fn flush_logs(&mut self) {
        for log in self.host.drain_logs() {
            let level = match log.level {
                PluginLogLevel::Debug => "debug",
                PluginLogLevel::Info => "info",
                PluginLogLevel::Warn => "warn",
                PluginLogLevel::Error => "error",
            };
            let name = log
                .plugin_name
                .clone()
                .unwrap_or_else(|| log.plugin_id.to_string());
            self.log_line(format!("plugin: {level} {name}: {}", log.message));
        }
        // Errors attached to a plugin are already forwarded as `log()` lines
        // above; only host-level failures (cap/rejection) are not.
        for error in self
            .host
            .errors()
            .into_iter()
            .filter(|error| error.plugin_id.is_none())
        {
            self.log_line(format!(
                "plugin: {}: {}",
                phase_label(error.phase),
                error.message
            ));
        }
    }

    fn log_line(&mut self, line: String) {
        if self.logs_seen.len() >= MAX_LOGGED_LINES {
            self.logs_seen.clear();
        }
        if self.logs_seen.insert(line.clone()) {
            if self.logs.len() >= MAX_RETAINED_LOGS {
                self.logs.remove(0);
            }
            self.logs.push(line.clone());
            eprintln!("{line}");
        }
    }
}

fn phase_label(phase: PluginPhase) -> &'static str {
    match phase {
        PluginPhase::Load => "load",
        PluginPhase::Init => "init",
        PluginPhase::PreFrame => "pre-frame",
        PluginPhase::PostFrame => "post-frame",
        PluginPhase::Shutdown => "shutdown",
        PluginPhase::Props => "props",
        PluginPhase::Transport => "transport",
        PluginPhase::Capture => "capture",
        PluginPhase::Host => "host",
    }
}

/// The read-only state the plugin sandbox may see.
///
/// This is the **allow-list boundary** for `modV.get(path)`: only the paths
/// implemented here exist, everything else returns `None` (and the JavaScript
/// side sees `undefined`). The live store is never exposed wholesale.
///
/// Supported paths:
///
/// * `beats.bpm`, `beats.kick`
/// * `fps.fps`
/// * `meyda.bands.bass`, `meyda.bands.mid`, `meyda.bands.treble`
/// * `meyda.getFeature.<name>` for the scalar Meyda features (`rms`, `zcr`,
///   `energy`, `spectralCentroid`, `spectralFlatness`, `spectralSlope`,
///   `spectralRolloff`, `spectralSpread`, `spectralSkewness`,
///   `spectralKurtosis`, `perceptualSpread`, `perceptualSharpness`)
/// * `groups.<id>.name`
/// * `modules.<id>.moduleName`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VmodvStateReader {
    bpm: f64,
    kick: bool,
    fps: f64,
    bands: [f32; 3],
    features: BTreeMap<String, Value>,
    groups: BTreeMap<String, Value>,
    modules: BTreeMap<String, Value>,
}

impl VmodvStateReader {
    /// Snapshot the allow-listed fields from the engine store.
    pub fn from_store(store: &Store) -> Self {
        let audio = store.audio();
        Self {
            bpm: store.bpm(),
            kick: store.kick(),
            fps: store.target_fps(),
            bands: [audio.bass, audio.mid, audio.treble],
            features: scalar_feature_map(&audio.features),
            groups: store
                .state()
                .groups
                .iter()
                .map(|group| (group.id.clone(), Value::String(group.name.clone())))
                .collect(),
            modules: store
                .state()
                .modules
                .iter()
                .map(|(id, module)| (id.clone(), Value::String(module.module_name.clone())))
                .collect(),
        }
    }
}

impl PluginStateReader for VmodvStateReader {
    fn get(&self, path: &str) -> Option<Value> {
        match path {
            "beats.bpm" => Some(json!(self.bpm)),
            "beats.kick" => Some(json!(self.kick)),
            "fps.fps" => Some(json!(self.fps)),
            "meyda.bands.bass" => Some(json!(self.bands[0])),
            "meyda.bands.mid" => Some(json!(self.bands[1])),
            "meyda.bands.treble" => Some(json!(self.bands[2])),
            _ => {
                if let Some(name) = path.strip_prefix("meyda.getFeature.") {
                    return self.features.get(name).cloned();
                }
                if let Some(rest) = path.strip_prefix("groups.") {
                    let (id, field) = rest.rsplit_once('.')?;
                    return (field == "name")
                        .then(|| self.groups.get(id).cloned())
                        .flatten();
                }
                if let Some(rest) = path.strip_prefix("modules.") {
                    let (id, field) = rest.rsplit_once('.')?;
                    return (field == "moduleName")
                        .then(|| self.modules.get(id).cloned())
                        .flatten();
                }
                None
            }
        }
    }
}

/// The audio feature payload handed to `preProcessFrame`/`postProcessFrame`.
///
/// It is the same data the engine's modules see, flattened for JavaScript:
/// tempo/kick, the AGC bands, the damping lanes, the 16-cell grid, wave, the
/// analyser sample rate and the scalar Meyda features.
pub fn features_from_store(store: &Store) -> Value {
    let audio = store.audio();
    let mut map = Map::new();
    map.insert("bpm".to_string(), json!(store.bpm()));
    map.insert("kick".to_string(), json!(store.kick()));
    map.insert("bass".to_string(), json!(audio.bass));
    map.insert("mid".to_string(), json!(audio.mid));
    map.insert("treble".to_string(), json!(audio.treble));
    map.insert("beat".to_string(), json!(audio.beat));
    map.insert("lanes".to_string(), float_array(&audio.lanes));
    map.insert("cells".to_string(), float_array(&audio.cells));
    map.insert("wave".to_string(), float_array(&audio.wave));
    map.insert("sampleRate".to_string(), json!(audio.sample_rate));
    for (name, value) in scalar_feature_map(&audio.features) {
        map.insert(name, value);
    }
    Value::Object(map)
}

fn float_array(values: &[f32]) -> Value {
    Value::Array(values.iter().map(|value| json!(value)).collect())
}

/// The scalar Meyda features, keyed the way `meyda/getFeature` links name them.
fn scalar_feature_map(features: &AudioFeatures) -> BTreeMap<String, Value> {
    [
        ("rms", features.rms),
        ("zcr", features.zcr),
        ("energy", features.energy),
        ("spectralCentroid", features.spectral_centroid),
        ("spectralFlatness", features.spectral_flatness),
        ("spectralSlope", features.spectral_slope),
        ("spectralRolloff", features.spectral_rolloff),
        ("spectralSpread", features.spectral_spread),
        ("spectralSkewness", features.spectral_skewness),
        ("spectralKurtosis", features.spectral_kurtosis),
        ("perceptualSpread", features.perceptual_spread),
        ("perceptualSharpness", features.perceptual_sharpness),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), json!(value)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn legacy_native_plugins_filename_is_read_as_a_fallback() {
        let dir = temp_dir("modv-plugins-compat");
        let legacy = dir.join("native-plugins.json");
        let saved = PluginSettings {
            enabled: true,
            consented: true,
            ..Default::default()
        };
        saved.write_to(&legacy).expect("write legacy plugins");
        let current = dir.join(PLUGIN_SETTINGS_FILENAME);
        assert_eq!(
            PluginSettings::read_from(&current).expect("read legacy plugins"),
            Some(saved)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    const COUNTER: &str = r#"
module.exports = {
  name: "Counter",
  author: "modV tests",
  version: "1.2.3",
  props: { count: { type: "int", default: 0, min: 0, max: 1000 } },
  init({ log }) { log("info", "init"); },
  preProcessFrame({ props }) { props.count = props.count + 1; },
  shutdown({ log }) { log("info", "shutdown"); },
};
"#;

    const FRAME_PROBE: &str = r#"
module.exports = {
  name: "FrameProbe",
  captureWidth: 4,
  captureHeight: 3,
  props: {
    bpm: { type: "float", default: 0 },
    kick: { type: "bool", default: false },
    group: { type: "text", default: "" },
    width: { type: "int", default: 0 },
    height: { type: "int", default: 0 },
    bytes: { type: "int", default: 0 },
  },
  preProcessFrame({ features, props, modV }) {
    props.bpm = features.bpm;
    props.kick = modV.get("beats.kick");
    props.group = modV.get("groups.g1.name");
  },
  postProcessFrame({ width, height, pixels, props }) {
    props.width = width;
    props.height = height;
    props.bytes = pixels ? pixels.length : 0;
  },
};
"#;

    const SENDER: &str = r#"
module.exports = {
  name: "Sender",
  props: { ok: { type: "bool", default: true } },
  preProcessFrame({ props, modV }) {
    props.ok = modV.ws("ws://localhost:3006/modV", { colors: [1, 2, 3] });
  },
};
"#;

    struct FakeState {
        values: BTreeMap<String, Value>,
    }

    impl FakeState {
        fn new(values: &[(&str, Value)]) -> Self {
            Self {
                values: values
                    .iter()
                    .map(|(path, value)| ((*path).to_string(), value.clone()))
                    .collect(),
            }
        }
    }

    impl PluginStateReader for FakeState {
        fn get(&self, path: &str) -> Option<Value> {
            self.values.get(path).cloned()
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "modv-plugin-runtime-{prefix}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_plugin(dir: &Path, name: &str, source: &str) -> PathBuf {
        std::fs::create_dir_all(dir).expect("plugin dir");
        let path = dir.join(name);
        std::fs::write(&path, source).expect("write plugin");
        path.canonicalize().expect("canonical plugin path")
    }

    fn runtime_for(dir: &Path) -> PluginRuntime {
        PluginRuntime::load(
            dir.join(PLUGIN_SETTINGS_FILENAME),
            vec![dir.join("plugins")],
        )
    }

    fn inputs(runtime: &mut PluginRuntime) {
        runtime.set_frame_inputs(
            json!({ "bpm": 128.0, "bass": 0.5, "mid": 0.25, "treble": 0.125 }),
            Rc::new(FakeState::new(&[
                ("beats.kick", json!(true)),
                ("groups.g1.name", json!("Effects")),
            ])),
        );
    }

    fn prop(plugin: &PluginView, name: &str) -> Value {
        plugin.props[name]["value"].clone()
    }

    fn number(value: &Value) -> f64 {
        value.as_f64().expect("a JSON number")
    }

    fn reader_number(reader: &VmodvStateReader, path: &str) -> f64 {
        number(&reader.get(path).expect("an allow-listed path"))
    }

    #[test]
    fn settings_round_trip_atomically_and_malformed_files_are_errors() {
        let dir = temp_dir("settings");
        let path = dir.join(PLUGIN_SETTINGS_FILENAME);
        assert_eq!(PluginSettings::read_from(&path), Ok(None));

        let mut settings = PluginSettings {
            enabled: true,
            consented: true,
            plugins: BTreeMap::new(),
            props: BTreeMap::new(),
        };
        settings.set_plugin_enabled("/plugins/a.js", false);
        settings.write_to(&path).expect("write settings");
        assert!(
            !dir.join("vmodv-plugins.json.tmp").exists(),
            "the atomic write must not leave its temporary file behind"
        );
        assert_eq!(
            PluginSettings::read_from(&path).expect("read settings"),
            Some(settings.clone())
        );

        let text = std::fs::read_to_string(&path).expect("read raw");
        let value: Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(
            value,
            json!({
                "enabled": true,
                "consented": true,
                "plugins": { "/plugins/a.js": false }
            })
        );

        std::fs::write(&path, "{ not json").expect("corrupt");
        assert!(PluginSettings::read_from(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enabled_without_consent_is_normalised_to_disabled() {
        let dir = temp_dir("consent");
        let path = dir.join(PLUGIN_SETTINGS_FILENAME);
        std::fs::write(
            &path,
            r#"{"enabled":true,"consented":false,"plugins":{"/a.js":false}}"#,
        )
        .expect("write document");

        let settings = PluginSettings::read_from(&path)
            .expect("read settings")
            .expect("document");
        assert!(
            !settings.enabled,
            "hand-edited enable must not bypass consent"
        );
        assert!(!settings.allowed());
        assert!(
            !settings.is_plugin_enabled("/a.js"),
            "per-plugin switches survive normalisation"
        );

        let mut runtime = runtime_for(&dir);
        runtime.start();
        assert!(!runtime.enabled());
        assert_eq!(runtime.loaded_count(), 0);
        let error = runtime.set_enabled(true).expect_err("consent is required");
        assert!(error.contains("consent"), "{error}");
        assert!(!runtime.enabled());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn discovery_is_sorted_deduplicated_capped_and_skips_bad_sources() {
        let dir = temp_dir("discovery");
        let plugins = dir.join("plugins");
        let first = write_plugin(&plugins, "a.JS", "module.exports = { name: 'A' };");
        let second = write_plugin(&plugins, "b.js", "module.exports = { name: 'B' };");
        std::fs::write(plugins.join("notes.txt"), "ignored").expect("write txt");
        std::fs::write(plugins.join(".hidden.js"), "ignored").expect("write hidden");
        std::fs::create_dir_all(plugins.join("nested")).expect("nested dir");
        std::fs::write(plugins.join("nested/c.js"), "module.exports = {};").expect("nested js");

        let discovery = discover(&[plugins.clone(), plugins.clone()]);
        let paths: Vec<&Path> = discovery
            .sources
            .iter()
            .map(|source| source.path.as_path())
            .collect();
        assert_eq!(
            paths,
            vec![first.as_path(), second.as_path()],
            "one entry per canonical path, sorted and de-duplicated"
        );
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);

        // The host caps: oversized and non-UTF-8 sources are skipped.
        let bad_dir = temp_dir("discovery-bad");
        std::fs::write(
            bad_dir.join("oversize.js"),
            " ".repeat(MAX_PLUGIN_SOURCE_BYTES + 1),
        )
        .expect("oversize");
        std::fs::write(bad_dir.join("binary.js"), [0xff, 0xfe, 0xfd]).expect("binary");
        let discovery = discover(std::slice::from_ref(&bad_dir));
        assert!(discovery.sources.is_empty());
        assert!(
            discovery
                .errors
                .iter()
                .any(|error| error.contains("oversize.js")),
            "{:?}",
            discovery.errors
        );
        assert!(
            discovery
                .errors
                .iter()
                .any(|error| error.contains("binary.js")),
            "{:?}",
            discovery.errors
        );

        let cap_dir = temp_dir("discovery-cap");
        for index in 0..(MAX_PLUGINS + 2) {
            std::fs::write(
                cap_dir.join(format!("p{index:02}.js")),
                "module.exports = { name: 'P' };",
            )
            .expect("plugin");
        }
        let discovery = discover(std::slice::from_ref(&cap_dir));
        assert_eq!(discovery.sources.len(), MAX_PLUGINS);
        assert!(
            discovery
                .errors
                .iter()
                .any(|error| error.contains("only the first")),
            "{:?}",
            discovery.errors
        );
        assert_eq!(
            discovery.sources[0].path.file_name().unwrap(),
            "p00.js",
            "the survivors keep path order"
        );

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(bad_dir);
        let _ = std::fs::remove_dir_all(cap_dir);
    }

    #[test]
    fn a_disabled_runtime_allocates_no_contexts_and_evaluates_nothing() {
        let dir = temp_dir("disabled");
        write_plugin(&dir.join("plugins"), "counter.js", COUNTER);
        let mut runtime = runtime_for(&dir);
        inputs(&mut runtime);
        runtime.start();

        assert!(!runtime.enabled());
        assert_eq!(runtime.loaded_count(), 0);
        let plugins = runtime.plugins();
        assert_eq!(plugins.len(), 1);
        assert!(plugins[0].path.ends_with("counter.js"));
        assert!(!plugins[0].loaded);
        assert!(!plugins[0].failed);
        assert!(runtime.host.logs().is_empty(), "no source may be evaluated");

        runtime.pre_frame();
        assert!(runtime.host.logs().is_empty());
        runtime.post_frame();
        assert!(runtime.host.logs().is_empty());
        assert!(!runtime.capture_frame(Instant::now(), |_, _| {
            panic!("a disabled runtime must not read a frame")
        }));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enabling_requires_consent_and_loads_only_enabled_plugins() {
        let dir = temp_dir("enable");
        let plugin = write_plugin(&dir.join("plugins"), "counter.js", COUNTER);
        let mut runtime = runtime_for(&dir);
        inputs(&mut runtime);
        runtime.start();

        runtime.set_consented(true).expect("consent persists");
        runtime.set_enabled(true).expect("enable persists");
        assert!(runtime.enabled());
        assert_eq!(runtime.loaded_count(), 1);
        let plugins = runtime.plugins();
        assert_eq!(plugins[0].name, "Counter");
        assert_eq!(plugins[0].author, "modV tests");
        assert_eq!(plugins[0].version, "1.2.3");
        assert!(plugins[0].loaded);

        runtime
            .set_plugin_enabled(&plugin.to_string_lossy(), false)
            .expect("disable one plugin");
        assert_eq!(runtime.loaded_count(), 0);
        assert!(!runtime.plugins()[0].enabled);
        assert!(!runtime.plugins()[0].loaded);

        runtime
            .set_plugin_enabled(&plugin.to_string_lossy(), true)
            .expect("enable one plugin");
        assert_eq!(runtime.loaded_count(), 1);

        let saved = PluginSettings::read_from(&dir.join(PLUGIN_SETTINGS_FILENAME))
            .expect("read saved settings")
            .expect("document");
        assert!(saved.enabled && saved.consented);
        assert_eq!(
            saved.plugins.get(plugin.to_string_lossy().as_ref()),
            Some(&true)
        );

        // Withdrawing consent disables the host and drops the context.
        runtime.set_consented(false).expect("withdraw consent");
        assert!(!runtime.enabled());
        assert_eq!(runtime.loaded_count(), 0);
        let saved = PluginSettings::read_from(&dir.join(PLUGIN_SETTINGS_FILENAME))
            .expect("read saved settings")
            .expect("document");
        assert!(!saved.enabled && !saved.consented);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pre_and_post_hooks_receive_the_frame_payload_and_a_full_capture() {
        let dir = temp_dir("frame");
        write_plugin(&dir.join("plugins"), "probe.js", FRAME_PROBE);
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        inputs(&mut runtime);
        runtime.start();
        runtime.set_enabled(true).expect("enable");

        let now = Instant::now();
        assert!(
            runtime.capture_frame(now, |width, height| {
                assert_eq!((width, height), (4, 3));
                Some(vec![7; (width * height * 4) as usize])
            }),
            "the first capture is due"
        );
        runtime.pre_frame();
        runtime.post_frame();

        let plugins = runtime.plugins();
        assert_eq!(prop(&plugins[0], "bpm"), json!(128.0));
        assert_eq!(prop(&plugins[0], "kick"), json!(true));
        assert_eq!(prop(&plugins[0], "group"), json!("Effects"));
        assert_eq!(prop(&plugins[0], "width"), json!(4));
        assert_eq!(prop(&plugins[0], "height"), json!(3));
        assert_eq!(prop(&plugins[0], "bytes"), json!(48));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn capture_is_throttled_bounded_and_cleared_on_disable() {
        let dir = temp_dir("capture");
        write_plugin(&dir.join("plugins"), "probe.js", FRAME_PROBE);
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        inputs(&mut runtime);
        runtime.start();
        runtime.set_enabled(true).expect("enable");

        let reads = Cell::new(0);
        let start = Instant::now();
        let mut read = |width: u32, height: u32| {
            reads.set(reads.get() + 1);
            Some(vec![1; (width * height * 4) as usize])
        };
        assert!(runtime.capture_frame(start, &mut read));
        assert_eq!(reads.get(), 1);
        assert!(!runtime.capture_frame(start + Duration::from_millis(10), &mut read));
        assert_eq!(reads.get(), 1, "the 20 Hz throttle skips the read-back");
        runtime.post_frame();
        assert_eq!(
            prop(&runtime.plugins()[0], "width"),
            json!(4),
            "the cached capture is still handed to postProcessFrame"
        );
        assert!(runtime.capture_frame(start + CAPTURE_INTERVAL, &mut read));
        assert_eq!(reads.get(), 2);

        // A stale/wrong-sized buffer is rejected, not handed to the plugin.
        let late = start + CAPTURE_INTERVAL * 2;
        assert!(!runtime.capture_frame(late, |_, _| Some(vec![0; 4])));
        runtime.post_frame();
        assert_eq!(
            prop(&runtime.plugins()[0], "bytes"),
            json!(48),
            "the previous valid capture is kept after a failed read"
        );

        runtime.set_enabled(false).expect("disable");
        assert!(!runtime.capture_frame(late + CAPTURE_INTERVAL, |_, _| {
            panic!("a disabled runtime must not read a frame")
        }));
        assert!(runtime.capture_config().is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn capture_due_clamps_requested_dimensions() {
        let mut capture = PluginCapture::default();
        let now = Instant::now();
        assert_eq!(
            capture.due(
                Some(CaptureConfig {
                    width: 0,
                    height: 100_000
                }),
                now
            ),
            Some((1, MAX_CAPTURE_DIMENSION))
        );
        assert_eq!(capture.due(None, now), None, "no request clears the cache");
        assert!(capture.frame().is_none());
    }

    #[test]
    fn the_state_reader_exposes_only_the_allow_list() {
        let reader = VmodvStateReader {
            bpm: 120.0,
            kick: true,
            fps: 30.0,
            bands: [0.1, 0.2, 0.3],
            features: BTreeMap::from([("rms".to_string(), json!(0.4))]),
            groups: BTreeMap::from([("g1".to_string(), json!("Effects"))]),
            modules: BTreeMap::from([("m1".to_string(), json!("Bands"))]),
        };

        assert_eq!(reader.get("beats.bpm"), Some(json!(120.0)));
        assert_eq!(reader.get("beats.kick"), Some(json!(true)));
        assert_eq!(reader.get("fps.fps"), Some(json!(30.0)));
        // The bands/features are f32 in the engine, so compare with tolerance.
        assert!((reader_number(&reader, "meyda.bands.bass") - 0.1).abs() < 1e-6);
        assert!((reader_number(&reader, "meyda.bands.mid") - 0.2).abs() < 1e-6);
        assert!((reader_number(&reader, "meyda.bands.treble") - 0.3).abs() < 1e-6);
        assert!((reader_number(&reader, "meyda.getFeature.rms") - 0.4).abs() < 1e-6);
        assert_eq!(reader.get("groups.g1.name"), Some(json!("Effects")));
        assert_eq!(reader.get("modules.m1.moduleName"), Some(json!("Bands")));

        for unknown in [
            "",
            "beats",
            "beats.bpm.extra",
            "meyda.getFeature",
            "meyda.getFeature.unknown",
            "meyda.getFeature.rms.extra",
            "groups",
            "groups.g1",
            "groups.g1.alpha",
            "modules.m1",
            "modules.m1.name",
            "inputs",
            "tweens",
            "groups.unknown.name",
        ] {
            assert_eq!(reader.get(unknown), None, "{unknown:?} is not allow-listed");
        }
    }

    #[test]
    fn the_feature_payload_flattens_the_store_audio_frame() {
        let mut store = Store::new();
        store
            .dispatch(
                "meyda/SET_BANDS",
                json!({
                    "bass": 0.5,
                    "mid": 0.25,
                    "treble": 0.125,
                    "beat": 0.75,
                    "wave": [0.1, 0.2],
                    "rms": 0.3,
                }),
            )
            .expect("bands dispatch");
        store
            .dispatch("beats/SET_BPM", json!({ "bpm": 128.0 }))
            .expect("bpm dispatch");
        store
            .dispatch("fps/SET_FPS", json!({ "fps": 30.0 }))
            .expect("fps dispatch");

        let features = features_from_store(&store);
        assert_eq!(features["bpm"], json!(128.0));
        assert!((number(&features["bass"]) - 0.5).abs() < 1e-6);
        assert!((number(&features["mid"]) - 0.25).abs() < 1e-6);
        assert!((number(&features["treble"]) - 0.125).abs() < 1e-6);
        assert!((number(&features["beat"]) - 0.75).abs() < 1e-6);
        assert!((number(&features["rms"]) - 0.3).abs() < 1e-6);
        assert_eq!(features["sampleRate"], json!(44_100.0));
        assert_eq!(features["lanes"].as_array().map(Vec::len), Some(12));
        assert_eq!(features["cells"].as_array().map(Vec::len), Some(16));
        assert_eq!(features["wave"].as_array().map(Vec::len), Some(1024));
        assert!((number(&features["wave"][0]) - 0.1).abs() < 1e-6);
        assert!((number(&features["wave"][1]) - 0.2).abs() < 1e-6);

        let reader = VmodvStateReader::from_store(&store);
        assert_eq!(reader.get("fps.fps"), Some(json!(30.0)));
        assert!((reader_number(&reader, "meyda.getFeature.rms") - 0.3).abs() < 1e-6);
        assert!((reader_number(&reader, "meyda.bands.bass") - 0.5).abs() < 1e-6);
    }

    #[test]
    fn transport_errors_are_recorded_once_and_do_not_stop_the_hook() {
        let dir = temp_dir("transport");
        write_plugin(&dir.join("plugins"), "sender.js", SENDER);
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        inputs(&mut runtime);
        runtime.start();
        runtime.set_enabled(true).expect("enable");

        runtime.pre_frame();
        runtime.pre_frame();

        let errors = runtime.errors();
        let transport: Vec<&PluginError> = errors
            .iter()
            .filter(|error| error.phase == PluginPhase::Transport)
            .collect();
        assert_eq!(
            transport.len(),
            1,
            "the host deduplicates by plugin + phase"
        );
        assert!(
            transport[0].message.contains("not wired"),
            "{}",
            transport[0].message
        );
        assert_eq!(
            prop(&runtime.plugins()[0], "ok"),
            json!(false),
            "modV.ws returns false but the hook still runs"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn one_throwing_plugin_does_not_stop_the_others() {
        let dir = temp_dir("isolation");
        write_plugin(
            &dir.join("plugins"),
            "a-fail.js",
            r#"
module.exports = {
  name: "Failer",
  preProcessFrame() { throw new Error("boom"); },
};
"#,
        );
        write_plugin(
            &dir.join("plugins"),
            "b-ok.js",
            r#"
module.exports = {
  name: "Survivor",
  props: { value: { type: "int", default: 0 } },
  preProcessFrame({ props }) { props.value = 42; },
};
"#,
        );
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        inputs(&mut runtime);
        runtime.start();
        runtime.set_enabled(true).expect("enable");

        runtime.pre_frame();
        runtime.pre_frame();

        let plugins = runtime.plugins();
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0].name, "Failer");
        assert!(
            plugins[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("boom")),
            "a throwing hook is reported per plugin: {:?}",
            plugins[0].error
        );
        assert!(!plugins[0].loaded || plugins[0].error.is_some());
        assert_eq!(prop(&plugins[1], "value"), json!(42));
        assert!(
            runtime
                .errors()
                .iter()
                .filter(|error| error.phase == PluginPhase::PreFrame)
                .count()
                == 1,
            "the failure is reported once, not every frame"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_sources_fail_per_plugin_without_panicking() {
        let dir = temp_dir("malformed");
        write_plugin(&dir.join("plugins"), "a-broken.js", "module.exports = {");
        write_plugin(&dir.join("plugins"), "b-good.js", COUNTER);
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        inputs(&mut runtime);
        runtime.start();
        runtime.set_enabled(true).expect("enable");
        runtime.pre_frame();

        let plugins = runtime.plugins();
        assert_eq!(plugins.len(), 2);
        assert!(plugins[0].failed);
        assert!(plugins[0].error.is_some());
        assert_eq!(plugins[1].name, "Counter");
        assert!(plugins[1].loaded);
        assert_eq!(runtime.loaded_count(), 1);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rescan_adds_removes_and_keeps_deterministic_order() {
        let dir = temp_dir("rescan");
        let plugins = dir.join("plugins");
        let a = write_plugin(&plugins, "a.js", COUNTER);
        write_plugin(&plugins, "b.js", &COUNTER.replace("Counter", "Second"));
        let mut runtime = runtime_for(&dir);
        inputs(&mut runtime);
        runtime.start();
        assert_eq!(runtime.plugins().len(), 2);

        std::fs::remove_file(&a).expect("remove a");
        let c = write_plugin(&plugins, "c.js", &COUNTER.replace("Counter", "Third"));
        runtime.rescan();
        let paths: Vec<String> = runtime
            .plugins()
            .into_iter()
            .map(|plugin| plugin.path)
            .collect();
        assert_eq!(
            paths,
            vec![
                plugins
                    .join("b.js")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                c.to_string_lossy().to_string(),
            ]
        );

        runtime.set_consented(true).expect("consent");
        runtime.set_enabled(true).expect("enable");
        assert_eq!(runtime.loaded_count(), 2);

        // An unchanged rescan keeps the contexts (no reload churn).
        runtime.rescan();
        assert_eq!(runtime.loaded_count(), 2);

        // `reload` re-evaluates a script edited in place, without a path
        // change, and the descriptor follows the new source.
        std::fs::write(&c, COUNTER.replace("Counter", "Renamed")).expect("rewrite c");
        runtime.reload();
        assert_eq!(runtime.loaded_count(), 2);
        assert!(
            runtime
                .plugins()
                .iter()
                .any(|plugin| plugin.name == "Renamed"),
            "{:?}",
            runtime
                .plugins()
                .into_iter()
                .map(|plugin| plugin.name)
                .collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn switching_discovery_roots_picks_up_the_new_profile_folder() {
        let dir = temp_dir("roots");
        let first = write_plugin(&dir.join("plugins"), "first.js", COUNTER);
        let other = dir.join("other-profile");
        let second = write_plugin(&other, "second.js", &COUNTER.replace("Counter", "Other"));
        let mut runtime = runtime_for(&dir);
        runtime.start();
        assert_eq!(
            runtime.plugins()[0].path,
            first.to_string_lossy().to_string()
        );

        runtime.set_roots(vec![other]);
        let plugins = runtime.plugins();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].path, second.to_string_lossy().to_string());

        // Enabling after the switch loads the new folder's script.
        runtime.set_consented(true).expect("consent");
        runtime.set_enabled(true).expect("enable");
        assert_eq!(runtime.plugins()[0].name, "Other");
        assert_eq!(runtime.loaded_count(), 1);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shutdown_drops_contexts_but_keeps_the_persisted_opt_in() {
        let dir = temp_dir("shutdown");
        write_plugin(&dir.join("plugins"), "probe.js", FRAME_PROBE);
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        inputs(&mut runtime);
        runtime.start();
        runtime.set_enabled(true).expect("enable");
        let now = Instant::now();
        assert!(runtime.capture_frame(now, |width, height| Some(vec![
            0;
            (width * height * 4)
                as usize
        ])));

        runtime.shutdown();
        assert_eq!(runtime.loaded_count(), 0);
        assert!(!runtime.host.enabled());
        assert!(!runtime.capture_frame(now, |_, _| panic!("no read after shutdown")));

        let saved = PluginSettings::read_from(&dir.join(PLUGIN_SETTINGS_FILENAME))
            .expect("read saved settings")
            .expect("document");
        assert!(saved.enabled && saved.consented);

        // `start` restores the persisted opt-in.
        runtime.start();
        assert!(runtime.enabled());
        assert_eq!(runtime.loaded_count(), 1);

        let _ = std::fs::remove_dir_all(dir);
    }

    const PROPS: &str = r#"
module.exports = {
  name: "Props",
  props: {
    count: { type: "int", default: 0, min: 0, max: 10 },
    gain: { type: "float", default: 0.5, min: 0, max: 2 },
    label: { type: "text", default: "hello" },
    tint: { type: "color", default: { r: 0, g: 0, b: 0, a: 1 } },
    mode: {
      type: "select",
      default: "a",
      options: [
        { label: "A", value: "a" },
        { label: "B", value: "b" },
      ],
    },
  },
};
"#;

    #[test]
    fn retained_logs_are_deduplicated_and_cleared() {
        let dir = temp_dir("logs");
        let plugins = dir.join("plugins");
        write_plugin(&plugins, "counter.js", COUNTER);
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        runtime.start();
        runtime.set_enabled(true).expect("enable");

        let logs = runtime.logs().to_vec();
        assert!(
            logs.iter().any(|line| line == "plugin: info Counter: init"),
            "the init log is retained for the panel: {logs:?}"
        );

        // Running the hook twice must not duplicate the line.
        runtime.pre_frame();
        runtime.pre_frame();
        let count = runtime
            .logs()
            .iter()
            .filter(|line| *line == "plugin: info Counter: init")
            .count();
        assert_eq!(count, 1, "retained lines are deduplicated");

        // `reload` re-runs init; the line is still one entry.
        runtime.reload();
        assert_eq!(
            runtime
                .logs()
                .iter()
                .filter(|line| *line == "plugin: info Counter: init")
                .count(),
            1
        );

        // Clearing drops the retained window. Already-seen lines stay deduped
        // (a per-frame logger must not repopulate the panel), while a new
        // distinct event is recorded again.
        runtime.clear_logs();
        assert!(runtime.logs().is_empty(), "{:?}", runtime.logs());
        runtime.pre_frame();
        runtime.pre_frame();
        assert!(
            runtime.logs().is_empty(),
            "seen lines are not re-emitted: {:?}",
            runtime.logs()
        );
        write_plugin(&plugins, "second.js", &COUNTER.replace("Counter", "Second"));
        runtime.rescan();
        assert!(
            runtime
                .logs()
                .iter()
                .any(|line| line == "plugin: info Second: init"),
            "{:?}",
            runtime.logs()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plugin_errors_reach_the_retained_log() {
        let dir = temp_dir("error-log");
        write_plugin(
            &dir.join("plugins"),
            "fail.js",
            r#"
module.exports = {
  name: "Failer",
  preProcessFrame() { throw new Error("boom"); },
};
"#,
        );
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        runtime.start();
        runtime.set_enabled(true).expect("enable");
        runtime.pre_frame();

        assert!(
            runtime
                .logs()
                .iter()
                .any(|line| line.contains("error") && line.contains("boom")),
            "{:?}",
            runtime.logs()
        );
        let plugin = &runtime.plugins()[0];
        assert!(
            plugin
                .error
                .as_deref()
                .is_some_and(|error| error.contains("boom"))
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prop_writes_are_coerced_persisted_and_reapplied_after_reload() {
        let dir = temp_dir("props");
        let plugin = write_plugin(&dir.join("plugins"), "props.js", PROPS);
        let key = plugin.to_string_lossy().to_string();
        let mut runtime = runtime_for(&dir);
        runtime.set_consented(true).expect("consent");
        runtime.start();
        runtime.set_enabled(true).expect("enable");

        assert_eq!(
            runtime
                .set_prop(&key, "count", json!(42.9))
                .expect("set int"),
            json!(10),
            "the host clamps and rounds against the schema"
        );
        assert_eq!(
            runtime
                .set_prop(&key, "gain", json!(1.25))
                .expect("set float"),
            json!(1.25)
        );
        assert_eq!(
            runtime
                .set_prop(&key, "label", json!("edited"))
                .expect("set text"),
            json!("edited")
        );
        assert_eq!(
            runtime
                .set_prop(&key, "tint", json!({ "r": 1, "g": 0.5, "b": 0, "a": 1 }))
                .expect("set colour"),
            json!({ "r": 1.0, "g": 0.5, "b": 0.0, "a": 1.0 })
        );
        assert_eq!(
            runtime
                .set_prop(&key, "mode", json!("b"))
                .expect("set select"),
            json!("b")
        );
        assert_eq!(
            runtime
                .set_prop(&key, "mode", json!("unknown"))
                .expect("coerced"),
            json!("a"),
            "an unknown option falls back to the default"
        );

        // The live descriptor follows the writes.
        let props = &runtime.plugins()[0].props;
        assert_eq!(props["count"]["value"], json!(10));
        assert_eq!(props["gain"]["value"], json!(1.25));
        assert_eq!(props["label"]["value"], json!("edited"));
        assert_eq!(props["mode"]["value"], json!("a"));

        // The write is persisted so a reload and a restart restore it.
        let saved = PluginSettings::read_from(&dir.join(PLUGIN_SETTINGS_FILENAME))
            .expect("read saved settings")
            .expect("document");
        assert_eq!(saved.props[&key]["count"], json!(10));
        assert_eq!(saved.props[&key]["label"], json!("edited"));

        runtime.reload();
        assert_eq!(
            runtime.plugins()[0].props["label"]["value"],
            json!("edited")
        );
        assert_eq!(runtime.plugins()[0].props["count"]["value"], json!(10));

        let mut restarted = runtime_for(&dir);
        restarted.set_consented(true).expect("consent");
        restarted.start();
        restarted.set_enabled(true).expect("enable");
        assert_eq!(
            restarted.plugins()[0].props["label"]["value"],
            json!("edited"),
            "a fresh runtime applies the saved prop values"
        );
        assert_eq!(restarted.plugins()[0].props["gain"]["value"], json!(1.25));

        // Unknown plugins/props are errors, not panics.
        assert!(runtime.set_prop("/missing.js", "count", json!(1)).is_err());
        assert!(runtime.set_prop(&key, "missing", json!(1)).is_err());
        assert!(
            runtime
                .set_prop(&key, "count", json!("not a number"))
                .is_ok()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prop_writes_require_a_loaded_schema() {
        let dir = temp_dir("props-disabled");
        let plugin = write_plugin(&dir.join("plugins"), "props.js", PROPS);
        let key = plugin.to_string_lossy().to_string();
        let mut runtime = runtime_for(&dir);
        runtime.start();

        let error = runtime
            .set_prop(&key, "count", json!(1))
            .expect_err("a disabled plugin has no live schema");
        assert!(
            error.contains("schema") || error.contains("loaded"),
            "{error}"
        );
        assert!(
            runtime.plugins().iter().all(|plugin| plugin
                .props
                .as_object()
                .is_none_or(serde_json::Map::is_empty)),
            "nothing is evaluated while the gate is off"
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
