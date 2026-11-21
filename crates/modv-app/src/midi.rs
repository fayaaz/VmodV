//! VmodV MIDI input (P7), replacing the Web MIDI plane.
//!
//! `setup-midi.js` is the behavioural reference: one device record per port,
//! Note On/Off and CC decoding (including `ccLatch`/`noteOnLatch`), MIDI clock
//! at 24 PPQN → BPM, and the same `midi/ADD_DEVICE`/`midi/WRITE_DATA` actions
//! the engine already stores for Input Links. `midir` exposes no device
//! notifications, so hot-plug is a 1 Hz `ports()` diff.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use midir::{MidiInput, MidiInputConnection};
use modv_ui::{MidiLearnArmView, MidiLearnResultView};
use serde_json::Value;

use crate::store_ops::Store;

/// MIDI status bytes used by the legacy handler.
const TYPE_NOTEON: i64 = 144;
const TYPE_CC: i64 = 176;
const CLOCK: u8 = 248;

/// One enumerated MIDI input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MidiDevice {
    /// `${id}-${name}-${manufacturer}`, the key the engine state uses.
    pub key: String,
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub connected: bool,
    pub listen_for_input: bool,
    pub listen_for_clock: bool,
    pub cc_latch: bool,
    pub note_on_latch: bool,
}

/// One decoded MIDI message (three-byte channel messages only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiMessage {
    pub status: u8,
    pub channel: i64,
    pub data: u8,
}

impl MidiMessage {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let status = *bytes.first()?;
        if status >= 0xF0 || bytes.len() < 3 {
            return None;
        }
        Some(Self {
            status,
            channel: i64::from(status & 0x0F),
            data: bytes[2],
        })
    }
}

/// The channel/type pair a message actually wrote to `channelData`, which is
/// what Electron's learning listener captures (`setup-midi.js:125-130` reads
/// the rewritten `message.data[0]`, so a latched CC learns type 144).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedMidiMessage {
    pub channel: i64,
    pub type_: i64,
}

/// The one-shot MIDI Learn arm.
///
/// `midi/learn` replaces any previous arm (the legacy `midi/learn` action
/// stores a single resolve callback, so a second Learn abandons the first).
/// The next message that writes `channelData` resolves it once and disarms;
/// a clock tick never resolves it because Electron's learning callback lives
/// in the `listenForInput` branch (`setup-midi.js:82-133`) and a clock never
/// writes `channelData`.
#[derive(Default)]
pub struct MidiLearn {
    next_id: u64,
    armed: Option<MidiLearnArmView>,
    last: Option<MidiLearnResultView>,
}

impl MidiLearn {
    /// Arm a new one-shot listener for `input_id`, replacing the previous arm.
    pub fn arm(&mut self, input_id: impl Into<String>) -> MidiLearnArmView {
        self.next_id += 1;
        let arm = MidiLearnArmView {
            input_id: input_id.into(),
            id: self.next_id,
        };
        self.armed = Some(arm.clone());
        arm
    }

    /// Disarm without a result. A late message can never resolve a cancelled
    /// arm because the arm is the only way [`Self::resolve`] produces a result.
    pub fn cancel(&mut self) -> Option<MidiLearnArmView> {
        self.armed.take()
    }

    /// The armed listener, while the UI should show “Waiting for MIDI…”.
    pub fn armed(&self) -> Option<&MidiLearnArmView> {
        self.armed.as_ref()
    }

    /// The most recent resolved event (kept for the panel readout).
    pub fn last(&self) -> Option<&MidiLearnResultView> {
        self.last.as_ref()
    }

    /// Resolve the armed listener once, capturing the device key, channel and
    /// written type. Returns `None` when nothing is armed (one-shot semantics).
    pub fn resolve(
        &mut self,
        device: &str,
        channel: i64,
        type_: i64,
    ) -> Option<MidiLearnResultView> {
        let arm = self.armed.take()?;
        let result = MidiLearnResultView {
            id: arm.id,
            input_id: arm.input_id,
            device: device.to_string(),
            channel,
            type_,
        };
        self.last = Some(result.clone());
        Some(result)
    }
}

#[derive(Default)]
struct ClockState {
    last: Option<Instant>,
    diffs: VecDeque<f64>,
    next_bpm_update: Option<Instant>,
}

/// The VmodV MIDI client.
pub struct MidiClient {
    tx: Sender<(String, Vec<u8>)>,
    rx: Receiver<(String, Vec<u8>)>,
    connections: HashMap<String, MidiInputConnection<()>>,
    devices: Vec<MidiDevice>,
    available: bool,
    error: Option<String>,
    last_scan: Instant,
    clock: HashMap<String, ClockState>,
    learn: MidiLearn,
}

