//! Plane9 "Stargate Event Horizon" — Rust/wgpu port of
//! `Other/Stargate Event Horizon.xml`.
//!
//! Source: `../Point10/public/plane9/xml/Other/Stargate Event Horizon.xml` by
//! Joakim Dahl, CC BY-NC-SA 3.0.
//!
//! # The graph
//!
//! ```text
//! Clear1 + Shader1 (portal lighting)      -> RenderToTexture1 (RGBA8)
//!   -> CopyTexture1 (c*c.a, 12.5%) -> Blur1 (separable, Width 4, Brightness 4)
//!   -> Shader2 (blur + raw, squared) -> RenderRect2 -> Screen1
//! ```
//!
//! # Ported exactly
//!
//! * `CamPos 0 0 -2`, `CamLookAt 0 0 1`;
//! * `Clear1 0 0 0 1`, `Clear2 0 0 0 0`;
//! * `Color1 0.188 0.447 0.643 0.01`;
//! * the Ashima simplex noise (`mod289`, `permute`, `taylorInvSqrt`, gradient
//!   scale 42) driving the height field, the `0.01` gradient epsilon, the
//!   `diffusePower = 0.8 + gIn1.y`, the `specularHardness 12`,
//!   `specularPower 0.6`, and the `(hardness+4)/8` specular normalisation;
//! * the expression `in1.x = time*0.2`, `in1.y = min(band(-1,1,0,0)*2, 0.5)`;
//! * `CopyTexture1` at `TextureSize 3` (12.5%) with `c*c.a`, `Blur1` with
//!   `Dir 2`, `Width 4`, `Brightness 4`, and `(c1 + c2)²`.
//!
//! `Blur1` is a separable Gaussian in this port; the reference JS reuses its
//! five-mip bloom chain, which is an approximation of the same node.

#![forbid(unsafe_code)]

use crate::point10::audio_bridge::{AudioBridge, P10Audio};
use crate::point10::scenes::plane9_common::{
    Plane9DualPass, Plane9Pass, Plane9TextureBinding, Plane9Uniforms, create_colour_target,
    plane9_sampler,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::{
    Point10Scene, SceneContext, SceneMeta, SceneProp, frame_dt, scene_props_with_levers,
};

/// `Screen1` camera position.
pub const CAMERA_POSITION: [f32; 3] = [0.0, 0.0, -2.0];
/// `Screen1` camera look direction.
pub const CAMERA_LOOK: [f32; 3] = [0.0, 0.0, 1.0];
/// `Clear1` colour (opaque black, the RTT).
pub const CLEAR_RTT: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// `Clear2` colour (transparent black, the screen).
pub const CLEAR_SCREEN: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};
/// `Shader1` `Color1`.
pub const COLOR1: [f32; 4] = [0.188, 0.447, 0.643, 0.01];
/// `RenderToTexture1` format (Format 1 = RGBA8).
pub const RTT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `CopyTexture1` `TextureSize` mode (3 = 12.5%).
pub const COPY_TEXTURE_SIZE: u32 = 3;
/// The combination-scale for mode 3.
pub const COPY_TEXTURE_SCALE: f32 = 0.125;
/// `Blur1` `Dir` (2 = both axes).
pub const BLUR_DIR: u32 = 2;
/// `Blur1` `Width`.
pub const BLUR_WIDTH: f32 = 4.0;
/// `Blur1` `Brightness`.
pub const BLUR_BRIGHTNESS: f32 = 4.0;
/// `Shader1` fixed specular hardness.
pub const SPECULAR_HARDNESS: f32 = 12.0;
/// `Shader1` fixed specular power.
pub const SPECULAR_POWER: f32 = 0.6;
/// `Shader1` diffuse base.
pub const DIFFUSE_BASE: f32 = 0.8;

const PROPS: &[SceneProp] = &[];

fn portal_wgsl() -> &'static str {
    use std::sync::OnceLock;
    static WGSL: OnceLock<String> = OnceLock::new();
    WGSL.get_or_init(|| format!("{P9_COMMON_WGSL}\n{PORTAL}"))
}

