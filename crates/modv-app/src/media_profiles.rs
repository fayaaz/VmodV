//! Project media profiles, mirroring the Electron media manager.
//!
//! Electron keeps every project under `<userData>/media/<profile>` with the
//! same folder set `fs-create-profile.js` makes:
//! `image`, `video`, `palette`, `preset`, `scene`, `module`, `plugin`,
//! `plugin_data` and `isf`. Presets and Scenes are ordinary files in those
//! folders, so the VmodV shell keeps the layout (and the shared
//! `<userData>/scenes.json` list) untouched.
//!
//! The active profile is remembered in `<userData>/vmodv-project.json`
//! beside `vmodv-audio.json` and `vmodv-media.json`. Everything here is
//! filesystem-simple and unit-testable on a headless host.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The persisted project choice inside the shared userData directory.
pub const PROJECT_SETTINGS_FILENAME: &str = "vmodv-project.json";

/// The profile the JS media manager creates on first launch.
pub const DEFAULT_PROFILE: &str = "default";

/// The folders `fs-create-profile.js` creates in every profile, plus the
/// native-only `milkdrop` folder for user MilkDrop presets.
pub const PROFILE_FOLDERS: [&str; 10] = [
    "image",
    "video",
    "palette",
    "preset",
    "scene",
    "module",
    "plugin",
    "plugin_data",
    "isf",
    "milkdrop",
];

/// The shared, non-profile folders created directly under `<media>`.
///
/// These are corpus folders for every profile (kept for older native builds)
/// and must not be mistaken for a project profile by [`list_profiles`]. They
/// share the `<media>` namespace with profiles, so their names are effectively
/// **reserved**: a profile cannot be named `milkdrop`, because its folder would
/// collide with the shared corpus folder. Naming one silently creates no usable
/// profile (it is filtered out again on the next [`list_profiles`] call).
pub const SHARED_MEDIA_FOLDERS: [&str; 1] = ["milkdrop"];

/// One persisted project selection.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectSettings {
    /// The active profile name; an empty document means [`DEFAULT_PROFILE`].
    #[serde(default)]
    pub profile: String,
    /// Extra user ISF folders chosen through the File menu.
    #[serde(default)]
    pub isf_dirs: Vec<PathBuf>,
    /// Profiles whose loose `scene/`/`preset/` documents were imported once.
    ///
    /// Keyed by profile name and only ever set to `true`; the map lives here so
    /// `vmodv-project.json` remembers the one-time import across restarts and
    /// profile switches. See [`crate::media_import`].
    #[serde(default)]
    pub imported_documents: BTreeMap<String, bool>,
}

impl ProjectSettings {
    /// Whether `profile`'s loose media documents were already imported.
    pub fn documents_imported(&self, profile: &str) -> bool {
        self.imported_documents
            .get(profile)
            .copied()
            .unwrap_or(false)
    }

    /// Remember that `profile`'s one-time document import ran.
    pub fn mark_documents_imported(&mut self, profile: &str) {
        self.imported_documents.insert(profile.to_string(), true);
    }

    /// The active profile, falling back to `default`.
    pub fn active_profile(&self) -> &str {
        if self.profile.is_empty() {
            DEFAULT_PROFILE
        } else {
            &self.profile
        }
    }

    /// Read the document, treating a missing file as "nothing saved".
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
        if let Err(error) = normalize_profile_name(settings.active_profile()) {
            return Err(format!("{}: {error}", path.display()));
        }
        Ok(Some(settings))
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
            .map_err(|error| format!("could not serialise project settings: {error}"))?;
        std::fs::write(&temporary, text)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(())
    }
}

/// A profile name that cannot be used as one folder under `<media>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileNameError {
    Empty,
    Hidden,
    Separator,
    Traversal,
    Control,
    TooLong,
}

impl std::fmt::Display for ProfileNameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "enter a profile name"),
            Self::Hidden => write!(formatter, "a profile name cannot start with a dot"),
            Self::Separator => write!(formatter, "a profile name cannot contain / or \\"),
            Self::Traversal => write!(formatter, "`.` and `..` are not profile names"),
            Self::Control => write!(
                formatter,
                "a profile name cannot contain control characters"
            ),
            Self::TooLong => write!(formatter, "a profile name is limited to 64 characters"),
        }
    }
}

impl std::error::Error for ProfileNameError {}

