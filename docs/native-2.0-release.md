# VmodV Native 2.0 — release preparation

Status: prepared, **not shipped**. The Linux-only native build (`modv-ui` +
`modv-app`) is now the default development entry point, but Electron stays in
the tree and remains the release fallback until the parity checklist is signed
off. Deleting `src/` is a separate, later task.

## What 2.0 is

VmodV Native is the single-process Rust build of modV: the engine, media hub,
audio analysis and egui UI run in one `modv` binary, with no Electron, Node or
web renderer. It reads and writes the same preset, Scene, playlist and media
documents as the Electron app.

Linux is the only 2.0 target. Windows and macOS keep using the Electron build.

## Getting it

| Build | Artifact |
| --- | --- |
| Native AppImage | `VmodV-Native-2.0.0-x86_64.AppImage` |
| Native Debian package | `VmodV-Native-2.0.0-amd64.deb` |
| Electron fallback AppImage | `VmodV-<version>.AppImage` |
| Electron fallback deb | `VmodV-<version>.deb` |

The `.deb` is package `vmodv-native`. It installs `modv` (plus the
`vmodv-native` compatibility symlink), `vmodv-native.desktop`, the icon, and
the ISF/texture assets under `/usr/share/vmodv`. The AppImage bundles the same
binary and assets.

Versions are independent:

- `NATIVE_VERSION ?= 2.0.0` in the `Makefile` names the native installers.
  Override it with `NATIVE_VERSION=...`; CI appends `-nightly.<YYYYMMDD>` for
  rolling builds.
- `APP_VERSION` keeps naming the Electron installers and remains the CI
  override for those builds.
- The Cargo workspace stays at its internal `0.1.0` metadata. The package
  version, not `modv --version`, is the release version.

AppImage runtime dependencies come from the host: Vulkan (or Mesa's lavapipe),
PipeWire, ALSA, the GStreamer runtime and plugins used by camera/file/screen
capture, and `libxkbcommon-x11` for the X11 fallback. The `.deb` declares them
and recommends `mesa-vulkan-drivers`.

## userData compatibility

Both builds use the same VmodV userData directory (`~/.config/VmodV` on Linux;
`MODV_USER_DATA` overrides it). Presets, `scenes.json`, Scenes playlists, the
`media/` profile tree and `window-state.json` are shared and keep their
formats. Logging still lands in `<userData>/logs/vmodv-YYYY-MM-DD.log`.

Native-only state is additive and ignored by Electron: `engine-state.json`,
`native-layout.json`, `native-media.json`, `native-audio.json`,
`native-project.json` and `native-plugins.json`. Electron-only settings are
likewise ignored by the native shell, so switching back is safe. Back up the
userData directory before the first 2.0 run, as with any major upgrade.

## Plugins and ISF

- The packaged app bundles the ISF corpus and Point10 textures. `--isf-dir`,
  `MODV_ISF_DIR` and `MODV_ASSETS_DIR` still override the lookup paths.
- User ISF folders under the media tree are watched and hot-reloaded, matching
  the Electron workflow.
- JavaScript plugins are opt-in behind the sandbox consent dialog. Scripts are
  discovered in the active profile's `plugin/` folder and
  `<userData>/plugins`; the Boa sandbox exposes no DOM, filesystem or network
  globals. `modV.ws` is not wired yet, and the legacy `grab-canvas` bundle is
  not shipped, so plugins that rely on either need a follow-up port.
- User-authored JavaScript 2D modules are not ported. ISF remains the
  third-party shader/module format; expressions continue to run in-engine.

## Local commands

```bash
make dev                   # native shell (default; release profile)
make native-debug          # native shell, debug profile
make dev-electron          # Electron UI escape hatch
MODV_UI=electron make dev  # same, via the environment switch
MODV_UI=native make dev    # explicit alias for the default
ENGINE_MODE=1 make dev     # Electron UI + Rust engine (implied Electron path)

make appimage-native       # native AppImage
make native-deb            # native .deb (runs native-deb-check.sh)
make native-deb-check      # re-check an existing .deb

make appimage              # Electron AppImage (fallback)
make build                 # Electron deb + AppImage (fallback)
```

`MODV_ENGINE`/`ENGINE_MODE` semantics are unchanged: they select the Rust
engine inside Electron. `MODV_UI=native` wins over `ENGINE_MODE=1` because the
native shell always runs the in-process engine.

## CI and release

- `.github/workflows/native.yml` keeps running clippy/fmt, the workspace tests
  (with `modv-app` serial) and both native packages on changes, uploading
  workflow artifacts.
- `.github/workflows/nightly.yml` still builds and publishes the Electron
  installers first. Its `native` job then builds
  `2.0.0-nightly.<YYYYMMDD>` AppImage/deb and uploads them to the same rolling
  `nightly` prerelease. A native failure leaves the Electron nightly intact.
- No Electron CI job has been removed. Stable 2.0.0 artifacts are built
  manually with `make appimage-native` / `make native-deb` (or `NATIVE_VERSION`
  overridden) from a green commit.

## Residuals before editing this note into a release announcement

- Electron tree deletion, and archiving the final 1.x build, wait for sign-off.
- Flatpak packaging is not built.
- `README.md` and `AGENTS.md` still describe the Electron-first workflow; they
  are deliberately unchanged in this prep task.