const PORTAL: &str = r#"
fn p9_mod289_3(x: vec3<f32>) -> vec3<f32> {
  return x - floor(x * (1.0 / 289.0)) * 289.0;
}
fn p9_mod289_4(x: vec4<f32>) -> vec4<f32> {
  return x - floor(x * (1.0 / 289.0)) * 289.0;
}
fn p9_permute4(x: vec4<f32>) -> vec4<f32> {
  return p9_mod289_4(((x * 34.0) + 1.0) * x);
}
fn p9_taylor_inv_sqrt(r: vec4<f32>) -> vec4<f32> {
  return 1.79284291400159 - 0.85373472095314 * r;
}
fn p9_snoise(v: vec3<f32>) -> f32 {
  let C = vec2<f32>(1.0 / 6.0, 1.0 / 3.0);
  let D = vec4<f32>(0.0, 0.5, 1.0, 2.0);
  var i = floor(v + vec3<f32>(dot(v, C.yyy)));
  let x0 = v - i + vec3<f32>(dot(i, C.xxx));
  let g = p9_step3(x0.yzx, x0.xyz);
  let l = vec3<f32>(1.0) - g;
  let i1 = min(g, l.zxy);
  let i2 = max(g, l.zxy);
  let x1 = x0 - i1 + C.xxx;
  let x2 = x0 - i2 + C.yyy;
  let x3 = x0 - D.yyy;
  i = p9_mod289_3(i);
  let p = p9_permute4(
    p9_permute4(
      p9_permute4(i.z + vec4<f32>(0.0, i1.z, i2.z, 1.0)) + i.y
        + vec4<f32>(0.0, i1.y, i2.y, 1.0)
    ) + i.x + vec4<f32>(0.0, i1.x, i2.x, 1.0)
  );
  let n_ = 0.142857142857;
  let ns = n_ * D.wyz - D.xzx;
  let j = p - 49.0 * floor(p * ns.z * ns.z);
  let x_ = floor(j * ns.z);
  let y_ = floor(j - 7.0 * x_);
  let x = x_ * ns.x + vec4<f32>(ns.y);
  let y = y_ * ns.x + vec4<f32>(ns.y);
  let h = vec4<f32>(1.0) - abs(x) - abs(y);
  let b0 = vec4<f32>(x.xy, y.xy);
  let b1 = vec4<f32>(x.zw, y.zw);
  let s0 = floor(b0) * 2.0 + vec4<f32>(1.0);
  let s1 = floor(b1) * 2.0 + vec4<f32>(1.0);
  let sh = select(vec4<f32>(0.0), vec4<f32>(-1.0), h <= vec4<f32>(0.0));
  let a0 = b0.xzyw + s0.xzyw * sh.xxyy;
  let a1 = b1.xzyw + s1.xzyw * sh.zzww;
  let p0 = vec3<f32>(a0.xy, h.x);
  let p1 = vec3<f32>(a0.zw, h.y);
  let p2 = vec3<f32>(a1.xy, h.z);
  let p3 = vec3<f32>(a1.zw, h.w);
  let norm = p9_taylor_inv_sqrt(vec4<f32>(
    dot(p0, p0), dot(p1, p1), dot(p2, p2), dot(p3, p3),
  ));
  let p0n = p0 * norm.x;
  let p1n = p1 * norm.y;
  let p2n = p2 * norm.z;
  let p3n = p3 * norm.w;
  var m = max(
    0.6 - vec4<f32>(dot(x0, x0), dot(x1, x1), dot(x2, x2), dot(x3, x3)),
    vec4<f32>(0.0),
  );
  m = m * m;
  return 42.0 * dot(
    m * m,
    vec4<f32>(dot(p0n, x0), dot(p1n, x1), dot(p2n, x2), dot(p3n, x3)),
  );
}

