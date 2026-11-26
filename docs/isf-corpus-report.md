# ISF corpus translation report

The corpus gate is the checked-in baseline `crates/modv-renderers/data/isf-corpus-baseline.json` (asserted by `cargo test -p modv-renderers`; the test no longer rewrites this file). The corpus is the vendored `resources/isf`.

- Corpus directory: `resources/isf`
- Fragment shaders (`.fs`): **402**
- Translated without an override: **402** (100.0 %)
- Translated via an override: **0**
- Failed: **0**
- Total passes across the corpus: 569
- Shaders with a `PERSISTENT` buffer: 53; with a `FLOAT` buffer: 17
- Vertex sidecars (`.vs`): 77 — translated 77, failed 0

## Overrides

None.

## Failures

None.

## Notes

- A shader counts as translated only when **every** pass translates.
- Coordinate systems differ from GL: `isf_FragNormCoord` is top-left origin in wgpu, `gl_FragCoord` is top-left instead of bottom-left — unless every sampler names one of the document's own `PASSES` buffers (self-contained generators), in which case the legacy bottom-left origin is used so the image matches WebGL. Texture sampling stays visually identical; shaders that treat `isf_FragNormCoord` as a Cartesian coordinate draw bottom-left like WebGL unless they sample an externally-supplied texture.
- `bool`/`event` inputs travel as `int` uniforms copied into private `bool` globals at the top of the entry point (a `bool(...)` macro would rewrite `.r` swizzles for inputs named `r`), and `_<name>_flip` is an `int` because WGSL forbids `bool` in a uniform block.
- The last seven shaders now translate via four general source rewrites: hoisted non-constant global initialisers, per-sampler cloning of functions with `sampler2D` parameters, default returns for non-void fall-through paths, and split `matN` varyings. No override is needed for the corpus; the override mechanism remains for genuinely ambiguous third-party shaders.
- Fragment-declared `in`/`varying` lines become zero-initialised private globals: the runtime always draws the built-in full-screen triangle, so unbound sidecar varyings (the corpus' `left_coord`/`texOffsets` template leftovers) read zero instead of failing to link.
