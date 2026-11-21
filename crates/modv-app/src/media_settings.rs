//! The VmodV shell's persisted webcam selection, order and capture modes.
//!
//! Electron persists its whole `mediaStream` module (`vuex-persistedstate`,
//! `paths: ["mediaStream"]`): an ordered `selectedVideoSources` list, where a
//! camera's 1-based position is the Webcam module's `camera` prop index, and a
//! per-device `videoSettings` map with the chosen resolution/frame rate. The
//! VmodV shell had neither: a restart forgot every camera and always asked for
//! 1280×720@30.
//!
//! The panel's choice is stored in `<userData>/vmodv-media.json`:
//!
//! ```json
//! {
//!   "cameras": ["/dev/video0", "/dev/video2"],
//!   "modes": {
//!     "/dev/video0": { "width": 1280, "height": 720, "fps": 30 }
//!   }
//! }
//! ```
//!
//! `cameras` is the ordered selected list (the Webcam index order). Ids that
//! are not enumerated right now are **kept**: an unplugged camera resumes on
//! the launch where it reappears instead of losing its slot and its mode.
//! `modes` holds the last chosen mode per camera id.
//!
//! Everything here is pure or filesystem-simple (no GStreamer, no window), so
//! the JSON round-trip, the order operations and the restore/fallback planning
//! are unit-testable on a headless host.

use std::collections::BTreeMap;
use std::path::Path;

use modv_media::video::{CameraInfo, VideoMode};

/// The persisted webcam choice inside the shared userData directory.
pub const MEDIA_SETTINGS_FILENAME: &str = "vmodv-media.json";

/// 1280×720@30: the legacy renderer's unset-camera default and
/// `VideoSourceConfig::normalised`'s fallback.
pub const DEFAULT_MODE: CameraMode = CameraMode {
    width: 1280,
    height: 720,
    fps: 30,
};

/// One persisted capture mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CameraMode {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl CameraMode {
    /// Pixel count, used for resolution ordering.
    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Whether every component is unset (`VideoSourceConfig::normalised`
    /// treats zero as "use the default").
    pub fn is_unset(&self) -> bool {
        self.width == 0 && self.height == 0 && self.fps == 0
    }

    /// Convert an advertised [`VideoMode`].
    pub fn from_video_mode(mode: VideoMode) -> Self {
        Self {
            width: mode.width,
            height: mode.height,
            fps: mode.fps,
        }
    }
}

/// The persisted webcam selection.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MediaSettings {
    /// Selected camera ids in Webcam-index order.
    #[serde(default)]
    pub cameras: Vec<String>,
    /// Last chosen capture mode per camera id.
    #[serde(default)]
    pub modes: BTreeMap<String, CameraMode>,
}

impl MediaSettings {
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
            .map_err(|error| format!("could not serialise media settings: {error}"))?;
        std::fs::write(&temporary, text)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(())
    }

    /// Whether `camera_id` is selected.
    pub fn is_selected(&self, camera_id: &str) -> bool {
        self.cameras.iter().any(|id| id == camera_id)
    }

    /// Select `camera_id`, appending it when it is new.
    ///
    /// Returns whether the document changed.
    pub fn add(&mut self, camera_id: &str) -> bool {
        if camera_id.is_empty() || self.is_selected(camera_id) {
            return false;
        }
        self.cameras.push(camera_id.to_string());
        true
    }

    /// Deselect `camera_id`, forgetting its saved mode too.
    ///
    /// Returns whether the document changed.
    pub fn remove(&mut self, camera_id: &str) -> bool {
        let selected = self.is_selected(camera_id);
        self.cameras.retain(|id| id != camera_id);
        let had_mode = self.modes.remove(camera_id).is_some();

        selected || had_mode
    }

    /// Move a selected camera by `delta` (`-1` earlier, `1` later), clamping
    /// at the ends. Returns whether the order changed.
    pub fn move_camera(&mut self, camera_id: &str, delta: i32) -> bool {
        let Some(position) = self.cameras.iter().position(|id| id == camera_id) else {
            return false;
        };
        let target = (position as i64 + i64::from(delta))
            .clamp(0, self.cameras.len().saturating_sub(1) as i64) as usize;
        if target == position {
            return false;
        }

        let id = self.cameras.remove(position);
        self.cameras.insert(target, id);
        true
    }

    /// Record the chosen mode for a camera. Returns whether it changed.
    pub fn set_mode(&mut self, camera_id: &str, mode: CameraMode) -> bool {
        if self.modes.get(camera_id) == Some(&mode) {
            return false;
        }
        self.modes.insert(camera_id.to_string(), mode);
        true
    }

    /// The saved mode for a camera, when one exists.
    pub fn mode_for(&self, camera_id: &str) -> Option<CameraMode> {
        self.modes.get(camera_id).copied()
    }
}

