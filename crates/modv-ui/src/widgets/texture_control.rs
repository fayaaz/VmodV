//! Texture source picker, ported from `TextureControl.vue`.
//!
//! P3 exposes the engine's live texture ids (`group:`, `isf:`, `two-d:`,
//! `point10:`, `camera:`, `video:`, `screen:`). Image assets and video transport
//! land with the media manager (P6); until then an image path can be typed in
//! directly and unresolved values keep the group canvas, exactly like the
//! engine's `TextureRequest`.

use egui::{Sense, Stroke, StrokeKind};
use serde_json::Value;

use crate::model::TextureSourceView;
use crate::theme;

/// One texture prop editor.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureControl {
    pub label: String,
    pub value: Value,
}

impl TextureControl {
    pub fn new(label: impl Into<String>, value: &Value) -> Self {
        Self {
            label: label.into(),
            value: value.clone(),
        }
    }

    pub fn kind(&self) -> &str {
        self.value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("inherit")
    }

    pub fn source_id(&self) -> Option<&str> {
        self.value
            .get("options")
            .and_then(|options| options.get("id").or_else(|| options.get("path")))
            .and_then(Value::as_str)
    }

    /// Draw the picker. Returns `Some(value)` when the prop should be written.
    pub fn draw(&mut self, ui: &mut egui::Ui, sources: &[TextureSourceView]) -> Option<Value> {
        let mut changed = false;
        let mut kind = self.kind().to_string();
        ui.horizontal(|ui| {
            ui.add_sized(
                [82.0, 18.0],
                egui::Label::new(egui::RichText::new(&self.label).size(11.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(&self.label);
            egui::ComboBox::from_id_salt(("texture-type", &self.label))
                .selected_text(&kind)
                .width(96.0)
                .show_ui(ui, |ui| {
                    for option in [
                        "inherit", "group", "canvas", "webcam", "image", "video", "screen",
                    ] {
                        if ui.selectable_label(kind == option, option).clicked() {
                            kind = option.to_string();
                            changed = true;
                        }
                    }
                });
        });

        let mut source = self.source_id().unwrap_or_default().to_string();
        match kind.as_str() {
            "inherit" => {
                ui.label(
                    egui::RichText::new("inherits the canvas below")
                        .size(9.0)
                        .color(theme::MUTED),
                );
            }
            "image" => {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut source)
                        .hint_text("media-relative image path")
                        .desired_width(190.0),
                );
                if response.changed() {
                    changed = true;
                }
            }
            "video" => {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut source)
                        .hint_text("media-relative video path")
                        .desired_width(190.0),
                );
                if response.changed() {
                    changed = true;
                }
            }
            _ => {
                let wanted = match kind.as_str() {
                    "group" | "canvas" => "group",
                    "webcam" => "camera",
                    other => other,
                };
                let options: Vec<&TextureSourceView> = sources
                    .iter()
                    .filter(|source| source.kind == wanted)
                    .collect();
                egui::ComboBox::from_id_salt(("texture-source", &self.label))
                    .selected_text(
                        options
                            .iter()
                            .find(|option| option.id == source)
                            .map(|option| option.label.as_str())
                            .unwrap_or("(none)"),
                    )
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        for option in options {
                            if ui
                                .selectable_label(source == option.id, &option.label)
                                .clicked()
                            {
                                source = option.id.clone();
                                changed = true;
                            }
                        }
                    });
            }
        }

        if changed {
            let options = match kind.as_str() {
                "inherit" => serde_json::json!({}),
                "image" | "video" => serde_json::json!({ "path": source }),
                _ => serde_json::json!({ "id": source }),
            };
            self.value = serde_json::json!({ "type": kind, "options": options });
            Some(self.value.clone())
        } else {
            None
        }
    }
}

/// A small preview swatch for a composed texture source (diagnostics/tests).
pub fn source_chip(ui: &mut egui::Ui, source: &TextureSourceView) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(2),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    ui.label(
        egui::RichText::new(format!("{}: {}", source.kind, source.label))
            .size(9.0)
            .color(theme::MUTED),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_group_and_inherit_values() {
        let group = TextureControl::new(
            "texture",
            &serde_json::json!({"type": "group", "options": {"id": "group:abc"}}),
        );
        assert_eq!(group.kind(), "group");
        assert_eq!(group.source_id(), Some("group:abc"));

        let inherit = TextureControl::new("texture", &serde_json::json!({"type": "inherit"}));
        assert_eq!(inherit.kind(), "inherit");
        assert_eq!(inherit.source_id(), None);
    }
}
