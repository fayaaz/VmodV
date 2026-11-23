//! The Scenes grid and its cell widget.
//!
//! Port of `src/renderer/src/components/Scenes.vue` (2212 LOC) as an egui
//! probe. P0 implements the load-bearing interactions: live hover preview,
//! current/disabled/timing states, Ctrl+drag cell copy and the cell context
//! menu. P4 adds column headers, the row-name editor, playlist import/export,
//! scene reordering and the transition picker.

use egui::emath::GuiRounding as _;
use egui::{Align2, Color32, CornerRadius, FontId, Sense, Stroke, StrokeKind, Vec2};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::gallery_drag::GalleryDrag;
use crate::group_drag::GroupDrag;
use crate::layout::{LayoutState, WINDOW_PERFORMANCE_ROW_NAMES, WINDOW_SCENE_TIMING};
use crate::model::{
    EngineSnapshot, PERFORMANCE_ORDERS, PreviewRequest, SceneCellView, SceneView, TimingUnit,
    performance_order_label, scene_preview_key, timing_hint,
};
use crate::panels::row_monitor::{RowMonitorDisplay, row_monitor_display};
use crate::state::UiAction;
use crate::theme;

/// A cell address inside the Scenes grid.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    /// Whether `copy_source` came from a Ctrl+drag (as opposed to the menu's
    /// Copy item). Only drag-originated copies are cancelled on pointer
    /// release; a menu copy is a durable clipboard for Paste.
    copy_is_drag: bool,
    /// Hover-preview requests drained by the shell after drawing.
    pub preview_requests: Vec<PreviewRequest>,
    last_preview_request: Option<std::time::Instant>,
    hover_scene: Option<(String, usize)>,
    /// Cells whose cached preview keeps painting after the pointer leaves
    /// (the legacy grid "leaves a still"), keyed by the saved modules they
    /// showed. Inserted on hover and pruned when the saved cell disappears,
    /// empties or changes.
    still_cells: BTreeMap<CellRef, Vec<String>>,
    /// The in-flight `Clear cell` confirmation, if any.
    pub clear_cell_edit: Option<ClearCellEdit>,
    /// The in-flight `Overwrite cell` confirmation for a Group drop, if any.
    pub overwrite_cell_edit: Option<OverwriteCellEdit>,
    /// Inline Scene-name editor: `(scene_id, draft)`.
    pub renaming: Option<(String, String)>,
    /// One-shot focus request for the inline editor.
    pub rename_focus: bool,
    /// Per-Scene timing override editor.
    pub timing_edit: Option<TimingEdit>,
    /// The row-name list editor.
    pub row_names_edit: Option<Vec<String>>,
    /// Column drag: the Scene id under the pointer.
    pub scene_drag: Option<String>,
    /// Column drop target highlighted while dragging.
    pub scene_drop_target: Option<String>,
    /// Scroll the grid to the last column after `+ Column`.
    pub scroll_to_last_scene: bool,
    /// Row drag: the row index under the pointer.
    pub row_drag: Option<usize>,
    /// Row drop target highlighted while dragging.
    pub row_drop_target: Option<usize>,
    /// Inline row-name editor: `(row_index, draft)`.
    pub row_renaming: Option<(usize, String)>,
    /// One-shot focus request for the inline row editor.
    pub row_rename_focus: bool,
}

/// A pending `Clear cell` confirmation: the cell to clear plus the label the
/// dialog shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearCellEdit {
    pub scene_id: String,
    pub row: usize,
    pub scene_name: String,
    pub module_names: Vec<String>,
}

/// A pending `Overwrite cell` confirmation: the occupied cell a dragged Group
/// was dropped on, plus the labels the dialog shows. The payload is kept so the
/// confirm path reuses [`GroupDrag::drop_action`] instead of rebuilding the
/// action by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverwriteCellEdit {
    pub scene_id: String,
    pub row: usize,
    pub scene_name: String,
    pub drag: GroupDrag,
}

/// The small Scene-timing dialog's draft values.
#[derive(Clone, Debug, PartialEq)]
pub struct TimingEdit {
    pub scene_id: String,
    pub name: String,
    /// Unit of `min_value` / `max_value` (the global performance unit).
    pub timing_unit: TimingUnit,
    /// Override minimum, in `timing_unit`.
    pub min_value: f64,
    /// Override maximum, in `timing_unit`.
    pub max_value: f64,
}

/// The minimum a timing input accepts and the smallest gap between min/max,
/// per unit. Seconds keep the historical 1 s / 1 s pair; bars use a quarter
/// bar / half bar pair, which is the same shape at 120 BPM.
fn timing_input_limits(unit: TimingUnit) -> (f64, f64, f64) {
    match unit {
        TimingUnit::Seconds => (1.0, 1.0, 0.5),
        TimingUnit::Bars => (0.25, 0.5, 0.25),
    }
}

/// Clamp a min/max pair for `unit`, keeping max above min.
fn clamp_timing_pair(unit: TimingUnit, min: f64, max: f64) -> (f64, f64) {
    let (min_floor, gap, _) = timing_input_limits(unit);
    let min = min.max(min_floor);
    (min, max.max(min + gap))
}

/// The Scene-cell timing badge: a quarter-bar step must not read `8s` when
/// the project is in bars.
fn timing_badge(timing: (f64, f64), unit: TimingUnit) -> String {
    let (_, max) = timing;
    match unit {
        TimingUnit::Seconds => format!("{max:.0}s"),
        TimingUnit::Bars => format!("{max:.2}b"),
    }
}

impl SceneGridState {
    /// Remember `cell` as the copy clipboard (e.g. from the context menu).
    /// This is durable: it survives pointer releases until cancelled.
    pub fn begin_copy(&mut self, cell: CellRef) {
        self.copy_source = Some(cell);
        self.copy_is_drag = false;
    }

    /// Start a Ctrl+drag copy from `cell`. Drag-originated copies are cancelled
    /// when the pointer is released outside a target cell.
    pub fn begin_drag_copy(&mut self, cell: CellRef) {
        self.copy_source = Some(cell);
        self.copy_is_drag = true;
    }

