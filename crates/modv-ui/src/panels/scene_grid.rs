//! The Scenes grid and its cell widget.
//!
//! Port of `src/renderer/src/components/Scenes.vue` (2212 LOC) as an egui
//! probe. P0 implements the load-bearing interactions: live hover preview,
//! current/disabled/timing states, Ctrl+drag cell copy and the cell context
//! menu. P4 adds column headers, the row-name editor, playlist import/export,
//! scene reordering and the transition picker.

use egui::{Align2, Color32, CornerRadius, FontId, Sense, Stroke, StrokeKind, Vec2};
use serde_json::Value;

use crate::model::{EngineSnapshot, PreviewRequest, SceneCellView, SceneView, scene_preview_key};
use crate::state::UiAction;
use crate::theme;
use crate::widgets::range_control::chip;

/// A cell address inside the Scenes grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRef {
    pub scene_id: String,
    pub row: usize,
}

impl CellRef {
    pub fn new(scene_id: impl Into<String>, row: usize) -> Self {
        Self {
            scene_id: scene_id.into(),
            row,
        }
    }
}

/// Durable interaction state for the grid.
#[derive(Clone, Debug, Default)]
pub struct SceneGridState {
    pub hovered: Option<CellRef>,
    pub copy_source: Option<CellRef>,
    /// Hover-preview requests drained by the shell after drawing.
    pub preview_requests: Vec<PreviewRequest>,
    last_preview_request: Option<std::time::Instant>,
    hover_scene: Option<(String, usize)>,
    /// Inline Scene-name editor: `(scene_id, draft)`.
    pub renaming: Option<(String, String)>,
    /// One-shot focus request for the inline editor.
    pub rename_focus: bool,
    /// Per-Scene timing override editor.
    pub timing_edit: Option<TimingEdit>,
    /// The row-name list editor.
    pub row_names_edit: Option<Vec<String>>,
}

/// The small Scene-timing dialog's draft values.
#[derive(Clone, Debug, PartialEq)]
pub struct TimingEdit {
    pub scene_id: String,
    pub name: String,
    pub min_seconds: f64,
    pub max_seconds: f64,
}

impl SceneGridState {
    /// Start a Ctrl+drag copy from `cell`.
    pub fn begin_copy(&mut self, cell: CellRef) {
        self.copy_source = Some(cell);
    }

    pub fn cancel_copy(&mut self) {
        self.copy_source = None;
    }

    pub fn is_copying(&self) -> bool {
        self.copy_source.is_some()
    }

    /// Ask for a cell preview at most every `interval`.
    /// Begin inline renaming of a Scene.
    pub fn begin_rename(&mut self, scene: &SceneView) {
        self.renaming = Some((scene.id.clone(), scene.name.clone()));
        self.rename_focus = true;
    }

    /// Begin editing a Scene's timing override.
    pub fn begin_timing(&mut self, scene: &SceneView, performance: &crate::PerformanceView) {
        self.timing_edit = Some(TimingEdit {
            scene_id: scene.id.clone(),
            name: scene.name.clone(),
            min_seconds: scene.timing.unwrap_or(performance.min_seconds),
            max_seconds: scene.timing.unwrap_or(performance.max_seconds),
        });
    }

    pub fn request_preview(&mut self, scene_id: &str, row: usize, interval: std::time::Duration) {
        let now = std::time::Instant::now();
        let current = (scene_id.to_string(), row);
        let due = self
            .last_preview_request
            .map(|last| now.saturating_duration_since(last) >= interval)
            .unwrap_or(true);
        if self.hover_scene.as_ref() != Some(&current) || due {
            self.hover_scene = Some(current);
            self.last_preview_request = Some(now);
            self.preview_requests.push(PreviewRequest::Scene {
                scene_id: scene_id.to_string(),
                row_index: row,
            });
        }
    }

    /// Finish a copy drag over `target`. Returns the engine action, or `None`
    /// for a same-cell drop (which cancels the drag without an action).
    pub fn drop_copy(&mut self, target: &CellRef) -> Option<UiAction> {
        let source = self.copy_source.take()?;
        if source == *target {
            return None;
        }
        Some(UiAction::new(
            "scenes/copySceneCellToScene",
            copy_payload(&source, target),
        ))
    }
}

/// Width/height of one Scene cell.
const CELL_SIZE: Vec2 = Vec2::new(154.0, 76.0);

