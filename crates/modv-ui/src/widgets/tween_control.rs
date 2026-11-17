//! Tween control: create/edit the descriptor that drives a prop's input link.
//!
//! The compact rows edit the descriptor fields the legacy `TweenControl.vue`
//! exposed (easing, direction, BPM division, loop, steps) plus the fields the
//! native editor needed (duration, BPM, total time). "Edit curve" opens a
//! keyframe editor window: overlaid channel polylines with draggable handles,
//! a beats-or-seconds axis driven by the engine's real cycle length, and the
//! Electron-style JSON underneath.
//!
//! Keyframes are dispatched with the native `keyframeTimes` extension. A tween
//! that still uses anime's evenly spaced multi-keyframe shape is normalised on
//! its first curve edit (a duplicate first row plus the even grid), and its
//! duration is re-expressed as a total so the visible cycle does not change.
//! Two-row tweens keep Electron's `[from, to]` shorthand.

use egui::{Align2, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::layout::WINDOW_TWEEN_CURVE;
use crate::model::TweenView;
use crate::state::{TweenCurveEditor, TweenHandleDrag, UiState};
use crate::theme;
use crate::widgets::tween_curve::{
    CHANNEL_COLORS, TweenAxis, TweenCurve, canvas_size, handle_pos, nearest_handle,
    parse_tween_json, progress_to_x, tween_axis, value_to_y, x_to_progress, y_to_value,
};

/// The editor's current descriptor values (persisted through
/// `tweens/createTween`, which upserts by id).
#[derive(Clone, Debug, PartialEq)]
pub struct TweenControl {
    pub label: String,
    pub input_id: Option<String>,
    pub tween: Option<TweenView>,
    pub min: f64,
    pub max: f64,
}

const EASINGS: &[&str] = &[
    "linear",
    "easeInSine",
    "easeOutSine",
    "easeInOutSine",
    "easeInQuad",
    "easeOutQuad",
    "easeInOutQuad",
    "easeInOutCubic",
    "easeInOutQuart",
    "easeInOutQuint",
    "easeOutElastic",
    "easeOutBounce",
];

impl TweenControl {
    /// The state path `Tween.vue` links for a tween descriptor.
    pub fn link_location(input_id: &str) -> String {
        format!("tweens.tweens['{input_id}'].value[0]")
    }

    pub fn new(
        label: impl Into<String>,
        input_id: Option<String>,
        tween: Option<TweenView>,
        min: f64,
        max: f64,
    ) -> Self {
        Self {
            label: label.into(),
            input_id,
            tween,
            min,
            max,
        }
    }

    /// Draw the tween editor. Changes are queued into `state.actions`.
    pub fn draw(&mut self, ui: &mut egui::Ui, state: &mut UiState) {
        let mut open_curve = false;
        let mut edit: Option<TweenView> = None;

        ui.horizontal_wrapped(|ui| {
            ui.add_sized(
                [82.0, 18.0],
                egui::Label::new(egui::RichText::new(&self.label).size(13.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(&self.label);
            match (&self.input_id, &self.tween) {
                (None, _) => {
                    ui.label(
                        egui::RichText::new("no input (module has no $props entry)")
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                }
                (Some(_), None) => {
                    if ui.button("Create tween").clicked() {
                        self.create(state);
                    }
                }
                (Some(_), Some(view)) => {
                    if ui.button("Edit curve").clicked() {
                        open_curve = true;
                    }
                    let mut easing = view.easing.clone();
                    egui::ComboBox::from_id_salt(("tween-easing", &self.label))
                        .selected_text(truncate(&easing, 14))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for option in EASINGS {
                                ui.selectable_value(&mut easing, option.to_string(), *option);
                            }
                        });
                    let mut direction = view.direction.clone();
                    egui::ComboBox::from_id_salt(("tween-direction", &self.label))
                        .selected_text(&direction)
                        .width(84.0)
                        .show_ui(ui, |ui| {
                            for option in ["normal", "reverse", "alternate"] {
                                ui.selectable_value(&mut direction, option.to_string(), option);
                            }
                        });
                    let mut division = view.bpm_division;
                    let division_changed = ui
                        .add(
                            egui::DragValue::new(&mut division)
                                .range(1.0..=128.0)
                                .speed(1.0)
                                .prefix("1/"),
                        )
                        .changed();
                    let mut looping = view.looping;
                    let loop_changed = ui.checkbox(&mut looping, "loop").changed();
                    let mut steps = view.steps;
                    let steps_changed = ui
                        .add(
                            egui::DragValue::new(&mut steps)
                                .range(0.0..=32.0)
                                .speed(1.0)
                                .prefix("steps "),
                        )
                        .changed();
                    if easing != view.easing
                        || direction != view.direction
                        || division_changed
                        || loop_changed
                        || steps_changed
                    {
                        let mut edited = view.clone();
                        edited.easing = easing;
                        edited.direction = direction;
                        edited.bpm_division = division;
                        edited.looping = looping;
                        edited.steps = steps;
                        edit = Some(edited);
                    }
                    if ui.small_button("x").on_hover_text("Remove tween").clicked() {
                        self.remove(state);
                    }
                }
            }
        });

        if let Some(view) = edit.take() {
            self.update(state, view);
        }

        if let Some(view) = self.tween.clone() {
            ui.horizontal_wrapped(|ui| {
                let mut use_bpm = view.use_bpm;
                let bpm_changed = ui
                    .checkbox(&mut use_bpm, "BPM")
                    .on_hover_text("Time the cycle by tempo instead of the millisecond duration")
                    .changed();
                let mut duration = view.duration;
                let duration_changed = ui
                    .add_enabled(
                        !use_bpm,
                        egui::DragValue::new(&mut duration)
                            .range(1.0..=600_000.0)
                            .speed(10.0)
                            .prefix("dur ")
                            .suffix(" ms"),
                    )
                    .changed();
                let mut total = view.duration_as_total_time;
                let total_changed = ui
                    .checkbox(&mut total, "total")
                    .on_hover_text("Treat the duration as the whole cycle, not one segment per row")
                    .changed();
                ui.label(
                    egui::RichText::new(format!("{} frames", view.frames))
                        .size(12.0)
                        .color(theme::MUTED),
                )
                .on_hover_text("Pre-rendered playback frames (read-only)");
                if bpm_changed || duration_changed || total_changed {
                    let mut edited = view.clone();
                    edited.use_bpm = use_bpm;
                    edited.duration = duration;
                    edited.duration_as_total_time = total;
                    edit = Some(edited);
                }
            });
            if let Some(edit) = edit.take() {
                self.update(state, edit);
            }
        }

        if open_curve {
            self.open_curve_editor(state);
        }
        self.draw_curve_editor(ui.ctx(), state);
    }

    /// Open the curve editor for the current descriptor.
    fn open_curve_editor(&mut self, state: &mut UiState) {
        let (Some(input_id), Some(view)) = (self.input_id.clone(), self.tween.clone()) else {
            return;
        };
        let Some(curve) = TweenCurve::from_view(&view) else {
            return;
        };
        if state
            .tween_editor
            .as_ref()
            .is_some_and(|editor| editor.input_id == input_id && editor.open)
        {
            return;
        }
        state.tween_editor = Some(TweenCurveEditor::new(input_id, self.label.clone(), curve));
    }

    fn create(&mut self, state: &mut UiState) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        let view = TweenView {
            id: input_id,
            data: vec![vec![self.min], vec![self.max]],
            keyframe_times: None,
            duration: 1000.0,
            direction: "alternate".to_string(),
            easing: "easeInOutSine".to_string(),
            looping: true,
            use_bpm: true,
            bpm_division: 16.0,
            duration_as_total_time: false,
            steps: 0.0,
            frames: 0,
            cycle_seconds: 0.0,
        };
        self.update(state, view);
    }

    /// Dispatch a descriptor edit and mirror it into the local view.
    fn update(&mut self, state: &mut UiState, mut view: TweenView) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        view.id = input_id.clone();
        state.push_action("tweens/createTween", descriptor_payload(&view));
        state.push_action(
            "inputs/createInputLink",
            serde_json::json!({
                "inputId": input_id,
                "type": "state",
                "source": "tween",
                "location": Self::link_location(&input_id),
                "min": 0,
                "max": 1,
            }),
        );
        self.tween = Some(view);
    }

    /// Dispatch an edited curve.
    ///
    /// A row-count change re-expresses the duration as the total cycle so the
    /// playback speed is preserved: adding the duplicate first row (anime's
    /// leading hold) or adding/removing a keyframe must not shorten or stretch
    /// the tween.
    fn apply_curve(&mut self, state: &mut UiState, editor: &mut TweenCurveEditor) {
        let Some(mut view) = self.tween.clone() else {
            return;
        };
        let rows_before = view.data.len();
        if editor.curve.rows() != rows_before && !view.duration_as_total_time {
            view.duration_as_total_time = true;
            let rows = rows_before.max(1) as f64;
            if view.use_bpm {
                view.bpm_division /= rows;
            } else {
                view.duration /= rows;
            }
        }
        view.data = editor.curve.data.clone();
        view.keyframe_times = editor
            .curve
            .explicit_times
            .then(|| editor.curve.times.clone());
        self.update(state, view);
    }

    /// Draw the open curve editor window, mirroring its curve into the
    /// descriptor on edits.
    fn draw_curve_editor(&mut self, ctx: &egui::Context, state: &mut UiState) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        let Some(mut editor) = state.tween_editor.take() else {
            return;
        };
        if editor.input_id != input_id || !editor.open {
            state.tween_editor = Some(editor);
            return;
        }
        let Some(view) = self.tween.clone() else {
            // The tween went away (removed here or by a Scene apply).
            return;
        };
        if editor.dragging.is_none()
            && !editor.json_dirty
            && !TweenCurve::from_view(&view).is_some_and(|current| current == editor.curve)
            && let Some(curve) = TweenCurve::from_view(&view)
        {
            editor.curve = curve;
            editor.json = editor.curve.to_json();
        }

        let axis = tween_axis(view.use_bpm, state.status.bpm, view.cycle_seconds);
        let mut open = editor.open;
        let mut dispatch = false;
        let mut json_edited = false;
        let min = self.min;
        let max = self.max;

        let window = state.layout.apply_window(
            egui::Window::new(format!("Tween curve · {}", editor.title))
                .id(egui::Id::unique("modv-window:tween_curve"))
                .open(&mut open)
                .default_pos([220.0, 90.0])
                .default_size([430.0, 430.0])
                .min_width(320.0)
                .min_height(300.0)
                .resizable(true),
            WINDOW_TWEEN_CURVE,
        );
        let response = window.show(ctx, |ui| {
            ui.label(
                egui::RichText::new(axis.title())
                    .size(12.0)
                    .color(theme::MUTED),
            );
            curve_canvas(ui, &mut editor, min, max, &axis, &mut dispatch);
            ui.horizontal_wrapped(|ui| {
                for (channel, colour) in CHANNEL_COLORS
                    .iter()
                    .cycle()
                    .take(editor.curve.channels())
                    .enumerate()
                {
                    ui.label(
                        egui::RichText::new(format!("ch {channel}"))
                            .size(12.0)
                            .color(*colour),
                    );
                }
                ui.label(
                    egui::RichText::new("click: add · drag: move · right-click: remove")
                        .size(12.0)
                        .color(theme::MUTED),
                );
            });
            if !editor.json_dirty {
                editor.json = editor.curve.to_json();
            }
            let response = ui.add(
                egui::TextEdit::multiline(&mut editor.json)
                    .desired_width(f32::INFINITY)
                    .desired_rows(3)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("{\"data\": [[0], [1]]}"),
            );
            if response.changed() {
                json_edited = true;
                match parse_tween_json(&editor.json) {
                    Ok(parsed) => match TweenCurve::from_rows(parsed.data, parsed.keyframe_times) {
                        Some(curve) => {
                            editor.curve = curve;
                            editor.error = None;
                            dispatch = true;
                            editor.json_dirty = true;
                        }
                        None => {
                            editor.error =
                                Some("keyframes need two rows of equal length".to_string())
                        }
                    },
                    Err(error) => editor.error = Some(error),
                }
            }
            if let Some(error) = &editor.error {
                ui.colored_label(theme::DANGER, error);
            }
        });

        if let Some(response) = response {
            state
                .layout
                .record_window_rect(WINDOW_TWEEN_CURVE, response.response.rect);
        }
        editor.open = open;
        if dispatch {
            self.apply_curve(state, &mut editor);
        }
        if json_edited {
            // Keep the user's text (and cursor) while they type.
            editor.json_dirty = true;
        } else if dispatch {
            editor.json_dirty = false;
            editor.json = editor.curve.to_json();
        }
        if editor.open {
            state.tween_editor = Some(editor);
        }
    }

    fn remove(&mut self, state: &mut UiState) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        state.push_action("tweens/REMOVE_TWEEN", serde_json::json!({ "id": input_id }));
        state.push_action(
            "inputs/removeInputLink",
            serde_json::json!({ "inputId": input_id }),
        );
        self.tween = None;
        if state
            .tween_editor
            .as_ref()
            .is_some_and(|editor| editor.input_id == input_id)
        {
            state.tween_editor = None;
        }
    }
}