impl Default for MidiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiClient {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let (available, error) = match MidiInput::new("VmodV") {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.to_string())),
        };
        Self {
            tx,
            rx,
            connections: HashMap::new(),
            devices: Vec::new(),
            available,
            error,
            last_scan: Instant::now()
                .checked_sub(Duration::from_secs(10))
                .unwrap_or_else(Instant::now),
            clock: HashMap::new(),
            learn: MidiLearn::default(),
        }
    }

    pub fn available(&self) -> bool {
        self.available
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn devices(&self) -> &[MidiDevice] {
        &self.devices
    }

    /// The arm shown as “Waiting for MIDI…” in the UI.
    pub fn learning(&self) -> Option<MidiLearnArmView> {
        self.learn.armed().cloned()
    }

    /// The most recently resolved Learn, for the editor and MIDI panel.
    pub fn last_learn(&self) -> Option<MidiLearnResultView> {
        self.learn.last().cloned()
    }

    /// Arm the one-shot Learn for `input_id`. A previous arm is replaced.
    ///
    /// Arming does not depend on MIDI being available or on a device being
    /// connected: the UI shows “Waiting for MIDI…” and the user can cancel, so
    /// a hot-plugged device still answers the arm.
    pub fn arm_learn(&mut self, input_id: &str) -> MidiLearnArmView {
        let arm = self.learn.arm(input_id);
        eprintln!(
            "ui: INFO MIDI learn armed for input {input_id} (arm {}); waiting for the next MIDI message",
            arm.id
        );
        arm
    }

    /// Cancel the armed listener. Always safe when nothing is armed.
    pub fn cancel_learn(&mut self) {
        if let Some(arm) = self.learn.cancel() {
            eprintln!(
                "ui: INFO MIDI learn cancelled for input {} (arm {})",
                arm.input_id, arm.id
            );
        }
    }

    /// Poll ports and drain messages into the engine store.
    pub fn pump(&mut self, store: &mut Store) {
        if !self.available {
            return;
        }
        if self.last_scan.elapsed() >= Duration::from_secs(1) {
            self.scan(store);
            self.last_scan = Instant::now();
        }
        while let Ok((key, bytes)) = self.rx.try_recv() {
            self.handle_message(store, &key, &bytes);
        }
    }

    /// Apply one message and, when it wrote `channelData`, answer the armed
    /// Learn exactly once.
    ///
    /// Split from [`Self::pump`] so the learn semantics (one-shot, cancel,
    /// clock, written type) are unit-testable without a hardware port.
    fn handle_message(&mut self, store: &mut Store, key: &str, bytes: &[u8]) {
        if bytes == [CLOCK] {
            self.handle_clock(store, key);
            return;
        }
        let Some(applied) = apply_message(store, key, bytes) else {
            return;
        };
        let Some(result) = self.learn.resolve(key, applied.channel, applied.type_) else {
            return;
        };
        eprintln!(
            "ui: INFO MIDI learn resolved: input {} -> {key} channel {} type {} (arm {})",
            result.input_id, result.channel, result.type_, result.id
        );
    }

    /// Diff the current input ports against the connected set.
    fn scan(&mut self, store: &mut Store) {
        let Ok(input) = MidiInput::new("VmodV") else {
            return;
        };
        let ports = input.ports();
        let mut seen = HashSet::new();
        for port in &ports {
            let id = port.id();
            let name = input
                .port_name(port)
                .unwrap_or_else(|_| "MIDI input".to_string());
            let manufacturer = String::new();
            let key = format!("{id}-{name}-{manufacturer}");
            seen.insert(key.clone());
            if !self.connections.contains_key(&key) {
                let Ok(connection_input) = MidiInput::new("VmodV") else {
                    continue;
                };
                let Some(connection_port) = connection_input.find_port_by_id(&id) else {
                    continue;
                };
                let tx = self.tx.clone();
                let callback_key = key.clone();
                match connection_input.connect(
                    &connection_port,
                    "VmodV",
                    move |_timestamp, message, _| {
                        let _ = tx.send((callback_key.clone(), message.to_vec()));
                    },
                    (),
                ) {
                    Ok(connection) => {
                        self.connections.insert(key.clone(), connection);
                    }
                    Err(error) => {
                        eprintln!("ui: WARN could not connect MIDI input {name}: {error}");
                        continue;
                    }
                }
                let _ = store.dispatch(
                    "midi/ADD_DEVICE",
                    serde_json::json!({
                        "id": id,
                        "name": name,
                        "manufacturer": manufacturer,
                    }),
                );
            }
            if let Some(device) = self.devices.iter_mut().find(|device| device.key == key) {
                device.connected = true;
            } else {
                self.devices.push(MidiDevice {
                    key,
                    id,
                    name,
                    manufacturer,
                    connected: true,
                    listen_for_input: true,
                    listen_for_clock: false,
                    cc_latch: false,
                    note_on_latch: false,
                });
            }
        }

        // Keep disappeared devices in the list so their learn mappings remain
        // visible, but drop the connection and mark them disconnected.
        self.connections.retain(|key, _| seen.contains(key));
        for device in &mut self.devices {
            if !seen.contains(&device.key) {
                device.connected = false;
            }
        }
        sync_device_flags(store, &mut self.devices);
    }

    fn handle_clock(&mut self, store: &mut Store, key: &str) {
        let Some(device) = store
            .state()
            .midi
            .get("devices")
            .and_then(Value::as_object)
            .and_then(|devices| devices.get(key))
        else {
            return;
        };
        if !device
            .get("listenForClock")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return;
        }
        let now = Instant::now();
        let state = self.clock.entry(key.to_string()).or_default();
        let Some(last) = state.last else {
            state.last = Some(now);
            return;
        };
        let difference = now.saturating_duration_since(last).as_secs_f64() * 1000.0;
        state.last = Some(now);
        if difference <= 0.0 || difference > 500.0 {
            return;
        }
        if let Some(previous) = state.diffs.back()
            && difference - previous > 50.0
        {
            state.diffs.clear();
        }
        state.diffs.push_back(difference);
        while state.diffs.len() > 94 {
            state.diffs.pop_front();
        }
        if state
            .next_bpm_update
            .map(|next| now >= next)
            .unwrap_or(true)
        {
            let average = state.diffs.iter().sum::<f64>() / state.diffs.len() as f64;
            let bpm: f64 = (1000.0 / average / 24.0) * 60.0;
            if bpm.is_finite() && (20.0..=400.0).contains(&bpm) {
                let _ = store.dispatch(
                    "beats/setBpm",
                    serde_json::json!({ "bpm": bpm.round(), "source": "midi" }),
                );
                state.next_bpm_update = Some(now + Duration::from_millis(500));
            }
        }
    }

    pub fn stop(&mut self) {
        self.connections.clear();
        self.devices.clear();
        self.clock.clear();
        self.learn.cancel();
    }
}

