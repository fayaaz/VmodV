//! Opt-in JSON-lines control socket for the native shell.
//!
//! `modv --socket <PATH>` starts a Unix-domain socket server that tests and
//! debug tools can drive the real native process with. Each request is one
//! JSON object per line:
//!
//! ```json
//! {"id": 1, "cmd": "state"}
//! {"id": 2, "cmd": "dispatch", "action": "groups/createGroup", "payload": {"name": "Effects"}}
//! {"id": 3, "cmd": "screenshot", "path": "/tmp/frame.png", "width": 640, "height": 360}
//! {"id": 4, "cmd": "save"}
//! {"id": 5, "cmd": "quit"}
//! ```
//!
//! Every request gets exactly one response on the same connection:
//! `{"id": 1, "ok": true, "result": ...}` or
//! `{"id": 1, "ok": false, "error": "..."}`.
//!
//! The socket is deliberately *not* part of the production path. It is only
//! bound when the flag is passed, and the protocol is a superset-free
//! alternative to the binary framed `modv-ipc` channel the Electron supervisor
//! uses: JSON lines are trivial to drive from any language and from the Rust
//! process harness under `crates/modv-app/tests/e2e/`.
//!
//! # Threading
//!
//! One dedicated thread owns the listener and every client stream. It reads
//! requests, forwards them to the winit loop over an mpsc channel (each with a
//! per-request reply sender), and writes the responses that come back. The
//! winit loop only drains the channel once per frame, so a slow request (a
//! full-resolution screenshot read-back) can never block the accept thread and
//! a client that stops reading can never block the render loop: writes are
//! buffered on the socket thread with a bounded backlog.

use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Map, Value};

/// Version of the JSON-lines protocol (bumped when a request/response shape
/// changes incompatibly; `version` reports it).
pub const PROTOCOL_VERSION: u32 = 1;

/// Longest accepted request line. State/screenshot requests are tiny; this
/// only bounds a broken client that never sends a newline.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Largest response backlog per client before the connection is dropped.
/// A state snapshot is a few hundred kilobytes at most, but a client that
/// never reads must not grow the process without bound.
pub const MAX_PENDING_OUT_BYTES: usize = 64 * 1024 * 1024;

/// How often the socket thread polls for requests/replies when idle.
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// One parsed request.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Correlation id echoed in the response (any JSON value; usually a
    /// number).
    pub id: Value,
    /// The parsed command.
    pub command: Command,
}

/// A command the native shell knows how to answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `state`: a JSON snapshot of the authoritative store plus runtime info.
    State,
    /// `dispatch`: run a Vuex-style store action.
    Dispatch {
        /// Action/mutation name (`groups/createGroup`, `scenes/applyScene`, …).
        action: String,
        /// Action payload (normally an object; `null` when omitted).
        payload: Value,
    },
    /// `screenshot`: render the current compositor frame and write it as PNG.
    Screenshot {
        /// Destination PNG path.
        path: PathBuf,
        /// Requested width (defaults to the compositor's main size).
        width: Option<u32>,
        /// Requested height (defaults to the compositor's main size).
        height: Option<u32>,
    },
    /// `quit`: graceful shutdown through the same path as File → Quit.
    Quit,
    /// `save`: write the engine snapshot (`engine-state.json`) now.
    Save,
    /// `ping`: readiness probe.
    Ping,
    /// `version`: binary/protocol version probe.
    Version,
}

/// A request that could not be parsed, with the correlation id when the id
/// field was readable so the client can match the error response.
#[derive(Clone, Debug, PartialEq)]
pub struct RequestError {
    /// The request id, or `null` when the line was not a JSON object with an
    /// `id`.
    pub id: Value,
    /// Human-readable parse failure.
    pub message: String,
}

