//! Shared helpers for the Point10 scene port (`point10/shared/`).
//!
//! Ported in dependency order (batch 1/2 first):
//!
//! | JS source | Rust module | Used by |
//! |-----------|-------------|---------|
//! | `shared/particleAlpha.js`, `shared/particleSprite.js` | [`particle_alpha`] | ParticleField, SpiralParticles |
//! | `shared/random.js` | [`random`] | ParticleField, SpiralParticles (deterministic stand-ins for `Math.random`), every `PERMRAND` in batch 2, `pickRoygbivHue` in batch 3 |
//! | `plane9/P9Glsl.js` (the helpers batch 2 calls) | [`p9_glsl`] | LightTunnel, Wormhole, ItIsComingForYou, AroundPlus |
//! | `plane9/P9Glsl.js` (the lighting/colour half) | [`p9_shading`] | Boxy, ColorCube |
//! | `shared/phraseColor.js` | [`phrase_color`] | WaterRipple, Mycelium, AroundPlus |
//! | `shared/waterWave.js` | [`water_wave`] | WaterRipple, AroundPlus |
//! | `plane9/P9Nodes.js` `SoundTexture` | [`sound_texture`] | MusicWaves (and the future spectrum presets) |
//! | `plane9/P9Nodes.js` `Ribbons` | [`ribbons`] | Gravity Spectrum |
//! | `shared/textures.js` (the loader half) | [`assets`] | AroundPlus, Boxy, UltraHDR |
//!
//! `plane9/P9Audio.js` is deliberately **not** here: it is subsumed by
//! [`crate::point10::audio_bridge::P10Audio`] (see the module docs there).
//! `P9Glsl.js`'s GLSL preprocessor is replaced with [`p9_glsl`]'s WGSL subset
//! plus per-scene WGSL.
//!
//! `shared/assets.js` is not ported: no vendored scene imports it (UltraHDR
//! uses its own `UltraHDRLoader`).
//!
//! [`assets`] resolves a texture from its ordered directories first; when no
//! directory holds it, the wasm build and unit tests fall back to the
//! compiled-in `bundled` copy of `resources/point10/textures`.

pub mod assets;
// The compiled-in texture bundle backs the wasm build (no filesystem) and the
// unit tests. Desktop release binaries get it from disk instead, so the bundle
// is not linked into them.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) mod bundled;
pub mod p9_glsl;
pub mod p9_shading;
pub mod particle_alpha;
pub mod phrase_color;
pub mod random;
pub mod ribbons;
pub mod sound_texture;
pub mod water_wave;
