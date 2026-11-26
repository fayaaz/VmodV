//! The wasm/eframe shell: render the real compositor, then draw the real
//! `modv-ui` shell over its main texture.

use std::error::Error;
use std::sync::Arc;

use eframe::egui;
use wasm_bindgen::prelude::*;

use modv_app::compositor::{Compositor, FrameInfo};
use modv_render::wgpu;
use modv_ui::state::{UiCommand, UiState};

use crate::audio::SimAudio;
use crate::state::WebState;

/// Start the browser shell on `canvas_id`.
///
/// `web/main.js` creates the canvas once, then calls this after the wasm
/// module is initialised.
#[wasm_bindgen]
pub async fn start(canvas_id: &str) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    log("start: entered");

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
            Box::new(|cc| Ok(Box::new(WebApp::new(cc)?))),
        )
        .await
}

struct WebApp {
    state: WebState,
    compositor: Compositor,
    ui_state: UiState,
    audio: SimAudio,
    preview_texture: Option<egui::TextureId>,
    render_size: [u32; 2],
    webgl_fallback: bool,
    last_time: f64,
    start_time: f64,
    frames: u64,
    fps: f32,
    frame_ms: f32,
}

impl WebApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        log("WebApp::new: entering");
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
        state.set_backend(
            format!("{} ({})", adapter_info.name, adapter_info.backend),
            backend,
        );
        // A shader rings module over a browser-native Point10 scene: both
        // paths go through the real compositor and registry.
        let bands = state.add_module("Bands").ok_or_else(|| {
            std::io::Error::other("the built-in registry is missing the Bands module")
        })?;
        let _ = state.add_module("Point10 Chromatic Wave");

        let compositor = Compositor::with_registry(
            &render_state.device,
            &render_state.queue,
            state.registry_arc(),
        );

        log("WebApp::new: compositor created");
        let mut ui_state = UiState::new();
        ui_state.show_p0 = false;
        ui_state.selected_group = Some(state.group_id().to_string());
        ui_state.selected_module = Some(bands);

        Ok(Self {
            state,
            compositor,
            ui_state,
            audio: SimAudio::new(),
            preview_texture: None,
            render_size: [1, 1],
            webgl_fallback: backend == "webgl2",
            last_time: 0.0,
            start_time: 0.0,
            frames: 0,
            fps: 0.0,
            frame_ms: 0.0,
        })
    }

    fn render_target_size(&self, ctx: &egui::Context) -> [u32; 2] {
        let rect = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        let width = (rect.width() * ppp).round().max(0.0) as u32;
        let height = (rect.height() * ppp).round().max(0.0) as u32;
        // Keep the internal targets bounded: the browser canvas can be a 4K
        // surface, and a couple of Point10 scenes at 4K would stall the tab.
        // The WebGL2 software fallback also gets a smaller target.
        let max_width = if self.webgl_fallback { 960 } else { 1280 };
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

    fn handle_command(&mut self, command: UiCommand, ctx: &egui::Context) {
        match command {
            UiCommand::Quit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            UiCommand::ToggleOutputFullscreen => {
                let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            UiCommand::SetOutputFullscreen(on) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
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
        let dt = if self.last_time == 0.0 {
            1.0 / 60.0
        } else {
            (now - self.last_time).clamp(0.0, 0.1)
        };
        self.last_time = now;
        self.frame_ms = (dt * 1000.0) as f32;
        self.fps = if dt > 0.0 { 1.0 / dt as f32 } else { 0.0 };

        let audio = crate::mic::frame(dt as f32).unwrap_or_else(|| self.audio.update(dt as f32));
        self.state.set_audio(audio.clone());

        let target = self.render_target_size(&ctx);
        if target != [self.compositor.size().0, self.compositor.size().1] {
            self.compositor.resize(target[0], target[1]);
        }

        let info = FrameInfo {
            time_seconds: (now - self.start_time) as f32,
            frame_index: self.frames,
            date: browser_date(),
            audio,
            row_fades: Vec::new(),
            scene_transition: None,
        };
        if let Err(error) = self.compositor.render_frame(self.state.state(), &info) {
            if self.frames < 5 {
                log(&format!("compositor error: {error}"));
            }
            self.state.set_error(Some(format!("compositor: {error}")));
        } else {
            self.state.set_error(None);
        }
        self.frames = self.frames.saturating_add(1);
        self.state.note_frame();
        self.update_preview_texture(frame);

        let snapshot = self
            .state
            .snapshot(self.render_size, self.fps, self.frame_ms);
        self.ui_state.ensure_default_group_focus(&snapshot);
        self.ui_state.sync_status(&snapshot);
        if self.ui_state.selected_module.is_none() {
            self.ui_state.selected_module =
                snapshot.modules.first().map(|module| module.id.clone());
        }

        modv_ui::shell::draw(ui, &snapshot, &mut self.ui_state);

        for action in self.ui_state.take_actions() {
            if !self.state.apply_action(&action) {
                // The UI emits actions for panels the browser shell has cut
                // (Scenes, Media, NDI/OSC/MIDI, plugins). Ignore those rather
                // than turning every click into a status-bar error.
            }
        }
        for command in self.ui_state.take_commands() {
            self.handle_command(command, &ctx);
        }

        // The compositor is animated every frame, so keep the browser's
        // requestAnimationFrame loop alive even when egui has no input.
        ctx.request_repaint();
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
