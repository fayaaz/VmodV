//! Focused-input link editor.
//!
//! Legacy `InputConfig.vue` only edits the focused input; inputs themselves are
//! auto-created for module props, meta fields and Group controls. The native
//! inspector uses the prop's `$props[prop].id` and edits that input's single
//! link here.
//!
//! The editor follows the legacy panel layout: Min/Max at the top, then
//! Audio / MIDI / Tween / OSC / Expression sections. Pure payload builders live
//! in [`LinkEditor`] so the JS action shapes are unit-testable without egui.

use crate::layout::WINDOW_INPUT_LINK;
use crate::model::{EngineSnapshot, InputLinkView};
use crate::state::{AUDIO_FEATURES, LinkEditor, MAX_SMOOTHING, SMOOTHING_STEP, UiState};
use crate::theme;
use crate::widgets::TweenControl;

/// Draw the link summary for one prop and open the editor on demand.
pub fn link_row(
    ui: &mut egui::Ui,
    prop_name: &str,
    input_id: &str,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
) {
    ui.horizontal(|ui| {
        if let Some(link) = snapshot.input_links.get(input_id) {
            let summary = link_summary(link);
            ui.label(
                egui::RichText::new("LINK")
                    .monospace()
                    .size(10.0)
                    .color(theme::ACCENT),
            );
            ui.label(egui::RichText::new(summary).size(11.0).color(theme::MUTED))
                .on_hover_text(format!("input {input_id}"));
            if ui.small_button("Edit").clicked() {
                state.link_editor = Some(LinkEditor::for_input(input_id, prop_name, snapshot));
            }
            if ui.small_button("x").on_hover_text("Remove link").clicked() {
                // Releasing the smoothing id keeps the evaluator registry from
                // leaking entries the way the JS panel had to fix.
                if let Some(id) = smoothing_id(link) {
                    state.push_action("meyda/removeSmoothingId", serde_json::json!(id));
                }
                state.push_action(
                    "inputs/removeInputLink",
                    serde_json::json!({ "inputId": input_id }),
                );
            }
        } else if ui
            .small_button("+ link")
            .on_hover_text("Bind this input to audio, state, MIDI or OSC")
            .clicked()
        {
            state.link_editor = Some(LinkEditor::for_input(input_id, prop_name, snapshot));
        }

        // The legacy E badge marks an input with an expression assignment,
        // whether or not it also has an Input Link.
        if let Some(assignment) = snapshot.expression_for_input(input_id) {
            theme::badge(ui, "E", theme::ACCENT).on_hover_text(format!(
                "Expression: {}\nClick Edit to change it",
                assignment.expression
            ));
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
        if link.kind == "mutation" {
            format!("audio.{feature}  (mutation  {location})")
        } else {
            format!(
                "audio.{feature}  ->  {}..{}",
                link.min.unwrap_or(0.0),
                link.max.unwrap_or(1.0)
            )
        }
    } else if source == "midi" {
        format!(
            "midi  {location}  ->  {}..{}",
            link.min.unwrap_or(0.0),
            link.max.unwrap_or(1.0)
        )
    } else if source == "osc" {
        format!("osc  {location}")
    } else {
        format!("{source}  {location} {args}")
    }
}

fn smoothing_id(link: &InputLinkView) -> Option<String> {
    link.args
        .as_ref()
        .and_then(|args| args.as_array())
        .filter(|args| args.len() > 2)
        .and_then(|args| args.get(1))
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_u64().map(|number| number.to_string()))
        })
}

/// A small live readout for the audio features the snapshot carries.
fn feature_readout(feature: &str, snapshot: &EngineSnapshot) -> Option<String> {
    match feature {
        "kick" => Some(format!(
            "kick {}",
            if snapshot.audio.kick { 1.0 } else { 0.0 }
        )),
        "bass" => Some(format!("bass {:.2}", snapshot.audio.bass)),
        "mid" => Some(format!("mid {:.2}", snapshot.audio.mid)),
        "treble" => Some(format!("treble {:.2}", snapshot.audio.treble)),
        _ => None,
    }
}

