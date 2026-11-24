//! Kittest coverage for the VmodV video-file transport.
//!
//! The Inspector's texture control and the Media tab's Sources list share
//! Play/Pause, Restart and Loop. Speed 0.1–10 lives only on the Inspector's
//! texture control (the single source of truth), which also persists
//! pause/loop/speed into the texture prop so presets and Scenes keep them.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::state::SidePanelTab;
use modv_ui::{
    EngineSnapshot, GroupView, MediaAction, MediaSnapshot, MediaSourceView, ModuleView, PropView,
    UiAction, UiState, VideoSourceProblemView, VideoTransportAction, VideoTransportView,
};

const CLIP: &str = "live/video/clip.mp4";
const SOURCE_ID: &str = "video:/media/live/video/clip.mp4";
const DURATION_MS: u64 = 12_400;

/// Set the Seek control's accesskit value to the midpoint of *its own* range,
/// so a scrubber expressed in milliseconds or a normalised 0..=1 value both
/// end up at 50% of the clip. The node borrow ends before the harness reruns.
macro_rules! seek_to_halfway {
    ($harness:expr, $panel:expr) => {{
        let node = $harness
            .query_all_by_label_contains("Seek")
            .next()
            .unwrap_or_else(|| panic!("{} exposes no accesskit control labelled 'Seek'", $panel));
        let accesskit = node.accesskit_node();
        let min = accesskit.min_numeric_value().unwrap_or(0.0);
        let max = accesskit.max_numeric_value().unwrap_or(DURATION_MS as f64);
        assert!(max > min, "the Seek control must have a usable range");
        let target = min + (max - min) * 0.5;
        node.set_value_accesskit(target);
    }};
}

fn assert_only_seek(actions: &[MediaAction]) {
    assert_eq!(
        actions.len(),
        1,
        "one transient Seek command, no other transport commands: {actions:?}"
    );
    match &actions[0] {
        MediaAction::VideoTransport {
            source,
            action: VideoTransportAction::Seek { position_ms: at },
        } => {
            assert_eq!(
                source, SOURCE_ID,
                "the seek targets the playing file source"
            );
            let expected = (DURATION_MS / 2) as f64;
            assert!(
                (*at as f64 - expected).abs() <= 1_000.0,
                "a 50% scrub should seek near {expected:.0}ms, got {at}ms"
            );
        }
        other => panic!("expected a Seek transport action, got {other:?}"),
    }
}

fn transport() -> VideoTransportView {
    VideoTransportView {
        paused: false,
        playing: true,
        position_ms: 4_200,
        duration_ms: Some(12_400),
        looping: true,
        eos: false,
        seekable: true,
        playback_rate: 1.0,
    }
}

fn file_source() -> MediaSourceView {
    MediaSourceView {
        source_id: SOURCE_ID.to_string(),
        kind: "file".to_string(),
        state: "running".to_string(),
        width: 1280,
        height: 720,
        frames: 90,
        dropped: 0,
        transport: Some(transport()),
        ..Default::default()
    }
}

fn snapshot(prop_value: serde_json::Value, sources: Vec<MediaSourceView>) -> EngineSnapshot {
    EngineSnapshot {
        groups: vec![GroupView {
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
            clearing: true,
            pipeline: false,
            module_ids: vec!["module-1".to_string()],
        }],
        modules: vec![ModuleView {
            id: "module-1".to_string(),
            module_name: "Image Layer".to_string(),
            group_id: "group-1".to_string(),
            enabled: true,
            alpha: 1.0,
            alpha_input_id: None,
            composite_operation: "normal".to_string(),
            composite_operation_input_id: None,
            module_type: "isf".to_string(),
            props: vec![PropView {
                name: "texture".to_string(),
                prop_type: "texture".to_string(),
                value: prop_value,
                min: None,
                max: None,
                step: None,
                default: None,
                input_id: None,
                components: Vec::new(),
                tween: None,
                enum_members: Vec::new(),
            }],
        }],
        media: MediaSnapshot {
            available: true,
            sources,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn video_prop() -> serde_json::Value {
    serde_json::json!({
        "type": "video",
        "options": { "path": CLIP, "loop": true }
    })
}

fn build(snapshot: EngineSnapshot, state: UiState) -> Harness<'static, UiState> {
    let harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness
}

fn inspector_state() -> UiState {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Inspector;
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    state
}

fn prop_actions(harness: &mut Harness<'static, UiState>) -> Vec<UiAction> {
    harness.state_mut().take_actions()
}

#[test]
fn the_inspector_transport_pauses_and_persists_the_option() {
    let mut harness = build(
        snapshot(video_prop(), vec![file_source()]),
        inspector_state(),
    );
    harness.run();

    harness.get_by_label(CLIP);
    harness.get_by_label("0:04 / 0:12");

    harness.get_by_label("Pause").click();
    harness.run();

    assert_eq!(
        prop_actions(&mut harness),
        vec![modv_ui::texture_prop_action(
            "module-1",
            "texture",
            serde_json::json!({
                "type": "video",
                "options": { "path": CLIP, "loop": true, "paused": true }
            })
        )]
    );
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::Pause,
        }]
    );
}

