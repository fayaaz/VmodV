//! CPU-side MilkDrop runtime core (Layer 5a).
//!
//! This module turns a parsed [`MilkDocument`] into an owned, device-free
//! [`MilkCompiledPreset`], then drives one or more [`MilkInstance`]s frame by
//! frame. Every piece in here is pure CPU logic and is unit-tested **without a
//! wgpu device**: equation compilation/evaluation, per-frame globals/audio
//! mapping, band followers, q/reset persistence and the warped mesh UV grid.
//!
//! # Layering / GPU hand-off (Layer 5b)
//!
//! [`MilkRuntime`] is the per-compositor owner defined here. In this CPU-only
//! cycle it holds only the device-free [`MilkFailureCache`]; the GPU cycle will
//! attach `wgpu::Device`, `wgpu::Queue`, a pipeline cache keyed by module name
//! and a sampler cache to it (behind a constructor like
//! `MilkRuntime::with_device(device, queue)`), without changing any of the
//! types below. In particular:
//!
//! * [`MilkCompiledPreset`] already owns both translated WGSL shader modules
//!   ([`warp`]/[`comp`]); the GPU cycle builds `wgpu::RenderPipeline`s from
//!   `compiled.warp.wgsl` / `compiled.comp.wgsl`.
//! * [`MilkMesh::uvs`] is the vertex buffer the GPU cycle uploads once per
//!   frame to drive the warp pass.
//! * The wave/spectrum arrays copied onto [`MilkInstance`] become the shader
//!   uniform/storage buffers for the wave consumers.
//!
//! # Per-frame contract (what the tests pin down)
//!
//! 1. [`begin_frame`] runs the preset's init equations exactly once (on the
//!    first frame of an instance), then [`VarPool::snapshot_q`] records the
//!    `q1..q32` baseline. Every subsequent frame restores that baseline via
//!    [`run_frame`] before running the per-frame equations, then sets the
//!    globals (see below) and runs those equations.
//! 2. Globals are built from [`MilkFrameInfo`]:
//!    `time = time_seconds`, `frame = frame_index`, `fps`, `meshx`, `meshy`,
//!    `pixelsx/pixelsy = width/height`. Aspect is chosen so the *smaller*
//!    axis is `1.0` and the larger axis carries the ratio: for `w >= h`,
//!    `aspectx = 1.0` and `aspecty = h / w`; otherwise `aspectx = w / h` and
//!    `aspecty = 1.0`.
//! 3. Audio is mapped as `bass = audio.bass * 2.0`, `mid = audio.mid * 2.0`,
//!    `treb = audio.treble * 2.0`. The `*_att` followers are per-instance and
//!    persist across frames: `att += (value - att) * 0.2`, starting at `0.0`.
//! 4. [`evaluate_mesh`] walks a `(meshx + 1) × (meshy + 1)` vertex grid. Each
//!    vertex gets `x`/`y` normalised to `-1.0..=1.0` (center at `(0, 0)`,
//!    `y` increasing upward), `rad = hypot(x, y)` and `ang = atan2(y, x)`, then
//!    runs the per-pixel equations. The returned UV is computed from
//!    `zoom`, `zoomexp`, `cx/cy`, `sx/sy`, `rot`, `dx/dy` and the four warp
//!    sine/cosine terms derived from `warp`/`warpscale`/`warpanimspeed`/`time`.
//!    The exact arithmetic is intentionally left to the implementation; the
//!    tests assert grid size, finiteness and determinism.
//!
//! # Compilation failures
//!
//! [`compile_preset`] records a failing module name in the supplied
//! [`MilkFailureCache`] (shared with the compositor, cleared on registry
//! replacement) and returns [`MilkRuntimeError::Compile`] instead of
//! panicking. A preset with an untranslatable warp or comp shader is therefore
//! skipped safely.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::milkdrop::eel::{CompiledEquations, Globals, VarPool, compile, run_frame, run_pixel};
use crate::milkdrop::preset::{MilkBaseVals, MilkDocument, extract_shader_body};
use crate::milkdrop::translate::{CompiledMilkShader, MilkShaderStage, translate_milk_shader};

