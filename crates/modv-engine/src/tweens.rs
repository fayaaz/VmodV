//! BPM tweens: the anime.js frame pre-renderer and its runtime.
//!
//! Ported from `src/renderer/src/application/worker/store/modules/tweens.js`
//! (264 lines) and the `tween` data type in `store/modules/dataTypes.js:108-128`.
//!
//! A tween is a small descriptor (`id`, `data`, `duration`, `direction`,
//! `easing`, `loop`, `useBpm`, `bpmDivision`, `durationAsTotalTime`, `steps`,
//! and the native `keyframeTimes` / `loopMode` extensions).
//! The worker pre-renders the whole animation into an array of value frames by
//! calling anime.js `seek()` at every frame boundary ([`build_tween_frames`]),
//! then treats that array as a step sequencer: an Input Link reads the current
//! frame and advances exactly one frame per read (`dataTypes.tween.get`), so a
//! tween advances once per link evaluation, not once per render.
//!
//! The automatic Scene links (`scene-link-defaults.js`) create a slow BPM
//! tween (`bpmDivision: 32`, `easeInOutSine`, `alternate`) whose value is read
//! through `tweens.tweens['<input id>'].value[0]`; the engine resolves that
//! location through [`resolve_tween_link`].
//!
//! # Per-frame call order
//!
//! ```
//! use modv_engine::tweens::{TweenSet, resolve_tween_link};
//! use serde_json::json;
//!
//! // Preset / Scene load: `state.tweens` is the descriptor array.
//! let document = json!([{
//!     "id": "input-1",
//!     "data": [[0], [1]],
//!     "easing": "easeInOutSine",
//!     "bpmDivision": 32,
//! }]);
//! let mut tweens = TweenSet::load_from_json(&document, 60.0, 128.0).expect("tweens");
//!
//! // Every frame, before rendering: each tween link read advances its tween
//! // exactly once (`dataTypes.tween.get` semantics).
//! let alpha = resolve_tween_link(&mut tweens, "input-1", 0).expect("link value");
//! assert!((0.0..=1.0).contains(&alpha));
//!
//! // `tweens.advance_all()` is only for tweens no link read this frame —
//! // calling it for "input-1" as well would advance it twice.
//! ```
//!
//! # Fidelity notes
//!
//! The maths is ported line for line, including the parts that look odd:
//!
//! - `newDuration` divides by `data.length` unless `durationAsTotalTime` is
//!   set, so a two-row tween plays `duration / 2` per row.
//! - `framesRequired = Math.round(newDuration / (1000 / fps)) || 1` — the
//!   rounding is JS `Math.round` (half up), and a zero/`NaN` result becomes 1.
//!   A leg is capped at [`MAX_TWEEN_FRAMES`] so a corrupt document cannot
//!   exhaust memory (the JS would allocate whatever it asked for).
//! - A looped tween records frames `0..=framesRequired` and then `-1` back to
//!   `1` (the ping-pong is done by the frame recorder, not by anime's
//!   `direction`), giving `2 × framesRequired` frames per cycle. The native
//!   extension `loopMode: "restart"` opts into a single forward leg instead,
//!   so the play head wraps to frame 0 and replays the same pattern.
//! - Each channel is sampled the way anime normalises a property array. A
//!   two-row tween is anime's `[from, to]` shorthand (one tween over the whole
//!   duration). Three or more rows become one tween per keyframe, each
//!   `newDuration / rows` long: the first goes from row 0 to keyframe 0 (row
//!   0 again), so the first `1/rows` of the run holds, then each following
//!   segment moves through one row. The easing and `steps` quantisation are
//!   evaluated per segment on local progress, not once across the whole run.
//!   `data/tween-keyframe-reference.json` records the anime 3.2.2 output.
//! - anime applies the easing at time 0 during `reset()`, so a two-row tween's
//!   frame 0 is `ease(0)` (not the raw start value) — observable with
//!   `steps()`, where it is `1/steps` of the way rather than 0. A 3+ row
//!   tween's frame 0 is the raw first row because the first segment holds.
//! - `direction: "reverse"` is effectively a no-op in anime 3.2: `seek()`
//!   applies the reversed-time adjustment twice, which cancels for every
//!   sample except the very first one (frame 0 keeps the reset value, which is
//!   `ease(1)` instead of `ease(0)`). The legacy worker records exactly that,
//!   so [`build_tween_frames`] reproduces it. `alternate` behaves like
//!   `normal` while recording; the ping-pong comes from the loop recorder.
//!
//! [`TweenDescriptor`] keeps unknown keys verbatim so a load → save round trip
//! is lossless. Gallery tweens (`isGallery: true`) stay in the document; the
//! preset serializer decides whether to keep them (`createPresetData` filters
//! them in JS), exactly as before — nothing here changes serialization
//! semantics.
//!
//! The reference numbers in `crates/modv-engine/data/tween-reference.json`
//! were produced by anime.js 3.2.2 itself (see the tests); every easing and
//! frame is compared against them. `tween-keyframe-reference.json` holds the
//! multi-keyframe (3+ `data` rows) frames captured from the legacy worker's
//! `buildFrames` with the same anime.js version.
//!
//! # Free keyframe times (native extension)
//!
//! A descriptor may carry an optional `keyframeTimes: [0, 0.35, 1]`: the
//! normalised progress of every `data` row, strictly increasing, with the
//! first at 0 and the last at 1. Electron ignores the key, so it stays
//! Electron-readable; the VmodV editor uses it to drag keyframes on both
//! axes. Valid times replace anime's even keyframe spacing (including the
//! leading hold) with a segment between each pair of times, easing the local
//! progress with the descriptor's easing/`steps`. With no times, or with an
//! invalid list (wrong length, non-finite, non-monotonic, wrong endpoints),
//! sampling uses the anime semantics above. Invalid raw values stay in
//! [`TweenDescriptor::extra`] so a load -> save round trip keeps them verbatim;
//! a two-row `[0, 1]` list samples exactly like an even-spaced two-row tween.
//!
//! [`TweenDescriptor::with_free_keyframes`] performs the one-way conversion the
//! curve editor uses: anime's evenly spaced multi-keyframe shape becomes an
//! explicit list (a duplicate first row plus the even grid) that plays back
//! identically. Evenly spaced free times are sampled with anime's segment
//! arithmetic rather than the normalised fractions, so the conversion is
//! bit-exact for wall-clock tweens (a BPM cycle can move by one ulp because the
//! `(60 * 60 / bpm) * division` base is re-expressed for the extra row).
//!

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::engine_state::serialize_minimal_f64;

/// Upper bound on recorded frames per animation leg.
///
/// The JS would allocate whatever the duration asks for, which lets a corrupt
/// document (a huge `duration` with a bogus `fps`) exhaust memory. Real tweens
/// are well under a thousand frames per leg, so the cap only ever affects
/// pathological input.
pub const MAX_TWEEN_FRAMES: usize = 100_000;

/// The anime.js version the reference fixture was generated with.
pub const ANIME_REFERENCE_VERSION: &str = "3.2.2";

// ————————————————— descriptors —————————————————

/// Which known keys were present in the document.
///
/// JS `JSON.stringify` omits `undefined`, and a tween the engine did not
/// create itself may legitimately be partial; keeping the presence flags lets
/// a load → save round trip emit exactly the keys the document had. Tweens
/// created by [`TweenDescriptor::new`] carry every key, like `createTween`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Presence {
    data: bool,
    duration: bool,
    direction: bool,
    easing: bool,
    looping: bool,
    use_bpm: bool,
    bpm_division: bool,
    duration_as_total_time: bool,
    steps: bool,
}

impl Presence {
    /// Every known key (what `createTween` writes).
    const ALL: Presence = Presence {
        data: true,
        duration: true,
        direction: true,
        easing: true,
        looping: true,
        use_bpm: true,
        bpm_division: true,
        duration_as_total_time: true,
        steps: true,
    };
}

/// A tween descriptor, as it appears in `state.tweens` (a preset array).
///
/// Field defaults follow `tweens.js` `createTween` (the destructuring
/// defaults), so a partial document behaves like the JS loader does. Keys the
/// engine does not know (including `frames` from the live worker store) are
/// kept verbatim in [`TweenDescriptor::extra`].
#[derive(Clone, Debug, PartialEq)]
pub struct TweenDescriptor {
    /// Tween id (the id of the Input Link that reads it).
    pub id: String,
    /// Keyframe rows; channel `i` is the sequence `data[row][i]`.
    pub data: Vec<Vec<f64>>,
    /// Optional native extension: the progress (0..1) of each `data` row.
    ///
    /// Electron-authored tweens omit it and get anime's even keyframe spacing.
    /// When present it must have one entry per row, be finite and strictly
    /// increasing, and start at 0 / end at 1; an invalid list is ignored for
    /// sampling but kept verbatim in [`TweenDescriptor::extra`] so a load ->
    /// save round trip does not lose it.
    pub keyframe_times: Option<Vec<f64>>,
    /// Duration in milliseconds (before the `data.length` division).
    pub duration: f64,
    /// `normal`, `reverse` or `alternate` (see the module docs).
    pub direction: String,
    /// anime.js easing name (`easeInOutSine`, `linear`, `steps(4)`, ...).
    pub easing: String,
    /// Whether the tween loops (`loop: true`). By default the recorder
    /// ping-pongs the frames; [`Self::loop_mode`] can opt into a restart.
    pub looping: bool,
    /// Optional native extension: how a looping tween restarts (`loopMode`).
    ///
    /// Absent or any value other than `"restart"` keeps the legacy ping-pong
    /// recorder. `"restart"` records a single forward leg, so
    /// [`TweenRuntime::advance`](TweenRuntime::advance) wraps to frame 0 and
    /// replays the drawn pattern beat-for-beat.
    pub loop_mode: Option<String>,
    /// Whether the duration follows the tempo (`useBpm: true`).
    pub use_bpm: bool,
    /// BPM subdivision the tween advances on.
    pub bpm_division: f64,
    /// Treat `duration` as the total time instead of per-row time.
    pub duration_as_total_time: bool,
    /// `steps(n)` quantisation, or 0 (default) for the eased interpolation.
    pub steps: f64,
    /// `isGallery` (absent means "not a gallery tween", like `undefined`).
    pub is_gallery: Option<bool>,
    /// Unknown keys, kept verbatim for round-trip fidelity.
    pub extra: Map<String, Value>,
    /// Which known keys the document carried.
    presence: Presence,
}

impl TweenDescriptor {
    /// A descriptor with the `createTween` defaults, every key present.
    ///
    /// Defaults (`tweens.js:147-163`): `duration: 1000`,
    /// `direction: "alternate"`, `easing: "linear"`, `loop: true`,
    /// `useBpm: true`, `bpmDivision: 16`, `durationAsTotalTime: false`,
    /// `steps: 0`.
    pub fn new(id: impl Into<String>, data: Vec<Vec<f64>>) -> Self {
        Self {
            id: id.into(),
            data,
            keyframe_times: None,
            duration: 1000.0,
            direction: "alternate".to_string(),
            easing: "linear".to_string(),
            looping: true,
            loop_mode: None,
            use_bpm: true,
            bpm_division: 16.0,
            duration_as_total_time: false,
            steps: 0.0,
            is_gallery: None,
            extra: Map::new(),
            presence: Presence::ALL,
        }
    }

