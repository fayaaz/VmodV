//! The native shell: window chrome, menus, status bar and panel composition.
//!
//! The composition deliberately mirrors the legacy layout closely enough to
//! judge the immediate-mode fit: Groups on the left, Inspector on the right,
//! a central live-output + Gallery + Scenes area, a status bar and a menu bar.
//! P5 replaces this hand layout with `egui_tiles` docking.

use egui::{Align, Color32, CornerRadius, Layout, Margin, Sense, Stroke, StrokeKind, vec2};

use crate::layout::{
    LayoutState, MIN_DOCK_WIDTH, TabPlacement, WINDOW_ABOUT, WINDOW_PALETTE, WINDOW_SHORTCUTS,
    WINDOW_TEMPO,
};
use crate::model::{
    COMPOSITE_OPERATIONS, EngineSnapshot, MediaAction, ModuleMetaView, PropView, bpm_with_rate,
    gallery_preview_key, sample_rate_label,
};
use crate::panels::scene_grid::draw_scene_grid;
use crate::state::{
    GalleryCategory, SidePanelTab, UiAction, UiCommand, UiState, filter_gallery_modules,
};
use crate::theme;
use crate::widgets::input_link::{link_editor_window, link_row};
use crate::widgets::range_control::chip;
use crate::widgets::{ColorControl, RangeControl, TextureControl, TweenControl, VecControl};

/// The Groups panel's minimum width, matching `ui_layout::MIN_GROUPS_WIDTH`.
const MIN_GROUPS_WIDTH: f32 = 190.0;
/// The right tab strip's minimum width, matching `ui_layout::MIN_INSPECTOR_WIDTH`.
const MIN_INSPECTOR_WIDTH: f32 = 232.0;
/// The centre column's minimum width, matching `ui_layout::MIN_CENTRE_WIDTH`.
const MIN_CENTRE_WIDTH: f32 = 360.0;
/// The Live Output section's minimum height.
const MIN_LIVE_HEIGHT: f32 = 120.0;
/// The Scenes grid's minimum height.
const MIN_SCENES_HEIGHT: f32 = 140.0;
/// The Gallery section's minimum height.
const MIN_GALLERY_HEIGHT: f32 = 96.0;

/// Draw the complete native UI for one frame.
pub fn draw(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    state.sync_status(snapshot);
    state
        .layout
        .tabs
        .ensure_strip_active(&mut state.side_panel_tab);
    handle_shortcuts(ui, snapshot, state);
    menu_bar(ui, snapshot, state);
    status_bar(ui, snapshot, state);
    groups_panel(ui, snapshot, state);
    inspector_panel(ui, snapshot, state);
    dock_panel(ui, snapshot, state);
    central_panel(ui, snapshot, state);
    floating_tab_windows(ui.ctx(), snapshot, state);
    about_window(ui.ctx(), state);
    shortcuts_window(ui.ctx(), state);
    link_editor_window(ui, snapshot, state);
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
                                egui::RichText::new(bpm_with_rate(
                                    state.status.bpm,
                                    snapshot.audio.sample_rate,
                                ))
                                .monospace()
                                .color(theme::MUTED),
                            )
                            .frame(false),
                        )
                        .on_hover_text(bpm_hover(snapshot.audio.sample_rate))
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

/// The BPM button's tooltip: the rate analysis is running at, and how to open
/// the full tempo window. The rate is the live feed's negotiated rate and is
/// omitted (with an explicit note) while capture is silent.
fn bpm_hover(capture_rate: Option<u32>) -> String {
    match capture_rate {
        Some(rate) => format!(
            "Detected tempo · audio analysis runs at {} · click to open the tempo window",
            sample_rate_label(rate)
        ),
        None => {
            "Detected tempo · audio capture is silent · click to open the tempo window".to_string()
        }
    }
}

/// The shared audio-path summary (`engine · 48 kHz` / `engine` / `silent`).
///
/// The status bar prefixes it with `audio: ` and the Info panel wraps it in
/// parentheses, so both always report the same live capture rate.
fn audio_status_text(active: bool, capture_rate: Option<u32>) -> String {
    match (active, capture_rate) {
        (true, Some(rate)) => format!("engine · {}", sample_rate_label(rate)),
        (true, None) => "engine".to_string(),
        (false, _) => "silent".to_string(),
    }
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
                    egui::RichText::new(bpm_with_rate(
                        state.status.bpm,
                        snapshot.audio.sample_rate,
                    ))
                    .monospace()
                    .size(10.0)
                    .color(theme::MUTED),
                )
                .on_hover_text(bpm_hover(snapshot.audio.sample_rate));
                ui.separator();
                fps_sparkline(ui, state);
                audio_meters(ui, snapshot);
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "audio: {}",
                        audio_status_text(state.status.audio_active, snapshot.audio.sample_rate)
                    ))
                    .size(10.0)
                    .color(theme::MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
    let reserved = reserved_side_width(state);
    let max_width = (ui.available_width() - reserved - MIN_CENTRE_WIDTH).max(MIN_GROUPS_WIDTH);
    let response = egui::Panel::left("modv-groups")
        .default_size(state.layout.groups_width)
        .min_size(MIN_GROUPS_WIDTH)
        .max_size(max_width)
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
                                if let Some(input_id) = &group.alpha_input_id {
                                    link_row(ui, "Group alpha", input_id, snapshot, state);
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
                                        for mode in COMPOSITE_OPERATIONS {
                                            if ui
                                                .selectable_label(
                                                    group.composite_operation == *mode,
                                                    *mode,
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
                                if let Some(input_id) = &group.composite_operation_input_id {
                                    link_row(ui, "Group blend", input_id, snapshot, state);
                                }
                            });
                        }
                        ui.indent(("group-modules", &group.id), |ui| {
                            group_module_list(ui, snapshot, state, group, selected);
                        });
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
    state.layout.groups_width = response.response.rect.width();
}

