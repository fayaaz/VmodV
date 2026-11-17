//! `RangeControl` — slider + drag/typed value + expression/tween badges.
//!
//! Port of `src/renderer/src/components/Controls/RangeControl.vue` (402 LOC).
//! P0 keeps the complete interaction model (slider, drag value, typed commit,
//! reset, badges) but uses egui's `DragValue` for the numeric field; P3 adds
//! the legacy scrub gesture and the full focus-request queue.

use egui::{Align2, Color32, CornerRadius, FontId, Sense, Stroke, StrokeKind};

use crate::theme;

/// A numeric control bound to one engine property.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeControl {
    pub label: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    /// Drag/typed increment; `0.0` disables quantisation.
    pub step: f64,
    pub unit: String,
    /// Expression badge (`E`) is visible.
    pub expression: bool,
    /// Tween badge (`T`) is visible.
    pub tween: bool,
    /// Uncommitted typed text while the field is being edited.
    pub text: String,
    pub editing: bool,
    pub error: Option<String>,
}

impl RangeControl {
    pub fn new(label: impl Into<String>, value: f64, min: f64, max: f64, step: f64) -> Self {
        let mut control = Self {
            label: label.into(),
            value,
            min,
            max,
            step,
            unit: String::new(),
            expression: false,
            tween: false,
            text: String::new(),
            editing: false,
            error: None,
        };
        control.set_value(value);
        control
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    pub fn with_badges(mut self, expression: bool, tween: bool) -> Self {
        self.expression = expression;
        self.tween = tween;
        self
    }

    /// Clamp `value` into the configured range. Returns whether it changed.
    pub fn set_value(&mut self, value: f64) -> bool {
        let value = if value.is_finite() {
            value.clamp(self.min.min(self.max), self.max.max(self.min))
        } else {
            self.value
        };
        if (value - self.value).abs() > f64::EPSILON {
            self.value = value;
            true
        } else {
            false
        }
    }

    /// The value as a 0..1 position.
    pub fn normalized(&self) -> f32 {
        let span = self.max - self.min;
        if span.abs() <= f64::EPSILON {
            0.0
        } else {
            ((self.value - self.min) / span).clamp(0.0, 1.0) as f32
        }
    }

    /// The typed representation, with the unit suffix.
    pub fn display_value(&self) -> String {
        let value = self.format_number(self.value);
        if self.unit.is_empty() {
            value
        } else if self.unit == "%" {
            format!("{value}%")
        } else {
            format!("{value} {}", self.unit)
        }
    }

    fn format_number(&self, value: f64) -> String {
        let decimals = if self.step >= 1.0 {
            0
        } else if self.step >= 0.01 {
            2
        } else {
            3
        };
        let text = format!("{value:.decimals$}");
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        }
    }

    /// Enter typed-edit mode with the current value selected.
    pub fn begin_edit(&mut self) {
        self.text = self.format_number(self.value);
        self.editing = true;
        self.error = None;
    }

