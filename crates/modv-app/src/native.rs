//! The native single-process shell (`modv` binary): winit + egui + the
//! in-process engine.
//!
//! P0 scope: one egui UI window and one compositor-owned output window sharing
//! a single `wgpu::Device`/`Queue`. The compositor's main texture is registered
//! with `egui-wgpu` as a native texture, so the live output preview has no
//! readback and no IPC. The UI reads an [`EngineSnapshot`] built from the
//! authoritative [`Store`] and emits [`UiAction`]s that are dispatched before
//! the next render.
//!
//! The legacy Electron app remains the shipping product; this binary is the
//! opt-in native path (`MODV_UI=native`, `make native`).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use modv_media::audio::engine::AudioFeed;
use modv_media::osc::{DEFAULT_OSC_HOST, DEFAULT_OSC_PORT};
use modv_render::Engine;
use modv_render::surface::{SurfaceTarget, create_surface, create_surface_with_present_mode};
use modv_render::wgpu;
use modv_renderers::two_d::AudioFrame;
use modv_ui::model::{
    ExpressionView, NdiOutputView, NdiReceiverView, NdiSenderView, NdiSnapshot, NdiSourceView,
    OscServerView, OscSnapshot, osc_addresses,
};
use modv_ui::model::{ScreenCaptureView, ScreenShareKindView, ScreenSourceView};
use modv_ui::{
    AudioDeviceView, AudioSelectionView, AudioView, CameraModeView, CameraView, EngineHealth,
    EngineSnapshot, FileDialogAction, GroupView, InputLinkView, MediaAction, MediaLibraryView,
    MediaSnapshot, MediaSourceView, MidiDeviceView, MidiSnapshot, ModuleMetaView, ModuleView,
    PerformanceView, PluginInfoView, PluginPropView, PluginView, PreviewRequest, PreviewTexture,
    ProjectView, PropComponentView, PropView, RowMonitorView, SceneCellView, SceneView,
    SidePanelTab, TextureSourceView, TweenView, UiAction, UiCommand, UiState,
    VideoSourceProblemView, VideoTransportAction, VideoTransportView, gallery_preview_key,
    scene_preview_key,
};
use serde_json::Value;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};

use crate::cli::{AudioSourceSelection, Cli};
use crate::compositor::{Compositor, FrameInfo, RowFadeInfo};
use crate::media::MediaHub;
use crate::media_dialogs::{self, DocumentKind, FilePicker};
use crate::media_profiles::{self, DEFAULT_PROFILE, PROJECT_SETTINGS_FILENAME, ProjectSettings};
use crate::media_settings::{CameraMode, MediaSettings, plan_restore, resolve_mode};
use crate::media_watch::{MediaFolder, MediaWatcher};
use crate::midi::MidiClient;
use crate::pacing::FramePacer;
use crate::plugin_runtime::{self, NativeStateReader, PluginRuntime};
use crate::report::AdapterSummary;
use crate::scene_transition::SceneTransitionInfo;
use crate::screen_share::{
    SCREEN_SOURCE_ID, ScreenAnswer, ScreenRequest, ScreenShare, ScreenShareError, ScreenShareKind,
    ScreenStream, spawn_portal_request,
};
use crate::store_ops::Store;
use crate::ui_layout::{LAYOUT_SETTINGS_FILENAME, LayoutSettings};
use crate::video_sources::VideoSources;
use crate::window::{DoubleClickTracker, is_fullscreen_exit_key, is_fullscreen_toggle_key};
use crate::window_state::{SavedWindow, WINDOW_STATE_FILENAME, WindowState, user_data_dir};
use modv_engine::engine_state::{GALLERY_GROUP_NAME, Group, ModuleInstance, js_truthy};
use modv_engine::tweens::TweenDescriptor;
use modv_media::video::{PortalError, PortalStream, ScreenSupport, TestPattern, VideoSourceConfig};

/// The native shell's run loop.
pub fn run(cli: Cli, engine: Engine, adapter: AdapterSummary) -> Result<(), String> {
    let event_loop =
        EventLoop::new().map_err(|error| format!("failed to create event loop: {error}"))?;
    let mut app = NativeApp::new(cli, engine, adapter);
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("native event loop failed: {error}"))?;
    app.shutdown();
    Ok(())
}

/// The output window's default geometry (logical pixels).
const OUTPUT_WIDTH: u32 = 960;
const OUTPUT_HEIGHT: u32 = 540;

/// How often the NDI and OSC panels re-read the engine while their tab is
/// visible. The engine runtime is cheap to read, but the panels only need to
/// show sender/receiver/address changes at a human cadence; hidden tabs do not
/// poll at all.
const PANEL_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Portal answers kept around after their request was cancelled, so a late
/// answer is recognised as stale instead of starting a source. The one pending
/// request is always the newest; older channels are abandoned dialogs the user
/// has not answered yet.
const MAX_SCREEN_ANSWERS: usize = 8;

/// One compositor-owned output window.
struct OutputWindow {
    window: Arc<Window>,
    surface: SurfaceTarget,
}

/// One uploaded egui preview texture and its pixel size.
struct PreviewEntry {
    handle: egui::TextureHandle,
    width: u32,
    height: u32,
}

/// One performance row's layer monitor: the live Group rendering the row and
/// the egui native texture holding its slow-cadence GPU snapshot.
///
/// The texture is registered after the row's first monitored refresh; the
/// entry (and the compositor-side snapshot) is dropped as soon as the row's
/// Group changes or disappears — a Scene apply rebuilds Groups with fresh ids,
/// so the old snapshot must never be sampled as if it were current.
struct RowMonitorEntry {
    group_id: String,
    texture: Option<egui::TextureId>,
    width: u32,
    height: u32,
}

/// A tessellated egui frame waiting for the present pass.
struct PendingUiFrame {
    clipped: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
    free: Vec<egui::TextureId>,
}

/// The negotiated analysis rate of the running native capture feed.
///
/// Every rate readout in the shell — the Audio panel's active banner and
/// picker, the status bar, the Tempo window and the Info panel — reads this
/// cache, so it reports the rate capture actually negotiated
/// ([`AudioFeed::sample_rate`]) instead of cpal's 44.1 kHz
/// `default_input_config()` heuristic. It is `None` whenever no feed is
/// running (a failed pump, a stopped capture, or a startup without a device),
/// which is exactly when the UI omits the rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AudioCaptureRate(Option<u32>);

impl AudioCaptureRate {
    /// Adopt the rate of a newly opened feed. A non-finite/zero rate is
    /// reported as unknown rather than rendered as `0 Hz`.
    fn opened(&mut self, feed_rate: f32) {
        self.0 = negotiated_sample_rate(feed_rate);
    }

    /// Forget the rate once the feed stops; the UI then omits it.
    fn stopped(&mut self) {
        self.0 = None;
    }

    fn get(self) -> Option<u32> {
        self.0
    }
}

/// `AudioFeed::sample_rate()` is an `f32`; the UI renders whole hertz.
fn negotiated_sample_rate(feed_rate: f32) -> Option<u32> {
    let rate = feed_rate.round();
    (rate.is_finite() && rate > 0.0).then_some(rate as u32)
}

/// Bind the default OSC listener and return its view.
///
/// Electron creates the same `0.0.0.0:3333` server at startup
/// (`application/index.js:471`); without it OSC Input Links never receive in
/// native mode. A failure (port in use, no network) is logged once and is not
/// fatal — the rest of the shell keeps working.
fn start_default_osc_listener(store: &mut Store) -> Vec<OscServerView> {
    match start_osc_listener(store, DEFAULT_OSC_HOST, DEFAULT_OSC_PORT) {
        Ok(server) => {
            eprintln!(
                "ui: INFO OSC listener on {} (default)",
                server.address_label()
            );
            vec![server]
        }
        Err(error) => {
            eprintln!(
                "ui: WARN could not start the default OSC listener on \
                 {DEFAULT_OSC_HOST}:{DEFAULT_OSC_PORT}: {error}"
            );
            Vec::new()
        }
    }
}

/// Bind one OSC listener through the engine's `osc/createServer` action.
fn start_osc_listener(store: &mut Store, host: &str, port: u16) -> Result<OscServerView, String> {
    let reply = store
        .dispatch(
            "osc/createServer",
            serde_json::json!({ "host": host, "port": port }),
        )
        .map_err(|error| error.to_string())?;
    OscServerView::from_create_reply(&reply)
        .ok_or_else(|| format!("unexpected osc/createServer reply: {reply}"))
}

/// Read the engine's NDI runtime into the panel view.
fn ndi_snapshot(store: &Store) -> NdiSnapshot {
    let runtime = store.ndi();
    let config = runtime.config();
    let sender = runtime.sender_status();

    NdiSnapshot {
        available: runtime.available(),
        library_path: runtime.library_path(),
        output: NdiOutputView {
            enabled: config.output_enabled,
            name: config.output_name.clone(),
            width: config.output_width,
            height: config.output_height,
            follow_output_size: config.follow_modv_output_size,
            follow_fps: config.follow_modv_fps,
            target_fps: config.output_target_fps,
        },
        sender: NdiSenderView {
            running: sender.running,
            unavailable: sender.unavailable,
            error: sender.error,
            frames_sent: sender.frames_sent,
        },
        discovering: runtime.discovering(),
        show_local_sources: runtime.show_local_sources(),
        sources: runtime
            .sources()
            .iter()
            .map(|source| NdiSourceView {
                name: source.name.clone(),
                url_address: source.url_address.clone(),
            })
            .collect(),
        receivers: runtime
            .receivers()
            .iter()
            .map(|receiver| NdiReceiverView {
                id: receiver.id.clone(),
                source_name: receiver.source_name.clone(),
                url_address: receiver.url_address.clone(),
                enabled: receiver.enabled,
                state: receiver.state.as_str().to_string(),
                source: receiver.source.clone(),
                error: receiver.error.clone(),
                width: receiver.width,
                height: receiver.height,
                frames: receiver.frames,
            })
            .collect(),
    }
}

/// Read the engine's OSC state into the panel view.
fn osc_snapshot(store: &Store, servers: &[OscServerView]) -> OscSnapshot {
    let state = store.state();
    let addresses = state.osc.get("data").map(osc_addresses).unwrap_or_default();

    OscSnapshot {
        servers: servers.to_vec(),
        addresses,
    }
}

