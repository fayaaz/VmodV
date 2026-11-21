//! Video-file texture sources for the VmodV shell.
//!
//! The legacy renderer starts one hidden looping `<video>` per `video` texture
//! prop, owned by the texture rather than the Inspector, and exposes
//! Play/Pause, Restart and Speed 0.1–10. The VmodV shell has to do the same
//! with the engine's GStreamer `MediaHub`: this module scans the live store for
//! texture props of type `video` with an `options.path`, resolves each path
//! under `<userData>/media`, and reconciles a path→source map.
//!
//! # Lifecycle
//!
//! * One [`MediaHub`] file source per **unique absolute path**, keyed
//!   `video:<absolute path>` (the id the compositor already matches against a
//!   prop's path). Two props sharing a clip share one pipeline.
//! * A source is reference-counted by `(module id, prop name)`; it stops when
//!   the last reference disappears (Scene change, module removed, prop
//!   changed, profile switch, shutdown).
//! * Reconcile runs at [`RECONCILE_INTERVAL`] and only starts/stops a pipeline
//!   when the store's video props changed, so a UI repaint never restarts a
//!   running source. Scene changes change the module/prop set, which plans
//!   the stops and starts for the new Scene (transition ghosts keep both
//!   sources alive for the length of the crossfade).
//! * A missing file is logged once and left unresolved (the prop falls back to
//!   the canvas); a video-folder change from the media watcher retries it.
//! * Loop/pause/speed options are applied on start and at runtime. A pause or
//!   speed change is a transport command, never a pipeline restart.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use modv_engine::engine_state::EngineState;
use modv_media::video::{VideoSourceConfig, VideoTransport, clamp_playback_rate};
use modv_ui::VideoTransportAction;

use crate::media::{MediaError, MediaHub};

/// The persisted playback options of a video texture prop.
///
/// Defaults mirror the legacy `<video>` element: looping on (the renderer set
/// the `loop` attribute unconditionally), playing, real time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoOptions {
    pub looping: bool,
    pub paused: bool,
    pub playback_rate: f64,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            looping: true,
            paused: false,
            playback_rate: 1.0,
        }
    }
}

/// How often the shell rescans the store for video texture props.
///
/// The scan is a handful of JSON lookups; 10 Hz keeps a Scene change or a
/// Library drop responsive without touching the prop tree every frame.
pub const RECONCILE_INTERVAL: Duration = Duration::from_millis(100);

/// One `(module, prop)` pair that references a video file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoRef {
    pub module_id: String,
    pub prop: String,
}

/// A video texture prop found in the live store.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoPropRequest {
    pub reference: VideoRef,
    /// The media-root-relative (or absolute) path stored in `options.path`.
    pub raw_path: String,
    pub options: VideoOptions,
}

impl VideoPropRequest {
    /// Parse one prop value; `None` when it is not a video texture.
    pub fn from_prop(module_id: &str, prop: &str, value: &Value) -> Option<Self> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .map(|kind| kind.trim().to_lowercase())
            .unwrap_or_default();
        let location = value
            .get("location")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if kind != "video" && location != "videos/video" {
            return None;
        }

        let options = value.get("options")?;
        let raw_path = options.get("path").and_then(Value::as_str)?.trim();
        if raw_path.is_empty() {
            return None;
        }

        Some(Self {
            reference: VideoRef {
                module_id: module_id.to_string(),
                prop: prop.to_string(),
            },
            raw_path: raw_path.to_string(),
            options: video_options(options),
        })
    }
}

/// Read the persisted playback options with the legacy defaults.
///
/// `loop` defaults to `true` (the renderer set `<video loop>`), `paused` to
/// `false`, and a missing/invalid `playbackrate` to 1.0. A numeric rate is
/// clamped into the transport's 0.1–10 range.
fn video_options(options: &Value) -> VideoOptions {
    let looping = options.get("loop").and_then(Value::as_bool).unwrap_or(true);
    let paused = options
        .get("paused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let playback_rate = options
        .get("playbackrate")
        .and_then(Value::as_f64)
        .and_then(clamp_playback_rate)
        .unwrap_or(1.0);

    VideoOptions {
        looping,
        paused,
        playback_rate,
    }
}

/// Collect every video texture prop in the store, in a deterministic order.
pub fn collect_video_props(state: &EngineState) -> Vec<VideoPropRequest> {
    let mut requests = Vec::new();

    for module in state.modules.values() {
        let Some(props) = module.props.as_object() else {
            continue;
        };
        for (name, value) in props {
            if let Some(request) = VideoPropRequest::from_prop(&module.id, name, value) {
                requests.push(request);
            }
        }
    }

    requests.sort_by(|a, b| a.reference.cmp(&b.reference));

    requests
}

/// Why a saved video path has no source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    /// The path is empty or contains `..`/NUL: never resolved.
    Rejected(String),
    /// The path is a well-formed media-library path but no file exists.
    Missing,
}

/// Resolve a saved video path to an absolute file path.
pub fn resolve_video_path(root: Option<&Path>, raw: &str) -> Result<PathBuf, PathError> {
    let trimmed = raw.trim();
    let raw = trimmed.strip_prefix("file://").unwrap_or(trimmed);
    let normalised = raw.replace('\\', "/");

    if normalised.contains('\0') {
        return Err(PathError::Rejected(
            "the path contains a NUL byte".to_string(),
        ));
    }
    if normalised.split('/').any(|segment| segment == "..") {
        return Err(PathError::Rejected(
            "the path contains a parent traversal".to_string(),
        ));
    }
    let relative = normalised.trim_matches('/');
    if relative.is_empty() {
        return Err(PathError::Rejected("the path is empty".to_string()));
    }

    let direct = Path::new(&normalised);
    if direct.is_absolute() && direct.is_file() {
        return Ok(direct.to_path_buf());
    }

    let root = root.ok_or_else(|| {
        PathError::Rejected("no media directory is configured to resolve the path".to_string())
    })?;
    let joined = root.join(relative);
    joined.is_file().then_some(joined).ok_or(PathError::Missing)
}

/// One decision [`plan`] makes for a resolved path.
#[derive(Clone, Debug, PartialEq)]
pub enum ReconcileAction {
    Start {
        path: PathBuf,
        id: String,
        options: VideoOptions,
    },
    Stop {
        path: PathBuf,
        id: String,
    },
    SetPaused {
        path: PathBuf,
        id: String,
        paused: bool,
    },
    SetLooping {
        path: PathBuf,
        id: String,
        looping: bool,
    },
    SetPlaybackRate {
        path: PathBuf,
        id: String,
        rate: f64,
    },
}

/// A source the manager already runs, as [`plan`] sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ExistingSource {
    pub path: PathBuf,
    pub id: String,
    pub options: VideoOptions,
}

/// The resolved references to one absolute path.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPath {
    pub references: BTreeSet<VideoRef>,
    pub options: VideoOptions,
}

/// The stable engine source id for a file path.
pub fn video_source_id(path: &Path) -> String {
    format!("video:{}", path.to_string_lossy())
}

/// Seek target (in milliseconds) for a video trimmed to `[start, finish]`,
/// where both trims are fractions of the clip in `0..1`.
///
/// When the reported playhead has reached (or passed) `finish`, playback is
/// wrapped back to `start` so the clip loops between the two points. Returns
/// `None` — "leave the playhead alone" — when the playhead is inside the trim,
/// when `start == finish` (an empty trim), and when the trims are the
/// full-clip defaults `0..1`: the default must be a complete no-op so the
/// VmodV end-of-stream/loop handling stays authoritative. Both trims are
/// clamped to `0..1`; when `start` exceeds `finish` the two values are used
/// as given (no swap). Non-finite trims or an unknown/zero duration also
/// yield `None`.
pub fn trim_seek_target(
    position_ms: u64,
    start: f64,
    finish: f64,
    duration_ms: Option<u64>,
) -> Option<u64> {
    let duration = duration_ms?;
    if duration == 0 || !start.is_finite() || !finish.is_finite() {
        return None;
    }
    let start = start.clamp(0.0, 1.0);
    let finish = finish.clamp(0.0, 1.0);
    if start == finish || (start == 0.0 && finish == 1.0) {
        return None;
    }
    let start_ms = (start * duration as f64).round() as u64;
    let finish_ms = (finish * duration as f64).round() as u64;
    (position_ms >= finish_ms).then_some(start_ms)
}

