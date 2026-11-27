//! Per-scene GPU smoke tests: two frames into a 64x64 target, assert non-empty
//! output.
//!
//! Rendered through [`crate::point10::postfx::Composer`] so the test also
//! exercises the real presentation path (scene target → output pass). Scenes
//! that own their `EffectComposer` in the JS present themselves instead
//! ([`render_owned_chain_two_frames`]). Skipped with a log line when no
//! fallback adapter is available.

use crate::point10::audio_bridge::{AUDIO_SAMPLES, AudioBridge, BandPayload};
use crate::point10::postfx::Composer;
use crate::point10::scenes::rolling_fire;
use crate::point10::test_support::{TestTarget, any_lit, gpu};
use crate::point10::{HDR_FORMAT, Point10Scene, SceneContext, all_scenes};

/// Scene smoke-test size.
const SIZE: u32 = 64;
/// Particle sizes are in world units, and `size_px = size * (height / 2) / 15`:
/// at 64 px the default 0.1–0.15 sizes are a third of a pixel, so the particle
/// scenes are rendered at 256 px (and with their size props maxed) to give the
/// billboards something to rasterise.
const PARTICLE_SIZE: u32 = 256;

/// An audio payload with a loud first bin, full waveform and a beat, so the
/// scenes that gate on `audioData` actually animate.
fn loud_payload() -> BandPayload {
    let mut payload = BandPayload::silent(44_100.0);
    payload.byte_frequency_data = vec![200; 1024];
    payload.wave = [0.5; AUDIO_SAMPLES];
    payload.beat = 0.8;
    payload.lanes = [0.6; 12];
    payload.cells = [0.6; 16];
    payload.freq_mag = [0.4; AUDIO_SAMPLES];
    payload
}

/// A quiet payload for the scenes whose bass lane is a *scale* (SoundCube's
/// `0.3 + bass * 8`): at `loud_payload`'s 0.6 the cube is 5.1 units wide and the
/// camera at z = 5 ends up inside it.
fn quiet_payload() -> BandPayload {
    let mut payload = BandPayload::silent(44_100.0);
    payload.byte_frequency_data = vec![40; 1024];
    payload.wave = [0.2; AUDIO_SAMPLES];
    payload.beat = 0.2;
    payload.lanes = [0.05; 12];
    payload.cells = [0.05; 16];
    payload.freq_mag = [0.05; AUDIO_SAMPLES];
    payload
}

/// Initialise `scene`, drive it for two frames and present it; returns the
/// final RGBA8 frame.
fn render_two_frames(scene: &mut dyn Point10Scene, size: u32) -> Option<Vec<u8>> {
    render_frames(scene, size, 2)
}

/// [`render_two_frames`] with an explicit frame count: the transparency tests
/// let the ring buffers and persistent passes settle over ten frames, matching
/// the e2e specs.
fn render_frames(scene: &mut dyn Point10Scene, size: u32, frames: usize) -> Option<Vec<u8>> {
    let (device, queue) = gpu()?;

    let mut composer = Composer::new(&device, &queue, size, size, wgpu::TextureFormat::Rgba8Unorm);
    let mut ctx = SceneContext::new(&device, &queue, HDR_FORMAT);
    ctx.resize(size, size);
    scene.init(&mut ctx);
    scene.resize(size, size);

    let mut bridge = AudioBridge::new();
    let payload = loud_payload();
    let target = TestTarget::new(&device, size, size);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("point10-scene-smoke"),
    });

    // The second frame proves per-frame state (buffers, uniforms, instance
    // uploads) survives the first render; the extra frames let the persistent
    // passes settle.
    for _ in 0..frames {
        bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
        scene.update(1.0 / 60.0, &bridge);
        scene.render(&mut encoder, composer.scene_view());
    }
    composer.render(&mut encoder, &target.view);
    queue.submit(Some(encoder.finish()));

    Some(target.read(&device, &queue))
}

/// [`render_two_frames`] for scenes that own their `EffectComposer`
/// (LightTunnel, Wormhole, ItIsComingForYou).
///
/// Those scenes present themselves: their `render` runs their own
/// `RenderPass -> UnrealBloomPass -> OutputPass` chain straight into the target
/// it is handed. Wrapping them in [`render_two_frames`]' composer would
/// tone-map and sRGB-encode twice, so this path initialises the scene with the
/// destination format and lets it draw into the destination directly — the
/// same contract the engine uses (`Point10Scene::render` writes the presented
/// frame).
fn render_owned_chain_two_frames(scene: &mut dyn Point10Scene, size: u32) -> Option<Vec<u8>> {
    let payload = loud_payload();
    render_owned_chain_two_frames_with(scene, size, &payload)
}

/// [`render_owned_chain_two_frames`] with an explicit payload.
fn render_owned_chain_two_frames_with(
    scene: &mut dyn Point10Scene,
    size: u32,
    payload: &BandPayload,
) -> Option<Vec<u8>> {
    render_owned_chain_n_frames_with(scene, size, payload, 2)
}

/// [`render_owned_chain_two_frames_with`] with an explicit frame count (the
/// transparency tests settle the scene over ten frames like the e2e specs).
fn render_owned_chain_n_frames_with(
    scene: &mut dyn Point10Scene,
    size: u32,
    payload: &BandPayload,
    frames: usize,
) -> Option<Vec<u8>> {
    let (device, queue) = gpu()?;

    let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    ctx.resize(size, size);
    scene.init(&mut ctx);
    scene.resize(size, size);

    let mut bridge = AudioBridge::new();
    let target = TestTarget::new(&device, size, size);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("point10-scene-owned-chain-smoke"),
    });

    for _ in 0..frames {
        bridge.update(1.0 / 60.0, Some(payload), &Default::default());
        scene.update(1.0 / 60.0, &bridge);
        scene.render(&mut encoder, &target.view);
    }
    queue.submit(Some(encoder.finish()));

    Some(target.read(&device, &queue))
}

/// [`render_owned_chain_two_frames_with`] over many frames: the volumetric
/// scenes need the simulation to build up a field before there is anything to
/// look at (and the JS fades the emitter in over three seconds).
///
/// Returns the last "settled" frame and the frame after it, rendered with
/// `final_payload` — the structural tests compare the pair.
struct RenderedFrames {
    before: Vec<u8>,
    after: Vec<u8>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn render_owned_chain_frames(
    scene: &mut dyn Point10Scene,
    size: u32,
    frames: usize,
    payload: &BandPayload,
    final_payload: &BandPayload,
) -> Option<RenderedFrames> {
    let (device, queue) = gpu()?;

    let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    ctx.resize(size, size);
    scene.init(&mut ctx);
    scene.resize(size, size);

    let mut bridge = AudioBridge::new();
    let target = TestTarget::new(&device, size, size);

    let mut draw = |payload: &BandPayload| {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-scene-owned-chain-frames"),
        });
        bridge.update(1.0 / 60.0, Some(payload), &Default::default());
        scene.update(1.0 / 60.0, &bridge);
        scene.render(&mut encoder, &target.view);
        queue.submit(Some(encoder.finish()));
    };

    for _ in 0..frames {
        draw(payload);
    }
    draw(payload);
    let before = target.read(&device, &queue);
    draw(final_payload);
    let after = target.read(&device, &queue);
    Some(RenderedFrames {
        before,
        after,
        device,
        queue,
    })
}

#[test]
fn chromatic_line_renders() {
    let mut scenes = all_scenes();
    let Some(pixels) = render_two_frames(scenes[0].as_mut(), SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "ChromaticLine produced an empty frame at {SIZE}x{SIZE}"
    );
}

#[test]
fn chromatic_wave_renders() {
    let mut scenes = all_scenes();
    let Some(pixels) = render_two_frames(scenes[1].as_mut(), SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "ChromaticWave produced an empty frame at {SIZE}x{SIZE}"
    );
}

/// The e2e transparency size (`CANVAS_SIZE = 96`).
const TRANSPARENCY_SIZE: u32 = 96;

/// [`chromatic_line_renders`] plus the e2e alpha contract
/// (point10ChromaticTransparency.spec.js:96-100): a transparent backdrop, drawn
/// pixels, and a magenta backdrop that survives a 50% composite.
#[test]
fn chromatic_line_keeps_the_background_transparent() {
    let mut scenes = all_scenes();
    let Some(pixels) = render_frames(scenes[0].as_mut(), TRANSPARENCY_SIZE, 10) else {
        return;
    };
    assert_transparent_composition(&pixels, 0.2, 0.2);
}

/// [`chromatic_wave_renders`] plus the e2e alpha contract.
#[test]
fn chromatic_wave_keeps_the_background_transparent() {
    let mut scenes = all_scenes();
    let Some(pixels) = render_frames(scenes[1].as_mut(), TRANSPARENCY_SIZE, 10) else {
        return;
    };
    assert_transparent_composition(&pixels, 0.2, 0.2);
}

#[test]
fn particle_field_renders() {
    let mut scene = crate::point10::scenes::particle_field::ParticleField::new();
    // Make the dots big enough to rasterise at the smoke-test size.
    scene.apply_props(|key| (key == "particleSize").then_some(1.0));

    let Some(pixels) = render_two_frames(&mut scene, PARTICLE_SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "ParticleField produced an empty frame at {PARTICLE_SIZE}x{PARTICLE_SIZE}"
    );
}

#[test]
fn spiral_particles_renders() {
    let mut scene = crate::point10::scenes::spiral_particles::SpiralParticles::new();
    scene.apply_props(|key| (key == "particleSize").then_some(4.0));

    let Some(pixels) = render_two_frames(&mut scene, PARTICLE_SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "SpiralParticles produced an empty frame at {PARTICLE_SIZE}x{PARTICLE_SIZE}"
    );
}

#[test]
fn waveform_renders() {
    let mut scenes = all_scenes();
    let Some(pixels) = render_two_frames(scenes[4].as_mut(), SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "Waveform produced an empty frame at {SIZE}x{SIZE}"
    );
}

// ————— batch 2 —————

#[test]
fn square_tunnel_renders() {
    let mut scene = crate::point10::scenes::square_tunnel::SquareTunnel::new();
    let Some(pixels) = render_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "SquareTunnel produced an empty frame at {SIZE}x{SIZE}"
    );
    // `Clear1` is opaque black, so every pixel has alpha 255.
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255),
        "SquareTunnel must stay opaque (setClearColor(0x000000, 1))"
    );
}

#[test]
fn light_tunnel_renders() {
    let mut scene = crate::point10::scenes::light_tunnel::LightTunnel::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "LightTunnel produced an empty frame at {SIZE}x{SIZE}"
    );
}

#[test]
fn wormhole_renders() {
    let mut scene = crate::point10::scenes::wormhole::Wormhole::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "Wormhole produced an empty frame at {SIZE}x{SIZE}"
    );
}

/// The e2e transparency contract (point10WormholeTransparency.spec.js:95-98):
/// the point cloud is sparse, so the space between points must leave the
/// magenta backdrop untouched instead of becoming an opaque bloom veil. The
/// thresholds are the spec's 5%.
#[test]
fn wormhole_keeps_the_background_transparent() {
    let mut scene = crate::point10::scenes::wormhole::Wormhole::new();
    let Some(pixels) =
        render_owned_chain_n_frames_with(&mut scene, TRANSPARENCY_SIZE, &loud_payload(), 10)
    else {
        return;
    };
    assert_transparent_composition(&pixels, 0.05, 0.05);
}

/// ItIsComingForYou's sprites are 50-60 px, so a 64 px frame is almost all
/// sprite. Render at 256 px to see both the cloud and the gaps around it; the
/// fragment count is the same (the sprite size is constant in pixels).
const IT_IS_COMING_SIZE: u32 = 256;

