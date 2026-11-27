//! VmodV shell smoke checks for the Gallery defaults and the retired P0 UI.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, ModuleMetaView, UiState};

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

fn gallery_meta(name: &str, module_type: &str) -> ModuleMetaView {
    ModuleMetaView {
        name: name.to_string(),
        module_type: module_type.to_string(),
        kind: module_type.to_string(),
        category: if module_type == "isf" {
            "3d-scenes".to_string()
        } else {
            "effects".to_string()
        },
        author: "modV".to_string(),
        version: "1.0".to_string(),
        description: String::new(),
        tags: Vec::new(),
    }
}

#[test]
fn typing_a_type_in_the_gallery_search_hides_other_module_tiles() {
    let snapshot = EngineSnapshot {
        registered: vec![
            gallery_meta("Bands", "shader"),
            gallery_meta("Tunnel", "isf"),
            gallery_meta("Deep Sea", "isf"),
        ],
        ..Default::default()
    };
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            UiState::new(),
        );
    harness.run();

    harness.get_by_label("Bands");

    {
        let search = harness
            .get_all_by_role(accesskit::Role::TextInput)
            .next()
            .expect("the gallery search field");
        search.focus();
        search.type_text("isf");
    }
    harness.run();

    assert!(
        harness.query_by_label("Bands").is_none(),
        "the shader tile must disappear when the search asks for ISF modules"
    );
    assert!(
        harness.query_by_label("Tunnel").is_some(),
        "the ISF tile must remain visible for an `isf` search"
    );
    assert!(
        harness.query_by_label("Deep Sea").is_some(),
        "every ISF tile must remain visible for an `isf` search"
    );
}