/// The width already reserved by the right-hand panels that are visible.
fn reserved_side_width(state: &UiState) -> f32 {
    let strip = if state.layout.tabs.has_strip_tabs() {
        state.layout.inspector_width
    } else {
        0.0
    };
    let dock = if state.layout.tabs.docked_tabs().is_empty() {
        0.0
    } else {
        state.layout.dock_width
    };
    strip + dock
}

/// One Group's module list: always visible, so every Group in the pane shows
/// what it contains. The per-Group name/alpha/blend editors stay selected-only
/// (they share one draft input); this list is safe to repeat per Group.
fn group_module_list(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
    group: &crate::model::GroupView,
    selected: bool,
) {
    ui.separator();
    let modules = snapshot.modules_in_group(&group.id);
    if modules.is_empty() {
        if selected {
            ui.label(
                egui::RichText::new("empty")
                    .italics()
                    .size(10.0)
                    .color(theme::MUTED),
            );
        }
        return;
    }
    let module_ids: Vec<String> = group.module_ids.clone();
    for (module_index, module) in modules.iter().enumerate() {
        let selected_module = state.selected_module.as_deref() == Some(module.id.as_str());
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
            let label = format!("{}  ({})", module.module_name, short_id(&module.id));
            if ui
                .selectable_label(selected_module, egui::RichText::new(label).size(10.0))
                .clicked()
            {
                state.selected_module = Some(module.id.clone());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
            });
        });
    }
}

fn inspector_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    if !state.layout.tabs.has_strip_tabs() {
        return;
    }
    let dock = if state.layout.tabs.docked_tabs().is_empty() {
        0.0
    } else {
        state.layout.dock_width
    };
    let max_width = (ui.available_width() - state.layout.groups_width - dock - MIN_CENTRE_WIDTH)
        .max(MIN_INSPECTOR_WIDTH);
    let response = egui::Panel::right("modv-inspector")
        .default_size(state.layout.inspector_width)
        .min_size(MIN_INSPECTOR_WIDTH)
        .max_size(max_width)
        .resizable(true)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(Margin::same(6)),
        )
        .show(ui, |ui| {
            tab_strip(ui, snapshot, state);
        });
    state.layout.inspector_width = response.response.rect.width();
}

/// The right-hand tab strip: one tab per placement plus a float control.
fn tab_strip(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for tab in SidePanelTab::ALL {
            if state.layout.tabs.placement(tab) != TabPlacement::Tabs {
                continue;
            }
            let selected = state.side_panel_tab == tab;
            if ui
                .selectable_label(
                    selected,
                    egui::RichText::new(tab.title().to_uppercase())
                        .strong()
                        .size(10.0),
                )
                .clicked()
            {
                state.side_panel_tab = tab;
            }
            if ui
                .small_button("Detach")
                .on_hover_text(format!("Detach {} into a floating window", tab.title()))
                .clicked()
            {
                state
                    .layout
                    .tabs
                    .place(tab, TabPlacement::Floating, &mut state.side_panel_tab);
            }
        }
    });
    ui.separator();

    let active = state.side_panel_tab;
    egui::ScrollArea::vertical()
        .id_salt("inspector-scroll")
        .show(ui, |ui| {
            tab_content(ui, snapshot, state, active);
        });
}

/// Render one right-panel tab, wherever it is placed.
fn tab_content(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
    tab: SidePanelTab,
) {
    match tab {
        SidePanelTab::Inspector => inspector_content(ui, snapshot, state),
        SidePanelTab::Media => media_panel(ui, snapshot, state),
        SidePanelTab::Audio => audio_panel(ui, snapshot, state),
        SidePanelTab::Info => info_panel(ui, snapshot),
        SidePanelTab::Plugins => plugins_panel(ui),
        SidePanelTab::Midi => midi_panel(ui, snapshot, state),
    }
}

/// The Inspector tab: the selected module's meta, blend and prop editors.
fn inspector_content(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
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
    if let Some(input_id) = &module.alpha_input_id {
        link_row(ui, "Alpha", input_id, snapshot, state);
    }
    ui.horizontal(|ui| {
        ui.label("Blend");
        egui::ComboBox::from_id_salt(("module-blend", &module.id))
            .selected_text(&module.composite_operation)
            .width(130.0)
            .show_ui(ui, |ui| {
                for mode in COMPOSITE_OPERATIONS {
                    if ui
                        .selectable_label(module.composite_operation == *mode, *mode)
                        .clicked()
                    {
                        state.push_action(
                            "modules/updateMeta",
                            serde_json::json!({
                                "moduleId": module.id,
                                "metaKey": "compositeOperation",
                                "value": mode,
                            }),
                        );
                    }
                }
            });
    });
    if let Some(input_id) = &module.composite_operation_input_id {
        link_row(ui, "Blend", input_id, snapshot, state);
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
        // The data-type component sub-inputs (Vec lanes,
        // Color channels) are listed under their parent prop,
        // each with its own link row — the expansion the JS
        // InputConfig reaches through the per-component
        // focused inputs (`ColorControl.focusInput`).
        if !prop.components.is_empty() {
            ui.indent(("prop-components", &prop.name), |ui| {
                for component in &prop.components {
                    link_row(
                        ui,
                        &format!("{}.{}", prop.name, component.key),
                        &component.input_id,
                        snapshot,
                        state,
                    );
                }
            });
        }
    }
}