    pub fn cancel_copy(&mut self) {
        self.copy_source = None;
        self.copy_is_drag = false;
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

    /// Begin inline renaming of a row label.
    pub fn begin_row_rename(&mut self, row: usize, draft: String) {
        self.row_renaming = Some((row, draft));
        self.row_rename_focus = true;
    }

    /// Begin dragging a Scene column header.
    pub fn begin_scene_drag(&mut self, scene_id: impl Into<String>) {
        self.scene_drag = Some(scene_id.into());
        self.scene_drop_target = None;
    }

    pub fn cancel_scene_drag(&mut self) {
        self.scene_drag = None;
        self.scene_drop_target = None;
    }

    /// Finish a column drag at `to_index`. The target index comes from the
    /// snapshot, so the engine receives the same payload as `Move left/right`.
    pub fn drop_scene(&mut self, to_index: usize) -> Option<UiAction> {
        let scene_id = self.scene_drag.take()?;
        let target = self.scene_drop_target.take()?;
        if target == scene_id {
            return None;
        }
        Some(UiAction::new(
            "scenes/moveScene",
            serde_json::json!({ "sceneId": scene_id, "toIndex": to_index }),
        ))
    }

    /// Begin dragging a row label.
    pub fn begin_row_drag(&mut self, row: usize) {
        self.row_drag = Some(row);
        self.row_drop_target = None;
    }

    pub fn cancel_row_drag(&mut self) {
        self.row_drag = None;
        self.row_drop_target = None;
    }

    /// Finish a row drag over `target`: splice the row name out of `names` and
    /// back in at the target, matching `Scenes.vue`'s `onRowDrop`.
    pub fn drop_row(&mut self, names: &mut Vec<String>, target: usize) -> Option<UiAction> {
        let source = self.row_drag.take()?;
        self.row_drop_target = None;
        if source == target || source >= names.len() {
            return None;
        }
        let target = target.min(names.len().saturating_sub(1));
        let name = names.remove(source);
        names.insert(target, name);
        Some(set_row_names_action(names.clone()))
    }

    /// Begin editing a Scene's timing override.
    pub fn begin_timing(&mut self, scene: &SceneView, performance: &crate::PerformanceView) {
        let (min_value, max_value) = scene
            .timing
            .unwrap_or((performance.min_value, performance.max_value));
        self.timing_edit = Some(TimingEdit {
            scene_id: scene.id.clone(),
            name: scene.name.clone(),
            timing_unit: performance.timing_unit,
            min_value,
            max_value,
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

    /// Whether `cell` paints a cached preview this frame: the hovered cell is
    /// live, and every cell hovered before keeps its last rendered frame as a
    /// still (the legacy grid's `scenes__gridPreviewCanvas--visible`).
    pub fn paints_preview(&self, cell: &CellRef, hovered: bool) -> bool {
        hovered || self.still_cells.contains_key(cell)
    }

    /// Pick the texture a Scene-grid cell paints this frame.
    ///
    /// A hovered cell may fall back to the live composited main output until
    /// its own isolated preview arrives, so the hover feedback is immediate.
    /// A non-hovered "still" cell, however, must paint ONLY its cached preview:
    /// stills are kept for every cell ever hovered, so falling back to the live
    /// output here would leak the main output into those cells whenever their
    /// cached preview was evicted from the shared cache. An evicted still
    /// therefore paints nothing. Cells that never paint a preview get nothing.
    pub fn preview_texture_for(
        &self,
        cell: &CellRef,
        hovered: bool,
        snapshot: &EngineSnapshot,
        live_preview: Option<egui::TextureId>,
    ) -> Option<egui::TextureId> {
        let cached = snapshot
            .scene_previews
            .get(&scene_preview_key(&cell.scene_id, cell.row))
            .map(|p| p.id);
        if hovered {
            cached.or(live_preview)
        } else if self.still_cells.contains_key(cell) {
            cached
        } else {
            None
        }
    }

    /// Remember `cell` as a still so its cached preview keeps painting once
    /// the pointer moves on. `module_names` is the saved content the preview
    /// shows; a later edit drops the still rather than painting stale pixels.
    pub fn keep_preview(&mut self, cell: &CellRef, module_names: &[String]) {
        self.still_cells.insert(cell.clone(), module_names.to_vec());
    }

    /// Drop stills whose Scene or saved cell disappeared, became empty or
    /// changed modules, so a removed/cleared/edited cell cannot keep painting
    /// a stale frame.
    pub fn prune_preview_stills(&mut self, snapshot: &EngineSnapshot) {
        self.still_cells.retain(|cell, module_names| {
            snapshot
                .scenes
                .iter()
                .find(|scene| scene.id == cell.scene_id)
                .and_then(|scene| scene.cell(cell.row))
                .is_some_and(|saved| !saved.is_empty() && saved.module_names == *module_names)
        });
    }

    /// Finish a copy drag over `target`. Returns the engine action, or `None`
    /// for a same-cell drop (which cancels the drag without an action).
    pub fn drop_copy(&mut self, target: &CellRef) -> Option<UiAction> {
        // Clear the drag flag unconditionally so no stale state survives,
        // even when there was no source to take.
        self.copy_is_drag = false;
        let source = self.copy_source.take()?;
        if source == *target {
            return None;
        }
        Some(UiAction::new(
            "scenes/copySceneCellToScene",
            copy_payload(&source, target),
        ))
    }

    /// Queue a copy of the remembered clipboard cell into `target` without
    /// consuming the clipboard, so Paste can be used repeatedly. Returns
    /// `None` for a missing source or a same-cell paste.
    pub fn paste_into(&self, target: &CellRef) -> Option<UiAction> {
        let source = self.copy_source.as_ref()?;
        if source == target {
            return None;
        }
        Some(UiAction::new(
            "scenes/copySceneCellToScene",
            copy_payload(source, target),
        ))
    }
}

/// Width/height of one Scene cell.
const CELL_SIZE: Vec2 = Vec2::new(154.0, 76.0);
/// Width of the row-label gutter and the column-header height.
const ROW_LABEL_WIDTH: f32 = 96.0;
const HEADER_HEIGHT: f32 = 20.0;
/// Fixed gap between consecutive Scene rows. Row layout must never depend on
/// cell content: the per-cell dot, timing chip and hover preview are painted
/// or placed without advancing the column cursor.
const ROW_GAP: f32 = 5.0;
/// Fixed gap between the header row and the first Scene row.
const HEADER_GAP: f32 = 3.0;
/// Fixed distance between the tops of consecutive rows, shared by the layer
/// gutter and every Scene column.
const ROW_PITCH: f32 = CELL_SIZE.y + ROW_GAP;
/// Fixed size of the row gutter's layer-monitor thumbnail.
const ROW_MONITOR_THUMB: Vec2 = Vec2::new(48.0, 27.0);

/// Height of the scrolled grid content (header plus every fixed-pitch row).
fn grid_content_height(row_count: usize) -> f32 {
    HEADER_HEIGHT + HEADER_GAP + row_count as f32 * ROW_PITCH
}

/// Draw the row's layer monitor in the gutter's bottom-right corner: a small
/// aspect-correct letterboxed snapshot of the row's live Group (the same
/// [`crate::shell::fit_preview_rect`] letterboxing the Gallery/output use),
/// dimmed while performance mode is off, or a subtle "no layer" placeholder
/// when the row has no active Group.
///
/// The pixels come from the shell's slow-cadence GPU snapshot of the Group's
/// rendered texture (see `modv-app`'s row-monitor plumbing); a live Group
/// whose first refresh has not landed yet keeps an empty plate instead of
/// the misleading placeholder.
fn draw_row_monitor(ui: &mut egui::Ui, snapshot: &EngineSnapshot, row: usize, cell: egui::Rect) {
    let monitor = snapshot.row_monitors.get(&row);
    let display = row_monitor_display(monitor.is_some(), snapshot.performance.enabled);
    let thumb = egui::Rect::from_min_size(
        cell.right_bottom() + egui::vec2(-ROW_MONITOR_THUMB.x - 4.0, -ROW_MONITOR_THUMB.y - 4.0),
        ROW_MONITOR_THUMB,
    );
    let painter = ui.painter();
    painter.rect_filled(thumb, CornerRadius::same(0), theme::INPUT);
    match (display, monitor.and_then(|view| view.texture)) {
        (RowMonitorDisplay::Monitor { alpha }, Some(texture)) => {
            let fitted =
                crate::shell::fit_preview_rect(thumb.size(), [texture.width, texture.height])
                    .translate(thumb.min.to_vec2());
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            // Every egui colour is premultiplied, so a tint of
            // white * alpha scales the premultiplied monitor content exactly
            // like a canvas `globalAlpha` fade.
            let tint = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            painter.image(texture.id, fitted, uv, Color32::from_white_alpha(tint));
        }
        (RowMonitorDisplay::Monitor { .. }, None) => {}
        (RowMonitorDisplay::NoLayer, _) => {
            painter.text(
                thumb.center(),
                Align2::CENTER_CENTER,
                "no layer",
                FontId::proportional(9.0),
                theme::MUTED,
            );
        }
    }
    painter.rect_stroke(
        thumb,
        CornerRadius::same(0),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
}

/// The grid height: the tallest Scene, the named rows and the live row labels.
pub(crate) fn grid_row_count(snapshot: &EngineSnapshot) -> usize {
    snapshot
        .scenes
        .iter()
        .map(|scene| scene.cells.len())
        .max()
        .unwrap_or(1)
        .max(snapshot.scene_rows.len())
        .max(1)
}

/// Row names padded/truncated to the grid height, so every row control sends a
/// complete `rowNames` list to `scenes/setRowNames`.
fn grid_row_names(snapshot: &EngineSnapshot, row_count: usize) -> Vec<String> {
    let mut names = snapshot.scene_rows.clone();
    names.resize(row_count, String::new());
    names
}

fn row_display_name(snapshot: &EngineSnapshot, row: usize) -> String {
    snapshot
        .scene_rows
        .get(row)
        .map(|name| {
            if name.trim().is_empty() {
                format!("Row {}", row + 1)
            } else {
                name.clone()
            }
        })
        .unwrap_or_else(|| format!("Row {}", row + 1))
}

/// The name of the Scene currently live in `row`, if the snapshot names one.
fn live_scene_name(snapshot: &EngineSnapshot, row: usize) -> Option<&str> {
    let scene_id = snapshot.active_scene_rows.get(&row)?;
    snapshot
        .scenes
        .iter()
        .find(|scene| &scene.id == scene_id)
        .map(|scene| scene.name.as_str())
}

/// `Scenes.vue`'s `addRow`: append a blank name and start renaming it.
fn add_row_action(names: &[String]) -> UiAction {
    let mut next = names.to_vec();
    next.push(String::new());
    set_row_names_action(next)
}

fn remove_row_from(names: &[String], row: usize) -> Option<UiAction> {
    if row >= names.len() {
        return None;
    }
    let mut next = names.to_vec();
    next.remove(row);
    Some(set_row_names_action(next))
}

fn set_row_names_action(names: Vec<String>) -> UiAction {
    UiAction::new(
        "scenes/setRowNames",
        serde_json::json!({ "rowNames": names }),
    )
}

/// Build the per-cell enabled toggle payload.
pub(crate) fn set_cell_enabled_action(scene_id: &str, row: usize, enabled: bool) -> UiAction {
    UiAction::new(
        "scenes/setCellEnabled",
        serde_json::json!({ "sceneId": scene_id, "rowIndex": row, "enabled": enabled }),
    )
}

/// Draw the whole Scene grid.
pub fn draw_scene_grid(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    live_preview: Option<egui::TextureId>,
    actions: &mut Vec<UiAction>,
    layout: &mut LayoutState,
) {
    if snapshot.scenes.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label(egui::RichText::new("No Scenes yet").size(16.0));
            ui.label(egui::RichText::new("Add a Scene or load the defaults.").color(theme::MUTED));
            if ui.button("Add empty Scene").clicked() {
                actions.push(UiAction::new("scenes/addEmptyScene", serde_json::json!({})));
            }
        });
        return;
    }

    // A still only lives while its saved cell exists and holds a Group.
    grid.prune_preview_stills(snapshot);

    let row_count = grid_row_count(snapshot);
    grid_controls(ui, snapshot, grid, row_count, actions);
    ui.separator();

    let row_names = grid_row_names(snapshot, row_count);
    // One scroll offset for both axes. The Scene columns live inside the
    // scroll area; the layer gutter is painted on top of its left edge with the
    // same vertical offset (see `draw_layer_gutter`), so it stays pinned while
    // the columns scroll sideways and still tracks the rows vertically.
    let scrolled = egui::ScrollArea::both()
        .id_salt("scenes-grid")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // Reserve the pinned gutter's width in the scrolled content so
                // the first Scene column is clear at the resting offset.
                ui.add_space(ROW_LABEL_WIDTH + ui.spacing().item_spacing.x);
                let last = snapshot.scenes.len().saturating_sub(1);
                for (index, scene) in snapshot.scenes.iter().enumerate() {
                    draw_scene_column(
                        ui,
                        scene,
                        snapshot,
                        grid,
                        live_preview,
                        actions,
                        index == last,
                        row_count,
                    );
                }
            });
        });
    draw_layer_gutter(ui, &scrolled, snapshot, grid, &row_names, actions);

    finish_drags(ui, snapshot, grid, &row_names, actions);

    // A copy drag released outside every cell is cancelled without an action.
    // A menu-initiated copy is a durable clipboard and must survive releases.
    if grid.copy_source.is_some()
        && grid.copy_is_drag
        && ui.input(|input| input.pointer.any_released())
    {
        grid.cancel_copy();
    }

    scene_dialogs(ui, snapshot, grid, actions, layout);
}

