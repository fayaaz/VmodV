//! NDI output must never block the frame loop on a texture read-back.
//!
//! The sender starts a `PendingReadback` from the compositor and polls it on
//! later frames. These tests exercise the state machine around that pending
//! copy with a scripted, in-memory source: no GPU and no libndi are needed.
//! A source returns `Ok(None)` while the copy is still pending, `Ok(Some(..))`
//! once with the pixels, or `Err(..)` if the read-back fails.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use modv_app::ndi_readback::{NdiFrame, NdiReadback, ReadbackSource};

/// A source driven by a script of `try_finish` results. Also counts polls so
/// the tests can prove that a rejected source is never touched, that polling
/// does not consume more than one step, and that a finished/failed source is
/// dropped rather than polled again.
struct FakeSource {
    steps: VecDeque<Result<Option<Vec<u8>>, String>>,
    polls: Rc<Cell<usize>>,
}

impl FakeSource {
    fn scripted(steps: Vec<Result<Option<Vec<u8>>, String>>) -> (Self, Rc<Cell<usize>>) {
        let polls = Rc::new(Cell::new(0));
        (
            Self {
                steps: steps.into(),
                polls: Rc::clone(&polls),
            },
            polls,
        )
    }
}

impl ReadbackSource for FakeSource {
    fn try_finish(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.polls.set(self.polls.get() + 1);
        self.steps
            .pop_front()
            .expect("fake read-back source polled more times than scripted")
    }
}

#[test]
fn start_accepts_one_source_and_rejects_a_second_while_busy() {
    let mut readback = NdiReadback::new();
    assert!(!readback.busy(), "a fresh readback must be idle");

    let first_pixels = vec![1, 2, 3, 4];
    let (first, first_polls) = FakeSource::scripted(vec![Ok(None), Ok(Some(first_pixels.clone()))]);
    let (second, second_polls) = FakeSource::scripted(vec![Ok(Some(vec![9, 9, 9, 9]))]);

    assert!(
        readback.start(first, (2, 2)),
        "first start must be accepted"
    );
    assert!(
        readback.busy(),
        "an accepted source makes the readback busy"
    );
    assert!(
        !readback.start(second, (3, 3)),
        "a second start while busy must be rejected"
    );
    assert!(
        readback.busy(),
        "a rejected start must not change busy state"
    );
    assert_eq!(
        second_polls.get(),
        0,
        "the rejected source must never be polled"
    );

    // The first source is still installed: it is pending once, then produces
    // its own pixels at its own size, never the rejected source's frame.
    assert!(
        readback.poll().is_none(),
        "the first source is pending, so no frame is ready"
    );
    assert_eq!(first_polls.get(), 1);
    let frame = readback.poll().expect("first source eventually finishes");
    assert_eq!(frame.pixels, first_pixels);
    assert_eq!(
        frame.size,
        (2, 2),
        "the rejected source's size must not replace the first's"
    );
}

#[test]
fn poll_is_pending_then_delivers_exactly_one_frame() {
    let pixels = vec![10, 20, 30, 40, 50, 60];
    let (source, polls) = FakeSource::scripted(vec![Ok(None), Ok(None), Ok(Some(pixels.clone()))]);

    let mut readback = NdiReadback::new();
    assert!(readback.start(source, (3, 2)));
    assert!(readback.poll().is_none(), "first poll: GPU copy pending");
    assert!(
        readback.poll().is_none(),
        "second poll: GPU copy still pending"
    );
    assert_eq!(
        polls.get(),
        2,
        "each poll must try the source at most once and never wait for it"
    );

    let frame: NdiFrame = readback.poll().expect("third poll finishes the copy");
    assert_eq!(frame.pixels, pixels);
    assert_eq!(frame.size, (3, 2));

    assert!(
        readback.poll().is_none(),
        "a completed frame must be returned exactly once"
    );
    assert_eq!(polls.get(), 3, "a finished source must not be polled again");
}

#[test]
fn completion_clears_busy_and_lets_a_new_source_start() {
    let first_pixels = vec![1, 1, 1, 1];
    let (first, first_polls) = FakeSource::scripted(vec![Ok(Some(first_pixels.clone()))]);
    let mut readback = NdiReadback::new();

    assert!(readback.start(first, (2, 2)));
    assert!(readback.busy());
    assert_eq!(readback.poll().expect("first frame").pixels, first_pixels);
    assert_eq!(first_polls.get(), 1);
    assert!(
        !readback.busy(),
        "finishing a copy must release the busy state"
    );

    let second_pixels = vec![2, 2, 2, 2, 2, 2];
    let (second, second_polls) = FakeSource::scripted(vec![Ok(Some(second_pixels.clone()))]);
    assert!(
        readback.start(second, (3, 2)),
        "a new read-back must start after the previous one completed"
    );
    assert!(readback.busy());
    let frame = readback.poll().expect("second frame");
    assert_eq!(frame.pixels, second_pixels);
    assert_eq!(frame.size, (3, 2));
    assert_eq!(second_polls.get(), 1);
    assert!(!readback.busy());
}

#[test]
fn source_error_clears_busy_and_poll_stays_none() {
    let (failed, failed_polls) =
        FakeSource::scripted(vec![Ok(None), Err("map callback failed".to_string())]);
    let mut readback = NdiReadback::new();

    assert!(readback.start(failed, (2, 2)));
    assert!(readback.poll().is_none(), "error source was pending first");
    assert!(
        readback.poll().is_none(),
        "an errored source must produce no frame, not a panic"
    );
    assert!(
        !readback.busy(),
        "an errored read-back must clear busy so the sender can recover"
    );
    assert_eq!(
        failed_polls.get(),
        2,
        "the errored source must not be polled again"
    );

    // Recovery: a fresh source can be started after the error.
    let pixels = vec![7, 7, 7, 7];
    let (recovery, recovery_polls) = FakeSource::scripted(vec![Ok(Some(pixels.clone()))]);
    assert!(
        readback.start(recovery, (2, 2)),
        "the frame loop must be able to start a new copy after a failure"
    );
    let frame = readback.poll().expect("recovery frame");
    assert_eq!(frame.pixels, pixels);
    assert_eq!(frame.size, (2, 2));
    assert_eq!(recovery_polls.get(), 1);
}
