//! Draw control: paint a whole input-link curve with the pointer.
//!
//! The Draw section sits beside Audio/MIDI/Tween in the focused-input link
//! editor. A drawn curve is stored as a tween descriptor with explicit
//! `keyframeTimes`, so the engine's existing tween runtime plays it (BPM or
//! seconds) and presets/Scenes persist it without a new store slot. The link's
//! `source` is `"draw"` so the two editors never claim the same descriptor.
//!
//! The window paints a fixed-resolution curve ([`DrawCurve`]): drag across the
//! graph and every sample between pointer events is filled, so the whole line
//! is drawn rather than only the points the pointer landed on.

use egui::{Align2, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::layout::WINDOW_DRAW_CURVE;
use crate::model::TweenView;
use crate::state::{DrawCurveEditor, UiState};
use crate::theme;
use crate::widgets::draw_curve::DrawCurve;
use crate::widgets::tween_curve::{
    TweenAxis, progress_to_x, tween_axis, value_to_y, x_to_progress, y_to_value,
};

/// The Draw section's descriptor values.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawControl {
    pub label: String,
    pub input_id: Option<String>,
    /// The descriptor linked to this input. The caller only passes it while
    /// the link's source is `draw`, so a real tween is never edited here.
    pub curve: Option<TweenView>,
    pub min: f64,
    pub max: f64,
}

/// Beats covered by one unit of the tween descriptor's `bpmDivision`.
///
/// `TweenDescriptor::effective_duration` computes
/// `(60 * 60 / bpm) * bpmDivision` ms for a BPM tween; one beat is
/// `60_000 / bpm` ms, so one division unit is `3600 / 60_000 = 0.06` beats.
/// Draw links store the span this way, so they keep playing through the
/// existing tween runtime.
const BEATS_PER_DIVISION: f64 = 0.06;

/// The smallest beat span the Draw editor writes.
const MIN_DRAW_BEATS: f64 = 1.0;

/// The largest beat span the Draw editor writes (the user-facing cap).
const MAX_DRAW_BEATS: f64 = 64.0;

/// The default span for a new draw, in beats.
const DEFAULT_DRAW_BEATS: f64 = 4.0;

/// Convert a beat span into the descriptor's `bpmDivision`.
fn beats_to_division(beats: f64) -> f64 {
    beats / BEATS_PER_DIVISION
}

/// Convert the descriptor's `bpmDivision` back into beats.
fn division_to_beats(division: f64) -> f64 {
    division * BEATS_PER_DIVISION
}

/// The live tempo used by the Draw editor (`store.state.beats.bpm || 120`).
fn draw_tempo(bpm: f64) -> f64 {
    if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        120.0
    }
}

impl DrawControl {
    /// The state path a draw link reads (the same tween slot the descriptor
    /// lives in).
    pub fn link_location(input_id: &str) -> String {
        format!("tweens.tweens['{input_id}'].value[0]")
    }

    pub fn new(
        label: impl Into<String>,
        input_id: Option<String>,
        draw: Option<TweenView>,
        min: f64,
        max: f64,
    ) -> Self {
        Self {
            label: label.into(),
            input_id,
            curve: draw,
            min,
            max,
        }
    }