/// MilkDrop scales each raw audio band by two before exposing it to EEL.
const AUDIO_GAIN: f32 = 2.0;
/// Per-frame interpolation rate for the `*_att` band follower variables.
const BAND_FOLLOWER_RATE: f32 = 0.2;
/// Warp displacement is `warp * warpscale * WARP_AMOUNT_SCALE`.
const WARP_AMOUNT_SCALE: f32 = 0.01;
/// Denominators smaller than this are treated as degenerate in the warp mesh.
const WARP_EPS: f32 = 1e-6;
/// Sampling UV used when the warp arithmetic would otherwise be non-finite.
const UV_FALLBACK: f32 = 0.5;

/// Audio snapshot consumed for one frame.
///
/// `wave` and `byte_frequency_data` are kept as `Vec`s (rather than fixed
/// arrays) so callers can pass whatever their capture backend produced; the
/// runtime copies them onto the instance for the GPU cycle. The canonical
/// lengths are 1024 samples each.
#[derive(Clone, Debug, Default)]
pub struct MilkAudio {
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub wave: Vec<f32>,
    pub byte_frequency_data: Vec<u8>,
}

/// All per-frame inputs to the CPU runtime.
#[derive(Clone, Debug)]
pub struct MilkFrameInfo {
    pub time_seconds: f64,
    pub frame_index: u32,
    pub fps: f32,
    /// Texture/render target width (feeds `pixelsx`).
    pub width: u32,
    /// Texture/render target height (feeds `pixelsy`).
    pub height: u32,
    pub meshx: u32,
    pub meshy: u32,
    pub audio: MilkAudio,
}

/// An owned, compiled preset, ready to be instantiated.
#[derive(Clone, Debug)]
pub struct MilkCompiledPreset {
    pub doc: MilkDocument,
    pub init: CompiledEquations,
    pub frame: CompiledEquations,
    pub pixel: CompiledEquations,
    pub warp: CompiledMilkShader,
    pub comp: CompiledMilkShader,
}

/// Typed failures from compiling or driving a MilkDrop preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MilkRuntimeError {
    /// Compiling the equations or translating a shader failed.
    Compile(String),
    /// Running an equation set for a frame/vertex failed.
    Evaluate(String),
}

/// Names of modules whose preset failed to compile.
///
/// Device-free and shareable: the compositor owns one cache per registry and
/// calls [`MilkFailureCache::clear`] from `Compositor::set_registry` (next
/// integration cycle). Interior mutability keeps the `compile_preset(&cache)`
/// signature borrow-only.
#[derive(Clone, Debug, Default)]
pub struct MilkFailureCache {
    // `Arc` keeps the handle `Clone` (and shareable with the compositor) while
    // `Mutex` gives the `&self` methods interior mutability. Poison is recovered
    // rather than propagated: a failed compilation must never abort rendering.
    failed: Arc<Mutex<HashSet<String>>>,
}

impl MilkFailureCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `name` as having failed compilation.
    pub fn mark_failed(&self, name: &str) {
        let mut failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        failed.insert(name.to_string());
    }

    /// Whether `name` is currently marked as failed.
    pub fn is_failed(&self, name: &str) -> bool {
        let failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        failed.contains(name)
    }

    /// Forget all recorded failures.
    pub fn clear(&self) {
        let mut failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        failed.clear();
    }
}

/// Per-compositor owner of MilkDrop state.
///
/// Device-free in this cycle; the GPU cycle attaches `wgpu::Device`/`Queue`
/// and pipeline/sampler caches here without renaming this type.
#[derive(Clone, Debug, Default)]
pub struct MilkRuntime {
    pub failures: MilkFailureCache,
}

