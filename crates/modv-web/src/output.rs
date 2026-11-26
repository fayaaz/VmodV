//! Chrome-less renderer for the output window.
//!
//! The main shell publishes the exact `EngineState`, analyser frame,
//! transition progress and clock through [`crate::shared`]. This app renders
//! that state to an offscreen canvas; `web/main.js` captures the canvas and
//! the output window plays it as video. It never owns or mutates Scene state,
//! audio capture or performance timers, so it cannot drift from the main
//! output.

use std::error::Error;

use eframe::egui;
use wasm_bindgen::prelude::*;

use modv_app::compositor::{Compositor, FrameInfo};
use modv_render::wgpu;

use crate::pacing::{FramePacer, TARGET_FPS};
use crate::shared;

/// Start the hidden output renderer on `canvas_id`.
///
/// `web/main.js` creates the offscreen canvas and attaches its capture stream
/// to the output window; this function is never called from the output tab.
#[wasm_bindgen]
pub async fn start_output(canvas_id: &str) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    log("output start: entered");
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("the output canvas is missing"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("the output canvas has the wrong type"))?;

    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| Ok(Box::new(OutputApp::new(cc)?))),
        )
        .await
}

struct OutputApp {
    compositor: Compositor,
    state: modv_engine::engine_state::EngineState,
    audio: modv_renderers::two_d::AudioFrame,
    row_fades: Vec<modv_app::compositor::RowFadeInfo>,
    scene_transition: Option<modv_app::scene_transition::SceneTransitionInfo>,
    time_seconds: f32,
    date: [f32; 4],
    frame_index: u64,
    render_size: [u32; 2],
    has_frame: bool,
    pacer: FramePacer,
    frames: u64,
    preview_texture: Option<egui::TextureId>,
}

impl OutputApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .ok_or_else(|| std::io::Error::other("eframe did not create a wgpu render state"))?;
        render_state
            .device
            .on_uncaptured_error(std::sync::Arc::new(|error| {
                web_sys::console::error_1(&JsValue::from_str(&format!(
                    "[modv-web] output wgpu: {error}"
                )));
            }));
        let registry = shared::registry().ok_or_else(|| {
            std::io::Error::other("the main shell has not registered its module registry")
        })?;
        let compositor =
            Compositor::with_registry(&render_state.device, &render_state.queue, registry);
        log("output app: compositor created");
        Ok(Self {
            compositor,
            state: modv_engine::engine_state::EngineState::new(),
            audio: Default::default(),
            row_fades: Vec::new(),
            scene_transition: None,
            time_seconds: 0.0,
            date: [0.0; 4],
            frame_index: 0,
            render_size: [1, 1],
            has_frame: false,
            pacer: FramePacer::new(TARGET_FPS / 2.0),
            frames: 0,
            preview_texture: None,
        })
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
        self.preview_texture = Some(match self.preview_texture {
            Some(id) => {
                renderer.update_egui_texture_from_wgpu_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                    id,
                );
                id
            }
            None => renderer.register_native_texture(
                &render_state.device,
                &view,
                wgpu::FilterMode::Linear,
            ),
        });
    }

    fn receive(&mut self) {
        if let Some(shared) = shared::take() {
            if !self.has_frame {
                log("output app: received first frame");
            }
            self.state = shared.state;
            self.audio = shared.audio;
            self.row_fades = shared.row_fades;
            self.scene_transition = shared.scene_transition;
            self.time_seconds = shared.time_seconds;
            self.date = shared.date;
            self.frame_index = shared.frame_index;
            self.render_size = shared.render_size;
            self.has_frame = true;
        }
    }

    fn render(&mut self, frame: &eframe::Frame) {
        if !self.has_frame {
            return;
        }
        if shared::take_capture_request() && !self.compositor.capture_transition_source() {
            // The main shell will fall back to the Fade alpha path on its next
            // frame; this frame simply shows the incoming state.
            self.scene_transition = None;
        }
        let mut scene_transition = self.scene_transition.clone();
        if let Some(info) = scene_transition.as_ref()
            && !self.compositor.prepare_scene_transition(&info.effect)
        {
            scene_transition = None;
        }
        let info = FrameInfo {
            time_seconds: self.time_seconds,
            frame_index: self.frame_index,
            date: self.date,
            audio: self.audio.clone(),
            row_fades: self.row_fades.clone(),
            scene_transition,
        };
        if let Err(error) = self.compositor.render_frame(&self.state, &info)
            && self.frames < 5
        {
            log(&format!("output compositor error: {error}"));
        }
        self.frames = self.frames.saturating_add(1);
        if self.frames == 1 {
            let (width, height) = self.compositor.main_texture_size();
            log(&format!(
                "output app: rendered first frame {width}x{height}, preview={}",
                self.preview_texture.is_some()
            ));
        }
        if self.frames.is_multiple_of(30)
            && let Some(window) = web_sys::window()
        {
            let _ = js_sys::Reflect::set(
                &window,
                &JsValue::from_str("__modvOutputStats"),
                &JsValue::from_str(&format!(
                    "{{\"frames\":{},\"preview\":{},\"has_frame\":{}}}",
                    self.frames,
                    self.preview_texture.is_some(),
                    self.has_frame
                )),
            );
        }
        self.update_preview_texture(frame);
    }
}

impl eframe::App for OutputApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !shared::output_active() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            return;
        }
        let now = ctx.input(|input| input.time);
        if self.pacer.due(now) {
            self.receive();
            if self.render_size[0] > 1 || self.render_size[1] > 1 {
                let size = [self.compositor.size().0, self.compositor.size().1];
                if self.render_size != size {
                    self.compositor
                        .resize(self.render_size[0], self.render_size[1]);
                }
            }
            self.render(frame);
            self.pacer.mark(now);
        }

        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
        if let Some(texture) = self.preview_texture {
            ui.painter().image(
                texture,
                fit_preview(rect, self.render_size),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        ctx.request_repaint();
    }
}

fn fit_preview(rect: egui::Rect, size: [u32; 2]) -> egui::Rect {
    let width = size[0].max(1) as f32;
    let height = size[1].max(1) as f32;
    let scale = (rect.width() / width).min(rect.height() / height);
    egui::Rect::from_center_size(rect.center(), egui::vec2(width * scale, height * scale))
}

fn log(message: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[modv-web] {message}")));
}
