//! NDI send/receive in engine mode (WS-11).
//!
//! The legacy renderer owns NDI in two places: `worker/ndi-worker.worker.js`
//! reads the composited main canvas (`transferToImageBitmap` → `getImageData`)
//! and submits one `grandiose` video frame per rendered frame, while
//! `store/modules/ndi.js` creates a `grandiose` receiver per source and draws
//! its frames into an auxiliary canvas. Engine mode has no worker and no
//! auxiliary canvases, so this module is the engine's replacement:
//!
//! * [`NdiConfig`] mirrors the JS `ndi` state (`outputEnabled`, `outputName`,
//!   `outputWidth`/`outputHeight`, `followModVOutputSize`, `followModVFps`,
//!   `outputTargetFps`) with the same defaults and clamping on write;
//! * the **sender** lives on its own thread. The render thread only copies a
//!   freshly presented frame into a bounded [`FrameQueue`] (capacity
//!   [`SENDER_QUEUE_CAPACITY`]) and never blocks: when the SDK falls behind the
//!   oldest queued frame is dropped, so the newest frame wins and latency never
//!   grows;
//! * the **receiver** side is a registry of ids ([`ReceiverInfo`]). Enabling a
//!   receiver spawns one thread which creates and connects the SDK receiver,
//!   then captures frames into a [`FrameSlot`] (one latest frame per id). The
//!   compositor uploads new slots into `ndi:<receiverId>` textures (see
//!   [`crate::compositor::Compositor::sync_ndi`]); a source that disappears
//!   (the SDK reports `frame_type_error`) moves the receiver to
//!   `ReceiverState::Error`, logs once and stops — it is not silently retried,
//!   which matches the legacy `receiver.data()` rejection path;
//! * discovery runs on a helper thread (`ndi/discoverSources`) so a finder wait
//!   never stalls the frame loop.
//!
//! ## Testability
//!
//! libndi is absent from CI (and most machines), so every SDK interaction goes
//! through the [`NdiTransport`] trait. [`LibNdiTransport`] is the real
//! `dlopen`-based implementation; the unit tests use a fake transport that
//! records sender opens and lets the test drive the spawned work closures. The
//! pure pieces ([`FrameClock`], [`FrameQueue`], [`pump_receiver`],
//! [`NdiConfig::normalised`]) are tested without any NDI at all, and the
//! unavailable path is asserted explicitly: enabling output or a receiver when
//! `libndi` is missing reports `Unavailable` and spawns nothing.
//!
//! ## Frame format and sizing
//!
//! The compositor reads the presented frame back as tightly packed RGBA8
//! (`Compositor::read_main_rgba8`), which is what the sender wants;
//! `NdiSender::send_rgba` performs the RGBA→BGRA repack the SDK needs.
//!
//! The legacy sender resized the canvas to the configured output size before
//! reading it. The engine instead reads at the compositor's size and scales
//! with [`scale_rgba_nearest`] when `follow_modv_output_size` is off and the
//! configured size differs. Nearest-neighbour is deliberate: it is a pure,
//! branch-free resample that costs one pass only while NDI output is enabled,
//! and a *send-side* size is advisory in NDI anyway (the receiver can request
//! its own geometry); the configured size is still honoured for the pixels that
//! are transmitted.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use modv_media::ndi::{self, NdiError, NdiFrame, NdiSourceInfo};
use modv_media::video::VideoFrame;

/// The legacy NDI sender name (`setup-grandiose.js:47`).
pub const DEFAULT_OUTPUT_NAME: &str = "VmodV";
/// The JS `ndi` module's default `outputWidth` (`store/modules/ndi.js:8`).
pub const DEFAULT_OUTPUT_WIDTH: u32 = 1920;
/// The JS `ndi` module's default `outputHeight` (`store/modules/ndi.js:9`).
pub const DEFAULT_OUTPUT_HEIGHT: u32 = 1080;
/// The JS `ndi` module's default `outputTargetFps` (`store/modules/ndi.js:12`).
pub const DEFAULT_OUTPUT_TARGET_FPS: f64 = 60.0;

/// Largest accepted output dimension. 8192² is the same frame guard the media
/// textures use, so a bogus preset cannot reserve gigabytes.
pub const MAX_OUTPUT_DIMENSION: u32 = 8192;
/// Largest accepted target frame rate.
pub const MAX_TARGET_FPS: f64 = 240.0;

/// Sender queue capacity. Two frames absorb one frame of SDK jitter while
/// keeping latency at most one frame; the third push drops the oldest.
pub const SENDER_QUEUE_CAPACITY: usize = 2;
/// How long the receiver thread blocks in `NDIlib_recv_capture_v2` per attempt.
/// Short enough that a stop request is honoured promptly, long enough that an
/// idle receiver burns no CPU.
pub const RECEIVER_CAPTURE_TIMEOUT_MS: u32 = 50;
/// How long `enable_receiver` waits for a matching source before reporting the
/// source as missing. (`ndi/discoverSources` normally found it already.)
pub const RECEIVER_CONNECT_TIMEOUT_MS: u32 = 1_000;
/// Discovery wait used by `ndi/discoverSources`.
pub const DISCOVERY_TIMEOUT_MS: u32 = 2_000;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// The NDI output configuration, mirroring the JS `ndi` state.
#[derive(Clone, Debug, PartialEq)]
pub struct NdiConfig {
    /// `outputEnabled`: whether frames are submitted to the sender.
    pub output_enabled: bool,
    /// `outputName`: the NDI sender name (`"VmodV"` by default).
    pub output_name: String,
    /// `outputWidth`: the sender's size when not following the output.
    pub output_width: u32,
    /// `outputHeight`: the sender's size when not following the output.
    pub output_height: u32,
    /// `followModVOutputSize`: send at the compositor's size.
    pub follow_modv_output_size: bool,
    /// `followModVFps`: pace/advertise at the engine's frame rate rather than
    /// `outputTargetFps`.
    pub follow_modv_fps: bool,
    /// `outputTargetFps`: the manual send rate.
    pub output_target_fps: f64,
}

impl Default for NdiConfig {
    fn default() -> Self {
        Self {
            output_enabled: false,
            output_name: DEFAULT_OUTPUT_NAME.to_string(),
            output_width: DEFAULT_OUTPUT_WIDTH,
            output_height: DEFAULT_OUTPUT_HEIGHT,
            follow_modv_output_size: true,
            follow_modv_fps: true,
            output_target_fps: DEFAULT_OUTPUT_TARGET_FPS,
        }
    }
}

impl NdiConfig {
    /// The config with every field inside its accepted range.
    ///
    /// The JS store accepts any value the input controls produce; the engine
    /// clamps on write so a divisible-by-zero size or a `NaN` frame rate can
    /// never reach the SDK. An empty/whitespace name falls back to `"VmodV"`
    /// (the JS mutation's default parameter).
    pub fn normalised(&self) -> Self {
        Self {
            output_enabled: self.output_enabled,
            output_name: normalise_output_name(&self.output_name),
            output_width: self.output_width.clamp(1, MAX_OUTPUT_DIMENSION),
            output_height: self.output_height.clamp(1, MAX_OUTPUT_DIMENSION),
            follow_modv_output_size: self.follow_modv_output_size,
            follow_modv_fps: self.follow_modv_fps,
            output_target_fps: normalise_fps(self.output_target_fps),
        }
    }

    /// The effective send/advertise rate.
    ///
    /// `engine_fps` is the measured render rate; `follow_modv_fps` uses it,
    /// otherwise `outputTargetFps` wins. Either way the result is clamped to
    /// `1..=MAX_TARGET_FPS`.
    pub fn target_fps(&self, engine_fps: f64) -> f64 {
        let fps = if self.follow_modv_fps {
            engine_fps
        } else {
            self.output_target_fps
        };

        normalise_fps(fps)
    }

    /// The size frames are submitted at.
    ///
    /// `source` is the compositor's size. Following the output sends it
    /// unchanged; otherwise the configured size is used and the frame is
    /// scaled (see [`scale_rgba_nearest`]).
    pub fn output_size(&self, source: (u32, u32)) -> (u32, u32) {
        if self.follow_modv_output_size {
            (source.0.max(1), source.1.max(1))
        } else {
            (self.output_width, self.output_height)
        }
    }
}

fn normalise_output_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        DEFAULT_OUTPUT_NAME.to_string()
    } else {
        name.to_string()
    }
}

