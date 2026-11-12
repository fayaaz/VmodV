import { createLogger } from "./logger";
import { createPipeWireCapture } from "./system-audio-pipewire";
import {
  isMonitorSourceId,
  monitorSourceLabel,
  parseMonitorSourceId,
} from "../../../shared/audio-monitor-source.js";

export const SYSTEM_AUDIO_SOURCE_ID = "modv:system-audio";
export const SYSTEM_AUDIO_SOURCE_LABEL = "System Audio (loopback)";

export const SYSTEM_AUDIO_UNAVAILABLE_MESSAGE =
  "System audio capture is not available on this system. On Linux, install PipeWire so that `pw-record` and `wpctl` are available, or expose a PulseAudio/PipeWire monitor source as an input device.";

const MONITOR_LABEL_PATTERN = /monitor|system audio|loopback/i;

// Electron's desktop audio loopback only works reliably on macOS and
// Windows. On Linux it hands back a stream without an audio track and has
// been observed to take the browser process down, so the system audio is
// captured from a PulseAudio/PipeWire monitor source there instead.
const DISPLAY_LOOPBACK_PLATFORMS = ["darwin", "win32"];
const NATIVE_CAPTURE_PLATFORMS = ["linux"];

function getDefaultMediaDevices() {
  if (typeof navigator === "undefined") {
    return undefined;
  }

  return navigator.mediaDevices;
}

function getDefaultMediaStream() {
  if (typeof window === "undefined") {
    return undefined;
  }

  return window.MediaStream;
}

function getDefaultNativeCapture(platform = getDefaultPlatform()) {
  if (!NATIVE_CAPTURE_PLATFORMS.includes(platform)) {
    return null;
  }

  if (typeof window === "undefined" || !window.electron?.ipcRenderer) {
    return null;
  }

  return createPipeWireCapture({ ipcRenderer: window.electron.ipcRenderer });
}

/**
 * Resolve the platform the renderer is running on.
 *
 * The Vite renderer bundle's `process` is a browser shim (no `platform`), so
 * the Node global is not reliable here. Prefer Electron's remote process,
 * then the preload's electronAPI process, then a Node global if one survived.
 * Kept pure so the "unknown platform" regression has a unit test — an unknown
 * platform silently disabled the Linux PipeWire capture.
 *
 * @param {{
 *   remoteProcess?: { platform?: string } | null,
 *   electronProcess?: { platform?: string } | null,
 *   nodeProcess?: { platform?: string } | null,
 * }} [sources]
 * @returns {string}
 */
export function resolvePlatform({
  remoteProcess = null,
  electronProcess = null,
  nodeProcess = null,
} = {}) {
  for (const candidate of [remoteProcess, electronProcess, nodeProcess]) {
    if (typeof candidate?.platform === "string" && candidate.platform) {
      return candidate.platform;
    }
  }

  return "unknown";
}

function getDefaultPlatform() {
  const browserScope = typeof window !== "undefined" ? window : undefined;

  return resolvePlatform({
    remoteProcess: browserScope?.remote?.process,
    electronProcess: browserScope?.electron?.process,
    nodeProcess: typeof process !== "undefined" ? process : null,
  });
}

function describeTracks(stream) {
  const tracks = stream?.getTracks?.() || [];

  return tracks.map((track) => ({
    kind: track.kind,
    label: track.label,
    readyState: track.readyState,
    settings: track.getSettings ? track.getSettings() : undefined,
  }));
}

export function isSystemAudioSource(sourceId) {
  // The legacy single loopback entry and every per-output monitor entry the
  // picker discovers (Firefox-style "Monitor of …" inputs).
  return sourceId === SYSTEM_AUDIO_SOURCE_ID || isMonitorSourceId(sourceId);
}

export function isMonitorDeviceLabel(label) {
  if (typeof label !== "string") {
    return false;
  }

  return MONITOR_LABEL_PATTERN.test(label);
}

export function findMonitorDevice(devices) {
  if (!Array.isArray(devices)) {
    return null;
  }

  for (const device of devices) {
    if (!device || device.kind !== "audioinput") {
      continue;
    }

    if (isMonitorDeviceLabel(device.label)) {
      return device;
    }
  }

  return null;
}

export function extractAudioStream(
  displayStream,
  { MediaStream: Ctor = getDefaultMediaStream() } = {},
) {
  if (!displayStream) {
    return null;
  }

  const audioTracks = displayStream.getAudioTracks() || [];
  const videoTracks = displayStream.getVideoTracks() || [];

  for (const track of videoTracks) {
    track.stop();
  }

  if (!audioTracks.length) {
    const remainingTracks = displayStream.getTracks() || [];

    for (const track of remainingTracks) {
      track.stop();
    }

    return null;
  }

  const Constructor = Ctor || displayStream.constructor;

  return new Constructor(audioTracks);
}

export class SystemAudioCapture {
  constructor({
    mediaDevices = getDefaultMediaDevices(),
    monitorDeviceId = null,
    platform = getDefaultPlatform(),
    nativeCapture = getDefaultNativeCapture(platform),
    logger = createLogger("renderer:system-audio"),
  } = {}) {
    this.mediaDevices = mediaDevices;
    this.monitorDeviceId = monitorDeviceId;
    this.platform = platform;
    this.nativeCapture = nativeCapture;
    this.logger = logger;
    this._displayStream = null;
  }

  static isSupported(mediaDevices) {
    return typeof mediaDevices?.getDisplayMedia === "function";
  }

