//! Several capture sources keyed by a caller id.
//!
//! The engine runs one source per selected camera, per screen share and per
//! video-file module, and addresses them with the id the control plane gave
//! them (the renderer's `deviceId`, a screen node id, a video module id). The
//! hub is that map plus a start/stop lifecycle and the playback transport
//! (pause/seek/loop/position) the media panel drives; it owns no threads of its
//! own — each [`VideoSource`] owns its pipeline, and GStreamer owns the
//! streaming threads.
//!
//! Starting (or restarting) a source replaces its pipeline, so the transport of
//! a restarted source begins again at zero: not paused, not ended, no cached
//! duration.

use std::collections::BTreeMap;

use super::frame::VideoFrame;
use super::pipeline::VideoSourceConfig;
use super::source::{VideoError, VideoSource, VideoSourceStatus};
use super::transport::VideoTransport;

/// A collection of running video sources, keyed by caller id.
///
/// Ids are kept in a `BTreeMap` so [`Self::statuses`] and [`Self::ids`] are
/// deterministic.
#[derive(Default)]
pub struct VideoHub {
    sources: BTreeMap<String, VideoSource>,
}

impl VideoHub {
    /// An empty hub.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start (or replace) the source registered as `id`.
    ///
    /// A restart **stops the previous pipeline for `id` first**: a V4L2 device
    /// can only be opened by one streaming client, so a new `v4l2src` fails
    /// with "Device is busy" while the old one is still running. The
    /// consequence is that a failed restart leaves no source registered for
    /// `id` (no half-started pipeline either); the engine retries on its next
    /// poll, exactly like the legacy renderer's reconnect loop.
    pub fn start_source(
        &mut self,
        id: impl Into<String>,
        config: VideoSourceConfig,
    ) -> Result<(), VideoError> {
        let id = id.into();

        if let Some(mut previous) = self.sources.remove(&id) {
            previous.stop();

            // Join the old pipeline's threads before the new start touches the
            // device.
            drop(previous);
        }

        let source = VideoSource::start(config)?;

        self.sources.insert(id, source);

        Ok(())
    }

    /// Stop and forget the source registered as `id`.
    ///
    /// Returns `false` when no source had that id.
    pub fn stop_source(&mut self, id: &str) -> bool {
        match self.sources.remove(id) {
            Some(mut source) => {
                source.stop();

                true
            }
            None => false,
        }
    }

    /// Stop every source.
    pub fn stop_all(&mut self) {
        for source in self.sources.values_mut() {
            source.stop();
        }

        self.sources.clear();
    }

    /// Drain the newest frame of `id` (allocation-free).
    ///
    /// Returns `false` when the id is unknown or no new frame is waiting.
    pub fn latest_into(&mut self, id: &str, frame: &mut VideoFrame) -> bool {
        self.sources
            .get_mut(id)
            .is_some_and(|source| source.latest_frame_into(frame))
    }

    /// Take the newest frame of `id` by value.
    pub fn latest(&mut self, id: &str) -> Option<VideoFrame> {
        self.sources.get_mut(id).and_then(VideoSource::latest_frame)
    }

    /// Status of `id`, or `None` when it is not running.
    pub fn status(&self, id: &str) -> Option<VideoSourceStatus> {
        self.sources.get(id).map(VideoSource::status)
    }

    /// Status of every source, sorted by id.
    pub fn statuses(&self) -> Vec<(String, VideoSourceStatus)> {
        self.sources
            .iter()
            .map(|(id, source)| (id.clone(), source.status()))
            .collect()
    }

    /// Whether `id` has a source that has not been stopped.
    pub fn is_running(&self, id: &str) -> bool {
        self.sources
            .get(id)
            .is_some_and(|source| source.is_running())
    }

    // --- transport --------------------------------------------------------

    /// Playback state of `id`, or `None` when it is not running.
    pub fn transport(&self, id: &str) -> Option<VideoTransport> {
        self.sources.get(id).map(VideoSource::transport)
    }

    /// Transport snapshots of every source, sorted by id.
    pub fn transports(&self) -> Vec<(String, VideoTransport)> {
        self.sources
            .iter()
            .map(|(id, source)| (id.clone(), source.transport()))
            .collect()
    }

    /// Pause `id`.
    pub fn pause_source(&self, id: &str) -> Result<VideoTransport, VideoError> {
        self.with_source(id)?.pause()
    }

    /// Resume `id` (a file parked at end-of-stream replays from zero).
    pub fn resume_source(&self, id: &str) -> Result<VideoTransport, VideoError> {
        self.with_source(id)?.resume()
    }

    /// Pause a playing source or resume a paused one.
    pub fn toggle_paused(&self, id: &str) -> Result<VideoTransport, VideoError> {
        self.with_source(id)?.toggle_paused()
    }

