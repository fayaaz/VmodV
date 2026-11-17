//! Golden snapshot of the native shell with representative engine content.
//!
//! Run `cargo test -p modv-ui shell_snapshot`; the first run writes
//! `tests/snapshots/shell.png` and the next run compares against it.

use egui_kittest::Harness;
use modv_ui::{
    AudioDeviceView, AudioSelectionView, AudioView, EngineHealth, EngineSnapshot, GroupView,
    ModuleMetaView, ModuleView, PerformanceView, PropView, SceneCellView, SceneView, UiState,
};

fn meta(name: &str, module_type: &str, category: &str) -> ModuleMetaView {
    ModuleMetaView {
        name: name.to_string(),
        module_type: module_type.to_string(),
        category: category.to_string(),
        author: "modV".to_string(),
        version: "1.0".to_string(),
        description: String::new(),
    }
}

fn demo_modules() -> Vec<ModuleMetaView> {
    let modules = vec![
        // Effects.
        meta("Bands", "shader", "effects"),
        meta("Kaleidoscope", "shader", "effects"),
        meta("Twirl", "shader", "effects"),
        meta("Mirror", "isf", "effects"),
        meta("Glow", "isf", "effects"),
        meta("Blur", "isf", "effects"),
        meta("Pixelate", "isf", "effects"),
        meta("Edge Detect", "isf", "effects"),
        meta("Color Shift", "isf", "effects"),
        meta("Feedback", "shader", "effects"),
        meta("Strobe", "shader", "effects"),
        meta("Vignette", "isf", "effects"),
        // 2D Scenes.
        meta("Smoke", "2d", "2d-scenes"),
        meta("Fire Chars", "2d", "2d-scenes"),
        meta("Water Waves", "2d", "2d-scenes"),
        meta("Particle Flow", "2d", "2d-scenes"),
        meta("Lissajous", "2d", "2d-scenes"),
        meta("Concentric", "2d", "2d-scenes"),
        meta("Grid Warp", "2d", "2d-scenes"),
        meta("Plasma", "2d", "2d-scenes"),
        meta("Tunnel Vision", "2d", "2d-scenes"),
        // 3D Scenes.
        meta("Chromatic Line", "2d", "3d-scenes"),
        meta("Tunnel", "isf", "3d-scenes"),
        meta("Deep Sea", "isf", "3d-scenes"),
        meta("Boxy", "isf", "3d-scenes"),
        meta("DVD Room", "isf", "3d-scenes"),
        meta("It Is Coming For You", "isf", "3d-scenes"),
        meta("Rolling Fire", "isf", "3d-scenes"),
        meta("Photon Cloud", "isf", "3d-scenes"),
        meta("Warp Gate", "isf", "3d-scenes"),
        meta("Voxel Garden", "isf", "3d-scenes"),
    ];
    modules
}

fn demo_snapshot() -> EngineSnapshot {
    let group = GroupView {
        id: "group-foreground".to_string(),
        name: "Foreground".to_string(),
        enabled: true,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "screen".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        clearing: true,
        pipeline: false,
        module_ids: vec!["module-1".to_string()],
    };
    let module = ModuleView {
        id: "module-1".to_string(),
        module_name: "Chromatic Line".to_string(),
        group_id: group.id.clone(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "2d".to_string(),
        props: vec![
            PropView {
                name: "gain".to_string(),
                prop_type: "float".to_string(),
                value: serde_json::json!(1.25),
                min: Some(0.0),
                max: Some(2.0),
                step: Some(0.001),
                default: Some(serde_json::json!(1.0)),
                input_id: Some("input-1".to_string()),
                components: Vec::new(),
                tween: None,
                options: Vec::new(),
            },
            PropView {
                name: "enabled".to_string(),
                prop_type: "bool".to_string(),
                value: serde_json::json!(true),
                min: None,
                max: None,
                step: None,
                default: None,
                input_id: None,
                components: Vec::new(),
                tween: None,
                options: Vec::new(),
            },
        ],
    };
    let scene = SceneView {
        id: "scene-1".to_string(),
        name: "Lone Chrome".to_string(),
        enabled: true,
        timing: Some(8.0),
        cells: vec![
            SceneCellView {
                row: 0,
                module_names: vec!["Bands".to_string()],
                disabled: false,
            },
            SceneCellView {
                row: 1,
                module_names: vec!["Chromatic Line".to_string()],
                disabled: true,
            },
        ],
    };
    EngineSnapshot {
        groups: vec![group],
        modules: vec![module],
        scenes: vec![scene],
        registered: demo_modules(),
        performance: PerformanceView {
            enabled: false,
            current_scene_id: Some("scene-1".to_string()),
            row_names: vec!["Foreground".to_string(), "Effects".to_string()],
            min_seconds: 8.0,
            max_seconds: 24.0,
            transition_seconds: 2.0,
            allowed_transitions: vec!["Fade".to_string()],
            order: "in-order".to_string(),
        },
        audio: AudioView {
            bpm: 128.0,
            kick: false,
            bass: 0.42,
            mid: 0.31,
            treble: 0.18,
        },
        resolution: [960, 540],
        fps: 60.0,
        frame_ms: 16.6,
        scene_rows: vec!["Foreground".to_string(), "Effects".to_string()],
        applied_scene_id: Some("scene-1".to_string()),
        health: EngineHealth {
            adapter: "lavapipe".to_string(),
            backend: "Vulkan".to_string(),
            last_error: None,
            frames_presented: 120,
            output_open: true,
            audio_active: false,
        },
        audio_devices: vec![AudioDeviceView {
            id: "alsa:builtin".to_string(),
            name: "Built-in Audio Analog Stereo".to_string(),
            channels: 2,
            capture_sample_rate: 48_000,
        }],
        audio_selection: AudioSelectionView {
            source: "pipewire".to_string(),
            device: None,
            capture_sample_rate: None,
            error: None,
        },
        ..Default::default()
    }
}

#[test]
fn shell_snapshot() {
    let snapshot = demo_snapshot();
    let mut state = UiState::new();
    state.selected_group = Some("group-foreground".to_string());
    state.selected_module = Some("module-1".to_string());
    state.selected_scene = Some("scene-1".to_string());
    state.p0.range_control_ok = true;
    state.p0.scene_cell_ok = true;
    state.p0.shared_texture_ok = true;
    state.p0.output_window_ok = true;
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();
    harness.snapshot("shell");
}
