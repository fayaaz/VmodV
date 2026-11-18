//! Read models the native UI renders.
//!
//! `modv-app` builds one [`EngineSnapshot`] per frame from the authoritative
//! `Store`. The UI only reads this snapshot and emits actions; it never holds
//! a reference to the store, so egui's immediate-mode draw calls cannot
//! conflict with a dispatch.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

/// The project profile shown by the File menu.
///
/// Electron's media manager keeps every profile under `<userData>/media`; the
/// active one provides the ISF/image/video/palette folders the shell watches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectView {
    /// The active profile name (`default` when nothing is saved).
    pub active_profile: String,
    /// Every profile folder under `<userData>/media`, sorted.
    pub profiles: Vec<String>,
}

/// The profile the JS media manager creates on first launch.
pub const DEFAULT_PROFILE_NAME: &str = "default";

impl Default for ProjectView {
    fn default() -> Self {
        Self {
            active_profile: DEFAULT_PROFILE_NAME.to_string(),
            profiles: Vec::new(),
        }
    }
}

/// Everything the shell needs to draw one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineSnapshot {
    /// Groups in store order (the render order is derived by the engine).
    pub groups: Vec<GroupView>,
    /// Active module instances.
    pub modules: Vec<ModuleView>,
    /// Saved Scenes in grid order.
    pub scenes: Vec<SceneView>,
    /// Every registered module definition (Gallery/Gallery search).
    pub registered: Vec<ModuleMetaView>,
    /// Performance-mode configuration and runtime state.
    pub performance: PerformanceView,
    /// Audio analysis for meters.
    pub audio: AudioView,
    /// Output resolution (the compositor's main texture size).
    pub resolution: [u32; 2],
    /// Measured main-loop frames per second.
    pub fps: f32,
    /// Most recent frame time in milliseconds.
    pub frame_ms: f32,
    /// Performance row names, in grid order.
    pub scene_rows: Vec<String>,
    /// The Scene the engine last applied.
    pub applied_scene_id: Option<String>,
    /// The live Scene id for each performance row, keyed by row
    /// (`scenes/SET_ACTIVE_ROWS` without the Group ids).
    ///
    /// Performance mode advances rows independently, so this can name
    /// different Scenes in different rows; transition ghosts and Groups with
    /// no Scene stamp are excluded.
    pub active_scene_rows: BTreeMap<usize, String>,
    /// Engine/adapter health for the status bar.
    pub health: EngineHealth,
    /// Gallery hover previews keyed by module name.
    pub gallery_previews: HashMap<String, PreviewTexture>,
    /// Scene-cell hover previews keyed by [`scene_preview_key`].
    pub scene_previews: HashMap<String, PreviewTexture>,
    /// Live row monitors keyed by performance row (the row gutter's
    /// thumbnail of the row's currently active Group).
    pub row_monitors: HashMap<usize, RowMonitorView>,
    /// Live texture sources the Inspector's texture picker can bind.
    pub texture_sources: Vec<TextureSourceView>,
    /// Input links keyed by the input id they write to.
    pub input_links: HashMap<String, InputLinkView>,
    /// Tween descriptors keyed by their id (which is the input id they drive).
    pub tweens: HashMap<String, TweenView>,
    /// Expression assignments in store order (`expressions.js` state shape).
    pub expressions: Vec<ExpressionView>,
    /// Live capture state for the Media panel.
    pub media: MediaSnapshot,
    /// Native MIDI input state.
    pub midi: MidiSnapshot,
    /// The opt-in JavaScript plugin runtime (the Plugins panel).
    ///
    /// `modv-app` rebuilds this from `PluginRuntime` (the runtime stays
    /// authoritative); it is refreshed at 1 Hz while the panel is visible and
    /// immediately after a plugin command.
    pub plugins: PluginView,
    /// Native NDI availability, output sender, discovery and receivers.
    pub ndi: NdiSnapshot,
    /// OSC listeners and the latest received address values.
    pub osc: OscSnapshot,
    /// Enumerated cpal input devices for the Audio panel.
    pub audio_devices: Vec<AudioDeviceView>,
    /// The active media project profile and every profile on disk.
    pub project: ProjectView,
    /// The live native capture selection (the Audio panel's active choice).
    pub audio_selection: AudioSelectionView,
}

/// Native NDI state for the NDI panel.
///
/// `modv-app` fills this from the engine's `NdiRuntime` read accessors
/// (`config`, `sender_status`, `sources`, `receivers`), the same state
/// `ndi/getState` returns to the legacy renderer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NdiSnapshot {
    /// Whether libndi loaded; every control is disabled when it did not.
    pub available: bool,
    /// The resolved library path, when one was found.
    pub library_path: Option<String>,
    /// The output sender configuration.
    pub output: NdiOutputView,
    /// The sender thread's status.
    pub sender: NdiSenderView,
    /// Whether a discovery run is in flight.
    pub discovering: bool,
    /// Whether discovery lists this machine's own sender.
    pub show_local_sources: bool,
    /// Sources found by the last completed discovery run.
    pub sources: Vec<NdiSourceView>,
    /// Created receivers, in creation order.
    pub receivers: Vec<NdiReceiverView>,
}

/// The NDI output sender's configuration (`ndi.outputEnabled`, …).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NdiOutputView {
    pub enabled: bool,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Send at the compositor size instead of `width`×`height`.
    pub follow_output_size: bool,
    /// Send at the engine frame rate instead of `target_fps`.
    pub follow_fps: bool,
    pub target_fps: f64,
}

/// The NDI sender thread's reported status.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NdiSenderView {
    pub running: bool,
    /// libndi is missing, so no sender can run.
    pub unavailable: bool,
    /// Why the sender stopped, when it did.
    pub error: Option<String>,
    pub frames_sent: u64,
}

/// One discovered NDI source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NdiSourceView {
    pub name: String,
    pub url_address: String,
}

/// One created NDI receiver and its live status.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NdiReceiverView {
    pub id: String,
    /// The requested source name.
    pub source_name: String,
    /// The requested source address (diagnostics).
    pub url_address: String,
    /// Whether the receiver thread should be running.
    pub enabled: bool,
    /// The lifecycle token: `idle`, `connecting`, `connected` or `error`.
    pub state: String,
    /// The connected source name, once connected.
    pub source: Option<String>,
    /// Why the receiver stopped, when it did.
    pub error: Option<String>,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
}

impl NdiSnapshot {
    /// The warning the panel shows when libndi could not be loaded.
    pub fn availability_warning(&self) -> Option<String> {
        if self.available {
            return None;
        }
        Some(match self.library_path.as_deref() {
            Some(path) => format!(
                "NDI is unavailable: libndi could not be loaded (found at {path}). \
                 Install the NDI SDK or set MODV_NDI_LIB, then restart the engine."
            ),
            None => "NDI is unavailable: libndi could not be loaded. \
                     Install the NDI SDK or set MODV_NDI_LIB, then restart the engine."
                .to_string(),
        })
    }

    /// The "NDI output stopped" notice, when the sender reported a failure.
    pub fn sender_warning(&self) -> Option<String> {
        self.sender
            .error
            .as_ref()
            .map(|error| format!("NDI output stopped: {error}"))
    }

    /// The discover button's label.
    pub fn discover_label(&self) -> &'static str {
        if self.discovering {
            "Discovering…"
        } else {
            "Discover Sources"
        }
    }

    /// Whether the output toggle may be flipped (libndi is loaded).
    pub fn can_enable_output(&self) -> bool {
        self.available
    }

    /// The effective send rate: the engine rate when following it, otherwise
    /// the configured target, clamped exactly like `NdiConfig::target_fps`.
    pub fn target_fps(&self, engine_fps: f64) -> f64 {
        let fps = if self.output.follow_fps {
            engine_fps
        } else {
            self.output.target_fps
        };
        if fps.is_finite() && fps > 0.0 {
            fps.clamp(1.0, 240.0)
        } else {
            60.0
        }
    }

    /// The sender's status line (never empty).
    pub fn sender_status_label(&self) -> String {
        if let Some(error) = &self.sender.error {
            return format!("Stopped: {error}");
        }
        if !self.available {
            return "NDI is unavailable".to_string();
        }
        if self.sender.running {
            return format!("Sending · {} frame(s)", self.sender.frames_sent);
        }
        if self.output.enabled {
            "Starting…".to_string()
        } else {
            "Not sending".to_string()
        }
    }

    /// The size frames are sent at (`followModVOutputSize` → the live output).
    pub fn output_size_label(&self, resolution: [u32; 2]) -> String {
        let (width, height) = if self.output.follow_output_size {
            (resolution[0].max(1), resolution[1].max(1))
        } else {
            (self.output.width, self.output.height)
        };
        format!("{width}×{height}")
    }

    /// The frame rate line (`60 fps · following modV`).
    pub fn output_fps_label(&self, engine_fps: f64) -> String {
        let fps = self.target_fps(engine_fps);
        if self.output.follow_fps {
            format!("{fps:.0} fps · following modV")
        } else {
            format!("{fps:.0} fps")
        }
    }
}

impl NdiReceiverView {
    /// The receiver's status (`connected 1920×1080`, `idle`, `error`).
    pub fn status_label(&self) -> String {
        let mut label = self.state.clone();
        if self.width > 0 && self.height > 0 {
            label.push_str(&format!(" {}×{}", self.width, self.height));
        }
        label
    }

    /// The enable/disable button's label.
    pub fn toggle_label(&self) -> &'static str {
        if self.enabled { "Disable" } else { "Enable" }
    }

    /// The row title: the requested source name, falling back to the live
    /// source or the id so a malformed record is still identifiable.
    pub fn title(&self) -> &str {
        if !self.source_name.trim().is_empty() {
            &self.source_name
        } else if let Some(source) = &self.source {
            source
        } else {
            &self.id
        }
    }
}

/// OSC listeners plus the latest values received under `osc.data`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OscSnapshot {
    /// Listeners in creation order (the default server first).
    pub servers: Vec<OscServerView>,
    /// One row per received leaf, sorted by path.
    pub addresses: Vec<OscAddressView>,
}

/// One bound OSC listener (`osc/addServer`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OscServerView {
    pub id: String,
    /// The listener label; the engine defaults it to `host:port`.
    pub name: String,
    /// The bound host (`0.0.0.0` for the default server).
    pub host: String,
    pub port: u16,
}