/// Parse one JSON line into a request.
pub fn parse_request(line: &str) -> Result<Request, RequestError> {
    let value: Value = serde_json::from_str(line).map_err(|error| RequestError {
        id: Value::Null,
        message: format!("invalid JSON: {error}"),
    })?;
    let object = value.as_object().ok_or_else(|| RequestError {
        id: Value::Null,
        message: "request must be a JSON object".to_string(),
    })?;
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let cmd = object
        .get("cmd")
        .and_then(Value::as_str)
        .ok_or_else(|| RequestError {
            id: id.clone(),
            message: "missing string \"cmd\"".to_string(),
        })?;

    let command = match cmd {
        "state" => Command::State,
        "ping" => Command::Ping,
        "version" => Command::Version,
        "quit" => Command::Quit,
        "save" => Command::Save,
        "dispatch" => {
            let action = object
                .get("action")
                .and_then(Value::as_str)
                .filter(|action| !action.trim().is_empty())
                .ok_or_else(|| RequestError {
                    id: id.clone(),
                    message: "dispatch needs a non-empty string \"action\"".to_string(),
                })?;
            Command::Dispatch {
                action: action.to_string(),
                payload: object.get("payload").cloned().unwrap_or(Value::Null),
            }
        }
        "screenshot" => {
            let path = object
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.trim().is_empty())
                .ok_or_else(|| RequestError {
                    id: id.clone(),
                    message: "screenshot needs a non-empty string \"path\"".to_string(),
                })?;
            let width = optional_dimension(object, "width", &id)?;
            let height = optional_dimension(object, "height", &id)?;
            Command::Screenshot {
                path: PathBuf::from(path),
                width,
                height,
            }
        }
        other => {
            return Err(RequestError {
                id,
                message: format!(
                    "unknown cmd {other:?} (accepted: state, dispatch, screenshot, quit, ping, version)"
                ),
            });
        }
    };

    Ok(Request { id, command })
}

fn optional_dimension(
    object: &Map<String, Value>,
    key: &str,
    id: &Value,
) -> Result<Option<u32>, RequestError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let dimension = value.as_u64().ok_or_else(|| RequestError {
                id: id.clone(),
                message: format!("\"{key}\" must be a positive integer"),
            })?;
            if dimension == 0 || dimension > u64::from(u32::MAX) {
                return Err(RequestError {
                    id: id.clone(),
                    message: format!("\"{key}\" must be between 1 and {}", u32::MAX),
                });
            }
            Ok(Some(dimension as u32))
        }
    }
}

/// Serialise one response line (including the trailing newline).
pub fn response_line(id: &Value, result: &Result<Value, String>) -> String {
    let mut response = Map::new();
    response.insert("id".to_string(), id.clone());
    match result {
        Ok(value) => {
            response.insert("ok".to_string(), Value::Bool(true));
            response.insert("result".to_string(), value.clone());
        }
        Err(error) => {
            response.insert("ok".to_string(), Value::Bool(false));
            response.insert("error".to_string(), Value::String(error.clone()));
        }
    }
    let mut line = serde_json::to_string(&Value::Object(response)).unwrap_or_else(|_| {
        "{\"id\":null,\"ok\":false,\"error\":\"serialise failed\"}".to_string()
    });
    line.push('\n');
    line
}

/// A request queued for the winit loop, carrying its own reply channel.
pub struct QueuedRequest {
    /// Correlation id echoed in the response.
    pub id: Value,
    /// The parsed command.
    pub command: Command,
    client: u64,
    reply: Sender<Reply>,
}

impl QueuedRequest {
    /// Write the response for this request. The socket thread performs the
    /// actual I/O; this only enqueues the line.
    pub fn respond(self, result: Result<Value, String>) -> bool {
        self.reply
            .send(Reply {
                client: self.client,
                line: response_line(&self.id, &result),
            })
            .is_ok()
    }
}

struct Reply {
    client: u64,
    line: String,
}

/// Write an RGBA8 buffer as a PNG, returning the file size.
///
/// Kept here (rather than in `native.rs`) so the encoding is unit-testable
/// without a GPU.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<u64, String> {
    if width == 0 || height == 0 {
        return Err("screenshot dimensions must be non-zero".to_string());
    }
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(format!(
            "screenshot buffer is {} bytes, expected {expected} for {width}x{height} RGBA",
            rgba.len()
        ));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    image::save_buffer_with_format(
        path,
        rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("could not stat {}: {error}", path.display()))
}