    /// Draw the section and the floating curve window. Changes are queued into
    /// `state.actions`.
    pub fn draw(&mut self, ui: &mut egui::Ui, state: &mut UiState) {
        let mut open_curve = false;
        let mut edit: Option<TweenView> = None;
        let mut remove = false;
        let bpm = state.status.bpm;

        ui.horizontal_wrapped(|ui| {
            ui.add_sized(
                [82.0, 18.0],
                egui::Label::new(egui::RichText::new(&self.label).size(13.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(&self.label);
            match (&self.input_id, &self.curve) {
                (None, _) => {
                    ui.label(
                        egui::RichText::new("no input (module has no $props entry)")
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                }
                (Some(_), None) => {
                    if ui
                        .button("Start drawing")
                        .on_hover_text("Create a drawn curve for this input")
                        .clicked()
                    {
                        self.create(state, bpm);
                    }
                }
                (Some(_), Some(view)) => {
                    if ui.button("Draw curve").clicked() {
                        open_curve = true;
                    }

                    let mut edited = view.clone();
                    let mut changed = false;
                    if ui
                        .checkbox(&mut edited.use_bpm, "BPM")
                        .on_hover_text("Time the curve by beats instead of seconds")
                        .changed()
                    {
                        changed = true;
                        let current_seconds = cycle_seconds(view, bpm);
                        if edited.use_bpm {
                            // Keep the wall-clock length when switching to
                            // beats; fall back to the default span if the
                            // current cycle is unusable.
                            let beats = if current_seconds > 0.0 {
                                (current_seconds * draw_tempo(bpm) / 60.0)
                                    .clamp(MIN_DRAW_BEATS, MAX_DRAW_BEATS)
                            } else {
                                DEFAULT_DRAW_BEATS
                            };
                            edited.bpm_division = beats_to_division(beats);
                        } else {
                            edited.duration = (current_seconds * 1000.0).clamp(1.0, 600_000.0);
                        }
                    }
                    if edited.use_bpm {
                        let mut beats = division_to_beats(edited.bpm_division)
                            .clamp(MIN_DRAW_BEATS, MAX_DRAW_BEATS);
                        if ui
                            .add(
                                egui::DragValue::new(&mut beats)
                                    .range(MIN_DRAW_BEATS..=MAX_DRAW_BEATS)
                                    .speed(0.25)
                                    .max_decimals(2)
                                    .suffix(" beats"),
                            )
                            .on_hover_text("Beats the drawn curve spans (1–64)")
                            .changed()
                        {
                            edited.bpm_division = beats_to_division(beats);
                            changed = true;
                        }
                    } else {
                        let mut duration = edited.duration;
                        if ui
                            .add(
                                egui::DragValue::new(&mut duration)
                                    .range(1.0..=600_000.0)
                                    .speed(10.0)
                                    .prefix("dur ")
                                    .suffix(" ms"),
                            )
                            .on_hover_text("Whole cycle length in milliseconds")
                            .changed()
                        {
                            edited.duration = duration;
                            changed = true;
                        }
                    }
                    let mut looping = edited.looping;
                    if ui
                        .checkbox(&mut looping, "loop")
                        .on_hover_text("Ping-pong the curve; off restarts from the left")
                        .changed()
                    {
                        edited.looping = looping;
                        changed = true;
                    }
                    if changed {
                        edited.duration_as_total_time = true;
                        edited.direction = "normal".to_string();
                        edited.easing = "linear".to_string();
                        edited.cycle_seconds = cycle_seconds(&edited, bpm);
                        edit = Some(edited);
                    }

                    if ui
                        .small_button("x")
                        .on_hover_text("Remove the drawn curve and its link")
                        .clicked()
                    {
                        remove = true;
                    }
                }
            }
        });

        if remove {
            self.remove(state);
        } else if let Some(view) = edit.take() {
            self.update(state, view);
        }

        if open_curve {
            self.open_curve_editor(state);
        }
        self.draw_curve_editor(ui.ctx(), state);
    }

    /// Create a flat drawn curve at the middle of the prop range.
    fn create(&mut self, state: &mut UiState, bpm: f64) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        let curve = DrawCurve::new(self.min, self.max);
        let view = draw_view(input_id, curve, bpm);
        self.update(state, view);
        // "Start drawing" opens the canvas immediately; the flat line is the
        // starting point for the first stroke.
        self.open_curve_editor(state);
    }

    /// Open the freehand editor for the current descriptor.
    fn open_curve_editor(&mut self, state: &mut UiState) {
        let (Some(input_id), Some(view)) = (self.input_id.clone(), self.curve.clone()) else {
            return;
        };
        if state
            .draw_editor
            .as_ref()
            .is_some_and(|editor| editor.input_id == input_id && editor.open)
        {
            return;
        }
        let Some(curve) = DrawCurve::from_view(&view, self.min, self.max) else {
            return;
        };
        state.draw_editor = Some(DrawCurveEditor::new(input_id, self.label.clone(), curve));
    }

    /// Dispatch an edited descriptor and keep the draw link in place.
    fn update(&mut self, state: &mut UiState, mut view: TweenView) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        view.id = input_id.clone();
        view.direction = "normal".to_string();
        view.easing = "linear".to_string();
        view.duration_as_total_time = true;
        if view.use_bpm {
            view.bpm_division = view.bpm_division.clamp(
                beats_to_division(MIN_DRAW_BEATS),
                beats_to_division(MAX_DRAW_BEATS),
            );
        }
        state.push_action("tweens/createTween", descriptor_payload(&view));
        state.push_action(
            "inputs/createInputLink",
            serde_json::json!({
                "inputId": input_id,
                "type": "state",
                "source": "draw",
                "location": Self::link_location(&input_id),
                "min": 0,
                "max": 1,
            }),
        );
        self.curve = Some(view);
    }

    /// Remove the descriptor and the link that reads it.
    fn remove(&mut self, state: &mut UiState) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        state.push_action("tweens/REMOVE_TWEEN", serde_json::json!({ "id": input_id }));
        state.push_action(
            "inputs/removeInputLink",
            serde_json::json!({ "inputId": input_id }),
        );
        self.curve = None;
        if state
            .draw_editor
            .as_ref()
            .is_some_and(|editor| editor.input_id == input_id)
        {
            state.draw_editor = None;
        }
    }

    /// Draw the open freehand window and dispatch a completed stroke.
    fn draw_curve_editor(&mut self, ctx: &egui::Context, state: &mut UiState) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        let Some(mut editor) = state.draw_editor.take() else {
            return;
        };
        if editor.input_id != input_id || !editor.open {
            state.draw_editor = Some(editor);
            return;
        }
        let Some(view) = self.curve.clone() else {
            // The descriptor went away (removed here or by a Scene apply).
            return;
        };
        let bpm = state.status.bpm;
        if editor.stroke.is_none()
            && editor.dragging.is_none()
            && let Some(current) = DrawCurve::from_view(&view, self.min, self.max)
            && current != editor.curve
        {
            editor.curve = current;
        }

        let axis = tween_axis(view.use_bpm, bpm, view.cycle_seconds);
        let mut open = editor.open;
        let mut dispatch = false;
        let mut reset = false;
        let min = self.min;
        let max = self.max;

        let window = state.layout.apply_window(
            egui::Window::new(format!("Draw curve · {}", editor.title))
                .id(egui::Id::unique("modv-window:draw_curve"))
                .open(&mut open)
                .default_pos([250.0, 110.0])
                .default_size([470.0, 360.0])
                .min_width(320.0)
                .min_height(280.0)
                .resizable(true),
            WINDOW_DRAW_CURVE,
        );
        let response = window.show(ctx, |ui| {
            ui.label(
                egui::RichText::new(axis.title())
                    .size(12.0)
                    .color(theme::MUTED),
            );
            draw_canvas(ui, &mut editor, min, max, &axis, &mut dispatch);
            let shift = ui.input(|input| input.modifiers.shift);
            let points = editor.curve.rows();
            ui.horizontal_wrapped(|ui| {
                if shift {
                    ui.label(
                        egui::RichText::new(format!(
                            "freehand: drag to paint · {points} points"
                        ))
                        .size(12.0)
                        .color(theme::ACCENT),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!(
                            "{points} points · click: add · drag: move · right-click: remove · Shift+drag: draw",
                        ))
                        .size(12.0)
                        .color(theme::MUTED),
                    );
                }
                if ui
                    .button("Reset")
                    .on_hover_text("Replace the curve with a flat line")
                    .clicked()
                {
                    reset = true;
                }
            });
        });

