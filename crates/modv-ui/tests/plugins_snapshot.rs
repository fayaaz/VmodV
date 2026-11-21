//! Golden snapshot of the VmodV Plugins panel.
//!
//! The panel is the VmodV counterpart of Electron's opt-in plugin host: the
//! consent gate and Advanced warning at the top, discovered scripts with their
//! status/capture size, generated prop controls and the deduplicated runtime
//! log window. The fixture is deterministic (fixed paths and values), so the
//! image is reproducible on every run.
//!
//! Run `cargo test -p modv-ui --test plugins_snapshot`; the first run writes
//! `tests/snapshots/plugins_panel.png` and the next compares against it.

use egui_kittest::Harness;
use modv_ui::state::SidePanelTab;
use modv_ui::{
    EngineSnapshot, PluginInfoView, PluginPropKindView, PluginPropOptionView, PluginPropView,
    PluginView, UiState,
};

fn prop(
    name: &str,
    kind: PluginPropKindView,
    value: serde_json::Value,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
) -> PluginPropView {
    PluginPropView {
        name: name.to_string(),
        kind,
        default: value.clone(),
        value,
        min,
        max,
        step,
        options: Vec::new(),
    }
}

fn plugins_snapshot() -> EngineSnapshot {
    let lumen = PluginInfoView {
        path: "/home/demo/VmodV/plugins/lumen.js".to_string(),
        name: "Lumen".to_string(),
        author: "Ari".to_string(),
        version: "1.4.0".to_string(),
        enabled: true,
        active: true,
        loaded: true,
        failed: false,
        error: None,
        source_bytes: 2048,
        capture: Some([160, 90]),
        props: vec![
            PluginPropView {
                min: Some(0.0),
                max: Some(2.0),
                step: Some(0.001),
                ..prop(
                    "gain",
                    PluginPropKindView::Float,
                    serde_json::json!(1.25),
                    None,
                    None,
                    None,
                )
            },
            prop(
                "mirror",
                PluginPropKindView::Bool,
                serde_json::json!(true),
                None,
                None,
                None,
            ),
            prop(
                "cycles",
                PluginPropKindView::Int,
                serde_json::json!(4),
                Some(1.0),
                Some(16.0),
                Some(1.0),
            ),
            prop(
                "tint",
                PluginPropKindView::Color,
                serde_json::json!({ "r": 0.1, "g": 0.8, "b": 0.4, "a": 1.0 }),
                None,
                None,
                None,
            ),
            PluginPropView {
                options: vec![
                    PluginPropOptionView {
                        label: "Wide".to_string(),
                        value: serde_json::json!("wide"),
                    },
                    PluginPropOptionView {
                        label: "Square".to_string(),
                        value: serde_json::json!("square"),
                    },
                ],
                ..prop(
                    "shape",
                    PluginPropKindView::Select,
                    serde_json::json!("wide"),
                    None,
                    None,
                    None,
                )
            },
            prop(
                "label",
                PluginPropKindView::Text,
                serde_json::json!("chroma"),
                None,
                None,
                None,
            ),
        ],
    };
    let glitch = PluginInfoView {
        path: "/home/demo/VmodV/plugins/glitch.js".to_string(),
        name: "Glitch".to_string(),
        author: "Kai".to_string(),
        version: "0.3.0".to_string(),
        enabled: true,
        active: false,
        loaded: false,
        failed: true,
        error: Some("load failed: unexpected token at line 4".to_string()),
        source_bytes: 512,
        capture: None,
        props: Vec::new(),
    };
    let sparks = PluginInfoView {
        path: "/home/demo/VmodV/plugins/sparks.js".to_string(),
        name: "Sparks".to_string(),
        author: "modV".to_string(),
        version: "2.0.0".to_string(),
        enabled: false,
        active: false,
        loaded: false,
        failed: false,
        error: None,
        source_bytes: 1024,
        capture: None,
        props: Vec::new(),
    };
    EngineSnapshot {
        plugins: PluginView {
            enabled: true,
            consented: true,
            loaded_count: 1,
            plugins: vec![lumen, glitch, sparks],
            logs: vec![
                "plugin: info Lumen: init".to_string(),
                "plugin: warn Lumen: capture is 160x90, upscaled from the output".to_string(),
                "plugin: error Glitch: load failed: unexpected token at line 4".to_string(),
                "plugin: info Sparks: disabled by the user".to_string(),
            ],
            capture: Some([160, 90]),
        },
        ..Default::default()
    }
}

#[test]
fn plugins_panel_snapshot() {
    let snapshot = plugins_snapshot();
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Plugins;
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();
    harness.snapshot("plugins_panel");
}
