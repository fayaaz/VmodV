//! Texture source picker, ported from `TextureControl.vue`.
//!
//! P3 exposes the engine's live texture ids (`group:`, `isf:`, `two-d:`,
//! `point10:`, `camera:`, `video:`, `screen:`). Image assets and video transport
//! land with the media manager (P6); until then an image path can be typed in
//! directly and unresolved values keep the group canvas, exactly like the
//! engine's `TextureRequest`.

use egui::{Sense, Stroke, StrokeKind};
use serde_json::Value;

use crate::library_drag::{MediaLibraryDrag, USE_FROM_LIBRARY_LABEL, texture_value_for_control};
use crate::model::{LibraryKind, MediaLibraryView, TextureSourceView};
use crate::theme;

/// What a texture control gesture asks the shell to do.
#[derive(Clone, Debug, PartialEq)]
pub enum TextureControlChange {
    /// Write the prop value (manual edit or a Library drop).
    Value(Value),
    /// Open the Library tab and wait for an asset of this kind.
    PickFromLibrary { kind: LibraryKind },
}

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

    /// Draw the picker.
    ///
    /// Returns a change when the prop should be written, or when the user asks
    /// to pick an asset from the Library tab. A Library tile dropped on the
    /// control writes the same value as typing the path by hand.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        sources: &[TextureSourceView],
        library: &MediaLibraryView,
    ) -> Option<TextureControlChange> {
        let mut changed = false;
        let mut pick_from_library = None;
        let mut kind = self.kind().to_string();
        let mut source = self.source_id().unwrap_or_default().to_string();
        let inner = ui.scope(|ui| {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [82.0, 18.0],
                    egui::Label::new(egui::RichText::new(&self.label).size(13.0))
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

            match kind.as_str() {
                "inherit" => {
                    ui.label(
                        egui::RichText::new("inherits the canvas below")
                            .size(11.0)
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

            if let Some(library_kind) = library_kind_for(&kind) {
                let count = library.count_for(library_kind, "");
                if ui
                    .button(egui::RichText::new(USE_FROM_LIBRARY_LABEL).size(11.5))
                    .on_hover_text(format!(
                        "Browse the {count} {} in the {:?} profile",
                        library_kind.title().to_lowercase(),
                        library.profile
                    ))
                    .clicked()
                {
                    pick_from_library = Some(library_kind);
                }
            }
        });

        if let Some(value) = library_drop_target(ui, inner.response.rect, &kind) {
            self.value = value.clone();
            return Some(TextureControlChange::Value(value));
        }
        if let Some(kind) = pick_from_library {
            return Some(TextureControlChange::PickFromLibrary { kind });
        }
        if changed {
            let options = match kind.as_str() {
                "inherit" => serde_json::json!({}),
                "image" | "video" => serde_json::json!({ "path": source }),
                _ => serde_json::json!({ "id": source }),
            };
            self.value = serde_json::json!({ "type": kind, "options": options });
            Some(TextureControlChange::Value(self.value.clone()))
        } else {
            None
        }
    }
}

/// The Library folder a texture kind can pick from.
fn library_kind_for(kind: &str) -> Option<LibraryKind> {
    match kind {
        "image" => Some(LibraryKind::Image),
        "video" => Some(LibraryKind::Video),
        _ => None,
    }
}

/// Paint and resolve a Library tile dropped on `rect`.
///
/// Only a matching asset kind is accepted, so a video can never rewrite an
/// image prop. The payload is consumed on release so two controls cannot both
/// answer the same drop.
fn library_drop_target(ui: &egui::Ui, rect: egui::Rect, control_kind: &str) -> Option<Value> {
    let payload = egui::DragAndDrop::payload::<MediaLibraryDrag>(ui.ctx())?;
    texture_value_for_control(control_kind, &payload)?;
    if !ui.rect_contains_pointer(rect) {
        return None;
    }

    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(0),
        theme::ACCENT.gamma_multiply(0.12),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(0),
        Stroke::new(1.5, theme::ACCENT),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.right_bottom() + egui::vec2(-4.0, -4.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("Drop {}", payload.name),
        egui::FontId::proportional(11.0),
        theme::ACCENT,
    );

    if ui.input(|input| input.pointer.any_released()) {
        let released = egui::DragAndDrop::take_payload::<MediaLibraryDrag>(ui.ctx())?;
        return texture_value_for_control(control_kind, &released);
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    None
}

/// A small preview swatch for a composed texture source (diagnostics/tests).
pub fn source_chip(ui: &mut egui::Ui, source: &TextureSourceView) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(0),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    ui.label(
        egui::RichText::new(format!("{}: {}", source.kind, source.label))
            .size(11.0)
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