/// Draw one Scene column: its header plus one cell per grid row.
#[allow(clippy::too_many_arguments)]
fn draw_scene_column(
    ui: &mut egui::Ui,
    scene: &SceneView,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    live_preview: Option<egui::TextureId>,
    actions: &mut Vec<UiAction>,
    is_last: bool,
    row_count: usize,
) {
    ui.vertical(|ui| {
        // The header and rows are placed by hand at a fixed pitch. Cell
        // decoration (the enable dot, timing chip, hover preview) must never
        // advance this cursor, or a column with more populated cells would
        // drift away from the others.
        ui.spacing_mut().item_spacing.y = 0.0;
        let applied = snapshot.performance.current_scene_id.as_deref() == Some(scene.id.as_str());
        // In performance mode every row advances on its own, so an applied
        // Scene header must not read like a whole live column: it keeps only
        // a thin dim accent and an `APPLIED` tag. The accent column highlight
        // belongs to performance-off mode, where the applied Scene really is
        // live in every row.
        let per_row = snapshot.performance.enabled;
        let renaming = grid
            .renaming
            .as_ref()
            .filter(|(id, _)| id == &scene.id)
            .map(|(_, draft)| draft.clone());
        if let Some(mut draft) = renaming {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(CELL_SIZE.x, HEADER_HEIGHT), Sense::hover());
            let response = ui.place(
                rect,
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
            let (rect, header) = ui.allocate_exact_size(
                Vec2::new(CELL_SIZE.x, HEADER_HEIGHT),
                Sense::click_and_drag(),
            );
            let dragging = grid.scene_drag.as_deref() == Some(scene.id.as_str());
            let drop_target = grid.scene_drop_target.as_deref() == Some(scene.id.as_str())
                && grid.scene_drag.as_deref() != Some(scene.id.as_str());
            let hovered = header.hovered();

            let fill = if drop_target {
                theme::ACCENT_DIM
            } else if dragging {
                theme::PANEL_ALT
            } else if hovered {
                theme::BORDER
            } else {
                theme::PANEL
            };
            let painter = ui.painter();
            painter.rect_filled(rect, CornerRadius::same(0), fill);
            let stroke = if drop_target {
                Stroke::new(1.5, theme::ACCENT)
            } else if dragging {
                Stroke::new(1.5, theme::BORDER)
            } else if applied && per_row {
                Stroke::new(1.0, theme::ACCENT_DIM)
            } else if applied {
                Stroke::new(1.5, theme::ACCENT)
            } else {
                Stroke::new(1.0, theme::BORDER)
            };
            painter.rect_stroke(rect, CornerRadius::same(0), stroke, StrokeKind::Inside);
            let dot = if scene.enabled {
                theme::OK
            } else {
                theme::DANGER
            };
            painter.circle_filled(rect.left_center() + egui::vec2(7.0, 0.0), 3.0, dot);
            // Leave room for the `APPLIED` tag so a long Scene name cannot
            // run underneath it.
            let clip_right = if applied && per_row { 52.0 } else { 16.0 };
            let clipped = painter.with_clip_rect(egui::Rect::from_min_max(
                egui::pos2(rect.left() + 16.0, rect.top()),
                egui::pos2(rect.right() - clip_right, rect.bottom()),
            ));
            clipped.text(
                rect.left_center() + egui::vec2(14.0, 0.0),
                Align2::LEFT_CENTER,
                &scene.name,
                FontId::proportional(13.0),
                if scene.enabled {
                    if applied && !per_row {
                        theme::ACCENT
                    } else {
                        theme::FOREGROUND
                    }
                } else {
                    theme::MUTED
                },
            );
            if applied && per_row {
                painter.text(
                    rect.right_center() + egui::vec2(-6.0, 0.0),
                    Align2::RIGHT_CENTER,
                    "APPLIED",
                    FontId::monospace(9.5),
                    theme::MUTED,
                );
            }

            if header.clicked() {
                actions.push(apply_scene_action(&scene.id));
            }
            if header.double_clicked() {
                grid.begin_rename(scene);
            }
            if header.drag_started() {
                grid.begin_scene_drag(scene.id.clone());
            }
            if grid.scene_drag.is_some() && !dragging && header.contains_pointer() {
                grid.scene_drop_target = Some(scene.id.clone());
            }
            let header = header.on_hover_text(if per_row {
                "Apply this Scene. In performance mode each row advances independently; APPLIED does not mean the whole column is live."
            } else {
                "Apply this Scene. Drag to reorder columns; double-click to rename."
            });
            let header_label = if applied && per_row {
                format!("{} (applied Scene)", scene.name)
            } else {
                scene.name.clone()
            };
            header.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, header_label.clone())
            });
            header.context_menu(|ui| scene_column_menu(ui, scene, snapshot, grid, actions));
            if is_last && grid.scroll_to_last_scene {
                header.scroll_to_me(Some(egui::Align::RIGHT));
                grid.scroll_to_last_scene = false;
            }
        }

        ui.add_space(HEADER_GAP);
        for row in 0..row_count {
            let cell = scene.cell(row).cloned().unwrap_or_default();
            let _ = draw_scene_cell(ui, scene, row, &cell, snapshot, grid, live_preview, actions);
            ui.add_space(ROW_GAP);
        }
    });
    ui.add_space(6.0);
}

