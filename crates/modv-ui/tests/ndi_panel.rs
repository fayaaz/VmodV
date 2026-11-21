//! Kittest coverage for the VmodV NDI panel.
//!
//! Electron's `InputDeviceConfig/NDI.vue` drives the engine's `ndi` store
//! module; the VmodV panel dispatches the same actions. The snapshot is a
//! fixture, so no libndi/NDI SDK is required to run these tests.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{NdiOutputView, NdiReceiverView, NdiSenderView, NdiSnapshot, NdiSourceView};
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineSnapshot, UiState};

fn demo_snapshot() -> EngineSnapshot {
    EngineSnapshot {
        resolution: [1920, 1080],
        fps: 60.0,
        ndi: NdiSnapshot {
            available: true,
            library_path: Some("/usr/lib/libndi.so.5".to_string()),
            output: NdiOutputView {
                enabled: false,
                name: "VmodV".to_string(),
                width: 1280,
                height: 720,
                follow_output_size: true,
                follow_fps: true,
                target_fps: 30.0,
            },
            sender: NdiSenderView::default(),
            discovering: false,
            show_local_sources: true,
            sources: vec![
                NdiSourceView {
                    name: "STUDIO (Camera 1)".to_string(),
                    url_address: "192.168.1.20:5961".to_string(),
                },
                NdiSourceView {
                    name: "LAPTOP (Screen)".to_string(),
                    url_address: "192.168.1.30:5962".to_string(),
                },
            ],
            receivers: vec![
                NdiReceiverView {
                    id: "rx-1".to_string(),
                    source_name: "STUDIO (Camera 1)".to_string(),
                    url_address: "192.168.1.20:5961".to_string(),
                    enabled: true,
                    state: "connected".to_string(),
                    source: Some("STUDIO (Camera 1)".to_string()),
                    error: None,
                    width: 1920,
                    height: 1080,
                    frames: 240,
                },
                NdiReceiverView {
                    id: "rx-2".to_string(),
                    source_name: "LAPTOP (Screen)".to_string(),
                    url_address: "192.168.1.30:5962".to_string(),
                    enabled: false,
                    state: "idle".to_string(),
                    source: None,
                    error: None,
                    width: 0,
                    height: 0,
                    frames: 0,
                },
            ],
        },
        ..Default::default()
    }
}

fn build(snapshot: EngineSnapshot) -> Harness<'static, UiState> {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Ndi;
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
fn the_ndi_panel_lists_sources_receivers_and_the_sender_status() {
    let mut harness = build(demo_snapshot());
    harness.run();

    assert_eq!(
        harness.get_all_by_label("STUDIO (Camera 1)").count(),
        2,
        "the source row and its receiver row both name the source"
    );
    assert_eq!(harness.get_all_by_label("LAPTOP (Screen)").count(), 2);
    harness.get_by_label("connected 1920×1080");
    harness.get_by_label("idle");
    harness.get_by_label("Not sending");
    harness.get_by_label("libndi: /usr/lib/libndi.so.5");
}

#[test]
fn toggling_the_output_and_its_options_dispatches_the_exact_mutations() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness.get_by_label("Enable Output").click();
    harness.run();
    harness.get_by_label("Follow ModV output size").click();
    harness.run();
    harness.get_by_label("Follow ModV frame rate").click();
    harness.run();

    let actions = taken_actions(&mut harness);
    assert_eq!(actions.len(), 3, "got {actions:?}");
    assert_eq!(actions[0].action, "ndi/SET_OUTPUT_ENABLED");
    assert_eq!(actions[0].payload, serde_json::json!(true));
    assert_eq!(actions[1].action, "ndi/SET_FOLLOW_MODV_OUTPUT_SIZE");
    assert_eq!(actions[1].payload, serde_json::json!(false));
    assert_eq!(actions[2].action, "ndi/SET_FOLLOW_MODV_FPS");
    assert_eq!(actions[2].payload, serde_json::json!(false));
}

