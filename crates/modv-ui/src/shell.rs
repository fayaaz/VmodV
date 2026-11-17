//! The native shell: window chrome, menus, status bar and panel composition.
//!
//! P0 composition deliberately mirrors the legacy layout closely enough to
//! judge the immediate-mode fit: Groups on the left, Inspector on the right,
//! a central live-output + Gallery + Scenes area, a status bar and a menu bar.
//! P5 replaces this hand layout with `egui_tiles` docking.

use egui::{Align, Color32, CornerRadius, Layout, Margin, Sense, Stroke, StrokeKind, vec2};

use crate::model::{EngineSnapshot, ModuleMetaView, PropView, gallery_preview_key};
use crate::panels::scene_grid::draw_scene_grid;
use crate::state::{GalleryCategory, UiAction, UiCommand, UiState};
use crate::theme;
use crate::widgets::RangeControl;
use crate::widgets::range_control::chip;

/// Draw the complete native UI for one frame.
pub fn draw(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    state.sync_status(snapshot);
    menu_bar(ui, snapshot, state);
    status_bar(ui, snapshot, state);
    groups_panel(ui, snapshot, state);
    inspector_panel(ui, snapshot, state);
    central_panel(ui, snapshot, state);
    about_window(ui.ctx(), state);
}

fn menu_bar(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    egui::Panel::top("modv-menu")
        .exact_size(28.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL_ALT)
                .inner_margin(Margin::symmetric(8, 3)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label(
                    egui::RichText::new("VmodV")
                        .strong()
                        .color(theme::ACCENT)
                        .size(13.0),
                );
                chip(ui, "NATIVE", theme::ACCENT);
                ui.separator();

                ui.menu_button("File", |ui| {
                    if ui.button("New Group").clicked() {
                        state.push_action("groups/createGroup", serde_json::json!({}));
                        ui.close();
                    }
                    if ui.button("Add Scene").clicked() {
                        state.push_action(
                            "scenes/addEmptyScene",
                            serde_json::json!({"name": "New Scene"}),
                        );
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Save snapshot").clicked() {
                        state.queue(UiCommand::SaveSnapshot);
                        ui.close();
                    }
                    if ui.button("Quit").clicked() {
                        state.queue(UiCommand::Quit);
                        ui.close();
                    }
                });

                ui.menu_button("Edit", |ui| {
                    ui.add_enabled(false, egui::Button::new("Undo"));
                    ui.add_enabled(false, egui::Button::new("Redo"));
                    ui.separator();
                    if ui.button("Preferences (P1)").clicked() {
                        ui.close();
                    }
                });

                ui.menu_button("Scene", |ui| {
                    if ui.button("Apply first Scene").clicked() {
                        if let Some(scene) = snapshot.scenes.first() {
                            state.push_action(
                                "scenes/applyScene",
                                serde_json::json!({"sceneId": scene.id}),
                            );
                        }
                        ui.close();
                    }
                    if ui.button("Add empty Scene").clicked() {
                        state.push_action(
                            "scenes/addEmptyScene",
                            serde_json::json!({"name": "New Scene"}),
                        );
                        ui.close();
                    }
                    ui.separator();
                    let label = if snapshot.performance.enabled {
                        "Stop performance mode"
                    } else {
                        "Start performance mode"
                    };
                    if ui.button(label).clicked() {
                        state.push_action(
                            "scenes/setPerformanceEnabled",
                            serde_json::json!({"enabled": !snapshot.performance.enabled}),
                        );
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Open output window").clicked() {
                        state.queue(UiCommand::OpenOutputWindow);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            snapshot.health.output_open,
                            egui::Button::new("Toggle output fullscreen"),
                        )
                        .clicked()
                    {
                        state.queue(UiCommand::ToggleOutputFullscreen);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            snapshot.health.output_open,
                            egui::Button::new("Close output window"),
                        )
                        .clicked()
                    {
                        state.queue(UiCommand::CloseOutputWindow);
                        ui.close();
                    }
                    ui.separator();
                    ui.checkbox(&mut state.show_p0, "P0 checklist");
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About modV Native").clicked() {
                        state.show_about = true;
                        ui.close();
                    }
                    if ui.button("Open logs (P1)").clicked() {
                        state.queue(UiCommand::FlushLogs);
                        ui.close();
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let fps = state.status.fps;
                    ui.label(
                        egui::RichText::new(format!("{fps:.1} fps"))
                            .monospace()
                            .color(if fps >= 55.0 { theme::OK } else { theme::MUTED }),
                    );
                    ui.label(
                        egui::RichText::new(format!("BPM {:.0}", state.status.bpm))
                            .monospace()
                            .color(theme::MUTED),
                    );
                    if let Some(error) = &state.last_error {
                        ui.label(
                            egui::RichText::new("engine error")
                                .monospace()
                                .color(theme::DANGER),
                        )
                        .on_hover_text(error);
                    }
                });
            });
        });
}

