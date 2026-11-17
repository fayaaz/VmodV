//! Right-panel tab drag and drop in the native shell.
//!
//! A strip tab (and a floating window's grip) is a drag source carrying a
//! typed `TabDrag` payload. The tab strip, the dock column and the central
//! area advertise themselves as drop targets while a drag is in flight:
//! dropping on the strip returns the tab to the strip, dropping on the dock
//! column docks it, and dropping on the central area floats it at the release
//! position. A release outside every target cancels and changes nothing.
//!
//! These tests drive real pointer events, matching `gallery_drag_drop.rs`.

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineSnapshot, LayoutState, TabPlacement, UiState};

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

fn center(harness: &Harness<'_, UiState>, label: &str) -> egui::Pos2 {
    harness.get_by_label(label).rect().center()
}

/// Press on a drag source and move far enough for `drag_started` to fire.
/// Targets are queried afterwards, once the drag payload exists.
fn start_drag(harness: &mut Harness<'static, UiState>, from: egui::Pos2) {
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(0.0, 24.0));
    harness.run();
}

/// Hover a target, then release the primary button there.
fn release_at(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

#[test]
fn dragging_a_strip_tab_onto_the_dock_area_docks_it() {
    let mut harness = app();
    harness.run();

    let tab = center(&harness, "INSPECTOR");
    start_drag(&mut harness, tab);

    // The dock column advertises itself while the drag is in flight even when
    // it is empty.
    let dock = center(&harness, "Dock here");
    release_at(&mut harness, dock);

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Docked
    );
    assert_eq!(
        harness.state().layout.tabs.docked_tabs(),
        vec![SidePanelTab::Inspector]
    );
    assert!(
        !harness
            .state()
            .layout
            .tabs
            .strip_tabs()
            .any(|tab| tab == SidePanelTab::Inspector),
        "the strip no longer shows the docked tab"
    );
    assert!(
        harness.query_by_label("Back to tabs").is_some(),
        "the dock section keeps its existing Dock to tabs control"
    );
    harness.snapshot("tab_drag_docked");
}

#[test]
fn dragging_a_strip_tab_onto_the_central_area_floats_it_at_the_release() {
    let mut harness = app();
    harness.run();

    let tab = center(&harness, "AUDIO");
    start_drag(&mut harness, tab);
    let target = center(&harness, "GALLERY");
    release_at(&mut harness, target);

    assert_eq!(
        harness.state().layout.tabs.placement(SidePanelTab::Audio),
        TabPlacement::Floating
    );
    let bounds = harness
        .state()
        .layout
        .window(&LayoutState::tab_window_id(SidePanelTab::Audio))
        .expect("the drop records the floating window bounds");
    assert!(
        (bounds.x - target.x).abs() <= 1.0 && (bounds.y - target.y).abs() <= 1.0,
        "the window opens at the release position ({:?}), got {}x{} at ({}, {})",
        target,
        bounds.width,
        bounds.height,
        bounds.x,
        bounds.y
    );
    assert!(
        harness.query_by_label("AUDIO").is_none(),
        "the floated tab leaves the strip"
    );
}

#[test]
fn dragging_a_floating_grip_onto_the_strip_returns_the_tab() {
    let mut harness = app();
    harness.run();

    // Detach Inspector through its compact icon, then drag the floating
    // window's grip back onto the strip.
    harness
        .get_all_by_label("Detach to a floating window")
        .next()
        .expect("Inspector detach icon")
        .click();
    harness.run();
    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Floating
    );

    let grip = center(&harness, "Drag Inspector window");
    start_drag(&mut harness, grip);
    let strip = center(&harness, "MEDIA");
    release_at(&mut harness, strip);

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Tabs
    );
    assert!(
        harness.query_by_label("Drag Inspector window").is_none(),
        "the floating window is gone after the grip drop"
    );
    harness.get_by_label("INSPECTOR");
}

#[test]
fn the_tab_context_menu_detaches_and_docks() {
    let mut harness = app();
    harness.run();

    let tab = center(&harness, "INSPECTOR");
    harness.hover_at(tab);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: tab,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run();
    }

    harness.get_by_label("Dock to area");
    harness.get_by_label("Detach to floating window").click();
    harness.run();

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Floating
    );
}

#[test]
fn a_cancelled_tab_drag_changes_no_placement() {
    let mut harness = app();
    harness.run();
    let before = harness.state().layout.clone();

    let tab = center(&harness, "INSPECTOR");
    start_drag(&mut harness, tab);
    let groups = center(&harness, "GROUPS");
    release_at(&mut harness, groups);

    assert_eq!(
        harness.state().layout.tabs,
        before.tabs,
        "a release outside every target leaves placements untouched"
    );
    assert!(
        harness
            .state()
            .layout
            .window(&LayoutState::tab_window_id(SidePanelTab::Inspector))
            .is_none(),
        "no floating window bounds were recorded"
    );
    harness.get_by_label("INSPECTOR");
}

#[test]
fn a_cancelled_floating_grip_drag_leaves_the_tab_floating() {
    let mut harness = app();
    harness.run();
    harness
        .get_all_by_label("Detach to a floating window")
        .next()
        .expect("Inspector detach icon")
        .click();
    harness.run();

    let grip = center(&harness, "Drag Inspector window");
    start_drag(&mut harness, grip);
    let groups = center(&harness, "GROUPS");
    release_at(&mut harness, groups);

    assert_eq!(
        harness
            .state()
            .layout
            .tabs
            .placement(SidePanelTab::Inspector),
        TabPlacement::Floating,
        "a release outside every target leaves a floating window floating"
    );
    harness.get_by_label("Drag Inspector window");
}

#[test]
fn the_tab_strip_wraps_tabs_in_drag_and_drop_hit_areas() {
    let mut harness = app();
    harness.run();

    // Resting tabs are still ordinary click targets.
    harness.get_by_label("NDI").click();
    harness.run();
    assert_eq!(harness.state().side_panel_tab, SidePanelTab::Ndi);
    harness.get_by_label("OSC").click();
    harness.run();
    assert_eq!(harness.state().side_panel_tab, SidePanelTab::Osc);
}