#[test]
fn it_is_coming_for_you_renders() {
    let mut scene = crate::point10::scenes::it_is_coming_for_you::ItIsComingForYou::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, IT_IS_COMING_SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "ItIsComingForYou produced an empty frame at {IT_IS_COMING_SIZE}x{IT_IS_COMING_SIZE}"
    );
}

/// The scene is deliberately background-free (`CLEAR_ALPHA = 0`,
/// `ItIsComingForYou.js:51-52`): the cloud must composite over the layers
/// below, so the pixels that no point covers keep alpha 0 while the cloud
/// itself is opaque.
#[test]
fn it_is_coming_for_you_keeps_the_background_transparent() {
    let mut scene = crate::point10::scenes::it_is_coming_for_you::ItIsComingForYou::new();
    assert_eq!(scene.clear_alpha(), 0.0);

    let Some(pixels) = render_owned_chain_two_frames(&mut scene, IT_IS_COMING_SIZE) else {
        return;
    };

    assert_transparency(&pixels);
}

// ————— batch 3 —————

/// Count the fully transparent and fully opaque pixels.
fn alpha_counts(pixels: &[u8]) -> (usize, usize, usize) {
    let mut transparent = 0_usize;
    let mut opaque = 0_usize;
    for pixel in pixels.as_chunks::<4>().0 {
        if pixel[3] == 0 {
            transparent += 1;
        }
        if pixel[3] == 255 {
            opaque += 1;
        }
    }
    (transparent, opaque, pixels.len() / 4)
}

/// The e2e backdrop check: composite the frame at 50% over solid magenta
/// (`point10ChromaticTransparency.spec.js:59-80`) and count the pixels the
/// magenta survives. A frame with a transparent backdrop leaves the magenta
/// untouched wherever nothing was drawn; an opaque veil covers it.
fn composite_half_over_magenta(pixels: &[u8]) -> Vec<u8> {
    let mut composited = Vec::with_capacity(pixels.len());

    for pixel in pixels.as_chunks::<4>().0 {
        let source_alpha = f32::from(pixel[3]) / 255.0 * 0.5;
        let backdrop = 1.0 - source_alpha;
        let blend = |source: u8, destination: u8| -> u8 {
            (f32::from(source) * source_alpha + f32::from(destination) * backdrop)
                .round()
                .clamp(0.0, 255.0) as u8
        };

        composited.extend_from_slice(&[
            blend(pixel[0], 255),
            blend(pixel[1], 0),
            blend(pixel[2], 255),
            255,
        ]);
    }

    composited
}

/// Count the composited pixels that are still the untouched magenta backdrop
/// (`r >= 240, g <= 15, b >= 240, a == 255`).
fn magenta_preserved(pixels: &[u8]) -> usize {
    composite_half_over_magenta(pixels)
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] >= 240 && pixel[1] <= 15 && pixel[2] >= 240 && pixel[3] == 255)
        .count()
}

/// Assert a background-free frame: a real transparent region (fraction of the
/// frame), drawn pixels, and a backdrop that survives the 50% magenta
/// composite (fraction of the frame).
fn assert_transparent_composition(
    pixels: &[u8],
    transparent_fraction: f64,
    preserved_fraction: f64,
) {
    let total = pixels.len() / 4;
    let transparent = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] == 0)
        .count();
    let opaque = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 200)
        .count();

    assert!(
        transparent as f64 > total as f64 * transparent_fraction,
        "expected more than {:.0}% of the {total} px to stay transparent, got {transparent}",
        transparent_fraction * 100.0
    );
    assert!(
        opaque > 0,
        "expected drawn pixels, but none reached alpha > 200"
    );

    let preserved = magenta_preserved(pixels);
    assert!(
        preserved as f64 > total as f64 * preserved_fraction,
        "expected the magenta backdrop to survive on more than {:.0}% of the {total} px, got {preserved}",
        preserved_fraction * 100.0
    );
}

/// Assert the frame has an alpha-0 background *and* drawn pixels, with most of
/// the frame still background.
fn assert_transparency(pixels: &[u8]) {
    let (transparent, opaque, total) = alpha_counts(pixels);
    assert!(
        transparent > 0,
        "expected an alpha-0 background, but every pixel was drawn ({total} px)"
    );
    assert!(
        opaque > 0,
        "expected opaque pixels, but none reached alpha 255"
    );
    assert!(
        transparent > total / 4,
        "the scene covers too much of the frame: {transparent} transparent px of {total}"
    );
}

#[test]
fn water_ripple_renders() {
    let mut scene = crate::point10::scenes::water_ripple::WaterRipple::new();
    let Some(pixels) = render_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "WaterRipple produced an empty frame at {SIZE}x{SIZE}"
    );
}

/// The ripple is a circular overlay (`CLEAR_ALPHA = 0`): the corners of the
/// frame are outside `inCircle` and must stay transparent.
#[test]
fn water_ripple_keeps_the_corners_transparent() {
    let mut scene = crate::point10::scenes::water_ripple::WaterRipple::new();
    assert_eq!(scene.clear_alpha(), 0.0);

    let Some(pixels) = render_two_frames(&mut scene, SIZE) else {
        return;
    };
    let (transparent, opaque, total) = alpha_counts(&pixels);
    assert!(
        transparent > 0,
        "expected an alpha-0 background, but every pixel was drawn ({total} px)"
    );
    assert!(
        opaque > 0,
        "expected opaque pixels, but none reached alpha 255"
    );

    // The artwork is a disc of radius ~1 in screenP (the JS default is 0.9),
    // so at most a fifth of the square is background; unlike ItIsComingForYou
    // the coverage is most of the frame. What must hold is that the *extreme
    // corners* (length(screenP) ≈ 1.39) sit outside radius + 0.1 and are
    // exactly transparent.
    let size = SIZE as usize;
    for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
        let alpha = pixels[(y * size + x) * 4 + 3];
        assert_eq!(alpha, 0, "corner ({x}, {y}) must stay transparent");
    }
}

#[test]
fn mycelium_renders() {
    let mut scene = crate::point10::scenes::mycelium::Mycelium::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "Mycelium produced an empty frame at {SIZE}x{SIZE}"
    );
    // `gl_FragColor = vec4(col, 1.0)` — the colony is opaque.
    let (_, opaque, total) = alpha_counts(&pixels);
    assert_eq!(opaque, total, "Mycelium must stay opaque");
}

#[test]
fn around_plus_renders() {
    let mut scene = crate::point10::scenes::around_plus::AroundPlus::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "AroundPlus produced an empty frame at {SIZE}x{SIZE}"
    );
    // The camera sits inside the tube: most of the frame is the concrete wall.
    let (_, opaque, total) = alpha_counts(&pixels);
    assert!(
        opaque > total / 4,
        "AroundPlus drew too little of the tube: {opaque} opaque px of {total}"
    );
}

// ————— batch 4 —————

#[test]
fn ultra_hdr_renders() {
    let mut scene = crate::point10::scenes::ultra_hdr::UltraHdr::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "UltraHDR produced an empty frame at {SIZE}x{SIZE}"
    );
    // The inherited layer below the module is the background: the knot is
    // drawn opaque but the pixels it does not cover must stay alpha 0 so the
    // layer underneath shows through (BUGS.md item 9).
    assert_transparency(&pixels);
}

#[test]
fn mirror_cube_renders() {
    let mut scene = crate::point10::scenes::mirror_cube::MirrorCube::new();
    // Expression2 zooms the camera to CAMERA_MIN_Z on a loud mid band, which
    // fills the frame with the cube. A quiet payload keeps the preset's default
    // 4.25 units so the frame shows both the cube and the transparent clear.
    let Some(pixels) = render_owned_chain_two_frames_with(&mut scene, SIZE, &quiet_payload())
    else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "MirrorCube produced an empty frame at {SIZE}x{SIZE}"
    );
    // The outer cube only covers the middle of the frame, so the corners stay
    // transparent (`CLEAR_ALPHA = 0`).
    let (transparent, opaque, _) = alpha_counts(&pixels);
    assert!(transparent > 0, "MirrorCube lost its transparent clear");
    assert!(opaque > 0, "MirrorCube never wrote an opaque cube pixel");
}

#[test]
fn color_cube_renders() {
    let mut scene = crate::point10::scenes::color_cube::ColorCube::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "ColorCube produced an empty frame at {SIZE}x{SIZE}"
    );
}

#[test]
fn boxy_renders() {
    let mut scene = crate::point10::scenes::boxy::Boxy::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "Boxy produced an empty frame at {SIZE}x{SIZE}"
    );
}

/// Boxy defines `preserveBloomAlpha` (`Boxy.js:73-86`) and blooms with
/// `threshold: 0`, so *every* pixel receives bloom. Without the preserve-alpha
/// combine the additive bloom alpha turns the transparent clear into an opaque
/// veil and the module's layer alpha is lost. This asserts the layer alpha
/// survives: the cleared background must still be alpha 0 while the cubes are
/// opaque.
#[test]
fn boxy_keeps_the_background_transparent() {
    let mut scene = crate::point10::scenes::boxy::Boxy::new();
    assert_eq!(scene.clear_alpha(), 0.0);

    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };

    let (transparent, opaque, total) = alpha_counts(&pixels);
    assert!(
        transparent > 0,
        "expected an alpha-0 background, but every pixel was drawn ({total} px)"
    );
    assert!(
        opaque > 0,
        "expected opaque cubes, but none reached alpha 255"
    );
}

#[test]
fn sound_cube_renders() {
    let mut scene = crate::point10::scenes::sound_cube::SoundCube::new();
    // `scale = 0.3 + bass * 8`, so the smoke test's full-scale payload would
    // put the camera inside a 5-unit cube. A quiet bass lane keeps the cube in
    // frame, which is what the preset looks like in normal playback.
    let Some(pixels) = render_owned_chain_two_frames_with(&mut scene, SIZE, &quiet_payload())
    else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "SoundCube produced an empty frame at {SIZE}x{SIZE}"
    );
    // The composite derives its alpha from the visible glow, so the frame
    // behind the cube stays transparent (`CLEAR_ALPHA = 0`).
    let (transparent, _, _) = alpha_counts(&pixels);
    assert!(transparent > 0, "SoundCube lost its transparent clear");
}

const SOUND_CUBE_QUALITY_WIDTH: u32 = 1280;
const SOUND_CUBE_QUALITY_HEIGHT: u32 = 720;
const SOUND_CUBE_QUALITY_FRAMES: usize = 12;

/// Render the real scene/composer path at the report's 1280x720 size. Spin is
/// frozen so the radial measurement follows the feedback waves rather than a
/// moving silhouette, and Lens is deliberately left unprop'd (its default 1).
/// The existing loud payload's bass would put the camera inside the cube, so
/// the preset's Pulse prop alone keeps the fixture framed; no band/Gain/AGC
/// mapping is changed.
fn render_sound_cube_quality(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    trail: f32,
    frames: usize,
) -> Vec<u8> {
    use crate::point10::scenes::sound_cube::SoundCube;

    let mut scene = SoundCube::new();
    scene.apply_props(|key| match key {
        "spinSpeed" => Some(0.0),
        "pulse" => Some(0.25),
        "trail" => Some(trail),
        _ => None,
    });

    let mut ctx = SceneContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm);
    ctx.resize(SOUND_CUBE_QUALITY_WIDTH, SOUND_CUBE_QUALITY_HEIGHT);
    scene.init(&mut ctx);
    scene.resize(SOUND_CUBE_QUALITY_WIDTH, SOUND_CUBE_QUALITY_HEIGHT);

    let payload = loud_payload();
    let mut bridge = AudioBridge::new();
    let target = TestTarget::new(device, SOUND_CUBE_QUALITY_WIDTH, SOUND_CUBE_QUALITY_HEIGHT);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("point10-sound-cube-quality"),
    });
    for _ in 0..frames {
        bridge.update(0.0, Some(&payload), &Default::default());
        scene.update(0.0, &bridge);
        scene.render(&mut encoder, &target.view);
    }
    queue.submit(Some(encoder.finish()));
    target.read(device, queue)
}