/// Draw the whole Scene grid.
pub fn draw_scene_grid(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    live_preview: Option<egui::TextureId>,
    actions: &mut Vec<UiAction>,
) {
    if snapshot.scenes.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label(egui::RichText::new("No Scenes yet").size(14.0));
            ui.label(egui::RichText::new("Add a Scene or load the defaults.").color(theme::MUTED));
            if ui.button("Add empty Scene").clicked() {
                actions.push(UiAction::new(
                    "scenes/addEmptyScene",
                    serde_json::json!({ "name": "New Scene" }),
                ));
            }
        });
        return;
    }

    performance_bar(ui, snapshot, grid, actions);
    ui.separator();

    let row_count = snapshot
        .scenes
        .iter()
        .map(|scene| scene.cells.len())
        .max()
        .unwrap_or(1)
        .max(snapshot.scene_rows.len())
        .max(1);

    egui::ScrollArea::horizontal()
        .id_salt("scenes-grid-horizontal")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for scene in &snapshot.scenes {
                    ui.vertical(|ui| {
                        let current = snapshot.performance.current_scene_id.as_deref()
                            == Some(scene.id.as_str());
                        let renaming = grid
                            .renaming
                            .as_ref()
                            .filter(|(id, _)| id == &scene.id)
                            .map(|(_, draft)| draft.clone());
                        if let Some(mut draft) = renaming {
                            let response = ui.add_sized(
                                [CELL_SIZE.x, 20.0],
                                egui::TextEdit::singleline(&mut draft)
                                    .font(egui::TextStyle::Small)
                                    .hint_text("Scene name"),
                            );
                            if grid.rename_focus {
                                response.request_focus();
                                grid.rename_focus = false;
                            }
                            let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                            let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                            if escape {
                                grid.renaming = None;
                            } else if enter || response.lost_focus() {
                                grid.renaming = None;
                                let name = draft.trim();
                                if !name.is_empty() && name != scene.name {
                                    actions.push(UiAction::new(
                                        "scenes/updateSceneName",
                                        serde_json::json!({
                                            "sceneId": scene.id,
                                            "name": name,
                                        }),
                                    ));
                                }
                            } else {
                                grid.renaming = Some((scene.id.clone(), draft));
                            }
                        } else {
                            let header = ui.add_sized(
                                [CELL_SIZE.x, 20.0],
                                egui::Button::new(
                                    egui::RichText::new(&scene.name)
                                        .size(11.0)
                                        .color(if current {
                                            theme::ACCENT
                                        } else {
                                            theme::FOREGROUND
                                        }),
                                )
                                .frame(false),
                            );
                            if header.clicked() {
                                actions.push(apply_scene_action(&scene.id));
                            }
                            if header.double_clicked() {
                                grid.begin_rename(scene);
                            }
                            header.context_menu(|ui| {
                                scene_column_menu(ui, scene, snapshot, grid, actions)
                            });
                        }

                        for row in 0..row_count {
                            let cell = scene.cell(row).cloned().unwrap_or_default();
                            let _ = draw_scene_cell(
                                ui,
                                scene,
                                row,
                                &cell,
                                snapshot,
                                grid,
                                live_preview,
                                actions,
                            );
                            ui.add_space(2.0);
                        }
                    });
                    ui.add_space(6.0);
                }
            });
        });

    // A copy drag released outside every cell is cancelled without an action.
    if grid.copy_source.is_some() && ui.input(|input| input.pointer.any_released()) {
        grid.cancel_copy();
    }

    scene_dialogs(ui, grid, actions);
}

