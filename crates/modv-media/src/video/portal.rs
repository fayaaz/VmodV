//! XDG desktop portal `ScreenCast`: the engine-owned screen picker.
//!
//! In the legacy renderer the *browser* asked for a screen through
//! `getDisplayMedia()`, and Electron's xdg-desktop-portal backend answered with
//! a PipeWire stream. Engine mode cannot reuse that: Electron 45 never exposes
//! the portal's PipeWire node id (`desktopCapturer.getSources()` yields
//! `screen:<display>:<n>`/`window:<id>:<n>`, and `useSystemPicker` hands the
//! whole dialog to the portal), so there is no node id to hand to
//! `media/start-screen`. Nothing is capturing in the renderer any more, so
//! forwarding a `MediaStream` would produce frames nobody consumes.
//!
//! The engine therefore asks the portal itself:
//!
//! ```text
//! Screencast::new()                  org.freedesktop.portal.Desktop / ScreenCast
//! create_session()                   one portal session on one DBus connection
//! select_sources(monitor|window, multiple = false, cursor hidden)
//! start()                            the system dialog — this *is* the picker
//!   └─ Stream::pipe_wire_node_id()   → VideoSourceKind::Screen { node_id }
//! ```
//!
//! No node id crosses the Electron/engine boundary and no portal consent UI has
//! to move into the renderer: the compositor's own dialog chooses the monitor
//! or window.
//!
//! # Threading
//!
//! [`request_screencast`] is **synchronous and blocking**: it returns only once
//! the user has answered the dialog, which can take any amount of time (or
//! never happen). Callers must run it on a worker thread — never on the winit
//! event loop, the render thread or an audio callback. It does the D-Bus work
//! on a dedicated single-threaded tokio runtime and keeps that thread alive for
//! as long as the returned [`PortalStream`].
//!
//! # Session lifetime
//!
//! The portal ties a screencast to the client's D-Bus connection: when the
//! connection goes away the session — and the PipeWire node `pipewiresrc` is
//! reading — is destroyed. [`PortalStream`] owns that connection through its
//! background thread, so the caller must keep it alive for the whole capture,
//! alongside the `VideoSource` it started:
//!
//! ```text
//! let stream = request_screencast(SourceSelection::Monitor)?;   // worker thread
//! hub.start_source("screen", stream.video_config());            // see VideoHub
//! sessions.insert("screen", stream);                            // keep it alive
//! // … stop the source, then drop the PortalStream (or call close())…
//! ```
//!
//! Dropping a [`PortalStream`] does not block: it signals the thread, which
//! closes the session in the background. [`PortalStream::close`] waits for that
//! (bounded by [`CLOSE_TIMEOUT`]) when a caller needs the node gone before it
//! starts another capture.
//!
//! # Node visibility
//!
//! The capture pipeline connects by node id (`pipewiresrc path=<node_id>`),
//! which is what a host (non-sandboxed) application sees on the session's
//! PipeWire daemon — the same way `gst-launch-1.0 pipewiresrc path=…` captures
//! a portal stream. A *sandboxed* client cannot see the portal's node that way
//! and needs the `OpenPipeWireRemote` file descriptor instead; that would mean
//! teaching [`VideoSourceKind::Screen`] an fd (and the description-based
//! pipeline builder an actual element graph), so it is deliberately not done
//! here.
//!
//! # What needs a live session
//!
//! Everything except the request itself is pure: the first-stream selection,
//! the size/source-type mapping and the `--list`-style table are unit tested
//! without D-Bus. Only the dialog needs a running D-Bus session, an
//! `xdg-desktop-portal` with a `ScreenCast` backend (GNOME, KDE,
//! wlroots/`xdg-desktop-portal-wlr`, …) and a user to click it.

use std::fmt;

use super::pipeline::{VideoSourceConfig, VideoSourceKind};