/// The separate dock column between the centre and the right tab strip.
///
/// Hidden while no tab is docked. Several docked tabs stack with a header and
/// separator each, inside one scroll area.
fn dock_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let docked = state.layout.tabs.docked_tabs();
    if docked.is_empty() {
        return;
    }
    let max_width = (ui.available_width() - MIN_CENTRE_WIDTH).max(MIN_DOCK_WIDTH);
    let response = egui::Panel::right("modv-dock")
        .default_size(state.layout.dock_width)
        .min_size(MIN_DOCK_WIDTH)
        .max_size(max_width)
        .resizable(true)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL_ALT)
                .inner_margin(Margin::same(6)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("modv-dock-scroll")
                .show(ui, |ui| {
                    for (index, tab) in docked.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(tab.title().to_uppercase())
                                    .strong()
                                    .size(10.0),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .small_button("Back to tabs")
                                    .on_hover_text("Return this panel to the tab strip")
                                    .clicked()
                                {
                                    state.layout.tabs.place(
                                        *tab,
                                        TabPlacement::Tabs,
                                        &mut state.side_panel_tab,
                                    );
                                }
                                if ui
                                    .small_button("Float")
                                    .on_hover_text("Open this panel in a floating window")
                                    .clicked()
                                {
                                    state.layout.tabs.place(
                                        *tab,
                                        TabPlacement::Floating,
                                        &mut state.side_panel_tab,
                                    );
                                }
                            });
                        });
                        ui.add_space(2.0);
                        tab_content(ui, snapshot, state, *tab);
                    }
                });
        });
    state.layout.dock_width = response.response.rect.width();
}

/// Every tab placed in a floating window, with dock controls in its header.
fn floating_tab_windows(ctx: &egui::Context, snapshot: &EngineSnapshot, state: &mut UiState) {
    for tab in state.layout.tabs.floating_tabs() {
        let window_id = LayoutState::tab_window_id(tab);
        let mut open = true;
        let default_pos = [
            240.0 + tab.index() as f32 * 26.0,
            90.0 + tab.index() as f32 * 26.0,
        ];
        let window = restore_window(
            egui::Window::new(tab.title())
                .id(egui::Id::unique(("modv-tab-window", tab.key())))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .min_width(240.0)
                .min_height(200.0)
                .default_pos(default_pos)
                .default_size([360.0, 460.0]),
            &window_id,
            &state.layout,
        );

        let mut dock_tabs = false;
        let mut dock_area = false;
        let response = window.show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button("Dock to tabs")
                    .on_hover_text("Return this panel to the right-hand tab strip")
                    .clicked()
                {
                    dock_tabs = true;
                }
                if ui
                    .button("Dock to area")
                    .on_hover_text("Move this panel into its own dock column")
                    .clicked()
                {
                    dock_area = true;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(("tab-window-scroll", tab.key()))
                .show(ui, |ui| {
                    tab_content(ui, snapshot, state, tab);
                });
        });

        if let Some(response) = response {
            state
                .layout
                .record_window_rect(&window_id, response.response.rect);
        }

        if dock_tabs {
            state
                .layout
                .tabs
                .place(tab, TabPlacement::Tabs, &mut state.side_panel_tab);
        } else if dock_area {
            state
                .layout
                .tabs
                .place(tab, TabPlacement::Docked, &mut state.side_panel_tab);
        } else if !open {
            state
                .layout
                .tabs
                .place(tab, TabPlacement::Tabs, &mut state.side_panel_tab);
        }
    }
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
            ui.label(format!(
                "{}  ({})",
                snapshot.audio_selection.describe(),
                audio_status_text(snapshot.health.audio_active, snapshot.audio.sample_rate)
            ));
            ui.end_row();
            ui.label(egui::RichText::new("Tempo").color(theme::MUTED));
            ui.label(bpm_with_rate(
                snapshot.audio.bpm,
                snapshot.audio.sample_rate,
            ));
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