/// Draw the modal-like link editor window.
pub fn link_editor_window(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let Some(mut editor) = state.link_editor.take() else {
        return;
    };

    // Keep the assignment id in sync with the authoritative snapshot. The UI
    // cannot await `expressions/create`'s id, so the next frame's records are
    // what tell a later Save whether to create or update. The expression text
    // is only adopted while the draft still holds the identity default, so a
    // half-typed source is never overwritten.
    editor.sync_expression(snapshot.expression_for_input(&editor.input_id));
    if let Some(tween) = snapshot.tween_for_input(&editor.input_id) {
        if editor.tween.is_none() && !editor.tween_cleared {
            editor.tween = Some(tween.clone());
        }
    } else {
        editor.tween_cleared = false;
    }

    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    let mut remove = false;
    let mut expression_save = false;
    let mut expression_clear = false;
    let mut evaluation_changed = false;

    let window = state.layout.apply_window(
        egui::Window::new(format!("Input link: {}", editor.title))
            .id(egui::Id::unique("modv-window:input_link"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .min_width(340.0)
            .min_height(300.0)
            .default_size([440.0, 560.0]),
        WINDOW_INPUT_LINK,
    );
    let response = window.show(ui.ctx(), |ui| {
            ui.label(
                egui::RichText::new(
                    "Inputs are created automatically for module props. This link drives the selected property.",
                )
                .size(12.0)
                .color(theme::MUTED),
            );
            ui.label(
                egui::RichText::new(format!("input {}", editor.input_id))
                    .monospace()
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.separator();

            // Legacy `InputConfig.vue`: Min/Max at the top, before any source
            // section.
            ui.horizontal(|ui| {
                ui.label("Min");
                ui.add(egui::DragValue::new(&mut editor.min).speed(0.001).max_decimals(3));
                ui.label("Max");
                ui.add(egui::DragValue::new(&mut editor.max).speed(0.001).max_decimals(3));
            });
            ui.separator();

            audio_section(ui, &mut editor, snapshot, state);

            let midi_enabled = !editor.exists
                || editor.source.is_empty()
                || matches!(editor.source.as_str(), "midi" | "state");
            egui::CollapsingHeader::new("MIDI")
                .default_open(editor.source == "midi")
                .show(ui, |ui| {
                    ui.add_enabled_ui(midi_enabled, |ui| {
                        ui.label(
                            egui::RichText::new(
                                "State path read from MIDI, e.g. midi.data.<device>.<channel>",
                            )
                            .size(12.0)
                            .color(theme::MUTED),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Use MIDI").clicked() {
                                editor.use_midi("midi.data.device.channel");
                            }
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.location)
                                    .desired_width(240.0)
                                    .hint_text("midi.data.device.channel"),
                            );
                        });
                    });
                });

            let tween_enabled = !editor.exists
                || editor.source.is_empty()
                || editor.source == "tween";
            egui::CollapsingHeader::new("Tween")
                .default_open(editor.source == "tween" || editor.tween.is_some())
                .show(ui, |ui| {
                    ui.add_enabled_ui(tween_enabled, |ui| {
                        let had_tween = editor.tween.is_some();
                        let mut control = TweenControl::new(
                            editor.title.clone(),
                            Some(editor.input_id.clone()),
                            editor.tween.clone(),
                            editor.min,
                            editor.max,
                        );
                        control.draw(ui, state);
                        editor.tween = control.tween.clone();
                        if had_tween && editor.tween.is_none() {
                            // TweenControl queued both removals; mirror that in
                            // the draft so a later Save cannot recreate a
                            // dangling tween-sourced link.
                            editor.exists = false;
                            editor.use_state("modules/active");
                            editor.tween_cleared = true;
                        } else if !had_tween && editor.tween.is_some() {
                            editor.tween_cleared = false;
                            editor.exists = true;
                            editor.kind = "state".to_string();
                            editor.source = "tween".to_string();
                            editor.location = TweenControl::link_location(&editor.input_id);
                            editor.matcher = None;
                            editor.args = "[]".to_string();
                        }
                    });
                });

            let osc_enabled = !editor.exists
                || editor.source.is_empty()
                || matches!(editor.source.as_str(), "osc" | "state");
            egui::CollapsingHeader::new("OSC")
                .default_open(editor.source == "osc")
                .show(ui, |ui| {
                    ui.add_enabled_ui(osc_enabled, |ui| {
                        ui.label(
                            egui::RichText::new("State path written by an OSC address, e.g. osc.data./address")
                                .size(12.0)
                                .color(theme::MUTED),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Use OSC").clicked() {
                                editor.use_osc("osc.data./address");
                            }
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.location)
                                    .desired_width(240.0)
                                    .hint_text("osc.data./address"),
                            );
                        });
                    });
                });

            state_section(ui, &mut editor);

            egui::CollapsingHeader::new("Expression")
                .default_open(editor.expression_id.is_some())
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Shape the incoming value with JS syntax/math. `value` is the incoming number; `value * 3` triples it. `value` is the identity and removes the assignment.",
                        )
                        .size(12.0)
                        .color(theme::MUTED),
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.expression)
                            .desired_width(f32::INFINITY)
                            .desired_rows(3)
                            .hint_text("value"),
                    );
                    if ui
                        .add_enabled(
                            editor.expression_id.is_some(),
                            egui::Checkbox::new(
                                &mut editor.evaluate_every_frame,
                                "Evaluate every frame",
                            ),
                        )
                        .changed()
                    {
                        evaluation_changed = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Save expression").clicked() {
                            expression_save = true;
                        }
                        if ui
                            .add_enabled(
                                editor.expression_id.is_some(),
                                egui::Button::new("Clear expression"),
                            )
                            .clicked()
                        {
                            expression_clear = true;
                        }
                    });
                });

            if let Some(error) = &editor.error {
                ui.colored_label(theme::DANGER, error);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(if editor.exists { "Save link" } else { "Create link" })
                    .clicked()
                {
                    save = true;
                }
                if editor.exists && ui.button("Remove link").clicked() {
                    remove = true;
                }
                if ui.button("Close").clicked() {
                    cancel = true;
                }
            });
        });

    if let Some(response) = response {
        state
            .layout
            .record_window_rect(WINDOW_INPUT_LINK, response.response.rect);
    }

    if expression_save {
        let actions = editor.expression_actions();
        let clears = editor.expression.trim().is_empty() || editor.expression.trim() == "value";
        state.actions.extend(actions);
        if clears {
            editor.mark_expression_cleared();
        } else {
            editor.expression_cleared = false;
        }
    }
    if expression_clear {
        state.actions.extend(editor.clear_expression_actions());
        editor.mark_expression_cleared();
    }
    if evaluation_changed && let Some(action) = editor.evaluation_action() {
        state.actions.push(action);
    }

    if save {
        match editor.save_actions() {
            Ok(actions) => state.actions.extend(actions),
            Err(error) => {
                editor.error = Some(error);
                state.link_editor = Some(editor);
            }
        }
    } else if remove {
        state.actions.extend(editor.remove_actions());
    } else if !cancel && open {
        state.link_editor = Some(editor);
    }
}

