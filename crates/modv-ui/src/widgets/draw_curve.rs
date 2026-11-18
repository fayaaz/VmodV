//! The curve model behind the native Draw input link.
//!
//! A draw curve is a single-channel polyline: explicit `values` joined at
//! explicit `times`. The default is two points (a flat line), so clicking the
//! graph adds points the way the tween curve editor does. Holding Shift and
//! dragging switches to freehand painting, which samples the current line onto
//! a fixed [`DRAW_POINTS`] grid and overwrites it along the stroke.
//!
//! Either representation is dispatched as a tween descriptor: the rows become
//! `data`, the positions `keyframeTimes`, and the engine's tween runtime plays
//! it (BPM or wall-clock cycles, looping, preset/Scene persistence) without a
//! second animation runtime.
//!
//! Pure helpers only: the widget lives in [`crate::widgets::draw_control`].

use egui::{Rect, pos2};

use crate::model::TweenView;
use crate::widgets::tween_curve::{progress_to_x, value_to_y};

/// Number of samples a freehand stroke is rasterised onto.
///
/// 96 points across a ~400 px canvas is roughly one sample every 4 px, fine
/// enough to look freehand and still small enough to store in every preset.
pub const DRAW_POINTS: usize = 96;

/// Minimum progress gap between two hand-placed points.
///
/// Smaller than the tween editor's gap because a freehand curve can already
/// have 96 points; this still leaves room to insert between them.
pub const MIN_POINT_GAP: f64 = 0.002;

/// Upper bound on hand-placed points (a runaway click guard, not a UI limit).
const MAX_DRAW_POINTS: usize = 512;

/// A single-channel polyline with explicit keyframe positions.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawCurve {
    /// One value per point.
    pub values: Vec<f64>,
    /// Progress (0..1) of every point, strictly increasing and spanning `0..1`.
    pub times: Vec<f64>,
}

impl DrawCurve {
    /// A flat two-point line at the middle of `min..max`.
    pub fn new(min: f64, max: f64) -> Self {
        let middle = draw_middle(min, max);
        Self {
            values: vec![middle, middle],
            times: vec![0.0, 1.0],
        }
    }

    /// Build a curve from a `state.tweens` view.
    ///
    /// The view's `keyframeTimes` are preserved when valid; an absent or
    /// invalid list falls back to even spacing. A single row becomes a flat
    /// two-point line, and an empty view has no curve to draw.
    pub fn from_view(view: &TweenView, min: f64, max: f64) -> Option<Self> {
        let values: Vec<f64> = view
            .data
            .iter()
            .filter_map(|row| row.first().copied())
            .filter(|value| value.is_finite())
            .collect();
        if values.is_empty() {
            return None;
        }
        let times = view
            .keyframe_times
            .as_deref()
            .filter(|times| valid_times(times, values.len()))
            .map(<[f64]>::to_vec);
        Some(Self::from_values(&values, times.as_deref(), min, max))
    }

    /// Build from values plus optional explicit times.
    pub fn from_values(values: &[f64], times: Option<&[f64]>, min: f64, max: f64) -> Self {
        let values: Vec<f64> = values
            .iter()
            .copied()
            .filter(|value| value.is_finite())
            .collect();
        match values.len() {
            0 => Self::new(min, max),
            1 => Self {
                values: vec![values[0], values[0]],
                times: vec![0.0, 1.0],
            },
            _ => {
                let times = times
                    .filter(|times| valid_times(times, values.len()))
                    .map(<[f64]>::to_vec)
                    .unwrap_or_else(|| even_times(values.len()));
                Self { values, times }
            }
        }
    }

    /// The curve as tween `data` rows (one channel).
    pub fn to_rows(&self) -> Vec<Vec<f64>> {
        self.values.iter().map(|value| vec![*value]).collect()
    }

    /// The explicit keyframe progress for every row.
    pub fn times(&self) -> Vec<f64> {
        self.times.clone()
    }

    /// Number of points.
    pub fn rows(&self) -> usize {
        self.values.len()
    }