#[test]
fn restart_only_drives_the_transport() {
    let mut harness = build(
        snapshot(video_prop(), vec![file_source()]),
        inspector_state(),
    );
    harness.run();

    harness.get_by_label("Restart").click();
    harness.run();

    assert!(
        prop_actions(&mut harness).is_empty(),
        "seeking does not change the saved options"
    );
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::Restart,
        }]
    );
}

#[test]
fn the_loop_checkbox_writes_the_prop_and_the_command() {
    let mut harness = build(
        snapshot(video_prop(), vec![file_source()]),
        inspector_state(),
    );
    harness.run();

    harness.get_by_label("Loop").click();
    harness.run();

    assert_eq!(
        prop_actions(&mut harness),
        vec![modv_ui::texture_prop_action(
            "module-1",
            "texture",
            serde_json::json!({
                "type": "video",
                "options": { "path": CLIP, "loop": false }
            })
        )]
    );
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::SetLooping { looping: false },
        }]
    );
}

#[test]
fn the_speed_slider_writes_the_clamped_rate() {
    let mut harness = build(
        snapshot(video_prop(), vec![file_source()]),
        inspector_state(),
    );
    harness.run();

    harness
        .get_by_role_and_label(accesskit::Role::Slider, "Speed")
        .set_value_accesskit(2.0);
    harness.run();

    assert_eq!(
        prop_actions(&mut harness),
        vec![modv_ui::texture_prop_action(
            "module-1",
            "texture",
            serde_json::json!({
                "type": "video",
                "options": { "path": CLIP, "loop": true, "playbackrate": 2.0 }
            })
        )]
    );
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::SetPlaybackRate { rate: 2.0 },
        }]
    );
}

#[test]
fn a_missing_clip_shows_the_shells_problem_instead_of_transport() {
    let mut snapshot = snapshot(video_prop(), Vec::new());
    snapshot.media.video_problems = vec![VideoSourceProblemView {
        path: CLIP.to_string(),
        message: "the video file is missing".to_string(),
    }];
    let mut harness = build(snapshot, inspector_state());
    harness.run();

    harness.get_by_label("the video file is missing");
    assert!(
        harness.query_by_label("Starting video source…").is_none(),
        "a missing clip shows the problem instead of a starting hint"
    );
    assert!(
        harness.query_by_label("Pause").is_none(),
        "an unresolved clip has no running source to drive"
    );
}

#[test]
fn the_media_tab_transport_dispatches_the_same_commands() {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Media;
    let mut harness = build(snapshot(video_prop(), vec![file_source()]), state);
    harness.run();

    harness.get_by_label_contains(SOURCE_ID);
    harness.get_by_label("0:04 / 0:12");

    harness.get_by_label("Pause").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::Pause,
        }]
    );

    harness.get_by_label("Loop").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::SetLooping { looping: false },
        }]
    );

    harness.get_by_label("Restart").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::Restart,
        }]
    );
}

#[test]
fn the_media_tab_has_no_speed_slider() {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Media;
    let mut harness = build(snapshot(video_prop(), vec![file_source()]), state);
    harness.run();

    harness.get_by_label_contains(SOURCE_ID);
    assert!(
        harness
            .query_by_role_and_label(accesskit::Role::Slider, "Speed")
            .is_none(),
        "playback speed is changed from the module's Inspector control, not the Media tab"
    );
}

