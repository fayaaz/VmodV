# Capture rate — why the picker says 44.1 kHz (capture runs at 48)

Status: standing reference (verified on Arches, PipeWire 1.6.8 / cpal 0.17.3,
Nov 2026).

## TL;DR

Every row in the native input picker (`2 ch · 44.1 kHz`) advertises 44.1 kHz
because of a hard-coded heuristic in cpal's ALSA backend, **not** because
modV or PipeWire capture at 44.1 kHz. modV captures at **48 kHz** everywhere:

- `crates/modv-media/src/audio/capture.rs` — `DEFAULT_SAMPLE_RATE = 48_000`.
  `AudioCapture::start` builds `CaptureConfig::default()` (48000 Hz / 2 ch /
  480-frame blocks) and calls `choose_config()`, which picks the advertised
  range covering 48 kHz (preferring f32 and the exact channel count). It only
  falls back to cpal's `default_input_config()` — the 44.1 kHz path — if **no
  advertised range covers 48 kHz**, which does not happen on any device this
  host enumerates.
- PipeWire monitor paths are 48 kHz as well: the legacy `pw-record` transport
  uses `PIPEWIRE_SAMPLE_RATE = 48000` (`src/main/system-audio-pipewire.js`)
  and the renderer's chunk assembler uses `PCM_CHUNK_SAMPLE_RATE = 48000`
  (`src/renderer/src/application/system-audio-pipewire.js`); the native
  engine monitor opens `CaptureConfig::default()` (48000). The band-analyser
  constants in `modv-media` assume this rate.
- The PipeWire graph itself is pinned to 48 kHz: `default.clock.rate = 48000`
  and the stock `10-rates.conf` (allowed-rates `[ 44100 48000 88200 96000 ]`)
  is **not enabled** anywhere (`/etc/pipewire`, `~/.config/pipewire` and
  `pipewire.conf.d/` do not exist), so allowed-rates is effectively
  `[ 48000 ]`. A 44.1 kHz client (Firefox requests 44100 for its audio
  pipeline) is resampled per-link to the 48 kHz graph; the hardware never
  leaves 48 kHz.

## Where the 44.1 kHz label comes from (cpal 0.17.3)

`cpal::default_input_config()` → ALSA `default_config()` in
`src/host/alsa/mod.rs`: sort candidate ranges by `cmp_default_heuristics`,
take the greatest, use `with_max_sample_rate()`, then:

```rust
const HZ_44100: SampleRate = 44_100;
if min_r <= HZ_44100 && HZ_44100 <= max_r {
    format.sample_rate = HZ_44100;
}
```

i.e. whenever 44.1 kHz sits inside the advertised range, the "default" rate
is reported as exactly **44100**, even when the max is much higher
(384 kHz virtual server PCMs) or 44.1 and 48 are equal citizens. Every device
listed on this host admits 44.1 kHz somewhere in its range, so every row says
`44.1 kHz`. It is a picker-label artefact only.

The label data flow:

```
cpal Device::default_input_config()          (44.1 kHz via the override above)
  → input_devices()                           crates/modv-media/src/audio/capture.rs
  → device_views()                            crates/modv-app/src/audio_settings.rs (filters probes, dedups)
  → AudioDeviceView.default_sample_rate       crates/modv-ui/src/model.rs (`detail()` / `active_detail()`)
```

Related: the virtual `alsa:pipewire` / `alsa:pulse` / `alsa:default` /
`alsa:plughw:*` rows advertise bogus bounds — `min = 1` and
`max = 4294967295` (0xFFFFFFFF, ALSA "no limit") — shown verbatim by
`--list-audio-devices`. Their "sample rates" are placeholders; hardware rows
(`alsa:hw:*`) advertise true ranges.

## Verifying on a live system

```sh
modv-engine --list-audio-devices   # prints channels + advertised rates; works headless
pw-cli info 0 | grep clock         # default.clock.rate = "48000"
cat /proc/asound/card1/stream0     # capture Altset: card1 (PCM2903B) is Altset 1 = 48000 Hz
pw-top                             # per-client negotiated rates; 48 kHz graph, 44.1 kHz
                                   # clients resampled at their link
```

Observed on Arches (this host): `alsa:pipewire` reports `1, 44100, 384000`;
`alsa:hw:CARD=CODEC,DEV=0` reports `8000, 11025, 16000, 22050, 32000,
44100, 48000`; `alsa:dsnoop:*` reports `48000` only; and the capture device
is running in its 48000 Altset (`Momentary freq = 48000 Hz`).

## Caveats / gotchas

- The picker's `min–max` extremes (`1 … 4294967295`) are the ALSA plugins'
  "any rate" advertisement, not supported hardware modes. Do not infer device
  capability from them.
- `choose_config()` falls back to the device default (i.e. 44.1 kHz) only when
  no advertised range covers the requested rate. Any config where **every**
  range's max < 48000 would open at 44.1 kHz; check `--list-audio-devices`
  if a new device ever shows up and confirm the app negotiated the rate via
  `AudioCapture::sample_rate()` (reported after `start()`).
- If anyone enables a `default.clock.allowed-rates` drop-in that includes
  44100 (e.g. symlinking `10-rates.conf` into `pipewire.conf.d/`), a 44.1
  kHz-driving client may switch the whole graph to 44.1 kHz and the ALSA
  source opens its fixed 44100 Altset (PCM2903B Altset 3). That is the one
  way this host's hardware genuinely runs at 44.1 kHz — keep the drop-in off
  unless bit-perfect 44.1 switching is wanted.
- Firefox's 44.1 kHz output stream (`Stream/Output/Audio ... F32LE 44100`) is
  expected; it is resampled to the 48 kHz graph and does not drive device
  rate while `allowed-rates = [ 48000 ]`.
- The analyser's band edges / bin mapping assume 48 kHz. If the device ever
  negotiates anything else, `AudioCapture::sample_rate()` must be threaded
  into the analysis (it is) rather than assuming 48 kHz.

## Possible future fix (not done)

Replace the cpal-heuristic `default_sample_rate` in the picker with something
truthful, e.g. the rate `choose_config` would pick for `DEFAULT_SAMPLE_RATE`
(same code path as `AudioCapture::start`) or a `min–max` label; the negotiated
rate is only truly known after the stream opens. Deliberately not done yet —
the capture behaviour is correct; only the label misleads.
