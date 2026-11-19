//! Non-Unix stub for the VMP1 shared-memory preview ring.
//!
//! The preview ring is backed by a `tmpfs` file created with Unix positional
//! I/O (`pwrite`/`pread`) and lives under `/dev/shm`. None of that exists on
//! Windows, and the preview channel is only used by the headless engine path
//! (not the native shell), so this stub keeps the public API compilable and
//! reports an explicit "unsupported" error from the operations that would need
//! real shared memory. Linux behaviour is unchanged: it uses `preview.rs`.

#![allow(dead_code)]

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Size of the per-slot header in bytes (kept for API parity).
pub const HEADER_BYTES: usize = 32;
/// Magic number `VMP1` little-endian (kept for API parity).
pub const MAGIC: u32 = 0x3150_4D56;
/// Pixel format: RGBA8 (kept for API parity).
pub const FORMAT_RGBA8: u32 = 1;
/// Pixel format: BGRA8 (kept for API parity).
pub const FORMAT_BGRA8: u32 = 2;

fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the VMP1 shared-memory preview ring is only supported on Unix",
    ))
}

/// Parsed slot header (API parity with the Unix implementation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewHeader {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub sequence: u32,
    pub frame_id: u64,
}

/// A frame written into the ring (API parity with the Unix implementation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewFrame {
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub sequence: u32,
    pub frame_id: u64,
}

impl PreviewFrame {
    /// Event payload published as `preview/frame`.
    pub fn to_event(&self) -> Value {
        json!({
            "slot": self.slot,
            "width": self.width,
            "height": self.height,
            "stride": self.stride,
            "format": self.format,
            "sequence": self.sequence,
            "frameId": self.frame_id,
        })
    }
}

/// Non-Unix stand-in for the `VMP1` ring.
pub struct PreviewRing {
    path: PathBuf,
}

impl PreviewRing {
    /// Shared-memory rings are unavailable on this platform.
    pub fn create(
        _path_hint: Option<PathBuf>,
        _slots: usize,
        _width: u32,
        _height: u32,
    ) -> io::Result<Self> {
        unsupported()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn handshake(&self) -> Value {
        json!({
            "path": self.path.to_string_lossy(),
            "slots": 0,
            "slotBytes": HEADER_BYTES as u64,
            "width": 0,
            "height": 0,
            "stride": 0,
            "format": FORMAT_RGBA8,
        })
    }

    pub fn write_frame(&mut self, _pixels: &[u8]) -> io::Result<PreviewFrame> {
        unsupported()
    }

    pub fn read_slot(
        _path: &Path,
        _slot_bytes: usize,
        _slot: u32,
    ) -> io::Result<Option<(PreviewHeader, Vec<u8>)>> {
        unsupported()
    }
}