/// Paint the layer gutter pinned to the left edge of the scroll viewport.
///
/// The gutter is a child UI positioned at the scrolled content's vertical
/// origin but at the viewport's fixed x: it shares the ScrollArea's one
/// vertical offset (so previews and names stay attached to their rows) and
/// never moves sideways, while the Scene columns scroll underneath it.
fn draw_layer_gutter(
    ui: &mut egui::Ui,
    scroll: &egui::scroll_area::ScrollAreaOutput<()>,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    names: &[String],
    actions: &mut Vec<UiAction>,
) {
    let offset = scroll.state.clamped_offset();
    let viewport = scroll.inner_rect.intersect(ui.clip_rect());
    if viewport.width() <= 0.0 || viewport.height() <= 0.0 {
        return;
    }
    // Match the pixel rounding the ScrollArea applies to its content origin,
    // so the gutter rows stay glued to the Scene rows while smooth-scrolling.
    let content_top = (scroll.inner_rect.top() - offset.y)
        .round_to_pixels(ui.pixels_per_point())
        .round_ui();
    let height = scroll.content_size.y.max(grid_content_height(names.len()));
    let mut gutter = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                egui::pos2(viewport.left(), content_top),
                Vec2::new(ROW_LABEL_WIDTH, height),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    gutter.set_clip_rect(viewport);
    // Opaque backing: the gutter sits on top of the scrolled Scene content, so
    // the header, row gaps and any uncovered sliver must not let cells or
    // headers show through while the columns slide underneath.
    gutter.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(viewport.left(), content_top),
            Vec2::new(ROW_LABEL_WIDTH, height),
        ),
        CornerRadius::same(0),
        theme::PANEL,
    );
    row_label_column(&mut gutter, snapshot, grid, names, actions);
}

/// The fixed row-label gutter: names, inline rename, drag reorder and delete.
fn row_label_column(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    names: &[String],
    actions: &mut Vec<UiAction>,
) {
    // Same fixed pitch as the Scene columns; the header uses the same height
    // and gap as a Scene header so every row line matches.
    ui.spacing_mut().item_spacing.y = 0.0;
    let (header, header_response) =
        ui.allocate_exact_size(Vec2::new(ROW_LABEL_WIDTH, HEADER_HEIGHT), Sense::hover());
    header_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Layers"));
    ui.painter().text(
        header.left_center() + egui::vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        "Layers",
        FontId::proportional(12.0),
        theme::MUTED,
    );
    ui.add_space(HEADER_GAP);
    for row in 0..names.len() {
        row_label_cell(ui, snapshot, grid, names, row, actions);
        ui.add_space(ROW_GAP);
    }
}

fn row_label_cell(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    names: &[String],
    row: usize,
    actions: &mut Vec<UiAction>,
) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ROW_LABEL_WIDTH, CELL_SIZE.y),
        Sense::click_and_drag(),
    );
    let dragging = grid.row_drag == Some(row);
    let drop_target = grid.row_drop_target == Some(row) && grid.row_drag != Some(row);
    let hovered = response.hovered();
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(0),
        if drop_target {
            theme::ACCENT_DIM
        } else if dragging || hovered {
            theme::PANEL_ALT
        } else {
            theme::PANEL
        },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(
            if drop_target || dragging { 1.5 } else { 1.0 },
            if drop_target {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );

    let renaming = grid
        .row_renaming
        .as_ref()
        .filter(|(index, _)| *index == row)
        .map(|(_, draft)| draft.clone());
    if let Some(mut draft) = renaming {
        painter.text(
            rect.left_top() + egui::vec2(6.0, 4.0),
            Align2::LEFT_TOP,
            format!("Row {}", row + 1),
            FontId::monospace(11.0),
            theme::MUTED,
        );
        let edit_rect = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(4.0, 22.0),
            Vec2::new(rect.width() - 8.0, 22.0),
        );
        let edit = ui.place(
            edit_rect,
            egui::TextEdit::singleline(&mut draft)
                .font(egui::TextStyle::Small)
                .hint_text("Row name"),
        );
        if grid.row_rename_focus {
            edit.request_focus();
            grid.row_rename_focus = false;
        }
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        if escape {
            grid.row_renaming = None;
        } else if enter || edit.lost_focus() {
            grid.row_renaming = None;
            let mut next = names.to_vec();
            if let Some(slot) = next.get_mut(row) {
                *slot = draft.trim().to_string();
            }
            if next != names {
                actions.push(set_row_names_action(next));
            }
        } else {
            grid.row_renaming = Some((row, draft));
        }
        return;
    }

    painter.text(
        rect.left_top() + egui::vec2(6.0, 4.0),
        Align2::LEFT_TOP,
        format!("Row {}", row + 1),
        FontId::monospace(11.0),
        theme::MUTED,
    );
    let label = snapshot
        .scene_rows
        .get(row)
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| row_display_name(snapshot, row));
    let clipped = painter.with_clip_rect(rect.shrink2(egui::vec2(8.0, 2.0)));
    clipped.text(
        rect.left_top() + egui::vec2(6.0, 24.0),
        Align2::LEFT_TOP,
        &label,
        FontId::proportional(13.0),
        theme::FOREGROUND,
    );

    // The row's live Scene name sits under the row name, in the strip left of
    // the monitor thumbnail. The full name is also in the accessible label so
    // kittest can assert it without reading pixels.
    if let Some(scene_name) = live_scene_name(snapshot, row) {
        let scene_rect = egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(6.0, 40.0),
            egui::pos2(
                rect.right() - ROW_MONITOR_THUMB.x - 6.0,
                rect.bottom() - 2.0,
            ),
        );
        painter.with_clip_rect(scene_rect).text(
            scene_rect.left_top(),
            Align2::LEFT_TOP,
            scene_name,
            FontId::proportional(10.0),
            if snapshot.performance.enabled {
                theme::ACCENT
            } else {
                theme::MUTED
            },
        );
    }

    draw_row_monitor(ui, snapshot, row, rect);

    let accessible_label = match live_scene_name(snapshot, row) {
        Some(scene_name) => format!("Layer row {}: {label} (live {scene_name})", row + 1),
        None => format!("Layer row {}: {label}", row + 1),
    };
    let row_enabled = ui.is_enabled();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::Role::Button, row_enabled, accessible_label.clone())
    });

    if response.double_clicked() {
        grid.begin_row_rename(row, names.get(row).cloned().unwrap_or_default());
    }
    if response.drag_started() {
        grid.begin_row_drag(row);
    }
    if grid.row_drag.is_some() && grid.row_drag != Some(row) && response.contains_pointer() {
        grid.row_drop_target = Some(row);
    }
    let response = response.on_hover_text("Double-click to rename. Drag to reorder rows.");
    response.context_menu(|ui| row_label_menu(ui, names, row, grid, actions));
}

/// Resolve a column/row drag once the pointer is released.
fn finish_drags(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    names: &[String],
    actions: &mut Vec<UiAction>,
) {
    if !ui.input(|input| input.pointer.any_released()) {
        return;
    }

    if grid.scene_drag.is_some() {
        let target = grid
            .scene_drop_target
            .as_deref()
            .and_then(|target| snapshot.scenes.iter().position(|scene| scene.id == target));
        match target {
            Some(index) => {
                if let Some(action) = grid.drop_scene(index) {
                    actions.push(action);
                }
            }
            None => grid.cancel_scene_drag(),
        }
        grid.cancel_scene_drag();
    }

    if grid.row_drag.is_some() {
        let mut next = names.to_vec();
        match grid.row_drop_target {
            Some(target) => {
                if let Some(action) = grid.drop_row(&mut next, target) {
                    actions.push(action);
                }
            }
            None => grid.cancel_row_drag(),
        }
        grid.cancel_row_drag();
    }
}

fn row_label_menu(
    ui: &mut egui::Ui,
    names: &[String],
    row: usize,
    grid: &mut SceneGridState,
    actions: &mut Vec<UiAction>,
) {
    if ui.button("Rename row").clicked() {
        grid.begin_row_rename(row, names.get(row).cloned().unwrap_or_default());
        ui.close();
    }
    if row > 0 {
        let mut next = names.to_vec();
        if row < next.len() {
            let name = next.remove(row);
            next.insert(row - 1, name);
            if ui.button("Move row up").clicked() {
                actions.push(set_row_names_action(next));
                ui.close();
            }
        }
    }
    if row + 1 < names.len() {
        let mut next = names.to_vec();
        if row < next.len() {
            let name = next.remove(row);
            next.insert(row + 1, name);
            if ui.button("Move row down").clicked() {
                actions.push(set_row_names_action(next));
                ui.close();
            }
        }
    }
    ui.separator();
    if ui
        .add_enabled(
            row < names.len(),
            egui::Button::new(egui::RichText::new("Delete row").color(theme::DANGER)),
        )
        .clicked()
    {
        if let Some(action) = remove_row_from(names, row) {
            actions.push(action);
        }
        ui.close();
    }
}

/// The start of the performance controls: the run toggle, Scene order and the
/// timing unit. The shell places this and [`draw_timing_controls`] on the one
/// header line.
pub fn draw_performance_bar(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    actions: &mut Vec<UiAction>,
) {
    performance_bar(ui, snapshot, actions);
}

