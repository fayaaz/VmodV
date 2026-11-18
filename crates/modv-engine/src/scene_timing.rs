//! Scene / performance-mode timing normalisation and per-cell dwell.
//!
//! Ported from `src/renderer/src/application/worker/scene-timing.js` (145
//! lines). The rules that matter (WS-8, "Per-cell dwell"):
//!
//! - Each row picks its own dwell target **once** when the cell becomes
//!   active ([`pick_cell_dwell`]) and advances independently; "max scene time"
//!   is a per-cell maximum.
//! - A missing/`null` target means "no earlier deadline than the maximum"
//!   ([`clamp_cell_dwell`]), which keeps the legacy `maxSeconds` behaviour.
//! - Dwell only advances while no transition is running — that rule lives in
//!   the scene loop (`scenes.js`), not in this module.
//!
//! ## JS → Rust notes
//!
//! - The JS numeric reader accepts numbers **and** numeric strings
//!   (`scene-timing.js:10-22`). [`to_finite_number`] keeps that, reading a
//!   [`serde_json::Value`] the way the store does.
//! - JS `undefined` / `NaN` / `Infinity` are not representable in JSON, so the
//!   equivalent inputs are `null` (absent), and the strings `"NaN"` /
//!   `"Infinity"` (which `Number()` maps to non-finite). Both mean "invalid".
//! - The JS `{ ...source, minSeconds, ... }` spread keeps unknown keys; the
//!   Rust port returns a typed [`PerformanceTiming`] and the playlist layer
//!   (see [`crate::scenes_playlist`]) carries the remaining performance keys.
//!
//! ## Bar timing (`timingUnit`)
//!
//! The native engine adds `performance.timingUnit: "seconds" | "bars"`
//! (missing means seconds). A bar is [`BEATS_PER_BAR`] beats and
//! `bar seconds = 4 * 60 / bpm`; an invalid/absent tempo uses
//! [`FALLBACK_BPM`] (120), the same `beats.bpm || 120` fallback the tween
//! compiler uses.
//!
//! While a unit is active, `performance.minSeconds`/`maxSeconds` **and** each
//! Scene override's `minSeconds`/`maxSeconds` hold numbers in that unit. The
//! runtime converts them to seconds at the point of use
//! ([`resolve_performance_timing_at`], [`resolve_scene_timing_at`],
//! [`pick_cell_dwell_numeric`]); stored documents keep the user's unit so a
//! Scene edit never silently converts the grid.
//!
//! **Electron compatibility:** the JS build does not read `timingUnit` and
//! would interpret the same numbers as seconds. A native document edited in
//! bars therefore looks wrong in an older Electron build (and switching back
//! to seconds before handing the project over is the safe path). The keys
//! themselves keep their historical names, so no document migration is
//! needed; an older file simply has no `timingUnit` and reads as seconds.
//!
//! Line numbers refer to the tree at commit `8a512b6e` (branch `rust-hybrid`).

use serde_json::Value;

/// Default performance timing (`scene-timing.js:1-5`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerformanceTiming {
    /// Shortest dwell a cell may pick, in seconds.
    pub min_seconds: f64,
    /// Longest dwell a cell may pick, in seconds.
    pub max_seconds: f64,
    /// Crossfade duration for a cell change, in seconds.
    pub transition_seconds: f64,
}

/// `DEFAULT_PERFORMANCE_TIMING` (`scene-timing.js:1-5`).
pub const DEFAULT_PERFORMANCE_TIMING: PerformanceTiming = PerformanceTiming {
    min_seconds: 8.0,
    max_seconds: 24.0,
    transition_seconds: 2.0,
};

/// Scenes never run shorter than this (`scene-timing.js:7`).
pub const MIN_SCENE_SECONDS: f64 = 1.0;

/// A crossfade never runs shorter than this (`scene-timing.js:8`).
pub const MIN_TRANSITION_SECONDS: f64 = 0.1;

/// Beats in a bar. Unger's bar-based Scene timing is always 4/4.
pub const BEATS_PER_BAR: f64 = 4.0;

/// Tempo used when the beat detector has no usable BPM. This is the same
/// `store.state.beats.bpm || 120` fallback the tween compiler uses.
pub const FALLBACK_BPM: f64 = 120.0;

/// Unit the performance `minSeconds` / `maxSeconds` values are expressed in.
///
/// The keys keep their historical names (`minSeconds`/`maxSeconds`) so the
/// Electron build can still read a document written by the native engine; see
/// the module compatibility note. The unit itself is stored alongside them as
/// `performance.timingUnit`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimingUnit {
    /// Plain seconds (`timingUnit` absent or `"seconds"`).
    #[default]
    Seconds,
    /// Musical bars, 4 beats each (`timingUnit: "bars"`).
    Bars,
}

impl TimingUnit {
    /// Read the stored `timingUnit` value. Unknown values fall back to seconds,
    /// which keeps documents written before the unit existed valid.
    pub fn parse(value: Option<&Value>) -> Self {
        match value
            .and_then(Value::as_str)
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("bars") | Some("bar") => Self::Bars,
            _ => Self::Seconds,
        }
    }

    /// The string written to `timingUnit`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Bars => "bars",
        }
    }
}

