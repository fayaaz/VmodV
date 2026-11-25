//! Native PipeWire monitor-source capture (WS-9, Linux only).
//!
//! Replaces the `pw-record` child process in
//! `src/main/system-audio-pipewire.js`: a `pipewire` crate stream connects to
//! the default server, captures the **default sink's monitor** (the same
//! `stream.capture.sink = true` mode `pw-record` was given), and delivers
//! interleaved f32 chunks through the same
//! [`super::capture::ChunkQueue`] as [`super::capture::AudioCapture`]. The
//! requested shape is 48 kHz / 2 ch / 480 frames; the server may negotiate
//! another supported rate/channel count, which is decoded and reported instead
//! of assumed (see *Format negotiation* below).
//!
//! # Threading model
//!
//! The `pipewire` crate's main loop owns `Rc`-based objects and must run on the
//! thread that created it, so [`PipeWireMonitor::start`] spawns a dedicated
//! thread (`modv-pw-monitor`) that builds the loop/context/stream and then
//! blocks in `MainLoop::run`. The data callback assembles chunks and pushes
//! them into the bounded queue (oldest dropped on overflow), exactly like the
//! cpal path. Dropping the monitor sends a quit message over a
//! `pipewire::channel` receiver attached to the loop and joins the thread, so
//! no loop thread is leaked.
//!
//! The stream is connected **without** `RT_PROCESS`: the callback runs on the
//! main loop thread, which is what makes it safe to lock the shared queue
//! there. `MAP_BUFFERS` gives the callback a mapped byte slice.
//!
//! # Format negotiation (R-2 / R-3)
//!
//! The server picks the final stream format; the `EnumFormat` pod is only a
//! request. `param_changed` is therefore the **only** place a format is
//! accepted:
//!
//! * [`negotiate`] accepts interleaved `F32LE`/`S16LE` at 8 kHz – 192 kHz with
//!   1–8 channels. [`decode_interleaved`] converts S16 to full-scale f32, so a
//!   44.1 kHz S16 monitor is analysed correctly instead of having its `i16`
//!   samples reinterpreted as garbage `f32`s.
//! * The `process` callback **parks** (drops) buffers until a format has been
//!   accepted, so no buffer is ever decoded with the wrong layout.
//! * A rejected format latches a readable error ([`ChunkQueue::error`]) *and*
//!   fails the readiness channel; it stays latched until the next accepted
//!   format (the queue clears it), so callers can distinguish "renegotiated
//!   successfully" from a one-shot transient.
//! * [`PipeWireMonitor::start`] returns only after the server negotiated a
//!   format, and reports the **negotiated** rate/channel count — the analyser
//!   is built from [`PipeWireMonitor::sample_rate`] /
//!   [`PipeWireMonitor::channels`], not from the request.
//!
//! # Errors
//!
//! `start` returns [`CaptureError::ServerUnavailable`] when no PipeWire daemon
//! answers (or negotiates a format) within [`START_TIMEOUT`]; callers (and the
//! smoke test) treat that as "no system audio available on this host" rather
//! than a fatal error. A negotiated format this port cannot decode fails
//! startup with [`CaptureError::ConfigUnsupported`]. Errors raised *after*
//! startup (a later renegotiation) surface through
//! [`PipeWireMonitor::next_chunk`] as [`CaptureError::Stream`].

use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::stream::StreamState;
use spa::param::ParamType;
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;

use super::capture::{CaptureConfig, CaptureError, ChunkAssembler, ChunkQueue};

/// How long [`PipeWireMonitor::start`] waits for the loop thread to negotiate a
/// stream format.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound on [`sources`]' registry roundtrip. A wedged PipeWire daemon
/// must not freeze source enumeration (the Audio panel calls it while building
/// its device list); once the deadline passes, the sources seen so far are
/// returned.
pub const SOURCE_ENUMERATION_TIMEOUT: Duration = Duration::from_secs(2);
/// Stream name shown in `pw-top` / `wpctl`.
pub const STREAM_NAME: &str = "modv-system-audio";
/// Stream name for a targeted `Audio/Source` capture (input devices).
pub const SOURCE_STREAM_NAME: &str = "modv-input";
/// Lowest sample rate the monitor decodes (anything below is a broken server
/// negotiation, not a usable stream).
pub const MIN_SUPPORTED_RATE: u32 = 8_000;
/// Highest sample rate the monitor decodes.
pub const MAX_SUPPORTED_RATE: u32 = 192_000;
/// Highest channel count the monitor decodes (surround monitors are downmixed
/// to their front L/R by [`super::capture::downmix_mono`]).
pub const MAX_SUPPORTED_CHANNELS: u32 = 8;

/// One PipeWire `Audio/Source` node (a microphone or audio interface) that
/// [`sources`] enumerated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInfo {
    /// `node.name`; the value the capture target is set to.
    pub name: String,
    /// `node.description` (falling back to `name`) — what the Audio panel
    /// shows for the row.
    pub description: String,
}

/// Decide whether a registry global is a capturable audio source, and build
/// its [`SourceInfo`]. `None` for sinks, nodes of any other media class, and
/// nodes without a `node.name`. Pure, so the registry roundtrip in
/// [`sources`] stays the only untestable part.
pub fn source_from_props(
    name: Option<&str>,
    media_class: Option<&str>,
    description: Option<&str>,
) -> Option<SourceInfo> {
    if media_class != Some("Audio/Source") {
        return None;
    }

    let name = name?.trim().to_string();

    if name.is_empty() {
        return None;
    }

    Some(SourceInfo {
        description: description
            .map(str::trim)
            .filter(|description| !description.is_empty())
            .unwrap_or(&name)
            .to_string(),
        name,
    })
}

