//! WebAudio microphone capture.
//!
//! `web/main.js` wires the "Use microphone" button to [`enable_microphone`].
//! Once granted, each output frame feeds the new time-domain samples into the
//! native [`AudioAnalyser`] (FFT, bands, Meyda features, BeatDetektor and
//! TempoDetector), then maps the resulting snapshot to the renderer's
//! `AudioFrame` contract.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

use modv_media::audio::analyser::AudioAnalyser;
use modv_renderers::two_d::AudioFrame;

use crate::audio::snapshot_to_frame;

thread_local! {
    static MIC: RefCell<MicCapture> = RefCell::new(MicCapture::default());
}

/// One output frame from the microphone analyser.
pub struct MicFrame {
    /// The frame the compositor and Input Links consume.
    pub audio: AudioFrame,
    /// A confident tempo estimate, once TempoDetector has one.
    pub bpm: Option<f32>,
    /// BeatDetektor's BassKick flag for this frame.
    pub kick: bool,
}

/// Ask for microphone permission and route the stream into an analyser.
///
/// The returned promise resolves once the AudioContext is running. A denied
/// permission rejects with the browser's error; the caller keeps silence.
#[wasm_bindgen]
pub fn enable_microphone(device_id: Option<String>) -> js_sys::Promise {
    future_to_promise(async move {
        let navigator = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))?
            .navigator();
        let devices = navigator
            .media_devices()
            .map_err(|_| JsValue::from_str("mediaDevices is unavailable"))?;
        let constraints = web_sys::MediaStreamConstraints::new();
        match device_id.filter(|id| !id.is_empty()) {
            Some(device_id) => {
                let device = js_sys::Object::new();
                let exact = js_sys::Object::new();
                js_sys::Reflect::set(
                    &exact,
                    &JsValue::from_str("exact"),
                    &JsValue::from_str(&device_id),
                )?;
                js_sys::Reflect::set(&device, &JsValue::from_str("deviceId"), &exact)?;
                constraints.set_audio(&device);
            }
            None => constraints.set_audio_bool(true),
        }
        let stream = JsFuture::from(
            devices
                .get_user_media_with_constraints(&constraints)
                .map_err(|_| JsValue::from_str("getUserMedia is unavailable"))?,
        )
        .await?;
        let stream: web_sys::MediaStream = stream.dyn_into()?;

        let context = web_sys::AudioContext::new()?;
        let source = context.create_media_stream_source(&stream)?;
        let analyser = context.create_analyser()?;
        analyser.set_fft_size(2_048);
        analyser.set_smoothing_time_constant(0.0);
        let gain = context.create_gain()?;
        gain.gain().set_value(0.0);
        source
            .connect_with_audio_node(&analyser)
            .map_err(|_| JsValue::from_str("could not connect the microphone"))?;
        analyser
            .connect_with_audio_node(&gain)
            .map_err(|_| JsValue::from_str("could not connect the analyser"))?;
        gain.connect_with_audio_node(&context.destination())
            .map_err(|_| JsValue::from_str("could not reach the audio destination"))?;
        JsFuture::from(context.resume()?).await?;

        let sample_rate = context.sample_rate();
        MIC.with(|mic| {
            let mut mic = mic.borrow_mut();
            mic.context = Some(context);
            mic.source = Some(source);
            mic.analyser = Some(analyser);
            mic.gain = Some(gain);
            mic.stream = Some(stream);
            mic.processor = Some(AudioAnalyser::new(sample_rate));
            mic.sample_rate = sample_rate;
            mic.pushed_until = 0;
            mic.time_domain.clear();
            mic.active = true;
        });
        Ok(JsValue::from_str("microphone live"))
    })
}

/// One analyser frame, or `None` before the microphone is enabled.
pub fn frame(dt: f32) -> Option<MicFrame> {
    MIC.with(|mic| mic.borrow_mut().frame(dt))
}

#[derive(Default)]
struct MicCapture {
    context: Option<web_sys::AudioContext>,
    source: Option<web_sys::MediaStreamAudioSourceNode>,
    analyser: Option<web_sys::AnalyserNode>,
    gain: Option<web_sys::GainNode>,
    stream: Option<web_sys::MediaStream>,
    processor: Option<AudioAnalyser>,
    sample_rate: f32,
    /// Total samples already pushed to the native analyser.
    pushed_until: u64,
    time_domain: Vec<f32>,
    active: bool,
}

impl MicCapture {
    fn frame(&mut self, dt: f32) -> Option<MicFrame> {
        let analyser = self.analyser.as_ref()?;
        if !self.active {
            return None;
        }
        let fft_size = analyser.fft_size().max(1) as usize;
        if self.time_domain.len() != fft_size {
            self.time_domain.resize(fft_size, 0.0);
        }
        analyser.get_float_time_domain_data(&mut self.time_domain);

        let sample_rate = self.sample_rate.max(1.0);
        let context_time = self.context.as_ref()?.current_time();
        let total = (context_time * f64::from(sample_rate)).max(0.0) as u64;
        let new = total.saturating_sub(self.pushed_until) as usize;
        self.pushed_until = total;
        if new > 0 {
            let take = new.min(fft_size);
            let start = fft_size - take;
            self.processor
                .as_mut()?
                .push_mono(&self.time_domain[start..]);
        }

        let processor = self.processor.as_mut()?;
        let snapshot = processor.frame(dt);
        let kick = processor.kick();
        Some(MicFrame {
            audio: snapshot_to_frame(&snapshot),
            bpm: snapshot.bpm,
            kick,
        })
    }
}
