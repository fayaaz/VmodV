//! VmodV shell screen capture through the XDG desktop portal.
//!
//! The VmodV shell asks the portal itself: [`request_screencast`] raises the
//! compositor's own "share a monitor/window" dialog and answers with a
//! [`PortalStream`] whose PipeWire node id the [`crate::media::MediaHub`]
//! captures. Nothing here is specific to the engine binary — the supervisor's
//! `media/pick-screen` uses the same portal call for the Electron path.
//!
//! ```text
//! Media panel  ──MediaAction::ShareScreen──▶  VmodvApp::share_screen
//!                                               │ spawn_portal_request (thread)
//!                                               ▼
//!                                          request_screencast   (blocks on the dialog)
//!                                               │ ScreenShare::complete(id, result)
//!                                               ▼
//!                    MediaHub::start("screen:portal", screen(node_id))
//!                    ScreenShare::started(...)   ← keeps the PortalStream alive
//! ```
//!
//! # Ownership
//!
//! A portal session lives exactly as long as its D-Bus connection, and the
//! PipeWire node `pipewiresrc` is reading dies with it. [`ScreenShare`] owns
//! the [`ScreenStream`] for the whole capture; stopping the source takes the
//! handle back ([`ScreenShare::stop`]) so the shell can close the session. The
//! state machine only ever holds **one** pending request and **one** live
//! session: the Media panel offers a single screen source (the Webcam module
//! indexes it like another camera), so a second share is refused until the
//! first is stopped.
//!
//! # Request ids
//!
//! Every request carries a monotonic id. The blocking portal call runs on a
//! worker thread and its answer can arrive after the user cancelled the
//! request (or after a stale answer raced a new one), so
//! [`ScreenShare::complete`] only applies an answer whose id is still pending;
//! everything else is [`ScreenAnswer::Stale`] and the handle is dropped, which
//! closes the portal session in the background.
//!
//! # Testing seam
//!
//! The portal dialog needs a live D-Bus session, an `xdg-desktop-portal` with a
//! `ScreenCast` backend and a user to answer it, so tests never open it. The
//! state machine is generic over [`ScreenStream`], and
//! [`ScreenShare::complete`] takes the worker's answer directly: tests inject a
//! fake `ScreenStream` success, a [`PortalError`] or a stale id. Only
//! [`spawn_portal_request`] touches the real portal, and the shell calls it at
//! exactly one place.

use std::fmt;
use std::sync::mpsc::Receiver;

use modv_media::video::VideoSourceConfig;
use modv_media::video::portal::{PortalError, PortalStream, SourceSelection};

/// The MediaHub source id the portal capture runs as.
///
/// It is camera-like (a `Screen` source), so it takes the next Webcam index
/// exactly like a camera or the test pattern and the Webcam module's `camera`
/// prop can draw it. One screen at a time keeps the id stable.
pub const SCREEN_SOURCE_ID: &str = "screen:portal";

/// Which portal picker the user asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenShareKind {
    /// `Share monitor…`: the dialog lists the monitors (and virtual outputs).
    #[default]
    Monitor,
    /// `Share window…`: the dialog lists application windows.
    Window,
}

impl ScreenShareKind {
    /// Map the `MediaAction::ShareScreen` flag.
    pub fn for_window(window: bool) -> Self {
        if window { Self::Window } else { Self::Monitor }
    }

    /// Whether this is the window picker.
    pub fn is_window(self) -> bool {
        matches!(self, Self::Window)
    }

    /// The portal's `types` selection for this picker.
    pub fn selection(self) -> SourceSelection {
        match self {
            Self::Monitor => SourceSelection::Monitor,
            Self::Window => SourceSelection::Window,
        }
    }

    /// The logging/`sourceType` spelling (`monitor`/`window`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Window => "window",
        }
    }
}