    /// Linear value at `progress`.
    pub fn sample(&self, progress: f64) -> f64 {
        let rows = self.values.len();
        if rows == 0 {
            return 0.0;
        }
        if progress <= self.times[0] {
            return self.values[0];
        }
        if progress >= self.times[rows - 1] {
            return self.values[rows - 1];
        }
        let index = self
            .times
            .windows(2)
            .position(|pair| progress < pair[1])
            .unwrap_or(rows - 2);
        let span = self.times[index + 1] - self.times[index];
        let local = if span == 0.0 {
            0.0
        } else {
            (progress - self.times[index]) / span
        };
        let from = self.values[index];
        from + local * (self.values[index + 1] - from)
    }

    /// Rasterise the curve onto the fixed draw grid if it is not already there.
    fn ensure_dense(&mut self) {
        let last = (DRAW_POINTS - 1) as f64;
        let dense = self.times.len() == DRAW_POINTS
            && self
                .times
                .iter()
                .enumerate()
                .all(|(index, time)| (time - index as f64 / last).abs() <= 1e-12);
        if dense {
            return;
        }
        self.values = (0..DRAW_POINTS)
            .map(|index| self.sample(index as f64 / last))
            .collect();
        self.times = even_times(DRAW_POINTS);
    }

    /// Paint a freehand stroke segment from `from` to `to`, both
    /// `(progress, value)`.
    ///
    /// The current line is rasterised onto the draw grid first; every sample
    /// between the two pointer positions is then filled from the straight line
    /// joining them, so a fast drag cannot leave gaps. A stationary pointer
    /// paints the nearest sample. Returns whether any value changed.
    pub fn paint(&mut self, from: (f64, f64), to: (f64, f64)) -> bool {
        if !from.0.is_finite() || !from.1.is_finite() || !to.0.is_finite() || !to.1.is_finite() {
            return false;
        }
        self.ensure_dense();
        let count = self.values.len();
        if count == 0 {
            return false;
        }

        let last = (count - 1) as f64;
        let a = from.0.clamp(0.0, 1.0) * last;
        let b = to.0.clamp(0.0, 1.0) * last;
        let horizontal = (to.0 - from.0).abs() <= f64::EPSILON;
        let (first, end) = if horizontal {
            let nearest = a.round() as usize;
            (nearest, nearest)
        } else {
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            (low.floor() as usize, high.ceil() as usize)
        };

        let mut changed = false;
        let last_index = end.min(count - 1);
        for (index, sample) in self
            .values
            .iter_mut()
            .enumerate()
            .take(last_index + 1)
            .skip(first)
        {
            let progress = index as f64 / last;
            let value = if horizontal {
                to.1
            } else {
                from.1 + (progress - from.0) / (to.0 - from.0) * (to.1 - from.1)
            };
            if value.is_finite() && (*sample - value).abs() > f64::EPSILON {
                *sample = value;
                changed = true;
            }
        }
        changed
    }

    /// Reset to a flat two-point line at `value`.
    pub fn reset(&mut self, value: f64) {
        if value.is_finite() {
            self.values = vec![value, value];
            self.times = vec![0.0, 1.0];
        }
    }

    /// Insert a hand-placed point at `progress`, returning its row index.
    ///
    /// Points too close to an endpoint or another point are rejected, matching
    /// the tween editor's minimum gap.
    pub fn insert_point(&mut self, progress: f64, value: f64) -> Option<usize> {
        let rows = self.rows();
        if !(2..MAX_DRAW_POINTS).contains(&rows)
            || !progress.is_finite()
            || !value.is_finite()
            || progress <= MIN_POINT_GAP
            || progress >= 1.0 - MIN_POINT_GAP
        {
            return None;
        }
        if self
            .times
            .iter()
            .any(|existing| (existing - progress).abs() < MIN_POINT_GAP)
        {
            return None;
        }
        let index = self
            .times
            .iter()
            .position(|&existing| existing > progress)
            .unwrap_or(rows - 1);
        if index == 0 || index >= rows {
            return None;
        }
        let lower = self.times[index - 1] + MIN_POINT_GAP;
        let upper = self.times[index] - MIN_POINT_GAP;
        if lower > upper {
            return None;
        }
        let time = progress.clamp(lower, upper);
        self.times.insert(index, time);
        self.values.insert(index, value);
        Some(index)
    }