    /// Number of animated channels (the widest `data` row).
    pub fn channel_count(&self) -> usize {
        self.data.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// The free keyframe times when they are valid for this descriptor.
    ///
    /// Validation also runs here (not just on load) because the field is
    /// public: a caller that sets `keyframe_times` and then changes `data`
    /// must not sample with a mismatched list.
    pub fn effective_keyframe_times(&self) -> Option<&[f64]> {
        self.keyframe_times
            .as_deref()
            .filter(|times| valid_keyframe_times(times, self.data.len()))
    }

    /// The effective duration of one animation leg, in milliseconds.
    ///
    /// `buildFrames` (`tweens.js:43-54`): BPM tweens compute
    /// `((60 * 60) / bpm) * bpmDivision`, everything else uses `duration`;
    /// both divide by `data.length` unless `durationAsTotalTime` is set. A
    /// falsy BPM (`0`, `NaN`) falls back to 120 like
    /// `store.state.beats.bpm || 120`. For 3+ rows this is the total anime
    /// duration, which anime then splits between its keyframe tweens.
    pub fn effective_duration(&self, bpm: f64) -> f64 {
        let base = if self.use_bpm {
            let bpm = if bpm == 0.0 || bpm.is_nan() {
                120.0
            } else {
                bpm
            };
            ((60.0 * 60.0) / bpm) * self.bpm_division
        } else {
            self.duration
        };

        if self.duration_as_total_time {
            base
        } else {
            base / self.data.len() as f64
        }
    }

    /// Whether a looping descriptor opts into restarting each cycle instead
    /// of the legacy ping-pong recorder (`loopMode: "restart"`).
    pub fn restarts_on_loop(&self) -> bool {
        self.loop_mode.as_deref() == Some("restart")
    }

    /// Whether the descriptor still uses anime's evenly spaced keyframes:
    /// three or more rows and no usable `keyframeTimes`.
    pub fn uses_even_keyframes(&self) -> bool {
        self.data.len() >= 3 && self.effective_keyframe_times().is_none()
    }

    /// Rewrite an evenly keyframed tween as a free-times tween that plays back
    /// the same way.
    ///
    /// anime's 3+ row shape holds row 0 for the first `1/N` of the run and
    /// then reaches rows `1..N-1` at `2/N..1` (one eased segment per
    /// keyframe). Duplicating row 0 and placing the `N+1` rows on the even grid
    /// `[0, 1/N, ..., 1]` expresses exactly the same segments with explicit
    /// times, so the curve editor can drag them.
    ///
    /// The extra row would otherwise divide the animation duration once more
    /// (`effective_duration` divides by the row count unless
    /// `durationAsTotalTime`), so the duration is re-expressed as the total
    /// time: the wall-clock shape is bit-exact, and a BPM tween's division is
    /// scaled so the cycle length is preserved to the last ulp the
    /// `(60 * 60 / bpm) * division` formula allows.
    ///
    /// Two-row tweens (anime's `[from, to]` shorthand) and descriptors that
    /// already carry valid times are returned unchanged.
    pub fn with_free_keyframes(&self) -> Self {
        if !self.uses_even_keyframes() {
            return self.clone();
        }

        let rows = self.data.len();
        let mut data = Vec::with_capacity(rows + 1);
        data.push(self.data[0].clone());
        data.extend(self.data.iter().cloned());
        let times: Vec<f64> = (0..=rows).map(|index| index as f64 / rows as f64).collect();

        let mut descriptor = self.clone();
        descriptor.data = data;
        descriptor.keyframe_times = Some(times);
        if !descriptor.duration_as_total_time {
            descriptor.duration_as_total_time = true;
            if descriptor.use_bpm {
                descriptor.bpm_division /= rows as f64;
            } else {
                descriptor.duration /= rows as f64;
            }
        }
        descriptor
    }

    /// Serialize back to the preset shape.
    ///
    /// Non-finite numeric fields are omitted rather than serialised: a
    /// `duration` of `Infinity` (what `update_bpm(0.0)` computes) has no JSON
    /// representation, and emitting `null` or `{}` would either fail the
    /// reload or drop the descriptor — and, through the set-level serialiser,
    /// every other tween with it (R-5). The rest of the descriptor (its `id`,
    /// `data`, easing, ...) is kept, so the entry survives a round trip and
    /// only the corrupt field falls back to its `create_tween` default.
    ///
    /// A non-finite value inside `data` omits the whole `data` field for the
    /// same reason: channels are positional, so dropping a channel would shift
    /// the rest.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("id".to_string(), Value::String(self.id.clone()));
        if self.presence.data
            && let Some(data) = finite_grid(&self.data)
        {
            map.insert("data".to_string(), data);
        }
        if self.presence.duration
            && let Some(value) = finite_number(self.duration)
        {
            map.insert("duration".to_string(), value);
        }
        if self.presence.direction {
            map.insert(
                "direction".to_string(),
                Value::String(self.direction.clone()),
            );
        }
        if self.presence.easing {
            map.insert("easing".to_string(), Value::String(self.easing.clone()));
        }
        if self.presence.looping {
            map.insert("loop".to_string(), Value::Bool(self.looping));
        }
        if let Some(loop_mode) = &self.loop_mode {
            map.insert("loopMode".to_string(), Value::String(loop_mode.clone()));
        }
        if self.presence.use_bpm {
            map.insert("useBpm".to_string(), Value::Bool(self.use_bpm));
        }
        if self.presence.bpm_division
            && let Some(value) = finite_number(self.bpm_division)
        {
            map.insert("bpmDivision".to_string(), value);
        }
        if self.presence.duration_as_total_time {
            map.insert(
                "durationAsTotalTime".to_string(),
                Value::Bool(self.duration_as_total_time),
            );
        }
        if let Some(is_gallery) = self.is_gallery {
            map.insert("isGallery".to_string(), Value::Bool(is_gallery));
        }
        if self.presence.steps
            && let Some(value) = finite_number(self.steps)
        {
            map.insert("steps".to_string(), value);
        }
        if let Some(times) = &self.keyframe_times
            && let Some(value) = finite_numbers(times)
        {
            map.insert("keyframeTimes".to_string(), value);
        }
        for (key, value) in &self.extra {
            map.insert(key.clone(), value.clone());
        }

        Value::Object(map)
    }
}

/// Whether `times` is a usable free keyframe list for a `rows`-row tween.
///
/// The contract the VmodV editor writes: one entry per `data` row, finite,
/// strictly increasing, first 0 and last 1. Anything else is ignored for
/// sampling (even spacing / anime keyframes) rather than failing the document.
fn valid_keyframe_times(times: &[f64], rows: usize) -> bool {
    times.len() == rows
        && times.len() >= 2
        && times.iter().all(|time| time.is_finite())
        && times[0] == 0.0
        && times[times.len() - 1] == 1.0
        && times.windows(2).all(|pair| pair[0] < pair[1])
}

/// Raw `keyframeTimes` value, present or not.
///
/// [`Value`] alone cannot distinguish an absent key from an explicit `null`,
/// and both must round-trip exactly: absent means "even spacing, emit nothing"
/// while `null` is invalid input that stays in `extra` verbatim.
#[derive(Default)]
struct RawKeyframeTimes(Option<Value>);

impl<'de> Deserialize<'de> for RawKeyframeTimes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Value::deserialize(deserializer).map(|value| Self(Some(value)))
    }
}

/// `f64` serialized the way `JSON.stringify` writes it (see
/// [`crate::engine_state::serialize_minimal_f64`]).
struct MinimalF64(f64);

impl Serialize for MinimalF64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_minimal_f64(&self.0, serializer)
    }
}

/// `Vec<Vec<f64>>` serialized with minimal numeric formatting.
struct MinimalF64Grid<'a>(&'a [Vec<f64>]);

impl Serialize for MinimalF64Grid<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            sequence.serialize_element(&MinimalF64Row(row))?;
        }
        sequence.end()
    }
}

/// One `data` row with minimal numeric formatting.
struct MinimalF64Row<'a>(&'a [f64]);

impl Serialize for MinimalF64Row<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&MinimalF64(*value))?;
        }
        sequence.end()
    }
}

/// A finite number in the minimal `JSON.stringify` form, or `None` for
/// `NaN`/`±Infinity`, which JSON cannot represent.
fn finite_number(value: f64) -> Option<Value> {
    if value.is_finite() {
        serde_json::to_value(MinimalF64(value)).ok()
    } else {
        None
    }
}

/// [`finite_number`] for the whole `data` grid: `None` when any value is
/// non-finite (channels are positional, so a dropped channel would shift the
/// rest).
fn finite_grid(data: &[Vec<f64>]) -> Option<Value> {
    if data.iter().flatten().all(|value| value.is_finite()) {
        serde_json::to_value(MinimalF64Grid(data)).ok()
    } else {
        None
    }
}

/// [`finite_number`] for a flat number list (`keyframeTimes`).
fn finite_numbers(values: &[f64]) -> Option<Value> {
    if values.iter().all(|value| value.is_finite()) {
        serde_json::to_value(MinimalF64Row(values)).ok()
    } else {
        None
    }
}

impl Serialize for TweenDescriptor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // One implementation: [`TweenDescriptor::to_json`] is the finite-only
        // shape, and it is infallible, so a serde caller can never end up with
        // a descriptor that serialises as `{}` (R-5).
        self.to_json().serialize(serializer)
    }
}

/// Raw shape for deserialization: every known key is optional so the presence
/// flags (and therefore the emitted key set) survive a round trip.
#[derive(Deserialize)]
struct TweenDescriptorRepr {
    id: String,
    #[serde(default)]
    data: Option<Vec<Vec<f64>>>,
    #[serde(default, rename = "keyframeTimes")]
    keyframe_times: RawKeyframeTimes,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    easing: Option<String>,
    #[serde(default, rename = "loop")]
    looping: Option<bool>,
    #[serde(default, rename = "loopMode")]
    loop_mode: Option<String>,
    #[serde(default, rename = "useBpm")]
    use_bpm: Option<bool>,
    #[serde(default, rename = "bpmDivision")]
    bpm_division: Option<f64>,
    #[serde(default, rename = "durationAsTotalTime")]
    duration_as_total_time: Option<bool>,
    #[serde(default)]
    steps: Option<f64>,
    #[serde(default, rename = "isGallery")]
    is_gallery: Option<bool>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl<'de> Deserialize<'de> for TweenDescriptor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let repr = TweenDescriptorRepr::deserialize(deserializer)?;
        let presence = Presence {
            data: repr.data.is_some(),
            duration: repr.duration.is_some(),
            direction: repr.direction.is_some(),
            easing: repr.easing.is_some(),
            looping: repr.looping.is_some(),
            use_bpm: repr.use_bpm.is_some(),
            bpm_division: repr.bpm_division.is_some(),
            duration_as_total_time: repr.duration_as_total_time.is_some(),
            steps: repr.steps.is_some(),
        };

        let mut extra = repr.extra;
        let data = repr.data.unwrap_or_default();
        let rows = data.len();
        // Keep invalid raw times verbatim (in `extra`) instead of failing the
        // descriptor; sampling falls back to anime's even keyframe spacing.
        let keyframe_times = match repr.keyframe_times.0 {
            Some(raw) => match serde_json::from_value::<Vec<f64>>(raw.clone()) {
                Ok(times) if valid_keyframe_times(&times, rows) => Some(times),
                _ => {
                    extra.insert("keyframeTimes".to_string(), raw);
                    None
                }
            },
            None => None,
        };

        Ok(TweenDescriptor {
            id: repr.id,
            data,
            keyframe_times,
            duration: repr.duration.unwrap_or(1000.0),
            direction: repr.direction.unwrap_or_else(|| "alternate".to_string()),
            easing: repr.easing.unwrap_or_else(|| "linear".to_string()),
            looping: repr.looping.unwrap_or(true),
            loop_mode: repr.loop_mode,
            use_bpm: repr.use_bpm.unwrap_or(true),
            bpm_division: repr.bpm_division.unwrap_or(16.0),
            duration_as_total_time: repr.duration_as_total_time.unwrap_or(false),
            steps: repr.steps.unwrap_or(0.0),
            is_gallery: repr.is_gallery,
            extra,
            presence,
        })
    }
}

// ————————————————— easings —————————————————

/// The anime.js `penner` easing catalogue, in `Object.keys` order.
///
/// The store exposes this list to the UI (`tweens.js:18-24`), which builds the
/// labels by splitting camel case, so the order is part of the port.
pub const PENNER_EASING_NAMES: &[&str] = &[
    "linear",
    "easeInSine",
    "easeOutSine",
    "easeInOutSine",
    "easeOutInSine",
    "easeInExpo",
    "easeOutExpo",
    "easeInOutExpo",
    "easeOutInExpo",
    "easeInCirc",
    "easeOutCirc",
    "easeInOutCirc",
    "easeOutInCirc",
    "easeInBack",
    "easeOutBack",
    "easeInOutBack",
    "easeOutInBack",
    "easeInBounce",
    "easeOutBounce",
    "easeInOutBounce",
    "easeOutInBounce",
    "easeInElastic",
    "easeOutElastic",
    "easeInOutElastic",
    "easeOutInElastic",
    "easeInQuad",
    "easeOutQuad",
    "easeInOutQuad",
    "easeOutInQuad",
    "easeInCubic",
    "easeOutCubic",
    "easeInOutCubic",
    "easeOutInCubic",
    "easeInQuart",
    "easeOutQuart",
    "easeInOutQuart",
    "easeOutInQuart",
    "easeInQuint",
    "easeOutQuint",
    "easeInOutQuint",
    "easeOutInQuint",
];