impl fmt::Display for ScreenShareKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A live portal session, as far as the shell's state machine is concerned.
///
/// [`PortalStream`] is the production implementation; tests use a fake so no
/// D-Bus session is needed.
pub trait ScreenStream: Send + 'static {
    /// The PipeWire node id `pipewiresrc path=` captures.
    fn node_id(&self) -> u32;

    /// Release the portal session (bounded by the portal's close timeout).
    fn close(self: Box<Self>);
}

impl ScreenStream for PortalStream {
    fn node_id(&self) -> u32 {
        PortalStream::node_id(self)
    }

    fn close(self: Box<Self>) {
        (*self).close();
    }
}

/// Why a screen share produced no running source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenShareError {
    /// The user dismissed the portal dialog (nothing is wrong with the host).
    Cancelled,
    /// No D-Bus portal, or no `ScreenCast` backend behind it.
    Unavailable(String),
    /// The portal completed the dialog without a stream.
    NoStreams,
    /// The portal or its backend refused the request.
    Failed(String),
    /// The worker thread could not be started or never answered.
    Runtime(String),
    /// The picked node did not start in the MediaHub (`pipewiresrc` error).
    StartFailed(String),
    /// The host lacks `pipewiresrc`/PipeWire, so no node could be captured.
    Unsupported(String),
}

impl ScreenShareError {
    /// The engine's wire error code (`media/start-*` vocabulary).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Unavailable(_) | Self::Runtime(_) | Self::Unsupported(_) => "media_unavailable",
            Self::NoStreams | Self::Failed(_) | Self::StartFailed(_) => "media_start_failed",
        }
    }

    /// The human-readable detail.
    pub fn message(&self) -> &str {
        match self {
            Self::Cancelled => "the screen share dialog was cancelled",
            Self::Unavailable(reason)
            | Self::Failed(reason)
            | Self::Runtime(reason)
            | Self::StartFailed(reason)
            | Self::Unsupported(reason) => reason,
            Self::NoStreams => {
                "the desktop portal completed the screen share dialog without a stream \
                 (nothing was selected)"
            }
        }
    }
}

impl fmt::Display for ScreenShareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("the screen share dialog was cancelled"),
            Self::Unavailable(reason) | Self::Unsupported(reason) => {
                write!(formatter, "screen capture is unavailable: {reason}")
            }
            Self::NoStreams => write!(
                formatter,
                "the desktop portal finished the screen share dialog without a stream \
                 (nothing was selected)"
            ),
            Self::Failed(reason) => write!(formatter, "the screen share request failed: {reason}"),
            Self::Runtime(reason) => write!(formatter, "could not run the screen share: {reason}"),
            Self::StartFailed(reason) => {
                write!(formatter, "the screen source did not start: {reason}")
            }
        }
    }
}

impl From<PortalError> for ScreenShareError {
    fn from(error: PortalError) -> Self {
        match error {
            PortalError::Cancelled => Self::Cancelled,
            PortalError::Unavailable(reason) => Self::Unavailable(reason),
            PortalError::NoStreams => Self::NoStreams,
            PortalError::Failed(reason) => Self::Failed(reason),
            PortalError::Runtime(reason) => Self::Runtime(reason),
        }
    }
}

/// One portal request the shell must run on a worker thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRequest {
    /// Monotonic request id (answers for older ids are discarded).
    pub id: u64,
    /// The picker the dialog offers.
    pub kind: ScreenShareKind,
}

/// What a worker's answer means for the shell.
pub enum ScreenAnswer<H> {
    /// No request with that id is outstanding any more: the user cancelled it,
    /// it already completed, or it was never issued. Drop the handle.
    Stale,
    /// Start `config` in the MediaHub; on success hand `handle` back through
    /// [`ScreenShare::started`] so the portal session outlives the capture.
    Start {
        request: ScreenRequest,
        config: VideoSourceConfig,
        handle: H,
    },
    /// The dialog failed or was dismissed; the panel shows the error.
    Failed {
        request: ScreenRequest,
        error: ScreenShareError,
    },
}