/// Performance-mode controls above the Scene grid.
fn performance_bar(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal_wrapped(|ui| {
        let label = if snapshot.performance.enabled {
            "Stop performance"
        } else {
            "Start performance"
        };
        if ui.button(label).clicked() {
            actions.push(UiAction::new(
                "scenes/setPerformanceEnabled",
                serde_json::json!({ "enabled": !snapshot.performance.enabled }),
            ));
        }
        ui.separator();
        ui.label(egui::RichText::new("min").size(10.0).color(theme::MUTED));
        let mut min = snapshot.performance.min_seconds;
        let min_changed = ui
            .add(
                egui::DragValue::new(&mut min)
                    .range(1.0..=3600.0)
                    .speed(0.5)
                    .suffix(" s"),
            )
            .changed();
        ui.label(egui::RichText::new("max").size(10.0).color(theme::MUTED));
        let mut max = snapshot.performance.max_seconds;
        let max_changed = ui
            .add(
                egui::DragValue::new(&mut max)
                    .range(2.0..=7200.0)
                    .speed(0.5)
                    .suffix(" s"),
            )
            .changed();
        ui.label(egui::RichText::new("fade").size(10.0).color(theme::MUTED));
        let mut fade = snapshot.performance.transition_seconds;
        let fade_changed = ui
            .add(
                egui::DragValue::new(&mut fade)
                    .range(0.1..=30.0)
                    .speed(0.05)
                    .suffix(" s"),
            )
            .changed();
        if min_changed || max_changed || fade_changed {
            let min = if min_changed || max_changed {
                min.max(1.0).min(max - 1.0)
            } else {
                snapshot.performance.min_seconds
            };
            actions.push(UiAction::new(
                "scenes/updatePerformance",
                serde_json::json!({
                    "config": {
                        "minSeconds": min,
                        "maxSeconds": max.max(min + 1.0),
                        "transitionSeconds": fade,
                    }
                }),
            ));
        }
        ui.separator();
        if ui.button("Row names").clicked() {
            grid.row_names_edit = Some(snapshot.scene_rows.clone());
        }
        if ui.button("+ Scene").clicked() {
            actions.push(UiAction::new(
                "scenes/addEmptyScene",
                serde_json::json!({ "name": "New Scene" }),
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} Scenes", snapshot.scenes.len()))
                    .size(10.0)
                    .color(theme::MUTED),
            );
            if snapshot.performance.enabled {
                chip(ui, "PERFORMANCE", theme::OK);
            }
        });
    });
}

/// Modal editors for Scene timing override and performance row names.
fn scene_dialogs(ui: &mut egui::Ui, grid: &mut SceneGridState, actions: &mut Vec<UiAction>) {
    if let Some(mut edit) = grid.timing_edit.take() {
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(format!("Scene timing: {}", edit.name))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    egui::RichText::new("Per-Scene override; reset returns to the global timing.")
                        .size(10.0)
                        .color(theme::MUTED),
                );
                ui.horizontal(|ui| {
                    ui.label("Min");
                    ui.add(
                        egui::DragValue::new(&mut edit.min_seconds)
                            .range(1.0..=3600.0)
                            .speed(0.5)
                            .suffix(" s"),
                    );
                    ui.label("Max");
                    ui.add(
                        egui::DragValue::new(&mut edit.max_seconds)
                            .range(2.0..=7200.0)
                            .speed(0.5)
                            .suffix(" s"),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("Save override").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            actions.push(UiAction::new(
                "scenes/updateSceneTiming",
                serde_json::json!({
                    "sceneId": edit.scene_id,
                    "minSeconds": edit.min_seconds,
                    "maxSeconds": edit.max_seconds.max(edit.min_seconds + 1.0),
                }),
            ));
        } else if !cancel && open {
            grid.timing_edit = Some(edit);
        }
    }

    if let Some(mut names) = grid.row_names_edit.take() {
        let mut open = true;
        let mut save = false;
        egui::Window::new("Performance row names")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.label(
                    egui::RichText::new(
                        "A named row matches its Group by name; empty names use position.",
                    )
                    .size(10.0)
                    .color(theme::MUTED),
                );
                for (index, name) in names.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("Row {index}"))
                                .monospace()
                                .size(10.0)
                                .color(theme::MUTED),
                        );
                        ui.text_edit_singleline(name);
                    });
                }
                if ui.button("Add row").clicked() {
                    names.push(String::new());
                }
                if ui.button("Save").clicked() {
                    save = true;
                }
            });
        if save {
            actions.push(UiAction::new(
                "scenes/setRowNames",
                serde_json::json!({ "rowNames": names }),
            ));
        } else if open {
            grid.row_names_edit = Some(names);
        }
    }
}

/// Hover previews are requested only for cells that hold a Group. An empty
/// cell has no renderable row, and requesting one only earns the engine's
/// "has no renderable Group at row N" warning.
fn should_request_preview(cell: &SceneCellView, hovered: bool) -> bool {
    hovered && !cell.is_empty()
}