/// `minMax` (`anime.es.js:34`).
fn min_max(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

/// anime's `steps(n)` easing (`anime.es.js:133-137`): progress is quantised
/// upwards to `1/n` steps, with the CSS `1e-6` lower clamp.
fn steps_ease(t: f64, steps: f64) -> f64 {
    (min_max(t, 0.000_001, 1.0) * steps).ceil() * (1.0 / steps)
}

/// The anime.js "ease in" half of a Penner family (`anime.es.js:227-242`).
fn ease_in_family(family: &str, t: f64) -> Option<f64> {
    match family {
        "Sine" => Some(1.0 - (t * std::f64::consts::FRAC_PI_2).cos()),
        "Expo" => Some(if t == 0.0 {
            0.0
        } else {
            2f64.powf(10.0 * t - 10.0)
        }),
        "Circ" => Some(1.0 - (1.0 - t * t).sqrt()),
        "Back" => Some(t * t * (3.0 * t - 2.0)),
        "Bounce" => Some(ease_in_bounce(t)),
        "Elastic" => Some(ease_in_elastic(t)),
        "Quad" => Some(t * t),
        "Cubic" => Some(t * t * t),
        "Quart" => Some(t * t * t * t),
        "Quint" => Some(t * t * t * t * t),
        _ => None,
    }
}

/// `easeInBounce` (`anime.es.js:233-236`).
fn ease_in_bounce(t: f64) -> f64 {
    // `while (t < ((pow2 = 2^--b) - 1) / 11) {}` — b ends on the first value
    // (3, 2, 1 or 0) whose threshold is at or below `t`.
    let mut b: i32 = 4;
    let mut pow2: f64;
    loop {
        b -= 1;
        pow2 = 2f64.powi(b);
        if t >= (pow2 - 1.0) / 11.0 {
            break;
        }
    }
    1.0 / 4f64.powi(3 - b) - 7.5625 * ((pow2 * 3.0 - 2.0) / 22.0 - t).powi(2)
}

/// `easeInElastic` with anime's default parameters (`amplitude = 1`,
/// `period = 0.5`), `anime.es.js:238-247`.
fn ease_in_elastic(t: f64) -> f64 {
    if t == 0.0 || t == 1.0 {
        return t;
    }
    let amplitude: f64 = 1.0;
    let period: f64 = 0.5;
    let offset = period / (std::f64::consts::TAU) * (1.0 / amplitude).asin();
    -amplitude
        * 2f64.powf(10.0 * (t - 1.0))
        * (((t - 1.0) - offset) * std::f64::consts::TAU / period).sin()
}

/// The four directions anime derives a Penner family into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EaseForm {
    /// `easeIn<Family>`.
    In,
    /// `easeOut<Family>`.
    Out,
    /// `easeInOut<Family>`.
    InOut,
    /// `easeOutIn<Family>`.
    OutIn,
}

/// Evaluate an anime.js easing name at progress `t`.
///
/// Covers the whole `penner` catalogue ([`PENNER_EASING_NAMES`]) plus
/// parameterised `steps(n)` strings. Unknown names fall back to `linear`
/// (`t`) — the JS would throw here, but a preset from a newer build must not
/// take the engine down, and `linear` is the value anime uses for a missing
/// easing (`tweens.js:89`).
pub fn ease(name: &str, t: f64) -> f64 {
    if let Some(argument) = name
        .strip_prefix("steps(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        // `applyArguments(steps, args)` with no argument defaults to 10.
        return match argument.trim().parse::<f64>() {
            Ok(steps) => steps_ease(t, steps),
            Err(_) => steps_ease(t, 10.0),
        };
    }

    if name == "linear" {
        return t;
    }

    // `easeOutIn` / `easeInOut` are longer prefixes than `easeIn`/`easeOut`.
    let (form, family) = if let Some(family) = name.strip_prefix("easeInOut") {
        (EaseForm::InOut, family)
    } else if let Some(family) = name.strip_prefix("easeOutIn") {
        (EaseForm::OutIn, family)
    } else if let Some(family) = name.strip_prefix("easeIn") {
        (EaseForm::In, family)
    } else if let Some(family) = name.strip_prefix("easeOut") {
        (EaseForm::Out, family)
    } else {
        return t;
    };

    // Unknown families (and unknown names) fall back to linear.
    let ease_in = |value: f64| match ease_in_family(family, value) {
        Some(eased) => eased,
        None => value,
    };

    if ease_in_family(family, 0.0).is_none() {
        return t;
    }

    match form {
        EaseForm::In => ease_in(t),
        EaseForm::Out => 1.0 - ease_in(1.0 - t),
        EaseForm::InOut => {
            if t < 0.5 {
                ease_in(t * 2.0) / 2.0
            } else {
                1.0 - ease_in(t * -2.0 + 2.0) / 2.0
            }
        }
        EaseForm::OutIn => {
            if t < 0.5 {
                (1.0 - ease_in(1.0 - t * 2.0)) / 2.0
            } else {
                (ease_in(t * 2.0 - 1.0) + 1.0) / 2.0
            }
        }
    }
}

// ————————————————— frame pre-renderer —————————————————

/// JS `Math.round`: ties go up, and non-finite input stays non-finite.
fn js_round(value: f64) -> f64 {
    if value.is_finite() {
        (value + 0.5).floor()
    } else {
        value
    }
}

/// `Math.round(newDuration / (1000 / fps)) || 1`, clamped to
/// [`MAX_TWEEN_FRAMES`].
fn frames_required(new_duration: f64, fps: f64) -> usize {
    let required = js_round(new_duration / (1000.0 / fps));

    if !required.is_finite() || required < 1.0 {
        return 1;
    }

    (required.min(MAX_TWEEN_FRAMES as f64)) as usize
}

/// One animated channel: its keyframe values in `data` row order.
struct Channel {
    values: Vec<f64>,
}

/// The per-channel keyframe values of a tween.
struct Channels {
    channels: Vec<Channel>,
}

impl Channels {
    /// Transpose the `data` rows into per-channel keyframe values.
    ///
    /// `tweens.js:68-83` builds `mapped[index]` from the value at `index` of
    /// every row; anime then receives that array as the property value. A row
    /// that does not have the channel is skipped, matching the JS loop.
    fn from_data(data: &[Vec<f64>]) -> Self {
        let channel_count = data.iter().map(Vec::len).max().unwrap_or(0);
        let mut channels = Vec::with_capacity(channel_count);

        for channel in 0..channel_count {
            let values: Vec<f64> = data
                .iter()
                .filter_map(|row| row.get(channel).copied())
                .collect();

            channels.push(Channel { values });
        }

        Self { channels }
    }

    /// The channel values at `time` (milliseconds) of the animation.
    ///
    /// anime's `setAnimationsProgress`: for a two-value property array the
    /// single tween runs over the whole duration; for three or more values it
    /// builds one tween per keyframe (`duration / valueCount` each) and the
    /// first one holds `data[0]`.
    fn sample(&self, tween: &TweenDescriptor, time: f64, duration: f64) -> Vec<f64> {
        self.channels
            .iter()
            .map(|channel| channel.sample(tween, time, duration))
            .collect()
    }
}

impl Channel {
    /// Sample this channel at `time`.
    ///
    /// The two-row shape is anime's `[from, to]` shorthand: one tween over the
    /// whole duration. Three or more rows become one tween per keyframe, each
    /// `duration / values.len()` long; the first goes from the target's
    /// current value (keyframe 0) to keyframe 0, so it is a hold. The easing
    /// (and `steps` quantisation) is evaluated on the segment's local progress,
    /// not across the whole run.
    fn sample(&self, tween: &TweenDescriptor, time: f64, duration: f64) -> f64 {
        let values = &self.values;

        // Free keyframe positions are a native extension: when present and
        // valid, they replace anime's even keyframe spacing. A ragged channel
        // whose value count differs from the row count cannot map onto the
        // times, so it keeps the anime semantics.
        if let Some(times) = tween
            .effective_keyframe_times()
            .filter(|times| times.len() == values.len())
        {
            return sample_free(self, tween, times, time, duration);
        }

        match values.len() {
            0 => 0.0,
            1 => values[0],
            2 => {
                let elapsed = normalize_elapsed(time, duration);
                interpolate(values[0], values[1], eased(tween, elapsed))
            }
            count => {
                let segment = duration / count as f64;
                let (active, start) = active_keyframe_tween(count, segment, time);
                let elapsed = normalize_elapsed(time - start, segment);
                interpolate(
                    values[active.saturating_sub(1)],
                    values[active],
                    eased(tween, elapsed),
                )
            }
        }
    }
}

/// Sample a channel whose keyframes sit at explicit `times`.
///
/// Unlike anime's normalised keyframes there is no leading hold: the first
/// segment starts at progress 0 and the last ends at 1. The easing/`steps` is
/// applied to the segment's local progress.
fn sample_free(
    channel: &Channel,
    tween: &TweenDescriptor,
    times: &[f64],
    time: f64,
    duration: f64,
) -> f64 {
    let values = &channel.values;
    let progress = if duration == 0.0 || duration.is_nan() {
        1.0
    } else {
        let progress = time / duration;
        if progress.is_nan() {
            1.0
        } else {
            progress.clamp(0.0, 1.0)
        }
    };

    // Evenly spaced free times are the representation a normalised tween uses
    // for anime's even keyframes (see [`TweenDescriptor::normalised_keyframes`])
    // and the maths is identical there: one segment per pair of rows, each
    // `duration / segments` long. Compute it with anime's segment arithmetic
    // rather than from the normalised fractions, so normalising an animated
    // tween is a pure representation change (bit-identical frames).
    if evenly_spaced(times) {
        let segments = values.len() - 1;
        let segment = duration / segments as f64;
        let (index, start) = active_keyframe_tween(segments, segment, time);
        let elapsed = normalize_elapsed(time - start, segment);
        return interpolate(values[index], values[index + 1], eased(tween, elapsed));
    }

    let mut index = values.len() - 2;
    for candidate in 0..values.len() - 1 {
        if progress < times[candidate + 1] {
            index = candidate;
            break;
        }
    }

    let width = times[index + 1] - times[index];
    let local = ((progress - times[index]) / width).clamp(0.0, 1.0);

    interpolate(values[index], values[index + 1], eased(tween, local))
}

/// Whether `times` is the exact even grid `0, 1/(n-1), ..., 1`.
fn evenly_spaced(times: &[f64]) -> bool {
    let last = times.len().saturating_sub(1);
    last > 0
        && times
            .iter()
            .enumerate()
            .all(|(index, time)| *time == index as f64 / last as f64)
}

/// The active tween index and its start time for a 3+ keyframe channel.
///
/// anime picks the first tween whose `end` is strictly greater than the seek
/// time and falls back to the last tween at (or past) the instance duration
/// (`filterArray(tweens, t => insTime < t.end)[0] || last`). The ends are
/// accumulated (`start` is the previous end), so this reproduces anime's
/// floating-point boundary behaviour.
fn active_keyframe_tween(count: usize, segment: f64, time: f64) -> (usize, f64) {
    let mut start = 0.0;
    let mut active_start = 0.0;

    for index in 0..count {
        active_start = start;
        let end = start + segment;

        if time < end {
            return (index, active_start);
        }

        start = end;
    }

    (count - 1, active_start)
}

/// `minMax(value, 0, duration) / duration` as anime computes `elapsed`.
///
/// A `NaN` elapsed (zero duration) is kept so [`eased`] can treat it as
/// complete, like anime's `isNaN(elapsed) ? 1 : easing(elapsed)`.
fn normalize_elapsed(value: f64, duration: f64) -> f64 {
    if value.is_nan() || duration.is_nan() {
        f64::NAN
    } else {
        min_max(value, 0.0, duration) / duration
    }
}

/// Apply the descriptor's easing (or `steps(n)` quantisation) to `elapsed`.
fn eased(tween: &TweenDescriptor, elapsed: f64) -> f64 {
    if elapsed.is_nan() {
        return 1.0;
    }

    if tween.steps != 0.0 && !tween.steps.is_nan() {
        steps_ease(elapsed, tween.steps)
    } else {
        ease(&tween.easing, elapsed)
    }
}

/// `from + progress × (to - from)`.
fn interpolate(from: f64, to: f64, progress: f64) -> f64 {
    from + progress * (to - from)
}

/// Pre-render the tween's frames, starting at the beginning.
///
/// A line-for-line port of `buildFrames` (`tweens.js:31-133`) without the
/// `anime` dependency. `fps` is the store's target frame rate and `bpm` the
/// current tempo (`0`/`NaN` falls back to 120 for BPM tweens).
///
/// The returned vector holds one frame per stored value row:
///
/// - `loop`: `2 × framesRequired` frames — `0..=framesRequired` then
///   `framesRequired - 1 ..= 1` (frame 0 is not repeated at the end), unless
///   the native `loopMode: "restart"` extension is set, which records only
///   the `framesRequired`-frame forward leg.
/// - no loop: `framesRequired` frames, `0..framesRequired`.
///
/// A tween without `data` has no channels and cannot animate (the JS would
/// divide by zero and loop forever), so it returns a single empty frame.
pub fn build_tween_frames(tween: &TweenDescriptor, fps: f64, bpm: f64) -> Vec<Vec<f64>> {
    build_frames_with_seek(tween, fps, bpm, 0.0).0
}