fn status_bar(ui: &mut egui::Ui, _snapshot: &EngineSnapshot, state: &mut UiState) {
    egui::Panel::bottom("modv-status")
        .exact_size(22.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(Margin::symmetric(8, 2)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{:.1} ms", state.status.frame_ms))
                        .monospace()
                        .size(10.0)
                        .color(theme::MUTED),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{}x{}",
                        state.status.resolution[0], state.status.resolution[1]
                    ))
                    .monospace()
                    .size(10.0)
                    .color(theme::MUTED),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("{} frames", state.status.frames_presented))
                        .monospace()
                        .size(10.0)
                        .color(theme::MUTED),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(if state.status.audio_active {
                        "audio: engine"
                    } else {
                        "audio: silent"
                    })
                    .size(10.0)
                    .color(theme::MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    chip(ui, "P0", theme::ACCENT);
                    ui.label(
                        egui::RichText::new(&state.status.backend)
                            .size(10.0)
                            .color(theme::MUTED),
                    )
                    .on_hover_text(&state.status.adapter);
                });
            });
        });
}

fn groups_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    egui::Panel::left("modv-groups")
        .default_size(224.0)
        .min_size(168.0)
        .resizable(true)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(Margin::same(6)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("GROUPS").strong().size(10.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .small_button("+")
                        .on_hover_text("Create a Group")
                        .clicked()
                    {
                        state.push_action("groups/createGroup", serde_json::json!({}));
                    }
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("groups-list")
                .show(ui, |ui| {
                    if snapshot.groups.is_empty() {
                        ui.label(
                            egui::RichText::new("No Groups. Apply a Scene or create one.")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    }

                    for group in &snapshot.groups {
                        let selected = state.selected_group.as_deref() == Some(group.id.as_str());
                        let mut enabled = group.enabled;
                        ui.horizontal(|ui| {
                            if ui
                                .checkbox(&mut enabled, "")
                                .on_hover_text("Enable this Group")
                                .changed()
                            {
                                state.push_action(
                                    "groups/UPDATE_GROUP_BY_KEY",
                                    serde_json::json!({
                                        "groupId": group.id,
                                        "key": "enabled",
                                        "value": if enabled { 1 } else { 0 },
                                    }),
                                );
                            }
                            let response = ui.selectable_label(
                                selected,
                                egui::RichText::new(&group.name).size(11.0),
                            );
                            if response.clicked() {
                                state.selected_group = Some(group.id.clone());
                                state.selected_module = None;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{}", group.module_count()))
                                        .monospace()
                                        .size(9.0)
                                        .color(theme::MUTED),
                                );
                            });
                        });

                        if selected {
                            ui.indent(("group-modules", &group.id), |ui| {
                                let modules = snapshot.modules_in_group(&group.id);
                                if modules.is_empty() {
                                    ui.label(
                                        egui::RichText::new("empty")
                                            .italics()
                                            .size(10.0)
                                            .color(theme::MUTED),
                                    );
                                }
                                for module in modules {
                                    let selected_module = state.selected_module.as_deref()
                                        == Some(module.id.as_str());
                                    let label = format!(
                                        "{}  ({})",
                                        module.module_name,
                                        short_id(&module.id)
                                    );
                                    if ui
                                        .selectable_label(
                                            selected_module,
                                            egui::RichText::new(label).size(10.0),
                                        )
                                        .clicked()
                                    {
                                        state.selected_module = Some(module.id.clone());
                                    }
                                }
                            });
                        }
                    }
                });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("+ Group").clicked() {
                    state.push_action("groups/createGroup", serde_json::json!({}));
                }
                if ui.button("Apply Scene 1").clicked()
                    && let Some(scene) = snapshot.scenes.first()
                {
                    state.push_action(
                        "scenes/applyScene",
                        serde_json::json!({"sceneId": scene.id}),
                    );
                }
            });
        });
}