/// Draw one Scene cell. Returns `true` when this cell owned the pointer.
#[allow(clippy::too_many_arguments)]
fn draw_scene_cell(
    ui: &mut egui::Ui,
    scene: &SceneView,
    row: usize,
    cell: &SceneCellView,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    live_preview: Option<egui::TextureId>,
    actions: &mut Vec<UiAction>,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(CELL_SIZE, Sense::click_and_drag());
    let target = CellRef::new(scene.id.clone(), row);
    let hovered = response.hovered();
    if hovered {
        grid.hovered = Some(target.clone());
    }

    let current = snapshot.performance.current_scene_id.as_deref() == Some(scene.id.as_str());
    let copying = grid.copy_source.as_ref() == Some(&target);
    let fill = if copying {
        theme::ACCENT_DIM
    } else if current {
        Color32::from_rgb(34, 32, 18)
    } else if hovered {
        theme::PANEL_ALT
    } else {
        theme::PANEL
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), fill);
    let border = if copying || current {
        theme::ACCENT
    } else if hovered {
        Color32::from_rgb(96, 96, 96)
    } else {
        theme::BORDER
    };
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(if copying || current { 1.5 } else { 1.0 }, border),
        StrokeKind::Inside,
    );

    // Hover preview: the engine's isolated Scene-cell render, falling back to
    // the live composited frame until the first preview arrives.
    if should_request_preview(cell, hovered) {
        let key = scene_preview_key(&scene.id, row);
        let texture = snapshot
            .scene_previews
            .get(&key)
            .map(|preview| preview.id)
            .or(live_preview);
        if let Some(texture_id) = texture {
            let inner = rect.shrink(2.0);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture_id, inner, uv, Color32::from_white_alpha(210));
            painter.rect_filled(inner, CornerRadius::same(2), Color32::from_black_alpha(70));
        }
        grid.request_preview(&scene.id, row, std::time::Duration::from_millis(150));
    }

    // Row label (top-left), scene name (bottom-left) and module summary.
    let row_name = snapshot
        .scene_rows
        .get(row)
        .map(String::as_str)
        .unwrap_or("Row");
    painter.text(
        rect.left_top() + egui::vec2(6.0, 3.0),
        Align2::LEFT_TOP,
        row_name,
        FontId::proportional(9.0),
        theme::MUTED,
    );
    painter.text(
        rect.left_bottom() + egui::vec2(6.0, -5.0),
        Align2::LEFT_BOTTOM,
        cell.label(),
        FontId::proportional(10.0),
        if cell.is_empty() {
            theme::MUTED
        } else {
            theme::FOREGROUND
        },
    );

    // Enabled dot.
    let dot_colour = if scene.enabled {
        theme::OK
    } else {
        theme::DANGER
    };
    painter.circle_filled(rect.left_top() + egui::vec2(8.0, 26.0), 3.0, dot_colour);

    // Timing badge (top-right) uses the per-Scene override when present.
    if let Some(timing) = scene.timing {
        let label = format!("{timing:.0}s");
        let chip_rect = egui::Rect::from_min_size(
            rect.right_top() + egui::vec2(-54.0, 4.0),
            egui::vec2(48.0, 14.0),
        );
        painter.rect_filled(
            chip_rect,
            CornerRadius::same(2),
            Color32::from_rgb(38, 38, 38),
        );
        painter.text(
            chip_rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(8.5),
            theme::MUTED,
        );
    }

    if copying {
        let label = egui::Rect::from_min_size(
            rect.right_bottom() + egui::vec2(-44.0, -17.0),
            egui::vec2(38.0, 13.0),
        );
        painter.rect_filled(label, CornerRadius::same(2), theme::ACCENT_DIM);
        painter.text(
            label.center(),
            Align2::CENTER_CENTER,
            "COPY",
            FontId::monospace(8.0),
            theme::ACCENT,
        );
    }

    // Primary click applies the Scene; a double-click on an empty cell
    // creates the row's Group in that Scene.
    if response.clicked() && !cell.is_empty() {
        actions.push(apply_scene_action(&scene.id));
    }
    if response.double_clicked() && cell.is_empty() {
        actions.push(UiAction::new(
            "scenes/createSceneCellGroup",
            serde_json::json!({ "sceneId": scene.id, "rowIndex": row }),
        ));
    }

    // Ctrl/Cmd+drag starts a cell copy.
    let copy_modifier = ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
    if response.drag_started() && copy_modifier {
        grid.begin_copy(target.clone());
    }
    if grid.copy_source.is_some()
        && response.hovered()
        && ui.input(|input| input.pointer.any_released())
        && let Some(action) = grid.drop_copy(&target)
    {
        actions.push(action);
    }

    // Context menu.
    response.context_menu(|ui| {
        scene_cell_menu(ui, scene, row, cell, snapshot, grid, actions);
    });

    hovered
}