/// [`build_tween_frames`] plus the `seek` position restore.
///
/// Returns the new frames and the frame index that preserves `seek`
/// (`frame = Math.floor(newFrames.len() * seek)`, `tweens.js:129`).
fn build_frames_with_seek(
    tween: &TweenDescriptor,
    fps: f64,
    bpm: f64,
    seek: f64,
) -> (Vec<Vec<f64>>, usize) {
    if tween.data.is_empty() {
        return (vec![Vec::new()], 0);
    }

    let new_duration = tween.effective_duration(bpm);
    let frames_required = frames_required(new_duration, fps);
    let channels = Channels::from_data(&tween.data);

    // The value anime installs during `reset()`, before the first `seek(0)`
    // runs (which does not update the target): the start of the timeline
    // normally, the end when the instance was created reversed.
    let reset_frame = channels.sample(
        tween,
        if tween.direction == "reverse" {
            new_duration
        } else {
            0.0
        },
        new_duration,
    );

    let sample = |frame: usize| {
        if frame == 0 {
            reset_frame.clone()
        } else {
            let time = (frame as f64 / frames_required as f64) * new_duration;
            channels.sample(tween, time, new_duration)
        }
    };

    if !tween.looping || tween.restarts_on_loop() {
        let mut frames = Vec::with_capacity(frames_required);
        frames.push(reset_frame.clone());
        for frame in 1..frames_required {
            frames.push(sample(frame));
        }
        // `tweens.js:129` restores the seek position for both branches, not
        // just the looped one: a rebuild must not snap a one-shot tween back
        // to frame 0. A restart loop wraps to frame 0 on its own, so a single
        // forward leg replays the drawn pattern beat-for-beat.
        let restored = restored_frame(frames.len(), seek);
        return (frames, restored);
    }

    let mut frames = Vec::with_capacity(frames_required * 2);
    let mut frame = 0usize;
    let mut step: i64 = 1;

    loop {
        frames.push(sample(frame));

        if frame == frames_required {
            step = -1;
        }

        let next = frame as i64 + step;
        frame = next.max(0) as usize;

        if frame == 0 {
            break;
        }
    }

    let restored = restored_frame(frames.len(), seek);
    (frames, restored)
}

/// `Math.floor(len * seek)`, clamped into the frame vector.
fn restored_frame(len: usize, seek: f64) -> usize {
    if len == 0 {
        return 0;
    }

    let restored = (len as f64 * seek).floor();

    if !restored.is_finite() || restored < 0.0 {
        return 0;
    }

    (restored.min((len - 1) as f64)) as usize
}

// ————————————————— runtime —————————————————

/// A live tween: its descriptor, the pre-rendered frames and the play head.
///
/// Mirrors the pair `state.tweens.tweens[id]` (descriptor + `frames`) and the
/// module-level `frames[id]` counter in `tweens.js:13,231-254`.
#[derive(Clone, Debug)]
pub struct TweenRuntime {
    descriptor: TweenDescriptor,
    frames: Vec<Vec<f64>>,
    frame: usize,
    fps: f64,
    bpm: f64,
}

impl TweenRuntime {
    /// Build the frames for `descriptor` at the current `fps`/`bpm`.
    pub fn new(descriptor: TweenDescriptor, fps: f64, bpm: f64) -> Self {
        let (frames, frame) = build_frames_with_seek(&descriptor, fps, bpm, 0.0);

        Self {
            descriptor,
            frames,
            frame,
            fps,
            bpm,
        }
    }

    /// The descriptor (including unknown keys).
    pub fn descriptor(&self) -> &TweenDescriptor {
        &self.descriptor
    }

    /// The descriptor, mutably. Call [`TweenRuntime::rebuild`] afterwards to
    /// re-render the frames for the edited settings.
    pub fn descriptor_mut(&mut self) -> &mut TweenDescriptor {
        &mut self.descriptor
    }

    /// The tween id.
    pub fn id(&self) -> &str {
        &self.descriptor.id
    }

    /// The pre-rendered frames.
    pub fn frames(&self) -> &[Vec<f64>] {
        &self.frames
    }

    /// The current frame index.
    pub fn frame(&self) -> usize {
        self.frame
    }

    /// The current frame's values without advancing.
    pub fn current_value(&self) -> Option<&[f64]> {
        self.frames.get(self.frame).map(Vec::as_slice)
    }

    /// Move the play head, clamped into the recorded frames.
    pub fn set_frame(&mut self, frame: usize) {
        if self.frames.is_empty() {
            self.frame = 0;
            return;
        }
        self.frame = frame.min(self.frames.len() - 1);
    }

    /// The frame rate the frames were rendered at.
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// The tempo the frames were rendered at.
    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    /// Advance one frame (`advanceFrame`, `tweens.js:248-254`).
    ///
    /// A looping descriptor wraps to 0 at the end of its frames; a one-shot
    /// holds its final frame instead of replaying the sweep forever.
    pub fn advance(&mut self) {
        if self.frames.is_empty() {
            self.frame = 0;
        } else if self.frame + 1 < self.frames.len() {
            self.frame += 1;
        } else if self.descriptor.looping {
            self.frame = 0;
        }
    }

    /// Read the current frame and advance (`dataTypes.tween.get`,
    /// `dataTypes.js:122-127`).
    ///
    /// The advance happens even when there is no frame to read, matching the
    /// JS getter. A looping tween wraps to frame 0 at the end of its recorded
    /// cycle; a one-shot holds its final frame instead of replaying it.
    pub fn value(&mut self) -> Option<Vec<f64>> {
        let value = self.frames.get(self.frame).cloned();
        self.advance();
        value
    }

    /// Re-render the frames for a new `fps`/`bpm`, keeping the position.
    ///
    /// The seek fraction is `frame / frames.len()` of the *old* frames, and
    /// the new frame is `floor(newLen * seek)` (`buildFrames`' `seek`,
    /// `tweens.js:56-60,129`).
    pub fn rebuild(&mut self, fps: f64, bpm: f64) {
        let seek = if self.frames.is_empty() {
            0.0
        } else {
            self.frame as f64 / self.frames.len() as f64
        };
        let (frames, frame) = build_frames_with_seek(&self.descriptor, fps, bpm, seek);

        self.frames = frames;
        self.frame = frame;
        self.fps = fps;
        self.bpm = bpm;
    }

    /// Follow a tempo change the way `updateBpm` does
    /// (`tweens.js:185-212`).
    ///
    /// Only `useBpm` tweens are rebuilt. Two legacy quirks are reproduced so a
    /// preset saved by the engine matches the worker:
    ///
    /// - the stored `duration` becomes `calculatedBpm` (or
    ///   `calculatedBpm × data.length`), which is *not* what the frames use;
    ///   a BPM tween's frames are computed from the tempo alone
    ///   ([`TweenDescriptor::effective_duration`]), so the field only becomes
    ///   observable if `useBpm` is later cleared;
    /// - `buildFrames` then divides that duration by `data.length` again, so
    ///   a hand-written descriptor whose `duration` a caller set to the
    ///   "total" time still plays as a two-row tween.
    pub fn update_bpm(&mut self, bpm: f64) {
        if !self.descriptor.use_bpm {
            return;
        }

        let calculated_bpm = ((60.0 * 60.0) / bpm) * self.descriptor.bpm_division;
        self.descriptor.duration = if self.descriptor.duration_as_total_time {
            calculated_bpm
        } else {
            calculated_bpm * self.descriptor.data.len() as f64
        };

        self.rebuild(self.fps, bpm);
    }
}

// ————————————————— set —————————————————

/// A set of live tweens keyed by id, in document order.
///
/// Replaces `state.tweens.tweens` (id-keyed map) plus the module-level
/// `frames`/`progress` maps of the JS store. Iteration order is the order the
/// descriptors appeared in the preset, so [`TweenSet::to_json`] writes back the
/// same array.
///
/// # Advancing
///
/// In the worker a tween advances **when it is read**: `dataTypes.tween.get`
/// calls `advanceFrame` on every read, and one link read is one advance
/// (`dataTypes.js:122-127`). Keep that behaviour by resolving tween links with
/// [`resolve_tween_link`]; use [`TweenSet::advance_all`] only for tweens no
/// link read during the frame (otherwise they advance twice). At the end of a
/// recorded cycle a looping tween wraps to frame 0, while a one-shot holds its
/// final frame.
#[derive(Clone, Debug, Default)]
pub struct TweenSet {
    tweens: IndexMap<String, TweenRuntime>,
}

/// Errors decoding a tween document.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TweenError {
    /// `tweens` was neither an array, an id-keyed map, nor a
    /// `{ tweens: ... }` wrapper.
    #[error("tween document is not an array or an id-keyed object")]
    InvalidShape,
    /// A descriptor did not decode. [`TweenSet::load_from_json`] and
    /// [`TweenSet::replace_from_json`] skip such an entry (and report it)
    /// instead of failing the document, so this only comes back from the
    /// single-descriptor [`TweenSet::upsert_json`].
    #[error("failed to decode tween `{id}`: {message}")]
    Decode {
        /// The descriptor id (or its array index / map key).
        id: String,
        /// The serde error message.
        message: String,
    },
}

impl TweenSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of tweens.
    pub fn len(&self) -> usize {
        self.tweens.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.tweens.is_empty()
    }

    /// Ids in document order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.tweens.keys().map(String::as_str)
    }

    /// Iterate over `(id, runtime)` in document order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &TweenRuntime)> {
        self.tweens.iter().map(|(id, tween)| (id.as_str(), tween))
    }

    /// Iterate mutably over `(id, runtime)` in document order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut TweenRuntime)> {
        self.tweens
            .iter_mut()
            .map(|(id, tween)| (id.as_str(), tween))
    }

    /// The tween with `id`.
    pub fn get(&self, id: &str) -> Option<&TweenRuntime> {
        self.tweens.get(id)
    }

    /// The tween with `id`, mutably.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut TweenRuntime> {
        self.tweens.get_mut(id)
    }

    /// Whether `id` is present.
    pub fn contains(&self, id: &str) -> bool {
        self.tweens.contains_key(id)
    }

    /// Insert or replace a runtime (`ADD_TWEEN`). The key is the tween id.
    pub fn upsert(&mut self, tween: TweenRuntime) {
        let id = tween.id().to_string();
        self.tweens.insert(id, tween);
    }

    /// Insert a descriptor, rendering its frames (`ADD_TWEEN`).
    pub fn insert_descriptor(&mut self, descriptor: TweenDescriptor, fps: f64, bpm: f64) {
        self.upsert(TweenRuntime::new(descriptor, fps, bpm));
    }

    /// Decode and insert a single descriptor (a `tweens/ADD_TWEEN` payload).
    pub fn upsert_json(
        &mut self,
        value: &Value,
        fps: f64,
        bpm: f64,
    ) -> Result<&mut TweenRuntime, TweenError> {
        let descriptor = decode_descriptor(value, value.get("id").and_then(Value::as_str))
            .map_err(|(id, message)| TweenError::Decode { id, message })?;
        self.insert_descriptor(descriptor, fps, bpm);
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.tweens.get_mut(&id).ok_or(TweenError::InvalidShape)
    }

    /// Remove a tween (`REMOVE_TWEEN`). Returns whether it existed.
    pub fn remove(&mut self, id: &str) -> bool {
        self.tweens.shift_remove(id).is_some()
    }

    /// Advance every tween once.
    ///
    /// This is *not* what the worker does on its own — see the type docs and
    /// only call it for tweens no Input Link read this frame.
    pub fn advance_all(&mut self) {
        for tween in self.tweens.values_mut() {
            tween.advance();
        }
    }

    /// Every tween's current frame values, without advancing.
    pub fn values_snapshot(&self) -> IndexMap<String, Vec<f64>> {
        self.tweens
            .iter()
            .filter_map(|(id, tween)| {
                tween
                    .current_value()
                    .map(|value| (id.clone(), value.to_vec()))
            })
            .collect()
    }

    /// Replace the set with the tweens in `value`, rendering frames at
    /// `fps`/`bpm`.
    ///
    /// Accepts the shapes `state.tweens` can have:
    ///
    /// - the preset array of descriptors (`[{ id, data, ... }]`);
    /// - the live worker map (`{ "<id>": { id, data, ... } }`);
    /// - the live worker root (`{ tweens: { "<id>": ... } }`).
    ///
    /// A `null` document clears the set.
    ///
    /// Decoding is **per entry**: a descriptor that does not decode is skipped
    /// and reported, and the rest are kept. The JS created tweens one at a time
    /// and lets the survivors live, and unknown easings already fall back to
    /// `linear` rather than failing the load, so one corrupt Scene/preset entry
    /// must not empty the whole runtime (R-9). A document that is not a tween
    /// container at all is still [`TweenError::InvalidShape`].
    pub fn load_from_json(value: &Value, fps: f64, bpm: f64) -> Result<Self, TweenError> {
        let mut set = Self::new();
        set.replace_from_json(value, fps, bpm)?;
        Ok(set)
    }

    /// [`TweenSet::load_from_json`] into an existing set.
    ///
    /// The previous set survives a malformed document (nothing is cleared
    /// until the document shape is known); an undecodable entry is skipped and
    /// reported (see [`TweenSet::load_from_json`]).
    pub fn replace_from_json(
        &mut self,
        value: &Value,
        fps: f64,
        bpm: f64,
    ) -> Result<(), TweenError> {
        let entries = tween_entries(value)?;
        self.tweens.clear();

        for (key, entry) in entries {
            match decode_descriptor(&entry, key.as_deref()) {
                Ok(descriptor) => self.insert_descriptor(descriptor, fps, bpm),
                Err((id, message)) => {
                    eprintln!("engine: WARN skipping an undecodable tween `{id}`: {message}")
                }
            }
        }

        Ok(())
    }

    /// Serialize the set back to the preset array of descriptors, in document
    /// order.
    ///
    /// Each descriptor is serialised on its own. One that cannot be serialised
    /// is skipped and reported instead of collapsing the array (the old
    /// `{}`-per-entry fallback lost every BPM tween from `state.tweens` when a
    /// single `duration` became `Infinity`, R-5). A non-finite *numeric* field
    /// does not reach that path: [`TweenDescriptor::to_json`] omits it and the
    /// descriptor keeps its identity, data and settings.
    pub fn to_json(&self) -> Value {
        let mut entries = Vec::with_capacity(self.tweens.len());

        for tween in self.tweens.values() {
            match serde_json::to_value(tween.descriptor()) {
                Ok(entry) => entries.push(entry),
                Err(error) => eprintln!(
                    "engine: WARN skipping tween `{}` while serialising: {error}",
                    tween.id()
                ),
            }
        }

        Value::Array(entries)
    }
}

