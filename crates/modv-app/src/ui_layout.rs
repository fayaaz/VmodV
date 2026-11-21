//! The VmodV shell's persisted resizable layout.
//!
//! The VmodV shell's centre area has a Live Output / Gallery / Scenes stack
//! with a draggable split, the Groups and Inspector side panels are resizable,
//! and the floating tool windows (Tempo, Command palette, Keyboard shortcuts,
//! About, Scene timing, Performance row names, Tween curve, …) can be moved and
//! resized. Their geometry is stored in `<userData>/vmodv-layout.json`:
//!
//! ```json
//! {
//!   "groups_width": 270.0,
//!   "inspector_width": 340.0,
//!   "live_height": 0.0,
//!   "gallery_height": 0.0,
//!   "dock_width": 320.0,
//!   "tabs": { "inspector": "docked", "media": "floating" },
//!   "windows": {
//!     "tempo": { "x": 40.0, "y": 60.0, "width": 320.0, "height": 200.0 }
//!   }
//! }
//! ```
//!
//! All sizes are logical points (the same unit egui and winit use). Panel
//! widths are non-zero; a section height of `0` means "automatic" (the current
//! proportional default). `windows` is keyed by a stable window id, so a window
//! that is not open this run keeps its saved geometry for the next one. `tabs`
//! holds only the non-default `docked`/`floating` placements of the right-panel
//! tabs (`inspector`, `media`, `audio`, `info`, `plugins`, `midi`); a missing
//! or unknown entry means `tabs`.
//!
//! Nothing here touches egui or the windowing system: the document, the atomic
//! read/write, the clamping rules and the conversions to
//! [`modv_ui::LayoutState`] are pure, so they are unit-testable on a headless
//! host. A saved layout is clamped against the *current* window before use
//! ([`LayoutSettings::clamp_to`]) so a smaller window or a resolution change can
//! never push a panel or a floating window off-screen.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use modv_ui::layout::{LayoutState, TabPlacement, WindowBounds as UiWindowBounds};
use modv_ui::state::SidePanelTab;

/// The persisted layout document inside the shared userData directory.
pub const LAYOUT_SETTINGS_FILENAME: &str = "vmodv-layout.json";

/// The Groups panel's default width (logical points), matching the shell's
/// current `egui::Panel::left("modv-groups").default_size(270.0)`.
pub const DEFAULT_GROUPS_WIDTH: f32 = 270.0;
/// The Inspector panel's default width (logical points), matching the shell's
/// current `egui::Panel::right("modv-inspector").default_size(340.0)`.
pub const DEFAULT_INSPECTOR_WIDTH: f32 = 340.0;
/// The dock column's default width.
pub const DEFAULT_DOCK_WIDTH: f32 = 320.0;

/// The Groups panel's minimum width (`min_size(210.0)`).
pub const MIN_GROUPS_WIDTH: f32 = 210.0;
/// The Inspector panel's minimum width (`min_size(250.0)`).
pub const MIN_INSPECTOR_WIDTH: f32 = 250.0;
/// The dock column's minimum width.
pub const MIN_DOCK_WIDTH: f32 = 250.0;
/// The centre column's minimum width: below this the Live Output/Gallery no
/// longer render usefully.
pub const MIN_CENTRE_WIDTH: f32 = 360.0;
/// The width at which the Groups/Inspector/centre minima fit.
pub const MIN_TOTAL_WIDTH: f32 = MIN_GROUPS_WIDTH + MIN_INSPECTOR_WIDTH + MIN_CENTRE_WIDTH;
/// The width at which all four columns fit with a docked tab.
pub const MIN_TOTAL_WIDTH_WITH_DOCK: f32 =
    MIN_GROUPS_WIDTH + MIN_INSPECTOR_WIDTH + MIN_DOCK_WIDTH + MIN_CENTRE_WIDTH;

/// The Live Output section's minimum height.
pub const MIN_LIVE_HEIGHT: f32 = 120.0;
/// The Scenes section's minimum height.
pub const MIN_SCENES_HEIGHT: f32 = 140.0;

/// A floating window's minimum width.
pub const MIN_WINDOW_WIDTH: f32 = 200.0;
/// A floating window's minimum height.
pub const MIN_WINDOW_HEIGHT: f32 = 120.0;

/// One floating window's saved geometry, in logical points.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowBounds {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
}

impl WindowBounds {
    /// A placement, for callers building the document from live geometry.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether the placement is usable as saved: finite and positively sized.
    pub fn is_usable(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }

    /// Clamp the placement onto an `available_width` × `available_height` area.
    ///
    /// Unusable placements are dropped (`None`): a NaN or non-positive size
    /// cannot be sanitised into a sensible window. A negative position is
    /// clamped to the top-left edge, and the window is kept fully on-screen.
    pub fn clamp_to(&self, available_width: f32, available_height: f32) -> Option<Self> {
        if !self.is_usable() {
            return None;
        }
        let max_width = available_width.max(MIN_WINDOW_WIDTH);
        let max_height = available_height.max(MIN_WINDOW_HEIGHT);
        let width = self.width.clamp(MIN_WINDOW_WIDTH, max_width);
        let height = self.height.clamp(MIN_WINDOW_HEIGHT, max_height);
        Some(Self {
            x: self.x.max(0.0).min((available_width - width).max(0.0)),
            y: self.y.max(0.0).min((available_height - height).max(0.0)),
            width,
            height,
        })
    }
}

/// The persisted resizable layout.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayoutSettings {
    /// The Groups side panel width in logical points.
    #[serde(default = "default_groups_width")]
    pub groups_width: f32,
    /// The Inspector side panel width in logical points.
    #[serde(default = "default_inspector_width")]
    pub inspector_width: f32,
    /// The centre stack's Live Output height; `0` is automatic.
    #[serde(default)]
    pub live_height: f32,
    /// The centre stack's Gallery height; `0` is automatic.
    #[serde(default)]
    pub gallery_height: f32,
    /// The dock column width in logical points.
    #[serde(default = "default_dock_width")]
    pub dock_width: f32,
    /// Non-default tab placements keyed by tab id (`inspector`, `media`, …).
    #[serde(default)]
    pub tabs: BTreeMap<String, String>,
    /// Per-floating-window geometry, keyed by a stable window id.
    #[serde(default)]
    pub windows: BTreeMap<String, WindowBounds>,
}

fn default_groups_width() -> f32 {
    DEFAULT_GROUPS_WIDTH
}

fn default_inspector_width() -> f32 {
    DEFAULT_INSPECTOR_WIDTH
}

fn default_dock_width() -> f32 {
    DEFAULT_DOCK_WIDTH
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            groups_width: DEFAULT_GROUPS_WIDTH,
            inspector_width: DEFAULT_INSPECTOR_WIDTH,
            live_height: 0.0,
            gallery_height: 0.0,
            dock_width: DEFAULT_DOCK_WIDTH,
            tabs: BTreeMap::new(),
            windows: BTreeMap::new(),
        }
    }
}

impl LayoutSettings {
    /// The document path inside a userData directory.
    pub fn settings_path(user_data: &Path) -> PathBuf {
        user_data.join(LAYOUT_SETTINGS_FILENAME)
    }