        if let Some(response) = response {
            state
                .layout
                .record_window_rect(WINDOW_DRAW_CURVE, response.response.rect);
        }
        editor.open = open;
        if reset {
            editor
                .curve
                .reset(crate::widgets::draw_curve::draw_middle(min, max));
            dispatch = true;
        }
        if dispatch {
            self.apply_curve(state, &mut editor, bpm);
        }
        if editor.open {
            state.draw_editor = Some(editor);
        }
    }

    /// Dispatch the painted curve as a tween descriptor.
    fn apply_curve(&mut self, state: &mut UiState, editor: &mut DrawCurveEditor, bpm: f64) {
        let Some(mut view) = self.curve.clone() else {
            return;
        };
        view.data = editor.curve.to_rows();
        view.keyframe_times = Some(editor.curve.times());
        view.direction = "normal".to_string();
        view.easing = "linear".to_string();
        view.duration_as_total_time = true;
        view.cycle_seconds = cycle_seconds(&view, bpm);
        self.update(state, view);
    }
}

/// A descriptor for a freshly drawn curve.
fn draw_view(input_id: String, curve: DrawCurve, bpm: f64) -> TweenView {
    let mut view = TweenView {
        id: input_id,
        data: curve.to_rows(),
        keyframe_times: Some(curve.times()),
        duration: 4000.0,
        direction: "normal".to_string(),
        easing: "linear".to_string(),
        looping: true,
        use_bpm: true,
        bpm_division: beats_to_division(DEFAULT_DRAW_BEATS),
        duration_as_total_time: true,
        steps: 0.0,
        frames: 0,
        cycle_seconds: 0.0,
    };
    view.cycle_seconds = cycle_seconds(&view, bpm);
    view
}