fn normalise_fps(fps: f64) -> f64 {
    if fps.is_finite() && fps > 0.0 {
        fps.clamp(1.0, MAX_TARGET_FPS)
    } else {
        DEFAULT_OUTPUT_TARGET_FPS
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Paces frame submission at a target rate.
///
/// [`FrameClock::due`] returns `true` at most once per `1 / fps`; the very
/// first call is always due. A skipped period does not accumulate credit:
/// after a long stall the next call is due immediately, which keeps the sender
/// from bursting to catch up.
#[derive(Clone, Debug)]
pub struct FrameClock {
    fps: f64,
    last: Option<Instant>,
}

impl FrameClock {
    /// A clock at `fps` that is due on its first [`Self::due`] call.
    pub fn new(fps: f64) -> Self {
        Self {
            fps: normalise_fps(fps),
            last: None,
        }
    }

    /// The current rate.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Changes the rate; `last` is kept, so a slower rate takes effect from the
    /// previous frame's timestamp.
    pub fn set_fps(&mut self, fps: f64) {
        self.fps = normalise_fps(fps);
    }

    /// Forgets the previous timestamp (the next call is due).
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// Whether a frame is due at `now`, consuming the slot when it is.
    pub fn due(&mut self, now: Instant) -> bool {
        let interval = Duration::from_secs_f64(1.0 / self.fps);

        match self.last {
            Some(last) if now.saturating_duration_since(last) < interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// A bounded, drop-oldest queue.
///
/// The producer is the render thread and must never block; the consumer is the
/// NDI sender thread and may block waiting for a frame. When the queue is full
/// the **oldest** frame is dropped so the freshest frame is always sent (the
/// legacy worker awaited `sender.video()` per frame, i.e. stale frames were
/// naturally never queued).
pub struct FrameQueue<T> {
    inner: Mutex<QueueState<T>>,
    ready: Condvar,
    capacity: usize,
}

struct QueueState<T> {
    items: std::collections::VecDeque<T>,
    closed: bool,
    dropped: u64,
}

impl<T> FrameQueue<T> {
    /// A queue holding at most `capacity` items (`0` clamps to `1`).
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(QueueState {
                items: std::collections::VecDeque::new(),
                closed: false,
                dropped: 0,
            }),
            ready: Condvar::new(),
            capacity: capacity.max(1),
        }
    }

    /// Pushes a frame, returning the item it displaced (if any).
    pub fn push(&self, value: T) -> Option<T> {
        let mut state = self.lock();
        if state.closed {
            return Some(value);
        }

        let dropped = if state.items.len() >= self.capacity {
            state.dropped += 1;
            state.items.pop_front()
        } else {
            None
        };
        state.items.push_back(value);
        self.ready.notify_one();
        dropped
    }

    /// Pops the oldest frame without blocking.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pop(&self) -> Option<T> {
        self.lock().items.pop_front()
    }

    /// Pops the oldest frame, waiting up to `timeout`.
    ///
    /// Returns `None` on timeout and when the queue was closed.
    pub fn pop_blocking(&self, timeout: Duration) -> Option<T> {
        let mut state = self.lock();
        if state.items.is_empty() && !state.closed {
            let (next, _) = self
                .ready
                .wait_timeout(state, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }

        state.items.pop_front()
    }

    /// Closes the queue: pending items are discarded and waiters wake.
    pub fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.items.clear();
        self.ready.notify_all();
    }

    /// Frames dropped because the consumer fell behind.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dropped(&self) -> u64 {
        self.lock().dropped
    }

    /// Number of queued frames (tests/diagnostics).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.lock().items.len()
    }

    /// Whether the queue is empty (tests/diagnostics).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the queue was closed.
    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState<T>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Scales a tightly packed RGBA8 frame to `(target_width, target_height)` with
/// nearest-neighbour sampling.
///
/// Returns the input unchanged (moved) at the same size, and an empty buffer
/// for degenerate sizes. The destination buffer is exactly
/// `target_width * target_height * 4` bytes.
pub fn scale_rgba_nearest(
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    target_width: u32,
    target_height: u32,
) -> Vec<u8> {
    if width == 0 || height == 0 || target_width == 0 || target_height == 0 {
        return Vec::new();
    }
    if (width, height) == (target_width, target_height) {
        return pixels;
    }
    if pixels.len() < width as usize * height as usize * 4 {
        return Vec::new();
    }

    let mut scaled = vec![0u8; target_width as usize * target_height as usize * 4];
    for y in 0..target_height {
        let source_y = (u64::from(y) * u64::from(height) / u64::from(target_height)) as usize;
        for x in 0..target_width {
            let source_x = (u64::from(x) * u64::from(width) / u64::from(target_width)) as usize;
            let source = (source_y * width as usize + source_x) * 4;
            let target = (y as usize * target_width as usize + x as usize) * 4;
            scaled[target..target + 4].copy_from_slice(&pixels[source..source + 4]);
        }
    }

    scaled
}

// ---------------------------------------------------------------------------
// Receiver state
// ---------------------------------------------------------------------------

/// Lifecycle of one receiver id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReceiverState {
    /// Created but not enabled; no SDK instance exists.
    #[default]
    Idle,
    /// Enabled; the thread is creating/connecting the SDK receiver.
    Connecting,
    /// Connected and receiving frames.
    Connected,
    /// The source is gone or the SDK refused the receiver; the thread stopped.
    Error,
}

impl ReceiverState {
    /// The lowercase token the UI/mirror carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }
}

/// One receiver's status, as reported to the UI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReceiverStatus {
    /// Current lifecycle state.
    pub state: ReceiverState,
    /// The connected source name, once connected.
    pub source: Option<String>,
    /// Why the receiver stopped, when it did.
    pub error: Option<String>,
    /// Most recent frame width.
    pub width: u32,
    /// Most recent frame height.
    pub height: u32,
    /// Frames captured since the receiver was enabled.
    pub frames: u64,
    /// Bumped on every observable change (not on every frame).
    pub revision: u64,
}

/// The public view of a receiver id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiverInfo {
    /// Receiver id (the store generates it; `ndi:<id>` is the texture id).
    pub id: String,
    /// The requested source name.
    pub source_name: String,
    /// The requested source address (diagnostics / display).
    pub url_address: String,
    /// Whether the receiver thread should be running.
    pub enabled: bool,
    /// Lifecycle state.
    pub state: ReceiverState,
    /// The connected source name, once connected.
    pub source: Option<String>,
    /// Why the receiver stopped, when it did.
    pub error: Option<String>,
    /// Most recent frame width.
    pub width: u32,
    /// Most recent frame height.
    pub height: u32,
    /// Frames captured since enabled.
    pub frames: u64,
}

impl ReceiverInfo {
    /// The stable texture/auxiliary id of the receiver's latest frame.
    pub fn texture_id(&self) -> String {
        format!("ndi:{}", self.id)
    }

    /// The mirror shape (`ndi/ADD_RECIEVER` / `ndi/UPDATE_RECIEVER`).
    ///
    /// The JS record is `{ id, outputId, receiver, enabled }` where `receiver`
    /// is the grandiose instance; the engine has no JS receiver object, so the
    /// mirror gets the source descriptor plus the engine's status fields. The
    /// UI reads `receiver.receiver.source.name`, `receiver.enabled`,
    /// `receiver.error` and `receiver.state`.
    pub fn to_mirror_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "outputId": self.texture_id(),
            "receiver": {
                "source": {
                    "name": self.source_name,
                    "urlAddress": self.url_address,
                },
            },
            "enabled": self.enabled,
            "state": self.state.as_str(),
            "source": self.source,
            "error": self.error,
            "width": self.width,
            "height": self.height,
            "frames": self.frames,
        })
    }
}

/// The latest frame of one receiver.
///
/// Publishing replaces the previous frame and allocates a fresh sequence, so a
/// consumer (the compositor) can tell whether it has already uploaded the
/// current frame. A slow consumer never makes the receiver block: it simply
/// misses intermediate frames, and the newest frame wins.
#[derive(Debug, Default)]
pub struct FrameSlot {
    latest: Mutex<Option<Arc<VideoFrame>>>,
    sequence: AtomicU64,
}

impl FrameSlot {
    /// An empty slot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the slot with `frame`, assigning a new sequence.
    pub fn publish(&self, width: u32, height: u32, rgba: Vec<u8>) -> u64 {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = VideoFrame {
            width,
            height,
            rgba,
            sequence,
        };
        let mut latest = self
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *latest = Some(Arc::new(frame));
        sequence
    }

    /// The latest frame if its sequence is newer than `after`.
    pub fn latest_after(&self, after: u64) -> Option<Arc<VideoFrame>> {
        let latest = self
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        latest
            .as_ref()
            .filter(|frame| frame.sequence > after)
            .cloned()
    }

    /// Empties the slot (receiver disabled/removed).
    pub fn clear(&self) {
        let mut latest = self
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *latest = None;
    }
}

// ---------------------------------------------------------------------------
// SDK abstraction
// ---------------------------------------------------------------------------

/// A frame captured by a [`ReceiverLink`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Tightly packed RGBA8 pixels.
    pub rgba: Vec<u8>,
}

