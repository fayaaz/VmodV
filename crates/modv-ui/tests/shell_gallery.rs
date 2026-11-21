//! VmodV shell smoke checks for the Gallery defaults and the retired P0 UI.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, UiState};

#[test]
fn gallery_offers_all_sections_and_no_p0_scaffolding() {
    let snapshot = EngineSnapshot::default();
    let state = UiState::new();
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

    harness.get_by_label("GALLERY");
    harness.get_by_label("All");
    harness.get_by_label("Effects");
    harness.get_by_label("2D Scenes");
    harness.get_by_label("3D Scenes");

    for retired in [
        "P0",
        "P0 checklist",
        "P0 PROBE",
        "P0 CHECKLIST",
        "P0 FRAMEWORK SPIKE",
        "RANGECONTROL PROBE",
    ] {
        assert!(
            harness.query_by_label(retired).is_none(),
            "retired P0 UI is still visible: {retired}"
        );
    }
}