fn audio_section(
    ui: &mut egui::Ui,
    editor: &mut LinkEditor,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
) {
    let audio_enabled = !editor.exists || editor.source.is_empty() || editor.source == "meyda";
    egui::CollapsingHeader::new("Audio")
        .default_open(editor.source == "meyda")
        .show(ui, |ui| {
            ui.add_enabled_ui(audio_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Feature");
                    let current = editor.audio_feature();
                    egui::ComboBox::from_id_salt("link-feature")
                        .selected_text(&current)
                        .show_ui(ui, |ui| {
                            for feature in AUDIO_FEATURES {
                                if ui.selectable_label(current == *feature, *feature).clicked() {
                                    editor.set_audio_feature(feature);
                                }
                            }
                        });
                    if let Some(readout) = feature_readout(&editor.feature, snapshot) {
                        ui.label(
                            egui::RichText::new(readout)
                                .monospace()
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    }
                });

                if editor.feature == "none" {
                    ui.label(
                        egui::RichText::new("No audio feature; Save removes the link.")
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                } else if editor.feature == "kick" {
                    ui.label(
                        egui::RichText::new(
                            "Kick is a mutation link on beats/SET_KICK (no smoothing).",
                        )
                        .size(12.0)
                        .color(theme::MUTED),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Smoothing");
                        // JS stores smoothing in `args[2]` and shows the
                        // inverted range control (`MAX - smoothingValue`).
                        let mut slider = editor.smoothing_slider_value();
                        let changed = ui
                            .add(
                                egui::Slider::new(
                                    &mut slider,
                                    0.0..=(MAX_SMOOTHING - SMOOTHING_STEP),
                                )
                                .step_by(SMOOTHING_STEP)
                                .max_decimals(3),
                            )
                            .changed();
                        if changed {
                            editor.set_smoothing_from_slider(slider);
                            editor.args = editor.audio_args().to_string();
                        }
                        if ui
                            .add_enabled(
                                editor.smoothing_value > 0.0 || editor.smoothing_id.is_some(),
                                egui::Button::new("Clear"),
                            )
                            .on_hover_text("Remove smoothing and release its id")
                            .clicked()
                            && let Some(action) = editor.clear_smoothing()
                        {
                            state.actions.push(action);
                            // The JS watcher writes the shortened args straight
                            // back into the link; do the same so Close without
                            // Save cannot leave a released smoothing id in it.
                            if editor.exists && editor.source == "meyda" {
                                state.push_action(
                                    "inputs/updateInputLink",
                                    serde_json::json!({
                                        "inputId": editor.input_id,
                                        "key": "args",
                                        "value": editor.audio_args(),
                                    }),
                                );
                            }
                        }
                    });
                }
            });
        });
}

