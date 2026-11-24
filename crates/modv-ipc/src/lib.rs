//! Protocol, framing and shared-memory preview transport for the headless engine.
//!
//! This crate implements the M1 control-channel milestone:
//!
//! - [`frame`] — the `[ u32 LE payload length ][ u8 kind ][ payload ]` wire format.
//! - [`message`] — the JSON `Request` / `Response` / `Event` messages carried by frames.
//! - [`preview`] — the `VMP1` shared-memory ring used for preview frames.
//! - [`socket`] — non-blocking Unix domain socket plumbing for the engine main loop.
//!
#![forbid(unsafe_code)]

pub mod frame;
pub mod message;

// The VMP1 preview ring and the control-channel socket are Unix-only (they use
// `/dev/shm`, `pwrite`/`pread` and Unix-domain sockets). On non-Unix targets,
// compile API-compatible stubs so the rest of the workspace still builds; the
// headless engine path reports these as unsupported there.
#[cfg(unix)]
pub mod preview;
#[cfg(not(unix))]
#[path = "preview_stub.rs"]
pub mod preview;

#[cfg(unix)]
pub mod socket;
#[cfg(not(unix))]
#[path = "socket_stub.rs"]
pub mod socket;