/// `XDP_OUTPUT_MONITOR`: the portal may return monitor streams.
pub const SOURCE_TYPE_MONITOR: u32 = 1;
/// `XDP_OUTPUT_WINDOW`: the portal may return window streams.
pub const SOURCE_TYPE_WINDOW: u32 = 2;
/// `XDP_OUTPUT_VIRTUAL`: backend-specific virtual output. Never requested here
/// (it is only advertised by some compositors), but a reply can name it.
pub const SOURCE_TYPE_VIRTUAL: u32 = 4;

/// How long [`PortalStream::close`] waits for the portal to drop the session.
///
/// The `Close` call is a round trip to a local socket; the timeout only guards
/// against a wedged portal backend swallowing it.
pub const CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// What the engine asks the portal to offer the user.
///
/// `multiple = false` is always requested, so the dialog selects exactly one
/// source and the reply normally carries one [`StreamInfo`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceSelection {
    /// Only monitors ("share a screen"). The default, matching the legacy
    /// Screen Capture panel's "Screen" mode.
    #[default]
    Monitor,
    /// Only windows ("share a window").
    Window,
    /// Both; the dialog decides which of the two the user picks.
    MonitorOrWindow,
}

impl SourceSelection {
    /// The portal's `types` bit flags.
    pub fn bits(self) -> u32 {
        match self {
            Self::Monitor => SOURCE_TYPE_MONITOR,
            Self::Window => SOURCE_TYPE_WINDOW,
            Self::MonitorOrWindow => SOURCE_TYPE_MONITOR | SOURCE_TYPE_WINDOW,
        }
    }

    /// The CLI/log spelling (`--screen-source monitor|window|both`).
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Window => "window",
            Self::MonitorOrWindow => "both",
        }
    }

    /// Parse [`Self::as_arg`]; `screen`/`display` are accepted as aliases for
    /// `monitor` so a caller can echo the legacy UI's labels.
    pub fn from_arg(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "monitor" | "monitors" | "screen" | "screens" | "display" => Some(Self::Monitor),
            "window" | "windows" | "application" => Some(Self::Window),
            "both" | "monitor|window" | "any" => Some(Self::MonitorOrWindow),
            _ => None,
        }
    }
}

impl fmt::Display for SourceSelection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_arg())
    }
}

/// The `source_type` the portal reported for one stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortalSourceType {
    /// A whole monitor.
    Monitor,
    /// A single window/application.
    Window,
    /// A virtual output provided by the backend.
    Virtual,
    /// The portal did not report a source type (older backends omit it).
    Unspecified,
    /// Flags this crate does not know, e.g. a newer portal's source type.
    Unknown(u32),
}

impl PortalSourceType {
    /// Decode the portal's `source_type` flags.
    pub fn from_bits(bits: u32) -> Self {
        match bits & (SOURCE_TYPE_MONITOR | SOURCE_TYPE_WINDOW | SOURCE_TYPE_VIRTUAL) {
            SOURCE_TYPE_MONITOR => Self::Monitor,
            SOURCE_TYPE_WINDOW => Self::Window,
            SOURCE_TYPE_VIRTUAL => Self::Virtual,
            0 if bits == 0 => Self::Unspecified,
            _ => Self::Unknown(bits),
        }
    }

    /// The decoded flags (`0` for [`Self::Unspecified`]).
    pub fn bits(self) -> u32 {
        match self {
            Self::Monitor => SOURCE_TYPE_MONITOR,
            Self::Window => SOURCE_TYPE_WINDOW,
            Self::Virtual => SOURCE_TYPE_VIRTUAL,
            Self::Unspecified => 0,
            Self::Unknown(bits) => bits,
        }
    }

    /// Short lowercase label for logs and the `--list` table.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Window => "window",
            Self::Virtual => "virtual",
            Self::Unspecified => "unspecified",
            Self::Unknown(_) => "unknown",
        }
    }
}

impl fmt::Display for PortalSourceType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(bits) => write!(formatter, "unknown({bits})"),
            other => formatter.write_str(other.as_str()),
        }
    }
}

