//! The VmodV shell: window chrome, menus, status bar and panel composition.
//!
//! The composition deliberately mirrors the legacy layout closely enough to
//! judge the immediate-mode fit: Groups on the left, Inspector on the right,
//! a central live-output + Gallery + Scenes area, a status bar and a menu bar.
//! P5 replaces this hand layout with `egui_tiles` docking.

use egui::{
    Align, Align2, Color32, CornerRadius, Layout, Margin, Sense, Stroke, StrokeKind, Vec2, vec2,
};

use crate::file_dialog::{FileDialogAction, file_dialog_window, profile_dialog_window};
use crate::gallery_drag::{GalleryDrag, gallery_drop_action};
use crate::group_drag::GroupDrag;
use crate::layout::{
    LayoutState, MIN_DOCK_WIDTH, TabPlacement, WINDOW_ABOUT, WINDOW_MODULE_INFO, WINDOW_PALETTE,
    WINDOW_SHORTCUTS, WINDOW_TEMPO,
};
use crate::library_drag::MediaLibraryDrag;
use crate::model::{
    COMPOSITE_OPERATIONS, EngineSnapshot, InheritTarget, LibraryKind, MediaAction, MediaAssetView,
    MediaSourceView, ModuleMetaView, ModuleView, PropView, ScreenCaptureView, VideoTransportAction,
    VideoTransportView, asset_meta_label, format_duration, gallery_preview_key,
    group_enabled_indicator, letterbox_rect, module_license, next_group_enabled, sample_rate_label,
};
use crate::module_drag::{ModuleDrag, insertion_index, module_drop_action};
use crate::panels::scene_grid::draw_scene_grid;
use crate::state::{
    DEFAULT_OSC_HOST, GalleryCategory, LibraryAssignmentTarget, SidePanelTab, UiAction, UiCommand,
    UiState, filter_gallery_modules, texture_prop_action,
};
use crate::tab_drag::{TabDrag, TabDropTarget, floating_bounds_for_drop};
use crate::theme;
use crate::widgets::input_link::{link_control_inline, link_editor_window, link_row};
use crate::widgets::palette_prop;
use crate::widgets::range_control::chip;
use crate::widgets::texture_control::{TextureControlChange, seek_position};
use crate::widgets::{ColorControl, RangeControl, TextureControl, TweenControl, VecControl};

/// The Groups panel's minimum width, matching `ui_layout::MIN_GROUPS_WIDTH`.
const MIN_GROUPS_WIDTH: f32 = 210.0;
/// The right tab strip's minimum width, matching `ui_layout::MIN_INSPECTOR_WIDTH`.
const MIN_INSPECTOR_WIDTH: f32 = 250.0;
/// The centre column's minimum width, matching `ui_layout::MIN_CENTRE_WIDTH`.
const MIN_CENTRE_WIDTH: f32 = 360.0;
/// The Live Output section's minimum height.
const MIN_LIVE_HEIGHT: f32 = 120.0;
/// The Scenes grid's minimum height.
const MIN_SCENES_HEIGHT: f32 = 140.0;
/// The Gallery section's minimum height.
const MIN_GALLERY_HEIGHT: f32 = 96.0;

/// Draw the complete VmodV UI for one frame.
pub fn draw(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    state.sync_status(snapshot);
    state.ensure_default_group_focus(snapshot);
    state.tab_drag = egui::DragAndDrop::payload::<TabDrag>(ui.ctx()).map(|payload| *payload);
    state
        .layout
        .tabs
        .ensure_strip_active(&mut state.side_panel_tab);
    handle_shortcuts(ui, snapshot, state);
    menu_bar(ui, snapshot, state);
    performance_header(ui, snapshot, state);
    status_bar(ui, snapshot, state);
    groups_panel(ui, snapshot, state);
    inspector_panel(ui, snapshot, state);
    dock_panel(ui, snapshot, state);
    central_panel(ui, snapshot, state);
    floating_tab_windows(ui.ctx(), snapshot, state);
    about_window(ui.ctx(), state);
    module_info_window(ui.ctx(), state);
    shortcuts_window(ui.ctx(), state);
    file_dialog_window(ui.ctx(), state);
    profile_dialog_window(ui.ctx(), state);
    plugin_consent_window(ui.ctx(), state);
    link_editor_window(ui, snapshot, state);
    bpm_window(ui.ctx(), snapshot, state);
    command_palette(ui.ctx(), snapshot, state);
    crate::splash::draw(ui.ctx(), state);
}

/// Keyboard shortcuts that work globally (outside text fields).
fn handle_shortcuts(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let typing = ui.ctx().memory(|memory| memory.focused().is_some());
    let (save, save_snapshot, open, open_scene, apply, search, escape, help, undo, redo) = ui
        .input(|input| {
            let command = input.modifiers.command || input.modifiers.ctrl;
            let shift = input.modifiers.shift;
            (
                command && !shift && input.key_pressed(egui::Key::S),
                command && shift && input.key_pressed(egui::Key::S),
                command && !shift && input.key_pressed(egui::Key::O),
                command && shift && input.key_pressed(egui::Key::O),
                command && input.key_pressed(egui::Key::Enter),
                command && input.key_pressed(egui::Key::K),
                input.key_pressed(egui::Key::Escape),
                input.key_pressed(egui::Key::F1),
                command && !shift && input.key_pressed(egui::Key::Z),
                command && shift && input.key_pressed(egui::Key::Z),
            )
        });
    // A modal picker owns the keyboard: Ctrl+S while naming a file must not
    // save a preset, and Ctrl+O must not stack a second picker.
    let modal =
        state.file_dialog.is_some() || state.show_profile_dialog || state.show_plugin_consent;
    // Backspace removes the focused Group (`Group.vue`'s `keydown.self`), but
    // only outside a text field and while no dialog owns the keyboard.
    let backspace = ui.input(|input| input.key_pressed(egui::Key::Backspace));
    let dialog_open = modal
        || state.link_editor.is_some()
        || state.tween_editor.is_some()
        || state.draw_editor.is_some()
        || state.scene_grid.timing_edit.is_some()
        || state.scene_grid.row_names_edit.is_some()
        || state.scene_grid.clear_cell_edit.is_some()
        || state.show_search
        || state.show_shortcuts
        || state.show_about
        || state.show_bpm
        || state.module_info.is_some();
    if backspace
        && !ui.ctx().text_edit_focused()
        && !dialog_open
        && let Some(group_id) = state.selected_group.clone()
    {
        state.push_action(
            "groups/removeGroup",
            serde_json::json!({ "groupId": group_id }),
        );
        state.selected_group = None;
        state.selected_module = None;
    }
    // Undo/redo reach the shell's Store history; text fields keep their own
    // Ctrl+Z, and a modal picker owns the keyboard like the Ctrl+S/O cases.
    if !typing && !modal {
        if undo {
            state.push_action("history/undo", serde_json::json!({}));
        } else if redo {
            state.push_action("history/redo", serde_json::json!({}));
        }
    }
    if save && !modal {
        state.queue(UiCommand::SavePreset);
    }
    if save_snapshot && !modal {
        state.queue(UiCommand::SaveSnapshot);
    }
    if open && !modal {
        state.queue(UiCommand::ShowFileDialog(
            crate::file_dialog::FileDialogAction::OpenPreset,
        ));
    }
    if open_scene && !modal {
        state.queue(UiCommand::ShowFileDialog(
            crate::file_dialog::FileDialogAction::OpenScene,
        ));
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
        state.splash = None;
        state.module_info = None;
        state.file_dialog = None;
        state.show_profile_dialog = false;
        state.profile_error = None;
        state.show_plugin_consent = false;
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
        .exact_size(32.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(Margin::symmetric(8, 4)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label(
                    egui::RichText::new("VmodV")
                        .strong()
                        .color(theme::ACCENT)
                        .size(15.0),
                );
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
                    if ui.button("Open preset…").clicked() {
                        state.queue(UiCommand::ShowFileDialog(FileDialogAction::OpenPreset));
                        ui.close();
                    }
                    if ui.button("Open Scene…").clicked() {
                        state.queue(UiCommand::ShowFileDialog(FileDialogAction::OpenScene));
                        ui.close();
                    }
                    if ui.button("Import playlist…").clicked() {
                        state.queue(UiCommand::ShowFileDialog(FileDialogAction::ImportPlaylist));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Save preset").clicked() {
                        state.queue(UiCommand::SavePreset);
                        ui.close();
                    }
                    if ui.button("Save preset as…").clicked() {
                        state.queue(UiCommand::ShowFileDialog(FileDialogAction::SavePresetAs));
                        ui.close();
                    }
                    if ui.button("Export playlist…").clicked() {
                        state.queue(UiCommand::ShowFileDialog(FileDialogAction::ExportPlaylist));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("New project profile…").clicked() {
                        state.open_profile_dialog();
                        ui.close();
                    }
                    ui.menu_button("Switch profile", |ui| {
                        let project = &snapshot.project;
                        if project.profiles.is_empty() {
                            ui.add_enabled(false, egui::Button::new("No profiles yet"));
                        }
                        for profile in &project.profiles {
                            let selected = profile == &project.active_profile;
                            if ui
                                .add(egui::Button::new(profile).selected(selected))
                                .clicked()
                            {
                                state.queue(UiCommand::SwitchProfile(profile.clone()));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Choose extra ISF folder…").clicked() {
                        state.queue(UiCommand::ShowFileDialog(FileDialogAction::AddIsfFolder));
                        ui.close();
                    }
                    if ui.button("Open media folder").clicked() {
                        state.queue(UiCommand::OpenMediaFolder);
                        ui.close();
                    }
                    if ui.button("Open logs").clicked() {
                        state.queue(UiCommand::OpenLogsFolder);
                        ui.close();
                    }
                    ui.separator();
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
                    if ui.button("Undo").clicked() {
                        state.push_action("history/undo", serde_json::json!({}));
                        ui.close();
                    }
                    if ui.button("Redo").clicked() {
                        state.push_action("history/redo", serde_json::json!({}));
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
                    if ui.button("About VmodV").clicked() {
                        state.show_about = true;
                        ui.close();
                    }
                    if ui.button("Open logs folder").clicked() {
                        state.queue(UiCommand::OpenLogsFolder);
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
        .exact_size(26.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(Margin::symmetric(8, 3)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{:.1} FPS", state.status.fps))
                        .monospace()
                        .size(12.0)
                        .color(if state.status.fps >= 55.0 {
                            theme::OK
                        } else {
                            theme::MUTED
                        }),
                );
                ui.label(
                    egui::RichText::new(format!("{:.1} ms", state.status.frame_ms))
                        .monospace()
                        .size(12.0)
                        .color(theme::MUTED),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{}x{}",
                        state.status.resolution[0], state.status.resolution[1]
                    ))
                    .monospace()
                    .size(12.0)
                    .color(theme::MUTED),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("BPM {:.0}", state.status.bpm))
                        .monospace()
                        .size(12.0)
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
                    .size(12.0)
                    .color(theme::MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(&state.status.backend)
                            .size(12.0)
                            .color(theme::MUTED),
                    )
                    .on_hover_text(&state.status.adapter);
                });
            });
        });
}

/// A small rolling FPS graph, drawn without a plotting dependency.
fn fps_sparkline(ui: &mut egui::Ui, state: &UiState) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(112.0, 17.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(0), theme::INPUT);
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
        egui::FontId::monospace(9.0),
        theme::MUTED,
    );
}

fn audio_meters(ui: &mut egui::Ui, snapshot: &EngineSnapshot) {
    for (label, value) in [
        ("B", snapshot.audio.bass),
        ("M", snapshot.audio.mid),
        ("T", snapshot.audio.treble),
    ] {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 16.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(0), theme::INPUT);
        let height = (value.clamp(0.0, 1.0) * rect.height()).max(0.5);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - height),
                rect.right_bottom(),
            ),
            egui::CornerRadius::same(0),
            theme::ACCENT,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(9.0),
            theme::BACKGROUND,
        );
    }
}