/// Validate and trim a profile name.
///
/// Names in [`SHARED_MEDIA_FOLDERS`] are reserved for the shared corpus folders
/// under `<media>`; currently that means `milkdrop` is not a usable profile
/// name. This is intentionally left unchecked here so persisted settings from
/// older installs still parse — [`list_profiles`] simply omits the collision.
pub fn normalize_profile_name(raw: &str) -> Result<String, ProfileNameError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(ProfileNameError::Empty);
    }
    if name == "." || name == ".." {
        return Err(ProfileNameError::Traversal);
    }
    if name.starts_with('.') {
        return Err(ProfileNameError::Hidden);
    }
    if name
        .chars()
        .any(|character| character == '/' || character == '\\')
    {
        return Err(ProfileNameError::Separator);
    }
    if name.chars().any(char::is_control) {
        return Err(ProfileNameError::Control);
    }
    if name.chars().count() > 64 {
        return Err(ProfileNameError::TooLong);
    }
    Ok(name.to_string())
}

/// `<media>/<profile>`.
pub fn profile_dir(media_root: &Path, profile: &str) -> PathBuf {
    media_root.join(profile)
}

/// `<media>/<profile>/<folder>`.
pub fn profile_folder(media_root: &Path, profile: &str, folder: &str) -> PathBuf {
    profile_dir(media_root, profile).join(folder)
}

/// Create (or repair) `profile` with the full Electron folder set.
///
/// Existing folders and files are left alone; missing ones are created.
pub fn create_profile(media_root: &Path, profile: &str) -> Result<PathBuf, String> {
    let name = normalize_profile_name(profile).map_err(|error| error.to_string())?;
    let root = profile_dir(media_root, &name);
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("could not create {}: {error}", root.display()))?;
    for folder in PROFILE_FOLDERS {
        let path = root.join(folder);
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    }
    // The shared flat corpus folders live beside the profiles so older native
    // installs (and explicit `user_*_dirs` overrides) keep working.
    for folder in SHARED_MEDIA_FOLDERS {
        let path = media_root.join(folder);
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    }
    Ok(root)
}

/// Every profile folder under `<media>`, sorted case-insensitively.
///
/// Files, hidden folders and the names in [`SHARED_MEDIA_FOLDERS`] are not
/// profiles — the latter are corpus folders shared by every profile.
pub fn list_profiles(media_root: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(media_root) else {
        return Vec::new();
    };
    let mut profiles: Vec<String> = read
        .flatten()
        .filter(|entry| entry.metadata().map(|meta| meta.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| !name.starts_with('.'))
        .filter(|name| !SHARED_MEDIA_FOLDERS.contains(&name.as_str()))
        .collect();
    profiles.sort_by_key(|name| name.to_lowercase());
    profiles
}

/// The profiles the shell shows at startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedProfiles {
    /// The active profile (the saved one when it exists, else `default`).
    pub active: String,
    /// Every profile folder after ensuring the default exists.
    pub profiles: Vec<String>,
}

/// Ensure the default profile exists and resolve the saved selection.
///
/// A saved profile that no longer exists (deleted outside the app) falls back
/// to `default` instead of recreating it or leaving the shell without a
/// watchable tree. The fallback is logged by [`resolve_profile`].
pub fn prepare_profiles(media_root: &Path, requested: &str) -> Result<PreparedProfiles, String> {
    create_profile(media_root, DEFAULT_PROFILE)?;
    let requested = normalize_profile_name(requested).ok();
    let profiles = list_profiles(media_root);
    let active = requested
        .filter(|name| profiles.iter().any(|profile| profile == name))
        .unwrap_or_else(|| DEFAULT_PROFILE.to_string());
    Ok(PreparedProfiles { active, profiles })
}

