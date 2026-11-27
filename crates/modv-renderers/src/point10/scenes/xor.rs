//! Plane9 "Xor" — Rust/wgpu port of `DemoScene/Xor.xml`.
//!
//! Source: `../Point10/public/plane9/xml/DemoScene/Xor.xml` by Joakim Dahl,
//! CC BY-NC-SA 3.0. Three rings whose centres wander with a noise field are
//! XORed together: each ring's distance is taken modulo `0.25` and `step`ped
//! against `gIn3.z`, then the three binary results are folded with GLSL
//! `mod(x + y, 2.0)`.
//!
//! # Ported exactly
//!
//! * `radius = vec3(length(p - gIn1.xy), length(p - gIn2.xy),
//!   length(p - gIn3.xy))`;
//! * `col = step(mod(radius, 0.25), gIn3.zzz)` and the two `mod` folds;
//! * the expression `perm += (beat(0.05) + 0.01) * dt`, with `pos = perm` and
//!   six `noise2(pos, seed)` lanes for the ring centres;
//! * `in3.z = 0.002 + band(-1, 1, 0, 0.14)`.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{Plane9Pass, Plane9Uniforms};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::shared::random::value_noise2;
use crate::point10::{
    HDR_FORMAT, Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Clear1` colour (opaque black).
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// Ring spacing (`mod(radius, 0.25)`).
pub const RING_MOD: f32 = 0.25;
/// `in3.z` floor.
pub const THRESHOLD_FLOOR: f32 = 0.002;

const XOR_PROPS: &[SceneProp] = &[];

/// One frame of the Xor expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XorDrive {
    /// `perm` for the next frame.
    pub perm: f32,
    /// `gIn1`.
    pub in1: [f32; 4],
    /// `gIn2`.
    pub in2: [f32; 4],
    /// `gIn3`.
    pub in3: [f32; 4],
}

/// Pure port of the Xor expression.
pub fn xor_drive(beat: f32, threshold_band: f32, dt: f32, perm: f32) -> XorDrive {
    let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
    let threshold_band = if threshold_band.is_finite() {
        threshold_band.max(0.0)
    } else {
        0.0
    };
    let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let perm = if perm.is_finite() { perm } else { 1.0 };
    let next = perm + (beat + 0.01) * step;
    let pos = next;
    XorDrive {
        perm: next,
        in1: [value_noise2(pos, 1.0), value_noise2(pos, 2.0), 0.0, 0.0],
        in2: [value_noise2(pos, 3.0), value_noise2(pos, 4.0), 0.0, 0.0],
        in3: [
            value_noise2(pos, 5.0),
            value_noise2(pos, 6.0),
            THRESHOLD_FLOOR + threshold_band,
            0.0,
        ],
    }
}

/// `Plane9 Xor` scene.
pub struct Xor {
    audio: P10Audio,
    perm: f32,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    uniforms: Plane9Uniforms,
    pass: Option<Plane9Pass>,
}

impl Default for Xor {
    fn default() -> Self {
        Self::new()
    }
}

impl Xor {
    /// Create the scene.
    pub fn new() -> Self {
        Self {
            audio: P10Audio::new(),
            perm: 1.0,
            elapsed: 0.0,
            size: (1, 1),
            target_format: HDR_FORMAT,
            device: None,
            queue: None,
            uniforms: Plane9Uniforms::new(1, 1),
            pass: None,
        }
    }

    /// The current `perm`.
    pub fn perm(&self) -> f32 {
        self.perm
    }

    /// The uniform block last uploaded.
    pub fn uniforms(&self) -> &Plane9Uniforms {
        &self.uniforms
    }

    /// Bind the modV props. These presets declare no `sceneProps` of their
    /// own, so this only exists to satisfy the registry's prop binder.
    pub fn apply_props(&mut self, _props: impl FnMut(&str) -> Option<f32>) {}

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        self.pass = Some(Plane9Pass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-xor",
            xor_wgsl(),
            None,
        ));
    }
}

fn xor_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| {
        format!(
            "{P9_COMMON_WGSL}\n{}\n{XOR_FRAGMENT}",
            crate::point10::scenes::plane9_common::PLANE9_BATCH_WGSL
        )
    })
}

const XOR_FRAGMENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let radius = vec3<f32>(
    length(p - u.in1.xy),
    length(p - u.in2.xy),
    length(p - u.in3.xy),
  );
  let col = p9_step3(
    vec3<f32>(
      p9_mod_f(radius.x, 0.25),
      p9_mod_f(radius.y, 0.25),
      p9_mod_f(radius.z, 0.25),
    ),
    u.in3.zzz,
  );
  let c = p9_mod_f(p9_mod_f(col.x + col.y, 2.0) + col.z, 2.0);
  return vec4<f32>(c, c, c, 1.0);
}
"#;

impl Point10Scene for Xor {
    fn name(&self) -> &'static str {
        "Plane9 Xor"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.rebuild();
        if let Some(pass) = &self.pass {
            pass.upload(&self.uniforms);
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if self.size == size {
            return;
        }
        self.size = size;
        self.uniforms.resolution = [size.0 as f32, size.1 as f32];
        self.rebuild();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;
        let drive = xor_drive(
            self.audio.beat(0.05),
            self.audio.band(-1, 1, 0, 0.14),
            dt,
            self.perm,
        );
        self.perm = drive.perm;
        self.uniforms.time = self.elapsed;
        self.uniforms.in1 = drive.in1;
        self.uniforms.in2 = drive.in2;
        self.uniforms.in3 = drive.in3;
        if let Some(pass) = &self.pass {
            pass.upload(&self.uniforms);
        }
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(pass) = &self.pass {
            pass.draw(encoder, target, CLEAR, "point10-plane9-xor");
        }
    }

    fn dispose(&mut self) {
        self.pass = None;
        self.device = None;
        self.queue = None;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(XOR_PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Xor", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::test_support::{TestTarget, any_lit, gpu};

    #[test]
    fn camera_and_clear_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR.a, 1.0);
    }

    #[test]
    fn drive_matches_the_expression() {
        let drive = xor_drive(0.5, 0.4, 0.5, 1.0);
        assert!((drive.perm - (1.0 + 0.51 * 0.5)).abs() < 1e-6);
        assert!((drive.in1[0] - value_noise2(drive.perm, 1.0)).abs() < 1e-6);
        assert!((drive.in3[2] - (THRESHOLD_FLOOR + 0.4)).abs() < 1e-6);
    }

    #[test]
    fn wgsl_parses_and_keeps_the_ring_algebra() {
        use crate::point10::scenes::plane9_common::PLANE9_UNIFORMS_WGSL;
        let source = format!("{}\n{}", PLANE9_UNIFORMS_WGSL, xor_wgsl());
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("Xor WGSL failed to parse: {error}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Xor WGSL validates");
        assert!(xor_wgsl().contains("p9_mod_f(radius.x, 0.25)"));
        assert!(xor_wgsl().contains("p9_mod_f(col.x + col.y, 2.0) + col.z"));
    }

    #[test]
    fn renders_two_frames_non_black() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = Xor::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.6; 12];
        payload.beat = 0.8;
        let target = TestTarget::new(&device, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));
        assert!(any_lit(&target.read(&device, &queue)));
    }
}