/// The Groups-pane Enabled control, matching `Checkbox.vue` with
/// `allow-partial`: a checkmark for `1`, a dash for `2` and an empty box for
/// `0`. The caller resolves the click into the next value (alt-click is the
/// partial state) so the gesture stays out of the painting code.
fn group_enabled_control(ui: &mut egui::Ui, enabled: i64) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(0), theme::INPUT);
    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(
            1.0,
            if response.hovered() {
                theme::MUTED
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );

    let indicator = group_enabled_indicator(enabled);
    if indicator != "off" {
        let colour = theme::FOREGROUND;
        let inner = rect.shrink(4.0);
        if indicator == "partial" {
            painter.line_segment(
                [inner.left_center(), inner.right_center()],
                Stroke::new(2.0, colour),
            );
        } else {
            painter.line_segment(
                [
                    egui::pos2(inner.left(), inner.center().y),
                    egui::pos2(inner.center().x, inner.bottom()),
                ],
                Stroke::new(2.0, colour),
            );
            painter.line_segment(
                [
                    egui::pos2(inner.center().x, inner.bottom()),
                    egui::pos2(inner.right(), inner.top()),
                ],
                Stroke::new(2.0, colour),
            );
        }
    }

    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::CheckBox,
            true,
            enabled != 0,
            "Enable this Group",
        )
    });
    response.on_hover_text(match indicator {
        "on" => {
            "Enabled. Click to disable; Alt-click to draw it without sending it to the output canvas."
        }
        "partial" => {
            "Enabled, but not drawn to the output canvas. Click to disable; Alt-click keeps this state."
        }
        _ => {
            "Disabled. Click to enable; Alt-click to draw it without sending it to the output canvas."
        }
    })
}

/// The `inheritFrom` selector, shown while the Group inherits. It lists the
/// previous Group (`-1`) plus every other user Group by name; the engine
/// accepts both the numeric and the UUID-string form in
/// `groups/UPDATE_GROUP_BY_KEY` (`groups.js`'s `inheritFrom` typedef).
fn group_inherit_from_selector(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
    group: &crate::GroupView,
) {
    ui.horizontal(|ui| {
        ui.label("Inherit from");
        let mut chosen: Option<InheritTarget> = None;
        egui::ComboBox::from_id_salt(("group-inherit-from", &group.id))
            .selected_text(group.inherit_from.label(&snapshot.groups))
            .width(150.0)
            // The "Composed Output" option is wider than the button; tighten
            // the popup's margins so the menu does not grow past the Group
            // pane's width.
            .popup_style(egui::style::StyleModifier::new(|style| {
                style.spacing.menu_margin = egui::Margin::same(0);
                style.spacing.button_padding.x = 2.0;
            }))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(group.inherit_from.is_previous(), "Previous Group")
                    .clicked()
                {
                    chosen = Some(InheritTarget::Previous);
                }
                if ui
                    .selectable_label(group.inherit_from.is_composition(), "Composed Output")
                    .on_hover_text(
                        "Inherit everything composited above this row this frame, regardless \
                         of each row's own Inherit setting",
                    )
                    .clicked()
                {
                    chosen = Some(InheritTarget::Composition);
                }
                for other in snapshot
                    .groups
                    .iter()
                    .filter(|other| other.id != group.id && !other.is_gallery())
                {
                    let selected = matches!(
                        &group.inherit_from,
                        InheritTarget::Group(id) if id == &other.id
                    );
                    if ui.selectable_label(selected, &other.name).clicked() {
                        chosen = Some(InheritTarget::Group(other.id.clone()));
                    }
                }
            });
        if let Some(target) = chosen {
            state.push_action(
                "groups/UPDATE_GROUP_BY_KEY",
                serde_json::json!({
                    "groupId": group.id,
                    "key": "inheritFrom",
                    "value": target.value(),
                }),
            );
        }
    });
}

fn groups_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let reserved = reserved_side_width(state, state.tab_drag.is_some());
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
                ui.label(egui::RichText::new("GROUPS").strong().size(12.0));
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
                                .size(13.0)
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
                        let header = ui.horizontal(|ui| {
                            if group_enabled_control(ui, group.enabled).clicked() {
                                let alt = ui.input(|input| input.modifiers.alt);
                                let next = next_group_enabled(group.enabled, alt);
                                state.push_action(
                                    "groups/UPDATE_GROUP_BY_KEY",
                                    serde_json::json!({
                                        "groupId": group.id,
                                        "key": "enabled",
                                        "value": next,
                                    }),
                                );
                            }
                            let response = ui.add(
                                egui::Button::selectable(
                                    selected,
                                    egui::RichText::new(&group.name).size(13.0),
                                )
                                .sense(Sense::click_and_drag()),
                            );
                            // A Group header is a drag source: its payload
                            // resolves into `scenes/setSceneCellGroup` when
                            // released over a Scene cell. Attaching on drag
                            // start leaves the existing click/double-click
                            // select/rename behaviour untouched.
                            if response.drag_started() {
                                response.dnd_set_drag_payload(GroupDrag::new(
                                    group.id.clone(),
                                    group.name.clone(),
                                ));
                            }
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
                                        .size(11.0)
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
                                if theme::slider(
                                    ui,
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
                                if group.inherit {
                                    group_inherit_from_selector(ui, snapshot, state, group);
                                }

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
                        let modules = ui.indent(("group-modules", &group.id), |ui| {
                            group_module_list(ui, snapshot, state, group, selected)
                        });
                        // Header, selected-Group controls and the module strip
                        // all accept a Gallery module dropped from the Gallery.
                        let drop_rect = header.response.rect.union(modules.response.rect);
                        gallery_drop_target(ui, drop_rect, &group.id, state);
                        // Dragging a module row inside the block: a row gap
                        // inserts above/below it, the rest of the block appends.
                        module_group_drop_target(ui, drop_rect, &group.id, &modules.inner, state);
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

/// Make a Group block a Gallery drop target: while a Gallery payload hovers
/// the block, paint the accent tint/border plus an "Add <module>" chip, and on
/// pointer release queue the same `ui/addModuleToGroup` action as the tile's
/// double-click path. The whole block rect (header plus module strip) accepts
/// the drop, including inside the scrolling Groups list.
fn gallery_drop_target(ui: &egui::Ui, rect: egui::Rect, group_id: &str, state: &mut UiState) {
    if !egui::DragAndDrop::has_payload_of_type::<GalleryDrag>(ui.ctx())
        || !ui.rect_contains_pointer(rect)
    {
        return;
    }
    let Some(payload) = egui::DragAndDrop::payload::<GalleryDrag>(ui.ctx()) else {
        return;
    };

    paint_gallery_drop_target(ui, rect, &payload.module_name);

    if ui.input(|input| input.pointer.any_released()) {
        // Consume the payload so a release cannot be answered twice; the pure
        // resolver returns None only if the drag carried no module name.
        let released = egui::DragAndDrop::take_payload::<GalleryDrag>(ui.ctx());
        if let Some(action) = gallery_drop_action(released.as_deref(), Some(group_id)) {
            state.actions.push(action);
        }
    } else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    }
}

/// The Group drop-target affordance: a translucent accent wash, an accent
/// border and a small "Add <module>" chip at the bottom-right of the block.
fn paint_gallery_drop_target(ui: &egui::Ui, rect: egui::Rect, module_name: &str) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(0),
        theme::ACCENT.gamma_multiply(0.18),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(1.5, theme::ACCENT),
        StrokeKind::Inside,
    );

    let galley = painter.layout_no_wrap(
        format!("Add {module_name}"),
        egui::FontId::proportional(11.0),
        theme::BACKGROUND,
    );
    let chip_size = galley.size() + vec2(8.0, 4.0);
    let chip = egui::Rect::from_min_size(
        egui::pos2(
            (rect.right() - chip_size.x - 4.0).max(rect.left() + 4.0),
            (rect.bottom() - chip_size.y - 4.0).max(rect.top() + 4.0),
        ),
        chip_size,
    );
    painter.rect_filled(chip, CornerRadius::same(0), theme::ACCENT);
    painter.galley(chip.min + vec2(4.0, 2.0), galley, theme::BACKGROUND);
}

/// The width already reserved by the right-hand panels that are visible.
fn reserved_side_width(state: &UiState, tab_dragging: bool) -> f32 {
    let strip = if state.layout.tabs.has_strip_tabs() || tab_dragging {
        state.layout.inspector_width
    } else {
        0.0
    };
    let dock = if state.layout.tabs.docked_tabs().is_empty() && !tab_dragging {
        0.0
    } else {
        state.layout.dock_width
    };
    strip + dock
}

/// One Group's module list: always visible, so every Group in the pane shows
/// what it contains. The per-Group name/alpha/blend editors stay selected-only
/// (they share one draft input); this list is safe to repeat per Group.
///
/// Returns each row's rect so the Group block can append a dragged module
/// after its last row.
fn group_module_list(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    state: &mut UiState,
    group: &crate::model::GroupView,
    selected: bool,
) -> Vec<egui::Rect> {
    ui.separator();
    let modules = snapshot.modules_in_group(&group.id);
    if modules.is_empty() {
        if selected {
            ui.label(
                egui::RichText::new("empty")
                    .italics()
                    .size(12.0)
                    .color(theme::MUTED),
            );
        }
        return Vec::new();
    }
    let mut row_rects = Vec::with_capacity(modules.len());
    for (module_index, module) in modules.iter().enumerate() {
        let selected_module = state.selected_module.as_deref() == Some(module.id.as_str());
        let row = ui.horizontal(|ui| {
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
            // The name is click-to-select only; the two-bar grip beside it is
            // the row's drag handle for re-ordering within the Group or moving
            // it to another one.
            module_drag_grip(ui, &group.id, module, module_index);
            let label = format!("{}  ({})", module.module_name, short_id(&module.id));
            let response = ui.add(egui::Button::selectable(
                selected_module,
                egui::RichText::new(label).size(12.0),
            ));
            if response.clicked() {
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
        row_rects.push(row.response.rect);
    }
    module_rows_drop_target(ui, &row_rects, &group.id, state);
    row_rects
}

/// The two-bar grip that is a module row's only drag handle.
///
/// The bars are painted, not text, so the widget declares its own accessible
/// `Drag <module> (<id>)` label: the name button next to it stays a plain
/// click-to-select target, and this grip is what re-orders the Module within
/// its Group or moves it to another one. The cursor shows `Grab` on hover and
/// `Grabbing` while the row is dragged.
fn module_drag_grip(ui: &mut egui::Ui, group_id: &str, module: &ModuleView, module_index: usize) {
    let (rect, response) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("Drag {} ({})", module.module_name, short_id(&module.id)),
        )
    });
    if response.drag_started() {
        response.dnd_set_drag_payload(ModuleDrag::new(&module.id, group_id, module_index));
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let painter = ui.painter();
    let stroke = Stroke::new(1.5, theme::MUTED.gamma_multiply(0.75));
    let left = rect.left() + 1.0;
    let right = rect.right() - 1.0;
    for offset in [-2.0f32, 2.0] {
        let y = rect.center().y + offset;
        painter.line_segment([egui::pos2(left, y), egui::pos2(right, y)], stroke);
    }
}

/// The module strip's insertion point: the pointer's y resolves against the
/// row midpoints ([`insertion_index`]), so the upper half of a row drops above
/// it and the lower half below it. The nearest row half is tinted and an accent
/// line marks the exact gap.
fn module_rows_drop_target(
    ui: &egui::Ui,
    rows: &[egui::Rect],
    group_id: &str,
    state: &mut UiState,
) {
    let Some(span) = module_rows_span(rows) else {
        return;
    };
    if !egui::DragAndDrop::has_payload_of_type::<ModuleDrag>(ui.ctx())
        || !ui.rect_contains_pointer(span)
    {
        return;
    }
    let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
        return;
    };

    let gap = insertion_index(rows, pointer.y);
    let (highlight, line_y) = if gap == 0 {
        let row = rows[0];
        (
            egui::Rect::from_min_max(row.min, egui::pos2(row.right(), row.center().y)),
            row.top(),
        )
    } else {
        let row = rows[gap - 1];
        (
            egui::Rect::from_min_max(egui::pos2(row.left(), row.center().y), row.max),
            row.bottom(),
        )
    };
    ui.painter().rect_filled(
        highlight,
        CornerRadius::same(0),
        theme::ACCENT.gamma_multiply(0.14),
    );
    paint_module_insertion_line(ui, ui.max_rect().x_range(), line_y);
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

    if ui.input(|input| input.pointer.any_released()) {
        // Consume the payload so neither the Group block nor a later frame can
        // answer the same release twice.
        let released = egui::DragAndDrop::take_payload::<ModuleDrag>(ui.ctx());
        if let Some(action) = module_drop_action(released.as_deref(), group_id, gap) {
            state.actions.push(action);
        }
    }
}