fn p9_gradient(p: vec2<f32>, v: f32, time: f32) -> vec3<f32> {
  var normal = vec3<f32>(0.0);
  let e = 0.01;
  normal.x = ((p9_snoise(vec3<f32>(p + vec2<f32>(e, 0.0), time))) * 0.5 + 0.5) * 0.5 - v;
  normal.y = ((p9_snoise(vec3<f32>(p + vec2<f32>(0.0, e), time))) * 0.5 + 0.5) * 0.5 - v;
  return normal / e;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let p = tex * vec2<f32>(2.0) - vec2<f32>(1.0);
  let t = p * 12.0;
  let dist_to_light = length(vec3<f32>(p, 0.5));
  let light_dir = normalize(vec3<f32>(p, 0.5));
  let view_dir = normalize(vec3<f32>(p, -0.1));
  let half_vec = normalize(light_dir + view_dir);
  let diffuse_power = 0.8 + u.in1.y;
  let specular_power = 0.6;

  let v = (p9_snoise(vec3<f32>(t, u.in1.x)) * 0.5 + 0.5) * 0.5;
  let n = normalize(vec3<f32>(0.0, 0.0, 1.0) - p9_gradient(t, v, u.in1.x));

  var c = u.color1 * 0.5;
  c = c + u.color1 * v * (dot(light_dir, n) * 0.5 + 0.5) * diffuse_power
    / (dist_to_light * dist_to_light);

  let specular = pow(p9_saturate(dot(n, half_vec)), 12.0) * specular_power
    * ((12.0 + 4.0) / 8.0);
  c = c + vec4<f32>(0.8, 0.9, 1.0, 1.0) * specular / (dist_to_light * dist_to_light);
  return c;
}
"#;

const COPY: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let c = textureSampleLevel(g_texture1, g_sampler, tex, 0.0);
  return c * c.a;
}
"#;

const BLUR: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let texel = u.in1.xy / u.resolution;
  var sum = vec3<f32>(0.0);
  var weight_sum = 0.0;
  for (var i = 0; i < 9; i = i + 1) {
    let d = f32(i) - 4.0;
    let w = exp(-0.5 * d * d / (u.in2.x * u.in2.x));
    sum = sum + textureSampleLevel(g_texture1, g_sampler, tex + texel * d, 0.0).rgb * w;
    weight_sum = weight_sum + w;
  }
  return vec4<f32>(sum / weight_sum * u.in2.y, 1.0);
}
"#;

const PRESENT: &str = r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let c1 = textureSampleLevel(g_texture1, g_sampler, tex, 0.0);
  let c2 = textureSampleLevel(g_texture2, g_sampler, tex, 0.0);
  var out = c1 + c2;
  out = out * out;
  return out;
}
"#;

/// The copy/blur target size from `TextureSize 3` (12.5%).
pub fn copy_size(width: u32, height: u32) -> (u32, u32) {
    (
        ((width as f32 * COPY_TEXTURE_SCALE).round() as u32).max(2),
        ((height as f32 * COPY_TEXTURE_SCALE).round() as u32).max(2),
    )
}

struct Gpu {
    portal: Plane9Pass,
    copy: Plane9Pass,
    blur_h: Plane9Pass,
    blur_v: Plane9Pass,
    present: Plane9DualPass,
    _rtt: wgpu::Texture,
    _rtt_view: wgpu::TextureView,
    _copy: wgpu::Texture,
    _copy_view: wgpu::TextureView,
    _blur_h: wgpu::Texture,
    _blur_h_view: wgpu::TextureView,
    _blur_v: wgpu::Texture,
    _blur_v_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
}

/// `Plane9 Stargate Event Horizon` scene.
pub struct StargateEventHorizon {
    audio: P10Audio,
    elapsed: f32,
    size: (u32, u32),
    target_format: wgpu::TextureFormat,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    portal_uniforms: Plane9Uniforms,
    copy_uniforms: Plane9Uniforms,
    blur_h_uniforms: Plane9Uniforms,
    blur_v_uniforms: Plane9Uniforms,
    present_uniforms: Plane9Uniforms,
    gpu: Option<Gpu>,
}

