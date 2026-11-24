//! The VmodV Library tab's media index (WS: full media library).
//!
//! The panel browses the **active profile's** `image/`, `video/` and
//! `palette/` folders recursively. `modv-app` owns the IO and decoding:
//!
//! * [`MediaLibrary`] scans the three folders into a deterministic, capped
//!   list ([`MAX_ASSETS_PER_KIND`], [`MAX_SCAN_DEPTH`]); hidden/temporary names
//!   are ignored at every level, exactly like the media watcher.
//! * Image thumbnails are decoded through the existing `image` crate path
//!   (`texture_sources::decode_file`) and downscaled to
//!   [`THUMBNAIL_MAX_SIZE`] with the aspect ratio preserved; the panel
//!   letterboxes them.
//! * Palette JSON is parsed into a colour list and rendered as a strip
//!   thumbnail.
//! * Video duration/resolution are read from the MP4 (`moov`/`mvhd`/`tkhd`) or
//!   WebM (EBML `Info`/`Tracks`) container header. A real first-frame poster
//!   is deliberately deferred; the tile is a clear video placeholder with the
//!   header metadata.
//!
//! Thumbnails/decodes are cached per absolute path and dropped on a rescan or
//! profile switch. [`LibraryRefresh`] coalesces watcher bursts so at most one
//! scan runs per quiet burst, never per frame.
//!
//! The recursive scan (palette parsing, video headers) and the thumbnail
//! decodes run on two background workers. The window thread only submits a
//! request and reads the shared index/cache, so a large folder or a large
//! image can never stall a frame. Startup uses [`MediaLibrary::rescan_blocking`]
//! because the window does not exist yet.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use modv_ui::model::{LibraryKind, MediaAssetView, MediaLibraryView, PreviewTexture};

use crate::media_watch::{self, MediaBurst, MediaFolder};

/// The largest number of assets one folder contributes to the index.
pub const MAX_ASSETS_PER_KIND: usize = 200;

/// How deep the recursive scan descends below a profile folder.
pub const MAX_SCAN_DEPTH: usize = 8;

/// The largest number of decoded thumbnails kept at once.
pub const MAX_THUMBNAILS: usize = 64;

/// Thumbnail decode box; the UI letterboxes the result into the tile.
pub const THUMBNAIL_MAX_SIZE: (u32, u32) = (256, 144);

/// The maximum number of colours a palette preview keeps.
pub const MAX_PALETTE_COLOURS: usize = 16;

/// The longest palette JSON document the scanner parses.
const MAX_PALETTE_BYTES: u64 = 256 * 1024;

/// The largest `moov` box the MP4 reader materialises.
const MAX_MOOV_BYTES: usize = 32 * 1024 * 1024;

/// One file discovered by the recursive scan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScannedAsset {
    /// Media-root-relative path with `/` separators.
    pub path: String,
    /// Absolute path on disk.
    pub absolute: PathBuf,
    /// Which profile folder the asset belongs to.
    pub kind: LibraryKind,
    /// The file name.
    pub name: String,
    /// Containing folder relative to the kind root (`""` at the root).
    pub folder: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Image/video dimensions when the header was cheap to read.
    pub dimensions: Option<[u32; 2]>,
    /// Video duration from the container header.
    pub duration_ms: Option<u64>,
    /// Palette colours (RGBA), capped to [`MAX_PALETTE_COLOURS`].
    pub palette: Option<Vec<[u8; 4]>>,
}

impl ScannedAsset {
    /// The UI read model for this asset, with its uploaded thumbnail if any.
    pub fn to_view(&self, thumbnail: Option<PreviewTexture>) -> MediaAssetView {
        MediaAssetView {
            path: self.path.clone(),
            absolute: Some(self.absolute.to_string_lossy().to_string()),
            kind: self.kind,
            name: self.name.clone(),
            folder: self.folder.clone(),
            size_bytes: self.size_bytes,
            dimensions: self.dimensions,
            duration_ms: self.duration_ms,
            palette: self.palette.clone(),
            thumbnail,
        }
    }
}

/// One decoded thumbnail, tightly packed straight-alpha RGBA8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The outcome of scanning one profile folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KindScan {
    pub assets: Vec<ScannedAsset>,
    /// The folder had more supported files than [`MAX_ASSETS_PER_KIND`].
    pub truncated: bool,
    /// A directory that could not be read.
    pub error: Option<String>,
}

/// The scanned index, decode cache and counters shared with the workers.
#[derive(Default)]
struct LibraryState {
    scans: BTreeMap<LibraryKind, KindScan>,
    /// Decoded thumbnails keyed by absolute path; `None` records a decode
    /// failure so the panel does not retry it every frame.
    thumbnails: HashMap<PathBuf, Option<Thumbnail>>,
    /// Absolute paths with an outstanding worker decode.
    pending: HashSet<PathBuf>,
    /// Worker decodes that finished since the last read.
    completed: Vec<CompletedThumbnail>,
    /// The last requested scan. A worker result only applies when it matches.
    generation: u64,
    /// Invalidates in-flight decodes (rescan, clear, profile switch).
    decode_epoch: u64,
    /// Bumped whenever the scanned index changes, so the shell can rebuild its
    /// read model only when the worker actually produced something new.
    revision: u64,
}

impl LibraryState {
    /// Drop every cached, finished and in-flight thumbnail and invalidate the
    /// outstanding decodes. Callers that also change the index bump
    /// `generation` (and `revision`) separately.
    fn invalidate_decodes(&mut self) {
        self.thumbnails.clear();
        self.pending.clear();
        self.completed.clear();
        self.decode_epoch = self.decode_epoch.wrapping_add(1);
    }
}

/// One worker decode that has not been handed to the shell yet.
struct CompletedThumbnail {
    kind: LibraryKind,
    folder: String,
    path: String,
    thumbnail: Thumbnail,
}

enum ScanMessage {
    Run {
        generation: u64,
        media_root: PathBuf,
        profile: String,
    },
    Shutdown,
}

enum DecodeMessage {
    Run { epoch: u64, asset: ScannedAsset },
    Shutdown,
}

/// The window thread's handle to the two media-library workers.
struct LibraryWorker {
    scan_jobs: Option<Sender<ScanMessage>>,
    decode_jobs: Option<Sender<DecodeMessage>>,
    threads: Vec<JoinHandle<()>>,
}

/// Start one named worker, or report why it could not be started. The shell
/// keeps working with the remaining worker(s) if a spawn fails.
fn spawn_worker<T, F>(name: &str, error_label: &str, run: F) -> Option<(Sender<T>, JoinHandle<()>)>
where
    T: Send + 'static,
    F: FnOnce(Receiver<T>) + Send + 'static,
{
    let (jobs, rx) = channel();
    match std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || run(rx))
    {
        Ok(thread) => Some((jobs, thread)),
        Err(error) => {
            eprintln!("ui: WARN could not start the {error_label}: {error}");
            None
        }
    }
}

impl LibraryWorker {
    fn new(state: Arc<Mutex<LibraryState>>) -> Self {
        let mut worker = Self {
            scan_jobs: None,
            decode_jobs: None,
            threads: Vec::new(),
        };

        let scan_state = Arc::clone(&state);
        if let Some((jobs, thread)) = spawn_worker(
            "modv-library-scan",
            "media-library scan worker",
            move |rx| run_scan_worker(scan_state, rx),
        ) {
            worker.scan_jobs = Some(jobs);
            worker.threads.push(thread);
        }

        if let Some((jobs, thread)) = spawn_worker(
            "modv-library-thumbnails",
            "media-library thumbnail worker",
            move |rx| run_decode_worker(state, rx),
        ) {
            worker.decode_jobs = Some(jobs);
            worker.threads.push(thread);
        }

        worker
    }

