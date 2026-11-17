//! Focused-input link editor.
//!
//! Legacy `InputConfig.vue` only edits the focused input; inputs themselves are
//! auto-created for module props, meta fields and Group controls. The native
//! inspector uses the prop's `$props[prop].id` and edits that input's single
//! link here.

use crate::model::{EngineSnapshot, InputLinkView};
use crate::state::{LinkEditor, UiState};
use crate::theme;

const FEATURES: &[&str] = &[
    "bass",
    "mid",
    "treble",
    "beat",
    "kick",
    "energy",
    "spectralCentroid",
    "spectralFlatness",
    "zcr",
];

/// Draw the link summary for one prop and open the editor on demand.
pub fn link_row(
    ui: &mut egui::Ui,
    prop_name: &str,
    input_id: &str,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
) {
    ui.horizontal(|ui| {
        match snapshot.input_links.get(input_id) {
            Some(link) => {
                let summary = link_summary(link);
                ui.label(
                    egui::RichText::new("LINK")
                        .monospace()
                        .size(8.0)
                        .color(theme::ACCENT),
                );
                ui.label(
                    egui::RichText::new(summary)
                        .size(9.0)
                        .color(theme::MUTED),
                )
                .on_hover_text(format!("input {input_id}"));
                if ui.small_button("Edit").clicked() {
                    state.link_editor = Some(LinkEditor::from_link(
                        input_id,
                        format!("{prop_name}  ({prop_name})"),
                        link,
                    ));
                }
                if ui.small_button("x").on_hover_text("Remove link").clicked() {
                    state.push_action(
                        "inputs/removeInputLink",
                        serde_json::json!({ "inputId": input_id }),
                    );
                }
            }
            None => {
                if ui
                    .small_button("+ link")
                    .on_hover_text("Bind this input to audio, state, MIDI or OSC")
                    .clicked()
                {
                    state.link_editor = Some(LinkEditor::audio(input_id, prop_name));
                }
            }
        }
    });
}

fn link_summary(link: &InputLinkView) -> String {
    let source = link.source.as_deref().unwrap_or("?");
    let location = link.location.as_deref().unwrap_or("");
    let args = link
        .args
        .as_ref()
        .map(|args| args.to_string())
        .unwrap_or_default();
    if source == "meyda" {
        let feature = link
            .args
            .as_ref()
            .and_then(|args| args.as_array())
            .and_then(|args| args.first())
            .and_then(|value| value.as_str())
            .unwrap_or("?");
        format!("audio.{feature}  ->  {}..{}", link.min.unwrap_or(0.0), link.max.unwrap_or(1.0))
    } else if source == "midi" {
        format!("midi  {location}  ->  {}..{}", link.min.unwrap_or(0.0), link.max.unwrap_or(1.0))
    } else if source == "osc" {
        format!("osc  {location}")
    } else {
        format!("{source}  {location} {args}")
    }
}