impl OscServerView {
    /// `0.0.0.0:3333`.
    pub fn address_label(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// The row title, falling back to the bound address when unnamed.
    pub fn title(&self) -> String {
        if self.name.trim().is_empty() {
            self.address_label()
        } else {
            self.name.clone()
        }
    }

    /// Parse the `osc/createServer` reply (`{ id, name, host, port }`).
    pub fn from_create_reply(value: &Value) -> Option<Self> {
        Some(Self {
            id: value.get("id")?.as_str()?.to_string(),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            host: value.get("host")?.as_str()?.to_string(),
            port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
        })
    }
}

/// One received OSC address leaf under `osc.data`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OscAddressView {
    /// The lodash path under `osc.data` (`modv.bass`, `layers.0.alpha`).
    pub path: String,
    /// The state path an OSC Input Link binds (`osc.data.modv.bass`).
    pub state_path: String,
    /// The latest message's arguments, formatted for display.
    pub values: Vec<String>,
}

impl OscAddressView {
    /// The received OSC address reconstructed from the path (`/modv/bass`).
    ///
    /// An address segment that itself contained a `/` cannot be recovered
    /// (the store only keeps the flattened path); this is a display label,
    /// while [`OscAddressView::state_path`] is what links bind.
    pub fn address(&self) -> String {
        format!("/{}", self.path.replace('.', "/"))
    }

    /// The latest values as one line (`0.5, 0.25`).
    pub fn value_label(&self) -> String {
        if self.values.is_empty() {
            "(no values)".to_string()
        } else {
            self.values.join(", ")
        }
    }
}

/// Most address rows the native OSC panel lists (the engine store can hold
/// thousands; the panel is a monitor, not a table view).
pub const MAX_OSC_ADDRESS_ROWS: usize = 512;

/// Deepest `osc.data` branch [`osc_addresses`] walks.
pub const MAX_OSC_ADDRESS_DEPTH: usize = 24;

/// Flatten the engine's `osc.data` tree into one row per received leaf.
///
/// The legacy store writes `set(state.data, msg[0].substring(1).replaceAll("/",
/// "."), msg.slice(1))`, so every message is an array of argument values. A
/// numeric path segment produces a JSON array branch; arrays whose elements
/// are all scalars are leaves. Rows are sorted by path so the panel is stable
/// between frames (the store's JSON map preserves arrival order).
pub fn osc_addresses(data: &Value) -> Vec<OscAddressView> {
    let mut rows = Vec::new();
    let mut path = Vec::new();
    walk_osc_data(data, &mut path, &mut rows);
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

fn walk_osc_data(node: &Value, path: &mut Vec<String>, rows: &mut Vec<OscAddressView>) {
    if rows.len() >= MAX_OSC_ADDRESS_ROWS || path.len() > MAX_OSC_ADDRESS_DEPTH {
        return;
    }
    match node {
        Value::Object(map) => {
            for (key, child) in map {
                path.push(key.clone());
                walk_osc_data(child, path, rows);
                path.pop();
            }
        }
        Value::Array(items) if items.iter().any(|item| item.is_object() || item.is_array()) => {
            for (index, child) in items.iter().enumerate() {
                path.push(index.to_string());
                walk_osc_data(child, path, rows);
                path.pop();
            }
        }
        Value::Array(items) => {
            if !path.is_empty() {
                rows.push(OscAddressView {
                    path: path.join("."),
                    state_path: format!("osc.data.{}", path.join(".")),
                    values: items.iter().map(osc_value_text).collect(),
                });
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            if !path.is_empty() {
                rows.push(OscAddressView {
                    path: path.join("."),
                    state_path: format!("osc.data.{}", path.join(".")),
                    values: vec![osc_value_text(node)],
                });
            }
        }
    }
}

/// One OSC value as a compact display string.
fn osc_value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// One enumerable audio input device (a cpal input from `modv-media`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioDeviceView {
    /// Stable cpal device id (`host:device`), the value
    /// [`AudioSelectionView::device`] stores.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Default input channel count.
    pub channels: u16,
    /// The rate capture will actually negotiate for this device
    /// (`InputDeviceInfo::capture_sample_rate`), never cpal's
    /// `default_input_config()` heuristic.
    pub capture_sample_rate: u32,
}

impl AudioDeviceView {
    /// The compact secondary line under a picker row (`2 ch · 48 kHz`).
    pub fn detail(&self) -> String {
        format!(
            "{} ch · {}",
            self.channels,
            sample_rate_label(self.capture_sample_rate)
        )
    }
}

/// The native shell's live audio capture selection.
///
/// `source` mirrors `AudioSourceSelection::as_str` (`pipewire` or `input`).
/// `input` with no `device` means the host default input device, exactly like
/// `--audio-source input` without a device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSelectionView {
    pub source: String,
    pub device: Option<String>,
    /// The rate the running feed negotiated (`AudioFeed::sample_rate()`),
    /// `None` while capture is silent (no feed, or a feed that stopped on an
    /// error). Preferred over a device's advertised capture rate wherever the
    /// banner names the rate analysis is running at.
    pub capture_sample_rate: Option<u32>,
    /// The last live-switch failure. The previous feed keeps running when this
    /// is set, so the panel can say so instead of implying silence.
    pub error: Option<String>,
}

impl Default for AudioSelectionView {
    fn default() -> Self {
        Self {
            source: "pipewire".to_string(),
            device: None,
            capture_sample_rate: None,
            error: None,
        }
    }
}

impl AudioSelectionView {
    pub fn is_pipewire(&self) -> bool {
        self.source == "pipewire"
    }

    pub fn is_default_input(&self) -> bool {
        self.source == "input" && self.device.is_none()
    }

    /// Whether `device` is the selected input (matching its id or name, like
    /// `--audio-device` accepts both).
    pub fn matches_device(&self, device: &AudioDeviceView) -> bool {
        let Some(selected) = self.device.as_deref() else {
            return false;
        };
        self.source == "input"
            && (selected.eq_ignore_ascii_case(&device.id)
                || selected.eq_ignore_ascii_case(&device.name))
    }

    /// The selected input device, when it is still present in the list.
    pub fn active_device<'a>(&self, devices: &'a [AudioDeviceView]) -> Option<&'a AudioDeviceView> {
        if self.source != "input" {
            return None;
        }
        devices.iter().find(|device| self.matches_device(device))
    }

    /// The friendly name the Audio panel's "in use" banner shows.
    ///
    /// Device rows resolve to their human-readable name; the system sources
    /// get an explicit label instead of exposing a cpal id.
    pub fn active_label(&self, devices: &[AudioDeviceView]) -> String {
        if let Some(device) = self.active_device(devices) {
            return device.name.clone();
        }
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => "System audio".to_string(),
            ("input", None) => "System default input".to_string(),
            ("input", Some(_)) => "Input device".to_string(),
            (source, _) => source.to_string(),
        }
    }

    /// The one-line explanation under the banner's friendly name.
    pub fn active_subtitle(&self, devices: &[AudioDeviceView]) -> String {
        if self.active_device(devices).is_some() {
            return "Microphone or audio interface".to_string();
        }
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => "Captures what your speakers play".to_string(),
            ("input", None) => "Follows the system's default device".to_string(),
            ("input", Some(_)) => "The selected device is not currently connected".to_string(),
            _ => String::new(),
        }
    }

    /// The rate analysis is actually running at: the live feed's negotiated
    /// rate when capture is running, otherwise the selected device's capture
    /// rate. `None` for the system sources while silent.
    pub fn capture_rate(&self, devices: &[AudioDeviceView]) -> Option<u32> {
        self.capture_sample_rate
            .filter(|rate| *rate > 0)
            .or_else(|| {
                self.active_device(devices)
                    .map(|device| device.capture_sample_rate)
                    .filter(|rate| *rate > 0)
            })
    }

    /// The banner's technical line: channel count, sample rate and the stable
    /// source id (the id stays out of the primary label).
    ///
    /// The rate is the live feed's negotiated rate when capture is running and
    /// the device's capture rate while silent — cpal's 44.1 kHz
    /// `default_input_config()` heuristic never reaches this line.
    pub fn active_detail(&self, devices: &[AudioDeviceView]) -> String {
        let rate = self.capture_rate(devices);
        let rate_note = rate
            .map(|rate| format!(" · {}", sample_rate_label(rate)))
            .unwrap_or_default();
        if let Some(device) = self.active_device(devices) {
            return format!("{} ch{rate_note} · {}", device.channels, device.id);
        }
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => format!("PipeWire monitor{rate_note} · pipewire"),
            ("input", None) => format!("System default{rate_note} · input"),
            ("input", Some(device)) => format!("input{rate_note} · {device}"),
            (source, _) => source.to_string(),
        }
    }

    /// A human-readable label for the active source (device names are
    /// resolved by the panel from [`EngineSnapshot::audio_devices`]).
    pub fn describe(&self) -> String {
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => "System audio (PipeWire monitor)".to_string(),
            ("input", Some(device)) => format!("input: {device}"),
            ("input", None) => "Default input (system default)".to_string(),
            (source, _) => source.to_string(),
        }
    }
}

/// `48000` → `48 kHz`, `44100` → `44.1 kHz`, `0` → `unknown rate`.
pub fn sample_rate_label(hz: u32) -> String {
    if hz == 0 {
        return "unknown rate".to_string();
    }
    if hz < 1_000 {
        return format!("{hz} Hz");
    }
    let khz = f64::from(hz) / 1_000.0;
    if khz.fract().abs() < f64::EPSILON {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    }
}

/// The tempo readout with the live capture rate appended (`BPM 128 · 48 kHz`).
///
/// The status bar, the Tempo window and the Info panel all use this formatter,
/// so the BPM shown next to a capture rate is always the rate analysis is
/// running at. A silent engine (`None`) keeps the bare `BPM 128`.
pub fn bpm_with_rate(bpm: f64, capture_rate: Option<u32>) -> String {
    match capture_rate.filter(|rate| *rate > 0) {
        Some(rate) => format!("BPM {bpm:.0} · {}", sample_rate_label(rate)),
        None => format!("BPM {bpm:.0}"),
    }
}

/// One uploaded preview texture (an egui texture owned by the shell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewTexture {
    pub id: egui::TextureId,
    pub width: u32,
    pub height: u32,
}