    fn request_scan(&self, generation: u64, media_root: PathBuf, profile: String) {
        if let Some(jobs) = &self.scan_jobs {
            let _ = jobs.send(ScanMessage::Run {
                generation,
                media_root,
                profile,
            });
        }
    }

    fn request_decode(&self, epoch: u64, asset: ScannedAsset) {
        if let Some(jobs) = &self.decode_jobs {
            let _ = jobs.send(DecodeMessage::Run { epoch, asset });
        }
    }
}

impl Drop for LibraryWorker {
    fn drop(&mut self) {
        if let Some(jobs) = &self.scan_jobs {
            let _ = jobs.send(ScanMessage::Shutdown);
        }
        if let Some(jobs) = &self.decode_jobs {
            let _ = jobs.send(DecodeMessage::Shutdown);
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn lock_library(state: &Mutex<LibraryState>) -> MutexGuard<'_, LibraryState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Scan all three profile folders into one index.
fn scan_all(media_root: &Path, profile: &str) -> BTreeMap<LibraryKind, KindScan> {
    LibraryKind::ALL
        .into_iter()
        .map(|kind| (kind, scan_kind(media_root, profile, kind)))
        .collect()
}

/// Publish a finished scan only if no newer scan was requested meanwhile.
fn apply_scan(state: &mut LibraryState, generation: u64, scans: BTreeMap<LibraryKind, KindScan>) {
    if state.generation == generation {
        state.scans = scans;
        state.revision = state.revision.wrapping_add(1);
    }
}

/// Scan all three folders for the newest requested generation.
///
/// Older/duplicate requests that are still queued when this runs are skipped:
/// their generation no longer matches the index the shell is waiting for.
fn run_scan_worker(state: Arc<Mutex<LibraryState>>, jobs: Receiver<ScanMessage>) {
    while let Ok(message) = jobs.recv() {
        let ScanMessage::Run {
            generation,
            media_root,
            profile,
        } = message
        else {
            break;
        };
        if lock_library(&state).generation != generation {
            continue;
        }
        let scans = scan_all(&media_root, &profile);
        apply_scan(&mut lock_library(&state), generation, scans);
    }
}

/// Decode one requested thumbnail and publish it to the shared cache.
fn run_decode_worker(state: Arc<Mutex<LibraryState>>, jobs: Receiver<DecodeMessage>) {
    while let Ok(message) = jobs.recv() {
        let DecodeMessage::Run { epoch, asset } = message else {
            break;
        };
        if lock_library(&state).decode_epoch != epoch {
            continue;
        }
        // Decoding reads the file/image and is the part that must stay off the
        // frame thread; a failure is cached as `None`, not panicked.
        let thumbnail = decode_asset_thumbnail(&asset);
        let mut state = lock_library(&state);
        if state.decode_epoch != epoch {
            continue;
        }
        state.pending.remove(&asset.absolute);
        state
            .thumbnails
            .insert(asset.absolute.clone(), thumbnail.clone());
        if let Some(thumbnail) = thumbnail {
            state.completed.push(CompletedThumbnail {
                kind: asset.kind,
                folder: asset.folder,
                path: asset.path,
                thumbnail,
            });
        }
    }
}

/// The scanned media index of one profile.
pub struct MediaLibrary {
    media_root: PathBuf,
    profile: String,
    state: Arc<Mutex<LibraryState>>,
    worker: LibraryWorker,
}

impl MediaLibrary {
    pub fn new(media_root: impl Into<PathBuf>, profile: impl Into<String>) -> Self {
        let state = Arc::new(Mutex::new(LibraryState::default()));
        Self {
            media_root: media_root.into(),
            profile: profile.into(),
            worker: LibraryWorker::new(Arc::clone(&state)),
            state,
        }
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Switch profile and drop the old index/decodes.
    pub fn set_profile(&mut self, profile: impl Into<String>) {
        let profile = profile.into();
        if self.profile == profile {
            return;
        }
        self.profile = profile;
        let mut state = lock_library(&self.state);
        state.scans.clear();
        state.invalidate_decodes();
        state.generation = state.generation.wrapping_add(1);
        state.revision = state.revision.wrapping_add(1);
    }

    pub fn generation(&self) -> u64 {
        lock_library(&self.state).generation
    }

    /// The Library index revision; it changes when a worker scan is applied.
    pub fn revision(&self) -> u64 {
        lock_library(&self.state).revision
    }

    /// Ask the worker to re-scan all three folders and drop the decoded
    /// thumbnails. Returns immediately with the new generation.
    pub fn rescan(&mut self) -> u64 {
        let (generation, media_root, profile) = {
            let mut state = lock_library(&self.state);
            state.generation = state.generation.wrapping_add(1);
            state.invalidate_decodes();
            (
                state.generation,
                self.media_root.clone(),
                self.profile.clone(),
            )
        };
        self.worker.request_scan(generation, media_root, profile);
        generation
    }

    /// Re-scan synchronously. Used at startup (before the window exists) and
    /// by callers/tests that need the index immediately.
    pub fn rescan_blocking(&mut self) -> u64 {
        let generation = {
            let mut state = lock_library(&self.state);
            state.generation = state.generation.wrapping_add(1);
            state.invalidate_decodes();
            state.generation
        };
        let scans = scan_all(&self.media_root, &self.profile);
        let mut state = lock_library(&self.state);
        // A newer request/blocking scan must not be overwritten by this one.
        apply_scan(&mut state, generation, scans);
        generation
    }

    pub fn clear_thumbnails(&mut self) {
        lock_library(&self.state).invalidate_decodes();
    }

    /// Every scanned asset, in kind then path order.
    pub fn assets(&self) -> impl Iterator<Item = ScannedAsset> {
        let state = lock_library(&self.state);
        state
            .scans
            .values()
            .flat_map(|scan| scan.assets.iter().cloned())
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub fn total_count(&self) -> usize {
        lock_library(&self.state)
            .scans
            .values()
            .map(|scan| scan.assets.len())
            .sum()
    }

    /// The total number of scanned assets in one folder.
    pub fn count_for(&self, kind: LibraryKind) -> usize {
        lock_library(&self.state)
            .scans
            .get(&kind)
            .map_or(0, |scan| scan.assets.len())
    }

    /// Decode up to `limit` missing thumbnails for the visible kind/scope.
    ///
    /// Returns only thumbnails that are already decoded; newly requested
    /// decodes run on the worker and surface to a later call. Decoded entries
    /// (including failures) stay cached; the cache is capped at
    /// [`MAX_THUMBNAILS`].
    pub fn decode_thumbnails(
        &mut self,
        kind: LibraryKind,
        folder: &str,
        limit: usize,
    ) -> Vec<(String, Thumbnail)> {
        if limit == 0 {
            return Vec::new();
        }
        let scope = modv_ui::model::normalise_library_folder(folder);
        let mut guard = lock_library(&self.state);
        let mut submissions = Vec::new();
        let decoded = {
            let state = &mut *guard;

            let mut deferred = Vec::new();
            let mut decoded = Vec::new();
            for completed in std::mem::take(&mut state.completed) {
                if completed.kind == kind
                    && modv_ui::model::folder_in_scope(&completed.folder, &scope)
                    && decoded.len() < limit
                {
                    decoded.push((completed.path, completed.thumbnail));
                } else {
                    deferred.push(completed);
                }
            }
            state.completed = deferred;

            let remaining = limit.saturating_sub(decoded.len());
            if remaining > 0 {
                let epoch = state.decode_epoch;
                if let Some(scan) = state.scans.get(&kind) {
                    // Only clone the few assets we actually submit, not the
                    // whole visible folder, on every frame the panel is open.
                    for asset in &scan.assets {
                        if submissions.len() >= remaining {
                            break;
                        }
                        if !modv_ui::model::folder_in_scope(&asset.folder, &scope) {
                            continue;
                        }
                        if state.thumbnails.contains_key(&asset.absolute)
                            || state.pending.contains(&asset.absolute)
                        {
                            continue;
                        }
                        if state.thumbnails.len() + state.pending.len() >= MAX_THUMBNAILS {
                            break;
                        }
                        state.pending.insert(asset.absolute.clone());
                        submissions.push((epoch, asset.clone()));
                    }
                }
            }
            decoded
        };
        drop(guard);

        for (epoch, asset) in submissions {
            self.worker.request_decode(epoch, asset);
        }
        decoded
    }

    /// Decode up to `limit` missing thumbnails synchronously.
    ///
    /// Existing tests and startup-adjacent callers use this when they need the
    /// pixels in the same call; the runtime panel uses [`Self::decode_thumbnails`].
    pub fn decode_thumbnails_blocking(
        &mut self,
        kind: LibraryKind,
        folder: &str,
        limit: usize,
    ) -> Vec<(String, Thumbnail)> {
        if limit == 0 {
            return Vec::new();
        }
        let scope = modv_ui::model::normalise_library_folder(folder);
        let mut guard = lock_library(&self.state);
        let state = &mut *guard;
        // This call owns the decode cache while it runs; discard anything the
        // worker still had in flight for the previous epoch, but keep the
        // already decoded thumbnails cached.
        state.pending.clear();
        state.completed.clear();
        state.decode_epoch = state.decode_epoch.wrapping_add(1);
        let Some(scan) = state.scans.get(&kind) else {
            return Vec::new();
        };
        let mut decoded = Vec::new();
        for asset in &scan.assets {
            if !modv_ui::model::folder_in_scope(&asset.folder, &scope) {
                continue;
            }
            if state.thumbnails.contains_key(&asset.absolute) {
                continue;
            }
            if state.thumbnails.len() >= MAX_THUMBNAILS {
                break;
            }
            let thumbnail = decode_asset_thumbnail(asset);
            state
                .thumbnails
                .insert(asset.absolute.clone(), thumbnail.clone());
            if let Some(thumbnail) = thumbnail {
                decoded.push((asset.path.clone(), thumbnail));
            }
            if decoded.len() >= limit {
                break;
            }
        }
        decoded
    }

    /// Build the UI read model, attaching uploaded thumbnail textures.
    pub fn view(&self, textures: &HashMap<String, PreviewTexture>) -> MediaLibraryView {
        let state = lock_library(&self.state);
        let mut assets = Vec::new();
        let mut truncated = false;
        let mut errors = Vec::new();
        for (kind, scan) in &state.scans {
            truncated |= scan.truncated;
            if let Some(error) = &scan.error {
                errors.push(format!("{}: {error}", kind.folder()));
            }
            for asset in &scan.assets {
                assets.push(asset.to_view(textures.get(&asset.path).copied()));
            }
        }
        MediaLibraryView {
            profile: self.profile.clone(),
            assets,
            truncated,
            error: (!errors.is_empty()).then(|| errors.join("; ")),
        }
    }
}

impl Default for MediaLibrary {
    fn default() -> Self {
        Self::new(PathBuf::new(), modv_ui::DEFAULT_PROFILE_NAME)
    }
}

/// Coalesced rescan requests from the media watcher and UI gestures.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryRefresh {
    pending: bool,
    generation: u64,
}

impl LibraryRefresh {
    /// A watcher burst asks for one rescan when it touched a library folder.
    pub fn note_burst(&mut self, burst: &MediaBurst) {
        if burst.touches_library() {
            self.pending = true;
        }
    }

    /// A UI gesture (Rescan, profile switch) asks for a rescan.
    pub fn request(&mut self) {
        self.pending = true;
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// Take the pending request; `None` when nothing changed since the last
    /// scan. One burst can only produce one scan.
    pub fn take(&mut self) -> Option<u64> {
        if !self.pending {
            return None;
        }
        self.pending = false;
        self.generation += 1;
        Some(self.generation)
    }
}

/// Recursively scan one profile folder.
pub fn scan_kind(media_root: &Path, profile: &str, kind: LibraryKind) -> KindScan {
    let root = crate::media_profiles::profile_folder(media_root, profile, kind.folder());
    let mut scan = KindScan::default();
    let mut stop = false;
    walk_kind(profile, kind, &root, &root, 0, &mut scan, &mut stop);
    scan.assets.sort_by(|left, right| {
        left.path
            .to_lowercase()
            .cmp(&right.path.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    scan
}

/// The watcher folder whose extension list this library kind mirrors.
fn watcher_folder(kind: LibraryKind) -> MediaFolder {
    match kind {
        LibraryKind::Image => MediaFolder::Image,
        LibraryKind::Video => MediaFolder::Video,
        LibraryKind::Palette => MediaFolder::Palette,
    }
}

/// Walk one directory depth-first in sorted order, collecting matching files.
fn walk_kind(
    profile: &str,
    kind: LibraryKind,
    root: &Path,
    dir: &Path,
    depth: usize,
    scan: &mut KindScan,
    stop: &mut bool,
) {
    if *stop || depth > MAX_SCAN_DEPTH {
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            scan.error = Some(format!("could not read {}: {error}", dir.display()));
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
        if *stop {
            return;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if media_watch::is_ignored_media_name(&name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            // Symlinked directories are skipped so a link back up the tree
            // cannot turn the recursive scan into an infinite loop.
            if !file_type.is_symlink() {
                walk_kind(profile, kind, root, &path, depth + 1, scan, stop);
            }
            continue;
        }
        if !file_type.is_file() || !media_watch::supported(watcher_folder(kind), &name) {
            continue;
        }
        if scan.assets.len() >= MAX_ASSETS_PER_KIND {
            scan.truncated = true;
            *stop = true;
            return;
        }
        let size_bytes = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        let folder = relative_folder(root, &path);
        let kind_folder = kind.folder();
        let relative = if folder.is_empty() {
            format!("{profile}/{kind_folder}/{name}")
        } else {
            format!("{profile}/{kind_folder}/{folder}/{name}")
        };
        let (dimensions, duration_ms) = match kind {
            LibraryKind::Image => (
                image::image_dimensions(&path)
                    .ok()
                    .map(|(width, height)| [width, height]),
                None,
            ),
            LibraryKind::Video => match video_metadata(&path) {
                Some(metadata) => (
                    (metadata.width > 0 && metadata.height > 0)
                        .then_some([metadata.width, metadata.height]),
                    metadata.duration_ms,
                ),
                None => (None, None),
            },
            LibraryKind::Palette => (None, None),
        };
        let palette = (kind == LibraryKind::Palette)
            .then(|| read_palette_file(&path))
            .flatten();
        scan.assets.push(ScannedAsset {
            path: relative,
            absolute: path,
            kind,
            name,
            folder,
            size_bytes,
            dimensions,
            duration_ms,
            palette,
        });
    }
}

fn read_palette_file(path: &Path) -> Option<Vec<[u8; 4]>> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_PALETTE_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    parse_palette_json(&text).ok()
}

/// Parse a palette JSON document into RGBA colours.
///
/// Electron's colour picker stores `{ r, g, b, a }` objects with 0–1 channels;
/// palette files written by hand may use hex strings or 0–255 channel arrays.
/// The document may be a bare array or an object with a `colors`/`colours`/
/// `palette` array. Invalid entries are skipped; a document with no usable
/// colour is an error.
pub fn parse_palette_json(text: &str) -> Result<Vec<[u8; 4]>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid palette JSON: {error}"))?;
    let entries = match &value {
        serde_json::Value::Array(entries) => entries.as_slice(),
        serde_json::Value::Object(map) => map
            .get("colors")
            .or_else(|| map.get("colours"))
            .or_else(|| map.get("palette"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .ok_or_else(|| "the palette has no colours array".to_string())?,
        _ => return Err("the palette must be an array or an object".to_string()),
    };
    let mut colours = Vec::new();
    for entry in entries {
        if let Some(colour) = parse_palette_colour(entry) {
            colours.push(colour);
            if colours.len() == MAX_PALETTE_COLOURS {
                break;
            }
        }
    }
    if colours.is_empty() {
        return Err("the palette has no usable colours".to_string());
    }
    Ok(colours)
}

fn parse_palette_colour(value: &serde_json::Value) -> Option<[u8; 4]> {
    match value {
        serde_json::Value::String(text) => parse_hex_colour(text),
        serde_json::Value::Array(channels) => {
            if channels.len() < 3 {
                return None;
            }
            let r = channels[0].as_f64()? as f32;
            let g = channels[1].as_f64()? as f32;
            let b = channels[2].as_f64()? as f32;
            let alpha = channels.get(3).and_then(serde_json::Value::as_f64);
            Some(channels_to_rgba([r, g, b], alpha.map(|value| value as f32)))
        }
        serde_json::Value::Object(map) => {
            let r = map.get("r")?.as_f64()? as f32;
            let g = map.get("g")?.as_f64()? as f32;
            let b = map.get("b")?.as_f64()? as f32;
            let alpha = map
                .get("a")
                .or_else(|| map.get("alpha"))
                .and_then(serde_json::Value::as_f64);
            Some(channels_to_rgba([r, g, b], alpha.map(|value| value as f32)))
        }
        _ => None,
    }
}

/// Scale a colour whose channels are either 0–1 floats or 0–255 integers.
fn channels_to_rgba(rgb: [f32; 3], alpha: Option<f32>) -> [u8; 4] {
    let rgb_scaled = rgb.iter().any(|channel| *channel > 1.0);
    let channel = |value: f32| {
        let value = if value.is_finite() {
            if rgb_scaled { value } else { value * 255.0 }
        } else {
            0.0
        };
        value.clamp(0.0, 255.0).round() as u8
    };
    let alpha = match alpha {
        Some(alpha) if alpha.is_finite() => {
            let alpha = if alpha > 1.0 { alpha } else { alpha * 255.0 };
            alpha.clamp(0.0, 255.0).round() as u8
        }
        _ => 255,
    };
    [channel(rgb[0]), channel(rgb[1]), channel(rgb[2]), alpha]
}

fn parse_hex_colour(text: &str) -> Option<[u8; 4]> {
    let hex = text.trim().strip_prefix('#')?;
    let digits = |value: &str| u8::from_str_radix(value, 16).ok();
    match hex.len() {
        3 => {
            let r = digits(&hex[0..1])?;
            let g = digits(&hex[1..2])?;
            let b = digits(&hex[2..3])?;
            Some([r * 17, g * 17, b * 17, 255])
        }
        4 => {
            let r = digits(&hex[0..1])?;
            let g = digits(&hex[1..2])?;
            let b = digits(&hex[2..3])?;
            let a = digits(&hex[3..4])?;
            Some([r * 17, g * 17, b * 17, a * 17])
        }
        6 => Some([
            digits(&hex[0..2])?,
            digits(&hex[2..4])?,
            digits(&hex[4..6])?,
            255,
        ]),
        8 => Some([
            digits(&hex[0..2])?,
            digits(&hex[2..4])?,
            digits(&hex[4..6])?,
            digits(&hex[6..8])?,
        ]),
        _ => None,
    }
}

/// Downscale RGBA pixels to fit `max`, preserving the aspect ratio.
pub fn downscale_thumbnail(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    max: (u32, u32),
) -> Option<Thumbnail> {
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return None;
    }
    let (max_width, max_height) = (max.0.max(1), max.1.max(1));
    if width <= max_width && height <= max_height {
        return Some(Thumbnail {
            width,
            height,
            rgba,
        });
    }
    let scale = (max_width as f32 / width as f32).min(max_height as f32 / height as f32);
    let target_width = ((width as f32 * scale).round() as u32).max(1);
    let target_height = ((height as f32 * scale).round() as u32).max(1);
    let image = image::RgbaImage::from_raw(width, height, rgba)?;
    let resized = image::imageops::resize(
        &image,
        target_width,
        target_height,
        image::imageops::FilterType::Triangle,
    );
    Some(Thumbnail {
        width: resized.width(),
        height: resized.height(),
        rgba: resized.into_raw(),
    })
}

/// The colour-strip preview for a parsed palette.
pub fn palette_thumbnail(colours: &[[u8; 4]]) -> Option<Thumbnail> {
    let colours = &colours[..colours.len().min(MAX_PALETTE_COLOURS)];
    if colours.is_empty() {
        return None;
    }
    let width = THUMBNAIL_MAX_SIZE.0;
    let height = (THUMBNAIL_MAX_SIZE.1 / 2).max(24);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let stripe = width.div_ceil(colours.len() as u32);
    for y in 0..height {
        for x in 0..width {
            let index = ((x / stripe) as usize).min(colours.len() - 1);
            let colour = colours[index];
            let offset = ((y * width + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&colour);
        }
    }
    Some(Thumbnail {
        width,
        height,
        rgba,
    })
}

/// Video metadata read from a container header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
}

/// Parse an MP4 `moov`/`mvhd`/`tkhd` header.
pub fn mp4_metadata(bytes: &[u8]) -> Option<VideoMetadata> {
    let mut offset = 0;
    while let Some((kind, content)) = next_mp4_box(bytes, &mut offset) {
        if kind == b"moov" {
            return parse_moov(content);
        }
    }
    None
}

fn parse_moov(moov: &[u8]) -> Option<VideoMetadata> {
    let mut offset = 0;
    let mut duration_ms = None;
    let mut dimensions = None;
    while let Some((kind, content)) = next_mp4_box(moov, &mut offset) {
        match kind {
            b"mvhd" => duration_ms = parse_mvhd(content),
            b"trak" if dimensions.is_none() => {
                dimensions = parse_trak_dimensions(content);
            }
            _ => {}
        }
    }
    let [width, height] = dimensions?;
    Some(VideoMetadata {
        width,
        height,
        duration_ms,
    })
}

/// One MP4 box: its four-character type and content slice.
fn next_mp4_box<'a>(bytes: &'a [u8], offset: &mut usize) -> Option<(&'a [u8; 4], &'a [u8])> {
    let start = *offset;
    if bytes.len().saturating_sub(start) < 8 {
        return None;
    }
    let size32 = u32::from_be_bytes(bytes[start..start + 4].try_into().ok()?);
    let kind: &[u8; 4] = bytes[start + 4..start + 8].try_into().ok()?;
    let (size, header) = match size32 {
        0 => ((bytes.len() - start) as u64, 8usize),
        1 => {
            if bytes.len().saturating_sub(start) < 16 {
                return None;
            }
            (
                u64::from_be_bytes(bytes[start + 8..start + 16].try_into().ok()?),
                16usize,
            )
        }
        size => (u64::from(size), 8usize),
    };
    if size < header as u64 {
        return None;
    }
    let end = start.checked_add(usize::try_from(size).ok()?)?;
    if end > bytes.len() {
        return None;
    }
    *offset = end;
    Some((kind, &bytes[start + header..end]))
}

fn parse_mvhd(content: &[u8]) -> Option<u64> {
    let version = *content.first()?;
    let (timescale, duration) = if version == 1 {
        (
            u32::from_be_bytes(content.get(20..24)?.try_into().ok()?),
            u64::from_be_bytes(content.get(24..32)?.try_into().ok()?),
        )
    } else {
        (
            u32::from_be_bytes(content.get(12..16)?.try_into().ok()?),
            u64::from(u32::from_be_bytes(content.get(16..20)?.try_into().ok()?)),
        )
    };
    (timescale > 0).then(|| (u128::from(duration) * 1000 / u128::from(timescale)) as u64)
}

fn parse_trak_dimensions(trak: &[u8]) -> Option<[u32; 2]> {
    let mut offset = 0;
    while let Some((kind, content)) = next_mp4_box(trak, &mut offset) {
        if kind != b"tkhd" {
            continue;
        }
        let version = *content.first()?;
        let (width_offset, height_offset) = if version == 1 { (88, 92) } else { (76, 80) };
        let width = u32::from_be_bytes(
            content
                .get(width_offset..width_offset + 4)?
                .try_into()
                .ok()?,
        );
        let height = u32::from_be_bytes(
            content
                .get(height_offset..height_offset + 4)?
                .try_into()
                .ok()?,
        );
        // 16.16 fixed point.
        let width = width >> 16;
        let height = height >> 16;
        if width > 0 && height > 0 {
            return Some([width, height]);
        }
    }
    None
}

/// EBML element ids the WebM reader understands.
const EBML_SEGMENT: u64 = 0x1853_8067;
const EBML_INFO: u64 = 0x1549_A966;
const EBML_TRACKS: u64 = 0x1654_AE6B;
const EBML_TRACK_ENTRY: u64 = 0xAE;
const EBML_VIDEO: u64 = 0xE0;
const EBML_TIMECODE_SCALE: u64 = 0x002A_D7B1;
const EBML_DURATION: u64 = 0x4489;
const EBML_PIXEL_WIDTH: u64 = 0xB0;
const EBML_PIXEL_HEIGHT: u64 = 0xBA;

#[derive(Default)]
struct WebmFields {
    timecode_scale: Option<u64>,
    duration: Option<f64>,
    width: Option<u64>,
    height: Option<u64>,
}

/// Parse a WebM EBML `Info`/`Tracks` header.
pub fn webm_metadata(bytes: &[u8]) -> Option<VideoMetadata> {
    let mut fields = WebmFields::default();
    parse_ebml(bytes, 0, bytes.len(), 0, &mut fields);
    let width = u32::try_from(fields.width?).ok()?;
    let height = u32::try_from(fields.height?).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    let duration_ms = fields.duration.map(|duration| {
        let scale = fields.timecode_scale.unwrap_or(1_000_000);
        (duration * scale as f64 / 1_000_000.0).max(0.0).round() as u64
    });
    Some(VideoMetadata {
        width,
        height,
        duration_ms,
    })
}

fn parse_ebml(bytes: &[u8], start: usize, end: usize, depth: usize, fields: &mut WebmFields) {
    if depth > 6 {
        return;
    }
    let mut position = start;
    while position < end {
        let Some((id, id_len)) = read_ebml_vint(bytes, position, true) else {
            return;
        };
        let Some((size, size_len)) = read_ebml_vint(bytes, position + id_len, false) else {
            return;
        };
        let content_start = position + id_len + size_len;
        let Ok(size) = usize::try_from(size) else {
            return;
        };
        let Some(content_end) = content_start.checked_add(size) else {
            return;
        };
        if content_end > end {
            return;
        }
        let content = &bytes[content_start..content_end];
        match id {
            EBML_SEGMENT | EBML_INFO | EBML_TRACKS | EBML_TRACK_ENTRY | EBML_VIDEO => {
                parse_ebml(bytes, content_start, content_end, depth + 1, fields);
            }
            EBML_TIMECODE_SCALE => fields.timecode_scale = ebml_uint(content),
            EBML_DURATION => fields.duration = ebml_float(content),
            EBML_PIXEL_WIDTH => fields.width = ebml_uint(content),
            EBML_PIXEL_HEIGHT => fields.height = ebml_uint(content),
            _ => {}
        }
        position = content_end;
    }
}

/// Read an EBML variable-length integer. IDs keep their marker bits; sizes
/// strip them.
fn read_ebml_vint(bytes: &[u8], position: usize, keep_marker: bool) -> Option<(u64, usize)> {
    let first = *bytes.get(position)?;
    let length = first.leading_zeros() as usize + 1;
    if length > 8 || position + length > bytes.len() {
        return None;
    }
    let mut value = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first & (0xFFu8 >> length))
    };
    for index in 1..length {
        value = (value << 8) | u64::from(bytes[position + index]);
    }
    Some((value, length))
}

fn ebml_uint(content: &[u8]) -> Option<u64> {
    if content.is_empty() || content.len() > 8 {
        return None;
    }
    Some(
        content
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)),
    )
}

fn ebml_float(content: &[u8]) -> Option<f64> {
    match content.len() {
        4 => Some(f64::from(f32::from_be_bytes(content.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(content.try_into().ok()?)),
        _ => None,
    }
}

/// Read video metadata from the file, handling a trailing `moov` atom.
pub fn video_metadata(path: &Path) -> Option<VideoMetadata> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("webm" | "webv") => {
            let file = File::open(path).ok()?;
            let mut bytes = Vec::new();
            file.take(8 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
            webm_metadata(&bytes)
        }
        _ => mp4_metadata_from_file(path),
    }
}

/// Walk the MP4 top-level boxes on disk, so a `moov` at the end of a large
/// file is found without reading the media payload.
fn mp4_metadata_from_file(path: &Path) -> Option<VideoMetadata> {
    let file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut reader = BufReader::new(file);
    let mut offset = 0u64;
    while offset + 8 <= length {
        reader.seek(SeekFrom::Start(offset)).ok()?;
        let mut header = [0u8; 8];
        reader.read_exact(&mut header).ok()?;
        let size32 = u32::from_be_bytes(header[0..4].try_into().ok()?);
        let kind = &header[4..8];
        let (size, header_len) = match size32 {
            0 => (length - offset, 8u64),
            1 => {
                let mut large = [0u8; 8];
                reader.read_exact(&mut large).ok()?;
                (u64::from_be_bytes(large), 16u64)
            }
            size => (u64::from(size), 8u64),
        };
        if size < header_len || offset + size > length {
            return None;
        }
        if kind == b"moov" {
            let content_len = usize::try_from(size - header_len).ok()?.min(MAX_MOOV_BYTES);
            let mut content = vec![0u8; content_len];
            reader.seek(SeekFrom::Start(offset + header_len)).ok()?;
            reader.read_exact(&mut content).ok()?;
            return parse_moov(&content);
        }
        offset += size;
    }
    None
}

fn decode_asset_thumbnail(asset: &ScannedAsset) -> Option<Thumbnail> {
    match asset.kind {
        LibraryKind::Image => {
            let decoded = crate::texture_sources::decode_file(&asset.absolute).ok()?;
            downscale_thumbnail(
                decoded.width,
                decoded.height,
                decoded.rgba,
                THUMBNAIL_MAX_SIZE,
            )
        }
        LibraryKind::Palette => palette_thumbnail(asset.palette.as_deref()?),
        LibraryKind::Video => None,
    }
}

/// Path components of `profile/kind/...` below the kind root.
fn relative_folder(root: &Path, path: &Path) -> String {
    let Ok(relative) = path.strip_prefix(root) else {
        return String::new();
    };
    let mut parts: Vec<String> = relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| match component {
            Component::Normal(name) => name.to_str().map(str::to_string),
            _ => None,
        })
        .collect();
    parts.retain(|part| !part.is_empty());
    parts.join("/")
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

    fn write_png(path: &Path, width: u32, height: u32, colour: [u8; 4]) {
        std::fs::create_dir_all(path.parent().unwrap()).expect("parent");
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba(colour));
        image.save(path).expect("save png");
    }

    #[test]
    fn the_scan_is_recursive_deterministic_and_classified() {
        let root = temp_dir("modv-library-scan");
        write_png(&root.join("live/image/b.png"), 4, 4, [255, 0, 0, 255]);
        write_png(
            &root.join("live/image/nested/a.png"),
            4,
            2,
            [0, 255, 0, 255],
        );
        std::fs::write(root.join("live/image/notes.txt"), "x").expect("notes");
        std::fs::write(root.join("live/image/.hidden.png"), "x").expect("hidden");
        std::fs::write(root.join("live/image/swap.png~"), "x").expect("temp");
        std::fs::create_dir_all(root.join("live/image/.cache")).expect("hidden dir");
        std::fs::write(root.join("live/image/.cache/ignored.png"), "x").expect("hidden nested");
        std::fs::create_dir_all(root.join("live/video")).expect("video dir");
        std::fs::write(root.join("live/video/clip.mp4"), "not a real mp4").expect("video");
        std::fs::create_dir_all(root.join("live/palette")).expect("palette dir");
        std::fs::write(root.join("live/palette/neon.json"), "[]").expect("palette");

        let images = scan_kind(&root, "live", LibraryKind::Image);
        assert_eq!(images.error, None);
        let names: Vec<&str> = images
            .assets
            .iter()
            .map(|asset| asset.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["b.png", "a.png"],
            "root files sort before a nested folder; hidden/temp names are ignored"
        );
        assert_eq!(images.assets[0].folder, "");
        assert_eq!(images.assets[0].path, "live/image/b.png");
        assert_eq!(images.assets[0].dimensions, Some([4, 4]));
        assert_eq!(images.assets[1].folder, "nested");
        assert_eq!(images.assets[1].path, "live/image/nested/a.png");
        assert_eq!(images.assets[1].dimensions, Some([4, 2]));

        let videos = scan_kind(&root, "live", LibraryKind::Video);
        assert_eq!(videos.assets.len(), 1);
        assert_eq!(videos.assets[0].name, "clip.mp4");
        assert_eq!(
            videos.assets[0].dimensions, None,
            "a bogus container header is not an error"
        );

        let palettes = scan_kind(&root, "live", LibraryKind::Palette);
        assert_eq!(palettes.assets.len(), 1);
        assert_eq!(palettes.assets[0].name, "neon.json");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_scan_caps_entries_and_marks_truncation() {
        let root = temp_dir("modv-library-cap");
        for index in 0..(MAX_ASSETS_PER_KIND + 5) {
            write_png(
                &root.join(format!("live/image/f{index:03}.png")),
                1,
                1,
                [1, 2, 3, 255],
            );
        }

        let scan = scan_kind(&root, "live", LibraryKind::Image);

        assert_eq!(scan.assets.len(), MAX_ASSETS_PER_KIND);
        assert!(scan.truncated, "the panel says the folder was capped");
        assert_eq!(scan.assets[0].name, "f000.png");
        assert_eq!(
            scan.assets[MAX_ASSETS_PER_KIND - 1].name,
            format!("f{:03}.png", MAX_ASSETS_PER_KIND - 1)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_scan_stops_at_the_depth_limit() {
        let root = temp_dir("modv-library-depth");
        let mut dir = root.join("live/image");
        for index in 0..=MAX_SCAN_DEPTH {
            dir = dir.join(format!("d{index}"));
        }
        write_png(&dir.join("too-deep.png"), 1, 1, [1, 1, 1, 255]);
        // A sibling at the allowed depth is still found.
        let mut allowed = root.join("live/image");
        for index in 0..MAX_SCAN_DEPTH {
            allowed = allowed.join(format!("d{index}"));
        }
        write_png(&allowed.join("visible.png"), 1, 1, [1, 1, 1, 255]);

        let scan = scan_kind(&root, "live", LibraryKind::Image);
        let names: Vec<&str> = scan
            .assets
            .iter()
            .map(|asset| asset.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["visible.png"],
            "a folder deeper than MAX_SCAN_DEPTH is not descended"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_folder_is_empty_and_a_file_named_like_it_is_an_error() {
        let root = temp_dir("modv-library-errors");
        std::fs::create_dir_all(root.join("live")).expect("profile");
        let missing = scan_kind(&root, "live", LibraryKind::Video);
        assert!(missing.assets.is_empty());
        assert_eq!(missing.error, None, "an empty profile is not an error");

        std::fs::write(root.join("live/image"), "not a folder").expect("file");
        let broken = scan_kind(&root, "live", LibraryKind::Image);
        assert!(broken.assets.is_empty());
        assert!(
            broken
                .error
                .as_deref()
                .is_some_and(|error| error.contains("image")),
            "{:?}",
            broken.error
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn palette_json_accepts_objects_hex_and_channel_arrays() {
        let colours = parse_palette_json(
            r##"{
                "colors": [
                    {"r": 1.0, "g": 0.0, "b": 0.5, "a": 1.0},
                    "#00ff00",
                    {"r": 255, "g": 128, "b": 0},
                    [0, 0, 255, 255],
                    [1.0, 1.0, 1.0]
                ]
            }"##,
        )
        .expect("palette");
        assert_eq!(colours.len(), 5);
        assert_eq!(colours[0], [255, 0, 128, 255]);
        assert_eq!(colours[1], [0, 255, 0, 255]);
        assert_eq!(colours[2], [255, 128, 0, 255]);
        assert_eq!(colours[3], [0, 0, 255, 255]);
        assert_eq!(colours[4], [255, 255, 255, 255]);

        let flat = parse_palette_json(r##"["#f00", "#0f08"]"##).expect("array root");
        assert_eq!(flat, vec![[255, 0, 0, 255], [0, 255, 0, 136]]);
    }

    #[test]
    fn palette_json_rejects_documents_without_colours_and_caps_the_strip() {
        assert!(parse_palette_json("{}").is_err());
        assert!(parse_palette_json(r#"{"colors": ["not a colour"]}"#).is_err());
        assert!(parse_palette_json("not json").is_err());

        let many: Vec<String> = (0..40).map(|_| "\"#abcdef\"".to_string()).collect();
        let parsed = parse_palette_json(&format!("[{}]", many.join(","))).expect("colours");
        assert_eq!(parsed.len(), MAX_PALETTE_COLOURS);
    }

    #[test]
    fn thumbnails_downscale_without_upscaling() {
        let pixels = vec![255u8; 400 * 100 * 4];
        let thumbnail = downscale_thumbnail(400, 100, pixels, (256, 144)).expect("thumbnail");
        assert_eq!((thumbnail.width, thumbnail.height), (256, 64));
        assert_eq!(thumbnail.rgba.len(), 256 * 64 * 4);

        let small = vec![9u8; 8 * 4 * 4];
        let thumbnail = downscale_thumbnail(8, 4, small, (256, 144)).expect("unchanged");
        assert_eq!((thumbnail.width, thumbnail.height), (8, 4));

        assert_eq!(downscale_thumbnail(2, 2, vec![0; 3], (256, 144)), None);
        assert_eq!(downscale_thumbnail(0, 0, Vec::new(), (256, 144)), None);
    }

    #[test]
    fn a_palette_thumbnail_is_one_stripe_per_colour() {
        let thumbnail = palette_thumbnail(&[
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ])
        .expect("strip");
        assert_eq!(thumbnail.width, THUMBNAIL_MAX_SIZE.0);
        assert_eq!(
            thumbnail.rgba.len(),
            (thumbnail.width * thumbnail.height * 4) as usize
        );
        let stripe = thumbnail.width / 4;
        let pixel = |x: u32, y: u32| {
            let offset = ((y * thumbnail.width + x) * 4) as usize;
            [
                thumbnail.rgba[offset],
                thumbnail.rgba[offset + 1],
                thumbnail.rgba[offset + 2],
                thumbnail.rgba[offset + 3],
            ]
        };
        assert_eq!(pixel(stripe / 2, 10), [255, 0, 0, 255]);
        assert_eq!(pixel(stripe * 3 + stripe / 2, 10), [255, 255, 255, 255]);
        assert_eq!(palette_thumbnail(&[]), None);
    }

    #[test]
    fn image_and_palette_thumbnails_are_decoded_once_and_cached() {
        let root = temp_dir("modv-library-thumbs");
        write_png(
            &root.join("live/image/still.png"),
            40,
            20,
            [12, 34, 56, 255],
        );
        std::fs::create_dir_all(root.join("live/palette")).expect("palette dir");
        std::fs::write(
            root.join("live/palette/neon.json"),
            r##"["#ff0000", "#00ff00"]"##,
        )
        .expect("palette");

        let mut library = MediaLibrary::new(&root, "live");
        library.rescan_blocking();

        let decoded = library.decode_thumbnails_blocking(LibraryKind::Image, "", 8);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].0, "live/image/still.png");
        assert_eq!((decoded[0].1.width, decoded[0].1.height), (40, 20));
        assert_eq!(
            library.decode_thumbnails_blocking(LibraryKind::Image, "", 8),
            Vec::new(),
            "the decode is cached, not repeated every frame"
        );

        let palettes = library.decode_thumbnails_blocking(LibraryKind::Palette, "", 8);
        assert_eq!(palettes.len(), 1);
        assert_eq!(palettes[0].0, "live/palette/neon.json");
        assert_eq!(palettes[0].1.width, THUMBNAIL_MAX_SIZE.0);

        let textures = HashMap::from([(
            "live/image/still.png".to_string(),
            PreviewTexture {
                id: egui::TextureId::User(11),
                width: 40,
                height: 20,
            },
        )]);
        let view = library.view(&textures);
        assert_eq!(view.profile, "live");
        assert_eq!(view.assets.len(), 2, "images and palettes share the index");
        assert!(
            view.assets
                .iter()
                .any(|asset| asset.path == "live/image/still.png" && asset.thumbnail.is_some())
        );

        library.clear_thumbnails();
        assert_eq!(
            library
                .decode_thumbnails_blocking(LibraryKind::Image, "", 8)
                .len(),
            1,
            "a rescan/clear drops the decode cache"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn decoded_thumbnails_stay_within_the_cache_cap() {
        let root = temp_dir("modv-library-thumb-cap");
        for index in 0..(MAX_THUMBNAILS + 6) {
            std::fs::create_dir_all(root.join("live/palette")).expect("palette dir");
            std::fs::write(
                root.join(format!("live/palette/p{index:03}.json")),
                r##"["#ff0000"]"##,
            )
            .expect("palette");
        }
        let mut library = MediaLibrary::new(&root, "live");
        library.rescan_blocking();
        let mut decoded = 0;
        loop {
            let batch = library.decode_thumbnails_blocking(LibraryKind::Palette, "", 7);
            if batch.is_empty() {
                break;
            }
            decoded += batch.len();
        }
        assert_eq!(
            decoded, MAX_THUMBNAILS,
            "the decode cache never grows past its cap"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn mp4_box(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
        let mut out = ((content.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(content);
        out
    }

    fn mvhd_content(timescale: u32, duration: u32) -> Vec<u8> {
        let mut content = vec![0u8; 4];
        content.extend([0u8; 8]);
        content.extend(timescale.to_be_bytes());
        content.extend(duration.to_be_bytes());
        content.extend([0u8; 80]);
        content
    }

    fn tkhd_content(width: u32, height: u32) -> Vec<u8> {
        let mut content = vec![0u8; 76];
        // tkhd v0 width/height are 16.16 fixed point.
        content.extend((width << 16).to_be_bytes());
        content.extend((height << 16).to_be_bytes());
        content
    }

    fn sample_mp4(mdat_bytes: usize) -> Vec<u8> {
        let mut moov = mp4_box(b"mvhd", &mvhd_content(1000, 12_500));
        let tkhd = mp4_box(b"tkhd", &tkhd_content(1920, 1080));
        moov.extend(mp4_box(b"trak", &tkhd));
        let moov = mp4_box(b"moov", &moov);
        let mut out = mp4_box(b"ftyp", b"isom\0\0\0\0isommp41");
        out.extend(mp4_box(b"mdat", &vec![0u8; mdat_bytes]));
        out.extend(moov);
        out
    }

    #[test]
    fn mp4_headers_supply_duration_and_resolution() {
        let metadata = mp4_metadata(&sample_mp4(16)).expect("mp4 metadata");
        assert_eq!(metadata.width, 1920);
        assert_eq!(metadata.height, 1080);
        assert_eq!(metadata.duration_ms, Some(12_500));
    }

    fn ebml_id(id: &[u8], content: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        assert!(content.len() < 127);
        out.push(0x80 | content.len() as u8);
        out.extend_from_slice(content);
        out
    }

    fn sample_webm() -> Vec<u8> {
        let scale = ebml_id(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40]);
        // WebM Duration is in TimecodeScale units: 12 500 × 1 ms = 12.5 s.
        let duration = ebml_id(&[0x44, 0x89], &12_500.0f64.to_be_bytes());
        let info = ebml_id(&[0x15, 0x49, 0xA9, 0x66], &[scale, duration].concat());
        let width = ebml_id(&[0xB0], &[0x07, 0x80]);
        let height = ebml_id(&[0xBA], &[0x04, 0x38]);
        let video = ebml_id(&[0xE0], &[width, height].concat());
        let track = ebml_id(&[0xAE], &video);
        let tracks = ebml_id(&[0x16, 0x54, 0xAE, 0x6B], &track);
        ebml_id(&[0x18, 0x53, 0x80, 0x67], &[info, tracks].concat())
    }

    #[test]
    fn webm_headers_supply_duration_and_resolution() {
        let metadata = webm_metadata(&sample_webm()).expect("webm metadata");
        assert_eq!(metadata.width, 1920);
        assert_eq!(metadata.height, 1080);
        assert_eq!(metadata.duration_ms, Some(12_500));
    }

    #[test]
    fn video_metadata_reads_a_trailing_moov_atom() {
        let root = temp_dir("modv-library-video");
        let path = root.join("live/video/clip.mp4");
        std::fs::create_dir_all(path.parent().unwrap()).expect("parent");
        std::fs::write(&path, sample_mp4(4096)).expect("write mp4");

        let metadata = video_metadata(&path).expect("metadata");
        assert_eq!(metadata.width, 1920);
        assert_eq!(metadata.height, 1080);
        assert_eq!(metadata.duration_ms, Some(12_500));

        let webm = root.join("live/video/clip.webm");
        std::fs::write(&webm, sample_webm()).expect("write webm");
        let metadata = video_metadata(&webm).expect("webm metadata");
        assert_eq!(metadata.width, 1920);
        assert_eq!(metadata.duration_ms, Some(12_500));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_watcher_burst_requests_exactly_one_rescan() {
        let mut refresh = LibraryRefresh::default();
        let isf_only = MediaBurst {
            folders: vec![MediaFolder::Isf],
            events: 2,
        };
        refresh.note_burst(&isf_only);
        assert!(!refresh.is_pending());
        assert_eq!(refresh.take(), None);

        let image_burst = MediaBurst {
            folders: vec![MediaFolder::Image, MediaFolder::Palette],
            events: 3,
        };
        refresh.note_burst(&image_burst);
        assert_eq!(refresh.take(), Some(1));
        assert_eq!(
            refresh.take(),
            None,
            "the burst produces one scan, not one per frame"
        );

        refresh.request();
        assert_eq!(
            refresh.take(),
            Some(2),
            "a manual rescan is a new generation"
        );
    }

    #[test]
    fn the_watcher_burst_rescans_the_new_asset_into_the_index() {
        let root = temp_dir("modv-library-burst");
        let mut library = MediaLibrary::new(&root, "live");
        library.rescan_blocking();
        assert_eq!(library.total_count(), 0);

        write_png(&root.join("live/image/new.png"), 2, 2, [1, 1, 1, 255]);
        let mut refresh = LibraryRefresh::default();
        refresh.note_burst(&MediaBurst {
            folders: vec![MediaFolder::Image],
            events: 1,
        });
        assert!(refresh.take().is_some());
        library.rescan_blocking();
        assert_eq!(library.count_for(LibraryKind::Image), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn switching_profiles_drops_the_old_index() {
        let root = temp_dir("modv-library-profile");
        write_png(&root.join("one/image/one.png"), 2, 2, [1, 1, 1, 255]);
        write_png(&root.join("two/image/two.png"), 2, 2, [2, 2, 2, 255]);

        let mut library = MediaLibrary::new(&root, "one");
        library.rescan_blocking();
        assert_eq!(library.total_count(), 1);
        library.set_profile("two");
        assert_eq!(library.total_count(), 0, "the old profile is not shown");
        library.rescan_blocking();
        assert_eq!(library.total_count(), 1);
        assert_eq!(
            library.view(&HashMap::new()).assets[0].path,
            "two/image/two.png"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// Runtime Library work must not run on the frame thread: `rescan` only
    /// submits a scan, and `decode_thumbnails` returns the decodes that are
    /// already available while newly requested ones are produced off-thread
    /// and surface to a later call. The finished work must reach the Library
    /// view. Both operations are deliberately made expensive here so the old
    /// synchronous behaviour visibly blows a generous latency budget.
    #[test]
    fn runtime_library_work_does_not_block_the_frame_thread() {
        use std::time::{Duration, Instant};

        let root = temp_dir("modv-library-offthread");

        // A large 16:9 image: decoding it inline is far slower than the
        // budget, and its aspect ratio matches the expected 256x144 thumbnail.
        write_png(
            &root.join("live/image/hero.png"),
            4000,
            2250,
            [30, 80, 130, 255],
        );
        // Chunky, valid palettes: parsing them inline makes the scan slow.
        let palette_entry = r#"{"r":1,"g":0,"b":0}"#;
        let palette_json = format!(
            "[{}]",
            (0..10_000)
                .map(|_| palette_entry)
                .collect::<Vec<_>>()
                .join(",")
        );
        std::fs::create_dir_all(root.join("live/palette")).expect("palette dir");
        for index in 0..MAX_ASSETS_PER_KIND {
            std::fs::write(
                root.join(format!("live/palette/p{index:03}.json")),
                &palette_json,
            )
            .expect("palette file");
        }

        let mut library = MediaLibrary::new(&root, "live");

        let scan_started = Instant::now();
        library.rescan();
        let scan_elapsed = scan_started.elapsed();

        let decode_started = Instant::now();
        let first = library.decode_thumbnails(LibraryKind::Image, "", 1);
        let decode_elapsed = decode_started.elapsed();

        assert!(
            decode_elapsed < Duration::from_millis(100),
            "decode_thumbnails blocked the caller for {decode_elapsed:?}; the \
             image decode must run on a background worker"
        );
        assert!(
            first.is_empty(),
            "a newly requested decode is not available until the worker finishes"
        );
        assert!(
            scan_elapsed < Duration::from_millis(250),
            "rescan blocked the caller for {scan_elapsed:?}; the directory walk \
             and palette parsing must run on a background worker"
        );

        // The finished scan reaches the index the view reads.
        let deadline = Instant::now() + Duration::from_secs(30);
        while library.total_count() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            library.total_count(),
            MAX_ASSETS_PER_KIND + 1,
            "the worker's scan populates the index the view reads"
        );
        assert!(
            library
                .assets()
                .any(|asset| asset.path == "live/image/hero.png"),
            "the large image is part of the worker's index"
        );

        // The finished decode becomes available to a later call/poll.
        let mut decoded = first;
        let decode_deadline = Instant::now() + Duration::from_secs(30);
        while decoded.is_empty() && Instant::now() < decode_deadline {
            std::thread::sleep(Duration::from_millis(10));
            decoded = library.decode_thumbnails(LibraryKind::Image, "", 1);
        }
        assert_eq!(
            decoded.len(),
            1,
            "the worker's decoded thumbnail is available to a later call"
        );
        assert_eq!((decoded[0].1.width, decoded[0].1.height), (256, 144));

        let textures = HashMap::from([(
            decoded[0].0.clone(),
            PreviewTexture {
                id: egui::TextureId::User(7),
                width: 256,
                height: 144,
            },
        )]);
        let view = library.view(&textures);
        assert!(
            view.assets
                .iter()
                .any(|asset| { asset.path == "live/image/hero.png" && asset.thumbnail.is_some() }),
            "the Library view eventually shows the worker's asset and thumbnail"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