/// Draw the graph and handle its interactions, updating `editor.curve` in
/// place and flagging `dispatch` on a completed edit.
fn curve_canvas(
    ui: &mut egui::Ui,
    editor: &mut TweenCurveEditor,
    min: f64,
    max: f64,
    axis: &TweenAxis,
    dispatch: &mut bool,
) {
    let (outer, response) = ui.allocate_exact_size(canvas_size(), Sense::click_and_drag());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, "Tween curve canvas"));
    let bounds = editor.curve.value_bounds(min, max);
    let plot = Rect::from_min_max(outer.min + vec2(38.0, 8.0), outer.max - vec2(8.0, 20.0));

    let pointer = ui.input(|input| input.pointer.interact_pos());
    let primary_pressed = ui.input(|input| input.pointer.primary_pressed());
    let primary_down = ui.input(|input| input.pointer.primary_down());
    let primary_released = ui.input(|input| input.pointer.primary_released());
    let secondary_clicked = ui.input(|input| input.pointer.secondary_clicked());

    if let Some(pointer) = pointer {
        if response.hovered() {
            if secondary_clicked
                && let Some((index, _)) = nearest_handle(&editor.curve, pointer, bounds, plot, 9.0)
            {
                let _ = editor.curve.remove_point(index);
            }

            if primary_pressed {
                if let Some((index, channel)) =
                    nearest_handle(&editor.curve, pointer, bounds, plot, 9.0)
                {
                    editor.dragging = Some(TweenHandleDrag { index, channel });
                    editor.pending_press = None;
                } else {
                    editor.pending_press = Some(pointer);
                }
            }
        }

        if primary_down && let Some(drag) = editor.dragging {
            let time = x_to_progress(pointer.x, plot);
            let _ = editor.curve.drag_time(drag.index, time);
            let value = y_to_value(pointer.y, bounds, plot);
            let _ = editor.curve.set_value(drag.index, drag.channel, value);
        }

        if primary_released {
            if editor.dragging.take().is_some() {
                // The curve was already updated on the previous frames of the
                // drag; the release is when the descriptor is queued.
                *dispatch = true;
            } else if let Some(start) = editor.pending_press.take()
                && start.distance(pointer) <= 4.0
            {
                let time = x_to_progress(pointer.x, plot);
                let value = y_to_value(pointer.y, bounds, plot);
                let channel = editor.curve.nearest_channel(time, value);
                if editor.curve.insert_point(time, value, channel).is_some() {
                    *dispatch = true;
                }
            }
        }
    }

    draw_curve(ui, outer, plot, editor, bounds, axis);
}