/// Draw the modal-like link editor window.
pub fn link_editor_window(ui: &mut egui::Ui, state: &mut UiState) {
    let Some(mut editor) = state.link_editor.take() else {
        return;
    };
    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    let mut remove = false;
    egui::Window::new(format!("Input link: {}", editor.title))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(420.0)
        .show(ui.ctx(), |ui| {
            ui.label(
                egui::RichText::new(
                    "Inputs are created automatically for module props. This link drives the selected property.",
                )
                .size(10.0)
                .color(theme::MUTED),
            );
            ui.horizontal(|ui| {
                ui.label("Preset");
                if ui.button("Audio feature").clicked() {
                    editor.set_audio_feature(&editor.audio_feature());
                }
                if ui.button("State path").clicked() {
                    editor.kind = "state".to_string();
                    editor.source = "state".to_string();
                    editor.location = "modules/active".to_string();
                    editor.args = "[]".to_string();
                }
                if ui.button("MIDI").clicked() {
                    editor.kind = "state".to_string();
                    editor.source = "midi".to_string();
                    editor.location = "midi.data.device.channel".to_string();
                }
                if ui.button("OSC").clicked() {
                    editor.kind = "state".to_string();
                    editor.source = "osc".to_string();
                    editor.location = "osc.data./address".to_string();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Kind");
                egui::ComboBox::from_id_salt("link-kind")
                    .selected_text(&editor.kind)
                    .show_ui(ui, |ui| {
                        for kind in ["getter", "state", "mutation"] {
                            ui.selectable_value(&mut editor.kind, kind.to_string(), kind);
                        }
                    });
                ui.label("Source");
                ui.add(egui::TextEdit::singleline(&mut editor.source).desired_width(100.0));
            });
            ui.horizontal(|ui| {
                ui.label("Location");
                ui.add(
                    egui::TextEdit::singleline(&mut editor.location)
                        .desired_width(260.0)
                        .hint_text("meyda/getFeature, osc.data./x, ..."),
                );
            });
            if editor.kind == "getter" {
                ui.horizontal(|ui| {
                    ui.label("Feature");
                    let current = editor.audio_feature();
                    egui::ComboBox::from_id_salt("link-feature")
                        .selected_text(&current)
                        .show_ui(ui, |ui| {
                            for feature in FEATURES {
                                if ui.selectable_label(current == *feature, *feature).clicked() {
                                    editor.set_audio_feature(feature);
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Args");
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.args)
                            .desired_width(260.0)
                            .hint_text("[\"bass\"]"),
                    );
                });
            }
            ui.horizontal(|ui| {
                ui.label("Min");
                ui.add(egui::DragValue::new(&mut editor.min).speed(0.001).max_decimals(3));
                ui.label("Max");
                ui.add(egui::DragValue::new(&mut editor.max).speed(0.001).max_decimals(3));
            });
            if let Some(error) = &editor.error {
                ui.colored_label(theme::DANGER, error);
            }
            ui.horizontal(|ui| {
                if ui.button(if editor.exists { "Save link" } else { "Create link" }).clicked() {
                    save = true;
                }
                if editor.exists && ui.button("Remove").clicked() {
                    remove = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if save {
        match editor.parsed_args() {
            Ok(args) => {
                if editor.exists {
                    for (key, value) in [
                        ("type", serde_json::json!(editor.kind)),
                        ("source", serde_json::json!(editor.source)),
                        ("location", serde_json::json!(editor.location)),
                        ("args", args.clone()),
                        ("min", serde_json::json!(editor.min)),
                        ("max", serde_json::json!(editor.max)),
                    ] {
                        state.push_action(
                            "inputs/updateInputLink",
                            serde_json::json!({
                                "inputId": editor.input_id,
                                "key": key,
                                "value": value,
                            }),
                        );
                    }
                } else {
                    state.push_action(
                        "inputs/createInputLink",
                        serde_json::json!({
                            "inputId": editor.input_id,
                            "type": editor.kind,
                            "source": editor.source,
                            "location": editor.location,
                            "args": args,
                            "min": editor.min,
                            "max": editor.max,
                        }),
                    );
                }
            }
            Err(error) => {
                editor.error = Some(error);
                state.link_editor = Some(editor);
                return;
            }
        }
    } else if remove {
        state.push_action(
            "inputs/removeInputLink",
            serde_json::json!({ "inputId": editor.input_id }),
        );
    } else if !cancel && open {
        state.link_editor = Some(editor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_defaults_and_feature_round_trip() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        assert_eq!(editor.kind, "getter");
        assert_eq!(editor.audio_feature(), "bass");
        editor.set_audio_feature("treble");
        assert_eq!(editor.audio_feature(), "treble");
        assert_eq!(editor.parsed_args().unwrap(), serde_json::json!(["treble"]));
    }

    #[test]
    fn invalid_args_are_reported() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.args = "[".to_string();
        assert!(editor.parsed_args().is_err());
    }

    #[test]
    fn state_links_do_not_require_args() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.kind = "state".to_string();
        editor.args = "not json".to_string();
        assert_eq!(editor.parsed_args(), Ok(serde_json::Value::Null));
    }
}
