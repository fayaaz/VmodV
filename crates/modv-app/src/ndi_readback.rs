//! Non-blocking state machine around the NDI output frame read-back.
//!
//! [`crate::compositor::Compositor::begin_read_main_rgba8`] submits a
//! texture→buffer copy; the matching [`PendingReadback`] finishes on a later
//! frame. This module owns that pending copy so the output frame loop never
//! blocks on the GPU map (and so a sender can be fed as soon as the pixels
//! arrive).

use crate::compositor::PendingReadback;

/// A GPU read-back that can be polled without blocking.
///
/// `Ok(None)` means the copy is still pending, `Ok(Some(..))` delivers the
/// tightly packed RGBA8 pixels once, and `Err(..)` reports a failed read-back.
pub trait ReadbackSource {
    fn try_finish(&mut self) -> Result<Option<Vec<u8>>, String>;
}

impl ReadbackSource for PendingReadback {
    fn try_finish(&mut self) -> Result<Option<Vec<u8>>, String> {
        PendingReadback::try_finish(self)
    }
}

/// A finished frame ready for the NDI sender.
pub struct NdiFrame {
    pub pixels: Vec<u8>,
    pub size: (u32, u32),
}

struct InFlight<S> {
    source: S,
    size: (u32, u32),
}

/// One in-flight NDI read-back at a time.
pub struct NdiReadback<S> {
    in_flight: Option<InFlight<S>>,
}

impl<S> Default for NdiReadback<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> NdiReadback<S> {
    pub fn new() -> Self {
        Self { in_flight: None }
    }
}

impl<S: ReadbackSource> NdiReadback<S> {
    /// Whether a read-back is currently in flight.
    pub fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Install `source` unless a read-back is already in flight.
    ///
    /// Returns `false` (keeping the original source) when busy.
    pub fn start(&mut self, source: S, size: (u32, u32)) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        self.in_flight = Some(InFlight { source, size });
        true
    }

    /// Try the in-flight source exactly once.
    ///
    /// Returns the finished frame once and clears busy. A pending copy returns
    /// `None` and stays installed; an error is swallowed (clears busy, returns
    /// `None`) so the sender can recover on a later frame.
    pub fn poll(&mut self) -> Option<NdiFrame> {
        let active = self.in_flight.as_mut()?;
        let result = active.source.try_finish();
        let size = active.size;
        match result {
            Ok(Some(pixels)) => {
                self.in_flight = None;
                Some(NdiFrame { pixels, size })
            }
            Ok(None) => None,
            Err(_) => {
                self.in_flight = None;
                None
            }
        }
    }
}