/// An NDI sender instance, created inside the sender thread.
///
/// The real implementation wraps [`ndi::NdiSender`], whose instance holds raw
/// SDK pointers and is therefore `!Send`. The link is created, used and dropped
/// inside the sender thread, so the trait deliberately has no `Send` bound: the
/// boxed link never crosses a thread boundary (only the `Send` transport factory
/// does).
pub trait SenderLink {
    /// Overrides the advertised frame rate (`frameRateN / frameRateD`).
    fn set_frame_rate(&mut self, numerator: i32, denominator: i32);
    /// Submits one tightly packed RGBA8 frame.
    fn send(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String>;
}

/// An NDI receiver instance, created inside the receiver thread.
///
/// `!Send` for the same reason as [`SenderLink`]: the SDK pointer stays on the
/// thread that created it.
pub trait ReceiverLink {
    /// Waits up to `timeout_ms` for a matching source and connects.
    fn connect(&mut self, timeout_ms: u32) -> Result<Option<String>, String>;
    /// Captures the next video frame, waiting up to `timeout_ms`.
    fn capture(&mut self, timeout_ms: u32) -> Result<Option<CapturedFrame>, String>;
}

/// The SDK surface the runtime drives (the transport trait).
///
/// Split behind a trait so the lifecycle/queue/rate logic is testable without
/// libndi: [`LibNdiTransport`] is the real implementation, the unit tests use a
/// fake that records calls and lets the test run the spawned work closures
/// synchronously.
pub trait NdiTransport: Send + Sync + 'static {
    /// Whether libndi could be loaded.
    fn available(&self) -> bool;
    /// The path libndi was loaded from, for the UI notice.
    fn library_path(&self) -> Option<String>;
    /// Opens a sender named `name` (called on the sender thread).
    fn open_sender(&self, name: &str) -> Result<Box<dyn SenderLink>, String>;
    /// Opens a receiver for `source_name` (called on the receiver thread).
    fn open_receiver(&self, source_name: &str) -> Result<Box<dyn ReceiverLink>, String>;
    /// Enumerates visible sources, waiting up to `timeout_ms`.
    fn discover(&self, show_local: bool, timeout_ms: u32) -> Result<Vec<NdiSourceInfo>, String>;
    /// Runs `work` on a helper thread named `name`.
    ///
    /// Returns the thread's join handle when the transport really spawns a
    /// thread (the runtime joins it on disable/shutdown). Test transports return
    /// `None` and drive the closure themselves.
    fn spawn(
        &self,
        name: &str,
        work: Box<dyn FnOnce() + Send>,
    ) -> Result<Option<JoinHandle<()>>, String>;
}

/// The real libndi transport.
pub struct LibNdiTransport;

impl NdiTransport for LibNdiTransport {
    fn available(&self) -> bool {
        ndi::ndi_available()
    }

    fn library_path(&self) -> Option<String> {
        ndi::ndi_library_path().map(|path| path.display().to_string())
    }

    fn open_sender(&self, name: &str) -> Result<Box<dyn SenderLink>, String> {
        let sender = ndi::NdiSender::create(name, false).map_err(|error| error.to_string())?;
        Ok(Box::new(LibSender { sender }))
    }

    fn open_receiver(&self, source_name: &str) -> Result<Box<dyn ReceiverLink>, String> {
        let receiver =
            ndi::NdiReceiver::create(Some(source_name)).map_err(|error| error.to_string())?;
        Ok(Box::new(LibReceiver { receiver }))
    }

    fn discover(&self, show_local: bool, timeout_ms: u32) -> Result<Vec<NdiSourceInfo>, String> {
        ndi::discover_sources(show_local, timeout_ms).map_err(|error| error.to_string())
    }

    fn spawn(
        &self,
        name: &str,
        work: Box<dyn FnOnce() + Send>,
    ) -> Result<Option<JoinHandle<()>>, String> {
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(work)
            .map(Some)
            .map_err(|error| format!("could not spawn {name}: {error}"))
    }
}

/// [`SenderLink`] over [`ndi::NdiSender`].
struct LibSender {
    sender: ndi::NdiSender,
}

impl SenderLink for LibSender {
    fn set_frame_rate(&mut self, numerator: i32, denominator: i32) {
        self.sender.set_frame_rate(numerator, denominator);
    }

    fn send(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| "the frame width overflows a row stride".to_string())?;
        self.sender
            .send_rgba(width, height, stride, rgba)
            .map_err(|error| error.to_string())
    }
}

/// [`ReceiverLink`] over [`ndi::NdiReceiver`].
struct LibReceiver {
    receiver: ndi::NdiReceiver,
}

impl ReceiverLink for LibReceiver {
    fn connect(&mut self, timeout_ms: u32) -> Result<Option<String>, String> {
        self.receiver
            .connect(timeout_ms)
            .map_err(|error| error.to_string())
    }