/// Split a tween document into `(key, descriptor)` pairs.
///
/// The key is `None` for array entries (a preset descriptor must carry its
/// own `id`) and `Some(key)` for the live worker map, which is keyed by id.
fn tween_entries(value: &Value) -> Result<Vec<(Option<String>, Value)>, TweenError> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(entries) => Ok(entries.iter().map(|entry| (None, entry.clone())).collect()),
        Value::Object(map) => {
            // The live worker root is `{ tweens: { ... } }`.
            if let Some(inner) = map.get("tweens") {
                return tween_entries(inner);
            }

            Ok(map
                .iter()
                .map(|(key, entry)| (Some(key.clone()), entry.clone()))
                .collect())
        }
        _ => Err(TweenError::InvalidShape),
    }
}

/// Decode one descriptor, falling back to `key` when the entry has no `id`
/// (the live worker map is keyed by id).
fn decode_descriptor(
    value: &Value,
    key: Option<&str>,
) -> Result<TweenDescriptor, (String, String)> {
    if !value.is_object() {
        return Err((
            key.unwrap_or_default().to_string(),
            "tween descriptor is not an object".to_string(),
        ));
    }

    let mut value = value.clone();
    if value.get("id").and_then(Value::as_str).is_none()
        && let (Some(key), Some(object)) = (key, value.as_object_mut())
    {
        object.insert("id".to_string(), Value::String(key.to_string()));
    }

    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    serde_json::from_value(value).map_err(|error| (id, error.to_string()))
}

// ————————————————— app-facing helpers —————————————————

/// Resolve one Input Link read of a tween, mirroring
/// `dataTypes.tween.get` + `value[channel]`.
///
/// The read advances the tween exactly once (even when the channel is
/// missing, like the JS getter), which is what makes a BPM tween breathe at
/// the link evaluation rate. Looping tweens wrap at the end of their recorded
/// cycle; one-shots hold their last value.
pub fn resolve_tween_link(tweens: &mut TweenSet, id: &str, channel: usize) -> Option<f64> {
    let frame = tweens.get_mut(id)?.value()?;
    frame.get(channel).copied()
}

/// Split a tween link `location` into `(tween id, channel)`.
///
/// Handles the shape the Scene link builder writes,
/// `tweens.tweens['<input id>'].value[0]`, plus the shorter
/// `tweens['<id>'].value[0]` and unquoted `tweens.tweens[<id>].value[0]`.
/// Anything else is `None` (the link is not a tween link).
pub fn parse_tween_link_location(location: &str) -> Option<(String, usize)> {
    let rest = location
        .strip_prefix("tweens.tweens[")
        .or_else(|| location.strip_prefix("tweens["))?;

    let (id, rest) = match rest.strip_prefix('\'') {
        Some(quoted) => {
            let end = quoted.find('\'')?;
            let id = quoted[..end].to_string();
            let tail = quoted[end + 1..].strip_prefix(']')?;
            (id, tail)
        }
        None => {
            let end = rest.find(']')?;
            (rest[..end].to_string(), &rest[end + 1..])
        }
    };

    let channel = rest.strip_prefix(".value[")?.strip_suffix(']')?;

    Some((id, channel.parse::<usize>().ok()?))
}

#[cfg(test)]
mod tests {
    //! Reference tests against anime.js 3.2.2 itself
    //! (`crates/modv-engine/data/tween-reference.json`, generated by running
    //! `buildFrames` from `store/modules/tweens.js` in Node 24 — see the
    //! module docs) plus the runtime/link semantics from `dataTypes.js`.
    use super::*;
    use crate::scene_link_defaults::default_link_tween;
    use serde_json::json;

    /// The anime.js reference: frame dumps for five tween configs and the
    /// whole `penner` easing catalogue.
    const REFERENCE: &str = include_str!("../data/tween-reference.json");

    /// The anime.js multi-keyframe reference: frame dumps for 3..5-row tweens
    /// captured from the legacy worker's `buildFrames`.
    const KEYFRAME_REFERENCE: &str = include_str!("../data/tween-keyframe-reference.json");

    fn reference() -> Value {
        serde_json::from_str(REFERENCE).expect("tween reference fixture parses")
    }

    fn keyframe_reference() -> Value {
        serde_json::from_str(KEYFRAME_REFERENCE).expect("keyframe reference fixture parses")
    }

    fn descriptor_from(config: &Value) -> TweenDescriptor {
        serde_json::from_value(config.clone()).expect("reference config decodes")
    }

    /// A plain, non-BPM tween at 10 fps (5 frames over 500 ms per row).
    fn linear_tween() -> TweenDescriptor {
        let mut tween = TweenDescriptor::new("t1", vec![vec![0.0], vec![1.0]]);
        tween.duration = 1000.0;
        tween.direction = "normal".to_string();
        tween.easing = "linear".to_string();
        tween.looping = false;
        tween.use_bpm = false;
        tween
    }

    // ————————————————— anime.js reference —————————————————

    #[test]
    fn reference_frames_match_anime() {
        let reference = reference();

        for entry in reference["configs"].as_array().expect("configs array") {
            let config = &entry["config"];
            let id = config["id"].as_str().expect("config id");
            let fps = config["fps"].as_f64().expect("fps");
            let bpm = config["bpm"].as_f64().expect("bpm");

            let frames = build_tween_frames(&descriptor_from(config), fps, bpm);
            let expected = entry["frames"].as_array().expect("frame array");

            assert_eq!(frames.len(), expected.len(), "{id}: frame count");

            for (index, expected_frame) in expected.iter().enumerate() {
                let expected_channels = expected_frame.as_object().expect("frame object");
                assert_eq!(
                    frames[index].len(),
                    expected_channels.len(),
                    "{id}: channel count at frame {index}"
                );

                for (channel, value) in frames[index].iter().enumerate() {
                    let expected_value = expected_channels[&channel.to_string()]
                        .as_f64()
                        .expect("numeric channel");
                    assert!(
                        (value - expected_value).abs() < 1e-9,
                        "{id}: frame {index} channel {channel}: {value} != {expected_value}"
                    );
                }
            }
        }
    }

    #[test]
    fn multi_keyframe_frames_match_anime() {
        // anime normalises a property array of 3+ values into one tween per
        // keyframe. The reference was captured from the legacy worker's
        // `buildFrames` against animejs 3.2.2 (same `Object.assign` snapshot);
        // see `crates/modv-engine/data/tween-keyframe-reference.json`.
        let reference = keyframe_reference();
        assert_eq!(reference["animeVersion"], json!(ANIME_REFERENCE_VERSION));
        assert_eq!(
            reference["generator"],
            json!("anime.js 3.2.2 legacy worker buildFrames recorder")
        );

        let configs = reference["configs"].as_array().expect("configs array");
        assert!(configs.len() >= 6, "expected the 3..5-row config set");

        for entry in configs {
            let config = &entry["config"];
            let id = config["id"].as_str().expect("config id");
            let fps = config["fps"].as_f64().expect("fps");
            let bpm = config["bpm"].as_f64().expect("bpm");
            let descriptor = descriptor_from(config);

            let frames = build_tween_frames(&descriptor, fps, bpm);
            let expected = entry["frames"].as_array().expect("frame array");

            assert_eq!(frames.len(), expected.len(), "{id}: frame count");

            for (index, expected_frame) in expected.iter().enumerate() {
                let expected_channels = expected_frame.as_object().expect("frame object");
                assert_eq!(
                    frames[index].len(),
                    expected_channels.len(),
                    "{id}: channel count at frame {index}"
                );

                for (channel, value) in frames[index].iter().enumerate() {
                    let expected_value = expected_channels[&channel.to_string()]
                        .as_f64()
                        .expect("numeric channel");
                    assert!(
                        (value - expected_value).abs() < 1e-9,
                        "{id}: frame {index} channel {channel}: {value} != {expected_value}"
                    );
                }
            }
        }
    }

    #[test]
    fn multi_keyframes_hold_the_first_row_then_ease_each_segment() {
        // What anime 3.2.2 measured for three rows: `newDuration` (1000 ms)
        // divided by three keyframes gives three 333.33 ms tweens. Tween 0
        // interpolates row 0 -> keyframe 0, i.e. row 0 -> row 0, so the first
        // third is a hold; the easing then applies inside each remaining
        // segment rather than across the whole run.
        let reference = keyframe_reference();
        let entry = reference["configs"]
            .as_array()
            .expect("configs")
            .iter()
            .find(|entry| entry["config"]["id"] == json!("three-row-linear-10"))
            .expect("three-row-linear-10 config");
        let descriptor = descriptor_from(&entry["config"]);
        let frames = build_tween_frames(&descriptor, 10.0, 120.0);
        let values: Vec<f64> = frames.iter().map(|frame| frame[0]).collect();

        // 10 fps over the 1000 ms anime duration: t = 0,100,...,900 ms.
        assert_eq!(values.len(), 10);
        assert_eq!(&values[..4], &[0.0, 0.0, 0.0, 0.0]);
        assert!((values[4] - 0.1).abs() < 1e-12);
        assert!((values[5] - 0.25).abs() < 1e-12);
        assert!((values[9] - 0.85).abs() < 1e-12);
    }

    #[test]
    fn multi_keyframe_loop_ping_pongs_the_recorded_leg() {
        let reference = keyframe_reference();
        let entry = reference["configs"]
            .as_array()
            .expect("configs")
            .iter()
            .find(|entry| entry["config"]["id"] == json!("three-row-linear-loop-10"))
            .expect("three-row-linear-loop-10 config");
        let descriptor = descriptor_from(&entry["config"]);
        let frames = build_tween_frames(&descriptor, 10.0, 120.0);
        let values: Vec<f64> = frames.iter().map(|frame| frame[0]).collect();

        // 10 recorded frames per leg, the top value once, then back down.
        assert_eq!(values.len(), 20);
        assert_eq!(values[10], 1.0);
        assert_eq!(values[19], 0.0);
        assert_eq!(values[11], values[9]);
        assert_eq!(values[18], values[2]);
    }

    #[test]
    fn keyframe_times_round_trip_and_preserve_unknown_keys() {
        let document = json!({
            "id": "free",
            "data": [[0], [1], [0]],
            "keyframeTimes": [0, 0.25, 1],
            "mysteryKey": {"deep": [true, null]},
        });

        let descriptor: TweenDescriptor =
            serde_json::from_value(document.clone()).expect("descriptor");
        assert_eq!(
            descriptor.keyframe_times,
            Some(vec![0.0, 0.25, 1.0]),
            "valid times are typed"
        );
        // The typed field must not duplicate or drop the raw key.
        assert_eq!(descriptor.to_json(), document);

        let reparsed: TweenDescriptor =
            serde_json::from_value(descriptor.to_json()).expect("reparse");
        assert_eq!(reparsed, descriptor);
        assert_eq!(reparsed.to_json(), document);

        // Preset/Scene documents carry `state.tweens` as the descriptor array.
        let set = TweenSet::load_from_json(&json!([document.clone()]), 60.0, 120.0).expect("load");
        assert_eq!(set.to_json(), json!([document]));

        // Absent means absent: no `keyframeTimes` key is invented.
        let plain = json!({"id": "plain", "data": [[0], [1]]});
        let descriptor: TweenDescriptor = serde_json::from_value(plain.clone()).expect("plain");
        assert_eq!(descriptor.keyframe_times, None);
        assert_eq!(descriptor.to_json(), plain);
    }