/// Compile a parsed preset into an owned handle.
///
/// On a shader/equation error the offending `name` is recorded in `failures`
/// and [`MilkRuntimeError::Compile`] is returned; this never panics.
pub fn compile_preset(
    name: &str,
    doc: &MilkDocument,
    failures: &MilkFailureCache,
) -> Result<MilkCompiledPreset, MilkRuntimeError> {
    // One failure path for every stage: record the module, then return a typed
    // error. Closures keep the borrow-only cache signature intact.
    let fail = |message: String| {
        failures.mark_failed(name);
        MilkRuntimeError::Compile(message)
    };

    let init =
        compile(&doc.init_eqs, doc.version).map_err(|e| fail(format!("init equations: {e}")))?;
    let frame =
        compile(&doc.frame_eqs, doc.version).map_err(|e| fail(format!("frame equations: {e}")))?;
    let pixel =
        compile(&doc.pixel_eqs, doc.version).map_err(|e| fail(format!("pixel equations: {e}")))?;

    let warp_src = extract_shader_body(&doc.warp);
    let warp = translate_milk_shader(&warp_src.prelude, &warp_src.body, MilkShaderStage::Warp)
        .map_err(|e| fail(format!("warp shader: {e}")))?;

    let comp_src = extract_shader_body(&doc.comp);
    let comp = translate_milk_shader(&comp_src.prelude, &comp_src.body, MilkShaderStage::Comp)
        .map_err(|e| fail(format!("comp shader: {e}")))?;

    Ok(MilkCompiledPreset {
        doc: doc.clone(),
        init,
        frame,
        pixel,
        warp,
        comp,
    })
}

/// One active module instance: compiled equations, a persistent variable pool
/// (which also holds the seeded RNG), the latest globals, persistent band
/// followers and the copied wave/spectrum data.
#[derive(Clone, Debug)]
pub struct MilkInstance {
    pub compiled: MilkCompiledPreset,
    pub pool: VarPool,
    pub globals: Globals,
    /// `[bass, mid, treble]` exponential followers, persistent across frames.
    pub band_att: [f32; 3],
    pub frame_index: u32,
    /// Whether the init equations have run for this preset.
    pub init_done: bool,
    pub wave: Vec<f32>,
    pub byte_frequency_data: Vec<u8>,
}

/// Build an instance for `compiled` whose `rand` draws are reproducible via
/// `seed`.
pub fn new_instance(compiled: MilkCompiledPreset, seed: u64) -> MilkInstance {
    MilkInstance {
        compiled,
        pool: VarPool::with_seed(seed),
        globals: Globals::default(),
        band_att: [0.0; 3],
        frame_index: 0,
        init_done: false,
        wave: Vec::new(),
        byte_frequency_data: Vec::new(),
    }
}

/// Advance `instance` to `frame`: run init once, build globals/audio, update
/// the band followers and run the per-frame equations.
pub fn begin_frame(
    instance: &mut MilkInstance,
    frame: &MilkFrameInfo,
) -> Result<(), MilkRuntimeError> {
    // (1) Init equations run exactly once and capture the q1..q32 baseline.
    // `compiled`, `pool` and `globals` are disjoint fields, so they can be
    // borrowed independently without cloning the compiled equations.
    if !instance.init_done {
        run_frame(
            &instance.compiled.init,
            &mut instance.pool,
            &Globals::default(),
        )
        .map_err(|e| MilkRuntimeError::Evaluate(format!("init equations: {e}")))?;
        instance.pool.snapshot_q();
        instance.init_done = true;
    }

    // (2) Rebuild the fifteen globals from this frame's geometry and audio.
    let audio = &frame.audio;
    let values = [
        audio.bass * AUDIO_GAIN,
        audio.mid * AUDIO_GAIN,
        audio.treble * AUDIO_GAIN,
    ];
    for (att, value) in instance.band_att.iter_mut().zip(values) {
        *att += (value - *att) * BAND_FOLLOWER_RATE;
    }

    let (aspectx, aspecty) = if frame.width >= frame.height {
        (1.0, frame.height as f32 / frame.width as f32)
    } else {
        (frame.width as f32 / frame.height as f32, 1.0)
    };

    instance.globals = Globals {
        time: frame.time_seconds as f32,
        frame: frame.frame_index as f32,
        fps: frame.fps,
        meshx: frame.meshx as f32,
        meshy: frame.meshy as f32,
        pixelsx: frame.width as f32,
        pixelsy: frame.height as f32,
        aspectx,
        aspecty,
        bass: values[0],
        bass_att: instance.band_att[0],
        mid: values[1],
        mid_att: instance.band_att[1],
        treb: values[2],
        treb_att: instance.band_att[2],
    };
    instance.frame_index = frame.frame_index;
    instance.wave = frame.audio.wave.clone();
    instance.byte_frequency_data = frame.audio.byte_frequency_data.clone();

    // (3) Per-frame equations: `run_frame` restores the captured q baseline
    // before injecting the globals, so q changes never leak into the next frame.
    run_frame(
        &instance.compiled.frame,
        &mut instance.pool,
        &instance.globals,
    )
    .map_err(|e| MilkRuntimeError::Evaluate(format!("frame equations: {e}")))?;
    Ok(())
}