fn audio_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let selection = &snapshot.audio_selection;
    audio_active_banner(ui, snapshot);

    if let Some(error) = &selection.error {
        ui.add_space(4.0);
        ui.colored_label(theme::DANGER, error);
        ui.label(
            egui::RichText::new("The previous source is still running.")
                .size(9.0)
                .color(theme::MUTED),
        );
    }

    ui.separator();
    ui.label(egui::RichText::new("OUTPUT MONITOR").strong().size(9.0));
    ui.label(
        egui::RichText::new(
            "Capture what this machine is playing (PipeWire system audio loopback).",
        )
        .size(9.0)
        .color(theme::MUTED),
    );
    if audio_choice_row(
        ui,
        selection.is_pipewire(),
        "System audio (PipeWire monitor)",
        "PipeWire monitor · captures everything your speakers play",
        "System audio loopback (pipewire)",
    ) {
        state.queue(UiCommand::SetAudioSource {
            source: "pipewire".to_string(),
            device: None,
        });
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("INPUT DEVICES").strong().size(9.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let count = snapshot.audio_devices.len();
            ui.label(
                egui::RichText::new(format!(
                    "{count} input{}",
                    if count == 1 { "" } else { "s" }
                ))
                .monospace()
                .size(9.0)
                .color(theme::MUTED),
            );
        });
    });
    ui.label(
        egui::RichText::new(
            "Microphones and audio interfaces. The system default follows hot-plug.",
        )
        .size(9.0)
        .color(theme::MUTED),
    );
    if audio_choice_row(
        ui,
        selection.is_default_input(),
        "System default input",
        "Chosen by the system · follows connected devices",
        "Let the system choose the default input (--audio-source input)",
    ) {
        state.queue(UiCommand::SetAudioSource {
            source: "input".to_string(),
            device: None,
        });
    }
    if snapshot.audio_devices.is_empty() {
        ui.label(
            egui::RichText::new("No usable input devices found. Connect one and Refresh.")
                .size(10.0)
                .color(theme::MUTED),
        );
    }
    for device in &snapshot.audio_devices {
        if audio_choice_row(
            ui,
            selection.matches_device(device),
            &device.name,
            &device.detail(),
            &format!("Device id: {}\n{}", device.id, device.name),
        ) {
            state.queue(UiCommand::SetAudioSource {
                source: "input".to_string(),
                device: Some(device.id.clone()),
            });
        }
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Refresh devices").clicked() {
            state.queue(UiCommand::RefreshAudioDevices);
        }
        ui.label(
            egui::RichText::new("Re-scans ALSA/PipeWire inputs")
                .size(9.0)
                .color(theme::MUTED),
        );
    });
}

/// The prominent "in use" banner above the picker.
///
/// The green dot means the engine is actually receiving frames
/// (`health.audio_active`); a selected-but-silent source stays visible so the
/// panel never hides what capture is configured to use.
fn audio_active_banner(ui: &mut egui::Ui, snapshot: &EngineSnapshot) {
    let selection = &snapshot.audio_selection;
    let live = snapshot.health.audio_active;
    let colour = if live { theme::OK } else { theme::MUTED };
    let state = if live { "IN USE" } else { "SELECTED · SILENT" };

    egui::Frame::NONE
        .fill(theme::PANEL_ALT)
        .stroke(Stroke::new(
            1.0,
            if live { theme::OK } else { theme::BORDER },
        ))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(8, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, colour);
                ui.label(
                    egui::RichText::new(state)
                        .strong()
                        .monospace()
                        .size(9.0)
                        .color(colour),
                );
                ui.label(
                    egui::RichText::new(selection.active_label(&snapshot.audio_devices))
                        .strong()
                        .size(13.0),
                );
            });
            ui.label(
                egui::RichText::new(selection.active_subtitle(&snapshot.audio_devices)).size(10.0),
            );
            ui.label(
                egui::RichText::new(selection.active_detail(&snapshot.audio_devices))
                    .monospace()
                    .size(8.5)
                    .color(theme::MUTED),
            );
        });
}

/// One radio row with a selected background and a hover-only device id.
/// Returns whether it was clicked.
fn audio_choice_row(
    ui: &mut egui::Ui,
    selected: bool,
    title: &str,
    detail: &str,
    hover: &str,
) -> bool {
    let mut clicked = false;
    egui::Frame::NONE
        .fill(if selected {
            theme::ACCENT_DIM
        } else {
            Color32::TRANSPARENT
        })
        .corner_radius(CornerRadius::same(2))
        .inner_margin(Margin::symmetric(4, 3))
        .show(ui, |ui| {
            let response = ui.radio(selected, egui::RichText::new(title).size(11.0));
            clicked = response.clicked();
            response.on_hover_text(hover);
            if !detail.is_empty() {
                ui.label(egui::RichText::new(detail).size(8.5).color(theme::MUTED))
                    .on_hover_text(hover);
            }
        });
    clicked
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
    ui.label(
        egui::RichText::new("Ticked cameras run in order; #N is the Webcam module's Camera index.")
            .size(9.0)
            .color(theme::MUTED),
    );
    if media.cameras.is_empty() {
        ui.label(
            egui::RichText::new("No cameras detected.")
                .size(10.0)
                .color(theme::MUTED),
        );
    }
    let running = media.running_cameras();
    for camera in &media.cameras {
        media_camera_row(ui, camera, &running, state);
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
                let title = if source.index > 0 {
                    format!("{}  #{}  {}", source.source_id, source.index, source.state)
                } else {
                    format!("{}  {}", source.source_id, source.state)
                };
                ui.label(egui::RichText::new(title).size(10.0));
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

/// One camera row: checkbox (checked = running), Webcam-index badge and the
/// reorder arrows for a running camera.
///
/// Checking appends the camera to the persisted selection and starts it;
/// unchecking stops it and drops it from the selection. The arrows reorder
/// running cameras without restarting their pipelines, so the Webcam module's
/// `camera` prop follows immediately.
fn media_camera_row(
    ui: &mut egui::Ui,
    camera: &crate::model::CameraView,
    running: &[&crate::model::MediaSourceView],
    state: &mut UiState,
) {
    let source_id = format!("camera:{}", camera.id);
    let position = running
        .iter()
        .position(|source| source.source_id == source_id);

    ui.horizontal(|ui| {
        let mut checked = position.is_some();
        let response = ui.checkbox(&mut checked, egui::RichText::new(&camera.name).size(11.0));
        let changed = response.changed();
        response.on_hover_text(
            "Run this camera and add it to the Webcam order\n\
             (the Webcam module's Camera prop indexes this list)",
        );
        if changed {
            if checked {
                state.push_media_action(MediaAction::StartCamera {
                    id: camera.id.clone(),
                    width: camera.selected.width,
                    height: camera.selected.height,
                    fps: camera.selected.fps,
                });
            } else {
                state.push_media_action(MediaAction::Stop {
                    source_id: source_id.clone(),
                });
            }
        }

        if let Some(position) = position {
            let index = running[position].index;
            ui.label(
                egui::RichText::new(format!("#{index}"))
                    .monospace()
                    .strong()
                    .size(9.0)
                    .color(theme::ACCENT),
            )
            .on_hover_text(format!("Webcam index #{index} (the module's Camera prop)"));
        }
        ui.label(
            egui::RichText::new(&camera.id)
                .monospace()
                .size(8.5)
                .color(theme::MUTED),
        );
    });

    if let Some(position) = position {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            media_camera_mode_controls(ui, camera, state);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_move_later = position + 1 < running.len();
                if ui
                    .add_enabled(
                        can_move_later,
                        egui::Button::new(egui::RichText::new("Down").size(9.0)).small(),
                    )
                    .on_hover_text("Move later in the Webcam order")
                    .clicked()
                {
                    state.push_media_action(MediaAction::MoveCamera {
                        id: camera.id.clone(),
                        delta: 1,
                    });
                }
                let can_move_earlier = position > 0;
                if ui
                    .add_enabled(
                        can_move_earlier,
                        egui::Button::new(egui::RichText::new("Up").size(9.0)).small(),
                    )
                    .on_hover_text("Move earlier in the Webcam order")
                    .clicked()
                {
                    state.push_media_action(MediaAction::MoveCamera {
                        id: camera.id.clone(),
                        delta: -1,
                    });
                }
            });
        });
    }
}