fn inspector_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    egui::Panel::right("modv-inspector")
        .default_size(320.0)
        .min_size(232.0)
        .resizable(true)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(Margin::same(6)),
        )
        .show(ui, |ui| {
            ui.label(egui::RichText::new("INSPECTOR").strong().size(10.0));
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("inspector-scroll")
                .show(ui, |ui| {
                    let Some(module_id) = state.selected_module.clone() else {
                        inspector_empty(ui, snapshot, state);
                        return;
                    };
                    let Some(module) = snapshot.module(&module_id) else {
                        state.selected_module = None;
                        inspector_empty(ui, snapshot, state);
                        return;
                    };

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&module.module_name).strong().size(13.0));
                        chip(ui, &module.module_type, theme::MUTED);
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "id {}  |  group {}",
                            short_id(&module.id),
                            short_id(&module.group_id)
                        ))
                        .monospace()
                        .size(9.0)
                        .color(theme::MUTED),
                    );
                    ui.add_space(4.0);

                    let mut enabled = module.enabled;
                    if ui.checkbox(&mut enabled, "Enabled").changed() {
                        state.push_action(
                            "modules/updateMeta",
                            serde_json::json!({
                                "moduleId": module.id,
                                "metaKey": "enabled",
                                "value": enabled,
                            }),
                        );
                    }
                    let mut alpha = module.alpha;
                    if ui
                        .add(
                            egui::Slider::new(&mut alpha, 0.0..=1.0)
                                .text("Alpha")
                                .max_decimals(3),
                        )
                        .changed()
                    {
                        state.push_action(
                            "modules/updateMeta",
                            serde_json::json!({
                                "moduleId": module.id,
                                "metaKey": "alpha",
                                "value": alpha,
                            }),
                        );
                    }
                    ui.separator();

                    if module.props.is_empty() {
                        ui.label(
                            egui::RichText::new("This module has no editable props.")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    }
                    for prop in &module.props {
                        draw_prop(ui, &module.id, prop, state);
                    }
                });
        });
}

fn inspector_empty(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    ui.label(
        egui::RichText::new("Select a Module in the Groups panel.")
            .size(11.0)
            .color(theme::MUTED),
    );
    ui.add_space(8.0);

    if let Some(group) = state.selected_group(snapshot) {
        ui.label(egui::RichText::new("GROUP").strong().size(10.0));
        ui.label(egui::RichText::new(&group.name).size(12.0));
        let mut alpha = group.alpha;
        if ui
            .add(
                egui::Slider::new(&mut alpha, 0.0..=1.0)
                    .text("Alpha")
                    .max_decimals(3),
            )
            .changed()
        {
            let group_id = group.id.clone();
            state.push_action(
                "groups/UPDATE_GROUP_BY_KEY",
                serde_json::json!({
                    "groupId": group_id,
                    "key": "alpha",
                    "value": alpha,
                }),
            );
        }
        let mut hidden = group.hidden;
        if ui.checkbox(&mut hidden, "Hidden").changed() {
            let group_id = group.id.clone();
            state.push_action(
                "groups/UPDATE_GROUP_BY_KEY",
                serde_json::json!({
                    "groupId": group_id,
                    "key": "hidden",
                    "value": hidden,
                }),
            );
        }
        ui.separator();
    }

    ui.label(
        egui::RichText::new("P0 PROBE")
            .strong()
            .size(10.0)
            .color(theme::ACCENT),
    );
    ui.label(
        egui::RichText::new("RangeControl: slider, drag value, reset and E/T badges.")
            .size(10.0)
            .color(theme::MUTED),
    );
    let changed = state.inspector_probe.draw(ui);
    if changed {
        state.inspector_probe_value = state.inspector_probe.value;
    }
    ui.label(
        egui::RichText::new(format!("probe value = {:.4}", state.inspector_probe_value))
            .monospace()
            .size(10.0)
            .color(theme::MUTED),
    );
}