/// One camera the restore pass should start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreStart {
    pub camera_id: String,
    pub mode: CameraMode,
}

/// What the restore pass can do with the selected cameras right now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestorePlan {
    /// Cameras that are enumerated: start them with this mode, in order.
    pub starts: Vec<RestoreStart>,
    /// Selected cameras that are not enumerated (yet): retry on a later scan.
    pub pending: Vec<String>,
}

/// Plan the startup restore for the selected cameras.
///
/// `saved` is the ordered selection (the pending subset on later scans) and
/// `modes` its per-camera mode map. Duplicate ids are collapsed (first wins)
/// and cameras that were never selected are never started.
pub fn plan_restore(
    saved: &[String],
    modes: &BTreeMap<String, CameraMode>,
    cameras: &[CameraInfo],
) -> RestorePlan {
    let mut plan = RestorePlan::default();
    let mut seen: Vec<&str> = Vec::new();

    for camera_id in saved {
        if camera_id.is_empty() || seen.contains(&camera_id.as_str()) {
            continue;
        }
        seen.push(camera_id.as_str());

        let Some(camera) = cameras.iter().find(|camera| &camera.id == camera_id) else {
            plan.pending.push(camera_id.clone());
            continue;
        };

        plan.starts.push(RestoreStart {
            camera_id: camera_id.clone(),
            mode: resolve_mode(modes.get(camera_id).copied(), &camera.caps),
        });
    }

    plan
}

/// The mode to start `saved` with, given the modes the camera advertises.
///
/// Rule: an exact match wins; otherwise the same resolution at the closest
/// advertised frame rate; otherwise the next smaller resolution at its highest
/// frame rate; otherwise the device's maximum mode. A camera that advertises
/// no modes falls back to [`DEFAULT_MODE`] (1280×720@30), since nothing can be
/// validated against its caps.
pub fn resolve_mode(saved: Option<CameraMode>, caps: &[VideoMode]) -> CameraMode {
    if caps.is_empty() {
        return DEFAULT_MODE;
    }

    let Some(saved) = saved.filter(|mode| !mode.is_unset()) else {
        return max_mode(caps);
    };

    if let Some(exact) = caps.iter().find(|mode| {
        mode.width == saved.width && mode.height == saved.height && mode.fps == saved.fps
    }) {
        return CameraMode::from_video_mode(*exact);
    }

    // Same resolution: the closest advertised rate. An unrated mode (`fps` 0)
    // says nothing about the rate, so the saved rate is kept in that case.
    let same = caps
        .iter()
        .filter(|mode| mode.width == saved.width && mode.height == saved.height)
        .collect::<Vec<_>>();
    if !same.is_empty() {
        if let Some(closest) = same
            .iter()
            .filter(|mode| mode.fps > 0)
            .min_by_key(|mode| (mode.fps.abs_diff(saved.fps), std::cmp::Reverse(mode.fps)))
        {
            return CameraMode::from_video_mode(**closest);
        }

        return CameraMode {
            fps: saved.fps.max(1),
            ..saved
        };
    }

    // The next smaller resolution at its highest advertised rate. `max_by_key`
    // picks the largest pixel count below the saved one, then the fastest rate.
    if let Some(smaller) = caps
        .iter()
        .filter(|mode| mode.pixels() < saved.pixels())
        .max_by_key(|mode| (mode.pixels(), mode.fps, mode.width))
    {
        return CameraMode {
            fps: if smaller.fps == 0 {
                saved.fps.max(1)
            } else {
                smaller.fps
            },
            ..CameraMode::from_video_mode(*smaller)
        };
    }

    max_mode(caps)
}

