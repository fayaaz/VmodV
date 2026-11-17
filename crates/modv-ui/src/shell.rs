//! The native shell: window chrome, menus, status bar and panel composition.
//!
//! P0 composition deliberately mirrors the legacy layout closely enough to
//! judge the immediate-mode fit: Groups on the left, Inspector on the right,
//! a central live-output + Gallery + Scenes area, a status bar and a menu bar.
//! P5 replaces this hand layout with `egui_tiles` docking.

use egui::{Align, Color32, CornerRadius, Layout, Margin, Sense, Stroke, StrokeKind, vec2};

use crate::model::{EngineSnapshot, MediaAction, ModuleMetaView, PropView, gallery_preview_key};
use crate::panels::scene_grid::draw_scene_grid;
use crate::state::{GalleryCategory, SidePanelTab, UiAction, UiCommand, UiState};
use crate::theme;
use crate::widgets::input_link::{link_editor_window, link_row};
use crate::widgets::range_control::chip;
use crate::widgets::{ColorControl, RangeControl, TextureControl, TweenControl, VecControl};

/// Draw the complete native UI for one frame.
pub fn draw(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    state.sync_status(snapshot);
    handle_shortcuts(ui, snapshot, state);
    menu_bar(ui, snapshot, state);
    status_bar(ui, snapshot, state);
    groups_panel(ui, snapshot, state);
    inspector_panel(ui, snapshot, state);
    central_panel(ui, snapshot, state);
    about_window(ui.ctx(), state);
    shortcuts_window(ui.ctx(), state);
    link_editor_window(ui, state);
    bpm_window(ui.ctx(), snapshot, state);
    command_palette(ui.ctx(), snapshot, state);
}

/// Keyboard shortcuts that work globally (outside text fields).
fn handle_shortcuts(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let typing = ui.ctx().memory(|memory| memory.focused().is_some());
    let (save, apply, search, escape, help) = ui.input(|input| {
        let command = input.modifiers.command || input.modifiers.ctrl;
        (
            command && input.key_pressed(egui::Key::S),
            command && input.key_pressed(egui::Key::Enter),
            command && input.key_pressed(egui::Key::K),
            input.key_pressed(egui::Key::Escape),
            input.key_pressed(egui::Key::F1),
        )
    });
    if save {
        state.queue(UiCommand::SaveSnapshot);
    }
    if search {
        state.show_search = !state.show_search;
        state.palette_focus = state.show_search;
        state.command_query.clear();
        state.command_selection = 0;
        if state.show_search {
            state.search_focus = false;
        }
    }
    if help {
        state.show_shortcuts = !state.show_shortcuts;
    }
    if escape {
        state.show_search = false;
        state.show_shortcuts = false;
        state.show_about = false;
        state.scene_grid.renaming = None;
        state.scene_grid.timing_edit = None;
        state.scene_grid.row_names_edit = None;
    }
    if apply && !typing {
        let scene_id = state
            .selected_scene
            .clone()
            .or_else(|| snapshot.performance.current_scene_id.clone())
            .or_else(|| snapshot.scenes.first().map(|scene| scene.id.clone()));
        if let Some(scene_id) = scene_id {
            state.push_action(
                "scenes/applyScene",
                serde_json::json!({"sceneId": scene_id}),
            );
        }
    }
    // Number keys jump to a Scene column (1-based), matching the legacy row
    // shortcuts' "number keys are direct selection" convention.
    if !typing {
        let digit = ui.input(|input| {
            for (index, key) in [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ]
            .into_iter()
            .enumerate()
            {
                if input.key_pressed(key) {
                    return Some(index);
                }
            }
            None
        });
        if let Some(index) = digit
            && let Some(scene) = snapshot.scenes.get(index)
        {
            state.selected_scene = Some(scene.id.clone());
            state.push_action(
                "scenes/applyScene",
                serde_json::json!({"sceneId": scene.id}),
            );
        }
    }
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
                    if ui.button("Save preset").clicked() {
                        state.queue(UiCommand::SavePreset);
                        ui.close();
                    }
                    if ui.button("Save engine snapshot").clicked() {
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
                    if ui.button("Keyboard shortcuts (F1)").clicked() {
                        state.show_shortcuts = true;
                        ui.close();
                    }
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
                        egui::RichText::new(format!("{fps:.1} FPS"))
                            .monospace()
                            .color(if fps >= 55.0 { theme::OK } else { theme::MUTED }),
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("BPM {:.0}", state.status.bpm))
                                    .monospace()
                                    .color(theme::MUTED),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Tempo source and tap tempo")
                        .clicked()
                    {
                        state.show_bpm = true;
                    }
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

fn status_bar(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
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
                    egui::RichText::new(format!("{:.1} FPS", state.status.fps))
                        .monospace()
                        .size(10.0)
                        .color(if state.status.fps >= 55.0 {
                            theme::OK
                        } else {
                            theme::MUTED
                        }),
                );
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
                    egui::RichText::new(format!("{} frames total", state.status.frames_presented))
                        .monospace()
                        .size(10.0)
                        .color(theme::MUTED),
                );
                ui.separator();
                fps_sparkline(ui, state);
                audio_meters(ui, snapshot);
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

