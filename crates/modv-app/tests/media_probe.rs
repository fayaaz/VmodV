//! Off-thread media host probing.
//!
//! Camera enumeration, the PipeWire screen-support check and project-profile
//! listing are host/filesystem queries that used to run inside the window loop
//! (once per second / two seconds in `vmodv::run`). They can stall a frame, so
//! `MediaProbeWorker` performs them on a worker thread and publishes finished
//! results over a channel. The window loop only submits a request and applies
//! a ready [`MediaProbeResult`].
//!
//! Media-health polling deliberately stays on the main thread: it mutates
//! GStreamer-owned pipelines and is not part of this worker's contract.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use modv_app::media_probe::{MediaProbeResult, MediaProbeWorker};
use modv_app::media_profiles;

/// A fresh temporary directory, unique per process and wall-clock instant.
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

/// Poll the worker until a result arrives, with a bounded wait.
fn wait_for_result(worker: &MediaProbeWorker, what: &str) -> MediaProbeResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = worker.try_result() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "the media probe worker never returned {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A profile listing must be *requested* without doing the walk on the caller:
/// the worker walks a deliberately large tree off-thread and the result must be
/// exactly what `media_profiles::list_profiles` reports for that tree.
#[test]
fn profile_listing_is_requested_off_thread_and_matches_list_profiles() {
    let root = temp_dir("modv-media-probe-profiles");
    let media = root.join("media");

    // A synchronous scan has to read_dir + stat every entry. A worker only
    // sends the PathBuf over a channel, so this tree makes a main-thread
    // implementation measurably slow while leaving the correct one fast.
    // Measured at ~22ms for a single synchronous `list_profiles` in the test
    // container, comfortably above the 10ms budget below.
    const PROFILE_COUNT: usize = 10_000;
    for index in 0..PROFILE_COUNT {
        std::fs::create_dir_all(media.join(format!("profile-{index:05}")))
            .expect("create profile directory");
    }
    // Filtered out exactly as `list_profiles` filters them.
    std::fs::create_dir_all(media.join(".cache")).expect("hidden directory");
    std::fs::write(media.join("notes.txt"), "x").expect("loose file");

    let expected = media_profiles::list_profiles(&media);
    assert_eq!(expected.len(), PROFILE_COUNT, "fixture has the profiles");

    let worker = MediaProbeWorker::new().expect("media probe worker starts");

    let started = Instant::now();
    worker.request_profiles(media.clone());
    let request_elapsed = started.elapsed();
    assert!(
        request_elapsed < Duration::from_millis(10),
        "request_profiles blocked the calling thread for {:?}; the profile scan must run off-thread",
        request_elapsed
    );

    match wait_for_result(&worker, "the profile listing") {
        MediaProbeResult::Profiles(profiles) => {
            assert_eq!(
                profiles, expected,
                "the worker reports the same profiles as list_profiles"
            );
        }
        MediaProbeResult::Cameras { .. } => {
            panic!("a profile request returned a camera/screen result");
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}

/// A camera request must produce a result without needing any capture hardware:
/// on a headless host this is an empty camera list plus an unavailable screen.
#[test]
fn camera_probe_returns_cameras_and_screen_support_even_without_hardware() {
    let worker = MediaProbeWorker::new().expect("media probe worker starts");

    worker.request_cameras();

    match wait_for_result(&worker, "the camera/screen probe") {
        MediaProbeResult::Cameras { cameras, screen } => {
            assert!(
                cameras.iter().all(|camera| !camera.id.is_empty()),
                "every reported camera has a stable id"
            );
            assert_eq!(
                screen.available(),
                screen.reason.is_none(),
                "screen support and its explanation must agree"
            );
        }
        MediaProbeResult::Profiles(_) => {
            panic!("a camera request returned a profile result");
        }
    }
}