/// The whole-cycle length in seconds for a draw descriptor.
fn cycle_seconds(view: &TweenView, bpm: f64) -> f64 {
    let bpm = draw_tempo(bpm);
    let base = if view.use_bpm {
        ((60.0 * 60.0) / bpm) * view.bpm_division
    } else {
        view.duration
    };
    let total = if view.duration_as_total_time {
        base
    } else {
        base / view.data.len().max(1) as f64
    };
    if total.is_finite() {
        total / 1000.0
    } else {
        0.0
    }
}

/// The `tweens/createTween` payload for a draw descriptor.
///
/// `keyframeTimes` is emitted for a curve with more than two points: without
/// it anime's 3+ row handling would hold the first sample for a whole segment
/// instead of joining the clicked points. A two-point curve uses the plain
/// `[from, to]` shorthand. `loopMode: "restart"` opts into replaying the drawn
/// cycle each time instead of ping-ponging it (what `loop: true` alone does).
fn descriptor_payload(view: &TweenView) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "id": view.id,
        "data": view.data,
        "duration": view.duration,
        "direction": view.direction,
        "easing": view.easing,
        "loop": view.looping,
        "loopMode": "restart",
        "useBpm": view.use_bpm,
        "bpmDivision": view.bpm_division,
        "durationAsTotalTime": true,
        "steps": 0.0,
    });
    if let Some(times) = &view.keyframe_times
        && view.data.len() > 2
    {
        payload["keyframeTimes"] = serde_json::json!(times);
    }
    payload
}

/// The y-axis range: the module prop's own min/max, so the labels are values
/// the input can write. A degenerate range is padded by a unit.
fn draw_bounds(min: f64, max: f64) -> (f64, f64) {
    let low = if min.is_finite() { min.min(max) } else { 0.0 };
    let high = if max.is_finite() { max.max(min) } else { 1.0 };
    if (high - low).abs() <= f64::EPSILON {
        (low - 1.0, high + 1.0)
    } else {
        (low, high)
    }
}

