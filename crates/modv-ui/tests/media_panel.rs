//! Kittest coverage for the native Media panel's webcam picker.
//!
//! Electron's `AudioVideo.vue` lists every camera with a checkbox; a checked
//! camera is appended to `mediaStream.selectedVideoSources` (the Webcam
//! module's 1-based `camera` index) and gets its resolution/frame-rate selects.
//! The native panel mirrors that: checkbox, `#N` Webcam-index badge and up/down
//! arrows that reorder the running cameras.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::state::SidePanelTab;
use modv_ui::{CameraView, EngineSnapshot, MediaAction, MediaSnapshot, MediaSourceView, UiState};

fn camera(id: &str, name: &str, max_width: u32, max_height: u32, max_fps: u32) -> CameraView {
    CameraView {
        id: id.to_string(),
        name: name.to_string(),
        max_width,
        max_height,
        max_fps,
    }
}

fn source(source_id: &str, kind: &str, index: usize, width: u32, height: u32) -> MediaSourceView {
    MediaSourceView {
        source_id: source_id.to_string(),
        kind: kind.to_string(),
        index,
        state: "running".to_string(),
        width,
        height,
        frames: 42,
        dropped: 0,
        last_error: None,
    }
}

/// Two running cameras (`/dev/video0` at index 1, `/dev/video2` at index 2)
/// and one available but unchecked camera (`/dev/video1`).
fn demo_snapshot() -> EngineSnapshot {
    EngineSnapshot {
        media: MediaSnapshot {
            available: true,
            error: None,
            cameras: vec![
                camera("/dev/video0", "Webcam Alpha", 1920, 1080, 60),
                camera("/dev/video1", "Webcam Beta", 1280, 720, 30),
                camera("/dev/video2", "Webcam Gamma", 640, 480, 30),
            ],
            sources: vec![
                source("file:a", "file", 0, 1280, 720),
                source("camera:/dev/video0", "camera", 1, 1920, 1080),
                source("camera:/dev/video2", "camera", 2, 640, 480),
            ],
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
fn the_media_panel_numbers_running_cameras_by_webcam_index() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness.get_by_label("Webcam Alpha");
    harness.get_by_label("Webcam Beta");
    harness.get_by_label("Webcam Gamma");
    harness.get_by_label("#1");
    harness.get_by_label("#2");
    // The file is not a Webcam source, so its status shows no index badge.
    assert!(
        harness.query_by_label("#0").is_none(),
        "a file must not claim a Webcam index"
    );
}

#[test]
fn checking_a_camera_queues_a_start_with_the_suggested_mode() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness.get_by_label("Webcam Beta").click();
    harness.run();

    assert!(
        harness
            .state()
            .media_actions
            .contains(&MediaAction::StartCamera {
                id: "/dev/video1".to_string(),
                width: 1280,
                height: 720,
                fps: 30,
            }),
        "checking a camera must start it with its selected/default mode, got {:?}",
        harness.state().media_actions
    );
}

#[test]
fn unchecking_a_running_camera_queues_a_stop() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness.get_by_label("Webcam Alpha").click();
    harness.run();

    assert!(
        harness.state().media_actions.contains(&MediaAction::Stop {
            source_id: "camera:/dev/video0".to_string(),
        }),
        "unchecking a running camera must stop it, got {:?}",
        harness.state().media_actions
    );
}

#[test]
fn the_arrows_queue_a_camera_reorder() {
    let mut harness = build(demo_snapshot());
    harness.run();

    // One up arrow per running camera, in row order: Alpha's, then Gamma's.
    harness
        .get_all_by_label("Up")
        .nth(1)
        .expect("Gamma has an up arrow")
        .click();
    harness.run();

    assert!(
        harness
            .state()
            .media_actions
            .contains(&MediaAction::MoveCamera {
                id: "/dev/video2".to_string(),
                delta: -1,
            }),
        "the second camera's up arrow must move it one slot earlier, got {:?}",
        harness.state().media_actions
    );
}

#[test]
fn the_sources_list_shows_the_camera_only_index_and_a_stop_button() {
    let mut harness = build(demo_snapshot());
    harness.run();

    harness.get_by_label("file:a  running");
    harness.get_by_label("camera:/dev/video0  #1  running");
    harness.get_by_label("camera:/dev/video2  #2  running");
    assert_eq!(
        harness.get_all_by_label("Stop").count(),
        3,
        "every running source has its own stop button"
    );
}