/// Enumerate the server's `Audio/Source` nodes. Returns an empty list when no
/// PipeWire daemon answers (the caller then shows cpal devices only).
///
/// One-shot registry roundtrip: register a listener, sync once, and return
/// when the server has delivered every global, or once
/// [`SOURCE_ENUMERATION_TIMEOUT`] expires (returning what was seen so far), so
/// a wedged server cannot block the caller forever.
pub fn sources() -> Vec<SourceInfo> {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    pw::init();

    let mainloop = match pw::main_loop::MainLoopRc::new(None) {
        Ok(mainloop) => mainloop,
        Err(_) => return Vec::new(),
    };
    let context = match pw::context::ContextRc::new(&mainloop, None) {
        Ok(context) => context,
        Err(_) => return Vec::new(),
    };
    let core = match context.connect_rc(None) {
        Ok(core) => core,
        Err(_) => return Vec::new(),
    };
    let registry = match core.get_registry() {
        Ok(registry) => registry,
        Err(_) => return Vec::new(),
    };

    let found = Rc::new(RefCell::new(Vec::new()));
    let found_clone = found.clone();
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != pw::types::ObjectType::Node {
                return;
            }

            let props = global.props.as_ref();
            let info = source_from_props(
                props.and_then(|props| props.get("node.name")),
                props.and_then(|props| props.get("media.class")),
                props.and_then(|props| props.get("node.description")),
            );

            if let Some(info) = info {
                found_clone.borrow_mut().push(info);
            }
        })
        .register();

    let done = Rc::new(Cell::new(false));
    let done_clone = done.clone();
    let loop_clone = mainloop.clone();
    let pending = match core.sync(0) {
        Ok(pending) => pending,
        Err(_) => return Vec::new(),
    };
    let _core_listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == pending {
                done_clone.set(true);
                loop_clone.quit();
            }
        })
        .register();

    let deadline = Instant::now() + SOURCE_ENUMERATION_TIMEOUT;

    while !done.get() {
        let now = Instant::now();

        if now >= deadline {
            break;
        }

        mainloop
            .loop_()
            .iterate(pw::loop_::Timeout::Finite(deadline - now));
    }

    Rc::try_unwrap(found)
        .map(|found| found.into_inner())
        .unwrap_or_else(|found| found.borrow().clone())
}

/// Sample layout of a negotiated raw-audio stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmFormat {
    /// Interleaved 32-bit float (the requested format).
    F32Le,
    /// Interleaved signed 16-bit.
    S16Le,
}

impl PcmFormat {
    /// Bytes per sample.
    pub fn bytes_per_sample(self) -> usize {
        match self {
            Self::F32Le => 4,
            Self::S16Le => 2,
        }
    }
}

/// A stream format the monitor accepted and knows how to decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NegotiatedFormat {
    /// Sample layout on the wire.
    pub sample: PcmFormat,
    /// Negotiated sample rate in Hz.
    pub rate: u32,
    /// Negotiated channel count (interleaved).
    pub channels: u32,
}

/// Decide whether a negotiated SPA audio format can be decoded.
///
/// Pure so the negotiation rules are testable without a PipeWire server. The
/// request is f32/stereo/48 kHz, but the server may hand back anything; only
/// interleaved `F32LE` and `S16LE` are decodable here (planar layouts and
/// 24/32-bit integer layouts are rejected with a descriptive error rather than
/// silently misinterpreted).
pub fn negotiate(
    format: spa::param::audio::AudioFormat,
    rate: u32,
    channels: u32,
) -> Result<NegotiatedFormat, String> {
    let sample = if format == spa::param::audio::AudioFormat::F32LE {
        PcmFormat::F32Le
    } else if format == spa::param::audio::AudioFormat::S16LE {
        PcmFormat::S16Le
    } else {
        return Err(format!(
            "unsupported PipeWire sample format {format:?} (need F32LE or S16LE interleaved)"
        ));
    };

    if !(MIN_SUPPORTED_RATE..=MAX_SUPPORTED_RATE).contains(&rate) {
        return Err(format!(
            "unsupported PipeWire sample rate {rate} Hz (supported {MIN_SUPPORTED_RATE}-{MAX_SUPPORTED_RATE} Hz)"
        ));
    }

    if !(1..=MAX_SUPPORTED_CHANNELS).contains(&channels) {
        return Err(format!(
            "unsupported PipeWire channel count {channels} (supported 1-{MAX_SUPPORTED_CHANNELS})"
        ));
    }

    Ok(NegotiatedFormat {
        sample,
        rate,
        channels,
    })
}

/// Decode one interleaved buffer of `format` PCM into `f32` samples.
///
/// A trailing partial sample (odd byte count for S16, non-multiple of four for
/// f32) is ignored; [`ChunkAssembler`] then re-blocks frames and drops a
/// dangling partial frame. S16 uses the same full-scale mapping as
/// [`super::capture`]'s cpal conversion (`i16 / 32768`).
///
/// Allocates a fresh `Vec`; the live `process` callback uses
/// [`decode_interleaved_into`] with maintained scratch space instead.
pub fn decode_interleaved(bytes: &[u8], format: PcmFormat) -> Vec<f32> {
    let mut samples = Vec::new();
    decode_interleaved_into(bytes, format, &mut samples);
    samples
}

