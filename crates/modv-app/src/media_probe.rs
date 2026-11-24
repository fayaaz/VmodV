//! Off-thread media host probing.
//!
//! Camera enumeration, the PipeWire screen-support check and project-profile
//! listing are host/filesystem queries. They used to run inside the window loop
//! (once per second / two seconds in `vmodv::run`), where a slow `/sys` walk or
//! a large media tree could stall a frame. The worker performs them on its own
//! thread and publishes finished results over a channel; the window loop only
//! submits a request and applies a ready [`MediaProbeResult`].
//!
//! Media-health polling deliberately stays on the main thread: it mutates
//! GStreamer-owned pipelines and is not part of this worker's contract.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use modv_media::video::{CameraInfo, ScreenSupport};

/// A finished host probe.
#[derive(Clone, Debug)]
pub enum MediaProbeResult {
    /// The cameras enumerated and the host's PipeWire screen support.
    Cameras {
        cameras: Vec<CameraInfo>,
        screen: ScreenSupport,
    },
    /// The project profiles found under a media root.
    Profiles(Vec<String>),
}

enum Message {
    Cameras,
    Profiles(PathBuf),
    Shutdown,
}

/// The window thread's handle to the media-probe thread.
pub struct MediaProbeWorker {
    jobs: Sender<Message>,
    results: Receiver<MediaProbeResult>,
    thread: Option<JoinHandle<()>>,
}

impl MediaProbeWorker {
    /// Start the worker thread.
    pub fn new() -> Result<Self, String> {
        let (jobs, job_rx) = channel();
        let (result_tx, results) = channel();
        let thread = std::thread::Builder::new()
            .name("modv-media-probe".to_string())
            .spawn(move || run(&job_rx, &result_tx))
            .map_err(|error| format!("could not spawn the media probe worker: {error}"))?;
        Ok(Self {
            jobs,
            results,
            thread: Some(thread),
        })
    }

    /// Queue a camera + screen-support probe. Never blocks on the host query.
    ///
    /// Returns `false` when the worker thread is gone (its send failed).
    pub fn request_cameras(&self) -> bool {
        self.jobs.send(Message::Cameras).is_ok()
    }

    /// Queue a profile listing for `media_root`. The walk runs off-thread.
    ///
    /// Returns `false` when the worker thread is gone (its send failed).
    pub fn request_profiles(&self, media_root: PathBuf) -> bool {
        self.jobs.send(Message::Profiles(media_root)).is_ok()
    }

    /// Non-blocking result drain for the window loop.
    pub fn try_result(&self) -> Option<MediaProbeResult> {
        self.results.try_recv().ok()
    }

    /// True once the worker thread has stopped (shutdown or panic).
    ///
    /// A request the thread accepted but never answered can otherwise leave a
    /// caller's in-flight flag set forever; callers use this to fall back to a
    /// synchronous probe.
    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
    }
}

impl Drop for MediaProbeWorker {
    fn drop(&mut self) {
        let _ = self.jobs.send(Message::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(jobs: &Receiver<Message>, results: &Sender<MediaProbeResult>) {
    while let Ok(message) = jobs.recv() {
        match message {
            Message::Shutdown => break,
            Message::Cameras => {
                let cameras = modv_media::video::list_cameras();
                let screen = modv_media::video::screen_support();
                let _ = results.send(MediaProbeResult::Cameras { cameras, screen });
            }
            Message::Profiles(media_root) => {
                let profiles = crate::media_profiles::list_profiles(&media_root);
                let _ = results.send(MediaProbeResult::Profiles(profiles));
            }
        }
    }
}