fn quality_luma(pixel: &[u8]) -> f32 {
    (0.2126 * f32::from(pixel[0]) + 0.7152 * f32::from(pixel[1]) + 0.0722 * f32::from(pixel[2]))
        / 255.0
}

/// Bilinear RGBA resize used only for the deterministic old-path control.
/// Pixel-centre mapping matches the GPU sampler's 1:1 texel centres while the
/// 256-square intermediate reproduces the old target's x=5 px upscale knots.
fn quality_bilinear_resize(
    pixels: &[u8],
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> Vec<u8> {
    assert_eq!(
        pixels.len(),
        source_width as usize * source_height as usize * 4,
        "source RGBA length must match its dimensions"
    );
    assert!(source_width > 0 && source_height > 0);
    assert!(target_width > 0 && target_height > 0);

    let sample = |x: f32, y: f32| -> [f32; 4] {
        let x = x.clamp(0.0, source_width as f32 - 1.0);
        let y = y.clamp(0.0, source_height as f32 - 1.0);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(source_width as usize - 1);
        let y1 = (y0 + 1).min(source_height as usize - 1);
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        let mut out = [0.0; 4];
        for channel in 0..4 {
            let top = f32::from(pixels[(y0 * source_width as usize + x0) * 4 + channel])
                * (1.0 - fx)
                + f32::from(pixels[(y0 * source_width as usize + x1) * 4 + channel]) * fx;
            let bottom = f32::from(pixels[(y1 * source_width as usize + x0) * 4 + channel])
                * (1.0 - fx)
                + f32::from(pixels[(y1 * source_width as usize + x1) * 4 + channel]) * fx;
            out[channel] = top * (1.0 - fy) + bottom * fy;
        }
        out
    };

    let mut resized = Vec::with_capacity(target_width as usize * target_height as usize * 4);
    for y in 0..target_height {
        let source_y = (y as f32 + 0.5) * source_height as f32 / target_height as f32 - 0.5;
        for x in 0..target_width {
            let source_x = (x as f32 + 0.5) * source_width as f32 / target_width as f32 - 0.5;
            let pixel = sample(source_x, source_y);
            for channel in pixel {
                resized.push(channel.round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    resized
}

/// Resolution-sensitive 5 px periodicity ratio over the central cube region.
///
/// A native smooth ring has no preferred horizontal sample phase. A 256-wide
/// bilinear upscale to 1280 px is linear within each five-pixel span and kinks
/// at every source texel boundary, concentrating horizontal curvature in one
/// `x mod 5` phase. The ratio is invariant to overall brightness.
fn quality_five_pixel_kink_ratio(pixels: &[u8], width: u32, height: u32) -> f32 {
    const PERIOD: usize = 5;
    let width = width as usize;
    let height = height as usize;
    assert!(width > 2 && height > 2);
    let x_start = width / 4 + 1;
    let x_end = width * 3 / 4 - 1;
    let y_start = height / 4;
    let y_end = height * 3 / 4;
    let mut phase_sums = [0.0_f64; PERIOD];
    let mut phase_counts = [0_usize; PERIOD];

    for y in y_start..y_end {
        for x in x_start..x_end {
            let index = (y * width + x) * 4;
            let left = f64::from(quality_luma(&pixels[index - 4..index]));
            let center = f64::from(quality_luma(&pixels[index..index + 4]));
            let right = f64::from(quality_luma(&pixels[index + 4..index + 8]));
            let curvature = (left - 2.0 * center + right).abs();
            let phase = x % PERIOD;
            phase_sums[phase] += curvature;
            phase_counts[phase] += 1;
        }
    }

    assert!(
        phase_counts.iter().all(|count| *count > 0),
        "all 5 px sample phases must be represented"
    );
    let phase_means: Vec<f32> = phase_sums
        .iter()
        .zip(&phase_counts)
        .map(|(sum, count)| (*sum / *count as f64) as f32)
        .collect();
    let mean = phase_means.iter().sum::<f32>() / PERIOD as f32;
    let max = phase_means.iter().copied().fold(0.0_f32, f32::max);
    if mean > 0.0 {
        max / mean
    } else {
        f32::INFINITY
    }
}

/// Outer radius of the direct-only cube, used to exclude its sharp silhouette
/// and face rings from the radial trail measurement.
fn quality_visible_radius(pixels: &[u8], width: u32, height: u32) -> f32 {
    let width = width as usize;
    let height = height as usize;
    let center_x = (width - 1) as f64 * 0.5;
    let center_y = (height - 1) as f64 * 0.5;
    let mut max_radius = 0.0_f64;

    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] > 0 {
            let x = (index % width) as f64 - center_x;
            let y = (index / width) as f64 - center_y;
            max_radius = max_radius.max(x.hypot(y));
        }
    }

    max_radius as f32
}

/// Angularly averaged luma profile outside the direct cube. The normalized
/// mean absolute radial slope measures profile shape independently of peak
/// brightness, so merely dimming the waves cannot satisfy diffuseness.
fn quality_radial_trail(
    pixels: &[u8],
    width: u32,
    height: u32,
    direct_radius: f32,
) -> (f32, f32, f32, f32, u32, u32) {
    let width = width as usize;
    let height = height as usize;
    let center_x = (width - 1) as f64 * 0.5;
    let center_y = (height - 1) as f64 * 0.5;
    let inner = (direct_radius + 4.0).ceil() as u32;
    let outer = (((height as f64) * 0.72)
        .min(f64::from(direct_radius) + 320.0)
        .floor() as u32)
        .max(inner + 8);
    let bins = (outer - inner + 1) as usize;
    let mut sums = vec![0.0_f64; bins];
    let mut counts = vec![0_u32; bins];

    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let x = (index % width) as f64 - center_x;
        let y = (index / width) as f64 - center_y;
        let radius = x.hypot(y);
        if radius >= f64::from(inner) && radius <= f64::from(outer) {
            let bin = (radius.floor() as usize).saturating_sub(inner as usize);
            sums[bin] += f64::from(quality_luma(pixel));
            counts[bin] += 1;
        }
    }

    assert!(
        counts.iter().filter(|count| **count > 0).count() >= bins / 2,
        "radial trail annulus {inner}..={outer} has too few sampled radii"
    );
    let profile: Vec<(u32, f32)> = sums
        .iter()
        .zip(&counts)
        .enumerate()
        .filter(|entry| *entry.1.1 > 0)
        .map(|(index, (sum, count))| (inner + index as u32, (*sum / f64::from(*count)) as f32))
        .collect();
    let peak = profile
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0_f32, f32::max);
    let radial_span = (profile.last().expect("non-empty profile").0
        - profile.first().expect("non-empty profile").0) as f32;
    let mean_abs_slope = profile
        .windows(2)
        .map(|pair| (pair[1].1 - pair[0].1).abs())
        .sum::<f32>()
        / radial_span;
    let max_abs_slope = profile
        .windows(2)
        .map(|pair| (pair[1].1 - pair[0].1).abs() / (pair[1].0 - pair[0].0) as f32)
        .fold(0.0_f32, f32::max);
    let normalized_slope = if peak > 0.0 {
        mean_abs_slope / peak
    } else {
        f32::INFINITY
    };

    (
        peak,
        mean_abs_slope,
        max_abs_slope,
        normalized_slope,
        inner,
        outer,
    )
}

/// Regression contract for the accidental 256-square Sound Cube port: the
/// cube must be sharp at scene resolution while its feedback remains a small,
/// diffuse buffer. This is deliberately pixel-based; no production target-size
/// accessor is required.
#[test]
fn sound_cube_uses_scene_resolution_for_cube_and_diffuses_the_trail() {
    use std::cmp::Ordering;

    use crate::point10::scenes::sound_cube::{RTT_SIZE, SCALE_AUDIO, SCALE_BASE, sound_cube_drive};

    // The native image must not prefer a 5 px sample phase. The deterministic
    // 256-square downscale/upscale control must strongly prefer one, proving
    // that this metric detects the old low-resolution capture path.
    const CUBE_KINK_RATIO_MAX: f32 = 2.0;
    const UPSCALE_CONTROL_KINK_RATIO_MIN: f32 = 2.0;
    // RED trail baselines: require a 30% reduction in profile slope
    // (independent of peak), and a 10% lower peak that remains clearly visible.
    const TRAIL_NORMALIZED_SLOPE_MAX: f32 = 0.003212 * 0.7;
    const TRAIL_PEAK_MAX: f32 = 0.287752 * 0.9;
    const TRAIL_PEAK_MIN: f32 = 0.02;

    let mut failures = Vec::new();

    // Audio mapping is not the tuning knob: preserve Expression1 and the
    // bridge's damping/AGC selection exactly.
    assert_eq!(SCALE_BASE, 0.3);
    assert_eq!(SCALE_AUDIO, 8.0);
    for (bass, expected) in [(0.0, 0.3), (0.5, 4.3), (1.0, 8.3)] {
        let drive = sound_cube_drive(bass, 1.0);
        assert!(
            (drive.expression - expected).abs() < 1e-6,
            "sound_cube_drive({bass}) changed: {:?}",
            drive.expression
        );
        assert!(
            (drive.scale - expected).abs() < 1e-6,
            "sound_cube_drive({bass}) scale changed: {:?}",
            drive.scale
        );
    }
    let scene_source = include_str!("scenes/sound_cube.rs");
    assert!(
        scene_source.contains("sound_cube_drive(self.audio.band(-1, 1, 0, 0.0), self.pulse_scale)"),
        "Sound Cube must keep reading the existing smoothed bass lane"
    );
    let audio_source = include_str!("audio_bridge.rs");
    for needle in [
        "let src_lanes = if levers.use_agc {",
        "let tau = 0.02 + levers.smoothing * 0.6;",
        "self.smoo[damping * 3 + index]",
    ] {
        assert!(
            audio_source.contains(needle),
            "audio bridge contract changed: lost {needle}"
        );
    }

    let Some((device, queue)) = gpu() else {
        panic!("GPU fallback unavailable: Sound Cube quality contract could not run");
    };
    let direct = render_sound_cube_quality(&device, &queue, 0.0, 1);
    let trail = render_sound_cube_quality(&device, &queue, 1.0, SOUND_CUBE_QUALITY_FRAMES);
    let cube_kink_ratio =
        quality_five_pixel_kink_ratio(&direct, SOUND_CUBE_QUALITY_WIDTH, SOUND_CUBE_QUALITY_HEIGHT);
    let downscaled_control = quality_bilinear_resize(
        &direct,
        SOUND_CUBE_QUALITY_WIDTH,
        SOUND_CUBE_QUALITY_HEIGHT,
        RTT_SIZE,
        RTT_SIZE,
    );
    let upscaled_control = quality_bilinear_resize(
        &downscaled_control,
        RTT_SIZE,
        RTT_SIZE,
        SOUND_CUBE_QUALITY_WIDTH,
        SOUND_CUBE_QUALITY_HEIGHT,
    );
    let control_kink_ratio = quality_five_pixel_kink_ratio(
        &upscaled_control,
        SOUND_CUBE_QUALITY_WIDTH,
        SOUND_CUBE_QUALITY_HEIGHT,
    );
    let direct_radius =
        quality_visible_radius(&direct, SOUND_CUBE_QUALITY_WIDTH, SOUND_CUBE_QUALITY_HEIGHT);
    let (trail_peak, radial_slope, radial_max_slope, normalized_slope, inner, outer) =
        quality_radial_trail(
            &trail,
            SOUND_CUBE_QUALITY_WIDTH,
            SOUND_CUBE_QUALITY_HEIGHT,
            direct_radius,
        );

    eprintln!(
        "sound cube quality: native_5px_kink_ratio={cube_kink_ratio:.6} \
         (max {CUBE_KINK_RATIO_MAX:.6}); simulated_256_5px_kink_ratio={control_kink_ratio:.6} \
         (min {UPSCALE_CONTROL_KINK_RATIO_MIN:.6}); direct_radius={direct_radius:.3}; \
         trail_annulus={inner}..={outer}; trail_peak={trail_peak:.6} \
         (min {TRAIL_PEAK_MIN:.6}, max {TRAIL_PEAK_MAX:.6}); \
         radial_mean_abs_slope={radial_slope:.6}; radial_max_abs_slope={radial_max_slope:.6}; \
         radial_normalized_slope={normalized_slope:.6} (max {TRAIL_NORMALIZED_SLOPE_MAX:.6})"
    );

    match cube_kink_ratio.partial_cmp(&CUBE_KINK_RATIO_MAX) {
        Some(Ordering::Less | Ordering::Equal) => {}
        _ => failures.push(format!(
            "native 5 px kink ratio {cube_kink_ratio:.6} exceeds {CUBE_KINK_RATIO_MAX:.6}; \
             the cube still has low-resolution resampling periodicity"
        )),
    }
    match control_kink_ratio.partial_cmp(&UPSCALE_CONTROL_KINK_RATIO_MIN) {
        Some(Ordering::Greater | Ordering::Equal) => {}
        _ => failures.push(format!(
            "simulated 256-wide 5 px kink ratio {control_kink_ratio:.6} is below \
             {UPSCALE_CONTROL_KINK_RATIO_MIN:.6}; the resolution control is not discriminating"
        )),
    }
    match trail_peak.partial_cmp(&TRAIL_PEAK_MAX) {
        Some(Ordering::Less | Ordering::Equal) => {}
        _ => failures.push(format!(
            "trail peak {trail_peak:.6} exceeds {TRAIL_PEAK_MAX:.6}; the waves are still too harsh/bright"
        )),
    }
    match trail_peak.partial_cmp(&TRAIL_PEAK_MIN) {
        Some(Ordering::Greater | Ordering::Equal) => {}
        _ => failures.push(format!(
            "trail peak {trail_peak:.6} is below {TRAIL_PEAK_MIN:.6}; the waves are no longer clearly visible"
        )),
    }
    match normalized_slope.partial_cmp(&TRAIL_NORMALIZED_SLOPE_MAX) {
        Some(Ordering::Less | Ordering::Equal) => {}
        _ => failures.push(format!(
            "radial normalized slope {normalized_slope:.6} exceeds {TRAIL_NORMALIZED_SLOPE_MAX:.6}; \
             the feedback waves are still too sharp"
        )),
    }

    if failures.is_empty() {
        eprintln!("Sound Cube resolution/diffuse-trail contract passed");
    } else {
        panic!(
            "Sound Cube resolution/diffuse-trail contract failed ({} checks):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

// ————— batch 5a —————

/// HollowCube's cubes are 0.01 world units in a 0.5-unit shell seen by a
/// 50° camera at z = 2.2: a 64 px frame gives them a fraction of a pixel, so
/// the smoke test renders at 128 px.
const HOLLOW_CUBE_SIZE: u32 = 128;

#[test]
fn hollow_cube_renders() {
    let mut scene = crate::point10::scenes::hollow_cube::HollowCube::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, HOLLOW_CUBE_SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "HollowCube produced an empty frame at {HOLLOW_CUBE_SIZE}x{HOLLOW_CUBE_SIZE}"
    );
}

/// `CLEAR_ALPHA = 0` and `Shader2`'s `alpha = c2.a` (the cube capture's), so
/// the empty space around the shell stays transparent.
#[test]
fn hollow_cube_keeps_the_background_transparent() {
    let mut scene = crate::point10::scenes::hollow_cube::HollowCube::new();
    assert_eq!(scene.clear_alpha(), 0.0);

    let Some(pixels) = render_owned_chain_two_frames(&mut scene, HOLLOW_CUBE_SIZE) else {
        return;
    };
    let (transparent, opaque, total) = alpha_counts(&pixels);
    assert!(
        transparent > 0,
        "expected an alpha-0 background, but every pixel was drawn ({total} px)"
    );
    assert!(
        opaque > 0,
        "expected opaque cubes, but none reached alpha 255"
    );
}

/// The particle field is ~2 px per billboard at 1080p (`scale = 0.008 *
/// particleSize` at a 9.9-unit orbit), so the smoke test maxes `particleSize`
/// and renders at [`PARTICLE_SIZE`] to give the dots something to rasterise.
fn attractor_particles_scene() -> crate::point10::scenes::attractor_particles::AttractorParticles {
    let mut scene = crate::point10::scenes::attractor_particles::AttractorParticles::new();
    // The smoke-test frame is square, so pull the orbit back until the 5x5
    // particle field no longer fills it; max the sprite size so the ~2 px
    // 1080p billboards still rasterise at 256 px.
    scene.apply_props(|key| match key {
        "particleSize" => Some(4.0),
        "cameraDistance" => Some(20.0),
        _ => None,
    });
    scene
}

#[test]
fn attractor_particles_renders() {
    let mut scene = attractor_particles_scene();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, PARTICLE_SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "AttractorParticles produced an empty frame at {PARTICLE_SIZE}x{PARTICLE_SIZE}"
    );
    // `SCENE_BACKGROUND = null` / `CLEAR_ALPHA = 0`: the layer alpha is the
    // JS's transparent clear.
    assert_eq!(scene.clear_alpha(), 0.0);
}

/// The particle field is additive on a transparent clear. At the JS density
/// (2^18 points at `PARTICLE_SIZE`) the cloud covers the whole frame, so the
/// meaningful layer-alpha property is that it varies (some pixels stay
/// translucent) and that opaque pixels exist — not alpha-0 corners.
#[test]
fn attractor_particles_keeps_a_translucent_layer() {
    let mut scene = attractor_particles_scene();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, PARTICLE_SIZE) else {
        return;
    };
    let (_, opaque, total) = alpha_counts(&pixels);
    let min = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3])
        .min()
        .unwrap_or(0);
    assert!(
        min < 255,
        "the additive field must leave translucent pixels ({total} px, all alpha 255)"
    );
    assert!(
        opaque > 0,
        "expected opaque pixels, but none reached alpha 255"
    );
}