/// A small rolling FPS graph, drawn without a plotting dependency.
fn fps_sparkline(ui: &mut egui::Ui, state: &UiState) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 13.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(1), theme::INPUT);
    if state.fps_history.len() < 2 {
        return;
    }
    let max = state
        .fps_history
        .iter()
        .copied()
        .fold(60.0_f32, f32::max)
        .max(1.0);
    let step = rect.width() / (state.fps_history.len() - 1).max(1) as f32;
    let mut previous = None;
    for (index, fps) in state.fps_history.iter().enumerate() {
        let x = rect.left() + index as f32 * step;
        let y = rect.bottom() - (fps / max).clamp(0.0, 1.0) * rect.height();
        let point = egui::pos2(x, y);
        if let Some(previous) = previous {
            painter.line_segment(
                [previous, point],
                Stroke::new(
                    1.0,
                    if *fps >= 55.0 {
                        theme::OK
                    } else {
                        theme::ACCENT
                    },
                ),
            );
        }
        previous = Some(point);
    }
    painter.text(
        rect.right_top() + egui::vec2(-2.0, 0.0),
        egui::Align2::RIGHT_TOP,
        format!("{max:.0}"),
        egui::FontId::monospace(7.0),
        theme::MUTED,
    );
}

fn audio_meters(ui: &mut egui::Ui, snapshot: &EngineSnapshot) {
    for (label, value) in [
        ("B", snapshot.audio.bass),
        ("M", snapshot.audio.mid),
        ("T", snapshot.audio.treble),
    ] {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 12.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(1), theme::INPUT);
        let height = (value.clamp(0.0, 1.0) * rect.height()).max(0.5);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - height),
                rect.right_bottom(),
            ),
            egui::CornerRadius::same(1),
            theme::ACCENT,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(7.0),
            theme::BACKGROUND,
        );
    }
}