    #[test]
    fn invalid_keyframe_times_fall_back_to_even_spacing_and_round_trip_verbatim() {
        let rows = json!([[0], [0.25], [0.6], [1]]);
        let invalid = [
            json!([0, 0.5, 1]),         // wrong length
            json!([0, 0.5, 0.4, 1]),    // not strictly increasing
            json!([0, 0.5, 0.5, 1]),    // duplicate position
            json!([0.1, 0.3, 0.6, 1]),  // first is not 0
            json!([0, 0.3, 0.6, 0.9]),  // last is not 1
            json!(["-0", 0.3, 0.6, 1]), // not numeric
            json!({"not": "times"}),    // wrong shape
            Value::Null,                // null
        ];

        let plain = descriptor_from(&json!({
            "id": "even",
            "data": rows.clone(),
            "useBpm": false,
            "duration": 400,
            "direction": "normal",
            "easing": "linear",
            "loop": false,
        }));
        let expected = build_tween_frames(&plain, 10.0, 120.0);

        for times in invalid {
            let document = json!({
                "id": "bad",
                "data": rows.clone(),
                "keyframeTimes": times.clone(),
            });
            let descriptor: TweenDescriptor =
                serde_json::from_value(document.clone()).expect("descriptor decodes");

            assert_eq!(
                descriptor.keyframe_times, None,
                "invalid times are ignored: {times}"
            );
            // The raw key is preserved verbatim for a load -> save round trip.
            assert_eq!(descriptor.to_json(), document, "raw key for {times}");

            let mut descriptor = descriptor;
            descriptor.use_bpm = false;
            descriptor.duration = 400.0;
            descriptor.direction = "normal".to_string();
            descriptor.easing = "linear".to_string();
            descriptor.looping = false;
            assert_eq!(
                build_tween_frames(&descriptor, 10.0, 120.0),
                expected,
                "invalid times sample like even spacing: {times}"
            );
        }

        // A programmatically assigned non-finite time is just as invalid; it
        // must not panic or be used in sampling.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut descriptor = plain.clone();
            descriptor.keyframe_times = Some(vec![0.0, bad, 0.6, 1.0]);
            assert_eq!(build_tween_frames(&descriptor, 10.0, 120.0), expected);
        }
    }

    #[test]
    fn keyframe_times_sample_free_positions() {
        let mut tween =
            TweenDescriptor::new("free", vec![vec![0.0], vec![1.0], vec![0.0], vec![2.0]]);
        tween.duration = 1000.0;
        tween.duration_as_total_time = true;
        tween.direction = "normal".to_string();
        tween.easing = "linear".to_string();
        tween.looping = false;
        tween.use_bpm = false;
        tween.keyframe_times = Some(vec![0.0, 0.2, 0.6, 1.0]);

        // 1000 ms at 10 fps: progress = 0, 0.1, ... 0.9.
        let frames = build_tween_frames(&tween, 10.0, 120.0);
        let values: Vec<f64> = frames.iter().map(|frame| frame[0]).collect();
        let expected = [0.0, 0.5, 1.0, 0.75, 0.5, 0.25, 0.0, 0.5, 1.0, 1.5];

        assert_eq!(values.len(), expected.len());
        for (index, (value, expected)) in values.iter().zip(expected).enumerate() {
            assert!(
                (value - expected).abs() < 1e-12,
                "frame {index}: {value} != {expected}"
            );
        }
    }

    #[test]
    fn two_row_keyframe_times_match_the_even_spacing_result() {
        let mut tween = linear_tween();
        let even = build_tween_frames(&tween, 10.0, 120.0);

        tween.keyframe_times = Some(vec![0.0, 1.0]);
        assert_eq!(build_tween_frames(&tween, 10.0, 120.0), even);

        // `steps` is applied identically on the free-time path.
        tween.steps = 4.0;
        let mut even_steps = tween.clone();
        even_steps.keyframe_times = None;
        assert_eq!(
            build_tween_frames(&tween, 10.0, 120.0),
            build_tween_frames(&even_steps, 10.0, 120.0)
        );
    }

    #[test]
    fn every_penner_easing_matches_anime() {
        let reference = reference();
        let times: Vec<f64> = reference["easingTimes"]
            .as_array()
            .expect("easing times")
            .iter()
            .map(|time| time.as_f64().expect("numeric time"))
            .collect();
        let easings = reference["easings"].as_object().expect("easings map");

        for (name, values) in easings {
            let values = values.as_array().expect("easing samples");
            assert_eq!(values.len(), times.len(), "{name}: sample count");

            for (time, expected) in times.iter().zip(values) {
                let expected = expected.as_f64().expect("numeric sample");
                let actual = ease(name, *time);
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "{name}({time}): {actual} != {expected}"
                );
            }
        }
    }

    #[test]
    fn penner_easing_names_match_the_catalogue() {
        let reference = reference();
        let fixture: Vec<&str> = reference["easings"]
            .as_object()
            .expect("easings map")
            .keys()
            .map(String::as_str)
            .collect();

        assert_eq!(PENNER_EASING_NAMES.len(), 41);
        let mut ours = PENNER_EASING_NAMES.to_vec();
        let mut theirs = fixture.clone();
        ours.sort_unstable();
        theirs.sort_unstable();
        assert_eq!(ours, theirs);
        // `linear` is first in anime's `Object.keys` order.
        assert_eq!(PENNER_EASING_NAMES[0], "linear");
        assert_eq!(ANIME_REFERENCE_VERSION, "3.2.2");
    }

    #[test]
    fn unknown_easings_fall_back_to_linear() {
        assert_eq!(ease("notAnEasing", 0.25), 0.25);
        assert_eq!(ease("", 0.75), 0.75);
        // `spring` is a separate anime case the penner catalogue does not own.
        assert_eq!(ease("spring(1, 100, 10, 0)", 0.4), 0.4);
    }

    #[test]
    fn steps_easing_quantises_progress() {
        // anime's steps(): ceil(clamp(t, 1e-6, 1) * n) / n.
        assert_eq!(ease("steps(4)", 0.0), 0.25);
        assert_eq!(ease("steps(4)", 0.25), 0.25);
        assert_eq!(ease("steps(4)", 0.3), 0.5);
        assert_eq!(ease("steps(4)", 1.0), 1.0);
        // No argument defaults to 10 steps.
        assert!((ease("steps()", 0.0) - 0.1).abs() < 1e-12);

        // The `steps` descriptor field wins over the easing name, like
        // `easing: steps ? \`steps(${steps})\` : easing` in buildFrames.
        let mut tween = linear_tween();
        tween.easing = "easeInOutSine".to_string();
        tween.steps = 4.0;
        let frames = build_tween_frames(&tween, 10.0, 120.0);
        assert!((frames[0][0] - 0.25).abs() < 1e-12);
        assert!((frames[2][0] - 0.5).abs() < 1e-12);
    }

    // ————————————————— frame recording —————————————————

    #[test]
    fn loop_records_a_ping_pong_and_no_loop_records_one_leg() {
        // duration 1000 / 2 rows = 500 ms; at 10 fps that is 5 frames a leg.
        let mut tween = linear_tween();
        let frames = build_tween_frames(&tween, 10.0, 120.0);
        assert_eq!(frames.len(), 5);
        let values: Vec<f64> = frames.iter().map(|frame| frame[0]).collect();
        assert_eq!(values, vec![0.0, 0.2, 0.4, 0.6, 0.8]);

        tween.looping = true;
        let frames = build_tween_frames(&tween, 10.0, 120.0);
        assert_eq!(frames.len(), 10);
        let values: Vec<f64> = frames.iter().map(|frame| frame[0]).collect();
        // 0..=framesRequired then framesRequired - 1 ..= 1: frame 0 and the
        // top frame appear once.
        assert_eq!(
            values,
            vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 0.8, 0.6, 0.4, 0.2]
        );
        assert_eq!(frames[10 - 1][0], 0.2);
    }

    #[test]
    fn a_restart_loop_repeats_the_drawn_pattern_instead_of_ping_ponging() {
        // A painted draw curve is a looping, free-keyframed descriptor. Read
        // through `loopMode: "restart"`, each cycle must replay the same
        // forward leg instead of oscillating back through the previous one.
        let document = json!({
            "id": "draw-link",
            "data": [[0.0], [1.0], [1.0], [0.0], [0.0]],
            "keyframeTimes": [0.0, 0.25, 0.5, 0.75, 1.0],
            "duration": 4000.0,
            "direction": "normal",
            "easing": "linear",
            "loop": true,
            "loopMode": "restart",
            "useBpm": false,
            "bpmDivision": 16.0,
            "durationAsTotalTime": true,
            "steps": 0.0,
        });
        let descriptor: TweenDescriptor =
            serde_json::from_value(document.clone()).expect("descriptor decodes");

        // 4000 ms at 10 fps is a 40-frame leg. Today the recorder writes an
        // 80-frame ping-pong, so the second batch of 40 reads mirrors the
        // first batch instead of repeating it.
        let mut runtime = TweenRuntime::new(descriptor.clone(), 10.0, 120.0);
        let first_cycle: Vec<Vec<f64>> = (0..40)
            .map(|_| runtime.value().expect("first-cycle value"))
            .collect();
        let second_cycle: Vec<Vec<f64>> = (0..40)
            .map(|_| runtime.value().expect("second-cycle value"))
            .collect();
        assert_eq!(
            first_cycle, second_cycle,
            "each loop must replay the same drawn pattern"
        );

        assert_eq!(
            build_tween_frames(&descriptor, 10.0, 120.0).len(),
            40,
            "a restart loop records one leg, not a mirrored leg"
        );

        // The key is opt-in: legacy descriptors keep the ping-pong recorder.
        let mut legacy_document = document.clone();
        legacy_document
            .as_object_mut()
            .expect("object")
            .remove("loopMode");
        let legacy: TweenDescriptor =
            serde_json::from_value(legacy_document).expect("legacy descriptor decodes");
        assert_eq!(
            build_tween_frames(&legacy, 10.0, 120.0).len(),
            80,
            "without loopMode the recorded ping-pong is unchanged"
        );
    }

    #[test]
    fn frames_required_rounds_like_javascript() {
        // 1000 ms / 2 rows = 500 ms; 500 / (1000/30) = 14.999...
        let tween = linear_tween();
        let frames = build_tween_frames(&tween, 30.0, 120.0);
        assert_eq!(frames.len(), 15);

        // 500 / (1000/20) = 10.000000000000002 → 10.
        let frames = build_tween_frames(&tween, 20.0, 120.0);
        assert_eq!(frames.len(), 10);

        // Degenerate fps values collapse to a single frame (`|| 1`).
        assert_eq!(build_tween_frames(&tween, 0.0, 120.0).len(), 1);
        assert_eq!(build_tween_frames(&tween, f64::NAN, 120.0).len(), 1);
    }

    #[test]
    fn two_channel_data_interpolates_each_channel() {
        let mut tween = TweenDescriptor::new("two", vec![vec![0.0, 10.0], vec![1.0, 20.0]]);
        tween.duration = 500.0;
        tween.direction = "normal".to_string();
        tween.easing = "linear".to_string();
        tween.looping = false;
        tween.use_bpm = false;

        let frames = build_tween_frames(&tween, 10.0, 120.0);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0], vec![0.0, 10.0]);
        assert!((frames[1][0] - 1.0 / 3.0).abs() < 1e-12);
        assert!((frames[1][1] - (10.0 + 10.0 / 3.0)).abs() < 1e-12);
        assert!((frames[2][0] - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn empty_data_is_a_single_empty_frame() {
        let mut tween = TweenDescriptor::new("empty", Vec::new());
        tween.use_bpm = false;
        let frames = build_tween_frames(&tween, 60.0, 120.0);
        assert_eq!(frames, vec![Vec::<f64>::new()]);
    }

    #[test]
    fn zero_duration_samples_the_end_value() {
        let mut tween = linear_tween();
        tween.duration = 0.0;
        let frames = build_tween_frames(&tween, 60.0, 120.0);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], vec![1.0]);
    }

    #[test]
    fn bpm_zero_or_nan_falls_back_to_120() {
        let tween = descriptor_from(&default_link_tween("in-1", None));
        assert_eq!(
            tween.effective_duration(0.0),
            tween.effective_duration(120.0)
        );
        assert_eq!(
            tween.effective_duration(f64::NAN),
            tween.effective_duration(120.0)
        );
        assert_eq!(
            build_tween_frames(&tween, 60.0, 0.0),
            build_tween_frames(&tween, 60.0, 120.0)
        );
    }

    // ————————————————— runtime —————————————————

    #[test]
    fn value_reads_then_advances_and_wraps() {
        let mut tween = linear_tween();
        tween.looping = true;
        let mut runtime = TweenRuntime::new(tween, 10.0, 120.0);
        assert_eq!(runtime.frames().len(), 10);
        assert_eq!(runtime.frame(), 0);

        let read: Vec<f64> = (0..10)
            .map(|_| runtime.value().expect("frame")[0])
            .collect();
        assert_eq!(read, vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 0.8, 0.6, 0.4, 0.2]);
        // A looping runtime wraps to 0 after its recorded ping-pong cycle.
        assert_eq!(runtime.frame(), 0);
        assert_eq!(runtime.value().expect("frame")[0], 0.0);

        runtime.set_frame(100);
        assert_eq!(runtime.frame(), 9);
        runtime.advance();
        assert_eq!(runtime.frame(), 0);
    }

    #[test]
    fn one_shot_holds_the_last_frame_after_reading_past_the_end() {
        let mut runtime = TweenRuntime::new(linear_tween(), 10.0, 120.0);
        assert_eq!(runtime.frames().len(), 5);

        let read: Vec<f64> = (0..5).map(|_| runtime.value().expect("frame")[0]).collect();
        assert_eq!(read, vec![0.0, 0.2, 0.4, 0.6, 0.8]);
        let last_frame = runtime.frames().last().cloned().expect("final frame");

        assert_eq!(
            runtime.frame(),
            4,
            "a non-looping tween must not wrap to frame 0"
        );
        assert_eq!(runtime.current_value(), Some(last_frame.as_slice()));

        // Every subsequent read is past the end of a one-shot, but the final
        // value remains available instead of replaying the sweep forever.
        for _ in 0..3 {
            assert_eq!(runtime.value(), Some(last_frame.clone()));
            assert_eq!(runtime.frame(), 4);
            assert_eq!(runtime.current_value(), Some(last_frame.as_slice()));
        }

        runtime.set_frame(100);
        runtime.advance();
        assert_eq!(runtime.frame(), 4);
        assert_eq!(runtime.current_value(), Some(last_frame.as_slice()));
    }

    #[test]
    fn set_frame_keeps_the_current_value() {
        let mut runtime = TweenRuntime::new(linear_tween(), 10.0, 120.0);
        runtime.set_frame(3);
        assert_eq!(runtime.frame(), 3);
        assert_eq!(runtime.current_value(), Some(&[0.6][..]));
    }

    #[test]
    fn update_bpm_recomputes_duration_and_frames() {
        let mut tween = TweenDescriptor::new("bpm", vec![vec![0.0], vec![1.0]]);
        tween.bpm_division = 32.0;
        let mut runtime = TweenRuntime::new(tween, 60.0, 120.0);

        // `createTween` leaves duration at 1000, but a BPM tween ignores it:
        // ((3600 / 120) * 32) / 2 rows = 480 ms → round(28.8) = 29 a leg.
        assert_eq!(runtime.descriptor().duration, 1000.0);
        assert_eq!(runtime.frames().len(), 58);

        // `updateBpm` stores `calculatedBpm * data.length` (960 * 2) and
        // rebuilds; at the same tempo the frames are unchanged because they
        // follow the tempo, not the stored duration.
        runtime.update_bpm(120.0);
        assert_eq!(runtime.descriptor().duration, 1920.0);
        assert_eq!(runtime.frames().len(), 58);
        assert_eq!(runtime.bpm(), 120.0);

        // 60 BPM doubles the leg: ((3600 / 60) * 32) / 2 = 960 ms → 58 a leg.
        runtime.update_bpm(60.0);
        assert_eq!(runtime.descriptor().duration, 3840.0);
        assert_eq!(runtime.frames().len(), 116);
        assert_eq!(runtime.bpm(), 60.0);
    }

    #[test]
    fn update_bpm_leaves_wall_clock_tweens_alone() {
        let mut runtime = TweenRuntime::new(linear_tween(), 10.0, 120.0);
        let frames = runtime.frames().to_vec();
        runtime.update_bpm(60.0);
        assert_eq!(runtime.descriptor().duration, 1000.0);
        assert_eq!(runtime.frames(), frames.as_slice());
        assert_eq!(runtime.bpm(), 120.0);
    }

    #[test]
    fn rebuild_preserves_the_position_fraction() {
        let mut tween = linear_tween();
        tween.looping = true;
        let mut runtime = TweenRuntime::new(tween, 10.0, 120.0);
        assert_eq!(runtime.frames().len(), 10);

        runtime.set_frame(2);
        let before = runtime.frame();
        runtime.rebuild(20.0, 120.0);

        // floor(20 frames * (2 / 10)) = 4.
        assert_eq!(runtime.frames().len(), 20);
        assert_eq!(runtime.frame(), 4);
        assert_eq!(before, 2);
        assert_eq!(runtime.fps(), 20.0);
    }

    #[test]
    fn rebuild_preserves_the_position_of_a_non_looping_tween() {
        // `buildFrames` restores `frames[id] = Math.floor(len * seek)` for the
        // loop *and* the no-loop branch (`tweens.js:129`), so a BPM/fps
        // rebuild must not snap a one-shot tween back to frame 0.
        let mut tween = linear_tween();
        tween.use_bpm = true;
        tween.bpm_division = 32.0;
        let mut runtime = TweenRuntime::new(tween, 60.0, 120.0);
        // ((3600 / 120) * 32) / 2 rows = 480 ms → round(28.8) = 29 frames.
        assert_eq!(runtime.frames().len(), 29);

        runtime.set_frame(14);

        // Same tempo: the position is unchanged.
        runtime.update_bpm(120.0);
        assert_eq!(runtime.frames().len(), 29);
        assert_eq!(runtime.frame(), 14);

        // Half the tempo doubles the leg: floor(58 frames * 14/29) = 28.
        runtime.update_bpm(60.0);
        assert_eq!(runtime.frames().len(), 58);
        assert_eq!(runtime.frame(), 28);

        // floor(58 * 28/58) = 28 again — the rebuild does not drift.
        runtime.update_bpm(60.0);
        assert_eq!(runtime.frame(), 28);
    }

    #[test]
    fn rebuild_keeps_a_wall_clock_non_looping_tween_on_its_position() {
        let mut runtime = TweenRuntime::new(linear_tween(), 10.0, 120.0);
        assert_eq!(runtime.frames().len(), 5);

        runtime.set_frame(3);
        runtime.rebuild(20.0, 120.0);

        // 500 ms leg at 20 fps → 10 frames; floor(10 * 3/5) = 6, not 0.
        assert_eq!(runtime.frames().len(), 10);
        assert_eq!(runtime.frame(), 6);
    }

    #[test]
    fn restored_frame_clamps_the_seek_fraction() {
        assert_eq!(restored_frame(29, 0.0), 0);
        assert_eq!(restored_frame(29, 0.5), 14);
        // A full or bogus fraction is clamped into the new frame vector rather
        // than indexing past it.
        assert_eq!(restored_frame(29, 1.0), 28);
        assert_eq!(restored_frame(29, -0.5), 0);
        assert_eq!(restored_frame(29, f64::NAN), 0);
        assert_eq!(restored_frame(29, f64::INFINITY), 0);
        assert_eq!(restored_frame(0, 0.5), 0);
    }

    // ————————————————— descriptors —————————————————

    #[test]
    fn descriptor_new_uses_the_create_tween_defaults() {
        let tween = TweenDescriptor::new("id-1", vec![vec![0.0], vec![1.0]]);
        assert_eq!(tween.duration, 1000.0);
        assert_eq!(tween.direction, "alternate");
        assert_eq!(tween.easing, "linear");
        assert!(tween.looping);
        assert!(tween.use_bpm);
        assert_eq!(tween.bpm_division, 16.0);
        assert!(!tween.duration_as_total_time);
        assert_eq!(tween.steps, 0.0);
        assert_eq!(tween.is_gallery, None);
        assert_eq!(tween.channel_count(), 1);
    }

    #[test]
    fn descriptor_round_trip_preserves_unknown_keys() {
        let mut config = default_link_tween("in-9", Some(&json!(8)));
        config["isGallery"] = json!(false);
        config["frames"] = json!([[0.0], [1.0]]);
        config["mysteryKey"] = json!({"deep": [true, null]});

        let descriptor: TweenDescriptor =
            serde_json::from_value(config.clone()).expect("descriptor");
        assert_eq!(descriptor.bpm_division, 8.0);
        assert_eq!(descriptor.is_gallery, Some(false));
        assert_eq!(descriptor.to_json(), config);

        // A second cycle is stable.
        let reparsed: TweenDescriptor =
            serde_json::from_value(descriptor.to_json()).expect("reparse");
        assert_eq!(reparsed, descriptor);
        assert_eq!(reparsed.to_json(), config);
    }

    #[test]
    fn descriptor_omits_keys_the_document_did_not_have() {
        let raw = json!({"id": "t1", "data": [[0], [1]]});
        let descriptor: TweenDescriptor = serde_json::from_value(raw.clone()).expect("descriptor");
        assert_eq!(descriptor.duration, 1000.0);
        assert!(descriptor.looping);
        assert_eq!(descriptor.to_json(), raw);

        // `isGallery` is never invented, only carried.
        let raw = json!({"id": "t2"});
        let descriptor: TweenDescriptor = serde_json::from_value(raw.clone()).expect("descriptor");
        assert_eq!(descriptor.to_json(), raw);
    }

    #[test]
    fn descriptor_requires_an_id() {
        assert!(serde_json::from_value::<TweenDescriptor>(json!({"data": [[0], [1]]})).is_err());
    }

    // ————————————————— links / the default Scene tween —————————————————

    #[test]
    fn default_link_tween_breathes_between_zero_and_one() {
        // The automatic Scene link tween (`scene-link-defaults.js:7-10`):
        // easeInOutSine, division 32, alternating, looped. At 128 BPM the leg
        // is `(3600 / 128) * 32 / 2 = 450 ms` → round(27) frames each way.
        let descriptor = descriptor_from(&default_link_tween("in-1", None));
        let frames = build_tween_frames(&descriptor, 60.0, 128.0);

        assert_eq!(frames.len(), 54);
        let values: Vec<f64> = frames.iter().map(|frame| frame[0]).collect();

        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(min, 0.0);
        assert_eq!(max, 1.0);

        // One full breath: up for 27 frames, back down for 27.
        assert_eq!(values[0], 0.0);
        assert_eq!(values[27], 1.0);
        assert!(values[1] > values[0] && values[26] < values[27]);
        assert!(values[28] < values[27] && values[53] > 0.0);
        // It never leaves the 0..1 range.
        assert!(values.iter().all(|value| (0.0..=1.0).contains(value)));
    }

    #[test]
    fn resolve_tween_link_advances_once_per_read() {
        let mut descriptor = linear_tween();
        // This checks the one-step advance and restart wrap, so use a looping
        // restart descriptor rather than the one-shot helper (which holds).
        descriptor.looping = true;
        descriptor.loop_mode = Some("restart".to_string());

        let mut set = TweenSet::new();
        set.insert_descriptor(descriptor, 10.0, 120.0);

        let values: Vec<f64> = (0..7)
            .map(|_| resolve_tween_link(&mut set, "t1", 0).expect("link value"))
            .collect();
        assert_eq!(values, vec![0.0, 0.2, 0.4, 0.6, 0.8, 0.0, 0.2]);

        // A missing channel advances the tween (like the JS getter) but
        // resolves to nothing, so the link keeps its saved value.
        let before = set.get("t1").expect("tween").frame();
        assert_eq!(resolve_tween_link(&mut set, "t1", 3), None);
        assert_eq!(set.get("t1").expect("tween").frame(), (before + 1) % 5);
        assert_eq!(resolve_tween_link(&mut set, "missing", 0), None);
    }

    #[test]
    fn parse_tween_link_location_handles_the_stored_forms() {
        assert_eq!(
            parse_tween_link_location("tweens.tweens['in-1'].value[0]"),
            Some(("in-1".to_string(), 0))
        );
        assert_eq!(
            parse_tween_link_location("tweens.tweens[in-2].value[1]"),
            Some(("in-2".to_string(), 1))
        );
        assert_eq!(
            parse_tween_link_location("tweens['in-3'].value[2]"),
            Some(("in-3".to_string(), 2))
        );

        assert_eq!(parse_tween_link_location("meyda/getFeature"), None);
        assert_eq!(
            parse_tween_link_location("tweens.tweens['in-1'].value"),
            None
        );
        assert_eq!(parse_tween_link_location("tweens.tweens['in-1']"), None);
        assert_eq!(
            parse_tween_link_location("tweens.tweens['in-1'].value[x]"),
            None
        );
        assert_eq!(parse_tween_link_location("tweens.tweens['in-1']"), None);
    }

    // ————————————————— the set —————————————————

    #[test]
    fn tween_set_loads_preset_arrays_and_round_trips() {
        let first = default_link_tween("in-1", None);
        let mut second = default_link_tween("in-2", Some(&json!(16)));
        second["isGallery"] = json!(true);
        let document = json!([first, second]);

        let set = TweenSet::load_from_json(&document, 60.0, 128.0).expect("load");
        assert_eq!(set.len(), 2);
        assert_eq!(set.ids().collect::<Vec<_>>(), vec!["in-1", "in-2"]);
        assert!(set.contains("in-2"));

        // Gallery tweens stay in the document — filtering is the caller's job.
        assert_eq!(set.to_json(), document);
    }

    #[test]
    fn tween_set_loads_the_live_worker_shapes() {
        // `state.tweens` root (`{ tweens: { id: descriptor } }`).
        let document = json!({
            "tweens": {
                "a": {"id": "a", "data": [[0], [1]]},
                "b": {"data": [[0], [2]], "easing": "easeInOutSine"}
            }
        });
        let set = TweenSet::load_from_json(&document, 60.0, 120.0).expect("load");
        assert_eq!(set.len(), 2);
        assert_eq!(set.ids().collect::<Vec<_>>(), vec!["a", "b"]);
        // A descriptor without an id takes the map key (`getSceneTweenEntries`
        // / `scene_ops::restore_scene_inputs` behaviour).
        assert_eq!(set.get("b").expect("b").id(), "b");
        assert_eq!(
            set.to_json(),
            json!([
                {"id": "a", "data": [[0], [1]]},
                {"id": "b", "data": [[0], [2]], "easing": "easeInOutSine"}
            ])
        );

        // A bare id-keyed map works too.
        let set =
            TweenSet::load_from_json(&json!({"c": {"id": "c", "data": [[0], [1]]}}), 60.0, 120.0)
                .expect("load");
        assert_eq!(set.ids().collect::<Vec<_>>(), vec!["c"]);

        // null clears; a non-container is an error.
        assert!(
            TweenSet::load_from_json(&Value::Null, 60.0, 120.0)
                .expect("load")
                .is_empty()
        );
        assert_eq!(
            TweenSet::load_from_json(&json!("nope"), 60.0, 120.0).unwrap_err(),
            TweenError::InvalidShape
        );
        // A descriptor without an `id` is skipped like any other undecodable
        // entry (the JS creates per tween and lets the rest survive, R-9).
        assert!(
            TweenSet::load_from_json(&json!([{"data": [[0], [1]]}]), 60.0, 120.0)
                .expect("load")
                .is_empty()
        );
    }

    #[test]
    fn tween_set_serialises_a_non_finite_descriptor_without_collapsing_the_others() {
        // R-5: `beats/SET_BPM 0` stored `Infinity` in a BPM tween's `duration`.
        // `serde_json` refuses a non-finite float, so `to_value` failed and
        // `to_json` used to emit `{}` for the descriptor — and the set-level
        // fallback wiped every tween from `state.tweens`.
        let good = default_link_tween("in-good", Some(&json!(8)));
        let good_descriptor = descriptor_from(&good);

        let mut corrupt = TweenDescriptor::new("in-bad", vec![vec![0.0], vec![1.0]]);
        corrupt.duration = f64::INFINITY;
        corrupt.easing = "easeInOutSine".to_string();

        let mut set = TweenSet::new();
        set.insert_descriptor(good_descriptor, 60.0, 120.0);
        set.insert_descriptor(corrupt, 60.0, 120.0);

        let value = set.to_json();
        let entries = value.as_array().expect("array");
        assert_eq!(entries.len(), 2, "{value}");
        // The healthy descriptor is untouched ...
        assert_eq!(entries[0], good);

        // ... and the corrupt one keeps everything that is representable,
        // omitting only the non-finite field (a reload restores the default).
        let corrupt_json = entries[1].as_object().expect("object");
        assert_eq!(corrupt_json["id"], json!("in-bad"));
        assert_eq!(corrupt_json["data"], json!([[0], [1]]));
        assert_eq!(corrupt_json["easing"], json!("easeInOutSine"));
        assert!(!corrupt_json.contains_key("duration"), "{value}");

        // The document still loads as a set.
        let reloaded = TweenSet::load_from_json(&value, 60.0, 120.0).expect("reload");
        assert_eq!(
            reloaded.ids().collect::<Vec<_>>(),
            vec!["in-good", "in-bad"]
        );
        assert_eq!(
            reloaded
                .get("in-bad")
                .expect("corrupt descriptor survives")
                .descriptor()
                .duration,
            1000.0
        );
    }

    #[test]
    fn tween_set_load_skips_undecodable_entries_and_keeps_the_rest() {
        // R-9: one bad descriptor used to fail the whole document, emptying
        // the runtime while the offender stayed in `state.tweens`. The JS
        // creates per tween (and unknown easings already fall back to linear),
        // so the rest must survive.
        let document = json!([
            {"id": "good-1", "data": [[0], [1]], "useBpm": false},
            "not a descriptor",
            {"id": "bad-data", "data": "nope"},
            {"id": "good-2", "data": [[0], [2]], "easing": "easeInOutSine"}
        ]);

        let set = TweenSet::load_from_json(&document, 60.0, 120.0).expect("load");
        assert_eq!(set.ids().collect::<Vec<_>>(), vec!["good-1", "good-2"]);
        assert_eq!(
            set.to_json(),
            json!([
                {"id": "good-1", "data": [[0], [1]], "useBpm": false},
                {"id": "good-2", "data": [[0], [2]], "easing": "easeInOutSine"}
            ])
        );

        // The id-keyed live-worker shape is just as lenient.
        let set = TweenSet::load_from_json(
            &json!({
                "tweens": {
                    "a": {"id": "a", "data": [[0], [1]]},
                    "b": {"id": "b", "data": "nope"}
                }
            }),
            60.0,
            120.0,
        )
        .expect("load");
        assert_eq!(set.ids().collect::<Vec<_>>(), vec!["a"]);

        // A document that is not a tween container is still an error: there is
        // nothing to be lenient about.
        assert_eq!(
            TweenSet::load_from_json(&json!(42), 60.0, 120.0).unwrap_err(),
            TweenError::InvalidShape
        );
    }

    #[test]
    fn tween_set_round_trip_is_stable_when_an_entry_is_skipped() {
        let document = json!([
            {"id": "kept", "data": [[0], [1]], "bpmDivision": 8, "useBpm": true},
            {"id": "dropped", "duration": "nope"}
        ]);

        let first = TweenSet::load_from_json(&document, 60.0, 120.0)
            .expect("load")
            .to_json();
        let second = TweenSet::load_from_json(&first, 60.0, 120.0)
            .expect("reload")
            .to_json();

        assert_eq!(
            first,
            json!([{"id": "kept", "data": [[0], [1]], "bpmDivision": 8, "useBpm": true}])
        );
        assert_eq!(first, second);
    }

    #[test]
    fn tween_set_upserts_removes_and_snapshots() {
        let mut set = TweenSet::new();
        set.insert_descriptor(linear_tween(), 10.0, 120.0);
        assert_eq!(set.len(), 1);

        // The same id replaces in place, keeping document order.
        set.insert_descriptor(linear_tween(), 10.0, 120.0);
        assert_eq!(set.len(), 1);

        // `tweens/ADD_TWEEN` payloads decode straight in.
        let runtime = set
            .upsert_json(
                &json!({"id": "added", "data": [[0], [1]], "useBpm": false}),
                10.0,
                120.0,
            )
            .expect("upsert");
        assert_eq!(runtime.id(), "added");
        assert_eq!(set.ids().collect::<Vec<_>>(), vec!["t1", "added"]);

        set.get_mut("t1").expect("t1").set_frame(2);
        let snapshot = set.values_snapshot();
        assert_eq!(snapshot["t1"], vec![0.4]);
        assert_eq!(snapshot["added"], vec![0.0]);

        set.advance_all();
        assert_eq!(set.get("t1").expect("t1").frame(), 3);
        assert_eq!(set.get("added").expect("added").frame(), 1);

        assert!(set.remove("t1"));
        assert!(!set.remove("t1"));
        assert_eq!(set.ids().collect::<Vec<_>>(), vec!["added"]);
    }

    #[test]
    fn tween_set_replace_clears_the_previous_document() {
        let mut set =
            TweenSet::load_from_json(&json!([default_link_tween("old", None)]), 60.0, 120.0)
                .expect("load");
        assert!(set.contains("old"));

        set.replace_from_json(&json!([default_link_tween("new", None)]), 60.0, 120.0)
            .expect("replace");
        assert!(!set.contains("old"));
        assert!(set.contains("new"));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn free_keyframe_normalisation_preserves_wall_clock_playback() {
        for rows in [3usize, 5] {
            for duration_as_total_time in [false, true] {
                let data = even_rows(rows);
                let mut even = TweenDescriptor::new("even", data);
                even.use_bpm = false;
                even.duration = 1000.0;
                even.easing = "easeInOutSine".to_string();
                even.direction = "normal".to_string();
                even.duration_as_total_time = duration_as_total_time;

                let free = even.with_free_keyframes();
                assert_eq!(free.data.len(), rows + 1);
                assert_eq!(free.data[0], even.data[0]);
                assert_eq!(&free.data[1..], &even.data[..]);
                assert_eq!(
                    free.keyframe_times,
                    Some((0..=rows).map(|index| index as f64 / rows as f64).collect())
                );

                for fps in [60.0, 30.0, 144.0] {
                    for looping in [false, true] {
                        let mut even = even.clone();
                        even.looping = looping;
                        let free = even.with_free_keyframes();
                        assert_eq!(
                            build_tween_frames(&even, fps, 120.0),
                            build_tween_frames(&free, fps, 120.0),
                            "{rows} rows, total={duration_as_total_time}, fps={fps}, loop={looping}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn free_keyframe_normalisation_keeps_the_bpm_cycle() {
        // The `duration`/`bpmDivision` re-expression multiplies and divides a
        // different row count, so a bpm tween can drift by an ulp; the frame
        // count and the animation itself must not change.
        for rows in [3usize, 5] {
            let mut even = TweenDescriptor::new("even", even_rows(rows));
            even.easing = "easeOutQuad".to_string();
            even.direction = "alternate".to_string();
            let free = even.with_free_keyframes();

            for bpm in [120.0, 128.0, 174.0, 0.0] {
                for fps in [60.0, 30.0, 144.0] {
                    let before = build_tween_frames(&even, fps, bpm);
                    let after = build_tween_frames(&free, fps, bpm);
                    assert_eq!(before.len(), after.len(), "frame count");
                    let mut worst = 0.0f64;
                    for (a, b) in before.iter().flatten().zip(after.iter().flatten()) {
                        worst = worst.max((a - b).abs());
                    }
                    assert!(
                        worst <= 1e-12,
                        "{rows} rows at {bpm} bpm / {fps} fps drifted by {worst:e}"
                    );
                }
            }
        }
    }

    #[test]
    fn normalisation_only_rewrites_evenly_keyframed_multi_row_tweens() {
        let two = TweenDescriptor::new("two", vec![vec![0.0], vec![1.0]]);
        assert!(!two.uses_even_keyframes());
        assert_eq!(two.with_free_keyframes(), two);

        let mut free = TweenDescriptor::new("free", even_rows(3));
        free.keyframe_times = Some(vec![0.0, 0.25, 1.0]);
        assert!(!free.uses_even_keyframes());
        assert_eq!(free.with_free_keyframes(), free);

        let mut explicit = TweenDescriptor::new("explicit", even_rows(3));
        explicit.keyframe_times = Some(vec![0.0, 0.5, 1.0]);
        assert!(!explicit.uses_even_keyframes());
        assert_eq!(explicit.with_free_keyframes(), explicit);

        let mut total = TweenDescriptor::new("three", even_rows(3));
        total.use_bpm = false;
        total.duration_as_total_time = true;
        assert!(total.uses_even_keyframes());
        // Already-total durations need no scaling at all.
        let normalised = total.with_free_keyframes();
        assert!(normalised.duration_as_total_time);
        assert_eq!(normalised.duration, total.duration);
    }

    #[test]
    fn evenly_spaced_free_times_sample_like_an_even_tween() {
        // A two-row `[0, 1]` list is anime's `[from, to]` shorthand; the even
        // grid fast path keeps it bit-identical.
        let mut even = TweenDescriptor::new("two", vec![vec![0.0], vec![1.0]]);
        even.use_bpm = false;
        even.duration = 500.0;
        even.easing = "easeInOutCubic".to_string();
        let mut free = even.clone();
        free.keyframe_times = Some(vec![0.0, 1.0]);
        assert_eq!(
            build_tween_frames(&even, 30.0, 120.0),
            build_tween_frames(&free, 30.0, 120.0)
        );
    }

    fn even_rows(rows: usize) -> Vec<Vec<f64>> {
        (0..rows)
            .map(|index| {
                let progress = index as f64 / (rows - 1) as f64;
                vec![progress, progress * progress, 1.0 - progress]
            })
            .collect()
    }
}