/// One `Streams` entry, exactly as the portal describes it.
///
/// Deliberately independent of `ashpd`'s types (which cannot be constructed
/// outside the crate) so the mapping to [`StreamInfo`] is unit tested without a
/// D-Bus session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawStream {
    /// `Stream::pipe_wire_node_id()` — the `pipewiresrc path=` value.
    pub node_id: u32,
    /// `Stream::size()` — `(width, height)` in the compositor's coordinate
    /// space, or `None` when the backend does not report one (typical for
    /// window streams).
    pub size: Option<(i32, i32)>,
    /// `Stream::source_type()`, as raw flags.
    pub source_type: Option<u32>,
}

impl RawStream {
    /// A stream with no size/type information.
    pub fn new(node_id: u32) -> Self {
        Self {
            node_id,
            ..Self::default()
        }
    }

    /// Set the reported size.
    pub fn with_size(mut self, width: i32, height: i32) -> Self {
        self.size = Some((width, height));
        self
    }

    /// Set the reported `source_type` flags.
    pub fn with_source_type(mut self, source_type: u32) -> Self {
        self.source_type = Some(source_type);
        self
    }

    /// The reported size as engine dimensions, with `0` for "unknown".
    ///
    /// A portal may report `0`/negative dimensions or omit them entirely; both
    /// become `0`, which [`StreamInfo::has_size`] recognises (the screen
    /// pipeline keeps the PipeWire node's native size anyway).
    pub fn dimensions(&self) -> (u32, u32) {
        match self.size {
            Some((width, height)) => (dimension(width), dimension(height)),
            None => (0, 0),
        }
    }
}

fn dimension(value: i32) -> u32 {
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(0)
}

/// What the engine needs to start a screen capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    /// PipeWire node id for `pipewiresrc path=`.
    pub node_id: u32,
    /// Capture width reported by the portal, `0` when unknown.
    pub width: u32,
    /// Capture height reported by the portal, `0` when unknown.
    pub height: u32,
    /// Which kind of source the user picked.
    pub source_type: PortalSourceType,
}

impl StreamInfo {
    /// Map a portal stream onto the engine's shape.
    ///
    /// This is the whole mapping [`request_screencast`] applies: node id
    /// verbatim, size coerced to `u32` with `0` for unknown, `source_type`
    /// decoded (absent → [`PortalSourceType::Unspecified`]).
    pub fn from_raw(stream: &RawStream) -> Self {
        let (width, height) = stream.dimensions();

        Self {
            node_id: stream.node_id,
            width,
            height,
            source_type: stream
                .source_type
                .map_or(PortalSourceType::Unspecified, PortalSourceType::from_bits),
        }
    }

