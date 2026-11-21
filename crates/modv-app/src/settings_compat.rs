//! Compatibility for persisted settings files renamed from `native-*.json` to
//! `vmodv-*.json`.
//!
//! Reads still honour the legacy filenames so an existing install keeps its
//! saved settings after the rebrand. Writes always use the new names, so a
//! settings change during any later launch naturally completes the migration.

use std::path::{Path, PathBuf};

/// Resolve a persisted settings path for reading.
///
/// If `path` names a `vmodv-*.json` document that does not exist yet but its
/// legacy `native-*.json` counterpart does, the legacy path is returned. Any
/// other path (new file present, no legacy file, or an unrelated filename) is
/// returned unchanged so each reader's existing "missing means nothing saved"
/// behaviour is preserved.
pub(crate) fn read_path(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.to_path_buf();
    };
    let Some(legacy_name) = name.strip_prefix("vmodv-") else {
        return path.to_path_buf();
    };
    let legacy = path.with_file_name(format!("native-{legacy_name}"));
    if legacy.exists() {
        return legacy;
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "modv-settings-compat-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn uses_the_legacy_file_only_when_the_new_file_is_absent() {
        let root = dir("fallback");
        let current = root.join("vmodv-layout.json");
        let legacy = root.join("vmodv-layout.json");
        std::fs::write(&legacy, "{}").expect("write legacy");

        assert_eq!(read_path(&current), legacy);

        std::fs::write(&current, "{}").expect("write current");
        assert_eq!(read_path(&current), current);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn leaves_unrelated_paths_and_missing_files_untouched() {
        let root = dir("unchanged");
        let unrelated = root.join("engine-state.json");
        assert_eq!(read_path(&unrelated), unrelated);

        let current = root.join("vmodv-audio.json");
        assert_eq!(read_path(&current), current);

        let _ = std::fs::remove_dir_all(root);
    }
}