fn scene_cell_menu(
    ui: &mut egui::Ui,
    scene: &SceneView,
    row: usize,
    cell: &SceneCellView,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    actions: &mut Vec<UiAction>,
) {
    if ui.button("Apply Scene").clicked() {
        actions.push(apply_scene_action(&scene.id));
        ui.close();
    }
    if ui
        .add_enabled(
            !cell.is_empty(),
            egui::Button::new("Capture live row into this cell"),
        )
        .clicked()
    {
        actions.push(UiAction::new(
            "scenes/captureSceneRowToScene",
            serde_json::json!({ "sceneId": scene.id, "rowIndex": row }),
        ));
        ui.close();
    }
    if ui.button("Capture whole live state into Scene").clicked() {
        actions.push(UiAction::new(
            "scenes/captureLiveStateToScene",
            serde_json::json!({ "sceneId": scene.id }),
        ));
        ui.close();
    }
    if ui
        .add_enabled(!cell.is_empty(), egui::Button::new("Clear cell"))
        .clicked()
    {
        actions.push(clear_cell_action(&scene.id, row));
        ui.close();
    }
    ui.separator();
    if ui.button("Copy cell (Ctrl+drag)").clicked() {
        grid.begin_copy(CellRef::new(scene.id.clone(), row));
        ui.close();
    }
    if ui
        .add_enabled(
            snapshot.performance.enabled,
            egui::Button::new("Stop performance mode"),
        )
        .clicked()
    {
        actions.push(UiAction::new(
            "scenes/setPerformanceEnabled",
            serde_json::json!({ "enabled": false }),
        ));
        ui.close();
    }
    ui.separator();
    if ui
        .button(egui::RichText::new("Remove Scene").color(theme::DANGER))
        .clicked()
    {
        actions.push(UiAction::new(
            "scenes/removeScene",
            serde_json::json!({ "sceneId": scene.id }),
        ));
        ui.close();
    }
}

fn scene_column_menu(
    ui: &mut egui::Ui,
    scene: &SceneView,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    actions: &mut Vec<UiAction>,
) {
    if ui.button("Apply Scene").clicked() {
        actions.push(apply_scene_action(&scene.id));
        ui.close();
    }
    if ui.button("Rename Scene").clicked() {
        grid.begin_rename(scene);
        ui.close();
    }
    if ui.button("Capture live state into Scene").clicked() {
        actions.push(UiAction::new(
            "scenes/captureLiveStateToScene",
            serde_json::json!({ "sceneId": scene.id }),
        ));
        ui.close();
    }
    if ui
        .button(if scene.enabled {
            "Disable Scene"
        } else {
            "Enable Scene"
        })
        .clicked()
    {
        actions.push(UiAction::new(
            "scenes/setSceneEnabled",
            serde_json::json!({ "sceneId": scene.id, "enabled": !scene.enabled }),
        ));
        ui.close();
    }
    if ui.button("Edit timing override").clicked() {
        grid.begin_timing(scene, &snapshot.performance);
        ui.close();
    }
    if ui.button("Reset timing override").clicked() {
        actions.push(UiAction::new(
            "scenes/resetSceneTiming",
            serde_json::json!({ "sceneId": scene.id }),
        ));
        ui.close();
    }
    let index = snapshot
        .scenes
        .iter()
        .position(|candidate| candidate.id == scene.id)
        .unwrap_or(0);
    if ui
        .add_enabled(index > 0, egui::Button::new("Move left"))
        .clicked()
    {
        actions.push(UiAction::new(
            "scenes/moveScene",
            serde_json::json!({ "sceneId": scene.id, "toIndex": index - 1 }),
        ));
        ui.close();
    }
    if ui
        .add_enabled(
            index + 1 < snapshot.scenes.len(),
            egui::Button::new("Move right"),
        )
        .clicked()
    {
        actions.push(UiAction::new(
            "scenes/moveScene",
            serde_json::json!({ "sceneId": scene.id, "toIndex": index + 1 }),
        ));
        ui.close();
    }
    ui.separator();
    if ui
        .button(egui::RichText::new("Remove Scene").color(theme::DANGER))
        .clicked()
    {
        actions.push(UiAction::new(
            "scenes/removeScene",
            serde_json::json!({ "sceneId": scene.id }),
        ));
        ui.close();
    }
}