/// The rest of a Group block (header, selected-Group controls, the space below
/// its last row) appends a dragged module after the last row. A row under the
/// pointer owns the drop, so this only paints/handles the block itself.
fn module_group_drop_target(
    ui: &egui::Ui,
    block_rect: egui::Rect,
    group_id: &str,
    rows: &[egui::Rect],
    state: &mut UiState,
) {
    if !egui::DragAndDrop::has_payload_of_type::<ModuleDrag>(ui.ctx())
        || !ui.rect_contains_pointer(block_rect)
    {
        return;
    }
    let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
        return;
    };
    if module_rows_span(rows).is_some_and(|span| span.contains(pointer)) {
        return;
    }

    let painter = ui.painter();
    painter.rect_filled(
        block_rect,
        CornerRadius::same(0),
        theme::ACCENT.gamma_multiply(0.08),
    );
    painter.rect_stroke(
        block_rect,
        CornerRadius::same(0),
        Stroke::new(1.0, theme::ACCENT),
        StrokeKind::Inside,
    );
    let line_y = rows.last().map_or(block_rect.bottom(), |row| row.bottom());
    paint_module_insertion_line(ui, block_rect.x_range(), line_y);
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

    if ui.input(|input| input.pointer.any_released()) {
        let released = egui::DragAndDrop::take_payload::<ModuleDrag>(ui.ctx());
        if let Some(action) = module_drop_action(released.as_deref(), group_id, rows.len()) {
            state.actions.push(action);
        }
    }
}

/// The accent two-pixel insertion line, with a dot at its left end so the gap
/// reads even against a selected row's accent fill.
fn paint_module_insertion_line(ui: &egui::Ui, x: egui::Rangef, y: f32) {
    let painter = ui.painter();
    painter.hline(x, y, Stroke::new(2.0, theme::ACCENT));
    painter.circle_filled(egui::pos2(x.min + 2.0, y), 2.5, theme::ACCENT);
}

/// The rect covering every row and the small gaps between them, so a pointer
/// between two rows still resolves to an insertion gap instead of the Group's
/// append target. `None` when the Group has no rows.
fn module_rows_span(rows: &[egui::Rect]) -> Option<egui::Rect> {
    rows.iter().copied().reduce(|span, row| span.union(row))
}

fn inspector_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    // While a tab is dragged from the dock or a floating window the strip is a
    // drop target, so it stays mounted (with a "Back to tabs" placeholder) even
    // when every tab currently lives elsewhere.
    if !state.layout.tabs.has_strip_tabs() && state.tab_drag.is_none() {
        return;
    }
    let dock = if state.layout.tabs.docked_tabs().is_empty() && state.tab_drag.is_none() {
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
                .fill(theme::STRIP_FILL)
                .inner_margin(Margin::same(6)),
        )
        .show(ui, |ui| {
            tab_strip(ui, snapshot, state);
        });
    state.layout.inspector_width = response.response.rect.width();
}

/// Height of one tab-strip entry, including its compact detach icon.
const TAB_HEIGHT: f32 = 24.0;
/// The detach icon's hit area inside a tab's right edge.
const TAB_ICON_SIZE: f32 = 14.0;

/// The right-hand tab strip: one draggable tab per placement.
///
/// A tab is both a click target (select it) and a drag source carrying a
/// [`TabDrag`]. While a drag is in flight the strip advertises itself, so a
/// floating window's grip can be dropped here to return the tab.
fn tab_strip(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let row = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut placed = 0;
        for tab in SidePanelTab::ALL {
            if state.layout.tabs.placement(tab) != TabPlacement::Tabs {
                continue;
            }
            placed += 1;
            tab_entry(ui, tab, state);
        }
        if placed == 0 && state.tab_drag.is_some() {
            ui.label(
                egui::RichText::new("Back to tabs")
                    .strong()
                    .size(12.0)
                    .color(theme::MUTED),
            );
        }
    });
    let strip_rect = row.response.rect;

    if state.tab_drag.is_some() {
        let hovered = ui.rect_contains_pointer(strip_rect);
        let from_elsewhere = state
            .tab_drag
            .is_some_and(|drag| state.layout.tabs.placement(drag.tab) != TabPlacement::Tabs);
        if hovered || from_elsewhere {
            paint_tab_drop_zone(ui, strip_rect, hovered, hovered, Some("Back to tabs"));
        }
        if let Some(payload) = released_tab(ui, strip_rect) {
            apply_tab_drop(ui.ctx(), state, payload.tab, TabDropTarget::Strip);
        }
    }

    ui.separator();

    let active = state.side_panel_tab;
    egui::ScrollArea::vertical()
        .id_salt("inspector-scroll")
        .show(ui, |ui| {
            tab_content(ui, snapshot, state, active);
        });
}

/// One tab-strip entry: a painted tab shape, a click/drag response and the
/// compact detach icon inside its right edge.
fn tab_entry(ui: &mut egui::Ui, tab: SidePanelTab, state: &mut UiState) {
    let title = tab.title().to_uppercase();
    let galley = ui.painter().layout_no_wrap(
        title.clone(),
        egui::FontId::proportional(12.0),
        theme::FOREGROUND,
    );
    let size = vec2(galley.size().x + 24.0 + TAB_ICON_SIZE, TAB_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &title));
    if response.drag_started() {
        response.dnd_set_drag_payload(TabDrag::new(tab));
    }

    let selected = state.side_panel_tab == tab;
    let hovered = response.hovered();
    let dragging = state.tab_drag.is_some_and(|drag| drag.tab == tab);
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 4.0 - TAB_ICON_SIZE * 0.5, rect.center().y),
        Vec2::splat(TAB_ICON_SIZE),
    );
    let icon_response = ui.interact(icon_rect, response.id.with("detach"), Sense::click());
    icon_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, "Detach to a floating window")
    });
    let icon_hovered = icon_response.hovered();
    let icon_visible = selected || hovered || icon_hovered;

    let painter = ui.painter();
    let fill = if selected {
        theme::ACCENT_DIM
    } else if hovered || dragging {
        theme::PANEL_ALT
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, CornerRadius::same(0), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(
            1.0,
            if selected || dragging {
                theme::ACCENT
            } else if hovered {
                theme::MUTED
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );
    if selected {
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom() - 2.0),
                vec2(rect.width(), 2.0),
            ),
            CornerRadius::same(0),
            theme::ACCENT,
        );
    }
    let text_center = egui::pos2(
        rect.left() + (rect.width() - TAB_ICON_SIZE - 4.0) * 0.5,
        rect.center().y,
    );
    painter.text(
        text_center,
        Align2::CENTER_CENTER,
        &title,
        egui::FontId::proportional(12.0),
        if selected || hovered || dragging {
            theme::FOREGROUND
        } else {
            theme::MUTED
        },
    );
    if icon_visible {
        paint_detach_icon(
            painter,
            icon_rect,
            if icon_hovered {
                theme::FOREGROUND
            } else {
                theme::MUTED
            },
        );
    }

    if response.clicked() && !icon_response.clicked() {
        state.side_panel_tab = tab;
    }
    if response.hovered() || icon_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.context_menu(|ui| {
        if ui.button("Detach to floating window").clicked() {
            state
                .layout
                .tabs
                .place(tab, TabPlacement::Floating, &mut state.side_panel_tab);
            ui.close();
        }
        if ui
            .button("Dock to area")
            .on_hover_text("Move this panel into its own dock column")
            .clicked()
        {
            state
                .layout
                .tabs
                .place(tab, TabPlacement::Docked, &mut state.side_panel_tab);
            ui.close();
        }
    });
    if icon_response
        .on_hover_text("Detach to a floating window")
        .clicked()
    {
        state
            .layout
            .tabs
            .place(tab, TabPlacement::Floating, &mut state.side_panel_tab);
    }
}

/// The compact detach affordance: a small outlined square with a pop-out
/// arrow. Deliberately not tab-shaped, so it cannot read as a second tab.
fn paint_detach_icon(painter: &egui::Painter, rect: egui::Rect, colour: Color32) {
    let glyph = egui::Rect::from_center_size(rect.center(), Vec2::splat(10.0));
    painter.rect_stroke(
        glyph,
        CornerRadius::same(1),
        Stroke::new(1.0, colour),
        StrokeKind::Inside,
    );
    let start = glyph.left_bottom() + vec2(2.5, -2.5);
    let end = glyph.right_top() + vec2(-2.5, 2.5);
    painter.line_segment([start, end], Stroke::new(1.0, colour));
    painter.line_segment([end, end + vec2(-3.5, 0.0)], Stroke::new(1.0, colour));
    painter.line_segment([end, end + vec2(0.0, 3.5)], Stroke::new(1.0, colour));
}

/// Consume a released [`TabDrag`] payload when the pointer is over `rect`.
///
/// The hit test uses `Ui::rect_contains_pointer` rather than a response's
/// `contains_pointer`: the whole target area accepts the drop even when the
/// pointer is over one of its child widgets (a tab, a label or a slider).
fn released_tab(ui: &egui::Ui, rect: egui::Rect) -> Option<TabDrag> {
    if !ui.rect_contains_pointer(rect) || !ui.input(|input| input.pointer.any_released()) {
        return None;
    }
    egui::DragAndDrop::take_payload::<TabDrag>(ui.ctx()).map(|payload| *payload)
}

/// Write a dropped tab's exclusive placement and, for a central-area drop,
/// remember the clamped floating-window bounds so the window opens where it
/// was released.
fn apply_tab_drop(
    ctx: &egui::Context,
    state: &mut UiState,
    tab: SidePanelTab,
    target: TabDropTarget,
) {
    state.tab_drag = None;
    state
        .layout
        .tabs
        .place(tab, target.placement(), &mut state.side_panel_tab);
    match target {
        TabDropTarget::Float(position) => {
            let bounds = floating_bounds_for_drop(position, ctx.content_rect());
            state
                .layout
                .record_window_rect(&LayoutState::tab_window_id(tab), bounds.as_rect());
            state.tab_float_target = Some((tab, bounds));
        }
        TabDropTarget::Strip | TabDropTarget::Dock => {
            if state
                .tab_float_target
                .is_some_and(|(target, _)| target == tab)
            {
                state.tab_float_target = None;
            }
        }
    }
}