/// The live layer state of one performance row (the grid's row monitor).
///
/// The shell keeps a slow-cadence GPU snapshot of the row's live Group and
/// registers it with `egui-wgpu` as a native texture; the grid letterboxes
/// it into the row gutter, dimmed while performance mode is off. `texture`
/// is `None` from the moment the Group appears until the row's first
/// monitored refresh lands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowMonitorView {
    /// The live Group rendering this row.
    pub group_id: String,
    /// The uploaded monitor snapshot, once the first refresh lands.
    pub texture: Option<PreviewTexture>,
}

/// The key used by [`EngineSnapshot::scene_previews`].
pub fn scene_preview_key(scene_id: &str, row: usize) -> String {
    format!("scene:{scene_id}:{row}")
}

/// The key used by [`EngineSnapshot::gallery_previews`].
pub fn gallery_preview_key(module_name: &str) -> String {
    format!("module:{module_name}")
}

/// Undo premultiplication on a tightly packed RGBA8 pixel buffer.
///
/// The compositor's read-backs (`Compositor::read_main_rgba8` and the
/// isolated Gallery/Scene preview renderers) produce **premultiplied**
/// RGBA — every RGB sample already scaled by its alpha. egui CPU textures
/// are created from straight alpha (`ColorImage::from_rgba_unmultiplied`
/// premultiplies on upload), so the preview upload path converts first:
/// shipping premultiplied bytes as straight alpha (or flattening alpha to
/// opaque, the old behaviour) renders transparent halos as dark fringes.
///
/// Opaque pixels are unchanged and a zero-alpha pixel stays zero (the
/// premultiplied contract already makes it `(0,0,0,0)`). Conversion is
/// round-to-nearest and clamped, so a malformed sample above its alpha
/// cannot wrap.
pub fn un_premultiply_rgba(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha == 0 || alpha == 255 {
            continue;
        }
        let alpha = u32::from(alpha);
        for sample in pixel.iter_mut().take(3) {
            let value = u32::from(*sample);
            *sample = ((value * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

/// The native MediaHub's camera and running-source state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSnapshot {
    pub available: bool,
    pub error: Option<String>,
    pub cameras: Vec<CameraView>,
    pub sources: Vec<MediaSourceView>,
    /// The XDG-desktop-portal screen capture (`MediaAction::ShareScreen`).
    pub screen: ScreenCaptureView,
}

/// The native shell's portal screen capture.
///
/// The shell owns the portal session (the PipeWire node dies with it), so the
/// panel only renders this read model and queues [`MediaAction`]s. At most one
/// request and one live capture exist (the source id is stable), which is what
/// the two Share buttons and the running hint reflect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenCaptureView {
    /// Why the host cannot capture screens (no `pipewiresrc`/PipeWire socket).
    pub unavailable: Option<String>,
    /// The picker whose portal dialog is waiting for the user.
    pub pending: Option<ScreenShareKindView>,
    /// The live portal capture.
    pub running: Option<ScreenSourceView>,
    /// The last request's failure or cancellation, shown until the next one.
    pub error: Option<String>,
}

impl ScreenCaptureView {
    /// Whether the Share buttons can start a request.
    ///
    /// A request needs a supported host and the one screen slot free: the
    /// portal dialog can only be answered once, and the Webcam module indexes
    /// the single `screen:portal` source like another camera.
    pub fn can_request(&self) -> bool {
        self.unavailable.is_none() && self.pending.is_none() && self.running.is_none()
    }
}

/// Which portal picker a request opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenShareKindView {
    /// `Share monitor…` (`SourceSelection::Monitor`).
    Monitor,
    /// `Share window…` (`SourceSelection::Window`).
    Window,
}

impl ScreenShareKindView {
    /// The logging/`sourceType` spelling (`monitor`/`window`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Window => "window",
        }
    }

    /// The noun the panel's pending/running lines use.
    pub fn label(self) -> &'static str {
        self.as_str()
    }
}

/// The live portal screen source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenSourceView {
    /// The MediaHub source id (`screen:portal`).
    pub source_id: String,
    /// The PipeWire node id the portal handed out.
    pub node_id: u32,
    /// The monitor/window picker that produced it.
    pub kind: ScreenShareKindView,
}

/// One enumerable camera.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CameraView {
    pub id: String,
    pub name: String,
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u32,
    /// The modes the device advertises (deduplicated, sorted). Empty when the
    /// provider reports none.
    pub modes: Vec<CameraModeView>,
    /// The resolved selection: the saved mode when it is still advertised,
    /// else the nearest advertised mode, else 1280×720@30.
    pub selected: CameraModeView,
}

/// One advertised camera capture mode (`1920×1080` at `60 fps`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CameraModeView {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl CameraModeView {
    /// Pixel count, used for resolution ordering.
    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// The Resolution picker's label (`1920×1080`).
    pub fn resolution_label(&self) -> String {
        format!("{}×{}", self.width, self.height)
    }

    /// The Frame rate picker's label (`60 fps`).
    pub fn fps_label(&self) -> String {
        format!("{} fps", self.fps)
    }
}

/// The unique resolutions in `modes`, largest first.
pub fn unique_resolutions(modes: &[CameraModeView]) -> Vec<(u32, u32)> {
    let mut resolutions: Vec<(u32, u32)> = Vec::new();

    for mode in modes {
        if mode.width == 0 || mode.height == 0 {
            continue;
        }
        let resolution = (mode.width, mode.height);
        if !resolutions.contains(&resolution) {
            resolutions.push(resolution);
        }
    }
    resolutions.sort_by_key(|&(width, height)| {
        std::cmp::Reverse((u64::from(width) * u64::from(height), width, height))
    });

    resolutions
}

/// The advertised rates for one resolution, fastest first.
///
/// `fps == 0` means the device did not report a rate, so it is not a choice.
pub fn fps_options(modes: &[CameraModeView], width: u32, height: u32) -> Vec<u32> {
    let mut rates = modes
        .iter()
        .filter(|mode| mode.width == width && mode.height == height && mode.fps > 0)
        .map(|mode| mode.fps)
        .collect::<Vec<_>>();

    rates.sort_unstable_by(|a, b| b.cmp(a));
    rates.dedup();
    rates
}

/// The camera's maximum mode: highest pixel count, then highest fps.
pub fn default_camera_mode(modes: &[CameraModeView]) -> Option<CameraModeView> {
    modes
        .iter()
        .filter(|mode| mode.width > 0 && mode.height > 0)
        .max_by_key(|mode| (mode.pixels(), mode.fps))
        .copied()
}

/// The mode for `width`×`height`: `preferred_fps` when advertised, else the
/// fastest rate at that resolution. `None` when the resolution is not offered.
pub fn mode_for_resolution(
    modes: &[CameraModeView],
    width: u32,
    height: u32,
    preferred_fps: u32,
) -> Option<CameraModeView> {
    let candidates = modes
        .iter()
        .filter(|mode| mode.width == width && mode.height == height)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }

    if preferred_fps > 0
        && let Some(exact) = candidates
            .iter()
            .find(|mode| mode.fps == preferred_fps)
            .copied()
    {
        return Some(*exact);
    }
    if let Some(fastest) = candidates
        .iter()
        .filter(|mode| mode.fps > 0)
        .max_by_key(|mode| mode.fps)
        .copied()
    {
        return Some(*fastest);
    }

    Some(CameraModeView {
        width,
        height,
        fps: preferred_fps.max(1),
    })
}

/// One running media source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaSourceView {
    pub source_id: String,
    pub kind: String,
    pub index: usize,
    pub state: String,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
    pub dropped: u64,
    pub last_error: Option<String>,
}

/// Native MIDI availability and devices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiSnapshot {
    pub available: bool,
    pub error: Option<String>,
    pub devices: Vec<MidiDeviceView>,
}

/// One MIDI input and its engine flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MidiDeviceView {
    /// The engine state key (`${id}-${name}-${manufacturer}`).
    pub key: String,
    pub name: String,
    pub connected: bool,
    pub listen_for_input: bool,
    pub listen_for_clock: bool,
    pub cc_latch: bool,
    pub note_on_latch: bool,
}

/// The opt-in JavaScript plugin panel read model.
///
/// Built by `modv-app` from the authoritative `PluginRuntime` and never
/// written back: the UI renders it and emits commands, the runtime applies
/// them. `plugins` is the discovery list even while the master gate is off, so
/// the panel can show what *would* run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginView {
    /// The master switch is on (only possible with consent).
    pub enabled: bool,
    /// The user accepted the sandbox consent notice.
    pub consented: bool,
    /// How many plugins currently hold a Boa context.
    pub loaded_count: usize,
    /// Discovered scripts, sorted by absolute path.
    pub plugins: Vec<PluginInfoView>,
    /// Deduplicated `plugin: …` messages retained by the runtime.
    pub logs: Vec<String>,
    /// The largest capture a running plugin requested, `(width, height)`.
    pub capture: Option<[u32; 2]>,
}

impl PluginView {
    /// Whether the host may run hooks (`enabled && consented`).
    pub fn is_running(&self) -> bool {
        self.enabled && self.consented
    }

    /// The explanation shown above a greyed plugin list, or `None` when the
    /// host is running.
    pub fn gate_reason(&self) -> Option<&'static str> {
        if !self.consented {
            Some(
                "JavaScript plugins need your consent. Review the warning above and enable \
                 them; the scripts below are discovered but never evaluated.",
            )
        } else if !self.enabled {
            Some(
                "JavaScript plugins are turned off. The scripts below are discovered but \
                 not running; enable them to load their controls.",
            )
        } else {
            None
        }
    }

    /// The loaded/total line under the master switch.
    pub fn summary(&self) -> String {
        if !self.is_running() {
            return "Not running".to_string();
        }
        let total = self.plugins.len();
        let loaded = self.loaded_count;
        if total == 1 {
            format!("{loaded} of 1 plugin loaded")
        } else {
            format!("{loaded} of {total} plugins loaded")
        }
    }

    /// The capture read-back line (`capture 160×90`), when one is requested.
    pub fn capture_label(&self) -> Option<String> {
        self.capture.map(capture_size_label)
    }
}

/// The lifecycle state a plugin row's status dot shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PluginStatus {
    /// A Boa context is loaded and the plugin runs its hooks.
    Loaded,
    /// Discovered but not running (master gate or per-plugin switch off).
    #[default]
    Disabled,
    /// Loading/initialising failed, or a hook threw.
    Error,
}