    fn capture(&mut self, timeout_ms: u32) -> Result<Option<CapturedFrame>, String> {
        match self.receiver.capture(timeout_ms) {
            Ok(Some(NdiFrame {
                width,
                height,
                pixels,
                ..
            })) => Ok(Some(CapturedFrame {
                width,
                height,
                rgba: pixels,
            })),
            Ok(None) => Ok(None),
            Err(NdiError::ConnectionLost) => Err("the NDI source disappeared".to_string()),
            Err(error) => Err(error.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

/// Report of the sender thread.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SenderStatus {
    /// Whether a sender instance is running.
    pub running: bool,
    /// Why the sender stopped or could not start.
    pub error: Option<String>,
    /// Whether libndi is missing (the sender cannot run at all).
    pub unavailable: bool,
    /// Frames the SDK accepted.
    pub frames_sent: u64,
}

/// What the sender thread has to know about one submission.
struct OutgoingFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    fps_numerator: i32,
    fps_denominator: i32,
}

struct SenderHandle {
    stop: Arc<AtomicBool>,
    queue: Arc<FrameQueue<OutgoingFrame>>,
    thread: Option<JoinHandle<()>>,
}

struct ReceiverEntry {
    id: String,
    source_name: String,
    url_address: String,
    enabled: bool,
    status: Arc<Mutex<ReceiverStatus>>,
    stop: Arc<AtomicBool>,
    slot: Arc<FrameSlot>,
    thread: Option<JoinHandle<()>>,
    /// Last revision reported through [`NdiRuntime::take_updates`].
    observed: u64,
}

/// The result of one discovery run: the visible sources or a failure detail.
type DiscoveryResult = Result<Vec<NdiSourceInfo>, String>;

/// One discovery run.
struct Discovery {
    result: Arc<Mutex<Option<DiscoveryResult>>>,
    running: bool,
}

/// The engine's NDI send/receive state.
pub struct NdiRuntime {
    transport: Arc<dyn NdiTransport>,
    config: NdiConfig,
    clock: FrameClock,
    sender: Option<SenderHandle>,
    sender_status: Arc<Mutex<SenderStatus>>,
    engine_fps: f64,
    receivers: Vec<ReceiverEntry>,
    discovery: Option<Discovery>,
    /// Sources from the most recent successful discovery run.
    sources: Vec<NdiSourceInfo>,
    show_local_sources: bool,
    warned_unavailable: bool,
}

impl Default for NdiRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl NdiRuntime {
    /// The runtime over the real libndi transport.
    pub fn new() -> Self {
        Self::with_transport(Arc::new(LibNdiTransport))
    }

    /// The runtime over `transport` (tests inject a fake).
    pub fn with_transport(transport: Arc<dyn NdiTransport>) -> Self {
        let config = NdiConfig::default().normalised();
        Self {
            transport,
            clock: FrameClock::new(config.output_target_fps),
            config,
            sender: None,
            sender_status: Arc::new(Mutex::new(SenderStatus::default())),
            engine_fps: DEFAULT_OUTPUT_TARGET_FPS,
            receivers: Vec::new(),
            discovery: None,
            sources: Vec::new(),
            show_local_sources: true,
            warned_unavailable: false,
        }
    }

    /// Whether libndi is available.
    ///
    /// Loads the library on first call; a missing SDK is logged once by
    /// `modv_media::ndi` and reported here, never panicked on.
    pub fn available(&self) -> bool {
        self.transport.available()
    }

    /// The path libndi was loaded from, if any.
    pub fn library_path(&self) -> Option<String> {
        self.transport.library_path()
    }

    /// The current configuration.
    pub fn config(&self) -> &NdiConfig {
        &self.config
    }

    /// The sender thread's status.
    pub fn sender_status(&self) -> SenderStatus {
        self.sender_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The measured engine frame rate used when `followModVFps` is set.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn engine_fps(&self) -> f64 {
        self.engine_fps
    }

    /// Updates the measured engine frame rate (called from the metric tick).
    pub fn set_engine_fps(&mut self, fps: f64) {
        if fps.is_finite() && fps > 0.0 {
            self.engine_fps = fps;
            self.refresh_rate();
        }
    }

    /// `ndi/SET_OUTPUT_ENABLED`.
    pub fn set_output_enabled(&mut self, enabled: bool) {
        if self.config.output_enabled == enabled {
            return;
        }
        self.config.output_enabled = enabled;
        if enabled {
            self.start_sender();
        } else {
            self.stop_sender();
        }
    }

    /// `ndi/SET_OUTPUT_NAME`.
    ///
    /// The UI disables the field while the output is on; if it changes anyway
    /// the sender is recreated under the new name.
    pub fn set_output_name(&mut self, name: &str) {
        let name = normalise_output_name(name);
        if self.config.output_name == name {
            return;
        }
        self.config.output_name = name;
        if self.config.output_enabled {
            self.stop_sender();
            self.start_sender();
        }
    }

    /// `ndi/SET_OUTPUT_SIZE` (`width`/`height` are each optional).
    pub fn set_output_size(&mut self, width: Option<u32>, height: Option<u32>) {
        if let Some(width) = width {
            self.config.output_width = width.clamp(1, MAX_OUTPUT_DIMENSION);
        }
        if let Some(height) = height {
            self.config.output_height = height.clamp(1, MAX_OUTPUT_DIMENSION);
        }
    }

    /// `ndi/SET_FOLLOW_MODV_OUTPUT_SIZE`.
    pub fn set_follow_output_size(&mut self, follow: bool) {
        self.config.follow_modv_output_size = follow;
    }

    /// `ndi/SET_FOLLOW_MODV_FPS`.
    pub fn set_follow_fps(&mut self, follow: bool) {
        self.config.follow_modv_fps = follow;
        self.refresh_rate();
    }

    /// `ndi/SET_OUTPUT_TARGET_FPS`.
    pub fn set_target_fps(&mut self, fps: f64) {
        self.config.output_target_fps = normalise_fps(fps);
        self.refresh_rate();
    }

    /// Whether a frame is due at `now`, consuming the slot when it is.
    ///
    /// Returns `false` when the output is disabled, no sender is running, or
    /// the rate limit has not elapsed — so the window loop only pays for the
    /// GPU read-back when the frame will actually be sent.
    pub fn frame_due(&mut self, now: Instant) -> bool {
        if !self.config.output_enabled || self.sender.is_none() {
            return false;
        }
        if !self.sender_status().running {
            return false;
        }

        self.clock.due(now)
    }

    /// Submits the presented frame to the sender.
    ///
    /// `width`/`height` are the compositor size; the frame is scaled when
    /// `followModVOutputSize` is off. `rgba` must be tightly packed RGBA8.
    /// Returns whether a frame was queued.
    pub fn submit_frame(&mut self, width: u32, height: u32, rgba: &[u8]) -> bool {
        let Some(sender) = &self.sender else {
            return false;
        };
        if !self.config.output_enabled || rgba.len() != width as usize * height as usize * 4 {
            return false;
        }

        let source = (width, height);
        let target = self.config.output_size(source);
        let pixels = if target == source {
            rgba.to_vec()
        } else {
            scale_rgba_nearest(width, height, rgba.to_vec(), target.0, target.1)
        };
        if pixels.is_empty() {
            return false;
        }

        let fps = self.config.target_fps(self.engine_fps);
        // A dropped frame is the bounded-queue contract, not an error: the
        // newest frame always wins when the SDK falls behind.
        sender.queue.push(OutgoingFrame {
            width: target.0,
            height: target.1,
            pixels,
            fps_numerator: (fps * 1000.0).round().max(1.0) as i32,
            fps_denominator: 1000,
        });

        true
    }

    /// Starts the sender thread if the output is enabled and libndi is there.
    fn start_sender(&mut self) {
        if self.sender.is_some() {
            return;
        }

        if !self.available() {
            *self.status_lock() = SenderStatus {
                running: false,
                error: Some("the NDI SDK (libndi) is not available".to_string()),
                unavailable: true,
                frames_sent: 0,
            };
            self.warn_unavailable("NDI output");
            return;
        }

        let queue = Arc::new(FrameQueue::new(SENDER_QUEUE_CAPACITY));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::clone(&self.sender_status);
        *status.lock().unwrap_or_else(|p| p.into_inner()) = SenderStatus::default();

        let transport = Arc::clone(&self.transport);
        let name = self.config.output_name.clone();
        let thread_queue = Arc::clone(&queue);
        let thread_stop = Arc::clone(&stop);
        let thread_status = Arc::clone(&status);

        let work = Box::new(move || {
            run_sender(transport, name, thread_queue, thread_stop, thread_status);
        });

        match self.transport.spawn("ndi-sender", work) {
            Ok(handle) => {
                self.sender = Some(SenderHandle {
                    stop,
                    queue,
                    thread: handle,
                });
            }
            Err(error) => {
                *status.lock().unwrap_or_else(|p| p.into_inner()) = SenderStatus {
                    running: false,
                    error: Some(error.clone()),
                    unavailable: false,
                    frames_sent: 0,
                };
                eprintln!("engine: WARN could not start the NDI sender thread: {error}");
            }
        }
    }

    /// Stops the sender thread (idempotent).
    fn stop_sender(&mut self) {
        let Some(mut sender) = self.sender.take() else {
            self.clock.reset();
            return;
        };

        sender.stop.store(true, Ordering::Relaxed);
        sender.queue.close();
        if let Some(thread) = sender.thread.take() {
            let _ = thread.join();
        }
        self.clock.reset();
        self.status_lock().running = false;
    }

    /// Re-derives the frame clock / sender rate after a config change.
    fn refresh_rate(&mut self) {
        self.clock.set_fps(self.config.target_fps(self.engine_fps));
    }

    fn status_lock(&self) -> std::sync::MutexGuard<'_, SenderStatus> {
        self.sender_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn warn_unavailable(&mut self, feature: &str) {
        if !self.warned_unavailable {
            self.warned_unavailable = true;
            eprintln!(
                "engine: WARN {feature} is unavailable: install the NDI SDK or set MODV_NDI_LIB"
            );
        }
    }

    // ————————————————— Receivers —————————————————

    /// Registers a receiver id without starting it (`ndi/createReceiver`).
    ///
    /// Creating a receiver is a registry operation: no SDK instance exists
    /// until [`Self::enable_receiver`], which matches the JS flow (the
    /// grandiose receiver is created eagerly there, but nothing is drawn until
    /// `waitForFrame` runs).
    pub fn create_receiver(
        &mut self,
        id: &str,
        source_name: &str,
        url_address: &str,
    ) -> ReceiverInfo {
        if let Some(existing) = self.receivers.iter().position(|entry| entry.id == id) {
            self.receivers.remove(existing);
        }

        let mut status = ReceiverStatus::default();
        if !self.available() {
            status.state = ReceiverState::Error;
            status.error = Some("the NDI SDK (libndi) is not available".to_string());
            status.revision = 1;
            self.warn_unavailable("NDI receiving");
        }

        let entry = ReceiverEntry {
            id: id.to_string(),
            source_name: source_name.to_string(),
            url_address: url_address.to_string(),
            enabled: false,
            status: Arc::new(Mutex::new(status)),
            stop: Arc::new(AtomicBool::new(false)),
            slot: Arc::new(FrameSlot::new()),
            thread: None,
            observed: 0,
        };
        let info = entry.info();
        self.receivers.push(entry);
        info
    }

    /// Enables a receiver, spawning its thread (`ndi/enableReceiver`).
    pub fn enable_receiver(&mut self, id: &str) -> Option<ReceiverInfo> {
        let index = self.receivers.iter().position(|entry| entry.id == id)?;
        if self.receivers[index].enabled {
            return Some(self.receivers[index].info());
        }

        {
            let entry = &mut self.receivers[index];
            entry.enabled = true;
            entry.stop = Arc::new(AtomicBool::new(false));
            entry.slot.clear();
            entry.observed = 0;
            *entry.status_lock() = ReceiverStatus {
                state: ReceiverState::Connecting,
                revision: 1,
                ..ReceiverStatus::default()
            };
        }

        if !self.available() {
            {
                let entry = &mut self.receivers[index];
                let mut status = entry.status_lock();
                status.state = ReceiverState::Error;
                status.error = Some("the NDI SDK (libndi) is not available".to_string());
                status.revision += 1;
            }
            self.warn_unavailable("NDI receiving");
            return Some(self.receivers[index].info());
        }

        let (transport, source_name, status, stop, slot) = {
            let entry = &self.receivers[index];
            (
                Arc::clone(&self.transport),
                entry.source_name.clone(),
                Arc::clone(&entry.status),
                Arc::clone(&entry.stop),
                Arc::clone(&entry.slot),
            )
        };

        let work = Box::new(move || {
            run_receiver(transport, source_name, stop, slot, status);
        });

        match self.transport.spawn("ndi-receiver", work) {
            Ok(handle) => self.receivers[index].thread = handle,
            Err(error) => {
                let entry = &mut self.receivers[index];
                let mut status = entry.status_lock();
                status.state = ReceiverState::Error;
                status.error = Some(error.clone());
                status.revision += 1;
                eprintln!("engine: WARN could not start the NDI receiver thread: {error}");
            }
        }

        Some(self.receivers[index].info())
    }

    /// Disables a receiver, stopping its thread (`ndi/disableReceiver`).
    ///
    /// The last frame is dropped so a disabled receiver stops feeding texture
    /// props (the compositor also drops the `ndi:<id>` texture).
    pub fn disable_receiver(&mut self, id: &str) -> Option<ReceiverInfo> {
        let index = self.receivers.iter().position(|entry| entry.id == id)?;

        {
            let entry = &mut self.receivers[index];
            entry.enabled = false;
            entry.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = entry.thread.take() {
                let _ = thread.join();
            }
            entry.slot.clear();
            let mut status = entry.status_lock();
            let revision = status.revision + 1;
            *status = ReceiverStatus {
                state: ReceiverState::Idle,
                revision,
                ..ReceiverStatus::default()
            };
        }

        Some(self.receivers[index].info())
    }

    /// Removes a receiver id (`ndi/removeReceiver`).
    pub fn remove_receiver(&mut self, id: &str) -> Option<ReceiverInfo> {
        let index = self.receivers.iter().position(|entry| entry.id == id)?;
        let info = self.disable_receiver(id)?;
        self.receivers.remove(index);
        Some(info)
    }

    /// One receiver's current info.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn receiver(&self, id: &str) -> Option<ReceiverInfo> {
        self.receivers
            .iter()
            .find(|entry| entry.id == id)
            .map(ReceiverEntry::info)
    }

    /// Every receiver, in creation order.
    pub fn receivers(&self) -> Vec<ReceiverInfo> {
        self.receivers.iter().map(ReceiverEntry::info).collect()
    }

    /// Enabled receiver ids, in creation order.
    pub fn enabled_receiver_ids(&self) -> Vec<String> {
        self.receivers
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// The frame slot of an enabled receiver.
    pub fn slot(&self, id: &str) -> Option<Arc<FrameSlot>> {
        self.receivers
            .iter()
            .find(|entry| entry.id == id && entry.enabled)
            .map(|entry| Arc::clone(&entry.slot))
    }

    /// Receiver ids whose observable status changed since the last call.
    ///
    /// The window loop drains this once per frame and emits the mirror's
    /// `ndi/UPDATE_RECIEVER`; a silent receiver produces no delta.
    pub fn take_updates(&mut self) -> Vec<ReceiverInfo> {
        let mut updates = Vec::new();

        for entry in &mut self.receivers {
            let revision = entry.status_lock().revision;
            if revision > entry.observed {
                entry.observed = revision;
                updates.push(entry.info());
            }
        }

        updates
    }

    // ————————————————— Discovery —————————————————

    /// Whether a discovery run is in flight.
    pub fn discovering(&self) -> bool {
        self.discovery.as_ref().is_some_and(|run| run.running)
    }

    /// The `showLocalSources` discovery option (`ndi/setDiscoveryOptions`).
    pub fn set_show_local_sources(&mut self, show_local: bool) {
        self.show_local_sources = show_local;
    }

    /// The `showLocalSources` discovery option.
    pub fn show_local_sources(&self) -> bool {
        self.show_local_sources
    }

    /// The sources the most recent successful discovery run reported.
    pub fn sources(&self) -> &[NdiSourceInfo] {
        &self.sources
    }

    /// Starts an asynchronous source discovery (`ndi/discoverSources`).
    ///
    /// Returns `false` when libndi is missing or a run is already in flight.
    /// The result is collected by [`Self::take_discovery`], so the finder wait
    /// (up to [`DISCOVERY_TIMEOUT_MS`]) never stalls the frame loop.
    pub fn start_discovery(&mut self) -> bool {
        if self.discovering() {
            return false;
        }
        if !self.available() {
            self.warn_unavailable("NDI discovery");
            return false;
        }

        let result = Arc::new(Mutex::new(None));
        let thread_result = Arc::clone(&result);
        let transport = Arc::clone(&self.transport);
        let show_local = self.show_local_sources;

        let work = Box::new(move || {
            let sources = transport.discover(show_local, DISCOVERY_TIMEOUT_MS);
            *thread_result
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sources);
        });

        match self.transport.spawn("ndi-discovery", work) {
            Ok(_handle) => {
                self.discovery = Some(Discovery {
                    result,
                    running: true,
                });
                true
            }
            Err(error) => {
                eprintln!("engine: WARN could not start NDI discovery: {error}");
                false
            }
        }
    }

    /// The completed discovery result, if the run finished.
    ///
    /// `Some(Err(message))` for a failed run; a successful run returns the
    /// sources. The discovery state is cleared, so only one caller observes it.
    pub fn take_discovery(&mut self) -> Option<DiscoveryResult> {
        let run = self.discovery.as_mut()?;
        let completed = run
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let sources = completed?;
        self.discovery = None;
        Some(sources)
    }

    /// Stops every thread (window shutdown).
    pub fn shutdown(&mut self) {
        self.stop_sender();
        for index in 0..self.receivers.len() {
            let id = self.receivers[index].id.clone();
            let _ = self.disable_receiver(&id);
        }
        self.discovery = None;
    }
}

impl Drop for NdiRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Test-only registry helpers.
///
/// The compositor tests need a receiver frame without libndi (and without
/// spawning threads), so they install a connected receiver and publish pixels
/// directly. Production code never enables a receiver without the SDK.
#[cfg(test)]
impl NdiRuntime {
    /// Registers an enabled, connected receiver holding `rgba` as its frame.
    pub fn test_install_receiver_frame(
        &mut self,
        id: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) {
        let slot = Arc::new(FrameSlot::new());
        slot.publish(width, height, rgba);
        let entry = ReceiverEntry {
            id: id.to_string(),
            source_name: id.to_string(),
            url_address: String::new(),
            enabled: true,
            status: Arc::new(Mutex::new(ReceiverStatus {
                state: ReceiverState::Connected,
                source: Some(id.to_string()),
                revision: 1,
                width,
                height,
                frames: 1,
                ..ReceiverStatus::default()
            })),
            stop: Arc::new(AtomicBool::new(false)),
            slot,
            thread: None,
            observed: 0,
        };
        self.receivers.retain(|existing| existing.id != id);
        self.receivers.push(entry);
    }
}

impl ReceiverEntry {
    fn status_lock(&self) -> std::sync::MutexGuard<'_, ReceiverStatus> {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn info(&self) -> ReceiverInfo {
        let status = self.status_lock();
        ReceiverInfo {
            id: self.id.clone(),
            source_name: self.source_name.clone(),
            url_address: self.url_address.clone(),
            enabled: self.enabled,
            state: status.state,
            source: status.source.clone(),
            error: status.error.clone(),
            width: status.width,
            height: status.height,
            frames: status.frames,
        }
    }
}

// ---------------------------------------------------------------------------
// Worker loops
// ---------------------------------------------------------------------------

/// The sender thread: waits for queued frames and hands them to the SDK.
///
/// The SDK is opened here (never on the render thread: `NdiSender` holds raw
/// SDK pointers and is `!Send`). A failed send means the instance is gone, so
/// the thread records the error and exits instead of retrying.
fn run_sender(
    transport: Arc<dyn NdiTransport>,
    name: String,
    queue: Arc<FrameQueue<OutgoingFrame>>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<SenderStatus>>,
) {
    let mut link = match transport.open_sender(&name) {
        Ok(link) => link,
        Err(error) => {
            *status.lock().unwrap_or_else(|p| p.into_inner()) = SenderStatus {
                running: false,
                error: Some(error),
                unavailable: false,
                frames_sent: 0,
            };
            return;
        }
    };

    {
        let mut guard = status.lock().unwrap_or_else(|p| p.into_inner());
        guard.running = true;
        guard.error = None;
    }
    eprintln!("engine: INFO NDI output '{name}' is available");

    while !stop.load(Ordering::Relaxed) {
        let Some(frame) = queue.pop_blocking(Duration::from_millis(50)) else {
            // A closed, drained queue means the runtime is stopping; without
            // this check the non-blocking `None` of a closed queue would spin.
            if stop.load(Ordering::Relaxed) || queue.is_closed() {
                break;
            }
            continue;
        };

        link.set_frame_rate(frame.fps_numerator, frame.fps_denominator);
        if let Err(error) = link.send(frame.width, frame.height, &frame.pixels) {
            let mut guard = status.lock().unwrap_or_else(|p| p.into_inner());
            guard.running = false;
            guard.error = Some(error.clone());
            eprintln!("engine: WARN NDI output stopped: {error}");
            return;
        }

        status.lock().unwrap_or_else(|p| p.into_inner()).frames_sent += 1;
    }

    let mut guard = status.lock().unwrap_or_else(|p| p.into_inner());
    guard.running = false;
}

/// The receiver thread: opens and connects the SDK receiver, then pumps it.
fn run_receiver(
    transport: Arc<dyn NdiTransport>,
    source_name: String,
    stop: Arc<AtomicBool>,
    slot: Arc<FrameSlot>,
    status: Arc<Mutex<ReceiverStatus>>,
) {
    let mut link = match transport.open_receiver(&source_name) {
        Ok(link) => link,
        Err(error) => {
            set_receiver_error(&status, error);
            return;
        }
    };

    pump_receiver(
        link.as_mut(),
        &stop,
        &slot,
        &status,
        RECEIVER_CONNECT_TIMEOUT_MS,
        RECEIVER_CAPTURE_TIMEOUT_MS,
    );

    if stop.load(Ordering::Relaxed) {
        let mut guard = lock_status(&status);
        if guard.state != ReceiverState::Error {
            guard.state = ReceiverState::Idle;
            guard.revision += 1;
        }
    }
}

/// Connects `link` and pumps frames until `stop` is set or the source is lost.
///
/// Extracted from [`run_receiver`] so the whole lifecycle (connect result,
/// frames published to the slot, error/stop transitions) is unit testable with
/// a scripted [`ReceiverLink`] and no threads.
pub fn pump_receiver(
    link: &mut dyn ReceiverLink,
    stop: &AtomicBool,
    slot: &FrameSlot,
    status: &Mutex<ReceiverStatus>,
    connect_timeout_ms: u32,
    capture_timeout_ms: u32,
) {
    {
        let mut guard = lock_status(status);
        guard.state = ReceiverState::Connecting;
        guard.error = None;
        guard.revision += 1;
    }

    match link.connect(connect_timeout_ms) {
        Ok(Some(source)) => {
            let mut guard = lock_status(status);
            guard.state = ReceiverState::Connected;
            guard.source = Some(source);
            guard.revision += 1;
        }
        Ok(None) => {
            set_receiver_error(
                status,
                "the NDI source was not found; is it still publishing?".to_string(),
            );
            return;
        }
        Err(error) => {
            set_receiver_error(status, error);
            return;
        }
    }

    while !stop.load(Ordering::Relaxed) {
        match link.capture(capture_timeout_ms) {
            Ok(Some(frame)) => {
                slot.publish(frame.width, frame.height, frame.rgba);

                let mut guard = lock_status(status);
                guard.frames += 1;
                // Only geometry changes need a mirror delta; the frame count
                // is diagnostic and would otherwise churn the store every frame.
                if (guard.width, guard.height) != (frame.width, frame.height) {
                    guard.width = frame.width;
                    guard.height = frame.height;
                    guard.revision += 1;
                }
            }
            Ok(None) => {}
            Err(error) => {
                set_receiver_error(status, error);
                return;
            }
        }
    }
}

fn set_receiver_error(status: &Mutex<ReceiverStatus>, error: String) {
    let mut guard = lock_status(status);
    guard.state = ReceiverState::Error;
    guard.error = Some(error);
    guard.revision += 1;
}

fn lock_status(status: &Mutex<ReceiverStatus>) -> std::sync::MutexGuard<'_, ReceiverStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    //! Unit tests for the NDI runtime. libndi is not installed in CI, so the
    //! SDK surface is the [`NdiTransport`] trait: `FakeTransport` records sender /
    //! receiver opens, scripts receiver frames, and parks the spawned work
    //! closures so a test can run them synchronously. The unavailable path is
    //! asserted explicitly with `available: false`.
    //!
    //! What needs a real NDI sender/receiver (manual verification):
    //! * `LibNdiTransport` actually loading libndi and creating instances;
    //! * a sender visible on another NDI machine and advertises the configured
    //!   name/frame rate;
    //! * a receiver connecting to a live source and painting frames through an
    //!   ISF texture prop (`ndi:<receiverId>`), and reporting `error` when the
    //!   publisher stops.

    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SentFrame {
        width: u32,
        height: u32,
        bytes: usize,
        fps_numerator: i32,
    }

