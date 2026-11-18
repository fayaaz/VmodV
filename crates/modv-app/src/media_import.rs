//! One-time import of loose Scene documents from the active media profile.
//!
//! Electron's media manager lists `scene/` files through `window-prefs.js`,
//! while the native shell keeps `<userData>/scenes.json` authoritative. A user
//! who saved Scenes before adopting the native shell therefore has documents
//! under `<userData>/media/<profile>/scene/` that the native Scene grid never
//! showed. This module imports them **once per profile**:
//!
//! - `scene/` and `preset/` are scanned recursively in a deterministic order
//!   (folders in that order, entries case-insensitively sorted); hidden,
//!   temporary, symlinked-directory and oversized files are ignored;
//! - a Scene whose id already exists is skipped, because Scenes are shared and
//!   `scenes.json` stays authoritative; a new id whose *name* collides
//!   (case-insensitive, trimmed) with an existing Scene is skipped too, keeping
//!   the existing Scene and reporting the file;
//! - plain presets have no index: they are validated and counted so the log can
//!   say they remain openable through File → Open preset…;
//! - playlists are reported once and not imported;
//! - [`ProjectSettings`] records `imported_documents: { "<profile>": true }` in
//!   `<userData>/native-project.json`, so later launches and profile switches
//!   do not re-read the folders. Adding files later therefore never silently
//!   duplicates a Scene.
//!
//! The originals are never moved, rewritten or deleted. To deliberately pick
//! up files added later, remove the profile's entry from `native-project.json`;
//! the id/name checks above still prevent duplicate Scenes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use modv_engine::default_scenes::scene_key;

use crate::media_dialogs::{DocumentKind, classify_document};
use crate::media_profiles::ProjectSettings;
use crate::store_ops::Store;

/// The largest document read from the media tree.
///
/// A Scene is a preset snapshot; 4 MiB is far above a realistic document while
/// keeping a stray multi-gigabyte file from being read into memory.
pub const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

/// How deep the recursive scan descends below a profile folder.
pub const MAX_SCAN_DEPTH: usize = 8;

/// The most documents one profile import examines.
pub const MAX_DOCUMENTS: usize = 4_096;

/// The profile folders scanned for loose documents, in import order.
const SCAN_FOLDERS: [&str; 2] = ["scene", "preset"];

/// What one profile import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Names (or ids for an unnamed Scene) of the Scenes added to the list.
    pub imported: Vec<String>,
    /// Documents whose Scene id already exists in `scenes.json`.
    pub skipped_existing: usize,
    /// New-id documents whose Scene name collides with an existing Scene.
    pub name_collisions: Vec<PathBuf>,
    /// Unreadable, oversized, malformed or non-Scene-normalisable documents.
    pub skipped_invalid: usize,
    /// Playlist documents that were intentionally not imported.
    pub playlists: usize,
    /// Valid preset documents left for File → Open preset….
    pub presets: usize,
    /// The folder held more documents than [`MAX_DOCUMENTS`].
    pub truncated: bool,
}

impl ImportReport {
    /// Every document that was not added (id/name/invalid).
    pub fn skipped(&self) -> usize {
        self.skipped_existing + self.name_collisions.len() + self.skipped_invalid
    }

    /// The one-line summary the startup log prints.
    pub fn summary(&self) -> String {
        let mut text = format!(
            "imported {} Scene(s), skipped {} existing/invalid",
            self.imported.len(),
            self.skipped()
        );
        if self.presets > 0 {
            text.push_str(&format!(
                "; {} preset document(s) remain openable through File → Open preset…",
                self.presets
            ));
        }
        if self.playlists > 0 {
            text.push_str(&format!(
                "; {} playlist document(s) not imported",
                self.playlists
            ));
        }
        if self.truncated {
            text.push_str(&format!("; scan capped at {MAX_DOCUMENTS} documents"));
        }
        text
    }
}