impl PluginStatus {
    /// The visible status label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Loaded => "Loaded",
            Self::Disabled => "Disabled",
            Self::Error => "Error",
        }
    }
}

/// One discovered plugin script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginInfoView {
    /// Absolute script path (the settings key and command payload).
    pub path: String,
    /// Descriptor name (empty until the source loads).
    pub name: String,
    pub author: String,
    pub version: String,
    /// The per-plugin user switch.
    pub enabled: bool,
    /// The host slot's active flag (the switch after the host saw it).
    pub active: bool,
    /// A Boa context is loaded and `init` succeeded.
    pub loaded: bool,
    /// Loading/initialising failed.
    pub failed: bool,
    /// The first recorded error for this plugin, if any.
    pub error: Option<String>,
    /// The source file's size in bytes.
    pub source_bytes: usize,
    /// The plugin's requested capture size, `(width, height)`.
    pub capture: Option<[u32; 2]>,
    /// Generated controls, in descriptor order.
    pub props: Vec<PluginPropView>,
}

impl PluginInfoView {
    /// The row title: the descriptor name, else the script's file stem.
    pub fn title(&self) -> &str {
        if !self.name.trim().is_empty() {
            return &self.name;
        }
        let file = self.file_name();
        file.strip_suffix(".js")
            .filter(|stem| !stem.is_empty())
            .unwrap_or(file)
    }

    /// The script's file name (`lumen.js`).
    pub fn file_name(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path)
    }

    /// `author · v1.2.3`, omitting whichever half is missing.
    pub fn credit(&self) -> String {
        let mut parts = Vec::new();
        if !self.author.trim().is_empty() {
            parts.push(self.author.trim().to_string());
        }
        if !self.version.trim().is_empty() {
            parts.push(format!("v{}", self.version.trim()));
        }
        parts.join(" · ")
    }

    /// The status dot's state. An error outranks a loaded context because the
    /// plugin is no longer running its hooks reliably.
    pub fn status(&self) -> PluginStatus {
        if self.failed || self.error.is_some() {
            PluginStatus::Error
        } else if self.loaded {
            PluginStatus::Loaded
        } else {
            PluginStatus::Disabled
        }
    }

    /// The capture read-back line, when this plugin asked for one.
    pub fn capture_label(&self) -> Option<String> {
        self.capture.map(capture_size_label)
    }
}

/// `[160, 90]` → `capture 160×90`.
fn capture_size_label([width, height]: [u32; 2]) -> String {
    format!("capture {width}×{height}")
}

/// The value type of a plugin prop (`PluginPropKind` in `modv-engine`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PluginPropKindView {
    /// A whole number.
    #[default]
    Int,
    /// A floating-point number.
    Float,
    /// A boolean toggle.
    Bool,
    /// Free text.
    Text,
    /// An RGBA colour whose channels are `0..=1`.
    Color,
    /// One of the descriptor's options (the legacy `enum` alias).
    Select,
}

impl PluginPropKindView {
    /// Parse the descriptor's `type` token. Unknown tokens are not props the
    /// panel can generate a control for.
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            "text" => Some(Self::Text),
            "color" => Some(Self::Color),
            "select" | "enum" => Some(Self::Select),
            _ => None,
        }
    }

    /// The wire token ([`Self::parse`]'s inverse for the canonical names).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Text => "text",
            Self::Color => "color",
            Self::Select => "select",
        }
    }

    /// Whether the control is a numeric slider.
    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::Float)
    }
}

/// One option of a `select` prop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginPropOptionView {
    /// Human-readable label for the ComboBox.
    pub label: String,
    /// Scalar value written back through `PluginHost::set_prop`.
    pub value: Value,
}

/// One generated control for a plugin prop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginPropView {
    /// Prop key inside the plugin's `props` object.
    pub name: String,
    pub kind: PluginPropKindView,
    /// The current value (the runtime's authoritative copy).
    pub value: Value,
    /// The declared default (the slider's reset target).
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub options: Vec<PluginPropOptionView>,
}

impl PluginPropView {
    /// Parse one serialised `PluginProp` from `PluginRuntime::plugins()`.
    ///
    /// Unknown types and malformed options are skipped by the caller, so a
    /// future descriptor cannot break the whole panel.
    pub fn from_schema(name: &str, schema: &Value) -> Option<Self> {
        let kind = PluginPropKindView::parse(schema.get("type")?.as_str()?)?;
        let default = schema.get("default").cloned().unwrap_or(Value::Null);
        let value = schema
            .get("value")
            .cloned()
            .unwrap_or_else(|| default.clone());
        let number = |key: &str| schema.get(key).and_then(Value::as_f64);
        let options = schema
            .get("options")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(PluginPropOptionView {
                            label: item.get("label")?.as_str()?.to_string(),
                            value: item.get("value").cloned()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            name: name.to_string(),
            kind,
            value,
            default,
            min: number("min"),
            max: number("max"),
            step: number("step"),
            options,
        })
    }

    /// The slider's `(min, max, step)`, widened to contain the current value
    /// and the default when the descriptor declares no bounds.
    pub fn bounds(&self) -> (f64, f64, f64) {
        let value = self.value.as_f64().unwrap_or(0.0);
        let default = self.default.as_f64().unwrap_or(value);
        let floor = value.min(default).min(0.0);
        let ceiling = value.max(default).max(1.0);
        let min = self.min.filter(|bound| bound.is_finite()).unwrap_or(floor);
        let mut max = self
            .max
            .filter(|bound| bound.is_finite())
            .unwrap_or(ceiling);
        if !max.is_finite() || max <= min {
            max = min + 1.0;
        }
        let step = self
            .step
            .filter(|step| step.is_finite() && *step > 0.0)
            .unwrap_or(if self.kind == PluginPropKindView::Int {
                1.0
            } else {
                0.001
            });
        (min, max, step)
    }

    /// The command payload for a numeric edit: ints round, floats stay floats.
    pub fn number_value(&self, value: f64) -> Value {
        if self.kind == PluginPropKindView::Int {
            Value::from(value.round() as i64)
        } else {
            Value::from(value)
        }
    }

    /// The index of the option matching the current value.
    pub fn selected_index(&self) -> Option<usize> {
        self.options
            .iter()
            .position(|option| option.value == self.value)
    }

    /// The current option's label, when the value matches one.
    pub fn selected_label(&self) -> Option<&str> {
        self.selected_index()
            .map(|index| self.options[index].label.as_str())
    }
}

/// Serialise a colour prop edit as the legacy `{r,g,b,a}` ratio object.
///
/// Channels clamp to `0..=1`; a non-finite channel becomes `0` so the JSON
/// payload never contains `null`.
pub fn plugin_color_value(rgba: [f32; 4]) -> Value {
    let channel = |value: f32| {
        if value.is_finite() {
            // The control is f32; round to a micro-ratio so the persisted JSON
            // reads `0.1` instead of `0.10000000149011612`.
            let ratio = f64::from(value.clamp(0.0, 1.0));
            (ratio * 1_000_000.0).round() / 1_000_000.0
        } else {
            0.0
        }
    };
    serde_json::json!({
        "r": channel(rgba[0]),
        "g": channel(rgba[1]),
        "b": channel(rgba[2]),
        "a": channel(rgba[3]),
    })
}

/// A MediaHub operation requested by the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaAction {
    /// Start a camera (or restart it with a new mode) at `width`×`height`@`fps`.
    /// The shell validates the mode against the advertised caps and remembers
    /// it; all-zero fields let the shell pick the saved/default mode.
    StartCamera {
        id: String,
        width: u32,
        height: u32,
        fps: u32,
    },
    /// Move a running camera one slot earlier (`-1`) or later (`1`) in the
    /// Webcam index; the shell persists the new order.
    MoveCamera {
        id: String,
        delta: i32,
    },
    StartTestPattern,
    Stop {
        source_id: String,
    },
    /// Ask the desktop portal for a screen and start it as `screen:portal`.
    ///
    /// `window` picks the portal's window picker; `false` asks for a monitor.
    /// The portal's own dialog chooses which one, so there is nothing to
    /// enumerate here. The shell runs the blocking request on a worker thread,
    /// keeps the portal session alive for the whole capture and reports the
    /// outcome through [`MediaSnapshot::screen`].
    ShareScreen {
        window: bool,
    },
    /// Stop waiting for the open portal dialog; a late answer is discarded
    /// (and closes the session it opened).
    CancelScreenShare,
}

impl MediaSnapshot {
    /// The running source for `camera_id` (`camera:<id>`), when it is running.
    pub fn running_camera(&self, camera_id: &str) -> Option<&MediaSourceView> {
        let source_id = format!("camera:{camera_id}");

        self.sources
            .iter()
            .find(|source| source.source_id == source_id)
    }

    /// Running camera-like sources in Webcam-index order.
    pub fn running_cameras(&self) -> Vec<&MediaSourceView> {
        let mut cameras = self
            .sources
            .iter()
            .filter(|source| matches!(source.kind.as_str(), "camera" | "screen" | "test-pattern"))
            .collect::<Vec<_>>();

        cameras.sort_by_key(|source| source.index);
        cameras
    }

    /// Cameras in display order: running cameras first, ordered by the Webcam
    /// module's 1-based index, then the unselected ones by name (the legacy
    /// `orderEngineCameraInputs`: checked cameras lead in engine order).
    pub fn ordered_cameras(&self) -> Vec<&CameraView> {
        let indexes: std::collections::HashMap<&str, usize> = self
            .sources
            .iter()
            .filter_map(|source| {
                source
                    .source_id
                    .strip_prefix("camera:")
                    .map(|id| (id, source.index))
            })
            .collect();

        let mut running: Vec<(&CameraView, usize)> = self
            .cameras
            .iter()
            .filter_map(|camera| {
                indexes
                    .get(camera.id.as_str())
                    .map(|index| (camera, *index))
            })
            .collect();
        running.sort_by(|(a, a_index), (b, b_index)| {
            a_index.cmp(b_index).then_with(|| a.name.cmp(&b.name))
        });

        let mut rest: Vec<&CameraView> = self
            .cameras
            .iter()
            .filter(|camera| !indexes.contains_key(camera.id.as_str()))
            .collect();
        rest.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });

        running
            .into_iter()
            .map(|(camera, _)| camera)
            .chain(rest)
            .collect()
    }
}