/// The camera's maximum mode (highest pixel count, then highest fps).
fn max_mode(caps: &[VideoMode]) -> CameraMode {
    match caps.iter().max_by_key(|mode| (mode.pixels(), mode.fps)) {
        Some(mode) => CameraMode {
            fps: if mode.fps == 0 {
                DEFAULT_MODE.fps
            } else {
                mode.fps
            },
            ..CameraMode::from_video_mode(*mode)
        },
        None => DEFAULT_MODE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modv_media::video::VideoMode;

    #[test]
    fn legacy_native_media_filename_is_read_as_a_fallback() {
        let (directory, _path) = temp_path();
        let legacy = directory.join("native-media.json");
        let mut saved = MediaSettings::default();
        saved.add("/dev/video0");
        saved.write_to(&legacy).expect("write legacy media");
        let current = directory.join(MEDIA_SETTINGS_FILENAME);
        assert_eq!(
            MediaSettings::read_from(&current).expect("read legacy media"),
            Some(saved)
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    fn camera(id: &str, caps: &[(u32, u32, u32)]) -> CameraInfo {
        CameraInfo {
            id: id.to_string(),
            name: format!("Camera {id}"),
            path: Some(id.to_string()),
            caps: caps
                .iter()
                .map(|&(width, height, fps)| VideoMode { width, height, fps })
                .collect(),
        }
    }

    fn mode(width: u32, height: u32, fps: u32) -> CameraMode {
        CameraMode { width, height, fps }
    }

    fn modes(entries: &[(&str, CameraMode)]) -> BTreeMap<String, CameraMode> {
        entries
            .iter()
            .map(|(id, mode)| (id.to_string(), *mode))
            .collect()
    }

    fn temp_path() -> (std::path::PathBuf, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "modv-media-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = directory.join(MEDIA_SETTINGS_FILENAME);
        (directory, path)
    }

    #[test]
    fn settings_round_trip_through_json_with_order_and_modes_intact() {
        let (directory, path) = temp_path();
        let mut saved = MediaSettings::default();
        saved.add("/dev/video2");
        saved.add("/dev/video0");
        saved.set_mode("/dev/video2", mode(1920, 1080, 60));
        saved.set_mode("/dev/video0", mode(1280, 720, 30));

        saved.write_to(&path).expect("write settings");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the atomic write must not leave its temporary file behind"
        );
        assert_eq!(
            MediaSettings::read_from(&path).expect("read settings"),
            Some(saved.clone())
        );
        assert_eq!(
            saved.cameras,
            vec!["/dev/video2".to_string(), "/dev/video0".to_string()],
            "the selection order is the Webcam index order"
        );

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn the_document_uses_the_documented_shape() {
        let mut saved = MediaSettings::default();
        saved.add("/dev/video0");
        saved.set_mode("/dev/video0", mode(1280, 720, 30));

        let value = serde_json::to_value(&saved).expect("serialise");
        assert_eq!(
            value,
            serde_json::json!({
                "cameras": ["/dev/video0"],
                "modes": {
                    "/dev/video0": { "width": 1280, "height": 720, "fps": 30 }
                }
            })
        );

        // A minimal document (no modes) is accepted.
        let minimal: MediaSettings =
            serde_json::from_str(r#"{"cameras":["/dev/video1"]}"#).expect("minimal document");
        assert_eq!(minimal.cameras, vec!["/dev/video1".to_string()]);
        assert!(minimal.modes.is_empty());
    }

    #[test]
    fn a_missing_file_is_not_an_error_and_bad_documents_are() {
        let (directory, path) = temp_path();

        assert_eq!(MediaSettings::read_from(&path).expect("missing file"), None);

        std::fs::create_dir_all(&directory).expect("temp dir");
        std::fs::write(&path, "{not json").expect("write broken json");
        assert!(MediaSettings::read_from(&path).is_err());

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn camera_order_operations_keep_the_selection_ordered() {
        let mut saved = MediaSettings::default();

        assert!(!saved.is_selected("/dev/video0"));
        assert!(saved.add("/dev/video0"), "a new camera changes the list");
        assert!(!saved.add("/dev/video0"), "adding twice is a no-op");
        saved.add("/dev/video1");
        saved.add("/dev/video2");
        assert_eq!(
            saved.cameras,
            vec!["/dev/video0", "/dev/video1", "/dev/video2"]
        );

        assert!(saved.move_camera("/dev/video2", -1));
        assert_eq!(
            saved.cameras,
            vec!["/dev/video0", "/dev/video2", "/dev/video1"]
        );
        assert!(saved.move_camera("/dev/video2", 1));
        assert_eq!(
            saved.cameras,
            vec!["/dev/video0", "/dev/video1", "/dev/video2"]
        );
        assert!(
            !saved.move_camera("/dev/video0", -1),
            "moving past the first slot is a no-op"
        );
        assert!(
            !saved.move_camera("/dev/video2", 1),
            "moving past the last slot is a no-op"
        );
        assert!(
            !saved.move_camera("/dev/missing", 1),
            "an unknown id moves nothing"
        );

        saved.set_mode("/dev/video1", mode(640, 480, 15));
        assert_eq!(saved.mode_for("/dev/video1"), Some(mode(640, 480, 15)));

        assert!(saved.remove("/dev/video1"));
        assert!(!saved.remove("/dev/video1"), "removing twice is a no-op");
        assert_eq!(saved.cameras, vec!["/dev/video0", "/dev/video2"]);
        assert_eq!(
            saved.mode_for("/dev/video1"),
            None,
            "a deselected camera forgets its mode"
        );
    }

    #[test]
    fn restore_starts_saved_cameras_in_saved_order_and_pends_missing_ones() {
        let saved = ["/dev/video2", "/dev/video0", "/dev/missing"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        // Enumeration order is deliberately different from the saved order.
        let cameras = vec![
            camera("/dev/video0", &[(1280, 720, 30)]),
            camera("/dev/video2", &[(640, 480, 30)]),
            camera("/dev/video9", &[(1920, 1080, 30)]),
        ];

        let plan = plan_restore(&saved, &BTreeMap::new(), &cameras);

        assert_eq!(
            plan.starts,
            vec![
                RestoreStart {
                    camera_id: "/dev/video2".to_string(),
                    mode: mode(640, 480, 30),
                },
                RestoreStart {
                    camera_id: "/dev/video0".to_string(),
                    mode: DEFAULT_MODE,
                },
            ],
            "saved order wins over enumeration order, and unselected cameras never start"
        );
        assert_eq!(plan.pending, vec!["/dev/missing".to_string()]);
    }

    #[test]
    fn restore_uses_the_exact_advertised_mode() {
        let saved = vec!["/dev/video0".to_string()];
        let modes = modes(&[("/dev/video0", mode(1280, 720, 30))]);
        let cameras = vec![camera(
            "/dev/video0",
            &[(640, 480, 30), (1280, 720, 30), (1920, 1080, 60)],
        )];

        assert_eq!(
            plan_restore(&saved, &modes, &cameras).starts[0].mode,
            mode(1280, 720, 30)
        );
    }

    #[test]
    fn restore_falls_back_to_the_closest_fps_at_the_same_resolution() {
        let saved = vec!["/dev/video0".to_string()];
        let cameras = vec![camera(
            "/dev/video0",
            &[(1280, 720, 25), (1280, 720, 60), (640, 480, 30)],
        )];

        assert_eq!(
            plan_restore(
                &saved,
                &modes(&[("/dev/video0", mode(1280, 720, 30))]),
                &cameras
            )
            .starts[0]
                .mode,
            mode(1280, 720, 25),
            "the closest advertised rate at the saved resolution wins"
        );
        assert_eq!(
            plan_restore(
                &saved,
                &modes(&[("/dev/video0", mode(1280, 720, 90))]),
                &cameras
            )
            .starts[0]
                .mode,
            mode(1280, 720, 60)
        );
    }

    #[test]
    fn restore_falls_back_to_the_next_smaller_resolution_with_its_max_fps() {
        let saved = vec!["/dev/video0".to_string()];
        let cameras = vec![camera(
            "/dev/video0",
            &[
                (640, 480, 30),
                (1280, 720, 25),
                (1280, 720, 60),
                (1920, 1080, 30),
            ],
        )];

        assert_eq!(
            plan_restore(
                &saved,
                &modes(&[("/dev/video0", mode(1600, 900, 30))]),
                &cameras
            )
            .starts[0]
                .mode,
            mode(1280, 720, 60),
            "the largest resolution below the saved one, at its highest rate"
        );
    }

    #[test]
    fn restore_falls_back_to_the_device_max_mode_when_nothing_is_smaller() {
        let saved = vec!["/dev/video0".to_string()];
        let cameras = vec![camera("/dev/video0", &[(640, 480, 30), (1920, 1080, 30)])];

        assert_eq!(
            plan_restore(
                &saved,
                &modes(&[("/dev/video0", mode(320, 240, 30))]),
                &cameras
            )
            .starts[0]
                .mode,
            mode(1920, 1080, 30)
        );
    }

    #[test]
    fn restore_uses_the_max_mode_for_a_camera_with_no_saved_mode() {
        let saved = vec!["/dev/video0".to_string()];
        let cameras = vec![camera(
            "/dev/video0",
            &[(640, 480, 30), (1280, 720, 25), (1280, 720, 60)],
        )];

        assert_eq!(
            plan_restore(&saved, &BTreeMap::new(), &cameras).starts[0].mode,
            mode(1280, 720, 60),
            "the default is the camera's maximum mode (highest pixels, then fps)"
        );
    }

    #[test]
    fn restore_falls_back_to_720p30_when_the_camera_advertises_no_modes() {
        let saved = vec!["/dev/video0".to_string()];
        let cameras = vec![camera("/dev/video0", &[])];

        assert_eq!(
            plan_restore(&saved, &BTreeMap::new(), &cameras).starts[0].mode,
            DEFAULT_MODE
        );
        // Nothing can be validated against caps that are not reported, so a
        // saved mode also falls back to the documented 1280×720@30 default.
        assert_eq!(
            plan_restore(
                &saved,
                &modes(&[("/dev/video0", mode(800, 600, 24))]),
                &cameras
            )
            .starts[0]
                .mode,
            DEFAULT_MODE
        );
    }

    #[test]
    fn restore_keeps_the_saved_rate_when_the_mode_advertises_no_rate() {
        let saved = vec!["/dev/video0".to_string()];
        let cameras = vec![camera("/dev/video0", &[(1280, 720, 0)])];

        assert_eq!(
            plan_restore(
                &saved,
                &modes(&[("/dev/video0", mode(1280, 720, 30))]),
                &cameras
            )
            .starts[0]
                .mode,
            mode(1280, 720, 30),
            "fps 0 means the device did not advertise a rate"
        );
        assert_eq!(
            resolve_mode(
                None,
                &[VideoMode {
                    width: 1280,
                    height: 720,
                    fps: 0
                }]
            ),
            DEFAULT_MODE,
            "an unrated maximum mode falls back to 30 fps"
        );
    }

    #[test]
    fn restore_deduplicates_saved_and_enumerated_camera_ids() {
        let saved = vec![
            "/dev/video0".to_string(),
            "/dev/video0".to_string(),
            "/dev/missing".to_string(),
        ];
        let cameras = vec![
            camera("/dev/video0", &[(640, 480, 30)]),
            camera("/dev/video0", &[(1280, 720, 30)]),
        ];

        let plan = plan_restore(&saved, &BTreeMap::new(), &cameras);
        assert_eq!(
            plan.starts
                .iter()
                .map(|start| start.camera_id.as_str())
                .collect::<Vec<_>>(),
            vec!["/dev/video0"],
            "a duplicate selection starts the device once"
        );
        assert_eq!(plan.pending, vec!["/dev/missing".to_string()]);
    }

    #[test]
    fn a_pending_selection_resumes_when_the_camera_reappears() {
        let first = plan_restore(
            &["/dev/video1".to_string()],
            &BTreeMap::new(),
            &[camera("/dev/video0", &[(640, 480, 30)])],
        );
        assert!(first.starts.is_empty());
        assert_eq!(first.pending, vec!["/dev/video1".to_string()]);

        let second = plan_restore(
            &first.pending,
            &BTreeMap::new(),
            &[camera("/dev/video1", &[(1920, 1080, 30)])],
        );
        assert_eq!(
            second.starts,
            vec![RestoreStart {
                camera_id: "/dev/video1".to_string(),
                mode: mode(1920, 1080, 30),
            }]
        );
        assert!(second.pending.is_empty());
    }
}