/// Build an `applyScene` action (used by tests and menu helpers).
pub(crate) fn apply_scene_action(scene_id: &str) -> UiAction {
    UiAction::new(
        "scenes/applyScene",
        serde_json::json!({ "sceneId": scene_id }),
    )
}

/// Build a `clearSceneCell` action.
pub(crate) fn clear_cell_action(scene_id: &str, row: usize) -> UiAction {
    UiAction::new(
        "scenes/clearSceneCell",
        serde_json::json!({ "sceneId": scene_id, "rowIndex": row }),
    )
}

/// Serialise a cell drop payload for diagnostics/tests.
pub(crate) fn copy_payload(source: &CellRef, target: &CellRef) -> Value {
    serde_json::json!({
        "fromSceneId": source.scene_id,
        "fromRowIndex": source.row,
        "toSceneId": target.scene_id,
        "toRowIndex": target.row,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_drag_emits_the_engine_payload() {
        let mut grid = SceneGridState::default();
        let source = CellRef::new("scene-a", 2);
        let target = CellRef::new("scene-b", 1);
        grid.begin_copy(source.clone());
        assert!(grid.is_copying());
        let action = grid
            .drop_copy(&target)
            .expect("a cross-cell drop emits an action");
        assert_eq!(action.action, "scenes/copySceneCellToScene");
        assert_eq!(action.payload["fromSceneId"], "scene-a");
        assert_eq!(action.payload["fromRowIndex"], 2);
        assert_eq!(action.payload["toSceneId"], "scene-b");
        assert_eq!(action.payload["toRowIndex"], 1);
        assert!(!grid.is_copying(), "the drag is consumed");
        assert_eq!(action.payload, copy_payload(&source, &target));
    }

    #[test]
    fn same_cell_drop_cancels_without_an_action() {
        let mut grid = SceneGridState::default();
        let cell = CellRef::new("scene-a", 0);
        grid.begin_copy(cell.clone());
        assert_eq!(grid.drop_copy(&cell), None);
        assert!(!grid.is_copying());
    }

    #[test]
    fn drop_without_a_source_is_a_no_op() {
        let mut grid = SceneGridState::default();
        assert_eq!(grid.drop_copy(&CellRef::new("scene-b", 0)), None);
    }

    #[test]
    fn cancel_clears_the_drag() {
        let mut grid = SceneGridState::default();
        grid.begin_copy(CellRef::new("scene-a", 0));
        grid.cancel_copy();
        assert!(!grid.is_copying());
    }

    #[test]
    fn cell_labels_summarise_the_module_list() {
        let empty = SceneCellView::default();
        assert_eq!(empty.label(), "Empty");
        assert!(empty.is_empty());

        let one = SceneCellView {
            row: 0,
            module_names: vec!["Bands".to_string()],
            disabled: false,
        };
        assert_eq!(one.label(), "Bands");

        let many = SceneCellView {
            row: 0,
            module_names: vec!["A".into(), "B".into(), "C".into()],
            disabled: false,
        };
        assert_eq!(many.label(), "A +2");
    }

    #[test]
    fn empty_cells_never_request_a_hover_preview() {
        // The WEBCAM row of a Scene without a WEBCAM Group is empty after the
        // named-row mapping; hovering it must not ask the engine for a
        // `preview/scene` row that cannot render.
        let webcam = SceneCellView {
            row: 0,
            module_names: Vec::new(),
            disabled: false,
        };
        assert!(!should_request_preview(&webcam, true));
        let background = SceneCellView {
            row: 1,
            module_names: vec!["Bands".to_string()],
            disabled: false,
        };
        assert!(should_request_preview(&background, true));
        assert!(!should_request_preview(&background, false));
    }

    #[test]
    fn scene_actions_match_the_store_payloads() {
        assert_eq!(
            apply_scene_action("scene-a").payload,
            serde_json::json!({"sceneId": "scene-a"})
        );
        assert_eq!(
            clear_cell_action("scene-a", 3).payload,
            serde_json::json!({"sceneId": "scene-a", "rowIndex": 3})
        );
    }
}
