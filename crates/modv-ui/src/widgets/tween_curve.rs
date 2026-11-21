//! The tween curve model behind the VmodV curve editor.
//!
//! Pure helpers only: the keyframe grid, axis maths, point editing and the
//! Electron-style JSON shape. [`crate::widgets::tween_control`] draws them and
//! queues the resulting `tweens/createTween` descriptor.
//!
//! # Keyframe model
//!
//! Electron tweens store `data` rows only: three or more rows play as anime's
//! one-tween-per-keyframe shape (hold row 0 for the first `1/N`, then reach
//! rows `1..N-1` at `2/N..1`). The engine's `keyframeTimes` extension makes
//! those positions draggable. A curve always carries explicit `times`; an
//! evenly keyframed multi-row tween is normalised on load (a duplicate of row 0
//! plus the even grid), which is exactly the playback shape and is what
//! `modv_engine::tweens::TweenDescriptor::with_free_keyframes` dispatches.
//!
//! Two-row tweens stay in Electron's `[from, to]` shape (`times = [0, 1]`,
//! `explicit_times = false`), so opening the editor on one changes nothing.

use egui::{Color32, Rect, pos2, vec2};

use crate::model::TweenView;
use crate::theme;

/// Distinct colours for overlaid channels, in row-channel order.
pub const CHANNEL_COLORS: [Color32; 6] = [
    theme::TWEEN,
    theme::ACCENT,
    theme::OK,
    theme::DANGER,
    Color32::from_rgb(190, 120, 255),
    Color32::from_rgb(255, 140, 60),
];

/// Minimum progress gap between two interior keyframes.
pub const MIN_KEYFRAME_GAP: f64 = 0.01;

/// A tween's keyframes with an explicit time for every row.
#[derive(Clone, Debug, PartialEq)]
pub struct TweenCurve {
    /// `data` rows: one value per channel per keyframe.
    pub data: Vec<Vec<f64>>,
    /// Progress (0..1) of every row, strictly increasing, `0` then `1`.
    pub times: Vec<f64>,
    /// Whether to emit `keyframeTimes`. False for the two-row shorthand, where
    /// an explicit `[0, 1]` list would be redundant.
    pub explicit_times: bool,
}

impl TweenCurve {
    /// Build a curve from a `state.tweens` view.
    ///
    /// Anime's evenly keyframed multi-row shape is normalised to explicit
    /// times; an invalid view (fewer than two rows, ragged rows or no
    /// channels) has no curve to draw and returns `None`.
    pub fn from_view(view: &TweenView) -> Option<Self> {
        let data = view.data.clone();
        if !rectangular(&data) || data.len() < 2 {
            return None;
        }
        let rows = data.len();

        if let Some(times) = view
            .keyframe_times
            .as_ref()
            .filter(|times| valid_times(times, rows))
        {
            return Some(Self {
                data,
                times: times.clone(),
                explicit_times: rows > 2,
            });
        }

        if rows > 2 {
            // The anime shape: hold row 0 for one segment, then move through
            // one row per segment.
            let mut held = Vec::with_capacity(rows + 1);
            held.push(data[0].clone());
            held.extend(data.iter().cloned());
            let times = even_times(rows + 1);
            return Some(Self {
                data: held,
                times,
                explicit_times: true,
            });
        }

        Some(Self {
            data,
            times: vec![0.0, 1.0],
            explicit_times: false,
        })
    }

    /// Build from parsed JSON rows plus optional explicit times.
    ///
    /// Even-spaced data without times is normalised the same way
    /// [`TweenCurve::from_view`] normalises an evenly keyframed tween.
    pub fn from_rows(data: Vec<Vec<f64>>, times: Option<Vec<f64>>) -> Option<Self> {
        if !rectangular(&data) || data.len() < 2 {
            return None;
        }
        let rows = data.len();
        if let Some(times) = times.filter(|times| valid_times(times, rows)) {
            return Some(Self {
                data,
                times,
                explicit_times: rows > 2,
            });
        }
        if rows > 2 {
            let mut held = Vec::with_capacity(rows + 1);
            held.push(data[0].clone());
            held.extend(data.iter().cloned());
            return Some(Self {
                data: held,
                times: even_times(rows + 1),
                explicit_times: true,
            });
        }
        Some(Self {
            data,
            times: vec![0.0, 1.0],
            explicit_times: false,
        })
    }

