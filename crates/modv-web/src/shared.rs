//! In-page hand-off to the hidden output renderer.
//!
//! The main shell owns the authoritative state. A chrome-less second
//! `eframe` app renders that exact state into an offscreen canvas; the output
//! window displays the canvas capture stream. Both apps run in one JavaScript
//! thread, so a thread-local carries the render frame without any cross-tab
//! serialisation or a second engine simulation.

use std::cell::RefCell;
use std::sync::Arc;

use wasm_bindgen::JsValue;

use modv_app::compositor::RowFadeInfo;
use modv_app::module_registry::ModuleRegistry;
use modv_app::scene_transition::SceneTransitionInfo;
use modv_engine::engine_state::EngineState;
use modv_renderers::two_d::AudioFrame;

/// Everything the hidden renderer needs to reproduce one main-shell frame.
#[derive(Clone)]
pub struct SharedFrame {
    /// The authoritative Groups, modules, props and Group/Scene opacities.
    pub state: EngineState,
    /// The analyser frame the main shell rendered with.
    pub audio: AudioFrame,
    /// Active row crossfades.
    pub row_fades: Vec<RowFadeInfo>,
    /// Active whole-Scene shader transition, if any.
    pub scene_transition: Option<SceneTransitionInfo>,
    /// Shared animation clock (`iGlobalTime`).
    pub time_seconds: f32,
    /// Shared `iDate` tuple.
    pub date: [f32; 4],
    /// Shared frame counter (`iFrame`).
    pub frame_index: u64,
    /// The main compositor's render-target size.
    pub render_size: [u32; 2],
}

thread_local! {
    static FRAME: RefCell<Option<SharedFrame>> = const { RefCell::new(None) };
    static REGISTRY: RefCell<Option<Arc<ModuleRegistry>>> = const { RefCell::new(None) };
    static CAPTURE: RefCell<bool> = const { RefCell::new(false) };
}

/// Register the main shell's module registry for the hidden renderer.
pub fn set_registry(registry: Arc<ModuleRegistry>) {
    REGISTRY.with(|slot| *slot.borrow_mut() = Some(registry));
}

/// The main shell's registry, if it has started.
pub fn registry() -> Option<Arc<ModuleRegistry>> {
    REGISTRY.with(|slot| slot.borrow().clone())
}

/// Whether an output window is currently open. The main shell only pays the
/// state-clone cost while it is.
pub fn output_active() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    js_sys::Reflect::get(&window, &JsValue::from_str("__modvOutputActive"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Publish the frame the main shell is about to render.
pub fn publish(frame: SharedFrame) {
    FRAME.with(|slot| *slot.borrow_mut() = Some(frame));
}

/// Take the latest frame for the hidden renderer.
pub fn take() -> Option<SharedFrame> {
    FRAME.with(|slot| slot.borrow_mut().take())
}

/// Ask the hidden renderer to capture its previous frame for a shader
/// transition.
pub fn request_capture() {
    CAPTURE.with(|slot| *slot.borrow_mut() = true);
}

/// Consume a pending transition-capture request.
pub fn take_capture_request() -> bool {
    CAPTURE.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}