// ————— the volumetric room scenes (SmokeRoom / DvdRoom) —————
//
// Both scenes run the JS `150 x 150 x 300` grid in production. The GPU tests
// scale it down: a `32 x 32 x 64` grid keeps every compute pass and the whole
// raymarch, but a fraction of the voxel work, so a software adapter finishes
// the simulation in reasonable time. Every world-space parameter (grid origin,
// volume size, ray step count) is unchanged.

/// The grid the volumetric tests run at.
///
/// Half the JS resolution: the emission is per emitter vertex, so a much
/// smaller grid concentrates it into fewer voxels and saturates faster; `64 x
/// 64 x 128` is the smallest grid that still reads like the JS field. The
/// production scenes run [`crate::point10::volume::VolumeGrid::JS`].
fn volume_test_grid() -> crate::point10::volume::VolumeGrid {
    crate::point10::volume::VolumeGrid::new(64, 64, 128)
}

/// Small on purpose: the volume covers most of the frame at any size and the
/// structural checks are all per-pixel.
const VOLUME_SIZE: u32 = 128;
/// Frames of simulation before the structural checks (two steps each, so ~240
/// simulation steps: past the JS's three-second emissive fade-in, with a
/// developed, turbulent dye field).
const VOLUME_WARMUP: usize = 120;

fn smoke_room_scene() -> crate::point10::scenes::smoke_room::SmokeRoom {
    crate::point10::scenes::smoke_room::SmokeRoom::with_grid(volume_test_grid())
}

fn dvd_room_scene() -> crate::point10::scenes::dvd_room::DvdRoom {
    crate::point10::scenes::dvd_room::DvdRoom::with_grid(volume_test_grid())
}

/// Mean composite luminance (colour × alpha — what the layer shows over
/// black) of the dimmest and the brightest `fraction` of the frame.
///
/// The pair encodes "a bounded room, not a flat sheet": the fire's core must
/// dominate the room's dim outer air. A fog constant, an ungated emissive
/// floor or a broken density falloff saturates the volume uniformly and
/// collapses the ratio towards one; a dead emitter collapses both to zero.
fn luminance_extremes(pixels: &[u8], fraction: f32) -> (f32, f32) {
    let mut values: Vec<f32> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                * (pixel[3] as f32 / 255.0)
        })
        .collect();
    values.sort_by(|a, b| a.total_cmp(b));
    let take = ((values.len() as f32 * fraction).ceil() as usize).clamp(1, values.len());
    let dim = values.iter().take(take).sum::<f32>() / take as f32;
    let bright = values[values.len() - take..].iter().sum::<f32>() / take as f32;
    (dim, bright)
}

/// Fraction of the frame that stays dark once composited over black.
///
/// The JS's `alpha = max(rgb)` layer is usually semi-transparent, so the
/// composite (not the raw alpha) is what the viewer sees; a working room scene
/// leaves most of the frame dark around the fire.
fn dark_fraction(pixels: &[u8]) -> f32 {
    let mut dark = 0_usize;
    let mut total = 0_usize;
    for pixel in pixels.as_chunks::<4>().0 {
        let alpha = pixel[3] as f32 / 255.0;
        let luminance =
            (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                * alpha;
        if luminance < 16.0 {
            dark += 1;
        }
        total += 1;
    }
    dark as f32 / total.max(1) as f32
}

/// Standard deviation of the luminance of the drawn (alpha > 0) pixels.
///
/// A flat haze or a solid clear would score ~0; a raymarched fire/smoke field
/// has hot cores, warm edges and transparent background.
fn luminance_spread(pixels: &[u8]) -> f32 {
    let mut values = Vec::new();
    for pixel in pixels.as_chunks::<4>().0 {
        if pixel[3] == 0 {
            continue;
        }
        values.push(0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32);
    }
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    (values
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f32>()
        / values.len() as f32)
        .sqrt()
}

/// The module composites over the layers below: the room's emptiness (and the
/// glass) must stay translucent while the fire reaches a strong alpha.
fn assert_translucent_layer(pixels: &[u8]) {
    let translucent = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] < 128)
        .count();
    let strong = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 200)
        .count();
    let total = pixels.len() / 4;
    assert!(
        translucent > total / 10,
        "the room must stay translucent around the fire ({translucent} of {total} px)"
    );
    assert!(
        strong > 0,
        "expected some strongly covered fire/logo pixels (max alpha {})",
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3])
            .max()
            .unwrap_or(0)
    );
}

/// `alpha = max(rgb)` (the JS's `outputNode`): alpha may only be zero where
/// the pixel is black, and every drawn pixel carries alpha.
fn assert_alpha_is_max_rgb(pixels: &[u8]) {
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let black = pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0;
        assert_eq!(
            pixel[3] == 0,
            black,
            "pixel {index} ({pixel:?}) breaks alpha = max(rgb)"
        );
    }
}

#[test]
fn smoke_room_renders() {
    let mut scene = smoke_room_scene();
    let payload = loud_payload();
    let Some(frames) =
        render_owned_chain_frames(&mut scene, VOLUME_SIZE, VOLUME_WARMUP, &payload, &payload)
    else {
        return;
    };
    assert!(
        any_lit(&frames.before),
        "SmokeRoom produced an empty frame at {VOLUME_SIZE}x{VOLUME_SIZE}"
    );
}

