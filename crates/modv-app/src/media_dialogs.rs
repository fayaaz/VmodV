//! Native file-dialog plumbing: document detection, preset/playlist IO and
//! the seam between a picked path and the engine.
//!
//! Electron's renderer owns the dialogs and the worker does the file IO
//! (`loadPreset` in `index.worker.js`, `export-scenes-playlist` /
//! `import-scenes-playlist` in `window-prefs.js`). The native shell keeps the
//! same document formats but does the IO in-process:
//!
//! - opening auto-detects a preset, a Scene document or a Scenes playlist,
//!   matching `preset-serializer.js`'s `isSceneDocument`/`isScenesPlaylist`;
//! - saving writes the engine's canonical preset document;
//! - playlist export/import use `scenes/exportPlaylist`/`scenes/importPlaylist`.
//!
//! [`FilePicker`] is the test seam: the menu and the in-app picker only
//! produce a path, and a picker implementation applies it. [`NativeApp`]
//! implements it for real; unit tests record the calls.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::store_ops::Store;
use modv_ui::FileDialogAction;

/// A document the File menu can open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    Preset,
    Scene,
    Playlist,
}

impl DocumentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preset => "preset",
            Self::Scene => "Scene",
            Self::Playlist => "Scenes playlist",
        }
    }
}

/// Auto-detect a document exactly like `preset-serializer.js`.
///
/// A playlist is checked first: it has no `id`/`name`, so it would otherwise
/// classify as a preset. A Scene is a preset plus `id`, `name` and a
/// `groups`/`layers` array.
pub fn classify_document(value: &Value) -> DocumentKind {
    if value.get("type").and_then(Value::as_str) == Some("modv-scenes-playlist")
        && value.get("scenes").is_some_and(Value::is_array)
    {
        return DocumentKind::Playlist;
    }
    let scene = value
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.is_empty())
        && value.get("name").and_then(Value::as_str).is_some()
        && (value.get("groups").is_some_and(Value::is_array)
            || value.get("layers").is_some_and(Value::is_array));
    if scene {
        DocumentKind::Scene
    } else {
        DocumentKind::Preset
    }
}

/// Read and parse a JSON document, with a path that explains the failure.
pub fn read_json(path: &Path) -> Result<Value, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))
}

/// Validate a path chosen for a JSON document action.
pub fn validate_json_path(path: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return Err(format!("{} is not a .json document", path.display()));
    }
    Ok(())
}

/// Open `path` into `store`, choosing the action from the document shape.
pub fn open_document(store: &mut Store, path: &Path) -> Result<DocumentKind, String> {
    validate_json_path(path)?;
    let document = read_json(path)?;
    let kind = classify_document(&document);
    match kind {
        DocumentKind::Playlist => {
            store
                .dispatch(
                    "scenes/importPlaylist",
                    serde_json::json!({ "playlist": document }),
                )
                .map_err(|error| format!("could not import the playlist: {error}"))?;
        }
        DocumentKind::Scene => {
            store
                .dispatch(
                    "scenes/loadSceneFromJson",
                    serde_json::json!({ "scene": document }),
                )
                .map_err(|error| format!("could not load the Scene: {error}"))?;
        }
        DocumentKind::Preset => store
            .restore(&document)
            .map_err(|error| format!("could not load the preset: {error}"))?,
    }
    Ok(kind)
}

/// Write the engine's canonical preset document to `path`.
pub fn save_preset_file(path: &Path, store: &Store) -> Result<(), String> {
    let document = store.snapshot();
    write_json(path, &document)
}

/// Export the Scenes playlist to `path` (the engine owns the document shape).
pub fn export_playlist_file(path: &Path, store: &mut Store) -> Result<(), String> {
    let playlist = store
        .dispatch("scenes/exportPlaylist", serde_json::json!({}))
        .map_err(|error| format!("could not export the playlist: {error}"))?;
    write_json(path, &playlist)
}

