//! The windowed code path, implemented as a winit 0.30 `ApplicationHandler`.
//!
//! Each frame: poll the control socket (mutations into the [`Store`]), render
//! the state through the [`Compositor`], present the main texture, and publish
//! a preview frame at ≤ 30 fps. Frame pacing relies on
//! `wgpu::PresentMode::Fifo` (blocking present); the window requests a redraw
//! at the end of each `RedrawRequested` handler, so frames are produced
//! back-to-back at the display's refresh rate.
//!
//! M2 removed the M0 `ChromaticLine`/`SyntheticSine` demo from this path: the
//! window now renders whatever the store describes (an empty state presents
//! the opaque black clear). `--audio-fixture` is still parsed for the M0
//! headless/report path but has no effect here.
//!
//! Audio is owned by exactly one side. With the default `--audio-source
//! renderer` the renderer's audio pump commits the full analyser payload
//! (`meyda/SET_BANDS`, WS-9) and the engine only stores it. With
//! `--audio-source pipewire|input` the engine opens an
//! [`modv_media::audio::engine::AudioFeed`] before the loop, pumps it each
//! frame before Input Links are evaluated, and pushes the analysis into the
//! store ([`Store::set_engine_audio`]); a capture failure logs once, stops the
//! feed and hands ownership back to the renderer instead of killing the loop.
//!
//! Each frame also dispatches `scenes/tick` with the real frame delta before
//! rendering (`worker/loop.js:94-111`), but only while performance mode is
//! running or a crossfade is active, so idle frames stay cheap. Input Links
//! are evaluated between the state update and the render, so audio-reactive
//! props are current in the same frame they were computed.
//!
//! Video capture (WS-10) is owned by the supervisor's [`crate::media::MediaHub`]
//! and uploaded into per-source textures by
//! [`Compositor::sync_media`](crate::compositor::Compositor::sync_media) right
//! after the control socket is polled — before the state is rendered, so the
//! `Webcam` module draws this frame's camera/file/screen capture. Sources are
//! user-level: they are started and stopped by `media/start-*` /
//! `media/stop-source`, survive Scene applies and `engine/restore`, and are all
//! released when the run exits.
//!
//! Output Windows ([`crate::windows`]) are extra winit windows sharing this
//! loop's device, queue and main texture. `windows/createWindow` queues a
//! [`WindowCommand::Open`] that [`WindowedApp::sync_windows`] applies each
//! frame; every frame's present pass then blits the same main texture into
//! each output surface in the same command encoder as the primary present.
//! There is no per-window render and no second compositor: the windows are
//! pure presentation targets, so they follow the primary frame exactly.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use modv_media::audio::engine::AudioFeed;
use modv_render::surface::{SurfaceTarget, create_surface, create_surface_with_present_mode};
use modv_render::wgpu;
use modv_render::{Engine, FrameReport, FrameStats};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};

use crate::cli::Cli;
use crate::compositor::{Compositor, FrameInfo, RowFadeInfo};
use crate::report::{AdapterSummary, report_json};
use crate::scene_preview;
use crate::scene_transition::SceneTransitionInfo;
use crate::store_ops::Store;
use crate::supervisor::{EngineSupervisor, PreviewKind};
use crate::windows::{WindowCommand, WindowSpec};

/// Most preview requests answered in one frame.
///
/// Each answer can block on a GPU read-back (≤ 1 MiB at 512²; a cold
/// volumetric Point10 thumbnail also builds pipelines/buffers), so a burst of
/// Gallery requests must never stall presentation. One per frame at up to
/// 120 Hz is far more than the visible-tile polling needs, and it keeps a
/// single expensive miss out of any other frame's tail. The cap is shared by
/// Gallery thumbnails (`preview/module`) and Scene hover cells
/// (`preview/scene`).
const MAX_PREVIEWS_PER_FRAME: usize = 1;

/// Run the windowed event loop. Returns an error string on fatal failures
/// (window creation, surface creation, event loop).
pub fn run(cli: Cli, engine: Engine, adapter: AdapterSummary) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("failed to create event loop: {e}"))?;
    let mut app = WindowedApp::new(cli, engine, adapter);
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("event loop failed: {e}"))?;
    // Flush the last <100 ms of Input Link deltas so the renderer mirror sees
    // the final values before the socket closes.
    app.store.flush_link_deltas();
    // Release the engine-owned capture (WS-9) on every exit path.
    app.stop_engine_audio();
    // Release the engine-owned video sources (WS-10) on every exit path, even
    // when the renderer never sent `media/stop-source`.
    app.stop_media();
    // Flush the snapshot on any exit path (window close, bench deadline,
    // engine/shutdown) so a supervisor restart restores the last state.
    if let Some(mut supervisor) = app.supervisor.take() {
        eprintln!("engine: INFO flushing state snapshot to disk");
        match supervisor.save_snapshot(&app.store) {
            Ok(()) => eprintln!("engine: INFO state snapshot flushed"),
            Err(error) => eprintln!("engine: WARN final snapshot failed: {error}"),
        }
    } else {
        eprintln!("engine: WARN no supervisor; state snapshot not flushed");
    }
    if app.failed {
        return Err("fatal error during windowed run (see above)".to_string());
    }
    Ok(())
}

/// State for the windowed run.
struct WindowedApp {
    cli: Cli,
    engine: Option<Engine>,
    surface: Option<SurfaceTarget>,
    compositor: Option<Compositor>,
    store: Store,
    stats: Option<FrameStats>,
    adapter: AdapterSummary,
    window: Option<Arc<Window>>,
    /// The primary window's winit id, so `window_event` can tell it from the
    /// Output Windows.
    main_window_id: Option<WindowId>,
    /// Engine-owned Output Windows, keyed by their winit id.
    output_windows: HashMap<WindowId, OutputWindow>,
    bench_deadline: Option<Duration>,
    frames_done: u64,
    /// Frames since the last `metrics/SET_FPS_MEASURE` publish.
    metric_frames: u32,
    last_metric: Instant,
    /// Latest surface size, republished with the status metrics so a late
    /// renderer connection still receives it.
    surface_size: (u32, u32),
    last_frame: Instant,
    start: Instant,
    done: bool,
    failed: bool,
    supervisor: Option<EngineSupervisor>,
    consecutive_timeouts: u32,
    preview_error_logged: bool,
    /// Engine-owned capture (WS-9): `Some` when `--audio-source` is not
    /// `renderer`. `None` means the renderer's pump owns the analysis (the
    /// default, and the fallback when capture could not start).
    audio_feed: Option<AudioFeed>,
    /// One-shot guard for the capture-failure log line.
    audio_error_logged: bool,
    /// Double-click detection per window: F or a double click toggles that
    /// window's fullscreen (the legacy Output Window does the same on the
    /// popup's document).
    double_click: HashMap<WindowId, DoubleClickTracker>,
    /// Live keyboard modifiers per window, so `Ctrl+F`/`Alt+F` are not treated
    /// as the fullscreen shortcut.
    modifiers: HashMap<WindowId, ModifiersState>,
    /// The media root currently applied to the compositor, so a
    /// `media/setMediaDirectoryPath` change is picked up on the next frame.
    applied_media_root: Option<PathBuf>,
}