fn draw_prop(ui: &mut egui::Ui, module_id: &str, prop: &PropView, state: &mut UiState) {
    match prop.prop_type.as_str() {
        "float" | "int" => {
            let value = prop.value.as_f64().unwrap_or(0.0);
            let min = prop.min.unwrap_or(0.0);
            let max = prop.max.unwrap_or(1.0);
            let step = prop
                .step
                .unwrap_or(if prop.prop_type == "int" { 1.0 } else { 0.001 });
            let mut control = RangeControl::new(&prop.name, value, min, max, step);
            if let Some(default) = prop.default.as_ref().and_then(serde_json::Value::as_f64) {
                control = control.with_default(default);
            }
            if control.draw(ui) {
                state.push_action(
                    "modules/updateProp",
                    serde_json::json!({
                        "moduleId": module_id,
                        "prop": prop.name,
                        "data": { "value": if prop.prop_type == "int" { serde_json::json!(control.value.round() as i64) } else { serde_json::json!(control.value) } },
                    }),
                );
            }
        }
        "bool" => {
            let mut value = prop.value.as_bool().unwrap_or(false);
            if ui.checkbox(&mut value, &prop.name).changed() {
                state.push_action(
                    "modules/updateProp",
                    serde_json::json!({
                        "moduleId": module_id,
                        "prop": prop.name,
                        "data": { "value": value },
                    }),
                );
            }
        }
        "enum" => {
            let current = prop.value.as_str().unwrap_or("").to_string();
            egui::ComboBox::from_id_salt((module_id, &prop.name))
                .selected_text(if current.is_empty() {
                    prop.name.clone()
                } else {
                    current.clone()
                })
                .show_ui(ui, |ui| {
                    for option in &prop.options {
                        if ui.selectable_label(&current == option, option).clicked() {
                            state.push_action(
                                "modules/updateProp",
                                serde_json::json!({
                                    "moduleId": module_id,
                                    "prop": prop.name,
                                    "data": { "value": option },
                                }),
                            );
                        }
                    }
                });
        }
        "color" | "vec2" | "vec3" | "vec4" | "palette" | "texture" | "tween" | "font" => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&prop.name)
                        .size(11.0)
                        .color(theme::MUTED),
                );
                chip(ui, &prop.prop_type, theme::TWEEN);
            });
        }
        _ => {
            let mut text = match &prop.value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            if ui.text_edit_singleline(&mut text).changed() {
                state.push_action(
                    "modules/updateProp",
                    serde_json::json!({
                        "moduleId": module_id,
                        "prop": prop.name,
                        "data": { "value": text },
                    }),
                );
            }
        }
    }
}

fn central_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::BACKGROUND)
                .inner_margin(Margin::same(8)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("modV Native")
                        .strong()
                        .size(18.0)
                        .color(theme::FOREGROUND),
                );
                chip(ui, "P0 FRAMEWORK SPIKE", theme::ACCENT);
                ui.label(
                    egui::RichText::new("egui + wgpu + in-process engine")
                        .size(11.0)
                        .color(theme::MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Open output").clicked() {
                        state.queue(UiCommand::OpenOutputWindow);
                    }
                    if ui
                        .button(if snapshot.performance.enabled {
                            "Stop performance"
                        } else {
                            "Start performance"
                        })
                        .clicked()
                    {
                        state.push_action(
                            "scenes/setPerformanceEnabled",
                            serde_json::json!({"enabled": !snapshot.performance.enabled}),
                        );
                    }
                });
            });
            ui.add_space(4.0);
            ui.separator();

            ui.columns(2, |columns| {
                preview_panel(&mut columns[0], snapshot, state);
                probe_panel(&mut columns[1], snapshot, state);
            });

            ui.separator();
            gallery_strip(ui, snapshot, state);
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("SCENES").strong().size(10.0));
                ui.label(
                    egui::RichText::new(format!("{} saved", snapshot.scenes.len()))
                        .size(10.0)
                        .color(theme::MUTED),
                );
                if !snapshot.scene_rows.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "rows: {}",
                            snapshot
                                .scene_rows
                                .iter()
                                .take(5)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(" | ")
                        ))
                        .size(10.0)
                        .color(theme::MUTED),
                    );
                }
            });
            draw_scene_grid(
                ui,
                snapshot,
                &mut state.scene_grid,
                state.preview_texture,
                &mut state.actions,
            );
            state
                .preview_requests
                .append(&mut state.scene_grid.preview_requests);
        });
}

