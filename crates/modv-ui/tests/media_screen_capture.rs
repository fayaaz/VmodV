//! Kittest coverage for the native Media panel's screen-capture section.
//!
//! Electron's `ScreenCapture.vue`/`ScreenPickerDialog.vue` asked for a screen
//! through `getDisplayMedia()`; the native shell asks the XDG desktop portal
//! itself, so the panel only offers two buttons and the portal's own dialog
//! chooses the monitor or window. The snapshot is a fixture, so no D-Bus portal
//! and no PipeWire server are needed to run these tests.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{ScreenCaptureView, ScreenShareKindView, ScreenSourceView};
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineSnapshot, MediaAction, MediaSnapshot, MediaSourceView, UiState};

fn source(source_id: &str, kind: &str, index: usize) -> MediaSourceView {
    MediaSourceView {
        source_id: source_id.to_string(),
        kind: kind.to_string(),
        index,
        state: "running".to_string(),
        width: 2560,
        height: 1440,
        frames: 120,
        dropped: 1,
        last_error: None,
    }
}

fn snapshot(screen: ScreenCaptureView) -> EngineSnapshot {
    EngineSnapshot {
        media: MediaSnapshot {
            available: true,
            error: None,
            cameras: Vec::new(),
            sources: Vec::new(),
            screen,
        },
        ..Default::default()
    }
}

fn build(snapshot: EngineSnapshot) -> Harness<'static, UiState> {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Media;
    Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        )
}

#[test]
fn the_screen_section_offers_monitor_and_window_sharing() {
    let mut harness = build(snapshot(ScreenCaptureView::default()));
    harness.run();

    harness.get_by_label("SCREEN CAPTURE");
    harness.get_by_label("Share monitor…");
    harness.get_by_label("Share window…");
    assert!(
        harness.state().media_actions.is_empty(),
        "drawing the buttons must not queue a share"
    );
}

#[test]
fn sharing_a_monitor_queues_a_monitor_request() {
    let mut harness = build(snapshot(ScreenCaptureView::default()));
    harness.run();

    harness.get_by_label("Share monitor…").click();
    harness.run();

    assert_eq!(
        harness.state().media_actions,
        vec![MediaAction::ShareScreen { window: false }],
        "Share monitor must ask the portal for a monitor"
    );
}

#[test]
fn sharing_a_window_queues_a_window_request() {
    let mut harness = build(snapshot(ScreenCaptureView::default()));
    harness.run();

    harness.get_by_label("Share window…").click();
    harness.run();

    assert_eq!(
        harness.state().media_actions,
        vec![MediaAction::ShareScreen { window: true }],
        "Share window must ask the portal for a window"
    );
}

#[test]
fn a_pending_dialog_disables_the_share_buttons() {
    let mut harness = build(snapshot(ScreenCaptureView {
        pending: Some(ScreenShareKindView::Monitor),
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label("Waiting for the portal dialog…");
    harness.get_by_label("Sharing a monitor");

    harness.get_by_label("Share monitor…").click();
    harness.run();
    harness.get_by_label("Share window…").click();
    harness.run();

    assert!(
        harness.state().media_actions.is_empty(),
        "one portal request at a time; a disabled button must not queue another, got {:?}",
        harness.state().media_actions
    );
}

#[test]
fn cancelling_a_pending_dialog_queues_the_cancel_action() {
    let mut harness = build(snapshot(ScreenCaptureView {
        pending: Some(ScreenShareKindView::Window),
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label("Sharing a window");
    harness.get_by_label("Cancel").click();
    harness.run();

    assert_eq!(
        harness.state().media_actions,
        vec![MediaAction::CancelScreenShare]
    );
}

#[test]
fn a_running_capture_disables_the_buttons_and_points_at_the_sources_list() {
    let mut harness = build(snapshot(ScreenCaptureView {
        running: Some(ScreenSourceView {
            source_id: "screen:portal".to_string(),
            node_id: 42,
            kind: ScreenShareKindView::Window,
        }),
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label(
        "Sharing a window (PipeWire node 42). Stop it in Sources to share something else.",
    );
    harness.get_by_label("Share monitor…").click();
    harness.run();
    harness.get_by_label("Share window…").click();
    harness.run();

    assert!(
        harness.state().media_actions.is_empty(),
        "a live screen capture owns the only screen slot, got {:?}",
        harness.state().media_actions
    );
}

#[test]
fn a_portal_error_is_shown_inline() {
    let message = "screen capture is unavailable: could not reach the desktop portal over D-Bus";
    let mut harness = build(snapshot(ScreenCaptureView {
        error: Some(message.to_string()),
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label(message);
}

#[test]
fn an_unsupported_host_explains_why_and_keeps_the_buttons_disabled() {
    let reason = "the pipewiresrc element is missing (install gstreamer1.0-pipewire)";
    let mut harness = build(snapshot(ScreenCaptureView {
        unavailable: Some(reason.to_string()),
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label(&format!("Screen capture is unavailable: {reason}"));
    harness.get_by_label("Share monitor…").click();
    harness.run();
    assert!(
        harness.state().media_actions.is_empty(),
        "a host without pipewiresrc cannot start a capture, got {:?}",
        harness.state().media_actions
    );
}

#[test]
fn a_running_screen_source_keeps_its_webcam_index_in_the_sources_list() {
    let mut snapshot = snapshot(ScreenCaptureView {
        running: Some(ScreenSourceView {
            source_id: "screen:portal".to_string(),
            node_id: 42,
            kind: ScreenShareKindView::Monitor,
        }),
        ..Default::default()
    });
    snapshot.media.sources = vec![source("screen:portal", "screen", 1)];

    let mut harness = build(snapshot);
    harness.run();

    // The Webcam module's Camera prop indexes screens like cameras, so the
    // source list keeps the same `#N` badge it gives a camera.
    harness.get_by_label("screen:portal  #1  running");
    assert_eq!(
        harness.get_all_by_label("Stop").count(),
        1,
        "the running screen source is stopped from the existing source list"
    );
}

#[test]
fn can_request_requires_a_supported_free_slot() {
    let free = ScreenCaptureView::default();
    assert!(free.can_request(), "a supported idle host can share");

    let mut pending = free.clone();
    pending.pending = Some(ScreenShareKindView::Window);
    assert!(!pending.can_request(), "a dialog is already open");

    let mut running = free.clone();
    running.running = Some(ScreenSourceView {
        source_id: "screen:portal".to_string(),
        node_id: 4,
        kind: ScreenShareKindView::Monitor,
    });
    assert!(!running.can_request(), "one screen capture at a time");

    let mut unsupported = free.clone();
    unsupported.unavailable = Some("no pipewiresrc".to_string());
    assert!(
        !unsupported.can_request(),
        "the host cannot capture screens"
    );
}