/// A secondary native window presenting the primary composited frame.
///
/// It owns no renderer state: the frame loop blits the shared main texture into
/// `surface` every frame, so an Output Window can never diverge from the
/// primary output.
struct OutputWindow {
    /// The store's window id (the `windows` Vuex-module key).
    id: String,
    /// The winit window; dropping it (together with the surface) closes it.
    window: Arc<Window>,
    /// The configured swapchain presenting the main texture.
    surface: SurfaceTarget,
}

/// How long two clicks may be apart to count as a double click.
pub const DOUBLE_CLICK_MS: u64 = 400;

/// Tracks the previous mouse click so two presses of the same button within
/// [`DOUBLE_CLICK_MS`] toggle fullscreen.
#[derive(Debug, Default)]
pub struct DoubleClickTracker {
    last: Option<(Instant, MouseButton)>,
}

impl DoubleClickTracker {
    /// Register a press; returns `true` when it completes a double click.
    ///
    /// Time is passed in so the behaviour is unit-testable. A triple click
    /// toggles once (the pair consumes the state and the third press starts
    /// over) rather than flickering twice.
    pub fn register(&mut self, now: Instant, button: MouseButton) -> bool {
        let double = matches!(
            self.last,
            Some((at, previous)) if previous == button
                && now.saturating_duration_since(at) <= Duration::from_millis(DOUBLE_CLICK_MS)
        );
        self.last = if double { None } else { Some((now, button)) };
        double
    }
}

/// Whether a key press should toggle the output window's fullscreen state.
///
/// `f`/`F` without Ctrl/Meta/Alt; Shift is allowed because it is how `F` is
/// typed. AltGr arrives as Ctrl+Alt on some layouts, so Ctrl is rejected on
/// purpose (the engine has no other F shortcut to clash with).
pub fn is_fullscreen_toggle_key(key: &Key, modifiers: ModifiersState) -> bool {
    let touches_command = modifiers.control_key() || modifiers.super_key() || modifiers.alt_key();
    !touches_command && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("f"))
}

/// Whether a key press should leave fullscreen.
pub fn is_fullscreen_exit_key(key: &Key) -> bool {
    matches!(key, Key::Named(NamedKey::Escape))
}

// `FrameStats` contains no public Clone; store it in an Option so `finish()`
// can consume it exactly once.

impl WindowedApp {
    /// Toggle `window_id` between borderless fullscreen and windowed.
    ///
    /// The primary window and each Output Window keep their own state, like the
    /// legacy popups (whose `dblclick` handler fullscreened that document).
    fn toggle_fullscreen(&mut self, window_id: WindowId) {
        let Some(window) = self.window_for(window_id) else {
            return;
        };
        let leaving = window.fullscreen().is_some();
        window.set_fullscreen(if leaving {
            None
        } else {
            // Wayland has no primary monitor; `None` means "the monitor the
            // window is on", which is what the user expects.
            Some(Fullscreen::Borderless(None))
        });
        if let Some(output) = self.output_windows.get(&window_id) {
            eprintln!(
                "engine: INFO output window {} fullscreen {}",
                output.id,
                if leaving { "off" } else { "on" }
            );
        } else {
            eprintln!(
                "engine: INFO fullscreen {}",
                if leaving { "off" } else { "on" }
            );
        }
    }

    /// The winit window for `window_id` (primary or Output Window).
    fn window_for(&self, window_id: WindowId) -> Option<&Arc<Window>> {
        if Some(window_id) == self.main_window_id {
            self.window.as_ref()
        } else {
            self.output_windows
                .get(&window_id)
                .map(|output| &output.window)
        }
    }

    fn new(cli: Cli, engine: Engine, adapter: AdapterSummary) -> Self {
        let start = Instant::now();
        // WS-10: initialise GStreamer (and log its version) before the loop, so
        // a host without the capture stack reports it at startup rather than on
        // the first camera. A failure is not fatal: media/start-* answers
        // `media_unavailable` and rendering is unaffected.
        let _ = crate::media::init();
        let mut store =
            Store::with_state_path_and_isf(cli.state.as_deref(), cli.isf_dir.as_deref());
        if let Some(path) = &cli.state {
            match store.restore_from_file(path) {
                Ok(true) => eprintln!(
                    "engine: INFO restored engine state from {} ({} groups, {} modules)",
                    path.display(),
                    store.state().groups.len(),
                    store.state().modules.len()
                ),
                Ok(false) => {
                    eprintln!(
                        "engine: INFO no state snapshot at {}; starting empty",
                        path.display()
                    );
                }
                Err(error) => {
                    eprintln!(
                        "engine: WARN could not load state snapshot {}: {error}",
                        path.display()
                    );
                }
            }

            // The Scene list lives beside the snapshot. The renderer also
            // dispatches these boot actions; running them here keeps a
            // headless/supervised engine usable before the UI connects, and
            // the second pass is idempotent (`ensureDefaultScenes` seeds
            // nothing once the defaults version matches).
            if let Err(error) = store.dispatch("scenes/hydrateScenes", serde_json::json!({})) {
                eprintln!("engine: WARN could not hydrate Scenes: {error}");
            }
            if let Err(error) = store.dispatch("scenes/ensureDefaultScenes", serde_json::json!({}))
            {
                eprintln!("engine: WARN could not seed default Scenes: {error}");
            }
        }
        let supervisor = cli.socket.as_ref().and_then(|socket_path| {
            match EngineSupervisor::new(socket_path.clone(), cli.state.clone()) {
                Ok(supervisor) => {
                    eprintln!(
                        "engine: INFO control socket listening on {}",
                        socket_path.display()
                    );
                    Some(supervisor)
                }
                Err(error) => {
                    eprintln!(
                        "engine: ERROR could not bind control socket {}: {error}",
                        socket_path.display()
                    );
                    None
                }
            }
        });
        let socket_failed = cli.socket.is_some() && supervisor.is_none();

        // WS-9: open the engine-owned feed before the loop when the renderer is
        // not the audio owner. A failure is not fatal: the engine runs with the
        // renderer's pump (which keeps committing `meyda/SET_BANDS`).
        let audio_feed = if cli.audio_source.engine_owned() {
            match crate::audio::open_feed(cli.audio_source, cli.audio_device.as_deref()) {
                Ok(feed) => {
                    let device_note = cli
                        .audio_device
                        .as_deref()
                        .map(|device| format!(" (device {device})"))
                        .unwrap_or_default();
                    eprintln!(
                        "engine: INFO audio source {} owns capture{} at {} Hz",
                        cli.audio_source.as_str(),
                        device_note,
                        feed.sample_rate()
                    );
                    Some(feed)
                }
                Err(error) => {
                    eprintln!(
                        "engine: WARN audio source {} unavailable ({error}); falling back to the renderer's audio pump",
                        cli.audio_source.as_str()
                    );
                    None
                }
            }
        } else {
            None
        };

        Self {
            bench_deadline: cli.bench_deadline(),
            cli,
            engine: Some(engine),
            surface: None,
            compositor: None,
            store,
            stats: None,
            adapter,
            window: None,
            main_window_id: None,
            output_windows: HashMap::new(),
            frames_done: 0,
            metric_frames: 0,
            last_metric: Instant::now(),
            surface_size: (0, 0),
            last_frame: start,
            start,
            done: false,
            failed: false,
            supervisor,
            consecutive_timeouts: 0,
            preview_error_logged: false,
            audio_feed,
            audio_error_logged: false,
            double_click: HashMap::new(),
            modifiers: HashMap::new(),
            applied_media_root: None,
        }
        .with_socket_failure(socket_failed)
    }

