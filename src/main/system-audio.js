import { ipcMain } from "electron";
import { createLogger } from "./logger";
import {
  resolveDefaultSink,
  startPipeWireCapture,
} from "./system-audio-pipewire";
import { isAudioMonitorTarget, listAudioMonitors } from "./audio-monitors";

const logger = createLogger("main:system-audio");
const START_TIMEOUT_MS = 3000;

let registered = false;
let active = null;

export function stopActive() {
  if (!active) {
    return;
  }

  const current = active;

  active = null;

  try {
    current.sender.removeListener("destroyed", current.onDestroyed);
  } catch (e) {
    // The renderer is already gone.
  }

  current.capture.stop();
}

function startForSender(sender) {
  const onDestroyed = () => {
    if (active?.sender === sender) {
      stopActive();
    }
  };

  sender.once("destroyed", onDestroyed);

  return {
    sender,
    onDestroyed,
    capture: null,
  };
}

export function registerSystemAudioIpc() {
  if (registered) {
    return;
  }

  registered = true;

  // List every output device's monitor so the Audio Input picker can offer
  // per-sink entries (Firefox's "Monitor of …" inputs).
  ipcMain.handle("system-audio:list-monitors", () => listAudioMonitors());

  ipcMain.handle("system-audio:start", async (event, payload = {}) => {
    if (process.platform !== "linux") {
      return {
        available: false,
        reason: "Native PipeWire capture is Linux only.",
      };
    }

    // `target` comes from a renderer-supplied monitor source id. Validate it
    // against the sinks `pw-dump` reports so the argument cannot point the
    // capture at an arbitrary node.
    const requested =
      typeof payload?.target === "string" && payload.target.length > 0
        ? payload.target
        : null;

    if (requested && !(await isAudioMonitorTarget(requested))) {
      return {
        available: false,
        reason: `Unknown audio output "${requested}".`,
      };
    }

    stopActive();

    const target =
      requested ||
      process.env.MODV_SYSTEM_AUDIO_TARGET ||
      (await resolveDefaultSink());
    const session = startForSender(event.sender);

    try {
      const capture = startPipeWireCapture({
        target,
        onData: (chunk) => {
          if (!event.sender.isDestroyed()) {
            event.sender.send("system-audio:chunk", chunk);
          }
        },
        onError: (error) => {
          logger.warn("the PipeWire capture reported an error", {
            message: error?.message,
          });
        },
      });

      await new Promise((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error("pw-record did not start in time")),
          START_TIMEOUT_MS,
        );

        capture.ready.then(
          () => {
            clearTimeout(timer);
            resolve();
          },
          (error) => {
            clearTimeout(timer);
            reject(error);
          },
        );
      });

      session.capture = capture;
      active = session;

      logger.info("capturing the system audio with PipeWire", {
        target,
        sampleRate: capture.sampleRate,
        channels: capture.channels,
      });

      return {
        available: true,
        backend: "pipewire",
        sampleRate: capture.sampleRate,
        channels: capture.channels,
        target,
      };
    } catch (e) {
      session.sender.removeListener("destroyed", session.onDestroyed);

      logger.error("could not start the PipeWire capture", e);

      return { available: false, reason: e.message };
    }
  });

  ipcMain.handle("system-audio:stop", () => {
    stopActive();

    return { stopped: true };
  });
}
