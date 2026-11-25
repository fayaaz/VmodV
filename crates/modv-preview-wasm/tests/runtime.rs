//! The browser preview runtime must mirror the engine's uniform contracts.
//!
//! These tests run on the host (no wasm-bindgen glue involved) so the byte
//! layout is checked against the same offsets the native renderer uses.

use modv_preview_wasm::runtime::PreviewRuntime;

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[test]
fn builtin_uniform_block_is_160_bytes_with_engine_offsets() {
    let mut runtime = PreviewRuntime::new(640, 360, 120.0);
    runtime.set_time(2.0);
    runtime.tick(0.0);

    let bytes = runtime.builtin_uniform_bytes();
    assert_eq!(bytes.len(), 160);
    assert_eq!(f32_at(&bytes, 0), 2.0, "iGlobalTime at offset 0");
    assert_eq!(u32_at(&bytes, 4), 1, "iFrame at offset 4 (one tick above)");
    assert_eq!(f32_at(&bytes, 8), 2.0, "iTime at offset 8 (JS drift)");
    assert_eq!(f32_at(&bytes, 12), 2.0, "iTimeDelta is not a delta");
    assert_eq!(f32_at(&bytes, 32), 640.0, "iResolution.x at offset 32");
    assert_eq!(f32_at(&bytes, 36), 360.0, "iResolution.y at offset 36");
    assert_eq!(f32_at(&bytes, 132), 2000.0, "u_time is milliseconds");
    assert_eq!(
        f32_at(&bytes, 136),
        512.0,
        "u_fftResolution is the FFT width"
    );
}

#[test]
fn simulated_audio_fires_a_kick_on_each_beat() {
    let mut runtime = PreviewRuntime::new(320, 180, 120.0);
    runtime.set_time(0.0);
    runtime.tick(0.0);
    assert!(runtime.kick(), "a beat starts at t=0");
    let kick_bass = runtime.bass();
    assert!(kick_bass > 0.5, "the kick should drive bass: {kick_bass}");

    runtime.set_time(0.25);
    runtime.tick(0.0);
    assert!(!runtime.kick(), "halfway between two beats is not a kick");
    assert!(runtime.bass() < kick_bass, "bass decays after the kick");

    runtime.set_time(0.5);
    runtime.tick(0.0);
    assert!(runtime.kick(), "the next beat is at t=0.5 for 120 BPM");
}

#[test]
fn isf_uniform_bytes_follow_the_layout_offsets() {
    let layout = r#"{
        "size": 32,
        "members": [
            {"name": "TIME", "kind": "float", "source": "time", "offset": 0},
            {"name": "RENDERSIZE", "kind": "vec2", "source": "renderSize", "offset": 8},
            {"name": "level", "kind": "float", "source": "input:level", "offset": 16}
        ]
    }"#;
    let props = r#"[{"name": "level", "default": 0.75}]"#;

    let mut runtime = PreviewRuntime::new(640, 360, 120.0);
    runtime.set_time(2.5);
    runtime.tick(0.0);

    let bytes = runtime.isf_uniform_bytes(layout, props).unwrap();
    assert_eq!(bytes.len(), 32);
    assert_eq!(f32_at(&bytes, 0), 2.5);
    assert_eq!(f32_at(&bytes, 8), 640.0);
    assert_eq!(f32_at(&bytes, 12), 360.0);
    assert_eq!(f32_at(&bytes, 16), 0.75);
}

#[test]
fn prop_bytes_use_one_16_byte_slot_per_prop() {
    let props = r#"[{"name": "scale", "default": 1.5}, {"name": "colour", "default": [0.1, 0.2, 0.3, 1.0]}]"#;
    let runtime = PreviewRuntime::new(640, 360, 120.0);
    let bytes = runtime.prop_bytes(props).unwrap();
    assert_eq!(bytes.len(), 32);
    assert_eq!(f32_at(&bytes, 0), 1.5);
    assert_eq!(f32_at(&bytes, 16), 0.1);
    assert_eq!(f32_at(&bytes, 28), 1.0);
}

#[test]
fn bad_layout_json_is_an_error_not_a_panic() {
    let runtime = PreviewRuntime::new(640, 360, 120.0);
    assert!(runtime.isf_uniform_bytes("{not json", "[]").is_err());
}