    fn with_socket_failure(mut self, failed: bool) -> Self {
        if failed {
            self.failed = true;
        }
        self
    }

    /// Stop every engine-owned video source (WS-10), releasing the devices.
    fn stop_media(&mut self) {
        if let Some(supervisor) = &mut self.supervisor {
            supervisor.media_mut().stop_all();
        }
    }

    /// Pump the engine-owned feed (WS-9) and publish the frame into the store.
    ///
    /// `dt` is the frame delta the loop hands the Scene tick; the analyser uses
    /// it for its AGC/envelope clocks. The feed's capture source is pumped
    /// non-blockingly, so a stalled device never delays a frame — it surfaces
    /// as an error instead, which stops the feed once and hands audio ownership
    /// back to the renderer's pump (logged once).
    fn pump_engine_audio(&mut self, dt: Duration) {
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
            eprintln!(
                "engine: WARN audio capture stopped ({error}); falling back to the renderer's audio pump"
            );
        }
        self.stop_engine_audio();
        self.store.disable_engine_audio();
    }

    /// Stop and release the engine-owned capture, if any (idempotent).
    ///
    /// Logs only when the engine actually owned the analysis, so a feed that
    /// failed at startup (ownership already with the renderer) stays quiet.
    fn stop_engine_audio(&mut self) {
        let Some(mut feed) = self.audio_feed.take() else {
            return;
        };

        feed.stop();
        if self.store.engine_audio_active() {
            eprintln!("engine: INFO audio capture stopped");
        }
    }

    /// Build the window, surface and compositor (first `resumed`).
    fn init(&mut self, event_loop: &ActiveEventLoop) {
        let Some(engine) = self.engine.as_ref() else {
            eprintln!("error: engine missing during window initialisation");
            self.failed = true;
            event_loop.exit();
            return;
        };

        let mut attributes = WindowAttributes::default()
            .with_title("modV Engine")
            .with_inner_size(winit::dpi::PhysicalSize::new(
                self.cli.width.max(1),
                self.cli.height.max(1),
            ));
        if self.cli.fullscreen {
            // Wayland does not report a primary monitor; fall back to the
            // monitor the window is on (`Borderless(None)`).
            attributes = attributes
                .with_fullscreen(Some(Fullscreen::Borderless(event_loop.primary_monitor())));
        }

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("error: failed to create window: {err}");
                self.failed = true;
                event_loop.exit();
                return;
            }
        };

        let surface = match create_surface(engine, Arc::clone(&window)) {
            Ok(surface) => surface,
            Err(err) => {
                eprintln!("error: failed to create surface: {err}");
                self.failed = true;
                event_loop.exit();
                return;
            }
        };

        let mut compositor =
            Compositor::with_registry(&engine.device, &engine.queue, self.store.registry_arc());
        let (width, height) = surface.dimensions();
        compositor.resize(width, height);

        self.surface = Some(surface);
        self.compositor = Some(compositor);
        self.main_window_id = Some(window.id());
        self.window = Some(window);
        self.stats = Some(FrameStats::start());
        self.last_frame = Instant::now();
        self.start = self.last_frame;
        // Any `windows/createWindow` dispatched before the first frame (the
        // renderer's boot actions can arrive first) opens now that the device
        // and event loop are ready.
        self.sync_windows(event_loop);
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Apply the queued Output Window operations (create/close).
    ///
    /// Called once per frame before rendering, so a `windows/createWindow`
    /// request opens the native window on the next frame instead of inside the
    /// control-socket handler (which has no event loop). A surface that cannot
    /// be created removes the spec again and emits `windows/REMOVE_WINDOW`, so
    /// the UI list never shows a window that does not exist.
    fn sync_windows(&mut self, event_loop: &ActiveEventLoop) {
        let commands = self.store.take_window_commands();
        if commands.is_empty() {
            return;
        }

        let mut removed = false;
        for command in commands {
            match command {
                WindowCommand::Open(spec) => {
                    if !self.open_output_window(event_loop, spec) {
                        removed = true;
                    }
                }
                WindowCommand::Close(id) => {
                    if let Some(window_id) = self
                        .output_windows
                        .iter()
                        .find(|(_, output)| output.id == id)
                        .map(|(window_id, _)| *window_id)
                    {
                        self.finish_output_window(window_id);
                    }
                }
            }
        }

        // A failed creation already emitted its `REMOVE_WINDOW`; broadcast it
        // now so the UI drops the entry instead of waiting for the next
        // periodic delta flush.
        if removed {
            self.broadcast_deltas();
        }
    }

    /// Create and register one Output Window. Returns `false` when it could not
    /// be opened (the caller then broadcasts the removal delta).
    fn open_output_window(&mut self, event_loop: &ActiveEventLoop, spec: WindowSpec) -> bool {
        let Some(engine) = self.engine.as_ref() else {
            return false;
        };

        let attributes = output_window_attributes(&spec, event_loop);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!(
                    "engine: WARN could not create output window {}: {error}",
                    spec.id
                );
                self.store.window_closed_natively(&spec.id);
                return false;
            }
        };

        // `AutoNoVsync`: the primary window's Fifo acquire paces the loop, and
        // a secondary surface must never throttle it to its own display rate.
        let surface = match create_surface_with_present_mode(
            engine,
            Arc::clone(&window),
            wgpu::PresentMode::AutoNoVsync,
        ) {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!(
                    "engine: WARN could not create the surface for output window {}: {error}",
                    spec.id
                );
                self.store.window_closed_natively(&spec.id);
                return false;
            }
        };

        let (width, height) = surface.dimensions();
        eprintln!(
            "engine: INFO output window {} opened ({}x{}{})",
            spec.id,
            width,
            height,
            if spec.fullscreen { ", fullscreen" } else { "" }
        );
        let window_id = window.id();
        self.output_windows.insert(
            window_id,
            OutputWindow {
                id: spec.id,
                window,
                surface,
            },
        );
        true
    }

    /// Forget an Output Window that no longer exists natively (closed by the
    /// window manager, a failed surface, or `windows/removeWindow`).
    ///
    /// Dropping the [`OutputWindow`] drops the winit window and its surface, so
    /// the OS window closes. The store emits `windows/REMOVE_WINDOW` only when
    /// the close was *not* already reflected by a `windows/removeWindow` action.
    fn finish_output_window(&mut self, window_id: WindowId) {
        let Some(output) = self.output_windows.remove(&window_id) else {
            return;
        };
        self.double_click.remove(&window_id);
        self.modifiers.remove(&window_id);

        let emitted = self.store.window_closed_natively(&output.id);
        eprintln!("engine: INFO output window {} closed", output.id);
        if emitted {
            self.broadcast_deltas();
        }
        drop(output);
    }

    /// Broadcast pending mirror deltas (no-op without a supervisor).
    fn broadcast_deltas(&mut self) {
        if let Some(supervisor) = &mut self.supervisor {
            supervisor.broadcast_deltas(&mut self.store);
        }
    }

    /// Render one frame (or skip it when the surface is not ready).
    fn draw_frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.done || self.failed {
            return;
        }
        let now = Instant::now();
        let dt_duration = if self.frames_done == 0 {
            Duration::from_secs_f64(1.0 / 60.0)
        } else {
            now.saturating_duration_since(self.last_frame)
        };
        self.last_frame = now;

        // 0. Engine-owned capture (WS-9): pump the feed and publish the frame
        // into the store before anything reads it. Runs ahead of the control
        // socket poll on purpose — while the engine owns the audio, renderer
        // frames committed by the socket are rejected, so the analysis that
        // drives this frame is the engine's.
        self.pump_engine_audio(dt_duration);

        // 1. Apply control-socket mutations before rendering the frame.
        if let Some(supervisor) = &mut self.supervisor {
            supervisor.poll(&mut self.store);
            if supervisor.take_reset_request() {
                if let Some(compositor) = &mut self.compositor {
                    compositor.reset();
                }
            }
        }

        // 1a. Open/close the Output Windows the store queued (createWindow /
        // removeWindow). Before the render so a new window can present this
        // frame's image.
        self.sync_windows(event_loop);

        // 1b. Upload any new video frames into their per-source textures
        // (WS-10), so the Webcam module draws this frame's capture. The
        // sources are user-level: they survive Scene applies and a restore, so
        // this runs whether or not the current Scene shows a Webcam layer.
        // Field-level borrows keep the `engine` handle above usable.
        if let (Some(supervisor), Some(compositor)) = (&mut self.supervisor, &mut self.compositor) {
            compositor.sync_media(supervisor.media_mut());
        }

        // 1b2. Follow the renderer's project media root so ISF texture props
        // and library image paths resolve under it (set once at startup and on
        // a media-directory change).
        let media_root = self.store.media_root().map(std::path::Path::to_path_buf);
        if media_root != self.applied_media_root {
            if let (Some(compositor), Some(root)) = (self.compositor.as_mut(), media_root.as_deref())
            {
                compositor.set_media_root(root);
            }
            self.applied_media_root = media_root;
        }

        // 1c. A whole-scene shader transition composites the outgoing Scene
        // itself, so it needs that frame captured. The main texture of the last
        // presented frame *is* the outgoing Scene: copy it once here, before
        // this frame renders the incoming Scene over it. When the capture is
        // unavailable (first frame, resized surface) the change is downgraded
        // to the Fade ghost path instead of blanking the output.
        if self.store.take_pending_transition_capture().is_some() {
            let captured = self
                .compositor
                .as_mut()
                .is_some_and(Compositor::capture_transition_source);
            if !captured && self.store.fall_back_scene_transition_to_fade() {
                eprintln!(
                    "engine: WARN no outgoing frame to capture; Scene transition falls back to Fade"
                );
            }
        }

        // 1d. Advance the Scene/performance state machine before rendering
        // (`loop.js:94-111`). Cheap when idle: `scenes_tick_due` is false
        // unless performance mode runs or a crossfade is active.
        if self.store.scenes_tick_due() {
            let delta = dt_duration.as_secs_f64() * 1000.0;
            if let Err(error) = self
                .store
                .dispatch("scenes/tick", serde_json::json!({ "delta": delta }))
            {
                eprintln!("engine: WARN scenes/tick failed: {error}");
            }
            if let Some(supervisor) = &mut self.supervisor {
                supervisor.broadcast_deltas(&mut self.store);
            }
        }

        // 1d2. Apply OSC datagrams that arrived since the last frame, before
        // the Input Links read `osc.data` (`node-osc` delivers asynchronously).
        self.store.drain_osc();

        // 1e. Evaluate Input Links against the frame the engine's feed (or the
        // renderer's pump) produced, then render with the updated props
        // (`worker/loop.js` runs the link loop before the renderers). `now` is
        // the monotonic frame clock; it drives the 10 Hz mirror-delta throttle.
        let audio = self.store.audio().clone();
        self.store.update_input_links(&audio, self.start.elapsed());
        if let Some(supervisor) = &mut self.supervisor {
            supervisor.broadcast_deltas(&mut self.store);
        }

        // 1f. Resolve the whole-scene transition for this frame. A shader
        // effect needs its captured outgoing frame; when the capture vanished
        // (a resize between frames invalidated it) downgrade to Fade without
        // dropping the change. Fade/None/unknown kinds stay on the alpha path,
        // so any capture from a previous change is released.
        let mut scene_transition = None;
        if let Some(transition) = self.store.performance_runtime().transition.clone()
            && let Some(effect) = transition.shader_effect()
        {
            let captured = self
                .compositor
                .as_ref()
                .is_some_and(Compositor::has_transition_capture);
            if captured {
                let prepared = self
                    .compositor
                    .as_mut()
                    .is_some_and(|compositor| compositor.prepare_scene_transition(effect));
                if prepared {
                    scene_transition = Some(SceneTransitionInfo {
                        effect,
                        progress: transition.progress(),
                        from_groups: transition
                            .from
                            .iter()
                            .map(|(group_id, _)| group_id.clone())
                            .collect(),
                    });
                } else if self.store.fall_back_scene_transition_to_fade() {
                    eprintln!(
                        "engine: WARN the shader transition pipeline failed to build; Scene transition falls back to Fade"
                    );
                }
            } else if self.store.fall_back_scene_transition_to_fade() {
                eprintln!(
                    "engine: WARN lost the captured outgoing frame; Scene transition falls back to Fade"
                );
            }
        }
        if scene_transition.is_none()
            && let Some(compositor) = &mut self.compositor
        {
            compositor.clear_transition_capture();
        }

        let frame_info = FrameInfo {
            time_seconds: self.start.elapsed().as_secs_f32(),
            frame_index: self.frames_done,
            date: wall_clock_date(self.start.elapsed().as_secs_f32()),
            // Either the renderer's audio pump committed the analyser payload
            // over the control socket (`meyda/SET_BANDS`) or the engine's own
            // feed just wrote it (`set_engine_audio`, WS-9): the store holds
            // the latest frame — bands, damping lanes, the 16-cell grid, wave,
            // FFT magnitudes, sample rate and the Meyda scalars.
            audio,
            row_fades: self
                .store
                .performance_runtime()
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
        };

        // 2. Render the state into the compositor's main texture.
        if let Some(compositor) = &mut self.compositor {
            if let Err(error) = compositor.render_frame(self.store.state(), &frame_info) {
                eprintln!("error: compositor frame failed: {error}");
                self.failed = true;
                self.fail_pending_previews();
                event_loop.exit();
                return;
            }
        }

        let Some(surface) = &mut self.surface else {
            return;
        };
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        let (frame, suboptimal) = match surface.current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Occluded => {
                // Window is behind another window or minimised; nothing to do.
                self.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.reconfigure(&engine.device);
                self.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                // Transient: the compositor did not signal the acquire fence in
                // time (GPU contention). Skip the frame; only repeated
                // timeouts in a row are fatal.
                self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
                if self.consecutive_timeouts == 1 || self.consecutive_timeouts % 60 == 0 {
                    eprintln!(
                        "engine: WARN surface acquire timed out ({} in a row); skipping frame",
                        self.consecutive_timeouts
                    );
                }
                if self.consecutive_timeouts >= 120 {
                    eprintln!("error: surface acquire timed out 120 times in a row; engine stops");
                    self.failed = true;
                    event_loop.exit();
                } else {
                    self.request_redraw();
                }
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                eprintln!("error: failed to acquire a surface texture (validation error)");
                self.failed = true;
                event_loop.exit();
                return;
            }
        };

        self.consecutive_timeouts = 0;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = engine
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("engine-frame"),
            });
        if let Some(compositor) = &mut self.compositor {
            compositor.present(&mut encoder, &view, surface.format());
        }

        // 2a. Blit the same main texture into every Output Window, in this
        // frame's encoder. No second render happens: the present pass is a
        // fullscreen sample of `main`, so the windows always show exactly the
        // primary frame (stretched to their own size).
        let mut output_frames = acquire_output_frames(
            &mut self.output_windows,
            &mut self.compositor,
            &mut encoder,
            &engine.device,
        );
        engine.queue.submit(Some(encoder.finish()));
        engine.queue.present(frame);
        if suboptimal {
            surface.reconfigure(&engine.device);
        }

        // Present the Output Window swapchains (and reconfigure the ones that
        // reported a suboptimal image).
        for (window_id, output_frame, suboptimal) in output_frames.drain(..) {
            engine.queue.present(output_frame);
            if suboptimal && let Some(output) = self.output_windows.get_mut(&window_id) {
                output.surface.reconfigure(&engine.device);
            }
        }

        // 2b. Answer queued preview requests (`preview/module` Gallery
        // thumbnails and `preview/scene` Scene hover cells). Bounded per frame
        // so a burst of hover requests cannot stall present; the rest stay
        // queued for the next frame. Answered after the present so the
        // read-back cost lands between frames.
        self.answer_previews();

        // 3. Publish a preview frame at the supervisor's cadence (≤ 30 fps).
        if let Some(supervisor) = &mut self.supervisor {
            if supervisor.preview_due() && self.compositor.is_some() {
                let mut preview = None;
                if let Some(ring) = supervisor.preview_ring_mut() {
                    if let Some(compositor) = &mut self.compositor {
                        preview = compositor.read_preview(ring).unwrap_or_else(|error| {
                            if !self.preview_error_logged {
                                self.preview_error_logged = true;
                                eprintln!("engine: WARN preview read-back failed: {error}");
                            }
                            None
                        });
                    }
                }
                if let Some(frame) = preview {
                    supervisor.publish_preview(frame);
                }
            }
        }

        if let Some(stats) = &mut self.stats {
            stats.record_frame(dt_duration);
        }
        self.frames_done += 1;

        // Publish the UI status metrics once a second: the legacy worker owned
        // `metrics/SET_FPS_MEASURE` and `size/SET_SIZE`, so engine mode showed
        // "0 FPS" without this.
        self.metric_frames += 1;
        let metric_now = Instant::now();
        let metric_elapsed = metric_now.saturating_duration_since(self.last_metric);
        if metric_elapsed >= Duration::from_secs(1) {
            let fps = self.metric_frames as f64 / metric_elapsed.as_secs_f64();
            self.store
                .push_delta("metrics/SET_FPS_MEASURE", serde_json::json!(fps));
            let (width, height) = self.surface_size;
            if width > 0 && height > 0 {
                self.store.push_delta(
                    "size/SET_SIZE",
                    serde_json::json!({ "width": width, "height": height, "dpr": 1.0 }),
                );
            }
            self.metric_frames = 0;
            self.last_metric = metric_now;
            if let Some(supervisor) = &mut self.supervisor {
                supervisor.broadcast_deltas(&mut self.store);
            }
        }

        if let Some(supervisor) = &mut self.supervisor {
            supervisor.note_frame_presented(&self.store);
            if supervisor.shutdown_requested() {
                self.finish_and_exit(event_loop);
                return;
            }
        }

        let reached_frames = self
            .cli
            .frames
            .map(|limit| self.frames_done >= limit)
            .unwrap_or(false);
        let reached_deadline = self
            .bench_deadline
            .map(|limit| self.start.elapsed() >= limit)
            .unwrap_or(false);
        if reached_frames || reached_deadline {
            self.finish_and_exit(event_loop);
        } else {
            self.request_redraw();
        }
    }

    /// Answer up to [`MAX_PREVIEWS_PER_FRAME`] queued preview requests,
    /// leaving the rest for the next frame.
    ///
    /// The supervisor only queues requests (it has no compositor), and the
    /// reply can block on a GPU read-back, so the work is capped here; the cap
    /// is shared between Gallery thumbnails (`preview/module`) and Scene hover
    /// cells (`preview/scene`). Before the window/compositor exist the queue
    /// simply waits; when the run stops, [`Self::fail_pending_previews`]
    /// answers whatever is left.
    fn answer_previews(&mut self) {
        let (Some(supervisor), Some(compositor)) = (&mut self.supervisor, &mut self.compositor)
        else {
            return;
        };

        let mut requests = supervisor.pending_previews();
        if requests.is_empty() {
            return;
        }
        // Keep the leftover requests queued, in their original order.
        let overflow = if requests.len() > MAX_PREVIEWS_PER_FRAME {
            requests.split_off(MAX_PREVIEWS_PER_FRAME)
        } else {
            Vec::new()
        };
        supervisor.requeue_previews(overflow);

        let registry = self.store.registry_arc();
        for request in requests {
            let result = match &request.kind {
                PreviewKind::Module {
                    module_name,
                    with_output,
                } => compositor.render_module_preview(
                    registry.as_ref(),
                    module_name,
                    request.width,
                    request.height,
                    *with_output,
                ),
                PreviewKind::Scene {
                    scene_id,
                    row_index,
                } => scene_preview::render_scene_preview(
                    compositor,
                    &self.store,
                    scene_id,
                    *row_index,
                    request.width,
                    request.height,
                ),
            };
            supervisor.complete_preview(request, result);
        }
    }

    /// Answer every queued preview request with an error (the engine is
    /// stopping, so no further frame will render it).
    fn fail_pending_previews(&mut self) {
        if let Some(supervisor) = &mut self.supervisor {
            supervisor.reject_pending_previews(
                "engine_shutting_down",
                "the engine stopped before rendering the preview",
            );
        }
    }

    /// Ask the window for another redraw, if it still exists.
    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Print the final report and leave the event loop (idempotent).
    fn finish_and_exit(&mut self, event_loop: &ActiveEventLoop) {
        if self.done {
            event_loop.exit();
            return;
        }
        self.done = true;
        self.fail_pending_previews();
        let resolution = self
            .surface
            .as_ref()
            .map(SurfaceTarget::dimensions)
            .unwrap_or((self.cli.width, self.cli.height));
        if let Some(stats) = self.stats.take() {
            let report: FrameReport = stats.finish();
            if self.cli.bench {
                let json = report_json(&report, resolution, self.cli.audio_fixture, &self.adapter);
                match serde_json::to_string_pretty(&json) {
                    Ok(text) => println!("{text}"),
                    Err(err) => eprintln!("error: failed to serialise the frame report: {err}"),
                }
            }
            eprintln!(
                "frames={} fps={:.2} p99={:.3}ms",
                report.frames, report.frames_per_second, report.p99_ms
            );
        }
        event_loop.exit();
    }
}

