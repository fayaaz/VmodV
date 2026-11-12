import { describe, expect, it, vi } from "vitest";

import { listAudioMonitorInputs } from "../../src/renderer/src/application/audio-monitor-inputs.js";

const MONITORS = {
  available: true,
  monitors: [
    {
      id: "modv:monitor:alsa_output.usb-ADAM_Audio_D3V-00.analog-stereo",
      target: "alsa_output.usb-ADAM_Audio_D3V-00.analog-stereo",
      label: "Monitor of PCM2704 16-bit stereo audio DAC Analogue Stereo",
      isDefault: true,
    },
    {
      id: "modv:monitor:alsa_output.pci-0000_03_00.1.hdmi-stereo-extra3",
      target: "alsa_output.pci-0000_03_00.1.hdmi-stereo-extra3",
      label: "Monitor of Navi 48 HDMI/DP Audio Controller",
      isDefault: false,
    },
  ],
};

describe("listAudioMonitorInputs", () => {
  it("maps the IPC payload to audio-input devices", async () => {
    const invoke = vi.fn(async () => MONITORS);

    await expect(listAudioMonitorInputs({ invoke })).resolves.toEqual([
      {
        deviceId:
          "modv:monitor:alsa_output.usb-ADAM_Audio_D3V-00.analog-stereo",
        label: "Monitor of PCM2704 16-bit stereo audio DAC Analogue Stereo",
        kind: "audioinput",
        isSystemAudio: true,
        isMonitor: true,
        monitorTarget: "alsa_output.usb-ADAM_Audio_D3V-00.analog-stereo",
        isDefault: true,
      },
      {
        deviceId:
          "modv:monitor:alsa_output.pci-0000_03_00.1.hdmi-stereo-extra3",
        label: "Monitor of Navi 48 HDMI/DP Audio Controller",
        kind: "audioinput",
        isSystemAudio: true,
        isMonitor: true,
        monitorTarget: "alsa_output.pci-0000_03_00.1.hdmi-stereo-extra3",
        isDefault: false,
      },
    ]);
    expect(invoke).toHaveBeenCalledWith("system-audio:list-monitors");
  });

  it("returns an empty list when the bridge is missing or fails", async () => {
    await expect(listAudioMonitorInputs(null)).resolves.toEqual([]);
    await expect(listAudioMonitorInputs({})).resolves.toEqual([]);
    await expect(
      listAudioMonitorInputs({
        invoke: vi.fn(async () => {
          throw new Error("no ipc");
        }),
      }),
    ).resolves.toEqual([]);
    await expect(
      listAudioMonitorInputs({
        invoke: vi.fn(async () => ({ available: false })),
      }),
    ).resolves.toEqual([]);
  });

  it("skips malformed monitor entries", async () => {
    const invoke = vi.fn(async () => ({
      available: true,
      monitors: [
        { id: "modv:monitor:a", target: "a" },
        { id: "modv:monitor:", target: "" },
        { target: "b" },
        null,
      ],
    }));

    const inputs = await listAudioMonitorInputs({ invoke });

    expect(inputs).toHaveLength(1);
    expect(inputs[0]).toMatchObject({
      deviceId: "modv:monitor:a",
      label: "Monitor of a",
      monitorTarget: "a",
    });
  });
});
