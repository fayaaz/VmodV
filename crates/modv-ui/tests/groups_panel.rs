//! Groups pane regression: every Group shows its module list, not only the
//! selected one. The per-Group editors stay selected-only.
//!
//! These are interaction/smoke tests, not golden images.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, GroupView, ModuleView, UiState};

fn group(id: &str, name: &str, module_id: &str) -> GroupView {
    GroupView {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        clearing: true,
        pipeline: false,
        module_ids: vec![module_id.to_string()],
    }
}

fn module(id: &str, name: &str, group_id: &str) -> ModuleView {
    ModuleView {
        id: id.to_string(),
        module_name: name.to_string(),
        group_id: group_id.to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        props: Vec::new(),
    }
}

#[test]
fn unselected_groups_still_show_their_modules() {
    let snapshot = EngineSnapshot {
        groups: vec![
            group("group-bg", "Background", "bands-1234"),
            group("group-fx", "Effects", "ball-5678"),
        ],
        modules: vec![
            module("bands-1234", "Bands", "group-bg"),
            module("ball-5678", "Ball", "group-fx"),
        ],
        ..Default::default()
    };

    let mut state = UiState::new();
    // Only Background is selected; Effects is collapsed today, so its Ball row
    // must still be rendered by the always-on module list.
    state.selected_group = Some("group-bg".to_string());

    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    harness.get_by_label("Bands  (bands-12)");
    harness.get_by_label("Ball  (ball-567)");
}