    /// Commit the typed text. Returns whether the value changed.
    pub fn commit_text(&mut self) -> Result<bool, String> {
        let raw = self.text.trim();
        let raw = raw.strip_suffix('%').unwrap_or(raw).trim();
        let raw = if !self.unit.is_empty() && self.unit != "%" {
            raw.strip_suffix(self.unit.as_str()).unwrap_or(raw).trim()
        } else {
            raw
        };
        if raw.is_empty() {
            let message = "enter a number".to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        let value: f64 = match raw.parse() {
            Ok(value) => value,
            Err(_) => {
                let message = format!("{raw:?} is not a number");
                self.error = Some(message.clone());
                return Err(message);
            }
        };
        if !value.is_finite() {
            let message = "value must be finite".to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        let changed = self.set_value(value);
        self.editing = false;
        self.error = None;
        Ok(changed)
    }

    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.error = None;
    }

    /// Draw the control. Returns `true` when the value changed this frame.
    pub fn draw(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.add_sized(
                [86.0, 18.0],
                egui::Label::new(egui::RichText::new(&self.label).size(11.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(&self.label);

            if self.expression {
                theme::badge(ui, "E", theme::ACCENT).on_hover_text("Expression assigned");
            }
            if self.tween {
                theme::badge(ui, "T", theme::TWEEN).on_hover_text("Tween assigned");
            }

            let value_width = 66.0;
            let slider_width = (ui.available_width() - value_width - 8.0).max(48.0);
            let slider = egui::Slider::new(&mut self.value, self.min..=self.max)
                .show_value(false)
                .trailing_fill(true);
            if ui.add_sized([slider_width, 18.0], slider).changed() {
                changed = true;
            }

            let decimals = if self.step >= 1.0 {
                0
            } else if self.step >= 0.01 {
                2
            } else {
                3
            };
            let speed = if self.step > 0.0 { self.step } else { 0.001 };
            let drag = egui::DragValue::new(&mut self.value)
                .speed(speed)
                .range(self.min..=self.max)
                .max_decimals(decimals)
                .suffix(if self.unit.is_empty() {
                    String::new()
                } else {
                    format!(" {}", self.unit)
                });
            if ui
                .add_sized([value_width, 18.0], drag)
                .on_hover_text("Drag to scrub; click to type")
                .changed()
            {
                changed = true;
            }

            if ui
                .add(egui::Button::new("R").small().frame(false))
                .on_hover_text("Reset to the default")
                .clicked()
            {
                // The engine-side default is not in the read model; P3 binds
                // this to `$props[prop].default`. For P0 it returns the
                // midpoint, which is the editor's neutral value.
                self.value = (self.min + self.max) * 0.5;
                changed = true;
            }
        });

        if let Some(error) = &self.error {
            ui.colored_label(theme::DANGER, error);
        }
        changed
    }
}

/// Draw a single small status chip.
pub fn chip(ui: &mut egui::Ui, text: &str, colour: Color32) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::monospace(9.0), colour);
    let size = galley.size() + egui::vec2(8.0, 3.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), colour.gamma_multiply(0.18));
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, colour.gamma_multiply(0.5)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(9.0),
        colour,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_into_the_configured_range() {
        let mut control = RangeControl::new("gain", 10.0, 0.0, 2.0, 0.001);
        assert_eq!(control.value, 2.0);
        assert!(control.set_value(-1.0));
        assert_eq!(control.value, 0.0);
        assert!(!control.set_value(f64::NAN));
        assert_eq!(control.value, 0.0);
    }

    #[test]
    fn typed_commit_parses_units_and_clamps() {
        let mut control = RangeControl::new("gain", 0.5, 0.0, 2.0, 0.001).with_unit("dB");
        control.begin_edit();
        control.text = "1.25 dB".to_string();
        assert_eq!(control.commit_text(), Ok(true));
        assert_eq!(control.value, 1.25);

        control.begin_edit();
        control.text = "99".to_string();
        assert_eq!(control.commit_text(), Ok(true));
        assert_eq!(control.value, 2.0);

        control.begin_edit();
        control.text = "nope".to_string();
        assert!(control.commit_text().is_err());
        assert!(control.error.is_some());
        // Invalid text keeps the field open so the user can fix it.
        assert!(control.editing);
        assert_eq!(control.value, 2.0);
    }

    #[test]
    fn percentage_suffix_is_accepted() {
        let mut control = RangeControl::new("mix", 0.0, 0.0, 100.0, 0.01).with_unit("%");
        control.begin_edit();
        control.text = "42%".to_string();
        assert_eq!(control.commit_text(), Ok(true));
        assert!((control.value - 42.0).abs() < 1e-9);
    }

    #[test]
    fn display_value_trims_trailing_zeros() {
        let control = RangeControl::new("gain", 1.5, 0.0, 2.0, 0.001).with_unit("");
        assert_eq!(control.display_value(), "1.5");
        let control = RangeControl::new("gain", 1.0, 0.0, 2.0, 1.0);
        assert_eq!(control.display_value(), "1");
    }

    #[test]
    fn normalized_position_is_clamped() {
        let mut control = RangeControl::new("gain", 0.5, 0.0, 2.0, 0.001);
        assert!((control.normalized() - 0.25).abs() < 1e-6);
        control.set_value(2.0);
        assert!((control.normalized() - 1.0).abs() < 1e-6);
    }
}
