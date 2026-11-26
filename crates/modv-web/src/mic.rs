//! WebAudio microphone capture.
//!
//! `web/main.js` wires the "Use microphone" button to [`enable_microphone`].
//! Once granted, each animation frame calls [`frame`] and the visualisers get
//! the same `AudioFrame` the native analyser produces. Until then the shell
//! runs on [`crate::audio::SimAudio`], so the canvas is never silent while a
//! permission prompt is pending.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

use modv_renderers::two_d::AudioFrame;

use crate::audio::band_energy;

thread_local! {
    static MIC: RefCell<MicCapture> = RefCell::new(MicCapture::default());
}

/// Ask for microphone permission and route the stream into an analyser.
///
/// The returned promise resolves once the AudioContext is running. A denied
/// permission rejects with the browser's error; the caller keeps the demo
/// audio path.
#[wasm_bindgen]
pub fn enable_microphone() -> js_sys::Promise {
    future_to_promise(async move {
        let navigator = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))?
            .navigator();
        let devices = navigator
            .media_devices()
            .map_err(|_| JsValue::from_str("mediaDevices is unavailable"))?;
        let constraints = web_sys::MediaStreamConstraints::new();
        constraints.set_audio_bool(true);
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
        analyser.set_smoothing_time_constant(0.8);
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

        MIC.with(|mic| {
            let mut mic = mic.borrow_mut();
            mic.context = Some(context.clone());
            mic.source = Some(source);
            mic.analyser = Some(analyser);
            mic.gain = Some(gain);
            mic.stream = Some(stream);
            mic.sample_rate = context.sample_rate();
            mic.db.clear();
            mic.wave.clear();
            mic.active = true;
            mic.beat = 0.0;
            mic.previous_rms = 0.0;
        });
        Ok(JsValue::from_str("microphone live"))
    })
}

/// One analyser frame, or `None` before the microphone is enabled.
pub fn frame(dt: f32) -> Option<AudioFrame> {
    MIC.with(|mic| mic.borrow_mut().frame(dt))
}

#[derive(Default)]
struct MicCapture {
    context: Option<web_sys::AudioContext>,
    source: Option<web_sys::MediaStreamAudioSourceNode>,
    analyser: Option<web_sys::AnalyserNode>,
    gain: Option<web_sys::GainNode>,
    stream: Option<web_sys::MediaStream>,
    sample_rate: f32,
    db: Vec<f32>,
    wave: Vec<f32>,
    beat: f32,
    previous_rms: f32,
    active: bool,
}

impl MicCapture {
    fn frame(&mut self, dt: f32) -> Option<AudioFrame> {
        let analyser = self.analyser.as_ref()?;
        if !self.active {
            return None;
        }
        let bins = analyser.frequency_bin_count() as usize;
        if self.db.len() != bins {
            self.db.resize(bins, -100.0);
        }
        analyser.get_float_frequency_data(&mut self.db);
        let fft_size = analyser.fft_size().max(1) as usize;
        if self.wave.len() != fft_size {
            self.wave.resize(fft_size, 0.0);
        }
        analyser.get_float_time_domain_data(&mut self.wave);

        let (bass, mid, treble) = band_energy(&self.db, self.sample_rate.max(1.0), fft_size);
        let rms = (self.wave.iter().map(|sample| sample * sample).sum::<f32>()
            / self.wave.len().max(1) as f32)
            .sqrt();
        let beat = if rms > (self.previous_rms * 1.35).max(0.02) {
            1.0
        } else {
            (self.beat - dt * 3.0).max(0.0)
        };
        self.previous_rms = if rms > self.previous_rms {
            rms
        } else {
            self.previous_rms * 0.92 + rms * 0.08
        };
        self.beat = beat;

        let mut frame = AudioFrame {
            bass,
            mid,
            treble,
            beat,
            sample_rate: self.sample_rate.max(1.0),
            ..AudioFrame::default()
        };

        // The engine's payload is fixed at 1024 samples/bins; resample the
        // analyser's 2048-bin frame into it. The preview runtime uses the same
        // shape, so every renderer sees one contract.
        for index in 0..1024 {
            let source = index * self.wave.len() / 1024;
            frame.wave[index] = self.wave.get(source).copied().unwrap_or(0.0);
            let source = index * self.db.len() / 1024;
            let magnitude = 10f32.powf(
                self.db
                    .get(source)
                    .copied()
                    .unwrap_or(-100.0)
                    .clamp(-100.0, 0.0)
                    / 20.0,
            );
            frame.freq_mag[index] = magnitude.clamp(0.0, 1.0);
            frame.byte_frequency_data[index] = (frame.freq_mag[index] * 255.0) as u8;
        }
        for damping in 0..4 {
            frame.lanes[damping * 3] = bass;
            frame.lanes[damping * 3 + 1] = mid;
            frame.lanes[damping * 3 + 2] = treble;
            frame.raw_lanes[damping * 3] = bass;
            frame.raw_lanes[damping * 3 + 1] = mid;
            frame.raw_lanes[damping * 3 + 2] = treble;
        }
        for (index, cell) in frame.cells.iter_mut().enumerate() {
            let position = index as f32 / 16.0;
            *cell = if position < 0.33 {
                bass
            } else if position < 0.66 {
                mid
            } else {
                treble
            };
        }
        frame.raw_cells = frame.cells;
        frame.features.rms = rms.clamp(0.0, 1.0);
        frame.features.energy = frame.features.rms;
        frame.features.spectral_centroid = 200.0 + 2_000.0 * treble;
        frame.features.buffer = frame.wave.to_vec();

        Some(frame)
    }
}