/// Performance-mode run/order/unit controls.
fn performance_bar(ui: &mut egui::Ui, snapshot: &EngineSnapshot, actions: &mut Vec<UiAction>) {
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
    // `sceneOrder` (`Scenes.vue`): the engine reads it on every pick, so
    // the select stays enabled while performance mode runs and a live
    // change applies to the next Scene.
    let order = snapshot.performance.scene_order();
    let mut selected = order;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Scene order")
                .size(12.0)
                .color(theme::MUTED),
        );
        egui::ComboBox::from_id_salt("performance-scene-order")
            .selected_text(performance_order_label(order))
            .width(90.0)
            .show_ui(ui, |ui| {
                for (value, label) in PERFORMANCE_ORDERS {
                    if ui.selectable_label(order == value, label).clicked() {
                        selected = value;
                    }
                }
            });
    });
    if selected != order {
        actions.push(UiAction::new(
            "scenes/updatePerformance",
            serde_json::json!({ "order": selected }),
        ));
    }
    ui.separator();
    // `timingUnit` is VmodV-only: switching it converts the global pair
    // and every Scene override with the current BPM, so the same
    // durations stay in place.
    let unit = snapshot.performance.timing_unit;
    let mut selected_unit = unit;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Timing").size(12.0).color(theme::MUTED));
        egui::ComboBox::from_id_salt("performance-timing-unit")
            .selected_text(unit.label())
            .width(80.0)
            .show_ui(ui, |ui| {
                for candidate in [TimingUnit::Seconds, TimingUnit::Bars] {
                    if ui
                        .selectable_label(unit == candidate, candidate.label())
                        .clicked()
                    {
                        selected_unit = candidate;
                    }
                }
            });
    });
    let unit_changed = selected_unit != unit;
    if unit_changed {
        actions.push(UiAction::new(
            "scenes/setTimingUnit",
            serde_json::json!({ "unit": selected_unit.as_str() }),
        ));
    }
}

/// The rest of the performance controls: the min/max/fade values and the
/// resolved-duration hint. These are grouped label/value pairs, so the shell
/// draws them on the same header line as [`draw_performance_bar`].
pub fn draw_timing_controls(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    actions: &mut Vec<UiAction>,
) {
    let unit = snapshot.performance.timing_unit;
    let (min_floor, _, speed) = timing_input_limits(unit);
    // Start from the displayed floor rather than the raw stored value:
    // a `DragValue` whose value is outside its range clamps on the first
    // frame and reports `changed`, which would queue an update every
    // frame for a fresh (zeroed) performance config.
    let mut min = snapshot.performance.min_value.max(min_floor);
    let min_changed = ui
        .horizontal(|ui| {
            ui.label(egui::RichText::new("min").size(12.0).color(theme::MUTED));
            ui.add(
                egui::DragValue::new(&mut min)
                    .range(min_floor..=3600.0)
                    .speed(speed)
                    .suffix(unit.suffix()),
            )
            .changed()
        })
        .inner;
    let mut max = snapshot.performance.max_value.max(min_floor * 2.0);
    let max_changed = ui
        .horizontal(|ui| {
            ui.label(egui::RichText::new("max").size(12.0).color(theme::MUTED));
            ui.add(
                egui::DragValue::new(&mut max)
                    .range(min_floor * 2.0..=7200.0)
                    .speed(speed)
                    .suffix(unit.suffix()),
            )
            .changed()
        })
        .inner;
    let mut fade = snapshot.performance.transition_seconds.max(0.1);
    let fade_changed = ui
        .horizontal(|ui| {
            ui.label(egui::RichText::new("fade").size(12.0).color(theme::MUTED));
            ui.add(
                egui::DragValue::new(&mut fade)
                    .range(0.1..=30.0)
                    .speed(0.05)
                    .suffix(" s"),
            )
            .changed()
        })
        .inner;
    if min_changed || max_changed || fade_changed {
        let (min, max) = clamp_timing_pair(
            unit,
            if min_changed {
                min
            } else {
                snapshot.performance.min_value
            },
            if max_changed {
                max
            } else {
                snapshot.performance.max_value
            },
        );
        actions.push(UiAction::new(
            "scenes/updatePerformance",
            serde_json::json!({
                "config": {
                    "minSeconds": min,
                    "maxSeconds": max,
                    "transitionSeconds": fade,
                }
            }),
        ));
    }
    ui.label(
        egui::RichText::new(timing_hint(
            snapshot.performance.min_value,
            snapshot.performance.max_value,
            unit,
            snapshot.audio.bpm,
        ))
        .size(11.0)
        .color(theme::MUTED),
    );
}

/// Row and column controls, drawn directly above the Scene cells.
fn grid_controls(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    row_count: usize,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("SCENES")
                .strong()
                .size(14.0)
                .color(theme::FOREGROUND),
        );
        ui.label(
            egui::RichText::new(format!("{} saved", snapshot.scenes.len()))
                .size(12.0)
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
                .size(12.0)
                .color(theme::MUTED),
            );
        }
        ui.separator();
        if ui.button("Row names").clicked() {
            grid.row_names_edit = Some(grid_row_names(snapshot, row_count));
        }
        if ui.button("+ Column").clicked() {
            // The engine mints the `Scene N` default; the legacy `+ Column`
            // sends no name either (`scenes/addEmptyScene` with no args).
            actions.push(UiAction::new("scenes/addEmptyScene", serde_json::json!({})));
            grid.scroll_to_last_scene = true;
        }
        if ui.button("+ Row").clicked() {
            let names = grid_row_names(snapshot, row_count);
            actions.push(add_row_action(&names));
            grid.begin_row_rename(names.len(), String::new());
        }
    });
}