/// One engine Input Link (audio getter, state path, MIDI or OSC).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputLinkView {
    pub input_id: String,
    pub kind: String,
    pub location: Option<String>,
    pub args: Option<Value>,
    pub source: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub matcher: Option<Value>,
}

/// One expression assignment (`expressions.js`: `{ id, inputId, expression }`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExpressionView {
    pub id: String,
    pub input_id: String,
    pub expression: String,
    pub evaluate_every_frame: bool,
}

/// The blend/composite operations `ActiveModule.vue` offers
/// (`util/composite-operations.js`).
pub const COMPOSITE_OPERATIONS: &[&str] = &[
    "normal",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color-dodge",
    "color-burn",
    "hard-light",
    "soft-light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
    "clear",
    "copy",
    "destination",
    "source-over",
    "destination-over",
    "source-in",
    "destination-in",
    "source-out",
    "destination-out",
    "source-atop",
    "destination-atop",
    "xor",
    "lighter",
];

/// One live texture source (`group:<id>`, `isf:<id>`, `camera:<id>`, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextureSourceView {
    pub id: String,
    pub label: String,
    /// `group`, `isf`, `two-d`, `point10`, `camera`, `video` or `screen`.
    pub kind: String,
}

/// A hover-preview request emitted by the UI and answered by the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewRequest {
    Module {
        module_name: String,
        /// Composite the preview over the live output (hover previews).
        with_output: bool,
    },
    Scene {
        scene_id: String,
        row_index: usize,
    },
}

impl EngineSnapshot {
    /// The modules belonging to `group_id`, in group order.
    pub fn modules_in_group(&self, group_id: &str) -> Vec<&ModuleView> {
        let Some(group) = self.groups.iter().find(|group| group.id == group_id) else {
            return Vec::new();
        };
        group
            .module_ids
            .iter()
            .filter_map(|id| self.modules.iter().find(|module| &module.id == id))
            .collect()
    }

    /// The module with `module_id`.
    pub fn module(&self, module_id: &str) -> Option<&ModuleView> {
        self.modules.iter().find(|module| module.id == module_id)
    }

    /// The Scene with `scene_id`.
    pub fn scene(&self, scene_id: &str) -> Option<&SceneView> {
        self.scenes.iter().find(|scene| scene.id == scene_id)
    }

    /// The module definition with `name`.
    pub fn registered_module(&self, name: &str) -> Option<&ModuleMetaView> {
        self.registered.iter().find(|module| module.name == name)
    }

    /// The expression assignment for `input_id`, when one exists
    /// (`expressions/getByInputId`).
    pub fn expression_for_input(&self, input_id: &str) -> Option<&ExpressionView> {
        self.expressions
            .iter()
            .find(|assignment| assignment.input_id == input_id)
    }

    /// The tween descriptor linked to `input_id`, when one exists.
    pub fn tween_for_input(&self, input_id: &str) -> Option<&TweenView> {
        self.tweens.get(input_id)
    }
}

/// One Group (a render-graph row).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub hidden: bool,
    pub alpha: f32,
    /// Engine `alphaInputId`; a link row is drawn when present.
    pub alpha_input_id: Option<String>,
    pub composite_operation: String,
    /// Engine `compositeOperationInputId`; a link row is drawn when present.
    pub composite_operation_input_id: Option<String>,
    pub inherit: bool,
    pub clearing: bool,
    pub pipeline: bool,
    /// Module ids in group draw order.
    pub module_ids: Vec<String>,
}

impl GroupView {
    pub fn module_count(&self) -> usize {
        self.module_ids.len()
    }
}

/// One active Module instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModuleView {
    pub id: String,
    /// Registry key, e.g. `Bands`.
    pub module_name: String,
    /// Owning Group id (`""` when ungrouped).
    pub group_id: String,
    pub enabled: bool,
    pub alpha: f32,
    /// Engine `meta.alphaInputId`; a link row is drawn when present.
    pub alpha_input_id: Option<String>,
    /// Engine `meta.compositeOperation` (Inspector Blend control).
    pub composite_operation: String,
    /// Engine `meta.compositeOperationInputId`; a link row is drawn when present.
    pub composite_operation_input_id: Option<String>,
    pub module_type: String,
    pub props: Vec<PropView>,
}

/// A single editable property of an active Module.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropView {
    pub name: String,
    /// Engine prop type token: `float`, `int`, `bool`, `text`, `enum`, `event`,
    /// `color`, `vec2`/`vec3`/`vec4`, `texture`, `palette`, `tween`, `font`.
    pub prop_type: String,
    pub value: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    /// Registered default value (the Inspector's reset target).
    pub default: Option<Value>,
    /// The prop's input id (`$props[prop].id`), when one exists.
    pub input_id: Option<String>,
    /// The prop's component sub-inputs (`${parent}-${key}`, the JS Vec/Color
    /// sliders' focused inputs), keyed `r/g/b/a` or `0..N`, when the engine
    /// minted them.
    pub components: Vec<PropComponentView>,
    /// The tween descriptor linked to this prop, when one exists.
    pub tween: Option<TweenView>,
    /// Enum options, when the prop declares them.
    pub options: Vec<String>,
}

/// One component sub-input of a prop (a Vec lane or Color channel).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PropComponentView {
    /// The component key: `r`/`g`/`b`/`a` for colors, `0`..`N` for vecs.
    pub key: String,
    /// Sub-input id (`${parent input id}-${key}`).
    pub input_id: String,
}

/// A tween descriptor read from the engine state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TweenView {
    pub id: String,
    /// Keyframe rows: channel `i` is the sequence `data[row][i]`.
    pub data: Vec<Vec<f64>>,
    /// Explicit keyframe progress (the native `keyframeTimes` extension), when
    /// the descriptor carries a valid list.
    pub keyframe_times: Option<Vec<f64>>,
    pub duration: f64,
    pub direction: String,
    pub easing: String,
    pub looping: bool,
    pub use_bpm: bool,
    pub bpm_division: f64,
    /// Treat `duration` as the whole cycle instead of per-row time.
    pub duration_as_total_time: bool,
    pub steps: f64,
    pub frames: u64,
    /// One animation leg in seconds at the live tempo, from the engine's
    /// `TweenDescriptor::effective_duration` (the axis must match playback).
    pub cycle_seconds: f64,
}

/// One saved Scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Per-Scene timing override as `(min, max)` in the performance timing
    /// unit (`performance.timing_unit`), when the Scene sets one.
    pub timing: Option<(f64, f64)>,
    /// Cells by performance row. A `null` Scene row is represented by an
    /// empty cell (`module_names` empty).
    pub cells: Vec<SceneCellView>,
}

impl SceneView {
    /// The cell at `row`, if the Scene defines one.
    pub fn cell(&self, row: usize) -> Option<&SceneCellView> {
        self.cells.get(row)
    }
}

/// One Scene-grid cell (a Group snapshot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneCellView {
    pub row: usize,
    pub module_names: Vec<String>,
    /// Disabled in performance mode only; a manual Scene/cell apply still
    /// renders it. Persisted in `scenes.json` as `performance.disabledCells`.
    pub disabled: bool,
}

impl SceneCellView {
    pub fn is_empty(&self) -> bool {
        self.module_names.is_empty()
    }

    pub fn label(&self) -> String {
        match self.module_names.as_slice() {
            [] => "Empty".to_string(),
            [one] => one.clone(),
            [first, rest @ ..] => format!("{first} +{}", rest.len()),
        }
    }
}

/// One registered module definition (Gallery metadata).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModuleMetaView {
    pub name: String,
    pub module_type: String,
    pub category: String,
    pub author: String,
    pub version: String,
    pub description: String,
}

impl ModuleMetaView {
    /// The Gallery section this module belongs to (`effects`, `2d-scenes`,
    /// `3d-scenes`), following `Gallery.vue`'s category fallback.
    pub fn gallery_category(&self) -> &'static str {
        match self.category.as_str() {
            "effects" => "effects",
            "2d-scenes" => "2d-scenes",
            "3d-scenes" => "3d-scenes",
            _ => match self.module_type.as_str() {
                "2d" => "2d-scenes",
                "3d" => "3d-scenes",
                _ => "effects",
            },
        }
    }
}

/// The default Scene order (`Scenes.vue`'s `performance.order || "in-order"`).
pub const PERFORMANCE_ORDER_IN_ORDER: &str = "in-order";
/// Random Scene order (`Scenes.vue`'s "Random" option).
pub const PERFORMANCE_ORDER_RANDOM: &str = "random";

/// The Scene-order select's options: engine value plus visible label.
pub const PERFORMANCE_ORDERS: [(&str, &str); 2] = [
    (PERFORMANCE_ORDER_IN_ORDER, "In order"),
    (PERFORMANCE_ORDER_RANDOM, "Random"),
];

/// Normalise a Scene-order string to the behaviour the engine applies.
///
/// The engine's pick sites treat anything other than `"random"` as in order
/// (`crates/modv-app/src/performance.rs`), so the UI shows what will
/// actually happen rather than an unknown saved value.
pub fn normalize_performance_order(order: &str) -> &'static str {
    if order == PERFORMANCE_ORDER_RANDOM {
        PERFORMANCE_ORDER_RANDOM
    } else {
        PERFORMANCE_ORDER_IN_ORDER
    }
}

/// The visible label for a Scene-order value.
pub fn performance_order_label(order: &str) -> &'static str {
    if normalize_performance_order(order) == PERFORMANCE_ORDER_RANDOM {
        "Random"
    } else {
        "In order"
    }
}

/// Unit the performance `min_value` / `max_value` and every Scene override
/// are expressed in (`performance.timingUnit`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimingUnit {
    /// Plain seconds (the legacy behaviour and the default).
    #[default]
    Seconds,
    /// Musical bars, 4 beats each.
    Bars,
}

impl TimingUnit {
    /// Parse the persisted `timingUnit` token; unknown values are seconds.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "bars" | "bar" => Self::Bars,
            _ => Self::Seconds,
        }
    }

    /// The token written to `timingUnit` by `scenes/setTimingUnit`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Bars => "bars",
        }
    }

    /// The selector label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Seconds => "Seconds",
            Self::Bars => "Bars",
        }
    }

    /// The input suffix (`egui::DragValue`).
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Seconds => " s",
            Self::Bars => " bar",
        }
    }

    pub fn is_bars(self) -> bool {
        self == Self::Bars
    }
}