/// Build the Plugins panel read model from the authoritative runtime.
///
/// Pure and cheap: the descriptor props are already serialised values, so this
/// parses each schema into a [`PluginPropView`] without evaluating JavaScript.
/// The shell calls it at 1 Hz while the tab is visible and after every plugin
/// command, never per frame.
fn plugin_snapshot(runtime: &PluginRuntime) -> PluginView {
    let plugins = runtime
        .plugins()
        .iter()
        .map(|entry| PluginInfoView {
            path: entry.path.clone(),
            name: entry.name.clone(),
            author: entry.author.clone(),
            version: entry.version.clone(),
            enabled: entry.enabled,
            active: entry.active,
            loaded: entry.loaded,
            failed: entry.failed,
            error: entry.error.clone(),
            source_bytes: entry.source_bytes,
            capture: entry.capture.map(|capture| [capture.width, capture.height]),
            props: entry
                .props
                .as_object()
                .map(|props| {
                    props
                        .iter()
                        .filter_map(|(name, schema)| PluginPropView::from_schema(name, schema))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    PluginView {
        enabled: runtime.settings().enabled,
        consented: runtime.consented(),
        loaded_count: runtime.loaded_count(),
        plugins,
        logs: runtime.logs().to_vec(),
        capture: runtime
            .capture_config()
            .map(|capture| [capture.width, capture.height]),
    }
}

/// A portal request's worker-thread receiver.
///
/// The request id travels with the receiver: the state machine only applies an
/// answer while that id is still pending, so a cancelled request's late answer
/// is dropped (and its portal session closed) instead of starting a source.
struct PendingScreenAnswer {
    request_id: u64,
    rx: std::sync::mpsc::Receiver<Result<PortalStream, PortalError>>,
}

/// The native application state.
pub struct NativeApp {
    cli: Cli,
    engine: Option<Engine>,
    adapter: AdapterSummary,
    store: Store,
    /// `<userData>` and the shared media tree it owns.
    user_data: PathBuf,
    media_root: PathBuf,
    /// The media root the compositor was last pointed at (image resolution).
    applied_media_root: Option<PathBuf>,
    /// The persisted active profile and extra user ISF folders.
    project_settings: ProjectSettings,
    project_settings_path: PathBuf,
    active_profile: String,
    profiles: Vec<String>,
    extra_isf_dirs: Vec<PathBuf>,
    /// Set after a failed project-settings write so a broken disk logs once.
    project_settings_error_logged: bool,
    last_profile_scan: Instant,
    /// The last Save preset target (CLI `--preset`, then Save As/Open Preset).
    preset_path: Option<PathBuf>,
    compositor: Option<Compositor>,
    egui_ctx: egui::Context,
    egui_state: Option<EguiWinitState>,
    egui_renderer: Option<EguiRenderer>,
    pending_ui: Option<PendingUiFrame>,
    ui_state: UiState,
    ui_window: Option<Arc<Window>>,
    ui_surface: Option<SurfaceTarget>,
    output: Option<OutputWindow>,
    preview_id: Option<egui::TextureId>,
    preview_size: (u32, u32),
    /// Gallery/Scene hover textures owned by the UI context.
    preview_textures: HashMap<String, PreviewEntry>,
    preview_order: VecDeque<String>,
    preview_error_counts: HashMap<String, u32>,
    /// Layer monitors keyed by performance row (the Scenes grid's per-row
    /// thumbnails).
    row_monitors: HashMap<usize, RowMonitorEntry>,
    window_state: WindowState,
    window_state_path: PathBuf,
    window_state_dirty: bool,
    last_window_state_save: Instant,
    window_state_error_logged: bool,
    /// The persisted resizable layout (panels, sections, tab placements, windows).
    layout_settings: LayoutSettings,
    layout_settings_path: PathBuf,
    layout_dirty: bool,
    last_layout_save: Instant,
    layout_error_logged: bool,
    media_hub: MediaHub,
    last_media_health: Instant,
    /// Video-file sources started from `video` texture props (one per unique
    /// absolute path, reference-counted by module/prop).
    video_sources: VideoSources,
    /// Cached V4L2 enumeration; rescanning every frame is 50–500 µs per camera.
    media_cameras: Vec<modv_media::video::CameraInfo>,
    last_camera_scan: Instant,
    /// The persisted Webcam selection (ordered ids + per-camera modes).
    media_settings: MediaSettings,
    media_settings_path: PathBuf,
    /// Selected cameras that were not enumerated yet; retried once a second.
    media_restore_pending: Vec<String>,
    /// Set after a failed settings write so a broken disk logs once.
    media_settings_error_logged: bool,
    /// The XDG-portal screen capture: one pending request and one live
    /// [`PortalStream`] (the session that keeps its PipeWire node alive).
    screen_share: ScreenShare<PortalStream>,
    /// Portal requests whose worker thread is still waiting for the dialog.
    /// Kept after a cancel so a late answer can be recognised as stale.
    screen_answers: Vec<PendingScreenAnswer>,
    /// Whether the host can capture a PipeWire screen node (`pipewiresrc` +
    /// a PipeWire socket), refreshed with the camera scan.
    screen_support: ScreenSupport,
    audio_feed: Option<AudioFeed>,
    audio_error_logged: bool,
    /// The running feed's negotiated rate, the only rate the UI renders.
    audio_capture_rate: AudioCaptureRate,
    /// The live capture selection (the Audio panel's active choice).
    audio_selection: AudioSourceSelection,
    audio_device: Option<String>,
    audio_devices: Vec<AudioDeviceView>,
    /// The last failed live switch, shown in the Audio panel.
    audio_switch_error: Option<String>,
    audio_settings_path: PathBuf,
    midi_client: MidiClient,
    /// The opt-in sandboxed Boa plugin runtime (settings, discovery, hooks).
    plugin_runtime: PluginRuntime,
    /// The Plugins panel view, rebuilt at 1 Hz while the tab is visible (and
    /// immediately after a plugin command).
    plugin_view: PluginView,
    last_plugin_poll: Instant,
    /// Whether the Plugins tab was visible on the previous frame (the panel
    /// clears its retained log window on reopen).
    plugins_visible: bool,
    /// Whether the Plugins tab has been on screen at all.
    plugins_seen: bool,
    /// Set after the first failed plugin capture read-back so a broken GPU
    /// path logs once instead of every capture interval.
    plugin_capture_error_logged: bool,
    /// Set after a failed NDI frame read-back so a broken GPU path logs once.
    ndi_error_logged: bool,
    /// NDI panel view, refreshed at 1 Hz while the tab is visible.
    ndi_view: NdiSnapshot,
    last_ndi_poll: Instant,
    /// OSC listeners created through this shell (the startup default plus any
    /// the user adds). `Store` keeps its `OscServer` list private, so the shell
    /// records the `osc/createServer` replies and drops ids on removal.
    osc_servers: Vec<OscServerView>,
    /// OSC panel view (listeners + the latest `osc.data` leaves).
    osc_view: OscSnapshot,
    last_osc_poll: Instant,
    isf_dir: Option<PathBuf>,
    user_isf_dirs: Vec<PathBuf>,
    media_watcher: Option<MediaWatcher>,
    /// The active profile's recursive image/video/palette index.
    media_library: crate::media_library::MediaLibrary,
    /// Coalesced rescan requests from the watcher and the Rescan button.
    library_refresh: crate::media_library::LibraryRefresh,
    /// The Library tab's read model (rebuilt after a scan/thumbnail decode).
    library_view: MediaLibraryView,
    /// Uploaded Library thumbnails keyed by media-root-relative path.
    library_textures: HashMap<String, PreviewEntry>,
    library_view_dirty: bool,
    modifiers: HashMap<WindowId, ModifiersState>,
    double_click: HashMap<WindowId, DoubleClickTracker>,
    frames: u64,
    start: Instant,
    last_frame: Instant,
    /// Redraw cadence, paced to the engine's configured FPS.
    pacer: FramePacer,
    last_metric: Instant,
    metric_frames: u32,
    fps: f32,
    frame_ms: f32,
    bench_deadline: Option<Duration>,
    done: bool,
    failed: bool,
    last_error: Option<String>,
}

impl NativeApp {
    /// The opt-in JavaScript plugin runtime (the future Plugins panel model).
    pub fn plugins(&self) -> &PluginRuntime {
        &self.plugin_runtime
    }

    /// Mutable access for the Plugins panel's enable/consent/reload actions.
    pub fn plugins_mut(&mut self) -> &mut PluginRuntime {
        &mut self.plugin_runtime
    }

    fn new(cli: Cli, engine: Engine, adapter: AdapterSummary) -> Self {
        let start = Instant::now();
        let mut media_hub = MediaHub::new();
        // One host query at startup so the Media panel can explain a missing
        // pipewiresrc/PipeWire before the user asks for a screen.
        let screen_support = media_hub.screen_support();

        let isf_dir = resolve_isf_dir(&cli);
        let user_data = user_data_dir();
        let media_root = user_data.join("media");
        let project_settings_path = user_data.join(PROJECT_SETTINGS_FILENAME);
        let mut project_settings = match ProjectSettings::read_from(&project_settings_path) {
            Ok(settings) => settings.unwrap_or_default(),
            Err(error) => {
                eprintln!("ui: WARN ignoring the saved project choice: {error}");
                ProjectSettings::default()
            }
        };
        let prepared = match media_profiles::prepare_profiles(
            &media_root,
            project_settings.active_profile(),
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("ui: WARN could not prepare the media profiles: {error}");
                media_profiles::PreparedProfiles {
                    active: DEFAULT_PROFILE.to_string(),
                    profiles: Vec::new(),
                }
            }
        };
        if prepared.active != project_settings.active_profile() {
            eprintln!(
                "ui: WARN project profile {:?} is missing; using {:?}",
                project_settings.active_profile(),
                prepared.active
            );
        }
        let active_profile = prepared.active;
        let profiles = prepared.profiles;
        let extra_isf_dirs = project_settings.isf_dirs.clone();
        let user_isf_dirs = user_isf_dirs(&media_root, &active_profile, &extra_isf_dirs);
        let mut store = build_native_store(
            cli.state.as_deref(),
            &user_data,
            isf_dir.as_deref(),
            &user_isf_dirs,
        );
        // The native shell has no renderer to forward the media manager's
        // path, so it dispatches `media/setMediaDirectoryPath` itself; the
        // compositor resolves texture props and library images under it.
        dispatch_media_root(&mut store, &media_root);
        // Loose Scene documents saved by earlier installs (and by Electron's
        // Scene editor) become part of the authoritative list once per
        // profile; `<userData>/scenes.json` still wins on every id/name.
        crate::media_import::import_profile_documents_once(
            &mut store,
            &media_root,
            &active_profile,
            &mut project_settings,
            &project_settings_path,
        );
        eprintln!(
            "ui: INFO native shell ready ({} groups, {} modules, {} scenes, {} registered)",
            store.state().groups.len(),
            store.state().modules.len(),
            store.scenes_state().scenes.len(),
            store
                .registry_arc()
                .registered_modules()
                .as_object()
                .map_or(0, serde_json::Map::len)
        );

        // Electron binds the default OSC listener at startup
        // (`application/index.js:186-189`); native mode must do the same or
        // OSC Input Links can never receive. A bind failure is reported and
        // the shell keeps running without that listener.
        let osc_servers = start_default_osc_listener(&mut store);
        let ndi_view = ndi_snapshot(&store);
        let osc_view = osc_snapshot(&store, &osc_servers);

        let mut preset_path = cli.preset.clone();
        if let Some(path) = crate::cli::startup_document_path(
            cli.preset.as_deref(),
            cli.scene.as_deref(),
            cli.playlist.as_deref(),
        ) {
            match media_dialogs::open_document(&mut store, path) {
                Ok(kind) => {
                    if kind != DocumentKind::Preset {
                        // A Scene/playlist is not a preset document: saving a
                        // preset over it would drop its metadata.
                        preset_path = None;
                    }
                    eprintln!(
                        "ui: INFO opened startup {} {}",
                        kind.label(),
                        path.display()
                    );
                }
                Err(error) => eprintln!(
                    "ui: WARN could not open startup document {}: {error}",
                    path.display()
                ),
            }
        }
        apply_startup_scene(&mut store, cli.apply_scene.as_deref());

        // Audio capture selection: explicit `--audio-source`/`--audio-device`
        // (or their MODV_* environment fallbacks) wins, then the choice saved
        // by the Audio panel, then the PipeWire monitor → default input
        // fallback the native shell has always used.
        let audio_settings_path = user_data.join(crate::audio_settings::AUDIO_SETTINGS_FILENAME);
        let audio_explicit = crate::audio_settings::audio_cli_explicit(
            &std::env::args().collect::<Vec<_>>(),
            std::env::var("MODV_AUDIO_SOURCE").ok().as_deref(),
            std::env::var("MODV_AUDIO_DEVICE").ok().as_deref(),
        );
        let persisted_audio =
            match crate::audio_settings::AudioSettings::read_from(&audio_settings_path) {
                Ok(settings) => settings,
                Err(error) => {
                    eprintln!("ui: WARN ignoring the saved audio selection: {error}");
                    None
                }
            };
        if let Some(settings) = &persisted_audio {
            eprintln!(
                "ui: INFO saved audio selection: {}{}",
                settings.describe(),
                if audio_explicit {
                    " (overridden by --audio-source/--audio-device or the environment)"
                } else {
                    ""
                }
            );
        }
        let audio_plan = crate::audio_settings::initial_plan(
            cli.audio_source,
            cli.audio_device.as_deref(),
            audio_explicit,
            persisted_audio.as_ref(),
        );
        let (audio_feed, audio_selection, audio_device) = match open_native_audio(&audio_plan) {
            Some((feed, selection, device)) => (Some(feed), selection, device),
            None => (
                None,
                audio_plan
                    .first()
                    .map(|(selection, _)| *selection)
                    .unwrap_or(AudioSourceSelection::Pipewire),
                audio_plan.first().and_then(|(_, device)| device.clone()),
            ),
        };
        let mut audio_capture_rate = AudioCaptureRate::default();
        if let Some(feed) = &audio_feed {
            audio_capture_rate.opened(feed.sample_rate());
        }
        let audio_devices =
            crate::audio_settings::device_views(&modv_media::audio::capture::input_devices());

        // Webcam selection: Electron persists `mediaStream` (ordered
        // `selectedVideoSources` + per-device `videoSettings`). The native
        // shell stores the same thing in `<userData>/native-media.json` and
        // restores the saved order/modes once a camera enumeration sees the
        // device, including cameras that are (re)plugged in later.
        let media_settings_path = user_data.join(crate::media_settings::MEDIA_SETTINGS_FILENAME);
        let media_settings = match MediaSettings::read_from(&media_settings_path) {
            Ok(settings) => settings.unwrap_or_default(),
            Err(error) => {
                eprintln!("ui: WARN ignoring the saved webcam selection: {error}");
                MediaSettings::default()
            }
        };
        if !media_settings.cameras.is_empty() {
            eprintln!(
                "ui: INFO restoring {} saved webcam(s) when they appear: {}",
                media_settings.cameras.len(),
                media_settings.cameras.join(", ")
            );
        }
        let media_restore_pending = media_settings.cameras.clone();

        // The resizable layout (panel/section sizes, tab placements and
        // floating-window bounds). Malformed documents are ignored with a
        // warning; the defaults keep the shell usable.
        let layout_settings_path = user_data.join(LAYOUT_SETTINGS_FILENAME);
        let layout_settings = match LayoutSettings::read_from(&layout_settings_path) {
            Ok(settings) => settings.unwrap_or_default(),
            Err(error) => {
                eprintln!("ui: WARN ignoring the saved layout: {error}");
                LayoutSettings::default()
            }
        };
        let midi_client = MidiClient::new();
        let media_watcher =
            MediaWatcher::for_profile(&media_root, &active_profile, &extra_isf_dirs);
        if media_watcher.is_some() {
            eprintln!(
                "ui: INFO watching the {active_profile:?} media tree for ISF/image/video/palette changes"
            );
        }
        let mut media_library =
            crate::media_library::MediaLibrary::new(media_root.clone(), active_profile.clone());
        media_library.rescan();
        eprintln!(
            "ui: INFO media library indexed {} asset(s) in the {active_profile:?} profile",
            media_library.total_count()
        );
        let library_view = media_library.view(&HashMap::new());
        if midi_client.available() {
            eprintln!("ui: INFO MIDI input available (hot-plug polled at 1 Hz)");
        } else if let Some(error) = midi_client.error() {
            eprintln!("ui: WARN MIDI unavailable: {error}");
        }

        // Opt-in JavaScript plugins: discovery reads the active profile's
        // `plugin/` folder and `<userData>/plugins`, but the host only loads a
        // context when both the saved master switch and consent are on.
        let mut plugin_runtime = PluginRuntime::load(
            user_data.join(plugin_runtime::PLUGIN_SETTINGS_FILENAME),
            vec![
                media_profiles::profile_folder(&media_root, &active_profile, "plugin"),
                user_data.join("plugins"),
            ],
        );
        plugin_runtime.set_frame_inputs(
            plugin_runtime::features_from_store(&store),
            Rc::new(NativeStateReader::from_store(&store)),
        );
        plugin_runtime.start();
        let discovered_plugins = plugin_runtime.plugins().len();
        if plugin_runtime.enabled() {
            eprintln!(
                "ui: INFO JavaScript plugins enabled ({} of {discovered_plugins} loaded)",
                plugin_runtime.loaded_count()
            );
        } else if discovered_plugins > 0 {
            eprintln!(
                "ui: INFO {discovered_plugins} JavaScript plugin(s) discovered; plugins are disabled"
            );
        }
        let plugin_view = plugin_snapshot(&plugin_runtime);

        let egui_ctx = egui::Context::default();
        modv_ui::theme::apply(&egui_ctx);
        let target_fps = store.target_fps();

        Self {
            bench_deadline: cli.bench_deadline(),
            preset_path,
            cli,
            engine: Some(engine),
            adapter,
            store,
            media_root,
            applied_media_root: None,
            project_settings,
            project_settings_path,
            active_profile,
            profiles,
            extra_isf_dirs,
            project_settings_error_logged: false,
            last_profile_scan: start,
            compositor: None,
            egui_ctx,
            egui_state: None,
            egui_renderer: None,
            pending_ui: None,
            ui_state: UiState::new(),
            ui_window: None,
            ui_surface: None,
            output: None,
            preview_id: None,
            preview_size: (0, 0),
            preview_textures: HashMap::new(),
            preview_order: VecDeque::new(),
            preview_error_counts: HashMap::new(),
            row_monitors: HashMap::new(),
            window_state: WindowState::read(&user_data.join(WINDOW_STATE_FILENAME)),
            window_state_path: user_data.join(WINDOW_STATE_FILENAME),
            user_data,
            window_state_dirty: false,
            last_window_state_save: start,
            window_state_error_logged: false,
            layout_settings,
            layout_settings_path,
            layout_dirty: false,
            last_layout_save: start,
            layout_error_logged: false,
            media_hub,
            last_media_health: start,
            video_sources: VideoSources::new(),
            media_cameras: Vec::new(),
            last_camera_scan: start.checked_sub(Duration::from_secs(10)).unwrap_or(start),
            media_settings,
            media_settings_path,
            media_restore_pending,
            media_settings_error_logged: false,
            screen_share: ScreenShare::new(),
            screen_answers: Vec::new(),
            screen_support,
            audio_feed,
            audio_error_logged: false,
            audio_capture_rate,
            audio_selection,
            audio_device,
            audio_devices,
            audio_switch_error: None,
            audio_settings_path,
            midi_client,
            plugin_runtime,
            plugin_view,
            last_plugin_poll: start,
            plugins_visible: false,
            plugins_seen: false,
            plugin_capture_error_logged: false,
            ndi_error_logged: false,
            ndi_view,
            last_ndi_poll: start,
            osc_servers,
            osc_view,
            last_osc_poll: start,
            isf_dir,
            user_isf_dirs,
            media_watcher,
            media_library,
            library_refresh: crate::media_library::LibraryRefresh::default(),
            library_view,
            library_textures: HashMap::new(),
            library_view_dirty: false,
            modifiers: HashMap::new(),
            double_click: HashMap::new(),
            frames: 0,
            start,
            last_frame: start,
            pacer: FramePacer::new(start, target_fps),
            last_metric: start,
            metric_frames: 0,
            fps: 0.0,
            frame_ms: 0.0,
            done: false,
            failed: false,
            last_error: None,
        }
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) {
        let Some(engine) = self.engine.as_ref() else {
            eprintln!("ui: ERROR engine missing during initialisation");
            self.failed = true;
            event_loop.exit();
            return;
        };

        let mut attributes = WindowAttributes::default()
            .with_title("modV Native")
            .with_inner_size(PhysicalSize::new(
                self.cli.width.max(480),
                self.cli.height.max(320),
            ))
            .with_min_inner_size(PhysicalSize::new(480, 320));
        if let Some(saved) = self
            .window_state
            .main_window
            .map(|saved| clamp_saved_to_monitor(saved, event_loop))
        {
            attributes = attributes
                .with_inner_size(LogicalSize::new(saved.width, saved.height))
                .with_position(LogicalPosition::new(saved.x, saved.y))
                .with_maximized(saved.maximized);
            if saved.fullscreen {
                attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
            }
        }
        if self.cli.fullscreen {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let ui_window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("ui: ERROR failed to create the UI window: {error}");
                self.failed = true;
                event_loop.exit();
                return;
            }
        };

        // Apply the saved layout to the real window size before the first frame.
        // `clamp_to` protects a layout saved on a larger display, and the shell
        // applies the same minimums while drawing.
        let inner = ui_window
            .inner_size()
            .to_logical::<f32>(ui_window.scale_factor());
        let applied = self
            .layout_settings
            .clone()
            .clamp_to(inner.width, inner.height);
        applied.apply_to_ui(&mut self.ui_state.layout);
        self.layout_settings = applied;

        let ui_surface = match create_surface(engine, Arc::clone(&ui_window)) {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!("ui: ERROR failed to create the UI surface: {error}");
                self.failed = true;
                event_loop.exit();
                return;
            }
        };

        let egui_state = EguiWinitState::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &ui_window,
            Some(ui_window.scale_factor() as f32),
            None,
            Some(2048),
        );
        let egui_renderer = EguiRenderer::new(
            &engine.device,
            ui_surface.format(),
            RendererOptions::default(),
        );

        let mut compositor =
            Compositor::with_registry(&engine.device, &engine.queue, self.store.registry_arc());
        compositor.resize(OUTPUT_WIDTH, OUTPUT_HEIGHT);

        self.ui_window = Some(ui_window);
        self.ui_surface = Some(ui_surface);
        self.egui_state = Some(egui_state);
        self.egui_renderer = Some(egui_renderer);
        self.compositor = Some(compositor);
        // Image props resolve under `<userData>/media` from the first frame.
        self.sync_media_root();

        self.ui_state.p0.range_control_ok = true;
        self.ui_state.p0.scene_cell_ok = true;

        // Render one frame so the main texture exists before registering it
        // with egui-wgpu. The output window opens on the next command/frame.
        self.render_engine(None);
        self.update_preview_texture();

        if let Err(error) = self.open_output(event_loop) {
            eprintln!("ui: WARN could not open the output window: {error}");
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.pacer.deadline()));
        if let Some(window) = &self.ui_window {
            window.request_redraw();
        }
    }

    /// Create the output window (idempotent).
    fn open_output(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.output.is_some() {
            return Ok(());
        }
        let Some(engine) = self.engine.as_ref() else {
            return Err("engine is not ready".to_string());
        };

        let mut attributes = WindowAttributes::default()
            .with_title("VmodV Output  (F = fullscreen)")
            .with_inner_size(LogicalSize::new(
                f64::from(OUTPUT_WIDTH),
                f64::from(OUTPUT_HEIGHT),
            ))
            .with_position(LogicalPosition::new(120.0, 120.0))
            .with_resizable(true);
        if let Some(saved) = self
            .window_state
            .output()
            .cloned()
            .map(|saved| clamp_saved_to_monitor(saved, event_loop))
        {
            attributes = attributes
                .with_inner_size(LogicalSize::new(saved.width, saved.height))
                .with_position(LogicalPosition::new(saved.x, saved.y));
            if saved.fullscreen {
                attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
            }
        }
        let window = event_loop
            .create_window(attributes)
            .map_err(|error| error.to_string())?;
        let window = Arc::new(window);
        let surface = create_surface_with_present_mode(
            engine,
            Arc::clone(&window),
            wgpu::PresentMode::AutoNoVsync,
        )
        .map_err(|error| error.to_string())?;

        let (width, height) = surface.dimensions();
        self.output = Some(OutputWindow { window, surface });
        self.ui_state.output_open = true;
        self.ui_state.p0.output_window_ok = true;

        // Resize the compositor to the new output and render one frame so the
        // registered preview texture is valid immediately.
        if let Some(compositor) = &mut self.compositor {
            compositor.resize(width, height);
        }
        self.render_engine(None);
        self.update_preview_texture();
        eprintln!("ui: INFO output window opened ({width}x{height})");
        Ok(())
    }

    fn close_output(&mut self) {
        if self.output.take().is_some() {
            self.ui_state.output_open = false;
            self.ui_state.p0.output_window_ok = false;
            eprintln!("ui: INFO output window closed");
        }
    }

    fn toggle_output_fullscreen(&mut self) {
        let Some(output) = &self.output else {
            return;
        };
        let leaving = output.window.fullscreen().is_some();
        output.window.set_fullscreen(if leaving {
            None
        } else {
            Some(Fullscreen::Borderless(None))
        });
        eprintln!(
            "ui: INFO output fullscreen {}",
            if leaving { "off" } else { "on" }
        );
    }

    fn tick_engine(&mut self, dt: Duration) {
        let phrase_flipped = self.pump_audio(dt);
        self.store.drain_osc();
        // Complete any NDI discovery run / receiver status change on the UI
        // thread; the finder and receivers run on their own threads, so this
        // only drains a finished result and never blocks the frame.
        self.store.poll_ndi();
        // Native has no Vuex mirror: the delta stream only exists for the IPC
        // bridge, which this shell does not run. Drop the records instead of
        // letting OSC traffic and meter updates grow the queue for the whole
        // session.
        let _ = self.store.drain_deltas();
        self.midi_client.pump(&mut self.store);
        if self.store.scenes_tick_due() {
            let delta = dt.as_secs_f64() * 1000.0;
            if let Err(error) = self.store.dispatch(
                "scenes/tick",
                serde_json::json!({
                    "delta": delta,
                    "phraseFlipped": phrase_flipped,
                }),
            ) {
                self.record_error(format!("scenes/tick failed: {error}"));
            }
        }
        let audio = self.store.audio().clone();
        self.store.update_input_links(&audio, self.start.elapsed());
    }

    /// Pump the native engine-owned audio feed and publish it into the store.
    ///
    /// The Electron path defaults to the renderer's WebAudio pump; native has
    /// no renderer, so it opens PipeWire's default monitor (the system-audio
    /// loopback) and falls back to the default input. A capture failure logs
    /// once, stops the feed and leaves the engine silent instead of killing
    /// the frame loop.
    ///
    /// Returns the frame's phrase-detector pulse so `scenes/tick` can use it
    /// for bar-window Scene changes (one frame only).
    fn pump_audio(&mut self, dt: Duration) -> bool {
        let Some(feed) = self.audio_feed.as_mut() else {
            return false;
        };
        let snapshot = feed.frame(dt.as_secs_f32());
        let kick = feed.analyser().kick();
        let phrase_flipped = snapshot.phrase_drop;
        let error = feed.take_error();
        self.store
            .set_engine_audio(&snapshot, kick, self.start.elapsed());
        let Some(error) = error else {
            return phrase_flipped;
        };
        if !self.audio_error_logged {
            self.audio_error_logged = true;
            eprintln!("ui: WARN native audio capture stopped ({error}); engine is silent");
        }
        self.audio_feed = None;
        self.audio_capture_rate.stopped();
        self.store.disable_engine_audio();
        phrase_flipped
    }

    /// Build this frame's plugin IO and run `preProcessFrame` hooks.
    ///
    /// The state/feature snapshot is only built while plugins are enabled; a
    /// disabled runtime (or one without consent) does no work and allocates no
    /// Boa context.
    fn plugin_pre_frame(&mut self) {
        if !self.plugin_runtime.enabled() {
            return;
        }
        self.plugin_runtime.set_frame_inputs(
            plugin_runtime::features_from_store(&self.store),
            Rc::new(NativeStateReader::from_store(&self.store)),
        );
        self.plugin_runtime.pre_frame();
    }

    /// Read a throttled capture (only when a plugin asked for one) and run
    /// `postProcessFrame` hooks after the frame is presented.
    fn plugin_post_frame(&mut self, now: Instant) {
        if !self.plugin_runtime.enabled() {
            return;
        }
        let mut read_error = None;
        {
            let runtime = &mut self.plugin_runtime;
            let compositor = &mut self.compositor;
            runtime.capture_frame(now, |width, height| {
                let compositor = compositor.as_mut()?;
                match compositor.read_frame_at(width, height) {
                    Ok(frame) => frame,
                    Err(error) => {
                        read_error = Some(error.to_string());
                        None
                    }
                }
            });
        }
        if let Some(error) = read_error
            && !self.plugin_capture_error_logged
        {
            self.plugin_capture_error_logged = true;
            eprintln!("ui: WARN plugin capture read-back failed: {error}");
        }
        self.plugin_runtime.post_frame();
    }

    fn frame_info(&self, scene_transition: Option<SceneTransitionInfo>) -> FrameInfo {
        let runtime = self.store.performance_runtime();
        FrameInfo {
            time_seconds: self.start.elapsed().as_secs_f32(),
            frame_index: self.frames,
            date: crate::window::wall_clock_date(self.start.elapsed().as_secs_f32()),
            audio: self.store.audio().clone(),
            row_fades: runtime
                .row_transitions
                .values()
                .map(|transition| RowFadeInfo {
                    row: transition.row,
                    group_id: transition.group_id.clone(),
                    ghost_group_id: transition.ghost_group_id.clone(),
                    from_alpha: transition.from_alpha,
                    target_alpha: transition.target_alpha,
                    progress: transition.progress(),
                })
                .collect(),
            scene_transition,
        }
    }

    fn render_engine(&mut self, scene_transition: Option<SceneTransitionInfo>) {
        let info = self.frame_info(scene_transition);
        let state = self.store.state();
        if let Some(compositor) = &mut self.compositor
            && let Err(error) = compositor.render_frame(state, &info)
        {
            self.record_error(format!("compositor frame failed: {error}"));
        }
    }

    fn update_preview_texture(&mut self) {
        let Some(compositor) = &self.compositor else {
            return;
        };
        let Some(view) = compositor.main_texture_view() else {
            return;
        };
        let size = compositor.main_texture_size();
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        let Some(engine) = &self.engine else {
            return;
        };
        let Some(renderer) = &mut self.egui_renderer else {
            return;
        };
        match self.preview_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &engine.device,
                &view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                let id = renderer.register_native_texture(
                    &engine.device,
                    &view,
                    wgpu::FilterMode::Linear,
                );
                self.preview_id = Some(id);
                self.ui_state.preview_texture = Some(id);
            }
        }
        self.preview_size = size;
        self.ui_state.preview_size = [size.0 as usize, size.1 as usize];
        self.ui_state.p0.shared_texture_ok = true;
    }

    fn draw_frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.done || self.failed {
            return;
        }
        let now = Instant::now();
        // Pace the whole shell to the engine's configured FPS. The worker
        // renders at `fps/SET_FPS` (60 by default) and advances tween frames,
        // audio smoothing and canvas feedback once per paced frame; rendering
        // as fast as the GPU allows made those systems run several times too
        // fast, so visuals changed speed with the host frame rate.
        self.pacer.set_fps(self.store.target_fps());
        if !self.pacer.due(now) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.pacer.deadline()));
            return;
        }
        self.pacer.advance(now);
        let dt = if self.frames == 0 {
            Duration::from_secs_f64(1.0 / 60.0)
        } else {
            now.saturating_duration_since(self.last_frame)
        };
        self.last_frame = now;

        self.tick_engine(dt);
        self.drain_media_actions();
        self.poll_screen_answers();
        if now.saturating_duration_since(self.last_media_health) >= Duration::from_secs(1) {
            self.media_hub.poll_health();
            self.last_media_health = now;
        }
        if now.saturating_duration_since(self.last_profile_scan) >= Duration::from_secs(2) {
            self.refresh_profiles();
            self.last_profile_scan = now;
        }
        if now.saturating_duration_since(self.last_camera_scan) >= Duration::from_secs(1) {
            self.media_cameras = self.media_hub.list_cameras();
            // PipeWire/`pipewiresrc` can appear (or disappear) while the shell
            // runs, so the screen buttons follow the host's current support.
            self.screen_support = self.media_hub.screen_support();
            self.restore_pending_cameras();
            self.last_camera_scan = now;
        }
        if let Some(burst) = self.media_watcher.as_mut().and_then(MediaWatcher::pump) {
            self.library_refresh.note_burst(&burst);
            if burst.has(MediaFolder::Isf) {
                self.rescan_modules();
            }
            if burst.has(MediaFolder::Video) {
                // A clip may have appeared/changed under a path a prop already
                // names: retry missing files and a failed decode now.
                self.video_sources.note_media_change();
            }
            let others = burst.other_folders();
            if !others.is_empty() {
                let labels: Vec<&str> = others.iter().map(|folder| folder.label()).collect();
                eprintln!(
                    "ui: INFO media folders changed: {} ({} event(s)); no engine re-registration needed",
                    labels.join(", "),
                    burst.events
                );
            }
        }
        self.drain_ui_commands(event_loop);
        if self.done {
            return;
        }
        self.poll_panel_views(now);

        let snapshot = self.build_snapshot();
        self.ui_state.sync_status(&snapshot);
        self.run_ui(&snapshot);
        self.drain_ui_actions();
        // Texture-prop writes from this frame have landed: reconcile the video
        // file sources before the compositor samples their textures.
        self.reconcile_video_sources(now);
        // Plugin `preProcessFrame` sees this frame's audio plus the state after
        // the UI actions, and runs before the composition is rendered.
        self.plugin_pre_frame();
        self.capture_pending_transition();
        self.sync_media_root();
        if let Some(compositor) = &mut self.compositor {
            compositor.sync_media(&mut self.media_hub);
            // Upload newer frames for every enabled NDI receiver to its
            // `ndi:<receiverId>` texture, like the media sources above.
            compositor.sync_ndi(self.store.ndi());
        }
        let scene_transition = self.prepare_scene_transition();
        self.render_engine(scene_transition);
        if self
            .compositor
            .as_ref()
            .is_some_and(|compositor| compositor.main_texture_size() != self.preview_size)
        {
            self.update_preview_texture();
        }
        self.present();
        // `postProcessFrame` runs after present; it only pays for a capture
        // read-back when a running plugin requested one (≤20 Hz).
        self.plugin_post_frame(now);
        // The NDI sender reads the presented framebuffer back only when the
        // configured rate is due and the output is enabled.
        self.submit_ndi_frame();
        self.answer_preview_requests();
        self.refresh_row_monitors();

        self.frames += 1;
        self.metric_frames += 1;
        let metric_elapsed = now.saturating_duration_since(self.last_metric);
        if metric_elapsed >= Duration::from_secs(1) {
            self.fps = self.metric_frames as f32 / metric_elapsed.as_secs_f32();
            self.frame_ms = 1000.0 / self.fps.max(0.001);
            // The NDI sender follows/advertises the measured render rate when
            // `followModVFps` is on.
            self.store.ndi_mut().set_engine_fps(f64::from(self.fps));
            self.metric_frames = 0;
            self.last_metric = now;
        }

        if self.window_state_dirty
            && now.saturating_duration_since(self.last_window_state_save) >= Duration::from_secs(1)
        {
            self.save_window_state();
        }
        if self.layout_dirty
            && now.saturating_duration_since(self.last_layout_save) >= Duration::from_secs(1)
        {
            self.save_layout();
        }

        let reached_frames = self
            .cli
            .frames
            .map(|limit| self.frames >= limit)
            .unwrap_or(false);
        let reached_deadline = self
            .bench_deadline
            .map(|limit| self.start.elapsed() >= limit)
            .unwrap_or(false);
        if reached_frames || reached_deadline {
            self.finish_and_exit(event_loop);
        } else {
            // `about_to_wait` requests the redraw when the next deadline is
            // due, instead of spinning at the display's maximum rate.
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.pacer.deadline()));
        }
    }

    /// Submit the just-presented frame to the NDI sender when it is due.
    ///
    /// Ported from `window.rs`: the read-back is synchronised and expensive,
    /// so it only runs when the NDI runtime says the sender is enabled,
    /// running and the configured frame rate has elapsed.
    fn submit_ndi_frame(&mut self) {
        if !self.store.ndi_mut().frame_due(Instant::now()) {
            return;
        }
        let Some(compositor) = &mut self.compositor else {
            return;
        };
        let size = compositor.size();
        match compositor.read_main_rgba8() {
            Ok(pixels) if !pixels.is_empty() => {
                self.store.ndi_mut().submit_frame(size.0, size.1, &pixels);
            }
            Ok(_) => {}
            Err(error) => {
                if !self.ndi_error_logged {
                    self.ndi_error_logged = true;
                    eprintln!("ui: WARN NDI frame read-back failed: {error}");
                }
            }
        }
    }

    fn run_ui(&mut self, snapshot: &EngineSnapshot) {
        let Some(window) = self.ui_window.clone() else {
            return;
        };
        let Some(state) = &mut self.egui_state else {
            return;
        };
        let raw_input = state.take_egui_input(&window);
        let ctx = self.egui_ctx.clone();
        let full_output = ctx.run_ui(raw_input, |ui| {
            modv_ui::shell::draw(ui, snapshot, &mut self.ui_state);
        });
        if let Some(state) = &mut self.egui_state {
            state.handle_platform_output(&window, full_output.platform_output);
        }

        let egui::FullOutput {
            mut textures_delta,
            shapes,
            pixels_per_point,
            ..
        } = full_output;
        let clipped = ctx.tessellate(shapes, pixels_per_point);
        if let Some(engine) = &self.engine
            && let Some(renderer) = &mut self.egui_renderer
        {
            for (id, deltas) in &textures_delta.set {
                for delta in deltas {
                    renderer.update_texture(&engine.device, &engine.queue, *id, delta);
                }
            }
        }
        let free = textures_delta.free.iter().copied().collect();
        // `TexturesDelta` panics on drop when deltas were not acknowledged;
        // the set deltas were uploaded above and the free list is kept in the
        // pending frame, so clear the epaint bookkeeping.
        textures_delta.clear();
        self.sync_layout();
        self.pending_ui = Some(PendingUiFrame {
            clipped,
            pixels_per_point,
            free,
        });
    }

    fn present(&mut self) {
        let Some(frame) = self.pending_ui.take() else {
            return;
        };
        let (Some(engine), Some(ui_surface), Some(renderer)) =
            (&self.engine, &mut self.ui_surface, &mut self.egui_renderer)
        else {
            return;
        };

        let ui_frame = acquire_surface(ui_surface, &engine.device);
        let output_frame = self
            .output
            .as_mut()
            .and_then(|output| acquire_surface(&mut output.surface, &engine.device));

        if ui_frame.is_none() && output_frame.is_none() {
            for id in frame.free {
                renderer.free_texture(&id);
            }
            return;
        }

        let mut encoder = engine
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("modv-native-frame"),
            });

        if let Some((output_frame, _)) = &output_frame
            && let Some(compositor) = &mut self.compositor
        {
            let view = output_frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let format = self
                .output
                .as_ref()
                .map(|output| output.surface.format())
                .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
            compositor.present(&mut encoder, &view, format);
        }

        let mut user_commands = Vec::new();
        if let Some((ui_frame, suboptimal)) = &ui_frame {
            let (width, height) = ui_surface.dimensions();
            let screen_descriptor = ScreenDescriptor {
                size_in_pixels: [width, height],
                pixels_per_point: frame.pixels_per_point,
            };
            user_commands = renderer.update_buffers(
                &engine.device,
                &engine.queue,
                &mut encoder,
                &frame.clipped,
                &screen_descriptor,
            );
            let view = ui_frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            {
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("modv-native-ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.066,
                                g: 0.066,
                                b: 0.066,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                renderer.render(
                    &mut pass.forget_lifetime(),
                    &frame.clipped,
                    &screen_descriptor,
                );
            }
            if *suboptimal {
                ui_surface.reconfigure(&engine.device);
            }
        }

        engine.queue.submit(
            user_commands
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        if let Some((frame, _)) = ui_frame {
            engine.queue.present(frame);
        }
        if let Some((frame, suboptimal)) = output_frame {
            engine.queue.present(frame);
            if suboptimal && let Some(output) = &mut self.output {
                output.surface.reconfigure(&engine.device);
            }
        }
        for id in frame.free {
            renderer.free_texture(&id);
        }
    }

    fn drain_media_actions(&mut self) {
        for action in self.ui_state.take_media_actions() {
            match action {
                MediaAction::StartCamera {
                    id,
                    width,
                    height,
                    fps,
                } => self.start_camera(&id, Some(CameraMode { width, height, fps })),
                MediaAction::MoveCamera { id, delta } => {
                    if self.media_settings.move_camera(&id, delta) {
                        self.sync_camera_order();
                        self.save_media_settings();
                        eprintln!(
                            "ui: INFO webcam order: {}",
                            self.media_settings.cameras.join(", ")
                        );
                    }
                }
                MediaAction::StartTestPattern => {
                    let config = VideoSourceConfig::test_pattern(TestPattern::Smpte)
                        .with_mode(1280, 720, 30);
                    if let Err(error) = self.media_hub.start("test-pattern:smpte", config) {
                        self.record_error(format!("test pattern failed: {error}"));
                    }
                }
                MediaAction::Stop { source_id } => {
                    // A video-file source is owned by its texture prop; an
                    // explicit stop holds it down until the prop changes.
                    if self
                        .video_sources
                        .stop_source(&mut self.media_hub, &source_id)
                    {
                        eprintln!("ui: INFO video texture source {source_id:?} stopped");
                    }
                    // An explicit stop deselects a webcam: forgetting the id is
                    // what stops the restore pass from re-opening it later.
                    if let Some(camera_id) = source_id.strip_prefix("camera:") {
                        self.media_restore_pending.retain(|id| id != camera_id);
                        if self.media_settings.remove(camera_id) {
                            self.save_media_settings();
                        }
                    }
                    self.media_hub.stop(&source_id);
                    if source_id == SCREEN_SOURCE_ID {
                        // The portal session outlives the pipeline; closing it
                        // releases the PipeWire node and the desktop's sharing
                        // indicator. A stop can also land on a pending dialog
                        // (the UI has no Stop for one, but a scripted stop
                        // should still cancel it).
                        if let Some(handle) = self.screen_share.stop() {
                            close_screen_stream(handle);
                        } else if let Some(request_id) = self.screen_share.cancel() {
                            eprintln!(
                                "ui: INFO screen share request {request_id} cancelled by a stop"
                            );
                        }
                    }
                }
                MediaAction::VideoTransport { source, action } => {
                    self.handle_video_transport(&source, action);
                }
                MediaAction::ShareScreen { window } => {
                    self.share_screen(ScreenShareKind::for_window(window));
                }
                MediaAction::CancelScreenShare => {
                    if let Some(request_id) = self.screen_share.cancel() {
                        eprintln!(
                            "ui: INFO screen share request {request_id} cancelled; the portal \
                             session closes when the dialog is answered"
                        );
                    }
                }
            }
        }
    }

    /// Reconcile the video-file sources with the live texture props.
    fn reconcile_video_sources(&mut self, now: Instant) {
        let requests = crate::video_sources::collect_video_props(self.store.state());
        let media_root = self
            .store
            .media_root()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.media_root.clone());
        self.video_sources.reconcile(
            &mut self.media_hub,
            Some(media_root.as_path()),
            &requests,
            now,
        );
    }

    /// Apply one UI transport command and persist its option on every prop
    /// that references the source (pause/loop/speed survive presets/Scenes).
    fn handle_video_transport(&mut self, source: &str, action: VideoTransportAction) {
        let transport = match self
            .video_sources
            .transport(&mut self.media_hub, source, action)
        {
            Ok(transport) => transport,
            Err(error) => {
                eprintln!("ui: WARN video transport {action:?} for {source:?} failed: {error}");
                return;
            }
        };

        let (key, value) = match action {
            VideoTransportAction::Pause
            | VideoTransportAction::Resume
            | VideoTransportAction::TogglePause => ("paused", serde_json::json!(transport.paused)),
            VideoTransportAction::SetLooping { .. } => {
                ("loop", serde_json::json!(transport.looping))
            }
            VideoTransportAction::SetPlaybackRate { .. } => {
                ("playbackrate", serde_json::json!(transport.playback_rate))
            }
            // Restart/seek move the playhead; there is nothing to persist.
            VideoTransportAction::Restart | VideoTransportAction::Seek { .. } => return,
        };

        self.persist_video_option(source, key, value);
    }

    /// Write `options.<key>` back into every prop that references `source`.
    fn persist_video_option(&mut self, source: &str, key: &str, value: Value) {
        let references = self.video_sources.references(source);
        if references.is_empty() {
            return;
        }

        let mut updates = Vec::new();
        for reference in references {
            let Some(module) = self.store.state().modules.get(&reference.module_id) else {
                continue;
            };
            let Some(current) = module.props.get(&reference.prop).cloned() else {
                continue;
            };
            let mut updated = current.clone();
            crate::video_sources::set_prop_option(&mut updated, key, value.clone());
            if updated != current {
                updates.push((reference, updated));
            }
        }

        for (reference, updated) in updates {
            let payload = serde_json::json!({
                "moduleId": reference.module_id,
                "prop": reference.prop,
                "data": { "value": updated },
            });
            if let Err(error) = self.store.dispatch("modules/updateProp", payload) {
                eprintln!(
                    "ui: WARN could not persist video option {key:?} on {}/{}: {error}",
                    reference.module_id, reference.prop
                );
            }
        }
    }

    /// Queue the portal's own picker for a monitor or window.
    ///
    /// The blocking `request_screencast` call runs on a worker thread; its
    /// answer is drained by [`Self::poll_screen_answers`]. One request and one
    /// live capture at a time, so a second Share is refused while the dialog is
    /// open or [`SCREEN_SOURCE_ID`] runs.
    fn share_screen(&mut self, kind: ScreenShareKind) {
        if self.screen_share.is_busy() {
            eprintln!(
                "ui: WARN screen share ({kind}) ignored: a screen request or capture is already \
                 active"
            );
            return;
        }
        // The portal can hand out a node, but without `pipewiresrc` (or a
        // PipeWire socket) nothing could capture it; say why instead of
        // opening a dialog that cannot work.
        if let Some(reason) = self.screen_support.reason.clone() {
            eprintln!("ui: WARN screen share ({kind}) unavailable: {reason}");
            self.screen_share
                .refuse(ScreenShareError::Unsupported(reason));
            return;
        }

        let Some(request) = self.screen_share.request(kind) else {
            return;
        };
        match spawn_portal_request(kind) {
            Ok(rx) => {
                eprintln!(
                    "ui: INFO screen share ({kind}) requested as request {}; waiting for the \
                     portal dialog",
                    request.id
                );
                self.screen_answers.push(PendingScreenAnswer {
                    request_id: request.id,
                    rx,
                });
                if self.screen_answers.len() > MAX_SCREEN_ANSWERS {
                    // The oldest channel belongs to a cancelled dialog (the
                    // pending request was just pushed). Dropping it makes that
                    // thread close its session as soon as it is answered.
                    let dropped = self.screen_answers.remove(0);
                    eprintln!(
                        "ui: WARN stopped waiting for abandoned screen share request {}",
                        dropped.request_id
                    );
                }
            }
            Err(error) => {
                eprintln!("ui: WARN screen share ({kind}) could not start: {error}");
                self.screen_share.cancel();
                self.screen_share.refuse(error);
            }
        }
    }

    /// Apply finished portal requests (non-blocking; one per frame at most is
    /// enough because at most one request is outstanding).
    fn poll_screen_answers(&mut self) {
        let mut index = 0;
        while index < self.screen_answers.len() {
            let answer = match self.screen_answers[index].rx.try_recv() {
                Ok(answer) => Some(answer),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err(PortalError::Runtime(
                        "the screen share thread stopped without an answer".to_string(),
                    )))
                }
            };
            let Some(result) = answer else {
                index += 1;
                continue;
            };

            let pending = self.screen_answers.remove(index);
            match self.screen_share.complete(pending.request_id, result) {
                ScreenAnswer::Stale => {
                    // Cancelled (or already answered): `complete` dropped the
                    // handle, which closes its portal session in the
                    // background.
                    eprintln!(
                        "ui: INFO discarded a screen share answer for stale request {}",
                        pending.request_id
                    );
                }
                ScreenAnswer::Failed { request, error } => {
                    eprintln!("ui: WARN screen share ({}) failed: {error}", request.kind);
                    if error != ScreenShareError::Cancelled {
                        self.record_error(format!("screen capture failed: {error}"));
                    }
                }
                ScreenAnswer::Start {
                    request,
                    config,
                    handle,
                } => self.start_screen_stream(request, config, handle),
            }
        }
    }

    /// Start the picked node and keep its portal session alive for the capture.
    fn start_screen_stream(
        &mut self,
        request: ScreenRequest,
        config: VideoSourceConfig,
        handle: PortalStream,
    ) {
        let node_id = handle.node_id();
        match self.media_hub.start(SCREEN_SOURCE_ID, config) {
            Ok(index) => {
                eprintln!(
                    "ui: INFO screen share ({}) started as {SCREEN_SOURCE_ID:?} (PipeWire node \
                     {node_id}, Webcam index {index})",
                    request.kind
                );
                // The `PortalStream` owns the D-Bus session, and the session
                // owns the PipeWire node: it must outlive the capture.
                self.screen_share.started(request.kind, node_id, handle);
            }
            Err(error) => {
                eprintln!(
                    "ui: WARN screen share ({}) could not start PipeWire node {node_id}: {error}",
                    request.kind
                );
                self.screen_share
                    .start_failed(ScreenShareError::StartFailed(error.to_string()));
                close_screen_stream(handle);
            }
        }
    }

    /// The Media panel's screen capture read model.
    fn screen_view(&self) -> ScreenCaptureView {
        ScreenCaptureView {
            unavailable: self.screen_support.reason.clone(),
            pending: self
                .screen_share
                .pending()
                .map(|request| request.kind.into()),
            running: self
                .screen_share
                .running()
                .map(|(source_id, kind, node_id)| ScreenSourceView {
                    source_id: source_id.to_string(),
                    node_id,
                    kind: kind.into(),
                }),
            error: self.screen_share.error().map(ToString::to_string),
        }
    }

    /// Start (or restart with a new mode) a camera, remembering the choice.
    ///
    /// `requested` is the mode the panel asked for (all zero means "use the
    /// saved mode"). It is validated against the advertised caps through
    /// [`resolve_mode`], so an unadvertised mode never reaches the pipeline.
    fn start_camera(&mut self, camera_id: &str, requested: Option<CameraMode>) {
        let caps = self
            .media_cameras
            .iter()
            .find(|camera| camera.id == camera_id)
            .map_or(&[][..], |camera| camera.caps.as_slice());
        let mode = match requested.filter(|mode| !mode.is_unset()) {
            Some(requested) => resolve_mode(Some(requested), caps),
            None => resolve_mode(self.media_settings.mode_for(camera_id), caps),
        };

        let mut changed = self.media_settings.add(camera_id);
        changed |= self.media_settings.set_mode(camera_id, mode);
        self.media_restore_pending.retain(|id| id != camera_id);

        self.start_camera_source(camera_id, mode);
        self.sync_camera_order();
        if changed {
            self.save_media_settings();
        }
    }

    /// Start a camera pipeline at `mode`; restart it when it already runs in a
    /// different mode.
    fn start_camera_source(&mut self, camera_id: &str, mode: CameraMode) -> bool {
        let source_id = format!("camera:{camera_id}");
        let running = self.media_hub.source_mode(&source_id);
        if running == Some((mode.width, mode.height, mode.fps)) {
            return true;
        }
        if running.is_some() {
            // A V4L2 device can only be opened once; release it before the
            // mode change. `sync_camera_order` restores the Webcam slot after
            // the start.
            self.media_hub.stop(&source_id);
        }

        let config =
            VideoSourceConfig::camera(camera_id).with_mode(mode.width, mode.height, mode.fps);
        match self.media_hub.start(&source_id, config) {
            Ok(_) => {
                eprintln!(
                    "ui: INFO webcam {camera_id} started at {}x{}@{}",
                    mode.width, mode.height, mode.fps
                );
                true
            }
            Err(error) => {
                self.record_error(format!("camera {camera_id} failed: {error}"));
                false
            }
        }
    }

    /// Start the saved cameras that are enumerated now, keeping the rest
    /// pending for a later scan.
    ///
    /// A saved mode that the camera no longer advertises is normalised by
    /// [`plan_restore`] and rewritten, so the next launch starts directly in
    /// the nearest still-valid mode.
    fn restore_pending_cameras(&mut self) {
        if self.media_restore_pending.is_empty() {
            return;
        }
        let plan = plan_restore(
            &self.media_restore_pending,
            &self.media_settings.modes,
            &self.media_cameras,
        );
        self.media_restore_pending = plan.pending;
        if plan.starts.is_empty() {
            return;
        }

        let mut normalised = false;
        for start in plan.starts {
            if self.media_settings.mode_for(&start.camera_id) != Some(start.mode) {
                normalised |= self.media_settings.set_mode(&start.camera_id, start.mode);
            }
            self.start_camera_source(&start.camera_id, start.mode);
        }
        self.sync_camera_order();
        if normalised {
            self.save_media_settings();
        }
    }

    /// Make the hub's camera order match the persisted selection.
    ///
    /// Only camera-like sources move (files keep their slots) and no pipeline
    /// restarts, so the Webcam module's `camera` prop follows immediately.
    fn sync_camera_order(&mut self) {
        let ids = self
            .media_settings
            .cameras
            .iter()
            .map(|camera_id| format!("camera:{camera_id}"))
            .collect::<Vec<_>>();
        self.media_hub.set_camera_order(&ids);
    }

    /// Persist the webcam selection; a write failure is reported, never fatal.
    fn save_media_settings(&mut self) {
        match self.media_settings.write_to(&self.media_settings_path) {
            Ok(()) => self.media_settings_error_logged = false,
            Err(error) => {
                if !self.media_settings_error_logged {
                    self.media_settings_error_logged = true;
                    self.record_error(format!("could not save the webcam selection: {error}"));
                }
            }
        }
    }

    fn drain_ui_actions(&mut self) {
        let mut refresh_ndi = false;
        let mut refresh_osc = false;
        for action in self.ui_state.take_actions() {
            if action.action == "ui/addModuleToGroup" {
                self.add_module_to_group(&action);
                continue;
            }
            // One-shot MIDI Learn targets one input and lives on the MIDI
            // client (the pump resolves it), so it never reaches the store.
            if action.action == "midi/learn" {
                match action.payload.get("inputId").and_then(Value::as_str) {
                    Some(input_id) => {
                        self.midi_client.arm_learn(input_id);
                    }
                    None => {
                        eprintln!("ui: WARN midi/learn without an inputId was ignored");
                    }
                }
                continue;
            }
            if action.action == "midi/cancelLearn" {
                self.midi_client.cancel_learn();
                continue;
            }
            let action_name = action.action.as_str();
            refresh_ndi |= action_name.starts_with("ndi/");
            refresh_osc |= action_name.starts_with("osc/");
            let removed_server_id = if action_name == "osc/removeServer" {
                action
                    .payload
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            };
            match self.store.dispatch(action_name, action.payload) {
                Ok(reply) => {
                    if action_name == "osc/createServer" {
                        match OscServerView::from_create_reply(&reply) {
                            Some(server) => {
                                eprintln!(
                                    "ui: INFO OSC listener {} on {}",
                                    server.title(),
                                    server.address_label()
                                );
                                self.osc_servers.push(server);
                            }
                            None => self.record_error(format!(
                                "osc/createServer returned an unexpected reply: {reply}"
                            )),
                        }
                    } else if let Some(id) = removed_server_id {
                        self.osc_servers.retain(|server| server.id != id);
                    }
                }
                Err(error) => self.record_error(format!("{action_name} failed: {error}")),
            }
        }
        // The panels read a cached view; reflect a user gesture immediately
        // instead of leaving the control stale until the next 1 Hz poll.
        if refresh_ndi {
            self.refresh_ndi_view();
        }
        if refresh_osc {
            self.refresh_osc_view();
        }
    }

    /// Rebuild the NDI panel view and restart its 1 Hz poll window.
    fn refresh_ndi_view(&mut self) {
        self.ndi_view = ndi_snapshot(&self.store);
        self.last_ndi_poll = Instant::now();
    }

    /// Rebuild the OSC panel view and restart its 1 Hz poll window.
    fn refresh_osc_view(&mut self) {
        self.osc_view = osc_snapshot(&self.store, &self.osc_servers);
        self.last_osc_poll = Instant::now();
    }

    /// Refresh the NDI/OSC/Plugins panel views at 1 Hz, only while their tab is
    /// visible (active in the strip, or docked/floating).
    fn poll_panel_views(&mut self, now: Instant) {
        self.refresh_media_library();
        if self.ui_state.is_tab_visible(SidePanelTab::Ndi)
            && now.saturating_duration_since(self.last_ndi_poll) >= PANEL_POLL_INTERVAL
        {
            self.refresh_ndi_view();
        }
        if self.ui_state.is_tab_visible(SidePanelTab::Osc)
            && now.saturating_duration_since(self.last_osc_poll) >= PANEL_POLL_INTERVAL
        {
            self.refresh_osc_view();
        }

        // The plugin runtime mutates outside the UI (hooks run and prop values
        // move every frame), so the panel reads a cached view. Reopening the
        // tab starts a fresh log window; a command refreshes immediately in
        // `drain_ui_commands`, so the 1 Hz poll only covers background churn.
        let visible = self.ui_state.is_tab_visible(SidePanelTab::Plugins);
        if visible {
            if !self.plugins_visible && self.plugins_seen {
                self.plugin_runtime.clear_logs();
            }
            self.plugins_seen = true;
            if !self.plugins_visible
                || now.saturating_duration_since(self.last_plugin_poll) >= PANEL_POLL_INTERVAL
            {
                self.refresh_plugin_view();
            }
        }
        self.plugins_visible = visible;
    }

    /// Rebuild the Plugins panel view and restart its 1 Hz poll window.
    fn refresh_plugin_view(&mut self) {
        self.plugin_view = plugin_snapshot(&self.plugin_runtime);
        self.last_plugin_poll = Instant::now();
    }

    /// Re-scan the media library when a watcher burst or the Rescan button
    /// asked, then decode a few missing thumbnails for the visible folder.
    ///
    /// The scan itself only runs on a coalesced request, never per frame; the
    /// thumbnail decode is capped per frame and cached, so scrolling a large
    /// folder does not touch the disk again.
    fn refresh_media_library(&mut self) {
        let mut dirty = self.library_view_dirty;
        if self.library_refresh.take().is_some() {
            self.drop_library_textures();
            self.media_library.rescan();
            dirty = true;
            eprintln!(
                "ui: INFO media library rescan: {} asset(s) in the {:?} profile",
                self.media_library.total_count(),
                self.media_library.profile()
            );
        }

        if self.ui_state.is_tab_visible(SidePanelTab::Library) {
            let kind = self.ui_state.library_kind;
            let folder = self.ui_state.library_folder.clone();
            let decoded = self.media_library.decode_thumbnails(kind, &folder, 4);
            if !decoded.is_empty() {
                dirty = true;
            }
            for (path, thumbnail) in decoded {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [thumbnail.width as usize, thumbnail.height as usize],
                    &thumbnail.rgba,
                );
                let handle = self.egui_ctx.load_texture(
                    format!("library:{path}"),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                if let Some(previous) = self.library_textures.insert(
                    path,
                    PreviewEntry {
                        handle,
                        width: thumbnail.width,
                        height: thumbnail.height,
                    },
                ) {
                    self.egui_ctx
                        .tex_manager()
                        .write()
                        .free(previous.handle.id());
                }
            }
        }

        if dirty {
            let textures: HashMap<String, PreviewTexture> = self
                .library_textures
                .iter()
                .map(|(path, entry)| {
                    (
                        path.clone(),
                        PreviewTexture {
                            id: entry.handle.id(),
                            width: entry.width,
                            height: entry.height,
                        },
                    )
                })
                .collect();
            self.library_view = self.media_library.view(&textures);
            self.library_view_dirty = false;
        }
    }

    /// Drop every uploaded Library thumbnail (a rescan/profile switch).
    fn drop_library_textures(&mut self) {
        let texture_ids: Vec<egui::TextureId> = self
            .library_textures
            .drain()
            .map(|(_, entry)| entry.handle.id())
            .collect();
        for id in texture_ids {
            self.egui_ctx.tex_manager().write().free(id);
        }
        self.library_view_dirty = true;
    }

    fn add_module_to_group(&mut self, action: &UiAction) {
        let Some(module_name) = action.payload.get("moduleName").and_then(Value::as_str) else {
            return;
        };
        let Some(group_id) = action.payload.get("groupId").and_then(Value::as_str) else {
            return;
        };
        match self.store.dispatch(
            "modules/makeActiveModule",
            serde_json::json!({ "moduleName": module_name }),
        ) {
            Ok(module) => {
                let module_id = module
                    .get("$id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if module_id.is_empty() {
                    self.record_error(format!("module {module_name} has no id"));
                    return;
                }
                if let Err(error) = self.store.dispatch(
                    "groups/ADD_MODULE_TO_GROUP",
                    serde_json::json!({ "groupId": group_id, "moduleId": module_id }),
                ) {
                    self.record_error(format!("addModuleToGroup failed: {error}"));
                } else {
                    self.ui_state.selected_module = Some(module_id);
                }
            }
            Err(error) => self.record_error(format!("makeActiveModule failed: {error}")),
        }
    }

    fn drain_ui_commands(&mut self, event_loop: &ActiveEventLoop) {
        for command in self.ui_state.take_commands() {
            match command {
                UiCommand::Quit => self.finish_and_exit(event_loop),
                UiCommand::OpenOutputWindow => {
                    if let Err(error) = self.open_output(event_loop) {
                        self.record_error(format!("open output window failed: {error}"));
                    }
                }
                UiCommand::CloseOutputWindow => self.close_output(),
                UiCommand::ToggleOutputFullscreen => self.toggle_output_fullscreen(),
                UiCommand::SaveSnapshot => self.save_snapshot(),
                UiCommand::SavePreset => self.save_preset(),
                UiCommand::FlushLogs => {}
                UiCommand::ShowFileDialog(action) => self.open_file_dialog(action),
                UiCommand::FileChosen { action, path } => {
                    self.apply_file_choice(action, &path);
                }
                UiCommand::CreateProfile(name) => self.create_project_profile(&name),
                UiCommand::SwitchProfile(name) => self.switch_project_profile(&name),
                UiCommand::RescanLibrary => self.library_refresh.request(),
                UiCommand::OpenMediaFolder => {
                    let media_root = self.media_root.clone();
                    self.open_folder(&media_root);
                }
                UiCommand::OpenLogsFolder => {
                    let logs = media_dialogs::logs_dir(&self.user_data);
                    if let Err(error) = std::fs::create_dir_all(&logs) {
                        self.record_error(format!(
                            "could not create the logs folder {}: {error}",
                            logs.display()
                        ));
                    } else {
                        self.open_folder(&logs);
                    }
                }
                UiCommand::SetAudioSource { source, device } => {
                    self.set_audio_source(&source, device.as_deref());
                }
                UiCommand::RefreshAudioDevices => self.refresh_audio_devices(),
                UiCommand::SetPluginsEnabled(enabled) => {
                    if let Err(error) = self.plugin_runtime.set_enabled(enabled) {
                        self.record_error(format!("plugin master switch failed: {error}"));
                    }
                    self.refresh_plugin_view();
                }
                UiCommand::SetPluginConsent(consented) => {
                    if let Err(error) = self.plugin_runtime.set_consented(consented) {
                        self.record_error(format!("plugin consent failed: {error}"));
                    }
                    self.refresh_plugin_view();
                }
                UiCommand::SetPluginEnabled { path, enabled } => {
                    if let Err(error) = self.plugin_runtime.set_plugin_enabled(&path, enabled) {
                        self.record_error(format!("plugin switch failed: {error}"));
                    }
                    self.refresh_plugin_view();
                }
                UiCommand::RescanPlugins => {
                    self.plugin_runtime.rescan();
                    self.refresh_plugin_view();
                }
                UiCommand::ReloadPlugins => {
                    self.plugin_runtime.reload();
                    self.refresh_plugin_view();
                }
                UiCommand::SetPluginProp { path, prop, value } => {
                    if let Err(error) = self.plugin_runtime.set_prop(&path, &prop, value) {
                        self.record_error(format!("plugin prop {prop} failed: {error}"));
                    }
                    self.refresh_plugin_view();
                }
                UiCommand::ClearPluginLogs => {
                    self.plugin_runtime.clear_logs();
                    self.refresh_plugin_view();
                }
            }
        }
    }

    /// Open the in-app picker at the active profile's folder for `action`.
    ///
    /// The dialog itself lives in `modv-ui` and is only state, so nothing here
    /// blocks the frame loop; the chosen path arrives as a
    /// [`UiCommand::FileChosen`] after the user confirms.
    fn open_file_dialog(&mut self, action: FileDialogAction) {
        let start = action
            .start_folder()
            .map(|folder| {
                media_profiles::profile_folder(&self.media_root, &self.active_profile, folder)
            })
            .unwrap_or_else(|| media_profiles::profile_dir(&self.media_root, &self.active_profile));
        let start = if start.is_dir() {
            start
        } else if self.media_root.is_dir() {
            self.media_root.clone()
        } else {
            self.user_data.clone()
        };
        self.ui_state.open_file_dialog(action, start);
    }

    /// Apply a confirmed picker path through the [`FilePicker`] seam.
    fn apply_file_choice(&mut self, action: FileDialogAction, path: &Path) {
        match media_dialogs::apply_file_action(self, action, path) {
            Ok(()) => eprintln!("ui: INFO {} {}", action.title(), path.display()),
            Err(error) => self.record_error(format!("{} failed: {error}", action.title())),
        }
    }

    /// Re-read the profile folders shown by the File menu.
    fn refresh_profiles(&mut self) {
        self.profiles = media_profiles::list_profiles(&self.media_root);
        if !self
            .profiles
            .iter()
            .any(|name| name == &self.active_profile)
        {
            self.profiles.push(self.active_profile.clone());
            self.profiles.sort_by_key(|name| name.to_lowercase());
        }
    }

    /// Persist the active profile and extra ISF folders.
    fn persist_project_settings(&mut self) -> Result<(), String> {
        self.project_settings.profile = self.active_profile.clone();
        self.project_settings.isf_dirs = self.extra_isf_dirs.clone();
        match self.project_settings.write_to(&self.project_settings_path) {
            Ok(()) => {
                self.project_settings_error_logged = false;
                Ok(())
            }
            Err(error) => {
                if !self.project_settings_error_logged {
                    self.project_settings_error_logged = true;
                    eprintln!(
                        "ui: WARN could not write {}: {error}",
                        self.project_settings_path.display()
                    );
                }
                Err(error)
            }
        }
    }

    /// Recompute the user ISF search folders and restart the watcher.
    fn rebuild_user_isf_dirs(&mut self) {
        self.user_isf_dirs =
            user_isf_dirs(&self.media_root, &self.active_profile, &self.extra_isf_dirs);
        self.media_watcher =
            MediaWatcher::for_profile(&self.media_root, &self.active_profile, &self.extra_isf_dirs);
    }

    /// Keep the compositor's image resolution on the shell's media root.
    ///
    /// The store holds the root (`media/setMediaDirectoryPath`); the compositor
    /// owns the actual `ImageTextures`, so it follows the store once at startup
    /// and whenever the root changes (a future media-directory picker).
    fn sync_media_root(&mut self) {
        let root = self.store.media_root().map(Path::to_path_buf);
        if root == self.applied_media_root {
            return;
        }
        if let (Some(compositor), Some(root)) = (self.compositor.as_mut(), root.as_deref()) {
            compositor.set_media_root(root);
        }
        self.applied_media_root = root;
    }

    /// Create `<userData>/media/<name>` with the Electron folder set and switch
    /// to it. A bad name reopens the modal with an inline error.
    fn create_project_profile(&mut self, name: &str) {
        match media_profiles::create_profile(&self.media_root, name) {
            Ok(root) => {
                eprintln!("ui: INFO created project profile {}", root.display());
                self.refresh_profiles();
                let profile = media_profiles::normalize_profile_name(name)
                    .unwrap_or_else(|_| DEFAULT_PROFILE.to_string());
                self.switch_project_profile(&profile);
            }
            Err(error) => {
                self.ui_state.profile_name = name.to_string();
                self.ui_state.profile_error = Some(error.clone());
                self.ui_state.show_profile_dialog = true;
                self.record_error(format!("could not create profile {name:?}: {error}"));
            }
        }
    }

    /// Switch the active media profile: persist it, rescan the user ISF
    /// modules and restart the media watcher.
    ///
    /// `<userData>/scenes.json` is deliberately not touched: Scenes are shared
    /// by both front ends and stay authoritative across profile switches.
    fn switch_project_profile(&mut self, name: &str) {
        let name = match media_profiles::normalize_profile_name(name) {
            Ok(name) => name,
            Err(error) => {
                self.record_error(format!("could not switch profile: {error}"));
                return;
            }
        };
        if !media_profiles::profile_dir(&self.media_root, &name).is_dir() {
            self.record_error(format!("project profile {name:?} does not exist"));
            return;
        }
        self.active_profile = name.clone();
        if let Err(error) = self.persist_project_settings() {
            self.record_error(format!("could not save the project choice: {error}"));
        }
        // Every profile imports its loose Scene documents once; a profile first
        // selected after launch imports now. The media root itself is shared,
        // so re-dispatching it is a no-op unless the tree moved.
        dispatch_media_root(&mut self.store, &self.media_root);
        // Video-file pipelines belong to the old profile's tree: release them
        // before the media root/asset index changes. The next reconcile starts
        // the clips the new profile can resolve.
        self.video_sources.stop_all(&mut self.media_hub);
        crate::media_import::import_profile_documents_once(
            &mut self.store,
            &self.media_root,
            &self.active_profile,
            &mut self.project_settings,
            &self.project_settings_path,
        );
        self.refresh_profiles();
        self.rebuild_user_isf_dirs();
        // The Library index belongs to the active profile: drop the old scan
        // and thumbnails, then index the new folders on the next frame.
        self.media_library.set_profile(self.active_profile.clone());
        self.drop_library_textures();
        self.library_refresh.request();
        // The active profile also owns a `plugin/` folder; point discovery at
        // the new profile (a rescan keeps loaded scripts whose paths survive).
        self.plugin_runtime.set_roots(vec![
            media_profiles::profile_folder(&self.media_root, &self.active_profile, "plugin"),
            self.user_data.join("plugins"),
        ]);
        self.rescan_modules();
        eprintln!(
            "ui: INFO switched project profile to {name:?} ({} user ISF folder(s))",
            self.user_isf_dirs.len()
        );
    }

    fn open_folder(&mut self, path: &Path) {
        if let Err(error) = media_dialogs::open_external(path) {
            self.record_error(error);
        }
    }

    /// Switch native audio capture live.
    ///
    /// The replacement feed opens before the current one stops, so a bad
    /// selection (unplugged device, busy PipeWire node) leaves the running
    /// capture untouched and only reports the error. A successful choice is
    /// persisted to `<userData>/native-audio.json` for the next launch.
    fn set_audio_source(&mut self, source: &str, device: Option<&str>) {
        let selection = match source {
            "pipewire" => AudioSourceSelection::Pipewire,
            "input" => AudioSourceSelection::Input,
            other => {
                let error = format!("unknown audio source {other:?} (expected pipewire or input)");
                self.audio_switch_error = Some(error.clone());
                self.record_error(error);
                return;
            }
        };
        let requested_device = device.map(str::to_string);
        let opened = crate::audio::open_feed(selection, requested_device.as_deref());
        let device_note = requested_device
            .as_deref()
            .map(|device| format!(" (device {device})"))
            .unwrap_or_default();
        match crate::audio_settings::decide_switch(self.audio_feed.is_some(), opened) {
            crate::audio_settings::SwitchDecision::Replace(feed) => {
                let sample_rate = feed.sample_rate();
                if let Some(mut previous) = self.audio_feed.replace(feed) {
                    previous.stop();
                }
                self.audio_capture_rate.opened(sample_rate);
                self.audio_selection = selection;
                self.audio_device = requested_device;
                self.audio_switch_error = None;
                self.audio_error_logged = false;
                if let Err(error) = crate::audio_settings::AudioSettings::from_selection(
                    selection,
                    self.audio_device.clone(),
                )
                .write_to(&self.audio_settings_path)
                {
                    self.record_error(format!("could not save the audio selection: {error}"));
                }
                eprintln!(
                    "ui: INFO audio source {} owns capture{} at {sample_rate} Hz",
                    selection.as_str(),
                    device_note
                );
            }
            crate::audio_settings::SwitchDecision::Keep { error }
            | crate::audio_settings::SwitchDecision::Silent { error } => {
                let message = format!(
                    "audio source {}{device_note} could not start: {error}",
                    selection.as_str()
                );
                eprintln!(
                    "ui: WARN {message}{}",
                    if self.audio_feed.is_some() {
                        "; keeping the previous source"
                    } else {
                        "; engine stays silent"
                    }
                );
                self.audio_switch_error = Some(message.clone());
                self.record_error(message);
            }
        }
    }

    /// Re-enumerate cpal inputs for the Audio panel.
    ///
    /// Enumeration probes every ALSA/JACK/OSS backend and is both slow and
    /// noisy on hosts without a configured server, so it runs on demand (the
    /// panel's Refresh button) rather than once a second. A periodic poll
    /// flooded stderr with backend errors and exhausted pthread keys.
    fn refresh_audio_devices(&mut self) {
        self.audio_devices =
            crate::audio_settings::device_views(&modv_media::audio::capture::input_devices());
    }

    /// Answer at most one queued Gallery/Scene hover preview.
    ///
    /// The engine's isolated preview renderers already exist; this method
    /// uploads their RGBA result into an egui texture owned by the shell and
    /// keeps a bounded LRU. The UI reads the texture id from the next
    /// snapshot, so there is no IPC and no per-frame readback for a tile that
    /// is merely visible (only the hovered tile polls).
    fn answer_preview_requests(&mut self) {
        let requests = self.ui_state.take_preview_requests();
        let Some(request) = requests.into_iter().next_back() else {
            return;
        };

        let key = match &request {
            PreviewRequest::Module { module_name, .. } => gallery_preview_key(module_name),
            PreviewRequest::Scene {
                scene_id,
                row_index,
            } => scene_preview_key(scene_id, *row_index),
        };

        let result = {
            let Some(compositor) = &mut self.compositor else {
                return;
            };
            match request {
                PreviewRequest::Module {
                    module_name,
                    with_output,
                } => {
                    let registry = self.store.registry_arc();
                    compositor
                        .render_module_preview(
                            registry.as_ref(),
                            &module_name,
                            256,
                            144,
                            with_output,
                        )
                        .map(|pixels| (pixels, 256u32, 144u32))
                }
                PreviewRequest::Scene {
                    scene_id,
                    row_index,
                } => crate::scene_preview::render_scene_preview(
                    compositor,
                    &self.store,
                    &scene_id,
                    row_index,
                    320,
                    180,
                )
                .map(|pixels| (pixels, 320u32, 180u32)),
            }
        };

        match result {
            Ok((pixels, width, height)) => self.upload_preview(key, pixels, width, height),
            Err(error) => {
                let count = self.preview_error_counts.entry(key.clone()).or_insert(0);
                *count += 1;
                if *count == 1 || count.is_multiple_of(10) {
                    eprintln!("ui: WARN preview {key} failed (attempt {count}): {error}");
                }
            }
        }
    }

    /// Sync the Scenes grid's row layer monitors.
    ///
    /// Each row with a live (non-ghost, non-Gallery) Group gets a monitor
    /// entry; an entry whose Group changed or vanished frees its egui
    /// texture immediately (Scene applies mint fresh Group ids, so the old
    /// snapshot must never be sampled as if current). The compositor drops
    /// the GPU-side snapshot with the Group on prune/resize/reset, and no
    /// renderer instance is ever disposed here — the monitor only copies
    /// compositor-owned textures, so the shared ISF/2D/Point10 state is
    /// untouched.
    ///
    /// One row's snapshot refreshes per frame, chosen by
    /// [`modv_ui::panels::row_monitor::row_monitor_refresh_row`]: every row
    /// stays inside a slow ~3 fps band, so 6+ rows never cost real render
    /// time (the copy is a GPU-GPU blit at output size, no readback).
    fn refresh_row_monitors(&mut self) {
        let live = live_row_groups(self.store.state());
        if live.is_empty() {
            if !self.row_monitors.is_empty() {
                self.free_all_row_monitors();
            }
            return;
        }

        // Sync the row map first: rows whose Group went away or changed drop
        // their entry (and egui texture) right away.
        self.row_monitors.retain(|row, entry| {
            let keep = live
                .iter()
                .any(|(live_row, group)| live_row == row && entry.group_id == *group);
            if !keep
                && let Some(renderer) = &mut self.egui_renderer
                && let Some(texture) = entry.texture
            {
                renderer.free_texture(&texture);
            }
            keep
        });

        let Some(compositor) = &mut self.compositor else {
            return;
        };
        let (width, height) = compositor.main_texture_size();
        if width == 0 || height == 0 {
            return;
        }
        for (row, group_id) in &live {
            match self.row_monitors.get_mut(row) {
                Some(entry) if entry.group_id == *group_id => {
                    if entry.width != width || entry.height != height {
                        // The compositor was resized: the old snapshot is a
                        // different size and its native texture must be
                        // re-registered from the new view.
                        if let Some(renderer) = &mut self.egui_renderer
                            && let Some(texture) = entry.texture
                        {
                            renderer.free_texture(&texture);
                        }
                        entry.texture = None;
                        entry.width = width;
                        entry.height = height;
                    }
                }
                Some(entry) => {
                    if let Some(renderer) = &mut self.egui_renderer
                        && let Some(texture) = entry.texture
                    {
                        renderer.free_texture(&texture);
                    }
                    *entry = RowMonitorEntry {
                        group_id: group_id.clone(),
                        texture: None,
                        width,
                        height,
                    };
                }
                None => {
                    self.row_monitors.insert(
                        *row,
                        RowMonitorEntry {
                            group_id: group_id.clone(),
                            texture: None,
                            width,
                            height,
                        },
                    );
                }
            }
        }

        // At most one row's monitor refreshes on this frame.
        let Some(index) = modv_ui::panels::row_monitor_refresh_row(self.frames, live.len()) else {
            return;
        };
        let Some((row, group_id)) = live.get(index) else {
            return;
        };
        if !compositor.refresh_row_monitor(group_id) {
            return;
        }
        let Some(view) = compositor.row_monitor_view(group_id) else {
            return;
        };
        let Some(engine) = &self.engine else {
            return;
        };
        let Some(renderer) = &mut self.egui_renderer else {
            return;
        };
        if let Some(entry) = self.row_monitors.get_mut(row) {
            entry.texture = match entry.texture {
                Some(id) => {
                    renderer.update_egui_texture_from_wgpu_texture(
                        &engine.device,
                        &view,
                        wgpu::FilterMode::Linear,
                        id,
                    );
                    Some(id)
                }
                None => Some(renderer.register_native_texture(
                    &engine.device,
                    &view,
                    wgpu::FilterMode::Linear,
                )),
            };
        }
    }

    /// Drop every row monitor, freeing the egui textures.
    fn free_all_row_monitors(&mut self) {
        self.row_monitors.retain(|_, entry| {
            if let Some(renderer) = &mut self.egui_renderer
                && let Some(texture) = entry.texture
            {
                renderer.free_texture(&texture);
            }
            false
        });
        self.row_monitors.clear();
    }

    fn upload_preview(&mut self, key: String, mut pixels: Vec<u8>, width: u32, height: u32) {
        if pixels.len() != (width as usize) * (height as usize) * 4 {
            let count = self.preview_error_counts.entry(key.clone()).or_insert(0);
            *count += 1;
            if *count == 1 || count.is_multiple_of(10) {
                eprintln!(
                    "ui: WARN preview {key} returned {} bytes (attempt {count})",
                    pixels.len()
                );
            }
            return;
        }
        // The engine's preview readbacks are premultiplied RGBA; egui CPU
        // textures take straight alpha (`ColorImage::from_rgba_unmultiplied`
        // premultiplies on upload). Undo the multiplication instead of the
        // old flatten-to-opaque, which kept the scaled RGB with alpha forced
        // to 255 and turned transparent halos into dark fringes over light
        // panel backgrounds. GPU-side previews (the main output and the row
        // monitors) skip this entirely: they are registered as native wgpu
        // textures and egui-wgpu blends premultiplied content directly.
        modv_ui::un_premultiply_rgba(&mut pixels);
        let image =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &pixels);
        if let Some(entry) = self.preview_textures.get_mut(&key) {
            entry.handle.set(image, egui::TextureOptions::LINEAR);
        } else {
            let handle =
                self.egui_ctx
                    .load_texture(key.clone(), image, egui::TextureOptions::LINEAR);
            self.preview_textures.insert(
                key.clone(),
                PreviewEntry {
                    handle,
                    width,
                    height,
                },
            );
            self.preview_order.push_back(key);
        }
        while self.preview_order.len() > 32 {
            if let Some(old) = self.preview_order.pop_front() {
                self.preview_textures.remove(&old);
            }
        }
    }

    /// Capture the outgoing frame for a starting whole-scene transition.
    /// Rebuild the module registry after a user ISF change.
    fn rescan_modules(&mut self) {
        let registry = self
            .store
            .reload_modules(self.isf_dir.as_deref(), &self.user_isf_dirs);
        if let Some(compositor) = &mut self.compositor {
            compositor.set_registry(registry);
        }
        let count = self
            .store
            .registry_arc()
            .registered_modules()
            .as_object()
            .map_or(0, serde_json::Map::len);
        eprintln!("ui: INFO user ISF rescan registered {count} modules");
    }

    fn capture_pending_transition(&mut self) {
        if self.store.take_pending_transition_capture().is_none() {
            return;
        }
        let captured = self
            .compositor
            .as_mut()
            .is_some_and(Compositor::capture_transition_source);
        if !captured && self.store.fall_back_scene_transition_to_fade() {
            eprintln!("ui: WARN no outgoing frame to capture; Scene transition falls back to Fade");
        }
    }

    /// Resolve the whole-scene shader transition for this frame.
    fn prepare_scene_transition(&mut self) -> Option<SceneTransitionInfo> {
        let transition = self.store.performance_runtime().transition.clone()?;
        let effect = transition.shader_effect()?;
        let captured = self
            .compositor
            .as_ref()
            .is_some_and(Compositor::has_transition_capture);
        if !captured {
            if self.store.fall_back_scene_transition_to_fade() {
                eprintln!(
                    "ui: WARN lost the captured outgoing frame; Scene transition falls back to Fade"
                );
            }
            if let Some(compositor) = &mut self.compositor {
                compositor.clear_transition_capture();
            }
            return None;
        }
        let prepared = self
            .compositor
            .as_mut()
            .is_some_and(|compositor| compositor.prepare_scene_transition(&effect));
        if !prepared {
            if self.store.fall_back_scene_transition_to_fade() {
                eprintln!(
                    "ui: WARN the shader transition pipeline failed to build; Scene transition falls back to Fade"
                );
            }
            if let Some(compositor) = &mut self.compositor {
                compositor.clear_transition_capture();
            }
            return None;
        }
        Some(SceneTransitionInfo {
            effect,
            progress: transition.progress(),
            from_groups: transition
                .from
                .iter()
                .map(|(group_id, _)| group_id.clone())
                .collect(),
        })
    }

    fn build_snapshot(&self) -> EngineSnapshot {
        let state = self.store.state();
        let registry = self.store.registry_arc();
        let (width, height) = self
            .compositor
            .as_ref()
            .map(Compositor::main_texture_size)
            .unwrap_or((self.cli.width, self.cli.height));

        let tween_by_id: HashMap<String, TweenView> = state
            .tweens
            .as_array()
            .map(|tweens| {
                let bpm = self.store.bpm();
                tweens
                    .iter()
                    .filter_map(|tween| tween_view_from_json(tween, bpm))
                    .map(|view| (view.id.clone(), view))
                    .collect()
            })
            .unwrap_or_default();
        let modules: Vec<ModuleView> = state
            .modules
            .values()
            .map(|module| {
                let group_id = state
                    .groups
                    .iter()
                    .find(|group| group.modules.iter().any(|id| id == &module.id))
                    .map(|group| group.id.clone())
                    .unwrap_or_default();
                module_view(module, group_id, &registry, &tween_by_id, state)
            })
            .collect();

        let groups = state.groups.iter().map(group_view).collect();

        let registered = registered_modules(&registry);
        let scenes_state = self.store.scenes_state();
        let row_names = self.store.row_names();
        let disabled_cells = modv_engine::scene_rows::normalize_disabled_cells_for_scenes(
            scenes_state.performance.get("disabledCells"),
            &scenes_state.scenes,
            &row_names,
        );
        let performance = performance_view(
            &scenes_state.performance,
            self.store.performance_runtime().enabled,
            &row_names,
        );
        let timing_fallback = (performance.min_value, performance.max_value);
        let scenes = scenes_state
            .scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| {
                scene_view(scene, index, &row_names, &disabled_cells, timing_fallback)
            })
            .collect::<Vec<_>>();
        let applied_scene_id = performance.current_scene_id.clone();
        // The per-row live Scene ids come from the same live Group stamps that
        // feed `scenes/SET_ACTIVE_ROWS`; transition ghosts and bare Groups are
        // excluded by the accessor. The UI needs the Scene ids (not the JSON
        // delta) to mark each row's live cell while performance mode advances
        // rows independently.
        let active_scene_rows = self
            .store
            .live_scene_rows_by_row()
            .into_iter()
            .map(|(row, (scene_id, _group_id))| (row, scene_id))
            .collect();

        let audio = audio_view(
            self.store.bpm(),
            self.store.kick(),
            self.store.audio(),
            self.audio_capture_rate.get(),
        );
        let mut texture_sources = state
            .groups
            .iter()
            .map(|group| TextureSourceView {
                id: format!("group:{}", group.id),
                label: group.name.clone(),
                kind: "group".to_string(),
            })
            .collect::<Vec<_>>();
        texture_sources.extend(self.media_hub.statuses().into_iter().map(|source| {
            let kind = match source.kind {
                "camera" => "camera",
                "file" => "video",
                other => other,
            };
            TextureSourceView {
                id: source.source_id.clone(),
                label: format!("{} ({})", source.source_id, source.state),
                kind: kind.to_string(),
            }
        }));
        // NDI receivers are textures the compositor uploads as `ndi:<id>`
        // (the legacy `outputs/ADD_AUXILLARY` group); list them so the
        // Inspector's texture picker can bind one.
        texture_sources.extend(
            self.ndi_view
                .receivers
                .iter()
                .map(|receiver| TextureSourceView {
                    id: format!("ndi:{}", receiver.id),
                    label: format!("{} (NDI)", receiver.source_name),
                    kind: "ndi".to_string(),
                }),
        );
        texture_sources.extend(state.modules.values().map(|module| {
            let kind = match module
                .meta
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "isf" => "isf",
                "2d" => "two-d",
                "3d" | "point10" => "point10",
                _ => "isf",
            };
            TextureSourceView {
                id: format!("{kind}:{}", module.id),
                label: module.module_name.clone(),
                kind: kind.to_string(),
            }
        }));
        let input_links = state
            .input_links
            .values()
            .map(|link| {
                (
                    link.id.clone(),
                    InputLinkView {
                        input_id: link.id.clone(),
                        kind: format!("{:?}", link.kind).to_ascii_lowercase(),
                        location: link.location.clone(),
                        args: link.args.clone().map(serde_json::Value::Array),
                        source: link.source.clone(),
                        min: link.min,
                        max: link.max,
                        matcher: link.matcher.clone(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let media = MediaSnapshot {
            available: self.media_hub.is_available(),
            error: self.media_hub.init_error().map(str::to_string),
            screen: self.screen_view(),
            video_problems: self
                .video_sources
                .problems()
                .into_iter()
                .map(|problem| VideoSourceProblemView {
                    path: problem.path,
                    message: problem.message,
                })
                .collect(),
            cameras: self
                .media_cameras
                .iter()
                .cloned()
                .map(|camera| {
                    let mode = camera.max_mode();
                    // The picker shows the resolved selection: the saved mode
                    // when the device still advertises it, else the nearest one.
                    let selected = crate::media_settings::resolve_mode(
                        self.media_settings.mode_for(&camera.id),
                        &camera.caps,
                    );
                    let mut modes = camera
                        .caps
                        .iter()
                        .map(|mode| CameraModeView {
                            width: mode.width,
                            height: mode.height,
                            fps: mode.fps,
                        })
                        .collect::<Vec<_>>();
                    modes.sort_by_key(|mode| (mode.width, mode.height, mode.fps));
                    modes.dedup();

                    CameraView {
                        id: camera.id,
                        name: camera.name,
                        max_width: mode.map_or(0, |mode| mode.width),
                        max_height: mode.map_or(0, |mode| mode.height),
                        max_fps: mode.map_or(0, |mode| mode.fps),
                        modes,
                        selected: CameraModeView {
                            width: selected.width,
                            height: selected.height,
                            fps: selected.fps,
                        },
                    }
                })
                .collect(),
            sources: self
                .media_hub
                .statuses()
                .into_iter()
                .map(|source| MediaSourceView {
                    source_id: source.source_id,
                    kind: source.kind.to_string(),
                    index: source.index,
                    state: source.state.to_string(),
                    width: source.width,
                    height: source.height,
                    frames: source.frames,
                    dropped: source.dropped,
                    last_error: source.last_error,
                    // Only files have a user-facing transport (a camera has no
                    // timeline and Restart/Speed would be rejected).
                    transport: (source.kind == "file")
                        .then_some(source.transport)
                        .flatten()
                        .map(|transport| VideoTransportView {
                            paused: transport.paused,
                            playing: transport.playing,
                            position_ms: transport.position_ms,
                            duration_ms: transport.duration_ms,
                            looping: transport.looping,
                            eos: transport.eos,
                            seekable: transport.seekable,
                            playback_rate: transport.playback_rate,
                        }),
                })
                .collect(),
        };
        let mut gallery_previews = HashMap::new();
        let mut scene_previews = HashMap::new();
        for (key, entry) in &self.preview_textures {
            let texture = PreviewTexture {
                id: entry.handle.id(),
                width: entry.width,
                height: entry.height,
            };
            if key.starts_with("module:") {
                gallery_previews.insert(key.clone(), texture);
            } else {
                scene_previews.insert(key.clone(), texture);
            }
        }
        let row_monitors = self
            .row_monitors
            .iter()
            .map(|(row, entry)| {
                (
                    *row,
                    RowMonitorView {
                        group_id: entry.group_id.clone(),
                        texture: entry.texture.map(|id| PreviewTexture {
                            id,
                            width: entry.width,
                            height: entry.height,
                        }),
                    },
                )
            })
            .collect();
        let midi = MidiSnapshot {
            available: self.midi_client.available(),
            error: self.midi_client.error().map(str::to_string),
            learning: self.midi_client.learning(),
            last_learn: self.midi_client.last_learn(),
            devices: {
                let state_devices = state.midi.get("devices").and_then(Value::as_object);
                self.midi_client
                    .devices()
                    .iter()
                    .map(|device| {
                        let record = state_devices
                            .and_then(|devices| devices.get(&device.key))
                            .and_then(Value::as_object);
                        MidiDeviceView {
                            key: device.key.clone(),
                            name: device.name.clone(),
                            connected: device.connected,
                            listen_for_input: record
                                .and_then(|record| record.get("listenForInput"))
                                .and_then(Value::as_bool)
                                .unwrap_or(true),
                            listen_for_clock: record
                                .and_then(|record| record.get("listenForClock"))
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            cc_latch: record
                                .and_then(|record| record.get("ccLatch"))
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            note_on_latch: record
                                .and_then(|record| record.get("noteOnLatch"))
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        }
                    })
                    .collect()
            },
        };
        let health = EngineHealth {
            adapter: format!("{} ({})", self.adapter.name, self.adapter.device_type),
            backend: self.adapter.backend.clone(),
            last_error: self.last_error.clone(),
            frames_presented: self.frames,
            output_open: self.output.is_some(),
            audio_active: self.store.engine_audio_active(),
        };
        let audio_selection = audio_selection_view(
            self.audio_selection,
            self.audio_device.clone(),
            self.audio_switch_error.clone(),
            self.audio_capture_rate.get(),
        );

        EngineSnapshot {
            groups,
            modules,
            scenes,
            registered,
            performance,
            audio,
            resolution: [width, height],
            fps: self.fps,
            frame_ms: self.frame_ms,
            scene_rows: self.store.row_names(),
            applied_scene_id,
            active_scene_rows,
            health,
            gallery_previews,
            scene_previews,
            row_monitors,
            texture_sources,
            input_links,
            tweens: tween_by_id,
            expressions: expression_views(state),
            media,
            library: self.library_view.clone(),
            midi,
            plugins: self.plugin_view.clone(),
            ndi: self.ndi_view.clone(),
            osc: self.osc_view.clone(),
            audio_devices: self.audio_devices.clone(),
            audio_selection,
            project: ProjectView {
                active_profile: self.active_profile.clone(),
                profiles: self.profiles.clone(),
            },
        }
    }

    /// Remember the main window's current logical placement.
    fn remember_main_window(&mut self) {
        let Some(window) = &self.ui_window else {
            return;
        };
        let scale = window.scale_factor();
        let size = window.inner_size();
        let previous = self.window_state.main_window.unwrap_or_default();
        let position = window
            .outer_position()
            .ok()
            .map(|position| (f64::from(position.x) / scale, f64::from(position.y) / scale));
        let saved = SavedWindow {
            x: position.map(|value| value.0).unwrap_or(previous.x),
            y: position.map(|value| value.1).unwrap_or(previous.y),
            width: f64::from(size.width) / scale,
            height: f64::from(size.height) / scale,
            display_id: None,
            maximized: window.is_maximized(),
            fullscreen: window.fullscreen().is_some(),
        };
        if saved.is_usable() {
            self.window_state.main_window = Some(saved);
            self.window_state_dirty = true;
        }
    }

    /// Remember an output window's current logical placement.
    fn remember_output_window(&mut self) {
        let Some(output) = &self.output else {
            return;
        };
        let scale = output.window.scale_factor();
        let size = output.window.inner_size();
        let previous = self.window_state.output().copied().unwrap_or_default();
        let position = output
            .window
            .outer_position()
            .ok()
            .map(|position| (f64::from(position.x) / scale, f64::from(position.y) / scale));
        let saved = SavedWindow {
            x: position.map(|value| value.0).unwrap_or(previous.x),
            y: position.map(|value| value.1).unwrap_or(previous.y),
            width: f64::from(size.width) / scale,
            height: f64::from(size.height) / scale,
            display_id: None,
            maximized: output.window.is_maximized(),
            fullscreen: output.window.fullscreen().is_some(),
        };
        if saved.is_usable() {
            self.window_state.set_output(saved);
            self.window_state_dirty = true;
        }
    }

    fn save_window_state(&mut self) {
        self.remember_main_window();
        self.remember_output_window();
        match self.window_state.write(&self.window_state_path) {
            Ok(()) => {
                self.window_state_dirty = false;
                self.last_window_state_save = Instant::now();
            }
            Err(error) => {
                if !self.window_state_error_logged {
                    self.window_state_error_logged = true;
                    eprintln!(
                        "ui: WARN could not write {}: {error}",
                        self.window_state_path.display()
                    );
                }
            }
        }
    }

    fn sync_layout(&mut self) {
        let current = LayoutSettings::from_ui(&self.ui_state.layout);
        if current != self.layout_settings {
            self.layout_settings = current;
            self.layout_dirty = true;
        }
    }

    /// Flush the layout if the shell changed it.
    ///
    /// Best-effort like the window state: a read-only userData directory logs
    /// once and never blocks the frame loop or quit.
    fn save_layout(&mut self) {
        self.sync_layout();
        if !self.layout_dirty {
            return;
        }
        match self.layout_settings.write_to(&self.layout_settings_path) {
            Ok(()) => {
                self.layout_dirty = false;
                self.last_layout_save = Instant::now();
            }
            Err(error) => {
                self.layout_dirty = false;
                if !self.layout_error_logged {
                    self.layout_error_logged = true;
                    eprintln!(
                        "ui: WARN could not write {}: {error}",
                        self.layout_settings_path.display()
                    );
                }
            }
        }
    }

    fn record_error(&mut self, message: String) {
        eprintln!("ui: ERROR {message}");
        self.last_error = Some(message);
    }

    fn save_preset(&mut self) {
        let path = self
            .preset_path
            .clone()
            .unwrap_or_else(|| self.user_data.join("native-preset.json"));
        match media_dialogs::save_preset_file(&path, &self.store) {
            Ok(()) => eprintln!("ui: INFO preset saved to {}", path.display()),
            Err(error) => {
                self.record_error(format!("could not save preset {}: {error}", path.display()))
            }
        }
    }

    fn save_snapshot(&mut self) {
        let Some(path) = &self.cli.state else {
            return;
        };
        match self.store.save_snapshot(path, self.frames) {
            Ok(()) => eprintln!("ui: INFO state snapshot flushed to {}", path.display()),
            Err(error) => self.record_error(format!("state snapshot failed: {error}")),
        }
    }

    fn finish_and_exit(&mut self, event_loop: &ActiveEventLoop) {
        if self.done {
            event_loop.exit();
            return;
        }
        self.done = true;
        let elapsed = self.start.elapsed().as_secs_f64().max(1e-6);
        let fps = self.frames as f64 / elapsed;
        self.fps = fps as f32;
        eprintln!(
            "native: frames={} fps={fps:.2} resolution={}x{} output={}",
            self.frames,
            self.preview_size.0,
            self.preview_size.1,
            if self.output.is_some() {
                "open"
            } else {
                "closed"
            }
        );
        self.save_snapshot();
        self.save_window_state();
        self.save_layout();
        event_loop.exit();
    }

    fn shutdown(&mut self) {
        if !self.done {
            self.save_snapshot();
        }
        self.save_window_state();
        self.save_layout();
        self.store.flush_link_deltas();
        self.store.shutdown_ndi();
        self.video_sources.stop_all(&mut self.media_hub);
        self.media_hub.stop_all();
        // Release the portal session (its PipeWire node dies with the D-Bus
        // connection) and stop waiting for a dialog nobody can answer.
        self.screen_answers.clear();
        if let Some(handle) = self.screen_share.stop() {
            close_screen_stream(handle);
        }
        self.midi_client.stop();
        self.plugin_runtime.shutdown();
        if let Some(mut feed) = self.audio_feed.take() {
            feed.stop();
        }
        self.audio_capture_rate.stopped();
        self.close_output();
    }
}

/// Close a portal session off the UI/render thread.
///
/// `PortalStream::close` waits (bounded by the portal's close timeout) for the
/// node to vanish, which must never hitch a frame; a plain `drop` closes the
/// session in the background without waiting. The short-lived thread only owns
/// the handle.
fn close_screen_stream<H: ScreenStream>(handle: H) {
    let spawned = std::thread::Builder::new()
        .name("modv-portal-close".to_string())
        .spawn(move || Box::new(handle).close());

    if let Err(error) = spawned {
        // The failed spawn dropped the closure, and with it the handle:
        // `PortalStream`'s `Drop` still tears the session down in the
        // background.
        eprintln!("ui: WARN could not close the portal session on a worker thread: {error}");
    }
}

impl From<ScreenShareKind> for ScreenShareKindView {
    fn from(kind: ScreenShareKind) -> Self {
        match kind {
            ScreenShareKind::Monitor => Self::Monitor,
            ScreenShareKind::Window => Self::Window,
        }
    }
}

/// The Audio panel/status-bar read model for one frame.
///
/// `capture_rate` is [`AudioCaptureRate::get`]: the live feed's negotiated
/// rate, or `None` when the engine is silent. It is the same value the Audio
/// panel banner and the BPM readouts render, so the tempo indicator can never
/// disagree with the picker about what rate analysis is running at.
fn audio_view(bpm: f64, kick: bool, audio: &AudioFrame, capture_rate: Option<u32>) -> AudioView {
    AudioView {
        bpm,
        kick,
        bass: audio.bass,
        mid: audio.mid,
        treble: audio.treble,
        sample_rate: capture_rate,
    }
}

/// The Audio panel's live selection, including the running feed's rate.
fn audio_selection_view(
    selection: AudioSourceSelection,
    device: Option<String>,
    error: Option<String>,
    capture_rate: Option<u32>,
) -> AudioSelectionView {
    AudioSelectionView {
        source: selection.as_str().to_string(),
        device,
        capture_sample_rate: capture_rate,
        error,
    }
}

/// The UI view of one `state.tweens` descriptor at the live tempo.
///
/// Decoding through [`TweenDescriptor`] keeps the engine's validation: an
/// invalid `keyframeTimes` list is dropped (sampling falls back to anime's even
/// spacing), and every field shares the descriptor's `createTween` defaults.
/// `cycle_seconds` comes from `effective_duration(bpm)`, the exact length the
/// engine pre-renders, so the curve editor's beat/seconds axis matches
/// playback.
fn tween_view_from_json(tween: &Value, bpm: f64) -> Option<TweenView> {
    let id = tween.get("id").and_then(Value::as_str)?.to_string();
    let descriptor: Option<TweenDescriptor> = serde_json::from_value(tween.clone()).ok();
    let field = |key: &str| tween.get(key);

    let data = descriptor
        .as_ref()
        .map(|descriptor| descriptor.data.clone())
        .unwrap_or_default();
    let keyframe_times = descriptor
        .as_ref()
        .and_then(TweenDescriptor::effective_keyframe_times)
        .map(<[f64]>::to_vec);
    let use_bpm = descriptor
        .as_ref()
        .map(|descriptor| descriptor.use_bpm)
        .or_else(|| field("useBpm").and_then(Value::as_bool))
        .unwrap_or(true);
    let cycle_seconds = descriptor
        .as_ref()
        .map(|descriptor| descriptor.effective_duration(bpm) / 1000.0)
        .unwrap_or_default();

    Some(TweenView {
        id,
        data,
        keyframe_times,
        duration: descriptor
            .as_ref()
            .map(|descriptor| descriptor.duration)
            .or_else(|| field("duration").and_then(Value::as_f64))
            .unwrap_or(1000.0),
        direction: descriptor
            .as_ref()
            .map(|descriptor| descriptor.direction.clone())
            .or_else(|| {
                field("direction")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "alternate".to_string()),
        easing: descriptor
            .as_ref()
            .map(|descriptor| descriptor.easing.clone())
            .or_else(|| field("easing").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| "linear".to_string()),
        looping: descriptor
            .as_ref()
            .map(|descriptor| descriptor.looping)
            .or_else(|| field("loop").and_then(Value::as_bool))
            .unwrap_or(true),
        use_bpm,
        bpm_division: descriptor
            .as_ref()
            .map(|descriptor| descriptor.bpm_division)
            .or_else(|| field("bpmDivision").and_then(Value::as_f64))
            .unwrap_or(16.0),
        duration_as_total_time: descriptor
            .as_ref()
            .map(|descriptor| descriptor.duration_as_total_time)
            .or_else(|| field("durationAsTotalTime").and_then(Value::as_bool))
            .unwrap_or(false),
        steps: descriptor
            .as_ref()
            .map(|descriptor| descriptor.steps)
            .or_else(|| field("steps").and_then(Value::as_f64))
            .unwrap_or(0.0),
        frames: field("frames")
            .and_then(Value::as_array)
            .map_or(0, |frames| frames.len() as u64),
        cycle_seconds,
    })
}

fn module_view(
    module: &ModuleInstance,
    group_id: String,
    registry: &crate::module_registry::ModuleRegistry,
    tween_by_id: &HashMap<String, TweenView>,
    state: &modv_engine::engine_state::EngineState,
) -> ModuleView {
    let meta = module.meta.as_object();
    let module_type = registry
        .meta_fields(&module.module_name)
        .and_then(|meta| meta.get("type").and_then(Value::as_str).map(str::to_string))
        .or_else(|| {
            module
                .meta
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let meta_string = |key: &str| {
        meta.and_then(|meta| meta.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let props = module
        .props
        .as_object()
        .map(|props| {
            props
                .iter()
                .map(|(name, value)| {
                    let schema = registry.prop_schema(&module.module_name, name);
                    let prop_type = schema
                        .as_ref()
                        .map(|schema| schema.type_.clone())
                        .unwrap_or_else(|| "text".to_string());
                    let input_id = module
                        .dollar_props
                        .as_object()
                        .and_then(|props| props.get(name))
                        .and_then(|entry| entry.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    // The data-type component sub-inputs (`${parent}-${key}`,
                    // `modules.js:91-113`) are exposed only when the engine
                    // actually minted them, so a preset-loaded module without
                    // its inputs shows no dangling rows.
                    let components = input_id
                        .as_deref()
                        .map(|parent| {
                            modv_engine::inputs::data_type_input_keys(&prop_type)
                                .iter()
                                .filter_map(|key| {
                                    let input_id = format!("{parent}-{key}");
                                    state.inputs.get(&input_id).is_some().then(|| {
                                        PropComponentView {
                                            key: (*key).to_string(),
                                            input_id,
                                        }
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    PropView {
                        name: name.clone(),
                        step: if prop_type == "int" {
                            Some(1.0)
                        } else {
                            Some(0.001)
                        },
                        prop_type,
                        value: value.clone(),
                        min: schema.as_ref().and_then(|schema| schema.min),
                        max: schema.as_ref().and_then(|schema| schema.max),
                        default: schema.as_ref().and_then(|schema| schema.default.clone()),
                        input_id: input_id.clone(),
                        components,
                        tween: input_id
                            .as_ref()
                            .and_then(|id| tween_by_id.get(id))
                            .cloned(),
                        options: Vec::new(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    ModuleView {
        id: module.id.clone(),
        module_name: module.module_name.clone(),
        group_id,
        enabled: meta
            .and_then(|meta| meta.get("enabled"))
            .map(js_truthy)
            .unwrap_or(true),
        alpha: meta
            .and_then(|meta| meta.get("alpha"))
            .and_then(Value::as_f64)
            .unwrap_or(1.0) as f32,
        alpha_input_id: meta_string("alphaInputId"),
        composite_operation: meta_string("compositeOperation")
            .unwrap_or_else(|| "normal".to_string()),
        composite_operation_input_id: meta_string("compositeOperationInputId"),
        module_type,
        props,
    }
}

/// The live Scene-row mapping for the row monitors: `(row, group id)` for
/// every row that a live (non-ghost, non-Gallery) Group renders, rows in
/// ascending order and at most one Group per row (the first in store order
/// wins, matching how the row-name remap stamps `sceneRow`).
///
/// Groups reach this list with `sceneRow` stamped either by a Scene apply or
/// by `scenes/setRowNames`'s remap; a Group with no row stamp is not tied to
/// a performance row and stays unmonitored.
fn live_row_groups(state: &modv_engine::engine_state::EngineState) -> Vec<(usize, String)> {
    let mut rows: Vec<(usize, String)> = state
        .groups
        .iter()
        .filter(|group| group.transition_ghost != Some(true) && group.name != GALLERY_GROUP_NAME)
        .filter_map(|group| group.scene_row.map(|row| (row, group.id.clone())))
        .collect();
    rows.sort_by_key(|(row, _)| *row);
    rows.dedup_by_key(|(row, _)| *row);
    rows
}

fn group_view(group: &Group) -> GroupView {
    GroupView {
        id: group.id.clone(),
        name: group.name.clone(),
        enabled: group.enabled == modv_engine::engine_state::GROUP_ENABLED,
        hidden: group.hidden,
        alpha: group.alpha as f32,
        alpha_input_id: group.alpha_input_id.clone(),
        composite_operation: group.composite_operation.clone(),
        composite_operation_input_id: group.composite_operation_input_id.clone(),
        inherit: group.inherit,
        clearing: group.clearing,
        pipeline: group.pipeline,
        module_ids: group.modules.clone(),
    }
}

fn expression_views(state: &modv_engine::engine_state::EngineState) -> Vec<ExpressionView> {
    state
        .expressions
        .values()
        .map(|assignment| ExpressionView {
            id: assignment.id.clone(),
            input_id: assignment.input_id.clone(),
            expression: assignment.expression.clone(),
            evaluate_every_frame: assignment.evaluate_every_frame,
        })
        .collect()
}

fn registered_modules(registry: &crate::module_registry::ModuleRegistry) -> Vec<ModuleMetaView> {
    let Value::Object(map) = registry.registered_modules() else {
        return Vec::new();
    };
    map.into_iter()
        .filter_map(|(name, entry)| {
            let meta = entry.get("meta").and_then(Value::as_object)?;
            let get = |key: &str| {
                meta.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            Some(ModuleMetaView {
                name,
                module_type: get("type"),
                category: get("category"),
                author: get("author"),
                version: get("version"),
                description: get("description"),
            })
        })
        .collect()
}

fn scene_view(
    scene: &Value,
    index: usize,
    row_names: &[String],
    disabled_cells: &std::collections::BTreeMap<String, Vec<usize>>,
    timing_fallback: (f64, f64),
) -> SceneView {
    let id = scene
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("scene-{index}"));
    let name = scene
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("Scene {}", index + 1));
    let enabled = scene.get("enabled").map(js_truthy).unwrap_or(true);
    // An override carries its own minimum *and* maximum (in the active timing
    // unit); the old view exposed only `maxSeconds`, so the Scenes dialog
    // opened with the minimum replaced by the maximum.
    let timing = if scene
        .get("timingOverride")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let min = scene
            .get("minSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(timing_fallback.0);
        let max = scene
            .get("maxSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(timing_fallback.1);
        Some((min, max))
    } else {
        None
    };
    let module_names = scene.get("modules").and_then(Value::as_object);
    let groups = scene.get("groups").and_then(Value::as_array);
    let row_indexes =
        groups.map(|groups| modv_engine::scene_rows::assign_scene_row_indexes(groups, row_names));
    let mut cells: Vec<SceneCellView> = if row_names.iter().any(|name| !name.is_empty()) {
        vec![SceneCellView::default(); row_names.len()]
    } else {
        Vec::new()
    };
    if let Some(groups) = groups {
        for (document_row, group) in groups.iter().enumerate() {
            if group.is_null() {
                continue;
            }
            let row = row_indexes
                .as_ref()
                .and_then(|indexes| indexes.get(document_row))
                .copied()
                .flatten()
                .unwrap_or(document_row);
            let names = group
                .get("modules")
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| {
                            let id = id.as_str()?;
                            module_names?
                                .get(id)?
                                .get("$moduleName")?
                                .as_str()
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if cells.len() <= row {
                cells.resize(row + 1, SceneCellView::default());
            }
            cells[row] = SceneCellView {
                row,
                module_names: names,
                disabled: modv_engine::scene_rows::is_scene_cell_disabled(disabled_cells, &id, row),
            };
        }
    }
    for (row, cell) in cells.iter_mut().enumerate() {
        cell.row = row;
    }
    SceneView {
        id,
        name,
        enabled,
        timing,
        cells,
    }
}

fn performance_view(config: &Value, enabled: bool, row_names: &[String]) -> PerformanceView {
    let number =
        |key: &str, fallback: f64| config.get(key).and_then(Value::as_f64).unwrap_or(fallback);
    let timing_unit = modv_ui::TimingUnit::parse(
        config
            .get("timingUnit")
            .and_then(Value::as_str)
            .unwrap_or("seconds"),
    );
    let (default_min, default_max) = modv_ui::default_timing_values(timing_unit);
    PerformanceView {
        enabled,
        current_scene_id: config
            .get("currentSceneId")
            .and_then(Value::as_str)
            .map(str::to_string),
        row_names: row_names.to_vec(),
        timing_unit,
        min_value: number("minSeconds", default_min),
        max_value: number("maxSeconds", default_max),
        transition_seconds: number("transitionSeconds", 2.0),
        allowed_transitions: config
            .get("allowedTransitions")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        // `sceneOrder`: the JS computed falls back to "in-order"; the engine
        // treats every value other than "random" as in order, so the view
        // normalises unknown values to what will actually happen.
        order: modv_ui::model::normalize_performance_order(
            config
                .get("order")
                .and_then(Value::as_str)
                .unwrap_or("in-order"),
        )
        .to_string(),
    }
}

/// Acquire a surface texture, reconfiguring a stale surface and skipping an
/// occluded one.
fn acquire_surface(
    surface: &mut SurfaceTarget,
    device: &wgpu::Device,
) -> Option<(wgpu::SurfaceTexture, bool)> {
    match surface.current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame) => Some((frame, false)),
        wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some((frame, true)),
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            surface.reconfigure(device);
            None
        }
        wgpu::CurrentSurfaceTexture::Occluded
        | wgpu::CurrentSurfaceTexture::Timeout
        | wgpu::CurrentSurfaceTexture::Validation => None,
    }
}

/// Apply a Scene by exact name or id after the store has seeded its defaults.
///
/// Used by `modv --apply-scene <name>` for reproducible performance demos and
/// P0 frame traces; a no-op when the query is absent.
fn apply_startup_scene(store: &mut Store, query: Option<&str>) {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return;
    };
    let Some(scene) = store
        .scenes_state()
        .scenes
        .iter()
        .find(|scene| {
            scene.get("id").and_then(Value::as_str) == Some(query)
                || scene.get("name").and_then(Value::as_str) == Some(query)
        })
        .cloned()
    else {
        eprintln!("ui: WARN --apply-scene: no Scene named or id {query:?}");
        return;
    };
    let Some(scene_id) = scene.get("id").and_then(Value::as_str) else {
        return;
    };
    match store.dispatch(
        "scenes/applyScene",
        serde_json::json!({ "sceneId": scene_id }),
    ) {
        Ok(_) => eprintln!(
            "ui: INFO applied Scene {:?} ({} groups)",
            scene
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(scene_id),
            store.state().groups.len()
        ),
        Err(error) => eprintln!("ui: WARN --apply-scene failed: {error}"),
    }
}

/// Build the native shell's store: restore the shared engine snapshot, then
/// hydrate/seed the Scene list beside it.
///
/// The native shell has no Electron supervisor, so an absent `--state` must
/// resolve to the supervisor's `<userData>/engine-state.json`
/// ([`crate::cli::native_state_path`]); otherwise the store has no Scene path
/// and `<userData>/scenes.json` — the authoritative list shared with
/// Electron — is never read, leaving only the seeded defaults.
fn build_native_store(
    explicit_state: Option<&Path>,
    user_data: &Path,
    isf_dir: Option<&Path>,
    user_isf_dirs: &[PathBuf],
) -> Store {
    let state_path = crate::cli::native_state_path(explicit_state, user_data);
    let mut store = Store::with_state_path_and_isf_dirs(Some(&state_path), isf_dir, user_isf_dirs);
    match store.restore_from_file(&state_path) {
        Ok(true) => eprintln!(
            "ui: INFO restored engine state from {} ({} groups, {} modules)",
            state_path.display(),
            store.state().groups.len(),
            store.state().modules.len()
        ),
        Ok(false) => eprintln!(
            "ui: INFO no state snapshot at {}; starting empty",
            state_path.display()
        ),
        Err(error) => {
            eprintln!(
                "ui: WARN could not load state snapshot {}: {error}",
                state_path.display()
            )
        }
    }

    if let Err(error) = store.dispatch("scenes/hydrateScenes", serde_json::json!({})) {
        eprintln!("ui: WARN could not hydrate Scenes: {error}");
    }
    if let Err(error) = store.dispatch("scenes/ensureDefaultScenes", serde_json::json!({})) {
        eprintln!("ui: WARN could not seed default Scenes: {error}");
    }
    store
}

/// Point the store's media root at the shell's `<userData>/media` tree.
///
/// Electron's renderer forwards the media manager path as
/// `media/setMediaDirectoryPath`; the native shell has no renderer, so it must
/// dispatch the action itself, or [`crate::texture_sources::ImageTextures`]
/// falls back to `$MODV_MEDIA_DIR` and Electron's default userData (the wrong
/// tree under `MODV_USER_DATA`).
fn dispatch_media_root(store: &mut Store, media_root: &Path) {
    if let Err(error) = store.dispatch(
        "media/setMediaDirectoryPath",
        serde_json::json!({ "path": media_root.to_string_lossy() }),
    ) {
        eprintln!(
            "ui: WARN could not point image resolution at {}: {error}",
            media_root.display()
        );
    }
}

/// User ISF folders under `<userData>/media`.
///
/// The legacy media manager keeps third-party shaders in
/// `<media>/<profile>/isf`; the active profile plus explicit extra folders
/// form the user corpus. The flat `<media>/isf` folder is kept for installs
/// created by older native builds. `MODV_USER_ISF_DIR` overrides all of it.
fn user_isf_dirs(media_root: &Path, profile: &str, extra: &[PathBuf]) -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os("MODV_USER_ISF_DIR").map(PathBuf::from) {
        return vec![dir];
    }
    media_profiles::user_isf_dirs(media_root, profile, extra)
}

/// Clamp a saved placement onto the primary monitor.
///
/// The legacy `resolveWindowBounds` does this at show time; without it a window
/// saved on a 4K display can sit off-screen on a smaller laptop.
fn clamp_saved_to_monitor(saved: SavedWindow, event_loop: &ActiveEventLoop) -> SavedWindow {
    let Some(monitor) = event_loop.primary_monitor() else {
        return saved;
    };
    let scale = monitor.scale_factor();
    let position = monitor.position();
    let size = monitor.size();
    let area = SavedWindow {
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
        ..Default::default()
    };
    if area.is_usable() {
        crate::window_state::clamp_bounds_to_area(saved, area)
    } else {
        saved
    }
}

/// The audio capture sources to try for a requested selection.
///
/// An explicit engine-owned `--audio-source` wins; the renderer default is
/// translated to the system monitor, then the default input. Startup order
/// (including a persisted Audio-panel choice) is
/// [`crate::audio_settings::initial_plan`].
#[cfg(test)]
fn native_audio_sources(selection: AudioSourceSelection) -> Vec<AudioSourceSelection> {
    crate::audio_settings::default_sources(selection)
}

/// Open the first working feed from the startup plan.
///
/// Returns the feed together with the selection that actually started, so the
/// Audio panel marks the live source even when a persisted device was
/// unplugged and a fallback took over.
fn open_native_audio(
    plan: &[(AudioSourceSelection, Option<String>)],
) -> Option<(AudioFeed, AudioSourceSelection, Option<String>)> {
    for (selection, device) in plan {
        match crate::audio::open_feed(*selection, device.as_deref()) {
            Ok(feed) => {
                let device_note = device
                    .as_deref()
                    .map(|device| format!(" (device {device})"))
                    .unwrap_or_default();
                eprintln!(
                    "ui: INFO audio source {} owns capture{} at {} Hz",
                    selection.as_str(),
                    device_note,
                    feed.sample_rate()
                );
                // The first `pump_audio` frame publishes the feed state; until
                // then the store reports silence (`audio_active == false`).
                return Some((feed, *selection, device.clone()));
            }
            Err(error) => {
                eprintln!(
                    "ui: WARN audio source {} unavailable: {error}",
                    selection.as_str()
                );
            }
        }
    }
    eprintln!("ui: WARN no native audio source started; engine runs silent");
    None
}

fn resolve_isf_dir(cli: &Cli) -> Option<PathBuf> {
    resolve_isf_dir_from(cli.isf_dir.as_deref())
}

fn resolve_isf_dir_from(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("MODV_ISF_DIR").map(PathBuf::from) {
        return Some(path);
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        PathBuf::from("resources/isf"),
        PathBuf::from("../resources/isf"),
        PathBuf::from("../../resources/isf"),
        manifest.join("../../resources/isf"),
        PathBuf::from("src/renderer/src/application/renderers/isf/modules/isf"),
        manifest.join("../../src/renderer/src/application/renderers/isf/modules/isf"),
    ];
    candidates.into_iter().find(|path| path.is_dir())
}

impl FilePicker for NativeApp {
    fn open_document(&mut self, path: &Path) -> Result<DocumentKind, String> {
        let kind = media_dialogs::open_document(&mut self.store, path)?;
        // Electron remembers the opened preset as the Save target. A Scene or
        // playlist is not a preset document, so saving over it would lose its
        // metadata; only a plain preset becomes the target.
        if kind == DocumentKind::Preset {
            self.preset_path = Some(path.to_path_buf());
        }
        Ok(kind)
    }

    fn save_preset_as(&mut self, path: &Path) -> Result<(), String> {
        media_dialogs::save_preset_file(path, &self.store)?;
        self.preset_path = Some(path.to_path_buf());
        Ok(())
    }

    fn export_playlist(&mut self, path: &Path) -> Result<(), String> {
        media_dialogs::export_playlist_file(path, &mut self.store)
    }

    fn import_playlist(&mut self, path: &Path) -> Result<(), String> {
        media_dialogs::import_playlist_file(path, &mut self.store)
    }

    fn add_isf_folder(&mut self, path: &Path) -> Result<(), String> {
        let path = path
            .canonicalize()
            .map_err(|error| format!("could not open {}: {error}", path.display()))?;
        if !path.is_dir() {
            return Err(format!("{} is not a folder", path.display()));
        }
        if !self.extra_isf_dirs.iter().any(|dir| dir == &path) {
            self.extra_isf_dirs.push(path);
            self.extra_isf_dirs.sort();
        }
        self.persist_project_settings()?;
        self.rebuild_user_isf_dirs();
        self.rescan_modules();
        Ok(())
    }
}

impl ApplicationHandler for NativeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.ui_window.is_some() || self.failed {
            return;
        }
        self.init(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let is_ui = self
            .ui_window
            .as_ref()
            .is_some_and(|window| window.id() == window_id);
        let is_output = self
            .output
            .as_ref()
            .is_some_and(|output| output.window.id() == window_id);
        if !is_ui && !is_output {
            return;
        }

        if is_ui
            && let Some(window) = self.ui_window.clone()
            && let Some(state) = &mut self.egui_state
        {
            let response = state.on_window_event(&window, &event);
            if response.repaint {
                window.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                if is_ui {
                    self.remember_main_window();
                    self.finish_and_exit(event_loop);
                } else {
                    self.remember_output_window();
                    self.close_output();
                }
            }
            WindowEvent::Destroyed => {
                if is_output {
                    self.close_output();
                }
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                if is_ui {
                    self.remember_main_window();
                } else {
                    self.remember_output_window();
                }
                if is_ui {
                    if let Some(engine) = &self.engine
                        && let Some(surface) = &mut self.ui_surface
                    {
                        surface.resize(&engine.device, size.width, size.height);
                    }
                } else if let Some(engine) = &self.engine {
                    if let Some(output) = &mut self.output {
                        output
                            .surface
                            .resize(&engine.device, size.width, size.height);
                    }
                    if let Some(compositor) = &mut self.compositor {
                        compositor.resize(size.width, size.height);
                    }
                    self.render_engine(None);
                    self.update_preview_texture();
                }
            }
            WindowEvent::RedrawRequested => {
                if is_ui {
                    self.draw_frame(event_loop);
                }
            }
            WindowEvent::Moved(_) => {
                if is_ui {
                    self.remember_main_window();
                } else {
                    self.remember_output_window();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers.insert(window_id, modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed || event.repeat || is_ui {
                    return;
                }
                let modifiers = self.modifiers.get(&window_id).copied().unwrap_or_default();
                let leaving = self
                    .output
                    .as_ref()
                    .is_some_and(|output| output.window.fullscreen().is_some());
                if is_fullscreen_toggle_key(&event.logical_key, modifiers)
                    || (leaving && is_fullscreen_exit_key(&event.logical_key))
                {
                    self.toggle_output_fullscreen();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if is_ui {
                    return;
                }
                let double = self
                    .double_click
                    .entry(window_id)
                    .or_default()
                    .register(Instant::now(), MouseButton::Left);
                if double {
                    self.toggle_output_fullscreen();
                }
            }
            WindowEvent::Occluded(false) => {
                if let Some(window) = &self.ui_window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.done || self.failed {
            return;
        }
        self.pacer.set_fps(self.store.target_fps());
        let now = Instant::now();
        if self.pacer.due(now) {
            if let Some(window) = &self.ui_window {
                window.request_redraw();
            }
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.pacer.deadline()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "modv-native-{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plugins")).expect("plugin temp dir");
        dir
    }

    #[test]
    fn plugin_snapshot_extracts_props_status_errors_and_capture() {
        use crate::plugin_runtime::PLUGIN_SETTINGS_FILENAME;
        use modv_ui::{PluginPropKindView, PluginStatus};

        let dir = plugin_temp_dir("plugin-snapshot");
        let plugins = dir.join("plugins");
        std::fs::write(plugins.join("a-broken.js"), "module.exports = {").expect("broken plugin");
        let probe = plugins.join("b-probe.js");
        std::fs::write(
            &probe,
            r#"
module.exports = {
  name: "Probe",
  author: "modV tests",
  version: "2.0.0",
  captureWidth: 4,
  captureHeight: 3,
  props: {
    gain: { type: "float", default: 0.25, min: 0, max: 1, step: 0.01 },
    mode: {
      type: "select",
      default: "a",
      options: [
        { label: "A", value: "a" },
        { label: "B", value: "b" },
      ],
    },
  },
  init({ log }) { log("info", "init"); },
  postProcessFrame() {},
};
"#,
        )
        .expect("probe plugin");

        let mut runtime =
            PluginRuntime::load(dir.join(PLUGIN_SETTINGS_FILENAME), vec![plugins.clone()]);
        runtime.start();

        // Gated: the scripts are listed, nothing is evaluated.
        let view = plugin_snapshot(&runtime);
        assert!(!view.enabled && !view.consented);
        assert!(!view.is_running());
        assert_eq!(view.summary(), "Not running");
        assert_eq!(view.plugins.len(), 2);
        assert!(view.plugins.iter().all(|plugin| !plugin.loaded));
        assert!(view.plugins.iter().all(|plugin| plugin.props.is_empty()));
        assert_eq!(view.loaded_count, 0);

        runtime.set_consented(true).expect("consent");
        runtime.set_enabled(true).expect("enable");
        let view = plugin_snapshot(&runtime);
        assert!(view.enabled && view.consented && view.is_running());
        assert_eq!(view.loaded_count, 1);
        assert_eq!(view.summary(), "1 of 2 plugins loaded");

        let broken = &view.plugins[0];
        assert_eq!(broken.title(), "a-broken");
        assert_eq!(broken.status(), PluginStatus::Error);
        assert!(broken.error.is_some(), "{:?}", broken.error);

        let info = &view.plugins[1];
        assert_eq!(info.name, "Probe");
        assert_eq!(info.author, "modV tests");
        assert_eq!(info.version, "2.0.0");
        assert_eq!(info.credit(), "modV tests · v2.0.0");
        assert_eq!(info.status(), PluginStatus::Loaded);
        assert_eq!(info.capture, Some([4, 3]));
        assert_eq!(info.capture_label().as_deref(), Some("capture 4×3"));
        assert_eq!(view.capture, Some([4, 3]));
        assert_eq!(view.capture_label().as_deref(), Some("capture 4×3"));

        let gain = info
            .props
            .iter()
            .find(|prop| prop.name == "gain")
            .expect("float prop");
        assert_eq!(gain.kind, PluginPropKindView::Float);
        assert_eq!(gain.value, serde_json::json!(0.25));
        assert_eq!(
            (gain.min, gain.max, gain.step),
            (Some(0.0), Some(1.0), Some(0.01))
        );

        let mode = info
            .props
            .iter()
            .find(|prop| prop.name == "mode")
            .expect("select prop");
        assert_eq!(mode.kind, PluginPropKindView::Select);
        assert_eq!(mode.selected_label(), Some("A"));

        assert!(
            view.logs
                .iter()
                .any(|line| line.contains("Probe") && line.contains("init")),
            "{:?}",
            view.logs
        );

        // A prop write is reflected by the next snapshot.
        runtime
            .set_prop(&probe.to_string_lossy(), "gain", serde_json::json!(0.75))
            .expect("set gain");
        let view = plugin_snapshot(&runtime);
        let gain = view.plugins[1]
            .props
            .iter()
            .find(|prop| prop.name == "gain")
            .expect("float prop");
        assert_eq!(gain.value, serde_json::json!(0.75));

        runtime.set_enabled(false).expect("disable");
        let view = plugin_snapshot(&runtime);
        assert!(!view.enabled);
        assert_eq!(view.plugins[1].status(), PluginStatus::Disabled);
        assert_eq!(view.loaded_count, 0);
        assert_eq!(view.capture, None);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_osc_listener_receives_into_the_panel_state_and_view() {
        use std::net::UdpSocket;

        let mut store = Store::new();
        let listener = start_osc_listener(&mut store, "127.0.0.1", 0).expect("listener");
        let servers = [listener.clone()];
        assert!(
            listener.port > 0,
            "port 0 binds an ephemeral port and the reply carries it"
        );

        // A minimal OSC message: "/modv/bass" + ",f" + the f32 argument.
        let socket = UdpSocket::bind("127.0.0.1:0").expect("client socket");
        let mut packet = Vec::new();
        packet.extend_from_slice(b"/modv/bass\0\0");
        packet.extend_from_slice(b",f\0\0");
        packet.extend_from_slice(&0.5_f32.to_be_bytes());
        socket
            .send_to(&packet, ("127.0.0.1", listener.port))
            .expect("send datagram");

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            store.drain_osc();
            let view = osc_snapshot(&store, &servers);
            if let Some(row) = view.addresses.iter().find(|row| row.path == "modv.bass") {
                assert_eq!(row.state_path, "osc.data.modv.bass");
                assert_eq!(row.value_label(), "0.5");
                break;
            }
            assert!(Instant::now() < deadline, "the OSC datagram never arrived");
            std::thread::sleep(Duration::from_millis(10));
        }

        store
            .dispatch("osc/removeServer", serde_json::json!({ "id": listener.id }))
            .expect("remove listener");
    }

    #[test]
    fn scene_views_flatten_group_module_names_and_null_holes() {
        let scene = serde_json::json!({
            "id": "s1",
            "name": "Test Scene",
            "enabled": 1,
            "timingOverride": true,
            "minSeconds": 4,
            "maxSeconds": 12,
            "groups": [
                {"modules": ["m1", "m2"]},
                null,
                {"modules": []}
            ],
            "modules": {
                "m1": {"$moduleName": "Bands"},
                "m2": {"$moduleName": "Bars"}
            }
        });
        let view = scene_view(
            &scene,
            0,
            &[],
            &std::collections::BTreeMap::new(),
            (8.0, 24.0),
        );
        assert_eq!(view.id, "s1");
        assert_eq!(view.name, "Test Scene");
        assert!(view.enabled);
        assert_eq!(
            view.timing,
            Some((4.0, 12.0)),
            "the override exposes its own minimum, not the maximum twice"
        );
        assert_eq!(view.cells.len(), 3);
        assert_eq!(view.cells[0].module_names, vec!["Bands", "Bars"]);
        // serde_json cannot represent a "hole" here; the null group has no
        // modules key and becomes an empty cell, which is the same UI state.
        assert!(view.cells[1].is_empty());
        assert!(view.cells[2].is_empty());
    }

    #[test]
    fn scene_view_places_cells_on_their_named_rows() {
        // User repro: `rowNames` is ["WEBCAM","Background","Foreground",
        // "Effects",""], while "Mycelium Bloom" stores only three positional
        // Groups (Background, Foreground, Effects) with no WEBCAM hole. The
        // snapshot must resolve each Group to its named row; mapping by
        // document position drew Background in the WEBCAM cell and made the
        // grid hover a row the Scene cannot preview.
        let scene = serde_json::json!({
            "id": "s1",
            "name": "Mycelium Bloom",
            "enabled": 1,
            "groups": [
                {"name": "Background", "modules": ["m-bg"]},
                {"name": "Foreground", "modules": []},
                {"name": "Effects", "modules": ["m-fx"]}
            ],
            "modules": {
                "m-bg": {"$moduleName": "Bands"},
                "m-fx": {"$moduleName": "Ball"}
            }
        });
        let row_names = vec![
            "WEBCAM".to_string(),
            "Background".to_string(),
            "Foreground".to_string(),
            "Effects".to_string(),
            "".to_string(),
        ];

        let mut disabled = std::collections::BTreeMap::new();
        disabled.insert("s1".to_string(), vec![3]);
        let view = scene_view(&scene, 0, &row_names, &disabled, (8.0, 24.0));

        assert_eq!(view.cells.len(), 5, "one cell per named row");
        assert!(
            view.cells[0].is_empty(),
            "WEBCAM has no Group; Background must not be drawn in its cell"
        );
        assert_eq!(view.cells[1].module_names, vec!["Bands"]);
        assert_eq!(view.cells[1].row, 1);
        assert!(!view.cells[1].disabled);
        assert!(
            view.cells[2].is_empty(),
            "the Foreground Group has no modules"
        );
        assert_eq!(view.cells[2].row, 2);
        assert_eq!(view.cells[3].module_names, vec!["Ball"]);
        assert_eq!(view.cells[3].row, 3);
        assert!(view.cells[3].disabled, "the saved per-cell flag is exposed");
        assert!(view.cells[4].is_empty());
    }

    #[test]
    fn performance_view_reads_global_timing_and_row_names() {
        let config = serde_json::json!({
            "enabled": true,
            "currentSceneId": "s1",
            "minSeconds": 4,
            "maxSeconds": 10,
            "transitionSeconds": 1.5,
            "allowedTransitions": ["Fade", "Wipe"]
        });
        let view = performance_view(&config, true, &["Background".to_string()]);
        assert!(view.enabled);
        assert_eq!(view.current_scene_id.as_deref(), Some("s1"));
        assert_eq!(view.row_names, vec!["Background"]);
        assert_eq!(view.timing_unit, modv_ui::TimingUnit::Seconds);
        assert_eq!(view.min_value, 4.0);
        assert_eq!(view.max_value, 10.0);
        assert_eq!(view.transition_seconds, 1.5);
        assert_eq!(view.allowed_transitions, vec!["Fade", "Wipe"]);
    }

    #[test]
    fn performance_view_reads_bar_timing_without_converting_the_edit_values() {
        let config = serde_json::json!({
            "timingUnit": "bars",
            "minSeconds": 4,
            "maxSeconds": 12,
        });
        let view = performance_view(&config, false, &[]);
        assert_eq!(view.timing_unit, modv_ui::TimingUnit::Bars);
        assert_eq!(view.min_value, 4.0);
        assert_eq!(view.max_value, 12.0);

        // Defaults follow the unit instead of carrying the seconds pair over.
        let defaults = performance_view(&serde_json::json!({"timingUnit": "bars"}), false, &[]);
        assert_eq!(
            (defaults.min_value, defaults.max_value),
            (4.0, 12.0),
            "bar defaults are 4/12 bars (8/24 s at 120 BPM)"
        );
    }

    #[test]
    fn performance_view_maps_the_scene_order() {
        // The JS `sceneOrder` computed falls back to "in-order" when the
        // saved config has no order.
        assert_eq!(
            performance_view(&serde_json::json!({}), false, &[]).order,
            "in-order"
        );
        assert_eq!(
            performance_view(&serde_json::json!({ "order": "random" }), false, &[]).order,
            "random"
        );
        // The engine treats anything but "random" as in order, so the view
        // must show the behaviour that will actually happen.
        assert_eq!(
            performance_view(&serde_json::json!({ "order": "shuffle" }), false, &[]).order,
            "in-order"
        );
    }

    #[test]
    fn native_audio_defaults_to_the_system_monitor_then_input() {
        assert_eq!(
            native_audio_sources(AudioSourceSelection::Renderer),
            vec![AudioSourceSelection::Pipewire, AudioSourceSelection::Input]
        );
        assert_eq!(
            native_audio_sources(AudioSourceSelection::Input),
            vec![AudioSourceSelection::Input]
        );
        assert_eq!(
            native_audio_sources(AudioSourceSelection::Pipewire),
            vec![AudioSourceSelection::Pipewire]
        );
    }

    #[test]
    fn an_empty_audio_plan_starts_no_feed() {
        assert!(
            open_native_audio(&[]).is_none(),
            "no plan entry means no capture attempt"
        );
    }

    #[test]
    fn the_live_capture_rate_follows_the_feed_and_clears_when_it_stops() {
        let mut rate = AudioCaptureRate::default();
        assert_eq!(rate.get(), None, "no feed, no rate");

        // `AudioFeed::new` is the device-free seam: it carries the negotiated
        // rate the capture-backed constructors get from cpal/PipeWire.
        let feed = AudioFeed::new(48_000.0);
        rate.opened(feed.sample_rate());
        assert_eq!(rate.get(), Some(48_000));

        rate.opened(44_100.0);
        assert_eq!(
            rate.get(),
            Some(44_100),
            "a device that genuinely negotiates 44.1 kHz is reported as such"
        );

        rate.stopped();
        assert_eq!(rate.get(), None, "a stopped feed has no negotiated rate");

        rate.opened(f32::NAN);
        assert_eq!(rate.get(), None, "a broken rate is unknown, not 0 Hz");
        rate.opened(0.0);
        assert_eq!(rate.get(), None);
        rate.opened(48_000.4);
        assert_eq!(rate.get(), Some(48_000), "f32 feed rates round to hertz");
    }

    #[test]
    fn the_snapshot_publishes_the_live_rate_to_the_audio_views_and_clears_it() {
        let feed = AudioFeed::new(48_000.0);
        let mut rate = AudioCaptureRate::default();
        rate.opened(feed.sample_rate());
        let frame = AudioFrame::default();

        let audio = audio_view(128.0, false, &frame, rate.get());
        assert_eq!(
            audio.sample_rate,
            Some(48_000),
            "the BPM indicator reads the feed's negotiated rate"
        );

        let selection =
            audio_selection_view(AudioSourceSelection::Pipewire, None, None, rate.get());
        assert_eq!(
            selection.capture_sample_rate,
            Some(48_000),
            "the active banner reads the same live rate for the monitor"
        );
        assert_eq!(
            selection.active_detail(&[]),
            "PipeWire monitor · 48 kHz · pipewire"
        );

        // `pump_audio` stops a failed feed and the cache clears with it: every
        // surface must drop the rate rather than keep showing the last one.
        rate.stopped();
        let silent = audio_view(128.0, false, &frame, rate.get());
        assert_eq!(silent.sample_rate, None);
        let silent_selection =
            audio_selection_view(AudioSourceSelection::Pipewire, None, None, rate.get());
        assert_eq!(silent_selection.capture_sample_rate, None);
        assert_eq!(
            silent_selection.active_detail(&[]),
            "PipeWire monitor · pipewire"
        );
    }

    #[test]
    fn preset_round_trip_restores_groups_and_modules() {
        let mut store = Store::new();
        let group = store
            .dispatch(
                "groups/createGroup",
                serde_json::json!({"name": "Round Trip"}),
            )
            .expect("create group");
        let group_id = group["id"].as_str().expect("group id").to_string();
        let module = store
            .dispatch(
                "modules/makeActiveModule",
                serde_json::json!({"moduleName": "Bands"}),
            )
            .expect("create module");
        let module_id = module["$id"].as_str().expect("module id").to_string();
        store
            .dispatch(
                "groups/ADD_MODULE_TO_GROUP",
                serde_json::json!({"groupId": group_id, "moduleId": module_id}),
            )
            .expect("add module");

        let path = std::env::temp_dir().join(format!(
            "modv-native-preset-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        media_dialogs::save_preset_file(&path, &store).expect("save preset");
        let mut restored = Store::new();
        media_dialogs::open_document(&mut restored, &path).expect("load preset");
        assert_eq!(restored.state().groups.len(), 1);
        assert_eq!(restored.state().modules.len(), 1);
        assert_eq!(restored.state().groups[0].name, "Round Trip");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn apply_startup_scene_resolves_names_and_ids() {
        let mut store = Store::new();
        store
            .dispatch("scenes/ensureDefaultScenes", serde_json::json!({}))
            .expect("seed defaults");
        let scene = store
            .scenes_state()
            .scenes
            .first()
            .cloned()
            .expect("at least one default Scene");
        let name = scene.get("name").and_then(Value::as_str).unwrap();
        apply_startup_scene(&mut store, Some(name));
        assert!(!store.state().groups.is_empty(), "Scene applied by name");
        let id = scene.get("id").and_then(Value::as_str).unwrap();
        apply_startup_scene(&mut store, Some(id));
        assert!(!store.state().groups.is_empty(), "Scene applied by id");
    }

    #[test]
    fn user_isf_dirs_follow_the_active_profile_and_extras() {
        let root = temp_dir("modv-user-isf");
        std::fs::create_dir_all(root.join("media/isf")).expect("flat isf");
        std::fs::create_dir_all(root.join("media/default/isf")).expect("default isf");
        std::fs::create_dir_all(root.join("media/live/isf")).expect("live isf");
        std::fs::create_dir_all(root.join("media/live/video")).expect("other folder");
        let extra = root.join("third-party");
        std::fs::create_dir_all(&extra).expect("extra isf");

        let dirs = user_isf_dirs(&root.join("media"), "live", std::slice::from_ref(&extra));

        assert!(dirs.contains(&root.join("media/isf")), "legacy flat folder");
        assert!(
            dirs.contains(&root.join("media/live/isf")),
            "active profile"
        );
        assert!(dirs.contains(&extra), "explicit extra folder");
        assert!(
            !dirs.contains(&root.join("media/default/isf")),
            "a switch must not keep scanning the previous profile"
        );
        assert!(!dirs.contains(&root.join("media/live/video")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn isf_dir_resolution_prefers_explicit_paths() {
        let explicit = Path::new("/tmp/isf-test");
        assert_eq!(
            resolve_isf_dir_from(Some(explicit)),
            Some(PathBuf::from(explicit))
        );
    }

    #[test]
    fn native_media_root_resolves_library_images_under_the_shell_tree() {
        // Regression: the native shell never dispatched
        // `media/setMediaDirectoryPath`, so `ImageTextures` fell back to
        // `$MODV_MEDIA_DIR`/Electron's default `<userData>/media` and a
        // `MODV_USER_DATA` profile's images failed to resolve.
        let root = temp_dir("modv-native-media-root");
        let user_data = root.join("user-data");
        let media_root = user_data.join("media");
        std::fs::create_dir_all(media_root.join("default/image")).expect("image folder");
        let image = media_root.join("default/image/poster.png");
        std::fs::write(&image, b"not decoded, only resolved").expect("image");

        let mut store = Store::new();
        dispatch_media_root(&mut store, &media_root);

        assert_eq!(store.media_root(), Some(media_root.as_path()));
        let mut images = crate::texture_sources::ImageTextures::new();
        images.set_media_root(store.media_root().expect("dispatched root"));
        assert_eq!(
            images.resolve_path("default/image/poster.png"),
            Some(image),
            "library-relative image paths resolve under <userData>/media"
        );
        assert!(
            images.resolve_path("default/image/missing.png").is_none(),
            "the shell root is not widened into a fallback search"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn simple_scene(id: &str, name: &str) -> Value {
        serde_json::json!({
            "id": id,
            "name": name,
            "enabled": true,
            "minSeconds": 6,
            "maxSeconds": 12,
            "groups": [],
            "modules": {},
            "inputs": { "inputs": {}, "inputLinks": {} },
            "expressions": { "assignments": {} },
            "tweens": [],
        })
    }

    fn write_scenes(dir: &Path, scenes: Vec<Value>) {
        let document = serde_json::json!({
            "version": 1,
            "defaultsSeeded": true,
            "defaultsVersion": modv_engine::default_scenes::DEFAULTS_VERSION,
            "seededDefaultScenes": [],
            "scenes": scenes,
            "performance": {},
        });
        std::fs::write(
            dir.join("scenes.json"),
            serde_json::to_vec(&document).expect("scenes json"),
        )
        .expect("write scenes.json");
    }

    fn scene_names(store: &Store) -> Vec<&str> {
        store
            .scenes_state()
            .scenes
            .iter()
            .filter_map(|scene| scene.get("name").and_then(Value::as_str))
            .collect()
    }

    #[test]
    fn native_store_reads_scenes_json_beside_the_default_state_path() {
        // Regression: the native shell used to pass `cli.state` straight
        // through, so without `--state` the store had no Scene path and only
        // the 62 seeded defaults appeared — the user's own Scenes were lost.
        let user_data = temp_dir("modv-native-scenes");
        write_scenes(
            &user_data,
            vec![
                simple_scene("scene-webcam", "WEBCAM"),
                simple_scene("scene-earth", "Earth"),
            ],
        );

        let store = build_native_store(None, &user_data, None, &[]);

        assert_eq!(
            scene_names(&store),
            ["WEBCAM", "Earth"],
            "the shared Scene list loads instead of defaults-only"
        );
        let _ = std::fs::remove_dir_all(&user_data);
    }

    #[test]
    fn native_store_restores_the_saved_row_names() {
        // The real install's `scenes.json` carries
        // `performance.rowNames: ["WEBCAM","Background","Foreground","Effects",""]`.
        // Building the native store must keep those names through hydration and
        // the default-seeding pass (and write them back unchanged).
        let user_data = temp_dir("modv-native-row-names");
        let names = serde_json::json!(["WEBCAM", "Background", "Foreground", "Effects", ""]);
        let document = serde_json::json!({
            "version": 1,
            "defaultsSeeded": true,
            "defaultsVersion": modv_engine::default_scenes::DEFAULTS_VERSION,
            "seededDefaultScenes": [],
            "scenes": [simple_scene("scene-webcam", "WEBCAM")],
            "performance": { "rowNames": names },
        });
        std::fs::write(
            user_data.join("scenes.json"),
            serde_json::to_vec(&document).expect("scenes json"),
        )
        .expect("write scenes.json");

        let store = build_native_store(None, &user_data, None, &[]);

        assert_eq!(
            store.row_names(),
            vec!["WEBCAM", "Background", "Foreground", "Effects", ""],
            "the native grid labels the saved rows"
        );
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(user_data.join("scenes.json")).expect("scenes.json"),
        )
        .expect("json");
        assert_eq!(
            saved["performance"]["rowNames"], names,
            "the startup seed pass must not drop the row names"
        );

        let _ = std::fs::remove_dir_all(&user_data);
    }

    #[test]
    fn native_store_keeps_an_explicit_state_path() {
        let root = temp_dir("modv-native-explicit");
        let explicit_dir = root.join("explicit");
        let user_data = root.join("user-data");
        std::fs::create_dir_all(&explicit_dir).expect("explicit dir");
        std::fs::create_dir_all(&user_data).expect("user data dir");
        write_scenes(&explicit_dir, vec![simple_scene("scene-a", "Explicit")]);
        write_scenes(&user_data, vec![simple_scene("scene-b", "UserData")]);

        let store = build_native_store(
            Some(&explicit_dir.join("engine-state.json")),
            &user_data,
            None,
            &[],
        );

        assert_eq!(
            scene_names(&store),
            ["Explicit"],
            "--state wins over the userData default"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn alpha_and_blend_input_ids_populate_module_and_group_views() {
        let module: ModuleInstance = serde_json::from_value(serde_json::json!({
            "$id": "module-1",
            "$moduleName": "Bands",
            "meta": {
                "alpha": 0.5,
                "enabled": true,
                "compositeOperation": "screen",
                "alphaInputId": "input-alpha",
                "compositeOperationInputId": "input-blend"
            },
            "props": { "u_bass": 0.3 },
            "$props": { "u_bass": { "id": "input-bass", "type": "float" } }
        }))
        .expect("module JSON");
        let registry = crate::module_registry::ModuleRegistry::builtin();
        let state = modv_engine::engine_state::EngineState::new();
        let view = module_view(
            &module,
            "group-1".to_string(),
            &registry,
            &HashMap::new(),
            &state,
        );
        assert_eq!(view.alpha_input_id.as_deref(), Some("input-alpha"));
        assert_eq!(
            view.composite_operation_input_id.as_deref(),
            Some("input-blend")
        );
        assert_eq!(view.composite_operation, "screen");

        let group: Group = serde_json::from_value(serde_json::json!({
            "id": "group-1",
            "name": "Effects",
            "hidden": false,
            "modules": [],
            "enabled": 1,
            "alpha": 1.0,
            "inherit": true,
            "inheritFrom": -1,
            "pipeline": false,
            "clearing": false,
            "compositeOperation": "normal",
            "alphaInputId": "group-alpha",
            "compositeOperationInputId": "group-blend"
        }))
        .expect("group JSON");
        let view = group_view(&group);
        assert_eq!(view.alpha_input_id.as_deref(), Some("group-alpha"));
        assert_eq!(
            view.composite_operation_input_id.as_deref(),
            Some("group-blend")
        );
    }

    #[test]
    fn live_row_groups_skip_ghosts_and_the_gallery_and_dedup_rows() {
        // The row monitors must follow exactly the Groups the composition
        // treats as live rows: a fading transition ghost and the Gallery
        // group are never a row's "current" layer, a Group without a row
        // stamp is not tied to a performance row, and when two live Groups
        // claim one row the first in store order wins.
        let group = |id: &str, name: &str, body: serde_json::Value| -> Group {
            let mut value = serde_json::json!({
                "id": id,
                "name": name,
                "hidden": false,
                "modules": [],
                "enabled": 1,
                "alpha": 1.0,
                "inherit": true,
                "inheritFrom": -1,
                "pipeline": false,
                "clearing": true,
                "compositeOperation": "normal",
            });
            if let (Some(object), Some(extra)) = (value.as_object_mut(), body.as_object()) {
                for (key, entry) in extra {
                    object.insert(key.clone(), entry.clone());
                }
            }
            serde_json::from_value(value).expect("group JSON")
        };
        let mut state = modv_engine::engine_state::EngineState::new();
        state.groups.push(group(
            "gallery",
            GALLERY_GROUP_NAME,
            serde_json::json!({ "sceneRow": 0 }),
        ));
        state.groups.push(group(
            "ghost",
            "Foreground",
            serde_json::json!({ "sceneRow": 1, "transitionGhost": true }),
        ));
        state
            .groups
            .push(group("unstamped", "Foreground", serde_json::json!({})));
        state.groups.push(group(
            "row-2-a",
            "Foreground",
            serde_json::json!({ "sceneRow": 2 }),
        ));
        state.groups.push(group(
            "row-2-b",
            "Foreground",
            serde_json::json!({ "sceneRow": 2 }),
        ));
        state.groups.push(group(
            "row-0",
            "Background",
            serde_json::json!({ "sceneRow": 0 }),
        ));

        assert_eq!(
            live_row_groups(&state),
            vec![(0, "row-0".to_string()), (2, "row-2-a".to_string())],
            "rows ascend; ghosts, the Gallery group and unstamped groups are \
             skipped; the first store-order group per row wins"
        );
    }

    #[test]
    fn a_freshly_created_group_view_carries_its_control_input_ids() {
        // `groups/createGroup` mints the six control inputs
        // (`groups.js:174-197`), so a fresh Group's view immediately exposes
        // the Alpha/Blend link rows in the Inspector.
        let mut store = Store::new();
        let group = store
            .dispatch(
                "groups/createGroup",
                serde_json::json!({ "name": "Effects" }),
            )
            .expect("createGroup dispatches");
        let group_id = group["id"].as_str().expect("id").to_string();

        let state_group = store.state().find_group(&group_id).expect("group");
        let view = group_view(state_group);
        let alpha = view.alpha_input_id.expect("alpha input id stamped");
        let blend = view
            .composite_operation_input_id
            .expect("compositeOperation input id stamped");
        assert_ne!(alpha, blend);
        assert!(store.state().inputs.contains_key(&alpha));
        assert!(store.state().inputs.contains_key(&blend));
    }

    #[test]
    fn component_inputs_populate_the_prop_views() {
        // makeActiveModule mints the data-type component sub-inputs
        // (`modules.js:91-113`); the Inspector lists each component as a
        // linkable row under its parent prop, like the JS ColorControl's
        // focused inputs.
        let mut store = Store::new();
        let module = store
            .dispatch(
                "modules/makeActiveModule",
                serde_json::json!({"moduleName": "Text"}),
            )
            .expect("makeActiveModule dispatches");
        let module_id = module["$id"].as_str().expect("id").to_string();

        let registry = crate::module_registry::ModuleRegistry::builtin();
        let engine_module = store.state().modules.get(&module_id).expect("module");
        let view = module_view(
            engine_module,
            String::new(),
            &registry,
            &HashMap::new(),
            store.state(),
        );

        let fill = view
            .props
            .iter()
            .find(|prop| prop.name == "fillColor")
            .expect("fillColor prop view");
        let parent = fill.input_id.clone().expect("parent input id");
        assert_eq!(
            fill.components,
            vec![
                PropComponentView {
                    key: "r".to_string(),
                    input_id: format!("{parent}-r"),
                },
                PropComponentView {
                    key: "g".to_string(),
                    input_id: format!("{parent}-g"),
                },
                PropComponentView {
                    key: "b".to_string(),
                    input_id: format!("{parent}-b"),
                },
                PropComponentView {
                    key: "a".to_string(),
                    input_id: format!("{parent}-a"),
                },
            ]
        );

        // A scalar prop owns no component rows.
        let size = view
            .props
            .iter()
            .find(|prop| prop.name == "size")
            .expect("size prop view");
        assert!(size.components.is_empty());

        // Every exposed component id resolves to a registered input, so the
        // link editor writes through the existing link path.
        for component in &fill.components {
            assert!(
                store.state().inputs.contains_key(&component.input_id),
                "{} is registered",
                component.input_id
            );
        }
    }

    #[test]
    fn expression_views_mirror_the_engine_assignments_in_store_order() {
        let mut store = Store::new();
        store
            .dispatch(
                "expressions/create",
                serde_json::json!({
                    "inputId": "input-alpha",
                    "expression": "value * 2",
                    "evaluateEveryFrame": true
                }),
            )
            .expect("create expression");
        store
            .dispatch(
                "expressions/create",
                serde_json::json!({
                    "inputId": "input-blend",
                    "expression": "value / 2"
                }),
            )
            .expect("create expression");

        let views = expression_views(store.state());
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].input_id, "input-alpha");
        assert_eq!(views[0].expression, "value * 2");
        assert!(views[0].evaluate_every_frame);
        assert_eq!(views[1].input_id, "input-blend");
        assert_eq!(views[1].expression, "value / 2");
        assert!(!views[1].evaluate_every_frame);
        assert!(!views[0].id.is_empty());
    }

    #[test]
    fn tween_views_carry_keyframes_and_the_engine_cycle_length() {
        let view = tween_view_from_json(
            &serde_json::json!({
                "id": "input-gain",
                "data": [[0], [0.5], [1]],
                "duration": 1000,
                "useBpm": false,
                "direction": "normal",
                "easing": "easeOutQuad",
                "loop": false,
                "steps": 2,
                "frames": [[0], [0], [0], [1], [1], [1]],
            }),
            120.0,
        )
        .expect("view");

        assert_eq!(view.id, "input-gain");
        assert_eq!(view.data, vec![vec![0.0], vec![0.5], vec![1.0]]);
        assert_eq!(view.keyframe_times, None);
        assert!(!view.use_bpm);
        assert!(!view.duration_as_total_time);
        assert_eq!(view.easing, "easeOutQuad");
        assert_eq!(view.direction, "normal");
        assert!(!view.looping);
        assert_eq!(view.steps, 2.0);
        assert_eq!(view.frames, 6);
        // 3+ rows divide the 1000 ms duration into one leg per row.
        assert!((view.cycle_seconds - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn tween_view_cycle_length_uses_the_live_bpm() {
        let document = serde_json::json!({
            "id": "input-gain",
            "data": [[0], [1]],
            "bpmDivision": 16,
        });

        // A BPM tween's cycle is (60 * 60 / bpm) * division / rows.
        let at_120 = tween_view_from_json(&document, 120.0).expect("view");
        assert!(at_120.use_bpm);
        assert!((at_120.cycle_seconds - 0.24).abs() < 1e-12);

        let at_60 = tween_view_from_json(&document, 60.0).expect("view");
        assert!((at_60.cycle_seconds - 0.48).abs() < 1e-12);

        // A falsy tempo falls back to 120 like the engine.
        let at_zero = tween_view_from_json(&document, 0.0).expect("view");
        assert!((at_zero.cycle_seconds - 0.24).abs() < 1e-12);
    }

    #[test]
    fn tween_view_keeps_valid_keyframe_times_and_drops_invalid_ones() {
        let valid = tween_view_from_json(
            &serde_json::json!({
                "id": "free",
                "data": [[0], [0.4], [1]],
                "keyframeTimes": [0, 0.35, 1],
                "useBpm": false,
                "duration": 1000,
            }),
            120.0,
        )
        .expect("view");
        assert_eq!(valid.keyframe_times, Some(vec![0.0, 0.35, 1.0]));

        for times in [
            serde_json::json!([0, 1]),
            serde_json::json!([0, 0.5, 0.5]),
            serde_json::json!([0.1, 0.5, 1]),
            serde_json::json!(null),
        ] {
            let view = tween_view_from_json(
                &serde_json::json!({
                    "id": "bad",
                    "data": [[0], [0.5], [1]],
                    "keyframeTimes": times,
                }),
                120.0,
            )
            .expect("view");
            assert_eq!(view.keyframe_times, None, "invalid times are dropped");
        }
    }

    #[test]
    fn tween_view_survives_a_decode_failure_and_needs_an_id() {
        assert!(tween_view_from_json(&serde_json::json!({"data": [[0], [1]]}), 120.0).is_none());

        // `data` that does not decode keeps the descriptor row for the UI's
        // compact controls rather than dropping the tween entirely.
        let view = tween_view_from_json(
            &serde_json::json!({
                "id": "broken",
                "data": [["nope"]],
                "duration": 500,
                "easing": "linear",
            }),
            120.0,
        )
        .expect("view");
        assert_eq!(view.id, "broken");
        assert!(view.data.is_empty());
        assert_eq!(view.duration, 500.0);
        assert_eq!(view.easing, "linear");
        assert_eq!(view.cycle_seconds, 0.0);
    }

    #[test]
    fn screen_share_kinds_map_to_the_media_panel_view() {
        assert_eq!(
            ScreenShareKindView::from(ScreenShareKind::Monitor),
            ScreenShareKindView::Monitor
        );
        assert_eq!(
            ScreenShareKindView::from(ScreenShareKind::Window),
            ScreenShareKindView::Window
        );
        assert_eq!(ScreenShareKindView::Monitor.label(), "monitor");
        assert_eq!(ScreenShareKindView::Window.label(), "window");
        assert_eq!(ScreenShareKindView::Monitor.as_str(), "monitor");
        assert_eq!(
            ScreenShareKind::for_window(true).as_str(),
            ScreenShareKindView::Window.as_str(),
            "the panel and the shell use the same spelling"
        );
    }
}
