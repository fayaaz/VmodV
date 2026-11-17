//! Kittest coverage for resizable sections and detachable right-panel tabs.
//!
//! The right-panel tabs (Inspector, Media, Audio, Info, Plugins, MIDI) can live
//! in the tab strip, in their own dock column, or in an in-app floating window.
//! These tests drive the real shell buttons and assert both the placement model
//! and the rendered state. Section sizes are captured from the real panel rects,
//! which is what `NativeApp` persists in `native-layout.json`.

use accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineSnapshot, GroupView, ModuleView, PropView, TabPlacement, UiState};

fn app() -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot::default();
    let state = UiState::new();
    let harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness
}

/// A selected module with one real `RangeControl`, so the Inspector tab draws
/// a slider wherever it is placed.
fn app_with_module() -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot {
        groups: vec![GroupView {
            id: "group-1".to_string(),
            name: "Effects".to_string(),
            enabled: true,
            hidden: false,
            alpha: 1.0,
            alpha_input_id: None,
            composite_operation: "normal".to_string(),
            composite_operation_input_id: None,
            inherit: true,
            clearing: true,
            pipeline: false,
            module_ids: vec!["module-1".to_string()],
        }],
        modules: vec![ModuleView {
            id: "module-1".to_string(),
            module_name: "Chromatic Line".to_string(),
            group_id: "group-1".to_string(),
            enabled: true,
            alpha: 1.0,
            alpha_input_id: None,
            composite_operation: "normal".to_string(),
            composite_operation_input_id: None,
            module_type: "2d".to_string(),
            props: vec![PropView {
                name: "gain".to_string(),
                prop_type: "float".to_string(),
                value: serde_json::json!(0.65),
                min: Some(0.0),
                max: Some(1.0),
                step: Some(0.01),
                default: Some(serde_json::json!(0.5)),
                input_id: None,
                components: Vec::new(),
                tween: None,
                options: Vec::new(),
            }],
        }],
        ..Default::default()
    };
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    let harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness
}

fn detach_buttons(harness: &Harness<'_, UiState>) -> usize {
    harness
        .get_all_by_label("Detach to a floating window")
        .count()
}

/// Hover first, then click: the first hover frame can settle a panel-width
/// measurement, and clicking the pinned icon at its pre-settle rect would land
/// on the neighbouring tab.
fn click_first_detach(harness: &mut Harness<'static, UiState>) {
    let pos = harness
        .get_all_by_label("Detach to a floating window")
        .next()
        .expect("Inspector detach control")
        .rect()
        .center();
    harness.hover_at(pos);
    harness.run();
    harness
        .get_all_by_label("Detach to a floating window")
        .next()
        .expect("Inspector detach control")
        .click();
    harness.run();
}

#[test]
fn every_strip_tab_has_a_detach_control() {
    let mut harness = app();
    harness.run();

    assert_eq!(
        detach_buttons(&harness),
        SidePanelTab::COUNT,
        "each tab in the strip has its own float control"
    );
    for tab in SidePanelTab::ALL {
        harness.get_by_label(tab.title().to_uppercase().as_str());
    }
}

#[test]
fn the_ndi_and_osc_tabs_sit_at_the_end_of_the_strip() {
    let mut harness = app();
    harness.run();

    harness.get_by_label("NDI").click();
    harness.run();
    assert_eq!(harness.state().side_panel_tab, SidePanelTab::Ndi);

    harness.get_by_label("OSC").click();
    harness.run();
    assert_eq!(harness.state().side_panel_tab, SidePanelTab::Osc);
}

#[test]
fn detaching_the_inspector_floats_it_and_selects_the_next_tab() {
    let mut harness = app();
    harness.run();

    // The first detach control belongs to the first strip tab (Inspector).
    click_first_detach(&mut harness);

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Floating
    );
    assert_eq!(
        harness.state().side_panel_tab,
        SidePanelTab::Media,
        "the strip selects another tab when the active one leaves"
    );
    assert!(
        harness.query_by_label("INSPECTOR").is_none(),
        "the Inspector is no longer in the tab strip"
    );
    harness.get_by_label("Dock to tabs");
    harness.get_by_label("Dock to area");
    assert_eq!(detach_buttons(&harness), SidePanelTab::COUNT - 1);
    harness.snapshot("tab_float");
}

