//! Demo audio for browser tests.
//!
//! The shell itself starts silent; the microphone button is the only capture
//! path. This deterministic 120 BPM source is kept for host tests and as an
//! opt-in fixture for future browser demos.

use modv_media::audio::analyser::AudioSnapshot;
use modv_renderers::two_d::{AudioFeatures, AudioFrame};

/// Sample rate used when the analyser reports a non-finite/zero rate.
pub const FALLBACK_SAMPLE_RATE: f32 = 44_100.0;

/// Map the native analyser snapshot onto the renderer's `AudioFrame`.
///
/// Every payload field the compositor and the module props read is carried
/// over verbatim, with non-finite values sanitised to silence so a broken
/// input can never poison a shader uniform.
pub fn snapshot_to_frame(snapshot: &AudioSnapshot) -> AudioFrame {
    let wave = sanitize_array(&snapshot.wave);
    let features = &snapshot.features;
    let sample_rate = if snapshot.sample_rate.is_finite() && snapshot.sample_rate > 0.0 {
        snapshot.sample_rate
    } else {
        FALLBACK_SAMPLE_RATE
    };
    AudioFrame {
        bass: sanitize(snapshot.bands[0]),
        mid: sanitize(snapshot.bands[1]),
        treble: sanitize(snapshot.bands[2]),
        beat: sanitize(snapshot.beat),
        wave,
        byte_frequency_data: snapshot.byte_frequency_data,
        lanes: sanitize_array(&snapshot.lanes),
        cells: sanitize_array(&snapshot.cells),
        raw_lanes: sanitize_array(&snapshot.raw_lanes),
        raw_cells: sanitize_array(&snapshot.raw_cells),
        freq_mag: sanitize_array(&snapshot.freq_mag),
        sample_rate,
        features: AudioFeatures {
            rms: sanitize(features.rms),
            zcr: sanitize(features.zcr),
            energy: sanitize(features.energy),
            spectral_centroid: sanitize(features.spectral_centroid),
            spectral_flatness: sanitize(features.spectral_flatness),
            spectral_slope: sanitize(features.spectral_slope),
            spectral_rolloff: sanitize(features.spectral_rolloff),
            spectral_spread: sanitize(features.spectral_spread),
            spectral_skewness: sanitize(features.spectral_skewness),
            spectral_kurtosis: sanitize(features.spectral_kurtosis),
            perceptual_spread: sanitize(features.perceptual_spread),
            perceptual_sharpness: sanitize(features.perceptual_sharpness),
            buffer: wave.to_vec(),
        },
    }
}