/// Build the winit attributes for an Output Window.
///
/// Sizes are logical (CSS) pixels: winit scales them by the target monitor's
/// DPI, so a 300 × 300 window keeps the legacy popup's apparent size on any
/// display. An explicit position is applied only when both axes are present;
/// otherwise the window manager places the window (`window.open` without
/// `top`).
fn output_window_attributes(spec: &WindowSpec, event_loop: &ActiveEventLoop) -> WindowAttributes {
    let monitor = spec
        .fullscreen
        .then(|| {
            spec.monitor
                .and_then(|index| event_loop.available_monitors().nth(index))
        })
        .flatten();
    output_window_attributes_for(spec, monitor)
}

/// [`output_window_attributes`] with the monitor already resolved, so the
/// window plan is testable without a display/driver.
fn output_window_attributes_for(
    spec: &WindowSpec,
    monitor: Option<winit::monitor::MonitorHandle>,
) -> WindowAttributes {
    let mut attributes = WindowAttributes::default()
        .with_title(&spec.title)
        .with_inner_size(LogicalSize::new(
            f64::from(spec.width),
            f64::from(spec.height),
        ))
        .with_resizable(true);

    if let (Some(x), Some(y)) = (spec.x, spec.y) {
        attributes = attributes.with_position(LogicalPosition::new(f64::from(x), f64::from(y)));
    }

    if spec.fullscreen {
        attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(monitor)));
    }

    attributes
}