#[test]
fn dock_to_area_shows_the_dock_column_and_back_to_tabs_restores_the_strip() {
    let mut harness = app();
    harness.run();
    harness
        .get_all_by_label("Detach to a floating window")
        .next()
        .expect("Inspector detach control")
        .click();
    harness.run();

    harness.get_by_label("Dock to area").click();
    harness.run();

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Docked
    );
    assert!(
        harness.query_by_label("Dock to area").is_none(),
        "the floating window is gone after docking"
    );
    harness.get_by_label("INSPECTOR");
    harness.get_by_label("Float");
    harness.get_by_label("Back to tabs");
    assert!(
        harness.query_by_label("INSPECTOR").is_some(),
        "the docked section header names the tab"
    );
    harness.snapshot("tab_docked");

    harness.get_by_label("Back to tabs").click();
    harness.run();

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Tabs
    );
    assert!(harness.query_by_label("Back to tabs").is_none());
    assert_eq!(detach_buttons(&harness), SidePanelTab::COUNT);
    harness.snapshot("tab_back_in_strip");
}

#[test]
fn docking_several_tabs_stacks_them_in_one_dock_column() {
    let mut harness = app();
    harness.run();

    // Float Inspector, dock it, then float and dock Media through the same
    // controls: the dock column stacks both sections.
    click_first_detach(&mut harness);
    harness.get_by_label("Dock to area").click();
    harness.run();
    harness
        .get_all_by_label("Detach to a floating window")
        .next()
        .expect("Media detach control")
        .click();
    harness.run();
    harness.get_by_label("Dock to area").click();
    harness.run();

    let docked = harness.state().layout.tabs.docked_tabs();
    assert_eq!(
        docked,
        vec![SidePanelTab::Inspector, SidePanelTab::Media],
        "docked tabs keep strip order in the stack"
    );
    assert_eq!(harness.get_all_by_label("Back to tabs").count(), 2);
    assert_eq!(harness.get_all_by_label("Float").count(), 2);
}

#[test]
fn a_floating_tab_keeps_its_slider_visible() {
    let mut harness = app_with_module();
    harness.run();

    click_first_detach(&mut harness);

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Floating
    );
    // The floating window really contains the RangeControl: its label and the
    // egui slider itself are both in the window (the old `tab_float` golden had
    // no module, so a missing slider could not show up).
    harness.get_by_label("gain");
    assert!(
        harness.get_all_by_role(Role::Slider).count() >= 2,
        "the module Alpha slider and the gain RangeControl slider are both rendered"
    );
    harness.snapshot("tab_float_range");
}

#[test]
fn a_docked_tab_keeps_its_slider_visible() {
    let mut harness = app_with_module();
    harness.run();

    click_first_detach(&mut harness);
    harness.get_by_label("Dock to area").click();
    harness.run();

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Docked
    );
    harness.get_by_label("gain");
    assert!(
        harness.get_all_by_role(Role::Slider).count() >= 2,
        "the module Alpha slider and the gain RangeControl slider are both rendered"
    );
    // The regression: the dock used to paint `PANEL_ALT` while the slider rail
    // also used `PANEL_ALT`, so the rail disappeared into the dock.
    harness.snapshot("tab_docked_range");
}

#[test]
fn the_live_and_gallery_sections_capture_their_real_heights() {
    let mut harness = app();
    harness.run();

    let live = harness.state().layout.live_height;
    let gallery = harness.state().layout.gallery_height;
    assert!(
        live >= 120.0,
        "the Live Output section is captured from its rect: {live}"
    );
    assert!(
        gallery >= 96.0,
        "the Gallery section is captured from its rect: {gallery}"
    );
    assert!(
        live + gallery < 800.0,
        "the Scenes grid keeps the remainder: {live} + {gallery}"
    );

    // A persisted height is applied before the shell lays the split out; the
    // panel then keeps the user's size across frames.
    let snapshot = EngineSnapshot::default();
    let mut first = true;
    let restored_state = UiState::new();
    let mut restored = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                if first {
                    state.layout.live_height = 220.0;
                    first = false;
                }
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            restored_state,
        );
    restored.run();
    assert!(
        (restored.state().layout.live_height - 220.0).abs() <= 1.0,
        "a saved section height is restored, got {}",
        restored.state().layout.live_height
    );
}

#[test]
fn dragging_the_groups_splitter_updates_the_remembered_width() {
    let mut harness = app();
    harness.run();

    let before = harness.state().layout.groups_width;
    let y = 400.0;
    let edge = egui::pos2(before, y);
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.run();
    harness.hover_at(egui::pos2(before + 40.0, y));
    harness.run();
    harness.drop_at(egui::pos2(before + 40.0, y));
    harness.run();

    let after = harness.state().layout.groups_width;
    assert!(
        after > before + 10.0,
        "dragging the Groups splitter widens the panel and updates the layout: {before} -> {after}"
    );
    assert!(
        after >= 210.0,
        "the Groups panel keeps its minimum width: {after}"
    );
}