/// Modal editors for Scene timing override and performance row names.
fn scene_dialogs(
    ui: &mut egui::Ui,
    snapshot: &EngineSnapshot,
    grid: &mut SceneGridState,
    actions: &mut Vec<UiAction>,
    layout: &mut LayoutState,
) {
    if let Some(mut edit) = grid.timing_edit.take() {
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let mut reset = false;
        let window = layout.apply_window(
            egui::Window::new(format!("Scene timing: {}", edit.name))
                .id(egui::Id::unique("modv-window:scene_timing"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .min_width(320.0)
                .min_height(180.0)
                .default_size([420.0, 240.0]),
            WINDOW_SCENE_TIMING,
        );
        let unit = edit.timing_unit;
        let (min_floor, _, speed) = timing_input_limits(unit);
        let bpm = snapshot.audio.bpm;
        let response = window.show(ui.ctx(), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Per-Scene override in {}; reset returns to the global timing.",
                    unit.label().to_ascii_lowercase()
                ))
                .size(12.0)
                .color(theme::MUTED),
            );
            ui.horizontal(|ui| {
                ui.label(format!("Min ({})", unit.label()));
                ui.add(
                    egui::DragValue::new(&mut edit.min_value)
                        .range(min_floor..=3600.0)
                        .speed(speed)
                        .suffix(unit.suffix()),
                );
                ui.label("Max");
                ui.add(
                    egui::DragValue::new(&mut edit.max_value)
                        .range(min_floor * 2.0..=7200.0)
                        .speed(speed)
                        .suffix(unit.suffix()),
                );
            });
            ui.label(
                egui::RichText::new(timing_hint(edit.min_value, edit.max_value, unit, bpm))
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.horizontal(|ui| {
                if ui.button("Save override").clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if ui.button("Reset override").clicked() {
                    reset = true;
                }
            });
        });
        if let Some(response) = response {
            layout.record_window_rect(WINDOW_SCENE_TIMING, response.response.rect);
        }
        if reset {
            actions.push(UiAction::new(
                "scenes/resetSceneTiming",
                serde_json::json!({ "sceneId": edit.scene_id }),
            ));
        } else if save {
            let (min, max) = clamp_timing_pair(unit, edit.min_value, edit.max_value);
            actions.push(UiAction::new(
                "scenes/updateSceneTiming",
                serde_json::json!({
                    "sceneId": edit.scene_id,
                    "minSeconds": min,
                    "maxSeconds": max,
                }),
            ));
        } else if !cancel && open {
            grid.timing_edit = Some(edit);
        }
    }

    if let Some(mut names) = grid.row_names_edit.take() {
        let mut open = true;
        let mut save = false;
        let window = layout.apply_window(
            egui::Window::new("Performance row names")
                .id(egui::Id::unique("modv-window:performance_row_names"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .min_width(320.0)
                .min_height(220.0)
                .default_size([460.0, 360.0]),
            WINDOW_PERFORMANCE_ROW_NAMES,
        );
        let response = window.show(ui.ctx(), |ui| {
            ui.label(
                egui::RichText::new(
                    "A named row matches its Group by name; empty names use position.",
                )
                .size(12.0)
                .color(theme::MUTED),
            );
            let mut remove: Option<usize> = None;
            let mut swap: Option<(usize, usize)> = None;
            for index in 0..names.len() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Row {index}"))
                            .monospace()
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                    ui.text_edit_singleline(&mut names[index]);
                    if ui
                        .add_enabled(index > 0, egui::Button::new("↑").small())
                        .on_hover_text("Move row up")
                        .clicked()
                    {
                        swap = Some((index, index - 1));
                    }
                    if ui
                        .add_enabled(index + 1 < names.len(), egui::Button::new("↓").small())
                        .on_hover_text("Move row down")
                        .clicked()
                    {
                        swap = Some((index, index + 1));
                    }
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("×").color(theme::DANGER))
                                .small(),
                        )
                        .on_hover_text("Delete row")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
            }
            if let Some((from, to)) = swap {
                names.swap(from, to);
            }
            if let Some(index) = remove {
                names.remove(index);
            }
            if ui.button("Add row").clicked() {
                names.push(String::new());
            }
            if ui.button("Save").clicked() {
                save = true;
            }
        });
        if let Some(response) = response {
            layout.record_window_rect(WINDOW_PERFORMANCE_ROW_NAMES, response.response.rect);
        }
        if save {
            actions.push(UiAction::new(
                "scenes/setRowNames",
                serde_json::json!({ "rowNames": names }),
            ));
        } else if open {
            grid.row_names_edit = Some(names);
        }
    }

    if let Some(edit) = grid.clear_cell_edit.take() {
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let row_name = snapshot
            .scene_rows
            .get(edit.row)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Row {}", edit.row + 1));
        let window = layout.apply_window(
            egui::Window::new(format!(
                "Clear cell: {} row {}",
                edit.scene_name,
                edit.row + 1
            ))
            .id(egui::Id::unique("modv-window:scene_clear_cell"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_width(320.0)
            .default_size([400.0, 150.0]),
            "scene_clear_cell",
        );
        let response = window.show(ui.ctx(), |ui| {
            ui.label(format!(
                "Clear the {row_name} cell of {}? This removes its saved modules from the Scene.",
                edit.scene_name
            ));
            ui.label(
                egui::RichText::new(edit.module_names.join(", "))
                    .monospace()
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Clear cell").clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if let Some(response) = response {
            layout.record_window_rect("scene_clear_cell", response.response.rect);
        }
        if confirm {
            actions.push(clear_cell_action(&edit.scene_id, edit.row));
        } else if !cancel && open {
            grid.clear_cell_edit = Some(edit);
        }
    }

    if let Some(edit) = grid.overwrite_cell_edit.take() {
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let row_name = snapshot
            .scene_rows
            .get(edit.row)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Row {}", edit.row + 1));
        let window = layout.apply_window(
            egui::Window::new(format!(
                "Overwrite cell: {} row {}",
                edit.scene_name,
                edit.row + 1
            ))
            .id(egui::Id::unique("modv-window:scene_overwrite_cell"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_width(320.0)
            .default_size([400.0, 150.0]),
            "scene_overwrite_cell",
        );
        let response = window.show(ui.ctx(), |ui| {
            ui.label(format!(
                "Overwrite the {row_name} cell of {} with Group {}? Its saved modules will be replaced.",
                edit.scene_name, edit.drag.group_name
            ));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Overwrite cell").clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if let Some(response) = response {
            layout.record_window_rect("scene_overwrite_cell", response.response.rect);
        }
        if confirm {
            actions.push(edit.drag.drop_action(&edit.scene_id, edit.row));
        } else if !cancel && open {
            grid.overwrite_cell_edit = Some(edit);
        }
    }
}

/// How one Scene cell is highlighted against the live state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneCellHighlight {
    /// Performance mode is off and the engine renders this whole Scene.
    Applied,
    /// Performance mode is on and this cell is its row's live Scene.
    Live,
    /// The cell belongs to neither the live row nor an applied Scene.
    None,
}

/// Decide how a cell is marked.
///
/// The authoritative per-row live Scenes (`active_scene_rows`, derived from
/// live Group stamps) decide the marking in both modes: only the cell whose
/// Scene id matches `active_scene_rows[row]` is [`SceneCellHighlight::Live`].
/// The applied Scene no longer lights a whole column when other rows are live
/// on different Scenes (BUGS.md). A row with no live entry marks nothing.
///
/// `performance_enabled` and `applied_scene_id` are kept in the signature for
/// callers, but no longer affect the decision.
pub fn scene_cell_highlight(
    _performance_enabled: bool,
    _applied_scene_id: Option<&str>,
    active_scene_rows: &BTreeMap<usize, String>,
    scene_id: &str,
    row: usize,
) -> SceneCellHighlight {
    if active_scene_rows.get(&row).is_some_and(|id| id == scene_id) {
        SceneCellHighlight::Live
    } else {
        SceneCellHighlight::None
    }
}

/// The visual treatment of the per-cell performance toggle.
///
/// Enabled cells keep the green dot and their normal chrome. Disabled cells
/// get a red dot, a red border and left stripe, a dimming scrim over the hover
/// preview and an `OFF` chip, so the skipped state is unmistakable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellEnableStyle {
    pub marker: Color32,
    pub border: Option<Color32>,
    pub scrim: Option<Color32>,
    pub chip: Option<&'static str>,
}

/// Resolve the enabled/disabled decoration for one Scene cell.
pub fn cell_enable_style(disabled: bool) -> CellEnableStyle {
    if disabled {
        CellEnableStyle {
            marker: theme::DANGER,
            border: Some(theme::DANGER),
            scrim: Some(Color32::from_black_alpha(150)),
            chip: Some("OFF"),
        }
    } else {
        CellEnableStyle {
            marker: theme::OK,
            border: None,
            scrim: None,
            chip: None,
        }
    }
}

/// The dot in a cell's top-left corner, as its own widget.
///
/// The dot used to be a bare `ui.interact` rectangle painted over the cell's
/// click-and-drag response, which let the cell win the press. A real widget
/// registered after the cell receives the click, and its hit area is larger
/// than the painted dot so it is easy to hit.
struct CellEnableDot {
    disabled: bool,
}

impl egui::Widget for CellEnableDot {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        let style = cell_enable_style(self.disabled);
        let centre = rect.center();
        let painter = ui.painter();
        painter.circle_filled(centre, 3.5, style.marker);
        if response.hovered() {
            painter.circle_stroke(centre, 5.5, Stroke::new(1.0, style.marker));
        }
        let label = if self.disabled {
            "Enable this cell in performance mode"
        } else {
            "Disable this cell in performance mode"
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        response.on_hover_text(label)
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
        if !cell.is_empty() {
            grid.keep_preview(&target, &cell.module_names);
        }
    }

    let highlight = scene_cell_highlight(
        snapshot.performance.enabled,
        snapshot.performance.current_scene_id.as_deref(),
        &snapshot.active_scene_rows,
        &scene.id,
        row,
    );
    let live = highlight == SceneCellHighlight::Live;
    // Performance off keeps the historical applied-column highlight; with
    // performance on the applied Scene is not live in this row unless
    // `active_scene_rows` says so, so the cell stays unmarked.
    let current = live || highlight == SceneCellHighlight::Applied;
    let copying = grid.copy_source.as_ref() == Some(&target);
    let enable_style = cell_enable_style(cell.disabled);
    let fill = if copying || current {
        theme::ACCENT_DIM
    } else if hovered {
        theme::PANEL_ALT
    } else {
        theme::PANEL
    };

    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(0), fill);
    let border = if let Some(colour) = enable_style.border {
        colour
    } else if copying || current {
        theme::ACCENT
    } else if hovered {
        theme::MUTED
    } else {
        theme::BORDER
    };
    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(if copying || current { 1.5 } else { 1.0 }, border),
        StrokeKind::Inside,
    );

    // Hover preview: the engine's isolated Scene-cell render. Only the hovered
    // cell falls back to the live composited frame until its preview arrives;
    // a non-hovered still paints ONLY its cached preview, never the live output
    // (which would leak the main scene into cells whose cache was evicted),
    // matching `performanceGrid`'s "leaves a still".
    if !cell.is_empty() && grid.paints_preview(&target, hovered) {
        let texture = grid.preview_texture_for(&target, hovered, snapshot, live_preview);
        if let Some(texture_id) = texture {
            let inner = rect.shrink(2.0);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture_id, inner, uv, Color32::from_white_alpha(210));
            painter.rect_filled(inner, CornerRadius::same(0), Color32::from_black_alpha(70));
        }
    }
    if should_request_preview(cell, hovered) {
        grid.request_preview(&scene.id, row, std::time::Duration::from_millis(150));
    }

    // Disabled state is painted after the hover preview so the preview cannot
    // hide it: a dimming scrim, a red border and a red left stripe.
    if let Some(scrim) = enable_style.scrim {
        let border = enable_style.border.unwrap_or(theme::DANGER);
        painter.rect_filled(rect.shrink(2.0), CornerRadius::same(0), scrim);
        painter.rect_filled(
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height())),
            CornerRadius::same(0),
            border,
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(0),
            Stroke::new(2.0, border),
            StrokeKind::Inside,
        );
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
        FontId::proportional(11.0),
        theme::MUTED,
    );
    painter.text(
        rect.left_bottom() + egui::vec2(6.0, -5.0),
        Align2::LEFT_BOTTOM,
        cell.label(),
        FontId::proportional(12.0),
        if cell.is_empty() {
            theme::MUTED
        } else {
            theme::FOREGROUND
        },
    );

    // Per-cell performance flag: red = skipped by performance mode, green =
    // eligible. The dot is a real widget registered after the cell (and after
    // its preview), so it reliably receives the click; a manual Scene/cell
    // apply still uses a disabled cell. `enabled` is the *new* enabled state
    // for `scenes/setCellEnabled`, so it is `cell.disabled`, not its negation.
    let dot_hit =
        egui::Rect::from_center_size(rect.left_top() + egui::vec2(10.0, 27.0), Vec2::splat(20.0));
    let mut cell_dot_clicked = false;
    if !cell.is_empty() {
        // `place` registers the widget without advancing the cell column's
        // cursor: `put` would rewind it to the dot's bottom, pulling every
        // following row of this column upward.
        let dot_response = ui.place(
            dot_hit,
            CellEnableDot {
                disabled: cell.disabled,
            },
        );
        cell_dot_clicked = dot_response.clicked();
        if cell_dot_clicked {
            actions.push(set_cell_enabled_action(&scene.id, row, cell.disabled));
        }
    }

    // Timing badge (top-right) uses the per-Scene override when present, in
    // the project's active timing unit.
    if let Some(timing) = scene.timing {
        let label = timing_badge(timing, snapshot.performance.timing_unit);
        let chip_rect = egui::Rect::from_min_size(
            rect.right_top() + egui::vec2(-54.0, 4.0),
            egui::vec2(48.0, 14.0),
        );
        painter.rect_filled(chip_rect, CornerRadius::same(0), theme::PANEL_ALT);
        painter.text(
            chip_rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(10.5),
            theme::MUTED,
        );
    }

    if let Some(chip) = enable_style.chip {
        let chip_rect = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(23.0, 20.0),
            egui::vec2(27.0, 13.0),
        );
        painter.rect_filled(chip_rect, CornerRadius::same(0), theme::DANGER);
        painter.text(
            chip_rect.center(),
            Align2::CENTER_CENTER,
            chip,
            FontId::monospace(9.5),
            theme::BACKGROUND,
        );
    }

    // The live marker sits below the timing badge, on the right edge: clear of
    // the enable dot (top-left), the timing badge (top-right), the old `OFF`
    // chip (left) and the `COPY` badge (bottom-right).
    if live {
        let label = egui::Rect::from_min_size(
            rect.right_top() + egui::vec2(-44.0, 20.0),
            egui::vec2(38.0, 13.0),
        );
        painter.rect_filled(label, CornerRadius::same(0), theme::ACCENT_DIM);
        painter.rect_stroke(
            label,
            CornerRadius::same(0),
            Stroke::new(1.0, theme::ACCENT),
            StrokeKind::Inside,
        );
        painter.text(
            label.center(),
            Align2::CENTER_CENTER,
            "LIVE",
            FontId::monospace(9.5),
            theme::ACCENT,
        );
    }

    if copying {
        let label = egui::Rect::from_min_size(
            rect.right_bottom() + egui::vec2(-44.0, -17.0),
            egui::vec2(38.0, 13.0),
        );
        painter.rect_filled(label, CornerRadius::same(0), theme::ACCENT_DIM);
        painter.text(
            label.center(),
            Align2::CENTER_CENTER,
            "COPY",
            FontId::monospace(10.0),
            theme::ACCENT,
        );
    }

    // Primary click applies the Scene; a double-click on an empty cell
    // creates the row's Group in that Scene. The enabled dot owns presses
    // inside its (enlarged) hit area, so the cell must never apply or start a
    // copy from there.
    let press_on_dot = !cell.is_empty()
        && response
            .interact_pointer_pos()
            .is_some_and(|position| dot_hit.contains(position));
    if response.clicked() && !cell_dot_clicked && !cell.is_empty() && !press_on_dot {
        // A cell click swaps only that row's layer, matching the Scene grid's
        // per-row model. Applying the whole column is the header's job.
        actions.push(apply_scene_row_action(&scene.id, row));
    }
    if response.double_clicked() && !cell_dot_clicked && cell.is_empty() && !press_on_dot {
        actions.push(UiAction::new(
            "scenes/createSceneCellGroup",
            serde_json::json!({ "sceneId": scene.id, "rowIndex": row }),
        ));
    }

    // Ctrl/Cmd+drag starts a cell copy.
    let copy_modifier = ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
    if response.drag_started() && copy_modifier && !press_on_dot {
        grid.begin_drag_copy(target.clone());
    }
    if grid.copy_source.is_some()
        && grid.copy_is_drag
        && response.hovered()
        && ui.input(|input| input.pointer.any_released())
        && let Some(action) = grid.drop_copy(&target)
    {
        actions.push(action);
    }

    // Gallery -> Scene cell drop: a module tile released over ANY cell (empty
    // or populated) adds that module to the cell's row. Checking the payload
    // type explicitly leaves other drags (ModuleDrag, TabDrag) untouched, and
    // taking the payload consumes the release so no other target answers it.
    if egui::DragAndDrop::has_payload_of_type::<GalleryDrag>(ui.ctx())
        && ui.rect_contains_pointer(rect)
        && ui.input(|input| input.pointer.any_released())
        && let Some(payload) = egui::DragAndDrop::take_payload::<GalleryDrag>(ui.ctx())
    {
        actions.push(UiAction::new(
            "scenes/addModuleToCell",
            serde_json::json!({
                "sceneId": scene.id,
                "rowIndex": row,
                "moduleName": payload.module_name,
            }),
        ));
    }

    // Group -> Scene cell drop: an empty cell queues the overwrite immediately;
    // an occupied cell stores a pending confirmation and queues nothing yet.
    if egui::DragAndDrop::has_payload_of_type::<GroupDrag>(ui.ctx())
        && ui.rect_contains_pointer(rect)
        && ui.input(|input| input.pointer.any_released())
        && let Some(payload) = egui::DragAndDrop::take_payload::<GroupDrag>(ui.ctx())
    {
        if cell.is_empty() {
            actions.push(payload.drop_action(&scene.id, row));
        } else {
            grid.overwrite_cell_edit = Some(OverwriteCellEdit {
                scene_id: scene.id.clone(),
                row,
                scene_name: scene.name.clone(),
                drag: payload.as_ref().clone(),
            });
        }
    }

    let cell_label = if live {
        format!("LIVE {} row {}", scene.name, row + 1)
    } else {
        format!("{} row {}", scene.name, row + 1)
    };
    let cell_enabled = ui.is_enabled();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::Role::Button, cell_enabled, cell_label.clone())
    });

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
    if ui
        .add_enabled(!cell.is_empty(), egui::Button::new("Apply this row"))
        .clicked()
    {
        actions.push(apply_scene_row_action(&scene.id, row));
        ui.close();
    }
    if ui.button("Apply whole Scene").clicked() {
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
        // The legacy grid confirms a clear before writing it; keep the saved
        // cell untouched until the dialog is accepted.
        grid.clear_cell_edit = Some(ClearCellEdit {
            scene_id: scene.id.clone(),
            row,
            scene_name: scene.name.clone(),
            module_names: cell.module_names.clone(),
        });
        ui.close();
    }
    if ui
        .add_enabled(
            !cell.is_empty(),
            egui::Button::new(if cell.disabled {
                "Enable cell in performance mode"
            } else {
                "Disable cell in performance mode"
            }),
        )
        .clicked()
    {
        // `enabled` is the new state, so a disabled cell toggles back on.
        actions.push(set_cell_enabled_action(&scene.id, row, cell.disabled));
        ui.close();
    }
    ui.separator();
    if ui.button("Copy cell (Ctrl+drag)").clicked() {
        grid.begin_copy(CellRef::new(scene.id.clone(), row));
        ui.close();
    }
    if ui
        .add_enabled(grid.copy_source.is_some(), egui::Button::new("Paste cell"))
        .clicked()
    {
        if let Some(action) = grid.paste_into(&CellRef::new(scene.id.clone(), row)) {
            actions.push(action);
        }
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

/// Build an `applyScene` action (used by the column header and menu helpers).
pub(crate) fn apply_scene_action(scene_id: &str) -> UiAction {
    UiAction::new(
        "scenes/applyScene",
        serde_json::json!({ "sceneId": scene_id }),
    )
}

/// Build an `applySceneRow` action: swap only this row's layer.
pub(crate) fn apply_scene_row_action(scene_id: &str, row: usize) -> UiAction {
    UiAction::new(
        "scenes/applySceneRow",
        serde_json::json!({ "sceneId": scene_id, "rowIndex": row }),
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
        grid.begin_drag_copy(source.clone());
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
        grid.begin_drag_copy(cell.clone());
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
        grid.begin_drag_copy(CellRef::new("scene-a", 0));
        grid.cancel_copy();
        assert!(!grid.is_copying());
        assert!(!grid.copy_is_drag, "cancelling clears the flag too");
    }

    #[test]
    fn menu_copy_is_a_durable_clipboard_while_drag_copy_is_one_shot() {
        let mut grid = SceneGridState::default();

        // A menu copy is not flagged as a drag, so the frame-level release
        // cleanup must not cancel it.
        grid.begin_copy(CellRef::new("scene-a", 0));
        assert!(grid.is_copying());
        assert!(!grid.copy_is_drag);

        // Paste borrows the clipboard: it can be repeated, leaves the source
        // intact, and ignores a same-cell paste.
        let target = CellRef::new("scene-b", 1);
        let first = grid.paste_into(&target).expect("a paste emits an action");
        let second = grid.paste_into(&target).expect("pastes can be repeated");
        assert_eq!(first.payload, second.payload);
        assert!(grid.is_copying(), "pasting must not consume the clipboard");
        assert_eq!(grid.paste_into(&CellRef::new("scene-a", 0)), None);

        // The frame-level cleanup cancels only drag-originated copies.
        grid.begin_drag_copy(target.clone());
        assert!(grid.copy_is_drag);
        grid.cancel_copy();
        assert!(!grid.is_copying());
        assert!(!grid.copy_is_drag);
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
        let empty = SceneCellView {
            row: 0,
            module_names: Vec::new(),
            disabled: false,
        };
        assert!(!should_request_preview(&empty, true));
        let populated = SceneCellView {
            row: 1,
            module_names: vec!["Bands".to_string()],
            disabled: false,
        };
        assert!(should_request_preview(&populated, true));
        assert!(!should_request_preview(&populated, false));
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
        assert_eq!(
            set_cell_enabled_action("scene-a", 2, false).payload,
            serde_json::json!({"sceneId": "scene-a", "rowIndex": 2, "enabled": false})
        );
    }

    #[test]
    fn add_row_appends_a_blank_name_for_the_renamer() {
        let names = vec!["WEBCAM".to_string(), "Effects".to_string()];
        let action = add_row_action(&names);
        assert_eq!(action.action, "scenes/setRowNames");
        assert_eq!(
            action.payload["rowNames"],
            serde_json::json!(["WEBCAM", "Effects", ""])
        );
        assert_eq!(names, vec!["WEBCAM", "Effects"], "the input is not mutated");
    }

    #[test]
    fn remove_row_splices_the_name_out() {
        let names = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let action = remove_row_from(&names, 1).expect("row exists");
        assert_eq!(action.payload["rowNames"], serde_json::json!(["A", "C"]));
        assert_eq!(remove_row_from(&names, 9), None);
    }

    #[test]
    fn row_names_pad_to_the_grid_height() {
        let snapshot = EngineSnapshot {
            scene_rows: vec!["Background".to_string()],
            ..Default::default()
        };
        let names = grid_row_names(&snapshot, 3);
        assert_eq!(names, vec!["Background", "", ""]);
    }

    #[test]
    fn dropping_a_row_reorders_the_names() {
        let mut grid = SceneGridState::default();
        let mut names = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        grid.begin_row_drag(0);
        grid.row_drop_target = Some(2);
        let action = grid.drop_row(&mut names, 2).expect("row drop");
        assert_eq!(action.action, "scenes/setRowNames");
        assert_eq!(
            action.payload["rowNames"],
            serde_json::json!(["B", "C", "A"])
        );
        assert_eq!(names, vec!["B", "C", "A"]);
        assert!(grid.row_drag.is_none());
    }

    #[test]
    fn dropping_a_scene_column_emits_move_scene_with_the_target_index() {
        let mut grid = SceneGridState::default();
        grid.begin_scene_drag("scene-a");
        grid.scene_drop_target = Some("scene-c".to_string());
        let action = grid.drop_scene(2).expect("column drop");
        assert_eq!(action.action, "scenes/moveScene");
        assert_eq!(
            action.payload,
            serde_json::json!({"sceneId": "scene-a", "toIndex": 2})
        );
    }

    #[test]
    fn same_column_drop_does_not_emit_an_action() {
        let mut grid = SceneGridState::default();
        grid.begin_scene_drag("scene-a");
        grid.scene_drop_target = Some("scene-a".to_string());
        assert_eq!(grid.drop_scene(0), None);
        assert!(grid.scene_drag.is_none());
    }

    fn live_rows(entries: &[(usize, &str)]) -> BTreeMap<usize, String> {
        entries
            .iter()
            .map(|(row, scene)| (*row, (*scene).to_string()))
            .collect()
    }

    #[test]
    fn performance_mode_marks_only_each_rows_live_cell() {
        let rows = live_rows(&[(0, "scene-a"), (1, "scene-b")]);

        assert_eq!(
            scene_cell_highlight(true, Some("scene-a"), &rows, "scene-a", 0),
            SceneCellHighlight::Live,
            "row 0 is live on Scene A"
        );
        assert_eq!(
            scene_cell_highlight(true, Some("scene-a"), &rows, "scene-b", 1),
            SceneCellHighlight::Live,
            "row 1 is live on Scene B"
        );
        assert_eq!(
            scene_cell_highlight(true, Some("scene-a"), &rows, "scene-a", 1),
            SceneCellHighlight::None,
            "Scene A is not live in row 1 even though it is the applied Scene"
        );
        assert_eq!(
            scene_cell_highlight(true, Some("scene-a"), &rows, "scene-c", 2),
            SceneCellHighlight::None,
            "a row with no live entry marks nothing"
        );
    }

    #[test]
    fn performance_mode_ignores_the_applied_scene_when_rows_diverge() {
        let rows = live_rows(&[(0, "scene-a"), (1, "scene-b")]);

        // The applied Scene is C, but the rows are on A and B: only A/0 and
        // B/1 are live. C never gets a whole-column highlight.
        for row in 0..3 {
            assert_eq!(
                scene_cell_highlight(true, Some("scene-c"), &rows, "scene-c", row),
                SceneCellHighlight::None,
                "applied Scene C is not live at row {row}"
            );
        }
        assert_eq!(
            scene_cell_highlight(true, Some("scene-c"), &rows, "scene-a", 0),
            SceneCellHighlight::Live
        );
        assert_eq!(
            scene_cell_highlight(true, Some("scene-c"), &rows, "scene-b", 1),
            SceneCellHighlight::Live
        );
    }

    #[test]
    fn performance_off_marks_only_each_rows_live_cell() {
        // Even with performance mode off, the live Group stamps are the
        // authoritative per-row answer. The applied Scene must not light its
        // whole column when other rows are live on different Scenes (BUGS.md).
        let rows = live_rows(&[(0, "scene-a"), (1, "scene-b")]);

        assert_eq!(
            scene_cell_highlight(false, Some("scene-a"), &rows, "scene-a", 0),
            SceneCellHighlight::Live,
            "row 0 is live on Scene A even with performance mode off"
        );
        assert_eq!(
            scene_cell_highlight(false, Some("scene-a"), &rows, "scene-b", 1),
            SceneCellHighlight::Live,
            "row 1 is live on Scene B even though Scene A is the applied Scene"
        );
        assert_eq!(
            scene_cell_highlight(false, Some("scene-a"), &rows, "scene-a", 1),
            SceneCellHighlight::None,
            "the applied Scene must not light a row that is live on another Scene"
        );
        assert_eq!(
            scene_cell_highlight(false, Some("scene-a"), &rows, "scene-a", 2),
            SceneCellHighlight::None,
            "the applied Scene must not light a row with no live Scene"
        );
        assert_eq!(
            scene_cell_highlight(false, Some("scene-c"), &rows, "scene-c", 0),
            SceneCellHighlight::None,
            "an applied Scene that is not live in any row marks nothing"
        );
        assert_eq!(
            scene_cell_highlight(false, None, &rows, "scene-a", 0),
            SceneCellHighlight::Live,
            "per-row live Scenes are marked even without an applied Scene"
        );
    }
}
