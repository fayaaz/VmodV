//! Kittest coverage for the one-shot MIDI Learn flow.
//!
//! The MIDI section of the focused-input editor arms `midi/learn`, shows the
//! waiting state until the snapshot carries the resolved event, then queues the
//! exact `inputs/createInputLink` payload Electron's `MIDI.vue` creates.
//! Cancel/Forget and the manual MIDI location field stay available, and the
//! MIDI panel shows the armed/last-learned state without a global Learn.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::state::SidePanelTab;
use modv_ui::{
    EngineSnapshot, InputLinkView, LinkEditor, MidiLearnArmView, MidiLearnResultView, MidiSnapshot,
    UiAction, UiState,
};
use serde_json::json;

const DEVICE: &str = "dev-Keyboard-Acme";

struct Fixture {
    snapshot: Arc<Mutex<EngineSnapshot>>,
    harness: Harness<'static, UiState>,
}

impl Fixture {
    fn new(snapshot: EngineSnapshot, state: UiState) -> Self {
        let snapshot = Arc::new(Mutex::new(snapshot));
        let view = Arc::clone(&snapshot);
        let harness = Harness::builder()
            .with_size([1280.0, 800.0])
            .with_accessibility_check(false)
            .build_ui_state(
                move |ui, state| {
                    let snapshot = view.lock().expect("snapshot lock");
                    modv_ui::shell::draw(ui, &snapshot, state);
                },
                state,
            );
        Self { snapshot, harness }
    }

    fn set(&self, snapshot: EngineSnapshot) {
        *self.snapshot.lock().expect("snapshot lock") = snapshot;
    }

    fn actions(&mut self) -> Vec<UiAction> {
        self.harness.state_mut().take_actions()
    }
}

fn midi(learning: Option<MidiLearnArmView>, last: Option<MidiLearnResultView>) -> MidiSnapshot {
    MidiSnapshot {
        available: true,
        error: None,
        devices: Vec::new(),
        learning,
        last_learn: last,
    }
}

fn snapshot_with(midi: MidiSnapshot, link: Option<InputLinkView>) -> EngineSnapshot {
    let mut snapshot = EngineSnapshot {
        midi,
        ..Default::default()
    };
    if let Some(link) = link {
        snapshot.input_links.insert(link.input_id.clone(), link);
    }
    snapshot
}

fn midi_editor() -> LinkEditor {
    let mut editor = LinkEditor::audio("input-1", "gain");
    // The MIDI section is only expanded by default for a MIDI draft; opening it
    // keeps the kittest clicks on the Learn/Forget buttons visible.
    editor.use_midi("");
    editor
}

fn learned(id: u64) -> MidiLearnResultView {
    MidiLearnResultView {
        id,
        input_id: "input-1".to_string(),
        device: DEVICE.to_string(),
        channel: 2,
        type_: 144,
    }
}

fn exact_link_action() -> UiAction {
    UiAction::new(
        "inputs/createInputLink",
        json!({
            "inputId": "input-1",
            "type": "mutation",
            "source": "midi",
            "location": format!("midi.devices['{DEVICE}'].channelData['2']['144']"),
            "match": {
                "type": "midi/WRITE_DATA",
                "payload": { "id": DEVICE, "channel": 2, "type": 144 },
            },
            "min": 0.0,
            "max": 1.0,
        }),
    )
}

#[test]
fn learn_arms_the_input_and_shows_waiting() {
    let mut state = UiState::new();
    state.link_editor = Some(midi_editor());
    let mut fixture = Fixture::new(snapshot_with(midi(None, None), None), state);
    fixture.harness.run();

    fixture.harness.get_by_label("Learn").click();
    fixture.harness.run();

    assert_eq!(
        fixture.actions(),
        vec![UiAction::new("midi/learn", json!({ "inputId": "input-1" }))]
    );
    let editor = fixture
        .harness
        .state()
        .link_editor
        .as_ref()
        .expect("editor stays open");
    assert!(editor.is_waiting_for_midi());
    fixture.harness.get_by_label("Waiting for MIDI…");
    fixture.harness.get_by_label("Cancel");
}

#[test]
fn a_learned_result_creates_the_exact_input_link() {
    let mut state = UiState::new();
    state.link_editor = Some(midi_editor());
    let mut fixture = Fixture::new(snapshot_with(midi(None, None), None), state);
    fixture.harness.run();
    fixture.harness.get_by_label("Learn").click();
    fixture.harness.run();
    assert_eq!(
        fixture.actions(),
        vec![UiAction::new("midi/learn", json!({ "inputId": "input-1" }))]
    );

    // The engine resolves the arm and reports the message on the snapshot.
    fixture.set(snapshot_with(midi(None, Some(learned(1))), None));
    fixture.harness.run();

    assert_eq!(fixture.actions(), vec![exact_link_action()]);
    let editor = fixture
        .harness
        .state()
        .link_editor
        .as_ref()
        .expect("editor stays open");
    assert_eq!(
        editor.location,
        format!("midi.devices['{DEVICE}'].channelData['2']['144']")
    );
    assert_eq!(editor.kind, "mutation");
    assert_eq!(editor.source, "midi");
    assert_eq!(
        editor.matcher,
        Some(json!({
            "type": "midi/WRITE_DATA",
            "payload": { "id": DEVICE, "channel": 2, "type": 144 },
        }))
    );
    assert!(!editor.is_waiting_for_midi());

    // Once the engine stores the link, the section offers Forget instead.
    let link = InputLinkView {
        input_id: "input-1".to_string(),
        kind: "mutation".to_string(),
        location: Some(format!("midi.devices['{DEVICE}'].channelData['2']['144']")),
        args: None,
        source: Some("midi".to_string()),
        min: Some(0.0),
        max: Some(1.0),
        matcher: Some(json!({
            "type": "midi/WRITE_DATA",
            "payload": { "id": DEVICE, "channel": 2, "type": 144 },
        })),
    };
    fixture.set(snapshot_with(midi(None, Some(learned(1))), Some(link)));
    fixture.harness.run();
    fixture.harness.get_by_label("Forget");
    assert_eq!(fixture.actions(), Vec::new());
}