/// The raymarch must resolve structure: a non-uniform, warm-lit field that
/// fills the room, follows the `alpha = max(rgb)` rule and reacts to audio.
#[test]
fn smoke_room_volume_is_structured() {
    let mut scene = smoke_room_scene();
    assert_eq!(scene.clear_alpha(), 0.0);
    assert_eq!(scene.grid().cell_count(), 64 * 64 * 128);

    // Quiet audio for the structural frame (the field does not saturate), then
    // a loud frame to prove the audio mapping reaches the volume.
    let Some(RenderedFrames {
        before: pixels,
        after: next,
        ..
    }) = render_owned_chain_frames(
        &mut scene,
        VOLUME_SIZE,
        VOLUME_WARMUP,
        &quiet_payload(),
        &loud_payload(),
    )
    else {
        return;
    };

    // Non-uniform: the drawn pixels span a wide luminance range.
    let spread = luminance_spread(&pixels);
    assert!(
        spread > 12.0,
        "the volume looks flat (luminance spread {spread})"
    );
    // Warm-lit: the average drawn pixel is red-dominant (the fire ramp).
    let mut red = 0.0;
    let mut blue = 0.0;
    let mut drawn = 0.0;
    for pixel in pixels.as_chunks::<4>().0 {
        if pixel[3] == 0 {
            continue;
        }
        red += pixel[0] as f32;
        blue += pixel[2] as f32;
        drawn += 1.0;
    }
    assert!(drawn > 0.0);
    assert!(
        red / drawn > blue / drawn * 1.2,
        "the volume is not warm-lit (r {:.1}, b {:.1})",
        red / drawn,
        blue / drawn
    );
    // Fills the room with a *bounded* glow. At this square frame the sim box
    // covers the whole view (the glass top is stretched past the frame and
    // the box's near face overflows it — the production 16:9 frame keeps
    // dark bands at the sides), so once the curl-noise precompute actually
    // runs — like the JS's init-time curl pass, fixed by the "real uniforms"
    // change — the developed turbulent dye field legitimately tints every ray
    // through the closed room, and no pixel stays dark. The old
    // `dark_fraction > 0.15` was calibrated against the pre-curl field, which
    // only stayed compact because the precompute never ran. The invariant
    // the JS actually guarantees is the gradient: the fire's core is several
    // times brighter than the room's dim bounds (measured: brightest 5% ≈
    // 248 vs dimmest 5% ≈ 31 on lavapipe; a flat sheet scores ~1, a dead
    // emitter ~0/0).
    let (dim, bright) = luminance_extremes(&pixels, 0.05);
    assert!(
        bright > dim * 3.0,
        "the glow is a flat sheet (brightest 5% {bright:.1} vs dimmest 5% {dim:.1})"
    );
    assert_alpha_is_max_rgb(&pixels);
    assert_translucent_layer(&pixels);

    // Two audio frames produce different images (the emission pulses).
    assert_ne!(pixels, next, "consecutive audio frames are identical");
}

#[test]
fn dvd_room_renders() {
    let mut scene = dvd_room_scene();
    let payload = loud_payload();
    let Some(frames) =
        render_owned_chain_frames(&mut scene, VOLUME_SIZE, VOLUME_WARMUP, &payload, &payload)
    else {
        return;
    };
    assert!(
        any_lit(&frames.before),
        "DvdRoom produced an empty frame at {VOLUME_SIZE}x{VOLUME_SIZE}"
    );
}

/// The DVD room adds the bouncing logo on top of the same volume: the logo
/// must be visible (opaque-ish pixels), the smoke field structured, the
/// background transparent in the corners and the audio-reactive palette must
/// change the frame.
#[test]
fn dvd_room_volume_is_structured() {
    let mut scene = dvd_room_scene();
    assert_eq!(scene.clear_alpha(), 0.0);
    assert_eq!(scene.grid().cell_count(), 64 * 64 * 128);

    // Quiet audio for the structural frame (the field does not saturate), then
    // a loud frame to prove the audio mapping reaches the volume.
    let Some(RenderedFrames {
        before: pixels,
        after: next,
        ..
    }) = render_owned_chain_frames(
        &mut scene,
        VOLUME_SIZE,
        VOLUME_WARMUP,
        &quiet_payload(),
        &loud_payload(),
    )
    else {
        return;
    };

    let spread = luminance_spread(&pixels);
    assert!(
        spread > 12.0,
        "the DVD room volume looks flat (luminance spread {spread})"
    );
    let dark = dark_fraction(&pixels);
    assert!(
        dark > 0.15,
        "the fire covers the whole frame (only {:.0}% dark) — the room bounds are lost",
        dark * 100.0
    );
    // The logo is bright: some pixel is well above the smoke's own level.
    let max_luminance = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| 0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
        .fold(0.0, f32::max);
    assert!(
        max_luminance > 180.0,
        "the DVD logo never lights up (max luminance {max_luminance})"
    );
    assert_alpha_is_max_rgb(&pixels);
    assert_translucent_layer(&pixels);
    assert_ne!(pixels, next, "consecutive audio frames are identical");
}

/// The mean luminance of the brightest `fraction` of the drawn pixels — the
/// fire's own colour, without the (grey) floor diluting it.
fn warm_ratio_of_brightest(pixels: &[u8], fraction: f32) -> f32 {
    let mut drawn: Vec<(f32, f32, f32)> = Vec::new();
    for pixel in pixels.as_chunks::<4>().0 {
        if pixel[3] == 0 {
            continue;
        }
        drawn.push((pixel[0] as f32, pixel[1] as f32, pixel[2] as f32));
    }
    if drawn.is_empty() {
        return 0.0;
    }
    drawn.sort_by(|a, b| (b.0 + b.1 + b.2).total_cmp(&(a.0 + a.1 + a.2)));
    let take = ((drawn.len() as f32 * fraction).ceil() as usize).max(1);
    let (mut red, mut blue) = (0.0, 0.0);
    for (r, _, b) in drawn.iter().take(take) {
        red += r;
        blue += b;
    }
    red / blue.max(1.0)
}

/// `alpha = max(rgb)` (the JS's `outputNode`) within 8-bit quantisation.
///
/// The final pass writes alpha from the **linear HDR** max channel while the
/// output pass tone-maps the colour afterwards (three does the same), so a very
/// dim pixel can round its colour up to 1..3/255 while its alpha rounds down to
/// 0. The layer contract is still "bright pixels carry alpha, black pixels stay
/// clear".
fn assert_alpha_tracks_rgb(pixels: &[u8]) {
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let max_rgb = pixel[0].max(pixel[1]).max(pixel[2]);
        let alpha = pixel[3];
        assert!(
            !(max_rgb >= 4 && alpha == 0),
            "pixel {index} ({pixel:?}) is visible but has zero alpha"
        );
        assert!(
            !(max_rgb == 0 && alpha > 8),
            "pixel {index} ({pixel:?}) is black but opaque"
        );
    }
}

// ————— batch 5b —————

/// The tunnel fills the frame with hoops, so 96 px shows several rings and the
/// SSAO creases without turning into a solid block.
/// `screenClearAlpha = 0` and the final pass writes `alpha = clamp(max(rgb))`.
/// With ACES exposure 1 the tone-mapped hoops never reach 1.0, so the layer is
/// never fully opaque — the JS behaves the same. What must hold is that the
/// background stays near-transparent (the fog floor is the only lift) while the
/// lit hoops carry a clearly higher alpha.

#[test]
fn rolling_fire_renders() {
    let mut scene =
        crate::point10::scenes::rolling_fire::RollingFire::with_grid(volume_test_grid());
    let payload = loud_payload();
    let Some(frames) =
        render_owned_chain_frames(&mut scene, VOLUME_SIZE, VOLUME_WARMUP, &payload, &payload)
    else {
        return;
    };
    assert!(
        any_lit(&frames.before),
        "RollingFire produced an empty frame at {VOLUME_SIZE}x{VOLUME_SIZE}"
    );
}

/// R-34: the JS analyser exists in silence, so RollingFire keeps animating
/// (emitter, sim steps, spin) instead of freezing. Exercise the GPU path, not
/// just `update()`: two consecutive silent frames must differ.
#[test]
fn rolling_fire_advances_without_audio() {
    let mut scene =
        crate::point10::scenes::rolling_fire::RollingFire::with_grid(volume_test_grid());
    let silent = BandPayload::silent(44_100.0);
    let Some(frames) =
        render_owned_chain_frames(&mut scene, VOLUME_SIZE, VOLUME_WARMUP + 2, &silent, &silent)
    else {
        return;
    };

    assert!(
        any_lit(&frames.before),
        "silent RollingFire produced an empty frame"
    );
    assert_ne!(
        frames.before, frames.after,
        "silent RollingFire froze: the volume/ball must keep advancing with zero bands"
    );
}

/// RollingFire must render a real raymarched fire volume around the lava
/// sphere, keep the transparent-overlay contract and react to the music.
#[test]
fn rolling_fire_volume_is_structured() {
    let mut scene =
        crate::point10::scenes::rolling_fire::RollingFire::with_grid(volume_test_grid());
    assert_eq!(scene.clear_alpha(), 0.0);
    assert_eq!(scene.grid().cell_count(), 64 * 64 * 128);

    let Some(RenderedFrames {
        before: pixels,
        after: next,
        ..
    }) = render_owned_chain_frames(
        &mut scene,
        VOLUME_SIZE,
        VOLUME_WARMUP,
        &quiet_payload(),
        &loud_payload(),
    )
    else {
        return;
    };

    let spread = luminance_spread(&pixels);
    assert!(
        spread > 12.0,
        "the fire volume looks flat (luminance spread {spread})"
    );
    // The fire is the brightest thing in the frame and it is warm.
    let warm = warm_ratio_of_brightest(&pixels, 0.1);
    assert!(
        warm > 1.2,
        "the fire is not warm-lit (brightest-pixel r/b {warm})"
    );
    // The volume does not cover the whole frame: the floor, the empty grid and
    // the black background keep the corners dark.
    let dark = dark_fraction(&pixels);
    assert!(
        dark > 0.15,
        "the fire covers the whole frame (only {:.0}% dark)",
        dark * 100.0
    );
    assert_alpha_tracks_rgb(&pixels);
    assert_translucent_layer(&pixels);
    // The ball moved under the quiet payload and then the loud one; two
    // different frames prove the fire is animating.
    assert_ne!(pixels, next, "consecutive audio frames are identical");
    assert_ne!(scene.ball(), rolling_fire::Ball::new());
}

#[test]
fn ball_of_fire_renders() {
    let mut scene = crate::point10::scenes::rolling_fire::BallOfFire::with_grid(volume_test_grid());
    let payload = loud_payload();
    let Some(frames) =
        render_owned_chain_frames(&mut scene, VOLUME_SIZE, VOLUME_WARMUP, &payload, &payload)
    else {
        return;
    };
    assert!(
        any_lit(&frames.before),
        "BallOfFire produced an empty frame at {VOLUME_SIZE}x{VOLUME_SIZE}"
    );
}