/// Plan the starts/stops/transport updates that reconcile `existing` with the
/// resolved prop references.
pub fn plan(
    references: &BTreeMap<PathBuf, ResolvedPath>,
    existing: &[ExistingSource],
) -> Vec<ReconcileAction> {
    let existing: BTreeMap<&Path, &ExistingSource> = existing
        .iter()
        .map(|source| (source.path.as_path(), source))
        .collect();
    let mut actions = Vec::new();

    // Existing sources nobody references any more stop first, so a Scene
    // change releases the old pipeline before the new one starts.
    for source in &existing {
        if !references.contains_key(*source.0) {
            actions.push(ReconcileAction::Stop {
                path: source.0.to_path_buf(),
                id: source.1.id.clone(),
            });
        }
    }

    for (path, resolved) in references {
        match existing.get(path.as_path()) {
            None => actions.push(ReconcileAction::Start {
                path: path.clone(),
                id: video_source_id(path),
                options: resolved.options,
            }),
            Some(source) => {
                let current = &source.options;
                let wanted = &resolved.options;
                if current.paused != wanted.paused {
                    actions.push(ReconcileAction::SetPaused {
                        path: path.clone(),
                        id: source.id.clone(),
                        paused: wanted.paused,
                    });
                }
                if current.looping != wanted.looping {
                    actions.push(ReconcileAction::SetLooping {
                        path: path.clone(),
                        id: source.id.clone(),
                        looping: wanted.looping,
                    });
                }
                if (current.playback_rate - wanted.playback_rate).abs() > 0.001 {
                    actions.push(ReconcileAction::SetPlaybackRate {
                        path: path.clone(),
                        id: source.id.clone(),
                        rate: wanted.playback_rate,
                    });
                }
            }
        }
    }

    actions
}

/// A missing/failed path the UI should explain next to the texture control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoSourceProblem {
    /// The prop's saved path (media-relative or absolute).
    pub path: String,
    pub message: String,
}

/// The engine operations the manager needs; implemented by [`MediaHub`] and,
/// in tests, by a recording fake.
pub trait VideoFileHub {
    fn start_file(
        &mut self,
        id: &str,
        path: &Path,
        options: &VideoOptions,
    ) -> Result<(), MediaError>;
    fn stop_file(&mut self, id: &str);
    fn set_paused(&mut self, id: &str, paused: bool) -> Result<VideoTransport, MediaError>;
    fn toggle_paused(&mut self, id: &str) -> Result<VideoTransport, MediaError>;
    fn seek_file(&mut self, id: &str, position: Duration) -> Result<VideoTransport, MediaError>;
    fn set_looping(&mut self, id: &str, looping: bool) -> Result<VideoTransport, MediaError>;
    fn set_playback_rate(&mut self, id: &str, rate: f64) -> Result<VideoTransport, MediaError>;
    fn transport(&self, id: &str) -> Option<VideoTransport>;
}

#[derive(Clone, Debug)]
struct ManagedSource {
    id: String,
    references: BTreeSet<VideoRef>,
    options: VideoOptions,
}

/// The path→source map the VmodV shell reconciles.
#[derive(Default)]
pub struct VideoSources {
    sources: BTreeMap<PathBuf, ManagedSource>,
    problems: BTreeMap<String, VideoSourceProblem>,
    /// Paths the user stopped explicitly; a prop still referencing one is not
    /// restarted until the prop changes or the media tree reports a change.
    suppressed: BTreeSet<PathBuf>,
    /// Paths whose pipeline refused to start; not retried in a loop.
    failed: BTreeSet<PathBuf>,
    /// Set by [`Self::note_media_change`]: retry the failed paths once.
    retry_failures: bool,
    last_requests: Vec<VideoPropRequest>,
    last_scan: Option<Instant>,
    force: bool,
}

