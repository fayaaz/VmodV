//! Window placement persistence, ported from `src/main/window-state.js`.
//!
//! The native shell writes the same `window-state.json` document as the
//! Electron app, so switching between the two builds keeps the main window (and
//! output windows) where the user left them. Bounds are logical pixels, the
//! same unit Electron's `BrowserWindow` uses.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The document version written by both apps.
pub const WINDOW_STATE_VERSION: u32 = 1;
/// The file name under `<userData>`.
pub const WINDOW_STATE_FILENAME: &str = "window-state.json";

/// One saved window placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(rename = "displayId", default, skip_serializing_if = "Option::is_none")]
    pub display_id: Option<u64>,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

impl SavedWindow {
    /// A usable placement has finite numbers and a positive size.
    pub fn is_usable(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }

    /// Round the stored bounds the way the JS `sanitiseBounds` does.
    pub fn sanitised(&self) -> Option<Self> {
        if !self.is_usable() {
            return None;
        }
        Some(Self {
            x: self.x.round(),
            y: self.y.round(),
            width: self.width.round(),
            height: self.height.round(),
            display_id: self.display_id,
            maximized: self.maximized,
            fullscreen: self.fullscreen,
        })
    }
}

/// The persisted placement document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub version: u32,
    #[serde(rename = "mainWindow", default)]
    pub main_window: Option<SavedWindow>,
    #[serde(default)]
    pub windows: BTreeMap<String, SavedWindow>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            version: WINDOW_STATE_VERSION,
            main_window: None,
            windows: BTreeMap::new(),
        }
    }
}

impl WindowState {
    /// Read a state document, returning the default on any error or mismatch.
    pub fn read(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Self::default();
        };
        Self::normalize(&value)
    }

    /// Normalise a parsed document, dropping unusable entries.
    pub fn normalize(value: &serde_json::Value) -> Self {
        if value.get("version").and_then(serde_json::Value::as_u64)
            != Some(u64::from(WINDOW_STATE_VERSION))
        {
            return Self::default();
        }
        let mut state = Self::default();
        state.main_window = value
            .get("mainWindow")
            .cloned()
            .and_then(|value| serde_json::from_value::<SavedWindow>(value).ok())
            .and_then(|window| window.sanitised());
        if let Some(windows) = value.get("windows").and_then(serde_json::Value::as_object) {
            for (key, saved) in windows {
                if let Ok(saved) = serde_json::from_value::<SavedWindow>(saved.clone())
                    && let Some(saved) = saved.sanitised()
                {
                    state.windows.insert(key.clone(), saved);
                }
            }
        }
        state
    }

    /// Write the document atomically (`tmp` + rename), returning an error on
    /// failure. Callers treat persistence as best-effort.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(&temporary, text)?;
        fs::rename(&temporary, path)?;
        Ok(())
    }

    /// The saved output-window placement, if any.
    pub fn output(&self) -> Option<&SavedWindow> {
        self.windows.get("output-0")
    }

    pub fn set_output(&mut self, window: SavedWindow) {
        self.windows.insert("output-0".to_string(), window);
    }
}

/// The userData directory shared with the Electron app.
pub fn user_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("MODV_USER_DATA") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(path).join("VmodV");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join("VmodV");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("VmodV");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("VmodV");
        }
    }
    PathBuf::from(".")
}

/// Clamp `bounds` into `area`, shrinking it when it is larger.
pub fn clamp_bounds_to_area(bounds: SavedWindow, area: SavedWindow) -> SavedWindow {
    let width = bounds.width.min(area.width);
    let height = bounds.height.min(area.height);
    SavedWindow {
        x: bounds.x.max(area.x).min(area.x + area.width - width),
        y: bounds.y.max(area.y).min(area.y + area.height - height),
        width,
        height,
        display_id: bounds.display_id,
        maximized: bounds.maximized,
        fullscreen: bounds.fullscreen,
    }
}

/// Whether `bounds` lies inside `area` (1 px tolerance).
pub fn fits_within(bounds: SavedWindow, area: SavedWindow) -> bool {
    const TOLERANCE: f64 = 1.0;
    bounds.x >= area.x - TOLERANCE
        && bounds.y >= area.y - TOLERANCE
        && bounds.x + bounds.width <= area.x + area.width + TOLERANCE
        && bounds.y + bounds.height <= area.y + area.height + TOLERANCE
}

/// The overlap area of two rectangles (0 when they do not intersect).
pub fn intersection_area(a: SavedWindow, b: SavedWindow) -> f64 {
    let width = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let height = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    if width <= 0.0 || height <= 0.0 {
        0.0
    } else {
        width * height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(x: f64, y: f64, width: f64, height: f64) -> SavedWindow {
        SavedWindow {
            x,
            y,
            width,
            height,
            ..Default::default()
        }
    }

    #[test]
    fn normalise_rejects_wrong_versions_and_bad_bounds() {
        let state = WindowState::normalize(&serde_json::json!({
            "version": 2,
            "mainWindow": {"x": 0, "y": 0, "width": 100, "height": 100}
        }));
        assert_eq!(state, WindowState::default());

        let state = WindowState::normalize(&serde_json::json!({
            "version": 1,
            "mainWindow": {"x": 0, "y": 0, "width": 0, "height": 100},
            "windows": {
                "output-0": {"x": 5, "y": 6, "width": 300, "height": 200},
                "bad": {"x": 0, "y": 0, "width": -1, "height": 10}
            }
        }));
        assert!(state.main_window.is_none());
        assert!(state.windows.contains_key("output-0"));
        assert!(!state.windows.contains_key("bad"));
    }

    #[test]
    fn bounds_clamp_into_the_display_area() {
        let area = window(0.0, 0.0, 1920.0, 1080.0);
        let clamped = clamp_bounds_to_area(window(1900.0, 1070.0, 800.0, 600.0), area);
        assert_eq!(clamped.x, 1120.0);
        assert_eq!(clamped.y, 480.0);
        assert_eq!(clamped.width, 800.0);
        assert!(fits_within(clamped, area));
    }

    #[test]
    fn intersection_area_is_zero_for_disjoint_rects() {
        let a = window(0.0, 0.0, 100.0, 100.0);
        let b = window(200.0, 200.0, 100.0, 100.0);
        assert_eq!(intersection_area(a, b), 0.0);
        assert_eq!(intersection_area(a, window(50.0, 50.0, 100.0, 100.0)), 2500.0);
    }

    #[test]
    fn write_and_read_round_trips_through_an_atomic_file() {
        let dir = std::env::temp_dir().join(format!(
            "modv-window-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let path = dir.join(WINDOW_STATE_FILENAME);
        let mut state = WindowState::default();
        state.main_window = Some(window(12.0, 34.0, 1280.0, 720.0));
        state.set_output(window(0.0, 0.0, 300.0, 300.0));
        state.write(&path).expect("write state");
        let loaded = WindowState::read(&path);
        assert_eq!(loaded, state);
        let _ = fs::remove_dir_all(dir);
    }
}