    /// Remove an interior point (endpoints and a two-point minimum stay).
    pub fn remove_point(&mut self, index: usize) -> bool {
        if index == 0 || index + 1 >= self.rows() || self.rows() <= 2 {
            return false;
        }
        self.values.remove(index);
        self.times.remove(index);
        true
    }

    /// Move a point's value and, for interior points, its time.
    ///
    /// Endpoints stay pinned at progress 0 and 1; their values can still move.
    /// Returns whether anything changed.
    pub fn drag_point(&mut self, index: usize, progress: f64, value: f64) -> bool {
        let rows = self.rows();
        if index >= rows {
            return false;
        }
        let mut changed = false;
        if index > 0 && index + 1 < rows && progress.is_finite() {
            let lower = self.times[index - 1] + MIN_POINT_GAP;
            let upper = self.times[index + 1] - MIN_POINT_GAP;
            if lower <= upper {
                let time = progress.clamp(lower, upper);
                if self.times[index] != time {
                    self.times[index] = time;
                    changed = true;
                }
            }
        }
        if value.is_finite() && self.values[index] != value {
            self.values[index] = value;
            changed = true;
        }
        changed
    }

    /// A point's screen position.
    pub fn point_pos(&self, index: usize, bounds: (f64, f64), rect: Rect) -> egui::Pos2 {
        pos2(
            progress_to_x(self.times[index], rect),
            value_to_y(self.values[index], bounds, rect),
        )
    }

    /// The point under `pointer`, within `radius` pixels.
    pub fn nearest_point(
        &self,
        pointer: egui::Pos2,
        bounds: (f64, f64),
        rect: Rect,
        radius: f32,
    ) -> Option<usize> {
        let mut best = None;
        let mut best_distance = radius;
        for index in 0..self.rows() {
            let distance = self.point_pos(index, bounds, rect).distance(pointer);
            if distance <= best_distance {
                best = Some(index);
                best_distance = distance;
            }
        }
        best
    }
}

/// The flat-line value between `min` and `max`, tolerant of reversed bounds.
pub fn draw_middle(min: f64, max: f64) -> f64 {
    let low = if min.is_finite() { min } else { 0.0 };
    let high = if max.is_finite() { max } else { 1.0 };
    (low + high) / 2.0
}

/// Whether `times` is a usable explicit keyframe list.
fn valid_times(times: &[f64], rows: usize) -> bool {
    times.len() == rows
        && rows >= 2
        && times.iter().all(|time| time.is_finite())
        && times[0] == 0.0
        && times[times.len() - 1] == 1.0
        && times.windows(2).all(|pair| pair[0] < pair[1])
}