/// Ball of Fire draws the same volume with the ball pinned: a lit, structurally
/// varied frame whose ball never moves, and a transparent overlay.
#[test]
fn ball_of_fire_is_frozen_and_structured() {
    let mut scene = crate::point10::scenes::rolling_fire::BallOfFire::with_grid(volume_test_grid());
    let start = scene.inner().ball();
    assert_eq!(scene.role(), rolling_fire::SceneRole::Standalone);
    assert!(scene.frozen());

    let Some(RenderedFrames {
        before: pixels,
        after: next,
        ..
    }) = render_owned_chain_frames(
        &mut scene,
        VOLUME_SIZE,
        VOLUME_WARMUP,
        &quiet_payload(),
        &loud_payload(),
    )
    else {
        return;
    };

    assert_eq!(scene.inner().ball(), start, "the ball must stay pinned");
    let spread = luminance_spread(&pixels);
    assert!(
        spread > 12.0,
        "the ball-of-fire volume looks flat (luminance spread {spread})"
    );
    let warm = warm_ratio_of_brightest(&pixels, 0.1);
    assert!(
        warm > 1.2,
        "the ball of fire is not warm-lit (brightest-pixel r/b {warm})"
    );
    let dark = dark_fraction(&pixels);
    assert!(
        dark > 0.15,
        "the fire covers the whole frame (only {:.0}% dark)",
        dark * 100.0
    );
    assert_alpha_tracks_rgb(&pixels);
    assert_translucent_layer(&pixels);
    assert_ne!(pixels, next, "consecutive audio frames are identical");
}

#[test]
fn latent_backrooms_renders() {
    let mut scene = crate::point10::scenes::latent_backrooms::LatentBackrooms::new();
    let Some(pixels) = render_owned_chain_two_frames(&mut scene, SIZE) else {
        return;
    };
    assert!(
        any_lit(&pixels),
        "LatentBackrooms produced an empty frame at {SIZE}x{SIZE}"
    );
    // `scene.background = 0x050505` and the CRT pass forces alpha 1, so the
    // module is opaque everywhere (its layer alpha is 1, not transparent).
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255),
        "LatentBackrooms must stay opaque (CRT pass alpha = 1)"
    );
}

#[test]
fn every_scene_survives_dispose_and_reinit() {
    let Some((device, queue)) = gpu() else {
        return;
    };
    for mut scene in all_scenes() {
        let mut ctx = SceneContext::new(&device, &queue, HDR_FORMAT);
        ctx.resize(SIZE, SIZE);
        scene.init(&mut ctx);
        let bridge = AudioBridge::new();
        scene.update(1.0 / 60.0, &bridge);
        scene.dispose();

        // Rendering after dispose is a no-op, not a panic.
        let target = TestTarget::new(&device, SIZE, SIZE);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("point10-dispose-test"),
        });
        scene.render(&mut encoder, &target.view);
        queue.submit(Some(encoder.finish()));

        // And re-initialising works.
        scene.init(&mut ctx);
        assert_eq!(scene.name(), scene.meta().name);
    }
}

