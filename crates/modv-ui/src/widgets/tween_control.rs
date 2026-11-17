//! Tween control: create/edit the descriptor that drives a prop's input link.
//!
//! The legacy 1064-LOC `TweenControl.vue` also edits keyframes and a palette;
//! P3 ships the load-bearing subset (easing, direction, BPM division, loop,
//! steps, min/max) and keeps the same engine payloads. Keyframes remain the
//! existing `[min] -> [max]` pair until the palette editor lands.

use egui::Sense;

use crate::model::TweenView;
use crate::state::UiState;
use crate::theme;

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
        ui.horizontal(|ui| {
            ui.add_sized(
                [82.0, 18.0],
                egui::Label::new(egui::RichText::new(&self.label).size(11.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(&self.label);
            match (&self.input_id, &self.tween) {
                (None, _) => {
                    ui.label(
                        egui::RichText::new("no input (module has no $props entry)")
                            .size(10.0)
                            .color(theme::MUTED),
                    );
                }
                (Some(_), None) => {
                    if ui.button("Create tween").clicked() {
                        self.create_or_update(state, None);
                    }
                }
                (Some(_), Some(tween)) => {
                    let mut easing = tween.easing.clone();
                    egui::ComboBox::from_id_salt(("tween-easing", &self.label))
                        .selected_text(truncate(&easing, 14))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for option in EASINGS {
                                ui.selectable_value(&mut easing, option.to_string(), *option);
                            }
                        });
                    let mut direction = tween.direction.clone();
                    egui::ComboBox::from_id_salt(("tween-direction", &self.label))
                        .selected_text(&direction)
                        .width(84.0)
                        .show_ui(ui, |ui| {
                            for option in ["normal", "reverse", "alternate"] {
                                ui.selectable_value(&mut direction, option.to_string(), option);
                            }
                        });
                    let mut division = tween.bpm_division;
                    let division_changed = ui
                        .add(
                            egui::DragValue::new(&mut division)
                                .range(1.0..=128.0)
                                .speed(1.0)
                                .prefix("1/"),
                        )
                        .changed();
                    let mut looping = tween.looping;
                    let loop_changed = ui.checkbox(&mut looping, "loop").changed();
                    let mut steps = tween.steps;
                    let steps_changed = ui
                        .add(
                            egui::DragValue::new(&mut steps)
                                .range(0.0..=32.0)
                                .speed(1.0)
                                .prefix("steps "),
                        )
                        .changed();
                    if easing != tween.easing
                        || direction != tween.direction
                        || division_changed
                        || loop_changed
                        || steps_changed
                    {
                        self.create_or_update(
                            state,
                            Some((easing, direction, division, looping, steps)),
                        );
                    }
                    if ui.small_button("x").on_hover_text("Remove tween").clicked() {
                        self.remove(state);
                    }
                }
            }
        });
    }

    fn create_or_update(
        &mut self,
        state: &mut UiState,
        edit: Option<(String, String, f64, bool, f64)>,
    ) {
        let Some(input_id) = self.input_id.clone() else {
            return;
        };
        let (easing, direction, division, looping, steps) = edit.unwrap_or_else(|| {
            let tween = self.tween.clone().unwrap_or_default();
            (
                if tween.easing.is_empty() {
                    "easeInOutSine".to_string()
                } else {
                    tween.easing
                },
                if tween.direction.is_empty() {
                    "alternate".to_string()
                } else {
                    tween.direction
                },
                if tween.bpm_division > 0.0 {
                    tween.bpm_division
                } else {
                    16.0
                },
                true,
                0.0,
            )
        });
        let data = serde_json::json!([[self.min], [self.max]]);
        state.push_action(
            "tweens/createTween",
            serde_json::json!({
                "id": input_id,
                "data": data,
                "direction": direction,
                "easing": easing,
                "loop": looping,
                "useBpm": true,
                "bpmDivision": division,
                "steps": steps,
            }),
        );
        state.push_action(
            "inputs/createInputLink",
            serde_json::json!({
                "inputId": input_id,
                "type": "state",
                "source": "tween",
                "location": format!("tweens.tweens['{input_id}'].value[0]"),
                "min": 0,
                "max": 1,
            }),
        );
        self.tween = Some(TweenView {
            id: input_id,
            easing,
            direction,
            bpm_division: division,
            looping,
            steps,
            ..self.tween.clone().unwrap_or_default()
        });
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
    }
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
    use crate::{TweenView, UiState};

    #[test]
    fn create_tween_emits_descriptor_and_state_link() {
        let mut state = UiState::new();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            None,
            0.0,
            1.0,
        );
        control.create_or_update(&mut state, None);
        let actions = state.take_actions();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, "tweens/createTween");
        assert_eq!(actions[0].payload["id"], "input-1");
        assert_eq!(actions[0].payload["data"], serde_json::json!([[0.0], [1.0]]));
        assert_eq!(actions[1].action, "inputs/createInputLink");
        assert_eq!(actions[1].payload["source"], "tween");
        assert_eq!(
            actions[1].payload["location"],
            "tweens.tweens['input-1'].value[0]"
        );
        assert!(control.tween.is_some());
    }

    #[test]
    fn remove_emits_both_removals() {
        let mut state = UiState::new();
        let mut control = TweenControl::new(
            "gain",
            Some("input-1".to_string()),
            Some(TweenView {
                id: "input-1".to_string(),
                ..Default::default()
            }),
            0.0,
            1.0,
        );
        control.remove(&mut state);
        let actions = state.take_actions();
        assert_eq!(actions[0].action, "tweens/REMOVE_TWEEN");
        assert_eq!(actions[1].action, "inputs/removeInputLink");
        assert!(control.tween.is_none());
    }

    #[test]
    fn missing_input_does_nothing() {
        let mut state = UiState::new();
        let mut control = TweenControl::new("gain", None, None, 0.0, 1.0);
        control.create_or_update(&mut state, None);
        assert!(state.take_actions().is_empty());
    }
}