/// Draw the graph and handle pointer input.
///
/// Without Shift the canvas edits points like the tween editor: click adds a
/// point, dragging a handle moves it, right-click removes an interior point.
/// Holding Shift switches to freehand painting, which rasterises the stroke
/// onto the fixed draw grid.
fn draw_canvas(
    ui: &mut egui::Ui,
    editor: &mut DrawCurveEditor,
    min: f64,
    max: f64,
    axis: &TweenAxis,
    dispatch: &mut bool,
) {
    let desired = vec2(ui.available_width().clamp(300.0, 760.0), 240.0);
    let (outer, response) = ui.allocate_exact_size(desired, Sense::click_and_drag());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, "Draw curve canvas"));
    let bounds = draw_bounds(min, max);
    let plot = Rect::from_min_max(outer.min + vec2(38.0, 8.0), outer.max - vec2(8.0, 20.0));

    let pointer = ui.input(|input| input.pointer.interact_pos());
    let shift = ui.input(|input| input.modifiers.shift);
    let primary_pressed = ui.input(|input| input.pointer.primary_pressed());
    let primary_down = ui.input(|input| input.pointer.primary_down());
    let primary_released = ui.input(|input| input.pointer.primary_released());
    let secondary_clicked = ui.input(|input| input.pointer.secondary_clicked());

    let position = pointer.map(|pointer| {
        (
            x_to_progress(pointer.x, plot),
            y_to_value(pointer.y, bounds, plot),
        )
    });

    if let (Some(pointer), Some(position)) = (pointer, position) {
        if secondary_clicked
            && response.hovered()
            && plot.contains(pointer)
            && let Some(index) = editor.curve.nearest_point(pointer, bounds, plot, 9.0)
            && editor.curve.remove_point(index)
        {
            *dispatch = true;
        }

        if primary_pressed && response.hovered() && plot.contains(pointer) {
            if shift {
                editor.curve.paint(position, position);
                editor.stroke = Some(position);
                editor.changed = true;
            } else if let Some(index) = editor.curve.nearest_point(pointer, bounds, plot, 9.0) {
                editor.dragging = Some(index);
                editor.pending_press = None;
            } else {
                editor.pending_press = Some(pointer);
            }
        } else if primary_down {
            if let Some(previous) = editor.stroke {
                if previous != position {
                    editor.curve.paint(previous, position);
                    editor.stroke = Some(position);
                    editor.changed = true;
                }
                editor.stroke = Some(position);
            } else if let Some(index) = editor.dragging
                && editor.curve.drag_point(index, position.0, position.1)
            {
                editor.changed = true;
            }
        }
    }

    if primary_released {
        let painted = editor.stroke.take().is_some();
        let dragged = editor.dragging.take().is_some();
        let clicked = if !painted && !dragged {
            let start = editor.pending_press.take();
            if let (Some(start), Some(pointer), Some(position)) = (start, pointer, position)
                && start.distance(pointer) <= 4.0
                && editor.curve.insert_point(position.0, position.1).is_some()
            {
                editor.changed = true;
                true
            } else {
                false
            }
        } else {
            editor.pending_press = None;
            false
        };
        if (painted || dragged || clicked) && std::mem::take(&mut editor.changed) {
            *dispatch = true;
        }
    }

    paint_graph(
        ui,
        outer,
        plot,
        editor,
        bounds,
        axis,
        editor.dragging.is_some() || !shift,
    );
}

