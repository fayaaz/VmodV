//! The Help frame below Groups, replacing the legacy Electron "Info View".
//!
//! It shows a short description of the feature the user last clicked: a Group,
//! a Module, a right-panel tab, a Gallery tile or a Scene. Module clicks also
//! surface that module's licence. These are interaction tests, not golden images.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget, module_license};
use modv_ui::{
    EngineSnapshot, GroupView, ModuleMetaView, ModuleView, PerformanceView, SceneCellView,
    SceneView, UiState,
};

fn group(id: &str, name: &str) -> GroupView {
    GroupView {
        id: id.to_string(),
        name: name.to_string(),
        enabled: GROUP_ENABLED,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        inherit_from: InheritTarget::Previous,
        clearing: true,
        pipeline: false,
        module_ids: Vec::new(),
    }
}

const MODULES_HELP: &str = "Modules are the visuals that render inside a Group. Drag a tile from \
    the Module Gallery onto a Group to add one, or double-click the tile to add it to the focused \
    Group. Select a Module to edit its properties in the Inspector, link its controls to audio, \
    MIDI or tweens, reorder it with its drag handle, or remove it with x.";

fn module_meta() -> ModuleMetaView {
    ModuleMetaView {
        name: "Bands".to_string(),
        module_type: "shader".to_string(),
        kind: "shader".to_string(),
        category: "effects".to_string(),
        author: "modV".to_string(),
        version: "1.0".to_string(),
        description: String::new(),
    }
}

fn module_view(id: &str, group_id: &str) -> ModuleView {
    ModuleView {
        id: id.to_string(),
        module_name: "Bands".to_string(),
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

fn harness(groups: Vec<GroupView>) -> Harness<'static, UiState> {
    harness_with_snapshot(EngineSnapshot {
        groups,
        ..Default::default()
    })
}

fn harness_with_snapshot(snapshot: EngineSnapshot) -> Harness<'static, UiState> {
    Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            UiState::new(),
        )
}

#[test]
fn the_help_frame_introduces_itself_before_anything_is_clicked() {
    let mut harness = harness(Vec::new());
    harness.run();

    harness.get_by_label(
        "The Help panel shows a short description of the feature you last clicked. \
         Click a Group, Gallery tile, tab or Scene to read about it here.",
    );
}

#[test]
fn the_help_frame_fills_the_remaining_panel_height() {
    let mut harness = harness(Vec::new());
    harness.run();

    let frame = harness.get_by_label("Help frame").rect();
    assert!(
        frame.height() > 200.0,
        "the Help frame should stretch down the left panel, got {frame:?}"
    );
    assert!(
        frame.bottom() > 700.0,
        "the Help frame should reach the bottom of the panel, got {frame:?}"
    );
}

#[test]
fn clicking_a_group_shows_help_about_groups() {
    let mut harness = harness(vec![group("group-help", "Help Target")]);
    harness.run();

    harness
        .get_by_role_and_label(accesskit::Role::Button, "Help Target")
        .click();
    harness.run();

    harness.get_by_label(
        "Groups contain Modules. Modules within Groups can be rearranged to change \
         the drawing order. Groups can also be rearranged by dragging their title bar.",
    );
}

#[test]
fn clicking_a_tab_shows_help_about_that_panel() {
    let mut harness = harness(Vec::new());
    harness.run();

    harness.get_by_label("NDI").click();
    harness.run();

    harness.get_by_label(
        "Configure your NDI settings here. Click Discover Sources to search for NDI sources \
         available on your network. Click Enable to create an NDI receiver so VmodV can consume \
         the NDI source. Once a receiver has been created, click Remove Receiver to stop VmodV \
         consuming the NDI source.",
    );
}

#[test]
fn clicking_a_gallery_tile_shows_module_help_and_its_licence() {
    let meta = module_meta();
    let snapshot = EngineSnapshot {
        registered: vec![meta.clone()],
        ..Default::default()
    };
    let mut harness = harness_with_snapshot(snapshot);
    harness.run();

    harness
        .get_by_role_and_label(accesskit::Role::Button, "Bands")
        .click();
    harness.run();

    harness.get_by_label(MODULES_HELP);
    assert_module_licence(&mut harness, &meta);
}

#[test]
fn clicking_a_module_row_shows_module_help_and_its_licence() {
    let meta = module_meta();
    let mut group = group("group-1", "Foreground");
    group.module_ids = vec!["module-1".to_string()];
    let snapshot = EngineSnapshot {
        groups: vec![group],
        modules: vec![module_view("module-1", "group-1")],
        registered: vec![meta.clone()],
        ..Default::default()
    };
    let mut harness = harness_with_snapshot(snapshot);
    harness.run();

    harness
        .get_all_by_label_contains("Bands  (")
        .next()
        .expect("the module row in the Groups panel")
        .click();
    harness.run();

    harness.get_by_label(MODULES_HELP);
    assert_module_licence(&mut harness, &meta);
}

fn assert_module_licence(harness: &mut Harness<'_, UiState>, meta: &ModuleMetaView) {
    harness.get_by_label(&format!("Module: {}", meta.name));
    harness.get_by_label(&format!("Author: {}", meta.author));
    harness.get_by_label(&format!("License: {}", module_license(meta)));
}

#[test]
fn clicking_a_scene_cell_shows_help_about_scenes() {
    let snapshot = EngineSnapshot {
        scenes: vec![SceneView {
            id: "scene-a".to_string(),
            name: "Alpha".to_string(),
            enabled: true,
            timing: None,
            cells: vec![SceneCellView {
                row: 0,
                module_names: vec!["Bands".to_string()],
                disabled: false,
            }],
        }],
        scene_rows: vec!["Background".to_string()],
        performance: PerformanceView {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut harness = harness_with_snapshot(snapshot);
    harness.run();

    harness
        .get_by_role_and_label(accesskit::Role::Button, "Alpha Group 1")
        .click();
    harness.run();

    harness.get_by_label(
        "Scenes are full snapshots of the app: every Group, Module, Input Link, Expression, \
         Tween and MIDI device is saved using the same JSON format as a preset. The grid shows \
         Scenes as columns and Groups as lanes: click a column header to apply a whole Scene, or \
         a cell to swap just that Group. Performance mode runs through the enabled Scenes, in \
         order or randomly, switching on musical phrase changes detected from the bass audio band \
         and crossfading the Groups between Scenes. Each Group independently waits a random time \
         between the default min and max cell times before it advances, so Groups change on their \
         own schedule rather than all at once. Ctrl-drag a grid cell onto another Scene to copy \
         its Group exactly. A Scene JSON can be opened like a preset, and all Scenes can be \
         exported and imported as a playlist JSON.",
    );
}
