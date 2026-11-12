// Renderer-side view of the per-output PipeWire monitors the main process
// discovers. Kept separate from MediaDeviceManager so the mapping from the
// IPC payload to `mediaStream.audio` entries is unit-testable.

/**
 * List the monitors of every output device as audio-input entries.
 *
 * @param {{ invoke?: (channel: string, ...args: unknown[]) => Promise<unknown> } | null} ipcRenderer
 *   `window.electron.ipcRenderer` in the app; a stub in tests.
 * @returns {Promise<Array<{
 *   deviceId: string,
 *   label: string,
 *   kind: "audioinput",
 *   isSystemAudio: true,
 *   isMonitor: true,
 *   monitorTarget: string,
 *   isDefault: boolean,
 * }>>}
 */
export async function listAudioMonitorInputs(ipcRenderer) {
  if (typeof ipcRenderer?.invoke !== "function") {
    return [];
  }

  let result;

  try {
    result = await ipcRenderer.invoke("system-audio:list-monitors");
  } catch (error) {
    return [];
  }

  if (!result?.available || !Array.isArray(result.monitors)) {
    return [];
  }

  return result.monitors
    .filter(
      (monitor) =>
        monitor &&
        typeof monitor.id === "string" &&
        typeof monitor.target === "string" &&
        monitor.target.length > 0,
    )
    .map((monitor) => ({
      deviceId: monitor.id,
      label:
        typeof monitor.label === "string" && monitor.label.length > 0
          ? monitor.label
          : `Monitor of ${monitor.target}`,
      kind: "audioinput",
      isSystemAudio: true,
      isMonitor: true,
      monitorTarget: monitor.target,
      isDefault: monitor.isDefault === true,
    }));
}