/// The warped mesh produced for one frame.
#[derive(Clone, Debug, Default)]
pub struct MilkMesh {
    /// One `[u, v]` per vertex, row-major over `width`×`height`.
    pub uvs: Vec<[f32; 2]>,
    /// `meshx + 1`.
    pub width: u32,
    /// `meshy + 1`.
    pub height: u32,
}

/// Resolve one warp parameter from the live EEL pool, falling back to the
/// preset's base-val default.
///
/// Per-frame equations may override any of these variables at runtime. The
/// pool is not seeded with the base vals, so a `0.0` read means "untouched"
/// and the base value wins. For a hand-written fixture whose base vals are all
/// zero this leaves the parameter at zero (the warp math below guards it),
/// keeping the result finite and deterministic.
fn warp_param(pool: &VarPool, name: &str, base: f64) -> f32 {
    let live = pool.get(name);
    if live == 0.0 { base as f32 } else { live }
}

/// `value` when it is finite and non-degenerate, otherwise `fallback`.
fn nonzero_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value.abs() > WARP_EPS {
        value
    } else {
        fallback
    }
}

/// `value` when finite, otherwise `fallback` (the texture centre for UVs).
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

/// Map a mesh vertex's `[-1, 1]` position to a warped sampling UV.
///
/// This mirrors MilkDrop's warp mesh: warp displacement, rotation, zoom (with
/// exponent), centre/offset (`cx`/`cy`/`dx`/`dy`) and per-axis stretch
/// (`sx`/`sy`). Parameter values come from the EEL pool (per-frame overrides)
/// with the preset's base vals as defaults. Every divisor is guarded so a
/// hand-written fixture with all-zero base vals still yields finite,
/// deterministic UVs; a non-finite result falls back to the texture centre.
fn warped_uv(pool: &VarPool, base: &MilkBaseVals, x: f32, y: f32, time: f32) -> [f32; 2] {
    let warp = warp_param(pool, "warp", base.warp);
    let warp_anim = warp_param(pool, "warpanimspeed", base.warpanimspeed);
    let warp_scale = warp_param(pool, "warpscale", base.warpscale);
    let t = time * warp_anim;
    let amount = warp * warp_scale * WARP_AMOUNT_SCALE;
    let wx = x + amount * (t.sin() + (2.0 * t).sin());
    let wy = y + amount * (t.cos() + (2.0 * t).cos());

    let zoom = nonzero_or(warp_param(pool, "zoom", base.zoom), 1.0);
    let zoomexp = warp_param(pool, "zoomexp", base.zoomexp);
    let zoom_factor = nonzero_or(zoom.powf(zoomexp), 1.0);

    let sx = nonzero_or(warp_param(pool, "sx", base.sx), 1.0);
    let sy = nonzero_or(warp_param(pool, "sy", base.sy), 1.0);
    let (sin_rot, cos_rot) = warp_param(pool, "rot", base.rot).sin_cos();
    let rx = wx * cos_rot - wy * sin_rot;
    let ry = wx * sin_rot + wy * cos_rot;

    let cx = warp_param(pool, "cx", base.cx);
    let cy = warp_param(pool, "cy", base.cy);
    let dx = warp_param(pool, "dx", base.dx);
    let dy = warp_param(pool, "dy", base.dy);
    let u = (rx / (sx * zoom_factor) + cx + dx) * 0.5 + 0.5;
    let v = (ry / (sy * zoom_factor) + cy + dy) * 0.5 + 0.5;
    [finite_or(u, UV_FALLBACK), finite_or(v, UV_FALLBACK)]
}

