//! The persisted "internal rendering resolution" for the native shell.
//!
//! Every module, Group target, canvas and scene in the compositor renders at
//! full output size, which is correct for a GPU with headroom but wasteful
//! for the raymarched corpus (see `resources/isf/Back The Way You Came.fs`).
//! The user-facing escape hatch is a single scale factor applied to the
//! compositor's internal targets: the window still presents at full size
//! (the present pass samples the main texture with bilinear filtering), the
//! image just gets a little softer, and the frame rate recovers the pixel
//! count.
//!
//! Everything here is pure or filesystem-simple so it is unit-testable:
//! no GPU, no window, no egui. The JSON document lives in the shared userData
//! directory as `<userData>/native-render.json` and is written atomically
//! (tmp + rename), like [`crate::audio_settings`].

use std::path::Path;

/// The persisted render-scale choice inside the shared userData directory.
pub const RENDER_SETTINGS_FILENAME: &str = "native-render.json";

/// The smallest admitted scale. Below a quarter of the output resolution the
/// scenes stop reading as 3D scenes at all, so it is not offered.
pub const MIN_SCALE: f32 = 0.25;
/// The largest admitted scale: full output resolution (the default).
pub const MAX_SCALE: f32 = 1.0;

/// One persisted internal rendering resolution.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RenderSettings {
    /// Linear scale of the internal targets (width and height).
    pub scale: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self { scale: MAX_SCALE }
    }
}

impl RenderSettings {
    /// Build a settings document for `scale`, clamped to [`MIN_SCALE`]..=[`MAX_SCALE`].
    pub fn new(scale: f32) -> Self {
        Self {
            scale: Self::clamp_scale(scale),
        }
    }

    /// Clamp a proposed scale into the admitted range; non-finite values
    /// (NaN from a corrupt UI, infinities) fall back to full resolution.
    pub fn clamp_scale(scale: f32) -> f32 {
        if !scale.is_finite() {
            MAX_SCALE
        } else {
            scale.clamp(MIN_SCALE, MAX_SCALE)
        }
    }

    /// Read the document, treating a missing file as "nothing saved".
    pub fn read_from(path: &Path) -> Result<Option<Self>, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("could not read {}: {error}", path.display()));
            }
        };
        let settings: Self = serde_json::from_str(&text)
            .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))?;
        Ok(Some(Self::new(settings.scale)))
    }

    /// Write the document atomically, creating the userData directory.
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let temporary = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("could not serialise render settings: {error}"))?;
        std::fs::write(&temporary, text)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "modv-render-settings-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn a_missing_file_reads_as_nothing_saved() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert_eq!(RenderSettings::read_from(&path), Ok(None));
    }

    #[test]
    fn a_written_document_round_trips() {
        let path = temp_path("round-trip");
        let _ = std::fs::remove_file(&path);
        RenderSettings::new(0.5)
            .write_to(&path)
            .expect("the document writes");
        let loaded = RenderSettings::read_from(&path)
            .expect("the document reads")
            .expect("the document exists");
        assert_eq!(loaded.scale, 0.5);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_out_of_range_scale_is_clamped_on_load() {
        let path = temp_path("clamped");
        std::fs::write(&path, r#"{"scale": 4.0}"#).expect("write the raw document");
        let loaded = RenderSettings::read_from(&path)
            .expect("the document reads")
            .expect("the document exists");
        assert_eq!(loaded.scale, MAX_SCALE);
        std::fs::write(&path, r#"{"scale": 0.01}"#).expect("write the raw document");
        let loaded = RenderSettings::read_from(&path)
            .expect("the document reads")
            .expect("the document exists");
        assert_eq!(loaded.scale, MIN_SCALE);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_json_is_an_error_naming_the_path() {
        let path = temp_path("invalid");
        std::fs::write(&path, "not json").expect("write the raw document");
        let error = RenderSettings::read_from(&path).expect_err("invalid JSON must error");
        assert!(
            error.contains(&path.display().to_string()),
            "the error names the file: {error}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_non_finite_scale_falls_back_to_full_resolution() {
        assert_eq!(RenderSettings::clamp_scale(f32::NAN), MAX_SCALE);
        assert_eq!(RenderSettings::clamp_scale(f32::INFINITY), MAX_SCALE);
        assert_eq!(RenderSettings::clamp_scale(-1.0), MIN_SCALE);
        assert_eq!(RenderSettings::clamp_scale(0.75), 0.75);
    }

    #[test]
    fn the_default_is_full_resolution() {
        assert_eq!(RenderSettings::default().scale, MAX_SCALE);
    }
}