/// Import the profile's loose documents unless it was already done.
///
/// Returns `None` when `<userData>/native-project.json` already records the
/// profile. Otherwise the folders are scanned, the flag is persisted and the
/// concise summary (plus one line per name collision) is logged.
pub fn import_profile_documents_once(
    store: &mut Store,
    media_root: &Path,
    profile: &str,
    settings: &mut ProjectSettings,
    settings_path: &Path,
) -> Option<ImportReport> {
    if settings.documents_imported(profile) {
        return None;
    }

    let report = import_profile_documents(store, media_root, profile);
    settings.mark_documents_imported(profile);
    if let Err(error) = settings.write_to(settings_path) {
        eprintln!(
            "ui: WARN could not record the {profile:?} document import in {}: {error}",
            settings_path.display()
        );
    }
    for path in &report.name_collisions {
        eprintln!(
            "ui: WARN media document import skipped {}: a Scene with that name already exists",
            path.display()
        );
    }
    eprintln!(
        "ui: INFO media document import for profile {profile:?}: {}",
        report.summary()
    );
    Some(report)
}

/// Scan the profile's loose documents and add every new Scene to the list.
///
/// Unlike [`import_profile_documents_once`] this always scans; the id/name
/// checks make it idempotent, so a lost settings flag cannot duplicate a Scene.
pub fn import_profile_documents(
    store: &mut Store,
    media_root: &Path,
    profile: &str,
) -> ImportReport {
    let mut report = ImportReport::default();
    let mut existing_ids: BTreeSet<String> = BTreeSet::new();
    let mut existing_names: BTreeSet<String> = BTreeSet::new();
    for scene in &store.scenes_state().scenes {
        if let Some(id) = scene.get("id").and_then(Value::as_str) {
            existing_ids.insert(id.to_string());
        }
        if let Some(name) = scene.get("name").and_then(Value::as_str) {
            let key = scene_key(name);
            if !key.is_empty() {
                existing_names.insert(key);
            }
        }
    }

    let mut documents = Vec::new();
    for folder in SCAN_FOLDERS {
        let root = crate::media_profiles::profile_folder(media_root, profile, folder);
        walk_documents(&root, 0, &mut documents, &mut report);
    }

    let mut imported_any = false;
    for path in documents {
        let Some(document) = read_document(&path, &mut report) else {
            continue;
        };
        match classify_document(&document) {
            DocumentKind::Playlist => report.playlists += 1,
            DocumentKind::Preset => report.presets += 1,
            DocumentKind::Scene => {
                let Some(scene) = modv_engine::scenes_playlist::normalize_scene_document(&document)
                else {
                    report.skipped_invalid += 1;
                    continue;
                };
                let Some(id) = scene
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .filter(|id| !id.is_empty())
                else {
                    report.skipped_invalid += 1;
                    continue;
                };
                if existing_ids.contains(&id) {
                    report.skipped_existing += 1;
                    continue;
                }
                let name = scene
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string();
                let key = scene_key(&name);
                if !key.is_empty() && existing_names.contains(&key) {
                    report.name_collisions.push(path.clone());
                    continue;
                }
                match store.dispatch(
                    "scenes/loadSceneFromJson",
                    serde_json::json!({ "scene": scene, "persist": false }),
                ) {
                    Ok(_) => {
                        imported_any = true;
                        report.imported.push(if name.is_empty() {
                            id.clone()
                        } else {
                            name.clone()
                        });
                        existing_ids.insert(id);
                        if !key.is_empty() {
                            existing_names.insert(key);
                        }
                    }
                    Err(error) => {
                        report.skipped_invalid += 1;
                        eprintln!(
                            "ui: WARN media document import could not load {}: {error}",
                            path.display()
                        );
                    }
                }
            }
        }
    }

    if imported_any && let Err(error) = store.persist_scenes() {
        eprintln!("ui: WARN could not persist the imported Scenes: {error}");
    }
    report
}