    /// Set the pause state of `id`.
    pub fn set_paused(&self, id: &str, paused: bool) -> Result<VideoTransport, VideoError> {
        self.with_source(id)?.set_paused(paused)
    }

    /// Change the playback speed of a video file (1.0 = real time, 0.1–10).
    ///
    /// See [`VideoSource::set_playback_rate`]: GStreamer applies a rate with a
    /// flushing seek at the current position, so the pause state and the
    /// position are preserved.
    pub fn set_playback_rate(&self, id: &str, rate: f64) -> Result<VideoTransport, VideoError> {
        self.with_source(id)?.set_playback_rate(rate)
    }

    /// Seek a video file to `position` (clamped to the clip; see
    /// [`VideoSource::seek`] for the paused/playing semantics).
    pub fn seek_source(
        &self,
        id: &str,
        position: std::time::Duration,
    ) -> Result<VideoTransport, VideoError> {
        self.with_source(id)?.seek(position)
    }

    /// Enable/disable file looping for `id`.
    ///
    /// Enabling looping on a file that already parked at end-of-stream replays
    /// it from the beginning, so the UI's loop toggle brings a finished clip
    /// back instead of leaving it on its last frame. Harmless for live sources.
    pub fn set_looping(&self, id: &str, looping: bool) -> Result<VideoTransport, VideoError> {
        let source = self.with_source(id)?;

        source.set_looping(looping);

        let transport = source.transport();

        if looping && transport.eos && source.is_seekable() {
            return source.seek(std::time::Duration::ZERO);
        }

        Ok(transport)
    }

    /// Alias for [`Self::set_looping`], matching the control-plane field name.
    pub fn set_loop(&self, id: &str, looping: bool) -> Result<VideoTransport, VideoError> {
        self.set_looping(id, looping)
    }

    fn with_source(&self, id: &str) -> Result<&VideoSource, VideoError> {
        self.sources
            .get(id)
            .ok_or_else(|| VideoError::UnknownSource(id.to_string()))
    }

    /// The registered ids, sorted.
    pub fn ids(&self) -> Vec<&str> {
        self.sources.keys().map(String::as_str).collect()
    }

    /// How many sources are registered.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Whether no source is registered.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Access one source (e.g. to read its [`VideoSource::transport`]).
    pub fn source(&self, id: &str) -> Option<&VideoSource> {
        self.sources.get(id)
    }

    /// Mutable access to one source.
    pub fn source_mut(&mut self, id: &str) -> Option<&mut VideoSource> {
        self.sources.get_mut(id)
    }
}

impl Drop for VideoHub {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::test_support;
    use crate::video::{TestPattern, VideoFrame};

    #[test]
    fn an_empty_hub_answers_missing_ids() {
        let mut hub = VideoHub::new();

        assert!(hub.is_empty());
        assert_eq!(hub.len(), 0);
        assert!(!hub.latest_into("missing", &mut VideoFrame::default()));
        assert!(hub.latest("missing").is_none());
        assert!(hub.status("missing").is_none());
        assert!(!hub.stop_source("missing"));
        assert!(!hub.is_running("missing"));
        assert_eq!(hub.ids(), Vec::<&str>::new());
        assert_eq!(hub.statuses(), Vec::new());
    }