/// Acquire every Output Window's swapchain image and encode its present pass.
///
/// Returns the images to present after the frame submit, with their winit id
/// and whether the surface reported `Suboptimal` (reconfigure afterwards). A
/// window that is occluded, lost, timing out or in validation fails is skipped
/// for this frame: the primary output is never held back by a secondary
/// surface. Encoded passes sample the same main texture the primary present
/// uses, so there is no second render.
fn acquire_output_frames(
    output_windows: &mut HashMap<WindowId, OutputWindow>,
    compositor: &mut Option<Compositor>,
    encoder: &mut wgpu::CommandEncoder,
    device: &wgpu::Device,
) -> Vec<(WindowId, wgpu::SurfaceTexture, bool)> {
    let mut frames = Vec::new();
    let Some(compositor) = compositor.as_mut() else {
        return frames;
    };

    for (window_id, output) in output_windows.iter_mut() {
        let (frame, suboptimal) = match output.surface.current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Occluded => continue,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                output.surface.reconfigure(device);
                continue;
            }
            wgpu::CurrentSurfaceTexture::Timeout => continue,
            wgpu::CurrentSurfaceTexture::Validation => {
                eprintln!(
                    "engine: WARN output window {} lost its surface (validation error)",
                    output.id
                );
                continue;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        compositor.present(encoder, &view, output.surface.format());
        frames.push((*window_id, frame, suboptimal));
    }

    frames
}

