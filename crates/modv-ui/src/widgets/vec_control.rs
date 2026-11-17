//! Vector control (`vec2`/`vec3`/`vec4`), ported from the legacy
//! `Vec2DControl`/`Vec3Control`/`Vec4Control` main rows.
//!
//! The legacy controls expose X/Y/Z/W rows plus a 2D pad/Sketch body. P3 keeps
//! the numeric rows (the load-bearing path for presets and links) and uses an
//! inline colour picker for `vec4`, which is how ISF declares colours.

use egui::Sense;
use serde_json::Value;

use crate::theme;

/// One vector prop control.
#[derive(Clone, Debug, PartialEq)]
pub struct VecControl {
    pub label: String,
    pub values: Vec<f64>,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl VecControl {
    pub fn new(
        label: impl Into<String>,
        value: &Value,
        lanes: usize,
        min: f64,
        max: f64,
        step: f64,
    ) -> Self {
        let mut values = vec![0.0; lanes];
        if let Value::Array(items) = value {
            for (index, item) in items.iter().take(lanes).enumerate() {
                values[index] = item.as_f64().unwrap_or(0.0);
            }
        } else if let Some(number) = value.as_f64() {
            values.fill(number);
        }
        Self {
            label: label.into(),
            values,
            min,
            max,
            step,
        }
    }

    pub fn to_value(&self) -> Value {
        Value::Array(
            self.values
                .iter()
                .map(|value| serde_json::json!(value))
                .collect(),
        )
    }

    /// Draw the numeric rows. Returns `true` when a lane changed.
    pub fn draw(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.add_sized(
                [82.0, 18.0],
                egui::Label::new(egui::RichText::new(&self.label).size(13.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(&self.label);
            for (index, lane) in ["X", "Y", "Z", "W"]
                .iter()
                .take(self.values.len())
                .enumerate()
            {
                ui.label(
                    egui::RichText::new(*lane)
                        .monospace()
                        .size(11.0)
                        .color(theme::MUTED),
                );
                if ui
                    .add_sized(
                        [58.0, 18.0],
                        egui::DragValue::new(&mut self.values[index])
                            .range(self.min..=self.max)
                            .speed(self.step.max(0.0001))
                            .max_decimals(3),
                    )
                    .changed()
                {
                    changed = true;
                }
            }
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_short_arrays_and_preserves_lanes() {
        let control = VecControl::new("position", &serde_json::json!([1, 2]), 3, -1.0, 1.0, 0.001);
        assert_eq!(control.values.len(), 3);
        assert_eq!(control.values[2], 0.0);
        assert_eq!(control.to_value(), serde_json::json!([1.0, 2.0, 0.0]));
    }

    #[test]
    fn scalar_values_fill_every_lane() {
        let control = VecControl::new("position", &serde_json::json!(0.5), 2, 0.0, 1.0, 0.001);
        assert_eq!(control.values, vec![0.5, 0.5]);
    }
}