/// Render SmokeRoom and DvdRoom at 256x256 and write PNGs for inspection.
///
/// `MODV_VOLUME_GRID="x,y,z"` overrides the simulation grid,
/// `MODV_VOLUME_FRAMES` the frame count, `MODV_VOLUME_SIZE` the image size and
/// `MODV_VOLUME_DIR` the output directory (default `/app/crates/target`).
///
/// Ignored by default: it is the offline fidelity check, not a CI test.
#[test]
#[ignore = "renders reference frames for manual inspection"]
fn dump_volume_reference_frames() {
    let size = std::env::var("MODV_VOLUME_SIZE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(256);
    let frames = std::env::var("MODV_VOLUME_FRAMES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(60);
    let grid = std::env::var("MODV_VOLUME_GRID")
        .ok()
        .and_then(|value| {
            let parts: Vec<u32> = value
                .split(',')
                .filter_map(|part| part.trim().parse::<u32>().ok())
                .collect();
            (parts.len() == 3)
                .then(|| crate::point10::volume::VolumeGrid::new(parts[0], parts[1], parts[2]))
        })
        .unwrap_or(crate::point10::volume::VolumeGrid::JS);
    let directory =
        std::env::var("MODV_VOLUME_DIR").unwrap_or_else(|_| "/app/crates/target".to_string());

    let quiet = std::env::var("MODV_VOLUME_QUIET").is_ok();
    let no_fire = std::env::var("MODV_VOLUME_NO_FIRE").is_ok();
    let density_override = std::env::var("MODV_VOLUME_DENSITY")
        .ok()
        .and_then(|value| value.parse::<f32>().ok());
    let mut smoke = crate::point10::scenes::smoke_room::SmokeRoom::with_grid(grid);
    if let Some(density) = density_override {
        smoke.apply_props(|key| (key == "smokeDensity").then_some(density));
    }
    if no_fire {
        smoke.apply_props(|key| match key {
            "fireIntensity" => Some(0.0),
            "smokeDensity" => Some(0.0),
            _ => None,
        });
    }
    let payload = if quiet {
        quiet_payload()
    } else if std::env::var("MODV_VOLUME_SILENT").is_ok() {
        BandPayload::silent(44_100.0)
    } else {
        loud_payload()
    };
    let mut dye_report: Option<String> = None;
    if let Some(rendered) = render_owned_chain_frames(&mut smoke, size, frames, &payload, &payload)
    {
        let (transparent, opaque, total) = alpha_counts(&rendered.before);
        eprintln!(
            "smoke: grid {:?} cell_count {} frames {frames} transparent {transparent}/{total} opaque {opaque} spread {:.2}",
            grid,
            grid.cell_count(),
            luminance_spread(&rendered.before)
        );
        write_png(
            &format!("{directory}/smoke-room.png"),
            &rendered.before,
            size,
        );
        write_png(
            &format!("{directory}/smoke-room-next.png"),
            &rendered.after,
            size,
        );
        if let Some(sim) = smoke.debug_sim() {
            dye_report = report_dye(sim, &rendered.device, &rendered.queue, grid);
        }
    }
    if let Some(report) = dye_report {
        eprintln!("{report}");
    }

    let mut dvd = crate::point10::scenes::dvd_room::DvdRoom::with_grid(grid);
    let mut dvd_dye: Option<String> = None;
    if let Some(rendered) = render_owned_chain_frames(&mut dvd, size, frames, &payload, &payload) {
        let (transparent, opaque, total) = alpha_counts(&rendered.before);
        eprintln!(
            "dvd:   grid {:?} cell_count {} frames {frames} transparent {transparent}/{total} opaque {opaque} spread {:.2}",
            grid,
            grid.cell_count(),
            luminance_spread(&rendered.before)
        );
        write_png(&format!("{directory}/dvd-room.png"), &rendered.before, size);
        write_png(
            &format!("{directory}/dvd-room-next.png"),
            &rendered.after,
            size,
        );
        if let Some(sim) = dvd.debug_sim() {
            dvd_dye = report_dye(sim, &rendered.device, &rendered.queue, grid);
        }
    }
    if let Some(report) = dvd_dye {
        eprintln!("{report}");
    }
}

/// Render RollingFire and Ball of Fire at 256x256 and write PNGs for
/// inspection (the fire volume is the shape check the unit tests cannot make).
///
/// `MODV_FIRE_GRID="x,y,z"` overrides the simulation grid,
/// `MODV_FIRE_FRAMES` the warm-up frame count, `MODV_FIRE_SIZE` the image size
/// and `MODV_FIRE_DIR` the output directory (default `/app/crates/target`).
///
/// Ignored by default: it is the offline fidelity check, not a CI test.
#[test]
#[ignore = "renders reference frames for manual inspection"]
fn dump_rolling_fire_reference_frames() {
    let size = std::env::var("MODV_FIRE_SIZE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(256);
    let frames = std::env::var("MODV_FIRE_FRAMES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(90);
    let grid = std::env::var("MODV_FIRE_GRID")
        .ok()
        .and_then(|value| {
            let parts: Vec<u32> = value
                .split(',')
                .filter_map(|part| part.trim().parse::<u32>().ok())
                .collect();
            (parts.len() == 3)
                .then(|| crate::point10::volume::VolumeGrid::new(parts[0], parts[1], parts[2]))
        })
        .unwrap_or(rolling_fire::GRID);
    let directory =
        std::env::var("MODV_FIRE_DIR").unwrap_or_else(|_| "/app/crates/target".to_string());
    let payload = if std::env::var("MODV_FIRE_SILENT").is_ok() {
        BandPayload::silent(44_100.0)
    } else if std::env::var("MODV_FIRE_QUIET").is_ok() {
        quiet_payload()
    } else if let Some(band) = std::env::var("MODV_FIRE_BAND")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
    {
        // A uniform band level, for a representative fidelity frame between
        // the silent and the `loud_payload` extremes.
        let mut payload = BandPayload::silent(44_100.0);
        payload.freq_mag = [band; AUDIO_SAMPLES];
        payload.wave = [band; AUDIO_SAMPLES];
        payload.beat = band;
        payload.lanes = [band; 12];
        payload.cells = [band; 16];
        payload.byte_frequency_data = vec![(band.clamp(0.0, 1.0) * 255.0) as u8; 1024];
        payload
    } else {
        loud_payload()
    };

    let mut rolling = rolling_fire::RollingFire::with_grid(grid);
    if let Some(rendered) =
        render_owned_chain_frames(&mut rolling, size, frames, &payload, &payload)
    {
        eprintln!(
            "rolling fire: grid {:?} cell_count {} frames {frames} ball {:?} spread {:.2} dark {:.2} warm {:.2}",
            grid,
            grid.cell_count(),
            rolling.ball(),
            luminance_spread(&rendered.before),
            dark_fraction(&rendered.before),
            warm_ratio_of_brightest(&rendered.before, 0.1)
        );
        write_png(
            &format!("{directory}/rolling-fire.png"),
            &rendered.before,
            size,
        );
        write_png(
            &format!("{directory}/rolling-fire-next.png"),
            &rendered.after,
            size,
        );
        if let Some(report) = rolling
            .debug_sim()
            .and_then(|sim| report_dye(sim, &rendered.device, &rendered.queue, grid))
        {
            eprintln!("{report}");
        }
    }

    let mut ball = rolling_fire::BallOfFire::with_grid(grid);
    if let Some(rendered) = render_owned_chain_frames(&mut ball, size, frames, &payload, &payload) {
        eprintln!(
            "ball of fire: grid {:?} cell_count {} frames {frames} ball {:?} spread {:.2} dark {:.2} warm {:.2}",
            grid,
            grid.cell_count(),
            ball.inner().ball(),
            luminance_spread(&rendered.before),
            dark_fraction(&rendered.before),
            warm_ratio_of_brightest(&rendered.before, 0.1)
        );
        write_png(
            &format!("{directory}/ball-of-fire.png"),
            &rendered.before,
            size,
        );
        write_png(
            &format!("{directory}/ball-of-fire-next.png"),
            &rendered.after,
            size,
        );
        if let Some(report) = ball
            .debug_sim()
            .and_then(|sim| report_dye(sim, &rendered.device, &rendered.queue, grid))
        {
            eprintln!("{report}");
        }
    }
}

/// The dye field's statistics and an ASCII slice through the centre.
#[cfg(test)]
fn report_dye(
    sim: &crate::point10::volume::VolumeSim,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    grid: crate::point10::volume::VolumeGrid,
) -> Option<String> {
    let voxels = sim.read_dye(device, queue);
    let mut max = 0.0_f32;
    let mut sum = 0.0_f64;
    let mut nonzero = 0_usize;
    let mut max_temp = 0.0_f32;
    for voxel in &voxels {
        max = max.max(voxel[0]);
        max_temp = max_temp.max(voxel[1]);
        sum += voxel[0] as f64;
        if voxel[0] > 0.01 {
            nonzero += 1;
        }
    }
    let mut out = format!(
        "dye: mean {:.4} max {:.3} max_temp {:.3} nonzero {}/{}",
        sum / voxels.len() as f64,
        max,
        max_temp,
        nonzero,
        voxels.len()
    );
    let z = grid.z / 2;
    let rows = 18;
    let columns = 36;
    for row in 0..rows {
        let y = (grid.y as usize - 1) * (rows - 1 - row) / (rows - 1).max(1);
        let mut line = String::new();
        for column in 0..columns {
            let x = (grid.x as usize - 1) * column / (columns - 1).max(1);
            let density = voxels[(z as usize * grid.y as usize + y) * grid.x as usize + x][0];
            let level = (density * 4.0).clamp(0.0, 1.0);
            let glyph = if level < 0.05 {
                ' '
            } else if level < 0.25 {
                '.'
            } else if level < 0.5 {
                ':'
            } else if level < 0.75 {
                '*'
            } else {
                '#'
            };
            line.push(glyph);
        }
        out.push_str(&format!("\n  |{line}|"));
    }
    Some(out)
}

#[cfg(test)]
fn write_png(path: &str, pixels: &[u8], size: u32) {
    let image = image::RgbaImage::from_raw(size, size, pixels.to_vec())
        .unwrap_or_else(|| panic!("{path}: pixel count does not match {size}x{size}"));
    image
        .save(path)
        .unwrap_or_else(|error| panic!("{path}: {error}"));
}

// ————— Plane9 Scene Lens RED contract —————
//
// One accumulating test keeps the RED phase to a single test target while still
// reporting every missing Lens behaviour in one run. This contract is strictly
// magnification-only: it makes no assertion about FOV conventions or aspect.

const LENS_PROP_SCENES: &[(&str, &[&str])] = &[
    ("Plane9 Boxy", &["scrollSpeed", "glow", "roughness"]),
    ("Plane9 Mirror Cube", &["spinSpeed", "brightness"]),
    (
        "Plane9 Color Cube",
        &["spinSpeed", "turbulence", "brightness"],
    ),
    ("Plane9 Hollow Cube", &["spinSpeed", "explode", "color"]),
    ("Point10 Hollow Cuboid", &["spinSpeed", "explode", "color"]),
    (
        "Point10 Hollow Cube Layers",
        &["spinSpeed", "explode", "color"],
    ),
    (
        "Point10 Hollow Cube Blur",
        &["spinSpeed", "explode", "color"],
    ),
    ("Plane9 Spiky", &[]),
    (
        "Plane9 Sun",
        &["surfaceSpeed", "brightness", "corona", "rays"],
    ),
    (
        "Plane9 Earth",
        &[
            "spinSpeed",
            "sunSpeed",
            "nightLights",
            "atmosphere",
            "brightness",
            "zoom",
        ],
    ),
    ("Plane9 Blob Fade", &[]),
    ("Plane9 Circle Crossover", &[]),
    ("Plane9 Color Beams", &[]),
    ("Plane9 Wormhole", &["warpSpeed", "bloom"]),
    ("Plane9 It Is Coming For You", &["speed", "glow", "bloom"]),
    ("Plane9 The Cave", &["scrollSpeed", "glow", "bloom"]),
    ("Plane9 Though The Noise", &[]),
    ("Plane9 Truchet Tentacles", &[]),
    ("Plane9 Cloud Flight", &[]),
    (
        "Plane9 Sound Cube",
        &["spinSpeed", "pulse", "trail", "brightness"],
    ),
    ("Plane9 Color Blob", &[]),
    ("Plane9 Mirror Tunnel", &[]),
];

#[derive(Clone, Copy, Debug)]
enum LensRayMechanism {
    InlineFovScale,
    Raymarch,
}

#[derive(Clone, Copy, Debug)]
struct LensRaySourceContract {
    file: &'static str,
    scene: &'static str,
    mechanism: LensRayMechanism,
}

const LENS_RAY_SOURCE_CONTRACTS: &[LensRaySourceContract] = &[
    LensRaySourceContract {
        file: "wormhole",
        scene: "Plane9 Wormhole",
        mechanism: LensRayMechanism::InlineFovScale,
    },
    LensRaySourceContract {
        file: "it_is_coming_for_you",
        scene: "Plane9 It Is Coming For You",
        mechanism: LensRayMechanism::InlineFovScale,
    },
    LensRaySourceContract {
        file: "the_cave",
        scene: "Plane9 The Cave",
        mechanism: LensRayMechanism::Raymarch,
    },
    LensRaySourceContract {
        file: "though_the_noise",
        scene: "Plane9 Though The Noise",
        mechanism: LensRayMechanism::Raymarch,
    },
    LensRaySourceContract {
        file: "truchet_tentacles",
        scene: "Plane9 Truchet Tentacles",
        mechanism: LensRayMechanism::Raymarch,
    },
    LensRaySourceContract {
        file: "cloud_flight",
        scene: "Plane9 Cloud Flight",
        mechanism: LensRayMechanism::Raymarch,
    },
];

fn plane9_scene_source(file: &str) -> &'static str {
    match file {
        "wormhole" => include_str!("scenes/wormhole.rs"),
        "it_is_coming_for_you" => include_str!("scenes/it_is_coming_for_you.rs"),
        "the_cave" => include_str!("scenes/the_cave.rs"),
        "though_the_noise" => include_str!("scenes/though_the_noise.rs"),
        "truchet_tentacles" => include_str!("scenes/truchet_tentacles.rs"),
        "cloud_flight" => include_str!("scenes/cloud_flight.rs"),
        _ => panic!("no Lens source contract registered for {file}"),
    }
}

fn production_scene_source(file: &str) -> &'static str {
    let source = plane9_scene_source(file);
    let start = source
        .find("#![forbid(unsafe_code)]")
        .map(|index| index + "#![forbid(unsafe_code)]".len())
        .unwrap_or(0);
    let production = &source[start..];
    let end = production.rfind("#[cfg(test)]").unwrap_or(production.len());
    &production[..end]
}

fn check_lens_prop_surface(failures: &mut Vec<String>) {
    let scenes = all_scenes();
    let lever_count = crate::point10::audio_bridge::RESPONSIVENESS_PROPS.len();
    let expected_lens = crate::point10::SceneProp::float("lens", "Lens", 1.0, 0.25, 4.0, 0.01);

    for &(name, own_props) in LENS_PROP_SCENES {
        let Some(scene) = scenes.iter().find(|scene| scene.name() == name) else {
            failures.push(format!("lens props: registry scene {name} is missing"));
            continue;
        };
        let props = scene.props();
        let levers_ok =
            props.get(..lever_count) == Some(crate::point10::audio_bridge::RESPONSIVENESS_PROPS);
        let lens_ok = props.get(lever_count) == Some(&expected_lens);
        let own_ok = props.get(lever_count + 1..).is_some_and(|actual| {
            actual
                .iter()
                .map(|prop| prop.name)
                .eq(own_props.iter().copied())
        });
        if !(levers_ok && lens_ok && own_ok) {
            let actual: Vec<&str> = props.iter().map(|prop| prop.name).collect();
            failures.push(format!(
                "lens props: {name} must be responsiveness levers, Lens(1.0, 0.25..4.0, 0.01), then {own_props:?}; got {actual:?}"
            ));
        }
    }
}

fn check_lens_ray_source_wiring(failures: &mut Vec<String>) {
    for contract in LENS_RAY_SOURCE_CONTRACTS {
        let source = production_scene_source(contract.file);
        let mut gaps = Vec::new();
        let declares_lens = source.contains("\"lens\"");
        let reads_lens = source.lines().any(|line| {
            line.contains("lens")
                && !line.contains("SceneProp::float")
                && !line.contains("\"lens\" =>")
        });
        if !declares_lens || !reads_lens {
            gaps.push(
                "production camera/ray source must declare and read Lens beyond its prop key"
                    .to_string(),
            );
        }
        if source.contains("pub fn apply_props(&mut self, _props") {
            gaps.push("prop binder is still a no-op".to_string());
        }
        match contract.mechanism {
            LensRayMechanism::InlineFovScale => {
                if !source.contains("fov_scale") {
                    gaps.push("inline camera lost its fov_scale path".to_string());
                }
            }
            LensRayMechanism::Raymarch => {
                if !source.contains("@fragment") {
                    gaps.push("raymarch fragment is missing".to_string());
                }
            }
        }
        if !gaps.is_empty() {
            failures.push(format!(
                "lens ray wiring: {} ({:?}): {}",
                contract.scene,
                contract.mechanism,
                gaps.join("; ")
            ));
        }
    }
}

fn init_plane9_test_scene(
    scene: &mut dyn Point10Scene,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
) {
    let mut ctx = SceneContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm);
    ctx.resize(width, height);
    scene.init(&mut ctx);
    scene.resize(width, height);
}

fn fresh_plane9_lens_scene(name: &str, lens: Option<f32>) -> Box<dyn Point10Scene> {
    use crate::point10::scenes::{mirror_cube, sound_cube, the_cave};

    match name {
        "Plane9 Mirror Cube" => {
            let mut scene = mirror_cube::MirrorCube::new();
            // The default rotated silhouette is ~69% high, so Lens 2 would
            // clip at 16:9. Freeze the existing spin prop at zero for every
            // Mirror render (including the Lens-unprop'd control), keeping the
            // 2x silhouette measurable without changing the camera or audio.
            scene.apply_props(|key| match key {
                "spinSpeed" => Some(0.0),
                "lens" => lens,
                _ => None,
            });
            Box::new(scene)
        }
        "Plane9 Sound Cube" => {
            let mut scene = sound_cube::SoundCube::new();
            if let Some(lens) = lens {
                scene.apply_props(|key| (key == "lens").then_some(lens));
            }
            Box::new(scene)
        }
        "Plane9 The Cave" => {
            let mut scene = the_cave::TheCave::new();
            if let Some(lens) = lens {
                scene.apply_props(|key| (key == "lens").then_some(lens));
            }
            Box::new(scene)
        }
        _ => panic!("no Lens render fixture registered for {name}"),
    }
}

fn render_frozen_plane9_scene(
    name: &str,
    width: u32,
    height: u32,
    lens: Option<f32>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Vec<u8> {
    let mut scene = fresh_plane9_lens_scene(name, lens);
    init_plane9_test_scene(&mut *scene, device, queue, width, height);
    scene.update(0.0, &AudioBridge::new());

    let target = TestTarget::new(device, width, height);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("point10-plane9-lens-red-contract"),
    });
    scene.render(&mut encoder, &target.view);
    queue.submit(Some(encoder.finish()));
    target.read(device, queue)
}

fn render_frozen_mirror_with_spin(
    lens: Option<f32>,
    spin_speed: Option<f32>,
    width: u32,
    height: u32,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Vec<u8> {
    let mut scene = crate::point10::scenes::mirror_cube::MirrorCube::new();
    scene.apply_props(|key| match key {
        "lens" => lens,
        "spinSpeed" => spin_speed,
        _ => None,
    });
    init_plane9_test_scene(&mut scene, device, queue, width, height);
    scene.update(0.0, &AudioBridge::new());

    let target = TestTarget::new(device, width, height);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("point10-plane9-mirror-lens-identity"),
    });
    scene.render(&mut encoder, &target.view);
    queue.submit(Some(encoder.finish()));
    target.read(device, queue)
}

#[derive(Clone, Copy, Debug)]
struct VisibleBbox {
    height_fraction: f32,
    visible_pixels: usize,
    min_y: usize,
    max_y: usize,
}

fn bbox_from_predicate(
    pixels: &[u8],
    width: u32,
    height: u32,
    mut visible: impl FnMut(&[u8]) -> bool,
) -> VisibleBbox {
    let width = width as usize;
    let height = height as usize;
    let mut min_y = usize::MAX;
    let mut max_y = 0_usize;
    let mut visible_pixels = 0_usize;
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if visible(pixel) {
            let y = index / width;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            visible_pixels += 1;
        }
    }
    if visible_pixels == 0 {
        VisibleBbox {
            height_fraction: 0.0,
            visible_pixels,
            min_y: 0,
            max_y: 0,
        }
    } else {
        VisibleBbox {
            height_fraction: (max_y - min_y + 1) as f32 / height as f32,
            visible_pixels,
            min_y,
            max_y,
        }
    }
}

