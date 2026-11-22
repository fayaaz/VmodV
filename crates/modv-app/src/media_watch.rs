//! Debounced filesystem watch for the active project media tree (P6).
//!
//! Electron's `create-watcher.js` watches the whole media directory and feeds
//! changed files back into its read handlers. The VmodV shell consumes ISF shaders and MilkDrop presets — both re-register engine modules on change — while an image/video/palette change is only worth one log line, so this watcher covers those five folders of the active profile (plus any extra user ISF folders) and reports a coalesced burst.
//!
//! `notify` reports every write; editors often produce several events per
//! save. A burst is only taken after 500 ms of quiet, exactly like the old
//! `isf_watch` debounce. The media folders and extra ISF roots are watched
//! **recursively**, so shader packs organised in subfolders behave like flat
//! ones. Only files the read handlers understand are recorded, and
//! temporary/hidden names are ignored at every level, so the app's own writes
//! and editor swap files do not wake the engine.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::media_profiles;

/// How long a burst must be quiet before it is reported.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// One media folder the watcher understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MediaFolder {
    Isf,
    Image,
    Video,
    Palette,
    /// MilkDrop `.mlk` presets.
    Milkdrop,
}

impl MediaFolder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Isf => "isf",
            Self::Image => "image",
            Self::Video => "video",
            Self::Palette => "palette",
            Self::Milkdrop => "milkdrop",
        }
    }

    /// A change in this folder re-registers engine modules (ISF shaders or
    /// MilkDrop presets) rather than only refreshing the media Library.
    fn registers_modules(self) -> bool {
        matches!(self, Self::Isf | Self::Milkdrop)
    }
}

/// One watched directory and the read-handler folder it feeds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchTarget {
    pub dir: PathBuf,
    pub folder: MediaFolder,
}

/// The folders in one quiet, debounced batch of changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaBurst {
    pub folders: Vec<MediaFolder>,
    /// The number of matching filesystem events that produced the burst.
    pub events: usize,
}

impl MediaBurst {
    pub fn has(&self, folder: MediaFolder) -> bool {
        self.folders.contains(&folder)
    }

    /// Whether any changed folder feeds the module registry (ISF or MilkDrop).
    pub(crate) fn registers_modules(&self) -> bool {
        self.folders.iter().any(|folder| folder.registers_modules())
    }

    /// The changed folders that are *not* module sources — logged for
    /// visibility, but needing no engine re-registration.
    pub fn other_folders(&self) -> Vec<MediaFolder> {
        self.folders
            .iter()
            .copied()
            .filter(|folder| !folder.registers_modules())
            .collect()
    }

    /// Whether the burst touched a folder the Library index mirrors.
    ///
    /// Shader-only and preset-only bursts never rescan the media library; an
    /// image, video or palette change asks for one coalesced rescan.
    pub fn touches_library(&self) -> bool {
        self.folders
            .iter()
            .any(|folder| !folder.registers_modules())
    }
}

/// The debounce state, split out so it is testable without a real watcher.
#[derive(Debug, Default)]
struct Debounce {
    folders: BTreeSet<MediaFolder>,
    events: usize,
    last_event: Option<Instant>,
}

impl Debounce {
    fn record(&mut self, folder: MediaFolder, now: Instant) {
        self.folders.insert(folder);
        self.events += 1;
        self.last_event = Some(now);
    }

    fn take(&mut self, now: Instant, quiet: Duration) -> Option<MediaBurst> {
        let last = self.last_event?;
        if now.saturating_duration_since(last) < quiet || self.folders.is_empty() {
            return None;
        }
        self.last_event = None;
        let folders = std::mem::take(&mut self.folders).into_iter().collect();
        let events = std::mem::take(&mut self.events);
        Some(MediaBurst { folders, events })
    }
}

/// One debounced watcher over the active project's media folders.
pub struct MediaWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<MediaFolder>,
    debounce: Debounce,
}