fn sanitize(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn sanitize_array<const N: usize>(values: &[f32; N]) -> [f32; N] {
    let mut sanitized = [0.0; N];
    for (slot, value) in sanitized.iter_mut().zip(values) {
        *slot = sanitize(*value);
    }
    sanitized
}

/// Convert WebAudio's dB spectrum to the three AGC-ish bands the visuals use.
///
/// WebAudio's `getFloatFrequencyData` returns dBFS (`0` = full scale, `-100` =
/// silence). The native analyser computes a log-spaced 16-cell grid; the
/// browser shell only needs the three headline bands, so this averages linear
/// magnitudes over bass/mid/treble ranges and applies the same 4× visual gain
/// the preview catalogue uses.
pub fn band_energy(db: &[f32], sample_rate: f32, fft_size: usize) -> (f32, f32, f32) {
    if db.is_empty() || fft_size == 0 || !sample_rate.is_finite() || sample_rate <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let bin_hz = sample_rate / fft_size as f32;
    let mut sums = [0.0f32; 3];
    let mut counts = [0u32; 3];
    for (index, db) in db.iter().enumerate() {
        let hz = index as f32 * bin_hz;
        let band = if (20.0..250.0).contains(&hz) {
            0
        } else if (250.0..2_000.0).contains(&hz) {
            1
        } else if (2_000.0..12_000.0).contains(&hz) {
            2
        } else {
            continue;
        };
        let magnitude = 10f32.powf(db.clamp(-100.0, 0.0) / 20.0);
        sums[band] += magnitude;
        counts[band] += 1;
    }
    let scale = |sum: f32, count: u32| {
        if count == 0 {
            0.0
        } else {
            (sum / count as f32 * 4.0).clamp(0.0, 1.0)
        }
    };
    (
        scale(sums[0], counts[0]),
        scale(sums[1], counts[1]),
        scale(sums[2], counts[2]),
    )
}

/// A deterministic audio source at [`BPM`](Self::BPM).
pub struct SimAudio {
    time: f64,
    bpm: f64,
}

impl Default for SimAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl SimAudio {
    /// A 120 BPM simulation.
    pub fn new() -> Self {
        Self {
            time: 0.0,
            bpm: 120.0,
        }
    }

    /// Advance the clock and return the frame the compositor should draw.
    pub fn update(&mut self, dt: f32) -> AudioFrame {
        self.time += f64::from(dt.clamp(0.0, 0.25));

        let beat = 60.0 / self.bpm;
        let phase = (self.time % beat) / beat;
        let kick = if phase < 0.18 {
            (1.0 - phase / 0.18).powi(2) as f32
        } else {
            0.0
        };
        let bass = (kick * 0.85 + 0.1 * (self.time * 1.7).sin().abs() as f32).min(1.0);
        let mid = (0.18 + 0.22 * (0.5 + 0.5 * (self.time * 2.4).sin()) as f32).min(1.0);
        let treble = (0.12 + 0.2 * (0.5 + 0.5 * (self.time * 5.3).cos()) as f32).min(1.0);
        let beat_envelope = (kick * 1.5).min(1.0);

        let mut frame = AudioFrame {
            bass,
            mid,
            treble,
            beat: beat_envelope,
            sample_rate: 48_000.0,
            ..AudioFrame::default()
        };

        for (index, sample) in frame.wave.iter_mut().enumerate() {
            let t = self.time as f32 + index as f32 / 48_000.0;
            *sample = (std::f32::consts::TAU * 55.0 * t).sin() * bass * 0.7
                + (std::f32::consts::TAU * 440.0 * t).sin() * mid * 0.15
                + (std::f32::consts::TAU * 3_200.0 * t).sin() * treble * 0.05;
        }

        for (index, magnitude) in frame.freq_mag.iter_mut().enumerate() {
            let position = index as f32 / 1024.0;
            let low = (1.0 - position * 3.0).max(0.0);
            let high = (position * 2.0 - 1.0).max(0.0);
            *magnitude = (bass * low + mid * 0.5 + treble * high).clamp(0.0, 1.0);
            frame.byte_frequency_data[index] = (*magnitude * 255.0) as u8;
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

        frame.features.rms = ((bass + mid + treble) / 3.0).clamp(0.0, 1.0);
        frame.features.energy = frame.features.rms;
        frame.features.spectral_centroid = 200.0 + 2_000.0 * treble;
        frame.features.buffer = frame.wave.to_vec();
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_produces_audio_from_the_first_frame() {
        let mut audio = SimAudio::new();
        let frame = audio.update(1.0 / 60.0);

        assert!(frame.bass > 0.0);
        assert!(frame.mid > 0.0);
        assert!(frame.treble > 0.0);
        assert_eq!(frame.wave.len(), 1024);
        assert_eq!(frame.freq_mag.len(), 1024);
        assert_eq!(frame.sample_rate, 48_000.0);
    }

    #[test]
    fn band_energy_puts_a_tone_in_the_right_band() {
        let sample_rate = 48_000.0;
        let fft_size = 2_048;
        let mut db = vec![-100.0; fft_size];

        // A full-scale 100 Hz tone: bin 4.27 at 23.4 Hz per bin.
        db[4] = 0.0;
        let (bass, mid, treble) = band_energy(&db, sample_rate, fft_size);
        assert!(bass > mid, "bass={bass} mid={mid}");
        assert!(bass > treble, "bass={bass} treble={treble}");

        let mut db = vec![-100.0; fft_size];
        // 1 kHz: bin 42.7.
        db[43] = 0.0;
        let (bass, mid, treble) = band_energy(&db, sample_rate, fft_size);
        assert!(mid > bass, "bass={bass} mid={mid}");
        assert!(mid > treble, "mid={mid} treble={treble}");

        let mut db = vec![-100.0; fft_size];
        // 6 kHz: bin 256.
        db[256] = 0.0;
        let (bass, mid, treble) = band_energy(&db, sample_rate, fft_size);
        assert!(treble > bass, "bass={bass} treble={treble}");
        assert!(treble > mid, "mid={mid} treble={treble}");
    }

    #[test]
    fn kick_lands_on_the_beat() {
        let mut audio = SimAudio::new();
        let mut saw_kick = false;
        for _ in 0..120 {
            saw_kick |= audio.update(1.0 / 60.0).beat > 0.5;
        }
        assert!(saw_kick, "a 2 Hz beat must kick within two seconds");
    }

    /// A real analyser frame (silent), then overridden: the snapshot shape is
    /// owned by `modv-media`, so building one through the analyser keeps this
    /// test honest about new fields.
    fn snapshot() -> AudioSnapshot {
        modv_media::audio::analyser::AudioAnalyser::new(48_000.0).frame(0.0)
    }

    #[test]
    fn analyser_snapshot_maps_every_payload_field() {
        let mut source = snapshot();

        source.bands = [0.6, 0.4, 0.3];
        source.lanes = [0.11; 12];
        source.raw_lanes = [1.11; 12];
        source.cells = [0.25; 16];
        source.raw_cells = [2.25; 16];
        source.beat = 0.75;
        source.wave = [0.5; 1024];
        source.freq_mag = [7.0; 1024];
        source.byte_frequency_data[0] = 255;
        source.byte_frequency_data[512] = 0;
        source.sample_rate = 48_000.0;
        source.features.rms = 0.3;
        source.features.zcr = 0.05;
        source.features.spectral_centroid = 1200.0;

        let frame = snapshot_to_frame(&source);

        assert_eq!([frame.bass, frame.mid, frame.treble], [0.6, 0.4, 0.3]);
        assert_eq!(frame.beat, 0.75);
        assert_eq!(frame.lanes, [0.11; 12]);
        assert_eq!(frame.raw_lanes, [1.11; 12]);
        assert_eq!(frame.cells, [0.25; 16]);
        assert_eq!(frame.raw_cells, [2.25; 16]);
        assert_eq!(frame.wave, [0.5; 1024]);
        assert_eq!(frame.freq_mag, [7.0; 1024]);
        assert_eq!(frame.byte_frequency_data[0], 255);
        assert_eq!(frame.byte_frequency_data[512], 0);
        assert_eq!(frame.sample_rate, 48_000.0);
        assert_eq!(frame.features.rms, 0.3);
        assert_eq!(frame.features.zcr, 0.05);
        assert_eq!(frame.features.spectral_centroid, 1200.0);
        assert_eq!(
            frame.features.buffer,
            frame.wave.to_vec(),
            "features.buffer mirrors the wave for Waveform.js"
        );
    }

    #[test]
    fn analyser_snapshot_sanitises_non_finite_values() {
        let mut source = snapshot();

        source.bands = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        source.lanes = [f32::NAN; 12];
        source.cells = [f32::INFINITY; 16];
        source.raw_lanes = [f32::NEG_INFINITY; 12];
        source.raw_cells = [f32::NAN; 16];
        source.beat = f32::NAN;
        source.wave = [f32::NAN; 1024];
        source.freq_mag = [f32::INFINITY; 1024];
        source.sample_rate = f32::NAN;
        source.features.rms = f32::NAN;
        source.features.spectral_flatness = f32::NAN;
        source.features.perceptual_sharpness = f32::INFINITY;

        let frame = snapshot_to_frame(&source);

        assert_eq!([frame.bass, frame.mid, frame.treble], [0.0; 3]);
        assert_eq!(frame.lanes, [0.0; 12]);
        assert_eq!(frame.cells, [0.0; 16]);
        assert_eq!(frame.raw_lanes, [0.0; 12]);
        assert_eq!(frame.raw_cells, [0.0; 16]);
        assert_eq!(frame.beat, 0.0);
        assert!(frame.wave.iter().all(|value| *value == 0.0));
        assert!(frame.freq_mag.iter().all(|value| *value == 0.0));
        assert_eq!(frame.sample_rate, FALLBACK_SAMPLE_RATE);
        assert_eq!(frame.features.rms, 0.0);
        assert_eq!(frame.features.spectral_flatness, 0.0);
        assert_eq!(frame.features.perceptual_sharpness, 0.0);
        assert!(frame.features.buffer.iter().all(|value| *value == 0.0));
    }
}