/// A tempo usable by the bar conversion (`beats.bpm || 120`).
pub fn effective_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        FALLBACK_BPM
    }
}

/// Seconds per beat at `bpm` (`60 / bpm`).
pub fn beat_seconds(bpm: f64) -> f64 {
    60.0 / effective_bpm(bpm)
}

/// Seconds per 4/4 bar at `bpm`.
pub fn bar_seconds(bpm: f64) -> f64 {
    BEATS_PER_BAR * beat_seconds(bpm)
}

/// Convert a value in `unit` to seconds at `bpm`.
pub fn unit_to_seconds(value: f64, unit: TimingUnit, bpm: f64) -> f64 {
    match unit {
        TimingUnit::Seconds => value,
        TimingUnit::Bars => value * bar_seconds(bpm),
    }
}

/// Convert seconds to `unit` at `bpm`.
pub fn seconds_to_unit(seconds: f64, unit: TimingUnit, bpm: f64) -> f64 {
    match unit {
        TimingUnit::Seconds => seconds,
        TimingUnit::Bars => seconds / bar_seconds(bpm),
    }
}

/// Performance timing as stored: values in the active [`TimingUnit`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimingConfig {
    /// Unit both values below are expressed in.
    pub unit: TimingUnit,
    /// Shortest dwell in `unit` (`performance.minSeconds`).
    pub min: f64,
    /// Longest dwell in `unit` (`performance.maxSeconds`).
    pub max: f64,
    /// Crossfade duration, always seconds.
    pub transition_seconds: f64,
}

/// The default config, in seconds (`DEFAULT_PERFORMANCE_TIMING`).
pub const DEFAULT_TIMING_CONFIG: TimingConfig = TimingConfig {
    unit: TimingUnit::Seconds,
    min: DEFAULT_PERFORMANCE_TIMING.min_seconds,
    max: DEFAULT_PERFORMANCE_TIMING.max_seconds,
    transition_seconds: DEFAULT_PERFORMANCE_TIMING.transition_seconds,
};

/// Bars equivalent of the legacy defaults at the 120 BPM reference tempo:
/// 4 bars = 8 s, 12 bars = 24 s.
const DEFAULT_BAR_MIN: f64 = 4.0;
const DEFAULT_BAR_MAX: f64 = 12.0;
/// Half a bar is the bar-unit equivalent of the legacy `min + 1 s` gap at
/// 120 BPM.
const MIN_BAR_GAP: f64 = 0.5;
const MIN_BAR_VALUE: f64 = 0.001;

/// Resolved timing for one Scene cell (`normalizeSceneTiming`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneTiming {
    /// Effective minimum dwell.
    pub min_seconds: f64,
    /// Effective maximum dwell.
    pub max_seconds: f64,
    /// Whether the Scene carries its own timing rather than the global one.
    pub timing_override: bool,
}

/// Read a number the way `toFiniteNumber` does (`scene-timing.js:10-22`):
/// finite numbers pass through, non-empty numeric strings are parsed, every
/// other value (including `null` and non-finite strings) is `None`.
pub fn to_finite_number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number.as_f64().filter(|n| n.is_finite()),
        Some(Value::String(text)) if !text.trim().is_empty() => {
            text.trim().parse::<f64>().ok().filter(|n| n.is_finite())
        }
        _ => None,
    }
}

