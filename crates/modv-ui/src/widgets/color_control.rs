//! Colour control (ISF `vec4`/legacy `color` props), ported from
//! `ColorControl.vue`.
//!
//! The legacy control edits `{ r, g, b, a }` ratios (0..1) and opens a separate
//! Electron colour window. P3 uses an inline swatch button that opens the
//! square/wheel [`crate::widgets::ColorPicker`] popup plus the four channel
//! fields; the value shape stays the same so presets round-trip.

use egui::{Color32, Sense};
use serde_json::Value;

use crate::theme;
use crate::widgets::color_picker::show_picker_button;

/// One `{r,g,b,a}` colour ratio control.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorControl {
    pub label: String,
    pub rgba: [f32; 4],
}

impl ColorControl {
    pub fn new(label: impl Into<String>, value: &Value) -> Self {
        let mut rgba = [0.0, 0.0, 0.0, 1.0];
        match value {
            Value::Object(map) => {
                for (index, key) in ["r", "g", "b", "a"].iter().enumerate() {
                    rgba[index] = map
                        .get(*key)
                        .and_then(Value::as_f64)
                        .unwrap_or(rgba[index] as f64) as f32;
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter().take(4).enumerate() {
                    rgba[index] = item.as_f64().unwrap_or(rgba[index] as f64) as f32;
                }
            }
            _ => {}
        }
        Self {
            label: label.into(),
            rgba,
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "r": self.rgba[0].clamp(0.0, 1.0),
            "g": self.rgba[1].clamp(0.0, 1.0),
            "b": self.rgba[2].clamp(0.0, 1.0),
            "a": self.rgba[3].clamp(0.0, 1.0),
        })
    }

    /// Draw the control. Returns `true` when any channel changed.
    pub fn draw(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let mut picked = None;
        ui.horizontal(|ui| {
            ui.add_sized(
                [82.0, 18.0],
                egui::Label::new(egui::RichText::new(&self.label).size(13.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(&self.label);

            picked = show_picker_button(
                ui,
                &self.label,
                self.rgba,
                egui::vec2(46.0, 18.0),
                ratio_to_color(self.rgba),
                "color-control-picker-mode",
            );

            for (index, channel) in ["R", "G", "B", "A"].iter().enumerate() {
                ui.label(
                    egui::RichText::new(*channel)
                        .monospace()
                        .size(11.0)
                        .color(theme::MUTED),
                );
                if ui
                    .add_sized(
                        [46.0, 18.0],
                        egui::DragValue::new(&mut self.rgba[index])
                            .range(0.0..=1.0)
                            .speed(0.005)
                            .max_decimals(3),
                    )
                    .changed()
                {
                    changed = true;
                }
            }
        });
        if let Some(rgba) = picked {
            for (channel, value) in self.rgba.iter_mut().take(3).zip(rgba) {
                *channel = value;
            }
            changed = true;
        }
        changed
    }
}

fn ratio_to_color(rgba: [f32; 4]) -> Color32 {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(rgba[0]),
        channel(rgba[1]),
        channel(rgba[2]),
        channel(rgba[3]),
    )
}

#[cfg(test)]
fn color_to_ratio(color: Color32) -> [f32; 4] {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_object_and_array_colours() {
        let object = ColorControl::new(
            "tint",
            &serde_json::json!({"r": 0.1, "g": 0.2, "b": 0.3, "a": 0.4}),
        );
        assert_eq!(object.rgba, [0.1, 0.2, 0.3, 0.4]);
        let array = ColorControl::new("tint", &serde_json::json!([1, 0, 0, 1]));
        assert_eq!(array.rgba, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn serialises_the_legacy_ratio_shape() {
        let control = ColorControl::new(
            "tint",
            &serde_json::json!({"r": 2, "g": -1, "b": 0.5, "a": 1}),
        );
        let value = control.to_value();
        assert_eq!(value["r"], 1.0);
        assert_eq!(value["g"], 0.0);
        assert_eq!(value["b"], 0.5);
        assert_eq!(value["a"], 1.0);
    }

    #[test]
    fn colour_round_trip_is_within_one_byte() {
        let control = ColorControl::new("tint", &serde_json::json!([0.25, 0.5, 0.75, 1.0]));
        let color = ratio_to_color(control.rgba);
        let ratios = color_to_ratio(color);
        for (a, b) in control.rgba.iter().zip(ratios.iter()) {
            assert!((a - b).abs() <= 1.0 / 255.0, "{a} vs {b}");
        }
    }
}
