# VmodV Native

<p align="center">
  <a href="https://github.com/fayaaz/modV/releases/latest">
    <img alt="GitHub release (latest by date including pre-releases)" src="https://img.shields.io/github/v/release/fayaaz/modV?color=blue&include_prereleases" />
  </a>
  <a href="https://github.com/fayaaz/modV/blob/main/LICENSE" target="_blank">
    <img alt="License: MIT (VmodV code)" src="https://img.shields.io/badge/license-MIT%20(VmodV%20code)-blue" />
  </a>
  <a href="https://github.com/fayaaz/modV/blob/main/THIRD_PARTY_NOTICES.md" target="_blank">
    <img alt="Bundled third-party modules: see notices (some CC BY-NC-SA)" src="https://img.shields.io/badge/bundled%20modules-see%20notices-orange" />
  </a>
</p>
<p align="center">
VmodV is a fork of <a href="https://github.com/vcync/modV">modV</a>: a modular audio
visualisation environment. This tree is the native Rust application — one
<code>modv</code> binary with the engine, GPU compositor, media/audio capture and
egui UI in-process.
</p>

> **License note:** VmodV's own code is MIT-licensed. It also bundles
> third-party modules and assets that are **not** MIT — several Shadertoy
> modules and the Point10/Plane9 scene ports are CC BY-NC-SA (no commercial
> use, share-alike). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for
> the full per-file attribution before redistributing or selling a build.

## What this tree is

- **Rust only.** `crates/` holds the whole application: `modv-engine`
  (state/presets/Scenes/tweens), `modv-render` + `modv-renderers` (wgpu
  compositor, shader/ISF/2D/Point10 renderers), `modv-media` (audio, GStreamer
  video, MIDI/OSC/NDI), `modv-ipc`, `modv-ui` (egui panels) and `modv-app`
  (the `modv` and `modv-engine` binaries).
- **No Node, Yarn or Electron.** The UI is egui; there is no web renderer,
  worker store or Electron main/preload process.
- **Linux is the release target.** The AppImage and `.deb` bundle the binary,
  the ISF corpus and the Point10 textures. Vulkan, PipeWire, ALSA and
  GStreamer are host runtime dependencies (the `.deb` declares them).
- The final Electron implementation is archived, not lost: see
  [docs/legacy-electron.md](docs/legacy-electron.md).

## Prerequisites

The host only needs **Docker (with Buildx)** and **make**. Rust, cargo and all
build dependencies live in the Docker toolchain image.

## Build

```bash
make build            # native AppImage + .deb in dist/
make appimage-native  # AppImage only
make native-deb       # .deb only (runs native-deb-check.sh)
make native-deb-check # re-check an existing .deb
```

`NATIVE_VERSION` names the installers (default `2.0.0`):

```bash
NATIVE_VERSION=2.0.0 make build
```

Artifacts:

- `dist/VmodV-Native-<version>-x86_64.AppImage`
- `dist/VmodV-Native-<version>-amd64.deb`

## Development

```bash
make dev          # native shell, release profile (Wayland preferred, X11 fallback)
make native-debug # native shell, debug profile
make engine-run   # same binary through the engine entry point
```

`make dev` forwards the host Wayland/X11 session, PulseAudio and `/dev/dri`
(GPU) into the container. Override with `DISPLAY_BACKEND=x11` or
`DISPLAY_BACKEND=wayland`, and pass app flags with `ARGS="..."`.

## Tests

```bash
make engine-test                                    # full workspace
make engine-test ARGS="-p modv-engine --lib"        # one package
make engine-test ARGS="-p modv-app --test notices"  # one integration test
make test-e2e-native                                # real-binary e2e + parity
make engine-lint                                    # clippy -D warnings + rustfmt
make format                                         # cargo fmt --all
```

The e2e harness launches the real `modv` binary under Xvfb (lavapipe provides
Vulkan when no hardware adapter is available). See
[docs/native-e2e.md](docs/native-e2e.md) for the control protocol and
[docs/native-parity-map.md](docs/native-parity-map.md) for coverage.

## Project layout

| Path | Purpose |
| --- | --- |
| `crates/` | the Rust workspace (engine, renderers, media, UI, app) |
| `resources/isf/` | bundled ISF shader corpus |
| `resources/point10/textures/` | Point10 texture assets |
| `packaging/native/` | AppImage/`.deb` launchers, desktop entry and control file |
| `tools/native-*.sh` | packaging, checks and the packaged-app smoke suite |
| `tests/e2e-rust/` | committed native e2e parity manifest |
| `docs/` | design, parity, release and legacy-archive notes |

## License

VmodV's own code is MIT — see [LICENSE](LICENSE). Bundled ISF shaders, Point10
scene ports, textures and Rust dependencies keep their own licences; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
