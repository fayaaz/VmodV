//! Video capture (WS-10): cameras, video files and screen sources.
//!
//! This replaces the renderer's `getUserMedia` → hidden `<video>` →
//! `ImageCapture.grabFrame()` → `postMessage(ImageBitmap)` chain (cameras), the
//! `<video src>` + WebCodecs path (video files) and `getDisplayMedia` →
//! `ScreenPickerDialog` (screen capture) with one GStreamer graph per source.
//!
//! ```text
//! Electron renderer                         engine (this crate)
//! ----------------                         -------------------
//! mediaDevices.enumerateDevices()  ──────▶ list_cameras()
//! ScreenPickerDialog (portal consent) ────▶ portal::request_screencast()
//!                                            └─▶ VideoSourceKind::Screen { node_id }
//! videos/assignVideoStream             ───▶ VideoSourceKind::File { uri }
//! ```
//!
//! # Layout
//!
//! * [`camera`] — `CameraInfo`/`VideoMode` enumeration through
//!   `gst::DeviceMonitor`, plus the 1-based `camera` prop resolver.
//! * [`pipeline`] — `VideoSourceConfig` and the pure pipeline-description
//!   builders (so the exact `gst-launch` text can be unit tested).
//! * [`portal`] — the engine-owned screen picker: an xdg-desktop-portal
//!   `ScreenCast` request whose system dialog yields the PipeWire node id a
//!   [`pipeline::VideoSourceKind::Screen`] needs (the portal's own dialog is
//!   the picker, so no node id has to come from Electron).
//! * [`source`] — a running [`VideoSource`]: pipeline, appsink callbacks, the
//!   latest-frame slot, bus polling, status and stop.
//! * [`transport`] — [`VideoTransport`], the playback snapshot (paused,
//!   position, duration, loop, end-of-stream) and the seek clamp.
//! * [`hub`] — [`VideoHub`], several sources keyed by a caller id, plus the
//!   pause/seek/loop controls the media panel calls.
//!
//! # Transport
//!
//! Video files play through the same pipeline as cameras and test patterns, so
//! `pause`/`resume` (a `PLAYING`/`PAUSED` state change) work for every source;
//! `seek` and the position/duration fields are meaningful for file sources
//! only. A non-looping file parks at end-of-stream (the legacy `<video>` stops
//! at the end); looping (the default, like `<video loop>`) seeks back to zero
//! seamlessly. See [`transport`] for the exact semantics.
//!
//! # Threading model
//!
//! GStreamer owns the capture/decode threads. Each source wires its `appsink`
//! to a `new-sample` callback that runs on the streaming thread, copies the
//! sample into a tightly packed RGBA8 frame and publishes it into a
//! [`frame::FrameSlot`]. The slot holds **one** ready frame plus a two-buffer
//! pool, so the streaming thread never grows memory when the consumer is slow:
//! a newer frame simply replaces the unread one and the old buffer is recycled
//! (the "newest frame wins" rule). The source publishes a `sequence` counter so
//! a consumer can tell whether it is looking at a new frame.
//!
//! The engine thread only ever takes the slot mutex for the duration of a
//! buffer swap ([`VideoSource::latest_frame_into`], which is allocation-free
//! once the two buffers exist). No GLib main loop is required: bus messages
//! (errors, end-of-stream) are drained from the engine thread whenever
//! [`VideoSource::status`], [`VideoSource::last_error`] or a frame drain is
//! called, so a dead camera surfaces as a latched error on the next poll.
//!
//! # Formats
//!
//! Camera, file and test-pattern pipelines end in
//! `video/x-raw,format=RGBA,width=W,height=H,framerate=F/1`; screen pipelines
//! keep the PipeWire node's native size/rate and only force RGBA (see
//! [`pipeline::VideoSourceKind::Screen`]). Either way
//! [`VideoFrame::rgba`] is always tightly packed row-major RGBA8 with
//! `width * height * 4` bytes and [`VideoFrame::width`]/[`VideoFrame::height`]
//! describe it — exactly what a `wgpu::TextureFormat::Rgba8Unorm` upload wants.
//! The legacy renderer drew the `ImageBitmap` into a per-device auxiliary
//! canvas; the engine instead uploads this buffer into a texture (see
//! `docs/rust-hybrid.md` WS-10).

pub mod camera;
pub mod frame;
pub mod hub;
pub mod pipeline;
pub mod portal;
pub mod source;
pub mod transport;

#[cfg(test)]
pub(crate) mod test_support;

pub use camera::{CameraInfo, VideoMode, list_cameras, select_camera};
pub use frame::VideoFrame;
pub use hub::VideoHub;
pub use pipeline::{
    DEFAULT_FPS, DEFAULT_HEIGHT, DEFAULT_WIDTH, FRAME_RATE_PRESETS, RESOLUTION_PRESETS,
    ScreenSupport, TestPattern, VideoSourceConfig, VideoSourceKind, path_to_file_uri,
    pipeline_description, screen_support,
};
pub use portal::{
    PortalError, PortalSourceType, PortalStream, RawStream, SourceSelection, StreamInfo,
    describe_streams, request_screencast, request_screencast_restoring, select_stream,
};
pub use source::{
    VideoError, VideoSource, VideoSourceState, VideoSourceStatus, ensure_gst, gstreamer_version,
};
pub use transport::{VideoTransport, clamp_seek};

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine owns sources and hands frames between threads; the public
    /// types have to move with them.
    #[test]
    fn public_types_can_move_between_threads() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<VideoSource>();
        assert_send::<VideoHub>();
        assert_send::<VideoSourceConfig>();
        assert_send::<CameraInfo>();
        assert_send::<VideoFrame>();
        assert_send::<VideoError>();
        // The portal answer travels from a worker thread to the engine that
        // stores it next to the running screen source.
        assert_send::<PortalStream>();
        assert_send::<PortalError>();
        assert_send::<SourceSelection>();
        assert_sync::<VideoHub>();
    }

    #[test]
    fn the_default_mode_is_the_legacy_texture_size() {
        let config = VideoSourceConfig::default();

        assert_eq!(
            (config.width, config.height, config.fps),
            (1280, 720, 30),
            "the engine's default texture is the legacy videoResolution"
        );
        assert!(config.caps_string().contains("format=RGBA"));
    }
}