/// Even keyframe progress for `rows` rows, ending exactly at `1`.
fn even_times(rows: usize) -> Vec<f64> {
    let last = rows.saturating_sub(1).max(1);
    (0..rows).map(|index| index as f64 / last as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_curve_is_a_flat_two_point_line() {
        let curve = DrawCurve::new(0.0, 2.0);
        assert_eq!(curve.values, vec![1.0, 1.0]);
        assert_eq!(curve.times, vec![0.0, 1.0]);
        assert_eq!(curve.to_rows(), vec![vec![1.0], vec![1.0]]);
    }

    #[test]
    fn from_view_preserves_sparse_points_and_times() {
        let view = TweenView {
            data: vec![vec![0.0], vec![0.8], vec![0.2]],
            keyframe_times: Some(vec![0.0, 0.3, 1.0]),
            ..Default::default()
        };
        let curve = DrawCurve::from_view(&view, 0.0, 1.0).expect("curve");
        assert_eq!(curve.values, vec![0.0, 0.8, 0.2]);
        assert_eq!(curve.times, vec![0.0, 0.3, 1.0]);
        assert!((curve.sample(0.15) - 0.4).abs() < 1e-12);
    }

    #[test]
    fn from_view_falls_back_to_even_times() {
        let view = TweenView {
            data: vec![vec![0.0], vec![1.0], vec![0.0]],
            ..Default::default()
        };
        let curve = DrawCurve::from_view(&view, 0.0, 1.0).expect("curve");
        assert_eq!(curve.times, vec![0.0, 0.5, 1.0]);

        let mut view = TweenView {
            data: vec![vec![0.0], vec![1.0], vec![0.0]],
            ..Default::default()
        };
        view.keyframe_times = Some(vec![0.0, 0.7, 0.3]);
        let curve = DrawCurve::from_view(&view, 0.0, 1.0).expect("curve");
        assert_eq!(curve.times, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn from_view_duplicates_a_single_row_and_ignores_an_empty_one() {
        let view = TweenView {
            data: vec![vec![0.25]],
            ..Default::default()
        };
        let curve = DrawCurve::from_view(&view, 0.0, 1.0).expect("curve");
        assert_eq!(curve.values, vec![0.25, 0.25]);
        assert_eq!(curve.times, vec![0.0, 1.0]);

        assert!(DrawCurve::from_view(&TweenView::default(), 0.0, 1.0).is_none());
    }

    #[test]
    fn sampling_interpolates_between_points() {
        let curve = DrawCurve {
            values: vec![0.0, 1.0, 0.0],
            times: vec![0.0, 0.25, 1.0],
        };
        assert!((curve.sample(0.0) - 0.0).abs() < 1e-12);
        assert!((curve.sample(0.125) - 0.5).abs() < 1e-12);
        assert!((curve.sample(0.25) - 1.0).abs() < 1e-12);
        assert!((curve.sample(0.625) - 0.5).abs() < 1e-12);
        assert!((curve.sample(1.0) - 0.0).abs() < 1e-12);
        assert!((curve.sample(4.0) - 0.0).abs() < 1e-12);
    }

    #[test]
    fn painting_densifies_to_the_grid_and_fills_the_stroke() {
        let mut curve = DrawCurve::new(0.0, 1.0);
        assert!(curve.paint((0.25, 0.2), (0.5, 0.8)));
        assert_eq!(curve.rows(), DRAW_POINTS);
        assert_eq!(curve.times, even_times(DRAW_POINTS));

        assert!((curve.sample(0.25) - 0.2).abs() < 0.01);
        assert!((curve.sample(0.5) - 0.8).abs() < 0.01);
        assert!((curve.sample(0.375) - 0.5).abs() < 0.02);
        assert!((curve.sample(0.0) - 0.5).abs() < 1e-12);
        assert!((curve.sample(1.0) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn painting_backwards_fills_the_same_segment() {
        let mut curve = DrawCurve::new(0.0, 1.0);
        assert!(curve.paint((0.75, 0.9), (0.5, 0.1)));
        assert!((curve.sample(0.5) - 0.1).abs() < 0.01);
        assert!((curve.sample(0.75) - 0.9).abs() < 0.01);
        assert!((curve.sample(0.625) - 0.5).abs() < 0.02);
    }

    #[test]
    fn a_stationary_paint_sets_only_the_nearest_sample() {
        let mut curve = DrawCurve::new(0.0, 1.0);
        assert!(curve.paint((0.5, 0.1), (0.5, 0.9)));
        let index = (0.5 * (DRAW_POINTS - 1) as f64).round() as usize;
        assert!((curve.values[index] - 0.9).abs() < 1e-12);
        assert!((curve.values[index - 1] - 0.5).abs() < 1e-12);
        assert!((curve.values[index + 1] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn painting_a_sparse_curve_keeps_its_line_shape() {
        let mut curve = DrawCurve {
            values: vec![0.0, 1.0, 0.0],
            times: vec![0.0, 0.25, 1.0],
        };
        curve.paint((0.6, 0.6), (0.7, 0.7));
        let expected = curve.sample(0.9);
        assert!(
            (expected - 0.133_333_333_333_333_3).abs() < 0.02,
            "the untouched right side keeps the original slope: {expected}"
        );
    }

    #[test]
    fn inserting_a_point_keeps_the_curve_sparse() {
        let mut curve = DrawCurve::new(0.0, 1.0);
        let index = curve.insert_point(0.5, 0.9).expect("point");
        assert_eq!(index, 1);
        assert_eq!(curve.values, vec![0.5, 0.9, 0.5]);
        assert_eq!(curve.times, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn insertion_rejects_endpoints_crowding_and_a_full_curve() {
        let mut curve = DrawCurve::new(0.0, 1.0);
        assert_eq!(curve.insert_point(0.0, 0.0), None);
        assert_eq!(curve.insert_point(1.0, 0.0), None);
        assert_eq!(curve.insert_point(0.001, 0.0), None);
        assert!(curve.insert_point(0.5, 0.5).is_some());
        assert_eq!(curve.insert_point(0.5005, 0.0), None, "too close");
    }

    #[test]
    fn dragging_pins_endpoint_times_but_moves_their_values() {
        let mut curve = DrawCurve {
            values: vec![0.0, 0.5, 1.0],
            times: vec![0.0, 0.5, 1.0],
        };
        assert!(curve.drag_point(0, 0.4, 0.2));
        assert_eq!(curve.times[0], 0.0, "the first endpoint stays pinned");
        assert_eq!(curve.values[0], 0.2);

        assert!(curve.drag_point(2, 0.2, 0.8));
        assert_eq!(curve.times[2], 1.0, "the last endpoint stays pinned");
        assert_eq!(curve.values[2], 0.8);

        assert!(curve.drag_point(1, 0.4, 0.6));
        assert_eq!(curve.times[1], 0.4);
        assert_eq!(curve.values[1], 0.6);
    }

    #[test]
    fn dragging_an_interior_point_cannot_cross_its_neighbours() {
        let mut curve = DrawCurve {
            values: vec![0.0, 0.5, 0.5, 1.0],
            times: vec![0.0, 0.3, 0.6, 1.0],
        };
        assert!(curve.drag_point(1, -1.0, 0.5));
        assert!((curve.times[1] - (0.0 + MIN_POINT_GAP)).abs() < 1e-12);
        assert!(curve.drag_point(1, 2.0, 0.5));
        assert!((curve.times[1] - (0.6 - MIN_POINT_GAP)).abs() < 1e-12);
    }

    #[test]
    fn removal_keeps_the_endpoints_and_two_points() {
        let mut curve = DrawCurve {
            values: vec![0.0, 0.5, 1.0],
            times: vec![0.0, 0.5, 1.0],
        };
        assert!(!curve.remove_point(0));
        assert!(!curve.remove_point(2));
        assert!(curve.remove_point(1));
        assert_eq!(curve.values, vec![0.0, 1.0]);
        assert!(!curve.remove_point(1));
    }

    #[test]
    fn nearest_point_hit_tests_the_handles() {
        let curve = DrawCurve {
            values: vec![0.0, 1.0],
            times: vec![0.0, 1.0],
        };
        let rect = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(360.0, 200.0));
        let bounds = (0.0, 1.0);
        let start = curve.point_pos(0, bounds, rect);
        let end = curve.point_pos(1, bounds, rect);
        assert_eq!(curve.nearest_point(start, bounds, rect, 9.0), Some(0));
        assert_eq!(curve.nearest_point(end, bounds, rect, 9.0), Some(1));
        assert_eq!(
            curve.nearest_point(pos2(200.0, 100.0), bounds, rect, 9.0),
            None
        );
    }

    #[test]
    fn reset_returns_to_a_flat_two_point_line() {
        let mut curve = DrawCurve::new(0.0, 1.0);
        curve.paint((0.0, 0.0), (1.0, 1.0));
        curve.reset(0.25);
        assert_eq!(curve.values, vec![0.25, 0.25]);
        assert_eq!(curve.times, vec![0.0, 1.0]);
        curve.reset(f64::NAN);
        assert_eq!(curve.values, vec![0.25, 0.25]);
    }
}