/// `Math.round(value * 10) / 10` (`scene-timing.js:24-26`).
fn round_tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Round a bar value to thousandths so quarter-bar edits survive the
/// normalisation (rounding to tenths would turn `0.25` into `0.3`).
fn round_milli(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// `normalizeCellBounds` (`scene-timing.js:28-40`): missing bounds fall back
/// to the defaults and the maximum is never below the minimum.
pub fn normalize_cell_bounds_numeric(
    min_seconds: Option<f64>,
    max_seconds: Option<f64>,
) -> (f64, f64) {
    let min = min_seconds
        .filter(|n| n.is_finite())
        .unwrap_or(DEFAULT_PERFORMANCE_TIMING.min_seconds);
    let max = max_seconds
        .filter(|n| n.is_finite())
        .unwrap_or(DEFAULT_PERFORMANCE_TIMING.max_seconds);

    (min, max.max(min))
}

/// [`normalize_cell_bounds_numeric`] for JSON inputs.
pub fn normalize_cell_bounds(
    min_seconds: Option<&Value>,
    max_seconds: Option<&Value>,
) -> (f64, f64) {
    normalize_cell_bounds_numeric(to_finite_number(min_seconds), to_finite_number(max_seconds))
}

/// `clampCellDwell` (`scene-timing.js:61-72`), numeric form.
///
/// A missing/invalid target returns the maximum, so a cell with a broken
/// target keeps the legacy "advance at maxSeconds" deadline.
pub fn clamp_cell_dwell_numeric(
    value: Option<f64>,
    min_seconds: Option<f64>,
    max_seconds: Option<f64>,
) -> f64 {
    let (min, max) = normalize_cell_bounds_numeric(min_seconds, max_seconds);

    match value.filter(|n| n.is_finite()) {
        Some(numeric) => max.min(min.max(numeric)),
        None => max,
    }
}

/// [`clamp_cell_dwell_numeric`] for JSON inputs (`scene-timing.js:61-72`).
pub fn clamp_cell_dwell(
    value: Option<&Value>,
    min_seconds: Option<&Value>,
    max_seconds: Option<&Value>,
) -> f64 {
    clamp_cell_dwell_numeric(
        to_finite_number(value),
        to_finite_number(min_seconds),
        to_finite_number(max_seconds),
    )
}

/// `pickCellDwell` (`scene-timing.js:47-59`), numeric form.
///
/// `roll` is the value the caller got from its RNG; out-of-range and
/// non-finite rolls clamp into `[0, 1]` (JS maps a non-finite roll to `0`).
pub fn pick_cell_dwell_numeric(
    min_seconds: Option<f64>,
    max_seconds: Option<f64>,
    roll: Option<f64>,
) -> f64 {
    let (min, max) = normalize_cell_bounds_numeric(min_seconds, max_seconds);
    let clamped_roll = roll
        .filter(|n| n.is_finite())
        .map(|n| n.clamp(0.0, 1.0))
        .unwrap_or(0.0);

    clamp_cell_dwell_numeric(Some(min + clamped_roll * (max - min)), Some(min), Some(max))
}

/// [`pick_cell_dwell_numeric`] for JSON inputs (`scene-timing.js:47-59`).
pub fn pick_cell_dwell(
    min_seconds: Option<&Value>,
    max_seconds: Option<&Value>,
    roll: Option<f64>,
) -> f64 {
    pick_cell_dwell_numeric(
        to_finite_number(min_seconds),
        to_finite_number(max_seconds),
        roll,
    )
}

/// `normalizeTransitionDuration` (`scene-timing.js:74-80`): at least
/// [`MIN_TRANSITION_SECONDS`], rounded to a tenth.
pub fn normalize_transition_duration(value: Option<&Value>) -> f64 {
    match to_finite_number(value) {
        None => DEFAULT_PERFORMANCE_TIMING.transition_seconds,
        Some(transition) => MIN_TRANSITION_SECONDS.max(round_tenth(transition)),
    }
}

/// `normalizePerformanceTiming` (`scene-timing.js:82-113`) over the active
/// unit.
///
/// `performance` is the store's performance object; `None`/non-object behaves
/// like `{}`. Unknown keys are ignored (the playlist layer keeps them). The
/// values returned by this function are **stored values** in the active
/// [`TimingUnit`] — use [`resolve_performance_timing_at`] to get the seconds
/// the runtime needs.
pub fn normalize_timing_config(performance: Option<&Value>) -> TimingConfig {
    let source = performance.filter(|value| value.is_object());
    let unit = TimingUnit::parse(source.and_then(|p| p.get("timingUnit")));

    let min = to_finite_number(source.and_then(|p| p.get("minSeconds")));
    let max = to_finite_number(source.and_then(|p| p.get("maxSeconds")));
    let transition = to_finite_number(source.and_then(|p| p.get("transitionSeconds")));

    let (min_value, max_value) = match unit {
        TimingUnit::Seconds => {
            // scene-timing.js:88-91
            let min_seconds = match min {
                None => DEFAULT_PERFORMANCE_TIMING.min_seconds,
                Some(value) => MIN_SCENE_SECONDS.max(round_tenth(value)),
            };

            // scene-timing.js:93-96
            let mut max_seconds = match max {
                None => (min_seconds + 1.0).max(DEFAULT_PERFORMANCE_TIMING.max_seconds),
                Some(value) => (MIN_SCENE_SECONDS + 1.0).max(round_tenth(value)),
            };

            // scene-timing.js:98-100
            if max_seconds <= min_seconds {
                max_seconds = round_tenth(min_seconds + 1.0);
            }

            (min_seconds, max_seconds)
        }
        TimingUnit::Bars => {
            // The bar-unit equivalents of the second rules at 120 BPM: a bar is
            // 2 s there, so the 1 s floor is half a bar and `min + 1 s` is
            // `min + 0.5 bar`.
            let min_bars = match min {
                None => DEFAULT_BAR_MIN,
                Some(value) => MIN_BAR_VALUE.max(round_milli(value)),
            };

            let mut max_bars = match max {
                None => (min_bars + MIN_BAR_GAP).max(DEFAULT_BAR_MAX),
                Some(value) => (MIN_BAR_VALUE * 2.0).max(round_milli(value)),
            };

            if max_bars <= min_bars {
                max_bars = round_milli(min_bars + MIN_BAR_GAP);
            }

            (min_bars, max_bars)
        }
    };

    // scene-timing.js:102-105
    let transition_seconds = match transition {
        None => DEFAULT_PERFORMANCE_TIMING.transition_seconds,
        Some(value) => MIN_TRANSITION_SECONDS.max(round_tenth(value)),
    };

    TimingConfig {
        unit,
        min: min_value,
        max: max_value,
        transition_seconds,
    }
}

/// Resolve the stored timing to seconds at the live tempo.
pub fn resolve_performance_timing_at(performance: Option<&Value>, bpm: f64) -> PerformanceTiming {
    let config = normalize_timing_config(performance);

    PerformanceTiming {
        min_seconds: unit_to_seconds(config.min, config.unit, bpm),
        max_seconds: unit_to_seconds(config.max, config.unit, bpm),
        transition_seconds: config.transition_seconds,
    }
}

/// `normalizePerformanceTiming` (`scene-timing.js:82-113`).
///
/// Without a tempo this resolves bars at [`FALLBACK_BPM`]; callers that drive
/// live dwell timing must use [`resolve_performance_timing_at`] with the
/// store's current BPM.
pub fn normalize_performance_timing(performance: Option<&Value>) -> PerformanceTiming {
    resolve_performance_timing_at(performance, FALLBACK_BPM)
}

/// A Scene's effective timing as stored: values in the active [`TimingUnit`].
///
/// Unlike [`SceneTiming`] this keeps the minimum and maximum separate, so the
/// native UI can edit an override without collapsing both fields to the
/// maximum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneTimingValues {
    /// Unit both values below are expressed in (the global performance unit).
    pub unit: TimingUnit,
    /// Effective minimum dwell in `unit`.
    pub min: f64,
    /// Effective maximum dwell in `unit`.
    pub max: f64,
    /// Whether the Scene carries its own timing rather than the global one.
    pub timing_override: bool,
}