#[test]
fn a_paused_source_offers_play_and_replays_a_finished_clip() {
    let mut source = file_source();
    source.transport = Some(VideoTransportView {
        paused: true,
        playing: false,
        ..transport()
    });
    let mut harness = build(snapshot(video_prop(), vec![source]), inspector_state());
    harness.run();

    harness.get_by_label("Play").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::Resume,
        }]
    );

    let mut ended = file_source();
    ended.transport = Some(VideoTransportView {
        paused: false,
        playing: false,
        eos: true,
        looping: false,
        ..transport()
    });
    let mut harness = build(snapshot(video_prop(), vec![ended]), inspector_state());
    harness.run();
    harness.get_by_label("Play").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_media_actions(),
        vec![MediaAction::VideoTransport {
            source: SOURCE_ID.to_string(),
            action: VideoTransportAction::Resume,
        }],
        "a parked clip replays from zero through Resume"
    );
}

#[test]
fn seek_scrubbers_queue_a_transient_command_in_both_panels_and_respect_gating() {
    // Inspector: scrubbing the seek bar is a command, not a persisted prop edit.
    let mut harness = build(
        snapshot(video_prop(), vec![file_source()]),
        inspector_state(),
    );
    harness.run();
    harness.get_by_label("0:04 / 0:12");
    let _ = prop_actions(&mut harness);
    let _ = harness.state_mut().take_media_actions();
    seek_to_halfway!(harness, "the Inspector");
    harness.run();
    assert!(
        prop_actions(&mut harness).is_empty(),
        "seeking is transient: no modules/updateProp action may be queued"
    );
    assert_only_seek(&harness.state_mut().take_media_actions());
    // The position/duration readout survives the scrub.
    harness.get_by_label("0:04 / 0:12");

    // Media tab: the per-source row exposes the same scrubber and targets the
    // same source id, without persisting anything either.
    let mut media_state = UiState::new();
    media_state.side_panel_tab = SidePanelTab::Media;
    let mut harness = build(snapshot(video_prop(), vec![file_source()]), media_state);
    harness.run();
    harness.get_by_label_contains(SOURCE_ID);
    harness.get_by_label("0:04 / 0:12");
    let _ = prop_actions(&mut harness);
    let _ = harness.state_mut().take_media_actions();
    seek_to_halfway!(harness, "the Media tab");
    harness.run();
    assert!(
        prop_actions(&mut harness).is_empty(),
        "the Media tab's scrub must not persist a prop edit either"
    );
    assert_only_seek(&harness.state_mut().take_media_actions());
    harness.get_by_label("0:04 / 0:12");

    // Gating: an unseekable source or an unknown/zero duration has no usable
    // scrubber and cannot emit a Seek, even if a disabled Seek label is drawn.
    for transport in [
        VideoTransportView {
            seekable: false,
            ..transport()
        },
        VideoTransportView {
            duration_ms: None,
            ..transport()
        },
        VideoTransportView {
            duration_ms: Some(0),
            ..transport()
        },
    ] {
        let source = MediaSourceView {
            transport: Some(transport),
            ..file_source()
        };
        let mut harness = build(snapshot(video_prop(), vec![source]), inspector_state());
        harness.run();
        let seek_slider = harness
            .query_all_by_label_contains("Seek")
            .find(|node| node.accesskit_node().role() == accesskit::Role::Slider);
        if let Some(node) = seek_slider {
            let accesskit = node.accesskit_node();
            let min = accesskit.min_numeric_value().unwrap_or(0.0);
            let max = accesskit.max_numeric_value().unwrap_or(DURATION_MS as f64);
            let target = min + (max - min) * 0.5;
            node.set_value_accesskit(target);
            harness.run();
        }
        assert!(
            prop_actions(&mut harness).is_empty(),
            "a gated scrubber never writes the prop"
        );
        let actions = harness.state_mut().take_media_actions();
        assert!(
            !actions.iter().any(|action| matches!(
                action,
                MediaAction::VideoTransport {
                    action: VideoTransportAction::Seek { .. },
                    ..
                }
            )),
            "an unseekable/zero-duration source must not emit Seek: {actions:?}"
        );
    }
}