fn sync_device_flags(store: &Store, devices: &mut [MidiDevice]) {
    let Some(state_devices) = store.state().midi.get("devices").and_then(Value::as_object) else {
        return;
    };
    for device in devices {
        let Some(record) = state_devices.get(&device.key).and_then(Value::as_object) else {
            continue;
        };
        device.listen_for_input = record
            .get("listenForInput")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        device.listen_for_clock = record
            .get("listenForClock")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        device.cc_latch = record
            .get("ccLatch")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        device.note_on_latch = record
            .get("noteOnLatch")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }
}

fn apply_message(store: &mut Store, key: &str, bytes: &[u8]) -> Option<AppliedMidiMessage> {
    let message = MidiMessage::decode(bytes)?;
    let state_devices = store
        .state()
        .midi
        .get("devices")
        .and_then(Value::as_object)?;
    let device = state_devices.get(key).and_then(Value::as_object)?;
    if !device
        .get("listenForInput")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        return None;
    }
    let cc_latch = device
        .get("ccLatch")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let note_on_latch = device
        .get("noteOnLatch")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut type_ = i64::from(message.status);
    let mut data = f64::from(message.data);
    if type_ == TYPE_CC && cc_latch {
        type_ = TYPE_NOTEON;
    }
    if type_ == TYPE_NOTEON {
        if message.data > 0 {
            let previous = device
                .get("channelData")
                .and_then(|data| data.get(message.channel.to_string()))
                .and_then(|types| types.get(type_.to_string()))
                .and_then(Value::as_i64);
            data = if previous == Some(1) { 0.0 } else { 1.0 };
        } else if note_on_latch {
            return None;
        }
    }
    if type_ == TYPE_CC {
        data /= 127.0;
    }
    // JS numbers serialise integral values without a fraction; keeping that
    // shape matters because Note On toggling reads the stored value back.
    let data = if data.fract() == 0.0 {
        serde_json::json!(data as i64)
    } else {
        serde_json::json!(data)
    };
    let _ = store.dispatch(
        "midi/WRITE_DATA",
        serde_json::json!({
            "id": key,
            "channel": message.channel,
            "type": type_,
            "data": data,
        }),
    );
    Some(AppliedMidiMessage {
        channel: message.channel,
        type_,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_device() -> (Store, String) {
        let mut store = Store::new();
        store
            .dispatch(
                "midi/ADD_DEVICE",
                serde_json::json!({"id": "dev", "name": "Keyboard", "manufacturer": "Acme"}),
            )
            .expect("add device");
        (store, "dev-Keyboard-Acme".to_string())
    }

    fn channel_value(store: &Store, key: &str, channel: i64, type_: i64) -> Option<Value> {
        store
            .state()
            .midi
            .get("devices")?
            .get(key)?
            .get("channelData")?
            .get(channel.to_string())?
            .get(type_.to_string())
            .cloned()
    }

    #[test]
    fn decodes_note_on_and_cc() {
        let note = MidiMessage::decode(&[0x90, 60, 100]).expect("note");
        assert_eq!(note.status, 0x90);
        assert_eq!(note.channel, 0);
        assert_eq!(note.data, 100);
        let cc = MidiMessage::decode(&[0xB3, 7, 64]).expect("cc");
        assert_eq!(cc.channel, 3);
        assert!(MidiMessage::decode(&[0xF8]).is_none());
    }

    #[test]
    fn note_on_toggles_and_cc_normalises() {
        let (mut store, key) = store_with_device();
        apply_message(&mut store, &key, &[0x90, 60, 100]);
        assert_eq!(
            channel_value(&store, &key, 0, 144),
            Some(serde_json::json!(1))
        );
        apply_message(&mut store, &key, &[0x90, 60, 100]);
        assert_eq!(
            channel_value(&store, &key, 0, 144),
            Some(serde_json::json!(0))
        );
        apply_message(&mut store, &key, &[0xB0, 7, 64]);
        assert_eq!(
            channel_value(&store, &key, 0, 176),
            Some(serde_json::json!(0.5039370078740157))
        );
    }

    #[test]
    fn cc_latch_rewrites_to_note_on() {
        let (mut store, key) = store_with_device();
        store
            .dispatch(
                "midi/UPDATE_DEVICE",
                serde_json::json!({"id": key, "key": "ccLatch", "value": true}),
            )
            .expect("latch");
        apply_message(&mut store, &key, &[0xB0, 7, 127]);
        assert_eq!(
            channel_value(&store, &key, 0, 144),
            Some(serde_json::json!(1))
        );
    }

    #[test]
    fn note_on_latch_ignores_note_off() {
        let (mut store, key) = store_with_device();
        store
            .dispatch(
                "midi/UPDATE_DEVICE",
                serde_json::json!({"id": key, "key": "noteOnLatch", "value": true}),
            )
            .expect("latch");
        apply_message(&mut store, &key, &[0x90, 60, 0]);
        assert_eq!(channel_value(&store, &key, 0, 144), None);
    }

    #[test]
    fn disabled_input_is_ignored() {
        let (mut store, key) = store_with_device();
        store
            .dispatch(
                "midi/UPDATE_DEVICE",
                serde_json::json!({"id": key, "key": "listenForInput", "value": false}),
            )
            .expect("disable");
        apply_message(&mut store, &key, &[0x90, 60, 100]);
        assert_eq!(channel_value(&store, &key, 0, 144), None);
    }

    // ————————————————— MIDI Learn —————————————————

    fn client_with_device() -> (MidiClient, Store, String) {
        let client = MidiClient::new();
        let (store, key) = store_with_device();
        (client, store, key)
    }

    #[test]
    fn learn_arms_and_resolves_once_with_the_written_type() {
        let (mut client, mut store, key) = client_with_device();
        store
            .dispatch(
                "midi/UPDATE_DEVICE",
                serde_json::json!({"id": key, "key": "ccLatch", "value": true}),
            )
            .expect("cc latch");

        let arm = client.arm_learn("input-7");
        assert_eq!(arm.input_id, "input-7");
        assert!(
            client
                .learning()
                .is_some_and(|view| view.input_id == "input-7")
        );

        client.handle_message(&mut store, &key, &[0xB0, 7, 127]);

        let result = client.last_learn().expect("learn resolved");
        assert_eq!(result.id, arm.id);
        assert_eq!(result.input_id, "input-7");
        assert_eq!(result.device, key);
        // The latched CC is written as note-on, so the learned type is 144 and
        // the location Electron generates points at channelData[0][144].
        assert_eq!(result.channel, 0);
        assert_eq!(result.type_, 144);
        assert!(
            client.learning().is_none(),
            "the arm disarms after resolving"
        );
    }

    #[test]
    fn learn_is_one_shot() {
        let (mut client, mut store, key) = client_with_device();
        client.arm_learn("input-1");
        client.handle_message(&mut store, &key, &[0xB0, 0, 64]);
        let first = client.last_learn().expect("first event resolves");

        client.handle_message(&mut store, &key, &[0x90, 60, 100]);

        assert_eq!(
            client.last_learn(),
            Some(first),
            "a second event must not overwrite the learned result"
        );
        assert!(client.learning().is_none());
        assert_eq!(
            channel_value(&store, &key, 0, 144),
            Some(serde_json::json!(1)),
            "the later event still writes its own value"
        );
    }

    #[test]
    fn cancel_disarms_and_a_stale_event_cannot_resolve_later() {
        let (mut client, mut store, key) = client_with_device();
        client.arm_learn("input-1");
        client.cancel_learn();
        assert!(client.learning().is_none());

        client.handle_message(&mut store, &key, &[0xB0, 5, 127]);

        assert!(
            client.last_learn().is_none(),
            "a cancelled arm never resolves"
        );
        assert!(client.learning().is_none());
        // The input itself still works; only the learn listener is gone.
        assert_eq!(
            channel_value(&store, &key, 0, 176),
            Some(serde_json::json!(1))
        );
    }

    #[test]
    fn one_arm_at_a_time_and_a_second_arm_replaces_the_first() {
        let (mut client, mut store, key) = client_with_device();
        let first = client.arm_learn("input-1");
        let second = client.arm_learn("input-2");
        assert_ne!(first.id, second.id);
        assert_eq!(
            client.learning().map(|view| view.input_id),
            Some("input-2".to_string())
        );

        client.handle_message(&mut store, &key, &[0xB0, 2, 127]);

        let result = client.last_learn().expect("resolved");
        assert_eq!(result.id, second.id);
        assert_eq!(result.input_id, "input-2");
        assert!(
            client
                .last_learn()
                .is_some_and(|result| result.id != first.id),
            "the replaced arm never fires"
        );
    }

    #[test]
    fn clock_messages_never_resolve_a_learn() {
        // Electron's learning callback lives in the `listenForInput` branch
        // (`setup-midi.js:125-130`): a clock tick carries the `learning` flag
        // on the media event but never answers a Learn.
        let (mut client, mut store, key) = client_with_device();
        store
            .dispatch(
                "midi/UPDATE_DEVICE",
                serde_json::json!({"id": key, "key": "listenForClock", "value": true}),
            )
            .expect("clock");
        client.arm_learn("input-1");

        client.handle_message(&mut store, &key, &[CLOCK]);

        assert!(
            client.learning().is_some(),
            "a clock tick leaves the arm waiting"
        );
        assert!(client.last_learn().is_none());
        assert_eq!(channel_value(&store, &key, 0, i64::from(u8::MAX)), None);
    }

    #[test]
    fn learn_stays_armed_when_midi_is_unavailable() {
        let mut client = MidiClient::new();
        // Force the startup-probe outcome so the test covers a host with no
        // MIDI stack instead of depending on the runner.
        client.available = false;
        client.error = Some("no MIDI backend".to_string());
        let mut store = Store::new();

        let arm = client.arm_learn("input-9");
        client.pump(&mut store);

        assert_eq!(client.learning().map(|view| view.id), Some(arm.id));
        assert!(client.last_learn().is_none());
        client.cancel_learn();
        assert!(client.learning().is_none());
    }

    #[test]
    fn stop_clears_the_learn_arm() {
        let (mut client, _store, _key) = client_with_device();
        client.arm_learn("input-1");
        client.stop();
        assert!(client.learning().is_none());
    }

    #[test]
    fn applied_messages_report_the_written_type() {
        let (mut store, key) = store_with_device();
        store
            .dispatch(
                "midi/UPDATE_DEVICE",
                serde_json::json!({"id": key, "key": "ccLatch", "value": true}),
            )
            .expect("cc latch");

        let applied = apply_message(&mut store, &key, &[0xB0, 4, 127]).expect("applied");
        assert_eq!(applied.channel, 0);
        assert_eq!(applied.type_, 144);
        assert!(apply_message(&mut store, &key, &[CLOCK]).is_none());
    }
}