/// `normalizeSceneTiming` (`scene-timing.js:115-141`) over the active unit.
pub fn normalize_scene_timing_values(
    scene: Option<&Value>,
    performance: Option<&Value>,
) -> SceneTimingValues {
    let global = normalize_timing_config(performance);
    let source = scene.filter(|value| value.is_object());
    let timing_override = source
        .and_then(|scene| scene.get("timingOverride"))
        .and_then(Value::as_bool)
        == Some(true);

    if !timing_override {
        return SceneTimingValues {
            unit: global.unit,
            min: global.min,
            max: global.max,
            timing_override: false,
        };
    }

    let min = to_finite_number(source.and_then(|scene| scene.get("minSeconds")));
    let max = to_finite_number(source.and_then(|scene| scene.get("maxSeconds")));

    let (min_value, max_value) = match global.unit {
        TimingUnit::Seconds => {
            // scene-timing.js:122-126
            let min_seconds = match min {
                None => global.min,
                Some(value) => MIN_SCENE_SECONDS.max(round_tenth(value)),
            };

            // scene-timing.js:127-131
            let mut max_seconds = match max {
                None => global.max,
                Some(value) => (MIN_SCENE_SECONDS + 1.0).max(round_tenth(value)),
            };

            // scene-timing.js:132-134
            if max_seconds <= min_seconds {
                max_seconds = round_tenth(min_seconds + 1.0);
            }

            (min_seconds, max_seconds)
        }
        TimingUnit::Bars => {
            let min_bars = match min {
                None => global.min,
                Some(value) => MIN_BAR_VALUE.max(round_milli(value)),
            };

            let mut max_bars = match max {
                None => global.max,
                Some(value) => (MIN_BAR_VALUE * 2.0).max(round_milli(value)),
            };

            if max_bars <= min_bars {
                max_bars = round_milli(min_bars + MIN_BAR_GAP);
            }

            (min_bars, max_bars)
        }
    };

    SceneTimingValues {
        unit: global.unit,
        min: min_value,
        max: max_value,
        timing_override: true,
    }
}

/// Resolve a Scene's stored timing to seconds at the live tempo.
pub fn resolve_scene_timing_at(
    scene: Option<&Value>,
    performance: Option<&Value>,
    bpm: f64,
) -> SceneTiming {
    let values = normalize_scene_timing_values(scene, performance);

    SceneTiming {
        min_seconds: unit_to_seconds(values.min, values.unit, bpm),
        max_seconds: unit_to_seconds(values.max, values.unit, bpm),
        timing_override: values.timing_override,
    }
}

/// `normalizeSceneTiming` (`scene-timing.js:115-141`).
///
/// Bars resolve at [`FALLBACK_BPM`]; live callers should use
/// [`resolve_scene_timing_at`].
pub fn normalize_scene_timing(scene: Option<&Value>, performance: Option<&Value>) -> SceneTiming {
    resolve_scene_timing_at(scene, performance, FALLBACK_BPM)
}

/// `resolveSceneTiming` (`scene-timing.js:143-145`) — an alias for
/// [`normalize_scene_timing`], keeping the JS call sites' name.
pub fn resolve_scene_timing(scene: Option<&Value>, performance: Option<&Value>) -> SceneTiming {
    normalize_scene_timing(scene, performance)
}