/// Paint the grid, axes, joined points and handles.
fn paint_graph(
    ui: &egui::Ui,
    outer: Rect,
    plot: Rect,
    editor: &DrawCurveEditor,
    bounds: (f64, f64),
    axis: &TweenAxis,
    show_points: bool,
) {
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, egui::CornerRadius::same(0), theme::INPUT);
    painter.rect_stroke(
        outer,
        egui::CornerRadius::same(0),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let tick_font = FontId::proportional(11.0);
    let last_tick = axis.ticks.len().saturating_sub(1);
    for (index, tick) in axis.ticks.iter().enumerate() {
        let progress = if axis.total > 0.0 {
            tick / axis.total
        } else {
            0.0
        };
        let x = progress_to_x(progress, plot);
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(0.5, theme::BORDER),
        );
        // Keep the first/last label inside the canvas instead of centring
        // half of it past the edge.
        let align = if index == 0 {
            Align2::LEFT_TOP
        } else if index == last_tick {
            Align2::RIGHT_TOP
        } else {
            Align2::CENTER_TOP
        };
        painter.text(
            pos2(x, plot.bottom() + 3.0),
            align,
            axis.label(*tick),
            tick_font.clone(),
            theme::MUTED,
        );
    }
    for index in 0..=4 {
        let value = bounds.0 + (bounds.1 - bounds.0) * index as f64 / 4.0;
        let y = value_to_y(value, bounds, plot);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(0.5, theme::BORDER),
        );
        painter.text(
            pos2(plot.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            format!("{value:.2}"),
            tick_font.clone(),
            theme::MUTED,
        );
    }

    if editor.curve.values.is_empty() {
        return;
    }
    let points: Vec<egui::Pos2> = (0..editor.curve.rows())
        .map(|index| editor.curve.point_pos(index, bounds, plot))
        .collect();
    painter.add(egui::Shape::line(
        points.clone(),
        Stroke::new(1.8, theme::ACCENT),
    ));

    if !show_points {
        return;
    }
    let hovered = ui
        .input(|input| input.pointer.interact_pos())
        .and_then(|pointer| editor.curve.nearest_point(pointer, bounds, plot, 9.0));
    for (index, point) in points.into_iter().enumerate() {
        let radius = if hovered == Some(index) { 5.5 } else { 3.5 };
        painter.circle_filled(point, radius, theme::ACCENT);
        painter.circle_stroke(point, radius, Stroke::new(1.0, theme::BACKGROUND));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UiState;
    use crate::widgets::draw_curve::DRAW_POINTS;

    fn two_row() -> TweenView {
        TweenView {
            id: "input-1".to_string(),
            data: vec![vec![0.0], vec![1.0]],
            duration: 1000.0,
            direction: "normal".to_string(),
            easing: "linear".to_string(),
            looping: false,
            use_bpm: false,
            ..Default::default()
        }
    }

    #[test]
    fn create_emits_a_flat_two_point_descriptor_and_a_draw_link() {
        let mut state = UiState::new();
        let mut control = DrawControl::new("gain", Some("input-1".to_string()), None, 0.0, 1.0);
        control.create(&mut state, 120.0);

        let actions = state.take_actions();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, "tweens/createTween");
        let payload = &actions[0].payload;
        assert_eq!(payload["id"], "input-1");
        assert_eq!(payload["direction"], "normal");
        assert_eq!(payload["easing"], "linear");
        assert_eq!(payload["loop"], true);
        assert_eq!(payload["useBpm"], true);
        assert_eq!(payload["durationAsTotalTime"], true);
        assert_eq!(
            payload["data"],
            serde_json::json!([[0.5], [0.5]]),
            "a new draw is the flat two-point line point-editing starts from"
        );
        assert!(
            payload.get("keyframeTimes").is_none(),
            "two rows use the [from, to] shorthand"
        );
        assert!(
            (division_to_beats(payload["bpmDivision"].as_f64().expect("division")) - 4.0).abs()
                < 1e-9,
            "the default cycle spans four beats"
        );

        assert_eq!(actions[1].action, "inputs/createInputLink");
        assert_eq!(actions[1].payload["source"], "draw");
        assert_eq!(
            actions[1].payload["location"],
            "tweens.tweens['input-1'].value[0]"
        );
        assert_eq!(actions[1].payload["min"], 0);
        assert_eq!(actions[1].payload["max"], 1);
        assert!(control.curve.is_some());
        assert!(
            state.draw_editor.is_some(),
            "starting a draw opens the curve canvas"
        );
    }

    #[test]
    fn a_draw_curve_descriptor_opts_into_restart_loops() {
        // A newly created draw curve must opt out of the ping-pong recorder.
        let mut state = UiState::new();
        let mut control = DrawControl::new("gain", Some("input-1".to_string()), None, 0.0, 1.0);
        control.create(&mut state, 120.0);

        let actions = state.take_actions();
        assert_eq!(actions[0].action, "tweens/createTween");
        assert_eq!(actions[0].payload["loopMode"], "restart");

        // Re-applying a painted curve to an existing draw must keep the same
        // restart semantics.
        let mut view = two_row();
        view.looping = true;
        let mut state = UiState::new();
        let mut control = DrawControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(view.clone()),
            0.0,
            1.0,
        );
        let mut editor = DrawCurveEditor::new(
            "input-1",
            "gain",
            DrawCurve::from_view(&view, 0.0, 1.0).expect("curve"),
        );
        editor.curve.paint((0.25, 0.1), (0.75, 0.9));
        control.apply_curve(&mut state, &mut editor, 120.0);

        let actions = state.take_actions();
        assert_eq!(actions[0].action, "tweens/createTween");
        assert_eq!(actions[0].payload["loopMode"], "restart");
    }

    #[test]
    fn the_draw_view_uses_the_live_tempo_for_its_cycle() {
        let view = draw_view("input-1".to_string(), DrawCurve::new(0.0, 1.0), 120.0);
        assert!((view.cycle_seconds - 2.0).abs() < 1e-12);
        assert!((division_to_beats(view.bpm_division) - 4.0).abs() < 1e-12);

        let view = draw_view("input-1".to_string(), DrawCurve::new(0.0, 1.0), 0.0);
        assert!((view.cycle_seconds - 2.0).abs() < 1e-12, "0 BPM uses 120");
    }

    #[test]
    fn a_cycle_of_64_beats_is_64_on_the_axis_and_is_capped_there() {
        let mut view = draw_view("input-1".to_string(), DrawCurve::new(0.0, 1.0), 120.0);
        view.bpm_division = beats_to_division(64.0);
        let seconds = cycle_seconds(&view, 120.0);
        assert!((seconds - 32.0).abs() < 1e-9, "64 beats at 120 BPM");
        let axis = tween_axis(true, 120.0, seconds);
        assert!((axis.total - 64.0).abs() < 1e-9);
        assert!(axis.title().contains("64.00 beats"));

        // Editing through the control clamps an out-of-range descriptor back
        // into the 1–64 beat window.
        let mut state = UiState::new();
        let mut control =
            DrawControl::new("gain", Some("input-1".to_string()), Some(view), 0.0, 1.0);
        let mut huge = control.curve.clone().expect("view");
        huge.bpm_division = beats_to_division(128.0);
        control.update(&mut state, huge);
        let payload = &state.take_actions()[0].payload;
        assert!(
            (division_to_beats(payload["bpmDivision"].as_f64().expect("division")) - 64.0).abs()
                < 1e-9
        );
    }

    #[test]
    fn applying_a_painted_curve_rewrites_the_descriptor_and_keeps_the_link() {
        let mut state = UiState::new();
        let mut control = DrawControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(two_row()),
            0.0,
            1.0,
        );
        let mut editor = DrawCurveEditor::new(
            "input-1",
            "gain",
            DrawCurve::from_view(&two_row(), 0.0, 1.0).expect("curve"),
        );
        editor.curve.paint((0.25, 0.1), (0.75, 0.9));
        control.apply_curve(&mut state, &mut editor, 120.0);

        let actions = state.take_actions();
        assert_eq!(actions[0].action, "tweens/createTween");
        assert_eq!(
            actions[0].payload["data"].as_array().map(Vec::len),
            Some(DRAW_POINTS)
        );
        assert_eq!(
            actions[0].payload["keyframeTimes"].as_array().map(Vec::len),
            Some(DRAW_POINTS)
        );
        assert_eq!(actions[1].action, "inputs/createInputLink");
        assert_eq!(actions[1].payload["source"], "draw");
        let view = control.curve.as_ref().expect("view");
        assert_eq!(view.data.len(), DRAW_POINTS);
        assert!((editor.curve.sample(0.25) - 0.1).abs() < 0.01);
    }

    #[test]
    fn remove_emits_both_removals_and_closes_the_editor() {
        let mut state = UiState::new();
        let mut control = DrawControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(two_row()),
            0.0,
            1.0,
        );
        state.draw_editor = Some(DrawCurveEditor::new(
            "input-1",
            "gain",
            DrawCurve::new(0.0, 1.0),
        ));
        control.remove(&mut state);
        let actions = state.take_actions();
        assert_eq!(actions[0].action, "tweens/REMOVE_TWEEN");
        assert_eq!(actions[1].action, "inputs/removeInputLink");
        assert!(control.curve.is_none());
        assert!(state.draw_editor.is_none());
    }

    #[test]
    fn opening_the_editor_preserves_an_existing_two_point_curve() {
        let mut state = UiState::new();
        let mut control = DrawControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(two_row()),
            0.0,
            1.0,
        );
        control.open_curve_editor(&mut state);
        let editor = state.draw_editor.as_ref().expect("editor");
        assert_eq!(editor.input_id, "input-1");
        assert_eq!(
            editor.curve.values.len(),
            2,
            "a sparse draw stays editable point-by-point"
        );
        assert!((editor.curve.sample(0.5) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn inserting_a_point_in_the_editor_dispatches_three_rows_with_times() {
        let mut state = UiState::new();
        let mut control = DrawControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(two_row()),
            0.0,
            1.0,
        );
        let mut editor = DrawCurveEditor::new(
            "input-1",
            "gain",
            DrawCurve::from_view(&two_row(), 0.0, 1.0).expect("curve"),
        );
        assert_eq!(editor.curve.insert_point(0.25, 0.75), Some(1));
        control.apply_curve(&mut state, &mut editor, 120.0);

        let payload = &state.take_actions()[0].payload;
        assert_eq!(payload["data"], serde_json::json!([[0.0], [0.75], [1.0]]));
        assert_eq!(
            payload["keyframeTimes"],
            serde_json::json!([0.0, 0.25, 1.0])
        );
    }

    #[test]
    fn the_y_axis_is_the_prop_range() {
        assert_eq!(draw_bounds(0.0, 1.0), (0.0, 1.0));
        assert_eq!(draw_bounds(1.0, 0.0), (0.0, 1.0));
        assert_eq!(draw_bounds(2.0, 2.0), (1.0, 3.0));
        assert_eq!(draw_bounds(f64::NAN, 4.0), (0.0, 4.0));
    }

    #[test]
    fn a_draw_edit_always_keeps_the_descriptor_linear_and_normal() {
        let mut state = UiState::new();
        let mut control = DrawControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(two_row()),
            0.0,
            1.0,
        );
        let mut edited = two_row();
        edited.direction = "alternate".to_string();
        edited.easing = "easeOutBounce".to_string();
        edited.duration_as_total_time = false;
        control.update(&mut state, edited);

        let actions = state.take_actions();
        let payload = &actions[0].payload;
        assert_eq!(payload["direction"], "normal");
        assert_eq!(payload["easing"], "linear");
        assert_eq!(payload["durationAsTotalTime"], true);
        assert_eq!(actions[1].action, "inputs/createInputLink");
        assert_eq!(actions[1].payload["source"], "draw");
    }
}
