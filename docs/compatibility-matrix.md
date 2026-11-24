# Compatibility matrix

What VmodV supports today, per input family. All of this is the Linux release
target (the packaged-app toolchain is Debian trixie; Windows and macOS are not
supported). Sources point at the implementing modules; this document is the
summary a reader can trust without opening them.

---

## 1. ISF shaders — `crates/modv-renderers/src/isf/`

### Versions

| Status | Detail |
| --- | --- |
| ISF v1 and v2 | Fully supported. |
| `isf_LoopedBuffer` | VmodV-local macro extension, not part of any published ISF version; translated to WGSL (rewritten to `IMG_NORM_PIXEL(buffer, coord)`; the looped buffer is a `PERSISTENT` pass target). ISF v1 and v2 are the published spec versions VmodV supports; no published ISF v3 spec exists. |
| Detection | Explicit `ISFVSN` < 2 wins; otherwise the version is **inferred**: `1.0` when v1 markers exist (`PERSISTENT_BUFFERS` or `vv_` identifiers in the body), else `2.0`. |
| Upgrades | v1 → v2: `PERSISTENT_BUFFERS` folds into pass-level `PERSISTENT`, `vv_` identifiers are renamed. |

The vendored corpus is all v2: 312 × `"ISFVSN": "2"`, 7 × `"ISFVSN": "2.0"`,
and the other 35 `.fs` files declare no `ISFVSN` (inferred v2). The corpus
gate is 100% translated: 354 `.fs` + 36 `.vs`.

### Inputs (`INPUTS` entries)

| `TYPE` | Prop | Synthesis |
| --- | --- | --- |
| `float` | `float` | default 0.0, step 0.01, optional MIN/MAX |
| `bool` | `bool` | `Boolean(DEFAULT)` |
| `long` | `enum` | VALUES/LABELS; note: the label is `NAME` — the header's `LABEL` is ignored |
| `color` | `vec4` | DEFAULT |
| `point2D` | `vec2` | DEFAULT or `[0, 0]` |
| `image` | `texture` | sets `meta.previewWithOutput = true` |
| `audio`, `audioFFT` | `texture` | bound from the app's per-frame audio textures (the compositor uploads them each frame) |
| `event` | `event` | label only (e.g. `restartNow`) |

Other header keys: `CREDIT`→author, `DESCRIPTION`, `VSN`, `MODV_CATEGORY`
(normalised via `CATEGORY_ALIASES`).

### Passes and buffers

- `PERSISTENT`: ping-pong texture pair; the JS one-frame lag is preserved.
- `FLOAT: true`: `Rgba32Float` with nearest filtering + unfilterable sampler.
- `WIDTH`/`HEIGHT` formulas: `$WIDTH`/`$HEIGHT`/`$<input>` expansion via `evaluate_size` (minimum 1 pixel, clamped to the device-dimension limit); `CLEAR` (ISF v2 default `true`); `TARGET` names.
- An empty/`{}` final pass renders to the destination; the runtime never
  paints a pre-final buffer over the result.

### Runtime uniforms and macros

Uniforms: `PASSINDEX`, `RENDERSIZE`, `TIME`, `TIMEDELTA`, `FRAMEINDEX`,
`DATE` (`[year, month 1-based, day, secs since midnight]`), `PI`, plus per
buffer `_NAME_imgSize` / `_NAME_imgRect` (always `[0,0,1,1]`) / `_NAME_flip`.
Inputs are plain scalars/vectors except `image`/`audio`/`audioFFT`
(texture + sampler pairs).

Macros: `IMG_THIS_PIXEL`, `IMG_THIS_NORM_PIXEL`, `IMG_PIXEL`,
`IMG_NORM_PIXEL`, `IMG_SIZE` (the ISF v2 macro set), plus the VmodV-local
extension `isf_LoopedBuffer(buffer, size, coord)` (not part of any published
ISF version), rewritten to `IMG_NORM_PIXEL(buffer, coord)`.

### GLSL dialect

`naga::front::glsl` after a strict ES→desktop-GLSL-450 preprocessor
(`crates/modv-renderers/src/isf/source.rs`, `translate.rs`):

