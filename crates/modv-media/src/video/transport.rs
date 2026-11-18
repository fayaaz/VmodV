//! Playback transport: pause/resume, seek and loop for video sources.
//!
//! A [`VideoSource`](super::VideoSource) is a live GStreamer pipeline, so its
//! transport is a *snapshot* of the pipeline rather than an authoritative
//! position: [`VideoTransport`] carries the last position/duration the pipeline
//! reported plus the flags the caller set. The engine UI polls it (cheaply)
//! while it draws the source; there is no callback and no extra thread.
//!
//! # Semantics
//!
//! * **Pause** moves the pipeline to `PAUSED`. The sink stops consuming the
//!   clock, so the position freezes and no new frames are published. Resuming
//!   moves it back to `PLAYING`; the engine keeps its last frame while paused,
//!   exactly like `<video>` when it is paused.
//! * **Seek** is a flushing, accurate `seek_simple`. It is only supported for
//!   file sources ([`VideoTransport::seekable`]); a live camera or test pattern
//!   has no timeline and reports [`super::VideoError::Unsupported`].
//!   A seek while paused stays paused (the pipeline prerolls the new frame and
//!   shows it), a seek while playing resumes playback from the new position, so
//!   seeking out of end-of-stream is the way to replay a finished file.
//! * **End of stream** is latched by the source's bus poll. With looping
//!   enabled (the default for files, like the legacy `<video loop>`) the source
//!   seeks back to zero and keeps playing; with looping disabled it parks on its
//!   last frame and [`VideoTransport::eos`] becomes `true` until a seek (or a
//!   resume) restarts it.
//! * **Playback rate** is applied with a flushing, accurate seek *at the current
//!   position* carrying GStreamer's seek rate (there is no separate "set rate"
//!   call). The pipeline state is untouched, so changing the rate while paused
//!   keeps the file paused (and changes nothing until it is resumed), and a
//!   file parked at end-of-stream stores the rate without replaying itself.
//!   Only positive rates in [`MIN_PLAYBACK_RATE`]..=[`MAX_PLAYBACK_RATE`] are
//!   accepted: GStreamer can play backwards with a negative rate, but the
//!   legacy `VideoControl.vue` offered 0.1–10 and the engine rejects anything
//!   outside that (and non-finite values) instead of silently reversing.
//!   The pipeline has no audio sink, so the rate only changes how fast frames
//!   are pulled from the clock.
//! * **Restart** (a new pipeline in [`super::VideoHub::start_source`])
//!   replaces the source, so the transport starts again at zero with no pause
//!   and no end-of-stream.
//!
//! All methods are called from the engine thread. GStreamer elements are
//! `Send + Sync`; state changes and seeks are queued by GStreamer and return
//! without waiting for the streaming threads, so a control never blocks the
//! render loop. Because a flushing seek makes the position query unavailable
//! for a poll or two, [`VideoTransport::position_ms`] reports the seek target
//! until the pipeline settles; the UI scrubber therefore never snaps back to
//! the start after a seek or a rate change.

use std::time::Duration;

/// A snapshot of a video source's playback state.
///
/// Positions and durations are milliseconds because that is what the control
/// plane (and the UI scrubber) uses; [`Self::position`] and [`Self::duration`]
/// convert to [`Duration`] for engine-side arithmetic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoTransport {
    /// Playback was paused through the transport API.
    pub paused: bool,
    /// Frames are being produced: the pipeline is in `PLAYING`, not paused and
    /// not parked at end-of-stream.
    pub playing: bool,
    /// The last position the pipeline reported, in milliseconds (the target of
    /// a seek until it flushes, 0 when the source has no clock, e.g. a camera).
    pub position_ms: u64,
    /// Clip length in milliseconds; `None` for live sources and for a file
    /// whose demuxer has not reported a duration yet.
    pub duration_ms: Option<u64>,
    /// File sources restart at end-of-stream instead of parking.
    pub looping: bool,
    /// The file reached end-of-stream and is parked on its last frame.
    pub eos: bool,
    /// The source supports seeking (file sources).
    pub seekable: bool,
    /// Playback speed (1.0 is real time, 0.1–10 accepted for files).
    pub playback_rate: f64,
}

impl Default for VideoTransport {
    fn default() -> Self {
        Self {
            paused: false,
            playing: false,
            position_ms: 0,
            duration_ms: None,
            looping: false,
            eos: false,
            seekable: false,
            playback_rate: 1.0,
        }
    }
}

impl VideoTransport {
    /// The position as a [`Duration`].
    pub fn position(&self) -> Duration {
        Duration::from_millis(self.position_ms)
    }

    /// The clip length as a [`Duration`], when known.
    pub fn duration(&self) -> Option<Duration> {
        self.duration_ms.map(Duration::from_millis)
    }

    /// Played fraction in `0.0..=1.0`, or `None` for live sources and before a
    /// duration is known.
    pub fn progress(&self) -> Option<f64> {
        let duration = self.duration_ms?;

        if duration == 0 {
            return None;
        }

        Some((self.position_ms as f64 / duration as f64).clamp(0.0, 1.0))
    }

    /// Milliseconds left, clamped to the clip; `None` when the duration is
    /// unknown.
    pub fn remaining_ms(&self) -> Option<u64> {
        self.duration_ms
            .map(|duration| duration.saturating_sub(self.position_ms))
    }
}

/// Clamp a requested seek position into the clip.
///
/// Seeking past the end is a UI mistake (a stale duration, a dragged scrubber),
/// and GStreamer would report end-of-stream immediately, so the target saturates
/// at the duration when one is known. Live sources have no duration and are
/// returned unchanged; [`super::VideoSource::seek`] rejects them anyway.
pub fn clamp_seek(position: Duration, duration: Option<Duration>) -> Duration {
    match duration {
        Some(duration) => position.min(duration),
        None => position,
    }
}