impl Default for StargateEventHorizon {
    fn default() -> Self {
        Self::new()
    }
}

impl StargateEventHorizon {
    /// Create the scene.
    pub fn new() -> Self {
        let mut portal_uniforms = Plane9Uniforms::new(1, 1);
        portal_uniforms.color1 = COLOR1;
        Self {
            audio: P10Audio::new(),
            elapsed: 0.0,
            size: (1, 1),
            target_format: crate::point10::HDR_FORMAT,
            device: None,
            queue: None,
            portal_uniforms,
            copy_uniforms: Plane9Uniforms::new(1, 1),
            blur_h_uniforms: Plane9Uniforms::new(1, 1),
            blur_v_uniforms: Plane9Uniforms::new(1, 1),
            present_uniforms: Plane9Uniforms::new(1, 1),
            gpu: None,
        }
    }

    /// The portal uniform block last uploaded.
    pub fn uniforms(&self) -> &Plane9Uniforms {
        &self.portal_uniforms
    }

    /// Bind the modV props (this preset declares no `sceneProps`).
    pub fn apply_props(&mut self, _props: impl FnMut(&str) -> Option<f32>) {}

    fn rebuild(&mut self) {
        let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) else {
            return;
        };
        let (rtt, rtt_view) = create_colour_target(
            &device,
            "point10-plane9-stargate-rtt",
            self.size.0,
            self.size.1,
            RTT_FORMAT,
        );
        let (copy_w, copy_h) = copy_size(self.size.0, self.size.1);
        let (copy, copy_view) = create_colour_target(
            &device,
            "point10-plane9-stargate-copy",
            copy_w,
            copy_h,
            RTT_FORMAT,
        );
        let (blur_h_tex, blur_h_view) = create_colour_target(
            &device,
            "point10-plane9-stargate-blur-h",
            copy_w,
            copy_h,
            RTT_FORMAT,
        );
        let (blur_v_tex, blur_v_view) = create_colour_target(
            &device,
            "point10-plane9-stargate-blur-v",
            copy_w,
            copy_h,
            RTT_FORMAT,
        );
        let sampler = plane9_sampler(&device, "point10-plane9-stargate-sampler");

        let portal = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-stargate-portal",
            portal_wgsl(),
            None,
        );
        let copy_pass = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-stargate-copy",
            COPY,
            Some(Plane9TextureBinding::D2 {
                view: &rtt_view,
                sampler: &sampler,
            }),
        );
        let blur_h = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-stargate-blur-h",
            BLUR,
            Some(Plane9TextureBinding::D2 {
                view: &copy_view,
                sampler: &sampler,
            }),
        );
        let blur_v = Plane9Pass::new(
            &device,
            &queue,
            RTT_FORMAT,
            "point10-plane9-stargate-blur-v",
            BLUR,
            Some(Plane9TextureBinding::D2 {
                view: &blur_h_view,
                sampler: &sampler,
            }),
        );
        let present = Plane9DualPass::new(
            &device,
            &queue,
            self.target_format,
            "point10-plane9-stargate-present",
            PRESENT,
            &blur_v_view,
            &rtt_view,
            &sampler,
        );

        self.copy_uniforms.resolution = [copy_w as f32, copy_h as f32];
        self.blur_h_uniforms.resolution = [copy_w as f32, copy_h as f32];
        self.blur_v_uniforms.resolution = [copy_w as f32, copy_h as f32];
        self.blur_h_uniforms.in1 = [1.0, 0.0, 0.0, 0.0];
        self.blur_v_uniforms.in1 = [0.0, 1.0, 0.0, 0.0];
        self.blur_h_uniforms.in2 = [BLUR_WIDTH, 1.0, 0.0, 0.0];
        self.blur_v_uniforms.in2 = [BLUR_WIDTH, BLUR_BRIGHTNESS, 0.0, 0.0];

        self.gpu = Some(Gpu {
            portal,
            copy: copy_pass,
            blur_h,
            blur_v,
            present,
            _rtt: rtt,
            _rtt_view: rtt_view,
            _copy: copy,
            _copy_view: copy_view,
            _blur_h: blur_h_tex,
            _blur_h_view: blur_h_view,
            _blur_v: blur_v_tex,
            _blur_v_view: blur_v_view,
            _sampler: sampler,
        });
        self.present_uniforms.resolution = [self.size.0 as f32, self.size.1 as f32];
        self.upload_uniforms();
    }

    fn upload_uniforms(&self) {
        if let Some(gpu) = &self.gpu {
            gpu.portal.upload(&self.portal_uniforms);
            gpu.copy.upload(&self.copy_uniforms);
            gpu.blur_h.upload(&self.blur_h_uniforms);
            gpu.blur_v.upload(&self.blur_v_uniforms);
            gpu.present.upload(&self.present_uniforms);
        }
    }
}

