//! WebAssembly runtime for the GitHub Pages module previews.
//!
//! The native compositor is wgpu/Vulkan and cannot run inside a browser, so
//! the catalogue renders its live previews with WebGPU driven by this small
//! runtime: it owns the clock, synthesised audio bands and the exact uniform
//! byte layouts the engine uses. The GPU draw itself stays in JavaScript.
//!
//! `runtime` is plain Rust so the byte layout can be tested on the host;
//! [`PreviewEngine`] is the thin `wasm-bindgen` skin the catalogue loads.

#![forbid(unsafe_code)]

pub mod runtime {
    use serde_json::Value;

    /// Size of the built-in shader shared uniform block (`ShaderUniforms`).
    pub const SHARED_UNIFORM_BYTES: usize = 160;
    /// Width of the FFT texture the engine reports in `u_fftResolution`.
    const FFT_WIDTH: f32 = 512.0;

    /// Clock and synthesised audio for one preview canvas.
    ///
    /// Offsets match `crates/modv-renderers/src/shader/uniforms.rs` exactly —
    /// the previews upload these bytes straight into a WebGPU uniform buffer.
    #[derive(Clone, Debug)]
    pub struct PreviewRuntime {
        width: u32,
        height: u32,
        bpm: f32,
        time: f32,
        frame: i32,
        bass: f32,
        mid: f32,
        treble: f32,
        kick: bool,
    }

    impl PreviewRuntime {
        /// A runtime for a `width`×`height` canvas at `bpm` beats per minute.
        pub fn new(width: u32, height: u32, bpm: f32) -> Self {
            let mut this = Self {
                width: width.max(1),
                height: height.max(1),
                bpm: if bpm > 0.0 { bpm } else { 120.0 },
                time: 0.0,
                frame: 0,
                bass: 0.0,
                mid: 0.0,
                treble: 0.0,
                kick: false,
            };
            this.refresh_audio();
            this
        }

        /// Change the preview size (e.g. on a device-pixel-ratio change).
        pub fn resize(&mut self, width: u32, height: u32) {
            self.width = width.max(1);
            self.height = height.max(1);
        }

        /// Jump to an absolute time in seconds.
        pub fn set_time(&mut self, seconds: f32) {
            self.time = seconds.max(0.0);
            self.refresh_audio();
        }

        /// Advance the clock by `dt_seconds` and refresh the audio bands.
        pub fn tick(&mut self, dt_seconds: f32) {
            self.time += dt_seconds.max(0.0);
            self.frame = self.frame.wrapping_add(1);
            self.refresh_audio();
        }

        /// Synthesised beat flag for this frame (the demo has no microphone).
        pub fn kick(&self) -> bool {
            self.kick
        }

        pub fn bass(&self) -> f32 {
            self.bass
        }

        pub fn mid(&self) -> f32 {
            self.mid
        }

        pub fn treble(&self) -> f32 {
            self.treble
        }

        /// A simple, deterministic four-on-the-floor simulation: the kick
        /// fires for the first 18% of every beat and the bass decays over the
        /// following 60%, so animated inputs move even with no audio capture.
        fn refresh_audio(&mut self) {
            let beat = 60.0 / self.bpm;
            let phase = (self.time % beat) / beat;
            self.kick = phase < 0.18;
            self.bass = (1.0 - phase / 0.6).clamp(0.0, 1.0) * 0.9 + 0.1;
            self.mid = 0.2 + 0.3 * (0.5 + 0.5 * (self.time * 2.0).sin());
            self.treble = 0.15 + 0.25 * (0.5 + 0.5 * (self.time * 5.0).cos());
        }