/// Beats in a bar (the engine's `BEATS_PER_BAR`; kept local so this crate
/// stays engine-agnostic).
pub const BEATS_PER_BAR: f64 = 4.0;

/// Tempo used when the engine has no usable BPM (`beats.bpm || 120`).
pub const FALLBACK_BPM: f64 = 120.0;

/// A tempo usable by the bar conversion.
pub fn effective_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        FALLBACK_BPM
    }
}

/// Seconds per 4/4 bar at `bpm`.
pub fn bar_seconds(bpm: f64) -> f64 {
    BEATS_PER_BAR * 60.0 / effective_bpm(bpm)
}

/// Convert a value in `unit` to seconds.
pub fn timing_value_seconds(value: f64, unit: TimingUnit, bpm: f64) -> f64 {
    match unit {
        TimingUnit::Seconds => value,
        TimingUnit::Bars => value * bar_seconds(bpm),
    }
}

/// Convert seconds to `unit`.
pub fn timing_seconds_value(seconds: f64, unit: TimingUnit, bpm: f64) -> f64 {
    match unit {
        TimingUnit::Seconds => seconds,
        TimingUnit::Bars => seconds / bar_seconds(bpm),
    }
}

/// The engine's default global timing in `unit` (8 s / 24 s at 120 BPM).
pub fn default_timing_values(unit: TimingUnit) -> (f64, f64) {
    match unit {
        TimingUnit::Seconds => (8.0, 24.0),
        TimingUnit::Bars => (4.0, 12.0),
    }
}

/// A live duration hint for a min/max pair, e.g.
/// `"4.00–12.00 bars ≈ 8.0–24.0 s at 120 BPM"` or `"8.0–24.0 s"`.
pub fn timing_hint(min: f64, max: f64, unit: TimingUnit, bpm: f64) -> String {
    match unit {
        TimingUnit::Seconds => format!("{min:.1}–{max:.1} s"),
        TimingUnit::Bars => {
            let bpm = effective_bpm(bpm);
            let min_seconds = timing_value_seconds(min, unit, bpm);
            let max_seconds = timing_value_seconds(max, unit, bpm);
            format!("{min:.2}–{max:.2} bars ≈ {min_seconds:.1}–{max_seconds:.1} s at {bpm:.0} BPM")
        }
    }
}

/// Performance-mode configuration plus the runtime Scene id.
#[derive(Clone, Debug, PartialEq)]
pub struct PerformanceView {
    pub enabled: bool,
    pub current_scene_id: Option<String>,
    pub row_names: Vec<String>,
    /// Unit of `min_value` / `max_value` and every Scene override.
    pub timing_unit: TimingUnit,
    /// Shortest dwell, in `timing_unit`.
    pub min_value: f64,
    /// Longest dwell, in `timing_unit`.
    pub max_value: f64,
    /// Crossfade duration, always seconds.
    pub transition_seconds: f64,
    pub allowed_transitions: Vec<String>,
    /// Scene order used for the next-Scene pick (`"in-order"` / `"random"`).
    pub order: String,
}

impl Default for PerformanceView {
    fn default() -> Self {
        Self {
            enabled: false,
            current_scene_id: None,
            row_names: Vec::new(),
            timing_unit: TimingUnit::Seconds,
            min_value: 0.0,
            max_value: 0.0,
            transition_seconds: 0.0,
            allowed_transitions: Vec::new(),
            order: PERFORMANCE_ORDER_IN_ORDER.to_string(),
        }
    }
}

impl PerformanceView {
    /// The normalised Scene order the engine will use.
    pub fn scene_order(&self) -> &'static str {
        normalize_performance_order(&self.order)
    }

    /// The ComboBox's selected label (`In order` / `Random`).
    pub fn scene_order_label(&self) -> &'static str {
        performance_order_label(&self.order)
    }
}

/// Audio analysis values shown by the meters and BPM readout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioView {
    pub bpm: f64,
    pub kick: bool,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    /// The live capture feed's negotiated rate in Hz (the rate the analyser,
    /// BPM detector and bands run at), `None` when capture is silent.
    pub sample_rate: Option<u32>,
}