/// Paint the grid, axes, channel polylines and handles.
fn draw_curve(
    ui: &egui::Ui,
    outer: Rect,
    plot: Rect,
    editor: &TweenCurveEditor,
    bounds: (f64, f64),
    axis: &TweenAxis,
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
    for tick in &axis.ticks {
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
        painter.text(
            pos2(x, plot.bottom() + 3.0),
            Align2::CENTER_TOP,
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

    let hovered = ui
        .input(|input| input.pointer.interact_pos())
        .and_then(|pointer| nearest_handle(&editor.curve, pointer, bounds, plot, 9.0));

    for (channel, colour) in CHANNEL_COLORS
        .iter()
        .cycle()
        .take(editor.curve.channels())
        .enumerate()
    {
        let colour = *colour;
        let points: Vec<egui::Pos2> = (0..editor.curve.rows())
            .map(|index| handle_pos(&editor.curve, index, channel, bounds, plot))
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, colour)));
        for (index, point) in (0..editor.curve.rows()).map(|index| {
            (
                index,
                handle_pos(&editor.curve, index, channel, bounds, plot),
            )
        }) {
            let radius = if hovered == Some((index, channel)) {
                6.0
            } else {
                4.0
            };
            painter.circle_filled(point, radius, colour);
            painter.circle_stroke(point, radius, Stroke::new(1.0, theme::BACKGROUND));
        }
    }
}

