//! Pure tab drag-and-drop resolution.
//!
//! The payload names the tab; the drop target names the placement. A floating
//! drop carries the pointer position and is clamped so the new window can never
//! open off-screen.

use modv_ui::layout::TabPlacement;
use modv_ui::state::SidePanelTab;
use modv_ui::tab_drag::{FLOATING_TAB_SIZE, TabDrag, TabDropTarget, floating_bounds_for_drop};

#[test]
fn the_payload_carries_the_dragged_tab() {
    for tab in SidePanelTab::ALL {
        assert_eq!(TabDrag::new(tab).tab, tab);
    }
}

#[test]
fn every_drop_target_maps_to_its_exclusive_placement() {
    assert_eq!(TabDropTarget::Strip.placement(), TabPlacement::Tabs);
    assert_eq!(TabDropTarget::Dock.placement(), TabPlacement::Docked);
    assert_eq!(
        TabDropTarget::Float(egui::pos2(300.0, 200.0)).placement(),
        TabPlacement::Floating
    );
}

#[test]
fn a_floating_drop_opens_where_it_was_released() {
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
    let bounds = floating_bounds_for_drop(egui::pos2(500.0, 240.0), screen);

    assert_eq!(bounds.x, 500.0);
    assert_eq!(bounds.y, 240.0);
    assert_eq!(bounds.width, FLOATING_TAB_SIZE.x);
    assert_eq!(bounds.height, FLOATING_TAB_SIZE.y);
    assert!(bounds.is_usable());
}

#[test]
fn a_drop_near_a_corner_is_clamped_fully_on_screen() {
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));

    let bottom_right = floating_bounds_for_drop(egui::pos2(1270.0, 790.0), screen);
    assert_eq!(bottom_right.x + bottom_right.width, 1280.0);
    assert_eq!(bottom_right.y + bottom_right.height, 800.0);

    let top_left = floating_bounds_for_drop(egui::pos2(-40.0, -10.0), screen);
    assert_eq!(top_left.x, 0.0);
    assert_eq!(top_left.y, 0.0);
}

#[test]
fn a_screen_smaller_than_the_window_still_produces_a_usable_rect() {
    let screen = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 150.0));
    let bounds = floating_bounds_for_drop(egui::pos2(100.0, 80.0), screen);

    assert!(bounds.is_usable());
    assert!(bounds.x >= screen.left());
    assert!(bounds.y >= screen.top());
    assert!(bounds.x + bounds.width <= screen.right());
    assert!(bounds.y + bounds.height <= screen.bottom());
}

#[test]
fn the_window_bounds_round_trip_to_a_rect() {
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
    let bounds = floating_bounds_for_drop(egui::pos2(60.0, 70.0), screen);
    let rect = bounds.as_rect();

    assert_eq!(rect.min, egui::pos2(60.0, 70.0));
    assert_eq!(rect.size(), FLOATING_TAB_SIZE);
}