/// Status-bar health information.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineHealth {
    pub adapter: String,
    pub backend: String,
    pub last_error: Option<String>,
    pub frames_presented: u64,
    pub output_open: bool,
    pub audio_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `round(a * alpha / 255)`, the premultiplication the compositor applies.
    fn premultiply(straight: [u8; 4]) -> [u8; 4] {
        let alpha = u32::from(straight[3]);
        let mut out = straight;
        for channel in 0..3 {
            let value = u32::from(straight[channel]);
            out[channel] = ((value * alpha + 127) / 255).min(255) as u8;
        }
        out
    }

    #[test]
    fn un_premultiply_restores_rgb_scaled_by_alpha() {
        // Straight (200, 100, 50) at alpha 51 premultiplies exactly to
        // (40, 20, 10): 200 * 51 / 255 == 40, no rounding on either leg.
        let mut pixels = [40, 20, 10, 51];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [200, 100, 50, 51]);
    }

    #[test]
    fn un_premultiply_keeps_opaque_pixels_unchanged() {
        let mut pixels = [200, 100, 50, 255];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [200, 100, 50, 255], "alpha 255 needs no scaling");
    }

    #[test]
    fn un_premultiply_keeps_zero_alpha_pixels_zero() {
        let mut pixels = [0, 0, 0, 0];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [0, 0, 0, 0]);
    }

    #[test]
    fn un_premultiply_round_trips_a_premultiplied_preview() {
        // The pixel-level regression the preview upload path needs: RGB
        // scaled by alpha before the conversion is restored after it. Both
        // legs round to nearest, so an arbitrary value can drift by ±2; the
        // first triple is chosen to be exact on both legs.
        for straight in [
            [200, 100, 50, 51],
            [255, 128, 0, 51],
            [1, 2, 3, 51],
            [17, 200, 99, 51],
        ] {
            let mut round = premultiply(straight);
            un_premultiply_rgba(&mut round);
            for channel in 0..4 {
                assert!(
                    (i16::from(round[channel]) - i16::from(straight[channel])).abs() <= 2,
                    "channel {channel}: {round:?} did not restore {straight:?}"
                );
            }
        }

        // Exact round trip for the well-behaved triple: 200·51/255 == 40.
        let mut round = premultiply([200, 100, 50, 51]);
        un_premultiply_rgba(&mut round);
        assert_eq!(round, [200, 100, 50, 51]);
    }

    #[test]
    fn un_premultiply_rounds_to_nearest_and_clamps() {
        // alpha 3: (1 * 255 + 1) / 3 == 85 (rounded to nearest).
        let mut pixels = [1, 1, 1, 3];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [85, 85, 85, 3]);

        // A malformed sample above its alpha clamps to 255 instead of
        // wrapping — premultiplied input can never do this, but the helper
        // must stay total.
        let mut malformed = [200, 200, 200, 1];
        un_premultiply_rgba(&mut malformed);
        assert_eq!(malformed, [255, 255, 255, 1]);
    }

    fn device(id: &str, name: &str) -> AudioDeviceView {
        AudioDeviceView {
            id: id.to_string(),
            name: name.to_string(),
            channels: 2,
            capture_sample_rate: 48_000,
        }
    }

    #[test]
    fn the_device_detail_reports_the_rate_capture_negotiates() {
        // cpal's `default_input_config()` reports 44.1 kHz whenever the device
        // advertises it, while capture opens at 48 kHz. The picker renders the
        // capture rate, so no 44.1 kHz heuristic leaks into a row.
        let usb = device("alsa:usb", "USB Microphone");
        assert_eq!(usb.detail(), "2 ch · 48 kHz");

        let hi_res = AudioDeviceView {
            capture_sample_rate: 96_000,
            ..usb.clone()
        };
        assert_eq!(hi_res.detail(), "2 ch · 96 kHz");

        // A device that genuinely negotiates 44.1 kHz still renders as 44.1 kHz.
        let fractional = AudioDeviceView {
            capture_sample_rate: 44_100,
            ..usb.clone()
        };
        assert_eq!(fractional.detail(), "2 ch · 44.1 kHz");

        let unknown = AudioDeviceView {
            capture_sample_rate: 0,
            ..usb
        };
        assert_eq!(unknown.detail(), "2 ch · unknown rate");
    }

    #[test]
    fn the_default_audio_selection_is_the_system_monitor() {
        let selection = AudioSelectionView::default();

        assert!(selection.is_pipewire());
        assert!(!selection.is_default_input());
        assert_eq!(selection.describe(), "System audio (PipeWire monitor)");
        assert!(!selection.matches_device(&device("alsa:usb", "USB Microphone")));
    }

    #[test]
    fn input_selection_matches_by_id_or_name_and_marks_the_default() {
        let usb = device("alsa:usb", "USB Microphone");
        let mut selection = AudioSelectionView {
            source: "input".to_string(),
            device: Some("ALSA:USB".to_string()),
            capture_sample_rate: None,
            error: None,
        };

        assert!(
            selection.matches_device(&usb),
            "ids match case-insensitively"
        );
        assert!(!selection.is_default_input());

        selection.device = Some("usb microphone".to_string());
        assert!(selection.matches_device(&usb), "names match too");

        selection.device = None;
        assert!(selection.is_default_input());
        assert_eq!(selection.describe(), "Default input (system default)");
        assert!(!selection.matches_device(&usb));

        selection.source = "pipewire".to_string();
        selection.device = Some(usb.id.clone());
        assert!(
            !selection.matches_device(&usb),
            "a device is only meaningful for the input source"
        );
    }

    #[test]
    fn the_active_banner_names_the_system_sources_without_exposing_ids() {
        let devices = [device("alsa:usb", "USB Microphone")];
        let pipewire = AudioSelectionView::default();

        assert_eq!(pipewire.active_label(&devices), "System audio");
        assert_eq!(
            pipewire.active_subtitle(&devices),
            "Captures what your speakers play"
        );
        assert_eq!(
            pipewire.active_detail(&devices),
            "PipeWire monitor · pipewire"
        );
        assert!(pipewire.active_device(&devices).is_none());

        let default_input = AudioSelectionView {
            source: "input".to_string(),
            device: None,
            capture_sample_rate: None,
            error: None,
        };
        assert_eq!(default_input.active_label(&devices), "System default input");
        assert_eq!(
            default_input.active_subtitle(&devices),
            "Follows the system's default device"
        );
        assert_eq!(
            default_input.active_detail(&devices),
            "System default · input"
        );
    }

    #[test]
    fn the_active_banner_resolves_the_friendly_device_name_and_details() {
        let usb = device("alsa:usb", "USB Microphone");
        let devices = [usb.clone()];
        let selection = AudioSelectionView {
            source: "input".to_string(),
            device: Some("ALSA:USB".to_string()),
            capture_sample_rate: None,
            error: None,
        };

        assert_eq!(selection.active_device(&devices), Some(&usb));
        assert_eq!(selection.active_label(&devices), "USB Microphone");
        assert_eq!(
            selection.active_subtitle(&devices),
            "Microphone or audio interface"
        );
        assert_eq!(
            selection.active_detail(&devices),
            "2 ch · 48 kHz · alsa:usb"
        );

        let mut fractional = usb.clone();
        fractional.capture_sample_rate = 44_100;
        assert_eq!(
            AudioSelectionView {
                source: "input".to_string(),
                device: Some(fractional.id.clone()),
                capture_sample_rate: None,
                error: None,
            }
            .active_detail(&[fractional]),
            "2 ch · 44.1 kHz · alsa:usb"
        );
    }

    #[test]
    fn the_active_banner_prefers_the_live_feed_rate_over_the_device_rate() {
        let usb = device("alsa:usb", "USB Microphone");
        let mut selection = AudioSelectionView {
            source: "input".to_string(),
            device: Some("alsa:usb".to_string()),
            capture_sample_rate: Some(96_000),
            error: None,
        };

        assert_eq!(
            selection.active_detail(std::slice::from_ref(&usb)),
            "2 ch · 96 kHz · alsa:usb",
            "the running feed's negotiated rate wins over the device row"
        );

        selection.capture_sample_rate = None;
        assert_eq!(
            selection.active_detail(&[usb]),
            "2 ch · 48 kHz · alsa:usb",
            "a silent capture falls back to the device's capture rate"
        );

        let monitor = AudioSelectionView {
            source: "pipewire".to_string(),
            device: None,
            capture_sample_rate: Some(48_000),
            error: None,
        };
        assert_eq!(
            monitor.active_detail(&[]),
            "PipeWire monitor · 48 kHz · pipewire",
            "the PipeWire monitor reports its negotiated rate too"
        );
        assert_eq!(monitor.capture_rate(&[]), Some(48_000));
        assert_eq!(AudioSelectionView::default().capture_rate(&[]), None);
    }

    #[test]
    fn bpm_readouts_append_the_live_capture_rate() {
        assert_eq!(bpm_with_rate(128.0, Some(48_000)), "BPM 128 · 48 kHz");
        assert_eq!(
            bpm_with_rate(128.4, Some(44_100)),
            "BPM 128 · 44.1 kHz",
            "a genuinely fractional negotiated rate keeps its decimal"
        );
        assert_eq!(
            bpm_with_rate(174.0, None),
            "BPM 174",
            "silent capture shows the tempo without a rate"
        );
        assert_eq!(
            bpm_with_rate(120.0, Some(0)),
            "BPM 120",
            "an unknown rate is omitted rather than rendered as 0 Hz"
        );
    }

    #[test]
    fn an_unplugged_device_still_gets_an_honest_banner() {
        let selection = AudioSelectionView {
            source: "input".to_string(),
            device: Some("alsa:gone".to_string()),
            capture_sample_rate: None,
            error: None,
        };

        assert_eq!(selection.active_label(&[]), "Input device");
        assert_eq!(
            selection.active_subtitle(&[]),
            "The selected device is not currently connected"
        );
        assert_eq!(selection.active_detail(&[]), "input · alsa:gone");
    }

    #[test]
    fn performance_scene_order_defaults_to_in_order() {
        let performance = PerformanceView::default();
        assert_eq!(performance.order, PERFORMANCE_ORDER_IN_ORDER);
        assert_eq!(performance.scene_order(), "in-order");
        assert_eq!(performance.scene_order_label(), "In order");
    }

    #[test]
    fn unknown_scene_orders_normalise_to_the_engine_behaviour() {
        for value in ["random", "RANDOM", "shuffle", "", "in-order"] {
            let performance = PerformanceView {
                order: value.to_string(),
                ..Default::default()
            };
            if value == "random" {
                assert_eq!(performance.scene_order(), "random", "value {value:?}");
                assert_eq!(performance.scene_order_label(), "Random", "value {value:?}");
            } else {
                assert_eq!(performance.scene_order(), "in-order", "value {value:?}");
                assert_eq!(
                    performance.scene_order_label(),
                    "In order",
                    "value {value:?}"
                );
            }
        }
    }

    #[test]
    fn scene_order_options_match_the_legacy_select() {
        assert_eq!(
            PERFORMANCE_ORDERS,
            [
                (PERFORMANCE_ORDER_IN_ORDER, "In order"),
                (PERFORMANCE_ORDER_RANDOM, "Random"),
            ]
        );
    }

    #[test]
    fn expression_and_tween_lookups_use_the_input_id() {
        let snapshot = EngineSnapshot {
            expressions: vec![ExpressionView {
                id: "expr-1".to_string(),
                input_id: "input-alpha".to_string(),
                expression: "value * 2".to_string(),
                evaluate_every_frame: true,
            }],
            tweens: HashMap::from([(
                "input-tween".to_string(),
                TweenView {
                    id: "input-tween".to_string(),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        assert_eq!(
            snapshot
                .expression_for_input("input-alpha")
                .map(|assignment| assignment.expression.as_str()),
            Some("value * 2")
        );
        assert!(snapshot.expression_for_input("input-missing").is_none());
        assert!(snapshot.tween_for_input("input-tween").is_some());
        assert!(snapshot.tween_for_input("input-alpha").is_none());
    }

    fn mode(width: u32, height: u32, fps: u32) -> CameraModeView {
        CameraModeView { width, height, fps }
    }

    #[test]
    fn resolutions_are_unique_and_sorted_largest_first() {
        let modes = vec![
            mode(1280, 720, 30),
            mode(1920, 1080, 30),
            mode(1280, 720, 60),
            mode(640, 480, 30),
            mode(1920, 1080, 60),
        ];

        assert_eq!(
            unique_resolutions(&modes),
            vec![(1920, 1080), (1280, 720), (640, 480)]
        );
        assert_eq!(
            unique_resolutions(&[mode(0, 0, 30)]),
            Vec::<(u32, u32)>::new(),
            "a zero-sized mode is not a resolution"
        );
        assert!(unique_resolutions(&[]).is_empty());
    }

    #[test]
    fn frame_rates_are_unique_descending_and_ignore_the_unrated_mode() {
        let modes = vec![
            mode(1280, 720, 30),
            mode(1280, 720, 60),
            mode(1280, 720, 30),
            mode(1280, 720, 0),
            mode(1920, 1080, 90),
        ];

        assert_eq!(fps_options(&modes, 1280, 720), vec![60, 30]);
        assert_eq!(fps_options(&modes, 1920, 1080), vec![90]);
        assert_eq!(fps_options(&modes, 640, 480), Vec::<u32>::new());
        assert_eq!(
            fps_options(&[mode(1280, 720, 0)], 1280, 720),
            Vec::<u32>::new(),
            "fps 0 means the device did not advertise a rate"
        );
        assert!(fps_options(&[], 1280, 720).is_empty());
    }

    #[test]
    fn the_default_mode_is_the_highest_resolution_then_rate() {
        let modes = vec![
            mode(1280, 720, 30),
            mode(1920, 1080, 30),
            mode(1920, 1080, 60),
            mode(1280, 720, 60),
        ];

        assert_eq!(
            default_camera_mode(&modes),
            Some(mode(1920, 1080, 60)),
            "resolution wins before frame rate"
        );
        assert_eq!(default_camera_mode(&[mode(0, 0, 30)]), None);
        assert_eq!(default_camera_mode(&[]), None);
    }

    #[test]
    fn choosing_a_resolution_keeps_the_rate_when_offered_else_takes_the_fastest() {
        let modes = vec![
            mode(1920, 1080, 60),
            mode(1280, 720, 30),
            mode(1280, 720, 25),
            mode(640, 480, 0),
        ];

        assert_eq!(
            mode_for_resolution(&modes, 1920, 1080, 30),
            Some(mode(1920, 1080, 60)),
            "the requested rate is not offered, so the fastest wins"
        );
        assert_eq!(
            mode_for_resolution(&modes, 1920, 1080, 60),
            Some(mode(1920, 1080, 60))
        );
        assert_eq!(
            mode_for_resolution(&modes, 1280, 720, 25),
            Some(mode(1280, 720, 25)),
            "an advertised preferred rate is kept"
        );
        assert_eq!(
            mode_for_resolution(&modes, 640, 480, 30),
            Some(mode(640, 480, 30)),
            "an unrated mode keeps the preferred rate"
        );
        assert_eq!(mode_for_resolution(&modes, 800, 600, 30), None);
    }

    #[test]
    fn bar_timing_uses_four_beats_and_the_120_bpm_fallback() {
        assert_eq!(bar_seconds(120.0), 2.0);
        assert_eq!(bar_seconds(60.0), 4.0);
        assert_eq!(bar_seconds(0.0), 2.0);
        assert_eq!(bar_seconds(f64::NAN), 2.0);
        assert_eq!(effective_bpm(f64::INFINITY), 120.0);

        assert_eq!(timing_value_seconds(4.0, TimingUnit::Bars, 120.0), 8.0);
        assert_eq!(timing_value_seconds(4.0, TimingUnit::Bars, 60.0), 16.0);
        assert_eq!(timing_seconds_value(8.0, TimingUnit::Bars, 120.0), 4.0);
        assert_eq!(timing_value_seconds(4.0, TimingUnit::Seconds, 60.0), 4.0);
    }

    #[test]
    fn timing_hints_name_the_unit_and_resolve_bar_durations() {
        assert_eq!(
            timing_hint(8.0, 24.0, TimingUnit::Seconds, 120.0),
            "8.0–24.0 s"
        );
        assert_eq!(
            timing_hint(4.0, 12.0, TimingUnit::Bars, 120.0),
            "4.00–12.00 bars ≈ 8.0–24.0 s at 120 BPM"
        );
        assert_eq!(
            timing_hint(4.0, 12.0, TimingUnit::Bars, 0.0),
            "4.00–12.00 bars ≈ 8.0–24.0 s at 120 BPM",
            "an invalid tempo falls back to 120"
        );
        assert_eq!(TimingUnit::parse("BAR"), TimingUnit::Bars);
        assert_eq!(TimingUnit::parse("nonsense"), TimingUnit::Seconds);
        assert_eq!(TimingUnit::Bars.suffix(), " bar");
    }

    fn plugin_prop(schema: serde_json::Value) -> PluginPropView {
        PluginPropView::from_schema("test", &schema).expect("a supported prop schema")
    }

    #[test]
    fn plugin_props_parse_the_descriptor_schema() {
        let gain = plugin_prop(serde_json::json!({
            "type": "float",
            "default": 0.5,
            "value": 0.75,
            "min": 0.0,
            "max": 2.0,
            "step": 0.01
        }));
        assert_eq!(gain.kind, PluginPropKindView::Float);
        assert_eq!(gain.value, serde_json::json!(0.75));
        assert_eq!(gain.default, serde_json::json!(0.5));
        assert_eq!(
            (gain.min, gain.max, gain.step),
            (Some(0.0), Some(2.0), Some(0.01))
        );
        assert!(gain.options.is_empty());

        let mode = plugin_prop(serde_json::json!({
            "type": "enum",
            "default": "wide",
            "value": "wide",
            "options": [
                { "label": "Wide", "value": "wide" },
                { "label": "Square", "value": "square" }
            ]
        }));
        assert_eq!(
            mode.kind,
            PluginPropKindView::Select,
            "the legacy `enum` alias maps to select"
        );
        assert_eq!(mode.options.len(), 2);
        assert_eq!(mode.options[1].label, "Square");
        assert_eq!(mode.options[1].value, serde_json::json!("square"));
        assert_eq!(mode.selected_index(), Some(0));
        assert_eq!(mode.selected_label(), Some("Wide"));

        // A missing current value falls back to the declared default.
        let text = plugin_prop(serde_json::json!({ "type": "text", "default": "hello" }));
        assert_eq!(text.value, serde_json::json!("hello"));
        assert_eq!(text.kind, PluginPropKindView::Text);

        assert_eq!(
            PluginPropKindView::parse("bool"),
            Some(PluginPropKindView::Bool)
        );
        assert_eq!(
            PluginPropKindView::parse("color"),
            Some(PluginPropKindView::Color)
        );
        assert_eq!(
            PluginPropKindView::parse("int"),
            Some(PluginPropKindView::Int)
        );
        assert_eq!(PluginPropKindView::parse("vec3"), None);
        assert!(
            PluginPropView::from_schema("bad", &serde_json::json!({ "type": "vec3" })).is_none()
        );
        assert!(PluginPropView::from_schema("bad", &serde_json::json!({})).is_none());
    }

    #[test]
    fn plugin_prop_bounds_always_contain_the_value_and_default() {
        let bounded = plugin_prop(serde_json::json!({
            "type": "int", "default": 4, "value": 6, "min": 0, "max": 10
        }));
        assert_eq!(bounded.bounds(), (0.0, 10.0, 1.0));

        // A float with no declared bounds widens to the value/default.
        let wide = plugin_prop(serde_json::json!({
            "type": "float", "default": 5.5, "value": 9.25
        }));
        let (min, max, step) = wide.bounds();
        assert!(min <= 0.0 && max >= 9.25, "{min}..{max}");
        assert_eq!(step, 0.001);

        // A broken min/max pair still yields a usable slider.
        let broken = plugin_prop(serde_json::json!({
            "type": "float", "default": 1.0, "value": 1.0, "min": 4.0, "max": 4.0
        }));
        let (min, max, _) = broken.bounds();
        assert!(max > min, "{min}..{max}");

        let declared_step = plugin_prop(serde_json::json!({
            "type": "float", "default": 0.0, "value": 0.0, "step": 0.25
        }));
        assert_eq!(declared_step.bounds().2, 0.25);
    }

    #[test]
    fn plugin_numeric_payloads_round_ints_and_keep_floats() {
        let int = plugin_prop(serde_json::json!({"type": "int", "default": 0, "value": 0}));
        assert_eq!(int.number_value(3.6), serde_json::json!(4));
        let float = plugin_prop(serde_json::json!({"type": "float", "default": 0, "value": 0}));
        assert_eq!(float.number_value(3.6), serde_json::json!(3.6));
    }

    #[test]
    fn plugin_colour_payloads_clamp_and_keep_the_ratio_shape() {
        assert_eq!(
            plugin_color_value([0.1, 0.2, 0.3, 0.4]),
            serde_json::json!({ "r": 0.1, "g": 0.2, "b": 0.3, "a": 0.4 })
        );
        assert_eq!(
            plugin_color_value([-1.0, 2.0, f32::NAN, 0.5]),
            serde_json::json!({ "r": 0.0, "g": 1.0, "b": 0.0, "a": 0.5 }),
            "nan clamps to zero instead of serialising as null"
        );
    }

    fn plugin_info(name: &str, loaded: bool, failed: bool, error: Option<&str>) -> PluginInfoView {
        PluginInfoView {
            path: format!("/plugins/{name}.js"),
            name: name.to_string(),
            author: "modV".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            active: loaded,
            loaded,
            failed,
            error: error.map(str::to_string),
            source_bytes: 128,
            capture: None,
            props: Vec::new(),
        }
    }

    #[test]
    fn plugin_status_prefers_errors_then_loaded_then_disabled() {
        assert_eq!(
            plugin_info("a", true, false, None).status(),
            PluginStatus::Loaded
        );
        assert_eq!(
            plugin_info("b", false, false, None).status(),
            PluginStatus::Disabled
        );
        assert_eq!(
            plugin_info("c", false, true, Some("boom")).status(),
            PluginStatus::Error
        );
        assert_eq!(
            plugin_info("d", true, false, Some("hook threw")).status(),
            PluginStatus::Error,
            "a runtime error outranks the loaded context"
        );
        assert_eq!(PluginStatus::Loaded.label(), "Loaded");
        assert_eq!(PluginStatus::Disabled.label(), "Disabled");
        assert_eq!(PluginStatus::Error.label(), "Error");
    }

    #[test]
    fn plugin_rows_fall_back_to_the_file_name_and_combine_author_and_version() {
        let mut plugin = plugin_info("", false, false, None);
        plugin.path = "/plugins/plugin.js".to_string();
        assert_eq!(
            plugin.title(),
            "plugin",
            "the file stem names an unnamed plugin"
        );
        assert_eq!(plugin.file_name(), "plugin.js");
        plugin.name = "Lumen".to_string();
        assert_eq!(plugin.title(), "Lumen");
        assert_eq!(plugin.credit(), "modV · v1.0.0");
        plugin.version.clear();
        assert_eq!(plugin.credit(), "modV");
        plugin.author.clear();
        plugin.version = "1.0.0".to_string();
        assert_eq!(plugin.credit(), "v1.0.0");
        plugin.version.clear();
        assert_eq!(plugin.credit(), "");
    }

    #[test]
    fn plugin_view_explains_why_the_list_is_greyed() {
        let mut view = PluginView {
            plugins: vec![plugin_info("Lumen", true, false, None)],
            loaded_count: 1,
            ..Default::default()
        };
        assert!(!view.is_running());
        assert!(
            view.gate_reason()
                .is_some_and(|reason| reason.contains("consent")),
            "unconsented plugins explain the gate: {:?}",
            view.gate_reason()
        );
        assert_eq!(view.summary(), "Not running");

        view.consented = true;
        assert!(
            view.gate_reason()
                .is_some_and(|reason| reason.contains("turned off")),
            "{:?}",
            view.gate_reason()
        );

        view.enabled = true;
        assert!(view.is_running());
        assert_eq!(view.gate_reason(), None);
        assert_eq!(view.summary(), "1 of 1 plugin loaded");
        view.capture = Some([160, 90]);
        assert_eq!(view.capture_label(), Some("capture 160×90".to_string()));
    }

    fn camera(id: &str, name: &str) -> CameraView {
        CameraView {
            id: id.to_string(),
            name: name.to_string(),
            max_width: 1280,
            max_height: 720,
            max_fps: 30,
            modes: Vec::new(),
            selected: CameraModeView::default(),
        }
    }

    fn camera_source(id: &str, index: usize) -> MediaSourceView {
        MediaSourceView {
            source_id: format!("camera:{id}"),
            kind: "camera".to_string(),
            index,
            ..Default::default()
        }
    }

    #[test]
    fn ordered_cameras_puts_running_cameras_first_in_webcam_index_order() {
        let media = MediaSnapshot {
            cameras: vec![
                camera("a", "Alpha"),
                camera("b", "Bravo"),
                camera("c", "Charlie"),
            ],
            sources: vec![camera_source("b", 2), camera_source("c", 1)],
            ..Default::default()
        };

        let names: Vec<&str> = media
            .ordered_cameras()
            .into_iter()
            .map(|camera| camera.name.as_str())
            .collect();
        assert_eq!(names, vec!["Charlie", "Bravo", "Alpha"]);
    }

    #[test]
    fn ordered_cameras_sorts_unselected_cameras_by_name() {
        let media = MediaSnapshot {
            cameras: vec![
                camera("z", "Zulu"),
                camera("a", "alpha"),
                camera("b", "Bravo"),
            ],
            ..Default::default()
        };

        let names: Vec<&str> = media
            .ordered_cameras()
            .into_iter()
            .map(|camera| camera.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "Bravo", "Zulu"]);
    }

    #[test]
    fn ordered_cameras_ignores_non_camera_sources_and_missing_indexes() {
        let media = MediaSnapshot {
            cameras: vec![camera("a", "Alpha"), camera("b", "Bravo")],
            sources: vec![
                MediaSourceView {
                    source_id: "screen:portal".to_string(),
                    kind: "screen".to_string(),
                    index: 1,
                    ..Default::default()
                },
                camera_source("b", 0),
            ],
            ..Default::default()
        };

        let names: Vec<&str> = media
            .ordered_cameras()
            .into_iter()
            .map(|camera| camera.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Bravo", "Alpha"],
            "a camera source with index 0 still leads; screens never enter the camera list"
        );
    }
}