    /// Number of keyframe rows.
    pub fn rows(&self) -> usize {
        self.data.len()
    }

    /// Number of overlaid channels.
    pub fn channels(&self) -> usize {
        self.data.first().map_or(0, Vec::len)
    }

    /// The value of `channel` at row `index`.
    pub fn value(&self, index: usize, channel: usize) -> f64 {
        self.data
            .get(index)
            .and_then(|row| row.get(channel))
            .copied()
            .unwrap_or(0.0)
    }

    /// Linear interpolation of all channels at `time`, in the editor's
    /// piecewise-linear display model (the descriptor's easing is applied by
    /// playback, not by the handle grid).
    pub fn sample(&self, time: f64) -> Vec<f64> {
        let channels = self.channels();
        (0..channels)
            .map(|channel| self.sample_channel(channel, time))
            .collect()
    }

    /// Linear interpolation of one channel at `time`.
    pub fn sample_channel(&self, channel: usize, time: f64) -> f64 {
        let rows = self.rows();
        if rows == 0 {
            return 0.0;
        }
        if time <= self.times[0] {
            return self.value(0, channel);
        }
        if time >= self.times[rows - 1] {
            return self.value(rows - 1, channel);
        }
        let index = self
            .times
            .windows(2)
            .position(|pair| time < pair[1])
            .unwrap_or(rows - 2);
        let span = self.times[index + 1] - self.times[index];
        let local = if span == 0.0 {
            0.0
        } else {
            (time - self.times[index]) / span
        };
        let from = self.value(index, channel);
        let to = self.value(index + 1, channel);
        from + local * (to - from)
    }

    /// The channel whose interpolated value is closest to `value` at `time`.
    pub fn nearest_channel(&self, time: f64, value: f64) -> usize {
        let mut best = 0;
        let mut best_distance = f64::INFINITY;
        for channel in 0..self.channels() {
            let distance = (self.sample_channel(channel, time) - value).abs();
            if distance < best_distance {
                best = channel;
                best_distance = distance;
            }
        }
        best
    }

    /// The y-axis range: every channel value and the prop's `min`/`max`,
    /// padded by 10% of the span (or a unit when the span is degenerate).
    pub fn value_bounds(&self, min: f64, max: f64) -> (f64, f64) {
        let mut low = min.min(max);
        let mut high = min.max(max);
        for value in self
            .data
            .iter()
            .flatten()
            .copied()
            .filter(|v| v.is_finite())
        {
            low = low.min(value);
            high = high.max(value);
        }
        if !low.is_finite() || !high.is_finite() {
            return (0.0, 1.0);
        }
        let span = high - low;
        let padding = if span > 0.0 { span * 0.1 } else { 1.0 };
        (low - padding, high + padding)
    }

    /// Insert a keyframe at `time`.
    ///
    /// The target channel gets `value`; every other channel is linearly
    /// interpolated at that time. Returns the inserted row index, or `None`
    /// when the point would sit on top of an existing keyframe.
    pub fn insert_point(&mut self, time: f64, value: f64, channel: usize) -> Option<usize> {
        let rows = self.rows();
        if rows < 2 || time <= MIN_KEYFRAME_GAP || time >= 1.0 - MIN_KEYFRAME_GAP {
            return None;
        }
        if self
            .times
            .iter()
            .any(|existing| (existing - time).abs() < MIN_KEYFRAME_GAP)
        {
            return None;
        }
        let index = self
            .times
            .iter()
            .position(|&existing| existing > time)
            .unwrap_or(rows - 1);
        if index == 0 || index >= rows {
            return None;
        }
        let lower = self.times[index - 1] + MIN_KEYFRAME_GAP;
        let upper = self.times[index] - MIN_KEYFRAME_GAP;
        if lower > upper {
            return None;
        }
        let time = time.clamp(lower, upper);
        let mut row = self.sample(time);
        if let Some(slot) = row.get_mut(channel) {
            *slot = value;
        }
        self.times.insert(index, time);
        self.data.insert(index, row);
        if self.rows() > 2 {
            self.explicit_times = true;
        }
        Some(index)
    }