/// Paint a tab drop target: an optional accent wash, a bright border when
/// hovered, and a small chip naming the drop ("Dock here", "Back to tabs",
/// "Float here").
fn paint_tab_drop_zone(
    ui: &egui::Ui,
    rect: egui::Rect,
    hovered: bool,
    outline: bool,
    chip: Option<&str>,
) {
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(
            rect,
            CornerRadius::same(2),
            theme::ACCENT.gamma_multiply(0.10),
        );
    }
    if hovered || outline {
        painter.rect_stroke(
            rect,
            CornerRadius::same(2),
            Stroke::new(
                if hovered { 1.5 } else { 1.0 },
                if hovered {
                    theme::ACCENT
                } else {
                    theme::BORDER
                },
            ),
            StrokeKind::Inside,
        );
    }

    let Some(label) = chip else {
        return;
    };
    let text_colour = if hovered {
        theme::BACKGROUND
    } else {
        theme::MUTED
    };
    let galley = painter.layout_no_wrap(
        label.to_string(),
        egui::FontId::proportional(10.0),
        text_colour,
    );
    let size = galley.size() + vec2(10.0, 5.0);
    let chip_rect = egui::Rect::from_min_size(
        egui::pos2(
            (rect.right() - size.x - 4.0).max(rect.left() + 4.0),
            rect.top() + 4.0,
        ),
        size,
    );
    painter.rect_filled(
        chip_rect,
        CornerRadius::same(2),
        if hovered {
            theme::ACCENT
        } else {
            theme::PANEL_ALT
        },
    );
    if !hovered {
        painter.rect_stroke(
            chip_rect,
            CornerRadius::same(2),
            Stroke::new(1.0, theme::BORDER),
            StrokeKind::Inside,
        );
    }
    painter.galley(chip_rect.min + vec2(5.0, 2.5), galley, text_colour);
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
        SidePanelTab::Library => library_panel(ui, snapshot, state),
        SidePanelTab::Quality => quality_panel(ui, snapshot, state),
        SidePanelTab::Output => output_panel(ui, snapshot, state),
        SidePanelTab::Audio => audio_panel(ui, snapshot, state),
        SidePanelTab::Info => info_panel(ui, snapshot),
        SidePanelTab::Plugins => plugins_panel(ui, snapshot, state),
        SidePanelTab::Midi => midi_panel(ui, snapshot, state),
        SidePanelTab::Ndi => ndi_panel(ui, snapshot, state),
        SidePanelTab::Osc => osc_panel(ui, snapshot, state),
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
        ui.label(egui::RichText::new(&module.module_name).strong().size(15.0));
        chip(ui, &module.module_type, theme::MUTED);
    });
    ui.label(
        egui::RichText::new(format!(
            "id {}  |  group {}",
            short_id(&module.id),
            short_id(&module.group_id)
        ))
        .monospace()
        .size(11.0)
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
    if theme::slider(
        ui,
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
                .size(13.0)
                .color(theme::MUTED),
        );
    }
    for prop in &module.props {
        draw_prop(ui, &module.id, &module.module_name, prop, snapshot, state);
        // Numeric props own their link control in `draw_prop`'s trailing slot
        // in both states, so only the remaining prop types need a link row
        // here.
        if !matches!(prop.prop_type.as_str(), "float" | "int")
            && let Some(input_id) = &prop.input_id
        {
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
/// Hidden while no tab is docked and no drag is in flight. During a drag it
/// appears as an explicit "Dock here" target even when empty. Several docked
/// tabs stack with a header and separator each, inside one scroll area.
fn dock_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let docked = state.layout.tabs.docked_tabs();
    let dragging = state.tab_drag.is_some();
    if docked.is_empty() && !dragging {
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
                .fill(theme::DOCK_FILL)
                .inner_margin(Margin::same(6)),
        )
        .show(ui, |ui| {
            if docked.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
                    ui.label(
                        egui::RichText::new("Dock here")
                            .strong()
                            .size(14.0)
                            .color(theme::MUTED),
                    );
                    ui.label(
                        egui::RichText::new("Drag a tab here to dock it")
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                });
                return;
            }
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
                                    .size(12.0),
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

    let dock_rect = response.response.rect;
    if dragging {
        let hovered = ui.rect_contains_pointer(dock_rect);
        paint_tab_drop_zone(
            ui,
            dock_rect,
            hovered,
            true,
            (!docked.is_empty()).then_some("Dock here"),
        );
        if let Some(payload) = released_tab(ui, dock_rect) {
            apply_tab_drop(ui.ctx(), state, payload.tab, TabDropTarget::Dock);
        }
    }
}

/// Every tab placed in a floating window, with dock controls and a drag grip
/// in its header.
fn floating_tab_windows(ctx: &egui::Context, snapshot: &EngineSnapshot, state: &mut UiState) {
    for tab in state.layout.tabs.floating_tabs() {
        let window_id = LayoutState::tab_window_id(tab);
        let mut open = true;
        let default_pos = [
            240.0 + tab.index() as f32 * 26.0,
            90.0 + tab.index() as f32 * 26.0,
        ];
        let drop_bounds = state
            .tab_float_target
            .filter(|(target, _)| *target == tab)
            .map(|(_, bounds)| bounds);
        let mut window = egui::Window::new(tab.title())
            .id(egui::Id::unique(("modv-tab-window", tab.key())))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .min_width(240.0)
            .min_height(200.0)
            .default_pos(default_pos)
            .default_size([360.0, 460.0]);
        if let Some(bounds) = drop_bounds {
            window = window.current_pos(bounds.pos());
        }
        let window = restore_window(window, &window_id, &state.layout);

        let mut dock_tabs = false;
        let mut dock_area = false;
        let response = window.show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (grip_rect, grip) =
                    ui.allocate_exact_size(Vec2::splat(16.0), Sense::click_and_drag());
                grip.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        true,
                        format!("Drag {} window", tab.title()),
                    )
                });
                if grip.drag_started() {
                    grip.dnd_set_drag_payload(TabDrag::new(tab));
                }
                if grip.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                paint_window_grip(ui.painter(), grip_rect, grip.hovered());
                grip.on_hover_text("Drag to the tab strip or dock column");

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
        if drop_bounds.is_some() {
            state.tab_float_target = None;
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

/// Six dots in two columns: the grab handle for a floating tab window.
fn paint_window_grip(painter: &egui::Painter, rect: egui::Rect, hovered: bool) {
    let colour = if hovered {
        theme::FOREGROUND
    } else {
        theme::MUTED
    };
    for row in 0..3 {
        for column in 0..2 {
            painter.circle_filled(
                rect.center() + vec2(-3.0 + column as f32 * 6.0, -5.0 + row as f32 * 5.0),
                1.25,
                colour,
            );
        }
    }
}

/// The Quality tab: performance vs fidelity knobs (internal rendering is the
/// first). Kept next to Library in the strip because low-resolution output
/// is a quality decision, not a resolution one.
fn quality_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    ui.label(egui::RichText::new("Internal rendering resolution").strong());
    ui.label(
        egui::RichText::new(
            "Only the heavy modules (3D Scenes ISF and Point10 scenes) render at this scale of the output size, upscaled at composite time. Light modules — 2D scenes, shaders, effects — always render at the output resolution. Persisted across restarts.",
        )
        .size(11.0)
        .color(theme::MUTED),
    );
    ui.add_space(4.0);
    const PRESETS: [(f32, &str); 3] = [
        (1.0, "Full (100%)"),
        (0.75, "Balanced (75%)"),
        (0.5, "Performance (50%)"),
    ];
    for (scale, label) in PRESETS {
        let selected = (state.render_scale - scale).abs() < f32::EPSILON;
        if ui.radio(selected, label).clicked() {
            state.set_render_scale(scale);
        }
    }
    ui.add_space(4.0);
    let (width, height) = (snapshot.resolution[0], snapshot.resolution[1]);
    let scale = state.render_scale;
    let heavy_width = (width as f32 * scale).round().max(1.0) as u32;
    let heavy_height = (height as f32 * scale).round().max(1.0) as u32;
    ui.label(
        egui::RichText::new(format!(
            "Heavy modules (3D Scenes and Point10) render at {heavy_width}x{heavy_height}, upscaled to the output size. Light modules always render at full output resolution."
        ))
        .size(11.0)
        .color(theme::MUTED),
    );
}

/// The Output tab: the output window's resolution and fullscreen state.
///
/// The compositor renders at the output window's size; picking a size
/// resizes the window (the surface/compositor follow on the `Resized`
/// event), and persists with the window state. Fullscreen reuses the
/// `F` key path. Choosing a resolution while fullscreen exits fullscreen
/// so the picked size applies.
fn output_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let open = snapshot.health.output_open;

    if !open {
        ui.label(
            egui::RichText::new("The output window is closed.")
                .size(12.0)
                .color(theme::MUTED),
        );
        if ui.button("Open output window").clicked() {
            state.queue(UiCommand::OpenOutputWindow);
        }
        return;
    }

    ui.label(egui::RichText::new("Output window").strong());
    ui.label(
        egui::RichText::new(
            "The output window is where the performance output renders. Resolution applies live; pick one or type exact dimensions.",
        )
        .size(11.0)
        .color(theme::MUTED),
    );
    ui.add_space(6.0);

    let (width, height) = (snapshot.resolution[0], snapshot.resolution[1]);

    ui.label(egui::RichText::new("Resolution").strong().size(11.0));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        const PRESETS: [(u32, u32); 5] = [
            (960, 540),
            (1280, 720),
            (1920, 1080),
            (2560, 1440),
            (3840, 2160),
        ];
        for (w, h) in PRESETS {
            let selected = width == w && height == h;
            if ui.radio(selected, format!("{w}x{h}")).clicked() {
                state.set_output_size(w, h);
            }
        }
    });

    let mut custom_width = width;
    let mut custom_height = height;
    ui.horizontal(|ui| {
        ui.label("Custom");
        let w = ui.add(
            egui::DragValue::new(&mut custom_width)
                .range(64..=7680)
                .speed(1.0),
        );
        ui.label("x");
        let h = ui.add(
            egui::DragValue::new(&mut custom_height)
                .range(64..=7680)
                .speed(1.0),
        );
        if w.changed() || h.changed() {
            state.set_output_size(custom_width, custom_height);
        }
    });

    ui.add_space(6.0);
    let mut fullscreen = snapshot.health.output_fullscreen;
    if ui.checkbox(&mut fullscreen, "Fullscreen (F)").changed() {
        state.set_output_fullscreen(fullscreen);
    }

    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("Current: {width}x{height}"))
            .monospace()
            .size(11.0)
            .color(theme::MUTED),
    );
}

