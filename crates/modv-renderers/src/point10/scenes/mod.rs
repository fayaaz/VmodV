//! Point10 scenes that live in their own modules.
//!
//! | Module | Scene | JS source |
//! |--------|-------|-----------|
//! | [`chromatic_wave`] | Chromatic Wave | `scenes/ChromaticWave.js` |
//! | [`particle_field`] | Particle Field | `scenes/ParticleField.js` |
//! | [`spiral_particles`] | Spiral Particles | `scenes/SpiralParticles.js` |
//! | [`waveform`] | Waveform | `scenes/Waveform.js` |
//! | [`square_tunnel`] | Square Tunnel | `scenes/SquareTunnel.js` |
//! | [`light_tunnel`] | Light Tunnel | `scenes/LightTunnel.js` |
//! | [`wormhole`] | Wormhole | `scenes/Wormhole.js` |
//! | [`it_is_coming_for_you`] | It Is Coming For You | `scenes/ItIsComingForYou.js` |
//! | [`water_ripple`] | Water Ripple | `scenes/WaterRipple.js` |
//! | [`mycelium`] | Mycelium | `scenes/Mycelium.js` |
//! | [`around_plus`] | Around Plus | `scenes/AroundPlus.js` |
//! | [`hollow_cube`] | Hollow Cube | `scenes/HollowCube.js` |
//! | [`attractor_particles`] | Attractor Particles | `scenes/AttractorParticles.js` |
//! | [`dvd_room`] | DVD Room | `scenes/DvdRoom.js` |
//! | [`ultra_hdr`] | Ultra HDR | `scenes/UltraHDR.js` |
//! | [`mirror_cube`] | Mirror Cube | `scenes/MirrorCube.js` |
//! | [`color_cube`] | Color Cube | `scenes/ColorCube.js` |
//! | [`boxy`] | Boxy | `scenes/Boxy.js` |
//! | [`sound_cube`] | Sound Cube | `scenes/SoundCube.js` |
//! | [`rolling_fire`] | Rolling Fire / Ball Of Fire | `scenes/RollingFire.js` |
//! | [`smoke_room`] | Smoke Room | `scenes/SmokeRoom.js` |
//! | [`latent_backrooms`] | Latent Backrooms | `scenes/LatentBackrooms.js` |
//! | [`the_cave`] | The Cave | `../Point10/public/plane9/xml/Nature/The Cave.xml` |
//! | [`spiky`] | Spiky | `Things/Spiky.xml` (mesh-modifier chain) |
//! | [`evil_box`] | Evil Box | `Cube/Evil Box.xml` (feedback RTT chain) |
//! | [`music_waves`] | Music Waves | `Music/Music Waves.xml` (instanced wave RTT + bloom) |
//! | [`hypno_toad`] | Hypno Toad | `Psychedelic/HypnoToad.xml` (single-pass instanced spirograph) |
//! | [`cell_culture`] | Cell Culture | `Cell/Cell Culture.xml` (reaction-diffusion feedback) |
//! | [`waves_of_sin`] | Waves of Sin | `Line/Waves of sin.xml` (50 000 instanced point sprites) |
//!
//! ChromaticLine lives with its shared simulation in
//! [`crate::point10::chromatic`] ([`crate::point10::chromatic::line`]) and is
//! registered from there by [`crate::point10::all_scenes`].
//!
//! [`dvd_logo`] is not a scene either: it builds the DVD wordmark mesh from the
//! inlined SVG path for [`dvd_room`], which — like [`smoke_room`] and
//! [`rolling_fire`] — renders through [`crate::point10::volume`] rather than an
//! approximation. [`rolling_fire`] also holds `BallOfFire`, the frozen variant.
//!
//! [`particle_quad`] is not a scene: it is the instanced billboard pipeline
//! ParticleField and SpiralParticles share. [`cube_mesh`] is the batch-4
//! counterpart — `BoxGeometry`, the matrix helpers and the depth attachment the
//! five cube/lighting scenes share. [`mesh_ops`] is not a scene either: it is
//! the CPU mesh-modifier chain (subdivide/select/extrude/randomize) that Spiky
//! is the first preset to need.

#[cfg(test)]
mod evil_box_red_contract;
#[cfg(test)]
mod plane9_cell_culture_fidelity_red_contract;
#[cfg(test)]
mod plane9_hypno_toad_fidelity_red_contract;
#[cfg(test)]
mod plane9_music_waves_fidelity_red_contract;
#[cfg(test)]
mod plane9_waves_of_sin_fidelity_red_contract;

pub mod abstract_clouds;
pub mod around_plus;
pub mod attractor_particles;
pub mod blob_fade;
pub mod boxy;
pub mod cell_culture;
pub mod chromatic_wave;
pub mod circle_crossover;
pub mod cloud_flight;
pub mod color_beams;
pub mod color_blob;
pub mod color_cube;
pub mod colors;
pub mod cube_mesh;
pub mod dvd_logo;
pub mod dvd_room;
pub mod earth;
pub mod evil_box;
pub mod fire_common;
pub mod gradient;
pub mod hollow_cube;
pub mod hypno_toad;
pub mod it_is_coming_for_you;
pub mod latent_backrooms;
pub mod light_tunnel;
pub mod magic_smoke_box;
pub mod mesh_ops;
pub mod mirror_cube;
pub mod mirror_tunnel;
pub mod music_waves;
pub mod mycelium;
pub mod noise_warp;
pub mod particle_field;
pub mod particle_quad;
pub mod plane9_common;
pub mod rolling_fire;
pub mod rorschach;
pub mod smoke_room;
pub mod sound_cube;
pub mod spiky;
pub mod spiral_particles;
pub mod square_tunnel;
pub mod stargate_event_horizon;
pub mod sun;
pub mod the_cave;
pub mod though_the_noise;
pub mod truchet_tentacles;
pub mod tv_noise;
pub mod ultra_hdr;
pub mod water_ripple;
pub mod waveform;
pub mod waves_of_sin;
pub mod wormhole;
pub mod xor;