/// Decode one interleaved buffer into `out`, reusing its allocation.
///
/// **Real-time constraint:** the `process` callback runs on the PipeWire loop
/// thread and must not allocate in steady state (R-22), so `out` lives in the
/// callback user data and is only reserved when a buffer arrives that is
/// bigger than the current capacity (a format/buffer-size change). The
/// capacity is kept afterwards so the next buffer reuses it.
pub fn decode_interleaved_into(bytes: &[u8], format: PcmFormat, out: &mut Vec<f32>) {
    out.clear();

    match format {
        PcmFormat::F32Le => {
            let chunks = bytes.as_chunks::<4>().0;

            if out.capacity() < chunks.len() {
                out.reserve(chunks.len());
            }

            for sample in chunks {
                out.push(f32::from_le_bytes(*sample));
            }
        }
        PcmFormat::S16Le => {
            let chunks = bytes.as_chunks::<2>().0;

            if out.capacity() < chunks.len() {
                out.reserve(chunks.len());
            }

            for sample in chunks {
                out.push(f32::from(i16::from_le_bytes(*sample)) / 32_768.0);
            }
        }
    }
}

/// Latest format the loop thread accepted, readable by the capture handle.
///
/// The `process` callback keeps its own copy (so it never locks while
/// decoding); only [`FormatState::set_format`] / [`FormatState::reject`] write
/// this slot and only [`PipeWireMonitor`]'s accessors read it. The lock is
/// therefore held for a single enum assignment at most.
type SharedFormat = Arc<Mutex<Option<NegotiatedFormat>>>;

/// Format state shared by the `param_changed` and `process` callbacks.
///
/// `param_changed` records the accepted format; `process` decodes nothing
/// until one was accepted (buffers are parked). A rejection latches its
/// message in the queue and fails the readiness channel exactly once, so
/// [`PipeWireMonitor::next_chunk`] can keep reporting it without the error
/// being a one-shot that a single log line swallows.
///
/// Every accepted/rejected negotiation is mirrored into [`Self::shared`] so a
/// [`PipeWireMonitor`] created at startup reports the *current* rate/channel
/// count after the server renegotiates mid-stream.
struct FormatState {
    negotiated: Option<NegotiatedFormat>,
    shared: SharedFormat,
    ready: Option<SyncSender<Result<NegotiatedFormat, CaptureError>>>,
    queue: ChunkQueue,
}

impl FormatState {
    /// Test-only constructor: a private shared slot (the production path always
    /// shares it with the [`PipeWireMonitor`] handle via [`Self::with_shared`]).
    #[cfg(test)]
    fn new(ready: SyncSender<Result<NegotiatedFormat, CaptureError>>, queue: ChunkQueue) -> Self {
        Self::with_shared(ready, queue, SharedFormat::default())
    }

    fn with_shared(
        ready: SyncSender<Result<NegotiatedFormat, CaptureError>>,
        queue: ChunkQueue,
        shared: SharedFormat,
    ) -> Self {
        Self {
            negotiated: None,
            shared,
            ready: Some(ready),
            queue,
        }
    }

    /// The accepted format, if the server has negotiated a usable one.
    fn negotiated(&self) -> Option<NegotiatedFormat> {
        self.negotiated
    }

    /// Mirror the callback-local format into the handle-readable slot. Callers
    /// must not hold the lock across decoding; it is only ever held for the
    /// assignment below.
    fn publish(&self) {
        if let Ok(mut current) = self.shared.lock() {
            *current = self.negotiated;
        }
    }

    /// Records a `SPA_PARAM_Format` outcome. Returns whether the format was
    /// accepted.
    fn set_format(
        &mut self,
        format: spa::param::audio::AudioFormat,
        rate: u32,
        channels: u32,
    ) -> bool {
        match negotiate(format, rate, channels) {
            Ok(negotiated) => {
                self.negotiated = Some(negotiated);
                self.publish();
                // A successful (re)negotiation clears an earlier rejection.
                self.queue.clear_error();
                if let Some(ready) = self.ready.take() {
                    let _ = ready.send(Ok(negotiated));
                }
                true
            }
            Err(message) => {
                self.reject(message);
                false
            }
        }
    }

    /// Latch a negotiation failure: the stream stays parked and the caller is
    /// told once (through the readiness channel, or through
    /// [`PipeWireMonitor::next_chunk`] after startup).
    fn reject(&mut self, message: String) {
        self.negotiated = None;
        self.publish();
        self.queue.set_error(message.clone());
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(Err(CaptureError::ConfigUnsupported(message)));
        }
    }

    /// Decode one buffer into `out`, or `false` while no format is accepted.
    ///
    /// Used by the real-time `process` callback with a scratch buffer it owns,
    /// so decoding does not allocate per buffer (R-22).
    fn decode_into(&self, bytes: &[u8], out: &mut Vec<f32>) -> bool {
        match self.negotiated() {
            Some(negotiated) => {
                decode_interleaved_into(bytes, negotiated.sample, out);
                true
            }
            None => false,
        }
    }
}

