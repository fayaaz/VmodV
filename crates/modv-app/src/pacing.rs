//! Render-loop pacing for the VmodV shell.
//!
//! The Electron worker renders at `store.state.fps.fps` (60 by default) and
//! advances per-frame work — tween frames, audio smoothing, canvas feedback —
//! once per paced frame. The VmodV winit loop used to redraw as fast as the
//! event loop allowed (~140 fps on a fast GPU), so those systems ran 2-4x
//! faster and visuals changed speed with the host frame rate. This pacer gives
//! the VmodV loop the same fixed cadence, independent of GPU speed.

use std::time::{Duration, Instant};

/// The default target when the store has no usable FPS value.
pub const DEFAULT_FPS: f64 = 60.0;

/// The lowest target the pacer accepts (one frame per second).
pub const MIN_FPS: f64 = 1.0;
/// The highest target the pacer accepts (1000 fps).
pub const MAX_FPS: f64 = 1000.0;

/// Convert a target frame rate into a frame interval.
///
/// Non-finite and non-positive values fall back to [`DEFAULT_FPS`]; the result
/// is clamped to [`MIN_FPS`]..=[`MAX_FPS`] so a corrupt store value cannot make
/// the loop spin or stall.
pub fn frame_interval(fps: f64) -> Duration {
    let fps = if fps.is_finite() && fps > 0.0 {
        fps.clamp(MIN_FPS, MAX_FPS)
    } else {
        DEFAULT_FPS
    };
    Duration::from_secs_f64(1.0 / fps)
}

/// A deadline-based frame clock.
///
/// `due` is true once the deadline has passed; `advance` schedules the next
/// frame from `now`, so a slow frame does not trigger a catch-up burst.
#[derive(Clone, Debug)]
pub struct FramePacer {
    interval: Duration,
    next: Instant,
}

impl FramePacer {
    /// A pacer that is due immediately at `now`.
    pub fn new(now: Instant, fps: f64) -> Self {
        Self {
            interval: frame_interval(fps),
            next: now,
        }
    }

    /// Change the target rate, keeping the current deadline.
    pub fn set_fps(&mut self, fps: f64) {
        self.interval = frame_interval(fps);
    }

    /// The current frame interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// The next deadline.
    pub fn deadline(&self) -> Instant {
        self.next
    }

    /// Whether a frame is due at `now`.
    pub fn due(&self, now: Instant) -> bool {
        now >= self.next
    }

    /// Schedule the next frame after a frame rendered at `now`.
    ///
    /// The deadline advances from the previous one rather than from `now`, so
    /// ordinary frame-time jitter does not slowly stretch the cadence. If the
    /// next deadline is already in the past (a long stall), it jumps forward
    /// from `now` instead of firing a catch-up burst.
    pub fn advance(&mut self, now: Instant) {
        self.next += self.interval;
        if self.next <= now {
            self.next = now + self.interval;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_defaults_and_clamps() {
        assert_eq!(frame_interval(60.0), Duration::from_secs_f64(1.0 / 60.0));
        assert_eq!(frame_interval(0.0), frame_interval(DEFAULT_FPS));
        assert_eq!(frame_interval(f64::NAN), frame_interval(DEFAULT_FPS));
        assert_eq!(frame_interval(-5.0), frame_interval(DEFAULT_FPS));
        assert_eq!(frame_interval(10_000.0), frame_interval(MAX_FPS));
        assert_eq!(frame_interval(0.1), frame_interval(MIN_FPS));
    }

    #[test]
    fn pacer_is_due_immediately_then_waits_one_interval() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start, 60.0);
        assert!(pacer.due(start));

        pacer.advance(start);
        assert!(!pacer.due(start + Duration::from_millis(16)));
        assert!(pacer.due(start + Duration::from_millis(17)));
    }

    #[test]
    fn sixty_hertz_pacing_renders_sixty_frames_per_second() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start, 60.0);
        let mut rendered = 0;
        // Tick the clock at 1 ms, as the winit loop's wait deadline does.
        for millis in 0..=2_000 {
            let now = start + Duration::from_millis(millis);
            if pacer.due(now) {
                rendered += 1;
                pacer.advance(now);
            }
        }
        // 2 s at 60 fps is 121 frames from t=0 (0, 16.7, …, 2000).
        assert!(
            (119..=121).contains(&rendered),
            "expected ~120 frames in 2 s, got {rendered}"
        );
    }

    #[test]
    fn a_slow_frame_does_not_burst_catch_up() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start, 60.0);
        pacer.advance(start);
        let late = start + Duration::from_millis(500);
        assert!(pacer.due(late));
        pacer.advance(late);
        assert!(!pacer.due(late + Duration::from_millis(1)));
        assert!(pacer.due(late + Duration::from_millis(17)));
    }

    #[test]
    fn changing_the_target_rate_takes_effect_after_the_current_deadline() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start, 60.0);
        pacer.advance(start);

        // The already-scheduled 60 Hz deadline still fires.
        pacer.set_fps(30.0);
        assert!(pacer.due(start + Duration::from_millis(17)));

        // The following frame uses the new 30 Hz interval.
        pacer.advance(start + Duration::from_millis(17));
        assert!(!pacer.due(start + Duration::from_millis(34)));
        assert!(pacer.due(start + Duration::from_millis(50)));
    }
}