fn preview_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    ui.label(egui::RichText::new("LIVE OUTPUT").strong().size(10.0));
    let width = ui.available_width().max(120.0);
    let height = (width * 9.0 / 16.0).clamp(80.0, 300.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), theme::INPUT);
    if let Some(texture_id) = state.preview_texture {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture_id, rect.shrink(1.0), uv, Color32::WHITE);
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for the first composited frame",
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    painter.text(
        rect.left_top() + vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        "LIVE",
        egui::FontId::monospace(9.0),
        theme::ACCENT,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
    }
    ui.label(
        egui::RichText::new(format!(
            "{}x{} compositor main texture, registered with egui-wgpu (zero readback)",
            snapshot.resolution[0], snapshot.resolution[1]
        ))
        .size(9.0)
        .color(theme::MUTED),
    );
}

fn probe_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("P0 CHECKLIST").strong().size(10.0));
        let fps = snapshot.fps;
        let verdict = state.p0.verdict(fps, 55.0);
        chip(
            ui,
            if verdict { "GO" } else { "PENDING" },
            if verdict { theme::OK } else { theme::ACCENT },
        );
    });

    let mut missing_glyph_free = Vec::new();
    for (label, ok, detail) in state.p0.checklist(snapshot.fps) {
        missing_glyph_free.push((label, ok, detail));
    }
    for (label, ok, detail) in missing_glyph_free {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if ok { "[x]" } else { "[ ]" })
                    .monospace()
                    .color(if ok { theme::OK } else { theme::MUTED }),
            );
            ui.label(egui::RichText::new(label).size(10.5).color(if ok {
                theme::FOREGROUND
            } else {
                theme::MUTED
            }))
            .on_hover_text(detail);
        });
    }

    ui.add_space(4.0);
    ui.separator();
    ui.label(
        egui::RichText::new("RANGECONTROL PROBE")
            .strong()
            .size(10.0)
            .color(theme::ACCENT),
    );
    let changed = state.inspector_probe.draw(ui);
    if changed {
        state.inspector_probe_value = state.inspector_probe.value;
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("value").size(10.0).color(theme::MUTED));
        ui.label(
            egui::RichText::new(format!("{:.4}", state.inspector_probe_value))
                .monospace()
                .size(11.0),
        );
        ui.label(
            egui::RichText::new("(E = expression, T = tween)")
                .size(9.0)
                .color(theme::MUTED),
        );
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("New Group").clicked() {
            state.push_action("groups/createGroup", serde_json::json!({}));
        }
        if ui.button("Apply first Scene").clicked()
            && let Some(scene) = snapshot.scenes.first()
        {
            state.push_action(
                "scenes/applyScene",
                serde_json::json!({"sceneId": scene.id}),
            );
        }
    });
}