    struct FakeSenderLink {
        sent: Arc<StdMutex<Vec<SentFrame>>>,
        fps_numerator: i32,
    }

    impl SenderLink for FakeSenderLink {
        fn set_frame_rate(&mut self, numerator: i32, _denominator: i32) {
            self.fps_numerator = numerator;
        }

        fn send(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
            self.sent.lock().unwrap().push(SentFrame {
                width,
                height,
                bytes: rgba.len(),
                fps_numerator: self.fps_numerator,
            });
            Ok(())
        }
    }

    struct FakeReceiverLink {
        connect: Result<Option<String>, String>,
        script: VecDeque<Result<Option<CapturedFrame>, String>>,
        stop: Arc<AtomicBool>,
    }

    impl ReceiverLink for FakeReceiverLink {
        fn connect(&mut self, _timeout_ms: u32) -> Result<Option<String>, String> {
            self.connect.clone()
        }

        fn capture(&mut self, _timeout_ms: u32) -> Result<Option<CapturedFrame>, String> {
            match self.script.pop_front() {
                Some(frame) => frame,
                // Script exhausted: stop the pump so a test never spins.
                None => {
                    self.stop.store(true, Ordering::Relaxed);
                    Ok(None)
                }
            }
        }
    }

    #[derive(Default)]
    struct FakeTransport {
        available: bool,
        library_path: Option<String>,
        sources: Vec<NdiSourceInfo>,
        sender_opens: StdMutex<Vec<String>>,
        receiver_opens: StdMutex<Vec<String>>,
        sent: Arc<StdMutex<Vec<SentFrame>>>,
        sender_fail: StdMutex<Option<String>>,
        receiver_connect: StdMutex<Option<Result<Option<String>, String>>>,
        receiver_script: StdMutex<VecDeque<Result<Option<CapturedFrame>, String>>>,
        spawns: StdMutex<Vec<Box<dyn FnOnce() + Send>>>,
    }

