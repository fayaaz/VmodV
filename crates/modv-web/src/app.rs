//! The wasm/eframe shell: render the real compositor, then draw the real
//! `modv-ui` shell over its main texture.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::Arc;

use eframe::egui;
use wasm_bindgen::prelude::*;

use modv_app::compositor::{Compositor, FrameInfo};
use modv_app::module_preview::ModulePreviews;
use modv_render::wgpu;
use modv_ui::model::{
    PreviewRequest, PreviewTexture, RowMonitorView, gallery_preview_key, scene_preview_key,
};
use modv_ui::state::{UiCommand, UiState};

use crate::pacing::{FramePacer, TARGET_FPS};
use crate::state::WebState;

/// Start the browser shell on `canvas_id`.
///
/// `web/main.js` creates the canvas once, then calls this after the wasm
/// module is initialised.
#[wasm_bindgen]
pub async fn start(canvas_id: &str) -> Result<(), JsValue> {
    run(canvas_id, false).await
}

/// Start the full-window output view on `canvas_id`.
///
/// `web/output.js` opens this in the output tab. It restores the persisted
/// live stack and Scenes but never writes them back, so the main tab remains
/// the only author of the saved state.
#[wasm_bindgen]
pub async fn start_output(canvas_id: &str) -> Result<(), JsValue> {
    run(canvas_id, true).await
}

async fn run(canvas_id: &str, output_only: bool) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    log(if output_only {
        "start_output: entered"
    } else {
        "start: entered"
    });

    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("the canvas is missing"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("the canvas element has the wrong type"))?;

    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |cc| Ok(Box::new(WebApp::new(cc, output_only)?))),
        )
        .await
}

struct WebApp {
    state: WebState,
    compositor: Compositor,
    ui_state: UiState,
    preview_texture: Option<egui::TextureId>,
    render_size: [u32; 2],
    webgl_fallback: bool,
    pacer: FramePacer,
    last_render_time: f64,
    start_time: f64,
    frames: u64,
    fps: f32,
    frame_ms: f32,
    resizes: u64,
    last_persist: f64,
    output_only: bool,
    previews: ModulePreviews,
    gallery_previews: HashMap<String, PreviewTexture>,
    scene_previews: HashMap<String, PreviewTexture>,
    preview_order: VecDeque<String>,
    row_monitors: HashMap<usize, RowMonitorEntry>,
}

struct RowMonitorEntry {
    group_id: String,
    texture: Option<PreviewTexture>,
    width: u32,
    height: u32,
}