/// Evaluate the per-pixel equations and warped UV grid for `frame`.
///
/// The caller must have called [`begin_frame`] for the same frame first.
pub fn evaluate_mesh(
    instance: &mut MilkInstance,
    frame: &MilkFrameInfo,
) -> Result<MilkMesh, MilkRuntimeError> {
    let width = frame.meshx + 1;
    let height = frame.meshy + 1;
    let time = instance.globals.time;

    let mut uvs = Vec::with_capacity((width as usize).saturating_mul(height as usize));
    for row in 0..height {
        let y = if height > 1 {
            1.0 - 2.0 * (row as f32 / (height - 1) as f32)
        } else {
            0.0
        };
        for col in 0..width {
            let x = if width > 1 {
                2.0 * (col as f32 / (width - 1) as f32) - 1.0
            } else {
                0.0
            };
            let rad = x.hypot(y);
            let ang = y.atan2(x);
            // `compiled` and `pool` are disjoint fields: borrow the equations
            // instead of cloning them per vertex.
            run_pixel(&instance.compiled.pixel, &mut instance.pool, x, y, rad, ang)
                .map_err(|e| MilkRuntimeError::Evaluate(format!("pixel equations: {e}")))?;
            uvs.push(warped_uv(
                &instance.pool,
                &instance.compiled.doc.base,
                x,
                y,
                time,
            ));
        }
    }

    Ok(MilkMesh { uvs, width, height })
}