    /// Remove the interior keyframe at `index` (endpoints stay, two rows are
    /// the minimum).
    pub fn remove_point(&mut self, index: usize) -> bool {
        if index == 0 || index + 1 >= self.rows() || self.rows() <= 2 {
            return false;
        }
        self.data.remove(index);
        self.times.remove(index);
        if self.rows() <= 2 {
            self.times = vec![0.0, 1.0];
            self.explicit_times = false;
        }
        true
    }

    /// Move an interior keyframe's time, keeping the list sorted and leaving
    /// at least [`MIN_KEYFRAME_GAP`] between neighbours. Endpoints stay pinned
    /// at 0 and 1.
    pub fn drag_time(&mut self, index: usize, time: f64) -> bool {
        let rows = self.rows();
        if index == 0 || index + 1 >= rows {
            return false;
        }
        let lower = self.times[index - 1] + MIN_KEYFRAME_GAP;
        let upper = self.times[index + 1] - MIN_KEYFRAME_GAP;
        if lower > upper || !time.is_finite() {
            return false;
        }
        let time = time.clamp(lower, upper);
        let changed = self.times[index] != time;
        self.times[index] = time;
        changed
    }

    /// Set one channel's value at a keyframe.
    pub fn set_value(&mut self, index: usize, channel: usize, value: f64) -> bool {
        let Some(slot) = self
            .data
            .get_mut(index)
            .and_then(|row| row.get_mut(channel))
        else {
            return false;
        };
        if !value.is_finite() || *slot == value {
            return false;
        }
        *slot = value;
        true
    }

    /// The Electron-style JSON: `data`, plus `keyframeTimes` when the tween
    /// carries explicit times.
    pub fn to_json(&self) -> String {
        let data = format_rows(&self.data);
        if self.explicit_times {
            format!(
                "{{\"data\": {data}, \"keyframeTimes\": {}}}",
                format_numbers(&self.times)
            )
        } else {
            format!("{{\"data\": {data}}}")
        }
    }
}

/// The parsed shape of the editable JSON box.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedTweenJson {
    pub data: Vec<Vec<f64>>,
    /// `None` for a bare row array and for an object without the key.
    pub keyframe_times: Option<Vec<f64>>,
}

