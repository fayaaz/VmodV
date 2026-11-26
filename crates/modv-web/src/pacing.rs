//! Output frame pacing.
//!
//! `requestAnimationFrame` runs at the display refresh rate (144 Hz here, for
//! example). A fixed output schedule is anchored to a 60 Hz deadline grid:
//! each render advances the deadline by one interval instead of scheduling
//! `now + interval`. That keeps the average at 60 fps on a 144 Hz display
//! (two- and three-frame gaps) instead of phase-locking to 144/3 = 48 fps.

use std::time::Duration;

/// The output frame rate the browser shell runs at.
pub const TARGET_FPS: f64 = 60.0;

/// A monotonic schedule in seconds (egui's `input.time`).
#[derive(Clone, Copy, Debug)]
pub struct FramePacer {
    target_fps: f64,
    next: f64,
    started: bool,
}

impl FramePacer {
    /// A pacer at `target_fps`. Non-finite or non-positive rates fall back to
    /// [`TARGET_FPS`].
    pub fn new(target_fps: f64) -> Self {
        let target_fps = if target_fps.is_finite() && target_fps > 0.0 {
            target_fps
        } else {
            TARGET_FPS
        };
        Self {
            target_fps,
            next: 0.0,
            started: false,
        }
    }

    /// The interval between output frames, in seconds.
    pub fn interval(&self) -> f64 {
        1.0 / self.target_fps
    }

    /// Whether an output frame is due at `now`.
    ///
    /// A millisecond of tolerance absorbs the difference between egui's input
    /// clock and the browser frame timestamp on a 60 Hz display.
    pub fn due(&self, now: f64) -> bool {
        const EPSILON: f64 = 0.001;
        !self.started || now + EPSILON >= self.next
    }

    /// Record that an output frame rendered at `now`.
    ///
    /// The deadline advances on a fixed grid; a late frame skips missed
    /// deadlines instead of queuing catch-up renders.
    pub fn mark(&mut self, now: f64) {
        if !self.started {
            self.started = true;
            self.next = now + self.interval();
            return;
        }
        // Advance the grid by one interval even when the tolerance allowed
        // the frame a hair early, so the same deadline cannot fire twice.
        self.next += self.interval();
        while self.next <= now {
            self.next += self.interval();
        }
    }

    /// How long to wait before the next output frame, for
    /// `Context::request_repaint_after`. Never negative.
    pub fn delay(&self, now: f64) -> Duration {
        if !self.started {
            return Duration::ZERO;
        }
        Duration::from_secs_f64((self.next - now).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_fps_is_a_sixteen_millisecond_budget() {
        let pacer = FramePacer::new(60.0);
        assert!((pacer.interval() - 1.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn the_first_frame_is_due_and_marking_starts_the_clock() {
        let mut pacer = FramePacer::new(60.0);
        assert!(pacer.due(0.0));
        pacer.mark(0.0);
        assert!(!pacer.due(0.0));
        assert!(!pacer.due(0.015));
        assert!(pacer.due(0.016));
    }

    #[test]
    fn the_delay_never_goes_negative() {
        let mut pacer = FramePacer::new(60.0);
        pacer.mark(1.0);
        assert_eq!(pacer.delay(2.0), Duration::ZERO);
    }

    #[test]
    fn a_late_frame_does_not_accumulate_a_burst() {
        let mut pacer = FramePacer::new(60.0);
        pacer.mark(0.0);
        pacer.mark(0.010);
        assert!(!pacer.due(0.020));
        assert!(pacer.due(0.033));
    }

    #[test]
    fn the_deadline_grid_keeps_sixty_on_a_144_hz_display() {
        // rAF timestamps at 144 Hz (6.94 ms). A fixed 60 Hz grid must render
        // 6 times in 100 ms, not 144/3 = 48.
        let mut pacer = FramePacer::new(60.0);
        let mut rendered = 0;
        let mut now = 0.0;
        let end = 1.0;
        while now < end {
            if pacer.due(now) {
                pacer.mark(now);
                rendered += 1;
            }
            now += 1.0 / 144.0;
        }
        assert!((58..=62).contains(&rendered), "rendered {rendered} frames");
    }

    #[test]
    fn invalid_rates_fall_back_to_sixty() {
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(FramePacer::new(rate).interval(), 1.0 / TARGET_FPS);
        }
    }
}