impl Point10Scene for StargateEventHorizon {
    fn name(&self) -> &'static str {
        "Plane9 Stargate Event Horizon"
    }

    fn init(&mut self, ctx: &mut SceneContext) {
        self.device = Some(ctx.device.clone());
        self.queue = Some(ctx.queue.clone());
        self.target_format = ctx.target_format;
        self.size = (ctx.width.max(1), ctx.height.max(1));
        self.portal_uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.portal_uniforms.color1 = COLOR1;
        self.copy_uniforms = Plane9Uniforms::new(1, 1);
        self.blur_h_uniforms = Plane9Uniforms::new(1, 1);
        self.blur_v_uniforms = Plane9Uniforms::new(1, 1);
        self.present_uniforms = Plane9Uniforms::new(self.size.0, self.size.1);
        self.rebuild();
    }

    fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if self.size == size {
            return;
        }
        self.size = size;
        self.portal_uniforms.resolution = [size.0 as f32, size.1 as f32];
        self.present_uniforms.resolution = [size.0 as f32, size.1 as f32];
        self.rebuild();
    }

    fn update(&mut self, dt: f32, audio: &AudioBridge) {
        let dt = frame_dt(dt);
        self.audio.update(dt, audio);
        self.elapsed += dt;
        let band = self.audio.band(-1, 1, 0, 0.0);
        self.portal_uniforms.time = self.elapsed;
        self.portal_uniforms.in1 = [self.elapsed * 0.2, (band * 2.0).min(0.5), 0.0, 0.0];
        self.copy_uniforms.time = self.elapsed;
        self.blur_h_uniforms.time = self.elapsed;
        self.blur_v_uniforms.time = self.elapsed;
        self.present_uniforms.time = self.elapsed;
        self.upload_uniforms();
    }

    fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        gpu.portal.draw(
            encoder,
            &gpu._rtt_view,
            CLEAR_RTT,
            "point10-plane9-stargate-portal",
        );
        gpu.copy.draw(
            encoder,
            &gpu._copy_view,
            wgpu::Color::TRANSPARENT,
            "point10-plane9-stargate-copy",
        );
        gpu.blur_h.draw(
            encoder,
            &gpu._blur_h_view,
            wgpu::Color::TRANSPARENT,
            "point10-plane9-stargate-blur-h",
        );
        gpu.blur_v.draw(
            encoder,
            &gpu._blur_v_view,
            wgpu::Color::TRANSPARENT,
            "point10-plane9-stargate-blur-v",
        );
        gpu.present.draw(
            encoder,
            target,
            CLEAR_SCREEN,
            "point10-plane9-stargate-present",
        );
    }

    fn dispose(&mut self) {
        self.gpu = None;
        self.device = None;
        self.queue = None;
        self.elapsed = 0.0;
    }

    fn props(&self) -> Vec<SceneProp> {
        scene_props_with_levers(PROPS)
    }

    fn meta(&self) -> SceneMeta {
        SceneMeta::for_plane9_scene("Plane9 Stargate Event Horizon", "3d-scenes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point10::audio_bridge::BandPayload;
    use crate::point10::scenes::plane9_common::{
        PLANE9_DUAL_TEXTURE_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL,
    };
    use crate::point10::test_support::{TestTarget, any_visible, gpu};

    #[test]
    fn camera_clear_copy_and_blur_match_the_preset() {
        assert_eq!(CAMERA_POSITION, [0.0, 0.0, -2.0]);
        assert_eq!(CAMERA_LOOK, [0.0, 0.0, 1.0]);
        assert_eq!(CLEAR_RTT.a, 1.0);
        assert_eq!(CLEAR_SCREEN.a, 0.0);
        assert_eq!(COLOR1, [0.188, 0.447, 0.643, 0.01]);
        assert_eq!(RTT_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(COPY_TEXTURE_SIZE, 3);
        assert_eq!(COPY_TEXTURE_SCALE, 0.125);
        assert_eq!(BLUR_DIR, 2);
        assert_eq!(BLUR_WIDTH, 4.0);
        assert_eq!(BLUR_BRIGHTNESS, 4.0);
        assert_eq!(SPECULAR_HARDNESS, 12.0);
        assert_eq!(SPECULAR_POWER, 0.6);
        assert_eq!(DIFFUSE_BASE, 0.8);
        assert_eq!(copy_size(64, 64), (8, 8));
        assert_eq!(copy_size(1920, 1080), (240, 135));
    }

    #[test]
    fn shaders_keep_the_noise_and_composite() {
        let portal = portal_wgsl();
        for needle in [
            "return p9_mod289_4(((x * 34.0) + 1.0) * x);",
            "1.79284291400159 - 0.85373472095314 * r",
            "let C = vec2<f32>(1.0 / 6.0, 1.0 / 3.0);",
            "return 42.0 * dot(",
            "let diffuse_power = 0.8 + u.in1.y;",
            "pow(p9_saturate(dot(n, half_vec)), 12.0) * specular_power",
            "* ((12.0 + 4.0) / 8.0)",
        ] {
            assert!(portal.contains(needle), "lost {needle}");
        }
        assert!(COPY.contains("return c * c.a;"));
        for needle in [
            "let texel = u.in1.xy / u.resolution;",
            "u.in2.x * u.in2.x",
            "* u.in2.y",
        ] {
            assert!(BLUR.contains(needle), "lost {needle}");
        }
        for needle in [
            "let c1 = textureSampleLevel(g_texture1, g_sampler, tex, 0.0);",
            "let c2 = textureSampleLevel(g_texture2, g_sampler, tex, 0.0);",
            "out = out * out;",
        ] {
            assert!(PRESENT.contains(needle), "lost {needle}");
        }
    }

    #[test]
    fn shaders_validate() {
        let sources = [
            format!("{PLANE9_UNIFORMS_WGSL}\n{}", portal_wgsl()),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{COPY}"),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{BLUR}"),
            format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_DUAL_TEXTURE_WGSL}\n{PRESENT}"),
        ];
        for wgsl in sources {
            let module = naga::front::wgsl::parse_str(&wgsl)
                .unwrap_or_else(|error| panic!("Stargate WGSL failed to parse: {error}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("Stargate WGSL validates");
        }
    }

    #[test]
    fn renders_two_frames_visible() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let mut scene = StargateEventHorizon::new();
        let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        ctx.resize(64, 64);
        scene.init(&mut ctx);
        let mut bridge = AudioBridge::new();
        let mut payload = BandPayload::silent(44_100.0);
        payload.lanes = [0.4; 12];
        let target = TestTarget::new(&device, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        for _ in 0..2 {
            bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
            scene.update(1.0 / 60.0, &bridge);
            scene.render(&mut encoder, &target.view);
        }
        queue.submit(Some(encoder.finish()));
        assert!(any_visible(&target.read(&device, &queue)));
    }
}
