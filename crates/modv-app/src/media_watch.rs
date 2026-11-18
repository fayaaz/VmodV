//! Debounced filesystem watch for the active project media tree (P6).
//!
//! Electron's `create-watcher.js` watches the whole media directory and feeds
//! changed files back into its read handlers. The native shell only consumes
//! ISF shaders, but an image/video/palette change is still worth one log line,
//! so this watcher covers those four folders of the active profile (plus any
//! extra user ISF folders) and reports a coalesced burst.
//!
//! `notify` reports every write; editors often produce several events per
//! save. A burst is only taken after 500 ms of quiet, exactly like the old
//! `isf_watch` debounce. Only files the read handlers understand are recorded,
//! and temporary/hidden names are ignored, so the app's own writes and editor
//! swap files do not wake the engine.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
}

impl MediaFolder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Isf => "isf",
            Self::Image => "image",
            Self::Video => "video",
            Self::Palette => "palette",
        }
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

    /// Whether any folder other than ISF changed (logged, no rescan).
    pub fn other_folders(&self) -> Vec<MediaFolder> {
        self.folders
            .iter()
            .copied()
            .filter(|folder| *folder != MediaFolder::Isf)
            .collect()
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
    /// Watch each existing target's directory non-recursively. `None` when the
    /// platform watcher or every directory watch failed.
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
            for path in &event.paths {
                if let Some(folder) = classify(&watched_targets, path) {
                    let _ = tx.send(folder);
                }
            }
        })
        .ok()?;

        let mut watched = 0;
        for target in &targets {
            match watcher.watch(&target.dir, RecursiveMode::NonRecursive) {
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

    /// Watch the active profile's `isf`, `image`, `video` and `palette`
    /// folders, plus the extra user ISF folders.
    pub fn for_profile(media_root: &Path, profile: &str, extra_isf: &[PathBuf]) -> Option<Self> {
        let mut targets = Vec::new();
        for folder in [
            MediaFolder::Isf,
            MediaFolder::Image,
            MediaFolder::Video,
            MediaFolder::Palette,
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
fn classify(targets: &[WatchTarget], path: &Path) -> Option<MediaFolder> {
    let name = path.file_name()?.to_str()?;
    if name.starts_with('.') || name.ends_with('~') || name.ends_with(".tmp") {
        return None;
    }
    let folder = targets
        .iter()
        .filter(|target| path.parent() == Some(target.dir.as_path()))
        .map(|target| target.folder)
        .next()?;
    supported(folder, name).then_some(folder)
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
        assert_eq!(classify(&targets, &isf.join("compiled/Blur.fs")), None);
        assert_eq!(
            classify(&targets, Path::new("/media/live/preset/song.json")),
            None,
            "preset/scene folders are not watched"
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
}
