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
use crate::model::{
    LibraryKind, MediaLibraryView, MediaSnapshot, TextureSourceView, VideoTransportAction,
    VideoTransportView,
};
use crate::theme;

/// What a texture control gesture asks the shell to do.
#[derive(Clone, Debug, PartialEq)]
pub enum TextureControlChange {
    /// Write the prop value (manual edit or a Library drop).
    Value(Value),
    /// Open the Library tab and wait for an asset of this kind.
    PickFromLibrary { kind: LibraryKind },
    /// Drive the running video source and (for pause/loop/speed) persist the
    /// option in the prop.
    VideoTransport {
        /// The updated prop value, when the option changed.
        value: Option<Value>,
        /// The engine source id (`video:<absolute path>`) or the saved path.
        source: String,
        action: VideoTransportAction,
    },
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
    /// Returns a change when the prop should be written, when the user asks
    /// to pick an asset from the Library tab, or when the video transport row
    /// issues a play/pause/restart/loop/speed command. A Library tile dropped
    /// on the control writes the same value as typing the path by hand.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        sources: &[TextureSourceView],
        library: &MediaLibraryView,
        media: &MediaSnapshot,
    ) -> Option<TextureControlChange> {
        let mut changed = false;
        let mut path_changed = false;
        let mut pick_from_library = None;
        let mut transport_change: Option<(Option<(String, Value)>, VideoTransportAction)> = None;
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
                        path_changed = true;
                    }
                }
                "video" => {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut source)
                            .hint_text("media-relative video path")
                            .desired_width(190.0),
                    );
                    if response.changed() {
                        path_changed = true;
                    }

                    if !source.trim().is_empty() {
                        video_transport_row(ui, &source, media, &mut transport_change);
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
        if let Some((option, action)) = transport_change {
            let value = match action {
                VideoTransportAction::Restart | VideoTransportAction::Seek { .. } => None,
                _ => option.and_then(|(key, option)| {
                    let updated = self.video_value_with(&key, option);
                    let value = (updated != self.value).then_some(updated);
                    if let Some(value) = &value {
                        self.value = value.clone();
                    }
                    value
                }),
            };
            return Some(TextureControlChange::VideoTransport {
                value,
                source: media
                    .video_source_for_path(&source)
                    .map(|running| running.source_id.clone())
                    .unwrap_or_else(|| format!("video:{}", source.trim())),
                action,
            });
        }
        if changed {
            let options = match kind.as_str() {
                "inherit" => serde_json::json!({}),
                "image" | "video" => serde_json::json!({ "path": source }),
                _ => serde_json::json!({ "id": source }),
            };
            self.value = serde_json::json!({ "type": kind, "options": options });
            Some(TextureControlChange::Value(self.value.clone()))
        } else if path_changed {
            // A manual path edit keeps the video options (loop/paused/speed)
            // already saved on the prop; only the clip changes.
            let options = if kind == "video" {
                let mut options = self
                    .value
                    .get("options")
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| serde_json::json!({}));
                options["path"] = serde_json::json!(source);
                options
            } else {
                serde_json::json!({ "path": source })
            };
            self.value = serde_json::json!({ "type": kind, "options": options });
            Some(TextureControlChange::Value(self.value.clone()))
        } else {
            None
        }
    }

    /// The prop value with one `options.<key>` replaced, preserving the other
    /// saved keys (type, path, and unrelated options).
    fn video_value_with(&self, key: &str, option: Value) -> Value {
        let mut definition = self.value.clone();
        if !definition.is_object() {
            definition = serde_json::json!({ "type": "video", "options": {} });
        }
        definition["type"] = serde_json::json!("video");
        if !definition.get("options").is_some_and(Value::is_object) {
            definition["options"] = serde_json::json!({});
        }
        definition["options"][key] = option.clone();
        definition
    }
}

