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
| `crates/modv-renderers/src/shader/modules/mattias_crt.rs` | [Ms23DR](https://www.shadertoy.com/view/Ms23DR) | Iñigo Quílez (postprocessing shader) | CC BY-NC-SA 3.0 (stated in file) |
| `crates/modv-renderers/src/shader/modules/chromatic_abberation.rs` | [MtXXDr](https://www.shadertoy.com/view/MtXXDr) | author not stated | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/ASCII Art.fs` | [lssGDj](https://www.shadertoy.com/view/lssGDj) | IMIMOT | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/st_Ms2SD1.fs.fs` | [Ms2SD1](https://www.shadertoy.com/view/Ms2SD1) ("Seascape") | Alexander Alekseev aka TDM | CC BY-NC-SA 3.0 (stated explicitly in file) |
| `resources/isf/Smoke Screen.fs` | [4t2SRz](https://www.shadertoy.com/view/4t2SRz) | jackdavenport | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/Zebre.fs` | [lsf3WH](https://www.shadertoy.com/view/lsf3WH) (noise functions); module by Joseph Fiola | Joseph Fiola; noise by Iñigo Quílez | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/WaveLines.fs` | [4dsGzH](https://www.shadertoy.com/view/4dsGzH) ("WAVES"); module by Joseph Fiola, remix by SilviaFabiani | Joseph Fiola / bonniem / SilviaFabiani | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/digital-crystal-tunnel.fs` | triangle noise from [4ts3z2](https://www.shadertoy.com/view/4ts3z2) (original usage noted as Xyptonjtroz; function by Nimitz); module credits WilstonOreo | WilstonOreo (module); Nimitz (noise function) | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/Broken LCD.fs` | noise functions [XdXGW8](https://www.shadertoy.com/view/XdXGW8), [lsf3WH](https://www.shadertoy.com/view/lsf3WH), [4dXBRH](https://www.shadertoy.com/view/4dXBRH), [XdXBRH](https://www.shadertoy.com/view/XdXBRH), [XsXfRH](https://www.shadertoy.com/view/XsXfRH), [4dffRH](https://www.shadertoy.com/view/4dffRH), [Msf3WH](https://www.shadertoy.com/view/Msf3WH), [Xsl3Dl](https://www.shadertoy.com/view/Xsl3Dl), [4sfGzS](https://www.shadertoy.com/view/4sfGzS) (value/gradient noise, 2D/3D) | VIDVOX (module); noise by Iñigo Quílez, ported by @colin_movecraft | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/Dirty Lens.fs`, `resources/isf/Smudged Lens.fs` | same Iñigo Quílez noise functions as Broken LCD | Iñigo Quílez, ported by @colin_movecraft and VIDVOX | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/spherical-shader-tut.fs` | [XdKGWm](https://www.shadertoy.com/view/XdKGWm) | WilstonOreo (http://omnido.me) | in-file license: "Creative Commons 3.0 Share Alike" (variant not specified); because the Shadertoy source defaults to CC BY-NC-SA 3.0 and a derivative cannot be less restrictive, this port is treated as **CC BY-NC-SA 3.0** |
| `resources/isf/Perlin Transition.fs` | [gl-transitions perlin.glsl](https://github.com/gl-transitions/gl-transitions/tree/master/perlin.glsl); the noise function credits [4dS3Wd](https://www.shadertoy.com/view/4dS3Wd) | Rich Harris (module); noise by Morgan McGuire | module stated MIT in file; the **4dS3Wd noise dependency states no license**, so it defaults to CC BY-NC-SA 3.0 |
| `resources/isf/Helix Eye.fs` | [cdSGRw](https://www.shadertoy.com/view/cdSGRw) ("Fear Inoculum"); `helix_sdf` adapted from [ttB3DV](https://www.shadertoy.com/view/ttB3DV); raymarch/fog helpers adapted from Iñigo Quílez (iq); camera rotation matrix from neilmendoza.com | RagePly (module); ttB3DV author not stated; Inigo Quilez (iq) raymarch/fog helpers; neilmendoza.com matrix (no license stated) | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/Traced Tunnel.fs` | [tdjfDR](https://www.shadertoy.com/view/tdjfDR); camera/tick code retained verbatim from NuSan; the source's two photo inputs are not redistributed (licenses unknown, replaced by procedural patterns) | Shane (module); NuSan (camera/tick code) | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/Hot Shower.fs` | [4lf3Rj](https://www.shadertoy.com/view/4lf3Rj); the source's noise and bark/rock textures are not redistributed (licenses unknown, replaced by procedural value noise) | Kali | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `resources/isf/CollapsingArchitecture.fs` | mod of [XltXWN](https://www.shadertoy.com/view/XltXWN) | mojovideotech (module); s23b (original) | CC BY-NC-SA 3.0 (stated in file) |
| `resources/isf/FractilianParabolicCircleInversion.fs` | based on "Fractal Soup" [lsB3zR](https://www.shadertoy.com/view/lsB3zR) | mojovideotech (module); @P_Malin (original) | CC BY-NC-SA 3.0 (stated in file) |
| `resources/isf/LogTransWarpSpiral.fs` | [Msd3Dn](https://www.shadertoy.com/view/Msd3Dn) ("Logarithmic Spiral Transform") | mojovideotech (module); Jakob Thomsen (original) | CC BY-NC-SA 3.0 (stated in file) |
| `resources/isf/UltimateFlame.fs` | "The Blue Flame" [lsjcRt](https://www.shadertoy.com/view/lsjcRt); 3D noise from Nikita Miropolskiy [XsX3zB](https://www.shadertoy.com/view/XsX3zB) | mojovideotech (module); Hadyn Lander (flame); Nikita Miropolskiy (3D noise) | CC BY-NC-SA 3.0 (stated in file) |
| `resources/isf/GreatBallOfFire.fs` | glslsandbox [e#30176.0](https://glslsandbox.com/e#30176.0) ("Fireball") | mojovideotech (module); @AlexWDunn (original) | CC BY-NC-SA 3.0 (stated in file) |
| `resources/isf/HexVortex.fs` | based on glslsandbox [e#4671.0](https://glslsandbox.com/e#4671.0) | mojovideotech | CC BY-NC-SA 3.0 (stated in file) |
| `resources/isf/Boxinator.fs` | mojovideotech; simplex noise by Ian McEwan / Ashima Arts | mojovideotech; Ashima Arts (MIT simplex noise) | CC BY-NC-SA 3.0 (stated in file); Ashima simplex noise is MIT |
| `resources/isf/UltimateSpiral.fs` | mojovideotech | mojovideotech | CC BY-NC-SA 3.0 (stated in file) |

All of the above are bundled **as-is or as modified ports**; the ports remain
under the licenses listed for them and are not relicensed under the MIT
License.

## Plane9-derived Point10 scenes

The Point10 scene ports are adapted from **Plane9** presets by **Joakim Dahl**.
The vendored Rust headers state CC BY-NC-SA (some presets 3.0, others 4.0 —
the variant is recorded per scene below). The shared Rust helpers (the
`point10/shared` modules: `p9_glsl`, `p9_shading`, `particle_alpha`,
`particle_sprite`, `random`, `water_wave`, `phrase_color` and `assets`) are
part of the same Plane9/Point10 port and are covered by these terms.

| Scene | Module file | License |
| --- | --- | --- |
| Boxy | `crates/modv-renderers/src/point10/scenes/boxy.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Color Cube | `crates/modv-renderers/src/point10/scenes/color_cube.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Earth | `crates/modv-renderers/src/point10/scenes/earth.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Hollow Cube | `crates/modv-renderers/src/point10/scenes/hollow_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| It Is Coming For You | `crates/modv-renderers/src/point10/scenes/it_is_coming_for_you.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Light Tunnel | `crates/modv-renderers/src/point10/scenes/light_tunnel.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Mirror Cube | `crates/modv-renderers/src/point10/scenes/mirror_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Sound Cube | `crates/modv-renderers/src/point10/scenes/sound_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Square Tunnel | `crates/modv-renderers/src/point10/scenes/square_tunnel.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Sun | `crates/modv-renderers/src/point10/scenes/sun.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Wormhole | `crates/modv-renderers/src/point10/scenes/wormhole.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Around Plus | `crates/modv-renderers/src/point10/scenes/around_plus.rs` | port of the Plane9 "Around And Around" preset; the vendored headers state no license, so it is treated as **CC BY-NC-SA** (Plane9 preset terms; variant unverified) |

## Bundled assets

| File | Source | Author | License |
| --- | --- | --- | --- |
| `resources/point10/textures/pcb.jpg` | PCB photograph bundled with the Point10/Plane9 Glass Lens scene | Miguel Alba | license variant unverified — treated as **not MIT**; see the note below |
| `resources/point10/textures/dvd_logo.svg` | Wikimedia Commons DVD text logo, extruded by the DvdRoom scene | Wikimedia Commons contributors | public domain (stated in the ported scene) |

Other Point10 textures use ambientCG assets (CC0; e.g. `tinore`, `rubber002`,
`moon_04`, `concrete044c`), Poly Haven skies (`stormy`, `spruit_sunrise`), or
NASA-derived Earth/Milky Way imagery. Where an asset's provenance or license
is unclear it is not relicensed under MIT.

## Fonts

The native shell (`crates/modv-ui/assets/fonts/`) bundles four IBM Plex faces:
IBM Plex Sans Regular and SemiBold, and IBM Plex Mono Regular and Medium. They
are licensed under the **SIL Open Font License, Version 1.1** (OFL-1.1); the
full licence text is included alongside them in
`crates/modv-ui/assets/fonts/OFL.txt`. The OFL permits bundling and
redistribution but forbids selling the fonts on their own; modified versions
must stay under the OFL and may not use the reserved font name "Plex". The
fonts are not relicensed under the MIT License of VmodV's own code.

| Files | Family / weight | Upstream | License |
| --- | --- | --- | --- |
| `crates/modv-ui/assets/fonts/IBMPlexSans-Regular.ttf`, `IBMPlexSans-SemiBold.ttf` | IBM Plex Sans Regular / SemiBold | [IBM/plex](https://github.com/IBM/plex) | SIL OFL 1.1 |
| `crates/modv-ui/assets/fonts/IBMPlexMono-Regular.ttf`, `IBMPlexMono-Medium.ttf` | IBM Plex Mono Regular / Medium | [IBM/plex](https://github.com/IBM/plex) | SIL OFL 1.1 |

egui's bundled Ubuntu/Hack faces remain in the fallback chains for glyphs the
Plex subsets do not cover; their notice is in the Rust dependency section
(`epaint_default_fonts`).

## Rust engine dependencies

The Rust workspace (`crates/`) statically links third-party crates into the
engine and UI binaries. Each crate remains under its own licence as declared
in its `Cargo.toml` — they are **not** relicensed under the MIT License of
VmodV's own code. The table below is the complete inventory: every registry
package resolved by `crates/Cargo.lock`, including transitive dependencies
(the workspace's own `modv-*` crates are first-party and not listed), sorted
by crate name, with the crate's SPDX licence expression and an upstream link.
Regenerate with `cargo metadata --format-version 1 --locked --offline` in the
toolchain.

Licences with obligations beyond permissive attribution:

- **MPL-2.0** (`colored`): file-level copyleft — MPL-2.0 source files keep
  their licence headers, and a copy of the MPL-2.0 text must accompany any
  distribution that includes the crate's code.
- **(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0**
  (`epaint_default_fonts`): embeds the Ubuntu font family; the SIL Open Font
  Licence and the Ubuntu font licence require this attribution, forbid selling
  the fonts on their own, and require renamed derivatives to stay under the
  OFL.
- **(MIT OR Apache-2.0) AND NCSA** (`libfuzzer-sys`): the bundled libFuzzer
  sources carry the University of Illinois/NCSA licence, which requires
  retaining its notice in redistributed source.

| Crate | Licence (SPDX) | Upstream |
| --- | --- | --- |
| `ab_glyph` | Apache-2.0 | https://github.com/alexheretic/ab-glyph |
| `ab_glyph_rasterizer` | Apache-2.0 | https://github.com/alexheretic/ab-glyph |
| `accesskit` | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| `accesskit_consumer` | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| `adler2` | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 |
| `ahash` | MIT OR Apache-2.0 | https://github.com/tkaitchuck/ahash |
| `aho-corasick` | Unlicense OR MIT | https://github.com/BurntSushi/aho-corasick |
| `aligned` | MIT OR Apache-2.0 | https://github.com/rust-embedded-community/aligned |
| `aligned-vec` | MIT | https://github.com/sarah-ek/aligned-vec/ |
| `allocator-api2` | MIT OR Apache-2.0 | https://github.com/zakarumych/allocator-api2 |
| `alsa` | Apache-2.0 OR MIT | https://github.com/diwic/alsa-rs |
| `alsa-sys` | MIT | https://github.com/diwic/alsa-sys |
| `android_system_properties` | MIT OR Apache-2.0 | https://github.com/nical/android_system_properties |
| `android-activity` | MIT OR Apache-2.0 | https://github.com/rust-mobile/android-activity |
| `android-build` | MIT | https://github.com/project-robius/android-build |
| `android-properties` | MIT | https://github.com/miklelappo/android-properties |
| `annotate-snippets` | MIT OR Apache-2.0 | https://github.com/rust-lang/annotate-snippets-rs |
| `anstream` | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle |
| `anstyle` | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle |
| `anstyle-parse` | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle |
| `anstyle-query` | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle |
| `anstyle-wincon` | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle |
| `anyhow` | MIT OR Apache-2.0 | https://github.com/dtolnay/anyhow |
| `arbitrary` | MIT OR Apache-2.0 | https://github.com/rust-fuzz/arbitrary/ |
| `arboard` | MIT OR Apache-2.0 | https://github.com/1Password/arboard |
| `arg_enum_proc_macro` | MIT | https://github.com/lu-zero/arg_enum_proc_macro |
| `arrayref` | BSD-2-Clause | https://github.com/droundy/arrayref |
| `arrayvec` | MIT OR Apache-2.0 | https://github.com/bluss/arrayvec |
| `as-raw-xcb-connection` | MIT OR Apache-2.0 | https://github.com/psychon/as-raw-xcb-connection |
| `as-slice` | MIT OR Apache-2.0 | https://github.com/japaric/as-slice |
| `ash` | MIT OR Apache-2.0 | https://github.com/ash-rs/ash |
| `ashpd` | MIT | https://github.com/bilelmoussaoui/ashpd |
| `async-channel` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-channel |
| `async-trait` | MIT OR Apache-2.0 | https://github.com/dtolnay/async-trait |
| `atomic_refcell` | Apache-2.0 OR MIT | https://github.com/mozilla/atomic_refcell |
| `atomic-waker` | Apache-2.0 OR MIT | https://github.com/smol-rs/atomic-waker |
| `autocfg` | Apache-2.0 OR MIT | https://github.com/cuviper/autocfg |
| `av-scenechange` | MIT | https://github.com/rust-av/av-scenechange |
| `av1-grain` | BSD-2-Clause | https://github.com/rust-av/av1-grain |
| `avif-serialize` | BSD-3-Clause | https://github.com/kornelski/avif-serialize |
| `bindgen` | BSD-3-Clause | https://github.com/rust-lang/rust-bindgen |
| `bit_field` | MIT OR Apache-2.0 | https://github.com/phil-opp/rust-bit-field |
| `bit-set` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-set |
| `bit-vec` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-vec |
| `bitflags` | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| `bitstream-io` | MIT OR Apache-2.0 | https://github.com/tuffy/bitstream-io |
| `block2` | MIT | https://github.com/madsmtm/objc2 |
| `boa_ast` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `boa_engine` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `boa_gc` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `boa_interner` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `boa_macros` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `boa_parser` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `boa_string` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `built` | MIT | https://github.com/lukaslueg/built |
| `bumpalo` | MIT OR Apache-2.0 | https://github.com/fitzgen/bumpalo |
| `bytemuck` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| `bytemuck_derive` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| `byteorder` | Unlicense OR MIT | https://github.com/BurntSushi/byteorder |
| `byteorder-lite` | Unlicense OR MIT | https://github.com/image-rs/byteorder-lite |
| `bytes` | MIT | https://github.com/tokio-rs/bytes |
| `calloop` | MIT | https://github.com/Smithay/calloop |
| `calloop-wayland-source` | MIT | https://github.com/smithay/calloop-wayland-source |
| `cc` | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| `cesu8` | Apache-2.0 OR MIT | https://github.com/emk/cesu8-rs |
| `cexpr` | Apache-2.0 OR MIT | https://github.com/jethrogb/rust-cexpr |
| `cfg_aliases` | MIT | https://github.com/katharostech/cfg_aliases |
| `cfg-expr` | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/cfg-expr |
| `cfg-if` | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| `chacha20` | MIT OR Apache-2.0 | https://github.com/RustCrypto/stream-ciphers |
| `clang-sys` | Apache-2.0 | https://github.com/KyleMayes/clang-sys |
| `clap` | MIT OR Apache-2.0 | https://github.com/clap-rs/clap |
| `clap_builder` | MIT OR Apache-2.0 | https://github.com/clap-rs/clap |
| `clap_lex` | MIT OR Apache-2.0 | https://github.com/clap-rs/clap |
| `clipboard-win` | BSL-1.0 | https://github.com/DoumanAsh/clipboard-win |
| `codespan-reporting` | Apache-2.0 | https://github.com/brendanzab/codespan |
| `color` | Apache-2.0 OR MIT | https://github.com/linebender/color |
| `color_quant` | MIT | https://github.com/image-rs/color_quant |
| `colorchoice` | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle |
| `colored` | MPL-2.0 | https://github.com/mackwic/colored |
| `combine` | MIT | https://github.com/Marwes/combine |
| `concurrent-queue` | Apache-2.0 OR MIT | https://github.com/smol-rs/concurrent-queue |
| `convert_case` | MIT | https://github.com/rutrum/convert-case |
| `cookie-factory` | MIT | https://github.com/rust-bakery/cookie-factory |
| `core-foundation` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `core-foundation-sys` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `core-graphics` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `core-graphics-types` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `coreaudio-rs` | MIT OR Apache-2.0 | https://github.com/RustAudio/coreaudio-rs |
| `coremidi` | MIT | https://github.com/chris-zen/coremidi |
| `coremidi-sys` | MIT | https://github.com/jonas-k/coremidi-sys |
| `cosmic-text` | MIT OR Apache-2.0 | https://github.com/pop-os/cosmic-text |
| `cow-utils` | MIT | https://github.com/RReverser/cow-utils-rs |
| `cpal` | Apache-2.0 | https://github.com/RustAudio/cpal |
| `cpufeatures` | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| `crc32fast` | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| `crossbeam-deque` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| `crossbeam-epoch` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| `crossbeam-utils` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| `crunchy` | MIT | https://github.com/eira-fransham/crunchy |
| `cursor-icon` | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/cursor-icon |
| `dashmap` | MIT | https://github.com/xacrimon/dashmap |
| `dasp_sample` | MIT OR Apache-2.0 | https://github.com/rustaudio/sample |
| `deranged` | MIT OR Apache-2.0 | https://github.com/jhpratt/deranged |
| `dify` | MIT | https://github.com/jihchi/dify |
| `dispatch` | MIT | https://github.com/SSheldon/rust-dispatch |
| `dispatch2` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `displaydoc` | MIT OR Apache-2.0 | https://github.com/yaahc/displaydoc |
| `dlib` | MIT | https://github.com/elinorbgr/dlib |
| `dlopen2` | MIT | https://github.com/OpenByteDev/dlopen2 |
| `document-features` | MIT OR Apache-2.0 | https://github.com/slint-ui/document-features |
| `downcast-rs` | MIT OR Apache-2.0 | https://github.com/marcianx/downcast-rs |
| `dpi` | Apache-2.0 AND MIT | https://github.com/rust-windowing/winit |
| `dynify` | MIT OR Apache-2.0 | https://github.com/loichyan/dynify |
| `dynify-macros` | MIT OR Apache-2.0 | https://github.com/loichyan/dynify |
| `ecolor` | MIT OR Apache-2.0 | https://github.com/emilk/egui |
| `eframe` | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/eframe |
| `egui` | MIT OR Apache-2.0 | https://github.com/emilk/egui |
| `egui_kittest` | MIT OR Apache-2.0 | https://github.com/emilk/egui |
| `egui-wgpu` | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/egui-wgpu |
| `egui-winit` | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/egui-winit |
| `either` | MIT OR Apache-2.0 | https://github.com/rayon-rs/either |
| `emath` | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/emath |
| `endi` | MIT | https://github.com/zeenix/endi |
| `epaint` | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/epaint |
| `epaint_default_fonts` | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts |
| `equator` | MIT | https://github.com/sarah-ek/equator/ |
| `equator-macro` | MIT | https://github.com/sarah-ek/equator/ |
| `equivalent` | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| `errno` | MIT OR Apache-2.0 | https://github.com/lambda-fairy/rust-errno |
| `error-code` | BSL-1.0 | https://github.com/DoumanAsh/error-code |
| `euclid` | MIT OR Apache-2.0 | https://github.com/servo/euclid |
| `event-listener` | Apache-2.0 OR MIT | https://github.com/smol-rs/event-listener |
| `event-listener-strategy` | Apache-2.0 OR MIT | https://github.com/smol-rs/event-listener-strategy |
| `exr` | BSD-3-Clause | https://github.com/johannesvollmer/exrs |
| `fast-float2` | MIT OR Apache-2.0 | https://github.com/Alexhuszagh/fast-float-rust |
| `fastrand` | Apache-2.0 OR MIT | https://github.com/smol-rs/fastrand |
| `fax` | MIT | https://github.com/pdf-rs/fax |
| `fdeflate` | MIT OR Apache-2.0 | https://github.com/image-rs/fdeflate |
| `fearless_simd` | Apache-2.0 OR MIT | https://github.com/linebender/fearless_simd |
| `find-msvc-tools` | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| `fixedbitset` | MIT OR Apache-2.0 | https://github.com/petgraph/fixedbitset |
| `flate2` | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs |
| `foldhash` | Zlib | https://github.com/orlp/foldhash |
| `font-types` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `fontconfig-parser` | MIT | https://github.com/Riey/fontconfig-parser |
| `fontdb` | MIT | https://github.com/RazrFalcon/fontdb |
| `foreign-types` | MIT OR Apache-2.0 | https://github.com/sfackler/foreign-types |
| `foreign-types-macros` | MIT OR Apache-2.0 | https://github.com/sfackler/foreign-types |
| `foreign-types-shared` | MIT OR Apache-2.0 | https://github.com/sfackler/foreign-types |
| `futures-channel` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-concurrency` | MIT OR Apache-2.0 | https://github.com/yoshuawuyts/futures-concurrency |
| `futures-core` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-executor` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-io` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-lite` | Apache-2.0 OR MIT | https://github.com/smol-rs/futures-lite |
| `futures-macro` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-sink` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-task` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-util` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `gethostname` | Apache-2.0 | https://codeberg.org/swsnr/gethostname.rs |
| `getopts` | MIT OR Apache-2.0 | https://github.com/rust-lang/getopts |
| `getrandom` | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| `gif` | MIT OR Apache-2.0 | https://github.com/image-rs/image-gif |
| `gio-sys` | MIT | https://github.com/gtk-rs/gtk-rs-core |
| `gl_generator` | Apache-2.0 | https://github.com/brendanzab/gl-rs/ |
| `glib` | MIT | https://github.com/gtk-rs/gtk-rs-core |
| `glib-macros` | MIT | https://github.com/gtk-rs/gtk-rs-core |
| `glib-sys` | MIT | https://github.com/gtk-rs/gtk-rs-core |
| `glifo` | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| `glob` | MIT OR Apache-2.0 | https://github.com/rust-lang/glob |
| `glow` | MIT OR Apache-2.0 OR Zlib | https://github.com/grovesNL/glow |
| `glutin_wgl_sys` | Apache-2.0 | https://github.com/rust-windowing/glutin |
| `gobject-sys` | MIT | https://github.com/gtk-rs/gtk-rs-core |
| `gpu-allocator` | MIT OR Apache-2.0 | https://github.com/Traverse-Research/gpu-allocator |
| `gstreamer` | MIT OR Apache-2.0 | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `gstreamer-app` | MIT OR Apache-2.0 | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `gstreamer-app-sys` | MIT | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `gstreamer-base` | MIT OR Apache-2.0 | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `gstreamer-base-sys` | MIT | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `gstreamer-sys` | MIT | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `gstreamer-video` | MIT OR Apache-2.0 | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `gstreamer-video-sys` | MIT | https://gitlab.freedesktop.org/gstreamer/gstreamer-rs |
| `guillotiere` | MIT OR Apache-2.0 | https://github.com/nical/guillotiere |
| `half` | MIT OR Apache-2.0 | https://github.com/VoidStarKat/half-rs |
| `harfrust` | MIT | https://github.com/harfbuzz/harfrust |
| `hashbrown` | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| `heck` | MIT OR Apache-2.0 | https://github.com/withoutboats/heck |
| `hermit-abi` | MIT OR Apache-2.0 | https://github.com/hermit-os/hermit-rs |
| `hex` | MIT OR Apache-2.0 | https://github.com/KokaKiwi/rust-hex |
| `home` | MIT OR Apache-2.0 | https://github.com/rust-lang/cargo |
| `icu_collections` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_locale_core` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_normalizer` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_normalizer_data` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_properties` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_properties_data` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_provider` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `image` | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| `image-webp` | MIT OR Apache-2.0 | https://github.com/image-rs/image-webp |
| `imgref` | CC0-1.0 OR Apache-2.0 | https://github.com/kornelski/imgref |
| `indexmap` | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| `inotify` | ISC | https://github.com/hannobraun/inotify-rs |
| `inotify-sys` | ISC | https://github.com/hannobraun/inotify-sys |
| `interpolate_name` | MIT | https://github.com/lu-zero/interpolate_name |
| `intrusive-collections` | MIT OR Apache-2.0 | https://github.com/Amanieu/intrusive-rs |
| `is_terminal_polyfill` | MIT OR Apache-2.0 | https://github.com/polyfill-rs/is_terminal_polyfill |
| `is-docker` | MIT | https://github.com/TheLarkInn/is-docker |
| `is-wsl` | MIT | https://github.com/TheLarkInn/is-wsl |
| `itertools` | MIT OR Apache-2.0 | https://github.com/rust-itertools/itertools |
| `itoa` | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| `java-locator` | MIT OR Apache-2.0 | https://github.com/astonbitecode/java-locator |
| `jni` | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-rs |
| `jni-macros` | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-rs |
| `jni-min-helper` | MIT OR Apache-2.0 | https://github.com/wuwbobo2021/jni-min-helper |
| `jni-sys` | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys |
| `jni-sys-macros` | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys |
| `jobserver` | MIT OR Apache-2.0 | https://github.com/rust-lang/jobserver-rs |
| `js-sys` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys |
| `khronos_api` | Apache-2.0 | https://github.com/brendanzab/gl-rs/ |
| `khronos-egl` | MIT OR Apache-2.0 | https://github.com/timothee-haudebourg/khronos-egl |
| `kittest` | MIT OR Apache-2.0 | https://github.com/rerun-io/kittest |
| `kqueue` | MIT | https://gitlab.com/rust-kqueue/rust-kqueue |
| `kqueue-sys` | MIT | https://gitlab.com/rust-kqueue/rust-kqueue-sys |
| `kstring` | MIT OR Apache-2.0 | https://github.com/cobalt-org/kstring |
| `kurbo` | Apache-2.0 OR MIT | https://github.com/linebender/kurbo |
| `lazy_static` | MIT OR Apache-2.0 | https://github.com/rust-lang-nursery/lazy-static.rs |
| `lazycell` | MIT OR Apache-2.0 | https://github.com/indiv0/lazycell |
| `lebe` | BSD-3-Clause | https://github.com/johannesvollmer/lebe |
| `libc` | MIT OR Apache-2.0 | https://github.com/rust-lang/libc |
| `libfuzzer-sys` | (MIT OR Apache-2.0) AND NCSA | https://github.com/rust-fuzz/libfuzzer |
| `libloading` | ISC | https://github.com/nagisa/rust_libloading/ |
| `libm` | MIT | https://github.com/rust-lang/compiler-builtins |
| `libredox` | MIT | https://gitlab.redox-os.org/redox-os/libredox |
| `libspa` | MIT | https://gitlab.freedesktop.org/pipewire/pipewire-rs |
| `libspa-sys` | MIT | https://gitlab.freedesktop.org/pipewire/pipewire-rs |
| `linebender_resource_handle` | Apache-2.0 OR MIT | https://github.com/linebender/raw_resource_handle |
| `linux-raw-sys` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys |
| `litemap` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `litrs` | MIT OR Apache-2.0 | https://github.com/LukasKalbertodt/litrs |
| `lock_api` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| `log` | MIT OR Apache-2.0 | https://github.com/rust-lang/log |
| `loop9` | MIT | https://gitlab.com/kornelski/loop9 |
| `mach2` | BSD-2-Clause OR MIT OR Apache-2.0 | https://github.com/JohnTitor/mach2 |
| `maybe-rayon` | MIT | https://github.com/shssoichiro/maybe-rayon |
| `memchr` | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| `memmap2` | MIT OR Apache-2.0 | https://github.com/RazrFalcon/memmap2-rs |
| `memoffset` | MIT | https://github.com/Gilnaa/memoffset |
| `midir` | MIT | https://github.com/Boddlnagg/midir |
| `minimal-lexical` | MIT OR Apache-2.0 | https://github.com/Alexhuszagh/minimal-lexical |
| `miniz_oxide` | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| `mio` | MIT | https://github.com/tokio-rs/mio |
| `moxcms` | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/moxcms |
| `muldiv` | MIT | https://github.com/sdroege/rust-muldiv |
| `multiversion` | MIT OR Apache-2.0 | https://github.com/calebzulawski/multiversion |
| `multiversion-macros` | MIT OR Apache-2.0 | https://github.com/calebzulawski/multiversion |
| `naga` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `naga-types` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `ndk` | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk |
| `ndk-context` | MIT OR Apache-2.0 | https://github.com/rust-windowing/android-ndk-rs |
| `ndk-sys` | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk |
| `new_debug_unreachable` | MIT | https://github.com/mbrubeck/rust-debug-unreachable |
| `nix` | MIT | https://github.com/nix-rust/nix |
| `no_std_io2` | Apache-2.0 OR MIT | https://github.com/wcampbell0x2a/no-std-io2 |
| `nohash-hasher` | Apache-2.0 OR MIT | https://github.com/paritytech/nohash-hasher |
| `nom` | MIT | https://github.com/rust-bakery/nom |
| `noop_proc_macro` | MIT | https://github.com/lu-zero/noop_proc_macro |
| `notify` | CC0-1.0 | https://github.com/notify-rs/notify |
| `notify-types` | MIT OR Apache-2.0 | https://github.com/notify-rs/notify |
| `num_enum` | BSD-3-Clause OR MIT OR Apache-2.0 | https://github.com/illicitonion/num_enum |
| `num_enum_derive` | BSD-3-Clause OR MIT OR Apache-2.0 | https://github.com/illicitonion/num_enum |
| `num_threads` | MIT OR Apache-2.0 | https://github.com/jhpratt/num_threads |
| `num-bigint` | MIT OR Apache-2.0 | https://github.com/rust-num/num-bigint |
| `num-complex` | MIT OR Apache-2.0 | https://github.com/rust-num/num-complex |
| `num-conv` | MIT OR Apache-2.0 | https://github.com/jhpratt/num-conv |
| `num-derive` | MIT OR Apache-2.0 | https://github.com/rust-num/num-derive |
| `num-integer` | MIT OR Apache-2.0 | https://github.com/rust-num/num-integer |
| `num-rational` | MIT OR Apache-2.0 | https://github.com/rust-num/num-rational |
| `num-traits` | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits |
| `objc-sys` | MIT | https://github.com/madsmtm/objc2 |
| `objc2` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-app-kit` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-audio-toolbox` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-avf-audio` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-cloud-kit` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-contacts` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-audio` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-audio-types` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-data` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-foundation` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-graphics` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-image` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-location` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-core-services` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-encode` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-foundation` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-io-surface` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-link-presentation` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-metal` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-quartz-core` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-symbols` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-ui-kit` | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| `objc2-uniform-type-identifiers` | MIT | https://github.com/madsmtm/objc2 |
| `objc2-user-notifications` | MIT | https://github.com/madsmtm/objc2 |
| `once_cell` | MIT OR Apache-2.0 | https://github.com/matklad/once_cell |
| `once_cell_polyfill` | MIT OR Apache-2.0 | https://github.com/polyfill-rs/once_cell_polyfill |
| `oneshot` | MIT OR Apache-2.0 | https://github.com/faern/oneshot |
| `open` | MIT | https://github.com/Byron/open-rs |
| `option-operations` | MIT OR Apache-2.0 | https://github.com/fengalin/option-operations |
| `orbclient` | MIT | https://gitlab.redox-os.org/redox-os/orbclient |
| `ordered-float` | MIT | https://github.com/reem/rust-ordered-float |
| `ordered-stream` | MIT OR Apache-2.0 | https://github.com/danieldg/ordered-stream |
| `os_pipe` | MIT | https://github.com/oconnor663/os_pipe.rs |
| `owned_ttf_parser` | Apache-2.0 | https://github.com/alexheretic/owned-ttf-parser |
| `parking` | Apache-2.0 OR MIT | https://github.com/smol-rs/parking |
| `parking_lot` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| `parking_lot_core` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| `paste` | MIT OR Apache-2.0 | https://github.com/dtolnay/paste |
| `pastey` | MIT OR Apache-2.0 | https://github.com/as1100k/pastey |
| `peeking_take_while` | Apache-2.0 OR MIT | https://github.com/fitzgen/peeking_take_while |
| `peniko` | Apache-2.0 OR MIT | https://github.com/linebender/peniko |
| `percent-encoding` | MIT OR Apache-2.0 | https://github.com/servo/rust-url/ |
| `petgraph` | MIT OR Apache-2.0 | https://github.com/petgraph/petgraph |
| `phf` | MIT | https://github.com/rust-phf/rust-phf |
| `phf_generator` | MIT | https://github.com/rust-phf/rust-phf |
| `phf_macros` | MIT | https://github.com/rust-phf/rust-phf |
| `phf_shared` | MIT | https://github.com/rust-phf/rust-phf |
| `pin-project` | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project |
| `pin-project-internal` | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project |
| `pin-project-lite` | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite |
| `pipewire` | MIT | https://gitlab.freedesktop.org/pipewire/pipewire-rs |
| `pipewire-sys` | MIT | https://gitlab.freedesktop.org/pipewire/pipewire-rs |
| `pkg-config` | MIT OR Apache-2.0 | https://github.com/rust-lang/pkg-config-rs |
| `plain` | MIT OR Apache-2.0 | https://github.com/randomites/plain |
| `png` | MIT OR Apache-2.0 | https://github.com/image-rs/image-png |
| `polling` | Apache-2.0 OR MIT | https://github.com/smol-rs/polling |
| `pollster` | Apache-2.0 OR MIT | https://github.com/zesterer/pollster |
| `polycool` | MIT OR Apache-2.0 | https://github.com/linebender/kurbo |
| `portable-atomic` | Apache-2.0 OR MIT | https://github.com/taiki-e/portable-atomic |
| `portable-atomic-util` | Apache-2.0 OR MIT | https://github.com/taiki-e/portable-atomic-util |
| `potential_utf` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `powerfmt` | MIT OR Apache-2.0 | https://github.com/jhpratt/powerfmt |
| `pp-rs` | BSD-3-Clause | https://github.com/Kangz/glslpp-rs |
| `ppv-lite86` | MIT OR Apache-2.0 | https://github.com/cryptocorrosion/cryptocorrosion |
| `presser` | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/presser |
| `prettyplease` | MIT OR Apache-2.0 | https://github.com/dtolnay/prettyplease |
| `primal-check` | MIT OR Apache-2.0 | https://github.com/huonw/primal |
| `proc-macro-crate` | MIT OR Apache-2.0 | https://github.com/bkchr/proc-macro-crate |
| `proc-macro2` | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| `process_path` | MIT OR Apache-2.0 | https://github.com/wesleywiser/process_path |
| `profiling` | MIT OR Apache-2.0 | https://github.com/aclysma/profiling |
| `profiling-procmacros` | MIT OR Apache-2.0 | https://github.com/aclysma/profiling |
| `pxfm` | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/pxfm |
| `qoi` | MIT OR Apache-2.0 | https://github.com/aldanor/qoi-rust |
| `quick-error` | MIT OR Apache-2.0 | https://github.com/tailhook/quick-error |
| `quick-xml` | MIT | https://github.com/tafia/quick-xml |
| `quote` | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| `r-efi` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | https://github.com/r-efi/r-efi |
| `rand` | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| `rand_chacha` | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| `rand_core` | MIT OR Apache-2.0 | https://github.com/rust-random/rand_core |
| `range-alloc` | MIT OR Apache-2.0 | https://github.com/gfx-rs/range-alloc |
| `rangemap` | MIT OR Apache-2.0 | https://github.com/jeffparsons/rangemap |
| `rav1e` | BSD-2-Clause | https://github.com/xiph/rav1e/ |
| `ravif` | BSD-3-Clause | https://github.com/kornelski/cavif-rs |
| `raw-window-handle` | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/raw-window-handle |
| `raw-window-metal` | MIT OR Apache-2.0 | https://github.com/rust-windowing/raw-window-metal |
| `rayon` | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| `rayon-core` | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| `read-fonts` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `redox_syscall` | MIT | https://gitlab.redox-os.org/redox-os/kernel |
| `regex` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| `regex-automata` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| `regex-syntax` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| `regress` | MIT OR Apache-2.0 | https://github.com/ridiculousfish/regress |
| `renderdoc-sys` | MIT OR Apache-2.0 | https://github.com/ebkalderon/renderdoc-rs |
| `rgb` | MIT | https://github.com/kornelski/rust-rgb |
| `rosc` | MIT OR Apache-2.0 | https://github.com/klingtnet/rosc |
| `roxmltree` | MIT OR Apache-2.0 | https://github.com/RazrFalcon/roxmltree |
| `rustc_version` | MIT OR Apache-2.0 | https://github.com/djc/rustc-version-rs |
| `rustc-hash` | Apache-2.0 OR MIT | https://github.com/rust-lang/rustc-hash |
| `rustfft` | MIT OR Apache-2.0 | https://github.com/ejmahler/RustFFT |
| `rustix` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/rustix |
| `rustversion` | MIT OR Apache-2.0 | https://github.com/dtolnay/rustversion |
| `rustybuzz` | MIT | https://github.com/RazrFalcon/rustybuzz |
| `ryu-js` | Apache-2.0 OR BSL-1.0 | https://github.com/boa-dev/ryu-js |
| `same-file` | Unlicense OR MIT | https://github.com/BurntSushi/same-file |
| `scoped-tls` | MIT OR Apache-2.0 | https://github.com/alexcrichton/scoped-tls |
| `scopeguard` | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard |
| `sctk-adwaita` | MIT | https://github.com/PolyMeilex/sctk-adwaita |
| `self_cell` | Apache-2.0 OR GPL-2.0-only | https://github.com/Voultapher/self_cell |
| `semver` | MIT OR Apache-2.0 | https://github.com/dtolnay/semver |
| `send_wrapper` | MIT OR Apache-2.0 | https://github.com/thk1/send_wrapper |
| `serde` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| `serde_core` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| `serde_derive` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| `serde_json` | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| `serde_repr` | MIT OR Apache-2.0 | https://github.com/dtolnay/serde-repr |
| `serde_spanned` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `shlex` | MIT OR Apache-2.0 | https://github.com/comex/rust-shlex |
| `signal-hook-registry` | MIT OR Apache-2.0 | https://github.com/vorner/signal-hook |
| `simd_cesu8` | Apache-2.0 OR MIT | https://github.com/seancroach/simd_cesu8 |
| `simd_helpers` | MIT | https://github.com/lu-zero/simd_helpers |
| `simd-adler32` | MIT | https://github.com/mcountryman/simd-adler32 |
| `simdutf8` | MIT OR Apache-2.0 | https://github.com/rusticstuff/simdutf8 |
| `siphasher` | MIT OR Apache-2.0 | https://github.com/jedisct1/rust-siphash |
| `skrifa` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `slab` | MIT | https://github.com/tokio-rs/slab |
| `slotmap` | Zlib | https://github.com/orlp/slotmap |
| `small_btree` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `smallvec` | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec |
| `smithay-client-toolkit` | MIT | https://github.com/smithay/client-toolkit |
| `smol_str` | MIT OR Apache-2.0 | https://github.com/rust-analyzer/smol_str |
| `socket2` | MIT OR Apache-2.0 | https://github.com/rust-lang/socket2 |
| `spirv` | Apache-2.0 | https://github.com/gfx-rs/rspirv |
| `stable_deref_trait` | MIT OR Apache-2.0 | https://github.com/storyyeller/stable_deref_trait |
| `static_assertions` | MIT OR Apache-2.0 | https://github.com/nvzqz/static-assertions-rs |
| `strength_reduce` | MIT OR Apache-2.0 | https://github.com/ejmahler/strength_reduce |
| `strict-num` | MIT | https://github.com/RazrFalcon/strict-num |
| `strsim` | MIT | https://github.com/rapidfuzz/strsim-rs |
| `strum` | MIT | https://github.com/Peternator7/strum |
| `strum_macros` | MIT | https://github.com/Peternator7/strum |
| `swash` | Apache-2.0 OR MIT | https://github.com/dfrg/swash |
| `syn` | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| `synstructure` | MIT | https://github.com/mystor/synstructure |
| `sys-locale` | MIT OR Apache-2.0 | https://github.com/1Password/sys-locale |
| `system-deps` | MIT OR Apache-2.0 | https://github.com/gdesmott/system-deps |
| `tag_ptr` | Unlicense OR MIT | https://github.com/boa-dev/boa |
| `tap` | MIT | https://github.com/myrrlyn/tap |
| `target-features` | MIT OR Apache-2.0 | https://github.com/calebzulawski/target-features |
| `target-lexicon` | Apache-2.0 WITH LLVM-exception | https://github.com/bytecodealliance/target-lexicon |
| `tempfile` | MIT OR Apache-2.0 | https://github.com/Stebalien/tempfile |
| `termcolor` | Unlicense OR MIT | https://github.com/BurntSushi/termcolor |
| `thin-vec` | MIT OR Apache-2.0 | https://github.com/mozilla/thin-vec |
| `thiserror` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| `thiserror-impl` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| `tiff` | MIT | https://github.com/image-rs/image-tiff |
| `time` | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| `time-core` | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| `time-macros` | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| `tiny-skia` | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia |
| `tiny-skia-path` | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia/tree/master/path |
| `tinystr` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `tinyvec` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/tinyvec |
| `tokio` | MIT | https://github.com/tokio-rs/tokio |
| `toml` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `toml_datetime` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `toml_edit` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `toml_parser` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `toml_writer` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `tracing` | MIT | https://github.com/tokio-rs/tracing |
| `tracing-core` | MIT | https://github.com/tokio-rs/tracing |
| `transpose` | MIT OR Apache-2.0 | https://github.com/ejmahler/transpose |
| `tree_magic_mini` | MIT | https://github.com/mbrubeck/tree_magic/ |
| `ttf-parser` | MIT OR Apache-2.0 | https://github.com/harfbuzz/ttf-parser |
| `type-map` | MIT OR Apache-2.0 | https://github.com/kardeiz/type-map |
| `uds_windows` | MIT | https://github.com/haraldh/rust_uds_windows |
| `unicode-bidi` | MIT OR Apache-2.0 | https://github.com/servo/unicode-bidi |
| `unicode-bidi-mirroring` | MIT OR Apache-2.0 | https://github.com/RazrFalcon/unicode-bidi-mirroring |
| `unicode-ccc` | MIT OR Apache-2.0 | https://github.com/RazrFalcon/unicode-ccc |
| `unicode-general-category` | Apache-2.0 | https://github.com/yeslogic/unicode-general-category |
| `unicode-ident` | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| `unicode-linebreak` | Apache-2.0 | https://github.com/axelf4/unicode-linebreak |
| `unicode-properties` | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/unicode-rs/unicode-properties |
| `unicode-script` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-script |
| `unicode-segmentation` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-segmentation |
| `unicode-width` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-width |
| `unicode-xid` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-xid |
| `utf_types` | Apache-2.0 OR MIT | https://github.com/hsivonen/utf_types |
| `utf16_iter` | Apache-2.0 OR MIT | https://github.com/hsivonen/utf16_iter |
| `utf8_iter` | Apache-2.0 OR MIT | https://github.com/hsivonen/utf8_iter |
| `utf8parse` | Apache-2.0 OR MIT | https://github.com/alacritty/vte |
| `uuid` | Apache-2.0 OR MIT | https://github.com/uuid-rs/uuid |
| `v_frame` | BSD-2-Clause | https://github.com/rust-av/v_frame |
| `v4l` | MIT | https://github.com/raymanfx/libv4l-rs |
| `v4l2-sys-mit` | MIT | https://crates.io/crates/v4l2-sys-mit |
| `vello_common` | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| `vello_cpu` | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| `version_check` | MIT OR Apache-2.0 | https://github.com/SergioBenitez/version_check |
| `version-compare` | MIT | https://gitlab.com/timvisee/version-compare |
| `walkdir` | Unlicense OR MIT | https://github.com/BurntSushi/walkdir |
| `wasi` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/wasi |
| `wasip2` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/wasi-rs |
| `wasm-bindgen` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen |
| `wasm-bindgen-futures` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures |
| `wasm-bindgen-macro` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro |
| `wasm-bindgen-macro-support` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support |
| `wasm-bindgen-shared` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared |
| `wayland-backend` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-client` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-csd-frame` | MIT | https://github.com/rust-windowing/wayland-csd-frame |
| `wayland-cursor` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-plasma` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-wlr` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-scanner` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-sys` | MIT | https://github.com/smithay/wayland-rs |
| `web-sys` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys |
| `web-time` | MIT OR Apache-2.0 | https://github.com/daxpedda/web-time |
| `weezl` | MIT OR Apache-2.0 | https://github.com/image-rs/weezl |
| `wgpu` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-core` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-core-deps-apple` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-core-deps-emscripten` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-core-deps-wasm` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-core-deps-windows-linux-android` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-hal` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-naga-bridge` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-types` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `which` | MIT | https://github.com/harryfei/which-rs |
| `winapi` | MIT OR Apache-2.0 | https://github.com/retep998/winapi-rs |
| `winapi-i686-pc-windows-gnu` | MIT OR Apache-2.0 | https://github.com/retep998/winapi-rs |
| `winapi-util` | Unlicense OR MIT | https://github.com/BurntSushi/winapi-util |
| `winapi-x86_64-pc-windows-gnu` | MIT OR Apache-2.0 | https://github.com/retep998/winapi-rs |
| `windows` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_aarch64_gnullvm` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_aarch64_msvc` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_i686_gnu` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_i686_gnullvm` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_i686_msvc` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_x86_64_gnu` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_x86_64_gnullvm` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows_x86_64_msvc` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-collections` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-core` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-future` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-implement` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-interface` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-link` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-numerics` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-result` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-strings` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-sys` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-targets` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `windows-threading` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `winit` | Apache-2.0 | https://github.com/rust-windowing/winit |
| `winnow` | MIT | https://github.com/winnow-rs/winnow |
| `wit-bindgen` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/wit-bindgen |
| `wl-clipboard-rs` | MIT OR Apache-2.0 | https://github.com/YaLTeR/wl-clipboard-rs |
| `write16` | Apache-2.0 OR MIT | https://github.com/hsivonen/write16 |
| `writeable` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `x11-dl` | MIT | https://github.com/AltF02/x11-rs |
| `x11rb` | MIT OR Apache-2.0 | https://github.com/psychon/x11rb |
| `x11rb-protocol` | MIT OR Apache-2.0 | https://github.com/psychon/x11rb |
| `xcursor` | MIT | https://github.com/esposm03/xcursor-rs |
| `xkbcommon-dl` | MIT | https://github.com/rust-windowing/xkbcommon-dl |
| `xkeysym` | MIT OR Apache-2.0 OR Zlib | https://github.com/notgull/xkeysym |
| `xml-rs` | MIT | https://github.com/kornelski/xml-rs |
| `y4m` | MIT | https://github.com/image-rs/y4m |
| `yazi` | Apache-2.0 OR MIT | https://github.com/dfrg/yazi |
| `yoke` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `yoke-derive` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zbus` | MIT | https://github.com/z-galaxy/zbus/ |
| `zbus_macros` | MIT | https://github.com/z-galaxy/zbus/ |
| `zbus_utils` | MIT | https://github.com/z-galaxy/zbus/ |
| `zcheapstr` | MIT | https://github.com/z-galaxy/zcheapstr/ |
| `zeno` | Apache-2.0 OR MIT | https://github.com/dfrg/zeno |
| `zerocopy` | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| `zerocopy-derive` | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| `zerofrom` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerofrom-derive` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerotrie` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerovec` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerovec-derive` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zlib-rs` | Zlib | https://github.com/trifectatechfoundation/zlib-rs |
| `zmij` | MIT | https://github.com/dtolnay/zmij |
| `zruntime` | MIT | https://github.com/z-galaxy/zruntime |
| `zune-core` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image |
| `zune-inflate` | MIT OR Apache-2.0 OR Zlib | https://crates.io/crates/zune-inflate |
| `zune-jpeg` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |

## Notes

- CC BY-NC-SA 3.0 requires attribution (preserved above and in the file
  headers), prohibits commercial use, and requires derivatives to be shared
  under the same license. CC BY-NC-SA 4.0 adds the same core terms with the
  4.0 legal code.
- If you are the author of any listed work and want different licensing
  terms, or believe a listed license is incorrect, please open an issue on
  the repository tracker.