/// The user ISF folders to scan for `profile`.
///
/// The flat `<media>/isf` folder is kept for installs created by older VmodV
/// builds. Only the **active** profile's `isf` folder counts, so switching
/// profiles changes the module corpus exactly like the JS media manager's
/// folder reload. Explicit extra folders are included when they exist.
pub fn user_isf_dirs(media_root: &Path, profile: &str, extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for candidate in [
        media_root.join("isf"),
        profile_folder(media_root, profile, "isf"),
    ] {
        if candidate.is_dir() {
            dirs.push(candidate);
        }
    }
    for path in extra {
        if path.is_dir() {
            dirs.push(path.clone());
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// The user MilkDrop preset folders to scan for `profile`.
///
/// Mirrors [`user_isf_dirs`]: the flat `<media>/milkdrop` folder is kept for
/// installs created by older native builds, only the **active** profile's
/// `milkdrop` folder counts, and existing explicit extras are included. An
/// explicit `MODV_USER_MILKDROP_DIR` environment override replaces all of them.
pub fn user_milkdrop_dirs(media_root: &Path, profile: &str, extra: &[PathBuf]) -> Vec<PathBuf> {
    let override_dir = std::env::var_os("MODV_USER_MILKDROP_DIR").map(PathBuf::from);
    user_milkdrop_dirs_with_override(media_root, profile, extra, override_dir.as_deref())
}

/// [`user_milkdrop_dirs`] with an explicit `MODV_USER_MILKDROP_DIR` override.
///
/// `Some` returns only that folder (without an existence check — an explicit
/// choice is honoured even before it is created). Otherwise performs the same
/// discovery as [`user_isf_dirs`] with the `milkdrop` folder name. The explicit
/// parameter keeps the override testable without mutating process-wide
/// environment variables.
pub fn user_milkdrop_dirs_with_override(
    media_root: &Path,
    profile: &str,
    extra: &[PathBuf],
    override_dir: Option<&Path>,
) -> Vec<PathBuf> {
    if let Some(dir) = override_dir {
        return vec![dir.to_path_buf()];
    }
    let mut dirs = Vec::new();
    for candidate in [
        media_root.join("milkdrop"),
        profile_folder(media_root, profile, "milkdrop"),
    ] {
        if candidate.is_dir() {
            dirs.push(candidate);
        }
    }
    for path in extra {
        if path.is_dir() {
            dirs.push(path.clone());
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_native_project_filename_is_read_as_a_fallback() {
        let root = temp_dir("modv-project-compat");
        let legacy = root.join("vmodv-project.json");
        let saved = ProjectSettings {
            profile: "live".to_string(),
            ..Default::default()
        };
        saved.write_to(&legacy).expect("write legacy project");
        let current = root.join(PROJECT_SETTINGS_FILENAME);
        assert_eq!(
            ProjectSettings::read_from(&current).expect("read legacy project"),
            Some(saved)
        );
        let _ = std::fs::remove_dir_all(root);
    }

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

    #[test]
    fn creating_a_profile_makes_the_electron_folder_set() {
        let root = temp_dir("modv-profile-create");
        let media = root.join("media");

        let profile = create_profile(&media, " live ").expect("create profile");

        assert_eq!(profile, media.join("live"));
        for folder in PROFILE_FOLDERS {
            assert!(
                profile.join(folder).is_dir(),
                "missing profile folder {folder}"
            );
        }
        assert_eq!(list_profiles(&media), vec!["live"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn creating_an_existing_profile_keeps_its_files() {
        let root = temp_dir("modv-profile-existing");
        let media = root.join("media");
        let profile = create_profile(&media, "default").expect("create");
        std::fs::write(profile.join("preset/saved.json"), "{}").expect("saved preset");

        create_profile(&media, "default").expect("idempotent");

        assert!(profile.join("preset/saved.json").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn profile_names_reject_traversal_and_separators() {
        assert_eq!(normalize_profile_name(" live "), Ok("live".to_string()));
        assert_eq!(normalize_profile_name("My Show"), Ok("My Show".to_string()));
        for (name, error) in [
            ("", ProfileNameError::Empty),
            ("   ", ProfileNameError::Empty),
            (".", ProfileNameError::Traversal),
            ("..", ProfileNameError::Traversal),
            (".hidden", ProfileNameError::Hidden),
            ("a/b", ProfileNameError::Separator),
            ("a\\b", ProfileNameError::Separator),
            ("nul\0byte", ProfileNameError::Control),
        ] {
            assert_eq!(normalize_profile_name(name), Err(error), "{name:?}");
        }
        assert!(matches!(
            normalize_profile_name(&"x".repeat(65)),
            Err(ProfileNameError::TooLong)
        ));
    }

    #[test]
    fn listing_profiles_ignores_files_and_hidden_folders() {
        let root = temp_dir("modv-profile-list");
        let media = root.join("media");
        create_profile(&media, "live").expect("live");
        create_profile(&media, "default").expect("default");
        std::fs::create_dir_all(media.join(".cache")).expect("hidden");
        std::fs::write(media.join("notes.txt"), "x").expect("file");

        assert_eq!(list_profiles(&media), vec!["default", "live"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preparing_profiles_seeds_default_and_falls_back_when_missing() {
        let root = temp_dir("modv-profile-prepare");
        let media = root.join("media");

        let prepared = prepare_profiles(&media, "live").expect("prepare");
        assert_eq!(prepared.active, DEFAULT_PROFILE);
        assert_eq!(prepared.profiles, vec![DEFAULT_PROFILE]);
        assert!(media.join("default/isf").is_dir());

        create_profile(&media, "live").expect("live");
        let prepared = prepare_profiles(&media, "live").expect("prepare");
        assert_eq!(prepared.active, "live");
        assert_eq!(prepared.profiles, vec!["default", "live"]);

        let prepared = prepare_profiles(&media, "../escape").expect("prepare");
        assert_eq!(prepared.active, DEFAULT_PROFILE);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn project_settings_round_trip_atomically() {
        let root = temp_dir("modv-profile-settings");
        let path = root.join(PROJECT_SETTINGS_FILENAME);
        assert_eq!(ProjectSettings::read_from(&path), Ok(None));

        let settings = ProjectSettings {
            profile: "live".to_string(),
            isf_dirs: vec![root.join("extra-isf")],
            ..Default::default()
        };
        settings.write_to(&path).expect("write");
        let mut round_tripped = ProjectSettings::read_from(&path)
            .expect("read")
            .expect("settings");

        assert_eq!(round_tripped, settings);
        round_tripped.mark_documents_imported("live");
        assert!(round_tripped.documents_imported("live"));
        assert!(!round_tripped.documents_imported("default"));
        round_tripped.write_to(&path).expect("write import flag");
        let saved = ProjectSettings::read_from(&path)
            .expect("read")
            .expect("settings");
        assert!(saved.documents_imported("live"), "the flag persists");
        assert!(!saved.documents_imported("default"));

        assert!(
            !root.join("vmodv-project.json.tmp").exists(),
            "the temporary file is renamed into place"
        );

        std::fs::write(&path, "{ not json").expect("corrupt");
        assert!(ProjectSettings::read_from(&path).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_empty_profile_field_means_default() {
        let settings: ProjectSettings = serde_json::from_str("{}").expect("json");
        assert_eq!(settings.active_profile(), DEFAULT_PROFILE);
        assert_eq!(
            ProjectSettings {
                profile: String::new(),
                isf_dirs: Vec::new(),
                imported_documents: BTreeMap::new(),
            }
            .active_profile(),
            DEFAULT_PROFILE
        );
    }

    #[test]
    fn isf_dirs_follow_the_active_profile_and_explicit_extras() {
        let root = temp_dir("modv-profile-isf");
        let media = root.join("media");
        create_profile(&media, "default").expect("default");
        create_profile(&media, "live").expect("live");
        std::fs::create_dir_all(media.join("isf")).expect("flat");
        let extra = root.join("third-party");
        std::fs::create_dir_all(&extra).expect("extra");

        let dirs = user_isf_dirs(&media, "live", &[extra.clone(), root.join("missing")]);

        assert!(dirs.contains(&media.join("isf")), "legacy flat folder");
        assert!(dirs.contains(&media.join("live/isf")), "active profile");
        assert!(dirs.contains(&extra), "explicit extra folder");
        assert!(
            !dirs.contains(&media.join("default/isf")),
            "other profiles are not searched"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn milkdrop_dirs_follow_the_active_profile_and_explicit_extras() {
        let root = temp_dir("modv-profile-milkdrop");
        let media = root.join("media");
        create_profile(&media, "default").expect("default");
        create_profile(&media, "live").expect("live");
        std::fs::create_dir_all(media.join("milkdrop")).expect("flat milkdrop");
        std::fs::create_dir_all(media.join("live/milkdrop")).expect("live milkdrop");
        let extra = root.join("third-party-presets");
        std::fs::create_dir_all(&extra).expect("extra");

        let dirs = user_milkdrop_dirs(&media, "live", &[extra.clone(), root.join("missing")]);

        assert!(dirs.contains(&media.join("milkdrop")), "legacy flat folder");
        assert!(
            dirs.contains(&media.join("live/milkdrop")),
            "active profile"
        );
        assert!(dirs.contains(&extra), "explicit extra folder");
        assert!(
            !dirs.contains(&media.join("default/milkdrop")),
            "other profiles are not searched"
        );
        assert!(
            !dirs.contains(&root.join("missing")),
            "missing extras are not returned"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn milkdrop_dir_override_returns_only_the_override() {
        let root = temp_dir("modv-profile-milkdrop-override");
        let media = root.join("media");
        create_profile(&media, "live").expect("live");
        std::fs::create_dir_all(media.join("milkdrop")).expect("flat milkdrop");
        let override_dir = root.join("chosen-milkdrop");

        let dirs = user_milkdrop_dirs_with_override(&media, "live", &[], Some(&override_dir));

        assert_eq!(
            dirs,
            vec![override_dir],
            "the override replaces every other folder"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn creating_a_profile_makes_the_milkdrop_folders() {
        let root = temp_dir("modv-profile-milkdrop-create");
        let media = root.join("media");

        let profile = create_profile(&media, " live ").expect("create profile");

        assert!(
            media.join("milkdrop").is_dir(),
            "shared <media>/milkdrop folder"
        );
        assert!(profile.join("milkdrop").is_dir(), "profile milkdrop folder");
        let _ = std::fs::remove_dir_all(root);
    }
}