/// Paint the video clip's transport row: Play/Pause, Restart, Speed, Seek and
/// Loop, plus the position/duration readout and a missing-file message.
///
/// The row only issues commands when a source is running; the shell persists
/// the pause/loop/speed option into the prop so presets and Scenes keep it.
fn video_transport_row(
    ui: &mut egui::Ui,
    path: &str,
    media: &MediaSnapshot,
    change: &mut Option<(Option<(String, Value)>, VideoTransportAction)>,
) {
    let clip_name = path.rsplit('/').next().unwrap_or(path);
    ui.label(
        egui::RichText::new(clip_name)
            .size(12.0)
            .color(theme::FOREGROUND),
    )
    .on_hover_text(path);
    ui.label(egui::RichText::new(path).size(10.5).color(theme::MUTED));

    if let Some(problem) = media.video_problem_for_path(path) {
        ui.colored_label(theme::DANGER, egui::RichText::new(problem).size(11.0));
        return;
    }

    let Some(running) = media.video_source_for_path(path) else {
        ui.label(
            egui::RichText::new("Starting video source…")
                .size(11.0)
                .color(theme::MUTED),
        );
        return;
    };
    let Some(transport) = running.transport else {
        ui.label(
            egui::RichText::new("Starting video source…")
                .size(11.0)
                .color(theme::MUTED),
        );
        return;
    };

    ui.horizontal(|ui| {
        let (label, action) = if transport.paused || transport.eos {
            ("Play", VideoTransportAction::Resume)
        } else {
            ("Pause", VideoTransportAction::Pause)
        };
        if ui.button(label).clicked() {
            *change = Some((
                Some((
                    "paused".to_string(),
                    serde_json::json!(matches!(action, VideoTransportAction::Pause)),
                )),
                action,
            ));
        }
        if ui
            .button("Restart")
            .on_hover_text("Seek back to the first frame")
            .clicked()
        {
            *change = Some((None, VideoTransportAction::Restart));
        }
    });

    ui.horizontal(|ui| {
        ui.spacing_mut().slider_width = 90.0;
        let mut speed = transport.playback_rate;
        let response = ui.add(
            egui::Slider::new(&mut speed, 0.1..=10.0)
                .step_by(0.05)
                .text("Speed"),
        );
        if response.changed() {
            *change = Some((
                Some(("playbackrate".to_string(), serde_json::json!(speed))),
                VideoTransportAction::SetPlaybackRate { rate: speed },
            ));
        }
    });

    if let Some(position_ms) = seek_position(ui, &transport) {
        // Seeking is transient: no option is persisted into the prop.
        *change = Some((None, VideoTransportAction::Seek { position_ms }));
    }

    let mut looping = transport.looping;
    if ui.checkbox(&mut looping, "Loop").changed() {
        *change = Some((
            Some(("loop".to_string(), serde_json::json!(looping))),
            VideoTransportAction::SetLooping { looping },
        ));
    }

    ui.label(
        egui::RichText::new(transport.position_label())
            .monospace()
            .size(10.5)
            .color(theme::MUTED),
    );
    if transport.eos && !transport.looping {
        ui.label(
            egui::RichText::new("end of clip")
                .size(10.5)
                .color(theme::MUTED),
        );
    }
}

/// A `Seek` slider in milliseconds, offered only when the source is seekable
/// and its duration is known and non-zero. Returns the requested position when
/// the user scrubs it this frame.
pub(crate) fn seek_position(ui: &mut egui::Ui, transport: &VideoTransportView) -> Option<u64> {
    let duration = transport.duration_ms.filter(|duration| *duration > 0)?;
    if !transport.seekable {
        return None;
    }
    let mut position = transport.position_ms.min(duration) as f64;
    let response = ui.add(
        egui::Slider::new(&mut position, 0.0..=duration as f64)
            .show_value(false)
            .text("Seek"),
    );
    response.changed().then(|| position.round() as u64)
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

    #[test]
    fn video_option_updates_keep_the_clip_and_other_options() {
        let control = TextureControl::new(
            "texture",
            &serde_json::json!({
                "type": "video",
                "options": { "path": "live/video/clip.mp4", "loop": false },
                "id": "saved-video"
            }),
        );

        assert_eq!(
            control.video_value_with("paused", serde_json::json!(true)),
            serde_json::json!({
                "type": "video",
                "options": {
                    "path": "live/video/clip.mp4",
                    "loop": false,
                    "paused": true
                },
                "id": "saved-video"
            })
        );
        assert_eq!(
            control.video_value_with("playbackrate", serde_json::json!(2.0))["options"]["playbackrate"],
            serde_json::json!(2.0)
        );
    }
}