/// The `tweens/createTween` payload for a descriptor.
///
/// `keyframeTimes` is emitted only for explicit times (three or more rows);
/// a two-row `[0, 1]` list would be redundant with anime's shorthand.
fn descriptor_payload(view: &TweenView) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "id": view.id,
        "data": view.data,
        "duration": view.duration,
        "direction": view.direction,
        "easing": view.easing,
        "loop": view.looping,
        "useBpm": view.use_bpm,
        "bpmDivision": view.bpm_division,
        "durationAsTotalTime": view.duration_as_total_time,
        "steps": view.steps,
    });
    if let Some(times) = &view.keyframe_times
        && view.data.len() > 2
    {
        payload["keyframeTimes"] = serde_json::json!(times);
    }
    payload
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('~');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::TweenCurve;
    use crate::{TweenView, UiState};

    fn two_row() -> TweenView {
        TweenView {
            id: "input-1".to_string(),
            data: vec![vec![0.0], vec![1.0]],
            duration: 1000.0,
            direction: "alternate".to_string(),
            easing: "linear".to_string(),
            looping: true,
            use_bpm: false,
            bpm_division: 16.0,
            ..Default::default()
        }
    }

    fn even_three_row() -> TweenView {
        TweenView {
            data: vec![vec![0.0], vec![0.5], vec![1.0]],
            ..two_row()
        }
    }

    fn editor_for(view: &TweenView) -> TweenCurveEditor {
        TweenCurveEditor::new(
            "input-1",
            "gain",
            TweenCurve::from_view(view).expect("curve"),
        )
    }

    #[test]
    fn create_tween_emits_the_full_descriptor_and_state_link() {
        let mut state = UiState::new();
        let mut control = TweenControl::new("gain", Some("input-1".to_string()), None, 0.0, 1.0);
        control.create(&mut state);
        let actions = state.take_actions();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, "tweens/createTween");
        assert_eq!(actions[0].payload["id"], "input-1");
        assert_eq!(
            actions[0].payload["data"],
            serde_json::json!([[0.0], [1.0]])
        );
        assert_eq!(actions[0].payload["duration"], 1000.0);
        assert_eq!(actions[0].payload["direction"], "alternate");
        assert_eq!(actions[0].payload["easing"], "easeInOutSine");
        assert_eq!(actions[0].payload["loop"], true);
        assert_eq!(actions[0].payload["useBpm"], true);
        assert_eq!(actions[0].payload["bpmDivision"], 16.0);
        assert_eq!(actions[0].payload["durationAsTotalTime"], false);
        assert_eq!(actions[0].payload["steps"], 0.0);
        assert!(actions[0].payload.get("keyframeTimes").is_none());
        assert_eq!(actions[1].action, "inputs/createInputLink");
        assert_eq!(actions[1].payload["source"], "tween");
        assert_eq!(
            actions[1].payload["location"],
            "tweens.tweens['input-1'].value[0]"
        );
        assert!(control.tween.is_some());
    }

    #[test]
    fn first_curve_edit_normalises_even_keyframes_and_keeps_the_cycle() {
        let mut state = UiState::new();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(even_three_row()),
            0.0,
            1.0,
        );
        let mut editor = editor_for(control.tween.as_ref().unwrap());

        // Dragging an interior handle is the first edit.
        assert!(editor.curve.drag_time(1, 0.5));
        control.apply_curve(&mut state, &mut editor);

        let actions = state.take_actions();
        assert_eq!(actions[0].action, "tweens/createTween");
        let payload = &actions[0].payload;
        assert_eq!(
            payload["data"],
            serde_json::json!([[0.0], [0.0], [0.5], [1.0]]),
            "a duplicate first row expresses anime's leading hold"
        );
        assert_eq!(
            payload["keyframeTimes"],
            serde_json::json!([0.0, 0.5, 2.0 / 3.0, 1.0])
        );
        let view = control.tween.as_ref().unwrap();
        assert!(view.duration_as_total_time);
        assert!((view.duration - 1000.0 / 3.0).abs() < 1e-12);
        assert_eq!(view.keyframe_times, Some(vec![0.0, 0.5, 2.0 / 3.0, 1.0]));
    }

    #[test]
    fn first_curve_edit_scales_a_bpm_division_instead_of_the_duration() {
        let mut state = UiState::new();
        let view = TweenView {
            use_bpm: true,
            bpm_division: 16.0,
            ..even_three_row()
        };
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(view.clone()),
            0.0,
            1.0,
        );
        let mut editor = editor_for(&view);
        assert!(editor.curve.set_value(1, 0, 0.25));
        control.apply_curve(&mut state, &mut editor);

        let view = control.tween.as_ref().unwrap();
        assert!(view.duration_as_total_time);
        assert!((view.bpm_division - 16.0 / 3.0).abs() < 1e-12);
        assert_eq!(view.duration, 1000.0);
        let payload = &state.take_actions()[0].payload;
        assert_eq!(payload["useBpm"], true);
        assert_eq!(payload["durationAsTotalTime"], true);
    }

    #[test]
    fn editing_a_two_row_tween_keeps_the_electron_shorthand() {
        let mut state = UiState::new();
        let view = two_row();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(view.clone()),
            0.0,
            1.0,
        );
        let mut editor = editor_for(&view);
        assert!(editor.curve.set_value(1, 0, 0.75));
        control.apply_curve(&mut state, &mut editor);

        let payload = &state.take_actions()[0].payload;
        assert_eq!(payload["data"], serde_json::json!([[0.0], [0.75]]));
        assert!(payload.get("keyframeTimes").is_none());
        let view = control.tween.as_ref().unwrap();
        assert!(!view.duration_as_total_time);
        assert_eq!(view.duration, 1000.0);
    }

    #[test]
    fn descriptor_edits_keep_the_saved_keyframes_and_times() {
        let mut state = UiState::new();
        let view = TweenView {
            keyframe_times: Some(vec![0.0, 0.35, 1.0]),
            ..even_three_row()
        };
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(view.clone()),
            0.0,
            1.0,
        );
        let mut edited = view.clone();
        edited.easing = "easeOutQuad".to_string();
        edited.duration = 2500.0;
        edited.use_bpm = false;
        edited.duration_as_total_time = true;
        control.update(&mut state, edited.clone());

        let payload = &state.take_actions()[0].payload;
        assert_eq!(payload["easing"], "easeOutQuad");
        assert_eq!(payload["duration"], 2500.0);
        assert_eq!(payload["durationAsTotalTime"], true);
        assert_eq!(
            payload["keyframeTimes"],
            serde_json::json!([0.0, 0.35, 1.0])
        );
        assert_eq!(payload["data"], serde_json::json!([[0.0], [0.5], [1.0]]));
    }

    #[test]
    fn adding_a_point_to_a_two_row_tween_preserves_the_cycle() {
        let mut state = UiState::new();
        let view = two_row();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(view.clone()),
            0.0,
            1.0,
        );
        let mut editor = editor_for(&view);
        let index = editor.curve.insert_point(0.5, 0.25, 0).expect("point");
        assert_eq!(index, 1);
        control.apply_curve(&mut state, &mut editor);

        let payload = &state.take_actions()[0].payload;
        assert_eq!(payload["data"], serde_json::json!([[0.0], [0.25], [1.0]]));
        assert_eq!(payload["keyframeTimes"], serde_json::json!([0.0, 0.5, 1.0]));
        let view = control.tween.as_ref().unwrap();
        assert!(view.duration_as_total_time);
        assert_eq!(view.duration, 500.0, "the 2-row leg length is kept");
    }

    #[test]
    fn remove_emits_both_removals_and_closes_the_editor() {
        let mut state = UiState::new();
        let view = even_three_row();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(view.clone()),
            0.0,
            1.0,
        );
        state.tween_editor = Some(editor_for(&view));
        control.remove(&mut state);
        let actions = state.take_actions();
        assert_eq!(actions[0].action, "tweens/REMOVE_TWEEN");
        assert_eq!(actions[1].action, "inputs/removeInputLink");
        assert!(control.tween.is_none());
        assert!(state.tween_editor.is_none());
    }

    #[test]
    fn missing_input_does_nothing() {
        let mut state = UiState::new();
        let mut control = TweenControl::new("gain", None, None, 0.0, 1.0);
        control.create(&mut state);
        control.update(&mut state, two_row());
        assert!(state.take_actions().is_empty());
    }

    #[test]
    fn a_valid_json_edit_replaces_the_curve_and_dispatches_it() {
        let mut state = UiState::new();
        let view = even_three_row();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(view.clone()),
            0.0,
            1.0,
        );
        let mut editor = editor_for(&view);

        // The window's JSON branch: parse, build a curve, then apply it.
        let parsed =
            parse_tween_json("{\"data\": [[0], [0.25], [1]], \"keyframeTimes\": [0, 0.1, 1]}")
                .expect("valid JSON");
        editor.curve = TweenCurve::from_rows(parsed.data, parsed.keyframe_times).expect("curve");
        assert_eq!(
            editor.curve.to_json(),
            "{\"data\": [[0], [0.25], [1]], \"keyframeTimes\": [0, 0.1, 1]}"
        );

        control.apply_curve(&mut state, &mut editor);
        let payload = &state.take_actions()[0].payload;
        assert_eq!(payload["data"], serde_json::json!([[0.0], [0.25], [1.0]]));
        assert_eq!(payload["keyframeTimes"], serde_json::json!([0.0, 0.1, 1.0]));
    }

    #[test]
    fn opening_the_editor_normalises_the_curve_and_mirrors_the_json() {
        let mut state = UiState::new();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(even_three_row()),
            0.0,
            1.0,
        );
        control.open_curve_editor(&mut state);
        let editor = state.tween_editor.as_ref().expect("editor");
        assert_eq!(editor.input_id, "input-1");
        assert_eq!(editor.curve.rows(), 4);
        assert_eq!(
            editor.json,
            "{\"data\": [[0], [0], [0.5], [1]], \"keyframeTimes\": [0, 0.3333333333333333, 0.6666666666666666, 1]}"
        );
    }
}