fn groups_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    egui::Panel::left("modv-groups")
        .default_size(250.0)
        .min_size(190.0)
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

                    let group_ids: Vec<String> = snapshot
                        .groups
                        .iter()
                        .map(|group| group.id.clone())
                        .collect();
                    for (index, group) in snapshot.groups.iter().enumerate() {
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
                            if response.double_clicked() {
                                state.group_name_draft = group.name.clone();
                                state.group_name_draft_id = Some(group.id.clone());
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("x").on_hover_text("Remove Group").clicked() {
                                    state.push_action(
                                        "groups/removeGroup",
                                        serde_json::json!({ "groupId": group.id }),
                                    );
                                    if state.selected_group.as_deref() == Some(group.id.as_str()) {
                                        state.selected_group = None;
                                        state.selected_module = None;
                                    }
                                }
                                if ui
                                    .add_enabled(
                                        index + 1 < snapshot.groups.len(),
                                        egui::Button::new("v").small().frame(false),
                                    )
                                    .on_hover_text("Move Group down")
                                    .clicked()
                                {
                                    let mut ids = group_ids.clone();
                                    ids.swap(index, index + 1);
                                    state.push_action(
                                        "groups/orderByIds",
                                        serde_json::json!({ "ids": ids }),
                                    );
                                }
                                if ui
                                    .add_enabled(
                                        index > 0,
                                        egui::Button::new("^").small().frame(false),
                                    )
                                    .on_hover_text("Move Group up")
                                    .clicked()
                                {
                                    let mut ids = group_ids.clone();
                                    ids.swap(index, index - 1);
                                    state.push_action(
                                        "groups/orderByIds",
                                        serde_json::json!({ "ids": ids }),
                                    );
                                }
                                ui.label(
                                    egui::RichText::new(format!("{}", group.module_count()))
                                        .monospace()
                                        .size(9.0)
                                        .color(theme::MUTED),
                                );
                            });
                        });

                        if selected {
                            ui.indent(("group-controls", &group.id), |ui| {
                                if state.group_name_draft_id.as_deref() != Some(group.id.as_str()) {
                                    state.group_name_draft = group.name.clone();
                                    state.group_name_draft_id = Some(group.id.clone());
                                }
                                let name_response = ui.add(
                                    egui::TextEdit::singleline(&mut state.group_name_draft)
                                        .desired_width(ui.available_width() - 8.0)
                                        .font(egui::TextStyle::Small),
                                );
                                if name_response.lost_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                {
                                    let name = state.group_name_draft.trim().to_string();
                                    if !name.is_empty() && name != group.name {
                                        state.push_action(
                                            "groups/updateGroupName",
                                            serde_json::json!({
                                                "groupId": group.id,
                                                "name": name,
                                            }),
                                        );
                                    }
                                    state.group_name_draft_id = None;
                                }

                                let mut alpha = group.alpha;
                                if ui
                                    .add(
                                        egui::Slider::new(&mut alpha, 0.0..=1.0)
                                            .text("Alpha")
                                            .max_decimals(3),
                                    )
                                    .changed()
                                {
                                    state.push_action(
                                        "groups/UPDATE_GROUP_BY_KEY",
                                        serde_json::json!({
                                            "groupId": group.id,
                                            "key": "alpha",
                                            "value": alpha,
                                        }),
                                    );
                                }

                                let mut hidden = group.hidden;
                                if ui.checkbox(&mut hidden, "Hidden").changed() {
                                    state.push_action(
                                        "groups/UPDATE_GROUP_BY_KEY",
                                        serde_json::json!({
                                            "groupId": group.id,
                                            "key": "hidden",
                                            "value": hidden,
                                        }),
                                    );
                                }
                                ui.horizontal(|ui| {
                                    let mut inherit = group.inherit;
                                    if ui
                                        .checkbox(&mut inherit, "Inherit")
                                        .on_hover_text("Inherit the previous Group's canvas")
                                        .changed()
                                    {
                                        state.push_action(
                                            "groups/UPDATE_GROUP_BY_KEY",
                                            serde_json::json!({
                                                "groupId": group.id,
                                                "key": "inherit",
                                                "value": inherit,
                                            }),
                                        );
                                    }
                                    let mut clearing = group.clearing;
                                    if ui.checkbox(&mut clearing, "Clear").changed() {
                                        state.push_action(
                                            "groups/UPDATE_GROUP_BY_KEY",
                                            serde_json::json!({
                                                "groupId": group.id,
                                                "key": "clearing",
                                                "value": clearing,
                                            }),
                                        );
                                    }
                                    let mut pipeline = group.pipeline;
                                    if ui.checkbox(&mut pipeline, "Pipeline").changed() {
                                        state.push_action(
                                            "groups/UPDATE_GROUP_BY_KEY",
                                            serde_json::json!({
                                                "groupId": group.id,
                                                "key": "pipeline",
                                                "value": pipeline,
                                            }),
                                        );
                                    }
                                });

                                egui::ComboBox::from_id_salt(("group-blend", &group.id))
                                    .selected_text(&group.composite_operation)
                                    .width(130.0)
                                    .show_ui(ui, |ui| {
                                        for mode in [
                                            "normal",
                                            "screen",
                                            "multiply",
                                            "overlay",
                                            "darken",
                                            "lighten",
                                            "color-dodge",
                                            "color-burn",
                                            "hard-light",
                                            "soft-light",
                                            "difference",
                                            "exclusion",
                                            "hue",
                                            "saturation",
                                            "color",
                                            "luminosity",
                                            "lighter",
                                        ] {
                                            if ui
                                                .selectable_label(
                                                    group.composite_operation == mode,
                                                    mode,
                                                )
                                                .clicked()
                                            {
                                                state.push_action(
                                                    "groups/UPDATE_GROUP_BY_KEY",
                                                    serde_json::json!({
                                                        "groupId": group.id,
                                                        "key": "compositeOperation",
                                                        "value": mode,
                                                    }),
                                                );
                                            }
                                        }
                                    });

                                ui.separator();
                                let modules = snapshot.modules_in_group(&group.id);
                                if modules.is_empty() {
                                    ui.label(
                                        egui::RichText::new("empty")
                                            .italics()
                                            .size(10.0)
                                            .color(theme::MUTED),
                                    );
                                }
                                let module_ids: Vec<String> = group.module_ids.clone();
                                for (module_index, module) in modules.iter().enumerate() {
                                    let selected_module = state.selected_module.as_deref()
                                        == Some(module.id.as_str());
                                    ui.horizontal(|ui| {
                                        let mut module_enabled = module.enabled;
                                        if ui.checkbox(&mut module_enabled, "").changed() {
                                            state.push_action(
                                                "modules/updateMeta",
                                                serde_json::json!({
                                                    "moduleId": module.id,
                                                    "metaKey": "enabled",
                                                    "value": module_enabled,
                                                }),
                                            );
                                        }
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
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button("x")
                                                    .on_hover_text("Remove module")
                                                    .clicked()
                                                {
                                                    state.push_action(
                                                        "groups/REMOVE_MODULE_FROM_GROUP",
                                                        serde_json::json!({
                                                            "groupId": group.id,
                                                            "moduleId": module.id,
                                                        }),
                                                    );
                                                    state.push_action(
                                                        "modules/removeActiveModule",
                                                        serde_json::json!({
                                                            "moduleId": module.id,
                                                        }),
                                                    );
                                                }
                                                if ui
                                                    .add_enabled(
                                                        module_index + 1 < modules.len(),
                                                        egui::Button::new("v").small().frame(false),
                                                    )
                                                    .on_hover_text("Move module down")
                                                    .clicked()
                                                {
                                                    let mut ids = module_ids.clone();
                                                    ids.swap(module_index, module_index + 1);
                                                    state.push_action(
                                                        "groups/REPLACE_GROUP_MODULES",
                                                        serde_json::json!({
                                                            "groupId": group.id,
                                                            "modules": ids,
                                                        }),
                                                    );
                                                }
                                                if ui
                                                    .add_enabled(
                                                        module_index > 0,
                                                        egui::Button::new("^").small().frame(false),
                                                    )
                                                    .on_hover_text("Move module up")
                                                    .clicked()
                                                {
                                                    let mut ids = module_ids.clone();
                                                    ids.swap(module_index, module_index - 1);
                                                    state.push_action(
                                                        "groups/REPLACE_GROUP_MODULES",
                                                        serde_json::json!({
                                                            "groupId": group.id,
                                                            "modules": ids,
                                                        }),
                                                    );
                                                }
                                                if ui
                                                    .small_button("D")
                                                    .on_hover_text("Duplicate module")
                                                    .clicked()
                                                {
                                                    state.push_action(
                                                        "groups/duplicateModule",
                                                        serde_json::json!({
                                                            "groupId": group.id,
                                                            "moduleId": module.id,
                                                        }),
                                                    );
                                                }
                                            },
                                        );
                                    });
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
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        state.side_panel_tab == SidePanelTab::Inspector,
                        egui::RichText::new("INSPECTOR").strong().size(10.0),
                    )
                    .clicked()
                {
                    state.side_panel_tab = SidePanelTab::Inspector;
                }
                if ui
                    .selectable_label(
                        state.side_panel_tab == SidePanelTab::Media,
                        egui::RichText::new("MEDIA").strong().size(10.0),
                    )
                    .clicked()
                {
                    state.side_panel_tab = SidePanelTab::Media;
                }
                if ui
                    .selectable_label(
                        state.side_panel_tab == SidePanelTab::Info,
                        egui::RichText::new("INFO").strong().size(10.0),
                    )
                    .clicked()
                {
                    state.side_panel_tab = SidePanelTab::Info;
                }
                if ui
                    .selectable_label(
                        state.side_panel_tab == SidePanelTab::Plugins,
                        egui::RichText::new("PLUGINS").strong().size(10.0),
                    )
                    .clicked()
                {
                    state.side_panel_tab = SidePanelTab::Plugins;
                }
                if ui
                    .selectable_label(
                        state.side_panel_tab == SidePanelTab::Midi,
                        egui::RichText::new("MIDI").strong().size(10.0),
                    )
                    .clicked()
                {
                    state.side_panel_tab = SidePanelTab::Midi;
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("inspector-scroll")
                .show(ui, |ui| {
                    match state.side_panel_tab {
                        SidePanelTab::Media => {
                            media_panel(ui, snapshot, state);
                            return;
                        }
                        SidePanelTab::Info => {
                            info_panel(ui, snapshot);
                            return;
                        }
                        SidePanelTab::Plugins => {
                            plugins_panel(ui);
                            return;
                        }
                        SidePanelTab::Midi => {
                            midi_panel(ui, snapshot, state);
                            return;
                        }
                        SidePanelTab::Inspector => {}
                    }
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
                        draw_prop(ui, &module.id, prop, snapshot, state);
                        if let Some(input_id) = &prop.input_id {
                            link_row(ui, &prop.name, input_id, snapshot, state);
                        }
                    }
                });
        });
}