    /// Whether the portal reported a usable size.
    pub fn has_size(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// The capture kind this stream feeds: `pipewiresrc path=<node_id>`.
    pub fn video_source(&self) -> VideoSourceKind {
        VideoSourceKind::Screen {
            node_id: self.node_id,
        }
    }

    /// A ready-to-start configuration for this stream.
    ///
    /// Screen pipelines keep the PipeWire node's native size and frame rate, so
    /// the config's `width`/`height`/`fps` are ignored by
    /// [`super::pipeline::pipeline_description`]; the engine sizes its texture
    /// from [`super::VideoFrame`] instead.
    pub fn video_config(&self) -> VideoSourceConfig {
        VideoSourceConfig::screen(self.node_id)
    }

    /// One line for logs and the `--list` table.
    pub fn describe(&self) -> String {
        let size = if self.has_size() {
            format!("{}x{}", self.width, self.height)
        } else {
            "unknown".to_string()
        };

        format!(
            "node={} size={} source={}",
            self.node_id, size, self.source_type
        )
    }
}

/// Pick the stream to capture: the first one the portal returned.
///
/// `multiple = false` means one stream, but a backend may still answer with
/// several (e.g. an already-running session or a compositor that exports one
/// node per output). The engine captures one node — the first — exactly like
/// the legacy renderer's `getDisplayMedia({ video: true })` single-track grab.
pub fn select_stream(streams: &[RawStream]) -> Result<StreamInfo, PortalError> {
    streams
        .first()
        .map(StreamInfo::from_raw)
        .ok_or(PortalError::NoStreams)
}

/// Render the `Streams` reply as a `--list`-style table.
///
/// Pure, so the engine's diagnostics (and its tests) can show exactly what the
/// portal offered without opening a pipewire element.
pub fn describe_streams(streams: &[RawStream]) -> String {
    if streams.is_empty() {
        return "the desktop portal returned no screen cast streams (nothing was selected)"
            .to_string();
    }

    let mut table = format!("screen cast streams ({}):", streams.len());

    for (index, stream) in streams.iter().enumerate() {
        table.push_str(&format!(
            "\n  [{index}] {}",
            StreamInfo::from_raw(stream).describe()
        ));
    }

    table
}

/// Why a screen capture could not be requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalError {
    /// No `xdg-desktop-portal`/D-Bus session, or no `ScreenCast` backend.
    ///
    /// Carries the underlying reason for the log; the UI should treat it as
    /// "screen capture is unavailable on this host", not as a bug.
    Unavailable(String),
    /// The user dismissed the dialog (or chose nothing).
    Cancelled,
    /// The dialog succeeded but the reply carried no streams.
    NoStreams,
    /// The portal (or a backend) refused the request.
    Failed(String),
    /// The free-standing portal runtime/thread could not be started.
    Runtime(String),
}

impl PortalError {
    /// Whether the host simply cannot do screen capture.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }

    /// Whether the user backed out (nothing is wrong with the host).
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl fmt::Display for PortalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => {
                write!(formatter, "screen capture is unavailable: {reason}")
            }
            Self::Cancelled => formatter.write_str("the screen cast dialog was cancelled"),
            Self::NoStreams => formatter.write_str(
                "the desktop portal completed the screen cast dialog without a stream \
                 (nothing was selected)",
            ),
            Self::Failed(reason) => write!(formatter, "the screen cast request failed: {reason}"),
            Self::Runtime(reason) => {
                write!(formatter, "could not run the screen cast request: {reason}")
            }
        }
    }
}

impl std::error::Error for PortalError {}

/// A live portal screencast: the node id plus the session that owns it.
///
/// Keep the value alive for as long as the screen source runs (see the module
/// docs); dropping it releases the portal session and the PipeWire node.
pub struct PortalStream {
    info: StreamInfo,
    restore_token: Option<String>,
    session: Option<PortalSession>,
}

impl PortalStream {
    /// The mapped stream (node id, size, source type).
    pub fn info(&self) -> &StreamInfo {
        &self.info
    }

    /// The PipeWire node id to hand to `pipewiresrc path=`.
    pub fn node_id(&self) -> u32 {
        self.info.node_id
    }

    /// Capture width reported by the portal (`0` when unknown).
    pub fn width(&self) -> u32 {
        self.info.width
    }

    /// Capture height reported by the portal (`0` when unknown).
    pub fn height(&self) -> u32 {
        self.info.height
    }

    /// Which kind of source the user picked.
    pub fn source_type(&self) -> PortalSourceType {
        self.info.source_type
    }

    /// The portal's restore token, when the backend offered one.
    ///
    /// Pass it back through [`request_screencast_restoring`] to skip the dialog
    /// for a previously approved source. Not persisted by the engine: a Scene
    /// or a preset decides whether it should be saved.
    pub fn restore_token(&self) -> Option<&str> {
        self.restore_token.as_deref()
    }

    /// A ready-to-start capture configuration for this stream.
    pub fn video_config(&self) -> VideoSourceConfig {
        self.info.video_config()
    }

    /// The capture kind this stream feeds.
    pub fn video_source(&self) -> VideoSourceKind {
        self.info.video_source()
    }