/// Parse the editable JSON box.
///
/// Accepts the descriptor object (`{"data": ..., "keyframeTimes": ...}`) and
/// Electron's bare `Data` array, which means evenly spaced keyframes. Rows
/// must be rectangular and finite with at least two rows, and explicit times
/// must match the row count, be strictly increasing and span `0..1`.
pub fn parse_tween_json(text: &str) -> Result<ParsedTweenJson, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;

    let (data_value, times_value) = match &value {
        serde_json::Value::Object(map) => {
            let data = map
                .get("data")
                .ok_or_else(|| "expected a `data` key of keyframe rows".to_string())?;
            let times = match map.get("keyframeTimes") {
                None | Some(serde_json::Value::Null) => None,
                Some(value) => Some(value),
            };
            (data, times)
        }
        serde_json::Value::Array(_) => (&value, None),
        _ => return Err("expected a JSON object or an array of keyframe rows".to_string()),
    };

    let rows = data_value
        .as_array()
        .ok_or_else(|| "`data` must be an array of keyframe rows".to_string())?;
    if rows.len() < 2 {
        return Err("at least two keyframe rows are required".to_string());
    }

    let mut data = Vec::with_capacity(rows.len());
    let mut channels = None;
    for (index, row) in rows.iter().enumerate() {
        let row = row
            .as_array()
            .ok_or_else(|| format!("row {index} must be an array of numbers"))?;
        if row.is_empty() {
            return Err(format!("row {index} has no channel values"));
        }
        if let Some(expected) = channels {
            if row.len() != expected {
                return Err("every row needs the same number of channels".to_string());
            }
        } else {
            channels = Some(row.len());
        }
        let mut values = Vec::with_capacity(row.len());
        for value in row {
            values.push(finite_number(value, index)?);
        }
        data.push(values);
    }

    let keyframe_times = times_value
        .map(|value| {
            let times = value
                .as_array()
                .ok_or_else(|| "`keyframeTimes` must be an array of numbers".to_string())?;
            let times: Vec<f64> = times
                .iter()
                .enumerate()
                .map(|(index, value)| finite_number(value, index))
                .collect::<Result<_, _>>()?;
            if times.len() != data.len() {
                return Err("`keyframeTimes` must have one entry per keyframe row".to_string());
            }
            if !times.windows(2).all(|pair| pair[0] < pair[1]) {
                return Err("`keyframeTimes` must be strictly increasing".to_string());
            }
            if times.first() != Some(&0.0) || times.last() != Some(&1.0) {
                return Err("`keyframeTimes` must start at 0 and end at 1".to_string());
            }
            Ok(times)
        })
        .transpose()?;

    Ok(ParsedTweenJson {
        data,
        keyframe_times,
    })
}

fn finite_number(value: &serde_json::Value, row: usize) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("row {row} contains a value that is not a number"))
}

/// Whether `times` is a usable explicit keyframe list for `rows` rows.
pub fn valid_times(times: &[f64], rows: usize) -> bool {
    times.len() == rows
        && rows >= 2
        && times.iter().all(|time| time.is_finite())
        && times[0] == 0.0
        && times[times.len() - 1] == 1.0
        && times.windows(2).all(|pair| pair[0] < pair[1])
}

fn rectangular(data: &[Vec<f64>]) -> bool {
    let Some(first) = data.first() else {
        return false;
    };
    if first.is_empty() {
        return false;
    }
    data.iter()
        .all(|row| row.len() == first.len() && row.iter().all(|value| value.is_finite()))
}

fn even_times(rows: usize) -> Vec<f64> {
    let last = rows.saturating_sub(1).max(1);
    (0..rows).map(|index| index as f64 / last as f64).collect()
}

fn format_rows(data: &[Vec<f64>]) -> String {
    let rows: Vec<String> = data.iter().map(|row| format_numbers(row)).collect();
    format!("[{}]", rows.join(", "))
}

fn format_numbers(values: &[f64]) -> String {
    let values: Vec<String> = values
        .iter()
        .map(|value| {
            // Rust's `Display` for f64 is the shortest round-trip form
            // (`0.3`, not `0.30000000000000004`) and writes whole numbers
            // without a trailing `.0`, like `JSON.stringify`.
            if value.is_finite() {
                format!("{value}")
            } else {
                "0".to_string()
            }
        })
        .collect();
    format!("[{}]", values.join(", "))
}

/// One graph axis: seconds, or musical beats when the tween follows the tempo.
#[derive(Clone, Debug, PartialEq)]
pub struct TweenAxis {
    pub use_bpm: bool,
    /// The tempo used for the beat scale (the live one, or 120 for a falsy
    /// tempo like the engine's `store.state.beats.bpm || 120`).
    pub bpm: f64,
    /// Total units across progress `0..1`: beats or seconds.
    pub total: f64,
    /// Tick positions in the same units, `0..=total`.
    pub ticks: Vec<f64>,
    /// The real cycle length in seconds.
    pub cycle_seconds: f64,
}

impl TweenAxis {
    /// The axis label shown under the graph.
    pub fn title(&self) -> String {
        if self.use_bpm {
            format!(
                "{:.2} beats · {:.2} s at {:.0} BPM",
                self.total, self.cycle_seconds, self.bpm
            )
        } else {
            format!("{:.2} s", self.cycle_seconds)
        }
    }