fn info_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot) {
    ui.label(
        egui::RichText::new("modV Native")
            .strong()
            .size(14.0)
            .color(theme::ACCENT),
    );
    ui.label(format!("Rust {}", env!("CARGO_PKG_VERSION")));
    ui.separator();
    egui::Grid::new("info-grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Adapter").color(theme::MUTED));
            ui.label(&snapshot.health.adapter);
            ui.end_row();
            ui.label(egui::RichText::new("Backend").color(theme::MUTED));
            ui.label(&snapshot.health.backend);
            ui.end_row();
            ui.label(egui::RichText::new("Frame").color(theme::MUTED));
            ui.label(format!(
                "{:.1} ms  /  {:.1} fps",
                snapshot.frame_ms, snapshot.fps
            ));
            ui.end_row();
            ui.label(egui::RichText::new("Output").color(theme::MUTED));
            ui.label(format!(
                "{}x{}  ({} frames)",
                snapshot.resolution[0], snapshot.resolution[1], snapshot.health.frames_presented
            ));
            ui.end_row();
            ui.label(egui::RichText::new("Audio").color(theme::MUTED));
            ui.label(if snapshot.health.audio_active {
                "engine capture"
            } else {
                "silent"
            });
            ui.end_row();
            ui.label(egui::RichText::new("Tempo").color(theme::MUTED));
            ui.label(format!("{:.0} BPM", snapshot.audio.bpm));
            ui.end_row();
            ui.label(egui::RichText::new("Scenes").color(theme::MUTED));
            ui.label(format!(
                "{} saved  /  {} rows",
                snapshot.scenes.len(),
                snapshot.scene_rows.len()
            ));
            ui.end_row();
        });
    ui.separator();
    if let Some(error) = &snapshot.health.last_error {
        ui.colored_label(theme::DANGER, error);
    } else {
        ui.label(
            egui::RichText::new("No engine errors this session.")
                .size(10.0)
                .color(theme::MUTED),
        );
    }
    ui.label(
        egui::RichText::new(
            "Presets, Scenes, playlists and the media directory stay in the shared VmodV userData tree.",
        )
        .size(9.0)
        .color(theme::MUTED),
    );
}