fn info_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot) {
    ui.label(
        egui::RichText::new("VmodV")
            .strong()
            .size(16.0)
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
            ui.label(format!("BPM {:.0}", snapshot.audio.bpm));
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
                .size(12.0)
                .color(theme::MUTED),
        );
    }
    ui.label(
        egui::RichText::new(
            "Presets, Scenes, playlists and the media directory stay in the shared VmodV userData tree.",
        )
        .size(11.0)
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
            ui.label(egui::RichText::new(error).size(12.0).color(theme::MUTED));
        }
    } else {
        ui.label(
            egui::RichText::new(format!("{} MIDI input(s)", midi.devices.len()))
                .size(12.0)
                .color(theme::MUTED),
        );
    }

    // One-shot Learn status. Learn itself is only offered by the focused
    // input's editor: a global Learn could not name the input it binds.
    if let Some(arm) = &midi.learning {
        ui.separator();
        ui.label(
            egui::RichText::new("Waiting for MIDI…")
                .strong()
                .color(theme::ACCENT),
        );
        ui.label(
            egui::RichText::new(format!(
                "The next MIDI message creates a link for input {}.",
                arm.input_id
            ))
            .size(12.0)
            .color(theme::MUTED),
        );
        if ui
            .button("Cancel")
            .on_hover_text("Stop waiting; no link is created")
            .clicked()
        {
            state.push_action("midi/cancelLearn", serde_json::Value::Null);
            if let Some(editor) = state.link_editor.as_mut() {
                editor.cancel_midi_learn();
            }
        }
    }
    if let Some(result) = &midi.last_learn {
        ui.label(
            egui::RichText::new(format!(
                "Last learned: {} channel {} type {}",
                result.device, result.channel, result.type_
            ))
            .size(12.0)
            .color(theme::MUTED),
        );
    }

    if !midi.available {
        return;
    }
    ui.separator();
    if midi.devices.is_empty() {
        ui.label(
            egui::RichText::new("No MIDI devices detected. Hot-plug is polled at 1 Hz.")
                .size(12.0)
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
            ui.label(egui::RichText::new(&device.name).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(if device.connected { "live" } else { "gone" })
                        .monospace()
                        .size(10.0)
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

/// The NDI tab: availability, the output sender, discovery and receivers.
///
/// Ported from `InputDeviceConfig/NDI.vue`. The values come from the engine's
/// `NdiRuntime` (the same state `ndi/getState` returns), refreshed at 1 Hz by
/// `modv-app` while the tab is visible.
fn ndi_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let ndi = &snapshot.ndi;
    let available = ndi.can_enable_output();

    if let Some(warning) = ndi.availability_warning() {
        ui.label(
            egui::RichText::new("NDI is unavailable")
                .strong()
                .color(theme::DANGER),
        );
        ui.label(egui::RichText::new(warning).size(12.0).color(theme::MUTED));
    }
    if let Some(warning) = ndi.sender_warning() {
        ui.colored_label(theme::DANGER, warning);
    }

    ui.label(egui::RichText::new("NDI OUTPUT").strong().size(11.0));
    let output = &ndi.output;
    let mut enabled = output.enabled;
    if ui
        .add_enabled(
            available,
            egui::Checkbox::new(&mut enabled, "Enable Output"),
        )
        .changed()
    {
        state.push_action("ndi/SET_OUTPUT_ENABLED", serde_json::json!(enabled));
    }

    let mut submitted_name = None;
    {
        let draft = state
            .ndi_name_draft
            .get_or_insert_with(|| output.name.clone());
        ui.horizontal(|ui| {
            ui.label("Output Name");
            let response = ui.add_enabled(
                !output.enabled && available,
                egui::TextEdit::singleline(draft).desired_width(160.0),
            );
            if response.lost_focus() {
                submitted_name = Some(draft.clone());
            }
        });
    }
    if let Some(name) = submitted_name {
        if name != output.name {
            state.push_action("ndi/SET_OUTPUT_NAME", serde_json::json!(name));
        }
        state.ndi_name_draft = None;
    }
    if output.enabled {
        ui.label(
            egui::RichText::new("Disable the output to rename it")
                .size(11.0)
                .color(theme::MUTED),
        );
    }

    let mut follow_size = output.follow_output_size;
    if ui
        .add_enabled(
            available,
            egui::Checkbox::new(&mut follow_size, "Follow ModV output size"),
        )
        .changed()
    {
        state.push_action(
            "ndi/SET_FOLLOW_MODV_OUTPUT_SIZE",
            serde_json::json!(follow_size),
        );
    }
    ui.horizontal(|ui| {
        ui.label("Resolution");
        if follow_size {
            ui.label(egui::RichText::new(ndi.output_size_label(snapshot.resolution)).monospace());
        } else {
            let mut width = output.width;
            if ui
                .add_enabled(
                    available,
                    egui::DragValue::new(&mut width).range(1..=8192).speed(1.0),
                )
                .changed()
            {
                state.push_action("ndi/SET_OUTPUT_SIZE", serde_json::json!({ "width": width }));
            }
            ui.label("×");
            let mut height = output.height;
            if ui
                .add_enabled(
                    available,
                    egui::DragValue::new(&mut height).range(1..=8192).speed(1.0),
                )
                .changed()
            {
                state.push_action(
                    "ndi/SET_OUTPUT_SIZE",
                    serde_json::json!({ "height": height }),
                );
            }
        }
    });

    let mut follow_fps = output.follow_fps;
    if ui
        .add_enabled(
            available,
            egui::Checkbox::new(&mut follow_fps, "Follow ModV frame rate"),
        )
        .changed()
    {
        state.push_action("ndi/SET_FOLLOW_MODV_FPS", serde_json::json!(follow_fps));
    }
    ui.horizontal(|ui| {
        ui.label("Frame rate");
        if follow_fps {
            ui.label(
                egui::RichText::new(ndi.output_fps_label(f64::from(snapshot.fps)))
                    .monospace()
                    .size(11.5),
            );
        } else {
            let mut fps = output.target_fps;
            if ui
                .add_enabled(
                    available,
                    egui::DragValue::new(&mut fps)
                        .range(1.0..=240.0)
                        .speed(1.0)
                        .suffix(" fps"),
                )
                .changed()
            {
                state.push_action("ndi/SET_OUTPUT_TARGET_FPS", serde_json::json!(fps));
            }
        }
    });

    ui.horizontal(|ui| {
        let colour = if ndi.sender.running {
            theme::OK
        } else {
            theme::MUTED
        };
        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, colour);
        ui.label(
            egui::RichText::new(ndi.sender_status_label())
                .size(12.0)
                .color(colour),
        );
    });
    if let Some(path) = &ndi.library_path {
        ui.label(
            egui::RichText::new(format!("libndi: {path}"))
                .monospace()
                .size(10.5)
                .color(theme::MUTED),
        );
    }

    ui.separator();
    ui.label(egui::RichText::new("NDI SOURCES").strong().size(11.0));
    let mut show_local = ndi.show_local_sources;
    if ui
        .add_enabled(
            available,
            egui::Checkbox::new(&mut show_local, "Show local sources"),
        )
        .changed()
    {
        state.push_action(
            "ndi/setDiscoveryOptions",
            serde_json::json!({ "showLocalSources": show_local }),
        );
    }
    if ui
        .add_enabled(
            available && !ndi.discovering,
            egui::Button::new(ndi.discover_label()),
        )
        .on_hover_text("Search the network for NDI sources")
        .clicked()
    {
        state.push_action("ndi/discoverSources", serde_json::json!({}));
    }
    if ndi.sources.is_empty() {
        ui.label(
            egui::RichText::new(
                "No NDI sources yet. Click Discover Sources to search the network.",
            )
            .size(12.0)
            .color(theme::MUTED),
        );
    }
    for source in &ndi.sources {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&source.name).size(13.0));
                ui.label(
                    egui::RichText::new(&source.url_address)
                        .monospace()
                        .size(10.5)
                        .color(theme::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        available,
                        egui::Button::new(egui::RichText::new("Receive source").size(11.0)).small(),
                    )
                    .on_hover_text("Create a receiver for this source")
                    .clicked()
                {
                    state.push_action(
                        "ndi/createReceiver",
                        serde_json::json!({
                            "source": {
                                "name": source.name,
                                "urlAddress": source.url_address,
                            },
                        }),
                    );
                }
            });
        });
    }

    ui.separator();
    ui.label(egui::RichText::new("NDI RECEIVERS").strong().size(11.0));
    if ndi.receivers.is_empty() {
        ui.label(
            egui::RichText::new("No NDI receivers.")
                .size(12.0)
                .color(theme::MUTED),
        );
    }
    for receiver in &ndi.receivers {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(receiver.title()).size(13.0));
                ui.label(
                    egui::RichText::new(receiver.status_label())
                        .monospace()
                        .size(10.5)
                        .color(theme::MUTED),
                );
                if let Some(error) = &receiver.error {
                    ui.colored_label(theme::DANGER, error);
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .small_button("Remove receiver")
                    .on_hover_text("Stop consuming this source")
                    .clicked()
                {
                    state.push_action(
                        "ndi/removeReceiver",
                        serde_json::json!({ "receiverId": receiver.id }),
                    );
                }
                if ui
                    .small_button(receiver.toggle_label())
                    .on_hover_text(if receiver.enabled {
                        "Stop receiving frames"
                    } else {
                        "Connect to the source"
                    })
                    .clicked()
                {
                    let action = if receiver.enabled {
                        "ndi/disableReceiver"
                    } else {
                        "ndi/enableReceiver"
                    };
                    state.push_action(action, serde_json::json!({ "receiverId": receiver.id }));
                }
            });
        });
    }
}

/// The OSC tab: UDP listeners and the latest `osc.data` values.
///
/// Electron creates one server at startup and the Input Link OSC source reads
/// `osc.data.<address>`; the VmodV shell does the same and this panel makes
/// both visible. The address tree is refreshed at 1 Hz by `modv-app` while the
/// tab is visible.
fn osc_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let osc = &snapshot.osc;

    ui.label(egui::RichText::new("OSC LISTENERS").strong().size(11.0));
    ui.label(
        egui::RichText::new("Input Links with the OSC source read osc.data.<address>.")
            .size(11.0)
            .color(theme::MUTED),
    );
    if osc.servers.is_empty() {
        ui.label(
            egui::RichText::new("No OSC listeners.")
                .size(12.0)
                .color(theme::MUTED),
        );
    }
    for server in &osc.servers {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(server.title()).size(13.0));
                if server.title() != server.address_label() {
                    ui.label(
                        egui::RichText::new(server.address_label())
                            .monospace()
                            .size(10.5)
                            .color(theme::MUTED),
                    );
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .small_button("Remove")
                    .on_hover_text("Stop this listener")
                    .clicked()
                {
                    state.push_action("osc/removeServer", serde_json::json!({ "id": server.id }));
                }
            });
        });
    }

    ui.horizontal(|ui| {
        ui.label("Host");
        ui.add(
            egui::TextEdit::singleline(&mut state.osc_host_draft)
                .desired_width(90.0)
                .hint_text(DEFAULT_OSC_HOST),
        );
        ui.label("Port");
        ui.add(
            egui::TextEdit::singleline(&mut state.osc_port_draft)
                .desired_width(56.0)
                .hint_text("3333"),
        );
        if ui
            .button("Add listener")
            .on_hover_text("Bind another UDP OSC listener")
            .clicked()
        {
            let host = state.osc_host_draft.trim();
            let host = if host.is_empty() {
                DEFAULT_OSC_HOST.to_string()
            } else {
                host.to_string()
            };
            match state.osc_port_draft.trim().parse::<u16>() {
                Ok(port) if port > 0 => {
                    state.osc_form_error = None;
                    state.push_action(
                        "osc/createServer",
                        serde_json::json!({ "host": host, "port": port }),
                    );
                }
                _ => {
                    state.osc_form_error =
                        Some("Port must be a number between 1 and 65535.".to_string());
                }
            }
        }
    });
    if let Some(error) = &state.osc_form_error {
        ui.colored_label(theme::DANGER, error);
    }

    ui.separator();
    ui.label(
        egui::RichText::new("RECEIVED ADDRESSES")
            .strong()
            .size(11.0),
    );
    if osc.addresses.is_empty() {
        ui.label(
            egui::RichText::new(
                "No OSC messages yet. Send to a listener above and the address appears as osc.data.<address>.",
            )
            .size(12.0)
            .color(theme::MUTED),
        );
    }
    for address in &osc.addresses {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(&address.state_path)
                        .monospace()
                        .size(11.5),
                );
                ui.label(
                    egui::RichText::new(address.value_label())
                        .size(11.0)
                        .color(theme::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .small_button("Copy")
                    .on_hover_text("Copy the state path for an Input Link")
                    .clicked()
                {
                    ui.ctx().copy_text(address.state_path.clone());
                }
            });
        });
    }
}

fn plugins_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let view = &snapshot.plugins;

    ui.label(
        egui::RichText::new("JavaScript plugins")
            .strong()
            .size(13.0),
    );
    ui.label(
        egui::RichText::new("Advanced: JavaScript plugins")
            .strong()
            .size(12.0)
            .color(theme::WARNING),
    );
    ui.label(
        egui::RichText::new(
            "Plugins are third-party JavaScript. In the VmodV shell they run in a \
             restricted Boa sandbox, not a security boundary.",
        )
        .size(11.5)
        .color(theme::MUTED),
    );
    ui.label(
        egui::RichText::new(
            "The sandbox exposes no DOM, and no filesystem or network globals: plugins \
             see an allow-listed read-only state snapshot and a small RGBA frame \
             capture. modV.ws is not wired yet. Only enable scripts you trust.",
        )
        .size(11.0)
        .color(theme::MUTED),
    );

    let mut enabled = view.enabled;
    if ui
        .checkbox(&mut enabled, "Enable JavaScript plugins (advanced)")
        .on_hover_text(
            "Run the scripts listed below. The first enable asks for consent; \
             disabling stops execution immediately.",
        )
        .changed()
    {
        if enabled && !view.consented {
            // Never queue the enable here: confirmation in the dialog is what
            // records consent and turns the host on.
            state.open_plugin_consent();
        } else {
            state.queue(UiCommand::SetPluginsEnabled(enabled));
        }
    }

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(view.summary())
                .size(11.0)
                .color(if view.is_running() {
                    theme::ACCENT
                } else {
                    theme::MUTED
                }),
        );
        if view.consented {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .small_button("Revoke consent")
                    .on_hover_text(
                        "Withdraw the plugin consent and stop every running script. \
                         The enable switch turns off too.",
                    )
                    .clicked()
                {
                    state.queue(UiCommand::SetPluginConsent(false));
                }
            });
        }
    });

    if let Some(reason) = view.gate_reason() {
        ui.label(egui::RichText::new(reason).size(11.5).color(theme::MUTED));
    }
    if let Some(capture) = view.capture_label() {
        ui.label(
            egui::RichText::new(format!("{capture} at up to 20 Hz"))
                .monospace()
                .size(10.5)
                .color(theme::MUTED),
        );
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("DISCOVERED PLUGINS")
                .strong()
                .size(11.0),
        );
        ui.label(
            egui::RichText::new(format!("({})", view.plugins.len()))
                .size(11.0)
                .color(theme::MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button("Reload")
                .on_hover_text(
                    "Re-evaluate every discovered script without changing the file set \
                     (use after editing a plugin in place).",
                )
                .clicked()
            {
                state.queue(UiCommand::ReloadPlugins);
            }
            if ui
                .small_button("Rescan")
                .on_hover_text(
                    "Re-read the active profile's plugin/ folder and <userData>/plugins.",
                )
                .clicked()
            {
                state.queue(UiCommand::RescanPlugins);
            }
        });
    });

    if view.plugins.is_empty() {
        ui.label(
            egui::RichText::new(
                "No plugin scripts found. Put .js files in the active project's plugin/ \
                 folder or <userData>/plugins, then Rescan.",
            )
            .size(12.0)
            .color(theme::MUTED),
        );
    } else {
        // A gated host greyed the whole list: the rows are visible so the user
        // can see what would run, but nothing is clickable until the gate is
        // on (the explanation above says why).
        ui.add_enabled_ui(view.is_running(), |ui| {
            for plugin in &view.plugins {
                plugin_row(ui, plugin, view.is_running(), state);
            }
        });
    }

    ui.separator();
    plugin_logs_section(ui, view, state);
}