impl VideoSources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Missing/failed paths, sorted by saved path.
    pub fn problems(&self) -> Vec<VideoSourceProblem> {
        self.problems.values().cloned().collect()
    }

    /// Running source ids, sorted (diagnostics/tests).
    pub fn source_ids(&self) -> Vec<String> {
        self.sources
            .values()
            .map(|source| source.id.clone())
            .collect()
    }

    /// The absolute paths the manager resolved, sorted (diagnostics/tests).
    pub fn paths(&self) -> Vec<PathBuf> {
        self.sources.keys().cloned().collect()
    }

    /// Retry missing/failed files and rescan on the next reconcile.
    pub fn note_media_change(&mut self) {
        self.force = true;
        self.retry_failures = true;
    }

    /// Stop every video source (shutdown/profile switch).
    pub fn stop_all(&mut self, hub: &mut impl VideoFileHub) {
        let ids: Vec<String> = self
            .sources
            .values()
            .map(|source| source.id.clone())
            .collect();

        for id in &ids {
            hub.stop_file(id);
        }
        if !ids.is_empty() {
            eprintln!("ui: INFO stopped {} video texture source(s)", ids.len());
        }

        self.sources.clear();
        self.suppressed.clear();
        self.failed.clear();
        self.problems.clear();
        self.last_requests.clear();
        self.last_scan = None;
        self.force = false;
        self.retry_failures = false;
    }

    /// Reconcile the store's video props with the running file sources.
    /// Returns whether a pipeline was started or stopped.
    pub fn reconcile(
        &mut self,
        hub: &mut impl VideoFileHub,
        media_root: Option<&Path>,
        requests: &[VideoPropRequest],
        now: Instant,
    ) -> bool {
        if !self.force
            && self
                .last_scan
                .is_some_and(|last| now.saturating_duration_since(last) < RECONCILE_INTERVAL)
        {
            return false;
        }
        let retry_failures = self.retry_failures;
        self.force = false;
        self.retry_failures = false;
        self.last_scan = Some(now);

        // Forget errors for props that are gone (a Scene change replaced the
        // module); a still-referenced but missing path keeps its message.
        let current_raws: BTreeSet<&str> = requests
            .iter()
            .map(|request| request.raw_path.as_str())
            .collect();
        self.problems
            .retain(|raw, _| current_raws.contains(raw.as_str()));

        // A prop that changed path/options (or is a new reference) is a new
        // request, so an explicit Stop does not hold a reassigned prop down.
        let previous: BTreeMap<&VideoRef, &VideoPropRequest> = self
            .last_requests
            .iter()
            .map(|request| (&request.reference, request))
            .collect();
        let mut reopened = BTreeSet::new();
        for request in requests {
            let changed = previous.get(&request.reference).is_none_or(|old| {
                old.raw_path != request.raw_path || old.options != request.options
            });
            if changed && let Ok(path) = resolve_video_path(media_root, &request.raw_path) {
                self.suppressed.remove(&path);
                reopened.insert(path);
            }
        }

        // Resolve every path once and group the references by absolute path.
        let mut resolved: BTreeMap<PathBuf, ResolvedPath> = BTreeMap::new();
        let mut raw_paths: BTreeMap<PathBuf, String> = BTreeMap::new();
        for request in requests {
            match resolve_video_path(media_root, &request.raw_path) {
                Ok(path) => {
                    self.problems.remove(&request.raw_path);
                    raw_paths
                        .entry(path.clone())
                        .or_insert_with(|| request.raw_path.clone());
                    resolved
                        .entry(path)
                        .or_insert_with(|| ResolvedPath {
                            references: BTreeSet::new(),
                            options: request.options,
                        })
                        .references
                        .insert(request.reference.clone());
                }
                Err(error) => {
                    let message = match error {
                        PathError::Missing => {
                            format!("the video file {:?} is missing", request.raw_path)
                        }
                        PathError::Rejected(reason) => {
                            format!(
                                "the video path {:?} cannot be read: {reason}",
                                request.raw_path
                            )
                        }
                    };
                    if self
                        .problems
                        .get(&request.raw_path)
                        .is_none_or(|problem| problem.message != message)
                    {
                        eprintln!("ui: WARN video texture {:?}: {message}", request.raw_path);
                        self.problems.insert(
                            request.raw_path.clone(),
                            VideoSourceProblem {
                                path: request.raw_path.clone(),
                                message,
                            },
                        );
                    }
                }
            }
        }

        // A path that vanished from the store no longer has a failure to retry.
        self.failed.retain(|path| resolved.contains_key(path));

        // Suppressed paths keep their references in the store but must not run;
        // a path whose pipeline refused to start waits for a media-tree change
        // (or a changed prop) instead of retrying every scan.
        let wanted: BTreeMap<PathBuf, ResolvedPath> = resolved
            .iter()
            .filter(|(path, _)| !self.suppressed.contains(*path))
            .filter(|(path, _)| {
                retry_failures || !self.failed.contains(*path) || reopened.contains(*path)
            })
            .map(|(path, resolved)| (path.clone(), resolved.clone()))
            .collect();
        let existing: Vec<ExistingSource> = self
            .sources
            .iter()
            .map(|(path, source)| ExistingSource {
                path: path.clone(),
                id: source.id.clone(),
                options: source.options,
            })
            .collect();

        let mut changed = false;
        for action in plan(&wanted, &existing) {
            match action {
                ReconcileAction::Stop { path, id } => {
                    hub.stop_file(&id);
                    self.sources.remove(&path);
                    eprintln!("ui: INFO video texture source {id:?} stopped");
                    changed = true;
                }
                ReconcileAction::Start { path, id, options } => {
                    match hub.start_file(&id, &path, &options) {
                        Ok(()) => {
                            self.failed.remove(&path);
                            if let Some(raw) = raw_paths.get(&path) {
                                self.problems.remove(raw);
                            }
                            self.sources.insert(
                                path.clone(),
                                ManagedSource {
                                    id: id.clone(),
                                    references: wanted[&path].references.clone(),
                                    options,
                                },
                            );
                            eprintln!(
                                "ui: INFO video texture source {id:?} started \
                             (loop {}, {}x speed)",
                                options.looping, options.playback_rate
                            );
                            changed = true;
                        }
                        Err(error) => {
                            self.failed.insert(path.clone());
                            let raw = raw_paths
                                .get(&path)
                                .cloned()
                                .unwrap_or_else(|| path.to_string_lossy().into_owned());
                            self.note_problem(
                                raw,
                                format!("the video source could not start: {error}"),
                            );
                        }
                    }
                }
                ReconcileAction::SetPaused { path, id, paused } => {
                    match hub.set_paused(&id, paused) {
                        Ok(_) => {
                            if let Some(source) = self.sources.get_mut(&path) {
                                source.options.paused = paused;
                            }
                        }
                        Err(error) => {
                            self.note_action_failure(&path, &raw_paths, "pause", error);
                        }
                    }
                }
                ReconcileAction::SetLooping { path, id, looping } => {
                    match hub.set_looping(&id, looping) {
                        Ok(_) => {
                            if let Some(source) = self.sources.get_mut(&path) {
                                source.options.looping = looping;
                            }
                        }
                        Err(error) => {
                            self.note_action_failure(&path, &raw_paths, "loop", error);
                        }
                    }
                }
                ReconcileAction::SetPlaybackRate { path, id, rate } => {
                    match hub.set_playback_rate(&id, rate) {
                        Ok(_) => {
                            if let Some(source) = self.sources.get_mut(&path) {
                                source.options.playback_rate = rate;
                            }
                        }
                        Err(error) => {
                            self.note_action_failure(&path, &raw_paths, "speed", error);
                        }
                    }
                }
            }
        }

        // Every running source adopts the current reference set (a shared clip
        // whose second prop moved away is still referenced by the first).
        for (path, resolved) in &wanted {
            if let Some(source) = self.sources.get_mut(path) {
                source.references = resolved.references.clone();
            }
        }

        // A health restart replaces the pipeline, so its pause/speed options
        // start from the GStreamer defaults. Reapply the saved options without
        // restarting anything.
        let checks: Vec<(PathBuf, String, VideoOptions, Option<VideoTransport>)> = self
            .sources
            .iter()
            .map(|(path, source)| {
                (
                    path.clone(),
                    source.id.clone(),
                    source.options,
                    hub.transport(&source.id),
                )
            })
            .collect();
        for (path, id, options, live) in checks {
            let Some(live) = live else {
                continue;
            };
            if live.paused != options.paused {
                match hub.set_paused(&id, options.paused) {
                    Ok(_) => {
                        if let Some(source) = self.sources.get_mut(&path) {
                            source.options.paused = options.paused;
                        }
                    }
                    Err(error) => {
                        self.note_action_failure(&path, &raw_paths, "pause", error);
                    }
                }
            }
            if live.looping != options.looping {
                match hub.set_looping(&id, options.looping) {
                    Ok(_) => {
                        if let Some(source) = self.sources.get_mut(&path) {
                            source.options.looping = options.looping;
                        }
                    }
                    Err(error) => {
                        self.note_action_failure(&path, &raw_paths, "loop", error);
                    }
                }
            }
            if (live.playback_rate - options.playback_rate).abs() > 0.001 {
                match hub.set_playback_rate(&id, options.playback_rate) {
                    Ok(_) => {
                        if let Some(source) = self.sources.get_mut(&path) {
                            source.options.playback_rate = options.playback_rate;
                        }
                    }
                    Err(error) => {
                        self.note_action_failure(&path, &raw_paths, "speed", error);
                    }
                }
            }
        }

        self.last_requests = requests.to_vec();

        changed
    }

    /// [`Self::reconcile`] ignoring the cadence gate (Scene switch, tests).
    pub fn reconcile_now(
        &mut self,
        hub: &mut impl VideoFileHub,
        media_root: Option<&Path>,
        requests: &[VideoPropRequest],
        now: Instant,
    ) -> bool {
        self.force = true;
        self.reconcile(hub, media_root, requests, now)
    }

    /// Stop one source by id (the Media panel's Stop). The referencing prop is
    /// not restarted until it changes.
    pub fn stop_source(&mut self, hub: &mut impl VideoFileHub, source: &str) -> bool {
        let Some(path) = self.find_path(source) else {
            return false;
        };
        let Some(entry) = self.sources.remove(&path) else {
            return false;
        };

        hub.stop_file(&entry.id);
        self.suppressed.insert(path);
        eprintln!(
            "ui: INFO video texture source {:?} stopped by the user",
            entry.id
        );

        true
    }

    /// Apply one transport command to a running source.
    pub fn transport(
        &mut self,
        hub: &mut impl VideoFileHub,
        source: &str,
        action: VideoTransportAction,
    ) -> Result<VideoTransport, MediaError> {
        let path = self.find_path(source).ok_or_else(|| {
            MediaError::StartFailed(format!("no running video source for {source:?}"))
        })?;
        let id = self.sources[&path].id.clone();

        let transport = match action {
            VideoTransportAction::Pause => hub.set_paused(&id, true)?,
            VideoTransportAction::Resume => hub.set_paused(&id, false)?,
            VideoTransportAction::TogglePause => hub.toggle_paused(&id)?,
            VideoTransportAction::Restart => hub.seek_file(&id, Duration::ZERO)?,
            VideoTransportAction::Seek { position_ms } => {
                hub.seek_file(&id, Duration::from_millis(position_ms))?
            }
            VideoTransportAction::SetLooping { looping } => hub.set_looping(&id, looping)?,
            VideoTransportAction::SetPlaybackRate { rate } => hub.set_playback_rate(&id, rate)?,
        };

        // Adopt the clamped state the hub reported, so the next reconcile sees
        // the same options and does not issue a duplicate command.
        if let Some(entry) = self.sources.get_mut(&path) {
            entry.options.paused = transport.paused;
            entry.options.looping = transport.looping;
            entry.options.playback_rate = transport.playback_rate;
        }

        Ok(transport)
    }

    /// Wrap playing video sources back to their trim start once they pass the
    /// finish. For each running source referenced by a module carrying both a
    /// `start` and a `finish` trim, the first such reference (in the ordered
    /// `references` set) picks the trim values when several props share a clip.
    ///
    /// A successful seek moves the reported playhead inside the trim, so the
    /// next tick produces no target and no per-source latch is needed. A seek
    /// failure is logged and may be retried on a later tick. Sources without a
    /// transport (not running) or without a usable duration are left alone.
    pub fn sync_trim(
        &mut self,
        hub: &mut impl VideoFileHub,
        start_by_module: &BTreeMap<String, f64>,
        finish_by_module: &BTreeMap<String, f64>,
    ) {
        // Collect owned decisions first so no reference into `self.sources`
        // is alive while we call out to the hub.
        let mut seeks: Vec<(String, u64)> = Vec::new();

        for source in self.sources.values() {
            // `references` is an ordered BTreeSet: the first module carrying
            // both trims makes the choice deterministic when several props
            // share a clip.
            let Some((start, finish)) = source.references.iter().find_map(|reference| {
                let start = *start_by_module.get(&reference.module_id)?;
                let finish = *finish_by_module.get(&reference.module_id)?;
                Some((start, finish))
            }) else {
                continue;
            };
            let Some(transport) = hub.transport(&source.id) else {
                continue;
            };
            if let Some(target_ms) =
                trim_seek_target(transport.position_ms, start, finish, transport.duration_ms)
            {
                seeks.push((source.id.clone(), target_ms));
            }
        }

        for (id, target_ms) in seeks {
            if let Err(error) = hub.seek_file(&id, Duration::from_millis(target_ms)) {
                eprintln!("ui: WARN video source {id:?} trim seek failed: {error}");
            }
        }
    }

    /// The props referencing `source` (for option persistence).
    pub fn references(&self, source: &str) -> Vec<VideoRef> {
        self.find_path(source)
            .and_then(|path| self.sources.get(&path))
            .map(|source| source.references.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn find_path(&self, source: &str) -> Option<PathBuf> {
        if let Some((path, _)) = self.sources.iter().find(|(_, entry)| entry.id == source) {
            return Some(path.clone());
        }

        let needle = source.strip_prefix("video:").unwrap_or(source);
        let needle = needle.replace('\\', "/");
        let needle = needle.trim_matches('/');
        if needle.is_empty() {
            return None;
        }

        self.sources
            .keys()
            .find(|path| {
                let full = path.to_string_lossy().replace('\\', "/");
                full == needle || full.ends_with(&format!("/{needle}"))
            })
            .cloned()
    }

    fn note_problem(&mut self, raw: String, message: String) {
        if self
            .problems
            .get(&raw)
            .is_none_or(|problem| problem.message != message)
        {
            eprintln!("ui: WARN video texture {raw:?}: {message}");
            self.problems
                .insert(raw.clone(), VideoSourceProblem { path: raw, message });
        }
    }

    fn note_action_failure(
        &mut self,
        path: &Path,
        raw_paths: &BTreeMap<PathBuf, String>,
        action: &str,
        error: MediaError,
    ) {
        let raw = raw_paths
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        self.note_problem(raw, format!("the video {action} command failed: {error}"));
    }
}

/// Set one key in a saved texture prop's `options`, preserving every other
/// field (the UI's transport writes go through this so presets stay intact).
pub fn set_prop_option(value: &mut Value, key: &str, option: Value) {
    if !value.is_object() {
        *value = serde_json::json!({ "type": "video", "options": {} });
    }
    let Some(object) = value.as_object_mut() else {
        return;
    };
    object.insert("type".to_string(), serde_json::json!("video"));
    let options = object
        .entry("options".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !options.is_object() {
        *options = serde_json::json!({});
    }
    if let Some(options) = options.as_object_mut() {
        options.insert(key.to_string(), option);
    }
}

impl VideoFileHub for MediaHub {
    fn start_file(
        &mut self,
        id: &str,
        path: &Path,
        options: &VideoOptions,
    ) -> Result<(), MediaError> {
        let config = VideoSourceConfig::file(path);
        self.start_with_looping(id, config, options.looping)?;

        // The pipeline starts unpaused at real time; apply the saved options.
        // A failed control releases the just-started pipeline so the manager
        // never loses track of a running source.
        if options.paused
            && let Err(error) = self.set_paused(id, true)
        {
            self.stop(id);
            return Err(error);
        }
        if (options.playback_rate - 1.0).abs() > 0.001
            && let Err(error) = self.set_playback_rate(id, options.playback_rate)
        {
            self.stop(id);
            return Err(error);
        }

        Ok(())
    }

    fn stop_file(&mut self, id: &str) {
        self.stop(id);
    }

    fn set_paused(&mut self, id: &str, paused: bool) -> Result<VideoTransport, MediaError> {
        MediaHub::set_paused(self, id, paused)
    }

    fn toggle_paused(&mut self, id: &str) -> Result<VideoTransport, MediaError> {
        MediaHub::toggle_paused(self, id)
    }

    fn seek_file(&mut self, id: &str, position: Duration) -> Result<VideoTransport, MediaError> {
        MediaHub::seek(self, id, position)
    }

    fn set_looping(&mut self, id: &str, looping: bool) -> Result<VideoTransport, MediaError> {
        MediaHub::set_looping(self, id, looping)
    }

    fn set_playback_rate(&mut self, id: &str, rate: f64) -> Result<VideoTransport, MediaError> {
        MediaHub::set_playback_rate(self, id, rate)
    }

    fn transport(&self, id: &str) -> Option<VideoTransport> {
        MediaHub::transport(self, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn options(looping: bool, paused: bool, playback_rate: f64) -> VideoOptions {
        VideoOptions {
            looping,
            paused,
            playback_rate,
        }
    }

    fn reference(module: &str, prop: &str) -> VideoRef {
        VideoRef {
            module_id: module.to_string(),
            prop: prop.to_string(),
        }
    }

    fn request(module: &str, prop: &str, path: &str) -> VideoPropRequest {
        VideoPropRequest {
            reference: reference(module, prop),
            raw_path: path.to_string(),
            options: VideoOptions::default(),
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "modv-video-sources-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn prop_parsing_reads_the_video_options_and_defaults() {
        let parsed = VideoPropRequest::from_prop(
            "module-1",
            "clip",
            &json!({
                "type": "video",
                "options": { "path": "default/video/clip.mp4" }
            }),
        )
        .expect("a video prop");
        assert_eq!(parsed.raw_path, "default/video/clip.mp4");
        assert_eq!(parsed.options, VideoOptions::default());
        assert!(
            parsed.options.looping,
            "looping is the <video loop> default"
        );
        assert!(!parsed.options.paused);
        assert_eq!(parsed.options.playback_rate, 1.0);

        let explicit = VideoPropRequest::from_prop(
            "module-1",
            "clip",
            &json!({
                "type": "video",
                "options": {
                    "path": "default/video/clip.mp4",
                    "loop": false,
                    "paused": true,
                    "playbackrate": 2.5
                }
            }),
        )
        .expect("a video prop");
        assert_eq!(explicit.options, options(false, true, 2.5));

        assert!(
            VideoPropRequest::from_prop(
                "module-1",
                "clip",
                &json!({"type": "image", "options": {"path": "a.png"}})
            )
            .is_none(),
            "an image prop is not a video source"
        );
        assert!(
            VideoPropRequest::from_prop(
                "module-1",
                "clip",
                &json!({"type": "video", "options": {}})
            )
            .is_none(),
            "a video prop without a path has nothing to play"
        );
    }

    #[test]
    fn option_values_are_clamped_to_the_transport_range() {
        let parsed = VideoPropRequest::from_prop(
            "module-1",
            "clip",
            &json!({
                "type": "video",
                "options": {
                    "path": "clip.mp4",
                    "playbackrate": 40.0
                }
            }),
        )
        .expect("a video prop");
        assert_eq!(parsed.options.playback_rate, 10.0);

        let invalid = VideoPropRequest::from_prop(
            "module-1",
            "clip",
            &json!({
                "type": "video",
                "options": {
                    "path": "clip.mp4",
                    "playbackrate": "fast"
                }
            }),
        )
        .expect("a video prop");
        assert_eq!(invalid.options.playback_rate, 1.0);
    }

    #[test]
    fn the_collector_only_finds_video_props_with_paths() {
        let mut state = EngineState::new();
        for (id, props) in [
            (
                "module-a",
                json!({
                    "texture": {"type": "video", "options": {"path": "default/video/clip.mp4"}},
                    "photo": {"type": "image", "options": {"path": "default/image/a.png"}}
                }),
            ),
            (
                "module-b",
                json!({
                    "clip": {"type": "video", "options": {"path": "default/video/other.mp4"}},
                    "webcam": {"type": "webcam", "options": {"id": "camera:/dev/video0"}}
                }),
            ),
        ] {
            let module = serde_json::from_value(json!({
                "$id": id,
                "$moduleName": "Image Layer",
                "props": props
            }))
            .expect("a module instance");
            state.modules.insert(id.to_string(), module);
        }

        let requests = collect_video_props(&state);
        assert_eq!(
            requests
                .iter()
                .map(|request| {
                    (
                        request.reference.module_id.as_str(),
                        request.reference.prop.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![("module-a", "texture"), ("module-b", "clip")]
        );
    }

    #[test]
    fn media_paths_resolve_under_the_root() {
        let root = temp_dir("resolve");
        let nested = root.join("default/video");
        std::fs::create_dir_all(&nested).expect("video folder");
        let clip = nested.join("clip.mp4");
        std::fs::write(&clip, b"not really a video").expect("clip");

        assert_eq!(
            resolve_video_path(Some(&root), "default/video/clip.mp4"),
            Ok(clip.clone())
        );
        assert_eq!(
            resolve_video_path(Some(&root), "/default/video/clip.mp4"),
            Ok(clip.clone()),
            "a leading separator is media-library relative"
        );
        assert_eq!(
            resolve_video_path(Some(&root), &clip.to_string_lossy()),
            Ok(clip.clone()),
            "an existing absolute path is used as-is"
        );
        assert_eq!(
            resolve_video_path(Some(&root), "default/video/missing.mp4"),
            Err(PathError::Missing)
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn parent_traversal_and_empty_paths_are_rejected() {
        let root = temp_dir("traversal");
        for path in ["../clip.mp4", "default/../../clip.mp4", "", "   "] {
            assert!(
                matches!(
                    resolve_video_path(Some(&root), path),
                    Err(PathError::Rejected(_))
                ),
                "{path:?} must be rejected"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn plan_starts_one_source_per_unique_path_and_shares_references() {
        let a = PathBuf::from("/media/default/video/clip.mp4");
        let b = PathBuf::from("/media/default/video/other.mp4");
        let mut references = BTreeMap::new();
        references.insert(
            a.clone(),
            ResolvedPath {
                references: [
                    reference("module-a", "texture"),
                    reference("module-b", "clip"),
                ]
                .into_iter()
                .collect(),
                options: options(true, false, 1.0),
            },
        );
        references.insert(
            b.clone(),
            ResolvedPath {
                references: [reference("module-c", "texture")].into_iter().collect(),
                options: options(false, true, 2.0),
            },
        );

        let actions = plan(&references, &[]);
        assert_eq!(
            actions,
            vec![
                ReconcileAction::Start {
                    path: a.clone(),
                    id: video_source_id(&a),
                    options: options(true, false, 1.0)
                },
                ReconcileAction::Start {
                    path: b.clone(),
                    id: video_source_id(&b),
                    options: options(false, true, 2.0)
                },
            ]
        );

        // The same references against the started sources plan no restart.
        let existing = vec![
            ExistingSource {
                path: a.clone(),
                id: video_source_id(&a),
                options: options(true, false, 1.0),
            },
            ExistingSource {
                path: b.clone(),
                id: video_source_id(&b),
                options: options(false, true, 2.0),
            },
        ];
        assert!(
            plan(&references, &existing).is_empty(),
            "an unchanged prop set never restarts a source"
        );
    }

    #[test]
    fn plan_stops_unreferenced_sources_and_updates_options_in_place() {
        let kept = PathBuf::from("/media/video/kept.mp4");
        let dropped = PathBuf::from("/media/video/dropped.mp4");
        let existing = vec![
            ExistingSource {
                path: kept.clone(),
                id: video_source_id(&kept),
                options: options(true, false, 1.0),
            },
            ExistingSource {
                path: dropped.clone(),
                id: video_source_id(&dropped),
                options: options(true, false, 1.0),
            },
        ];
        let mut references = BTreeMap::new();
        references.insert(
            kept.clone(),
            ResolvedPath {
                references: [reference("module-a", "clip")].into_iter().collect(),
                options: options(false, true, 2.0),
            },
        );

        let actions = plan(&references, &existing);
        assert_eq!(
            actions,
            vec![
                ReconcileAction::Stop {
                    path: dropped.clone(),
                    id: video_source_id(&dropped)
                },
                ReconcileAction::SetPaused {
                    path: kept.clone(),
                    id: video_source_id(&kept),
                    paused: true
                },
                ReconcileAction::SetLooping {
                    path: kept.clone(),
                    id: video_source_id(&kept),
                    looping: false
                },
                ReconcileAction::SetPlaybackRate {
                    path: kept.clone(),
                    id: video_source_id(&kept),
                    rate: 2.0
                },
            ]
        );
    }

    #[test]
    fn set_prop_option_preserves_the_rest_of_the_definition() {
        let mut value = json!({
            "type": "video",
            "options": { "path": "default/video/clip.mp4", "loop": true },
            "extra": 7
        });
        set_prop_option(&mut value, "paused", json!(true));
        assert_eq!(
            value,
            json!({
                "type": "video",
                "options": {
                    "path": "default/video/clip.mp4",
                    "loop": true,
                    "paused": true
                },
                "extra": 7
            })
        );

        let mut bare = json!(null);
        set_prop_option(&mut bare, "playbackrate", json!(2.0));
        assert_eq!(
            bare,
            json!({"type": "video", "options": {"playbackrate": 2.0}})
        );
    }

    /// A recording hub: no pipeline, so reconcile decisions are host-independent.
    #[derive(Default)]
    struct FakeHub {
        calls: Vec<String>,
        started: BTreeMap<String, (PathBuf, VideoOptions)>,
        transport: Option<VideoTransport>,
        fail_starts: bool,
    }

    impl FakeHub {
        fn start_calls(&self) -> Vec<&String> {
            self.calls
                .iter()
                .filter(|call| call.starts_with("start "))
                .collect()
        }
    }

    impl VideoFileHub for FakeHub {
        fn start_file(
            &mut self,
            id: &str,
            path: &Path,
            options: &VideoOptions,
        ) -> Result<(), MediaError> {
            self.calls.push(format!("start {id}"));
            if self.fail_starts {
                return Err(MediaError::StartFailed("boom".to_string()));
            }
            self.started
                .insert(id.to_string(), (path.to_path_buf(), *options));
            if options.paused {
                self.set_paused(id, true)?;
            }
            if (options.playback_rate - 1.0).abs() > 0.001 {
                self.set_playback_rate(id, options.playback_rate)?;
            }
            Ok(())
        }

        fn stop_file(&mut self, id: &str) {
            self.calls.push(format!("stop {id}"));
            self.started.remove(id);
        }

        fn set_paused(&mut self, id: &str, paused: bool) -> Result<VideoTransport, MediaError> {
            self.calls.push(format!("paused {id} {paused}"));
            let mut transport = self.transport.unwrap_or_default();
            transport.paused = paused;
            self.transport = Some(transport);
            Ok(transport)
        }

        fn toggle_paused(&mut self, id: &str) -> Result<VideoTransport, MediaError> {
            let paused = self.transport.unwrap_or_default().paused;
            self.set_paused(id, !paused)
        }

        fn seek_file(
            &mut self,
            id: &str,
            position: Duration,
        ) -> Result<VideoTransport, MediaError> {
            self.calls
                .push(format!("seek {id} {}", position.as_millis()));
            let mut transport = self.transport.unwrap_or_default();
            transport.position_ms = position.as_millis() as u64;
            transport.eos = false;
            self.transport = Some(transport);
            Ok(transport)
        }

        fn set_looping(&mut self, id: &str, looping: bool) -> Result<VideoTransport, MediaError> {
            self.calls.push(format!("loop {id} {looping}"));
            let mut transport = self.transport.unwrap_or_default();
            transport.looping = looping;
            self.transport = Some(transport);
            Ok(transport)
        }

        fn set_playback_rate(&mut self, id: &str, rate: f64) -> Result<VideoTransport, MediaError> {
            self.calls.push(format!("rate {id} {rate}"));
            let mut transport = self.transport.unwrap_or_default();
            transport.playback_rate = rate;
            self.transport = Some(transport);
            Ok(transport)
        }

        fn transport(&self, _id: &str) -> Option<VideoTransport> {
            self.transport
        }
    }

    #[test]
    fn reconcile_starts_one_pipeline_and_refcounts_it() {
        let root = temp_dir("refcount");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        let clip = root.join("default/video/clip.mp4");
        std::fs::write(&clip, b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let now = Instant::now();
        let requests = vec![
            request("module-a", "texture", "default/video/clip.mp4"),
            request("module-b", "clip", "default/video/clip.mp4"),
        ];

        assert!(sources.reconcile_now(&mut hub, Some(&root), &requests, now));
        assert_eq!(hub.start_calls().len(), 1, "the clip is started once");
        assert_eq!(sources.source_ids(), vec![video_source_id(&clip)]);

        // A repaint with the same props neither restarts nor stops anything.
        hub.calls.clear();
        assert!(!sources.reconcile_now(&mut hub, Some(&root), &requests, now));
        assert!(hub.calls.is_empty(), "unchanged props do no work");

        // One reference disappears: the shared source stays.
        let remaining = vec![request("module-b", "clip", "default/video/clip.mp4")];
        assert!(!sources.reconcile_now(&mut hub, Some(&root), &remaining, now));
        assert_eq!(sources.source_ids(), vec![video_source_id(&clip)]);

        // The last reference disappears: the pipeline stops.
        assert!(sources.reconcile_now(&mut hub, Some(&root), &[], now));
        assert_eq!(hub.calls, vec![format!("stop {}", video_source_id(&clip))]);
        assert!(sources.source_ids().is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn starting_a_source_applies_its_options() {
        let root = temp_dir("options");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let mut request = request("module-a", "clip", "default/video/clip.mp4");
        request.options = options(false, true, 2.0);

        sources.reconcile_now(&mut hub, Some(&root), &[request], Instant::now());

        let id = video_source_id(&root.join("default/video/clip.mp4"));
        assert_eq!(hub.started[&id].1, options(false, true, 2.0));
        assert!(
            hub.calls
                .iter()
                .any(|call| call == &format!("paused {id} true")),
            "the saved pause state is applied on start: {:?}",
            hub.calls
        );
        assert!(
            hub.calls.iter().any(|call| call == &format!("rate {id} 2")),
            "the saved speed is applied on start: {:?}",
            hub.calls
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn option_changes_are_transport_updates_not_restarts() {
        let root = temp_dir("option-update");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let now = Instant::now();
        sources.reconcile_now(
            &mut hub,
            Some(&root),
            &[request("module-a", "clip", "default/video/clip.mp4")],
            now,
        );
        hub.calls.clear();

        let mut paused = request("module-a", "clip", "default/video/clip.mp4");
        paused.options = options(true, true, 1.0);
        assert!(!sources.reconcile_now(&mut hub, Some(&root), &[paused], now));
        assert!(
            hub.calls.iter().all(|call| !call.starts_with("start")),
            "an option change never restarts the pipeline: {:?}",
            hub.calls
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_saved_speed_option_change_issues_a_rate_command_without_restarting() {
        let root = temp_dir("speed-option");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let now = Instant::now();
        let id = video_source_id(&root.join("default/video/clip.mp4"));

        let at_real_time = VideoPropRequest::from_prop(
            "module-a",
            "texture",
            &json!({"type": "video", "options": {"path": "default/video/clip.mp4"}}),
        )
        .expect("a video prop");
        sources.reconcile_now(&mut hub, Some(&root), &[at_real_time], now);
        hub.calls.clear();

        // The Inspector slider writes options.playbackrate back into the prop.
        let sped_up = VideoPropRequest::from_prop(
            "module-a",
            "texture",
            &json!({"type": "video", "options": {
                "path": "default/video/clip.mp4", "loop": true, "playbackrate": 2.0
            }}),
        )
        .expect("a video prop");
        sources.reconcile_now(&mut hub, Some(&root), &[sped_up], now);

        assert!(
            hub.calls.iter().any(|call| call == &format!("rate {id} 2")),
            "a speed option change drives the transport: {:?}",
            hub.calls
        );
        assert!(
            hub.calls.iter().all(|call| !call.starts_with("start")),
            "the clip is not restarted to change speed: {:?}",
            hub.calls
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_file_is_kept_unresolved_and_retried_after_a_media_change() {
        let root = temp_dir("missing");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let requests = vec![request("module-a", "clip", "default/video/clip.mp4")];

        assert!(!sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now()));
        assert!(sources.source_ids().is_empty());
        let problems = sources.problems();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("missing"), "{problems:?}");
        assert!(hub.start_calls().is_empty());

        // Still missing: no repeated start attempt and no duplicate log.
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert!(sources.problems()[0].message.contains("missing"));
        assert!(hub.start_calls().is_empty());

        // The watcher reports a video change after the file appears.
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");
        sources.note_media_change();
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());

        assert_eq!(sources.problems(), Vec::new());
        assert_eq!(hub.start_calls().len(), 1);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_failed_start_is_logged_once_and_retried_after_a_media_change() {
        let root = temp_dir("start-failure");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub {
            fail_starts: true,
            ..FakeHub::default()
        };
        let mut sources = VideoSources::new();
        let requests = vec![request("module-a", "clip", "default/video/clip.mp4")];

        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert_eq!(sources.problems().len(), 1);
        assert_eq!(hub.start_calls().len(), 1);

        // Unchanged: no start storm while the failure is unresolved.
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert_eq!(hub.start_calls().len(), 1);

        hub.fail_starts = false;
        sources.note_media_change();
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert_eq!(sources.problems(), Vec::new());
        assert_eq!(sources.source_ids().len(), 1);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_scene_change_stops_the_old_clip_and_starts_the_new_one() {
        let root = temp_dir("scene-change");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        for name in ["a.mp4", "b.mp4"] {
            std::fs::write(root.join("default/video").join(name), b"fake").expect("clip");
        }

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let now = Instant::now();

        sources.reconcile_now(
            &mut hub,
            Some(&root),
            &[request("scene-a-module", "clip", "default/video/a.mp4")],
            now,
        );
        hub.calls.clear();

        let changed = [request("scene-b-module", "clip", "default/video/b.mp4")];
        assert!(sources.reconcile_now(&mut hub, Some(&root), &changed, now));

        let a = video_source_id(&root.join("default/video/a.mp4"));
        let b = video_source_id(&root.join("default/video/b.mp4"));
        assert_eq!(
            hub.calls,
            vec![format!("stop {a}"), format!("start {b}")],
            "the old Scene's pipeline is stopped and the new one started"
        );
        assert_eq!(sources.source_ids(), vec![b]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn transport_commands_route_to_the_shared_source_and_update_options() {
        let root = temp_dir("transport");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let requests = vec![request("module-a", "clip", "default/video/clip.mp4")];
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        let id = video_source_id(&root.join("default/video/clip.mp4"));
        hub.calls.clear();

        let paused = sources
            .transport(&mut hub, &id, VideoTransportAction::Pause)
            .expect("pause");
        assert!(paused.paused);
        assert_eq!(hub.calls, vec![format!("paused {id} true")]);

        let resumed = sources
            .transport(&mut hub, &id, VideoTransportAction::Resume)
            .expect("resume");
        assert!(!resumed.paused);

        sources
            .transport(&mut hub, &id, VideoTransportAction::Restart)
            .expect("restart");
        assert!(hub.calls.contains(&format!("seek {id} 0")));

        let looped = sources
            .transport(
                &mut hub,
                &id,
                VideoTransportAction::SetLooping { looping: false },
            )
            .expect("loop");
        assert!(!looped.looping);

        let rated = sources
            .transport(
                &mut hub,
                &id,
                VideoTransportAction::SetPlaybackRate { rate: 3.0 },
            )
            .expect("rate");
        assert_eq!(rated.playback_rate, 3.0);

        // The Media panel addresses a source by path as well as by id.
        let by_path = sources
            .transport(
                &mut hub,
                "default/video/clip.mp4",
                VideoTransportAction::Pause,
            )
            .expect("pause by media-relative path");
        assert!(by_path.paused);

        assert_eq!(sources.references(&id), vec![reference("module-a", "clip")]);

        // The options changed by the transport commands persist into the
        // manager's view, so reconcile does not reapply them.
        hub.calls.clear();
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert!(
            !hub.calls.iter().any(|call| call.starts_with("start")),
            "a transport change never restarts the source: {:?}",
            hub.calls
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_user_stop_is_not_undone_by_the_next_reconcile() {
        let root = temp_dir("user-stop");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let requests = vec![request("module-a", "clip", "default/video/clip.mp4")];
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        let id = video_source_id(&root.join("default/video/clip.mp4"));

        assert!(sources.stop_source(&mut hub, &id));
        assert!(sources.source_ids().is_empty());
        hub.calls.clear();

        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert!(
            hub.start_calls().is_empty(),
            "a stopped prop stays stopped until it changes: {:?}",
            hub.calls
        );

        // Reassigned (a different path) and then back: it starts again.
        sources.reconcile_now(&mut hub, Some(&root), &[], Instant::now());
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert_eq!(sources.source_ids(), vec![id]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn profile_switch_stops_every_video_source() {
        let root = temp_dir("profile");
        std::fs::create_dir_all(root.join("a/video")).expect("video folder");
        std::fs::create_dir_all(root.join("b/video")).expect("video folder");
        std::fs::write(root.join("a/video/clip.mp4"), b"fake").expect("clip");
        std::fs::write(root.join("b/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        sources.reconcile_now(
            &mut hub,
            Some(&root),
            &[request("module-a", "clip", "a/video/clip.mp4")],
            Instant::now(),
        );
        let first = video_source_id(&root.join("a/video/clip.mp4"));
        assert_eq!(sources.source_ids(), vec![first.clone()]);

        hub.calls.clear();
        sources.stop_all(&mut hub);
        assert_eq!(hub.calls, vec![format!("stop {first}")]);
        assert!(sources.source_ids().is_empty());

        // The next reconcile (new profile) starts the reference that exists.
        sources.reconcile_now(
            &mut hub,
            Some(&root),
            &[request("module-a", "clip", "b/video/clip.mp4")],
            Instant::now(),
        );
        assert_eq!(
            sources.source_ids(),
            vec![video_source_id(&root.join("b/video/clip.mp4"))]
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn applying_a_scene_reconciles_the_video_sources() {
        use crate::store_ops::Store;

        fn scene(id: &str, clip: &str) -> serde_json::Value {
            serde_json::json!({
                "id": id,
                "name": id,
                "groups": [{
                    "id": "doc-group",
                    "name": "Effects",
                    "hidden": false,
                    "modules": ["doc-module"],
                    "enabled": 1,
                    "alpha": 1,
                    "inherit": false,
                    "inheritFrom": -1,
                    "pipeline": false,
                    "clearing": true,
                    "compositeOperation": "normal",
                }],
                "modules": {
                    "doc-module": {
                        "$id": "doc-module",
                        "$moduleName": "Bands",
                        "meta": { "name": "Bands", "enabled": true, "type": "shader" },
                        "props": {
                            "texture": { "type": "video", "options": { "path": clip } }
                        },
                        "data": {},
                        "$props": {},
                    }
                },
            })
        }

        let root = temp_dir("scene-store");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        for name in ["a.mp4", "b.mp4"] {
            std::fs::write(root.join("default/video").join(name), b"fake").expect("clip");
        }

        let mut store = Store::new();
        store
            .dispatch(
                "scenes/loadSceneFromJson",
                serde_json::json!({ "scene": scene("scene-a", "default/video/a.mp4"), "apply": false, "persist": false }),
            )
            .expect("Scene A loads");
        store
            .dispatch(
                "scenes/loadSceneFromJson",
                serde_json::json!({
                    "scene": scene("scene-b", "default/video/b.mp4"),
                    "apply": false,
                    "persist": false
                }),
            )
            .expect("Scene B loads");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();

        store
            .dispatch(
                "scenes/applyScene",
                serde_json::json!({ "sceneId": "scene-a" }),
            )
            .expect("Scene A applies");
        let requests = collect_video_props(store.state());
        assert_eq!(requests.len(), 1, "{requests:?}");
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        let a = video_source_id(&root.join("default/video/a.mp4"));
        assert_eq!(sources.source_ids(), vec![a.clone()]);

        store
            .dispatch(
                "scenes/applyScene",
                serde_json::json!({ "sceneId": "scene-b" }),
            )
            .expect("Scene B applies");
        let b = video_source_id(&root.join("default/video/b.mp4"));

        // During the crossfade both Scenes' Groups are live, so both clips keep
        // playing; the new one starts and the outgoing one is not stopped.
        let transitioning = collect_video_props(store.state());
        assert_eq!(transitioning.len(), 2, "{transitioning:?}");
        hub.calls.clear();
        sources.reconcile_now(&mut hub, Some(&root), &transitioning, Instant::now());
        assert_eq!(
            hub.calls,
            vec![format!("start {b}")],
            "the outgoing clip keeps its pipeline for the crossfade"
        );

        // stopPerformance completes the in-flight transition and releases the
        // old Groups/modules, so the old clip stops on the next reconcile.
        store
            .dispatch("scenes/stopPerformance", serde_json::Value::Null)
            .expect("the Scene transition completes");
        let requests = collect_video_props(store.state());
        assert_eq!(requests.len(), 1, "{requests:?}");
        hub.calls.clear();
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        assert_eq!(
            hub.calls,
            vec![format!("stop {a}")],
            "the Scene's video props own the pipelines"
        );
        assert_eq!(sources.source_ids(), vec![b]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reconcile_respects_the_cadence_unless_forced() {
        let root = temp_dir("cadence");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        std::fs::write(root.join("default/video/clip.mp4"), b"fake").expect("clip");

        let mut hub = FakeHub::default();
        let mut sources = VideoSources::new();
        let now = Instant::now();
        let requests = vec![request("module-a", "clip", "default/video/clip.mp4")];

        sources.reconcile_now(&mut hub, Some(&root), &requests, now);
        assert_eq!(hub.start_calls().len(), 1);

        // A brand-new path before the interval elapses is deferred…
        let mut later = vec![request("module-a", "clip", "default/video/later.mp4")];
        std::fs::write(root.join("default/video/later.mp4"), b"fake").expect("clip");
        assert!(!sources.reconcile(&mut hub, Some(&root), &later, now));
        assert_eq!(sources.source_ids().len(), 1);

        // …and applied once the interval passed (or a Scene forces it).
        let due = now + RECONCILE_INTERVAL;
        assert!(sources.reconcile(&mut hub, Some(&root), &later, due));
        assert_eq!(sources.source_ids().len(), 1);

        // A forced reconcile ignores the cadence.
        later.clear();
        assert!(sources.reconcile_now(&mut hub, Some(&root), &later, due));
        assert!(sources.source_ids().is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    /// Encode a short clip with the first usable GStreamer encoder.
    ///
    /// Mirrors `modv_media::video::test_support::video_file_fixture_frames`,
    /// which is crate-private; the test skips (with a printed reason) on a host
    /// without GStreamer or a file encoder.
    fn generated_clip(root: &Path) -> Option<PathBuf> {
        use gst::prelude::*;

        if let Err(error) = modv_media::video::ensure_gst() {
            println!("skipping the video transport test: GStreamer is unavailable: {error}");
            return None;
        }

        let candidates: [(&str, &[&str], &str, &str); 4] = [
            (
                "vp8/webm",
                &["videotestsrc", "vp8enc", "webmmux", "filesink"],
                "webm",
                "videotestsrc num-buffers=60 pattern=smpte ! \
                 video/x-raw,width=160,height=120,framerate=30/1 ! vp8enc deadline=1 ! \
                 webmmux ! filesink location={location}",
            ),
            (
                "h264/matroska",
                &["videotestsrc", "x264enc", "matroskamux", "filesink"],
                "mkv",
                "videotestsrc num-buffers=60 pattern=smpte ! \
                 video/x-raw,width=160,height=120,framerate=30/1 ! \
                 x264enc speed-preset=ultrafast tune=zerolatency ! matroskamux ! \
                 filesink location={location}",
            ),
            (
                "theora/ogg",
                &["videotestsrc", "theoraenc", "oggmux", "filesink"],
                "ogv",
                "videotestsrc num-buffers=60 pattern=smpte ! \
                 video/x-raw,width=160,height=120,framerate=30/1 ! theoraenc ! oggmux ! \
                 filesink location={location}",
            ),
            (
                "raw/avi",
                &["videotestsrc", "avimux", "avidemux", "filesink"],
                "avi",
                "videotestsrc num-buffers=60 pattern=smpte ! \
                 video/x-raw,format=YUY2,width=160,height=120,framerate=30/1 ! avimux ! \
                 filesink location={location}",
            ),
        ];

        let video_dir = root.join("default/video");
        std::fs::create_dir_all(&video_dir).ok()?;

        for (label, elements, extension, description) in candidates {
            if elements
                .iter()
                .any(|element| gst::ElementFactory::find(element).is_none())
            {
                continue;
            }

            let path = video_dir.join(format!("clip.{extension}"));
            let location = path.to_string_lossy().replace('\\', "/");
            let description = description.replace("{location}", &location);

            let Ok(element) = gst::parse::launch(&description) else {
                continue;
            };
            let Ok(pipeline) = element.downcast::<gst::Pipeline>() else {
                continue;
            };
            let Some(bus) = pipeline.bus() else {
                continue;
            };
            let _ = pipeline.set_state(gst::State::Playing);
            let message = bus.timed_pop_filtered(
                gst::ClockTime::from_nseconds(20_000_000_000),
                &[gst::MessageType::Eos, gst::MessageType::Error],
            );
            let _ = pipeline.set_state(gst::State::Null);

            let ended = match message {
                Some(message) => matches!(message.view(), gst::MessageView::Eos(..)),
                None => false,
            };
            if ended {
                println!("generated the video transport clip with {label}: {path:?}");

                return Some(path);
            }

            std::fs::remove_file(&path).ok();
        }

        println!("skipping the video transport test: no suitable file encoder is installed");

        None
    }

    #[test]
    fn a_generated_clip_plays_pauses_seeks_and_rates() {
        let root = temp_dir("gstreamer");
        let Some(clip) = generated_clip(&root) else {
            std::fs::remove_dir_all(&root).ok();
            return;
        };

        let mut hub = MediaHub::new();
        if !hub.is_available() {
            println!(
                "skipping the video transport test: {}",
                hub.init_error().unwrap_or("no GStreamer")
            );
            std::fs::remove_dir_all(&root).ok();
            return;
        }

        let file_name = clip
            .strip_prefix(&root)
            .expect("the clip is inside the media root")
            .to_string_lossy()
            .into_owned();
        let request = request("module-a", "clip", &file_name);
        let mut sources = VideoSources::new();
        sources.reconcile_now(&mut hub, Some(&root), &[request], Instant::now());

        assert_eq!(
            sources.problems(),
            Vec::new(),
            "the generated clip starts through the manager"
        );
        assert_eq!(sources.source_ids(), vec![video_source_id(&clip)]);
        let id = video_source_id(&clip);

        // Wait for the demuxer to report a duration (the clip is decodable).
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut transport = None;
        while Instant::now() < deadline {
            if let Some(live) = hub.transport(&id)
                && live.duration_ms.is_some()
            {
                transport = Some(live);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let transport = transport.unwrap_or_else(|| {
            panic!(
                "the clip never reported a transport: {:?} / {:?}",
                hub.transport(&id),
                hub.statuses()
            )
        });
        assert!(transport.seekable, "{transport:?}");
        assert!(transport.looping, "files loop by default: {transport:?}");

        let paused = sources
            .transport(&mut hub, &id, VideoTransportAction::Pause)
            .expect("pause");
        assert!(paused.paused);
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            hub.transport(&id).is_some_and(|live| live.paused),
            "the pipeline stays paused"
        );
        let status = &hub.statuses()[0];
        assert_eq!(status.state, "paused");
        assert!(status.last_error.is_none());
        assert!(status.transport.is_some_and(|live| live.paused));

        let sought = sources
            .transport(
                &mut hub,
                &id,
                VideoTransportAction::Seek { position_ms: 500 },
            )
            .expect("seek");
        assert!(sought.paused, "seeking a paused clip keeps it paused");
        assert_eq!(
            hub.transport(&id).expect("transport").position_ms,
            500,
            "the seek target is reported until the flush settles"
        );

        let rated = sources
            .transport(
                &mut hub,
                &id,
                VideoTransportAction::SetPlaybackRate { rate: 2.0 },
            )
            .expect("rate");
        assert_eq!(rated.playback_rate, 2.0);

        let looped = sources
            .transport(
                &mut hub,
                &id,
                VideoTransportAction::SetLooping { looping: false },
            )
            .expect("loop off");
        assert!(!looped.looping);
        assert!(!hub.statuses()[0].transport.expect("transport").looping);

        let resumed = sources
            .transport(&mut hub, &id, VideoTransportAction::Resume)
            .expect("resume");
        assert!(!resumed.paused);

        let restarted = sources
            .transport(&mut hub, &id, VideoTransportAction::Restart)
            .expect("restart");
        assert_eq!(restarted.position_ms, 0);

        sources.stop_all(&mut hub);
        assert!(hub.ids().is_empty(), "stop_all releases the pipeline");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn trim_seek_target_wraps_past_finish_and_is_a_no_op_by_default() {
        assert_eq!(
            trim_seek_target(7_500, 0.25, 0.75, Some(10_000)),
            Some(2_500),
            "past finish wraps to the trim start"
        );
        assert_eq!(
            trim_seek_target(1_000, 0.25, 0.75, Some(10_000)),
            None,
            "inside the trim plays untouched"
        );
        assert_eq!(
            trim_seek_target(5_000, 0.0, 1.0, Some(10_000)),
            None,
            "default 0..1 must never seek"
        );
        assert_eq!(
            trim_seek_target(5_000, 0.5, 0.5, Some(10_000)),
            None,
            "empty trim has no target"
        );
        assert_eq!(
            trim_seek_target(1_000, 0.75, 0.25, Some(10_000)),
            None,
            "start>finish is used as-is (no swap), so 1000 is below finish"
        );
        assert_eq!(
            trim_seek_target(9_000, 0.75, 0.25, Some(10_000)),
            Some(7_500),
            "start>finish wraps to the given (unswapped) start"
        );
        assert_eq!(
            trim_seek_target(5_000, 0.25, 0.75, None),
            None,
            "unknown duration has no target"
        );
        assert_eq!(
            trim_seek_target(5_000, f64::NAN, 0.75, Some(10_000)),
            None,
            "non-finite trims are ignored"
        );
    }

    #[test]
    fn trim_settings_wrap_a_playing_clip_to_the_start() {
        let root = temp_dir("trim-wrap");
        std::fs::create_dir_all(root.join("default/video")).expect("video folder");
        let clip = root.join("default/video/clip.mp4");
        std::fs::write(&clip, b"fake").expect("clip");

        let mut hub = FakeHub {
            transport: Some(VideoTransport {
                duration_ms: Some(10_000),
                position_ms: 9_500,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut sources = VideoSources::new();
        let requests = vec![request("module-a", "texture", "default/video/clip.mp4")];
        sources.reconcile_now(&mut hub, Some(&root), &requests, Instant::now());
        hub.calls.clear();

        let mut start = BTreeMap::new();
        start.insert("module-a".to_string(), 0.25);
        let mut finish = BTreeMap::new();
        finish.insert("module-a".to_string(), 0.75);

        sources.sync_trim(&mut hub, &start, &finish);
        assert!(
            hub.calls
                .iter()
                .any(|c| c == &format!("seek {} 2500", video_source_id(&clip))),
            "past finish must seek to 25% of the clip: {:?}",
            hub.calls
        );

        hub.calls.clear();
        sources.sync_trim(&mut hub, &start, &finish);
        assert!(
            !hub.calls.iter().any(|c| c.starts_with("seek ")),
            "once back inside the trim there must be no further seek: {:?}",
            hub.calls
        );

        hub.transport.as_mut().unwrap().position_ms = 7_600;
        hub.calls.clear();
        sources.sync_trim(&mut hub, &start, &finish);
        assert!(
            hub.calls
                .iter()
                .any(|c| c == &format!("seek {} 2500", video_source_id(&clip))),
            "reaching finish again must seek back to the trim start: {:?}",
            hub.calls
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