/// Walk one folder depth-first in sorted order, collecting `.json` documents.
fn walk_documents(
    dir: &Path,
    depth: usize,
    documents: &mut Vec<PathBuf>,
    report: &mut ImportReport,
) {
    if depth > MAX_SCAN_DEPTH || report.truncated {
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        // A missing profile folder is the normal empty case.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            report.skipped_invalid += 1;
            eprintln!(
                "ui: WARN media document import could not read {}: {error}",
                dir.display()
            );
            return;
        }
    };
    let mut entries: Vec<std::fs::DirEntry> = read.flatten().collect();
    entries.sort_by(|left, right| {
        let left = left.file_name().to_string_lossy().to_lowercase();
        let right = right.file_name().to_string_lossy().to_lowercase();
        left.cmp(&right)
    });

    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if crate::media_watch::is_ignored_media_name(&name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            // Symlinked directories could loop back up the tree.
            if !file_type.is_symlink() {
                walk_documents(&entry.path(), depth + 1, documents, report);
            }
            continue;
        }
        if !file_type.is_file()
            || !Path::new(&name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        if documents.len() >= MAX_DOCUMENTS {
            report.truncated = true;
            return;
        }
        documents.push(entry.path());
    }
}

/// Read one candidate document, counting an unreadable/oversized file.
fn read_document(path: &Path, report: &mut ImportReport) -> Option<Value> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            report.skipped_invalid += 1;
            eprintln!(
                "ui: WARN media document import could not open {}: {error}",
                path.display()
            );
            return None;
        }
    };
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        report.skipped_invalid += 1;
        return None;
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            report.skipped_invalid += 1;
            eprintln!(
                "ui: WARN media document import could not read {}: {error}",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(_) => {
            report.skipped_invalid += 1;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_profiles::{PROJECT_SETTINGS_FILENAME, create_profile};
    use serde_json::json;

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
        json!({
            "id": id,
            "name": name,
            "enabled": true,
            "groups": [],
        })
    }

    fn write_json(path: &Path, value: &Value) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("document parent");
        }
        std::fs::write(path, serde_json::to_vec(value).expect("json")).expect("write document");
    }

    struct Fixture {
        root: PathBuf,
        user_data: PathBuf,
        media_root: PathBuf,
        settings_path: PathBuf,
        store: Store,
        settings: ProjectSettings,
    }

    /// A `default` profile with the full mix of loose documents.
    fn fixture() -> Fixture {
        let root = temp_dir("modv-media-import");
        let user_data = root.join("user-data");
        let media_root = user_data.join("media");
        create_profile(&media_root, "default").expect("profile");
        let scene_dir = media_root.join("default/scene");
        let preset_dir = media_root.join("default/preset");

        write_json(
            &scene_dir.join("new.json"),
            &scene_document("scene-imported", "Legacy New"),
        );
        write_json(
            &scene_dir.join("existing-id.json"),
            &scene_document("scene-existing", "Existing Copy"),
        );
        write_json(
            &scene_dir.join("collide.json"),
            &scene_document("scene-collide", "  existing "),
        );
        std::fs::write(scene_dir.join("broken.json"), "{ nope").expect("broken json");
        write_json(
            &scene_dir.join("plain-preset.json"),
            &json!({ "groups": [], "modules": {} }),
        );
        write_json(
            &scene_dir.join("playlist.json"),
            &json!({ "type": "modv-scenes-playlist", "scenes": [] }),
        );
        write_json(
            &scene_dir.join(".hidden.json"),
            &scene_document("scene-hidden", "Hidden"),
        );
        write_json(
            &scene_dir.join("new.json.tmp"),
            &scene_document("scene-temp", "Temporary"),
        );
        write_json(
            &scene_dir.join("notes.json~"),
            &scene_document("scene-tilde", "Editor Backup"),
        );
        write_json(
            &scene_dir.join("nested/deeper/third.json"),
            &scene_document("scene-nested", "Nested Legacy"),
        );
        std::fs::write(
            scene_dir.join("oversized.json"),
            format!("{}{{}}", " ".repeat(MAX_DOCUMENT_BYTES as usize)),
        )
        .expect("oversized json");

        write_json(
            &preset_dir.join("another-preset.json"),
            &json!({ "groups": [], "modules": {} }),
        );
        write_json(
            &preset_dir.join("duplicate-scene-id.json"),
            &scene_document("scene-imported", "Duplicate Id"),
        );

        let settings_path = user_data.join(PROJECT_SETTINGS_FILENAME);
        let settings = ProjectSettings {
            profile: "default".to_string(),
            ..Default::default()
        };
        settings.write_to(&settings_path).expect("settings");

        // The authoritative list already holds one Scene. The import must not
        // duplicate it by id, and must keep it when a new id reuses the name.
        let mut store = Store::with_state_path(Some(&user_data.join("engine-state.json")));
        store
            .dispatch(
                "scenes/loadSceneFromJson",
                json!({ "scene": scene_document("scene-existing", "Existing") }),
            )
            .expect("existing scene");

        Fixture {
            root,
            user_data,
            media_root,
            settings_path,
            store,
            settings,
        }
    }

    fn scene_ids(store: &Store) -> Vec<String> {
        store
            .scenes_state()
            .scenes
            .iter()
            .filter_map(|scene| scene.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    fn scene_names(store: &Store) -> Vec<String> {
        store
            .scenes_state()
            .scenes
            .iter()
            .filter_map(|scene| scene.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.is_file() {
                    files.push((path.clone(), std::fs::read(&path).expect("read original")));
                }
            }
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }

    #[test]
    fn imports_only_new_loose_scenes_and_leaves_the_originals() {
        let mut fixture = fixture();
        let before = snapshot_tree(&fixture.media_root);

        let report = import_profile_documents_once(
            &mut fixture.store,
            &fixture.media_root,
            "default",
            &mut fixture.settings,
            &fixture.settings_path,
        )
        .expect("first import runs");

        assert_eq!(
            report.imported,
            ["Nested Legacy", "Legacy New"],
            "the scan is deterministic: nested/deeper sorts before new.json"
        );
        assert_eq!(report.skipped_existing, 2, "existing id + duplicate id");
        assert_eq!(
            report.name_collisions,
            vec![fixture.media_root.join("default/scene/collide.json")],
            "a new id reusing an existing name is reported, not added"
        );
        assert_eq!(report.skipped_invalid, 2, "malformed + oversized");
        assert_eq!(report.presets, 2, "plain presets are counted, not loaded");
        assert_eq!(report.playlists, 1, "playlists are not imported");
        assert!(!report.truncated);

        let ids = scene_ids(&fixture.store);
        assert_eq!(
            ids,
            ["scene-existing", "scene-nested", "scene-imported"],
            "one existing Scene plus the two new documents"
        );
        assert_eq!(
            scene_names(&fixture.store),
            ["Existing", "Nested Legacy", "Legacy New"],
            "a colliding name never replaces the existing Scene"
        );
        for hidden in ["scene-hidden", "scene-temp", "scene-tilde"] {
            assert!(
                !ids.iter().any(|id| id == hidden),
                "{hidden} came from a hidden/temporary file"
            );
        }

        let persisted: Value = serde_json::from_slice(
            &std::fs::read(fixture.user_data.join("scenes.json")).expect("scenes.json"),
        )
        .expect("scenes json");
        assert_eq!(
            persisted["scenes"].as_array().map(Vec::len),
            Some(3),
            "the import is persisted once, not per document"
        );

        assert_eq!(
            snapshot_tree(&fixture.media_root),
            before,
            "the originals are neither rewritten nor moved"
        );
        assert!(fixture.user_data.join("native-project.json").is_file());
        let saved = ProjectSettings::read_from(&fixture.settings_path)
            .expect("read settings")
            .expect("settings exist");
        assert!(saved.documents_imported("default"));
        assert_eq!(saved.profile, "default");

        let summary = report.summary();
        assert!(summary.contains("imported 2 Scene(s)"), "{summary}");
        assert!(
            summary.contains("skipped 5 existing/invalid"),
            "2 existing + 1 collision + 2 invalid: {summary}"
        );
        assert!(summary.contains("2 preset document(s)"), "{summary}");
        assert!(summary.contains("1 playlist document(s)"), "{summary}");

        let _ = std::fs::remove_dir_all(fixture.root);
    }

    #[test]
    fn rerunning_the_import_adds_nothing_and_the_flag_skips_the_scan() {
        let mut fixture = fixture();
        import_profile_documents_once(
            &mut fixture.store,
            &fixture.media_root,
            "default",
            &mut fixture.settings,
            &fixture.settings_path,
        )
        .expect("first import");
        let count = fixture.store.scenes_state().scenes.len();

        assert_eq!(
            import_profile_documents_once(
                &mut fixture.store,
                &fixture.media_root,
                "default",
                &mut fixture.settings,
                &fixture.settings_path,
            ),
            None,
            "the settings flag ends the one-time import"
        );
        // Even a forced rescan cannot duplicate an already-imported document:
        // the id/name checks are the second line of defence when the flag is
        // lost.
        let forced = import_profile_documents(&mut fixture.store, &fixture.media_root, "default");
        assert_eq!(forced.imported, Vec::<String>::new());
        assert_eq!(fixture.store.scenes_state().scenes.len(), count);

        // A file added after the import must not be picked up on the next
        // launch: the authoritative Scene list changes only through the UI.
        write_json(
            &fixture.media_root.join("default/scene/later.json"),
            &scene_document("scene-later", "Added Later"),
        );
        assert_eq!(
            import_profile_documents_once(
                &mut fixture.store,
                &fixture.media_root,
                "default",
                &mut fixture.settings,
                &fixture.settings_path,
            ),
            None
        );
        assert_eq!(fixture.store.scenes_state().scenes.len(), count);
        assert!(
            !scene_ids(&fixture.store)
                .iter()
                .any(|id| id == "scene-later")
        );

        let _ = std::fs::remove_dir_all(fixture.root);
    }

    #[test]
    fn each_profile_runs_its_own_import_once() {
        let mut fixture = fixture();
        import_profile_documents_once(
            &mut fixture.store,
            &fixture.media_root,
            "default",
            &mut fixture.settings,
            &fixture.settings_path,
        )
        .expect("default import");

        create_profile(&fixture.media_root, "live").expect("live profile");
        write_json(
            &fixture.media_root.join("live/scene/live.json"),
            &scene_document("scene-live", "Live Profile Scene"),
        );

        let report = import_profile_documents_once(
            &mut fixture.store,
            &fixture.media_root,
            "live",
            &mut fixture.settings,
            &fixture.settings_path,
        )
        .expect("live import");

        assert_eq!(report.imported, ["Live Profile Scene"]);
        assert_eq!(fixture.store.scenes_state().scenes.len(), 4);
        let saved = ProjectSettings::read_from(&fixture.settings_path)
            .expect("read settings")
            .expect("settings exist");
        assert!(saved.documents_imported("default"));
        assert!(saved.documents_imported("live"));
        assert_eq!(
            import_profile_documents_once(
                &mut fixture.store,
                &fixture.media_root,
                "live",
                &mut fixture.settings,
                &fixture.settings_path,
            ),
            None,
            "the live profile import is also one-time"
        );

        let _ = std::fs::remove_dir_all(fixture.root);
    }

    #[test]
    fn the_report_summary_matches_the_log_wording() {
        let report = ImportReport::default();
        assert_eq!(
            report.summary(),
            "imported 0 Scene(s), skipped 0 existing/invalid"
        );
    }
}