    /// Format one tick value with its unit suffix.
    pub fn label(&self, value: f64) -> String {
        if self.use_bpm {
            format!("{value:.2}")
        } else {
            format!("{value:.2}s")
        }
    }
}

/// Build the axis for a tween.
///
/// X spans progress 0..1. With BPM the scale is the real cycle length in
/// beats: `cycle_seconds / (60 / bpm)`, i.e. the same
/// `effective_duration(bpm)` the engine plays, measured in beats. Without BPM
/// it is simply the cycle length in seconds. Ticks sit at quarter fractions of
/// the span.
pub fn tween_axis(use_bpm: bool, bpm: f64, cycle_seconds: f64) -> TweenAxis {
    let cycle_seconds = if cycle_seconds.is_finite() && cycle_seconds > 0.0 {
        cycle_seconds
    } else {
        0.0
    };
    let bpm = if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        120.0
    };
    let total = if use_bpm {
        cycle_seconds / (60.0 / bpm)
    } else {
        cycle_seconds
    };
    let ticks = (0..=4).map(|index| total * index as f64 / 4.0).collect();
    TweenAxis {
        use_bpm,
        bpm,
        total,
        ticks,
        cycle_seconds,
    }
}

/// Progress (0..1) to an x coordinate inside `rect`.
pub fn progress_to_x(progress: f64, rect: Rect) -> f32 {
    rect.left() + (progress.clamp(0.0, 1.0) as f32) * rect.width()
}

/// An x coordinate inside `rect` to progress, clamped to 0..1.
pub fn x_to_progress(x: f32, rect: Rect) -> f64 {
    if rect.width() <= 0.0 {
        return 0.0;
    }
    (((x - rect.left()) / rect.width()).clamp(0.0, 1.0)) as f64
}

/// A value to a y coordinate inside `rect`.
pub fn value_to_y(value: f64, bounds: (f64, f64), rect: Rect) -> f32 {
    let (low, high) = bounds;
    let span = high - low;
    let normalized = if span > 0.0 && value.is_finite() {
        (value - low) / span
    } else {
        0.5
    };
    rect.bottom() - (normalized.clamp(0.0, 1.0) as f32) * rect.height()
}

/// A y coordinate inside `rect` to a value, clamped to `bounds`.
pub fn y_to_value(y: f32, bounds: (f64, f64), rect: Rect) -> f64 {
    let (low, high) = bounds;
    if rect.height() <= 0.0 {
        return low;
    }
    let normalized = ((rect.bottom() - y) / rect.height()).clamp(0.0, 1.0) as f64;
    low + normalized * (high - low)
}

/// A keyframe handle's screen position.
pub fn handle_pos(
    curve: &TweenCurve,
    index: usize,
    channel: usize,
    bounds: (f64, f64),
    rect: Rect,
) -> egui::Pos2 {
    pos2(
        progress_to_x(curve.times[index], rect),
        value_to_y(curve.value(index, channel), bounds, rect),
    )
}

/// The handle under `pointer`, within `radius` pixels.
pub fn nearest_handle(
    curve: &TweenCurve,
    pointer: egui::Pos2,
    bounds: (f64, f64),
    rect: Rect,
    radius: f32,
) -> Option<(usize, usize)> {
    let mut best = None;
    let mut best_distance = radius;
    for channel in 0..curve.channels() {
        for index in 0..curve.rows() {
            let distance = handle_pos(curve, index, channel, bounds, rect).distance(pointer);
            if distance <= best_distance {
                best = Some((index, channel));
                best_distance = distance;
            }
        }
    }
    best
}