  /**
   * Capture system audio.
   *
   * With `sourceId` naming a per-output monitor, the native PipeWire capture
   * targets that sink. Without one it keeps the legacy behaviour (a browser
   * monitor device if one exists, otherwise the default sink's monitor).
   *
   * @param {{ sourceId?: string | null, label?: string | null }} [options]
   */
  async capture({ sourceId = null, label = null } = {}) {
    const logger = this.logger;
    const monitorTarget = parseMonitorSourceId(sourceId);

    logger.info("system audio capture requested", {
      platform: this.platform,
      target: monitorTarget,
    });

    try {
      if (monitorTarget && this.nativeCapture) {
        const captured = await this.nativeCapture.start({
          target: monitorTarget,
        });

        if (captured?.stream) {
          logger.info("capturing a selected monitor source", {
            target: monitorTarget,
            sampleRate: captured.sampleRate,
            channels: captured.channels,
          });

          return {
            stream: captured.stream,
            label:
              label ||
              monitorSourceLabel({ target: monitorTarget, description: "" }),
            backend: captured.backend || "pipewire",
          };
        }

        throw new Error(
          captured?.reason ||
            `Could not capture the monitor of "${monitorTarget}".`,
        );
      }

      const device = await this._findMonitorDevice();

      if (device) {
        logger.info("capturing the system audio from a monitor source", {
          deviceId: device.deviceId,
          label: device.label,
        });

        const stream = await this._captureMonitorDevice(device);

        logger.info("monitor source captured", {
          tracks: describeTracks(stream),
        });

        return {
          stream,
          label: device.label || SYSTEM_AUDIO_SOURCE_LABEL,
          backend: "monitor",
        };
      }

      logger.info("no monitor source was found", {
        devices: await this._describeDevices(),
      });

      if (
        NATIVE_CAPTURE_PLATFORMS.includes(this.platform) &&
        this.nativeCapture
      ) {
        try {
          const captured = await this.nativeCapture.start();

          if (captured?.stream) {
            logger.info("capturing the system audio with native PipeWire", {
              sampleRate: captured.sampleRate,
              channels: captured.channels,
              label: captured.label,
            });

            return {
              stream: captured.stream,
              label: captured.label || SYSTEM_AUDIO_SOURCE_LABEL,
              backend: captured.backend || "pipewire",
            };
          }

          logger.warn("native PipeWire capture is unavailable", {
            reason: captured?.reason,
          });
        } catch (e) {
          logger.error("native PipeWire capture failed", e);
        }
      }

      if (!SystemAudioCapture.isSupported(this.mediaDevices)) {
        logger.warn("getDisplayMedia is not available");
      } else if (!DISPLAY_LOOPBACK_PLATFORMS.includes(this.platform)) {
        logger.warn("skipping the display capture fallback on this platform", {
          platform: this.platform,
        });
      } else {
        const displayStream = await this.mediaDevices.getDisplayMedia({
          video: true,
          audio: true,
        });

        this._displayStream = displayStream;

        logger.info("display capture returned a stream", {
          tracks: describeTracks(displayStream),
        });

        const stream = extractAudioStream(displayStream);

        if (stream) {
          logger.info("display capture provided system audio", {
            tracks: describeTracks(stream),
          });

          return {
            stream,
            label: SYSTEM_AUDIO_SOURCE_LABEL,
            backend: "display",
          };
        }

        logger.warn("the display capture returned no audio track");

        this._stopStream(displayStream);
        this._displayStream = null;
      }
    } catch (e) {
      logger.error("system audio capture failed", e);

      this._stopStream(this._displayStream);
      this._displayStream = null;

      throw e;
    }

    logger.error("no usable system audio source was found");

    throw new Error(SYSTEM_AUDIO_UNAVAILABLE_MESSAGE);
  }

  stop(stream) {
    this._stopStream(stream);
    this.nativeCapture?.stop?.();

    if (this._displayStream && this._displayStream !== stream) {
      this._stopStream(this._displayStream);
    }

    this._displayStream = null;
  }

  async _captureMonitorDevice(device) {
    const stream = await this.mediaDevices.getUserMedia({
      audio: {
        deviceId: { exact: device.deviceId },
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      },
    });

    const tracks = stream?.getAudioTracks?.() || [];
    const live = tracks.filter((track) => track.readyState !== "ended");

    if (!live.length) {
      this._stopStream(stream);

      throw new Error(
        `The system audio source "${device.label || device.deviceId}" did not provide a live audio track.`,
      );
    }

    return stream;
  }

  async _describeDevices() {
    if (typeof this.mediaDevices?.enumerateDevices !== "function") {
      return [];
    }

    const devices = await this.mediaDevices.enumerateDevices();

    return devices.map((device) => ({
      deviceId: device.deviceId,
      kind: device.kind,
      label: device.label,
    }));
  }

  async _findMonitorDevice() {
    if (this.monitorDeviceId) {
      if (typeof this.mediaDevices?.enumerateDevices === "function") {
        const devices = await this.mediaDevices.enumerateDevices();
        const match = (devices || []).find(
          (device) => device.deviceId === this.monitorDeviceId,
        );

        if (match) {
          return match;
        }
      }

      return {
        deviceId: this.monitorDeviceId,
        label: SYSTEM_AUDIO_SOURCE_LABEL,
      };
    }

    if (typeof this.mediaDevices?.enumerateDevices !== "function") {
      return null;
    }

    const devices = await this.mediaDevices.enumerateDevices();

    return findMonitorDevice(devices);
  }

  _stopStream(stream) {
    const tracks = stream?.getTracks?.() || [];

    for (const track of tracks) {
      track.stop();
    }
  }
}