impl MediaWatcher {
    /// Watch each existing target's directory recursively. `None` when the
    /// platform watcher or every directory watch failed. A missing media
    /// subfolder is simply not watched; the remaining folders still work.
    pub fn new(targets: &[WatchTarget]) -> Option<Self> {
        let mut seen = BTreeSet::new();
        let targets: Vec<WatchTarget> = targets
            .iter()
            .filter(|target| target.dir.is_dir() && seen.insert(target.dir.clone()))
            .cloned()
            .collect();
        if targets.is_empty() {
            return None;
        }

        let (tx, rx): (Sender<MediaFolder>, Receiver<MediaFolder>) = channel();
        let watched_targets = targets.clone();
        let mut watcher = notify::recommended_watcher(move |result: Result<Event, _>| {
            let Ok(event) = result else {
                return;
            };
            if !event_kind_relevant(&event.kind) {
                return;
            }
            for path in &event.paths {
                if let Some(folder) = classify(&watched_targets, path) {
                    let _ = tx.send(folder);
                }
            }
        })
        .ok()?;

        let mut watched = 0;
        for target in &targets {
            match watcher.watch(&target.dir, RecursiveMode::Recursive) {
                Ok(()) => watched += 1,
                Err(error) => eprintln!(
                    "ui: WARN could not watch media folder {}: {error}",
                    target.dir.display()
                ),
            }
        }
        if watched == 0 {
            return None;
        }
        Some(Self {
            _watcher: watcher,
            rx,
            debounce: Debounce::default(),
        })
    }

    /// Watch the active profile's `isf`, `image`, `video`, `palette` and
    /// `milkdrop` folders, plus the extra user ISF folders, recursively.
    pub fn for_profile(media_root: &Path, profile: &str, extra_isf: &[PathBuf]) -> Option<Self> {
        let mut targets = Vec::new();
        for folder in [
            MediaFolder::Isf,
            MediaFolder::Image,
            MediaFolder::Video,
            MediaFolder::Palette,
            MediaFolder::Milkdrop,
        ] {
            targets.push(WatchTarget {
                dir: media_profiles::profile_folder(media_root, profile, folder.label()),
                folder,
            });
        }
        for dir in extra_isf {
            targets.push(WatchTarget {
                dir: dir.clone(),
                folder: MediaFolder::Isf,
            });
        }
        Self::new(&targets)
    }

    /// Drain pending events. Returns the coalesced burst once it has been
    /// quiet for [`DEBOUNCE`].
    pub fn pump(&mut self) -> Option<MediaBurst> {
        self.pump_at(Instant::now())
    }

    /// [`Self::pump`] using a caller-supplied clock (tests).
    fn pump_at(&mut self, now: Instant) -> Option<MediaBurst> {
        while let Ok(folder) = self.rx.try_recv() {
            self.debounce.record(folder, now);
        }
        self.debounce.take(now, DEBOUNCE)
    }
}

/// The read-handler folder `path` belongs to, if the handler understands it.
///
/// The watched roots are recursive, so a nested path belongs to the **longest**
/// matching root (an extra ISF folder nested inside a profile folder stays
/// attributed to the extra root). Temporary, hidden and backup names are
/// ignored in every path component.
fn classify(targets: &[WatchTarget], path: &Path) -> Option<MediaFolder> {
    let name = path.file_name()?.to_str()?;
    if is_ignored_media_name(name) {
        return None;
    }
    let target = targets
        .iter()
        .filter(|target| path.starts_with(&target.dir))
        .max_by_key(|target| target.dir.components().count())?;
    if has_ignored_component(&target.dir, path) {
        return None;
    }
    supported(target.folder, name).then_some(target.folder)
}

/// A media file or folder name the read handlers must not react to.
pub(crate) fn is_ignored_media_name(name: &str) -> bool {
    name.starts_with('.') || name.ends_with('~') || name.ends_with(".tmp")
}

/// Whether an event kind can change the media tree.
///
/// Reading a video/image for a library thumbnail touches its access time on
/// some filesystems, and inotify reports that as `Access`; reacting to those
/// makes a library rescan read the same file again and loop forever. Only
/// create/modify/remove events matter to the read handlers.
fn event_kind_relevant(kind: &notify::EventKind) -> bool {
    !matches!(kind, notify::EventKind::Access(_))
}

/// Whether any component below the watched root is hidden/temporary.
fn has_ignored_component(dir: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(dir) else {
        return true;
    };
    relative.components().any(|component| match component {
        Component::Normal(name) => name.to_str().is_none_or(is_ignored_media_name),
        _ => false,
    })
}