/// The slowest playback rate the transport accepts.
pub const MIN_PLAYBACK_RATE: f64 = 0.1;
/// The fastest playback rate the transport accepts.
pub const MAX_PLAYBACK_RATE: f64 = 10.0;

/// Validate and clamp a requested playback rate.
///
/// `None` means the caller passed something that is not a usable speed (zero,
/// negative or non-finite: `NaN`/`±infinity`); a finite positive rate is
/// returned clamped into [`MIN_PLAYBACK_RATE`]..=[`MAX_PLAYBACK_RATE`], which is
/// the span the legacy `VideoControl.vue` speed slider covered.
pub fn clamp_playback_rate(rate: f64) -> Option<f64> {
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }

    Some(rate.clamp(MIN_PLAYBACK_RATE, MAX_PLAYBACK_RATE))
}

/// Convert a duration into the millisecond field of [`VideoTransport`].
///
/// `u64::MAX` is reserved as the "duration unknown" sentinel inside the source,
/// so an absurd (`Duration::MAX`) value saturates one millisecond below it
/// instead of wrapping.
pub fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millisecond_fields_round_trip_to_durations() {
        let transport = VideoTransport {
            position_ms: 1_234,
            duration_ms: Some(5_000),
            ..VideoTransport::default()
        };

        assert_eq!(transport.position(), Duration::from_millis(1_234));
        assert_eq!(transport.duration(), Some(Duration::from_millis(5_000)));
        assert_eq!(transport.remaining_ms(), Some(3_766));
    }

    #[test]
    fn progress_reports_the_played_fraction_and_saturates() {
        let at = |position_ms, duration_ms| VideoTransport {
            position_ms,
            duration_ms,
            ..VideoTransport::default()
        };

        assert_eq!(at(1_000, Some(4_000)).progress(), Some(0.25));
        assert_eq!(at(0, Some(4_000)).progress(), Some(0.0));
        assert_eq!(
            at(9_000, Some(4_000)).progress(),
            Some(1.0),
            "a stale position past the duration must not report > 1"
        );
        assert_eq!(
            at(1_000, None).progress(),
            None,
            "live sources have no progress"
        );
        assert_eq!(
            at(1_000, Some(0)).progress(),
            None,
            "a zero duration is not a fraction"
        );
    }

    #[test]
    fn a_default_transport_has_no_timeline() {
        let transport = VideoTransport::default();

        assert!(!transport.paused);
        assert!(!transport.playing);
        assert_eq!(transport.position_ms, 0);
        assert_eq!(transport.duration_ms, None);
        assert!(!transport.looping);
        assert!(!transport.eos);
        assert!(!transport.seekable);
        assert_eq!(transport.playback_rate, 1.0);
        assert_eq!(transport.progress(), None);
        assert_eq!(transport.remaining_ms(), None);
    }

    #[test]
    fn playback_rates_are_clamped_to_the_legacy_speed_range() {
        assert_eq!(clamp_playback_rate(1.0), Some(1.0));
        assert_eq!(clamp_playback_rate(0.5), Some(0.5));
        assert_eq!(clamp_playback_rate(MIN_PLAYBACK_RATE), Some(0.1));
        assert_eq!(clamp_playback_rate(MAX_PLAYBACK_RATE), Some(10.0));
        assert_eq!(
            clamp_playback_rate(0.01),
            Some(MIN_PLAYBACK_RATE),
            "a slower request clamps to the slowest supported speed"
        );
        assert_eq!(
            clamp_playback_rate(100.0),
            Some(MAX_PLAYBACK_RATE),
            "a faster request clamps to the fastest supported speed"
        );
    }

    #[test]
    fn unusable_playback_rates_are_rejected() {
        assert_eq!(clamp_playback_rate(0.0), None, "zero is not a speed");
        assert_eq!(
            clamp_playback_rate(-1.0),
            None,
            "backwards play is rejected"
        );
        assert_eq!(clamp_playback_rate(f64::NAN), None);
        assert_eq!(clamp_playback_rate(f64::INFINITY), None);
        assert_eq!(clamp_playback_rate(f64::NEG_INFINITY), None);
    }

    #[test]
    fn seek_targets_are_clamped_to_the_clip() {
        assert_eq!(
            clamp_seek(
                Duration::from_millis(2_500),
                Some(Duration::from_millis(1_000))
            ),
            Duration::from_millis(1_000),
            "seeking past the end saturates at the duration"
        );
        assert_eq!(
            clamp_seek(
                Duration::from_millis(500),
                Some(Duration::from_millis(1_000))
            ),
            Duration::from_millis(500)
        );
        assert_eq!(
            clamp_seek(Duration::from_millis(500), None),
            Duration::from_millis(500),
            "a live source has no duration to clamp to"
        );
        assert_eq!(
            clamp_seek(Duration::ZERO, Some(Duration::from_millis(1_000))),
            Duration::ZERO
        );
    }

    #[test]
    fn duration_millis_truncates_and_saturates_below_the_sentinel() {
        assert_eq!(duration_millis(Duration::from_millis(0)), 0);
        assert_eq!(duration_millis(Duration::from_millis(1_234)), 1_234);
        assert_eq!(duration_millis(Duration::from_micros(1_999)), 1);
        assert_eq!(
            duration_millis(Duration::MAX),
            u64::MAX - 1,
            "Duration::MAX must not become the unknown-duration sentinel"
        );
    }
}