/// User data handed to the PipeWire callbacks.
struct UserData {
    format: spa::param::audio::AudioInfoRaw,
    assembler: ChunkAssembler,
    formats: FormatState,
    /// Decode scratch, owned by the loop thread so `process` does not
    /// allocate a `Vec` per buffer.
    scratch: Vec<f32>,
}

/// A running PipeWire monitor capture.
pub struct PipeWireMonitor {
    quit: Option<pw::channel::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    queue: ChunkQueue,
    /// Live format published by the loop thread (updated on renegotiation).
    format: SharedFormat,
    /// First negotiated format; only used while the live slot is empty (a
    /// rejected renegotiation parks the stream).
    startup: NegotiatedFormat,
}

impl PipeWireMonitor {
    /// Connect to the default PipeWire server and start capturing.
    ///
    /// `config.capture_sink` selects the shape: `true` (the default)
    /// captures the default sink's monitor (`stream.capture.sink=true`);
    /// `false` captures an `Audio/Source` node. On PipeWire desktops the
    /// daemon holds the hardware, so a direct cpal/ALSA open of an input
    /// device fails `DeviceNotAvailable`; targeting the source node through
    /// PipeWire is the reliable route for an input device instead.
    ///
    /// `config.device` is an optional `target.object`: a sink `node.name`
    /// for a monitor capture, or a source `node.name` for a source capture.
    /// `None` follows the default sink (or the default source).
    ///
    /// Returns only once the server has negotiated a decodable stream format
    /// (or with an error explaining why it did not).
    pub fn start(config: CaptureConfig) -> Result<Self, CaptureError> {
        let queue = ChunkQueue::new(super::capture::QUEUE_CAPACITY);
        let (quit_tx, quit_rx) = pw::channel::channel::<()>();
        let (ready_tx, ready_rx) = sync_channel::<Result<NegotiatedFormat, CaptureError>>(1);
        let thread_queue = queue.clone();
        let shared = SharedFormat::default();
        let thread_shared = shared.clone();

        let handle = thread::Builder::new()
            .name("modv-pw-monitor".to_string())
            .spawn(move || {
                monitor_thread(config, thread_queue, ready_tx, thread_shared, quit_rx);
            })
            .map_err(|error| CaptureError::ServerUnavailable(error.to_string()))?;

        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(format)) => Ok(Self {
                quit: Some(quit_tx),
                handle: Some(handle),
                queue,
                format: shared,
                startup: format,
            }),
            Ok(Err(error)) => {
                let _ = quit_tx.send(());
                let _ = handle.join();
                Err(error)
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = quit_tx.send(());
                let _ = handle.join();
                Err(CaptureError::ServerUnavailable(format!(
                    "timed out after {}s waiting for the PipeWire stream to negotiate a format",
                    START_TIMEOUT.as_secs()
                )))
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = handle.join();
                Err(CaptureError::ServerUnavailable(
                    "PipeWire monitor thread exited during startup".to_string(),
                ))
            }
        }
    }

    /// The **current** negotiated sample rate in Hz (the analyser is built from
    /// it and rebuilt when the server renegotiates).
    pub fn sample_rate(&self) -> u32 {
        self.current_format().rate
    }

    /// The **current** negotiated channel count (the monitor emits this
    /// interleaved).
    pub fn channels(&self) -> u16 {
        u16::try_from(self.current_format().channels).unwrap_or(1)
    }

    /// The full current format (sample layout, rate and channels).
    ///
    /// Reads the slot the loop thread publishes on every negotiation, so a
    /// mid-stream renegotiation is visible without another readiness exchange.
    pub fn negotiated_format(&self) -> NegotiatedFormat {
        self.current_format()
    }

    /// Live negotiated format, falling back to the startup format only while
    /// the stream is parked (the shared slot is cleared on a rejection).
    fn current_format(&self) -> NegotiatedFormat {
        self.format
            .lock()
            .ok()
            .and_then(|current| *current)
            .unwrap_or(self.startup)
    }

    /// Wait up to `timeout` for the next interleaved f32 chunk.
    ///
    /// Returns `Ok(None)` on timeout. A latched format/stream error (a
    /// renegotiation the monitor cannot decode, or a capture error) is
    /// returned as [`CaptureError::Stream`] on **every** call until a later
    /// negotiation succeeds.
    pub fn next_chunk(&mut self, timeout: Duration) -> Result<Option<Vec<f32>>, CaptureError> {
        if let Some(error) = self.queue.error() {
            return Err(CaptureError::Stream(error));
        }

        Ok(self.queue.pop(timeout))
    }

    /// Stop the loop thread and close the queue. Idempotent.
    pub fn stop(&mut self) {
        if let Some(quit) = self.quit.take() {
            let _ = quit.send(());
        }

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }

        self.queue.close();
    }
}