        /// The 160-byte shared uniform block for built-in WGSL modules.
        pub fn builtin_uniform_bytes(&self) -> [u8; SHARED_UNIFORM_BYTES] {
            let mut bytes = [0u8; SHARED_UNIFORM_BYTES];
            put_f32(&mut bytes, 0, self.time);
            put_i32(&mut bytes, 4, self.frame);
            put_f32(&mut bytes, 8, self.time);
            put_f32(&mut bytes, 12, self.time);
            // iDate = [year, month (0-based), weekday, seconds]. The browser
            // has no wall clock in this pure runtime, so use a fixed date.
            put_f32(&mut bytes, 16, 2026.0);
            put_f32(&mut bytes, 20, 10.0);
            put_f32(&mut bytes, 24, 2.0);
            put_f32(&mut bytes, 28, self.time);
            put_f32(&mut bytes, 32, self.width as f32);
            put_f32(&mut bytes, 36, self.height as f32);
            put_f32(&mut bytes, 40, 1.0);
            for slot in 0..4 {
                let base = 64 + slot * 16;
                put_f32(&mut bytes, base, self.width as f32);
                put_f32(&mut bytes, base + 4, self.height as f32);
                put_f32(&mut bytes, base + 8, 1.0);
            }
            put_f32(&mut bytes, 128, self.time);
            put_f32(&mut bytes, 132, self.time * 1000.0);
            put_f32(&mut bytes, 136, FFT_WIDTH);
            put_u32(&mut bytes, 140, u32::from(self.kick));
            put_f32(&mut bytes, 144, self.bass);
            put_f32(&mut bytes, 148, self.mid);
            put_f32(&mut bytes, 152, self.treble);
            bytes
        }

        /// Fill an ISF uniform block from the generated layout descriptor.
        ///
        /// `layout` is `IsfUniformLayout` from the catalogue (offsets in
        /// bytes); `props` is the module's synthesised prop list with its
        /// default values. Unknown sources stay zero, as in the native engine.
        pub fn isf_uniform_bytes(&self, layout: &str, props: &str) -> Result<Vec<u8>, String> {
            let layout: Value = serde_json::from_str(layout)
                .map_err(|error| format!("invalid ISF uniform layout: {error}"))?;
            let props: Value = serde_json::from_str(props)
                .map_err(|error| format!("invalid ISF prop list: {error}"))?;
            let size = layout["size"]
                .as_u64()
                .ok_or_else(|| "ISF uniform layout has no size".to_string())?
                as usize;
            if size > 1 << 20 {
                return Err("ISF uniform block is unreasonably large".to_string());
            }
            let mut bytes = vec![0u8; size];
            let members = layout["members"]
                .as_array()
                .ok_or_else(|| "ISF uniform layout has no members".to_string())?;
            for member in members {
                let offset = member["offset"]
                    .as_u64()
                    .ok_or_else(|| format!("ISF member {} has no offset", member["name"]))?
                    as usize;
                let kind = member["kind"].as_str().unwrap_or("float");
                let source = member["source"].as_str().unwrap_or("");
                match source {
                    "time" => write_scalar(&mut bytes, offset, kind, Value::from(self.time))?,
                    "timeDelta" => write_scalar(&mut bytes, offset, kind, Value::from(self.time))?,
                    "frameIndex" => {
                        write_scalar(&mut bytes, offset, kind, Value::from(self.frame))?
                    }
                    "passIndex" => write_scalar(&mut bytes, offset, kind, Value::from(0))?,
                    "renderSize" => {
                        write_vec(&mut bytes, offset, &[self.width as f32, self.height as f32])?
                    }
                    "date" => write_vec(&mut bytes, offset, &[2026.0, 10.0, 2.0, self.time])?,
                    _ if source.starts_with("imageSize:") => {
                        write_vec(&mut bytes, offset, &[self.width as f32, self.height as f32])?
                    }
                    _ if source.starts_with("imageRect:") => write_vec(
                        &mut bytes,
                        offset,
                        &[0.0, 0.0, self.width as f32, self.height as f32],
                    )?,
                    _ if source.starts_with("imageFlip:") => {
                        write_scalar(&mut bytes, offset, kind, Value::from(0))?
                    }
                    _ if source.starts_with("input:") => {
                        let name = &source["input:".len()..];
                        let value = props
                            .as_array()
                            .and_then(|array| {
                                array
                                    .iter()
                                    .find(|prop| prop["name"].as_str() == Some(name))
                            })
                            .map(|prop| prop["default"].clone())
                            .unwrap_or(Value::Null);
                        write_scalar(&mut bytes, offset, kind, value)?;
                    }
                    _ => {}
                }
            }
            Ok(bytes)
        }