    /// Read the document, treating a missing file as "nothing saved".
    ///
    /// A malformed document is an `Err`, but callers recover with
    /// [`LayoutSettings::load_or_default`]: a broken file must never stop the
    /// shell from opening.
    pub fn read_from(path: &Path) -> Result<Option<Self>, String> {
        let path = crate::settings_compat::read_path(path);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("could not read {}: {error}", path.display()));
            }
        };
        let settings: Self = serde_json::from_str(&text)
            .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))?;
        Ok(Some(settings))
    }

    /// Load the document, falling back to the defaults on a missing or
    /// malformed file.
    pub fn load_or_default(path: &Path) -> Self {
        Self::read_from(path).ok().flatten().unwrap_or_default()
    }

    /// Write the document atomically, creating the userData directory.
    ///
    /// Persistence is best-effort: callers log the error and keep running.
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let temporary = path.with_extension("json.tmp");
        // A NaN/negative live reading is never persisted: serde_json would
        // write `null`, which no longer deserialises as `f32`.
        let text = serde_json::to_string_pretty(&self.sanitised())
            .map_err(|error| format!("could not serialise layout settings: {error}"))?;
        std::fs::write(&temporary, text)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(())
    }

    /// A document with non-finite/negative values replaced by defaults and
    /// unusable window entries dropped, without reference to a window size.
    fn sanitised(&self) -> Self {
        let width = |value: f32, fallback: f32| {
            if value.is_finite() && value > 0.0 {
                value
            } else {
                fallback
            }
        };
        let height = |value: f32| {
            if value.is_finite() && value >= 0.0 {
                value
            } else {
                0.0
            }
        };
        Self {
            groups_width: width(self.groups_width, DEFAULT_GROUPS_WIDTH),
            inspector_width: width(self.inspector_width, DEFAULT_INSPECTOR_WIDTH),
            live_height: height(self.live_height),
            gallery_height: height(self.gallery_height),
            dock_width: width(self.dock_width, DEFAULT_DOCK_WIDTH),
            // Keep only known tab ids and placements, and only non-default
            // placements: the document is canonical and unknown values degrade
            // to the tab strip.
            tabs: self
                .tabs
                .iter()
                .filter_map(|(tab, placement)| {
                    let tab = SidePanelTab::from_key(tab)?;
                    let placement = TabPlacement::from_key(placement);
                    (placement != TabPlacement::Tabs)
                        .then(|| (tab.key().to_string(), placement.key().to_string()))
                })
                .collect(),
            windows: self
                .windows
                .iter()
                .filter(|(_, bounds)| bounds.is_usable())
                .map(|(window, bounds)| (window.clone(), *bounds))
                .collect(),
        }
    }

    /// Clamp a saved layout against the current window/monitor size.
    ///
    /// * Panel widths keep their minimums and leave at least
    ///   [`MIN_CENTRE_WIDTH`] for the centre column. Below
    ///   [`MIN_TOTAL_WIDTH`] the Groups/Inspector/centre minima are scaled
    ///   proportionally so a tiny window never produces a negative centre.
    /// * Section heights are clamped so the Scenes grid keeps
    ///   [`MIN_SCENES_HEIGHT`] and an explicitly saved Live Output keeps
    ///   [`MIN_LIVE_HEIGHT`]; the Gallery yields its height first. `0` (auto)
    ///   is preserved.
    /// * Floating windows are clamped on-screen; NaN/non-positive entries are
    ///   dropped so the window reopens at its default placement.
    pub fn clamp_to(&self, available_width: f32, available_height: f32) -> Self {
        let sanitised = self.sanitised();
        let dock_visible = sanitised
            .tabs
            .values()
            .any(|placement| TabPlacement::from_key(placement) == TabPlacement::Docked);

        // An unknown/zero size means "use the minimum layout" for widths and
        // "leave both sections automatic" for heights.
        let available_width = if available_width.is_finite() && available_width > 0.0 {
            available_width
        } else if dock_visible {
            MIN_TOTAL_WIDTH_WITH_DOCK
        } else {
            MIN_TOTAL_WIDTH
        };
        let available_height = if available_height.is_finite() && available_height > 0.0 {
            available_height
        } else {
            0.0
        };

        let (groups_width, inspector_width, dock_width) = if dock_visible {
            if available_width >= MIN_TOTAL_WIDTH_WITH_DOCK {
                let dock_width = sanitised.dock_width.clamp(
                    MIN_DOCK_WIDTH,
                    available_width - MIN_GROUPS_WIDTH - MIN_INSPECTOR_WIDTH - MIN_CENTRE_WIDTH,
                );
                let groups_width = sanitised.groups_width.clamp(
                    MIN_GROUPS_WIDTH,
                    available_width - dock_width - MIN_INSPECTOR_WIDTH - MIN_CENTRE_WIDTH,
                );
                let inspector_width = sanitised.inspector_width.clamp(
                    MIN_INSPECTOR_WIDTH,
                    available_width - dock_width - groups_width - MIN_CENTRE_WIDTH,
                );
                (groups_width, inspector_width, dock_width)
            } else {
                // Too small for every column: scale all four minima so the
                // centre column never goes negative.
                let scale = available_width / MIN_TOTAL_WIDTH_WITH_DOCK;
                (
                    MIN_GROUPS_WIDTH * scale,
                    MIN_INSPECTOR_WIDTH * scale,
                    MIN_DOCK_WIDTH * scale,
                )
            }
        } else if available_width >= MIN_TOTAL_WIDTH {
            let groups_width = sanitised.groups_width.clamp(
                MIN_GROUPS_WIDTH,
                available_width - MIN_INSPECTOR_WIDTH - MIN_CENTRE_WIDTH,
            );
            let inspector_width = sanitised.inspector_width.clamp(
                MIN_INSPECTOR_WIDTH,
                available_width - groups_width - MIN_CENTRE_WIDTH,
            );
            let dock_width = sanitised
                .dock_width
                .clamp(MIN_DOCK_WIDTH, available_width.max(MIN_DOCK_WIDTH));
            (groups_width, inspector_width, dock_width)
        } else {
            // Too small for the panel minimums: scale them (and the centre)
            // proportionally so the centre column never goes negative.
            let scale = available_width / MIN_TOTAL_WIDTH;
            (
                MIN_GROUPS_WIDTH * scale,
                MIN_INSPECTOR_WIDTH * scale,
                sanitised
                    .dock_width
                    .clamp(MIN_DOCK_WIDTH, available_width.max(MIN_DOCK_WIDTH)),
            )
        };

        let mut live_height = if sanitised.live_height > 0.0 {
            sanitised.live_height.max(MIN_LIVE_HEIGHT)
        } else {
            0.0
        };
        let mut gallery_height = if sanitised.gallery_height > 0.0 {
            sanitised.gallery_height
        } else {
            0.0
        };
        // Only the height left above the Scenes minimum is available to the
        // Live Output + Gallery sections.
        let section_budget = (available_height - MIN_SCENES_HEIGHT).max(0.0);
        if live_height + gallery_height > section_budget {
            let overflow = live_height + gallery_height - section_budget;
            let gallery_cut = gallery_height.min(overflow);
            gallery_height -= gallery_cut;
            live_height = (live_height - (overflow - gallery_cut)).max(0.0);
        }
        if live_height > 0.0 {
            live_height = live_height.max(MIN_LIVE_HEIGHT.min(section_budget));
        }

        Self {
            groups_width,
            inspector_width,
            live_height,
            gallery_height,
            dock_width,
            tabs: sanitised.tabs,
            windows: sanitised
                .windows
                .iter()
                .filter_map(|(window, bounds)| {
                    bounds
                        .clamp_to(available_width, available_height)
                        .map(|bounds| (window.clone(), bounds))
                })
                .collect(),
        }
    }

    /// The live UI layout expressed as a persisted document.
    pub fn from_ui(layout: &LayoutState) -> Self {
        let mut tabs = BTreeMap::new();
        for tab in SidePanelTab::ALL {
            let placement = layout.tabs.placement(tab);
            if placement != TabPlacement::Tabs {
                tabs.insert(tab.key().to_string(), placement.key().to_string());
            }
        }
        Self {
            groups_width: layout.groups_width,
            inspector_width: layout.inspector_width,
            live_height: layout.live_height,
            gallery_height: layout.gallery_height,
            dock_width: layout.dock_width,
            tabs,
            windows: layout
                .windows
                .iter()
                .map(|(window, bounds)| {
                    (
                        window.clone(),
                        WindowBounds::new(bounds.x, bounds.y, bounds.width, bounds.height),
                    )
                })
                .collect(),
        }
        .sanitised()
    }

    /// Apply the document to the live UI layout.
    ///
    /// Values are sanitised first, so a hand-edited NaN or an unknown tab
    /// placement can never reach the shell.
    pub fn apply_to_ui(&self, layout: &mut LayoutState) {
        let settings = self.sanitised();
        layout.groups_width = settings.groups_width;
        layout.inspector_width = settings.inspector_width;
        layout.live_height = settings.live_height;
        layout.gallery_height = settings.gallery_height;
        layout.dock_width = settings.dock_width;
        layout.windows = settings
            .windows
            .iter()
            .map(|(window, bounds)| {
                (
                    window.clone(),
                    UiWindowBounds::new(bounds.x, bounds.y, bounds.width, bounds.height),
                )
            })
            .collect();
        for tab in SidePanelTab::ALL {
            let placement = settings
                .tabs
                .get(tab.key())
                .map(|placement| TabPlacement::from_key(placement))
                .unwrap_or(TabPlacement::Tabs);
            layout.tabs.set_placement(tab, placement);
        }
    }

    /// Record the live Groups/Inspector widths after a splitter drag.
    ///
    /// Non-finite or non-positive readings are ignored, so a failed geometry
    /// read cannot replace a good saved width.
    pub fn record_panels(&mut self, groups_width: f32, inspector_width: f32) {
        if groups_width.is_finite() && groups_width > 0.0 {
            self.groups_width = groups_width;
        }
        if inspector_width.is_finite() && inspector_width > 0.0 {
            self.inspector_width = inspector_width;
        }
    }

    /// Record the live centre section heights (`0` means automatic).
    pub fn record_sections(&mut self, live_height: f32, gallery_height: f32) {
        if live_height.is_finite() && live_height >= 0.0 {
            self.live_height = live_height;
        }
        if gallery_height.is_finite() && gallery_height >= 0.0 {
            self.gallery_height = gallery_height;
        }
    }

    /// Record one floating window's geometry; an unusable reading forgets the
    /// saved entry instead of writing NaN/negative bounds.
    pub fn record_window(&mut self, window: impl Into<String>, bounds: WindowBounds) {
        let window = window.into();
        if bounds.is_usable() && bounds.x >= 0.0 && bounds.y >= 0.0 {
            self.windows.insert(window, bounds);
        } else {
            self.windows.remove(&window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_native_layout_filename_is_read_as_a_fallback() {
        let (directory, _path) = temp_path();
        let legacy = directory.join("native-layout.json");
        let saved = LayoutSettings::default();
        saved.write_to(&legacy).expect("write legacy layout");
        let current = directory.join(LAYOUT_SETTINGS_FILENAME);
        assert_eq!(
            LayoutSettings::read_from(&current).expect("read legacy layout"),
            Some(saved)
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    fn temp_path() -> (PathBuf, PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "modv-ui-layout-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = LayoutSettings::settings_path(&directory);
        (directory, path)
    }

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> WindowBounds {
        WindowBounds::new(x, y, width, height)
    }

    #[test]
    fn defaults_match_the_current_shell_and_round_trip_through_json() {
        let (directory, path) = temp_path();
        let layout = LayoutSettings::default();

        assert_eq!(layout.groups_width, 270.0);
        assert_eq!(layout.inspector_width, 340.0);
        assert_eq!(layout.live_height, 0.0, "0 means the automatic split");
        assert_eq!(layout.gallery_height, 0.0);
        assert_eq!(layout.dock_width, 320.0);
        assert!(layout.tabs.is_empty(), "every tab starts in the strip");
        assert!(layout.windows.is_empty());

        layout.write_to(&path).expect("write layout");
        assert_eq!(
            LayoutSettings::read_from(&path).expect("read layout"),
            Some(layout),
            "the default layout round-trips"
        );
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the atomic write must not leave its temporary file behind"
        );
        assert_eq!(path, directory.join(LAYOUT_SETTINGS_FILENAME));

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_partial_or_legacy_document_loads_defaults_and_tolerates_unknown_keys() {
        let (directory, path) = temp_path();
        std::fs::create_dir_all(&directory).expect("temp dir");

        std::fs::write(
            &path,
            r#"{"groups_width": 300.0, "future_splitter": {"ratio": 0.4}}"#,
        )
        .expect("write partial document");
        assert_eq!(
            LayoutSettings::read_from(&path).expect("read partial document"),
            Some(LayoutSettings {
                groups_width: 300.0,
                ..LayoutSettings::default()
            }),
            "missing fields fall back to today's defaults and unknown keys are ignored"
        );

        std::fs::write(
            &path,
            r#"{
                "inspector_width": 360.0,
                "live_height": 260.0,
                "windows": {
                    "tempo": {"x": 40.0, "y": 60.0, "width": 320.0, "height": 200.0}
                }
            }"#,
        )
        .expect("write window document");
        let layout = LayoutSettings::read_from(&path)
            .expect("read window document")
            .expect("document");
        assert_eq!(layout.groups_width, DEFAULT_GROUPS_WIDTH);
        assert_eq!(layout.inspector_width, 360.0);
        assert_eq!(layout.live_height, 260.0);
        assert_eq!(layout.gallery_height, 0.0);
        assert_eq!(layout.windows["tempo"], bounds(40.0, 60.0, 320.0, 200.0));

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_missing_file_is_none_and_malformed_documents_are_an_error() {
        let (directory, path) = temp_path();

        assert_eq!(
            LayoutSettings::read_from(&path).expect("missing file"),
            None
        );
        assert_eq!(
            LayoutSettings::load_or_default(&path),
            LayoutSettings::default(),
            "a missing file opens with the defaults"
        );

        std::fs::create_dir_all(&directory).expect("temp dir");
        std::fs::write(&path, "{not json").expect("write broken json");
        assert!(LayoutSettings::read_from(&path).is_err());
        assert_eq!(
            LayoutSettings::load_or_default(&path),
            LayoutSettings::default(),
            "a malformed file must never stop the shell from opening"
        );

        std::fs::write(&path, r#"{"groups_width": "wide"}"#).expect("write wrong type");
        assert!(LayoutSettings::read_from(&path).is_err());

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_normal_window_keeps_panel_minimums_and_a_full_centre() {
        let layout = LayoutSettings {
            groups_width: 1.0,
            inspector_width: 1.0,
            ..Default::default()
        };

        let clamped = layout.clamp_to(1280.0, 800.0);
        assert_eq!(clamped.groups_width, MIN_GROUPS_WIDTH);
        assert_eq!(clamped.inspector_width, MIN_INSPECTOR_WIDTH);
        assert_eq!(
            1280.0 - clamped.groups_width - clamped.inspector_width,
            1280.0 - MIN_GROUPS_WIDTH - MIN_INSPECTOR_WIDTH
        );

        let layout = LayoutSettings {
            groups_width: 5000.0,
            inspector_width: 5000.0,
            ..Default::default()
        };
        let clamped = layout.clamp_to(1280.0, 800.0);
        assert_eq!(
            clamped.groups_width,
            1280.0 - MIN_INSPECTOR_WIDTH - MIN_CENTRE_WIDTH
        );
        assert_eq!(
            clamped.inspector_width, MIN_INSPECTOR_WIDTH,
            "the Inspector keeps its minimum when the Groups panel is greedy"
        );
        assert_eq!(
            1280.0 - clamped.groups_width - clamped.inspector_width,
            MIN_CENTRE_WIDTH,
            "the centre column is never squeezed below its minimum"
        );
    }

    #[test]
    fn a_tiny_window_scales_both_panels_proportionally_and_keeps_a_positive_centre() {
        let layout = LayoutSettings::default();

        let clamped = layout.clamp_to(600.0, 800.0);
        assert!(
            clamped.groups_width < MIN_GROUPS_WIDTH,
            "a window below the minimum layout scales the panels down"
        );
        assert!(clamped.inspector_width < MIN_INSPECTOR_WIDTH);
        assert!(clamped.groups_width > 0.0 && clamped.inspector_width > 0.0);

        let ratio = clamped.groups_width / clamped.inspector_width;
        let expected = MIN_GROUPS_WIDTH / MIN_INSPECTOR_WIDTH;
        assert!(
            (ratio - expected).abs() < 1e-4,
            "the minima scale proportionally: {ratio} vs {expected}"
        );
        let centre = 600.0 - clamped.groups_width - clamped.inspector_width;
        assert!(centre > 0.0, "the centre column stays positive: {centre}");
        assert!(
            (centre - MIN_CENTRE_WIDTH * 600.0 / MIN_TOTAL_WIDTH).abs() < 1e-3,
            "the centre scales with the same proportion"
        );
    }

    #[test]
    fn section_heights_leave_the_scenes_minimum_and_the_gallery_yields_first() {
        let layout = LayoutSettings {
            live_height: 200.0,
            gallery_height: 1000.0,
            ..Default::default()
        };
        let clamped = layout.clamp_to(1280.0, 900.0);
        assert_eq!(clamped.live_height, 200.0);
        assert_eq!(clamped.gallery_height, 560.0);
        assert_eq!(
            900.0 - clamped.live_height - clamped.gallery_height,
            MIN_SCENES_HEIGHT,
            "Scenes keeps its minimum height"
        );

        let layout = LayoutSettings {
            live_height: 800.0,
            gallery_height: 400.0,
            ..Default::default()
        };
        let clamped = layout.clamp_to(1280.0, 900.0);
        assert_eq!(clamped.gallery_height, 0.0, "the Gallery yields first");
        assert_eq!(clamped.live_height, 760.0);
        assert_eq!(900.0 - clamped.live_height - clamped.gallery_height, 140.0);

        let tiny = LayoutSettings {
            live_height: 300.0,
            gallery_height: 300.0,
            ..Default::default()
        };
        let clamped = tiny.clamp_to(1280.0, 200.0);
        assert_eq!(clamped.gallery_height, 0.0);
        assert_eq!(clamped.live_height, 60.0);
        assert_eq!(200.0 - clamped.live_height - clamped.gallery_height, 140.0);
    }

    #[test]
    fn automatic_section_heights_stay_automatic() {
        let layout = LayoutSettings::default();
        assert_eq!(layout.clamp_to(1280.0, 800.0).live_height, 0.0);
        assert_eq!(layout.clamp_to(1280.0, 800.0).gallery_height, 0.0);

        let layout = LayoutSettings {
            live_height: 40.0,
            ..Default::default()
        };
        assert_eq!(
            layout.clamp_to(1280.0, 800.0).live_height,
            MIN_LIVE_HEIGHT,
            "an explicitly saved section is raised to its minimum when there is room"
        );
    }

    #[test]
    fn window_bounds_clamp_on_screen_and_invalid_entries_are_dropped() {
        let layout = LayoutSettings {
            windows: BTreeMap::from([
                ("tempo".to_string(), bounds(5000.0, -10.0, 900.0, 700.0)),
                ("palette".to_string(), bounds(10.0, 10.0, 50.0, 20.0)),
                (
                    "scene_timing".to_string(),
                    bounds(50.0, 60.0, 5000.0, 5000.0),
                ),
                ("about".to_string(), bounds(f32::NAN, 0.0, 300.0, 200.0)),
                ("shortcuts".to_string(), bounds(0.0, 0.0, -10.0, 200.0)),
                (
                    "performance_row_names".to_string(),
                    bounds(0.0, 0.0, 300.0, 0.0),
                ),
            ]),
            ..Default::default()
        };

        let clamped = layout.clamp_to(1000.0, 800.0);

        assert_eq!(
            clamped.windows["tempo"],
            bounds(100.0, 0.0, 900.0, 700.0),
            "the window is kept fully on-screen and a negative y is pulled to 0"
        );
        assert_eq!(
            clamped.windows["palette"],
            bounds(10.0, 10.0, MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT),
            "a too-small window grows to the minimum size"
        );
        assert_eq!(
            clamped.windows["scene_timing"],
            bounds(0.0, 0.0, 1000.0, 800.0),
            "a window larger than the screen is clamped to the available area"
        );

        assert!(!clamped.windows.contains_key("about"), "NaN is dropped");
        assert!(
            !clamped.windows.contains_key("shortcuts"),
            "a negative width is dropped"
        );
        assert!(
            !clamped.windows.contains_key("performance_row_names"),
            "a zero height is dropped"
        );
        assert_eq!(clamped.windows.len(), 3);
    }

    #[test]
    fn record_helpers_sanitise_live_values_before_persisting() {
        let mut layout = LayoutSettings::default();

        layout.record_panels(300.0, 340.0);
        assert_eq!(
            (layout.groups_width, layout.inspector_width),
            (300.0, 340.0)
        );

        layout.record_panels(f32::NAN, -1.0);
        assert_eq!(
            (layout.groups_width, layout.inspector_width),
            (300.0, 340.0),
            "a failed geometry read never replaces a good width"
        );

        layout.record_sections(0.0, 210.0);
        assert_eq!((layout.live_height, layout.gallery_height), (0.0, 210.0));

        layout.record_sections(f32::INFINITY, -4.0);
        assert_eq!((layout.live_height, layout.gallery_height), (0.0, 210.0));

        layout.record_window("tempo", bounds(40.0, 60.0, 320.0, 200.0));
        assert_eq!(layout.windows["tempo"], bounds(40.0, 60.0, 320.0, 200.0));

        layout.record_window(
            "about",
            bounds(f32::NAN, 0.0, MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT),
        );
        assert!(
            !layout.windows.contains_key("about"),
            "an unusable reading must not be persisted"
        );

        layout.record_window("palette", bounds(5.0, 5.0, 250.0, 300.0));
        layout.record_window("palette", bounds(5.0, 5.0, 0.0, 300.0));
        assert!(
            !layout.windows.contains_key("palette"),
            "a later unusable reading forgets the saved placement"
        );
    }

    #[test]
    fn dock_width_and_tab_placements_round_trip() {
        let (directory, path) = temp_path();
        let layout = LayoutSettings {
            dock_width: 360.0,
            tabs: BTreeMap::from([
                ("inspector".to_string(), "docked".to_string()),
                ("media".to_string(), "floating".to_string()),
            ]),
            ..Default::default()
        };

        layout.write_to(&path).expect("write layout");
        assert_eq!(
            LayoutSettings::read_from(&path).expect("read layout"),
            Some(layout.clone())
        );
        assert!(!path.with_extension("json.tmp").exists());

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_partial_document_keeps_the_default_dock_and_tab_placements() {
        let (directory, path) = temp_path();
        std::fs::create_dir_all(&directory).expect("temp dir");
        std::fs::write(&path, r#"{"groups_width": 300.0}"#).expect("write partial document");

        let layout = LayoutSettings::read_from(&path)
            .expect("read partial document")
            .expect("document");
        assert_eq!(layout.dock_width, DEFAULT_DOCK_WIDTH);
        assert!(layout.tabs.is_empty(), "missing tabs mean the strip");

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn dock_width_clamping_keeps_the_centre_and_the_other_columns_usable() {
        let layout = LayoutSettings {
            groups_width: 250.0,
            inspector_width: 320.0,
            dock_width: 5000.0,
            tabs: BTreeMap::from([("audio".to_string(), "docked".to_string())]),
            ..Default::default()
        };

        let clamped = layout.clamp_to(1200.0, 800.0);
        assert_eq!(
            clamped.dock_width,
            1200.0 - MIN_GROUPS_WIDTH - MIN_INSPECTOR_WIDTH - MIN_CENTRE_WIDTH
        );
        assert_eq!(clamped.groups_width, MIN_GROUPS_WIDTH);
        assert_eq!(clamped.inspector_width, MIN_INSPECTOR_WIDTH);
        assert_eq!(
            1200.0 - clamped.groups_width - clamped.inspector_width - clamped.dock_width,
            MIN_CENTRE_WIDTH,
            "the centre column is never squeezed below its minimum"
        );

        let tiny = layout.clamp_to(700.0, 800.0);
        let centre = 700.0 - tiny.groups_width - tiny.inspector_width - tiny.dock_width;
        assert!(centre > 0.0, "a tiny window scales every column: {centre}");
        assert!(tiny.dock_width < MIN_DOCK_WIDTH);
        assert!(tiny.dock_width > 0.0, "NaN/zero is rejected by the scaling");

        let no_dock = LayoutSettings {
            dock_width: 5000.0,
            ..Default::default()
        };
        let clamped = no_dock.clamp_to(1280.0, 800.0);
        assert_eq!(
            clamped.dock_width, 1280.0,
            "a hidden dock has no centre budget to protect"
        );
    }

    #[test]
    fn unknown_tab_ids_and_placements_degrade_to_the_strip() {
        let settings = LayoutSettings {
            tabs: BTreeMap::from([
                ("inspector".to_string(), "docked".to_string()),
                ("media".to_string(), "floating".to_string()),
                ("audio".to_string(), "sideways".to_string()),
                ("future-tab".to_string(), "floating".to_string()),
            ]),
            ..Default::default()
        };

        let mut layout = LayoutState::default();
        settings.apply_to_ui(&mut layout);

        assert_eq!(
            layout.tabs.placement(SidePanelTab::Inspector),
            TabPlacement::Docked
        );
        assert_eq!(
            layout.tabs.placement(SidePanelTab::Media),
            TabPlacement::Floating
        );
        assert_eq!(
            layout.tabs.placement(SidePanelTab::Audio),
            TabPlacement::Tabs,
            "an unknown placement degrades to the tab strip"
        );
        assert_eq!(
            layout.tabs.placement(SidePanelTab::Midi),
            TabPlacement::Tabs
        );

        let written = LayoutSettings::from_ui(&layout);
        assert_eq!(
            written.tabs.len(),
            2,
            "unknown ids/placements are not written back"
        );
        assert!(written.tabs.contains_key("inspector"));
        assert!(written.tabs.contains_key("media"));
    }

    #[test]
    fn the_ui_layout_round_trips_through_the_settings_document() {
        let mut ui_layout = LayoutState {
            groups_width: 280.0,
            inspector_width: 340.0,
            live_height: 210.0,
            gallery_height: 160.0,
            dock_width: 350.0,
            ..Default::default()
        };
        ui_layout
            .tabs
            .set_placement(SidePanelTab::Inspector, TabPlacement::Docked);
        ui_layout
            .tabs
            .set_placement(SidePanelTab::Midi, TabPlacement::Floating);
        ui_layout.record_window_rect(
            "tempo",
            egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(320.0, 200.0)),
        );

        let settings = LayoutSettings::from_ui(&ui_layout);
        assert_eq!(settings.dock_width, 350.0);
        assert_eq!(settings.live_height, 210.0);
        assert_eq!(
            settings.tabs.get("inspector").map(String::as_str),
            Some("docked")
        );
        assert_eq!(
            settings.windows.get("tempo"),
            Some(&bounds(10.0, 20.0, 320.0, 200.0))
        );

        let mut restored = LayoutState::default();
        settings.apply_to_ui(&mut restored);
        assert_eq!(restored, ui_layout);
    }

    #[test]
    fn a_write_never_persists_non_finite_geometry() {
        let (directory, path) = temp_path();
        let layout = LayoutSettings {
            groups_width: f32::NAN,
            windows: BTreeMap::from([(
                "tempo".to_string(),
                bounds(f32::NAN, f32::NAN, f32::NAN, f32::NAN),
            )]),
            ..Default::default()
        };

        layout.write_to(&path).expect("write layout");
        let loaded = LayoutSettings::read_from(&path)
            .expect("the written document must stay readable")
            .expect("document");
        assert_eq!(loaded.groups_width, DEFAULT_GROUPS_WIDTH);
        assert!(loaded.windows.is_empty());

        std::fs::remove_dir_all(directory).ok();
    }
}