    #[test]
    fn an_empty_hub_answers_transport_controls_gracefully() {
        let hub = VideoHub::new();

        assert!(hub.transport("missing").is_none());
        assert_eq!(hub.transports(), Vec::new());

        for result in [
            hub.pause_source("missing"),
            hub.resume_source("missing"),
            hub.toggle_paused("missing"),
            hub.set_paused("missing", true),
            hub.seek_source("missing", std::time::Duration::from_millis(100)),
            hub.set_looping("missing", true),
            hub.set_playback_rate("missing", 2.0),
        ] {
            match result {
                Err(VideoError::UnknownSource(id)) => assert_eq!(id, "missing"),
                other => panic!("expected UnknownSource, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_hub_routes_transport_controls_to_its_sources() {
        if let Some(reason) = test_support::missing_elements(&["videotestsrc", "appsink"]) {
            println!("skipping the hub transport: {reason}");
            return;
        }

        let mut hub = VideoHub::new();

        hub.start_source(
            "pattern",
            VideoSourceConfig::test_pattern(TestPattern::Smpte).with_mode(160, 120, 30),
        )
        .expect("test pattern source");

        let mut frame = VideoFrame::with_size(160, 120);
        let deadline = std::time::Instant::now() + test_support::FRAME_TIMEOUT;
        let mut started = false;

        while !started && std::time::Instant::now() < deadline {
            started = hub.latest_into("pattern", &mut frame);
        }

        assert!(started, "the pattern delivered no frame");

        let transport = hub.transport("pattern").expect("a transport");
        assert!(!transport.paused);
        assert!(!transport.seekable);

        let paused = hub.pause_source("pattern").expect("pause");
        assert!(paused.paused);
        assert!(hub.transport("pattern").expect("status").paused);

        let toggled = hub.toggle_paused("pattern").expect("toggle");
        assert!(!toggled.paused, "toggle resumed the paused source");

        assert!(hub.set_paused("pattern", true).expect("set_paused").paused);
        assert!(!hub.resume_source("pattern").expect("resume").paused);

        let looping = hub.set_looping("pattern", false).expect("loop off");
        assert!(!looping.looping);
        assert!(!hub.transport("pattern").expect("status").looping);

        let error = hub
            .seek_source("pattern", std::time::Duration::from_millis(10))
            .expect_err("a live source cannot seek");
        assert!(matches!(error, VideoError::Unsupported(_)), "{error}");

        let error = hub
            .set_playback_rate("pattern", 2.0)
            .expect_err("a live source has no timeline to rate");
        assert!(matches!(error, VideoError::Unsupported(_)), "{error}");

        let transports = hub.transports();
        assert_eq!(transports.len(), 1);
        assert_eq!(transports[0].0, "pattern");

        assert!(hub.stop_source("pattern"));
        assert!(hub.transport("pattern").is_none());
    }

    #[test]
    fn restarting_a_source_clears_its_transport() {
        if let Some(reason) = test_support::missing_elements(&["videotestsrc", "appsink"]) {
            println!("skipping the transport restart: {reason}");
            return;
        }

        let mut hub = VideoHub::new();

        hub.start_source(
            "pattern",
            VideoSourceConfig::test_pattern(TestPattern::Smpte).with_mode(64, 48, 30),
        )
        .expect("first source");
        hub.pause_source("pattern").expect("pause");
        hub.set_looping("pattern", false).expect("loop off");

        assert!(hub.transport("pattern").expect("status").paused);

        hub.start_source(
            "pattern",
            VideoSourceConfig::test_pattern(TestPattern::Smpte).with_mode(64, 48, 30),
        )
        .expect("restarted source");

        let fresh = hub.transport("pattern").expect("restarted transport");

        assert!(!fresh.paused, "a restart starts unpaused: {fresh:?}");
        assert!(!fresh.eos, "{fresh:?}");
        assert_eq!(fresh.position_ms, 0);
        assert!(fresh.looping, "a new source loops by default: {fresh:?}");
        assert_eq!(
            fresh.playback_rate, 1.0,
            "a restart resets the playback rate"
        );

        hub.stop_all();
    }

    #[test]
    fn enabling_loop_after_end_of_stream_restarts_a_file() {
        let Some(path) = test_support::transport_video_fixture() else {
            return;
        };

        let mut hub = VideoHub::new();

        hub.start_source(
            "file",
            VideoSourceConfig::file(&path).with_mode(160, 120, 30),
        )
        .unwrap_or_else(|error| panic!("decode {path:?}: {error}"));
        hub.set_looping("file", false).expect("loop off");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);

        while std::time::Instant::now() < deadline
            && !hub.transport("file").is_some_and(|transport| transport.eos)
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let ended = hub.transport("file").expect("file transport");

        assert!(ended.eos, "the file did not reach EOS: {ended:?}");
        assert!(!ended.paused, "{ended:?}");

        let restarted = hub.set_looping("file", true).expect("loop on");

        assert!(restarted.looping);
        assert!(
            !restarted.eos,
            "enabling loop restarts a parked file: {restarted:?}"
        );

        let frames = hub.status("file").expect("file status").frames;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut playing = false;

        while std::time::Instant::now() < deadline {
            if hub
                .status("file")
                .is_some_and(|status| status.frames > frames)
            {
                playing = true;

                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(playing, "the restarted file delivered no frame");

        hub.stop_source("file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn hub_runs_independent_sources_and_stops_them() {
        if let Some(reason) = test_support::missing_elements(&["videotestsrc", "appsink"]) {
            println!("skipping the hub capture: {reason}");
            return;
        }

        let mut hub = VideoHub::new();

        hub.start_source(
            "camera-a",
            VideoSourceConfig::test_pattern(TestPattern::Smpte).with_mode(160, 120, 30),
        )
        .expect("first source");
        hub.start_source(
            "camera-b",
            VideoSourceConfig::test_pattern(TestPattern::Snow).with_mode(320, 240, 30),
        )
        .expect("second source");

        assert_eq!(hub.ids(), vec!["camera-a", "camera-b"]);
        assert!(hub.is_running("camera-a"));

        let mut first = VideoFrame::with_size(160, 120);
        let mut second = VideoFrame::with_size(320, 240);
        let deadline = std::time::Instant::now() + test_support::FRAME_TIMEOUT;
        let (mut saw_first, mut saw_second) = (false, false);

        while !(saw_first && saw_second) && std::time::Instant::now() < deadline {
            saw_first |= hub.latest_into("camera-a", &mut first);
            saw_second |= hub.latest_into("camera-b", &mut second);

            if !(saw_first && saw_second) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        assert!(saw_first, "camera-a delivered no frame");
        assert!(saw_second, "camera-b delivered no frame");
        assert_eq!((first.width, first.height), (160, 120));
        assert_eq!((second.width, second.height), (320, 240));

        let statuses = hub.statuses();

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].0, "camera-a");
        assert!(statuses[0].1.frames >= 1);

        assert!(hub.stop_source("camera-a"));
        assert!(!hub.stop_source("camera-a"));
        assert_eq!(hub.ids(), vec!["camera-b"]);

        hub.stop_all();

        assert!(hub.is_empty());
        assert!(!hub.is_running("camera-b"));
    }

    #[test]
    fn a_failed_restart_releases_the_previous_source() {
        if let Some(reason) = test_support::missing_elements(&["videotestsrc", "appsink"]) {
            println!("skipping the hub restart: {reason}");
            return;
        }

        let mut hub = VideoHub::new();

        hub.start_source(
            "camera",
            VideoSourceConfig::test_pattern(TestPattern::Smpte).with_mode(64, 48, 30),
        )
        .expect("initial source");

        let failure = hub
            .start_source(
                "camera",
                // A file URI that does not exist fails to preroll.
                VideoSourceConfig::file("/nonexistent/modv-engine-test.mp4").with_mode(64, 48, 30),
            )
            .expect_err("the missing file must fail");

        assert!(matches!(failure, VideoError::State(_)), "{failure}");
        assert!(
            !hub.is_running("camera"),
            "the old pipeline is released before the new one starts"
        );
        assert_eq!(hub.len(), 0, "no half-started source is registered");

        // Re-registering works: the engine's reconnect path just calls again.
        hub.start_source(
            "camera",
            VideoSourceConfig::test_pattern(TestPattern::Smpte).with_mode(64, 48, 30),
        )
        .expect("re-registering");

        assert!(hub.is_running("camera"));

        hub.stop_all();
    }

    #[test]
    fn restarting_a_camera_releases_the_device_first() {
        if let Some(reason) = test_support::missing_elements(&["v4l2src", "appsink"]) {
            println!("skipping the camera restart: {reason}");
            return;
        }

        let _camera = test_support::live_video_lock();
        let id = match std::env::var("MODV_TEST_CAMERA_ID") {
            Ok(id) => id,
            Err(_) => {
                let cameras = crate::video::list_cameras();

                match cameras.first() {
                    Some(camera) => camera.id.clone(),
                    None => {
                        println!(
                            "skipping the camera restart: no V4L2 source devices on this host"
                        );

                        return;
                    }
                }
            }
        };

        let mut hub = VideoHub::new();

        // The device can be held by another client (a PipeWire camera node, a
        // browser); skip instead of failing a shared-hardware run.
        if let Err(error) = hub.start_source(
            "camera",
            VideoSourceConfig::camera(id.clone()).with_mode(640, 480, 30),
        ) {
            println!("skipping the camera restart ({id:?}): {error}");

            return;
        }

        let mut frame = VideoFrame::with_size(640, 480);
        let deadline = std::time::Instant::now() + test_support::FRAME_TIMEOUT;

        while std::time::Instant::now() < deadline && !hub.latest_into("camera", &mut frame) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert_eq!((frame.width, frame.height), (640, 480));

        // A second v4l2src on the same device fails with "Device is busy"
        // unless the hub released the first pipeline.
        hub.start_source(
            "camera",
            VideoSourceConfig::camera(id.clone()).with_mode(320, 240, 30),
        )
        .unwrap_or_else(|error| panic!("restart of {id:?} while it was busy: {error}"));

        let mut frame = VideoFrame::with_size(320, 240);
        let deadline = std::time::Instant::now() + test_support::FRAME_TIMEOUT;
        let mut restarted = false;

        while std::time::Instant::now() < deadline {
            if hub.latest_into("camera", &mut frame) {
                restarted = true;

                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(
            restarted,
            "the restarted camera delivered no frame: {:?}",
            hub.status("camera")
        );
        assert_eq!((frame.width, frame.height), (320, 240));

        hub.stop_all();
    }
}