        /// One 16-byte slot per prop, in declaration order — the built-in
        /// shader renderer's binding 7 layout.
        pub fn prop_bytes(&self, props: &str) -> Result<Vec<u8>, String> {
            let props: Value = serde_json::from_str(props)
                .map_err(|error| format!("invalid prop list: {error}"))?;
            let list = props
                .as_array()
                .ok_or_else(|| "prop descriptor must be an array".to_string())?;
            let mut bytes = vec![0u8; list.len() * 16];
            for (index, prop) in list.iter().enumerate() {
                let offset = index * 16;
                match &prop["default"] {
                    Value::Number(number) => {
                        if let Some(value) = number.as_f64() {
                            put_f32(&mut bytes, offset, value as f32);
                        }
                    }
                    Value::Bool(value) => put_u32(&mut bytes, offset, u32::from(*value)),
                    Value::Array(values) => {
                        for (lane, value) in values.iter().take(4).enumerate() {
                            if let Some(value) = value.as_f64() {
                                put_f32(&mut bytes, offset + lane * 4, value as f32);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(bytes)
        }
    }

    fn put_f32(bytes: &mut [u8], offset: usize, value: f32) {
        let end = offset + 4;
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        let end = offset + 4;
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        let end = offset + 4;
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn write_scalar(
        bytes: &mut [u8],
        offset: usize,
        kind: &str,
        value: Value,
    ) -> Result<(), String> {
        if offset + 4 > bytes.len() {
            return Err(format!("uniform member at {offset} is outside the block"));
        }
        match value {
            Value::Number(number) if number.as_i64().is_some() => {
                let number = number.as_i64().unwrap() as i32;
                if kind == "int" {
                    put_i32(bytes, offset, number);
                } else {
                    put_f32(bytes, offset, number as f32);
                }
            }
            Value::Number(number) => {
                put_f32(bytes, offset, number.as_f64().unwrap_or(0.0) as f32);
            }
            Value::Bool(value) => put_u32(bytes, offset, u32::from(value)),
            Value::Array(values) => write_vec(
                bytes,
                offset,
                &values
                    .iter()
                    .filter_map(|value| value.as_f64().map(|value| value as f32))
                    .take(4)
                    .collect::<Vec<_>>(),
            )?,
            _ => {}
        }
        Ok(())
    }

    fn write_vec(bytes: &mut [u8], offset: usize, values: &[f32]) -> Result<(), String> {
        let lanes = values.len();
        if offset + lanes * 4 > bytes.len() {
            return Err(format!("uniform member at {offset} is outside the block"));
        }
        for (lane, value) in values.iter().enumerate() {
            put_f32(bytes, offset + lane * 4, *value);
        }
        Ok(())
    }
}

use wasm_bindgen::prelude::*;

/// JavaScript-facing preview runtime.
#[wasm_bindgen]
pub struct PreviewEngine {
    inner: runtime::PreviewRuntime,
}

#[wasm_bindgen]
impl PreviewEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32, bpm: f32) -> Self {
        Self {
            inner: runtime::PreviewRuntime::new(width, height, bpm),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.inner.resize(width, height);
    }

    pub fn set_time(&mut self, seconds: f32) {
        self.inner.set_time(seconds);
    }

    pub fn tick(&mut self, dt_seconds: f32) {
        self.inner.tick(dt_seconds);
    }

    pub fn kick(&self) -> bool {
        self.inner.kick()
    }

    pub fn bass(&self) -> f32 {
        self.inner.bass()
    }

    pub fn mid(&self) -> f32 {
        self.inner.mid()
    }

    pub fn treble(&self) -> f32 {
        self.inner.treble()
    }

    /// 160-byte `ShaderUniforms` block for built-in WGSL modules.
    pub fn builtin_uniform_bytes(&self) -> Vec<u8> {
        self.inner.builtin_uniform_bytes().to_vec()
    }

    /// Variable-size ISF uniform block for the given layout and prop defaults.
    pub fn isf_uniform_bytes(&self, layout: &str, props: &str) -> Result<Vec<u8>, String> {
        self.inner.isf_uniform_bytes(layout, props)
    }

    /// 16-byte-slot prop buffer for built-in WGSL modules.
    pub fn prop_bytes(&self, props: &str) -> Result<Vec<u8>, String> {
        self.inner.prop_bytes(props)
    }
}