/// The one live portal capture.
///
/// `source_id` is the MediaHub key it runs under ([`SCREEN_SOURCE_ID`]); the
/// handle owns the portal session behind it.
struct Running<H> {
    source_id: &'static str,
    kind: ScreenShareKind,
    node_id: u32,
    handle: H,
}

/// The shell's screen-capture state: one pending request, one live session.
pub struct ScreenShare<H> {
    next_id: u64,
    pending: Option<ScreenRequest>,
    running: Option<Running<H>>,
    error: Option<ScreenShareError>,
}

impl<H> Default for ScreenShare<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> ScreenShare<H> {
    /// No request and no session.
    pub fn new() -> Self {
        Self {
            next_id: 0,
            pending: None,
            running: None,
            error: None,
        }
    }

    /// The request waiting for the portal dialog.
    pub fn pending(&self) -> Option<ScreenRequest> {
        self.pending
    }

    /// Whether a request or a live session owns the one screen slot.
    pub fn is_busy(&self) -> bool {
        self.pending.is_some() || self.running.is_some()
    }

    /// The live session's MediaHub source id, picker and PipeWire node.
    pub fn running(&self) -> Option<(&'static str, ScreenShareKind, u32)> {
        self.running
            .as_ref()
            .map(|running| (running.source_id, running.kind, running.node_id))
    }

    /// The last failure, shown in the Media panel until the next request.
    pub fn error(&self) -> Option<&ScreenShareError> {
        self.error.as_ref()
    }

    /// Record a refusal before any worker started (support gate, spawn error).
    pub fn refuse(&mut self, error: ScreenShareError) {
        self.error = Some(error);
    }

    /// Reserve the one screen slot for a request; `None` when it is taken.
    ///
    /// A new request also clears the previous failure so the panel never shows
    /// a stale reason next to a fresh dialog.
    pub fn request(&mut self, kind: ScreenShareKind) -> Option<ScreenRequest> {
        if self.is_busy() {
            return None;
        }

        self.next_id += 1;
        let request = ScreenRequest {
            id: self.next_id,
            kind,
        };
        self.pending = Some(request);
        self.error = None;
        Some(request)
    }

    /// Stop waiting for the open dialog; `None` when nothing was pending.
    ///
    /// The portal dialog itself cannot be dismissed from here. The worker's
    /// late answer is dropped ([`Self::complete`] reports it stale), which also
    /// closes the portal session it opened.
    pub fn cancel(&mut self) -> Option<u64> {
        let pending = self.pending.take()?;
        self.error = None;
        Some(pending.id)
    }
}

impl<H: ScreenStream> ScreenShare<H> {
    /// Apply the worker's answer for `request_id`.
    pub fn complete(&mut self, request_id: u64, result: Result<H, PortalError>) -> ScreenAnswer<H> {
        let Some(request) = self.pending.filter(|pending| pending.id == request_id) else {
            // Cancelled, already answered, or never issued: the session the
            // handle owns (if any) is closed by dropping it.
            return ScreenAnswer::Stale;
        };
        self.pending = None;

        match result {
            Ok(handle) => {
                let node_id = handle.node_id();
                self.error = None;
                ScreenAnswer::Start {
                    request,
                    config: screen_config(node_id),
                    handle,
                }
            }
            Err(error) => {
                let error = ScreenShareError::from(error);
                self.error = Some(error.clone());
                ScreenAnswer::Failed { request, error }
            }
        }
    }

    /// Record the running session after the MediaHub accepted its node.
    pub fn started(&mut self, kind: ScreenShareKind, node_id: u32, handle: H) {
        self.running = Some(Running {
            source_id: SCREEN_SOURCE_ID,
            kind,
            node_id,
            handle,
        });
    }

    /// Record a failed MediaHub start; nothing is running afterwards.
    pub fn start_failed(&mut self, error: ScreenShareError) {
        self.error = Some(error);
    }

