//! Vector control (`vec2`/`vec3`/`vec4`), ported from the legacy
//! `Vec2DControl`/`Vec3Control`/`Vec4Control` main rows.
//!
//! The control keeps the numeric X/Y/Z/W rows (the load-bearing path for
//! presets and links) and adds the legacy 2D pad for the first two lanes.
//! `vec4` also gets an inline colour picker, which is how ISF declares colours.

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

    /// Draw the numeric rows and the 2D pad. Returns `true` when a lane changed.
    pub fn draw(&mut self, ui: &mut egui::Ui) -> bool {
        let rows_changed = self.draw_numeric_rows(ui);
        let pad_changed = self.draw_pad(ui);
        rows_changed || pad_changed
    }

    fn draw_numeric_rows(&mut self, ui: &mut egui::Ui) -> bool {
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

    /// The legacy 2D pad: lane 0 follows the pointer's x and lane 1 its y,
    /// both clamped to the prop's range. Returns `true` when a lane changed.
    fn draw_pad(&mut self, ui: &mut egui::Ui) -> bool {
        if self.values.len() < 2 {
            return false;
        }
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(64.0, 64.0), Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, "Vector pad"));

        let low = self.min.min(self.max);
        let high = self.max.max(self.min);
        let span = high - low;
        let span = if span.abs() <= f64::EPSILON {
            1.0
        } else {
            span
        };
        let to_screen = |x: f64, y: f64| {
            egui::pos2(
                rect.left() + (((x - low) / span) as f32) * rect.width(),
                rect.top() + (((high - y) / span) as f32) * rect.height(),
            )
        };

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, egui::CornerRadius::same(0), theme::INPUT);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(0),
            egui::Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );
        let centre = rect.center();
        for (start, end) in [
            (
                egui::pos2(centre.x, rect.top()),
                egui::pos2(centre.x, rect.bottom()),
            ),
            (
                egui::pos2(rect.left(), centre.y),
                egui::pos2(rect.right(), centre.y),
            ),
        ] {
            painter.line_segment([start, end], egui::Stroke::new(0.5, theme::BORDER));
        }
        let marker = to_screen(self.values[0], self.values[1]);
        painter.circle_filled(
            egui::pos2(
                marker.x.clamp(rect.left(), rect.right()),
                marker.y.clamp(rect.top(), rect.bottom()),
            ),
            3.0,
            theme::ACCENT,
        );

        let primary_down = ui.input(|input| input.pointer.primary_down());
        let interacting = response.dragged()
            || response.clicked()
            || (primary_down && response.contains_pointer());
        if !interacting {
            return false;
        }
        let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
            return false;
        };
        // Invert `to_screen`, clamping the normalised coordinate first so a
        // drag that leaves the pad still pins each lane to its bound (and
        // degenerate ranges stay put).
        let to_value = |unit: f32| (low + f64::from(unit.clamp(0.0, 1.0)) * span).clamp(low, high);
        let x = to_value((pointer.x - rect.left()) / rect.width());
        let y = to_value(1.0 - (pointer.y - rect.top()) / rect.height());
        let mut changed = false;
        for (lane, value) in [(0, x), (1, y)] {
            if (self.values[lane] - value).abs() > f64::EPSILON {
                self.values[lane] = value;
                changed = true;
            }
        }
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
