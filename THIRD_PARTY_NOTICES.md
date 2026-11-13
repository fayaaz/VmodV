# Third-party notices

VmodV is released under the MIT License — see [LICENSE](LICENSE). It bundles
modules and code derived from the third-party sources listed below. Unless a
file says otherwise, these works are **not** covered by the MIT license; they
remain under the license shown for them.

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
| `src/renderer/src/application/renderers/isf/modules/isf/st_Ms2SD1.fs.fs` | [Ms2SD1](https://www.shadertoy.com/view/Ms2SD1) | author not stated | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Smoke Screen.fs` | [4t2SRz](https://www.shadertoy.com/view/4t2SRz) | jackdavenport | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Zebre.fs` | [lsf3WH](https://www.shadertoy.com/view/lsf3WH) (noise functions); module by Joseph Fiola | Joseph Fiola; noise by Iñigo Quílez | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/WaveLines.fs` | [4dsGzH](https://www.shadertoy.com/view/4dsGzH) ("WAVES"); module by Joseph Fiola, remix by SilviaFabiani | Joseph Fiola / bonniem / SilviaFabiani | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/digital-crystal-tunnel.fs` | triangle noise from [4ts3z2](https://www.shadertoy.com/view/4ts3z2) (original usage noted as Xyptonjtroz; function by Nimitz); module credits WilstonOreo | WilstonOreo (module); Nimitz (noise function) | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Broken LCD.fs` | noise functions [XdXGW8](https://www.shadertoy.com/view/XdXGW8), [lsf3WH](https://www.shadertoy.com/view/lsf3WH), [4dXBRH](https://www.shadertoy.com/view/4dXBRH), [XdXBRH](https://www.shadertoy.com/view/XdXBRH), [XsXfRH](https://www.shadertoy.com/view/XsXfRH), [4dffRH](https://www.shadertoy.com/view/4dffRH), [Msf3WH](https://www.shadertoy.com/view/Msf3WH), [Xsl3Dl](https://www.shadertoy.com/view/Xsl3Dl), [4sfGzS](https://www.shadertoy.com/view/4sfGzS) (value/gradient noise, 2D/3D) | VIDVOX (module); noise by Iñigo Quílez, ported by @colin_movecraft | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Dirty Lens.fs`, `src/renderer/src/application/renderers/isf/modules/isf/Smudged Lens.fs` | same Iñigo Quílez noise functions as Broken LCD | Iñigo Quílez, ported by @colin_movecraft and VIDVOX | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/spherical-shader-tut.fs` | [XdKGWm](https://www.shadertoy.com/view/XdKGWm) | WilstonOreo (http://omnido.me) | in-file license: "Creative Commons 3.0 Share Alike" (variant not specified — treated as CC BY-SA 3.0) |
| `src/renderer/src/application/renderers/isf/modules/isf/Perlin Transition.fs` | [gl-transitions perlin.glsl](https://github.com/gl-transitions/gl-transitions/tree/master/perlin.glsl); the noise function credits [4dS3Wd](https://www.shadertoy.com/view/4dS3Wd) | Rich Harris (module); noise by Morgan McGuire | MIT (stated in file) |
| `src/renderer/src/application/renderers/isf/modules/isf/Helix Eye.fs` | [cdSGRw](https://www.shadertoy.com/view/cdSGRw) ("Fear Inoculum"); `helix_sdf` adapted from [ttB3DV](https://www.shadertoy.com/view/ttB3DV) | RagePly (module); ttB3DV author not stated | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Traced Tunnel.fs` | [tdjfDR](https://www.shadertoy.com/view/tdjfDR); the source's two photo inputs are not redistributed (licenses unknown, replaced by procedural patterns) | Shane | not stated → Shadertoy default CC BY-NC-SA 3.0 |
| `src/renderer/src/application/renderers/isf/modules/isf/Hot Shower.fs` | [4lf3Rj](https://www.shadertoy.com/view/4lf3Rj); the source's noise and bark/rock textures are not redistributed (licenses unknown, replaced by procedural value noise) | Kali | not stated → Shadertoy default CC BY-NC-SA 3.0 |

All of the above are bundled **as-is or as modified ports**; the ports remain
under the licenses listed for them and are not relicensed under the MIT
License.

## Notes

- CC BY-NC-SA 3.0 requires attribution (preserved above and in the file
  headers), prohibits commercial use, and requires derivatives to be shared
  under the same license.
- If you are the author of any listed work and want different licensing
  terms, or believe a listed license is incorrect, please open an issue on
  the repository tracker.
