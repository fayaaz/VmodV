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
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use modv_media::audio::engine::AudioFeed;
use modv_render::Engine;
use modv_render::surface::{SurfaceTarget, create_surface, create_surface_with_present_mode};
use modv_render::wgpu;
use modv_ui::model::ExpressionView;
use modv_ui::{
    AudioDeviceView, AudioSelectionView, AudioView, CameraView, EngineHealth, EngineSnapshot,
    GroupView, InputLinkView, MediaAction, MediaSnapshot, MediaSourceView, MidiDeviceView,
    MidiSnapshot, ModuleMetaView, ModuleView, PerformanceView, PreviewRequest, PreviewTexture,
    PropView, SceneCellView, SceneView, TextureSourceView, TweenView, UiAction, UiCommand, UiState,
    gallery_preview_key, scene_preview_key,
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
use crate::isf_watch::IsfWatcher;
use crate::media::MediaHub;
use crate::midi::MidiClient;
use crate::pacing::FramePacer;
use crate::report::AdapterSummary;
use crate::scene_transition::SceneTransitionInfo;
use crate::store_ops::Store;
use crate::window::{DoubleClickTracker, is_fullscreen_exit_key, is_fullscreen_toggle_key};
use crate::window_state::{SavedWindow, WINDOW_STATE_FILENAME, WindowState, user_data_dir};
use modv_engine::engine_state::{Group, ModuleInstance, js_truthy};
use modv_media::video::{TestPattern, VideoSourceConfig};

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

/// A tessellated egui frame waiting for the present pass.
struct PendingUiFrame {
    clipped: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
    free: Vec<egui::TextureId>,
}

/// The native application state.
pub struct NativeApp {
    cli: Cli,
    engine: Option<Engine>,
    adapter: AdapterSummary,
    store: Store,
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
    window_state: WindowState,
    window_state_path: PathBuf,
    window_state_dirty: bool,
    last_window_state_save: Instant,
    window_state_error_logged: bool,
    media_hub: MediaHub,
    last_media_health: Instant,
    /// Cached V4L2 enumeration; rescanning every frame is 50–500 µs per camera.
    media_cameras: Vec<modv_media::video::CameraInfo>,
    last_camera_scan: Instant,
    audio_feed: Option<AudioFeed>,
    audio_error_logged: bool,
    /// The live capture selection (the Audio panel's active choice).
    audio_selection: AudioSourceSelection,
    audio_device: Option<String>,
    audio_devices: Vec<AudioDeviceView>,
    /// The last failed live switch, shown in the Audio panel.
    audio_switch_error: Option<String>,
    audio_settings_path: PathBuf,
    midi_client: MidiClient,
    isf_dir: Option<PathBuf>,
    user_isf_dirs: Vec<PathBuf>,
    isf_watcher: Option<IsfWatcher>,
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
    fn new(cli: Cli, engine: Engine, adapter: AdapterSummary) -> Self {
        let start = Instant::now();
        let media_hub = MediaHub::new();

        let isf_dir = resolve_isf_dir(&cli);
        let user_data = user_data_dir();
        let user_isf_dirs = user_isf_dirs(&user_data);
        let mut store = build_native_store(
            cli.state.as_deref(),
            &user_data,
            isf_dir.as_deref(),
            &user_isf_dirs,
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

        if let Some(path) = &cli.preset
            && let Err(error) = load_preset_from(path, &mut store)
        {
            eprintln!("ui: WARN could not load preset {}: {error}", path.display());
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
        let audio_devices =
            crate::audio_settings::device_views(&modv_media::audio::capture::input_devices());
        let midi_client = MidiClient::new();
        let isf_watcher = IsfWatcher::new(&user_isf_dirs);
        if isf_watcher.is_some() {
            eprintln!("ui: INFO watching user ISF folders for changes");
        }
        if midi_client.available() {
            eprintln!("ui: INFO MIDI input available (hot-plug polled at 1 Hz)");
        } else if let Some(error) = midi_client.error() {
            eprintln!("ui: WARN MIDI unavailable: {error}");
        }

        let egui_ctx = egui::Context::default();
        modv_ui::theme::apply(&egui_ctx);
        let target_fps = store.target_fps();

        Self {
            bench_deadline: cli.bench_deadline(),
            cli,
            engine: Some(engine),
            adapter,
            store,
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
            window_state: WindowState::read(&user_data.join(WINDOW_STATE_FILENAME)),
            window_state_path: user_data.join(WINDOW_STATE_FILENAME),
            window_state_dirty: false,
            last_window_state_save: start,
            window_state_error_logged: false,
            media_hub,
            last_media_health: start,
            media_cameras: Vec::new(),
            last_camera_scan: start.checked_sub(Duration::from_secs(10)).unwrap_or(start),
            audio_feed,
            audio_error_logged: false,
            audio_selection,
            audio_device,
            audio_devices,
            audio_switch_error: None,
            audio_settings_path,
            midi_client,
            isf_dir,
            user_isf_dirs,
            isf_watcher,
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
        self.pump_audio(dt);
        self.store.drain_osc();
        self.midi_client.pump(&mut self.store);
        if self.store.scenes_tick_due() {
            let delta = dt.as_secs_f64() * 1000.0;
            if let Err(error) = self
                .store
                .dispatch("scenes/tick", serde_json::json!({ "delta": delta }))
            {
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
    fn pump_audio(&mut self, dt: Duration) {
        let Some(feed) = self.audio_feed.as_mut() else {
            return;
        };
        let snapshot = feed.frame(dt.as_secs_f32());
        let kick = feed.analyser().kick();
        let error = feed.take_error();
        self.store
            .set_engine_audio(&snapshot, kick, self.start.elapsed());
        let Some(error) = error else {
            return;
        };
        if !self.audio_error_logged {
            self.audio_error_logged = true;
            eprintln!("ui: WARN native audio capture stopped ({error}); engine is silent");
        }
        self.audio_feed = None;
        self.store.disable_engine_audio();
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
        if now.saturating_duration_since(self.last_media_health) >= Duration::from_secs(1) {
            self.media_hub.poll_health();
            self.last_media_health = now;
        }
        if now.saturating_duration_since(self.last_camera_scan) >= Duration::from_secs(1) {
            self.media_cameras = self.media_hub.list_cameras();
            self.last_camera_scan = now;
        }
        if self.isf_watcher.as_mut().is_some_and(IsfWatcher::pump) {
            self.rescan_modules();
        }
        self.drain_ui_commands(event_loop);
        if self.done {
            return;
        }

        let snapshot = self.build_snapshot();
        self.ui_state.sync_status(&snapshot);
        self.run_ui(&snapshot);
        self.drain_ui_actions();
        self.capture_pending_transition();
        if let Some(compositor) = &mut self.compositor {
            compositor.sync_media(&mut self.media_hub);
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
        self.answer_preview_requests();

        self.frames += 1;
        self.metric_frames += 1;
        let metric_elapsed = now.saturating_duration_since(self.last_metric);
        if metric_elapsed >= Duration::from_secs(1) {
            self.fps = self.metric_frames as f32 / metric_elapsed.as_secs_f32();
            self.frame_ms = 1000.0 / self.fps.max(0.001);
            self.metric_frames = 0;
            self.last_metric = now;
        }

        if self.window_state_dirty
            && now.saturating_duration_since(self.last_window_state_save) >= Duration::from_secs(1)
        {
            self.save_window_state();
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
                MediaAction::StartCamera { id } => {
                    let source_id = format!("camera:{id}");
                    let config = VideoSourceConfig::camera(&id).with_mode(1280, 720, 30);
                    if let Err(error) = self.media_hub.start(&source_id, config) {
                        self.record_error(format!("camera {id} failed: {error}"));
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
                    self.media_hub.stop(&source_id);
                }
            }
        }
    }

    fn drain_ui_actions(&mut self) {
        for action in self.ui_state.take_actions() {
            if action.action == "ui/addModuleToGroup" {
                self.add_module_to_group(&action);
                continue;
            }
            if let Err(error) = self.store.dispatch(&action.action, action.payload) {
                self.record_error(format!("{} failed: {error}", action.action));
            }
        }
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
                UiCommand::SetAudioSource { source, device } => {
                    self.set_audio_source(&source, device.as_deref());
                }
                UiCommand::RefreshAudioDevices => self.refresh_audio_devices(),
            }
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
        // The engine's preview readbacks are premultiplied; flatten onto
        // opaque black before handing them to egui, which expects straight
        // alpha. This is the same flatten the main preview path applies.
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
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
                tweens
                    .iter()
                    .filter_map(|tween| {
                        let id = tween.get("id").and_then(Value::as_str)?.to_string();
                        Some((
                            id.clone(),
                            TweenView {
                                id,
                                duration: tween
                                    .get("duration")
                                    .and_then(Value::as_f64)
                                    .unwrap_or(1000.0),
                                direction: tween
                                    .get("direction")
                                    .and_then(Value::as_str)
                                    .unwrap_or("alternate")
                                    .to_string(),
                                easing: tween
                                    .get("easing")
                                    .and_then(Value::as_str)
                                    .unwrap_or("linear")
                                    .to_string(),
                                looping: tween.get("loop").and_then(Value::as_bool).unwrap_or(true),
                                use_bpm: tween
                                    .get("useBpm")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(true),
                                bpm_division: tween
                                    .get("bpmDivision")
                                    .and_then(Value::as_f64)
                                    .unwrap_or(16.0),
                                steps: tween.get("steps").and_then(Value::as_f64).unwrap_or(0.0),
                                frames: tween
                                    .get("frames")
                                    .and_then(Value::as_array)
                                    .map_or(0, |frames| frames.len() as u64),
                            },
                        ))
                    })
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
                module_view(module, group_id, &registry, &tween_by_id)
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
        let scenes = scenes_state
            .scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| scene_view(scene, index, &row_names, &disabled_cells))
            .collect::<Vec<_>>();
        let performance = performance_view(
            &scenes_state.performance,
            self.store.performance_runtime().enabled,
            &row_names,
        );
        let applied_scene_id = performance.current_scene_id.clone();
        let audio = AudioView {
            bpm: self.store.bpm(),
            kick: self.store.kick(),
            bass: self.store.audio().bass,
            mid: self.store.audio().mid,
            treble: self.store.audio().treble,
        };
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
            cameras: self
                .media_cameras
                .iter()
                .cloned()
                .map(|camera| {
                    let mode = camera.max_mode();
                    CameraView {
                        id: camera.id,
                        name: camera.name,
                        max_width: mode.map_or(0, |mode| mode.width),
                        max_height: mode.map_or(0, |mode| mode.height),
                        max_fps: mode.map_or(0, |mode| mode.fps),
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
        let midi = MidiSnapshot {
            available: self.midi_client.available(),
            error: self.midi_client.error().map(str::to_string),
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
        let audio_selection = AudioSelectionView {
            source: self.audio_selection.as_str().to_string(),
            device: self.audio_device.clone(),
            error: self.audio_switch_error.clone(),
        };

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
            health,
            gallery_previews,
            scene_previews,
            texture_sources,
            input_links,
            tweens: tween_by_id,
            expressions: expression_views(state),
            media,
            midi,
            audio_devices: self.audio_devices.clone(),
            audio_selection,
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

    fn record_error(&mut self, message: String) {
        eprintln!("ui: ERROR {message}");
        self.last_error = Some(message);
    }

    fn save_preset(&mut self) {
        let path = self
            .cli
            .preset
            .clone()
            .unwrap_or_else(|| user_data_dir().join("native-preset.json"));
        match save_preset_to(&path, &self.store) {
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
        event_loop.exit();
    }

    fn shutdown(&mut self) {
        if !self.done {
            self.save_snapshot();
        }
        self.save_window_state();
        self.store.flush_link_deltas();
        self.store.shutdown_ndi();
        self.media_hub.stop_all();
        self.midi_client.stop();
        if let Some(mut feed) = self.audio_feed.take() {
            feed.stop();
        }
        self.close_output();
    }
}

fn module_view(
    module: &ModuleInstance,
    group_id: String,
    registry: &crate::module_registry::ModuleRegistry,
    tween_by_id: &HashMap<String, TweenView>,
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
    let timing = if scene
        .get("timingOverride")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        scene.get("maxSeconds").and_then(Value::as_f64)
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
    PerformanceView {
        enabled,
        current_scene_id: config
            .get("currentSceneId")
            .and_then(Value::as_str)
            .map(str::to_string),
        row_names: row_names.to_vec(),
        min_seconds: number("minSeconds", 8.0),
        max_seconds: number("maxSeconds", 24.0),
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

/// Resolve the ISF corpus directory for the native binary.
///
/// Packaging moves the corpus to `resources/isf`; during development it still
/// lives beside the legacy renderer tree. `$MODV_ISF_DIR` and `--isf-dir` win.
/// Write the six-key preset document for `store` to `path`.
fn save_preset_to(path: &Path, store: &Store) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let document = store.snapshot();
    let text = serde_json::to_string_pretty(&document).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// Load a preset/Scene document from `path` into `store`.
fn load_preset_from(path: &Path, store: &mut Store) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let document: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    store.restore(&document).map_err(|error| error.to_string())
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

/// User ISF folders under `<userData>/media`.
///
/// The legacy media manager keeps third-party shaders in
/// `<media>/<project>/isf`; scan the flat `<media>/isf` folder too. This is a
/// startup scan; the watch/reload wrapper is still P6 work.
fn user_isf_dirs(user_data: &Path) -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os("MODV_USER_ISF_DIR").map(PathBuf::from) {
        return vec![dir];
    }
    let media = user_data.join("media");
    let mut dirs = Vec::new();
    let flat = media.join("isf");
    if flat.is_dir() {
        dirs.push(flat);
    }
    if let Ok(entries) = std::fs::read_dir(&media) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let candidate = entry.path().join("isf");
                if candidate.is_dir() {
                    dirs.push(candidate);
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
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

    #[test]
    fn scene_views_flatten_group_module_names_and_null_holes() {
        let scene = serde_json::json!({
            "id": "s1",
            "name": "Test Scene",
            "enabled": 1,
            "timingOverride": true,
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
        let view = scene_view(&scene, 0, &[], &std::collections::BTreeMap::new());
        assert_eq!(view.id, "s1");
        assert_eq!(view.name, "Test Scene");
        assert!(view.enabled);
        assert_eq!(view.timing, Some(12.0));
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
        let view = scene_view(&scene, 0, &row_names, &disabled);

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
        assert_eq!(view.min_seconds, 4.0);
        assert_eq!(view.max_seconds, 10.0);
        assert_eq!(view.transition_seconds, 1.5);
        assert_eq!(view.allowed_transitions, vec!["Fade", "Wipe"]);
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
        save_preset_to(&path, &store).expect("save preset");
        let mut restored = Store::new();
        load_preset_from(&path, &mut restored).expect("load preset");
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
    fn user_isf_dirs_find_flat_and_project_folders() {
        let root = std::env::temp_dir().join(format!(
            "modv-user-isf-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(root.join("media/isf")).expect("flat isf");
        std::fs::create_dir_all(root.join("media/default/isf")).expect("project isf");
        std::fs::create_dir_all(root.join("media/default/video")).expect("other folder");
        let dirs = user_isf_dirs(&root);
        assert!(dirs.contains(&root.join("media/isf")));
        assert!(dirs.contains(&root.join("media/default/isf")));
        assert!(!dirs.contains(&root.join("media/default/video")));
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
        let view = module_view(&module, "group-1".to_string(), &registry, &HashMap::new());
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
}