/// One discovered plugin: status, metadata, path, capture size, its generated
/// controls and the per-plugin switch.
fn plugin_row(
    ui: &mut egui::Ui,
    plugin: &crate::model::PluginInfoView,
    running: bool,
    state: &mut UiState,
) {
    let status = plugin.status();
    let status_colour = match status {
        crate::model::PluginStatus::Loaded => theme::OK,
        crate::model::PluginStatus::Disabled => theme::MUTED,
        crate::model::PluginStatus::Error => theme::DANGER,
    };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 4.0, status_colour);
        ui.label(egui::RichText::new(plugin.title()).strong().size(13.0));
        ui.label(
            egui::RichText::new(status.label())
                .size(10.5)
                .color(status_colour),
        );
        ui.add_space(2.0);
        let mut enabled = plugin.enabled;
        if ui
            .checkbox(&mut enabled, "Enabled")
            .on_hover_text(format!("Run {}", plugin.title()))
            .changed()
        {
            state.queue(UiCommand::SetPluginEnabled {
                path: plugin.path.clone(),
                enabled,
            });
        }
    });

    if !plugin.credit().is_empty() {
        ui.label(
            egui::RichText::new(plugin.credit())
                .size(11.0)
                .color(theme::MUTED),
        );
    }
    let path = truncate(&plugin.path, 52);
    ui.label(
        egui::RichText::new(path)
            .monospace()
            .size(10.5)
            .color(theme::MUTED),
    )
    .on_hover_text(&plugin.path);
    if let Some(error) = &plugin.error {
        ui.colored_label(theme::DANGER, error);
    }
    if let Some(capture) = plugin.capture_label() {
        ui.label(
            egui::RichText::new(capture)
                .monospace()
                .size(10.5)
                .color(theme::MUTED),
        );
    }

    if !plugin.props.is_empty() {
        ui.add_space(2.0);
        // Controls are live only while the host runs this plugin; a disabled
        // plugin has no Boa context to write into.
        ui.add_enabled_ui(running && plugin.loaded && plugin.enabled, |ui| {
            for prop in &plugin.props {
                plugin_prop_control(ui, plugin, prop, state);
            }
        });
    }
    ui.separator();
}

/// One generated control for a plugin prop schema.
fn plugin_prop_control(
    ui: &mut egui::Ui,
    plugin: &crate::model::PluginInfoView,
    prop: &crate::model::PluginPropView,
    state: &mut UiState,
) {
    use crate::model::PluginPropKindView;

    let queue = |state: &mut UiState, value: serde_json::Value| {
        state.queue(UiCommand::SetPluginProp {
            path: plugin.path.clone(),
            prop: prop.name.clone(),
            value,
        });
    };

    match prop.kind {
        PluginPropKindView::Int | PluginPropKindView::Float => {
            let (min, max, step) = prop.bounds();
            let mut control = RangeControl::new(
                &prop.name,
                prop.value.as_f64().unwrap_or(0.0),
                min,
                max,
                step,
            );
            if let Some(default) = prop.default.as_f64() {
                control = control.with_default(default);
            }
            if control.draw(ui) {
                queue(state, prop.number_value(control.value));
            }
        }
        PluginPropKindView::Bool => {
            let mut value = prop.value.as_bool().unwrap_or(false);
            if ui.checkbox(&mut value, &prop.name).changed() {
                queue(state, serde_json::Value::Bool(value));
            }
        }
        PluginPropKindView::Text => {
            let mut text = prop.value.as_str().unwrap_or_default().to_string();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&prop.name)
                        .size(13.0)
                        .color(theme::MUTED),
                );
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id_salt(("modv-plugin-prop", &plugin.path, &prop.name))
                        .desired_width(ui.available_width().max(80.0)),
                );
                if response.changed() {
                    queue(state, serde_json::Value::String(text.clone()));
                }
            });
        }
        PluginPropKindView::Color => {
            let mut control = ColorControl::new(&prop.name, &prop.value);
            if control.draw(ui) {
                queue(state, crate::model::plugin_color_value(control.rgba));
            }
        }
        PluginPropKindView::Select => {
            let selected = prop
                .selected_label()
                .map(str::to_string)
                .unwrap_or_else(|| prop.value.to_string());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&prop.name)
                        .size(13.0)
                        .color(theme::MUTED),
                );
                egui::ComboBox::from_id_salt(("modv-plugin-prop", &plugin.path, &prop.name))
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for option in &prop.options {
                            let is_selected = option.value == prop.value;
                            if ui.selectable_label(is_selected, &option.label).clicked() {
                                queue(state, option.value.clone());
                            }
                        }
                    });
            });
        }
    }
}

/// The collapsible deduplicated runtime-log window.
fn plugin_logs_section(ui: &mut egui::Ui, view: &crate::model::PluginView, state: &mut UiState) {
    let has_problem = view.logs.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("error") || lower.contains("warn")
    });
    let label = if view.logs.is_empty() {
        "Logs & errors".to_string()
    } else {
        format!("Logs & errors ({})", view.logs.len())
    };
    egui::CollapsingHeader::new(label)
        .id_salt("modv-plugin-logs")
        .default_open(has_problem)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button("Clear")
                    .on_hover_text("Drop the retained panel lines")
                    .clicked()
                {
                    state.queue(UiCommand::ClearPluginLogs);
                }
                if ui
                    .small_button("Copy")
                    .on_hover_text("Copy every retained line to the clipboard")
                    .clicked()
                {
                    ui.ctx().copy_text(view.logs.join("\n"));
                }
            });
            if view.logs.is_empty() {
                ui.label(
                    egui::RichText::new("No plugin messages.")
                        .size(11.5)
                        .color(theme::MUTED),
                );
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("modv-plugin-logs-scroll")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for line in &view.logs {
                            let colour = if line.contains("error") {
                                theme::DANGER
                            } else if line.contains("warn") {
                                theme::WARNING
                            } else {
                                theme::MUTED
                            };
                            ui.label(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(10.5)
                                    .color(colour),
                            );
                        }
                    });
            }
        });
}

/// The JavaScript-plugin consent dialog.
///
/// The first enable only opens this window; the checkbox itself never queues a
/// command. Confirming records consent **and** enables the host, cancelling
/// changes nothing.
fn plugin_consent_window(ctx: &egui::Context, state: &mut UiState) {
    if !state.show_plugin_consent {
        return;
    }
    let mut open = true;
    let mut confirm = false;
    let mut cancel = false;
    egui::Window::new("Enable JavaScript plugins")
        .id(egui::Id::unique("modv-plugin-consent"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Advanced")
                    .strong()
                    .size(12.0)
                    .color(theme::WARNING),
            );
            ui.label("JavaScript plugins run arbitrary JavaScript inside modV.");
            ui.label(
                "They run in a restricted Boa sandbox with no DOM, no filesystem or \
                 network globals, and no bundled grab-canvas.",
            );
            ui.label(
                "Plugins can read an allow-listed, read-only snapshot of modV state \
                 (tempo, audio bands, group and module names) and request a small RGBA \
                 capture of the output. modV.ws is not wired in the VmodV shell yet.",
            );
            ui.label(
                "The sandbox is a containment layer, not a security boundary: a bug in \
                 the host or in a plugin can still affect the running app. Only enable \
                 scripts you trust.",
            );
            ui.label(
                egui::RichText::new(
                    "You can disable plugins or revoke this consent at any time in the \
                     Plugins panel.",
                )
                .size(11.0)
                .color(theme::MUTED),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Enable plugins").clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if confirm {
        state.confirm_plugin_consent();
    }
    if cancel || !open {
        state.cancel_plugin_consent();
    }
}

/// The Library tab: the active profile's images, videos and palettes.
///
/// The panel browses recursively (the scan is one level of folders, the tiles
/// are every asset below the current scope), with a folder breadcrumb/back
/// control, subfolder chips and a rescan button. Clicking an asset assigns it
/// to a waiting texture prop; dragging one onto a texture control does the
/// same through [`MediaLibraryDrag`].
fn library_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let library = &snapshot.library;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("MEDIA LIBRARY").strong().size(12.0));
        chip(ui, &library.profile, theme::MUTED);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button("Rescan")
                .on_hover_text("Re-read the profile's image, video and palette folders")
                .clicked()
            {
                state.queue(UiCommand::RescanLibrary);
            }
            if ui
                .button("Open folder")
                .on_hover_text("Show the current Library folder in the file manager")
                .clicked()
            {
                state.queue(UiCommand::OpenLibraryFolder {
                    folder: state.library_kind.folder().to_string(),
                    relative: state.library_folder.clone(),
                });
            }
        });
    });
    if let Some(error) = &library.error {
        ui.colored_label(theme::DANGER, error);
    }
    ui.horizontal_wrapped(|ui| {
        for kind in LibraryKind::ALL {
            if ui
                .selectable_label(state.library_kind == kind, kind.title())
                .clicked()
            {
                state.select_library_kind(kind);
            }
        }
    });

    ui.horizontal(|ui| {
        let can_go_up = !state.library_folder.is_empty();
        if ui
            .add_enabled(can_go_up, egui::Button::new("Up"))
            .on_hover_text("Up one folder")
            .clicked()
        {
            state.library_up();
        }
        let mut breadcrumb = format!("{} / {}", library.profile, state.library_kind.folder());
        if !state.library_folder.is_empty() {
            breadcrumb.push_str(" / ");
            breadcrumb.push_str(&state.library_folder.replace('/', " / "));
        }
        ui.label(
            egui::RichText::new(breadcrumb)
                .monospace()
                .size(11.0)
                .color(theme::MUTED),
        );
    });

    if let Some(target) = state.library_target.clone() {
        egui::Frame::NONE
            .fill(theme::ACCENT_DIM)
            .inner_margin(Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(target.describe()).size(12.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            state.clear_library_target();
                        }
                    });
                });
                ui.label(
                    egui::RichText::new("Click a matching asset to assign it.")
                        .size(10.5)
                        .color(theme::MUTED),
                );
            });
    }

    let subfolders = library.subfolders(state.library_kind, &state.library_folder);
    if !subfolders.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("FOLDERS")
                    .size(10.5)
                    .color(theme::MUTED),
            );
            for name in &subfolders {
                if ui
                    .selectable_label(false, format!("{name}/"))
                    .on_hover_text(format!("Open the {name} folder"))
                    .clicked()
                {
                    state.open_library_subfolder(name);
                }
            }
        });
    }

    let assets = library.assets_for(state.library_kind, &state.library_folder);
    let count = assets.len();
    let available_height = ui.available_height().max(1.0);
    egui::ScrollArea::vertical()
        .id_salt("library-grid")
        .max_height(available_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if assets.is_empty() {
                let kind = state.library_kind;
                ui.label(
                    egui::RichText::new(format!(
                        "No {} in the {} profile yet.",
                        kind.title().to_lowercase(),
                        library.profile
                    ))
                    .size(12.0)
                    .color(theme::MUTED),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "Copy files into {} / {} (subfolders are scanned too), then Rescan.",
                        library.profile,
                        kind.folder()
                    ))
                    .size(11.0)
                    .color(theme::MUTED),
                );
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for asset in assets {
                    library_tile(ui, asset, state);
                }
            });
        });

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(if count == 1 {
                "1 asset".to_string()
            } else {
                format!("{count} assets")
            })
            .size(10.5)
            .color(theme::MUTED),
        );
        if library.truncated {
            ui.label(
                egui::RichText::new("· folder capped, open a subfolder")
                    .size(10.5)
                    .color(theme::TWEEN),
            );
        }
    });
}

