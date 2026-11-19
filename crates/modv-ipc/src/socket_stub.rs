//! Non-Unix stub for the Unix-domain control-channel server.
//!
//! The `modv-ipc` control channel is a non-blocking Unix-domain socket. On
//! Windows there is no Unix-domain socket, so this stub keeps the public API
//! compilable and reports an explicit "unsupported" error from `bind` (and the
//! other operations). Linux behaviour is unchanged: it uses `socket.rs`.

#![allow(dead_code)]

use std::io;
use std::path::{Path, PathBuf};

use crate::message::{Event, Response};

/// Default maximum number of simultaneous clients (API parity).
pub const DEFAULT_MAX_CLIENTS: usize = 8;
/// Maximum bytes buffered per client (API parity).
pub const MAX_PENDING_OUT_BYTES: usize = 16 * 1024 * 1024;

fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the modv-ipc control socket is only supported on Unix",
    ))
}

/// One complete frame read from a client (API parity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inframe {
    pub client: usize,
    pub kind: u8,
    pub payload: Vec<u8>,
}

/// Outcome of one poll (API parity).
#[derive(Debug, Default)]
pub struct PollResult {
    pub frames: Vec<Inframe>,
    pub accepted: Vec<usize>,
    pub disconnected: Vec<usize>,
}

/// Non-Unix stand-in for the Unix-domain socket server.
pub struct SocketServer {
    path: PathBuf,
}

impl SocketServer {
    /// Unix-domain control sockets are unavailable on this platform.
    pub fn bind(path: &Path) -> io::Result<Self> {
        let _ = path;
        unsupported()
    }

    pub fn with_max_clients(self, _max_clients: usize) -> Self {
        self
    }

    pub fn poll(&mut self) -> io::Result<PollResult> {
        unsupported()
    }

    pub fn send(&self, _client: usize, _kind: u8, _payload: &[u8]) -> io::Result<()> {
        unsupported()
    }

    pub fn send_response(&self, _client: usize, _response: &Response) -> io::Result<()> {
        unsupported()
    }

    pub fn broadcast_event(&self, _event: &Event) -> io::Result<()> {
        unsupported()
    }

    pub fn client_count(&self) -> usize {
        0
    }

    pub fn paths(&self) -> &Path {
        &self.path
    }
}
