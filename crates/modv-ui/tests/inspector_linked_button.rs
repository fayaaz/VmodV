//! Regression test for the linked-prop Inspector row:
//!
//! "the input slider changes size depending on if its linked or not. When
//! linked have the linked button change to LINKED and be lit up."
//!
//! The Inspector currently swaps a linked numeric prop from the inline
//! `+ link` row to `RangeControl::draw` plus a separate link row, which both
//! changes the slider's width and replaces the inline control with a LINK
//! badge, a long summary and Edit/x buttons. This test measures the real
//! slider rect before and after linking and inspects the rendered link control.

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, InputLinkView, ModuleView, PropView, UiState};

fn float_prop(name: &str, input_id: &str) -> PropView {
    PropView {
        name: name.to_string(),
        prop_type: "float".to_string(),
        value: serde_json::json!(1.0),
        min: Some(0.0),
        max: Some(10.0),
        step: Some(0.001),
        default: None,
        input_id: Some(input_id.to_string()),
        components: Vec::new(),
        tween: None,
        enum_members: Vec::new(),
    }
}

fn snapshot() -> EngineSnapshot {
    let group = GroupView {
        id: "group-1".to_string(),
        name: "Effects".to_string(),
        enabled: GROUP_ENABLED,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        inherit_from: InheritTarget::Previous,
        clearing: false,
        pipeline: false,
        module_ids: vec!["ball-5678".to_string()],
    };
    let module = ModuleView {
        id: "ball-5678".to_string(),
        module_name: "Ball".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        // A sibling keeps the pre-link measurement honest: both props travel
        // the same unlinked code path before Amount gains its link.
        props: vec![
            float_prop("Amount", "in-amount"),
            float_prop("Width", "in-width"),
        ],
    };
    EngineSnapshot {
        groups: vec![group],
        modules: vec![module],
        ..Default::default()
    }
}

fn amount_link() -> InputLinkView {
    InputLinkView {
        input_id: "in-amount".to_string(),
        kind: "getter".to_string(),
        location: Some("meyda/getFeature".to_string()),
        args: Some(serde_json::json!(["bass"])),
        source: Some("meyda".to_string()),
        min: Some(0.0),
        max: Some(1.0),
        matcher: None,
    }
}

/// Width of the slider drawn on the same Inspector row as `prop_label`.
fn slider_width_for(harness: &Harness<'_, UiState>, prop_label: &str) -> f32 {
    let label = harness
        .get_all_by_label(prop_label)
        .next()
        .unwrap_or_else(|| panic!("the Inspector shows the {prop_label} prop label"))
        .rect();
    harness
        .get_all_by_role(Role::Slider)
        .map(|node| node.rect())
        .find(|rect| {
            (rect.center().y - label.center().y).abs() <= 12.0
                && rect.left() >= label.right()
                && rect.left() > 900.0 // the Inspector sits on the right
        })
        .unwrap_or_else(|| panic!("the Inspector shows a slider on the {prop_label} row"))
        .width()
}

/// Number of accent-green pixels in and immediately around a control's rect.
/// `theme::ACCENT` is rgb(0, 230, 118); an unlit control uses the neutral text
/// colour, whose blue channel is high. This is the "lit up" signal available
/// from a rendered kittest frame (AccessKit exposes no colour property).
fn accent_pixels_near(harness: &mut Harness<'_, UiState>, rect: egui::Rect) -> usize {
    let image = harness.render().expect("render the Inspector frame");
    let padding = 3u32;
    let left = (rect.left().max(0.0) as u32).saturating_sub(padding);
    let top = (rect.top().max(0.0) as u32).saturating_sub(padding);
    let right = ((rect.right() as u32) + padding).min(image.width());
    let bottom = ((rect.bottom() as u32) + padding).min(image.height());
    let mut count = 0;
    for y in top..bottom {
        for x in left..right.min(image.width()) {
            let [r, g, b, _] = image.get_pixel(x, y).0;
            if g > 120 && r < 120 && b < 170 && g as i32 > b as i32 + 40 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn a_linked_prop_keeps_its_slider_width_and_shows_a_lit_linked_button() {
    let snapshot = Rc::new(RefCell::new(snapshot()));
    let ui_snapshot = snapshot.clone();
    let mut state = UiState::new();
    state.selected_module = Some("ball-5678".to_string());
    // Widen the Inspector so the row-derived slider width differs visibly
    // from whatever the linked branch computes.
    state.layout.inspector_width = 600.0;

    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let snapshot = ui_snapshot.borrow();
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();

    // Selecting the module in the Groups pane focuses the Inspector tab.
    harness.get_by_label("Ball  (ball-567)").click();
    harness.run();
    harness.run();

    let width_before = slider_width_for(&harness, "Amount");
    let sibling_before = slider_width_for(&harness, "Width");
    assert!(
        (width_before - sibling_before).abs() <= 2.0,
        "sanity: the unlinked sibling sliders start at the same width: \
         Amount={width_before}, Width={sibling_before}"
    );
    harness
        .query_all_by_label("+ link")
        .next()
        .expect("the Inspector shows an unlinked prop's + link button");

    // Echo the Save/Create-link flow back through the engine snapshot.
    snapshot
        .borrow_mut()
        .input_links
        .insert("in-amount".to_string(), amount_link());
    harness.run();
    harness.run();

    let width_after = slider_width_for(&harness, "Amount");
    let linked_rect = harness
        .query_all_by_label("LINKED")
        .next()
        .map(|n| n.rect());
    let old_badge = harness.query_all_by_label("LINK").next().is_some();
    let accent_pixels = linked_rect
        .map(|rect| accent_pixels_near(&mut harness, rect))
        .unwrap_or(0);

    let width_stable = (width_after - width_before).abs() <= 2.0;
    assert!(
        width_stable && linked_rect.is_some() && !old_badge && accent_pixels >= 3,
        "a linked prop must keep its slider width and show a lit LINKED button: \
         slider before={width_before}, after={width_after} (stable={width_stable}); \
         LINKED control={linked_rect:?}, old LINK badge present={old_badge}, \
         accent pixels around the link control={accent_pixels}"
    );
}