/// The Resolution and Frame rate pickers for a running camera.
///
/// Both lists come from the device's advertised caps
/// ([`unique_resolutions`]/[`fps_options`]); a change queues a
/// [`MediaAction::StartCamera`] so the shell restarts the source in the chosen
/// mode and rewrites the saved mode. A camera that reports no caps keeps the
/// documented 1280×720@30 fallback and says so instead of offering fake modes.
fn media_camera_mode_controls(
    ui: &mut egui::Ui,
    camera: &crate::model::CameraView,
    state: &mut UiState,
) {
    let resolutions = crate::model::unique_resolutions(&camera.modes);
    if resolutions.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "{} · mode not reported",
                camera.selected.resolution_label()
            ))
            .monospace()
            .size(8.5)
            .color(theme::MUTED),
        )
        .on_hover_text(
            "The camera did not advertise capture modes; the engine asks for 1280×720@30",
        );
        return;
    }

    let mut chosen = camera.selected;
    let mut changed = false;

    egui::ComboBox::from_id_salt(("camera-resolution", &camera.id))
        .selected_text(
            egui::RichText::new(chosen.resolution_label())
                .monospace()
                .size(9.5),
        )
        .width(78.0)
        .show_ui(ui, |ui| {
            for (width, height) in resolutions {
                let label = crate::model::CameraModeView {
                    width,
                    height,
                    fps: 0,
                }
                .resolution_label();
                let selected = width == chosen.width && height == chosen.height;
                if ui.selectable_label(selected, label).clicked()
                    && let Some(mode) =
                        crate::model::mode_for_resolution(&camera.modes, width, height, chosen.fps)
                {
                    chosen = mode;
                    changed = true;
                }
            }
        })
        .response
        .on_hover_text("Capture resolution");

    let rates = crate::model::fps_options(&camera.modes, chosen.width, chosen.height);
    if rates.is_empty() {
        ui.label(
            egui::RichText::new(format!("{} · rate not reported", chosen.fps_label()))
                .monospace()
                .size(8.5)
                .color(theme::MUTED),
        );
    } else {
        egui::ComboBox::from_id_salt(("camera-fps", &camera.id))
            .selected_text(
                egui::RichText::new(chosen.fps_label())
                    .monospace()
                    .size(9.5),
            )
            .width(58.0)
            .show_ui(ui, |ui| {
                for fps in rates {
                    if ui
                        .selectable_label(chosen.fps == fps, format!("{fps} fps"))
                        .clicked()
                    {
                        chosen.fps = fps;
                        changed = true;
                    }
                }
            })
            .response
            .on_hover_text("Capture frame rate");
    }

    if changed {
        state.push_media_action(MediaAction::StartCamera {
            id: camera.id.clone(),
            width: chosen.width,
            height: chosen.height,
            fps: chosen.fps,
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
        if let Some(input_id) = &group.alpha_input_id {
            link_row(ui, "Group alpha", input_id, snapshot, state);
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

            // Live Output and Gallery are resizable top sections; the Scenes
            // grid takes the remainder. The saved heights are applied as the
            // first-show defaults and captured from the real section rects so
            // NativeApp can persist them.
            let total_height = ui.available_height().max(1.0);
            let live_default = if state.layout.live_height > 0.0 {
                state.layout.live_height
            } else {
                (total_height * 0.28).clamp(160.0, 260.0)
            };
            let live_max =
                (total_height - MIN_GALLERY_HEIGHT - MIN_SCENES_HEIGHT).max(MIN_LIVE_HEIGHT);
            let live = egui::Panel::top("modv-live-output")
                .resizable(true)
                .default_size(live_default)
                .min_size(MIN_LIVE_HEIGHT)
                .max_size(live_max)
                .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
                .show(ui, |ui| {
                    preview_panel(ui, snapshot, state);
                });
            state.layout.live_height = live.response.rect.height();

            let remaining = ui.available_height().max(1.0);
            let gallery_default = if state.layout.gallery_height > 0.0 {
                state.layout.gallery_height
            } else {
                (remaining * 0.46)
                    .clamp(MIN_GALLERY_HEIGHT, 340.0)
                    .min((remaining - MIN_SCENES_HEIGHT).max(MIN_GALLERY_HEIGHT))
            };
            let gallery_max = (remaining - MIN_SCENES_HEIGHT).max(MIN_GALLERY_HEIGHT);
            let gallery = egui::Panel::top("modv-gallery")
                .resizable(true)
                .default_size(gallery_default)
                .min_size(MIN_GALLERY_HEIGHT)
                .max_size(gallery_max)
                .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
                .show(ui, |ui| {
                    gallery_strip(ui, snapshot, state);
                });
            state.layout.gallery_height = gallery.response.rect.height();

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
            let live_preview = state.preview_texture;
            draw_scene_grid(
                ui,
                snapshot,
                &mut state.scene_grid,
                live_preview,
                &mut state.actions,
                &mut state.layout,
            );
            state
                .preview_requests
                .append(&mut state.scene_grid.preview_requests);
        });
}

/// Fit a `resolution`-sized frame inside `available` without stretching.
///
/// The frame is centered and letterboxed/pillarboxed within the available
/// area. A zero or otherwise unusable resolution falls back to 16:9.
pub fn fit_preview_rect(available: egui::Vec2, resolution: [u32; 2]) -> egui::Rect {
    let available = vec2(available.x.max(1.0), available.y.max(1.0));
    let (source_width, source_height) = if resolution[0] > 0 && resolution[1] > 0 {
        (resolution[0] as f32, resolution[1] as f32)
    } else {
        (16.0, 9.0)
    };
    let scale = (available.x / source_width).min(available.y / source_height);
    let size = vec2(source_width * scale, source_height * scale);
    let min = (available - size) * 0.5;
    egui::Rect::from_min_size(egui::pos2(min.x, min.y), size)
}

fn preview_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    // The section is resizable; the preview fills it and letterboxes inside.
    // The title is painted over the image rather than allocated above it so the
    // section's content is exactly its frame height (a layout label here makes
    // the resizable section grow by one text row on every frame).
    let width = ui.available_width().max(120.0);
    let height = ui.available_height().max(80.0);
    let (container, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    // The container spans the full central width; the output is fitted inside
    // it and the leftover panel background forms the letterbox bars.
    let image_rect =
        fit_preview_rect(container.size(), snapshot.resolution).translate(container.min.to_vec2());
    let painter = ui.painter();
    painter.rect_filled(image_rect, CornerRadius::same(2), theme::INPUT);
    if let Some(texture_id) = state.preview_texture {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture_id, image_rect.shrink(1.0), uv, Color32::WHITE);
    } else {
        painter.text(
            image_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for the first composited frame",
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );
    }
    painter.rect_stroke(
        image_rect,
        CornerRadius::same(2),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    painter.text(
        image_rect.left_top() + vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        "LIVE OUTPUT",
        egui::FontId::monospace(9.0),
        theme::ACCENT,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
    }
    let aspect = if snapshot.resolution[0] > 0 && snapshot.resolution[1] > 0 {
        snapshot.resolution[0] as f64 / snapshot.resolution[1] as f64
    } else {
        16.0 / 9.0
    };
    // Painted, not allocated: a trailing label here would make the resizable
    // section's content one row taller than its frame and the split would
    // creep by that row on every frame.
    painter.text(
        image_rect.left_bottom() + vec2(6.0, -4.0),
        egui::Align2::LEFT_BOTTOM,
        format!(
            "{}x{} ({aspect:.2}:1) compositor main texture",
            snapshot.resolution[0], snapshot.resolution[1]
        ),
        egui::FontId::proportional(9.0),
        theme::MUTED,
    );
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

    let filtered =
        filter_gallery_modules(&snapshot.registered, state.gallery_category, &state.search);
    // The Gallery section is resizable: the grid fills it and scrolls
    // vertically inside, instead of imposing its own height on the split.
    let available_height = ui.available_height().max(1.0);

    egui::ScrollArea::vertical()
        .id_salt("gallery-grid")
        .max_height(available_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
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
    let size = vec2(108.0, 76.0);
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
    // The full name is laid out once and never shortened: a name that fits
    // sits at the strip's left edge, an overlong one marquees while hovered.
    // The strip clips it away from the tile border and the type label.
    let name_strip = gallery_name_strip(rect);
    let name_galley = painter.layout_no_wrap(
        module.name.clone(),
        egui::FontId::proportional(10.0),
        theme::FOREGROUND,
    );
    let name_layout = gallery_name_layout(&module.name, name_galley.size().x, name_strip.width());
    let now = ui.input(|input| input.time);
    let hovered_name = state
        .gallery_name_hover
        .as_ref()
        .filter(|(name, _)| name == &module.name)
        .map(|(_, started)| *started);
    if name_layout.overflow > 0.0 && response.hovered() {
        if hovered_name.is_none() {
            state.gallery_name_hover = Some((module.name.clone(), now));
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    } else if hovered_name.is_some() {
        state.gallery_name_hover = None;
    }
    let elapsed = if response.hovered() {
        state
            .gallery_name_hover
            .as_ref()
            .filter(|(name, _)| name == &module.name)
            .map(|(_, started)| (now - started).max(0.0) as f32)
            .unwrap_or(0.0)
    } else {
        0.0
    };
    let name_offset = marquee_offset(
        elapsed,
        name_layout.overflow,
        GALLERY_NAME_SPEED,
        GALLERY_NAME_HOLD_START,
        GALLERY_NAME_HOLD_END,
    );
    let name_pos = egui::pos2(
        name_strip.left() - name_offset,
        name_strip.bottom() - 3.0 - name_galley.size().y,
    );
    painter
        .with_clip_rect(name_strip)
        .galley(name_pos, name_galley, theme::FOREGROUND);
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
    let window = restore_window(
        egui::Window::new("Command palette")
            .id(egui::Id::unique("modv-window:palette"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .min_width(320.0)
            .min_height(200.0)
            .default_size([520.0, 380.0])
            .default_pos(egui::pos2(
                ctx.content_rect().center().x - 260.0,
                ctx.content_rect().top() + 80.0,
            )),
        WINDOW_PALETTE,
        &state.layout,
    );
    let response = window.show(ctx, |ui| {
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

    if let Some(response) = response {
        state
            .layout
            .record_window_rect(WINDOW_PALETTE, response.response.rect);
    }

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
    let window = restore_window(
        egui::Window::new("Tempo")
            .id(egui::Id::unique("modv-window:tempo"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .min_width(260.0)
            .min_height(180.0)
            .default_size([340.0, 260.0]),
        WINDOW_TEMPO,
        &state.layout,
    );
    let response = window.show(ctx, |ui| {
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
            ui.label(
                egui::RichText::new(match snapshot.audio.sample_rate {
                    Some(rate) => format!("Capture {}", sample_rate_label(rate)),
                    None => "Capture rate unavailable".to_string(),
                })
                .size(10.0)
                .color(theme::MUTED),
            )
            .on_hover_text("The rate the analyser and BPM detector run at");
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
    if let Some(response) = response {
        state
            .layout
            .record_window_rect(WINDOW_TEMPO, response.response.rect);
    }
    state.show_bpm = open;
}

fn shortcuts_window(ctx: &egui::Context, state: &mut UiState) {
    let window = restore_window(
        egui::Window::new("Keyboard shortcuts")
            .id(egui::Id::unique("modv-window:shortcuts"))
            .open(&mut state.show_shortcuts)
            .collapsible(false)
            .resizable(true)
            .min_width(300.0)
            .min_height(220.0)
            .default_size([440.0, 320.0]),
        WINDOW_SHORTCUTS,
        &state.layout,
    );
    let response = window.show(ctx, |ui| {
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
    if let Some(response) = response {
        state
            .layout
            .record_window_rect(WINDOW_SHORTCUTS, response.response.rect);
    }
}

fn about_window(ctx: &egui::Context, state: &mut UiState) {
    let window = restore_window(
        egui::Window::new("About modV Native")
            .id(egui::Id::unique("modv-window:about"))
            .open(&mut state.show_about)
            .collapsible(false)
            .resizable(true)
            .min_width(320.0)
            .min_height(200.0)
            .default_size([420.0, 280.0]),
        WINDOW_ABOUT,
        &state.layout,
    );
    let response = window.show(ctx, |ui| {
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
    if let Some(response) = response {
        state
            .layout
            .record_window_rect(WINDOW_ABOUT, response.response.rect);
    }
}

/// Apply saved outer bounds to a floating window on its first show.
///
/// `egui::Window::show` returns the area's outer rect, which is what the shell
/// records afterwards (title bar included).
fn restore_window<'a>(
    window: egui::Window<'a>,
    id: &str,
    layout: &LayoutState,
) -> egui::Window<'a> {
    if let Some(bounds) = layout.window(id) {
        window.default_pos(bounds.pos()).default_size(bounds.size())
    } else {
        window
    }
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

/// Horizontal marquee speed for overlong Gallery tile names, in points/second.
const GALLERY_NAME_SPEED: f32 = 28.0;
/// How long the marquee rests at the start of the name before scrolling.
const GALLERY_NAME_HOLD_START: f32 = 0.5;
/// How long the marquee rests on the end of the name before returning.
const GALLERY_NAME_HOLD_END: f32 = 0.8;

/// The full Gallery tile name plus how far it overruns the tile's name strip.
#[derive(Debug, Clone, PartialEq)]
struct GalleryNameLayout {
    /// The full module name. It is never shortened with `~` or an ellipsis;
    /// an overlong name marquees instead.
    text: String,
    /// Width of the full name at the tile font size.
    width: f32,
    /// `width - strip_width`, clamped to zero when the name fits.
    overflow: f32,
}

/// Describe the full Gallery tile name against the width of the name strip.
fn gallery_name_layout(name: &str, text_width: f32, strip_width: f32) -> GalleryNameLayout {
    let width = text_width.max(0.0);
    GalleryNameLayout {
        text: name.to_string(),
        width,
        overflow: (width - strip_width).max(0.0),
    }
}

/// The band a Gallery tile's name is painted (and clipped) inside.
fn gallery_name_strip(tile: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(tile.left() + 5.0, tile.bottom() - 22.0),
        egui::pos2(tile.right() - 4.0, tile.bottom() - 2.0),
    )
}

/// Ping-pong marquee offset after `elapsed_secs` of hovering.
///
/// The cycle holds at the start, scrolls left by `overflow_px` at `speed`
/// px/s, holds at the end, and travels back to the start.
fn marquee_offset(
    elapsed_secs: f32,
    overflow_px: f32,
    speed: f32,
    hold_start: f32,
    hold_end: f32,
) -> f32 {
    if !overflow_px.is_finite() || !speed.is_finite() || overflow_px <= 0.0 || speed <= 0.0 {
        return 0.0;
    }
    let hold_start = hold_start.max(0.0);
    let hold_end = hold_end.max(0.0);
    let travel = overflow_px / speed;
    let cycle = hold_start + travel + hold_end + travel;
    if !cycle.is_finite() || cycle <= 0.0 || !elapsed_secs.is_finite() {
        return 0.0;
    }
    let elapsed = elapsed_secs.max(0.0) % cycle;
    if elapsed < hold_start {
        0.0
    } else if elapsed < hold_start + travel {
        ((elapsed - hold_start) * speed).clamp(0.0, overflow_px)
    } else if elapsed < hold_start + travel + hold_end {
        overflow_px
    } else {
        (overflow_px - (elapsed - hold_start - travel - hold_end) * speed).clamp(0.0, overflow_px)
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

    #[test]
    fn gallery_name_stays_full_and_overflows_the_strip() {
        let tile = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), vec2(108.0, 76.0));
        let strip = gallery_name_strip(tile);
        assert!(strip.min.x >= tile.min.x, "strip starts inside the tile");
        assert!(strip.max.x <= tile.max.x, "strip ends inside the tile");
        assert!(strip.min.y >= tile.min.y, "strip top inside the tile");
        assert!(strip.max.y <= tile.max.y, "strip bottom inside the tile");
        assert!(
            strip.min.y > tile.min.y + 20.0,
            "strip stays clear of the type label at the top"
        );

        let short = gallery_name_layout("Ball", 18.0, strip.width());
        assert_eq!(short.text, "Ball");
        assert_eq!(short.width, 18.0);
        assert_eq!(short.overflow, 0.0, "a name that fits has no overflow");

        let long_name = "It Is Coming For You";
        let long = gallery_name_layout(long_name, strip.width() + 41.0, strip.width());
        assert_eq!(
            long.text, long_name,
            "the full name is kept; nothing is ~-shortened"
        );
        assert!(!long.text.contains('~'));
        assert_eq!(long.overflow, 41.0, "overflow is text_width - strip_width");
    }

    #[test]
    fn marquee_ignores_names_that_fit() {
        for elapsed in [0.0, 0.1, 0.9, 7.5, 120.0] {
            assert_eq!(
                marquee_offset(
                    elapsed,
                    0.0,
                    GALLERY_NAME_SPEED,
                    GALLERY_NAME_HOLD_START,
                    GALLERY_NAME_HOLD_END,
                ),
                0.0
            );
        }
        assert_eq!(
            marquee_offset(
                1.0,
                -3.0,
                GALLERY_NAME_SPEED,
                GALLERY_NAME_HOLD_START,
                GALLERY_NAME_HOLD_END,
            ),
            0.0,
            "negative overflow never scrolls"
        );
    }

    #[test]
    fn marquee_holds_scrolls_clamps_holds_and_returns() {
        let (overflow, speed, hold_start, hold_end) = (56.0, 28.0, 0.5, 0.8);
        let travel = overflow / speed; // 2 s
        let cycle = hold_start + travel + hold_end + travel;
        let offset = |elapsed: f32| marquee_offset(elapsed, overflow, speed, hold_start, hold_end);

        assert_eq!(offset(0.0), 0.0, "starts at the left edge");
        assert_eq!(offset(0.4), 0.0, "holds at the start for 0.5 s");
        assert_eq!(offset(hold_start), 0.0, "hold ends at 0.5 s");

        let quarter = offset(hold_start + 0.5);
        let half = offset(hold_start + 1.0);
        assert!(
            (quarter - speed * 0.5).abs() < 0.01,
            "advances at `speed` px/s"
        );
        assert!((half - 2.0 * quarter).abs() < 0.01, "the scroll is linear");
        assert_eq!(
            offset(hold_start + travel),
            overflow,
            "clamps at the overflow distance"
        );

        assert_eq!(
            offset(hold_start + travel + hold_end * 0.5),
            overflow,
            "holds at the end"
        );

        let returning = offset(hold_start + travel + hold_end + travel * 0.25);
        assert!(
            returning < overflow && returning > 0.0,
            "travels back towards the start ({returning})"
        );

        assert_eq!(offset(cycle), 0.0, "a full cycle returns to the left edge");
        assert!(
            (offset(cycle + hold_start + travel * 0.5) - overflow * 0.5).abs() < 0.01,
            "the cycle repeats"
        );
    }
}