    impl FakeTransport {
        fn with_available(mut self) -> Self {
            self.available = true;
            self
        }

        fn with_connect(self, connect: Result<Option<String>, String>) -> Self {
            *self.receiver_connect.lock().unwrap() = Some(connect);
            self
        }

        fn run_spawned(&self) -> usize {
            let work = self.spawns.lock().unwrap().pop();
            let Some(work) = work else {
                return 0;
            };
            work();
            1
        }
    }

    impl NdiTransport for FakeTransport {
        fn available(&self) -> bool {
            self.available
        }

        fn library_path(&self) -> Option<String> {
            self.library_path.clone()
        }

        fn open_sender(&self, name: &str) -> Result<Box<dyn SenderLink>, String> {
            self.sender_opens.lock().unwrap().push(name.to_string());
            if let Some(error) = self.sender_fail.lock().unwrap().clone() {
                return Err(error);
            }
            Ok(Box::new(FakeSenderLink {
                sent: Arc::clone(&self.sent),
                fps_numerator: 0,
            }))
        }

        fn open_receiver(&self, source_name: &str) -> Result<Box<dyn ReceiverLink>, String> {
            self.receiver_opens
                .lock()
                .unwrap()
                .push(source_name.to_string());
            Ok(Box::new(FakeReceiverLink {
                connect: self
                    .receiver_connect
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or(Ok(Some(source_name.to_string()))),
                script: self.receiver_script.lock().unwrap().clone(),
                stop: Arc::new(AtomicBool::new(false)),
            }))
        }

        fn discover(
            &self,
            _show_local: bool,
            _timeout_ms: u32,
        ) -> Result<Vec<NdiSourceInfo>, String> {
            Ok(self.sources.clone())
        }

        fn spawn(
            &self,
            _name: &str,
            work: Box<dyn FnOnce() + Send>,
        ) -> Result<Option<JoinHandle<()>>, String> {
            self.spawns.lock().unwrap().push(work);
            Ok(None)
        }
    }

    fn frame(width: u32, height: u32, value: u8) -> CapturedFrame {
        CapturedFrame {
            width,
            height,
            rgba: vec![value; width as usize * height as usize * 4],
        }
    }

    /// A runtime over `transport`, keeping the concrete handle for assertions.
    fn install(transport: &Arc<FakeTransport>) -> NdiRuntime {
        let shared: Arc<dyn NdiTransport> = transport.clone();
        NdiRuntime::with_transport(shared)
    }

    // ————————————————— config —————————————————

    #[test]
    fn config_defaults_match_the_js_store() {
        let config = NdiConfig::default();

        assert!(!config.output_enabled);
        assert_eq!(config.output_name, "VmodV");
        assert_eq!((config.output_width, config.output_height), (1920, 1080));
        assert!(config.follow_modv_output_size);
        assert!(config.follow_modv_fps);
        assert_eq!(config.output_target_fps, 60.0);
    }