/// One Library tile: a letterboxed thumbnail, the file name and cheap
/// metadata. It is both a click target and a drag source.
fn library_tile(ui: &mut egui::Ui, asset: &MediaAssetView, state: &mut UiState) {
    let size = vec2(132.0, 112.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &asset.name));
    if response.drag_started() {
        response.dnd_set_drag_payload(MediaLibraryDrag::new(asset));
    }

    let selected = state.selected_library_asset.as_deref() == Some(asset.path.as_str());
    let armed = state
        .library_target
        .as_ref()
        .is_some_and(|target| target.kind == asset.kind);
    let dragging = response.dragged();
    let fill = if selected || dragging {
        theme::ACCENT_DIM
    } else if response.hovered() {
        theme::PANEL_ALT
    } else {
        theme::PANEL
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(0), fill);

    let thumb = egui::Rect::from_min_size(rect.min + vec2(2.0, 2.0), vec2(size.x - 4.0, 74.0));
    painter.rect_filled(thumb, CornerRadius::same(0), theme::PANEL_ALT);
    if let Some(preview) = asset.thumbnail {
        let fitted = letterbox_rect(thumb, preview.width, preview.height);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(preview.id, fitted, uv, Color32::WHITE);
    } else {
        let placeholder = match asset.kind {
            LibraryKind::Image => "IMAGE",
            LibraryKind::Video => "VIDEO",
            LibraryKind::Palette => "PALETTE",
        };
        painter.text(
            thumb.center(),
            Align2::CENTER_CENTER,
            placeholder,
            egui::FontId::proportional(10.5),
            theme::MUTED,
        );
        if asset.kind == LibraryKind::Video
            && let Some(duration) = asset.duration_ms
        {
            painter.text(
                thumb.center() + vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                format_duration(duration),
                egui::FontId::proportional(10.0),
                theme::MUTED,
            );
        }
    }

    let text_painter = ui.painter().with_clip_rect(rect);
    text_painter.text(
        egui::pos2(rect.left() + 4.0, thumb.bottom() + 3.0),
        Align2::LEFT_TOP,
        &asset.name,
        egui::FontId::proportional(11.5),
        theme::FOREGROUND,
    );
    text_painter.text(
        egui::pos2(rect.left() + 4.0, thumb.bottom() + 18.0),
        Align2::LEFT_TOP,
        asset_meta_label(asset),
        egui::FontId::proportional(10.0),
        theme::MUTED,
    );

    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(
            1.0,
            if armed || selected || dragging {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );

    if response.clicked() {
        state.selected_library_asset = Some(asset.path.clone());
        if armed {
            state.assign_library_asset(asset);
        }
    }
}

fn audio_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let selection = &snapshot.audio_selection;
    audio_active_banner(ui, snapshot);

    if let Some(error) = &selection.error {
        ui.add_space(4.0);
        ui.colored_label(theme::DANGER, error);
        ui.label(
            egui::RichText::new("The previous source is still running.")
                .size(11.0)
                .color(theme::MUTED),
        );
    }

    ui.separator();
    ui.label(egui::RichText::new("OUTPUT MONITOR").strong().size(11.0));
    ui.label(
        egui::RichText::new("Capture what this machine is playing (system audio loopback).")
            .size(11.0)
            .color(theme::MUTED),
    );
    if audio_choice_row(
        ui,
        selection.is_system(),
        "System audio (loopback)",
        "Captures everything your speakers play",
        "Capture the default output device (system audio loopback)",
    ) {
        state.queue(UiCommand::SetAudioSource {
            source: "system".to_string(),
            device: None,
        });
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("INPUT DEVICES").strong().size(11.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let count = snapshot.audio_devices.len();
            ui.label(
                egui::RichText::new(format!(
                    "{count} input{}",
                    if count == 1 { "" } else { "s" }
                ))
                .monospace()
                .size(11.0)
                .color(theme::MUTED),
            );
        });
    });
    ui.label(
        egui::RichText::new(
            "Microphones and audio interfaces. The system default follows hot-plug.",
        )
        .size(11.0)
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
                .size(12.0)
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
            egui::RichText::new("Re-scans audio input devices")
                .size(11.0)
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
        .corner_radius(CornerRadius::same(0))
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
                        .size(11.0)
                        .color(colour),
                );
                ui.label(
                    egui::RichText::new(selection.active_label(&snapshot.audio_devices))
                        .strong()
                        .size(15.0),
                );
            });
            ui.label(
                egui::RichText::new(selection.active_subtitle(&snapshot.audio_devices)).size(12.0),
            );
            ui.label(
                egui::RichText::new(selection.active_detail(&snapshot.audio_devices))
                    .monospace()
                    .size(10.5)
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
        .corner_radius(CornerRadius::same(0))
        .inner_margin(Margin::symmetric(4, 3))
        .show(ui, |ui| {
            let response = ui.radio(selected, egui::RichText::new(title).size(13.0));
            clicked = response.clicked();
            response.on_hover_text(hover);
            if !detail.is_empty() {
                ui.label(egui::RichText::new(detail).size(10.5).color(theme::MUTED))
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
            ui.label(egui::RichText::new(error).size(12.0).color(theme::MUTED));
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
            .size(12.0)
            .color(theme::MUTED),
        );
    });
    ui.separator();
    ui.label(egui::RichText::new("CAMERAS").strong().size(12.0));
    ui.label(
        egui::RichText::new("Ticked cameras run in order; #N is the Webcam module's Camera index.")
            .size(11.0)
            .color(theme::MUTED),
    );
    if media.cameras.is_empty() {
        ui.label(
            egui::RichText::new("No cameras detected.")
                .size(12.0)
                .color(theme::MUTED),
        );
    }
    let running = media.running_cameras();
    for camera in media.ordered_cameras() {
        media_camera_row(ui, camera, &running, state);
    }
    ui.separator();
    media_screen_section(ui, &media.screen, state);
    ui.separator();
    ui.label(egui::RichText::new("SOURCES").strong().size(12.0));
    if media.sources.is_empty() {
        ui.label(
            egui::RichText::new("No running sources.")
                .size(12.0)
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
                ui.label(egui::RichText::new(title).size(12.0));
                ui.label(
                    egui::RichText::new(format!(
                        "{}x{}  {} frames  {} dropped",
                        source.width, source.height, source.frames, source.dropped
                    ))
                    .monospace()
                    .size(10.5)
                    .color(theme::MUTED),
                );
                if source.kind == "file"
                    && let Some(transport) = source.transport
                {
                    media_transport_row(ui, source, transport, state);
                }
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

/// The Media panel's transport for one running file source.
///
/// Playback speed is deliberately *not* offered here — it lives only on the
/// module's Inspector texture control so there is a single source of truth.
///
/// The shell persists pause/loop into every prop that references the source,
/// so a change here survives a preset/Scene reload just like the Inspector's
/// transport row. Seeking is transient: it drives the source but is never
/// written back into a prop.
fn media_transport_row(
    ui: &mut egui::Ui,
    source: &MediaSourceView,
    transport: VideoTransportView,
    state: &mut UiState,
) {
    ui.horizontal(|ui| {
        let (label, action) = if transport.paused || transport.eos {
            ("Play", VideoTransportAction::Resume)
        } else {
            ("Pause", VideoTransportAction::Pause)
        };
        if ui.small_button(label).clicked() {
            state.push_media_action(MediaAction::VideoTransport {
                source: source.source_id.clone(),
                action,
            });
        }
        if ui.small_button("Restart").clicked() {
            state.push_media_action(MediaAction::VideoTransport {
                source: source.source_id.clone(),
                action: VideoTransportAction::Restart,
            });
        }
    });

    ui.horizontal(|ui| {
        ui.spacing_mut().slider_width = 120.0;
        if let Some(position_ms) = seek_position(ui, &transport) {
            state.push_media_action(MediaAction::VideoTransport {
                source: source.source_id.clone(),
                action: VideoTransportAction::Seek { position_ms },
            });
        }
    });

    ui.horizontal(|ui| {
        let mut looping = transport.looping;
        if ui.checkbox(&mut looping, "Loop").changed() {
            state.push_media_action(MediaAction::VideoTransport {
                source: source.source_id.clone(),
                action: VideoTransportAction::SetLooping { looping },
            });
        }
    });
    ui.label(
        egui::RichText::new(transport.position_label())
            .monospace()
            .size(10.5)
            .color(theme::MUTED),
    );
}

/// The Media panel's screen-capture section.
///
/// The two buttons raise the XDG desktop portal's own picker, which is also
/// what chooses *which* monitor or window: there is nothing to enumerate in the
/// panel. A request is one at a time (the shell owns a single `screen:portal`
/// source), so both buttons are disabled while a dialog is open or a screen is
/// running; the running source is stopped from the existing Sources list.
fn media_screen_section(ui: &mut egui::Ui, screen: &ScreenCaptureView, state: &mut UiState) {
    ui.label(egui::RichText::new("SCREEN CAPTURE").strong().size(12.0));
    ui.label(
        egui::RichText::new(
            "Share a monitor or a window through the desktop's own picker. \
             The source runs as screen:portal like a camera.",
        )
        .size(11.0)
        .color(theme::MUTED),
    );
    if let Some(reason) = &screen.unavailable {
        ui.colored_label(
            theme::DANGER,
            format!("Screen capture is unavailable: {reason}"),
        );
    }

    let can_request = screen.can_request();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_request, egui::Button::new("Share monitor…"))
            .on_hover_text("Ask the desktop portal for a monitor to capture")
            .clicked()
        {
            state.push_media_action(MediaAction::ShareScreen { window: false });
        }
        if ui
            .add_enabled(can_request, egui::Button::new("Share window…"))
            .on_hover_text("Ask the desktop portal for an application window to capture")
            .clicked()
        {
            state.push_media_action(MediaAction::ShareScreen { window: true });
        }
    });

    if let Some(pending) = screen.pending {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Waiting for the portal dialog…")
                    .size(12.0)
                    .color(theme::ACCENT),
            );
            if ui
                .small_button("Cancel")
                .on_hover_text(
                    "Stop waiting for the dialog; the portal session closes when it is answered",
                )
                .clicked()
            {
                state.push_media_action(MediaAction::CancelScreenShare);
            }
        });
        ui.label(
            egui::RichText::new(format!("Sharing a {}", pending.label()))
                .size(11.0)
                .color(theme::MUTED),
        );
    }

    if let Some(running) = &screen.running {
        ui.label(
            egui::RichText::new(format!(
                "Sharing a {} (PipeWire node {}). Stop it in Sources to share something else.",
                running.kind.label(),
                running.node_id
            ))
            .size(11.0)
            .color(theme::ACCENT),
        )
        .on_hover_text(format!("Source id: {}", running.source_id));
    }

    if let Some(error) = &screen.error {
        ui.colored_label(theme::DANGER, error);
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
        let response = ui.checkbox(&mut checked, egui::RichText::new(&camera.name).size(13.0));
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
                    .size(11.0)
                    .color(theme::ACCENT),
            )
            .on_hover_text(format!("Webcam index #{index} (the module's Camera prop)"));
        }
        ui.label(
            egui::RichText::new(&camera.id)
                .monospace()
                .size(10.5)
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
                        egui::Button::new(egui::RichText::new("Down").size(11.0)).small(),
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
                        egui::Button::new(egui::RichText::new("Up").size(11.0)).small(),
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
            .size(10.5)
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
                .size(11.5),
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
                .size(10.5)
                .color(theme::MUTED),
        );
    } else {
        egui::ComboBox::from_id_salt(("camera-fps", &camera.id))
            .selected_text(
                egui::RichText::new(chosen.fps_label())
                    .monospace()
                    .size(11.5),
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
            .size(13.0)
            .color(theme::MUTED),
    );
    ui.add_space(8.0);

    if let Some(group) = state.selected_group(snapshot) {
        ui.label(egui::RichText::new("GROUP").strong().size(12.0));
        ui.label(egui::RichText::new(&group.name).size(14.0));
        let mut alpha = group.alpha;
        if theme::slider(
            ui,
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
    module_name: &str,
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
            // Keep the link control in the prop editor's trailing slot in both
            // states so the slider reserves the same width whether or not the
            // prop is linked. `link_control_inline` renders `+ link` while
            // unlinked and a lit `LINKED` button once a link exists.
            let changed = match prop.input_id.as_deref() {
                Some(input_id) => {
                    let mut link = |ui: &mut egui::Ui| {
                        link_control_inline(ui, &prop.name, input_id, snapshot, state);
                    };
                    control.draw_with_trailing(ui, Some(&mut link))
                }
                None => control.draw(ui),
            };
            if changed {
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
            let current = prop.value.clone();
            let current_label = prop
                .enum_members
                .iter()
                .find(|member| member.value == current)
                .map(|member| member.label.clone())
                .unwrap_or_else(|| {
                    current
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| current.to_string())
                });
            egui::ComboBox::from_id_salt((module_id, &prop.name))
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    for member in &prop.enum_members {
                        let selected = member.value == current;
                        if ui.selectable_label(selected, &member.label).clicked() {
                            state.push_action(
                                "modules/updateProp",
                                serde_json::json!({
                                    "moduleId": module_id,
                                    "prop": prop.name,
                                    "data": { "value": member.value },
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
                        .size(13.0)
                        .color(theme::MUTED),
                );
                chip(ui, "palette", theme::TWEEN);
            });
            if let serde_json::Value::Array(colors) = &prop.value {
                for (index, color) in colors.iter().take(8).enumerate() {
                    ui.horizontal(|ui| {
                        let [r, g, b, a] = palette_prop::entry_rgba(color);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(0),
                            egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                        );
                        let mut text = palette_prop::entry_hex(color);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .id_salt(("palette-prop-hex", module_id, &prop.name, index))
                                .desired_width(90.0)
                                .hint_text("#RRGGBB"),
                        );
                        if response.changed() {
                            let mut value = prop.value.clone();
                            if palette_prop::set_entry_hex(&mut value, index, &text) {
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
                    });
                }
            }
        }
        "texture" => {
            let mut control = TextureControl::new(&prop.name, &prop.value);
            match control.draw(
                ui,
                &snapshot.texture_sources,
                &snapshot.library,
                &snapshot.media,
            ) {
                Some(TextureControlChange::Value(value)) => {
                    state
                        .actions
                        .push(texture_prop_action(module_id, &prop.name, value));
                }
                Some(TextureControlChange::PickFromLibrary { kind }) => {
                    state.arm_library_target(LibraryAssignmentTarget {
                        module_id: module_id.to_string(),
                        module_name: module_name.to_string(),
                        prop_name: prop.name.clone(),
                        kind,
                    });
                }
                Some(TextureControlChange::VideoTransport {
                    value,
                    source,
                    action,
                }) => {
                    if let Some(value) = value {
                        state
                            .actions
                            .push(texture_prop_action(module_id, &prop.name, value));
                    }
                    state.push_media_action(MediaAction::VideoTransport { source, action });
                }
                None => {}
            }
        }
        "font" => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&prop.name)
                        .size(13.0)
                        .color(theme::MUTED),
                );
                chip(ui, "font", theme::MUTED);
                let mut text = prop.value.as_str().unwrap_or("default").to_string();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id_salt(("font-prop", module_id, &prop.name))
                        .desired_width(120.0)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("default"),
                );
                if response.changed() {
                    state.push_action(
                        "modules/updateProp",
                        serde_json::json!({
                            "moduleId": module_id,
                            "prop": prop.name,
                            "data": { "value": text },
                        }),
                    );
                }
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

/// The performance toolbar: the run toggle, Scene order, timing unit, the
/// min/max/fade values with their resolved-duration hint, and the right-pinned
/// `Open output` button, all on one line.
///
/// It spans the whole window rather than the centre column: the centre column
/// is narrower than the full control set at 1280x800, so a one-line row there
/// would run under `Open output`.
fn performance_header(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    egui::Panel::top("modv-performance")
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .inner_margin(Margin::symmetric(8, 4)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                crate::panels::scene_grid::draw_performance_bar(ui, snapshot, &mut state.actions);
                ui.separator();
                crate::panels::scene_grid::draw_timing_controls(ui, snapshot, &mut state.actions);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Open output").clicked() {
                        state.queue(UiCommand::OpenOutputWindow);
                    }
                    if snapshot.performance.enabled {
                        chip(ui, "PERFORMANCE", theme::OK);
                    }
                });
            });
        });
}

