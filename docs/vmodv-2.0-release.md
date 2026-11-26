# VmodV 2.0

Status: release preparation in the Rust-only tree. The Linux VmodV build
(`modv-ui` + `modv-app`) is the application; Electron, Node and the Yarn
toolchain were removed. The final 1.x/Electron tree is archived on branch
`legacy-electron` and tag `v1.x-final` (see
[legacy-electron.md](./legacy-electron.md)).

## What 2.0 is

VmodV is the single-process Rust build of modV: the engine, media hub,
audio analysis and egui UI run in one `modv` binary, with no Electron, Node or
web renderer. It reads and writes the same preset, Scene, playlist and media
documents as the archived Electron app.

Linux remains the primary 2.0 target. CI additionally produces a portable
Windows ZIP and portable macOS x86_64 (Intel) and arm64 (Apple Silicon) `.app`
ZIPs as best-effort artifacts; these are not installers and do not imply
feature parity with Linux. The archived Electron tree is a further
(unsupported) reference.

## Getting it

| Build | Artifact |
| --- | --- |
| VmodV AppImage (x86_64) | `VmodV-2.0.0-x86_64.AppImage` |
| VmodV AppImage (arm64) | `VmodV-2.0.0-aarch64.AppImage` |
| VmodV Debian package | `VmodV-2.0.0-amd64.deb` |
| Portable Windows ZIP (best effort) | `VmodV-Windows-x86_64-portable-2.0.0.zip` |
| Portable macOS x86_64 ZIP (Intel, best effort) | `VmodV-macOS-x86_64-portable-2.0.0.zip` |
| Portable macOS arm64 ZIP (Apple Silicon, best effort) | `VmodV-macOS-arm64-portable-2.0.0.zip` |

The `.deb` is package `vmodv`. It installs `modv` (plus the
`vmodv` compatibility symlink), `vmodv.desktop`, the icon, and
the ISF/texture assets under `/usr/share/vmodv`. The AppImage bundles the same
binary and assets, built natively for its architecture (`x86_64` or `aarch64`);
the `.deb` remains amd64-only.

Versions:

- The installer version is not kept in the tree. `VMODV_VERSION` (a
  make/environment variable, not a checked-in literal) names the installers;
  its default comes from the git tags via `tools/version.sh` — the tag on
  HEAD when building a tagged checkout, else the highest `vX.Y.Z` tag.
  Override it with `VMODV_VERSION=...` for local builds. `release.yml`
  passes the computed version automatically for stable releases and tags the
  released commit instead of committing a version bump (see
  `docs/releasing.md`). `make version` prints the resolved value.
- The Cargo workspace stays at its internal `0.1.0` metadata. The package
  version, not `modv --version`, is the release version.

AppImage runtime dependencies come from the host: Vulkan (or Mesa's lavapipe),
PipeWire, ALSA, the GStreamer runtime and plugins used by camera/file/screen
capture, and `libxkbcommon-x11` for the X11 fallback. The `.deb` declares them
and recommends `mesa-vulkan-drivers`.

### Portable macOS build (best effort)

`.github/workflows/macos.yml` builds portable macOS `.app` bundles with a
single matrix job covering both architectures: `macos-15-intel` (x86_64, Intel)
and `macos-15` (pinned arm64/Apple Silicon). Each leg stages, ad-hoc signs and
ZIPs its own bundle, uploading a separately named artifact —
`vmodv-macos-x86_64-portable` or `vmodv-macos-arm64-portable` —
with ZIP names `VmodV-macOS-x86_64-portable-<version>.zip` and
`VmodV-macOS-arm64-portable-<version>.zip`. The matrix uses
`fail-fast: false` so one architecture's failure does not cancel the other.
The default arm64 runner (`macos-latest`) was avoided because capacity failures
prevented the earlier run from acquiring a runner at all; both legs use the
pinned labels instead. The arm64 CI path has not been verified yet.

Both legs require Homebrew `pkg-config gstreamer gst-plugins-base` at build time
(`gstreamer-sys` locates the libraries via pkg-config) and GStreamer at runtime;
the launcher discovers the Homebrew prefix at startup. Each bundle is ad-hoc
signed with `codesign --sign -`, so a downloaded ZIP may need
`xattr -d com.apple.quarantine` on the extracted files before it will launch.

Known runtime gaps on macOS are unchanged: screen capture (portal), PipeWire
system-audio monitor capture and V4L2 camera enumeration are Linux-only;
cpal/CoreAudio input, CoreMIDI, ISF/visual rendering and Metal work.

## userData compatibility

The VmodV app uses the VmodV userData directory (`~/.config/VmodV` on Linux;
`MODV_USER_DATA` overrides it). Presets, `scenes.json`, Scenes playlists, the
`media/` profile tree and `window-state.json` keep the formats written by the
archived 1.x app. Logging lands in
`<userData>/logs/vmodv-YYYY-MM-DD.log`.

VmodV state is additive: `engine-state.json`, `vmodv-layout.json`,
`vmodv-media.json`, `vmodv-audio.json`, `vmodv-project.json` and
`vmodv-plugins.json`. Back up the userData directory before the first 2.0 run,
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
make build                 # VmodV AppImage + .deb in dist/
make appimage             # VmodV AppImage only
make vmodv-deb            # VmodV .deb (runs vmodv-deb-check.sh)
make vmodv-deb-check      # re-check an existing .deb
make docker-package        # same artifacts via the Dockerfile `artifacts` target

make dev                   # VmodV shell (release profile)
make vmodv-debug          # VmodV shell, debug profile
make engine-test           # cargo test --workspace
make test-e2e-vmodv       # real-binary e2e + parity tests
make engine-lint           # clippy -D warnings + rustfmt --check
```

`tools/vmodv-smoke.sh` exercises a packaged AppImage with a throwaway
`MODV_USER_DATA` (headless benchmark, windowed apply/restore, crash recovery
and logging).

## CI and release

- `.github/workflows/vmodv.yml` runs clippy/fmt, the workspace tests
  (`modv-app` serial), the VmodV e2e harness and both package builds on
  changes, uploading workflow artifacts. The `package` job is a two-leg native
  matrix: x86_64 on `ubuntu-latest` builds the AppImage and amd64 `.deb`, and
  aarch64 on `ubuntu-24.04-arm` builds the arm64 AppImage only.
- `.github/workflows/macos.yml` builds and ad-hoc signs portable macOS `.app`
  bundles for both x86_64 (Intel) and arm64 (Apple Silicon) via a matrix on
  every push, uploading both architecture ZIPs as CI artifacts so feature
  branches can be verified without opening a PR.
- `.github/workflows/release.yml` runs on every push to `main` that contains
  qualifying conventional commits, tags the released commit `vX.Y.Z` (no
  version-bump commit; the version comes from the tags) and publishes the
  versioned AppImage (x86_64 and aarch64) and Ubuntu `.deb`, attaching the
  portable Windows ZIP and both portable macOS architecture ZIPs to the
  `vX.Y.Z` GitHub release (see `docs/releasing.md`). If only one AppImage
  architecture builds, the surviving image is still attached; likewise for the
  macOS ZIPs. There is no Electron job and no rolling nightly.
- Stable 2.0.0 artifacts are built with `make build` (or `VMODV_VERSION`
  overridden) from a green commit.

## Residuals

- Flatpak packaging is not built.
- A clean-VM installer pass and the manual camera/NDI/audio checklist remain.