#[cfg(test)]
mod tests {
    //! Port of `tests/unit/scene-timing.test.js` plus the timing
    //! normalisation cases the JS suite leaves to its callers.
    use super::*;
    use serde_json::json;

    // ————————————————— pickCellDwell —————————————————

    #[test]
    fn pick_cell_dwell_returns_minimum_at_roll_zero() {
        // scene-timing.test.js:11-15
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), Some(0.0)),
            5.0
        );
    }

    #[test]
    fn pick_cell_dwell_stays_below_maximum_for_roll_just_under_one() {
        // scene-timing.test.js:17-26
        let value = pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), Some(0.999_999));
        assert!(value > 5.0);
        assert!(value < 10.0);
    }

    #[test]
    fn pick_cell_dwell_returns_maximum_at_roll_one() {
        // scene-timing.test.js:28-32
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), Some(1.0)),
            10.0
        );
    }

    #[test]
    fn pick_cell_dwell_clamps_out_of_range_rolls() {
        // scene-timing.test.js:34-41
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), Some(-2.0)),
            5.0
        );
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), Some(2.0)),
            10.0
        );
    }

    #[test]
    fn pick_cell_dwell_falls_back_to_minimum_for_non_finite_roll() {
        // scene-timing.test.js:43-51
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), Some(f64::NAN)),
            5.0
        );
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), None),
            5.0
        );
    }

    #[test]
    fn pick_cell_dwell_uses_the_roll_once() {
        // scene-timing.test.js:53-58 — the JS asserts the injected RNG is
        // called exactly once; the Rust port receives the already-rolled
        // value, so the equivalent check is that the roll is applied linearly.
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(10)), Some(0.5)),
            7.5
        );
    }

    #[test]
    fn pick_cell_dwell_collapses_equal_and_inverted_bounds_to_minimum() {
        // scene-timing.test.js:60-70
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!(5)), Some(0.5)),
            5.0
        );
        assert_eq!(
            pick_cell_dwell(Some(&json!(10)), Some(&json!(5)), Some(0.5)),
            10.0
        );
        assert_eq!(
            pick_cell_dwell(Some(&json!(10)), Some(&json!(5)), Some(1.0)),
            10.0
        );
    }

    #[test]
    fn pick_cell_dwell_uses_finite_fallbacks_for_missing_or_invalid_bounds() {
        // scene-timing.test.js:72-93
        assert_eq!(
            pick_cell_dwell(None, None, Some(0.0)),
            DEFAULT_PERFORMANCE_TIMING.min_seconds
        );
        assert_eq!(
            pick_cell_dwell(Some(&json!(5)), Some(&json!("NaN")), Some(0.0)),
            5.0
        );

        let value = pick_cell_dwell(Some(&json!(5)), Some(&json!("Infinity")), Some(0.5));
        assert!(value.is_finite());
        assert!(value >= 5.0);
    }

    // ————————————————— clampCellDwell —————————————————

    #[test]
    fn clamp_cell_dwell_keeps_a_value_inside_the_interval() {
        // scene-timing.test.js:98-100
        assert_eq!(
            clamp_cell_dwell(Some(&json!(7)), Some(&json!(5)), Some(&json!(10))),
            7.0
        );
    }

    #[test]
    fn clamp_cell_dwell_clamps_below_minimum_and_above_maximum() {
        // scene-timing.test.js:102-104
        assert_eq!(
            clamp_cell_dwell(Some(&json!(2)), Some(&json!(5)), Some(&json!(10))),
            5.0
        );
        assert_eq!(
            clamp_cell_dwell(Some(&json!(20)), Some(&json!(5)), Some(&json!(10))),
            10.0
        );
    }

    #[test]
    fn clamp_cell_dwell_falls_back_to_maximum_for_non_finite_value() {
        // scene-timing.test.js:106-109
        assert_eq!(
            clamp_cell_dwell(Some(&json!("NaN")), Some(&json!(5)), Some(&json!(10))),
            10.0
        );
        assert_eq!(
            clamp_cell_dwell(None, Some(&json!(5)), Some(&json!(10))),
            10.0
        );
    }

    #[test]
    fn clamp_cell_dwell_collapses_equal_and_inverted_bounds_to_minimum() {
        // scene-timing.test.js:111-114
        assert_eq!(
            clamp_cell_dwell(Some(&json!(7)), Some(&json!(5)), Some(&json!(5))),
            5.0
        );
        assert_eq!(
            clamp_cell_dwell(Some(&json!(7)), Some(&json!(10)), Some(&json!(5))),
            10.0
        );
    }

    #[test]
    fn clamp_cell_dwell_uses_finite_fallbacks_for_missing_bounds() {
        // scene-timing.test.js:116-121
        let value = clamp_cell_dwell(Some(&json!(7)), None, None);
        assert!(value.is_finite());
        assert!(value >= DEFAULT_PERFORMANCE_TIMING.min_seconds);
    }

    // ————————————————— normalizeTransitionDuration —————————————————

    #[test]
    fn normalize_transition_duration_keeps_a_valid_duration() {
        // scene-timing.test.js:126-128
        assert_eq!(normalize_transition_duration(Some(&json!(2.5))), 2.5);
        assert_eq!(normalize_transition_duration(Some(&json!("3"))), 3.0);
    }

    #[test]
    fn normalize_transition_duration_falls_back_to_default() {
        // scene-timing.test.js:130-143
        assert_eq!(
            normalize_transition_duration(None),
            DEFAULT_PERFORMANCE_TIMING.transition_seconds
        );
        assert_eq!(
            normalize_transition_duration(Some(&Value::Null)),
            DEFAULT_PERFORMANCE_TIMING.transition_seconds
        );
        assert_eq!(
            normalize_transition_duration(Some(&json!("NaN"))),
            DEFAULT_PERFORMANCE_TIMING.transition_seconds
        );
        assert_eq!(
            normalize_transition_duration(Some(&json!("Infinity"))),
            DEFAULT_PERFORMANCE_TIMING.transition_seconds
        );
    }

    #[test]
    fn normalize_transition_duration_never_shorter_than_the_minimum() {
        // scene-timing.test.js:145-148
        assert_eq!(
            normalize_transition_duration(Some(&json!(0))),
            MIN_TRANSITION_SECONDS
        );
        assert_eq!(
            normalize_transition_duration(Some(&json!(-5))),
            MIN_TRANSITION_SECONDS
        );
    }

    // ————————————————— normalizePerformanceTiming —————————————————

    #[test]
    fn performance_timing_defaults_from_missing_input() {
        // scene-timing.js:88-105 with `{}` / undefined.
        assert_eq!(
            normalize_performance_timing(None),
            DEFAULT_PERFORMANCE_TIMING
        );
        assert_eq!(
            normalize_performance_timing(Some(&json!({}))),
            DEFAULT_PERFORMANCE_TIMING
        );
        assert_eq!(
            normalize_performance_timing(Some(&json!("not an object"))),
            DEFAULT_PERFORMANCE_TIMING
        );
    }

    #[test]
    fn performance_timing_rounds_to_tenths_and_floors_scene_minimum() {
        // scene-timing.js:88-96
        let timing = normalize_performance_timing(Some(&json!({
            "minSeconds": std::f64::consts::PI,
            "maxSeconds": 7.25,
            "transitionSeconds": 0.049,
        })));
        assert_eq!(timing.min_seconds, 3.1);
        assert_eq!(timing.max_seconds, 7.3);
        assert_eq!(timing.transition_seconds, MIN_TRANSITION_SECONDS);

        let timing = normalize_performance_timing(Some(&json!({"minSeconds": 0.2})));
        assert_eq!(timing.min_seconds, MIN_SCENE_SECONDS);
        // Default max is `max(min + 1, 24)`.
        assert_eq!(timing.max_seconds, DEFAULT_PERFORMANCE_TIMING.max_seconds);
    }

    #[test]
    fn performance_timing_default_max_tracks_a_large_minimum() {
        // scene-timing.js:93-100
        let timing = normalize_performance_timing(Some(&json!({"minSeconds": 30})));
        assert_eq!(timing.min_seconds, 30.0);
        assert_eq!(timing.max_seconds, 31.0);
    }

    #[test]
    fn performance_timing_pushes_max_above_min() {
        // scene-timing.js:98-100
        let timing = normalize_performance_timing(Some(&json!({
            "minSeconds": 10,
            "maxSeconds": 4,
        })));
        assert_eq!(timing.min_seconds, 10.0);
        assert_eq!(timing.max_seconds, 11.0);

        let timing = normalize_performance_timing(Some(&json!({
            "minSeconds": 10,
            "maxSeconds": 10,
        })));
        assert_eq!(timing.max_seconds, 11.0);
    }

    // ————————————————— normalizeSceneTiming —————————————————

    #[test]
    fn scene_timing_without_override_uses_global_timing() {
        // scene-timing.js:117-120
        let performance = json!({"minSeconds": 4, "maxSeconds": 9});
        let timing = normalize_scene_timing(Some(&json!({})), Some(&performance));
        assert_eq!(
            timing,
            SceneTiming {
                min_seconds: 4.0,
                max_seconds: 9.0,
                timing_override: false,
            }
        );

        // A Scene with min/max but no `timingOverride: true` ignores them.
        let timing = normalize_scene_timing(
            Some(&json!({"minSeconds": 1, "maxSeconds": 2})),
            Some(&performance),
        );
        assert_eq!(timing.min_seconds, 4.0);
        assert_eq!(timing.max_seconds, 9.0);
        assert!(!timing.timing_override);
    }

    #[test]
    fn scene_timing_override_uses_scene_bounds() {
        // scene-timing.js:119-134
        let scene = json!({"timingOverride": true, "minSeconds": 6, "maxSeconds": 12});
        let timing = normalize_scene_timing(
            Some(&scene),
            Some(&json!({"minSeconds": 4, "maxSeconds": 9})),
        );
        assert_eq!(
            timing,
            SceneTiming {
                min_seconds: 6.0,
                max_seconds: 12.0,
                timing_override: true,
            }
        );
    }

    #[test]
    fn scene_timing_override_with_missing_bounds_falls_back_to_global() {
        // scene-timing.js:119-131
        let scene = json!({"timingOverride": true});
        let timing = normalize_scene_timing(
            Some(&scene),
            Some(&json!({"minSeconds": 4, "maxSeconds": 9})),
        );
        assert_eq!(timing.min_seconds, 4.0);
        assert_eq!(timing.max_seconds, 9.0);
        assert!(timing.timing_override);
    }

    #[test]
    fn scene_timing_override_floors_and_orders_bounds() {
        // scene-timing.js:122-134
        let timing = normalize_scene_timing(
            Some(&json!({"timingOverride": true, "minSeconds": 0.1, "maxSeconds": 0.5})),
            None,
        );
        assert_eq!(timing.min_seconds, MIN_SCENE_SECONDS);
        assert_eq!(timing.max_seconds, MIN_SCENE_SECONDS + 1.0);

        let timing = normalize_scene_timing(
            Some(&json!({"timingOverride": true, "minSeconds": 9, "maxSeconds": 3})),
            None,
        );
        assert_eq!(timing.min_seconds, 9.0);
        assert_eq!(timing.max_seconds, 10.0);
    }

    #[test]
    fn scene_timing_reads_numeric_strings() {
        // scene-timing.js:10-22 via toFiniteNumber.
        let timing = normalize_scene_timing(
            Some(&json!({"timingOverride": true, "minSeconds": "3.14", "maxSeconds": "8"})),
            None,
        );
        assert_eq!(timing.min_seconds, 3.1);
        assert_eq!(timing.max_seconds, 8.0);
    }

    #[test]
    fn resolve_scene_timing_matches_normalize() {
        let scene = json!({"timingOverride": true, "minSeconds": 2, "maxSeconds": 5});
        assert_eq!(
            resolve_scene_timing(Some(&scene), None),
            normalize_scene_timing(Some(&scene), None)
        );
    }

    #[test]
    fn to_finite_number_matches_js_number_coercion() {
        assert_eq!(to_finite_number(Some(&json!(3))), Some(3.0));
        assert_eq!(to_finite_number(Some(&json!(3.5))), Some(3.5));
        assert_eq!(to_finite_number(Some(&json!("3.5"))), Some(3.5));
        assert_eq!(to_finite_number(Some(&json!(" 3.5 "))), Some(3.5));
        assert_eq!(to_finite_number(Some(&json!(""))), None);
        assert_eq!(to_finite_number(Some(&json!("   "))), None);
        assert_eq!(to_finite_number(Some(&json!("abc"))), None);
        assert_eq!(to_finite_number(Some(&json!("NaN"))), None);
        assert_eq!(to_finite_number(Some(&json!(true))), None);
        assert_eq!(to_finite_number(Some(&Value::Null)), None);
        assert_eq!(to_finite_number(None), None);
    }

    // ————————————————— TimingUnit / bars —————————————————

    #[test]
    fn a_bar_is_four_beats_and_uses_the_engine_bpm_fallback() {
        assert_eq!(BEATS_PER_BAR, 4.0);
        assert_eq!(beat_seconds(120.0), 0.5);
        assert_eq!(bar_seconds(120.0), 2.0);
        assert_eq!(bar_seconds(60.0), 4.0);
        // `beats.bpm || 120`: 0 and NaN fall back to the reference tempo.
        assert_eq!(beat_seconds(0.0), 0.5);
        assert_eq!(beat_seconds(f64::NAN), 0.5);
        assert_eq!(bar_seconds(0.0), 2.0);
        assert_eq!(bar_seconds(f64::NAN), 2.0);
        // Negative/infinite tempos are just as unusable.
        assert_eq!(bar_seconds(-120.0), 2.0);
        assert_eq!(bar_seconds(f64::INFINITY), 2.0);
    }

    #[test]
    fn timing_unit_converts_values_to_seconds_and_back() {
        assert_eq!(unit_to_seconds(4.0, TimingUnit::Seconds, 120.0), 4.0);
        assert_eq!(unit_to_seconds(4.0, TimingUnit::Bars, 120.0), 8.0);
        assert_eq!(unit_to_seconds(4.0, TimingUnit::Bars, 60.0), 16.0);
        assert_eq!(seconds_to_unit(8.0, TimingUnit::Bars, 120.0), 4.0);
        assert_eq!(seconds_to_unit(16.0, TimingUnit::Bars, 60.0), 4.0);
        assert_eq!(seconds_to_unit(8.0, TimingUnit::Seconds, 120.0), 8.0);
        // A round trip at the reference tempo is exact.
        let seconds = unit_to_seconds(1.25, TimingUnit::Bars, 120.0);
        assert_eq!(seconds_to_unit(seconds, TimingUnit::Bars, 120.0), 1.25);
    }

    #[test]
    fn timing_unit_parses_the_config_value_and_defaults_to_seconds() {
        assert_eq!(TimingUnit::parse(Some(&json!("bars"))), TimingUnit::Bars);
        assert_eq!(TimingUnit::parse(Some(&json!("Bars"))), TimingUnit::Bars);
        assert_eq!(
            TimingUnit::parse(Some(&json!("seconds"))),
            TimingUnit::Seconds
        );
        assert_eq!(
            TimingUnit::parse(Some(&json!("nonsense"))),
            TimingUnit::Seconds
        );
        assert_eq!(TimingUnit::parse(Some(&Value::Null)), TimingUnit::Seconds);
        assert_eq!(TimingUnit::parse(None), TimingUnit::Seconds);
        assert_eq!(TimingUnit::Bars.as_str(), "bars");
        assert_eq!(TimingUnit::Seconds.as_str(), "seconds");
    }

    #[test]
    fn performance_timing_config_reads_bar_values_and_converts_at_use() {
        let performance = json!({
            "timingUnit": "bars",
            "minSeconds": 4,
            "maxSeconds": 12,
            "transitionSeconds": 2,
        });
        let config = normalize_timing_config(Some(&performance));
        assert_eq!(config.unit, TimingUnit::Bars);
        assert_eq!(config.min, 4.0);
        assert_eq!(config.max, 12.0);

        let timing = resolve_performance_timing_at(Some(&performance), 120.0);
        assert_eq!(timing.min_seconds, 8.0);
        assert_eq!(timing.max_seconds, 24.0);
        assert_eq!(timing.transition_seconds, 2.0);

        let timing = resolve_performance_timing_at(Some(&performance), 60.0);
        assert_eq!(timing.min_seconds, 16.0);
        assert_eq!(timing.max_seconds, 48.0);

        // An invalid tempo uses the 120 BPM fallback.
        let timing = resolve_performance_timing_at(Some(&performance), f64::NAN);
        assert_eq!(timing.min_seconds, 8.0);
        assert_eq!(timing.max_seconds, 24.0);
    }

    #[test]
    fn missing_timing_unit_keeps_the_legacy_seconds_config() {
        let config = normalize_timing_config(Some(&json!({"minSeconds": 4, "maxSeconds": 10})));
        assert_eq!(config.unit, TimingUnit::Seconds);
        assert_eq!(config.min, 4.0);
        assert_eq!(config.max, 10.0);
        assert_eq!(
            resolve_performance_timing_at(Some(&json!({"minSeconds": 4, "maxSeconds": 10})), 60.0)
                .min_seconds,
            4.0,
            "seconds values do not depend on the tempo"
        );
    }

    #[test]
    fn bar_timing_normalisation_keeps_quarter_steps_and_orders_the_pair() {
        let config = normalize_timing_config(Some(&json!({
            "timingUnit": "bars",
            "minSeconds": 0.25,
            "maxSeconds": 0.25,
        })));
        assert_eq!(config.min, 0.25, "quarter-bar stays a quarter-bar");
        assert_eq!(config.max, 0.75, "max is pushed above min");

        let config = normalize_timing_config(Some(&json!({
            "timingUnit": "bars",
            "minSeconds": 8,
            "maxSeconds": 2,
        })));
        assert_eq!(config.min, 8.0);
        assert_eq!(config.max, 8.5);
    }

    #[test]
    fn scene_timing_override_exposes_separate_bar_min_and_max() {
        let performance = json!({
            "timingUnit": "bars",
            "minSeconds": 4,
            "maxSeconds": 12,
        });
        let scene = json!({
            "timingOverride": true,
            "minSeconds": 1,
            "maxSeconds": 3,
        });

        let values = normalize_scene_timing_values(Some(&scene), Some(&performance));
        assert_eq!(values.unit, TimingUnit::Bars);
        assert_eq!(values.min, 1.0);
        assert_eq!(values.max, 3.0);
        assert!(values.timing_override);

        let timing = resolve_scene_timing_at(Some(&scene), Some(&performance), 120.0);
        assert_eq!(timing.min_seconds, 2.0);
        assert_eq!(timing.max_seconds, 6.0);

        // The default `typing` wrapper resolves at the reference tempo.
        assert_eq!(
            resolve_scene_timing(Some(&scene), Some(&performance)),
            timing
        );
    }

    #[test]
    fn scene_timing_without_override_inherits_bar_values() {
        let performance = json!({
            "timingUnit": "bars",
            "minSeconds": 4,
            "maxSeconds": 12,
        });
        let values =
            normalize_scene_timing_values(Some(&json!({"minSeconds": 1})), Some(&performance));
        assert_eq!(values.min, 4.0);
        assert_eq!(values.max, 12.0);
        assert!(!values.timing_override);

        let timing = resolve_scene_timing_at(None, Some(&performance), 60.0);
        assert_eq!(timing.min_seconds, 16.0);
        assert_eq!(timing.max_seconds, 48.0);
        assert!(!timing.timing_override);
    }
}