/// Owns the socket path, listener thread and request queue.
#[derive(Debug)]
pub struct ControlServer {
    path: PathBuf,
    identity: Option<(u64, u64)>,
    requests: Receiver<QueuedRequest>,
    flush: Sender<SyncSender<()>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    /// Bind `path`, replacing a stale socket file that nobody is listening on.
    pub fn bind(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        prepare_path(&path)?;
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let identity = socket_identity(&path);

        let (request_tx, requests) = mpsc::channel();
        let (reply_tx, replies) = mpsc::channel();
        let (flush, flush_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread = {
            let path = path.clone();
            let shutdown = Arc::clone(&shutdown);
            thread::Builder::new()
                .name("modv-control-socket".to_string())
                .spawn(move || {
                    serve(
                        listener, &path, identity, request_tx, reply_tx, replies, flush_rx,
                        &shutdown,
                    );
                })?
        };

        Ok(Self {
            path,
            identity,
            requests,
            flush,
            shutdown,
            thread: Some(thread),
        })
    }

    /// The bound socket path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Take the next queued request, if any. Called once per frame by the
    /// winit loop; never blocks.
    pub fn try_recv(&self) -> Option<QueuedRequest> {
        self.requests.try_recv().ok()
    }

    /// Wait until every response enqueued before this call has been written
    /// (or the timeout expires). Used before a quit so the acknowledgement is
    /// on the wire before the process exits.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        if self.flush.send(ack_tx).is_err() {
            return false;
        }
        ack_rx.recv_timeout(timeout).is_ok()
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if self.identity.is_some() && socket_identity(&self.path) == self.identity {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn prepare_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() || !is_stale_socket(path) {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!(
                        "{} exists and is not a stale control socket",
                        path.display()
                    ),
                ));
            }
            fs::remove_file(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn socket_identity(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_socket() {
        return None;
    }
    Some((metadata.dev(), metadata.ino()))
}

fn is_stale_socket(path: &Path) -> bool {
    match UnixStream::connect(path) {
        Ok(_) => false,
        Err(error) => {
            error.kind() == io::ErrorKind::ConnectionRefused && socket_identity(path).is_some()
        }
    }
}

struct Client {
    id: u64,
    stream: UnixStream,
    input: Vec<u8>,
    output: VecDeque<u8>,
    /// One slot per received line, filled when its response is ready. This
    /// keeps responses in request order even when a parse error is known
    /// immediately and a store request waits for the next frame.
    responses: VecDeque<Option<String>>,
    closed: bool,
}

impl Client {
    fn expect_response(&mut self) {
        self.responses.push_back(None);
    }

    fn queue_immediate(&mut self, line: String) {
        self.responses.push_back(Some(line));
        self.drain_responses();
    }

    fn deliver(&mut self, line: String) {
        if let Some(slot) = self.responses.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(line);
        } else {
            // Should be unreachable (one reply per request); keep the line
            // rather than dropping it if a bug ever answers twice.
            self.responses.push_back(Some(line));
        }
        self.drain_responses();
    }

    fn drain_responses(&mut self) {
        while matches!(self.responses.front(), Some(Some(_))) {
            if let Some(Some(line)) = self.responses.pop_front() {
                self.send_line(&line);
            }
        }
    }

    fn send_line(&mut self, line: &str) {
        if self.output.len() + line.len() > MAX_PENDING_OUT_BYTES {
            self.closed = true;
            return;
        }
        self.output.extend(line.as_bytes());
    }

    fn flush(&mut self) {
        while !self.output.is_empty() {
            self.output.make_contiguous();
            match self.stream.write(self.output.as_slices().0) {
                Ok(0) => {
                    self.closed = true;
                    return;
                }
                Ok(written) => {
                    self.output.drain(..written);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.closed = true;
                    return;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn serve(
    listener: UnixListener,
    path: &Path,
    identity: Option<(u64, u64)>,
    requests: Sender<QueuedRequest>,
    reply_tx: Sender<Reply>,
    replies: Receiver<Reply>,
    flush_rx: Receiver<SyncSender<()>>,
    shutdown: &AtomicBool,
) {
    let mut clients: Vec<Client> = Vec::new();
    let mut next_client = 0u64;
    let mut pending_flush: Option<SyncSender<()>> = None;

    while !shutdown.load(Ordering::SeqCst) {
        // 1. Accept every pending connection.
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(true).is_err() {
                        continue;
                    }
                    clients.push(Client {
                        id: next_client,
                        stream,
                        input: Vec::new(),
                        output: VecDeque::new(),
                        responses: VecDeque::new(),
                        closed: false,
                    });
                    next_client += 1;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }

        // 2. Read available bytes and split complete JSON lines.
        for client in &mut clients {
            if client.closed {
                continue;
            }
            let mut scratch = [0u8; 8192];
            loop {
                match client.stream.read(&mut scratch) {
                    Ok(0) => {
                        client.closed = true;
                        break;
                    }
                    Ok(read) => {
                        client.input.extend_from_slice(&scratch[..read]);
                        if client.input.len() > MAX_LINE_BYTES {
                            client.closed = true;
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        client.closed = true;
                        break;
                    }
                }
            }
            while let Some(position) = client.input.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = client.input.drain(..=position).collect();
                let text = String::from_utf8_lossy(&line[..line.len() - 1]);
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                match parse_request(text) {
                    Ok(request) => {
                        client.expect_response();
                        // A disconnected winit loop is a fatal condition for
                        // the server: stop accepting and let Drop clean up.
                        if requests
                            .send(QueuedRequest {
                                id: request.id,
                                command: request.command,
                                client: client.id,
                                reply: reply_tx.clone(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        client.queue_immediate(response_line(&error.id, &Err(error.message)));
                    }
                }
            }
        }

        // 3. Move replies from the winit loop onto their client's buffer.
        while let Ok(reply) = replies.try_recv() {
            if let Some(client) = clients.iter_mut().find(|client| client.id == reply.client) {
                client.deliver(reply.line);
            }
        }
        // 4. A flush request waits until the reply channel has been drained
        //    and every buffered byte is written.
        while let Ok(ack) = flush_rx.try_recv() {
            pending_flush = Some(ack);
        }
        for client in &mut clients {
            client.drain_responses();
            client.flush();
        }
        if let Some(ack) = pending_flush.take() {
            while let Ok(reply) = replies.try_recv() {
                if let Some(client) = clients.iter_mut().find(|client| client.id == reply.client) {
                    client.deliver(reply.line);
                }
            }
            for client in &mut clients {
                client.drain_responses();
                client.flush();
            }
            let _ = ack.send(());
        }

        clients.retain(|client| !client.closed);
        thread::sleep(POLL_INTERVAL);
    }

    // Best effort final flush; Drop follows immediately.
    while let Ok(reply) = replies.try_recv() {
        if let Some(client) = clients.iter_mut().find(|client| client.id == reply.client) {
            client.deliver(reply.line);
        }
    }
    for client in &mut clients {
        client.flush();
    }
    if identity.is_some() && socket_identity(path) == identity {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("modv-control-{tag}-{}-{nanos}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn parses_supported_commands_and_echoes_ids() {
        let state = parse_request(r#"{"id":7,"cmd":"state"}"#).expect("state");
        assert_eq!(state.id, json!(7));
        assert_eq!(state.command, Command::State);

        let dispatch = parse_request(
            r#"{"id":"a","cmd":"dispatch","action":"groups/createGroup","payload":{"name":"Effects"}}"#,
        )
        .expect("dispatch");
        assert_eq!(dispatch.id, json!("a"));
        assert_eq!(
            dispatch.command,
            Command::Dispatch {
                action: "groups/createGroup".to_string(),
                payload: json!({"name": "Effects"}),
            }
        );

        let screenshot = parse_request(
            r#"{"id":3,"cmd":"screenshot","path":"/tmp/frame.png","width":640,"height":360}"#,
        )
        .expect("screenshot");
        assert_eq!(
            screenshot.command,
            Command::Screenshot {
                path: PathBuf::from("/tmp/frame.png"),
                width: Some(640),
                height: Some(360),
            }
        );

        let no_payload = parse_request(r#"{"id":4,"cmd":"dispatch","action":"scenes/tick"}"#)
            .expect("dispatch default payload");
        assert_eq!(
            no_payload.command,
            Command::Dispatch {
                action: "scenes/tick".to_string(),
                payload: Value::Null,
            }
        );

        for (line, expected) in [
            (r#"{"id":5,"cmd":"ping"}"#, Command::Ping),
            (r#"{"id":6,"cmd":"version"}"#, Command::Version),
            (r#"{"id":7,"cmd":"quit"}"#, Command::Quit),
            (r#"{"id":8,"cmd":"save"}"#, Command::Save),
        ] {
            assert_eq!(parse_request(line).expect("command").command, expected);
        }
    }

    #[test]
    fn rejects_malformed_requests_with_the_id_when_readable() {
        let error = parse_request("not json").expect_err("invalid JSON");
        assert_eq!(error.id, Value::Null);
        assert!(error.message.contains("invalid JSON"));

        let error = parse_request(r#"{"id":9,"cmd":"nope"}"#).expect_err("unknown cmd");
        assert_eq!(error.id, json!(9));
        assert!(error.message.contains("unknown cmd"), "{}", error.message);

        let error = parse_request(r#"{"id":10,"cmd":"dispatch"}"#).expect_err("missing action");
        assert_eq!(error.id, json!(10));

        let error =
            parse_request(r#"{"id":11,"cmd":"screenshot","path":""}"#).expect_err("empty path");
        assert!(error.message.contains("path"), "{}", error.message);

        let error = parse_request(r#"{"id":12,"cmd":"screenshot","path":"/tmp/a.png","width":0}"#)
            .expect_err("zero width");
        assert_eq!(error.id, json!(12));
        assert!(error.message.contains("width"), "{}", error.message);
    }

    #[test]
    fn responses_are_correlated_and_errors_are_strings() {
        let ok = response_line(&json!(3), &Ok(json!({"hello": "world"})));
        assert!(ok.ends_with('\n'));
        let parsed: Value = serde_json::from_str(ok.trim_end()).expect("response JSON");
        assert_eq!(
            parsed,
            json!({"id": 3, "ok": true, "result": {"hello": "world"}})
        );

        let failure = response_line(&json!("x"), &Err("boom \"quoted\"".to_string()));
        let parsed: Value = serde_json::from_str(failure.trim_end()).expect("error JSON");
        assert_eq!(parsed["id"], json!("x"));
        assert_eq!(parsed["ok"], json!(false));
        assert_eq!(parsed["error"], json!("boom \"quoted\""));
        assert!(parsed.get("result").is_none());
    }

    #[test]
    fn write_png_round_trips_dimensions_and_pixels() {
        let dir = temp_dir("png");
        let path = dir.join("nested/frame.png");
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let bytes = write_png(&path, 2, 2, &rgba).expect("write PNG");
        assert!(bytes > 8, "PNG should have content");

        let file = fs::read(&path).expect("read PNG");
        assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
        let image = image::open(&path).expect("decode PNG").to_rgba8();
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.into_raw(), rgba);

        let error = write_png(&path, 2, 2, &[0; 4]).expect_err("short buffer");
        assert!(error.contains("expected 16"), "{error}");
    }

    #[test]
    fn server_replaces_a_stale_socket_and_round_trips_a_request() {
        let dir = temp_dir("server");
        let path = dir.join("control.sock");

        // Simulate an engine killed without cleanup: the listener is gone but
        // the socket node remains.
        let stale = UnixListener::bind(&path).expect("stale listener");
        drop(stale);
        assert!(path.exists(), "stale socket node");

        let server = ControlServer::bind(&path).expect("bind");
        assert!(path.exists());

        let mut client = UnixStream::connect(&path).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        client
            .write_all(b"{\"id\":1,\"cmd\":\"ping\"}\n{\"id\":2,\"cmd\":\"nope\"}\n")
            .expect("write requests");

        let mut requests = Vec::new();
        let started = Instant::now();
        while requests.is_empty() {
            while let Some(request) = server.try_recv() {
                requests.push(request);
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "request was not queued"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(requests[0].id, json!(1));
        assert_eq!(requests[0].command, Command::Ping);
        requests.remove(0).respond(Ok(json!({"pong": true})));

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).expect("ping response");
        let response: Value = serde_json::from_str(line.trim_end()).expect("ping JSON");
        assert_eq!(
            response,
            json!({"id": 1, "ok": true, "result": {"pong": true}})
        );

        let mut error_line = String::new();
        reader
            .read_line(&mut error_line)
            .expect("parse-error response");
        let error: Value = serde_json::from_str(error_line.trim_end()).expect("error JSON");
        assert_eq!(error["id"], json!(2));
        assert_eq!(error["ok"], json!(false));
        assert!(
            error["error"]
                .as_str()
                .expect("error text")
                .contains("unknown cmd"),
            "{error}"
        );

        drop(server);
        assert!(!path.exists(), "the socket file is removed on drop");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn server_refuses_to_replace_a_live_socket_or_regular_file() {
        let dir = temp_dir("refuse");
        let path = dir.join("live.sock");
        let live = UnixListener::bind(&path).expect("live listener");
        let error = ControlServer::bind(&path).expect_err("must not steal a live socket");
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert!(path.exists());
        drop(live);
        let _ = fs::remove_file(&path);

        let regular = dir.join("regular.txt");
        fs::write(&regular, b"not a socket").expect("regular file");
        let error = ControlServer::bind(&regular).expect_err("must not delete a file");
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert_eq!(fs::read(&regular).expect("file survives"), b"not a socket");

        let _ = fs::remove_dir_all(dir);
    }
}