fn state_section(ui: &mut egui::Ui, editor: &mut LinkEditor) {
    let state_enabled =
        editor.source.is_empty() || matches!(editor.source.as_str(), "state" | "midi" | "osc");
    egui::CollapsingHeader::new("State")
        .default_open(editor.source == "state")
        .show(ui, |ui| {
            ui.add_enabled_ui(state_enabled, |ui| {
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
                            .hint_text("modules/active, beats.bpm, ..."),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Args");
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.args)
                            .desired_width(260.0)
                            .hint_text("[\"bass\"]"),
                    );
                });
                if ui.button("State path").clicked() {
                    editor.use_state("modules/active");
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExpressionView, TweenView};

    #[test]
    fn feature_list_matches_the_legacy_audio_panel() {
        assert_eq!(
            AUDIO_FEATURES,
            &[
                "none",
                "kick",
                "bass",
                "mid",
                "treble",
                "rms",
                "zcr",
                "energy",
                "spectralCentroid",
                "spectralFlatness",
                "spectralSlope",
                "spectralRolloff",
                "spectralSpread",
                "spectralSkewness",
                "spectralKurtosis",
                "perceptualSpread",
                "perceptualSharpness",
            ]
        );
    }

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
    fn kick_saves_a_mutation_link_instead_of_a_getter() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.set_audio_feature("kick");
        assert_eq!(editor.kind, "mutation");
        assert_eq!(editor.location, "beats.kick");
        let actions = editor.save_actions().expect("payload parses");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "inputs/createInputLink");
        assert_eq!(actions[0].payload["type"], "mutation");
        assert_eq!(actions[0].payload["location"], "beats.kick");
        assert_eq!(actions[0].payload["source"], "meyda");
        assert_eq!(actions[0].payload["args"], serde_json::json!(["kick"]));
        assert_eq!(
            actions[0].payload["match"],
            serde_json::json!({ "type": "beats/SET_KICK" })
        );
    }

    #[test]
    fn a_getter_switch_clears_a_stale_kick_matcher() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.set_audio_feature("kick");
        editor.save_actions().expect("kick payload");
        editor.set_audio_feature("bass");
        let payload = editor.link_payload().expect("getter payload");
        assert_eq!(payload["type"], "getter");
        assert_eq!(payload["location"], "meyda/getFeature");
        assert!(payload.get("match").is_none());
    }

    #[test]
    fn smoothing_mints_a_client_id_and_keeps_it_across_saves() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.smoothing_value = 0.25;
        let first = editor.link_payload().expect("payload");
        let args = first["args"].as_array().expect("args array");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], serde_json::json!("bass"));
        let id = args[1].as_str().expect("smoothing id");
        assert!(
            id.starts_with("native-smoothing-"),
            "unexpected smoothing id {id}"
        );
        assert_eq!(args[2], serde_json::json!(0.25));

        let second = editor.link_payload().expect("payload");
        assert_eq!(second["args"], first["args"]);
        assert_eq!(
            editor.smoothing_id.as_deref(),
            Some(id),
            "the minted id is reused"
        );
    }

    #[test]
    fn smoothing_ids_are_unique_between_inputs() {
        let mut first = LinkEditor::audio("input-1", "gain");
        let mut second = LinkEditor::audio("input-2", "gain");
        first.smoothing_value = 0.5;
        second.smoothing_value = 0.5;
        first.link_payload().expect("payload");
        second.link_payload().expect("payload");
        assert_ne!(first.smoothing_id, second.smoothing_id);
    }

    #[test]
    fn the_smoothing_slider_is_inverted_like_the_js_panel() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.smoothing_value = 0.25;
        assert_eq!(editor.smoothing_slider_value(), MAX_SMOOTHING - 0.25);
        editor.set_smoothing_from_slider(0.8);
        assert!((editor.smoothing_value - 0.2).abs() < 1e-9);
    }

    #[test]
    fn restoring_a_smoothed_link_keeps_its_id_and_value() {
        let link = InputLinkView {
            input_id: "input-1".to_string(),
            kind: "getter".to_string(),
            location: Some("meyda/getFeature".to_string()),
            args: Some(serde_json::json!(["mid", "smoothing-id", 0.4])),
            source: Some("meyda".to_string()),
            min: Some(0.0),
            max: Some(1.0),
            matcher: None,
        };
        let editor = LinkEditor::from_link("input-1", "gain", &link);
        assert_eq!(editor.feature, "mid");
        assert_eq!(editor.smoothing_id.as_deref(), Some("smoothing-id"));
        assert_eq!(editor.smoothing_value, 0.4);
    }

    #[test]
    fn clearing_smoothing_releases_the_id_and_drops_the_args() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.smoothing_value = 0.4;
        editor.link_payload().expect("payload");
        let id = editor.smoothing_id.clone().expect("minted id");
        let action = editor.clear_smoothing().expect("release action");
        assert_eq!(action.action, "meyda/removeSmoothingId");
        assert_eq!(action.payload, serde_json::json!(id));
        let payload = editor.link_payload().expect("payload");
        assert_eq!(payload["args"], serde_json::json!(["bass"]));
        assert!(editor.smoothing_id.is_none());
    }

    #[test]
    fn saving_after_clearing_emits_the_release_before_the_link() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.smoothing_value = 0.4;
        editor.link_payload().expect("payload");
        let id = editor.smoothing_id.clone().expect("minted id");
        editor.smoothing_value = 0.0;
        let actions = editor.save_actions().expect("payload");
        assert_eq!(actions[0].action, "meyda/removeSmoothingId");
        assert_eq!(actions[0].payload, serde_json::json!(id));
        assert_eq!(actions[1].action, "inputs/createInputLink");
        assert_eq!(actions[1].payload["args"], serde_json::json!(["bass"]));
    }

    #[test]
    fn none_removes_the_link_and_releases_smoothing() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.smoothing_value = 0.4;
        editor.link_payload().expect("payload");
        editor.set_audio_feature("none");
        let actions = editor.save_actions().expect("no error");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, "meyda/removeSmoothingId");
        assert_eq!(actions[1].action, "inputs/removeInputLink");
        assert_eq!(actions[1].payload["inputId"], "input-1");
    }

    #[test]
    fn min_and_max_are_written_with_the_link() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.min = -0.5;
        editor.max = 2.0;
        let payload = editor.link_payload().expect("payload");
        assert_eq!(payload["min"], serde_json::json!(-0.5));
        assert_eq!(payload["max"], serde_json::json!(2.0));
    }

    #[test]
    fn expression_create_update_and_clear_payloads() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.expression = "value * 3".to_string();
        assert_eq!(
            editor.expression_actions(),
            vec![crate::UiAction::new(
                "expressions/create",
                serde_json::json!({
                    "expression": "value * 3",
                    "inputId": "input-1",
                    "evaluateEveryFrame": false,
                }),
            )]
        );

        editor.expression_id = Some("expr-1".to_string());
        assert_eq!(
            editor.expression_actions(),
            vec![crate::UiAction::new(
                "expressions/update",
                serde_json::json!({ "id": "expr-1", "expression": "value * 3" }),
            )]
        );

        editor.expression = "value".to_string();
        assert_eq!(
            editor.expression_actions(),
            vec![crate::UiAction::new(
                "expressions/remove",
                serde_json::json!({ "id": "expr-1" }),
            )]
        );

        editor.expression_id = None;
        assert!(editor.expression_actions().is_empty());
    }

    #[test]
    fn a_cleared_expression_is_not_readopted_from_a_stale_snapshot() {
        let assignment = ExpressionView {
            id: "expr-1".to_string(),
            input_id: "input-1".to_string(),
            expression: "value * 2".to_string(),
            evaluate_every_frame: true,
        };
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.sync_expression(Some(&assignment));
        assert_eq!(editor.expression_id.as_deref(), Some("expr-1"));
        assert_eq!(editor.expression, "value * 2");

        editor.mark_expression_cleared();
        editor.sync_expression(Some(&assignment));
        assert!(
            editor.expression_id.is_none(),
            "the stale assignment stays cleared"
        );
        assert_eq!(editor.expression, "value");
        assert!(editor.expression_cleared);

        editor.sync_expression(None);
        assert!(
            !editor.expression_cleared,
            "the latch clears once the engine drops it"
        );
    }

    #[test]
    fn evaluate_every_frame_is_sent_on_create_and_update() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.expression = "value + 1".to_string();
        editor.evaluate_every_frame = true;
        assert_eq!(
            editor.expression_actions()[0].payload["evaluateEveryFrame"],
            serde_json::json!(true)
        );

        editor.expression_id = Some("expr-1".to_string());
        assert_eq!(
            editor.evaluation_action(),
            Some(crate::UiAction::new(
                "expressions/updateEvaluation",
                serde_json::json!({ "id": "expr-1", "evaluateEveryFrame": true }),
            ))
        );
    }

    #[test]
    fn for_input_preloads_the_expression_and_tween_views() {
        let mut snapshot = EngineSnapshot::default();
        snapshot.expressions.push(ExpressionView {
            id: "expr-1".to_string(),
            input_id: "input-1".to_string(),
            expression: "value / 2".to_string(),
            evaluate_every_frame: true,
        });
        snapshot.tweens.insert(
            "input-1".to_string(),
            TweenView {
                id: "input-1".to_string(),
                ..Default::default()
            },
        );
        let editor = LinkEditor::for_input("input-1", "gain", &snapshot);
        assert_eq!(editor.expression, "value / 2");
        assert_eq!(editor.expression_id.as_deref(), Some("expr-1"));
        assert!(editor.evaluate_every_frame);
        assert!(editor.tween.is_some());
    }

    #[test]
    fn tween_links_keep_the_legacy_state_location() {
        // Reused from `TweenControl`, which owns the descriptor payload; this
        // pins the location `Tween.vue` writes for a tween link.
        assert_eq!(
            TweenControl::link_location("input-1"),
            "tweens.tweens['input-1'].value[0]"
        );
    }

    #[test]
    fn invalid_args_are_reported() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.use_state("modules/active");
        editor.kind = "getter".to_string();
        editor.args = "[".to_string();
        assert!(editor.parsed_args().is_err());
    }

    #[test]
    fn state_links_do_not_require_args() {
        let mut editor = LinkEditor::audio("input-1", "gain");
        editor.use_state("modules/active");
        editor.args = "not json".to_string();
        assert_eq!(editor.parsed_args(), Ok(serde_json::Value::Null));
        let payload = editor.link_payload().expect("state payload");
        assert_eq!(payload["type"], "state");
        assert_eq!(payload["location"], "modules/active");
        assert!(payload.get("args").is_none());
    }
}
