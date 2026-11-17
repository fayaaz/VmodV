//! Pure formatting/view coverage for the native NDI and OSC panels.
//!
//! The panel code is immediate-mode and reads [`EngineSnapshot`]s, so the
//! interesting parts — availability warnings, sender/receiver status lines,
//! the output size/rate display and flattening `osc.data` into rows — are
//! pure functions and are tested here without an egui harness.

use modv_ui::model::{
    MAX_OSC_ADDRESS_ROWS, NdiOutputView, NdiReceiverView, NdiSenderView, NdiSnapshot,
    OscServerView, osc_addresses,
};

fn ndi(enabled: bool, follow_size: bool, follow_fps: bool) -> NdiSnapshot {
    NdiSnapshot {
        available: true,
        library_path: Some("/usr/lib/libndi.so.5".to_string()),
        output: NdiOutputView {
            enabled,
            name: "VmodV".to_string(),
            width: 1280,
            height: 720,
            follow_output_size: follow_size,
            follow_fps,
            target_fps: 30.0,
        },
        sender: NdiSenderView::default(),
        discovering: false,
        show_local_sources: true,
        sources: Vec::new(),
        receivers: Vec::new(),
    }
}

#[test]
fn an_available_ndi_snapshot_has_no_warning() {
    let snapshot = ndi(false, true, true);
    assert_eq!(snapshot.availability_warning(), None);
    assert_eq!(snapshot.sender_warning(), None);
    assert!(snapshot.can_enable_output());
}

#[test]
fn the_unavailable_warning_names_the_library_path() {
    let mut snapshot = ndi(false, true, true);
    snapshot.available = false;

    assert_eq!(
        snapshot.availability_warning().as_deref(),
        Some(
            "NDI is unavailable: libndi could not be loaded (found at /usr/lib/libndi.so.5). \
             Install the NDI SDK or set MODV_NDI_LIB, then restart the engine."
        )
    );
    assert!(!snapshot.can_enable_output());

    snapshot.library_path = None;
    assert_eq!(
        snapshot.availability_warning().as_deref(),
        Some(
            "NDI is unavailable: libndi could not be loaded. \
             Install the NDI SDK or set MODV_NDI_LIB, then restart the engine."
        )
    );
}

#[test]
fn the_sender_status_line_reports_each_state() {
    let mut snapshot = ndi(false, true, true);
    assert_eq!(snapshot.sender_status_label(), "Not sending");

    snapshot.output.enabled = true;
    assert_eq!(snapshot.sender_status_label(), "Starting…");

    snapshot.sender.running = true;
    snapshot.sender.frames_sent = 12;
    assert_eq!(snapshot.sender_status_label(), "Sending · 12 frame(s)");

    snapshot.sender.error = Some("libndi refused the frame".to_string());
    assert_eq!(
        snapshot.sender_warning().as_deref(),
        Some("NDI output stopped: libndi refused the frame")
    );
    assert_eq!(
        snapshot.sender_status_label(),
        "Stopped: libndi refused the frame"
    );

    let mut unavailable = ndi(false, true, true);
    unavailable.available = false;
    assert_eq!(unavailable.sender_status_label(), "NDI is unavailable");
}

#[test]
fn the_output_display_follows_the_configured_size_and_rate() {
    let following = ndi(false, true, true);
    assert_eq!(following.output_size_label([1920, 1080]), "1920×1080");
    assert_eq!(
        following.output_fps_label(59.94),
        "60 fps · following modV",
        "the live engine rate is rounded for display"
    );

    let manual = ndi(false, false, false);
    assert_eq!(manual.output_size_label([1920, 1080]), "1280×720");
    assert_eq!(manual.output_fps_label(59.94), "30 fps");

    // A silent/zero engine rate falls back to the documented default instead
    // of rendering 0 fps.
    assert_eq!(following.target_fps(0.0), 60.0);
    assert_eq!(following.target_fps(f64::NAN), 60.0);
}

