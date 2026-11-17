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

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use modv_render::Engine;
use modv_render::surface::{SurfaceTarget, create_surface, create_surface_with_present_mode};
use modv_render::wgpu;
use modv_ui::{
    AudioView, CameraView, MediaAction, MediaSnapshot, MediaSourceView, EngineHealth, EngineSnapshot, GroupView, ModuleMetaView, ModuleView,
    InputLinkView, PerformanceView, PreviewRequest, PreviewTexture, PropView, SceneCellView, SceneView, TextureSourceView, TweenView, UiAction,
    UiCommand, UiState, gallery_preview_key, scene_preview_key,
};
use serde_json::Value;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};

use crate::cli::Cli;
use crate::compositor::{Compositor, FrameInfo, RowFadeInfo};
use crate::media::MediaHub;
use crate::report::AdapterSummary;
use crate::store_ops::Store;
use crate::window_state::{SavedWindow, WINDOW_STATE_FILENAME, WindowState, user_data_dir};
use modv_media::video::{TestPattern, VideoSourceConfig};
use crate::window::{DoubleClickTracker, is_fullscreen_exit_key, is_fullscreen_toggle_key};
use modv_engine::engine_state::js_truthy;

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
    preview_errors: HashSet<String>,
    window_state: WindowState,
    window_state_path: PathBuf,
    window_state_dirty: bool,
    last_window_state_save: Instant,
    window_state_error_logged: bool,
    media_hub: MediaHub,
    last_media_health: Instant,
    modifiers: HashMap<WindowId, ModifiersState>,
    double_click: HashMap<WindowId, DoubleClickTracker>,
    frames: u64,
    start: Instant,
    last_frame: Instant,
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
        let mut store = Store::with_state_path_and_isf(cli.state.as_deref(), isf_dir.as_deref());
        if let Some(path) = &cli.state {
            match store.restore_from_file(path) {
                Ok(true) => eprintln!(
                    "ui: INFO restored engine state from {} ({} groups, {} modules)",
                    path.display(),
                    store.state().groups.len(),
                    store.state().modules.len()
                ),
                Ok(false) => eprintln!(
                    "ui: INFO no state snapshot at {}; starting empty",
                    path.display()
                ),
                Err(error) => {
                    eprintln!(
                        "ui: WARN could not load state snapshot {}: {error}",
                        path.display()
                    )
                }
            }
        }

        // Hydrate/seed even without an explicit --state: the native shell owns
        // the whole state plane, so the Scenes panel must be populated on a
        // fresh launch. Seeding is idempotent.
        if let Err(error) = store.dispatch("scenes/hydrateScenes", serde_json::json!({})) {
            eprintln!("ui: WARN could not hydrate Scenes: {error}");
        }
        if let Err(error) = store.dispatch("scenes/ensureDefaultScenes", serde_json::json!({})) {
            eprintln!("ui: WARN could not seed default Scenes: {error}");
        }
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

        let egui_ctx = egui::Context::default();
        modv_ui::theme::apply(&egui_ctx);

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
            preview_errors: HashSet::new(),
            window_state: WindowState::read(&user_data_dir().join(WINDOW_STATE_FILENAME)),
            window_state_path: user_data_dir().join(WINDOW_STATE_FILENAME),
            window_state_dirty: false,
            last_window_state_save: start,
            window_state_error_logged: false,
            media_hub,
            last_media_health: start,
            modifiers: HashMap::new(),
            double_click: HashMap::new(),
            frames: 0,
            start,
            last_frame: start,
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
        if let Some(saved) = self.window_state.main_window.clone() {
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
        self.render_engine(0.0);
        self.update_preview_texture();

        if let Err(error) = self.open_output(event_loop) {
            eprintln!("ui: WARN could not open the output window: {error}");
        }

        event_loop.set_control_flow(ControlFlow::Poll);
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
        if let Some(saved) = self.window_state.output().cloned() {
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
        self.render_engine(0.0);
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
        self.store.drain_osc();
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

    fn frame_info(&self) -> FrameInfo {
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
            // Whole-scene shader transitions need the outgoing-frame capture
            // path from the engine window loop; P4 ports it with the Scenes
            // panel. Row crossfades (the common performance transition) work.
            scene_transition: None,
        }
    }

    fn render_engine(&mut self, _dt: f64) {
        let info = self.frame_info();
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
        self.drain_ui_commands(event_loop);
        if self.done {
            return;
        }

        let snapshot = self.build_snapshot();
        self.ui_state.sync_status(&snapshot);
        self.run_ui(&snapshot);
        self.drain_ui_actions();
        if let Some(compositor) = &mut self.compositor {
            compositor.sync_media(&mut self.media_hub);
        }
        self.render_engine(dt.as_secs_f64());
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
            && now.saturating_duration_since(self.last_window_state_save)
                >= Duration::from_secs(1)
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
        } else if let Some(window) = &self.ui_window {
            window.request_redraw();
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
                UiCommand::FlushLogs => {}
            }
        }
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
                if self.preview_errors.insert(key.clone()) {
                    eprintln!("ui: WARN preview {key} failed: {error}");
                }
            }
        }
    }

    fn upload_preview(&mut self, key: String, mut pixels: Vec<u8>, width: u32, height: u32) {
        if pixels.len() != (width as usize) * (height as usize) * 4 {
            if self.preview_errors.insert(key.clone()) {
                eprintln!("ui: WARN preview {key} returned {} bytes", pixels.len());
            }
            return;
        }
        // The engine's preview readbacks are premultiplied; flatten onto
        // opaque black before handing them to egui, which expects straight
        // alpha. This is the same flatten the main preview path applies.
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            &pixels,
        );
        if let Some(entry) = self.preview_textures.get_mut(&key) {
            entry.handle.set(image, egui::TextureOptions::LINEAR);
        } else {
            let handle = self
                .egui_ctx
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
                                duration: tween.get("duration").and_then(Value::as_f64).unwrap_or(1000.0),
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
                                use_bpm: tween.get("useBpm").and_then(Value::as_bool).unwrap_or(true),
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
                                    default: schema
                                        .as_ref()
                                        .and_then(|schema| schema.default.clone()),
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
                    module_type,
                    props,
                }
            })
            .collect();

        let groups = state
            .groups
            .iter()
            .map(|group| GroupView {
                id: group.id.clone(),
                name: group.name.clone(),
                enabled: group.enabled == modv_engine::engine_state::GROUP_ENABLED,
                hidden: group.hidden,
                alpha: group.alpha as f32,
                composite_operation: group.composite_operation.clone(),
                inherit: group.inherit,
                clearing: group.clearing,
                pipeline: group.pipeline,
                module_ids: group.modules.clone(),
            })
            .collect();

        let registered = registered_modules(&registry);
        let scenes_state = self.store.scenes_state();
        let scenes = scenes_state
            .scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| scene_view(scene, index))
            .collect::<Vec<_>>();
        let performance = performance_view(
            &scenes_state.performance,
            self.store.performance_runtime().enabled,
            &self.store.row_names(),
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
                .media_hub
                .list_cameras()
                .into_iter()
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
        let health = EngineHealth {
            adapter: format!("{} ({})", self.adapter.name, self.adapter.device_type),
            backend: self.adapter.backend.clone(),
            last_error: self.last_error.clone(),
            frames_presented: self.frames,
            output_open: self.output.is_some(),
            audio_active: self.store.engine_audio_active(),
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
            media,
        }
    }

    /// Remember the main window's current logical placement.
    fn remember_main_window(&mut self) {
        let Some(window) = &self.ui_window else {
            return;
        };
        let scale = window.scale_factor();
        let size = window.inner_size();
        let previous = self.window_state.main_window.clone().unwrap_or_default();
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
        let previous = self.window_state.output().cloned().unwrap_or_default();
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
        self.close_output();
    }
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

fn scene_view(scene: &Value, index: usize) -> SceneView {
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
    let cells = scene
        .get("groups")
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .enumerate()
                .map(|(row, group)| {
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
                    SceneCellView {
                        row,
                        module_names: names,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
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
                    self.render_engine(0.0);
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if !self.done
            && !self.failed
            && let Some(window) = &self.ui_window
        {
            window.request_redraw();
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
        let view = scene_view(&scene, 0);
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
    fn isf_dir_resolution_prefers_explicit_paths() {
        let explicit = Path::new("/tmp/isf-test");
        assert_eq!(
            resolve_isf_dir_from(Some(explicit)),
            Some(PathBuf::from(explicit))
        );
    }
}