fn midi_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let midi = &snapshot.midi;
    if !midi.available {
        ui.label(
            egui::RichText::new("MIDI is unavailable")
                .strong()
                .color(theme::DANGER),
        );
        if let Some(error) = &midi.error {
            ui.label(egui::RichText::new(error).size(10.0).color(theme::MUTED));
        }
        return;
    }
    ui.label(
        egui::RichText::new(format!("{} MIDI input(s)", midi.devices.len()))
            .size(10.0)
            .color(theme::MUTED),
    );
    ui.separator();
    if midi.devices.is_empty() {
        ui.label(
            egui::RichText::new("No MIDI devices detected. Hot-plug is polled at 1 Hz.")
                .size(10.0)
                .color(theme::MUTED),
        );
    }
    for device in &midi.devices {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if device.connected { "●" } else { "○" }).color(
                    if device.connected {
                        theme::OK
                    } else {
                        theme::MUTED
                    },
                ),
            );
            ui.label(egui::RichText::new(&device.name).size(11.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(if device.connected { "live" } else { "gone" })
                        .monospace()
                        .size(8.0)
                        .color(theme::MUTED),
                );
            });
        });
        for (label, key, value) in [
            ("Input", "listenForInput", device.listen_for_input),
            ("Clock", "listenForClock", device.listen_for_clock),
            ("CC latch", "ccLatch", device.cc_latch),
            ("Note latch", "noteOnLatch", device.note_on_latch),
        ] {
            let mut checked = value;
            if ui.checkbox(&mut checked, label).changed() {
                state.push_action(
                    "midi/UPDATE_DEVICE",
                    serde_json::json!({
                        "id": device.key,
                        "key": key,
                        "value": checked,
                    }),
                );
            }
        }
        ui.separator();
    }
}

fn plugins_panel(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("JavaScript plugins")
            .strong()
            .size(11.0),
    );
    ui.label(
        egui::RichText::new(
            "D5 keeps plugins, but the native shell has not landed the sandboxed Boa host yet. The switch is deliberately disabled until the consent flow and restricted host API exist; the legacy Electron build remains available for grab-canvas.",
        )
        .size(10.0)
        .color(theme::MUTED),
    );
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut false, "Enable JavaScript plugins (advanced)"),
    );
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Bundled plugins").strong().size(10.0));
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut false, "grab-canvas  (not ported)"),
    );
    ui.separator();
    ui.label(
        egui::RichText::new(
            "Expressions already run in-engine through Boa; this panel is only about the opt-in plugin host.",
        )
        .size(9.0)
        .color(theme::MUTED),
    );
}