impl Drop for PipeWireMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Body of the dedicated PipeWire loop thread.
fn monitor_thread(
    config: CaptureConfig,
    queue: ChunkQueue,
    ready: SyncSender<Result<NegotiatedFormat, CaptureError>>,
    shared: SharedFormat,
    quit_rx: pw::channel::Receiver<()>,
) {
    pw::init();

    let mainloop = match pw::main_loop::MainLoopRc::new(None) {
        Ok(mainloop) => mainloop,
        Err(error) => {
            let _ = ready.send(Err(CaptureError::ServerUnavailable(error.to_string())));
            return;
        }
    };

    let context = match pw::context::ContextRc::new(&mainloop, None) {
        Ok(context) => context,
        Err(error) => {
            let _ = ready.send(Err(CaptureError::ServerUnavailable(error.to_string())));
            return;
        }
    };

    let core = match context.connect_rc(None) {
        Ok(core) => core,
        Err(error) => {
            let _ = ready.send(Err(CaptureError::ServerUnavailable(error.to_string())));
            return;
        }
    };

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
    };

    if config.capture_sink {
        // Capture the sink monitor instead of a microphone (the property
        // `pw-record` received through `-P '{"stream.capture.sink":true}'`).
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }

    if let Some(target) = &config.device {
        // `TARGET_OBJECT` lives behind the crate's `v0_3_44` feature; the
        // property key itself is stable.
        props.insert("target.object", target.clone());
    }

    let stream_name = if config.capture_sink {
        STREAM_NAME
    } else {
        SOURCE_STREAM_NAME
    };

    let stream = match pw::stream::StreamBox::new(&core, stream_name, props) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = ready.send(Err(CaptureError::ServerUnavailable(error.to_string())));
            return;
        }
    };

    let data = UserData {
        format: spa::param::audio::AudioInfoRaw::new(),
        assembler: ChunkAssembler::new(config.channels, config.block_frames),
        formats: FormatState::with_shared(ready.clone(), queue.clone(), shared),
        scratch: Vec::new(),
    };

    let listener = stream
        .add_local_listener_with_user_data(data)
        // R-2 residual: a server restart/shutdown used to stop callbacks
        // silently (the analyser then reported silence forever). Latch the
        // stream loss into the same error channel as a bad format, so
        // `AudioFeed` stops the monitor and the engine falls back to the
        // renderer pump.
        .state_changed(|_, user_data, old, new| {
            let lost = matches!(new, StreamState::Error(_))
                || (matches!(old, StreamState::Streaming | StreamState::Paused)
                    && matches!(new, StreamState::Unconnected));
            if lost {
                user_data
                    .formats
                    .reject(format!("PipeWire stream lost ({old:?} -> {new:?})"));
            }
        })
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };

            if id != ParamType::Format.as_raw() {
                return;
            }

            let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                user_data
                    .formats
                    .reject("PipeWire sent an unparsable audio format".to_string());
                return;
            };

            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                user_data.formats.reject(format!(
                    "PipeWire negotiated a non-raw audio format ({media_type:?}/{media_subtype:?})"
                ));
                return;
            }

            if user_data.format.parse(param).is_err() {
                user_data
                    .formats
                    .reject("PipeWire sent an unparsable audio format".to_string());
                return;
            }

            let format = user_data.format.format();
            let rate = user_data.format.rate();
            let channels = user_data.format.channels();

            if user_data.formats.set_format(format, rate, channels) {
                user_data
                    .assembler
                    .set_channels(u16::try_from(channels).unwrap_or(1));
            }
        })
        .process(|stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();

            if datas.is_empty() {
                return;
            }

            let data = &mut datas[0];
            let size = data.chunk().size() as usize;
            let Some(bytes) = data.data() else {
                return;
            };

            let Some(negotiated) = user_data.formats.negotiated() else {
                // No accepted format yet: park the buffer instead of decoding
                // it with a guessed sample layout (R-2).
                return;
            };

            let bytes = &bytes[..size.min(bytes.len())];

            let channels = u16::try_from(negotiated.channels).unwrap_or(1);

            let UserData {
                assembler,
                formats,
                scratch,
                ..
            } = user_data;

            if !formats.decode_into(bytes, scratch) {
                return;
            }

            if assembler.channels() != usize::from(channels) {
                assembler.set_channels(channels);
            }

            assembler.push(scratch, |chunk| formats.queue.push(chunk));
        })
        .register();

    let _listener = match listener {
        Ok(listener) => listener,
        Err(error) => {
            let _ = ready.send(Err(CaptureError::ServerUnavailable(error.to_string())));
            return;
        }
    };

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(config.sample_rate);
    audio_info.set_channels(u32::from(config.channels));

    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = match spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    ) {
        Ok(serialized) => serialized.0.into_inner(),
        Err(error) => {
            let _ = ready.send(Err(CaptureError::ConfigUnsupported(error.to_string())));
            return;
        }
    };
    let Some(pod) = Pod::from_bytes(&values) else {
        let _ = ready.send(Err(CaptureError::ConfigUnsupported(
            "could not build the PipeWire format pod".to_string(),
        )));
        return;
    };
    let mut params = [pod];

    if let Err(error) = stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    ) {
        let _ = ready.send(Err(CaptureError::ServerUnavailable(error.to_string())));
        return;
    }

    let _quit = quit_rx.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });

    // Readiness is sent from `param_changed`: the format negotiation happens
    // while the loop runs, so this thread reports back from there (R-3).
    mainloop.run();
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) use spa::param::audio::AudioFormat;

    /// Drives negotiation and queues PCM for a hardware-free [`PipeWireMonitor`].
    /// [`test_monitor`] hands the monitor the same shared format slot this
    /// control publishes to, so tests can renegotiate mid-stream exactly as a
    /// live PipeWire server would.
    pub(crate) struct TestFormatControl {
        formats: FormatState,
    }

    impl TestFormatControl {
        pub(crate) fn set_format(&mut self, format: AudioFormat, rate: u32, channels: u32) -> bool {
            self.formats.set_format(format, rate, channels)
        }

        pub(crate) fn negotiated(&self) -> Option<NegotiatedFormat> {
            self.formats.negotiated()
        }

        pub(crate) fn push_chunk(&self, chunk: Vec<f32>) {
            self.formats.queue.push(chunk);
        }
    }

    pub(crate) fn test_monitor() -> (PipeWireMonitor, TestFormatControl) {
        let queue = ChunkQueue::new(super::super::capture::QUEUE_CAPACITY);
        let (ready_tx, _ready_rx) = sync_channel(1);
        let shared = SharedFormat::default();
        let mut formats = FormatState::with_shared(ready_tx, queue.clone(), shared.clone());
        assert!(formats.set_format(AudioFormat::F32LE, 48_000, 2));

        let startup = formats.negotiated().expect("startup format");
        let monitor = PipeWireMonitor {
            quit: None,
            handle: None,
            queue,
            format: shared,
            startup,
        };

        (monitor, TestFormatControl { formats })
    }

    #[test]
    fn source_props_keep_only_named_audio_sources() {
        assert_eq!(
            source_from_props(
                Some("alsa_input.usb-mic"),
                Some("Audio/Source"),
                Some("USB Mic")
            ),
            Some(SourceInfo {
                name: "alsa_input.usb-mic".to_string(),
                description: "USB Mic".to_string(),
            }),
        );
        // Sinks are not capturable inputs.
        assert_eq!(
            source_from_props(Some("sink.node"), Some("Audio/Sink"), None),
            None
        );
        assert_eq!(
            source_from_props(Some("stream.node"), Some("Stream/Output/Audio"), None),
            None,
        );
        // A source without a node.name is unusable as a target.
        assert_eq!(
            source_from_props(None, Some("Audio/Source"), Some("Mic")),
            None
        );
        assert_eq!(
            source_from_props(Some("  "), Some("Audio/Source"), None),
            None
        );
        // The description falls back to the node name.
        assert_eq!(
            source_from_props(Some("node.b"), Some("Audio/Source"), None),
            Some(SourceInfo {
                name: "node.b".to_string(),
                description: "node.b".to_string(),
            }),
        );
        // A blank description falls back too.
        assert_eq!(
            source_from_props(Some("node.c"), Some("Audio/Source"), Some("  ")),
            Some(SourceInfo {
                name: "node.c".to_string(),
                description: "node.c".to_string(),
            }),
        );
    }

    // A live-daemon test cannot deterministically simulate a connected server
    // that never completes the registry sync, so inspect the source contract.
    // Residual (out of scope): sources() still runs on the UI thread.
    #[test]
    fn sources_enumeration_has_a_bounded_registry_wait() {
        let source = include_str!("pipewire.rs");
        let start = source
            .find("pub fn sources()")
            .expect("sources() must exist");
        let body = &source[start..];
        let body = body
            .split_once("\n}\n")
            .expect("sources() must have a top-level closing brace")
            .0;

        let compact: String = body.chars().filter(|char| !char.is_whitespace()).collect();
        assert!(
            !compact.contains("while!done.get(){mainloop.run();}"),
            "sources() must not use an unbounded mainloop.run() loop"
        );
        assert!(
            body.contains("SOURCE_ENUMERATION_TIMEOUT")
                || (body.contains("Duration") && body.contains("Instant::now")),
            "sources() must declare a monotonic enumeration deadline"
        );
        assert!(
            body.contains("iterate(")
                || body.contains("recv_timeout(")
                || body.contains("add_timer("),
            "sources() must wait with a bounded/timed mainloop iteration"
        );
    }

    #[test]
    fn sources_enumeration_smoke_test_or_skip() {
        // Either no PipeWire daemon answered (empty) or every enumerated row
        // is a named source node — a smoke check, not an assertion of
        // hardware.
        let found = sources();

        for info in &found {
            assert!(!info.name.is_empty());
            assert!(!info.description.is_empty());
        }

        println!("pipewire sources: {}", found.len());

        for info in &found {
            println!("  {} [{}]", info.description, info.name);
        }
    }

    #[test]
    fn source_device_ids_carry_the_pw_prefix() {
        assert_eq!(super::super::capture::PIPEWIRE_SOURCE_PREFIX, "pw:");
        assert_eq!(
            format!(
                "{}{}",
                super::super::capture::PIPEWIRE_SOURCE_PREFIX,
                "node"
            ),
            "pw:node",
        );
    }

    /// Decode through the buffered path (the one the RT callback uses).
    fn decode(formats: &FormatState, bytes: &[u8]) -> Option<Vec<f32>> {
        let mut out = Vec::new();
        formats.decode_into(bytes, &mut out).then_some(out)
    }

    #[test]
    fn accepted_formats_are_interleaved_f32_or_s16_at_a_usable_rate() {
        assert_eq!(
            negotiate(AudioFormat::F32LE, 48_000, 2),
            Ok(NegotiatedFormat {
                sample: PcmFormat::F32Le,
                rate: 48_000,
                channels: 2,
            })
        );
        assert_eq!(
            negotiate(AudioFormat::S16LE, 48_000, 2),
            Ok(NegotiatedFormat {
                sample: PcmFormat::S16Le,
                rate: 48_000,
                channels: 2,
            })
        );
        // Mono and non-48 kHz monitors are still decodable: the caller builds
        // the analyser from the negotiated rate.
        assert_eq!(
            negotiate(AudioFormat::S16LE, 44_100, 1),
            Ok(NegotiatedFormat {
                sample: PcmFormat::S16Le,
                rate: 44_100,
                channels: 1,
            })
        );
        assert!(negotiate(AudioFormat::F32LE, 96_000, 6).is_ok());
        assert!(negotiate(AudioFormat::F32LE, MIN_SUPPORTED_RATE, 1).is_ok());
        assert!(negotiate(AudioFormat::F32LE, MAX_SUPPORTED_RATE, 8).is_ok());
    }

    #[test]
    fn rejected_formats_explain_why() {
        let planar = negotiate(AudioFormat::F32P, 48_000, 2).expect_err("planar is undecodable");
        assert!(planar.contains("F32P"), "{planar}");

        let s24 = negotiate(AudioFormat::S24LE, 48_000, 2).expect_err("24-bit is undecodable");
        assert!(s24.contains("S24LE"), "{s24}");

        let unknown = negotiate(AudioFormat::Unknown, 48_000, 2).expect_err("unknown rejected");
        assert!(unknown.contains("Unknown"), "{unknown}");

        let rate = negotiate(AudioFormat::F32LE, 0, 2).expect_err("0 Hz rejected");
        assert!(rate.contains("0 Hz"), "{rate}");

        let fast = negotiate(AudioFormat::F32LE, 384_000, 2).expect_err("384 kHz rejected");
        assert!(fast.contains("384000"), "{fast}");

        let no_channels = negotiate(AudioFormat::F32LE, 48_000, 0).expect_err("0 ch rejected");
        assert!(no_channels.contains("channel count 0"), "{no_channels}");

        let many = negotiate(AudioFormat::F32LE, 48_000, 9).expect_err("9 ch rejected");
        assert!(many.contains("channel count 9"), "{many}");
    }

    #[test]
    fn f32_buffers_decode_verbatim() {
        let mut bytes = Vec::new();
        for sample in [0.0f32, 0.5, -1.0] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        assert_eq!(
            decode_interleaved(&bytes, PcmFormat::F32Le),
            vec![0.0, 0.5, -1.0]
        );
        // A trailing partial sample is ignored.
        assert_eq!(
            decode_interleaved(&[0, 0, 128], PcmFormat::F32Le),
            Vec::<f32>::new()
        );
    }

    #[test]
    fn s16_buffers_are_converted_to_full_scale_f32() {
        let mut bytes = Vec::new();
        for sample in [0i16, 16_384, -32_768, 32_767] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        assert_eq!(
            decode_interleaved(&bytes, PcmFormat::S16Le),
            vec![0.0, 0.5, -1.0, 32_767.0 / 32_768.0]
        );
        // Odd byte counts keep whole samples only.
        assert_eq!(
            decode_interleaved(&[1], PcmFormat::S16Le),
            Vec::<f32>::new()
        );
        assert_eq!(decode_interleaved(&[0, 0, 9], PcmFormat::S16Le).len(), 1);
        assert_eq!(PcmFormat::S16Le.bytes_per_sample(), 2);
        assert_eq!(PcmFormat::F32Le.bytes_per_sample(), 4);
    }

    #[test]
    fn decoding_reuses_its_scratch_buffer_without_reallocating() {
        let queue = ChunkQueue::new(1);
        let (ready_tx, _ready_rx) = sync_channel(1);
        let mut formats = FormatState::new(ready_tx, queue.clone());
        assert!(formats.set_format(AudioFormat::F32LE, 48_000, 2));

        let mut scratch = Vec::new();
        let mut bytes = Vec::new();
        for sample in [0.0f32, 0.5, -1.0] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        assert!(formats.decode_into(&bytes, &mut scratch));
        assert_eq!(scratch, vec![0.0, 0.5, -1.0]);

        // A server-sized buffer allocates once...
        let mut block = Vec::new();
        for sample in [0.25f32; 96] {
            block.extend_from_slice(&sample.to_le_bytes());
        }

        assert!(formats.decode_into(&block, &mut scratch));
        let capacity = scratch.capacity();
        assert!(capacity >= 96);

        // ...and every following `process` callback reuses the allocation: the
        // loop thread must not touch the heap in steady state (R-22).
        for _ in 0..16 {
            assert!(formats.decode_into(&block[..192], &mut scratch));
        }

        assert_eq!(
            scratch.capacity(),
            capacity,
            "steady-state decoding must not reallocate"
        );
        assert_eq!(scratch.len(), 48);

        // A bigger buffer (a format/buffer-size change) grows once; the bigger
        // allocation is then kept for smaller buffers.
        let mut big = Vec::new();
        for sample in [0i16; 64] {
            big.extend_from_slice(&sample.to_le_bytes());
        }

        assert!(formats.set_format(AudioFormat::S16LE, 48_000, 2));
        assert!(formats.decode_into(&big, &mut scratch));
        assert_eq!(scratch.len(), 64);
        let grown = scratch.capacity();
        assert!(grown >= 64);

        assert!(formats.decode_into(&bytes[..4], &mut scratch));
        assert_eq!(scratch.capacity(), grown, "the grown buffer is kept");
        // The active format is S16 by now: four bytes are two samples.
        assert_eq!(scratch.len(), 2);
    }

    #[test]
    fn decoding_matches_the_previous_per_sample_mapping() {
        let mut out = Vec::new();

        let mut f32_bytes = Vec::new();
        for sample in [0.0f32, 0.5, -1.0] {
            f32_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        decode_interleaved_into(&f32_bytes, PcmFormat::F32Le, &mut out);
        assert_eq!(out, vec![0.0, 0.5, -1.0]);

        let mut s16_bytes = Vec::new();
        for sample in [0i16, 16_384, -32_768, 32_767] {
            s16_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        decode_interleaved_into(&s16_bytes, PcmFormat::S16Le, &mut out);
        assert_eq!(out, vec![0.0, 0.5, -1.0, 32_767.0 / 32_768.0]);

        // Partial trailing samples are still ignored, and a shorter buffer
        // truncates the previous result rather than leaving stale samples.
        decode_interleaved_into(&[0, 0, 9], PcmFormat::S16Le, &mut out);
        assert_eq!(out.len(), 1);
        decode_interleaved_into(&[0, 0, 128], PcmFormat::F32Le, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn readiness_is_reported_only_after_a_format_is_accepted() {
        let queue = ChunkQueue::new(1);
        let (ready_tx, ready_rx) = sync_channel(1);
        let mut formats = FormatState::new(ready_tx, queue.clone());

        // Nothing negotiated yet: no readiness, and buffers are parked.
        assert!(formats.negotiated().is_none());
        assert!(decode(&formats, &1.0f32.to_le_bytes()).is_none());
        assert!(
            ready_rx.try_recv().is_err(),
            "start must not report ready before the server negotiated a format"
        );

        assert!(formats.set_format(AudioFormat::F32LE, 48_000, 2));
        assert_eq!(
            ready_rx.try_recv().expect("ready after negotiation"),
            Ok(NegotiatedFormat {
                sample: PcmFormat::F32Le,
                rate: 48_000,
                channels: 2,
            })
        );
        assert_eq!(
            decode(&formats, &1.0f32.to_le_bytes()),
            Some(vec![1.0]),
            "buffers decode once the format is accepted"
        );

        // Ready stays a single report.
        assert!(formats.set_format(AudioFormat::F32LE, 44_100, 2));
        assert!(ready_rx.try_recv().is_err());
        assert_eq!(formats.negotiated().expect("renegotiated").rate, 44_100);
    }

    #[test]
    fn a_rejected_format_parks_the_stream_and_latches_its_error() {
        let queue = ChunkQueue::new(1);
        let (ready_tx, ready_rx) = sync_channel(1);
        let mut formats = FormatState::new(ready_tx, queue.clone());

        assert!(!formats.set_format(AudioFormat::S24LE, 48_000, 2));
        assert!(
            decode(&formats, &[0u8; 8]).is_none(),
            "an unsupported format must never be decoded as f32"
        );
        match ready_rx.try_recv().expect("readiness reports the failure") {
            Err(CaptureError::ConfigUnsupported(message)) => {
                assert!(message.contains("S24LE"), "{message}");
            }
            other => panic!("unexpected readiness: {other:?}"),
        }

        // The error is latched, not one-shot: the engine sees it every frame.
        let first = queue.error().expect("latched error");
        let second = queue.error().expect("still latched");
        assert_eq!(first, second);
        assert!(first.contains("S24LE"), "{first}");

        // A later accepted negotiation clears the latch (and becomes the
        // reported format).
        assert!(formats.set_format(AudioFormat::S16LE, 44_100, 1));
        assert!(queue.error().is_none());
        assert_eq!(
            formats.negotiated(),
            Some(NegotiatedFormat {
                sample: PcmFormat::S16Le,
                rate: 44_100,
                channels: 1,
            })
        );
    }

    #[test]
    fn format_state_renegotiation_updates_the_shared_format() {
        let (monitor, mut format) = test_monitor();

        assert_eq!(
            (monitor.sample_rate(), monitor.channels()),
            (48_000, 2),
            "the monitor starts at the first negotiation"
        );

        assert!(format.set_format(AudioFormat::F32LE, 44_100, 1));
        assert_eq!(
            (monitor.sample_rate(), monitor.channels()),
            (44_100, 1),
            "monitor accessors must follow renegotiation, not the startup copy"
        );

        assert!(!format.set_format(AudioFormat::S24LE, 44_100, 1));
        assert_eq!(
            format.negotiated(),
            None,
            "a rejected negotiation must clear the shared format"
        );
    }

    /// Live smoke test: starts the monitor when a server is reachable and
    /// prints a skip line otherwise (the Docker test image has no daemon).
    #[test]
    fn monitor_smoke_test_or_skip_without_a_server() {
        match PipeWireMonitor::start(CaptureConfig::default()) {
            Ok(mut monitor) => {
                let chunk = monitor.next_chunk(Duration::from_millis(500));

                println!(
                    "pipewire monitor: {:?} {} Hz / {} ch, first chunk: {:?}",
                    monitor.negotiated_format().sample,
                    monitor.sample_rate(),
                    monitor.channels(),
                    chunk.map(|chunk| chunk.map(|chunk| chunk.len()))
                );
                monitor.stop();
            }
            Err(error) => {
                println!("skipping PipeWire monitor smoke test (no server): {error}");
            }
        }
    }
}