fn gallery_strip(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("GALLERY").strong().size(10.0));
        for category in GalleryCategory::ALL {
            if ui
                .selectable_label(
                    state.gallery_category == category,
                    egui::RichText::new(category.label()).size(10.0),
                )
                .clicked()
            {
                state.gallery_category = category;
            }
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search modules")
                .desired_width(180.0),
        );
        ui.label(
            egui::RichText::new(format!("{} registered", snapshot.registered.len()))
                .size(10.0)
                .color(theme::MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                egui::RichText::new("double-click a tile to add it to the selected Group")
                    .size(9.0)
                    .color(theme::MUTED),
            );
        });
    });

    let filter = state.search.trim().to_ascii_lowercase();
    let category = state.gallery_category.key();
    let filtered: Vec<&ModuleMetaView> = snapshot
        .registered
        .iter()
        .filter(|module| module.gallery_category() == category)
        .filter(|module| {
            filter.is_empty()
                || module.name.to_ascii_lowercase().contains(&filter)
                || module.author.to_ascii_lowercase().contains(&filter)
        })
        .take(24)
        .collect();

    egui::ScrollArea::horizontal()
        .id_salt("gallery-strip")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if filtered.is_empty() {
                    ui.label(
                        egui::RichText::new("No modules match this filter.")
                            .size(10.0)
                            .color(theme::MUTED),
                    );
                }
                for module in filtered {
                    gallery_tile(ui, module, snapshot, state);
                }
            });
        });
}

fn gallery_tile(
    ui: &mut egui::Ui,
    module: &ModuleMetaView,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
) {
    let size = vec2(92.0, 66.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let selected = state.selected_gallery_module.as_deref() == Some(module.name.as_str());
    let fill = if selected {
        theme::ACCENT_DIM
    } else if response.hovered() {
        theme::PANEL_ALT
    } else {
        theme::PANEL
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), fill);
    if let Some(preview) = snapshot
        .gallery_previews
        .get(&gallery_preview_key(&module.name))
    {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(preview.id, rect.shrink(2.0), uv, Color32::WHITE);
        painter.rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(2),
            Color32::from_black_alpha(55),
        );
    } else {
        painter.rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(2),
            Color32::from_rgb(28, 30, 36),
        );
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(
            1.0,
            if selected {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );
    painter.text(
        rect.left_bottom() + vec2(5.0, -5.0),
        egui::Align2::LEFT_BOTTOM,
        truncate(&module.name, 12),
        egui::FontId::proportional(10.0),
        theme::FOREGROUND,
    );
    painter.text(
        rect.left_top() + vec2(5.0, 4.0),
        egui::Align2::LEFT_TOP,
        truncate(&module.module_type, 8),
        egui::FontId::monospace(8.0),
        theme::MUTED,
    );

    if response.hovered() {
        state.request_gallery_preview(
            &module.name,
            true,
            std::time::Duration::from_millis(150),
        );
    }
    if response.clicked() {
        state.selected_gallery_module = Some(module.name.clone());
    }
    if response.double_clicked() {
        if let Some(group_id) = state.selected_group.clone() {
            state
                .actions
                .push(UiAction::add_module_to_group(&module.name, &group_id));
        } else {
            state.last_error = Some("Select a Group before adding a module.".to_string());
        }
    }
    response.context_menu(|ui| {
        ui.label(egui::RichText::new(&module.name).strong());
        ui.label(
            egui::RichText::new(format!(
                "{} | {} {}",
                module.module_type, module.author, module.version
            ))
            .size(9.0)
            .color(theme::MUTED),
        );
        ui.separator();
        if ui
            .add_enabled(
                state.selected_group.is_some(),
                egui::Button::new("Add to selected Group"),
            )
            .clicked()
        {
            if let Some(group_id) = state.selected_group.clone() {
                state
                    .actions
                    .push(UiAction::add_module_to_group(&module.name, &group_id));
            }
            ui.close();
        }
    });
}

fn about_window(ctx: &egui::Context, state: &mut UiState) {
    egui::Window::new("About modV Native")
        .open(&mut state.show_about)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("modV Native")
                    .strong()
                    .size(18.0)
                    .color(theme::ACCENT),
            );
            ui.label("A native Rust rewrite of modV. The legacy Electron UI and JS engine remain available behind the environment switch until parity is signed off.");
            ui.separator();
            ui.label(format!("Adapter: {}", state.status.adapter));
            ui.label(format!("Backend: {}", state.status.backend));
            ui.label("Data formats, media layout and userData stay unchanged.");
        });
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
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

    #[test]
    fn short_ids_and_truncation_are_stable() {
        assert_eq!(short_id("abcdefghijkl"), "abcdefgh");
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(truncate("short", 12), "short");
        assert_eq!(truncate("a very long module name", 8), "a very ~");
    }
}