#[test]
fn a_second_arm_result_never_reuses_the_previous_id() {
    let mut state = UiState::new();
    let mut editor = midi_editor();
    // The editor has already applied arm 1; a stale result for it must not
    // create another link, while the newer arm 2 does.
    editor.start_midi_learn(&snapshot_with(midi(None, Some(learned(1))), None));
    state.link_editor = Some(editor);
    let mut fixture = Fixture::new(snapshot_with(midi(None, Some(learned(1))), None), state);
    fixture.harness.run();
    assert_eq!(fixture.actions(), Vec::new());

    fixture.set(snapshot_with(midi(None, Some(learned(2))), None));
    fixture.harness.run();
    assert_eq!(fixture.actions(), vec![exact_link_action()]);
}

#[test]
fn cancel_disarms_the_pending_learn() {
    let mut state = UiState::new();
    state.link_editor = Some(midi_editor());
    let mut fixture = Fixture::new(snapshot_with(midi(None, None), None), state);
    fixture.harness.run();
    fixture.harness.get_by_label("Learn").click();
    fixture.harness.run();
    assert_eq!(fixture.actions().len(), 1);

    fixture.harness.get_by_label("Cancel").click();
    fixture.harness.run();

    assert_eq!(
        fixture.actions(),
        vec![UiAction::new("midi/cancelLearn", json!(null))]
    );
    let editor = fixture
        .harness
        .state()
        .link_editor
        .as_ref()
        .expect("editor stays open");
    assert!(!editor.is_waiting_for_midi());
    fixture.harness.get_by_label("Learn");
}

#[test]
fn forget_removes_the_link_and_releases_its_smoothing_id() {
    let link = InputLinkView {
        input_id: "input-1".to_string(),
        kind: "mutation".to_string(),
        location: Some(format!("midi.devices['{DEVICE}'].channelData['2']['144']")),
        // A stale smoothing id on a MIDI link (e.g. an audio link that was
        // replaced) must be released like the link row's remove path.
        args: Some(json!(["bass", "sm-1", 0.4])),
        source: Some("midi".to_string()),
        min: Some(0.0),
        max: Some(1.0),
        matcher: Some(json!({
            "type": "midi/WRITE_DATA",
            "payload": { "id": DEVICE, "channel": 2, "type": 144 },
        })),
    };
    let snapshot = snapshot_with(midi(None, Some(learned(1))), Some(link.clone()));
    let mut state = UiState::new();
    state.link_editor = Some(LinkEditor::for_input("input-1", "gain", &snapshot));
    let mut fixture = Fixture::new(snapshot, state);
    fixture.harness.run();

    fixture.harness.get_by_label("Forget").click();
    fixture.harness.run();

    assert_eq!(
        fixture.actions(),
        vec![
            UiAction::new("meyda/removeSmoothingId", json!("sm-1")),
            UiAction::new("inputs/removeInputLink", json!({ "inputId": "input-1" })),
        ]
    );
}

#[test]
fn use_midi_and_manual_save_still_create_a_midi_path_link() {
    let mut state = UiState::new();
    state.link_editor = Some(midi_editor());
    let mut fixture = Fixture::new(snapshot_with(midi(None, None), None), state);
    fixture.harness.run();

    fixture.harness.get_by_label("Use MIDI").click();
    fixture.harness.run();

    let editor = fixture
        .harness
        .state()
        .link_editor
        .as_ref()
        .expect("editor stays open");
    assert_eq!(editor.location, "midi.data.device.channel");
    assert_eq!(editor.source, "midi");

    fixture.harness.get_by_label("Create link").click();
    fixture.harness.run();

    assert_eq!(
        fixture.actions(),
        vec![UiAction::new(
            "inputs/createInputLink",
            json!({
                "inputId": "input-1",
                "type": "state",
                "source": "midi",
                "location": "midi.data.device.channel",
                "min": 0.0,
                "max": 1.0,
            }),
        )]
    );
}

#[test]
fn the_midi_panel_shows_the_arm_and_the_last_learned_message() {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Midi;
    let mut fixture = Fixture::new(
        snapshot_with(
            midi(
                Some(MidiLearnArmView {
                    input_id: "input-3".to_string(),
                    id: 4,
                }),
                Some(MidiLearnResultView {
                    id: 3,
                    input_id: "input-2".to_string(),
                    device: DEVICE.to_string(),
                    channel: 7,
                    type_: 176,
                }),
            ),
            None,
        ),
        state,
    );
    fixture.harness.run();

    fixture.harness.get_by_label("Waiting for MIDI…");
    fixture
        .harness
        .get_by_label(&format!("Last learned: {DEVICE} channel 7 type 176"));

    fixture.harness.get_by_label("Cancel").click();
    fixture.harness.run();

    assert_eq!(
        fixture.actions(),
        vec![UiAction::new("midi/cancelLearn", json!(null))]
    );
}
