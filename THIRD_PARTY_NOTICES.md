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

MilkDrop `.mlk` presets are not bundled: user-supplied presets are read in place and never redistributed by VmodV.
See [docs/milkdrop-presets.md](docs/milkdrop-presets.md) for where user presets
are loaded from and what that means when redistributing a build.

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

## Ethereios ISF shader import

The 41 shaders below were imported from [Ethereios/ISF-shaders](https://github.com/Ethereios/ISF-shaders) and are **not** MIT-licensed.
Upstream is contradictory: its `LICENSE` is CC0-1.0, while its README says the shaders are released under a Creative Commons non-commercial license and that they were "heavily customized from shadertoy". Because Shadertoy shaders with no explicit license default to CC BY-NC-SA 3.0 and a derivative cannot be less restrictive than its source, the conservative reading applies to the whole set: **CC BY-NC-SA 3.0**, with attribution to the adapter/publisher Ethereios (Sleepless Monk) and to the original authors where the upstream file names them. Commercial use is not permitted and modifications must stay under the same license. Each imported `.fs` repeats this note and its source in-file.

| File | Upstream source | Original author(s) | License |
| --- | --- | --- | --- |
| `resources/isf/Fork of 4th eye.fs` (with `Fork of 4th eye.vs` sidecar) | [`Fork of 4th eye.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/Fork%20of%204th%20eye.zip); source shader https://www.shadertoy.com/view/lX3cDB | sleeplessmonk | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/Neon psy.fs` (with `Neon psy.vs` sidecar) | [`Neon psy.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/Neon%20psy.zip); source shader https://www.shadertoy.com/view/DlVSWD | LydianLights | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/Rotating circles.fs` (with `Rotating circles.vs` sidecar) | [`Rotating circles.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/Rotating%20circles.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/Tryptamine visions (sleeplessmonk original).fs` (with `Tryptamine visions (sleeplessmonk original).vs` sidecar) | [`Tryptamine visions (sleeplessmonk original).zip`](https://github.com/Ethereios/ISF-shaders/blob/main/Tryptamine%20visions%20%28sleeplessmonk%20original%29.zip); original source not stated upstream | sleeplessmonk | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/bloom.fs` (with `bloom.vs` sidecar) | [`bloom.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/bloom.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/bulb volumetrics.fs` (with `bulb volumetrics.vs` sidecar) | [`bulb volumetrics.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/bulb%20volumetrics.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/color fractal nayk.fs` (with `color fractal nayk.vs` sidecar) | [`color fractal nayk.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/color%20fractal%20nayk.zip); original source not stated upstream | nayk (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/day 15 jeyko.fs` (with `day 15 jeyko.vs` sidecar) | [`day 15 jeyko.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/day%2015%20jeyko.zip); original source not stated upstream | jeyko (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/deepseek r1 audiovisual.fs` (with `deepseek r1 audiovisual.vs` sidecar) | [`deepseek r1 audiovisual.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/deepseek%20r1%20audiovisual.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/diatribes -water cavern mod.fs` (with `diatribes -water cavern mod.vs` sidecar) | [`diatribes -water cavern mod.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/diatribes%20-water%20cavern%20mod.zip); original source not stated upstream | diatribes (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/drostled.fs` (with `drostled.vs` sidecar) | [`drostled.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/drostled.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/evvvvil dna tracer.fs` (with `evvvvil dna tracer.vs` sidecar) | [`evvvvil dna tracer.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/evvvvil%20dna%20tracer.zip); source shader https://www.shadertoy.com/view/WtfBzX | evvvvil | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/evvvvil frankenslug.fs` (with `evvvvil frankenslug.vs` sidecar) | [`evvvvil frankenslug (1).zip`](https://github.com/Ethereios/ISF-shaders/blob/main/evvvvil%20frankenslug%20%281%29.zip); source shader https://www.shadertoy.com/view/3dtcDB | evvvvil | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/fol mod.fs` (with `fol mod.vs` sidecar) | [`fol mod.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/fol%20mod.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/fork phantom star (nikolaerceg).fs` (with `fork phantom star (nikolaerceg).vs` sidecar) | [`fork phantom star (nikolaerceg).zip`](https://github.com/Ethereios/ISF-shaders/blob/main/fork%20phantom%20star%20%28nikolaerceg%29.zip); source shader https://www.shadertoy.com/view/7dBSR3 | NikolaErceg | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/gamma xor.fs` (with `gamma xor.vs` sidecar) | [`gamma xor.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/gamma%20xor.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/iq - line fractal tunnel.fs` (with `iq - line fractal tunnel.vs` sidecar) | [`iq - line fractal tunnel.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/iq%20-%20line%20fractal%20tunnel.zip); original source not stated upstream | Inigo Quilez (named in the upstream description) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/julia.fs` (with `julia.vs` sidecar) | [`julia.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/julia.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/kabbalistic fractal circles.fs` (with `kabbalistic fractal circles.vs` sidecar) | [`kabbalistic fractal circles.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/kabbalistic%20fractal%20circles.zip); source shader https://www.shadertoy.com/view/X33yRB | ekaunt | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/kali electromagnetic politics.fs` (with `kali electromagnetic politics.vs` sidecar) | [`kali electromagnetic politics.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/kali%20electromagnetic%20politics.zip); original source not stated upstream | Kali (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/kali inversion machine.fs` (with `kali inversion machine.vs` sidecar) | [`kali inversion machine.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/kali%20inversion%20machine.zip); original source not stated upstream | Kali (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/kali liberation of the true self.fs` (with `kali liberation of the true self.vs` sidecar) | [`kali liberation of the true self.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/kali%20liberation%20of%20the%20true%20self.zip); original source not stated upstream | Kali (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/kali psychoid mod.fs` (with `kali psychoid mod.vs` sidecar) | [`kali psychoid mod.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/kali%20psychoid%20mod.zip); original source not stated upstream | Kali (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/kali the artist parody.fs` (with `kali the artist parody.vs` sidecar) | [`kali the artist parody.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/kali%20the%20artist%20parody.zip); original source not stated upstream | Kali (named in the upstream description) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/mrange - colorful failure.fs` (with `mrange - colorful failure.vs` sidecar) | [`mrange - colorful failure (1).zip`](https://github.com/Ethereios/ISF-shaders/blob/main/mrange%20-%20colorful%20failure%20%281%29.zip); original source not stated upstream | mrange (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/nayk spherical remake.fs` (with `nayk spherical remake.vs` sidecar) | [`nayk spherical remake.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/nayk%20spherical%20remake.zip); original source not stated upstream | nayk (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/neon psychedelic lydianlights mod.fs` (with `neon psychedelic lydianlights mod.vs` sidecar) | [`neon psychedelic lydianlights mod.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/neon%20psychedelic%20lydianlights%20mod.zip); original source not stated upstream | LydianLights (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/nested octahedron modded.fs` (with `nested octahedron modded.vs` sidecar) | [`nested octahedron modded.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/nested%20octahedron%20modded.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/phreax blue lotus.fs` (with `phreax blue lotus.vs` sidecar) | [`phreax blue lotus.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/phreax%20blue%20lotus.zip); source shader https://www.shadertoy.com/view/Xds3zN | tdhooper ('Echeveria', named in the upstream description); phreax | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/phreax butterfly effect.fs` (with `phreax butterfly effect.vs` sidecar) | [`phreax butterfly effect.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/phreax%20butterfly%20effect.zip); original source not stated upstream | phreax (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/phreax hexflower.fs` (with `phreax hexflower.vs` sidecar) | [`phreax hexflower.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/phreax%20hexflower.zip); source shader https://www.shadertoy.com/view/tX23Dy | phreax (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/phreax portal.fs` (with `phreax portal.vs` sidecar) | [`phreax portal.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/phreax%20portal.zip); original source not stated upstream | phreax (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/phreax twisted carbon.fs` (with `phreax twisted carbon.vs` sidecar) | [`phreax twisted carbon (1).zip`](https://github.com/Ethereios/ISF-shaders/blob/main/phreax%20twisted%20carbon%20%281%29.zip); source shader https://www.shadertoy.com/view/Nsd3Wl | phreax (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/polyhedral.fs` (with `polyhedral.vs` sidecar) | [`polyhedral.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/polyhedral.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/posthelios sky is sinus.fs` (with `posthelios sky is sinus.vs` sidecar) | [`posthelios sky is sinus.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/posthelios%20sky%20is%20sinus.zip); source shader https://iquilezles.org/articles/palettes/ | posthelios (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/rhodium fractalscape virgil.fs` (with `rhodium fractalscape virgil.vs` sidecar) | [`rhodium fractalscape virgil.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/rhodium%20fractalscape%20virgil.zip); source shader http://www.pouet.net/prod.php?which=68239 | Virgill / Rhodium (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/spiral riders kali.fs` (with `spiral riders kali.vs` sidecar) | [`spiral riders kali.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/spiral%20riders%20kali.zip); original source not stated upstream | Kali (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/tail of the dragon - diatribes.fs` (with `tail of the dragon - diatribes.vs` sidecar) | [`tail of the dragon - diatribes.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/tail%20of%20the%20dragon%20-%20diatribes.zip); original source not stated upstream | diatribes (named in the upstream title) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/torus thingy 2.fs` (with `torus thingy 2.vs` sidecar) | [`torus thingy 2.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/torus%20thingy%202.zip); original source not stated upstream | bal-khan (stated in the upstream CREDIT field) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/waves.fs` (with `waves.vs` sidecar) | [`waves (1).zip`](https://github.com/Ethereios/ISF-shaders/blob/main/waves%20%281%29.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |
| `resources/isf/xor ascend.fs` (with `xor ascend.vs` sidecar) | [`xor ascend.zip`](https://github.com/Ethereios/ISF-shaders/blob/main/xor%20ascend.zip); original source not stated upstream | not stated upstream; adapter Ethereios (Sleepless Monk) | CC BY-NC-SA 3.0 (conservative reading of the conflicting upstream LICENSE/README) |

`phreax twisted carbon` had its unused `IMPORTED` cube-map block dropped: the upstream archive ships only one of six faces and the `textureCube` sampling is commented out in the upstream shader. The evvvvil shaders' declared JPEG is bundled under its hashed filename. Three shaders needed small WGSL-translation fixes, recorded in their in-file notes: `diatribes -water cavern mod` (matrix-first rotation instead of `*=`), `phreax blue lotus` (replaced the unsupported Shadertoy `MOUSE` uniform with its declared `iMouse` input) and `phreax twisted carbon` (removed unsupported GLSL precision qualifiers/statements; temporaries for `inout` calls with `p.xy`/`p.z`) and `phreax hexflower` (a float temporary instead of passing `p.z` to an `inout` parameter, which WGSL forbids). All other shader bodies are the upstream text with only the ISF `CREDIT` field and attribution comment added.

## glslop CC0 shader imports

The six shaders below were imported verbatim from [glslop](https://glslop.com/) via its public
`/api/v1/shaders/<id>` endpoint. glslop publishes its shader commons under CC0-1.0, so no
attribution is required; the upstream page, author and in-file `CREDIT` are recorded here for
provenance. `REC` is a two-pass ISF shader; it translates and runs through the normal ISF
multi-pass path without an override.

| File | Upstream | Author / credit | Licence |
| --- | --- | --- | --- |
| `resources/isf/Skrrt.fs` | [ouq919a0](https://glslop.com/shader/ouq919a0) | ksin — "CC0. Original. An homage - no marque assets, every number typed in by hand." | CC0-1.0 |
| `resources/isf/REC.fs` | [e5nbqrtg](https://glslop.com/shader/e5nbqrtg) | ksin — "CC0" | CC0-1.0 |
| `resources/isf/neptune.exe.fs` | [8wbksuit](https://glslop.com/shader/8wbksuit) | ksin — "CC0" | CC0-1.0 |
| `resources/isf/Not Responding.fs` | [k0xfhmb2](https://glslop.com/shader/k0xfhmb2) | ksin — "CC0" | CC0-1.0 |
| `resources/isf/Orbiting Prism Cluster.fs` | [07m1ixly](https://glslop.com/shader/07m1ixly) | glslop agent (Claude) | CC0-1.0 |
| `resources/isf/Laser Lattice Wormhole.fs` | [720112cj](https://glslop.com/shader/720112cj) | glslop agent (Claude) | CC0-1.0 |

## Point10 scenes

The 51 registered Point10-derived scenes do **not** share one provenance. Only
the thirty-three Plane9 (Joakim Dahl) presets are share-alike; they register as
`Plane9 <Name>` (the historical `Point10 <Name>` names remain load aliases for
the twelve renamed ports).
The three Hollow Cuboid/Layers/Blur variants re-parameterise the Plane9
"Hollow Cube" preset and stay share-alike too. The three.js/CodePen-derived
scenes are MIT, and the visuals written for Point10 are first-party MIT. The
licences below are keyed to the scene name suffix and are what the module info
panel shows (the labels are assembled in `crates/modv-ui/src/model.rs`; that
first-party file is named here because the licence-marker scan finds the label
strings, not because it contains licensed source). Glass Lens is not shipped:
its upstream CodePen states no licence.

### Plane9 ports (Joakim Dahl — CC BY-NC-SA)

These adapt Plane9 presets by **Joakim Dahl**. The version is stated in each
ported file; the "Around And Around" preset behind Around Plus does not state
one, so only the share-alike terms (not a version) are asserted for it. The
Plane9-specific helpers in `point10/shared` (`p9_glsl`, `p9_shading`) are
covered by these terms when used by these scenes. The Cave additionally derives
from BoyC's Shadertoy "The Cave" (https://www.shadertoy.com/view/MsX3RH).

| Scene | Module file | License |
| --- | --- | --- |
| Boxy | `crates/modv-renderers/src/point10/scenes/boxy.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Color Cube | `crates/modv-renderers/src/point10/scenes/color_cube.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Earth | `crates/modv-renderers/src/point10/scenes/earth.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Hollow Cube | `crates/modv-renderers/src/point10/scenes/hollow_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Hollow Cuboid | `crates/modv-renderers/src/point10/scenes/hollow_cube.rs` | Point10 variant of the Plane9 "Hollow Cube" preset by Joakim Dahl — CC BY-NC-SA 4.0 (stated in file) |
| Hollow Cube Layers | `crates/modv-renderers/src/point10/scenes/hollow_cube.rs` | Point10 variant of the Plane9 "Hollow Cube" preset by Joakim Dahl — CC BY-NC-SA 4.0 (stated in file) |
| Hollow Cube Blur | `crates/modv-renderers/src/point10/scenes/hollow_cube.rs` | Point10 variant of the Plane9 "Hollow Cube" preset by Joakim Dahl — CC BY-NC-SA 4.0 (stated in file) |
| It Is Coming For You | `crates/modv-renderers/src/point10/scenes/it_is_coming_for_you.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Light Tunnel | `crates/modv-renderers/src/point10/scenes/light_tunnel.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Mirror Cube | `crates/modv-renderers/src/point10/scenes/mirror_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Sound Cube | `crates/modv-renderers/src/point10/scenes/sound_cube.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Square Tunnel | `crates/modv-renderers/src/point10/scenes/square_tunnel.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Sun | `crates/modv-renderers/src/point10/scenes/sun.rs` | CC BY-NC-SA 3.0 (stated in file) |
| The Cave | `crates/modv-renderers/src/point10/scenes/the_cave.rs` | CC BY-NC-SA 3.0 (Plane9 preset by Joakim Dahl, itself based on BoyC's Shadertoy "The Cave") |
| Wormhole | `crates/modv-renderers/src/point10/scenes/wormhole.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Around Plus | `crates/modv-renderers/src/point10/scenes/around_plus.rs` | Rust/wgpu port by this repository's author of the Plane9 "Around And Around" preset by Joakim Dahl; the original **CC BY-NC-SA** licence and attribution still bind the derivative |
| Blob Fade | `crates/modv-renderers/src/point10/scenes/blob_fade.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Circle Crossover | `crates/modv-renderers/src/point10/scenes/circle_crossover.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Colors | `crates/modv-renderers/src/point10/scenes/colors.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Gradient | `crates/modv-renderers/src/point10/scenes/gradient.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Noise Warp | `crates/modv-renderers/src/point10/scenes/noise_warp.rs` | CC BY-NC-SA 4.0 (stated in file; after Inigo Quilez's domain-warping article) |
| Rorschach | `crates/modv-renderers/src/point10/scenes/rorschach.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Though The Noise | `crates/modv-renderers/src/point10/scenes/though_the_noise.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Truchet Tentacles | `crates/modv-renderers/src/point10/scenes/truchet_tentacles.rs` | CC BY-NC-SA 3.0 (stated in file; after WAHa_06x36's Shadertoy, https://www.shadertoy.com/view/ldfGWn) |
| Tv Noise | `crates/modv-renderers/src/point10/scenes/tv_noise.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Xor | `crates/modv-renderers/src/point10/scenes/xor.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Abstract Clouds | `crates/modv-renderers/src/point10/scenes/abstract_clouds.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Magic Smoke Box | `crates/modv-renderers/src/point10/scenes/magic_smoke_box.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Color Blob | `crates/modv-renderers/src/point10/scenes/color_blob.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Stargate Event Horizon | `crates/modv-renderers/src/point10/scenes/stargate_event_horizon.rs` | CC BY-NC-SA 3.0 (stated in file) |
| Color Beams | `crates/modv-renderers/src/point10/scenes/color_beams.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Mirror Tunnel | `crates/modv-renderers/src/point10/scenes/mirror_tunnel.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Cloud Flight | `crates/modv-renderers/src/point10/scenes/cloud_flight.rs` | CC BY-NC-SA 3.0 (stated in file; after Inigo Quilez's Shadertoy "XslGRr") |
| Spiky | `crates/modv-renderers/src/point10/scenes/spiky.rs` | CC BY-NC-SA 4.0 (stated in file) |
| Evil Box | `crates/modv-renderers/src/point10/scenes/evil_box.rs` | CC BY-NC-SA 4.0 (stated in file; Joakim Dahl, "Light trying to escape an evil box") |
| Music Waves | `crates/modv-renderers/src/point10/scenes/music_waves.rs` | CC BY-NC-SA 4.0 (stated in file; Joakim Dahl, "Music that creates waves") |
| Hypno Toad | `crates/modv-renderers/src/point10/scenes/hypno_toad.rs` | CC BY-NC-SA 3.0 (stated in file; Joakim Dahl, "Dark yet, colorfull and hypnotic") |

### Plane9 effects (Joakim Dahl — CC BY-NC-SA)

These forty-nine shader effects and ISF transitions are Plane9 presets by
**Joakim Dahl**, audited as Plane9 originals and registered as `Plane9 <Name>`
(the historical `Point10 <Name>` names remain load aliases). The version is
stated in the ported source or notices below. The five effects without a
Plane9 preset (`Pixelation`, `RGB Shift`, `Digital Glitch`, `Vignette`,
`Bloom`) keep their `Point10 ` names and `Point10` author.

| Effect | Source | License |
| --- | --- | --- |
| Gray | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Invert | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Posterization | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Film Grain | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| BlackAndWhite | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Gray Stripes | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Invert Gray | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Crosshatching | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Blocks | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| 3d Blocks | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 3.0 (Plane9 shader effect by Joakim Dahl) |
| Bayer 8 Matrix Dithering BW | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 3.0 (Plane9 shader effect by Joakim Dahl) |
| Bayer Matrix Dithering Color | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 3.0 (Plane9 shader effect by Joakim Dahl) |
| Fuzzy Pixels | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Fractured | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Bathroom Window | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Contrast | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 3.0 (Plane9 shader effect by Joakim Dahl) |
| Color Distort | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Crt | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl; the CRT post-process adapts Timothy Lottes' public domain "CRT" shader) |
| Cubic Lens Distortion | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 3.0 (Plane9 shader effect by Joakim Dahl; lens-distortion reference from a public CodePen sample (https://codepen.io)) |
| Hexagons | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Limit Hue | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Embos Merge | `crates/modv-renderers/src/shader/modules/point10_effects.rs` | CC BY-NC-SA 4.0 (Plane9 shader effect by Joakim Dahl) |
| Banded Swirl | `resources/isf/Plane9 Banded Swirl.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Blinds | `resources/isf/Plane9 Blinds.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Block | `resources/isf/Plane9 Block.fs` | CC BY-NC-SA 4.0 (Plane9 ISF transition by Joakim Dahl) |
| Circle Reveal | `resources/isf/Plane9 Circle Reveal.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Circle Stretch | `resources/isf/Plane9 Circle Stretch.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Clock Swipe | `resources/isf/Plane9 Clock Swipe.fs` | CC BY-NC-SA 4.0 (Plane9 ISF transition by Joakim Dahl) |
| Close Horizontal | `resources/isf/Plane9 Close Horizontal.fs` | CC BY-NC-SA 4.0 (Plane9 ISF transition by Joakim Dahl) |
| Crossfade | `resources/isf/Plane9 Crossfade.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Crumble | `resources/isf/Plane9 Crumble.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Disolve | `resources/isf/Plane9 Disolve.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Dropfade | `resources/isf/Plane9 Dropfade.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Erode | `resources/isf/Plane9 Erode.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Line Reveal | `resources/isf/Plane9 Line Reveal.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Lines | `resources/isf/Plane9 Lines.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Pixelate | `resources/isf/Plane9 Pixelate.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Pour | `resources/isf/Plane9 Pour.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Radial Blur | `resources/isf/Plane9 Radial Blur.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Random Circle | `resources/isf/Plane9 Random Circle.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Ripple | `resources/isf/Plane9 Ripple.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Shockwave | `resources/isf/Plane9 Shockwave.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Shrink | `resources/isf/Plane9 Shrink.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| SlideIn | `resources/isf/Plane9 SlideIn.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Smooth Swirl Grid | `resources/isf/Plane9 Smooth Swirl Grid.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Swirl | `resources/isf/Plane9 Swirl.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Swirl Grid | `resources/isf/Plane9 Swirl Grid.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Water | `resources/isf/Plane9 Water.fs` | CC BY-NC-SA 3.0 (Plane9 ISF transition by Joakim Dahl) |
| Wave | `resources/isf/Plane9 Wave.fs` | CC BY-NC-SA 4.0 (Plane9 ISF transition by Joakim Dahl) |
### three.js examples and other permissive sources (MIT)

These scenes are adaptations of permissively licensed third-party demos, not
Plane9 presets. Attribution to the upstream author/source is retained here and
in the ported file headers. Attractor Particles in particular is a port of the
three.js compute-attractors example, not a Plane9 preset.

| Scene | Module file | Upstream source | License |
| --- | --- | --- | --- |
| Attractor Particles | `crates/modv-renderers/src/point10/scenes/attractor_particles.rs` | three.js `webgpu_tsl_compute_attractors_particles` example | MIT (three.js) |
| Rolling Fire | `crates/modv-renderers/src/point10/scenes/rolling_fire.rs` | three.js `webgpu_volume_fire` example | MIT (three.js) |
| Ball Of Fire | `crates/modv-renderers/src/point10/scenes/rolling_fire.rs` | frozen variant of the same three.js `webgpu_volume_fire` port | MIT (three.js) |
| Smoke Room | `crates/modv-renderers/src/point10/scenes/smoke_room.rs` | minimal fork of RollingFire's three.js-derived volume simulation | MIT (three.js lineage) |
| DVD Room | `crates/modv-renderers/src/point10/scenes/dvd_room.rs` | inherits Smoke Room's three.js-derived simulation; DVD logo is public domain | MIT (three.js lineage); logo public domain |
| Ultra HDR | `crates/modv-renderers/src/point10/scenes/ultra_hdr.rs` | three.js "Ultra HDR" example | MIT (three.js) |
| Mycelium | `crates/modv-renderers/src/point10/scenes/mycelium.rs` | E. T. Carter's `shader.gallery/mycelium` fragment shader | MIT (stated upstream) |
| Chromatic Line | `crates/modv-renderers/src/point10/chromatic/line.rs` | CodePen "GLSL Sample027" (yLgYpWM) by Web_yuki1027 | MIT (stated in upstream header) |
| Chromatic Wave | `crates/modv-renderers/src/point10/chromatic/wave.rs` | same CodePen "GLSL Sample027" look by Web_yuki1027 | MIT (stated in upstream header) |
| Water Ripple | `crates/modv-renderers/src/point10/scenes/water_ripple.rs` | CodePen "[threejs/shaders] Water Ripple Effect with Audio" (https://codepen.io/filipz/pen/WbvmvBM) by Filip Zrnzevic; wave simulation from `jeantimex/threejs-water` | MIT (CodePen license page and threejs-water upstream) |
| Shared postfx passes | `crates/modv-renderers/src/point10/postfx/afterimage.rs`, `output_pass.rs`, `unreal_bloom_pass.rs` | three.js `AfterimagePass`, `OutputPass`, `UnrealBloomPass` examples | MIT (three.js) |
| Shared wave simulation | `crates/modv-renderers/src/point10/shared/water_wave.rs` | `jeantimex/threejs-water` (itself after Evan Wallace's WebGL Water) | MIT (stated upstream) |

### First-party Point10 visuals (MIT)

These visuals were written for Point10 by this repository's author or an agent
acting on their behalf, with no third-party shader source, and are covered by
VmodV's MIT License.

| Scene | Module file | License |
| --- | --- | --- |
| Latent Backrooms | `crates/modv-renderers/src/point10/scenes/latent_backrooms.rs` | MIT (first-party) |
| Particle Field | `crates/modv-renderers/src/point10/scenes/particle_field.rs` | MIT (first-party) |
| Spiral Particles | `crates/modv-renderers/src/point10/scenes/spiral_particles.rs` | MIT (first-party) |
| Waveform | `crates/modv-renderers/src/point10/scenes/waveform.rs` | MIT (first-party) |

## Bundled assets

| File | Source | Author | License |
| --- | --- | --- | --- |
| `resources/point10/textures/dvd_logo.svg` | Wikimedia Commons DVD text logo, extruded by the DvdRoom scene | Wikimedia Commons contributors | public domain (stated in the ported scene) |
| `resources/point10/textures/steel_diffuse.jpg` | Steel diffuse texture shipped with the Plane9 "The Cave" preset in `../Point10/public/plane9/data/` | not recorded upstream | source licence unverified — treated as **not MIT**; bundled only with the CC BY-NC-SA scene port |

Other Point10 textures use ambientCG assets (CC0; e.g. `tinore`, `rubber002`,
`moon_04`, `concrete044c`), Poly Haven skies (`stormy`, `spruit_sunrise`), or
NASA-derived Earth/Milky Way imagery. Where an asset's provenance or license
is unclear it is not relicensed under MIT.

## MilkDrop user presets (not bundled)

No MilkDrop preset corpus is shipped with VmodV. User-supplied `.mlk` presets
are read in place from the user's media folders and are never copied into the
installer, AppImage or `.deb`. The only `.mlk` file in this tree is a test
fixture that exists solely so the NC-SA scanner above is exercised for the
MilkDrop format; it is not shipped as a preset.

| Fixture | Source | Author | License |
| --- | --- | --- | --- |
| `crates/modv-app/tests/fixtures/notices/milkdrop_ncsa.mlk` | [NC-SA Header Fixture](https://example.org/presets/nc-sa-header-fixture.mlk) (test fixture, not bundled) | Ada Lovelace | CC BY-NC-SA 3.0 (stated in header) |

Real users' MilkDrop presets stay under their own licenses; the attribution and
share-alike duties of any preset you load or share remain yours — see
[docs/milkdrop-presets.md](docs/milkdrop-presets.md).

## Fonts

The VmodV shell (`crates/modv-ui/assets/fonts/`) bundles four IBM Plex faces:
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
| `console_error_panic_hook` | MIT OR Apache-2.0 | https://github.com/rustwasm/console_error_panic_hook |
| `cookie-factory` | MIT | https://github.com/rust-bakery/cookie-factory |
| `core-foundation` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `core-foundation-sys` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `core-graphics` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `core-graphics-types` | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| `core_maths` | MIT | https://github.com/robertbastian/core_maths |
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
| `form_urlencoded` | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
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
| `idna` | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
| `idna_adapter` | Apache-2.0 OR MIT | https://github.com/hsivonen/idna_adapter |
| `image` | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| `image-webp` | MIT OR Apache-2.0 | https://github.com/image-rs/image-webp |
| `imgref` | CC0-1.0 OR Apache-2.0 | https://github.com/kornelski/imgref |
| `include_dir` | MIT | https://github.com/Michael-F-Bryan/include_dir |
| `include_dir_macros` | MIT | https://github.com/Michael-F-Bryan/include_dir |
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
| `smithay-clipboard` | MIT | https://github.com/Smithay/smithay-clipboard |
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
| `unicode-general-category` | Apache-2.0 | https://github.com/yeslogic/unicode-general-category |
| `unicode-ident` | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| `unicode-linebreak` | Apache-2.0 | https://github.com/axelf4/unicode-linebreak |
| `unicode-properties` | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/unicode-rs/unicode-properties |
| `unicode-script` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-script |
| `unicode-segmentation` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-segmentation |
| `unicode-width` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-width |
| `unicode-xid` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-xid |
| `url` | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
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
| `wayland-protocols-experimental` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-misc` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-plasma` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-wlr` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-scanner` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-sys` | MIT | https://github.com/smithay/wayland-rs |
| `web-sys` | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys |
| `web-time` | MIT OR Apache-2.0 | https://github.com/daxpedda/web-time |
| `webbrowser` | MIT OR Apache-2.0 | https://github.com/amodm/webbrowser-rs |
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
| `xxhash-rust` | BSL-1.0 | https://github.com/DoumanAsh/xxhash-rust |
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