fn visible_bbox(pixels: &[u8], width: u32, height: u32) -> VisibleBbox {
    bbox_from_predicate(pixels, width, height, |pixel| pixel[3] > 0)
}

fn frame_max_channel(pixels: &[u8]) -> u8 {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[0].max(pixel[1]).max(pixel[2]))
        .max()
        .unwrap_or(0)
}

fn bright_bbox(pixels: &[u8], width: u32, height: u32, threshold: u8) -> VisibleBbox {
    bbox_from_predicate(pixels, width, height, |pixel| {
        threshold > 0 && pixel[0].max(pixel[1]).max(pixel[2]) >= threshold
    })
}

fn bright_threshold(pixels: &[u8]) -> u8 {
    let max_channel = frame_max_channel(pixels);
    if max_channel < 16 {
        0
    } else {
        ((f32::from(max_channel) * 0.4).ceil() as u8).max(16)
    }
}

fn check_ratio(
    failures: &mut Vec<String>,
    label: &str,
    numerator: f32,
    denominator: f32,
    expected: f32,
) {
    let ratio = if denominator > 0.0 {
        numerator / denominator
    } else {
        f32::NAN
    };
    eprintln!(
        "ratio: {label}: {numerator:.6}/{denominator:.6} = {ratio:.6}, expected {expected:.6}"
    );
    if !(numerator > 0.0 && denominator > 0.0 && ratio.is_finite()) {
        failures.push(format!("ratio: {label}: empty/non-finite bbox measurement"));
    } else if (ratio - expected).abs() > expected * 0.05 {
        failures.push(format!(
            "ratio: {label}: got {ratio:.6}, expected {expected:.6} within 5%"
        ));
    }
}

fn check_rendered_lens(failures: &mut Vec<String>, device: &wgpu::Device, queue: &wgpu::Queue) {
    const WIDTH: u32 = 1280;
    const HEIGHT: u32 = 720;

    // Lens 1 must preserve the actual default/current Mirror framing too, not
    // just the frozen-spin magnification fixture below.
    let default_unprop = render_frozen_mirror_with_spin(None, None, WIDTH, HEIGHT, device, queue);
    let default_one = render_frozen_mirror_with_spin(Some(1.0), None, WIDTH, HEIGHT, device, queue);
    if default_one != default_unprop {
        failures.push(
            "lens identity: default Mirror Cube lens=1.0 changes the current framing".to_string(),
        );
    }
    eprintln!(
        "default Mirror Lens identity: unprop={:?} lens1={:?}",
        visible_bbox(&default_unprop, WIDTH, HEIGHT),
        visible_bbox(&default_one, WIDTH, HEIGHT)
    );

    for name in ["Plane9 Mirror Cube", "Plane9 Sound Cube"] {
        let unprop = render_frozen_plane9_scene(name, WIDTH, HEIGHT, None, device, queue);
        let one = render_frozen_plane9_scene(name, WIDTH, HEIGHT, Some(1.0), device, queue);
        let two = render_frozen_plane9_scene(name, WIDTH, HEIGHT, Some(2.0), device, queue);
        let half = render_frozen_plane9_scene(name, WIDTH, HEIGHT, Some(0.5), device, queue);

        let unprop_bbox = visible_bbox(&unprop, WIDTH, HEIGHT);
        let one_bbox = visible_bbox(&one, WIDTH, HEIGHT);
        let two_bbox = visible_bbox(&two, WIDTH, HEIGHT);
        let half_bbox = visible_bbox(&half, WIDTH, HEIGHT);
        eprintln!(
            "lens framing: {name} {WIDTH}x{HEIGHT}: unprop={:.6} {:?}; lens1={:.6} {:?}; lens2={:.6} {:?}; lens0.5={:.6} {:?}",
            unprop_bbox.height_fraction,
            unprop_bbox,
            one_bbox.height_fraction,
            one_bbox,
            two_bbox.height_fraction,
            two_bbox,
            half_bbox.height_fraction,
            half_bbox
        );
        eprintln!(
            "lens bbox rows: {name}: unprop={}..{} lens1={}..{} lens2={}..{} lens0.5={}..{}",
            unprop_bbox.min_y,
            unprop_bbox.max_y,
            one_bbox.min_y,
            one_bbox.max_y,
            two_bbox.min_y,
            two_bbox.max_y,
            half_bbox.min_y,
            half_bbox.max_y
        );

        if unprop_bbox.visible_pixels == 0 {
            failures.push(format!(
                "lens behaviour: {name} Lens-unprop'd frame is empty"
            ));
        }
        if one != unprop {
            failures.push(format!(
                "lens behaviour: {name} lens=1.0 is not bit-identical to the Lens-unprop'd scene"
            ));
        }
        check_ratio(
            failures,
            &format!("{name} lens=2.0 magnification"),
            two_bbox.height_fraction,
            one_bbox.height_fraction,
            2.0,
        );
        check_ratio(
            failures,
            &format!("{name} lens=0.5 magnification"),
            half_bbox.height_fraction,
            one_bbox.height_fraction,
            0.5,
        );
    }

    // Clamping is checked on the same frozen Mirror fixture.
    let at_min = render_frozen_plane9_scene(
        "Plane9 Mirror Cube",
        WIDTH,
        HEIGHT,
        Some(0.25),
        device,
        queue,
    );
    let zero = render_frozen_plane9_scene(
        "Plane9 Mirror Cube",
        WIDTH,
        HEIGHT,
        Some(0.0),
        device,
        queue,
    );
    let at_max = render_frozen_plane9_scene(
        "Plane9 Mirror Cube",
        WIDTH,
        HEIGHT,
        Some(4.0),
        device,
        queue,
    );
    let ninety_nine = render_frozen_plane9_scene(
        "Plane9 Mirror Cube",
        WIDTH,
        HEIGHT,
        Some(99.0),
        device,
        queue,
    );
    let one = render_frozen_plane9_scene(
        "Plane9 Mirror Cube",
        WIDTH,
        HEIGHT,
        Some(1.0),
        device,
        queue,
    );
    let nan = render_frozen_plane9_scene(
        "Plane9 Mirror Cube",
        WIDTH,
        HEIGHT,
        Some(f32::NAN),
        device,
        queue,
    );
    let min_bbox = visible_bbox(&at_min, WIDTH, HEIGHT);
    let max_bbox = visible_bbox(&at_max, WIDTH, HEIGHT);
    eprintln!(
        "lens clamping: Mirror Cube lens0.25={:.6} {:?}; lens4={:.6} {:?}",
        min_bbox.height_fraction, min_bbox, max_bbox.height_fraction, max_bbox
    );
    if zero != at_min {
        failures.push("lens clamping: 0.0 does not behave as 0.25".to_string());
    }
    if ninety_nine != at_max {
        failures.push("lens clamping: 99.0 does not behave as 4.0".to_string());
    }
    if nan != one {
        failures.push("lens clamping: NaN does not fall back to 1.0".to_string());
    }
    if min_bbox.visible_pixels == 0 || max_bbox.visible_pixels == 0 {
        failures.push("lens clamping: comparison is vacuous (empty clamped frame)".to_string());
    }
}

fn check_area_ratio(
    failures: &mut Vec<String>,
    label: &str,
    numerator: usize,
    denominator: usize,
    expected: f64,
) {
    let ratio = if denominator > 0 {
        numerator as f64 / denominator as f64
    } else {
        f64::NAN
    };
    // The Cave applies Lens as a centered composite-image zoom, so lit area
    // scales as lens². A 5% relative window accommodates threshold-edge and
    // bilinear-sampling differences; the 1280x720 counts are hundreds of
    // pixels, so this is not an integer-row quantisation allowance.
    const RELATIVE_TOLERANCE: f64 = 0.05;
    eprintln!(
        "cave lit-area ratio: {label}: {numerator}/{denominator} = {ratio:.6}, expected {expected:.6} +/- {:.0}%",
        RELATIVE_TOLERANCE * 100.0
    );
    if !(numerator > 0 && denominator > 0 && ratio.is_finite()) {
        failures.push(format!(
            "cave lit-area ratio: {label}: empty/non-finite measurement"
        ));
    } else if (ratio - expected).abs() > expected * RELATIVE_TOLERANCE {
        failures.push(format!(
            "cave lit-area ratio: {label}: got {ratio:.6}, expected {expected:.6} within 5%"
        ));
    }
}

fn check_cave_lens(failures: &mut Vec<String>, device: &wgpu::Device, queue: &wgpu::Queue) {
    const WIDTH: u32 = 1280;
    const HEIGHT: u32 = 720;
    let unprop = render_frozen_plane9_scene("Plane9 The Cave", WIDTH, HEIGHT, None, device, queue);
    let one =
        render_frozen_plane9_scene("Plane9 The Cave", WIDTH, HEIGHT, Some(1.0), device, queue);
    let two =
        render_frozen_plane9_scene("Plane9 The Cave", WIDTH, HEIGHT, Some(2.0), device, queue);
    let half =
        render_frozen_plane9_scene("Plane9 The Cave", WIDTH, HEIGHT, Some(0.5), device, queue);

    if one != unprop {
        failures.push(
            "cave lens: lens=1.0 is not bit-identical to the Lens-unprop'd render".to_string(),
        );
    }
    if one == two || one == half {
        failures.push(
            "cave lens: rendered frame is unchanged, so Lens does not reach the composite zoom"
                .to_string(),
        );
    }

    // Use one threshold for all four frames so the measurement isolates image
    // magnification rather than per-frame max/tone-map changes.
    let max_channel = frame_max_channel(&one);
    let threshold = bright_threshold(&one);
    let one_bbox = bright_bbox(&one, WIDTH, HEIGHT, threshold);
    let two_bbox = bright_bbox(&two, WIDTH, HEIGHT, threshold);
    let half_bbox = bright_bbox(&half, WIDTH, HEIGHT, threshold);
    eprintln!(
        "cave lit-area framing: {WIDTH}x{HEIGHT}: max={max_channel} shared_threshold={threshold}; lens1={} px {:?}; lens2={} px {:?}; lens0.5={} px {:?}",
        one_bbox.visible_pixels,
        one_bbox,
        two_bbox.visible_pixels,
        two_bbox,
        half_bbox.visible_pixels,
        half_bbox
    );
    eprintln!(
        "cave bbox rows (diagnostic only): lens1={}..{} lens2={}..{} lens0.5={}..{}",
        one_bbox.min_y,
        one_bbox.max_y,
        two_bbox.min_y,
        two_bbox.max_y,
        half_bbox.min_y,
        half_bbox.max_y
    );

    check_area_ratio(
        failures,
        "Plane9 The Cave lens=2.0 area magnification",
        two_bbox.visible_pixels,
        one_bbox.visible_pixels,
        4.0,
    );
    check_area_ratio(
        failures,
        "Plane9 The Cave lens=0.5 area magnification",
        half_bbox.visible_pixels,
        one_bbox.visible_pixels,
        0.25,
    );
}

#[test]
fn plane9_scene_lens_contract() {
    let mut failures = Vec::new();
    check_lens_prop_surface(&mut failures);
    check_lens_ray_source_wiring(&mut failures);

    if let Some((device, queue)) = gpu() {
        check_rendered_lens(&mut failures, &device, &queue);
        check_cave_lens(&mut failures, &device, &queue);
    } else {
        failures.push(
            "GPU fallback unavailable: rendered Lens and The Cave checks could not run".to_string(),
        );
    }

    if failures.is_empty() {
        eprintln!("Plane9 Scene Lens contract passed");
    } else {
        panic!(
            "Plane9 Scene Lens contract failed ({} checks):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
