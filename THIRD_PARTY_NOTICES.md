# Third-party notices

VmodV is released under the MIT License — see [LICENSE](LICENSE). It bundles
modules, assets and code derived from the third-party sources listed below.
Unless a file says otherwise, these works are **not** covered by the MIT
license; they remain under the license shown for them.

Where a Shadertoy shader does not state a license, the
[Shadertoy Terms of Service](https://www.shadertoy.com/terms) apply: unlicensed
Shadertoy shaders default to the
[Creative Commons Attribution-NonCommercial-ShareAlike 3.0 Unported License](https://creativecommons.org/licenses/by-nc-sa/3.0/)
(CC BY-NC-SA 3.0).

Files known to carry an NC (non-commercial) license — notably everything whose
license defaults to CC BY-NC-SA 3.0 — may not be used commercially, and
modified versions must remain under the same license ("share-alike").

## Shadertoy-derived modules

| File(s) | Source | Author | License |
| --- | --- | --- | --- |
| `src/renderer/src/application/renderers/shader/modules/MattiasCRT/mattiasCrt.frag`, `crates/modv-renderers/src/shader/modules/mattias_crt.rs` | [Ms23DR](https://www.shadertoy.com/view/Ms23DR) | Iñigo Quílez (postprocessing shader) | CC BY-NC-SA 3.0 (stated in file) |
| `src/renderer/src/application/renderers/shader/modules/ChromaticAbberation/chromaticAbberation.frag`, `crates/modv-renderers/src/shader/modules/chromatic_abberation.rs` | [MtXXDr](https://www.shadertoy.com/view/MtXXDr) | author not stated | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/ASCII Art.fs` | [lssGDj](https://www.shadertoy.com/view/lssGDj) | IMIMOT | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/st_Ms2SD1.fs.fs` | [Ms2SD1](https://www.shadertoy.com/view/Ms2SD1) ("Seascape") | Alexander Alekseev aka TDM | CC BY-NC-SA 3.0 (stated explicitly in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/Smoke Screen.fs` | [4t2SRz](https://www.shadertoy.com/view/4t2SRz) | jackdavenport | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Zebre.fs` | [lsf3WH](https://www.shadertoy.com/view/lsf3WH) (noise functions); module by Joseph Fiola | Joseph Fiola; noise by Iñigo Quílez | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/WaveLines.fs` | [4dsGzH](https://www.shadertoy.com/view/4dsGzH) ("WAVES"); module by Joseph Fiola, remix by SilviaFabiani | Joseph Fiola / bonniem / SilviaFabiani | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/digital-crystal-tunnel.fs` | triangle noise from [4ts3z2](https://www.shadertoy.com/view/4ts3z2) (original usage noted as Xyptonjtroz; function by Nimitz); module credits WilstonOreo | WilstonOreo (module); Nimitz (noise function) | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Broken LCD.fs` | noise functions [XdXGW8](https://www.shadertoy.com/view/XdXGW8), [lsf3WH](https://www.shadertoy.com/view/lsf3WH), [4dXBRH](https://www.shadertoy.com/view/4dXBRH), [XdXBRH](https://www.shadertoy.com/view/XdXBRH), [XsXfRH](https://www.shadertoy.com/view/XsXfRH), [4dffRH](https://www.shadertoy.com/view/4dffRH), [Msf3WH](https://www.shadertoy.com/view/Msf3WH), [Xsl3Dl](https://www.shadertoy.com/view/Xsl3Dl), [4sfGzS](https://www.shadertoy.com/view/4sfGzS) (value/gradient noise, 2D/3D) | VIDVOX (module); noise by Iñigo Quílez, ported by @colin_movecraft | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Dirty Lens.fs`, `src/renderer/src/application/renderers/isf/modules/isf/Smudged Lens.fs` | same Iñigo Quílez noise functions as Broken LCD | Iñigo Quílez, ported by @colin_movecraft and VIDVOX | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/spherical-shader-tut.fs` | [XdKGWm](https://www.shadertoy.com/view/XdKGWm) | WilstonOreo (http://omnido.me) | in-file license: "Creative Commons 3.0 Share Alike" (variant not specified); because the Shadertoy source defaults to CC BY-NC-SA 3.0 and a derivative cannot be less restrictive, this port is treated as **CC BY-NC-SA 3.0** |
| `src/renderer/src/application/renderers/isf/modules/isf/Perlin Transition.fs` | [gl-transitions perlin.glsl](https://github.com/gl-transitions/gl-transitions/tree/master/perlin.glsl); the noise function credits [4dS3Wd](https://www.shadertoy.com/view/4dS3Wd) | Rich Harris (module); noise by Morgan McGuire | module stated MIT in file; the **4dS3Wd noise dependency states no license**, so it defaults to CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Helix Eye.fs` | [cdSGRw](https://www.shadertoy.com/view/cdSGRw) ("Fear Inoculum"); `helix_sdf` adapted from [ttB3DV](https://www.shadertoy.com/view/ttB3DV); raymarch/fog helpers adapted from Iñigo Quílez (iq); camera rotation matrix from neilmendoza.com | RagePly (module); ttB3DV author not stated; Inigo Quilez (iq) raymarch/fog helpers; neilmendoza.com matrix (no license stated) | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Traced Tunnel.fs` | [tdjfDR](https://www.shadertoy.com/view/tdjfDR); camera/tick code retained verbatim from NuSan; the source's two photo inputs are not redistributed (licenses unknown, replaced by procedural patterns) | Shane (module); NuSan (camera/tick code) | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Hot Shower.fs` | [4lf3Rj](https://www.shadertoy.com/view/4lf3Rj); the source's noise and bark/rock textures are not redistributed (licenses unknown, replaced by procedural value noise) | Kali | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/CollapsingArchitecture.fs` | mod of [XltXWN](https://www.shadertoy.com/view/XltXWN) | mojovideotech (module); s23b (original) | CC BY-NC-SA 3.0 (stated in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/FractilianParabolicCircleInversion.fs` | based on "Fractal Soup" [lsB3zR](https://www.shadertoy.com/view/lsB3zR) | mojovideotech (module); @P_Malin (original) | CC BY-NC-SA 3.0 (stated in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/LogTransWarpSpiral.fs` | [Msd3Dn](https://www.shadertoy.com/view/Msd3Dn) ("Logarithmic Spiral Transform") | mojovideotech (module); Jakob Thomsen (original) | CC BY-NC-SA 3.0 (stated in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/UltimateFlame.fs` | "The Blue Flame" [lsjcRt](https://www.shadertoy.com/view/lsjcRt); 3D noise from Nikita Miropolskiy [XsX3zB](https://www.shadertoy.com/view/XsX3zB) | mojovideotech (module); Hadyn Lander (flame); Nikita Miropolskiy (3D noise) | CC BY-NC-SA 3.0 (stated in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/GreatBallOfFire.fs`, `src/renderer/src/application/renderers/isf/modules/GreatBallOfFire.js` | glslsandbox [e#30176.0](https://glslsandbox.com/e#30176.0) ("Fireball") | mojovideotech (module); @AlexWDunn (original) | CC BY-NC-SA 3.0 (stated in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/HexVortex.fs` | based on glslsandbox [e#4671.0](https://glslsandbox.com/e#4671.0) | mojovideotech | CC BY-NC-SA 3.0 (stated in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/Boxinator.fs` | mojovideotech; simplex noise by Ian McEwan / Ashima Arts | mojovideotech; Ashima Arts (MIT simplex noise) | CC BY-NC-SA 3.0 (stated in file); Ashima simplex noise is MIT |
| `src/renderer/src/application/renderers/isf/modules/isf/UltimateSpiral.fs` | mojovideotech | mojovideotech | CC BY-NC-SA 3.0 (stated in file) |

All of the above are bundled **as-is or as modified ports**; the ports remain
under the licenses listed for them and are not relicensed under the MIT
License.

## Plane9-derived Point10 scenes

The Point10 scene ports are adapted from **Plane9** presets by **Joakim Dahl**.
The vendored JS and Rust headers state CC BY-NC-SA (some presets 3.0, others
4.0 — the variant is recorded per scene below). The shared helpers
(`src/renderer/src/application/point10/plane9/P9Audio.js`,
`P9Expression.js`, `P9Glsl.js`) are part of the same Plane9/Point10 port and
are covered by these terms.

| Scene | Module files | License |
| --- | --- | --- |
| Boxy | `src/renderer/src/application/point10/scenes/Boxy.js`, `crates/modv-renderers/src/point10/scenes/boxy.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Color Cube | `src/renderer/src/application/point10/scenes/ColorCube.js`, `crates/modv-renderers/src/point10/scenes/color_cube.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Earth | `src/renderer/src/application/point10/scenes/Earth.js`, `crates/modv-renderers/src/point10/scenes/earth.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Hollow Cube | `src/renderer/src/application/point10/scenes/HollowCube.js`, `crates/modv-renderers/src/point10/scenes/hollow_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| It Is Coming For You | `src/renderer/src/application/point10/scenes/ItIsComingForYou.js`, `crates/modv-renderers/src/point10/scenes/it_is_coming_for_you.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Light Tunnel | `src/renderer/src/application/point10/scenes/LightTunnel.js`, `crates/modv-renderers/src/point10/scenes/light_tunnel.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Mirror Cube | `src/renderer/src/application/point10/scenes/MirrorCube.js`, `crates/modv-renderers/src/point10/scenes/mirror_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Sound Cube | `src/renderer/src/application/point10/scenes/SoundCube.js`, `crates/modv-renderers/src/point10/scenes/sound_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Square Tunnel | `src/renderer/src/application/point10/scenes/SquareTunnel.js`, `crates/modv-renderers/src/point10/scenes/square_tunnel.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Sun | `src/renderer/src/application/point10/scenes/Sun.js`, `crates/modv-renderers/src/point10/scenes/sun.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Wormhole | `src/renderer/src/application/point10/scenes/Wormhole.js`, `crates/modv-renderers/src/point10/scenes/wormhole.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Around Plus | `src/renderer/src/application/point10/scenes/AroundPlus.js`, `crates/modv-renderers/src/point10/scenes/around_plus.rs` | port of the Plane9 "Around And Around" preset; the vendored headers state no license, so it is treated as **CC BY-NC-SA** (Plane9 preset terms; variant unverified) |

## Bundled assets

| File | Source | Author | License |
| --- | --- | --- | --- |
| `src/renderer/src/application/point10/textures/pcb.jpg` | PCB photograph bundled with the Point10/Plane9 Glass Lens scene | Miguel Alba | license variant unverified — treated as **not MIT**; see the note below |
| `src/renderer/src/application/point10/textures/dvd_logo.svg` | Wikimedia Commons DVD text logo, extruded by the DvdRoom scene | Wikimedia Commons contributors | public domain (stated in `DvdRoom.js`) |

Other Point10 textures use ambientCG assets (CC0; e.g. `tinore`, `rubber002`,
`moon_04`, `concrete044c`), Poly Haven skies (`stormy`, `spruit_sunrise`), or
NASA-derived Earth/Milky Way imagery. Where an asset's provenance or license
is unclear it is not relicensed under MIT.

## Rust engine dependencies

The native engine binary statically links third-party Rust crates. Each crate
remains under its own license as declared in its `Cargo.toml`; they are **not**
relicensed under the MIT License of VmodV's own code. The direct dependencies
of the workspace crates are:

| Crate | License |
| --- | --- |
| `ashpd` | MIT |
| `boa_engine` | Unlicense OR MIT |
| `bytemuck` | Zlib OR Apache-2.0 OR MIT |
| `clap` | MIT OR Apache-2.0 |
| `cosmic-text` | MIT OR Apache-2.0 |
| `cpal` | Apache-2.0 |
| `egui`, `egui_kittest`, `egui-wgpu`, `egui-winit` | MIT OR Apache-2.0 |
| `gstreamer`, `gstreamer-app`, `gstreamer-video` | MIT OR Apache-2.0 |
| `image` | MIT OR Apache-2.0 |
| `indexmap` | Apache-2.0 OR MIT |
| `libc` | MIT OR Apache-2.0 |
| `libloading` | ISC |
| `midir` | MIT |
| `naga` | MIT OR Apache-2.0 |
| `notify` | CC0-1.0 |
| `pipewire` | MIT |
| `rosc` | MIT OR Apache-2.0 |
| `rustfft` | MIT OR Apache-2.0 |
| `serde`, `serde_json`, `thiserror` | MIT OR Apache-2.0 |
| `tokio` | MIT |
| `v4l` | MIT |
| `wgpu` | MIT OR Apache-2.0 |
| `winit` | Apache-2.0 |

Transitive dependencies (for example `zbus` via `ashpd`) carry the licenses
declared by their own crates. The authoritative inventory is the committed
`Cargo.lock` plus `cargo metadata --format-version 1`; packaging a release
should include the licence texts of every crate in that graph.

## Notes

- CC BY-NC-SA 3.0 requires attribution (preserved above and in the file
  headers), prohibits commercial use, and requires derivatives to be shared
  under the same license. CC BY-NC-SA 4.0 adds the same core terms with the
  4.0 legal code.
- If you are the author of any listed work and want different licensing
  terms, or believe a listed license is incorrect, please open an issue on
  the repository tracker.