fn media_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let media = &snapshot.media;
    if !media.available {
        ui.label(
            egui::RichText::new("Video capture is unavailable")
                .strong()
                .color(theme::DANGER),
        );
        if let Some(error) = &media.error {
            ui.label(egui::RichText::new(error).size(10.0).color(theme::MUTED));
        }
        return;
    }
    if let Some(error) = &media.error {
        ui.colored_label(theme::DANGER, error);
    }
    ui.horizontal(|ui| {
        if ui.button("Test pattern").clicked() {
            state.push_media_action(MediaAction::StartTestPattern);
        }
        ui.label(
            egui::RichText::new(format!(
                "{} camera(s), {} source(s)",
                media.cameras.len(),
                media.sources.len()
            ))
            .size(10.0)
            .color(theme::MUTED),
        );
    });
    ui.separator();
    ui.label(egui::RichText::new("CAMERAS").strong().size(10.0));
    if media.cameras.is_empty() {
        ui.label(
            egui::RichText::new("No cameras detected.")
                .size(10.0)
                .color(theme::MUTED),
        );
    }
    for camera in &media.cameras {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&camera.name).size(11.0));
                ui.label(
                    egui::RichText::new(format!(
                        "{}  {}x{}@{}",
                        camera.id, camera.max_width, camera.max_height, camera.max_fps
                    ))
                    .monospace()
                    .size(8.5)
                    .color(theme::MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Start").clicked() {
                    state.push_media_action(MediaAction::StartCamera {
                        id: camera.id.clone(),
                    });
                }
            });
        });
    }
    ui.separator();
    ui.label(egui::RichText::new("SOURCES").strong().size(10.0));
    if media.sources.is_empty() {
        ui.label(
            egui::RichText::new("No running sources.")
                .size(10.0)
                .color(theme::MUTED),
        );
    }
    for source in &media.sources {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{}  #{}  {}",
                        source.source_id, source.index, source.state
                    ))
                    .size(10.0),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{}x{}  {} frames  {} dropped",
                        source.width, source.height, source.frames, source.dropped
                    ))
                    .monospace()
                    .size(8.5)
                    .color(theme::MUTED),
                );
                if let Some(error) = &source.last_error {
                    ui.colored_label(theme::DANGER, error);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Stop").clicked() {
                    state.push_media_action(MediaAction::Stop {
                        source_id: source.source_id.clone(),
                    });
                }
            });
        });
    }
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

fn draw_prop(
    ui: &mut egui::Ui,
    module_id: &str,
    prop: &PropView,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
) {
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
        "color" => {
            let mut control = ColorControl::new(&prop.name, &prop.value);
            if control.draw(ui) {
                state.push_action(
                    "modules/updateProp",
                    serde_json::json!({
                        "moduleId": module_id,
                        "prop": prop.name,
                        "data": { "value": control.to_value() },
                    }),
                );
            }
        }
        "vec2" | "vec3" | "vec4" => {
            let lanes = match prop.prop_type.as_str() {
                "vec2" => 2,
                "vec3" => 3,
                _ => 4,
            };
            let mut control = VecControl::new(
                &prop.name,
                &prop.value,
                lanes,
                prop.min.unwrap_or(-1.0),
                prop.max.unwrap_or(1.0),
                prop.step.unwrap_or(0.001),
            );
            if control.draw(ui) {
                state.push_action(
                    "modules/updateProp",
                    serde_json::json!({
                        "moduleId": module_id,
                        "prop": prop.name,
                        "data": { "value": control.to_value() },
                    }),
                );
            }
        }
        "tween" => {
            let mut control = TweenControl::new(
                &prop.name,
                prop.input_id.clone(),
                prop.tween.clone(),
                prop.min.unwrap_or(0.0),
                prop.max.unwrap_or(1.0),
            );
            control.draw(ui, state);
        }
        "event" => {
            if ui.button(&prop.name).clicked() {
                state.push_action(
                    "modules/updateProp",
                    serde_json::json!({
                        "moduleId": module_id,
                        "prop": prop.name,
                        "data": { "value": 1 },
                    }),
                );
            }
        }
        "palette" => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&prop.name)
                        .size(11.0)
                        .color(theme::MUTED),
                );
                chip(ui, "palette", theme::TWEEN);
                if let serde_json::Value::Array(colors) = &prop.value {
                    for color in colors.iter().take(8) {
                        let control = ColorControl::new("", color);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        let [r, g, b, a] = control.rgba;
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(2),
                            egui::Color32::from_rgba_unmultiplied(
                                (r.clamp(0.0, 1.0) * 255.0) as u8,
                                (g.clamp(0.0, 1.0) * 255.0) as u8,
                                (b.clamp(0.0, 1.0) * 255.0) as u8,
                                (a.clamp(0.0, 1.0) * 255.0) as u8,
                            ),
                        );
                    }
                }
            });
        }
        "texture" => {
            let mut control = TextureControl::new(&prop.name, &prop.value);
            if let Some(value) = control.draw(ui, &snapshot.texture_sources) {
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
        "font" => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&prop.name)
                        .size(11.0)
                        .color(theme::MUTED),
                );
                chip(ui, "font", theme::MUTED);
                ui.label(
                    egui::RichText::new(prop.value.as_str().unwrap_or("default"))
                        .monospace()
                        .size(9.0)
                        .color(theme::MUTED),
                );
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
        let search = ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search modules  (Ctrl+K)")
                .desired_width(180.0),
        );
        if state.search_focus {
            search.request_focus();
            state.search_focus = false;
        }
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
        state.request_gallery_preview(&module.name, true, std::time::Duration::from_millis(150));
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