/// The editor canvas's nominal size.
pub fn canvas_size() -> egui::Vec2 {
    vec2(360.0, 200.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(360.0, 200.0))
    }

    fn curve() -> TweenCurve {
        TweenCurve {
            data: vec![vec![0.0, 10.0], vec![0.4, 14.0], vec![1.0, 20.0]],
            times: vec![0.0, 0.35, 1.0],
            explicit_times: true,
        }
    }

    fn view() -> TweenView {
        TweenView {
            id: "input-1".to_string(),
            data: vec![vec![0.0], vec![0.5], vec![1.0]],
            duration: 1000.0,
            ..Default::default()
        }
    }

    #[test]
    fn progress_and_x_round_trip() {
        let rect = rect();
        for progress in [0.0, 0.25, 0.5, 1.0] {
            let x = progress_to_x(progress, rect);
            assert!((x_to_progress(x, rect) - progress).abs() < 1e-6);
        }
        assert_eq!(progress_to_x(-1.0, rect), rect.left());
        assert_eq!(progress_to_x(2.0, rect), rect.right());
        assert_eq!(x_to_progress(f32::INFINITY, rect), 1.0);
    }

    #[test]
    fn value_and_y_round_trip_with_clamping() {
        let rect = rect();
        let bounds = (-1.0, 3.0);
        for value in [-1.0, 0.5, 3.0] {
            let y = value_to_y(value, bounds, rect);
            assert!((y_to_value(y, bounds, rect) - value).abs() < 1e-6);
        }
        assert_eq!(value_to_y(-10.0, bounds, rect), rect.bottom());
        assert_eq!(value_to_y(10.0, bounds, rect), rect.top());
        assert_eq!(y_to_value(f32::NEG_INFINITY, bounds, rect), 3.0);
    }

    #[test]
    fn bounds_include_the_prop_range_and_pad_ten_percent() {
        let curve = TweenCurve {
            data: vec![vec![0.2], vec![2.0]],
            times: vec![0.0, 1.0],
            explicit_times: false,
        };
        let (low, high) = curve.value_bounds(0.0, 1.0);
        assert!((low - (0.0 - 0.2)).abs() < 1e-9);
        assert!((high - (2.0 + 0.2)).abs() < 1e-9);

        let flat = TweenCurve {
            data: vec![vec![1.0], vec![1.0]],
            times: vec![0.0, 1.0],
            explicit_times: false,
        };
        assert_eq!(flat.value_bounds(1.0, 1.0), (0.0, 2.0));
    }

    #[test]
    fn axis_uses_beats_when_bpm_is_on() {
        let axis = tween_axis(true, 120.0, 0.5);
        assert_eq!(axis.bpm, 120.0);
        assert!((axis.total - 1.0).abs() < 1e-12);
        assert_eq!(
            axis.ticks,
            vec![0.0, 0.25, 0.5, 0.75, 1.0],
            "quarter-beat ticks"
        );
        assert_eq!(axis.label(0.25), "0.25");
        assert!(axis.title().contains("1.00 beats"));
        assert!(axis.title().contains("0.50 s"));
        assert!(axis.title().contains("120 BPM"));
    }

    #[test]
    fn axis_uses_seconds_without_bpm() {
        let axis = tween_axis(false, 120.0, 2.0);
        assert_eq!(axis.total, 2.0);
        assert_eq!(axis.ticks, vec![0.0, 0.5, 1.0, 1.5, 2.0]);
        assert_eq!(axis.label(1.5), "1.50s");
        assert_eq!(axis.title(), "2.00 s");
    }

    #[test]
    fn axis_falls_back_to_120_bpm_and_clamps_non_finite_cycles() {
        let axis = tween_axis(true, 0.0, 0.5);
        assert_eq!(axis.bpm, 120.0);
        assert!((axis.total - 1.0).abs() < 1e-12);

        let axis = tween_axis(true, f64::NAN, 0.5);
        assert_eq!(axis.bpm, 120.0);

        let axis = tween_axis(true, 120.0, f64::NAN);
        assert_eq!(axis.total, 0.0);
        assert_eq!(axis.ticks, vec![0.0; 5]);
    }

    #[test]
    fn inserting_a_point_interpolates_the_other_channels() {
        let mut curve = curve();
        let index = curve.insert_point(0.6, 7.5, 0).expect("inserted");
        assert_eq!(index, 2);
        assert_eq!(curve.rows(), 4);
        assert_eq!(curve.times, vec![0.0, 0.35, 0.6, 1.0]);
        assert_eq!(curve.data[2][0], 7.5);
        // Channel 1 at 0.6 sits between 14 at 0.35 and 20 at 1.
        let expected = 14.0 + (0.6 - 0.35) / (1.0 - 0.35) * 6.0;
        assert!((curve.data[2][1] - expected).abs() < 1e-12);
        assert!(curve.explicit_times);
    }

    #[test]
    fn insertion_rejects_crowded_points() {
        let curve = TweenCurve {
            data: vec![vec![0.0], vec![0.5], vec![1.0]],
            times: vec![0.0, 0.5, 1.0],
            explicit_times: true,
        };
        let mut curve = curve;
        assert_eq!(curve.insert_point(0.002, 0.0, 0), None);
        assert_eq!(curve.insert_point(0.501, 0.0, 0), None);

        let mut tight = TweenCurve {
            data: vec![vec![0.0], vec![0.5], vec![1.0]],
            times: vec![0.0, 0.005, 1.0],
            explicit_times: true,
        };
        assert_eq!(tight.insert_point(0.01, 0.0, 0), None);
    }

    #[test]
    fn removal_keeps_endpoints_and_two_rows() {
        let mut curve = curve();
        assert!(!curve.remove_point(0));
        assert!(!curve.remove_point(curve.rows() - 1));
        assert!(curve.remove_point(1));
        assert_eq!(curve.rows(), 2);
        assert_eq!(curve.times, vec![0.0, 1.0]);
        assert!(!curve.explicit_times);
        assert!(!curve.remove_point(1));
    }

    #[test]
    fn dragging_pins_endpoints_and_keeps_the_gap() {
        let mut curve = curve();
        assert!(!curve.drag_time(0, 0.2));
        assert!(!curve.drag_time(curve.rows() - 1, 0.2));
        assert_eq!(curve.times[0], 0.0);
        assert_eq!(curve.times[2], 1.0);

        // The interior point cannot cross its neighbours.
        assert!(curve.drag_time(1, -1.0));
        assert!((curve.times[1] - MIN_KEYFRAME_GAP).abs() < 1e-12);
        assert!(curve.drag_time(1, 2.0));
        assert!((curve.times[1] - (1.0 - MIN_KEYFRAME_GAP)).abs() < 1e-12);
        assert!(curve.times.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn dragging_a_value_updates_only_that_channel() {
        let mut curve = curve();
        assert!(curve.set_value(1, 1, 3.0));
        assert_eq!(curve.data[1], vec![0.4, 3.0]);
        assert!(!curve.set_value(1, 7, 3.0), "no such channel");
        assert!(!curve.set_value(1, 0, f64::NAN));
    }

    #[test]
    fn json_object_round_trips_through_the_box() {
        let text = "{\"data\": [[0], [0.4], [1]], \"keyframeTimes\": [0, 0.35, 1]}";
        let parsed = parse_tween_json(text).expect("parses");
        assert_eq!(parsed.data, vec![vec![0.0], vec![0.4], vec![1.0]]);
        assert_eq!(parsed.keyframe_times, Some(vec![0.0, 0.35, 1.0]));

        let curve = TweenCurve::from_rows(parsed.data, parsed.keyframe_times).expect("curve");
        assert_eq!(curve.to_json(), text);
    }

    #[test]
    fn json_bare_array_means_even_spacing() {
        let parsed = parse_tween_json("[[0], [0.4], [1]]").expect("parses");
        assert_eq!(parsed.data, vec![vec![0.0], vec![0.4], vec![1.0]]);
        assert_eq!(parsed.keyframe_times, None);

        let curve = TweenCurve::from_rows(parsed.data, parsed.keyframe_times).expect("curve");
        // Anime's shape: hold row 0, then reach 0.4 and 1 at the next thirds.
        assert_eq!(curve.data, vec![vec![0.0], vec![0.0], vec![0.4], vec![1.0]]);
        assert_eq!(curve.times, vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]);
        assert!(curve.explicit_times);
        assert!(curve.to_json().contains("keyframeTimes"));
    }

    #[test]
    fn json_validation_reports_invalid_shapes() {
        let cases = [
            ("", "invalid JSON"),
            ("42", "object or an array"),
            ("[]", "at least two keyframe rows"),
            ("[[0]]", "at least two keyframe rows"),
            ("[[0], [1, 2]]", "same number of channels"),
            ("[[], [1]]", "no channel values"),
            ("[[\"a\"], [1]]", "not a number"),
            ("{\"keyframeTimes\": [0, 1]}", "`data` key"),
            (
                "{\"data\": [[0], [1], [0]], \"keyframeTimes\": [0, 1]}",
                "one entry per keyframe row",
            ),
            (
                "{\"data\": [[0], [1], [0]], \"keyframeTimes\": [0, 0.5, 0.5]}",
                "strictly increasing",
            ),
            (
                "{\"data\": [[0], [1], [0]], \"keyframeTimes\": [0.1, 0.5, 1]}",
                "start at 0",
            ),
            (
                "{\"data\": [[0], [1], [0]], \"keyframeTimes\": [0, 0.5, 0.9]}",
                "end at 1",
            ),
        ];
        for (text, expected) in cases {
            let error = parse_tween_json(text).expect_err(text);
            assert!(
                error.contains(expected),
                "{text} -> {error:?} (expected {expected:?})"
            );
        }
    }

    #[test]
    fn json_null_times_mean_even_spacing() {
        let parsed =
            parse_tween_json("{\"data\": [[0], [1]], \"keyframeTimes\": null}").expect("parses");
        assert_eq!(parsed.keyframe_times, None);
    }

    #[test]
    fn view_with_anime_keyframes_normalises_to_free_times() {
        let curve = TweenCurve::from_view(&view()).expect("curve");
        assert_eq!(curve.data, vec![vec![0.0], vec![0.0], vec![0.5], vec![1.0]]);
        assert_eq!(curve.times, vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]);
        assert!(curve.explicit_times);
    }

    #[test]
    fn view_drops_invalid_times_and_falls_back_to_the_anime_shape() {
        let mut view = view();
        view.keyframe_times = Some(vec![0.0, 0.5, 0.5]);
        let curve = TweenCurve::from_view(&view).expect("curve");
        assert_eq!(curve.rows(), 4);
        assert_eq!(curve.times[1], 1.0 / 3.0);
    }

    #[test]
    fn two_row_view_keeps_the_shorthand() {
        let mut view = view();
        view.data = vec![vec![0.0], vec![1.0]];
        let curve = TweenCurve::from_view(&view).expect("curve");
        assert_eq!(curve.times, vec![0.0, 1.0]);
        assert!(!curve.explicit_times);
        assert_eq!(curve.to_json(), "{\"data\": [[0], [1]]}");
    }

    #[test]
    fn ragged_or_empty_views_have_no_curve() {
        let mut view = view();
        view.data = vec![vec![0.0], vec![1.0, 2.0]];
        assert!(TweenCurve::from_view(&view).is_none());

        view.data = vec![vec![0.0]];
        assert!(TweenCurve::from_view(&view).is_none());

        view.data = vec![vec![], vec![]];
        assert!(TweenCurve::from_view(&view).is_none());
    }

    #[test]
    fn sampling_matches_the_piecewise_linear_handles() {
        let curve = curve();
        assert!((curve.sample_channel(1, 0.0) - 10.0).abs() < 1e-12);
        assert!((curve.sample_channel(1, 0.675) - 17.0).abs() < 1e-12);
        assert!((curve.sample_channel(1, 2.0) - 20.0).abs() < 1e-12);
        assert_eq!(curve.nearest_channel(0.5, 13.0), 1);
        assert_eq!(curve.nearest_channel(0.5, 2.5), 0);
    }
}