impl ApplicationHandler for WindowedApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.failed {
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
        let is_main = Some(window_id) == self.main_window_id;
        match event {
            WindowEvent::CloseRequested => {
                if is_main {
                    self.finish_and_exit(event_loop);
                } else {
                    // Dropping the window closes it; the store removes the
                    // spec and emits `windows/REMOVE_WINDOW`.
                    self.finish_output_window(window_id);
                }
            }
            WindowEvent::Destroyed => {
                // A window the compositor/window manager destroyed without a
                // CloseRequested (e.g. a display change) must not linger in the
                // registry.
                if !is_main {
                    self.finish_output_window(window_id);
                }
            }
            WindowEvent::Resized(size) => {
                if !is_main {
                    // The Output Window's surface follows its own size; the
                    // compositor/main texture keep the primary resolution and
                    // the present pass stretches it to this window.
                    if let (Some(output), Some(engine)) =
                        (self.output_windows.get_mut(&window_id), &self.engine)
                    {
                        output
                            .surface
                            .resize(&engine.device, size.width, size.height);
                    }
                    return;
                }
                if let (Some(surface), Some(engine)) = (&mut self.surface, &self.engine) {
                    surface.resize(&engine.device, size.width, size.height);
                }
                if size.width > 0 && size.height > 0 {
                    if let Some(compositor) = &mut self.compositor {
                        compositor.resize(size.width, size.height);
                    }
                    let dpr = self
                        .window
                        .as_ref()
                        .map(|window| window.scale_factor())
                        .unwrap_or(1.0);
                    self.surface_size = (size.width, size.height);
                    self.store.push_delta(
                        "size/SET_SIZE",
                        serde_json::json!({
                            "width": size.width,
                            "height": size.height,
                            "dpr": dpr,
                        }),
                    );
                }
            }
            WindowEvent::RedrawRequested => {
                // Output Windows are redrawn by the primary frame's present
                // pass; only the primary window drives the render loop.
                if is_main {
                    self.draw_frame(event_loop);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers.insert(window_id, modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed || event.repeat {
                    return;
                }
                let modifiers = self.modifiers.get(&window_id).copied().unwrap_or_default();
                let leaving = self
                    .window_for(window_id)
                    .is_some_and(|window| window.fullscreen().is_some());
                if is_fullscreen_toggle_key(&event.logical_key, modifiers)
                    || (leaving && is_fullscreen_exit_key(&event.logical_key))
                {
                    self.toggle_fullscreen(window_id);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                // A left double-click toggles that window's fullscreen,
                // mirroring the legacy output window's `dblclick` handler.
                if button != MouseButton::Left {
                    return;
                }
                let double = self
                    .double_click
                    .entry(window_id)
                    .or_default()
                    .register(Instant::now(), button);
                if double {
                    self.toggle_fullscreen(window_id);
                }
            }
            _ => {}
        }
    }
}

/// `iDate`: `[year, month (0-based), weekday (0 = Sunday), monotonic seconds]`,
/// built from the system clock exactly like the JS `Date` getters
/// (`renderers/shader/index.js:98-121`). Calendar conversion is Howard
/// Hinnant's `civil_from_days` over Unix days, so it works without a
/// chrono/time dependency.
fn wall_clock_date(monotonic_seconds: f32) -> [f32; 4] {
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = unix_seconds.div_euclid(86_400);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };

    // 1970-01-01 was a Thursday; JS `getDay()` is 0 = Sunday.
    let weekday = (days + 4).rem_euclid(7);

    [
        year as f32,
        (month - 1) as f32,
        weekday as f32,
        monotonic_seconds,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_toggles_fullscreen_without_command_modifiers() {
        let f = Key::Character("f".into());
        let shift_f = Key::Character("F".into());
        assert!(is_fullscreen_toggle_key(&f, ModifiersState::default()));
        assert!(is_fullscreen_toggle_key(&shift_f, ModifiersState::SHIFT));
        assert!(!is_fullscreen_toggle_key(&f, ModifiersState::CONTROL));
        assert!(!is_fullscreen_toggle_key(&f, ModifiersState::ALT));
        assert!(!is_fullscreen_toggle_key(&f, ModifiersState::SUPER));
        assert!(!is_fullscreen_toggle_key(
            &Key::Character("g".into()),
            ModifiersState::default()
        ));
        assert!(is_fullscreen_exit_key(&Key::Named(NamedKey::Escape)));
        assert!(!is_fullscreen_exit_key(&f));
    }

    #[test]
    fn double_click_needs_two_presses_within_the_window() {
        let start = Instant::now();
        let mut tracker = DoubleClickTracker::default();
        assert!(!tracker.register(start, MouseButton::Left));
        assert!(tracker.register(start + Duration::from_millis(200), MouseButton::Left));
        // A triple click only toggles once: the pair cleared the state.
        assert!(!tracker.register(start + Duration::from_millis(350), MouseButton::Left));
        assert!(tracker.register(start + Duration::from_millis(500), MouseButton::Left));

        let mut slow = DoubleClickTracker::default();
        assert!(!slow.register(start, MouseButton::Left));
        assert!(!slow.register(
            start + Duration::from_millis(DOUBLE_CLICK_MS + 1),
            MouseButton::Left
        ));

        // A right press does not pair with a left press.
        let mut mixed = DoubleClickTracker::default();
        assert!(!mixed.register(start, MouseButton::Left));
        assert!(!mixed.register(start + Duration::from_millis(50), MouseButton::Right));
        assert!(!mixed.register(start + Duration::from_millis(100), MouseButton::Left));
        assert!(mixed.register(start + Duration::from_millis(150), MouseButton::Left));
    }

    #[test]
    fn wall_clock_date_has_the_shader_uniform_shape() {
        let date = wall_clock_date(12.5);
        // Year is sane for this project's lifetime, month is 0-based.
        assert!(date[0] >= 2024.0 && date[0] < 2100.0, "year {}", date[0]);
        assert!((0.0..12.0).contains(&date[1]), "month {}", date[1]);
        assert!((0.0..7.0).contains(&date[2]), "weekday {}", date[2]);
        assert!(date[2].fract() == 0.0, "weekday is an integer");
        assert_eq!(date[3], 12.5);
    }

    #[test]
    fn output_window_attributes_use_logical_geometry_and_optional_placement() {
        let spec = WindowSpec::from_request("w".into(), crate::windows::NewWindow::default());
        let attributes = output_window_attributes_for(&spec, None);
        assert_eq!(attributes.title, "VmodV Output");
        assert_eq!(
            attributes.inner_size,
            Some(winit::dpi::Size::Logical(LogicalSize::new(300.0, 300.0)))
        );
        assert_eq!(attributes.position, None);
        assert_eq!(attributes.fullscreen, None);
        assert!(attributes.resizable);

        let placed = WindowSpec {
            x: Some(64),
            y: Some(32),
            fullscreen: true,
            ..spec.clone()
        };
        let attributes = output_window_attributes_for(&placed, None);
        assert_eq!(
            attributes.position,
            Some(winit::dpi::Position::Logical(LogicalPosition::new(
                64.0, 32.0
            )))
        );
        assert_eq!(attributes.fullscreen, Some(Fullscreen::Borderless(None)));

        // A single axis is not a placement: the window manager decides.
        let half_placed = WindowSpec {
            x: Some(10),
            y: None,
            ..spec
        };
        assert_eq!(
            output_window_attributes_for(&half_placed, None).position,
            None
        );
    }

    /// Frames the multi-window smoke test presents before exiting.
    #[cfg(target_os = "linux")]
    const SMOKE_FRAMES: u32 = 3;
    /// Hard cap on `about_to_wait` iterations, so a hidden/wedged surface
    /// cannot hang the manual smoke test.
    #[cfg(target_os = "linux")]
    const SMOKE_MAX_ATTEMPTS: u32 = 900;

    /// The event loop for the smoke test.
    ///
    /// `cargo test` runs each test on a worker thread, and winit refuses to
    /// create an event loop off the main thread unless the platform extension
    /// allows it. Both Linux backends expose the same switch (it sets one
    /// shared flag), so calling it through either trait is enough; the Wayland
    /// trait is used because the crate always builds with default features.
    #[cfg(target_os = "linux")]
    fn smoke_event_loop() -> EventLoop<()> {
        use winit::platform::wayland::EventLoopBuilderExtWayland;

        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        builder.build().expect("create the winit event loop")
    }

    /// Manual multi-surface smoke test.
    ///
    /// This is the only check that creates a *second* `wgpu::Surface` and
    /// presents it from the same device/queue as the primary, which is exactly
    /// what `windows/createWindow` does in the engine loop. It needs a display
    /// and a GPU adapter, so it is env-gated and a no-op in CI.
    ///
    /// Build it in the Docker toolchain, then run the test binary on a host
    /// with a display (the `modv-rust` build image has no X11 client libraries,
    /// and the host has no cargo by design):
    ///
    /// ```text
    /// make engine-test ARGS="-p modv-app --release --no-run"
    /// MODV_ENGINE_WINDOW_SMOKE=1 DISPLAY=:0 \
    ///     crates/target/release/build/modv-app/*/out/modv_engine-* \
    ///     window::tests::output_window_smoke_presents_a_second_surface --nocapture
    /// ```
    ///
    /// It opens two 96 × 96 windows, presents three cleared frames to both
    /// surfaces and exits; any failure to create or acquire either surface is
    /// reported instead of hanging (there is an attempt cap).
    #[cfg(target_os = "linux")]
    #[test]
    fn output_window_smoke_presents_a_second_surface() {
        if std::env::var_os("MODV_ENGINE_WINDOW_SMOKE").is_none() {
            eprintln!(
                "skipping the multi-window smoke test: set MODV_ENGINE_WINDOW_SMOKE=1 \
                 and provide a display/GPU to run it"
            );
            return;
        }

        let event_loop = smoke_event_loop();
        let mut smoke = TwoWindowSmoke::default();
        event_loop.run_app(&mut smoke).expect("run the smoke loop");

        assert_eq!(smoke.failure, None);
        assert_eq!(smoke.surfaces_created, 2);
        assert_eq!(smoke.frames, SMOKE_FRAMES);
    }

    /// Minimal handler for [`output_window_smoke_presents_a_second_surface`].
    #[cfg(target_os = "linux")]
    #[derive(Default)]
    struct TwoWindowSmoke {
        engine: Option<Engine>,
        windows: Vec<(Arc<Window>, SurfaceTarget)>,
        surfaces_created: usize,
        frames: u32,
        attempts: u32,
        failure: Option<String>,
    }

    #[cfg(target_os = "linux")]
    impl ApplicationHandler for TwoWindowSmoke {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if !self.windows.is_empty() || self.failure.is_some() {
                return;
            }

            let engine = match Engine::new(&modv_render::EngineConfig::default()) {
                Ok(engine) => engine,
                Err(error) => {
                    self.failure = Some(format!("no GPU adapter: {error}"));
                    event_loop.exit();
                    return;
                }
            };

            // The primary window is Fifo (as in `init`); the extra one is the
            // Output Window path (`AutoNoVsync`).
            for (index, present_mode) in [wgpu::PresentMode::Fifo, wgpu::PresentMode::AutoNoVsync]
                .into_iter()
                .enumerate()
            {
                let attributes = WindowAttributes::default()
                    .with_title(format!("modv-engine smoke {index}"))
                    .with_inner_size(LogicalSize::new(96.0, 96.0));
                let window = match event_loop.create_window(attributes) {
                    Ok(window) => Arc::new(window),
                    Err(error) => {
                        self.failure = Some(format!("create window {index}: {error}"));
                        event_loop.exit();
                        return;
                    }
                };
                match create_surface_with_present_mode(&engine, Arc::clone(&window), present_mode) {
                    Ok(surface) => {
                        self.windows.push((window, surface));
                        self.surfaces_created += 1;
                    }
                    Err(error) => {
                        self.failure = Some(format!("create surface {index}: {error}"));
                        event_loop.exit();
                        return;
                    }
                }
            }

            self.engine = Some(engine);
            event_loop.set_control_flow(ControlFlow::Poll);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            if matches!(event, WindowEvent::CloseRequested) {
                event_loop.exit();
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.windows.len() != 2 || self.frames >= SMOKE_FRAMES || self.failure.is_some() {
                return;
            }
            let Some(engine) = self.engine.as_ref() else {
                return;
            };

            self.attempts += 1;
            if self.attempts > SMOKE_MAX_ATTEMPTS {
                self.failure = Some("timed out waiting for both surfaces".to_string());
                event_loop.exit();
                return;
            }

            let mut presented = 0;
            for (_, surface) in self.windows.iter_mut() {
                // A freshly mapped window can be occluded for a frame or two;
                // retry rather than failing the smoke test.
                let frame = match surface.current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame) => frame,
                    wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        surface.reconfigure(&engine.device);
                        continue;
                    }
                    other => {
                        eprintln!("smoke: surface not ready yet ({other:?})");
                        continue;
                    }
                };
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    engine
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("modv-smoke"),
                        });
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("modv-smoke-clear"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
                engine.queue.submit(Some(encoder.finish()));
                engine.queue.present(frame);
                presented += 1;
            }

            if presented == self.windows.len() {
                self.frames += 1;
            }
            if self.frames >= SMOKE_FRAMES {
                event_loop.exit();
            }
        }
    }
}