    /// Stop the session and hand the portal handle back for closing.
    pub fn stop(&mut self) -> Option<H> {
        self.running.take().map(|running| running.handle)
    }
}

/// Start the blocking portal request on a worker thread.
///
/// This is the only place that touches the real portal. It returns a receiver;
/// the shell drains it once per frame and feeds the answer (and its request id)
/// to [`ScreenShare::complete`]. A request the user never answers parks its
/// thread inside `request_screencast` until the process exits; dropping the
/// receiver instead lets the portal thread close the session as soon as the
/// dialog is answered.
pub fn spawn_portal_request(
    kind: ScreenShareKind,
) -> Result<Receiver<Result<PortalStream, PortalError>>, ScreenShareError> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("modv-portal-share".to_string())
        .spawn(move || {
            // Blocks until the compositor's dialog is answered.
            let _ = tx.send(modv_media::video::portal::request_screencast(
                kind.selection(),
            ));
        })
        .map_err(|error| {
            ScreenShareError::Runtime(format!("could not start the screen share thread: {error}"))
        })?;

    Ok(rx)
}

/// The video-source config for a portal node (`pipewiresrc path=<node>`).
pub fn screen_config(node_id: u32) -> VideoSourceConfig {
    VideoSourceConfig::screen(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use modv_media::video::VideoSourceKind;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A `PortalStream` stand-in: a node id plus a "session closed" flag.
    struct FakeStream {
        node_id: u32,
        closed: Arc<AtomicBool>,
    }

    impl FakeStream {
        fn new(node_id: u32) -> (Self, Arc<AtomicBool>) {
            let closed = Arc::new(AtomicBool::new(false));
            (
                Self {
                    node_id,
                    closed: closed.clone(),
                },
                closed,
            )
        }
    }

    impl ScreenStream for FakeStream {
        fn node_id(&self) -> u32 {
            self.node_id
        }

        fn close(self: Box<Self>) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn the_pickers_map_to_the_portal_selections() {
        assert_eq!(ScreenShareKind::for_window(false), ScreenShareKind::Monitor);
        assert_eq!(ScreenShareKind::for_window(true), ScreenShareKind::Window);
        assert!(!ScreenShareKind::Monitor.is_window());
        assert!(ScreenShareKind::Window.is_window());
        assert_eq!(
            ScreenShareKind::Monitor.selection(),
            SourceSelection::Monitor
        );
        assert_eq!(ScreenShareKind::Window.selection(), SourceSelection::Window);
        assert_eq!(ScreenShareKind::default(), ScreenShareKind::Monitor);

        for kind in [ScreenShareKind::Monitor, ScreenShareKind::Window] {
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn a_screen_source_is_one_slot_with_an_increasing_request_id() {
        let mut share = ScreenShare::<FakeStream>::new();
        assert!(!share.is_busy());

        let first = share.request(ScreenShareKind::Monitor).expect("free slot");
        assert_eq!(first.id, 1);
        assert_eq!(first.kind, ScreenShareKind::Monitor);
        assert_eq!(share.pending(), Some(first));
        assert!(share.is_busy());

        assert_eq!(
            share.request(ScreenShareKind::Window),
            None,
            "one portal dialog at a time"
        );

        let (handle, _closed) = FakeStream::new(42);
        match share.complete(first.id, Ok(handle)) {
            ScreenAnswer::Start {
                request, config, ..
            } => {
                assert_eq!(request, first);
                assert_eq!(
                    config.kind,
                    VideoSourceKind::Screen { node_id: 42 },
                    "the answer asks for the portal node"
                );
            }
            _ => panic!("a fresh answer must start the source"),
        }
        assert_eq!(share.pending(), None, "the answer is no longer pending");
        assert!(
            !share.is_busy(),
            "the source is not running until the hub starts it"
        );

        let (handle, closed) = FakeStream::new(42);
        share.started(ScreenShareKind::Monitor, 42, handle);
        assert_eq!(
            share.running(),
            Some((SCREEN_SOURCE_ID, ScreenShareKind::Monitor, 42)),
            "the session is live under the stable source id"
        );
        assert!(
            share.request(ScreenShareKind::Window).is_none(),
            "one live screen capture at a time"
        );

        let handle = share.stop().expect("the session handle");
        Box::new(handle).close();
        assert!(
            closed.load(Ordering::SeqCst),
            "stopping closes the portal session"
        );
        assert_eq!(share.running(), None);

        let second = share.request(ScreenShareKind::Window).expect("free again");
        assert_eq!(second.id, 2, "request ids keep increasing");
    }

    #[test]
    fn an_answer_for_a_cancelled_request_is_stale() {
        let mut share = ScreenShare::<FakeStream>::new();
        let request = share.request(ScreenShareKind::Window).expect("free slot");
        assert_eq!(share.cancel(), Some(request.id));
        assert_eq!(share.pending(), None);
        assert!(!share.is_busy());

        let (handle, closed) = FakeStream::new(7);
        match share.complete(request.id, Ok(handle)) {
            ScreenAnswer::Stale => {}
            _ => panic!("a cancelled request must not start a source"),
        }
        assert_eq!(share.running(), None);
        assert!(
            !closed.load(Ordering::SeqCst),
            "the state machine hands the stale handle to the caller, which drops it"
        );

        assert_eq!(share.cancel(), None, "nothing is pending any more");
        assert!(matches!(
            share.complete(request.id, Err(PortalError::Cancelled)),
            ScreenAnswer::Stale
        ));
        assert!(
            share.error().is_none(),
            "a stale dismissal must not overwrite the panel"
        );

        assert!(matches!(
            share.complete(999, Err(PortalError::Cancelled)),
            ScreenAnswer::Stale
        ));
    }

    #[test]
    fn an_answer_after_the_user_stopped_the_source_is_stale() {
        let mut share = ScreenShare::<FakeStream>::new();
        let request = share.request(ScreenShareKind::Monitor).expect("free slot");
        let (handle, _closed) = FakeStream::new(5);
        let ScreenAnswer::Start { handle, .. } = share.complete(request.id, Ok(handle)) else {
            panic!("the fake answer starts a source");
        };
        share.started(ScreenShareKind::Monitor, 5, handle);

        let running = share.stop().expect("the live session");
        Box::new(running).close();
        assert_eq!(share.running(), None);

        // A duplicated/late answer for the stopped request must not resurrect
        // the capture (the `screen:portal` slot is free but idle).
        let (late, closed) = FakeStream::new(5);
        assert!(matches!(
            share.complete(request.id, Ok(late)),
            ScreenAnswer::Stale
        ));
        assert_eq!(share.running(), None);
        assert!(!closed.load(Ordering::SeqCst));
        assert!(share.request(ScreenShareKind::Window).is_some());
    }

    #[test]
    fn portal_failures_become_panel_errors_with_wire_codes() {
        let mut share = ScreenShare::<FakeStream>::new();
        let request = share.request(ScreenShareKind::Monitor).expect("free slot");
        match share.complete(
            request.id,
            Err(PortalError::Unavailable("no D-Bus session".to_string())),
        ) {
            ScreenAnswer::Failed {
                request: failed,
                error,
            } => {
                assert_eq!(failed, request);
                assert_eq!(
                    error,
                    ScreenShareError::Unavailable("no D-Bus session".to_string())
                );
                assert_eq!(error.code(), "media_unavailable");
                assert!(error.to_string().contains("no D-Bus session"), "{error}");
            }
            _ => panic!("an unavailable portal must be reported"),
        }
        assert_eq!(share.pending(), None, "a failure releases the slot");
        assert_eq!(
            share.error(),
            Some(&ScreenShareError::Unavailable(
                "no D-Bus session".to_string()
            ))
        );
        assert!(share.request(ScreenShareKind::Monitor).is_some());
        assert_eq!(share.error(), None, "a new request clears the old error");
    }

    #[test]
    fn a_dismissed_dialog_is_a_cancellation_not_a_host_problem() {
        let mut share = ScreenShare::<FakeStream>::new();
        let request = share.request(ScreenShareKind::Window).expect("free slot");

        let answer = share.complete(request.id, Err(PortalError::Cancelled));
        let ScreenAnswer::Failed { error, .. } = answer else {
            panic!("a cancellation is still an answer the panel shows");
        };

        assert_eq!(error, ScreenShareError::Cancelled);
        assert_eq!(error.code(), "cancelled");
        assert!(error.to_string().contains("cancelled"), "{error}");
        assert_eq!(
            ScreenShareError::from(PortalError::Cancelled),
            ScreenShareError::Cancelled
        );
    }

    #[test]
    fn every_portal_error_keeps_its_reason_and_wire_code() {
        let cases = [
            (
                PortalError::Unavailable("no portal backend".to_string()),
                "media_unavailable",
            ),
            (PortalError::NoStreams, "media_start_failed"),
            (
                PortalError::Failed("the backend refused".to_string()),
                "media_start_failed",
            ),
            (
                PortalError::Runtime("the thread vanished".to_string()),
                "media_unavailable",
            ),
        ];

        for (portal, code) in cases {
            let expected = ScreenShareError::from(portal.clone());
            assert_eq!(expected.code(), code, "{expected:?}");
            let message = expected.to_string();
            assert!(!message.is_empty(), "{expected:?}");
            if matches!(expected, ScreenShareError::NoStreams) {
                assert!(message.contains("without a stream"), "{message}");
            } else {
                let reason = match &portal {
                    PortalError::Unavailable(reason)
                    | PortalError::Failed(reason)
                    | PortalError::Runtime(reason) => reason,
                    _ => unreachable!(),
                };
                assert!(
                    message.contains(reason),
                    "{message} must include {reason:?}"
                );
            }
        }

        let start = ScreenShareError::StartFailed("pipewiresrc error".to_string());
        assert_eq!(start.code(), "media_start_failed");
        assert!(start.to_string().contains("pipewiresrc error"));
        let unsupported = ScreenShareError::Unsupported("no pipewiresrc".to_string());
        assert_eq!(unsupported.code(), "media_unavailable");
        assert!(unsupported.to_string().contains("no pipewiresrc"));
        assert!(
            ScreenShareError::Runtime("x".to_string())
                .to_string()
                .contains("could not run the screen share")
        );
    }

    #[test]
    fn the_support_gate_explains_why_and_leaves_the_slot_free() {
        let mut share = ScreenShare::<FakeStream>::new();
        share.refuse(ScreenShareError::Unsupported(
            "no PipeWire server socket was found".to_string(),
        ));

        let error = share.error().expect("the panel shows the reason");
        assert_eq!(error.code(), "media_unavailable");
        assert!(
            error.to_string().contains("no PipeWire server socket"),
            "{error}"
        );
        assert!(!share.is_busy());
        assert!(
            share.request(ScreenShareKind::Monitor).is_some(),
            "the gate does not reserve the slot: the host may recover"
        );
    }

    #[test]
    fn a_failed_pipeline_start_reports_and_leaves_the_slot_free() {
        let mut share = ScreenShare::<FakeStream>::new();
        let request = share.request(ScreenShareKind::Monitor).expect("free slot");
        let (handle, _closed) = FakeStream::new(11);
        let ScreenAnswer::Start { handle, .. } = share.complete(request.id, Ok(handle)) else {
            panic!("the fake answer starts a source");
        };
        drop(handle);

        share.start_failed(ScreenShareError::StartFailed(
            "pipewiresrc path=11 failed".to_string(),
        ));
        assert_eq!(share.running(), None, "nothing is running");
        assert_eq!(
            share.error().map(ScreenShareError::code),
            Some("media_start_failed")
        );
        assert!(share.request(ScreenShareKind::Window).is_some());
    }
}