enum PaletteAction {
    Action(&'static str, serde_json::Value),
    Command(UiCommand),
    AddModule(String),
    Scene(String),
    ShowShortcuts,
}

fn command_palette(ctx: &egui::Context, snapshot: &EngineSnapshot, state: &mut UiState) {
    if !state.show_search {
        return;
    }
    let mut commands: Vec<(String, PaletteAction)> = vec![
        (
            "Start/stop performance mode".to_string(),
            PaletteAction::Action(
                "scenes/setPerformanceEnabled",
                serde_json::json!({ "enabled": !snapshot.performance.enabled }),
            ),
        ),
        (
            "Create Group".to_string(),
            PaletteAction::Action("groups/createGroup", serde_json::json!({})),
        ),
        (
            "Add empty Scene".to_string(),
            PaletteAction::Action(
                "scenes/addEmptyScene",
                serde_json::json!({ "name": "New Scene" }),
            ),
        ),
        (
            "Save snapshot".to_string(),
            PaletteAction::Command(UiCommand::SaveSnapshot),
        ),
        (
            "Open output window".to_string(),
            PaletteAction::Command(UiCommand::OpenOutputWindow),
        ),
        (
            "Close output window".to_string(),
            PaletteAction::Command(UiCommand::CloseOutputWindow),
        ),
        (
            "Toggle output fullscreen".to_string(),
            PaletteAction::Command(UiCommand::ToggleOutputFullscreen),
        ),
        (
            "Show keyboard shortcuts".to_string(),
            PaletteAction::ShowShortcuts,
        ),
    ];
    for scene in &snapshot.scenes {
        commands.push((
            format!("Apply Scene: {}", scene.name),
            PaletteAction::Scene(scene.id.clone()),
        ));
    }
    if let Some(group_id) = state.selected_group.clone() {
        for module in snapshot.registered.iter().take(80) {
            commands.push((
                format!("Add module: {} (to selected Group)", module.name),
                PaletteAction::AddModule(format!("{}|{}", module.name, group_id)),
            ));
        }
    }

    let query = state.command_query.trim().to_ascii_lowercase();
    let filtered: Vec<&(String, PaletteAction)> = commands
        .iter()
        .filter(|(label, _)| query.is_empty() || label.to_ascii_lowercase().contains(&query))
        .take(14)
        .collect();
    if state.command_selection >= filtered.len() {
        state.command_selection = filtered.len().saturating_sub(1);
    }

    let mut open = true;
    let mut cancel = false;
    let mut execute = None;
    egui::Window::new("Command palette")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .fixed_pos(egui::pos2(
            ctx.content_rect().center().x - 260.0,
            ctx.content_rect().top() + 80.0,
        ))
        .show(ctx, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.command_query)
                    .hint_text("Type a command, Scene or module  (Enter to run)")
                    .desired_width(f32::INFINITY),
            );
            if state.palette_focus {
                response.request_focus();
                state.palette_focus = false;
            }
            if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                state.command_selection =
                    (state.command_selection + 1).min(filtered.len().saturating_sub(1));
            }
            if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                state.command_selection = state.command_selection.saturating_sub(1);
            }
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            ui.separator();
            if filtered.is_empty() {
                ui.label(
                    egui::RichText::new("No matching command")
                        .size(11.0)
                        .color(theme::MUTED),
                );
            }
            for (index, (label, _)) in filtered.iter().enumerate() {
                let selected = index == state.command_selection;
                if ui
                    .selectable_label(selected, egui::RichText::new(label).size(11.0))
                    .clicked()
                {
                    execute = Some(index);
                }
            }
            if ui.input(|input| input.key_pressed(egui::Key::Enter))
                && let Some((_, action)) = filtered.get(state.command_selection)
            {
                match action {
                    PaletteAction::Action(action, payload) => {
                        state.push_action(*action, payload.clone());
                    }
                    PaletteAction::Command(command) => state.queue(command.clone()),
                    PaletteAction::AddModule(value) => {
                        if let Some((module, group)) = value.split_once('|') {
                            state
                                .actions
                                .push(UiAction::add_module_to_group(module, group));
                        }
                    }
                    PaletteAction::Scene(scene_id) => {
                        state.push_action(
                            "scenes/applyScene",
                            serde_json::json!({ "sceneId": scene_id }),
                        );
                    }
                    PaletteAction::ShowShortcuts => state.show_shortcuts = true,
                }
                state.show_search = false;
            }
        });

    if let Some(index) = execute
        && let Some((_, action)) = filtered.get(index)
    {
        match action {
            PaletteAction::Action(action, payload) => state.push_action(*action, payload.clone()),
            PaletteAction::Command(command) => state.queue(command.clone()),
            PaletteAction::AddModule(value) => {
                if let Some((module, group)) = value.split_once('|') {
                    state
                        .actions
                        .push(UiAction::add_module_to_group(module, group));
                }
            }
            PaletteAction::Scene(scene_id) => {
                state.push_action(
                    "scenes/applyScene",
                    serde_json::json!({ "sceneId": scene_id }),
                );
            }
            PaletteAction::ShowShortcuts => state.show_shortcuts = true,
        }
        state.show_search = false;
    }
    if cancel || !open {
        state.show_search = false;
    }
}