/// The extensions Electron's read handlers list for `folder`.
pub fn supported(folder: MediaFolder, name: &str) -> bool {
    let Some(extension) = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
    else {
        return false;
    };
    let extension = extension.to_ascii_lowercase();
    match folder {
        // The handler lists `fs`; a `.vs` sibling changes the same module.
        MediaFolder::Isf => matches!(extension.as_str(), "fs" | "vs"),
        MediaFolder::Image => matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "gif"),
        MediaFolder::Video => matches!(extension.as_str(), "mp4" | "webm" | "webv"),
        MediaFolder::Palette => extension == "json",
        // Lower-cased above, so both the legacy `.mlk` and the conventional
        // `.milk` are matched case-insensitively.
        MediaFolder::Milkdrop => matches!(extension.as_str(), "mlk" | "milk"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_read_handler_extensions_trigger_a_burst() {
        assert!(supported(MediaFolder::Isf, "Soft Blur.fs"));
        assert!(supported(MediaFolder::Isf, "shader.FS"));
        assert!(supported(MediaFolder::Isf, "shader.vs"));
        assert!(!supported(MediaFolder::Isf, "shader.js"));
        assert!(!supported(MediaFolder::Isf, "shader.fs.bak"));
        assert!(!supported(MediaFolder::Isf, "shader"));

        assert!(supported(MediaFolder::Image, "still.PNG"));
        assert!(supported(MediaFolder::Image, "anim.gif"));
        assert!(!supported(MediaFolder::Image, "clip.mp4"));
        assert!(supported(MediaFolder::Video, "clip.mp4"));
        assert!(supported(MediaFolder::Video, "clip.webm"));
        assert!(!supported(MediaFolder::Video, "clip.mkv"));
        assert!(supported(MediaFolder::Palette, "neon.json"));
        assert!(!supported(MediaFolder::Palette, "neon.txt"));
    }

    #[test]
    fn classification_uses_the_watched_folder_and_ignores_temporary_names() {
        let isf = Path::new("/media/live/isf");
        let image = Path::new("/media/live/image");
        let targets = vec![
            WatchTarget {
                dir: isf.to_path_buf(),
                folder: MediaFolder::Isf,
            },
            WatchTarget {
                dir: image.to_path_buf(),
                folder: MediaFolder::Image,
            },
        ];

        assert_eq!(
            classify(&targets, &isf.join("Blur.fs")),
            Some(MediaFolder::Isf)
        );
        assert_eq!(
            classify(&targets, &image.join("still.png")),
            Some(MediaFolder::Image)
        );
        assert_eq!(classify(&targets, &image.join("notes.txt")), None);
        assert_eq!(classify(&targets, &isf.join(".Blur.fs.swp")), None);
        assert_eq!(classify(&targets, &isf.join("Blur.fs~")), None);
        assert_eq!(
            classify(&targets, &isf.join("compiled/Blur.fs")),
            Some(MediaFolder::Isf),
            "the watch is recursive"
        );
        assert_eq!(
            classify(&targets, &isf.join(".cache/Blur.fs")),
            None,
            "a hidden subfolder is ignored"
        );
        assert_eq!(
            classify(&targets, &image.join("nested/still.png")),
            Some(MediaFolder::Image),
            "nested image folders keep their profile classification"
        );
        assert_eq!(classify(&targets, &image.join("nested/notes.txt")), None);
        assert_eq!(
            classify(&targets, Path::new("/media/live/preset/song.json")),
            None,
            "preset/scene folders are not watched"
        );
    }

    #[test]
    fn the_most_specific_watched_folder_classifies_a_nested_path() {
        let isf = Path::new("/media/live/isf");
        let extra = isf.join("third-party");
        let targets = vec![
            WatchTarget {
                dir: isf.to_path_buf(),
                folder: MediaFolder::Isf,
            },
            WatchTarget {
                dir: extra.clone(),
                folder: MediaFolder::Isf,
            },
        ];

        assert_eq!(
            classify(&targets, &extra.join("artist/pack/Blur.fs")),
            Some(MediaFolder::Isf)
        );
    }

    #[test]
    fn the_debounce_coalesces_events_and_waits_for_quiet() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        debounce.record(MediaFolder::Isf, start);
        debounce.record(MediaFolder::Isf, start + Duration::from_millis(100));
        debounce.record(MediaFolder::Image, start + Duration::from_millis(200));

        assert_eq!(
            debounce.take(start + Duration::from_millis(400), DEBOUNCE),
            None
        );
        let burst = debounce
            .take(start + Duration::from_millis(800), DEBOUNCE)
            .expect("quiet burst");
        assert_eq!(burst.folders, vec![MediaFolder::Isf, MediaFolder::Image]);
        assert_eq!(burst.events, 3);
        assert!(burst.has(MediaFolder::Isf));
        assert_eq!(burst.other_folders(), vec![MediaFolder::Image]);
        assert!(
            burst.touches_library(),
            "the image change rescans the Library"
        );

        let isf_only = MediaBurst {
            folders: vec![MediaFolder::Isf],
            events: 1,
        };
        assert!(
            !isf_only.touches_library(),
            "a shader save does not touch the media library"
        );
        assert_eq!(
            debounce.take(start + Duration::from_secs(2), DEBOUNCE),
            None,
            "a burst is reported once"
        );
    }

    #[test]
    fn burst_events_are_counted_and_reset() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        debounce.record(MediaFolder::Palette, start);
        let first = debounce
            .take(start + DEBOUNCE, DEBOUNCE)
            .expect("first burst");
        assert_eq!(first.events, 1);

        debounce.record(MediaFolder::Palette, start + Duration::from_secs(1));
        let second = debounce
            .take(start + Duration::from_secs(1) + DEBOUNCE, DEBOUNCE)
            .expect("second burst");
        assert_eq!(second.events, 1, "the count does not accumulate");
    }

    #[test]
    fn a_real_watcher_reports_a_debounced_isf_and_image_change() {
        let root = std::env::temp_dir().join(format!(
            "modv-media-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(root.join("live/isf")).expect("isf");
        std::fs::create_dir_all(root.join("live/image")).expect("image");
        let mut watcher =
            MediaWatcher::for_profile(&root, "live", &[]).expect("a watchable media tree");

        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(root.join("live/isf/Blur.fs"), "// shader").expect("write isf");
        std::fs::write(root.join("live/image/still.png"), b"png").expect("write image");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(burst) = watcher.pump_at(Instant::now()) {
                assert!(burst.has(MediaFolder::Isf), "{burst:?}");
                assert!(burst.has(MediaFolder::Image), "{burst:?}");
                assert!(!burst.has(MediaFolder::Video));
                break;
            }
            assert!(Instant::now() < deadline, "no burst within 5 seconds");
            std::thread::sleep(Duration::from_millis(50));
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_real_watcher_reports_nested_isf_changes_and_missing_folders() {
        let root = std::env::temp_dir().join(format!(
            "modv-media-watch-nested-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        // No `image`/`video`/`palette` folder: a partial media tree still
        // watches the folders that exist.
        std::fs::create_dir_all(root.join("live/isf/artist/pack")).expect("nested isf");
        std::fs::write(root.join("live/isf/artist/pack/Delete Me.fs"), "// shader")
            .expect("pre-existing nested shader");
        let mut watcher =
            MediaWatcher::for_profile(&root, "live", &[]).expect("a watchable media tree");

        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(
            root.join("live/isf/artist/pack/Nested Save.fs"),
            "// shader",
        )
        .expect("nested save");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(burst) = watcher.pump_at(Instant::now()) {
                assert!(burst.has(MediaFolder::Isf), "{burst:?}");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no nested save burst in 5 seconds"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        std::fs::remove_file(root.join("live/isf/artist/pack/Delete Me.fs"))
            .expect("delete nested shader");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(burst) = watcher.pump_at(Instant::now()) {
                assert!(burst.has(MediaFolder::Isf), "{burst:?}");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no nested delete burst in 5 seconds"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_real_watcher_reports_nested_extra_isf_folders() {
        let root = std::env::temp_dir().join(format!(
            "modv-media-watch-extra-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let extra = root.join("third-party");
        std::fs::create_dir_all(root.join("live/isf")).expect("profile isf");
        std::fs::create_dir_all(extra.join("artist/pack")).expect("nested extra isf");
        let mut watcher = MediaWatcher::for_profile(&root, "live", std::slice::from_ref(&extra))
            .expect("extra ISF folder is watchable");

        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(extra.join("artist/pack/Extra.fs"), "// shader").expect("extra save");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(burst) = watcher.pump_at(Instant::now()) {
                assert!(burst.has(MediaFolder::Isf), "{burst:?}");
                break;
            }
            assert!(Instant::now() < deadline, "no extra ISF burst in 5 seconds");
            std::thread::sleep(Duration::from_millis(50));
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn milkdrop_presets_have_their_own_label_extension_and_library_policy() {
        assert!(
            supported(MediaFolder::Milkdrop, "preset.mlk"),
            "lowercase .mlk is a MilkDrop preset"
        );
        assert!(
            supported(MediaFolder::Milkdrop, "PRESET.MLK"),
            "the extension check is case-insensitive"
        );
        assert!(
            !supported(MediaFolder::Milkdrop, "shader.fs"),
            "an ISF shader is not a MilkDrop preset"
        );
        assert!(!supported(MediaFolder::Milkdrop, "notes.txt"));
        assert_eq!(MediaFolder::Milkdrop.label(), "milkdrop");

        let burst = MediaBurst {
            folders: vec![MediaFolder::Milkdrop],
            events: 1,
        };
        assert!(burst.has(MediaFolder::Milkdrop));
        assert!(
            !burst.touches_library(),
            "a preset-only burst must not rescan the media library"
        );
    }

    #[test]
    fn conventional_milk_extension_is_supported_with_the_same_library_policy() {
        assert!(
            supported(MediaFolder::Milkdrop, "preset.milk"),
            "the conventional .milk extension is a MilkDrop preset"
        );
        assert!(
            supported(MediaFolder::Milkdrop, "PRESET.MILK"),
            "the .milk extension check is case-insensitive"
        );
        assert!(
            supported(MediaFolder::Milkdrop, "preset.mlk"),
            "the legacy .mlk extension is still accepted"
        );
        assert!(!supported(MediaFolder::Milkdrop, "shader.fs"));
        assert!(!supported(MediaFolder::Milkdrop, "notes.txt"));
        assert!(
            !MediaBurst {
                folders: vec![MediaFolder::Milkdrop],
                events: 1,
            }
            .touches_library(),
            "a .milk preset burst must not rescan the media library"
        );
    }

    #[test]
    fn milkdrop_burst_is_coalesced_once_and_does_not_touch_the_library() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        for offset in [0, 20, 40, 60, 80] {
            debounce.record(MediaFolder::Milkdrop, start + Duration::from_millis(offset));
        }

        assert_eq!(
            debounce.take(start + Duration::from_millis(300), DEBOUNCE),
            None,
            "the 500 ms debounce is unchanged"
        );
        let burst = debounce
            .take(start + Duration::from_millis(800), DEBOUNCE)
            .expect("quiet burst");
        assert_eq!(burst.folders, vec![MediaFolder::Milkdrop]);
        assert_eq!(burst.events, 5, "the burst is coalesced into one");
        assert!(
            !burst.touches_library(),
            "a preset-only burst must not rescan the media library"
        );
        assert_eq!(
            debounce.take(start + Duration::from_secs(2), DEBOUNCE),
            None,
            "a burst is reported once"
        );
    }

    #[test]
    fn a_real_watcher_reports_debounced_milkdrop_preset_changes() {
        let root = std::env::temp_dir().join(format!(
            "modv-media-watch-milkdrop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        // The other folders exist so `for_profile` still builds a watcher;
        // only the MilkDrop folder is written to.
        for folder in ["isf", "image", "video", "palette", "milkdrop"] {
            std::fs::create_dir_all(root.join("live").join(folder)).expect("media folder");
        }
        let mut watcher =
            MediaWatcher::for_profile(&root, "live", &[]).expect("a watchable media tree");

        std::thread::sleep(Duration::from_millis(100));
        for name in ["a.mlk", "b.MLK", "c.mlk"] {
            std::fs::write(root.join("live/milkdrop").join(name), b"preset").expect("write preset");
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(burst) = watcher.pump_at(Instant::now()) {
                assert!(burst.has(MediaFolder::Milkdrop), "{burst:?}");
                assert!(
                    !burst.touches_library(),
                    "a preset-only burst must not rescan the media library"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no MilkDrop burst within 5 seconds"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_real_watcher_reports_debounced_conventional_milk_preset_changes() {
        let root = std::env::temp_dir().join(format!(
            "modv-media-watch-milkdrop-milk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        for folder in ["isf", "image", "video", "palette", "milkdrop"] {
            std::fs::create_dir_all(root.join("live").join(folder)).expect("media folder");
        }
        let mut watcher =
            MediaWatcher::for_profile(&root, "live", &[]).expect("a watchable media tree");

        std::thread::sleep(Duration::from_millis(100));
        for name in ["a.milk", "b.MILK", "c.milk"] {
            std::fs::write(root.join("live/milkdrop").join(name), b"preset").expect("write preset");
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(burst) = watcher.pump_at(Instant::now()) {
                assert!(burst.has(MediaFolder::Milkdrop), "{burst:?}");
                assert!(
                    !burst.touches_library(),
                    "a preset-only burst must not rescan the media library"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no .milk MilkDrop burst within 5 seconds"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn access_events_do_not_wake_the_media_handlers() {
        use notify::EventKind;
        use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

        assert!(!event_kind_relevant(&EventKind::Access(AccessKind::Read)));
        assert!(!event_kind_relevant(&EventKind::Access(AccessKind::Close(
            notify::event::AccessMode::Read
        ))));
        assert!(event_kind_relevant(&EventKind::Create(CreateKind::File)));
        assert!(event_kind_relevant(&EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content
        ))));
        assert!(event_kind_relevant(&EventKind::Remove(RemoveKind::File)));
    }
}