// ---------------------------------------------------------------------------
// RED tests — pure CPU, no GPU device required.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! approx {
        ($got:expr, $want:expr) => {
            approx!($got, $want, "")
        };
        ($got:expr, $want:expr, $msg:expr) => {{
            let got = $got;
            let want = $want;
            assert!(
                (got - want).abs() < 1e-4,
                "expected {want}, got {got} (delta {}) -- {}",
                (got - want).abs(),
                $msg
            );
        }};
    }

    const VALID_WARP: &str = "shader_body {\n\
float2 uv = uv_orig + float2(roam_cos, roam_sin) * 0.01;\n\
float3 ret = tex2D(sampler_main, uv).rgb;\n\
}\n";

    const VALID_COMP: &str = "shader_body {\n\
float3 ret = GetMain(uv_orig).rgb;\n\
}\n";

    const MALFORMED_WARP: &str = "shader_body {\n\
float2 uv = ;\n\
this is not valid GLSL @@@\n\
ret = uv;\n\
}\n";

    fn doc_with(init: &str, frame: &str, pixel: &str, warp: &str, comp: &str) -> MilkDocument {
        MilkDocument {
            version: 2,
            name: "fixture".to_string(),
            init_eqs: init.to_string(),
            frame_eqs: frame.to_string(),
            pixel_eqs: pixel.to_string(),
            warp: warp.to_string(),
            comp: comp.to_string(),
            ..Default::default()
        }
    }

    fn frame(bass: f32, mid: f32, treble: f32, index: u32, time: f64) -> MilkFrameInfo {
        MilkFrameInfo {
            time_seconds: time,
            frame_index: index,
            fps: 60.0,
            width: 640,
            height: 360,
            meshx: 8,
            meshy: 6,
            audio: MilkAudio {
                bass,
                mid,
                treble,
                wave: vec![0.0; 1024],
                byte_frequency_data: vec![128; 1024],
            },
        }
    }

    #[test]
    fn full_preset_two_frames_q_reset_band_followers_and_determinism() {
        let doc = doc_with(
            "q1 = 3; q2 = 7;",
            "seen_q1 = q1; q1 = q1 + 1; j = rand(1000);",
            "px = x + y + rad + ang;",
            VALID_WARP,
            VALID_COMP,
        );
        let cache = MilkFailureCache::new();
        let compiled_a = compile_preset("fixture/a", &doc, &cache).expect("fixture compiles");
        let compiled_b = compile_preset("fixture/b", &doc, &cache).expect("fixture compiles");
        assert!(
            !cache.is_failed("fixture/a"),
            "valid preset must not be cached failed"
        );

        let mut a = new_instance(compiled_a, 0xFEED);
        let mut b = new_instance(compiled_b, 0xFEED);

        let f1 = frame(0.5, 0.25, 1.0, 1, 0.016);
        let f2 = frame(0.5, 0.25, 1.0, 2, 0.032);

        begin_frame(&mut a, &f1).expect("frame 1 on a");
        begin_frame(&mut a, &f2).expect("frame 2 on a");
        begin_frame(&mut b, &f1).expect("frame 1 on b");
        begin_frame(&mut b, &f2).expect("frame 2 on b");

        // (1) q reset: baseline from init is q1=3; each frame adds 1, so after
        // two frames q1 is 4 (NOT 5), and the frame equation saw baseline 3.
        approx!(
            a.pool.get("q1"),
            4.0,
            "q1 restored to init baseline each frame"
        );
        approx!(
            a.pool.get("seen_q1"),
            3.0,
            "per-frame equations see baseline q1"
        );

        // (2) band followers: value = audio*2, att += (value - att)*0.2.
        // frame1: 0.2 / 0.1 / 0.4 ; frame2: 0.36 / 0.18 / 0.72.
        approx!(a.globals.bass_att, 0.36, "bass_att across two frames");
        approx!(a.globals.mid_att, 0.18, "mid_att across two frames");
        approx!(a.globals.treb_att, 0.72, "treb_att across two frames");

        // (3) determinism: two fresh instances with the same seed agree.
        assert_eq!(a.globals, b.globals, "globals identical for equal seeds");
        approx!(a.pool.get("q1"), b.pool.get("q1"), "q1 identical");
        approx!(a.pool.get("j"), b.pool.get("j"), "seeded rand identical");
        let ma = evaluate_mesh(&mut a, &f2).expect("mesh a");
        let mb = evaluate_mesh(&mut b, &f2).expect("mesh b");
        assert_eq!(ma.uvs, mb.uvs, "UV grid identical for equal seeds");
    }

    #[test]
    fn globals_audio_mapping_and_aspect() {
        let doc = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("map", &doc, &cache).expect("compiles");

        let mut f = frame(0.5, 0.25, 1.0, 42, 2.5);
        f.fps = 30.0;
        f.width = 1920;
        f.height = 1080;
        f.meshx = 16;
        f.meshy = 9;
        for (i, sample) in f.audio.wave.iter_mut().enumerate() {
            *sample = i as f32 * 0.5;
        }
        for (i, byte) in f.audio.byte_frequency_data.iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }

        let mut instance = new_instance(compiled.clone(), 7);
        begin_frame(&mut instance, &f).expect("landscape frame");

        approx!(instance.globals.time, 2.5, "time_seconds -> time");
        approx!(instance.globals.frame, 42.0, "frame_index -> frame");
        approx!(instance.globals.fps, 30.0);
        approx!(instance.globals.meshx, 16.0);
        approx!(instance.globals.meshy, 9.0);
        approx!(instance.globals.pixelsx, 1920.0);
        approx!(instance.globals.pixelsy, 1080.0);
        approx!(instance.globals.bass, 1.0, "audio.bass * 2.0");
        approx!(instance.globals.mid, 0.5, "audio.mid * 2.0");
        approx!(instance.globals.treb, 2.0, "audio.treble * 2.0");
        approx!(instance.globals.bass_att, 0.2, "first-frame bass follower");
        approx!(instance.globals.mid_att, 0.1, "first-frame mid follower");
        approx!(
            instance.globals.treb_att,
            0.4,
            "first-frame treble follower"
        );
        approx!(instance.globals.aspectx, 1.0, "landscape aspectx");
        approx!(
            instance.globals.aspecty,
            1080.0 / 1920.0,
            "landscape aspecty"
        );
        assert_eq!(instance.frame_index, 42);
        assert_eq!(instance.wave, f.audio.wave, "wave data copied to instance");
        assert_eq!(
            instance.byte_frequency_data, f.audio.byte_frequency_data,
            "spectrum data copied to instance"
        );

        // Portrait: the smaller axis becomes 1.0, the wider axis carries ratio.
        let mut fp = f.clone();
        fp.width = 600;
        fp.height = 800;
        let mut portrait = new_instance(compiled, 8);
        begin_frame(&mut portrait, &fp).expect("portrait frame");
        approx!(portrait.globals.aspectx, 600.0 / 800.0, "portrait aspectx");
        approx!(portrait.globals.aspecty, 1.0, "portrait aspecty");
    }

    #[test]
    fn mesh_uv_grid_length_and_finite() {
        let doc = doc_with("", "", "px = x + y + rad + ang;", VALID_WARP, VALID_COMP);
        let cache = MilkFailureCache::new();
        let compiled = compile_preset("mesh", &doc, &cache).expect("compiles");
        let mut instance = new_instance(compiled, 0x1234);

        let mut f = frame(0.2, 0.3, 0.4, 3, 1.0);
        f.meshx = 4;
        f.meshy = 3;
        f.width = 320;
        f.height = 240;

        begin_frame(&mut instance, &f).expect("frame");
        let mesh = evaluate_mesh(&mut instance, &f).expect("mesh");

        assert_eq!(mesh.width, 5, "width = meshx + 1");
        assert_eq!(mesh.height, 4, "height = meshy + 1");
        assert_eq!(
            mesh.uvs.len(),
            ((f.meshx + 1) * (f.meshy + 1)) as usize,
            "one UV per grid vertex"
        );
        for (i, uv) in mesh.uvs.iter().enumerate() {
            assert!(uv[0].is_finite(), "u[{i}] finite, got {}", uv[0]);
            assert!(uv[1].is_finite(), "v[{i}] finite, got {}", uv[1]);
        }
    }

    #[test]
    fn failure_cache_mark_query_clear() {
        let cache = MilkFailureCache::new();
        assert!(!cache.is_failed("mod/a"), "fresh cache is empty");
        cache.mark_failed("mod/a");
        assert!(cache.is_failed("mod/a"), "marked module is failed");
        assert!(!cache.is_failed("mod/b"), "other modules unaffected");

        let runtime = MilkRuntime::default();
        assert!(
            !runtime.failures.is_failed("mod/a"),
            "runtime owns its own cache"
        );

        cache.clear();
        assert!(!cache.is_failed("mod/a"), "clear forgets failures");
    }

    #[test]
    fn shader_compilation_valid_and_malformed() {
        let cache = MilkFailureCache::new();

        let good = doc_with("", "", "", VALID_WARP, VALID_COMP);
        let compiled = compile_preset("good", &good, &cache).expect("valid shaders compile");
        assert_eq!(compiled.warp.stage, MilkShaderStage::Warp);
        assert_eq!(compiled.comp.stage, MilkShaderStage::Comp);
        assert!(!compiled.warp.wgsl.is_empty(), "valid warp yields WGSL");
        assert!(!compiled.comp.wgsl.is_empty(), "valid comp yields WGSL");

        let bad = doc_with("", "", "", MALFORMED_WARP, VALID_COMP);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compile_preset("broken", &bad, &cache)
        }));
        let result = result.expect("malformed shader must not panic");
        assert!(result.is_err(), "malformed shader returns an error");
        assert!(
            cache.is_failed("broken"),
            "malformed shader marks the module failed"
        );
    }
}