fn bpm_window(ctx: &egui::Context, snapshot: &EngineSnapshot, state: &mut UiState) {
    let mut open = state.show_bpm;
    egui::Window::new("Tempo")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Source");
                egui::ComboBox::from_id_salt("bpm-source")
                    .selected_text(&state.bpm_source)
                    .show_ui(ui, |ui| {
                        for source in ["beatdetektor", "midi", "tap"] {
                            if ui
                                .selectable_label(state.bpm_source == source, source)
                                .clicked()
                            {
                                state.bpm_source = source.to_string();
                                state.push_action(
                                    "beats/SET_BPM_SOURCE",
                                    serde_json::json!({ "source": source }),
                                );
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(format!("Detected {:.1} BPM", snapshot.audio.bpm));
                if ui.button("Tap").clicked() {
                    let now = std::time::Instant::now();
                    state
                        .tap_times
                        .retain(|tap| now.saturating_duration_since(*tap).as_secs_f64() <= 2.0);
                    state.tap_times.push(now);
                    if state.tap_times.len() >= 2 {
                        let intervals: Vec<f64> = state
                            .tap_times
                            .windows(2)
                            .map(|pair| pair[1].saturating_duration_since(pair[0]).as_secs_f64())
                            .filter(|interval| *interval > 0.0)
                            .collect();
                        if !intervals.is_empty() {
                            let average = intervals.iter().sum::<f64>() / intervals.len() as f64;
                            let bpm = 60.0 / average;
                            if bpm.is_finite() && (30.0..=300.0).contains(&bpm) {
                                state.bpm_source = "tap".to_string();
                                state.push_action(
                                    "beats/SET_BPM_SOURCE",
                                    serde_json::json!({ "source": "tap" }),
                                );
                                state.push_action(
                                    "beats/setBpm",
                                    serde_json::json!({ "bpm": bpm, "source": "tap" }),
                                );
                            }
                        }
                    }
                }
                if ui.button("Reset taps").clicked() {
                    state.tap_times.clear();
                }
            });
            ui.label(
                egui::RichText::new(format!("{} tap(s) in the last 2 s", state.tap_times.len()))
                    .size(10.0)
                    .color(theme::MUTED),
            );
        });
    state.show_bpm = open;
}

fn shortcuts_window(ctx: &egui::Context, state: &mut UiState) {
    egui::Window::new("Keyboard shortcuts")
        .open(&mut state.show_shortcuts)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let rows = [
                ("Ctrl/Cmd+K", "Focus Gallery search"),
                ("Ctrl/Cmd+S", "Save engine snapshot"),
                ("Ctrl/Cmd+Enter", "Apply the selected Scene"),
                ("1..9", "Apply Scene column 1..9"),
                ("F1", "Show this list"),
                ("F", "Toggle Output Window fullscreen"),
                ("Escape", "Exit fullscreen / close dialogs"),
                ("Ctrl+drag", "Copy a Scene cell to another Scene"),
            ];
            egui::Grid::new("shortcut-grid")
                .num_columns(2)
                .spacing([18.0, 4.0])
                .show(ui, |ui| {
                    for (key, action) in rows {
                        ui.label(egui::RichText::new(key).monospace().color(theme::ACCENT));
                        ui.label(action);
                        ui.end_row();
                    }
                });
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