fn central_panel(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    let response = egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::BACKGROUND)
                .inner_margin(Margin::same(8)),
        )
        .show(ui, |ui| {
            // Live Output and Gallery are resizable top sections; the Scenes
            // grid takes the remainder. The saved heights are applied as the
            // first-show defaults and captured from the real section rects so
            // VmodvApp can persist them.
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

    // The central area is the "float it here" target: release anywhere over it
    // to open the tab as a floating window at the pointer, clamped on-screen.
    let central_rect = response.response.rect;
    if state.tab_drag.is_some() {
        let hovered = ui.rect_contains_pointer(central_rect);
        paint_tab_drop_zone(
            ui,
            central_rect.shrink(3.0),
            hovered,
            hovered,
            Some("Float here"),
        );
        if let Some(payload) = released_tab(ui, central_rect) {
            let pointer = ui
                .input(|input| input.pointer.interact_pos())
                .unwrap_or_else(|| central_rect.center());
            apply_tab_drop(ui.ctx(), state, payload.tab, TabDropTarget::Float(pointer));
        }
    }
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
    painter.rect_filled(image_rect, CornerRadius::same(0), theme::INPUT);
    if let Some(texture_id) = state.preview_texture {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture_id, image_rect.shrink(1.0), uv, Color32::WHITE);
    } else {
        painter.text(
            image_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for the first composited frame",
            egui::FontId::proportional(14.0),
            theme::MUTED,
        );
    }
    painter.rect_stroke(
        image_rect,
        CornerRadius::same(0),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    painter.text(
        image_rect.left_top() + vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        "LIVE OUTPUT",
        egui::FontId::monospace(11.0),
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
        egui::FontId::proportional(11.0),
        theme::MUTED,
    );
}

fn gallery_strip(ui: &mut egui::Ui, snapshot: &EngineSnapshot, state: &mut UiState) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("GALLERY").strong().size(12.0));
        for category in GalleryCategory::ALL {
            if ui
                .selectable_label(
                    state.gallery_category == category,
                    egui::RichText::new(category.label()).size(12.0),
                )
                .clicked()
            {
                state.gallery_category = category;
            }
        }
        ui.separator();
        let search_width = (ui.available_width() - 120.0).clamp(140.0, 220.0);
        let search = ui
            .add(
                egui::TextEdit::singleline(&mut state.search)
                    .hint_text("Search modules  (type:isf)  (Ctrl+K)")
                    .desired_width(search_width),
            )
            .on_hover_text("Double-click a tile to add it to the selected Group");
        if state.search_focus {
            search.request_focus();
            state.search_focus = false;
        }
        ui.label(
            egui::RichText::new(format!("{} registered", snapshot.registered.len()))
                .size(12.0)
                .color(theme::MUTED),
        );
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
                            .size(12.0)
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
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    // The tile name is painted, not a label widget; expose it to assistive
    // technology (and to the kittest pointer-driven drag tests) as a button.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &module.name));
    // The tile is a Gallery drag source: the payload is the registered module
    // name, which every Group block resolves into `ui/addModuleToGroup`.
    if response.drag_started() {
        response.dnd_set_drag_payload(GalleryDrag::new(module.name.clone()));
    }
    let selected = state.selected_gallery_module.as_deref() == Some(module.name.as_str());
    let dragging = response.dragged();
    let fill = if selected || dragging {
        theme::ACCENT_DIM
    } else if response.hovered() {
        theme::PANEL_ALT
    } else {
        theme::PANEL
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(0), fill);
    if let Some(preview) = snapshot
        .gallery_previews
        .get(&gallery_preview_key(&module.name))
    {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(preview.id, rect.shrink(2.0), uv, Color32::WHITE);
        painter.rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(0),
            Color32::from_black_alpha(55),
        );
    } else {
        painter.rect_filled(rect.shrink(2.0), CornerRadius::same(0), theme::PANEL_ALT);
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(
            1.0,
            if selected || dragging {
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
        egui::FontId::proportional(12.0),
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
        egui::FontId::monospace(10.0),
        theme::MUTED,
    );

    if response.hovered() {
        // The tile is draggable as well as clickable; the grab cursor is the
        // only resting hint, so it does not add a permanent affordance.
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
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
            .size(11.0)
            .color(theme::MUTED),
        );
        ui.separator();
        if ui.button("Info").clicked() {
            state.open_module_info(module.clone());
            ui.close();
        }
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
                    .size(13.0)
                    .color(theme::MUTED),
            );
        }
        for (index, (label, _)) in filtered.iter().enumerate() {
            let selected = index == state.command_selection;
            if ui
                .selectable_label(selected, egui::RichText::new(label).size(13.0))
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
                .size(12.0)
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
                .size(12.0)
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
            ("Ctrl/Cmd+O", "Open preset…"),
            ("Ctrl/Cmd+Shift+O", "Open Scene…"),
            ("Ctrl/Cmd+S", "Save preset"),
            ("Ctrl/Cmd+Shift+S", "Save preset as…"),
            ("Ctrl/Cmd+K", "Focus Gallery search"),
            ("Ctrl/Cmd+Enter", "Apply the selected Scene"),
            ("Ctrl/Cmd+Z", "Undo the last edit"),
            ("Ctrl/Cmd+Shift+Z", "Redo the last undone edit"),
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

/// The ordinary About dialog. The startup splash is a separate, logo-only
/// surface (see [`crate::splash`]); About keeps its build/profile information.
fn about_window(ctx: &egui::Context, state: &mut UiState) {
    let window = restore_window(
        egui::Window::new("About VmodV")
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
            egui::RichText::new("VmodV")
                .strong()
                .size(20.0)
                .color(theme::ACCENT),
        );
        ui.label("VmodV — a Rust build of modV, one binary, no Electron. The 1.x tree is archived (branch `legacy-electron`, tag `v1.x-final`).");
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

/// The module info window opened from a Gallery tile's context menu.
///
/// Shows the module's Gallery metadata and its third-party licence, as recorded
/// in `THIRD_PARTY_NOTICES.md`.
fn module_info_window(ctx: &egui::Context, state: &mut UiState) {
    let Some(meta) = state.module_info.as_ref() else {
        return;
    };
    let mut open = true;
    let window = restore_window(
        egui::Window::new(format!("{} — Module info", meta.name))
            .id(egui::Id::unique("modv-window:module-info"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .min_width(320.0)
            .default_size([420.0, 300.0]),
        WINDOW_MODULE_INFO,
        &state.layout,
    );
    let response = window.show(ctx, |ui| {
        ui.label(
            egui::RichText::new(&meta.name)
                .strong()
                .size(18.0)
                .color(theme::ACCENT),
        );
        ui.label(format!("Type: {}", meta.module_type));
        ui.label(format!("Author: {}", meta.author));
        ui.label(format!("Version: {}", meta.version));
        ui.label(format!("License: {}", module_license(meta)));
        ui.separator();
        ui.label(&meta.description);
    });
    if !open {
        state.module_info = None;
    }
    if let Some(response) = response {
        state
            .layout
            .record_window_rect(WINDOW_MODULE_INFO, response.response.rect);
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
