# Module catalogue data

`modules.json` and `previews.json` are **generated files**, not hand-written
content. They are checked in so GitHub Pages can serve the site as plain
static files with no build step.

## Where the data comes from

`modules.json` is the metadata catalogue loaded on page load; WGSL is split
into `previews.json` and fetched only when a live preview opens. Both are
projections of one `build_document()` call, shared by the generator and the
catalogue guards in `notices.rs`.

| File | Source |
| --- | --- |
| module names, authors, categories, props | `ModuleRegistry` in `crates/modv-app/src/module_registry.rs` — the same registry the running app uses |
| ISF descriptions, passes, inputs | the `/*{ … }*/` headers in `resources/isf/*.fs`, parsed by `modv_renderers::isf` |
| preview WGSL | `ShaderModule::fragment_wgsl` for built-in effects, and `translate_fragment` (naga GLSL → WGSL) for each ISF shader |
| uniform layouts | `shader::uniforms::ShaderUniforms` (160-byte block) and `isf::source::uniform_layout` |

The generator lives in `crates/modv-app/src/module_docs.rs` with a thin CLI in
`crates/modv-app/src/bin/gen-module-docs.rs`. It is covered by
`crates/modv-app/tests/module_docs.rs`, which checks that every module in the
parity manifest (`tests/fixtures/module-parity.json`) is present, that IDs are
unique and URL-safe, and that every shader/ISF module ships compilable WGSL.

## Regenerating

The `wasm-bindgen-cli` version in the `wasm` Dockerfile stage is pinned to
the `wasm-bindgen` version in `crates/Cargo.lock` (currently 0.2.130); bump
both together.

Everything runs in Docker; the host needs no Rust toolchain:

```bash
make module-docs     # rewrites modules.json + previews.json
make preview-wasm    # rebuilds docs/assets/wasm/ (WASM preview runtime)
make pages-data      # both
```

`make module-docs` resolves the corpus relative to the crate, so it works from
a worktree too. Override with `--isf-dir` / `--out` when invoking the binary
directly.

## Browser previews

The native compositor is wgpu/Vulkan and cannot run in a browser. The site
uses a split runtime:

- **WebAssembly** (`crates/modv-preview-wasm`, built to `docs/assets/wasm`)
  owns the clock, a deterministic four-on-the-floor audio simulation, and the
  exact uniform byte layouts the engine uploads. Its byte offsets are pinned
  by host-side tests in `crates/modv-preview-wasm/tests/runtime.rs`.
- **WebGPU** (`docs/assets/preview.js`) compiles each module's real WGSL and
  draws it with dummy input/FFT textures. Browsers without WebGPU get a
  metadata card instead of a canvas.

The catalogue's preview shim only draws a single fragment pass, so 438 of 501
modules get a live preview. The 16 2D canvas modules and 47 Point10 scenes are
catalogued with full metadata but are not drawn by this shim; they render in
the browser shell at the site root, which runs the real compositor and registry
(shader, 2D and Point10 modules).

Multi-pass and persistent-buffer ISF shaders are marked `simplified`: the
catalogue draws pass 0 rather than replaying the full pass graph, which would
need the compositor's ping-pong targets. The browser shell at the root
embeds and registers the full bundled ISF corpus; MilkDrop presets and user
ISF folders are not loaded there yet.

## Publishing

`.github/workflows/pages.yml` builds the real browser shell on every push to
`main` and deploys it at the GitHub Pages site root through GitHub Actions
(`actions/upload-pages-artifact` + `actions/deploy-pages`), with `/modules/`
and `/app/` redirecting there. The generated catalogue JSON and preview runtime
are still rebuilt locally with `make pages-data` for inspection, but are no
longer deployed. The generated catalogue is still checked in for
`make pages-data`, but it is no longer what the site root serves.

Third-party licence information is in
[`THIRD_PARTY_NOTICES.md`](https://github.com/fayaaz/VmodV/blob/main/THIRD_PARTY_NOTICES.md).