/// Import a Scenes playlist from `path`.
pub fn import_playlist_file(path: &Path, store: &mut Store) -> Result<(), String> {
    validate_json_path(path)?;
    let playlist = read_json(path)?;
    store
        .dispatch(
            "scenes/importPlaylist",
            serde_json::json!({ "playlist": playlist }),
        )
        .map_err(|error| format!("could not import the playlist: {error}"))?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("could not serialise the document: {error}"))?;
    std::fs::write(path, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// The platform command that opens `path` in the file manager.
pub fn open_external_command(path: &Path) -> (String, Vec<OsString>) {
    #[cfg(target_os = "macos")]
    {
        ("open".to_string(), vec![path.as_os_str().to_os_string()])
    }
    #[cfg(target_os = "windows")]
    {
        (
            "explorer".to_string(),
            vec![path.as_os_str().to_os_string()],
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        (
            "xdg-open".to_string(),
            vec![path.as_os_str().to_os_string()],
        )
    }
}

/// Open a folder in the platform file manager without waiting for it.
pub fn open_external(path: &Path) -> Result<(), String> {
    let (program, args) = open_external_command(path);
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open {}: {error}", path.display()))
}

/// `<userData>/logs`, where both front ends write their daily log files.
pub fn logs_dir(user_data: &Path) -> PathBuf {
    user_data.join("logs")
}

/// The seam between a picked path and the engine.
///
/// The native shell implements this for [`crate::native::NativeApp`]; tests use
/// a recording implementation, so menu wiring can be asserted without a real
/// dialog or file system.
pub trait FilePicker {
    /// Load `path`, reporting which document kind the engine chose.
    fn open_document(&mut self, path: &Path) -> Result<DocumentKind, String>;
    fn save_preset_as(&mut self, path: &Path) -> Result<(), String>;
    fn export_playlist(&mut self, path: &Path) -> Result<(), String>;
    fn import_playlist(&mut self, path: &Path) -> Result<(), String>;
    fn add_isf_folder(&mut self, path: &Path) -> Result<(), String>;
}

/// Apply the confirmed action to the picker's engine.
pub fn apply_file_action(
    picker: &mut dyn FilePicker,
    action: FileDialogAction,
    path: &Path,
) -> Result<(), String> {
    match action {
        FileDialogAction::OpenPreset | FileDialogAction::OpenScene => {
            picker.open_document(path).map(|_| ())
        }
        FileDialogAction::SavePresetAs => picker.save_preset_as(path),
        FileDialogAction::ExportPlaylist => picker.export_playlist(path),
        FileDialogAction::ImportPlaylist => picker.import_playlist(path),
        FileDialogAction::AddIsfFolder => picker.add_isf_folder(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn scene_document(id: &str, name: &str) -> Value {
        serde_json::json!({
            "id": id,
            "name": name,
            "enabled": true,
            "groups": [],
        })
    }

    #[test]
    fn document_detection_matches_the_js_serializer() {
        assert_eq!(
            classify_document(&serde_json::json!({
                "type": "modv-scenes-playlist",
                "scenes": [],
            })),
            DocumentKind::Playlist
        );
        assert_eq!(
            classify_document(&serde_json::json!({
                "type": "modv-scenes-playlist",
                "scenes": {},
            })),
            DocumentKind::Preset,
            "a playlist type without a scenes array is not a playlist"
        );
        assert_eq!(
            classify_document(&scene_document("s1", "Scene")),
            DocumentKind::Scene
        );
        assert_eq!(
            classify_document(&serde_json::json!({"id": "s1", "name": "Scene"})),
            DocumentKind::Preset,
            "a Scene needs its groups/layers array"
        );
        assert_eq!(
            classify_document(&serde_json::json!({"groups": [], "modules": {}})),
            DocumentKind::Preset
        );
        assert_eq!(
            classify_document(&serde_json::json!({"id": "", "name": "x", "layers": []})),
            DocumentKind::Preset,
            "an empty id is not a Scene id"
        );
    }

    #[test]
    fn opening_detects_and_loads_a_preset_scene_and_playlist() {
        let root = temp_dir("modv-open-docs");

        // A preset document.
        let mut source = Store::new();
        source
            .dispatch("groups/createGroup", serde_json::json!({"name": "Opened"}))
            .expect("group");
        let preset_path = root.join("preset.json");
        save_preset_file(&preset_path, &source).expect("save preset");
        let mut store = Store::new();
        assert_eq!(
            open_document(&mut store, &preset_path),
            Ok(DocumentKind::Preset)
        );
        assert_eq!(store.state().groups[0].name, "Opened");

        // A Scene document is upserted into the Scene list and applied, like
        // the JS preset/Scene open path (`scenes.js:2048`, `apply` defaults to
        // true).
        let scene_path = root.join("scene.json");
        std::fs::write(
            &scene_path,
            serde_json::to_vec(&scene_document("scene-opened", "Opened Scene")).expect("json"),
        )
        .expect("write scene");
        assert_eq!(
            open_document(&mut store, &scene_path),
            Ok(DocumentKind::Scene)
        );
        assert!(
            store
                .scenes_state()
                .scenes
                .iter()
                .any(|scene| scene.get("id").and_then(Value::as_str) == Some("scene-opened")),
            "the Scene is added to the authoritative list"
        );
        assert_eq!(
            store.scenes_state().performance["currentSceneId"],
            serde_json::json!("scene-opened"),
            "opening a Scene JSON applies it"
        );

        // A playlist document.
        let playlist_path = root.join("playlist.json");
        export_playlist_file(&playlist_path, &mut store).expect("export");
        let mut imported = Store::new();
        assert_eq!(
            open_document(&mut imported, &playlist_path),
            Ok(DocumentKind::Playlist)
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn playlist_export_and_import_round_trip() {
        let root = temp_dir("modv-playlist");
        let mut source = Store::new();
        source
            .dispatch("scenes/ensureDefaultScenes", serde_json::json!({}))
            .expect("seed");
        let expected = source.scenes_state().scenes.len();
        let path = root.join("nested/scenes-playlist.json");
        export_playlist_file(&path, &mut source).expect("export");

        let mut destination = Store::new();
        import_playlist_file(&path, &mut destination).expect("import");
        assert_eq!(destination.scenes_state().scenes.len(), expected);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn open_validates_the_path_and_json() {
        let root = temp_dir("modv-open-invalid");
        let mut store = Store::new();

        let missing = root.join("missing.json");
        assert!(
            open_document(&mut store, &missing)
                .expect_err("missing")
                .contains("could not open")
        );

        let text = root.join("preset.txt");
        std::fs::write(&text, "{}").expect("write");
        assert!(
            open_document(&mut store, &text)
                .expect_err("extension")
                .contains("not a .json")
        );

        let broken = root.join("broken.json");
        std::fs::write(&broken, "{ nope").expect("write");
        assert!(
            open_document(&mut store, &broken)
                .expect_err("json")
                .contains("not valid JSON")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn open_external_uses_the_platform_file_manager() {
        let (program, args) = open_external_command(Path::new("/tmp/media"));
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(program, "xdg-open");
        assert_eq!(args, vec![OsString::from("/tmp/media")]);
    }

    #[derive(Default)]
    struct RecordingPicker {
        calls: Vec<(String, PathBuf)>,
    }

    impl FilePicker for RecordingPicker {
        fn open_document(&mut self, path: &Path) -> Result<DocumentKind, String> {
            self.calls.push(("open".to_string(), path.to_path_buf()));
            Ok(DocumentKind::Preset)
        }

        fn save_preset_as(&mut self, path: &Path) -> Result<(), String> {
            self.calls.push(("save".to_string(), path.to_path_buf()));
            Ok(())
        }

        fn export_playlist(&mut self, path: &Path) -> Result<(), String> {
            self.calls.push(("export".to_string(), path.to_path_buf()));
            Ok(())
        }

        fn import_playlist(&mut self, path: &Path) -> Result<(), String> {
            self.calls.push(("import".to_string(), path.to_path_buf()));
            Ok(())
        }

        fn add_isf_folder(&mut self, path: &Path) -> Result<(), String> {
            self.calls.push(("isf".to_string(), path.to_path_buf()));
            Ok(())
        }
    }

    #[test]
    fn each_confirmed_action_reaches_the_picker_once() {
        let mut picker = RecordingPicker::default();
        let path = Path::new("/tmp/picked.json");
        for action in FileDialogAction::ALL {
            apply_file_action(&mut picker, action, path).expect("apply");
        }

        assert_eq!(
            picker.calls,
            vec![
                ("open".to_string(), path.to_path_buf()),
                ("open".to_string(), path.to_path_buf()),
                ("import".to_string(), path.to_path_buf()),
                ("save".to_string(), path.to_path_buf()),
                ("export".to_string(), path.to_path_buf()),
                ("isf".to_string(), path.to_path_buf()),
            ],
            "Open preset/Open Scene both auto-detect; the other actions map 1:1"
        );
    }
}