impl WebApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        output_only: bool,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        log(if output_only {
            "WebApp::new (output): entering"
        } else {
            "WebApp::new: entering"
        });
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .ok_or_else(|| std::io::Error::other("eframe did not create a wgpu render state"))?;
        // The default wgpu handler panics on validation errors; a browser
        // tab should stay alive when a WebGL2 fallback (or an unsupported
        // Point10 compute scene) cannot build a pipeline. Log and let the
        // compositor skip the module instead.
        render_state.device.on_uncaptured_error(Arc::new(|error| {
            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                "[modv-web] wgpu: {error}"
            )));
        }));
        log(&format!(
            "WebApp::new: backend={:?} target_format={:?}",
            render_state.adapter.get_info().backend,
            render_state.target_format
        ));

        modv_ui::theme::apply(&cc.egui_ctx);

        let adapter_info = render_state.adapter.get_info();
        let backend = match adapter_info.backend.to_str() {
            "webgpu" => "webgpu",
            "gl" => "webgl2",
            other => other,
        };
        let mut state = WebState::new();
        let restored =
            crate::storage::load().is_some_and(|document| state.restore_persisted_json(&document));
        state.set_backend(
            format!("{} ({})", adapter_info.name, adapter_info.backend),
            backend,
        );
        // A shader rings module over a browser-native Point10 scene: both
        // paths go through the real compositor and registry. A restored
        // session keeps exactly the modules the user left active.
        let selected_module = if restored {
            state
                .state()
                .groups
                .iter()
                .flat_map(|group| group.modules.iter())
                .next()
                .cloned()
        } else {
            let bands = state.add_module("Bands").ok_or_else(|| {
                std::io::Error::other("the built-in registry is missing the Bands module")
            })?;
            let _ = state.add_module("Point10 Chromatic Wave");
            Some(bands)
        };

        let compositor = Compositor::with_registry(
            &render_state.device,
            &render_state.queue,
            state.registry_arc(),
        );
        let previews = ModulePreviews::new(&render_state.device, &render_state.queue);

        log("WebApp::new: compositor created");
        let mut ui_state = UiState::new();
        ui_state.show_p0 = false;
        ui_state.selected_group = Some(state.group_id().to_string());
        ui_state.selected_module = selected_module;

        Ok(Self {
            state,
            compositor,
            ui_state,
            preview_texture: None,
            render_size: [1, 1],
            webgl_fallback: backend == "webgl2",
            // Two compositors are expensive; the output tab is a visual
            // monitor, so 30 fps halves its GPU cost without visible judder.
            pacer: FramePacer::new(if output_only { 30.0 } else { TARGET_FPS }),
            last_render_time: 0.0,
            start_time: 0.0,
            frames: 0,
            fps: 0.0,
            frame_ms: 0.0,
            resizes: 0,
            last_persist: 0.0,
            output_only,
            previews,
            gallery_previews: HashMap::new(),
            scene_previews: HashMap::new(),
            preview_order: VecDeque::new(),
            row_monitors: HashMap::new(),
        })
    }

    fn render_target_size(&self, ctx: &egui::Context) -> [u32; 2] {
        let rect = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        let width = (rect.width() * ppp).round().max(0.0) as u32;
        let height = (rect.height() * ppp).round().max(0.0) as u32;
        // Keep the internal targets bounded at the native reference size
        // (960x540). The browser canvas can be a 4K surface, and a couple of
        // Point10 scenes at 4K would stall the tab; 960x540 keeps the 60 fps
        // output budget reachable on ordinary GPUs.
        let max_width = 960;
        if width > max_width {
            let height = (u64::from(height) * u64::from(max_width) / u64::from(width)) as u32;
            [max_width, height.max(1)]
        } else {
            [width.max(1), height.max(1)]
        }
    }

    fn update_preview_texture(&mut self, frame: &eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else {
            return;
        };
        let Some(view) = self.compositor.main_texture_view() else {
            return;
        };
        let (width, height) = self.compositor.main_texture_size();
        if width == 0 || height == 0 {
            return;
        }
        let mut renderer = render_state.renderer.write();
        match self.preview_texture {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &render_state.device,
                &view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                let id = renderer.register_native_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                );
                self.preview_texture = Some(id);
            }
        }
        self.ui_state.preview_texture = self.preview_texture;
        self.ui_state.preview_size = [width as usize, height as usize];
        self.ui_state.p0.shared_texture_ok = true;
        self.render_size = [width, height];
    }

    /// Publish a small JSON snapshot on `window.__modvStats` for browser
    /// soak tests. This is a debug hook; the app never reads it.
    fn publish_stats(&self) {
        if !self.frames.is_multiple_of(30) {
            return;
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let audio = self.state.audio();
        let stats = format!(
            "{{\"fps\":{:.2},\"frame_ms\":{:.2},\"frames\":{},\"resizes\":{},\"width\":{},\"height\":{},\"webgl2\":{},\"preview\":{},\"bass\":{:.3},\"mid\":{:.3},\"treble\":{:.3},\"bpm\":{:.1},\"modules\":{},\"scenes\":{},\"rows\":{}}}",
            self.fps,
            self.frame_ms,
            self.frames,
            self.resizes,
            self.render_size[0],
            self.render_size[1],
            self.webgl_fallback,
            self.preview_texture.is_some(),
            audio.bass,
            audio.mid,
            audio.treble,
            self.state.bpm(),
            self.state.state().modules.len(),
            self.state.scene_library().scenes().len(),
            self.row_monitors.len()
        );
        let _ = js_sys::Reflect::set(
            &window,
            &wasm_bindgen::JsValue::from_str("__modvStats"),
            &wasm_bindgen::JsValue::from_str(&stats),
        );
    }

    /// The top module in a Scene cell, used for its hover thumbnail.
    fn scene_preview_module(&self, scene_id: &str, row: usize) -> Option<String> {
        let scene = self.state.scene_library().find(scene_id)?;
        let ids = scene
            .get("groups")?
            .as_array()?
            .get(row)?
            .get("modules")?
            .as_array()?;
        let modules = scene.get("modules")?.as_object()?;
        ids.iter().find_map(|id| {
            modules
                .get(id.as_str()?)?
                .get("$moduleName")?
                .as_str()
                .map(str::to_string)
        })
    }

    /// Render one hover preview into a GPU texture registered with egui.
    fn answer_preview(&mut self, request: PreviewRequest, frame: &eframe::Frame, now: f64) {
        let (key, module_name, with_output, is_scene) = match request {
            PreviewRequest::Module {
                module_name,
                with_output,
            } => (
                gallery_preview_key(&module_name),
                module_name,
                with_output,
                false,
            ),
            PreviewRequest::Scene {
                scene_id,
                row_index,
            } => {
                let Some(module_name) = self.scene_preview_module(&scene_id, row_index) else {
                    return;
                };
                (
                    scene_preview_key(&scene_id, row_index),
                    module_name,
                    false,
                    true,
                )
            }
        };
        let info = FrameInfo {
            time_seconds: (now - self.start_time) as f32,
            frame_index: self.frames,
            date: browser_date(),
            audio: self.state.audio().clone(),
            row_fades: Vec::new(),
            scene_transition: None,
        };
        let base = with_output
            .then(|| self.compositor.main_texture_view())
            .flatten();
        let Ok(view) = self.previews.render_view(
            self.state.registry(),
            &module_name,
            128,
            72,
            &info,
            base.as_ref(),
        ) else {
            return;
        };
        let Some(render_state) = frame.wgpu_render_state() else {
            return;
        };
        let mut renderer = render_state.renderer.write();
        let existing = if is_scene {
            self.scene_previews.get(&key)
        } else {
            self.gallery_previews.get(&key)
        }
        .map(|preview| preview.id);
        let id = match existing {
            Some(id) => {
                renderer.update_egui_texture_from_wgpu_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                    id,
                );
                id
            }
            None => {
                let id = renderer.register_native_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                );
                let texture = PreviewTexture {
                    id,
                    width: 128,
                    height: 72,
                };
                if is_scene {
                    self.scene_previews.insert(key.clone(), texture);
                } else {
                    self.gallery_previews.insert(key.clone(), texture);
                }
                self.preview_order.push_back(key.clone());
                id
            }
        };
        while self.preview_order.len() > 32 {
            let Some(old) = self.preview_order.pop_front() else {
                break;
            };
            let texture = self
                .scene_previews
                .remove(&old)
                .or_else(|| self.gallery_previews.remove(&old));
            if let Some(texture) = texture {
                renderer.free_texture(&texture.id);
            }
        }
        let _ = id;
    }

    /// Refresh at most one row's live gutter monitor this frame.
    fn refresh_row_monitors(&mut self, frame: &eframe::Frame) {
        let live: Vec<(usize, String)> = {
            let mut rows = std::collections::BTreeMap::new();
            for group in &self.state.state().groups {
                if group.transition_ghost == Some(true) {
                    continue;
                }
                if let Some(row) = group.scene_row {
                    rows.entry(row).or_insert_with(|| group.id.clone());
                }
            }
            rows.into_iter().collect()
        };
        let (width, height) = self.compositor.main_texture_size();
        if live.is_empty() || width == 0 || height == 0 {
            if !self.row_monitors.is_empty() {
                if let Some(render_state) = frame.wgpu_render_state() {
                    let mut renderer = render_state.renderer.write();
                    for (_, entry) in self.row_monitors.drain() {
                        if let Some(texture) = entry.texture {
                            renderer.free_texture(&texture.id);
                        }
                    }
                } else {
                    self.row_monitors.clear();
                }
            }
            return;
        }
        let live_map: HashMap<usize, String> = live.iter().cloned().collect();
        let stale: Vec<usize> = self
            .row_monitors
            .iter()
            .filter(|(row, entry)| live_map.get(row) != Some(&entry.group_id))
            .map(|(row, _)| *row)
            .collect();
        for row in stale {
            if let Some(entry) = self.row_monitors.remove(&row)
                && let Some(texture) = entry.texture
                && let Some(render_state) = frame.wgpu_render_state()
            {
                render_state.renderer.write().free_texture(&texture.id);
            }
        }
        for (row, group_id) in &live_map {
            let entry = self
                .row_monitors
                .entry(*row)
                .or_insert_with(|| RowMonitorEntry {
                    group_id: group_id.clone(),
                    texture: None,
                    width,
                    height,
                });
            if entry.group_id != *group_id || entry.width != width || entry.height != height {
                if let Some(texture) = entry.texture.take()
                    && let Some(render_state) = frame.wgpu_render_state()
                {
                    render_state.renderer.write().free_texture(&texture.id);
                }
                entry.group_id = group_id.clone();
                entry.width = width;
                entry.height = height;
            }
        }
        let Some(index) = modv_ui::panels::row_monitor_refresh_row(self.frames, live.len()) else {
            return;
        };
        let (row, group_id) = &live[index];
        if !self.compositor.refresh_row_monitor(group_id) {
            if self.frames.is_multiple_of(60) {
                log(&format!(
                    "row monitor: no rendered texture for {group_id} (row {row})"
                ));
            }
            return;
        }
        let Some(view) = self.compositor.row_monitor_view(group_id) else {
            return;
        };
        let Some(render_state) = frame.wgpu_render_state() else {
            return;
        };
        let mut renderer = render_state.renderer.write();
        let Some(entry) = self.row_monitors.get_mut(row) else {
            return;
        };
        entry.texture = match entry.texture {
            Some(texture) => {
                renderer.update_egui_texture_from_wgpu_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                    texture.id,
                );
                Some(texture)
            }
            None => Some(PreviewTexture {
                id: renderer.register_native_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                ),
                width,
                height,
            }),
        };
    }

    fn handle_command(&mut self, command: UiCommand, ctx: &egui::Context) {
        match command {
            UiCommand::Quit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            // The browser's Output Window is a chrome-less second engine
            // instance in a new tab. The browser may ask for pop-up
            // permission the first time; the page shows a toast if it is
            // blocked.
            UiCommand::OpenOutputWindow | UiCommand::ToggleOutputFullscreen => {
                if !call_window_function("openModvOutput") {
                    open_output_tab();
                }
            }
            UiCommand::CloseOutputWindow => {
                call_window_function("closeModvOutput");
            }
            UiCommand::SetOutputFullscreen(on) => {
                if on {
                    if !call_window_function("openModvOutput") {
                        open_output_tab();
                    }
                } else {
                    call_window_function("closeModvOutput");
                }
            }
            UiCommand::SetRenderScale(scale) => {
                self.compositor.set_render_scale(scale);
                self.ui_state.render_scale = scale;
            }
            // Files, presets, output windows, profiles and logs are not part
            // of the browser cut line yet.
            _ => {}
        }
    }
}

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|input| input.time);
        if self.start_time == 0.0 {
            self.start_time = now;
        }

        // The compositor advances on a fixed 60 Hz deadline grid. egui may
        // still call this method for input events; those frames draw the UI
        // over the last output texture without advancing audio or the scene.
        if self.pacer.due(now) {
            let dt = if self.last_render_time == 0.0 {
                1.0 / TARGET_FPS
            } else {
                (now - self.last_render_time).clamp(0.0, 0.1)
            };
            self.last_render_time = now;
            // A 60 Hz deadline grid on a 144 Hz display renders in alternating
            // two/three-rAF gaps. Smooth the frame time and derive FPS from it
            // so the status bar shows the sustained rate instead of
            // flickering between 48 and 72, and never reports above the cap.
            let instant_ms = (dt * 1000.0) as f32;
            self.frame_ms = if self.frame_ms <= 0.0 {
                instant_ms
            } else {
                self.frame_ms * 0.9 + instant_ms * 0.1
            };
            self.fps = if self.frame_ms > 0.0 {
                (1000.0 / self.frame_ms).min(TARGET_FPS as f32)
            } else {
                0.0
            };

            // Silence until the user explicitly enables the microphone.
            // Nothing is captured or synthesized on open. Once enabled, the
            // native analyser supplies the bands, tempo and BeatDetektor kick.
            let (audio, bpm, kick, phrase_drop) = match crate::mic::frame(dt as f32) {
                Some(frame) => (frame.audio, frame.bpm, frame.kick, frame.phrase_drop),
                None => (Default::default(), None, false, false),
            };
            self.state.set_audio(audio.clone());
            self.state.set_tempo(bpm, kick);
            // Audio/draw/tween links write into the props before the
            // compositor reads the state this frame.
            self.state.evaluate_links();
            // Performance mode advances rows on their own dwell timers plus
            // the analyser's phrase/drop pulse.
            self.state.tick_performance(dt as f32, phrase_drop);

            let target = self.render_target_size(&ctx);
            if target != [self.compositor.size().0, self.compositor.size().1] {
                self.compositor.resize(target[0], target[1]);
                self.resizes = self.resizes.saturating_add(1);
            }

            // A shader transition captures the outgoing frame while main
            // still holds it, then the compositor runs the effect over the
            // captured and incoming frames.
            if self.state.take_pending_transition_capture().is_some()
                && !self.compositor.capture_transition_source()
            {
                self.state.fall_back_transition_to_fade();
            }
            if let Some(info) = self.state.scene_transition_info()
                && (!self.compositor.has_transition_capture()
                    || !self.compositor.prepare_scene_transition(&info.effect))
            {
                self.state.fall_back_transition_to_fade();
            }
            let scene_transition = self.state.scene_transition_info();

            let info = FrameInfo {
                time_seconds: (now - self.start_time) as f32,
                frame_index: self.frames,
                date: browser_date(),
                audio,
                row_fades: self.state.row_fade_infos(),
                scene_transition,
            };
            if let Err(error) = self.compositor.render_frame(self.state.state(), &info) {
                if self.frames < 5 {
                    log(&format!("compositor error: {error}"));
                }
                self.state.set_error(Some(format!("compositor: {error}")));
            } else {
                self.state.set_error(None);
            }
            self.state.tick_transitions(dt as f32);
            self.frames = self.frames.saturating_add(1);
            self.state.note_frame();
            self.update_preview_texture(frame);
            if !self.output_only {
                self.refresh_row_monitors(frame);
            }
            self.pacer.mark(now);
            self.publish_stats();
        }

        // The output tab is a second engine instance: no panels, just the
        // compositor's main texture filling the window.
        if self.output_only {
            ctx.set_cursor_icon(egui::CursorIcon::None);
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
            if let Some(texture) = self.preview_texture {
                ui.painter().image(
                    texture,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            // The output window has no widgets asking for animation, so keep
            // the frame loop alive explicitly; the pacer still caps the
            // compositor at 30 fps.
            ctx.request_repaint();
            return;
        }

        let mut snapshot = self
            .state
            .snapshot(self.render_size, self.fps, self.frame_ms);
        snapshot.gallery_previews = self.gallery_previews.clone();
        snapshot.scene_previews = self.scene_previews.clone();
        snapshot.row_monitors = self
            .row_monitors
            .iter()
            .map(|(row, entry)| {
                (
                    *row,
                    RowMonitorView {
                        group_id: entry.group_id.clone(),
                        texture: entry.texture,
                    },
                )
            })
            .collect();
        let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
        snapshot.health.output_open = fullscreen;
        snapshot.health.output_fullscreen = fullscreen;
        self.ui_state.ensure_default_group_focus(&snapshot);
        self.ui_state.sync_status(&snapshot);
        if self.ui_state.selected_module.is_none() {
            self.ui_state.selected_module =
                snapshot.modules.first().map(|module| module.id.clone());
        }

        modv_ui::shell::draw(ui, &snapshot, &mut self.ui_state);

        if let Some(request) = self.ui_state.take_preview_requests().into_iter().last() {
            self.answer_preview(request, frame, now);
        }

        let actions = self.ui_state.take_actions();
        // Scene/Group/Module edits are structural and saved immediately;
        // slider drags are throttled below.
        let structural = actions.iter().any(|action| {
            let name = action.action.as_str();
            name.starts_with("scenes/")
                || name.starts_with("groups/")
                || name.starts_with("modules/")
                || name.starts_with("inputs/")
                || name.starts_with("tweens/")
        });
        for action in actions {
            if !self.state.apply_action(&action) {
                // The UI emits actions for panels the browser shell has cut
                // (Media, NDI/OSC/MIDI, plugins). Ignore those rather than
                // turning every click into a status-bar error.
            }
        }
        for command in self.ui_state.take_commands() {
            self.handle_command(command, &ctx);
        }

        // Persist the live stack and Scenes. A tab hide/unload flushes
        // immediately; ordinary prop edits are throttled so dragging a slider
        // does not serialise the whole document every frame.
        if self.state.is_dirty() {
            let hidden = web_sys::window()
                .and_then(|window| window.document())
                .is_some_and(|document| {
                    document.visibility_state() == web_sys::VisibilityState::Hidden
                });
            if hidden || structural || now - self.last_persist >= 0.4 {
                crate::storage::save(&self.state.to_persisted_json());
                self.state.clear_dirty();
                self.last_persist = now;
            }
        }

        // Wake at the next 60 Hz deadline. UI-only input frames before it
        // simply redraw over the current output texture.
        ctx.request_repaint_after(self.pacer.delay(now));
    }
}

/// Call a function installed on `window` by `web/main.js`.
fn call_window_function(name: &str) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(function) = js_sys::Reflect::get(&window, &JsValue::from_str(name)) else {
        return false;
    };
    function
        .dyn_ref::<js_sys::Function>()
        .is_some_and(|function| function.call0(&window).is_ok())
}

/// Fallback when `main.js` has not installed its output helpers yet.
fn open_output_tab() {
    if let Some(window) = web_sys::window() {
        let _ = window.open_with_url_and_target("./output.html", "vmodv-output");
    }
}

fn log(message: &str) {
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "[modv-web] {message}"
    )));
}

/// `iDate` / ISF DATE: `[year, month (1-based), weekday (0 = Sunday),
/// seconds since midnight]`, in UTC.
fn browser_date() -> [f32; 4] {
    let date = js_sys::Date::new_0();
    [
        date.get_utc_full_year() as f32,
        (date.get_utc_month() + 1) as f32,
        date.get_utc_day() as f32,
        (date.get_utc_hours() * 3600 + date.get_utc_minutes() * 60 + date.get_utc_seconds()) as f32,
    ]
}
