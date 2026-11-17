//! Debounced filesystem watch for user ISF folders (P6).
//!
//! `notify` reports every write; a `fs::write` often arrives as several
//! events. The watcher coalesces them for 500 ms and then asks the native
//! shell to rebuild the module registry once.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

/// One debounced ISF watcher over the user media folders.
pub struct IsfWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<()>,
    dirty: bool,
    last_event: Instant,
}

impl IsfWatcher {
    /// Watch each directory non-recursively. `None` when the platform watcher
    /// or every directory watch failed.
    pub fn new(dirs: &[PathBuf]) -> Option<Self> {
        if dirs.is_empty() {
            return None;
        }
        let (tx, rx): (Sender<()>, Receiver<()>) = channel();
        let mut watcher = notify::recommended_watcher(move |result: Result<Event, _>| {
            let Ok(event) = result else {
                return;
            };
            if event.paths.iter().any(|path| is_isf(path)) {
                let _ = tx.send(());
            }
        })
        .ok()?;

        let mut watched = 0;
        for dir in dirs {
            match watcher.watch(dir, RecursiveMode::NonRecursive) {
                Ok(()) => watched += 1,
                Err(error) => eprintln!(
                    "ui: WARN could not watch ISF folder {}: {error}",
                    dir.display()
                ),
            }
        }
        if watched == 0 {
            return None;
        }
        Some(Self {
            _watcher: watcher,
            rx,
            dirty: false,
            last_event: Instant::now(),
        })
    }

    /// Drain pending events. Returns `true` once a burst has been quiet for
    /// 500 ms, meaning the shell should rescan the ISF dirs.
    pub fn pump(&mut self) -> bool {
        let mut saw_event = false;
        while self.rx.try_recv().is_ok() {
            saw_event = true;
        }
        if saw_event {
            self.dirty = true;
            self.last_event = Instant::now();
        }
        if self.dirty && self.last_event.elapsed() >= Duration::from_millis(500) {
            self.dirty = false;
            return true;
        }
        false
    }
}

fn is_isf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("fs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fs_paths_trigger_a_rescan() {
        assert!(is_isf(Path::new("/tmp/Soft Blur.fs")));
        assert!(is_isf(Path::new("/tmp/shader.FS")));
        assert!(!is_isf(Path::new("/tmp/shader.vs")));
        assert!(!is_isf(Path::new("/tmp/shader.fs.bak")));
        assert!(!is_isf(Path::new("/tmp/shader")));
    }
}