#[test]
fn discovery_toggles_and_source_receivers_dispatch_their_payloads() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness.get_by_label("Show local sources").click();
    harness.run();
    harness.get_by_label("Discover Sources").click();
    harness.run();
    harness
        .get_all_by_label("Receive source")
        .next()
        .expect("the first source has a Receive button")
        .click();
    harness.run();

    let actions = taken_actions(&mut harness);
    assert_eq!(actions.len(), 3, "got {actions:?}");
    assert_eq!(actions[0].action, "ndi/setDiscoveryOptions");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({ "showLocalSources": false })
    );
    assert_eq!(actions[1].action, "ndi/discoverSources");
    assert_eq!(actions[1].payload, serde_json::json!({}));
    assert_eq!(actions[2].action, "ndi/createReceiver");
    assert_eq!(
        actions[2].payload,
        serde_json::json!({
            "source": {
                "name": "STUDIO (Camera 1)",
                "urlAddress": "192.168.1.20:5961",
            },
        })
    );
}

#[test]
fn receiver_rows_toggle_and_remove_by_id() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness
        .get_all_by_label("Disable")
        .next()
        .expect("the connected receiver has a Disable button")
        .click();
    harness.run();
    harness.get_by_label("Enable").click();
    harness.run();
    harness
        .get_all_by_label("Remove receiver")
        .next()
        .expect("the first receiver has a Remove button")
        .click();
    harness.run();

    let actions = taken_actions(&mut harness);
    assert_eq!(actions.len(), 3, "got {actions:?}");
    assert_eq!(actions[0].action, "ndi/disableReceiver");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({ "receiverId": "rx-1" })
    );
    assert_eq!(actions[1].action, "ndi/enableReceiver");
    assert_eq!(
        actions[1].payload,
        serde_json::json!({ "receiverId": "rx-2" })
    );
    assert_eq!(actions[2].action, "ndi/removeReceiver");
    assert_eq!(
        actions[2].payload,
        serde_json::json!({ "receiverId": "rx-1" })
    );
}

#[test]
fn an_unavailable_sdk_warns_and_disables_every_control() {
    let mut snapshot = demo_snapshot();
    snapshot.ndi.available = false;
    snapshot.ndi.library_path = Some("/opt/ndi/libndi.so".to_string());
    let warning = snapshot
        .ndi
        .availability_warning()
        .expect("unavailable snapshot has a warning");
    let mut harness = build(snapshot);
    harness.run();

    harness.get_by_label(warning.as_str());
    harness.get_by_label("Enable Output").click();
    harness.get_by_label("Discover Sources").click();
    harness
        .get_all_by_label("Receive source")
        .next()
        .expect("sources are still listed")
        .click();
    harness.run();

    assert!(
        taken_actions(&mut harness).is_empty(),
        "the unavailable panel must not dispatch NDI actions"
    );
}

#[test]
fn empty_source_and_receiver_lists_explain_the_next_step() {
    let mut snapshot = demo_snapshot();
    snapshot.ndi.sources.clear();
    snapshot.ndi.receivers.clear();
    let mut harness = build(snapshot);
    harness.run();

    harness.get_by_label("No NDI sources yet. Click Discover Sources to search the network.");
    harness.get_by_label("No NDI receivers.");
    assert_eq!(harness.query_all_by_label("Remove receiver").count(), 0);
}

#[test]
fn a_discovering_panel_reports_progress_and_blocks_a_second_run() {
    let mut snapshot = demo_snapshot();
    snapshot.ndi.discovering = true;
    let mut harness = build(snapshot);
    harness.run();

    harness.get_by_label("Discovering…");
    harness.get_by_label("Discovering…").click();
    harness.run();
    assert!(
        taken_actions(&mut harness).is_empty(),
        "the discover button is disabled while a run is in flight"
    );
}