#[test]
fn receiver_status_labels_cover_connected_idle_and_error_states() {
    let connected = NdiReceiverView {
        id: "rx-1".to_string(),
        source_name: "STUDIO (Camera 1)".to_string(),
        url_address: "192.168.1.20:5961".to_string(),
        enabled: true,
        state: "connected".to_string(),
        source: Some("STUDIO (Camera 1)".to_string()),
        error: None,
        width: 1920,
        height: 1080,
        frames: 42,
    };
    assert_eq!(connected.status_label(), "connected 1920×1080");
    assert_eq!(connected.toggle_label(), "Disable");
    assert_eq!(connected.title(), "STUDIO (Camera 1)");

    let idle = NdiReceiverView {
        id: "rx-2".to_string(),
        source_name: String::new(),
        state: "idle".to_string(),
        source: None,
        error: None,
        enabled: false,
        ..Default::default()
    };
    assert_eq!(idle.status_label(), "idle");
    assert_eq!(idle.toggle_label(), "Enable");
    assert_eq!(idle.title(), "rx-2", "an unnamed receiver falls back to id");

    let failed = NdiReceiverView {
        id: "rx-3".to_string(),
        state: "error".to_string(),
        ..Default::default()
    };
    assert_eq!(failed.status_label(), "error");
}

#[test]
fn osc_addresses_flatten_messages_nested_branches_and_array_indexes() {
    let data = serde_json::json!({
        "modv": { "treble": [0.25, 1], "bass": [0.5] },
        "layers": [{ "alpha": [1.0] }, { "alpha": [0.5] }],
        "flags": { "on": [1], "label": ["hello"] },
        "flat": "value",
        "empty": [],
    });

    let rows = osc_addresses(&data);
    assert_eq!(
        rows.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(),
        vec![
            "empty",
            "flags.label",
            "flags.on",
            "flat",
            "layers.0.alpha",
            "layers.1.alpha",
            "modv.bass",
            "modv.treble",
        ],
        "rows are sorted by path, even though the store preserves arrival order"
    );

    let bass = rows
        .iter()
        .find(|row| row.path == "modv.bass")
        .expect("bass row");
    assert_eq!(bass.state_path, "osc.data.modv.bass");
    assert_eq!(bass.address(), "/modv/bass");
    assert_eq!(bass.value_label(), "0.5");

    assert_eq!(
        rows.iter()
            .find(|row| row.path == "modv.treble")
            .expect("treble row")
            .value_label(),
        "0.25, 1"
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.path == "layers.1.alpha")
            .expect("nested array branch")
            .value_label(),
        "0.5"
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.path == "flags.label")
            .expect("string value")
            .value_label(),
        "hello",
        "strings render without quotes"
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.path == "empty")
            .expect("empty leaf")
            .value_label(),
        "(no values)"
    );
}

#[test]
fn osc_address_flattening_is_bounded() {
    let mut data = serde_json::Map::new();
    for index in 0..(MAX_OSC_ADDRESS_ROWS + 25) {
        data.insert(format!("address{index}"), serde_json::json!([index]));
    }
    let rows = osc_addresses(&serde_json::Value::Object(data));
    assert_eq!(
        rows.len(),
        MAX_OSC_ADDRESS_ROWS,
        "the panel is a monitor, not an unbounded table"
    );
}

#[test]
fn osc_flattening_handles_empty_and_scalar_roots() {
    assert!(osc_addresses(&serde_json::json!({})).is_empty());
    assert!(
        osc_addresses(&serde_json::json!([])).is_empty(),
        "a root-level value has no address to bind"
    );
}

#[test]
fn osc_server_rows_parse_the_engine_create_reply() {
    let server = OscServerView::from_create_reply(&serde_json::json!({
        "id": "server-1",
        "name": "Studio",
        "host": "0.0.0.0",
        "port": 3333,
    }))
    .expect("reply parses");
    assert_eq!(server.id, "server-1");
    assert_eq!(server.title(), "Studio");
    assert_eq!(server.address_label(), "0.0.0.0:3333");

    let unnamed = OscServerView::from_create_reply(&serde_json::json!({
        "id": "server-2",
        "host": "127.0.0.1",
        "port": 9000,
    }))
    .expect("a missing name falls back to the address");
    assert_eq!(unnamed.title(), "127.0.0.1:9000");

    assert!(OscServerView::from_create_reply(&serde_json::json!({"id": "server-3"})).is_none());
    assert!(
        OscServerView::from_create_reply(&serde_json::json!({
            "id": "server-4",
            "host": "0.0.0.0",
            "port": "3333",
        }))
        .is_none(),
        "a non-numeric port is rejected rather than defaulted"
    );
}