    #[test]
    fn config_clamps_sizes_names_and_frame_rates() {
        let config = NdiConfig {
            output_name: "   ".to_string(),
            output_width: 0,
            output_height: 100_000,
            output_target_fps: f64::NAN,
            ..NdiConfig::default()
        }
        .normalised();

        assert_eq!(config.output_name, "VmodV");
        assert_eq!(config.output_width, 1);
        assert_eq!(config.output_height, MAX_OUTPUT_DIMENSION);
        assert_eq!(config.output_target_fps, DEFAULT_OUTPUT_TARGET_FPS);

        let clamped = NdiConfig {
            output_width: 400,
            output_height: 300,
            output_target_fps: 10_000.0,
            ..NdiConfig::default()
        }
        .normalised();
        assert_eq!((clamped.output_width, clamped.output_height), (400, 300));
        assert_eq!(clamped.output_target_fps, MAX_TARGET_FPS);

        // A real name is trimmed, not replaced.
        assert_eq!(
            NdiConfig {
                output_name: "  Studio  ".to_string(),
                ..NdiConfig::default()
            }
            .normalised()
            .output_name,
            "Studio"
        );
    }

    #[test]
    fn target_fps_follows_the_engine_or_the_target() {
        let mut config = NdiConfig {
            follow_modv_fps: true,
            ..NdiConfig::default()
        };
        assert_eq!(config.target_fps(144.0), 144.0);
        // An invalid measurement falls back to the default rate.
        assert_eq!(config.target_fps(f64::NAN), DEFAULT_OUTPUT_TARGET_FPS);

        config.follow_modv_fps = false;
        config.output_target_fps = 30.0;
        assert_eq!(config.target_fps(144.0), 30.0);

        config.output_target_fps = 0.0;
        assert_eq!(config.target_fps(60.0), DEFAULT_OUTPUT_TARGET_FPS);
    }

    #[test]
    fn output_size_follows_the_compositor_or_the_configured_size() {
        let mut config = NdiConfig::default();
        assert_eq!(config.output_size((640, 360)), (640, 360));

        config.follow_modv_output_size = false;
        config.output_width = 1280;
        config.output_height = 720;
        assert_eq!(config.output_size((640, 360)), (1280, 720));

        // A zero-sized compositor is never propagated into the SDK.
        config.follow_modv_output_size = true;
        assert_eq!(config.output_size((0, 0)), (1, 1));
    }

    // ————————————————— rate limiting —————————————————

    #[test]
    fn frame_clock_limits_the_send_rate() {
        let start = Instant::now();
        let mut clock = FrameClock::new(10.0);

        assert!(clock.due(start), "the first frame is always due");
        assert!(!clock.due(start + Duration::from_millis(50)));
        assert!(clock.due(start + Duration::from_millis(100)));
        assert!(!clock.due(start + Duration::from_millis(150)));
        assert!(clock.due(start + Duration::from_millis(250)));

        // A long stall is due immediately and does not accumulate credit.
        assert!(clock.due(start + Duration::from_secs(5)));
        assert!(!clock.due(start + Duration::from_millis(5_050)));
    }

    #[test]
    fn frame_clock_rejects_invalid_rates() {
        let clock = FrameClock::new(0.0);
        assert_eq!(clock.fps(), DEFAULT_OUTPUT_TARGET_FPS);

        let clock = FrameClock::new(f64::INFINITY);
        assert_eq!(clock.fps(), DEFAULT_OUTPUT_TARGET_FPS);
    }

    // ————————————————— bounded queue —————————————————

    #[test]
    fn bounded_queue_drops_the_oldest_frame() {
        let queue = FrameQueue::new(SENDER_QUEUE_CAPACITY);

        assert!(queue.push(1).is_none());
        assert!(queue.push(2).is_none());
        // Capacity reached: the oldest frame is displaced, not the newest.
        assert_eq!(queue.push(3), Some(1));
        assert_eq!(queue.push(4), Some(2));
        assert_eq!(queue.len(), SENDER_QUEUE_CAPACITY);
        assert_eq!(queue.dropped(), 2);

        assert_eq!(queue.pop(), Some(3));
        assert_eq!(queue.pop(), Some(4));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn a_closed_queue_refuses_frames_and_wakes_waiters() {
        let queue = FrameQueue::new(2);
        queue.push(1);
        queue.close();

        assert!(queue.is_closed());
        assert!(queue.is_empty(), "closing discards pending frames");
        assert_eq!(queue.push(2), Some(2));
        assert!(
            queue.pop_blocking(Duration::from_millis(10)).is_none(),
            "a closed queue returns immediately"
        );
    }

    // ————————————————— scaling —————————————————

    #[test]
    fn nearest_neighbour_scaling_keeps_the_corners() {
        // 2×2: red top-left, green top-right, blue bottom-left, white bottom-right.
        let source = vec![
            255, 0, 0, 255, 0, 255, 0, 255, // row 0
            0, 0, 255, 255, 255, 255, 255, 255, // row 1
        ];
        let scaled = scale_rgba_nearest(2, 2, source.clone(), 4, 4);
        assert_eq!(scaled.len(), 4 * 4 * 4);
        assert_eq!(&scaled[0..4], &[255, 0, 0, 255], "top-left red");
        assert_eq!(&scaled[12..16], &[0, 255, 0, 255], "top-right green");
        assert_eq!(&scaled[48..52], &[0, 0, 255, 255], "bottom-left blue");
        assert_eq!(&scaled[60..64], &[255, 255, 255, 255], "bottom-right white");

        // Downscaling picks the sampled pixels; the same size is a no-op.
        assert_eq!(
            scale_rgba_nearest(2, 2, source.clone(), 1, 1),
            vec![255, 0, 0, 255]
        );
        assert_eq!(scale_rgba_nearest(2, 2, source.clone(), 2, 2), source);
        assert!(scale_rgba_nearest(0, 2, source, 4, 4).is_empty());
    }

    // ————————————————— receiver pump —————————————————

    #[test]
    fn receiver_pump_publishes_frames_and_stops_on_loss() {
        let stop = AtomicBool::new(false);
        let slot = FrameSlot::new();
        let status = Mutex::new(ReceiverStatus::default());
        let mut link = FakeReceiverLink {
            connect: Ok(Some("STUDIO (Test Pattern)".to_string())),
            script: VecDeque::from(vec![
                Ok(Some(frame(4, 2, 0x11))),
                Ok(Some(frame(4, 2, 0x22))),
                Err("the NDI source disappeared".to_string()),
            ]),
            stop: Arc::new(AtomicBool::new(false)),
        };

        pump_receiver(&mut link, &stop, &slot, &status, 10, 10);

        let latest = slot.latest_after(0).expect("a frame was published");
        assert_eq!((latest.width, latest.height), (4, 2));
        assert_eq!(latest.rgba[0], 0x22, "the newest frame is kept");
        let revision = status.lock().unwrap().revision;

        let guard = status.lock().unwrap();
        assert_eq!(guard.state, ReceiverState::Error);
        assert_eq!(guard.frames, 2);
        assert_eq!(guard.source.as_deref(), Some("STUDIO (Test Pattern)"));
        assert!(guard.error.as_deref().unwrap().contains("disappeared"));
        drop(guard);

        // A status revision was published for the error transition.
        assert!(revision >= 3);
    }

    #[test]
    fn receiver_pump_reports_a_missing_source_and_never_captures() {
        let stop = AtomicBool::new(false);
        let slot = FrameSlot::new();
        let status = Mutex::new(ReceiverStatus::default());
        let mut link = FakeReceiverLink {
            connect: Ok(None),
            script: VecDeque::from(vec![Ok(Some(frame(2, 2, 0x11)))]),
            stop: Arc::new(AtomicBool::new(false)),
        };

        pump_receiver(&mut link, &stop, &slot, &status, 10, 10);

        assert!(slot.latest_after(0).is_none(), "nothing was captured");
        let guard = status.lock().unwrap();
        assert_eq!(guard.state, ReceiverState::Error);
        assert_eq!(guard.frames, 0);
        assert!(guard.error.as_deref().unwrap().contains("not found"));
    }

    #[test]
    fn receiver_pump_reports_a_connect_error() {
        let stop = AtomicBool::new(false);
        let slot = FrameSlot::new();
        let status = Mutex::new(ReceiverStatus::default());
        let mut link = FakeReceiverLink {
            connect: Err("the NDI SDK could not create a receiver instance".to_string()),
            script: VecDeque::new(),
            stop: Arc::new(AtomicBool::new(false)),
        };

        pump_receiver(&mut link, &stop, &slot, &status, 10, 10);

        let guard = status.lock().unwrap();
        assert_eq!(guard.state, ReceiverState::Error);
        assert!(guard.error.as_deref().unwrap().contains("could not create"));
    }

    #[test]
    fn frame_slots_only_return_newer_frames() {
        let slot = FrameSlot::new();
        assert!(slot.latest_after(0).is_none());

        let first = slot.publish(2, 2, vec![1; 16]);
        let frame = slot.latest_after(0).expect("first frame");
        assert_eq!(frame.sequence, first);
        assert!(slot.latest_after(first).is_none());

        let second = slot.publish(2, 2, vec![2; 16]);
        assert!(second > first);
        let frame = slot.latest_after(first).expect("second frame");
        assert_eq!(frame.rgba[0], 2);

        slot.clear();
        assert!(slot.latest_after(0).is_none());
    }

    // ————————————————— runtime lifecycle —————————————————

    #[test]
    fn an_unavailable_transport_reports_errors_without_spawning() {
        let transport = Arc::new(FakeTransport::default());
        let shared: Arc<dyn NdiTransport> = transport.clone();
        let mut runtime = NdiRuntime::with_transport(shared);

        assert_eq!(runtime.engine_fps(), DEFAULT_OUTPUT_TARGET_FPS);
        assert!(!runtime.available());

        runtime.set_output_enabled(true);
        assert_eq!(transport.spawns.lock().unwrap().len(), 0);
        let status = runtime.sender_status();
        assert!(!status.running);
        assert!(status.unavailable);
        assert!(status.error.as_deref().unwrap().contains("libndi"));

        let created = runtime.create_receiver("r1", "STUDIO (Test Pattern)", "10.0.0.2:5961");
        assert_eq!(created.state, ReceiverState::Error);
        let enabled = runtime.enable_receiver("r1").expect("receiver exists");
        assert!(enabled.enabled);
        assert_eq!(enabled.state, ReceiverState::Error);
        assert_eq!(transport.spawns.lock().unwrap().len(), 0);

        // Discovery fails without spawning a finder thread.
        assert!(!runtime.start_discovery());
        assert_eq!(transport.spawns.lock().unwrap().len(), 0);
    }

    #[test]
    fn receiver_lifecycle_registers_enables_disables_and_removes() {
        let transport = Arc::new(
            FakeTransport::default()
                .with_available()
                // The discovery list goes stale before the receiver connects.
                .with_connect(Ok(None)),
        );
        let mut runtime = install(&transport);

        let created = runtime.create_receiver("r1", "STUDIO (Test Pattern)", "10.0.0.2:5961");
        assert_eq!(created.state, ReceiverState::Idle);
        assert!(!created.enabled);
        assert_eq!(created.texture_id(), "ndi:r1");
        assert_eq!(runtime.enabled_receiver_ids(), Vec::<String>::new());
        assert_eq!(runtime.receiver("r1"), Some(created.clone()));

        let enabled = runtime.enable_receiver("r1").expect("receiver exists");
        assert!(enabled.enabled);
        assert_eq!(enabled.state, ReceiverState::Connecting);
        assert_eq!(runtime.enabled_receiver_ids(), vec!["r1".to_string()]);
        assert_eq!(transport.spawns.lock().unwrap().len(), 1, "one thread");
        assert_eq!(
            transport.receiver_opens.lock().unwrap().len(),
            0,
            "opened on the thread"
        );

        // Running the parked work connects and captures until the source is
        // lost, which moves the receiver to Error and stops it.
        transport.run_spawned();
        assert_eq!(
            transport.receiver_opens.lock().unwrap().as_slice(),
            &["STUDIO (Test Pattern)".to_string()]
        );
        let updates = runtime.take_updates();
        let error = updates
            .iter()
            .find(|info| info.state == ReceiverState::Error)
            .expect("an error update was produced");
        assert!(error.error.as_deref().unwrap().contains("not found"));
        assert!(
            runtime.take_updates().is_empty(),
            "each update is delivered once"
        );

        let disabled = runtime.disable_receiver("r1").expect("receiver exists");
        assert!(!disabled.enabled);
        assert_eq!(disabled.state, ReceiverState::Idle);
        assert!(runtime.slot("r1").is_none());

        let removed = runtime.remove_receiver("r1").expect("receiver existed");
        assert_eq!(removed.id, "r1");
        assert!(runtime.receivers().is_empty());
        assert!(runtime.disable_receiver("r1").is_none());
    }

    #[test]
    fn enabling_a_missing_receiver_is_a_noop() {
        let transport = Arc::new(FakeTransport::default().with_available());
        let mut runtime = install(&transport);

        assert!(runtime.enable_receiver("missing").is_none());
        assert!(runtime.disable_receiver("missing").is_none());
        assert!(runtime.remove_receiver("missing").is_none());
    }

    #[test]
    fn the_sender_thread_submits_queued_frames() {
        let transport = Arc::new(FakeTransport::default().with_available());
        let queue = Arc::new(FrameQueue::new(SENDER_QUEUE_CAPACITY));
        let status = Arc::new(Mutex::new(SenderStatus::default()));
        let stop = Arc::new(AtomicBool::new(false));

        let worker = {
            let transport = Arc::clone(&transport);
            let queue = Arc::clone(&queue);
            let status = Arc::clone(&status);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                run_sender(transport, "VmodV".to_string(), queue, stop, status);
            })
        };

