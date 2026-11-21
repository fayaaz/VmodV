//! Kittest coverage for the VmodV OSC panel.
//!
//! The panel lists the engine's UDP listeners (the startup default plus any
//! the user adds) and the latest values received under `osc.data`, so an OSC
//! Input Link can be pointed at a state path the user can see.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{OscServerView, OscSnapshot, osc_addresses};
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineSnapshot, UiState};

fn demo_snapshot() -> EngineSnapshot {
    EngineSnapshot {
        osc: OscSnapshot {
            servers: vec![
                OscServerView {
                    id: "default".to_string(),
                    name: String::new(),
                    host: "0.0.0.0".to_string(),
                    port: 3333,
                },
                OscServerView {
                    id: "studio".to_string(),
                    name: "Studio".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 9000,
                },
            ],
            addresses: osc_addresses(&serde_json::json!({
                "modv": { "bass": [0.5], "treble": [0.25, 1] },
                "layers": [{ "alpha": [1.0] }],
            })),
        },
        ..Default::default()
    }
}

fn build(snapshot: EngineSnapshot) -> Harness<'static, UiState> {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Osc;
    Harness::builder()
        .with_size([1280.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        )
}

fn taken_actions(harness: &mut Harness<'_, UiState>) -> Vec<modv_ui::UiAction> {
    harness.state_mut().take_actions()
}

#[test]
fn the_osc_panel_lists_listeners_and_the_latest_addresses() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness.get_by_label("0.0.0.0:3333");
    harness.get_by_label("Studio");
    harness.get_by_label("127.0.0.1:9000");
    harness.get_by_label("osc.data.modv.bass");
    harness.get_by_label("0.5");
    harness.get_by_label("osc.data.modv.treble");
    harness.get_by_label("0.25, 1");
    harness.get_by_label("osc.data.layers.0.alpha");
    assert_eq!(
        harness.get_all_by_label("Remove").count(),
        2,
        "the default listener is removable like the legacy per-link servers"
    );
}

#[test]
fn adding_a_listener_dispatches_the_typed_host_and_port() {
    let mut harness = build(demo_snapshot());
    harness.state_mut().osc_port_draft = "9001".to_string();
    harness.run();

    harness.get_by_label("Add listener").click();
    harness.run();

    let actions = taken_actions(&mut harness);
    assert_eq!(actions.len(), 1, "got {actions:?}");
    assert_eq!(actions[0].action, "osc/createServer");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({ "host": "0.0.0.0", "port": 9001 })
    );
}

#[test]
fn an_invalid_port_is_reported_without_dispatching() {
    let mut harness = build(demo_snapshot());
    harness.state_mut().osc_port_draft = "not-a-port".to_string();
    harness.run();

    harness.get_by_label("Add listener").click();
    harness.run();

    assert!(
        taken_actions(&mut harness).is_empty(),
        "an invalid port must not reach the engine"
    );
    let error = harness
        .state()
        .osc_form_error
        .as_deref()
        .expect("the form reports the bad port");
    assert!(
        error.to_lowercase().contains("port"),
        "unexpected error: {error}"
    );
}

#[test]
fn removing_a_listener_dispatches_its_id() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness
        .get_all_by_label("Remove")
        .next()
        .expect("the default listener has a Remove button")
        .click();
    harness.run();

    let actions = taken_actions(&mut harness);
    assert_eq!(actions.len(), 1, "got {actions:?}");
    assert_eq!(actions[0].action, "osc/removeServer");
    assert_eq!(actions[0].payload, serde_json::json!({ "id": "default" }));
}

#[test]
fn an_empty_osc_panel_explains_how_addresses_appear() {
    let mut harness = build(EngineSnapshot::default());
    harness.run();

    harness.get_by_label("No OSC listeners.");
    harness.get_by_label(
        "No OSC messages yet. Send to a listener above and the address appears as osc.data.<address>.",
    );
    assert_eq!(harness.query_all_by_label("Remove").count(), 0);
}
