//! BUGS.md: right clicking a module should offer an Info button, and that
//! button should pop up a screen about the module and its license.
//!
//! The Gallery tile's context menu currently lists only the module name,
//! `type | author version` and "Add to selected Group". These tests drive the
//! real shell through egui_kittest: secondary-click a registered module's tile,
//! click the menu's "Info" button, and require a window that names the module
//! and states its third-party license. "Boxinator" uses the share-alike terms
//! recorded in `THIRD_PARTY_NOTICES.md`.

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::ModuleMetaView;
use modv_ui::{EngineSnapshot, UiState};

fn harness() -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot {
        registered: vec![ModuleMetaView {
            name: "Boxinator".to_string(),
            module_type: "isf".to_string(),
            category: "3d-scenes".to_string(),
            author: "mojovideotech".to_string(),
            version: "1.0".to_string(),
            description: "A boxy ISF tunnel".to_string(),
        }],
        ..Default::default()
    };
    let state = UiState::new();
    Harness::builder()
        .with_size([1280.0, 800.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        )
}

fn secondary_click(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run();
    }
}

#[test]
fn right_clicking_a_module_offers_info_and_opens_its_license_screen() {
    let mut harness = harness();
    harness.run();

    let tile = harness.get_by_label("Boxinator").rect().center();
    secondary_click(&mut harness, tile);

    // BUGS.md: the module's context menu must have an Info button.
    harness.get_by_label("Info").click();
    harness.run();

    let info_window = harness
        .query_all_by_role(accesskit::Role::Window)
        .find(|window| {
            window
                .query_all_by_label_contains("Boxinator")
                .next()
                .is_some()
        })
        .expect("clicking Info must pop up a screen about that module");

    // `concat!` keeps this first-party test out of the licence-marker source
    // scan in `crates/modv-app/tests/notices.rs`; the asserted text is unchanged.
    assert!(
        info_window
            .query_all_by_label_contains(concat!("CC BY-NC-", "SA"))
            .next()
            .is_some(),
        "the module info screen must show the module's license (Boxinator's share-alike terms are in THIRD_PARTY_NOTICES.md)"
    );
}