        queue.push(OutgoingFrame {
            width: 4,
            height: 2,
            pixels: vec![7; 4 * 2 * 4],
            fps_numerator: 60_000,
            fps_denominator: 1000,
        });

        // Wait for the frame to be accepted, then stop the thread.
        let deadline = Instant::now() + Duration::from_secs(5);
        while transport.sent.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        stop.store(true, Ordering::Relaxed);
        queue.close();
        worker.join().expect("sender thread joins");

        assert_eq!(
            transport.sender_opens.lock().unwrap().as_slice(),
            &["VmodV".to_string()]
        );
        let sent = transport.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].width, sent[0].height), (4, 2));
        assert_eq!(sent[0].bytes, 4 * 2 * 4);
        assert_eq!(sent[0].fps_numerator, 60_000);
        drop(sent);
        assert!(!status.lock().unwrap().running);
        assert_eq!(status.lock().unwrap().frames_sent, 1);
    }

    #[test]
    fn the_sender_reports_an_open_failure() {
        let transport = Arc::new(FakeTransport::default().with_available());
        *transport.sender_fail.lock().unwrap() = Some("could not create a sender".to_string());
        let queue = Arc::new(FrameQueue::new(1));
        queue.close();
        let status = Arc::new(Mutex::new(SenderStatus::default()));

        run_sender(
            transport,
            "VmodV".to_string(),
            queue,
            Arc::new(AtomicBool::new(false)),
            status.clone(),
        );

        let status = status.lock().unwrap();
        assert!(!status.running);
        assert_eq!(status.error.as_deref(), Some("could not create a sender"));
    }

    #[test]
    fn submitting_scales_to_the_configured_size_and_queues_at_most_two_frames() {
        let transport = Arc::new(FakeTransport::default().with_available());
        let mut runtime = install(&transport);

        // Not enabled: frames are ignored.
        assert!(!runtime.submit_frame(2, 2, &[0; 16]));

        runtime.set_follow_output_size(false);
        runtime.set_output_size(Some(4), Some(4));
        runtime.set_output_enabled(true);
        // The fake transport parks the sender thread; the queue exists as soon as
        // `start_sender` installed its handle.
        assert!(runtime.sender.is_some(), "the sender queue was installed");

        assert!(runtime.submit_frame(2, 2, &[9; 16]));
        assert!(runtime.submit_frame(2, 2, &[9; 16]));
        assert!(runtime.submit_frame(2, 2, &[9; 16]));

        let queue = runtime
            .sender
            .as_ref()
            .map(|sender| Arc::clone(&sender.queue))
            .expect("sender queue");
        assert_eq!(queue.len(), SENDER_QUEUE_CAPACITY);
        assert_eq!(queue.dropped(), 1);

        let frame = queue.pop().expect("queued frame");
        assert_eq!((frame.width, frame.height), (4, 4));
        assert_eq!(frame.pixels.len(), 4 * 4 * 4);
        assert_eq!(frame.fps_numerator, 60_000);
    }

    #[test]
    fn discovery_runs_on_a_helper_and_reports_sources() {
        let transport = Arc::new(FakeTransport::default().with_available());
        let shared: Arc<dyn NdiTransport> = transport.clone();
        let mut runtime = NdiRuntime::with_transport(shared);

        assert!(runtime.start_discovery());
        assert!(runtime.discovering());
        // A second request while one is in flight is refused.
        assert!(!runtime.start_discovery());

        transport.run_spawned();
        let sources = runtime
            .take_discovery()
            .expect("a completed run")
            .expect("sources");
        assert!(sources.is_empty());
        assert!(!runtime.discovering());
        assert!(runtime.take_discovery().is_none());
    }

    #[test]
    fn mirror_payload_matches_the_js_receiver_record() {
        let info = ReceiverInfo {
            id: "receiver-1".to_string(),
            source_name: "STUDIO (Test Pattern)".to_string(),
            url_address: "10.0.0.2:5961".to_string(),
            enabled: true,
            state: ReceiverState::Connected,
            source: Some("STUDIO (Test Pattern)".to_string()),
            error: None,
            width: 1920,
            height: 1080,
            frames: 42,
        };

        let payload = info.to_mirror_json();
        assert_eq!(payload["id"], "receiver-1");
        assert_eq!(payload["outputId"], "ndi:receiver-1");
        assert_eq!(
            payload["receiver"]["source"]["name"],
            "STUDIO (Test Pattern)"
        );
        assert_eq!(payload["enabled"], true);
        assert_eq!(payload["state"], "connected");
        assert_eq!(payload["frames"], 42);
    }
}