    /// Close the portal session and wait (bounded by [`CLOSE_TIMEOUT`]).
    ///
    /// Use this when the next step depends on the node being gone — e.g.
    /// capturing the same monitor again. Plain `drop` also closes the session,
    /// but off-thread and without waiting.
    pub fn close(mut self) {
        if let Some(session) = self.session.take() {
            session.close();
        }
    }
}

impl fmt::Debug for PortalStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortalStream")
            .field("info", &self.info)
            .field("restore_token", &self.restore_token.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for PortalStream {
    fn drop(&mut self) {
        // Assigning `None` runs `PortalSession::drop`, which stops the runtime
        // thread without waiting (use `close` to wait).
        self.session = None;
    }
}

/// Ask the desktop portal for one screen and wait for the user's answer.
///
/// Blocks until the system dialog is answered. **Run it on a worker thread**;
/// see the module docs.
///
/// # Errors
///
/// * [`PortalError::Unavailable`] — no D-Bus session or no `ScreenCast` portal
///   (headless hosts, CI, a compositor without a portal backend).
/// * [`PortalError::Cancelled`] — the user dismissed the dialog.
/// * [`PortalError::NoStreams`] — the portal completed but returned nothing.
/// * [`PortalError::Failed`] — the portal refused the request.
pub fn request_screencast(selection: SourceSelection) -> Result<PortalStream, PortalError> {
    request(selection, None)
}

/// [`request_screencast`], resuming a previously approved source.
///
/// A backend that supports restore tokens skips the dialog as long as the token
/// is still valid; otherwise it shows it as usual.
pub fn request_screencast_restoring(
    selection: SourceSelection,
    restore_token: &str,
) -> Result<PortalStream, PortalError> {
    request(selection, Some(restore_token.to_string()))
}

#[cfg(target_os = "linux")]
mod platform {
    use std::sync::mpsc;

    use ashpd::desktop::screencast::{
        CursorMode, Screencast, SelectSourcesOptions, StartCastOptions, Stream,
    };
    use ashpd::desktop::{CreateSessionOptions, PersistMode, ResponseError, Session};

    use super::{
        CLOSE_TIMEOUT, PortalError, PortalStream, RawStream, SourceSelection, StreamInfo,
        select_stream,
    };

    /// What the background runtime thread hands back to the caller.
    type Answer = Result<(StreamInfo, Option<String>), PortalError>;

    pub(super) fn request(
        selection: SourceSelection,
        restore_token: Option<String>,
    ) -> Result<PortalStream, PortalError> {
        let (answer_tx, answer_rx) = mpsc::channel::<Answer>();
        // Created outside the runtime: a oneshot channel needs no reactor, so
        // the caller can hold the sender while the thread owns the runtime.
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

        let thread = std::thread::Builder::new()
            .name("modv-portal-screencast".to_string())
            .spawn(move || run(selection, restore_token, answer_tx, stop_rx))
            .map_err(|error| {
                PortalError::Runtime(format!("could not spawn the portal thread: {error}"))
            })?;

        let answer = answer_rx.recv().map_err(|_| {
            PortalError::Runtime("the portal thread exited before answering".to_string())
        })?;

        match answer {
            Ok((info, restore_token)) => Ok(PortalStream {
                info,
                restore_token,
                session: Some(super::PortalSession::new(stop_tx, thread)),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    /// Body of the `modv-portal-screencast` thread.
    ///
    /// Opens the session, answers the caller, then parks. Parking matters: the
    /// portal destroys the session when its D-Bus connection closes, and a
    /// current-thread runtime parked in `block_on` keeps driving zbus's socket,
    /// so the node stays alive without any further work.
    fn run(
        selection: SourceSelection,
        restore_token: Option<String>,
        answer: mpsc::Sender<Answer>,
        stop: tokio::sync::oneshot::Receiver<()>,
    ) {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = answer.send(Err(PortalError::Runtime(format!(
                    "could not start the portal runtime: {error}"
                ))));
                return;
            }
        };

        let opened = match runtime.block_on(open(selection, restore_token.as_deref())) {
            Ok(opened) => opened,
            Err(error) => {
                let _ = answer.send(Err(error));
                return;
            }
        };

        let Opened {
            info,
            restore_token,
            session,
        } = opened;

        if answer.send(Ok((info, restore_token))).is_err() {
            // The caller vanished while the dialog was open: close the session
            // instead of leaking it, since nobody can stop it any more.
            runtime.block_on(async { close_session(&session).await });
            return;
        }

        // Wait for the owner to drop/close the stream, then shut the session
        // down deterministically so the node is released before the process
        // leaves.
        let _ = runtime.block_on(stop);
        runtime.block_on(async { close_session(&session).await });
    }

    async fn close_session(session: &Session<Screencast>) {
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, session.close()).await;
    }

    /// The live parts of an answered request.
    struct Opened {
        info: StreamInfo,
        restore_token: Option<String>,
        session: Session<Screencast>,
    }

    async fn open(
        selection: SourceSelection,
        restore_token: Option<&str>,
    ) -> Result<Opened, PortalError> {
        // First contact: every failure here means the host has no usable
        // ScreenCast portal, which is an "unavailable" answer, not a bug.
        let proxy = Screencast::new().await.map_err(unavailable)?;
        let session = proxy
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(unavailable)?;

        proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_sources(source_type(selection))
                    .set_multiple(false)
                    .set_cursor_mode(CursorMode::HIDDEN)
                    .set_restore_token(restore_token)
                    .set_persist_mode(PersistMode::ExplicitlyRevoked),
            )
            .await
            .map_err(request_failed)?;

        // `start` is the call that raises the dialog, and `response()` is where
        // a dismissal surfaces as `ResponseError::Cancelled`.
        let streams = proxy
            .start(&session, None, StartCastOptions::default())
            .await
            .map_err(request_failed)?
            .response()
            .map_err(request_failed)?;

        let reply = raw_streams(streams.streams());
        let info = select_stream(&reply)?;

        Ok(Opened {
            info,
            restore_token: streams.restore_token().map(str::to_string),
            session,
        })
    }

    fn source_type(selection: SourceSelection) -> ashpd::desktop::screencast::SourceType {
        ashpd::desktop::screencast::SourceType::from_bits_truncate(selection.bits())
    }

    /// Convert the portal's `Streams` reply into the pure [`RawStream`] shape.
    fn raw_streams(streams: &[Stream]) -> Vec<RawStream> {
        streams
            .iter()
            .map(|stream| RawStream {
                node_id: stream.pipe_wire_node_id(),
                size: stream.size(),
                source_type: stream.source_type().map(|kind| kind.bits()),
            })
            .collect()
    }

    /// Is this the user dismissing the dialog?
    fn is_cancelled(error: &ashpd::Error) -> bool {
        matches!(
            error,
            ashpd::Error::Response(ResponseError::Cancelled)
                | ashpd::Error::Portal(ashpd::PortalError::Cancelled(_))
        )
    }

    fn unavailable(error: ashpd::Error) -> PortalError {
        match error {
            ashpd::Error::PortalNotFound(interface) => PortalError::Unavailable(format!(
                "{interface} is not provided by xdg-desktop-portal (install a ScreenCast \
                 backend such as xdg-desktop-portal-gnome, -kde or -wlr)"
            )),
            ashpd::Error::Zbus(error) => PortalError::Unavailable(format!(
                "could not reach the desktop portal over D-Bus: {error} (is a D-Bus session \
                 bus and xdg-desktop-portal running?)"
            )),
            other => PortalError::Unavailable(other.to_string()),
        }
    }

    fn request_failed(error: ashpd::Error) -> PortalError {
        if is_cancelled(&error) {
            PortalError::Cancelled
        } else {
            PortalError::Failed(error.to_string())
        }
    }
}

#[cfg(target_os = "linux")]
use platform::request;

#[cfg(not(target_os = "linux"))]
fn request(
    _selection: SourceSelection,
    _restore_token: Option<String>,
) -> Result<PortalStream, PortalError> {
    Err(PortalError::Unavailable(
        "xdg-desktop-portal ScreenCast is only available on Linux; use the renderer's \
         getDisplayMedia path on this platform"
            .to_string(),
    ))
}

/// Owns the portal runtime thread: keeping it alive keeps the session (and its
/// PipeWire node) alive.
#[cfg(target_os = "linux")]
struct PortalSession {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl PortalSession {
    fn new(stop: tokio::sync::oneshot::Sender<()>, thread: std::thread::JoinHandle<()>) -> Self {
        Self {
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// Signal the thread without waiting; the `JoinHandle` is dropped, which
    /// detaches it. It still closes the session, just off-thread.
    fn request_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.thread.take();
    }

    /// Signal the thread and wait for it to close the session.
    fn close(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for PortalSession {
    fn drop(&mut self) {
        self.request_stop();
    }
}

/// Non-Linux builds never construct a session: [`request`] always reports
/// [`PortalError::Unavailable`].
#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
struct PortalSession;

#[cfg(not(target_os = "linux"))]
impl PortalSession {
    fn close(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(node_id: u32, size: Option<(i32, i32)>, source_type: Option<u32>) -> RawStream {
        RawStream {
            node_id,
            size,
            source_type,
        }
    }

    #[test]
    fn the_first_stream_wins_and_keeps_its_node_id() {
        // `multiple = false` should mean one stream, but a backend can return
        // more; the engine captures the first, like a single-track
        // getDisplayMedia grab.
        let streams = [
            stream(42, Some((2560, 1440)), Some(SOURCE_TYPE_MONITOR)),
            stream(47, Some((1920, 1080)), Some(SOURCE_TYPE_WINDOW)),
        ];

        let selected = select_stream(&streams).expect("a stream");

        assert_eq!(selected.node_id, 42);
        assert_eq!((selected.width, selected.height), (2560, 1440));
        assert_eq!(selected.source_type, PortalSourceType::Monitor);
        assert_eq!(
            selected.video_source(),
            VideoSourceKind::Screen { node_id: 42 }
        );
        assert_eq!(selected.video_config(), VideoSourceConfig::screen(42));
    }

    #[test]
    fn a_missing_or_nonsense_size_is_reported_as_zero() {
        let no_size = StreamInfo::from_raw(&stream(7, None, None));
        let zero = StreamInfo::from_raw(&stream(7, Some((0, 0)), None));
        let negative = StreamInfo::from_raw(&stream(7, Some((-1, -1080)), None));
        let half = StreamInfo::from_raw(&stream(7, Some((1280, 0)), None));

        for info in [&no_size, &zero, &negative, &half] {
            assert!(!info.has_size(), "{info:?}");
        }

        assert_eq!((no_size.width, no_size.height), (0, 0));
        assert_eq!((zero.width, zero.height), (0, 0));
        assert_eq!((negative.width, negative.height), (0, 0));
        assert_eq!(
            (half.width, half.height),
            (1280, 0),
            "a partial size is kept"
        );

        // An absent source type is "unspecified", not an error.
        assert_eq!(no_size.source_type, PortalSourceType::Unspecified);
    }

    #[test]
    fn an_empty_reply_is_its_own_error() {
        let error = select_stream(&[]).expect_err("no streams");

        assert_eq!(error, PortalError::NoStreams);
        assert!(!error.is_unavailable());
        assert!(error.to_string().contains("without a stream"));
    }

    #[test]
    fn portal_source_type_flags_are_decoded() {
        assert_eq!(
            PortalSourceType::from_bits(SOURCE_TYPE_MONITOR),
            PortalSourceType::Monitor
        );
        assert_eq!(
            PortalSourceType::from_bits(SOURCE_TYPE_WINDOW),
            PortalSourceType::Window
        );
        assert_eq!(
            PortalSourceType::from_bits(SOURCE_TYPE_VIRTUAL),
            PortalSourceType::Virtual
        );
        assert_eq!(
            PortalSourceType::from_bits(0),
            PortalSourceType::Unspecified
        );

        // Combined/unknown flags keep their bits so a newer portal is visible
        // in the log instead of looking like "no source type".
        let combined = PortalSourceType::from_bits(SOURCE_TYPE_MONITOR | SOURCE_TYPE_WINDOW);
        assert_eq!(combined, PortalSourceType::Unknown(3));
        assert_eq!(combined.bits(), 3);
        assert_eq!(combined.to_string(), "unknown(3)");

        for kind in [
            PortalSourceType::Monitor,
            PortalSourceType::Window,
            PortalSourceType::Virtual,
            PortalSourceType::Unspecified,
        ] {
            assert_eq!(PortalSourceType::from_bits(kind.bits()), kind);
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn selection_bits_and_cli_args_round_trip() {
        assert_eq!(SourceSelection::Monitor.bits(), SOURCE_TYPE_MONITOR);
        assert_eq!(SourceSelection::Window.bits(), SOURCE_TYPE_WINDOW);
        assert_eq!(
            SourceSelection::MonitorOrWindow.bits(),
            SOURCE_TYPE_MONITOR | SOURCE_TYPE_WINDOW
        );
        assert_eq!(SourceSelection::default(), SourceSelection::Monitor);

        for selection in [
            SourceSelection::Monitor,
            SourceSelection::Window,
            SourceSelection::MonitorOrWindow,
        ] {
            assert_eq!(
                SourceSelection::from_arg(selection.as_arg()),
                Some(selection)
            );
            assert_eq!(selection.to_string(), selection.as_arg());
        }

        assert_eq!(
            SourceSelection::from_arg("  SCREEN "),
            Some(SourceSelection::Monitor)
        );
        assert_eq!(SourceSelection::from_arg("minimap"), None);
    }

    #[test]
    fn the_list_helper_renders_every_stream() {
        let empty = describe_streams(&[]);
        assert!(empty.contains("no screen cast streams"), "{empty}");

        let table = describe_streams(&[
            stream(42, Some((2560, 1440)), Some(SOURCE_TYPE_MONITOR)),
            stream(47, None, Some(SOURCE_TYPE_WINDOW)),
            stream(51, Some((800, 600)), None),
        ]);

        assert!(table.starts_with("screen cast streams (3):"), "{table}");
        assert!(
            table.contains("[0] node=42 size=2560x1440 source=monitor"),
            "{table}"
        );
        assert!(
            table.contains("[1] node=47 size=unknown source=window"),
            "{table}"
        );
        assert!(
            table.contains("[2] node=51 size=800x600 source=unspecified"),
            "{table}"
        );
    }

    #[test]
    fn raw_stream_builders_match_the_portal_shape() {
        let raw = RawStream::new(9)
            .with_size(1920, 1080)
            .with_source_type(SOURCE_TYPE_WINDOW);

        assert_eq!(raw.size, Some((1920, 1080)));
        assert_eq!(raw.dimensions(), (1920, 1080));
        assert_eq!(raw.source_type, Some(SOURCE_TYPE_WINDOW));
    }

    #[test]
    fn portal_answers_can_move_between_threads() {
        fn assert_send<T: Send>() {}

        assert_send::<PortalError>();
        assert_send::<StreamInfo>();
        assert_send::<PortalStream>();
        assert_send::<SourceSelection>();
    }

    #[test]
    fn an_unavailable_answer_says_so() {
        let error = PortalError::Unavailable("no portal".to_string());

        assert!(error.is_unavailable());
        assert!(!error.is_cancelled());
        assert!(error.to_string().contains("unavailable"));
    }
}
