# VmodV Native 2.0

Status: release preparation in the Rust-only tree. The Linux native build
(`modv-ui` + `modv-app`) is the application; Electron, Node and the Yarn
toolchain were removed. The final 1.x/Electron tree is archived on branch
`legacy-electron` and tag `v1.x-final` (see
[legacy-electron.md](./legacy-electron.md)).

## What 2.0 is

VmodV Native is the single-process Rust build of modV: the engine, media hub,
audio analysis and egui UI run in one `modv` binary, with no Electron, Node or
web renderer. It reads and writes the same preset, Scene, playlist and media
documents as the archived Electron app.

Linux is the only 2.0 target. Windows and macOS builds of the native shell are
not produced; the archived Electron tree is their only (unsupported) reference.

## Getting it

| Build | Artifact |
| --- | --- |
| Native AppImage | `VmodV-Native-2.0.0-x86_64.AppImage` |
| Native Debian package | `VmodV-Native-2.0.0-amd64.deb` |

The `.deb` is package `vmodv-native`. It installs `modv` (plus the
`vmodv-native` compatibility symlink), `vmodv-native.desktop`, the icon, and
the ISF/texture assets under `/usr/share/vmodv`. The AppImage bundles the same
binary and assets.

Versions:

- `NATIVE_VERSION ?= 2.0.0` in the `Makefile` names the installers. Override it
  with `NATIVE_VERSION=...`; CI appends `-nightly.<YYYYMMDD>` for rolling
  builds.
- The Cargo workspace stays at its internal `0.1.0` metadata. The package
  version, not `modv --version`, is the release version.

AppImage runtime dependencies come from the host: Vulkan (or Mesa's lavapipe),
PipeWire, ALSA, the GStreamer runtime and plugins used by camera/file/screen
capture, and `libxkbcommon-x11` for the X11 fallback. The `.deb` declares them
and recommends `mesa-vulkan-drivers`.

## userData compatibility

The native app uses the VmodV userData directory (`~/.config/VmodV` on Linux;
`MODV_USER_DATA` overrides it). Presets, `scenes.json`, Scenes playlists, the
`media/` profile tree and `window-state.json` keep the formats written by the
archived 1.x app. Logging lands in
`<userData>/logs/vmodv-YYYY-MM-DD.log`.

Native state is additive: `engine-state.json`, `native-layout.json`,
`native-media.json`, `native-audio.json`, `native-project.json` and
`native-plugins.json`. Back up the userData directory before the first 2.0 run,
as with any major upgrade.

## Plugins and ISF

- The packaged app bundles the ISF corpus and Point10 textures. `--isf-dir`,
  `MODV_ISF_DIR` and `MODV_ASSETS_DIR` still override the lookup paths.
- User ISF folders under the media tree are watched and hot-reloaded.
- JavaScript plugins are opt-in behind the sandbox consent dialog. Scripts are
  discovered in the active profile's `plugin/` folder and
  `<userData>/plugins`; the Boa sandbox exposes no DOM, filesystem or network
  globals. `modV.ws` is not wired yet, and the legacy `grab-canvas` bundle is
  not shipped.
- User-authored JavaScript 2D modules are not ported. ISF remains the
  third-party shader/module format; expressions continue to run in-engine.

## Local commands

```bash
make build                 # native AppImage + .deb in dist/
make appimage-native       # native AppImage only
make native-deb            # native .deb (runs native-deb-check.sh)
make native-deb-check      # re-check an existing .deb
make docker-package        # same artifacts via the Dockerfile `artifacts` target

make dev                   # native shell (release profile)
make native-debug          # native shell, debug profile
make engine-test           # cargo test --workspace
make test-e2e-native       # real-binary e2e + parity tests
make engine-lint           # clippy -D warnings + rustfmt --check
```

`tools/native-smoke.sh` exercises a packaged AppImage with a throwaway
`MODV_USER_DATA` (headless benchmark, windowed apply/restore, crash recovery
and logging).

## CI and release

- `.github/workflows/native.yml` runs clippy/fmt, the workspace tests
  (`modv-app` serial), the native e2e harness and both package builds on
  changes, uploading workflow artifacts.
- `.github/workflows/nightly.yml` runs the same checks on `main` and publishes
  `2.0.0-nightly.<YYYYMMDD>` AppImage/deb to the rolling `nightly`
  prerelease. There is no Electron job.
- Stable 2.0.0 artifacts are built with `make build` (or `NATIVE_VERSION`
  overridden) from a green commit.

## Residuals

- Flatpak packaging is not built.
- A clean-VM installer pass and the manual camera/NDI/audio checklist remain.