- `#version 300 es` / `100` / absent → `#version 450` (the only versions naga's GLSL front-end accepts).
- `precision` lines dropped; `texture2D`→`texture` (Lod/Grad variants too); `gl_FragColor`→`isf_FragColor`; `attribute`/`varying`→`in`/`out`; `texture2DRect`/`sampler2DRect` **rejected** (sampling in pixel space can't map onto normalised `texture()`).
- Vertex sidecars (`.vs`, 36 in corpus) are translated and drawn with the legacy six-vertex quad.
- `isf_FragNormCoord` is **top-left origin** in wgpu (matching `gl_FragCoord`); texture-free generators (raymarchers) have their `gl_FragCoord` reads Y-flipped, so they match WebGL.

### Known gaps

- Ambiguous third-party shaders can be routed through the override mechanism
  (`crates/modv-renderers/src/isf/overrides/`), but the vendored corpus needs none.

---

## 2. MilkDrop presets — `crates/modv-renderers/src/milkdrop/`

### Versions

| Status | Detail |
| --- | --- |
| MilkDrop 1 | Default. Presets without the version marker. |
| MilkDrop 2 | `MILKDROP_PRESET_VERSION=201` present (case-insensitive contains). Other values read as v1. |
| v2 metadata keys | `psversion`, `psversion_warp`, `psversion_comp`, `fRating`, `fShader`, `fPreview` are recognised but hold no state; `MILKDROP_PRESET_VERSION=201` is the only version switch. |

### Format compatibility (`crates/modv-renderers/src/milkdrop/preset.rs`)

- UTF-8 BOM tolerated before `[preset00]`; section/keys case-insensitive;
  CRLF normalised; both `.mlk` and `.milk` accepted case-insensitively.
- Real v2 files' leading backtick escapes on every `key=value` line are
  stripped (exactly one per line).
- 4 custom waves + 4 custom shapes, numbered **0..3** in real packs
  (`wavecode_0..3`, `shapecode_0..3`); equation lines `per_frame_N`,
  `per_frame_init_N`, `per_pixel_N`, `warp_N`, `comp_N` with trailing-digit
  spellings accepted.
- Legacy base vals — canonical names plus legacy spellings, as in
  `BASE_VALS` (`preset.rs`): `decay`/`fdecay`, `gammaadj`/`fgammaadj`,
  `zoom`/`fzoom`, `zoomexp`/`fZoomExponent`, `warp`/`fwarp`, `rot`/`frot`,
  `cx`/`fcx`, `cy`/`fcy`, `dx`/`fdx`, `dy`/`fdy`, `sx`/`fsx`, `sy`/`fsy`,
  `echo_zoom`/`fVideoEchoZoom`, `echo_alpha`/`fVideoEchoAlpha`,
  `echo_orient`/`nVideoEchoOrientation`, `wave_mode`/`nWaveMode`,
  `additivewave`/`bAdditiveWaves`, `wave_dots`/`bUseDots`/`bUsedots`,
  `wave_thick`, `wave_brighten`/`bMaximizeWaveColor`, `wrap`,
  `darken_center`, `red_blue`, `brighten`, `darken`, `solarize`, `invert`,
  `wave_a`/`fWaveAlpha`, `wave_r`, `wave_g`, `wave_b`, `wave_x`, `wave_y`,
  `wave_scale`, `wave_smoothing`, `wave_mystery`, `modwavealphastart`,
  `modwavealphaend`, `warpanimspeed`, `warpscale`, `mv_x`/`mv_y`
  (`nMotionVectorsX/Y`), `mv_a`, `mv_r`, `mv_g`, `mv_b`, `mv_len`,
  `ob_size`, `ob_a`, `ib_size`, `ib_a`, `b1n`/`b2n`/`b3n`,
  `brightness`/`fBrightness`, `fshader`/`shader` (legacy hue).
- Unknown base vals are recorded in `MilkDocument::diagnostics`, never
  silently dropped.
- A file without `[preset00]` is rejected (`MilkParseError::PresetSectionMissing`).

### Feature set (Butterchurn-compatible subset)

**Overlay**

- Built-in waveform, **modes 0–7** (port of Butterchurn's `basicWaveform.js`).
- Custom waves: up to 4 per preset, `bUseDots`/dots/thick/additive/spectrum,
  `sep`, `scaling`, `smoothing`, per-point colours, 512-sample limit.
- Custom shapes: up to 4 closed regular polygons (`sides`, `cx`, `cy`,
  `radius`, `rot`).
- Motion vectors: parsed and exposed as `mv_x/mv_y/…` globals — **no drawing
  path was found** in the runtime (parsed ≠ rendered).

**Noise** — `sampler_noise_lq`, `sampler_noise_mq`, `sampler_noise_hq` and the
two preset-wave fields `sampler_pw_noise_lq` / `sampler_pw_noise_hq`: five
deterministic 256×256 RGBA8 fields generated on the CPU.

**Sampling dialect** (`milkdrop/translate.rs`) — HLSL spellings bridge to GLSL
via `naga` (desktop GLSL 450):

- Macros: `tex2D`, `tex3D`, `GetMain`, `GetPixel`, `GetBlur1..3`, `lum`,
  `lerp`→`mix`, `saturate`, `frac`, `M_PI`, `M_PI_2`, `M_INV_PI_2`.
- v2 conventions: helper functions (HLSL `float2/3/4` → GLSL vector types),
  `static` globals, `shader_body { … }` split into prelude + body, bare `ret`
  declarations, redeclared noise samplers stripped.

**Globals** (warp + comp bodies; `milkdrop/translate.rs` + `runtime.rs`)

`time`, `frame`, `fps`, `progress`, `decay`, `bass`, `mid`, `treb`,
`bass_att`, `mid_att`, `treb_att`, `vol`, `rand_preset`, `rand_frame`,
`aspect_x` (shadowable `aspect` vec2), `slow_roam_cos/sin`, `roam_cos/sin`,
`hue_shader`, `scale1..3`, `bias1..3`, `blur1..3_min/max`, `texsize`,
`texsize_fw/pw/fc/pc`, `uv_orig`, `rad`, `ang`, and `q1..q32`
(`milkdrop/q_uniforms.rs`).

**Audio follows the Butterchurn relative-band contract** (`milkdrop/runtime.rs`):
FFT band sums — bass 20–320 Hz, mid 320–2800 Hz, treble 2800+ Hz — divided by
slow per-instance averages (relative to steady state ≈ 1.0, clamped to
0..=16).

**EEL2/NSEIL** (`crates/modv-renderers/src/milkdrop/eel.rs`) — the fixed
builtin set:

`int`, `floor`, `abs`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `exp`,
`sqr`, `sqrt`, `log`, `log10`, `sign`, `rand` (deterministic per-instance
RNG), `bnot`, `atan2`, `pow`, `min`, `max`, `sigmoid`, `bor`, `band`, `equal`,
`above`, `below`, `if`, `exec2`, `exec3`; the classic NSEIL helpers
`getTime` (returns the injected frame `time` global, never wall clock),
`invsqrt`, `randev`, `fadetonext`, `sample`/`samplex`/`sampley`,
`blend`/`blendavg`/`blendmax`/`blendmin`, `movement`/`movementxy`/
`spatx`/`spaty`, and `read_text`; `megabuf`/`gmegabuf`
(1,048,576 floats each, per-preset/global); user variables persist
frame-to-frame; `if/else/if`, `while`, `loop` with hard iteration caps;
assignments, comments, `q` variables.

The NSEIL host-field helpers are backed by an optional `EelExtras` attached to
the evaluation pool (`VarPool::set_extras`). With no host data they are fully
deterministic: `sample*`/`blend*`/`movement*`/`spat*`/`read_text` return `0.0`,
`fadetonext` is the identity, `invsqrt` is `1/sqrt(x)` (finite `0.0` for
non-positive input) and `randev` is a seeded Bernoulli draw. When a runtime
injects a luma field, motion field, text table and/or fade factor, the same
builtins return real, reproducible values sampled from that data.

**Runtime pipeline** (`milkdrop/runtime.rs`) — per frame: init equations →
per-frame equations → `evaluate_mesh` per-pixel equations → warp pass (CPU
warped UV grid) → blur1/2/3 (normalized 5×5 Gaussian, radius from
`b1n/b2n/b3n`) → comp pass → custom waves/shapes overlays → feedback copy →
post (video echo zoom/alpha/orientation, gamma, legacy hue shader `fShader`,
brighten, darken, solarize, invert, red/blue swap, brightness). Failures are
section-scoped: a warp/comp/wave/shape parse or runtime-EEL failure is logged
and skips that section only — the preset keeps running, and failures are
cached (`MilkFailureCache`) until a registry replacement or an explicit
reload retries it.

### Known gaps

- v1 presets are parsed with the same engine; base-val-only v1 presets work,
  but anything the EEL layer can't compile is skipped.

---

## 3. Video — `crates/modv-media/src/video/` (+ app glue: `video_sources.rs`, `media_watch.rs`, `media_library.rs`)

### Sources (`VideoSourceKind`)

| Source | Mechanism | Status |
| --- | --- | --- |
| Files | `uridecodebin` over a `file://` URI | Supported (also accepts `http(s)://` URIs) |
| Cameras | `v4l2src device=/dev/videoN` | Supported (V4L2) |
| Screen | `pipewiresrc path=<node id>` | Supported via XDG portal (see below); requires PipeWire + `gstreamer1.0-pipewire` |
| Test patterns | `videotestsrc` (smpte, snow, black, white, ball, circular, checkers-1, pinwheel, gradient + any other name passthrough) | Supported (also the engine's "no camera" fallback) |

### Media-tree files (library + watcher)

- Video folder extensions (case-insensitive, `crates/modv-app/src/media_watch.rs`):
  **`mp4`, `webm`, `webv`** — `webv` is the legacy spelling kept by the ported
  watcher; the library reads MP4 (`moov`/`mvhd`/`tkhd`) and WebM/EBML
  (`media_library.rs`).
- 2D modules: **Video** (`two_d/modules/video.rs`: file texture + `start`/`finish`
  trim props) and **Webcam** (`two_d/modules/webcam.rs`, camera picker);
  other modules' file texture props resolve against the video folder.

### Codecs/containers

- Decoding is delegated to GStreamer's `uridecodebin` — anything the installed
  plugins-base/-good/-bad/-libav plugins demux/decode works (h264, h265, vp8/vp9,
  av1, theora, mpeg4/mjpeg/mpeg2, `ogv`/`avi`/`mkv`/`flv`/`mpegts`… if the
  corresponding plugins are installed). The set beyond `mp4`/`webm`/`webv` is
  **not enumerated or guaranteed** — the app's own lists are those three.
- Encoded fixtures exercised by the repo's tests: vp8/webm, h264/mkv,
  theora/ogv, raw/avi.
- Required elements per source (`video/pipeline.rs`): files →
  `uridecodebin, videoconvert, videoscale, videorate, appsink`
  (`gstreamer1.0-plugins-base` + `-bad`/`-libav`); cameras →
  `v4l2src, videoconvert, videoscale, videorate, appsink`
  (`gstreamer1.0-plugins-good`); screens → `pipewiresrc, videoconvert, appsink`
  (`gstreamer1.0-pipewire`); patterns → `videotestsrc, videoconvert, videoscale,
  videorate, appsink`. A missing element is a typed error naming the element
  and the Debian package (`gstreamer1.0-plugins-*`).
- Dockerfile installs: `gstreamer1.0-plugins-base`, `-good`, `-bad`, `-libav`,
  `gstreamer1.0-pipewire`, `libgstreamer1.0-dev`,
  `libgstreamer-plugins-base1.0-dev`, `libpipewire-0.3-dev`,
  `libpipewire-0.3-0`.
- Screen capture availability requires the `pipewiresrc` element **and** a
  running PipeWire server socket (`ScreenSupport::available()`); the portal
  itself is chosen by the user via the D-Bus ScreenCast dialog (GNOME, KDE,
  wlr, …). Not available on hosts without it.

### Transport

- Files: pause/resume, seek, playback rate clamped 0.1–10.0, looping
  (default looping; the legacy `<video>` default), seek/EOS edge cases
  (parking at the last frame, restart).
- Cameras/screens/patterns: live, `sync=false`; files are clock-synced.
- "Newest wins": `appsink max-buffers=2 drop=true` — at most ~3 frames in
  flight between device and engine.

### Cameras

- V4L2 devices (`/dev/videoN`); on Linux enumeration queries V4L2 **directly**
  (the GStreamer `DeviceMonitor` path is avoided because the libcamera
  provider segfaults on some hosts with GStreamer 1.28 + libcamera 0.7).
- Resolution/fps preset lists: 176×144…4096×2160 × 15–240 fps; requested size
  is met by `videoscale` when the camera advertises fewer.

### Video file audio

- **Not ingested.** The file pipeline decodes video only:
  `uridecodebin ! videoconvert ! videoscale ! video/x-raw,format=RGBA,… ! appsink`
  — the appsink caps are video-only and no audioconvert is attached, so the
  audio track of a video file is not used by the app.

### Platform notes

- Linux is the release target. Windows/macOS are not supported
  (`list_cameras` returns empty off-Linux; `screen_support` requires
  PipeWire, which is Linux-only in practice).
