import { describe, expect, it, vi } from "vitest";

import {
  audioMonitorSources,
  isAudioMonitorTarget,
  listAudioMonitors,
  parseAudioSinks,
} from "../../src/main/audio-monitors.js";
import { buildMonitorSourceId } from "../../src/shared/audio-monitor-source.js";

const DUMP = JSON.stringify([
  {
    id: 36,
    type: "PipeWire:Interface:Node",
    info: {
      props: {
        "media.class": "Audio/Sink",
        "node.name":
          "alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo",
        "node.description": "PCM2903B Audio CODEC Analogue Stereo",
        "node.nick": "USB Audio CODEC",
      },
    },
  },
  {
    id: 47,
    type: "PipeWire:Interface:Node",
    info: {
      props: {
        "media.class": "Audio/Sink",
        "node.name":
          "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo",
        "node.description": "PCM2704 16-bit stereo audio DAC Analogue Stereo",
        "node.nick": "ADAM Audio D3V",
      },
    },
  },
  {
    id: 48,
    type: "PipeWire:Interface:Node",
    info: {
      props: {
        "media.class": "Audio/Source",
        "node.name": "alsa_input.usb-microphone",
        "node.description": "USB Microphone",
      },
    },
  },
  {
    id: 10,
    type: "PipeWire:Interface:Node",
    info: { props: { "media.class": "Video/Source" } },
  },
  {
    id: 11,
    type: "PipeWire:Interface:Node",
    info: { props: { "media.class": "Audio/Sink" } },
  },
  {
    id: 12,
    type: "PipeWire:Interface:Node",
    info: {
      props: {
        "media.class": "Audio/Sink",
        "node.name":
          "alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo",
        "node.description": "duplicate",
      },
    },
  },
  { id: 13, type: "PipeWire:Interface:Client", info: { props: {} } },
]);

describe("parseAudioSinks", () => {
  it("returns only audio sinks with a node.name, deduplicated", () => {
    expect(parseAudioSinks(DUMP).map((sink) => sink.target)).toEqual([
      "alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo",
      "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo",
    ]);
  });

  it("keeps the description, nick and object id", () => {
    expect(parseAudioSinks(DUMP)[1]).toEqual({
      target:
        "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo",
      description: "PCM2704 16-bit stereo audio DAC Analogue Stereo",
      nick: "ADAM Audio D3V",
      id: 47,
    });
  });

  it("tolerates missing/!invalid dumps", () => {
    expect(parseAudioSinks("")).toEqual([]);
    expect(parseAudioSinks("not json")).toEqual([]);
    expect(parseAudioSinks("{}")).toEqual([]);
    expect(parseAudioSinks("null")).toEqual([]);
    expect(parseAudioSinks(null)).toEqual([]);
    expect(parseAudioSinks(undefined)).toEqual([]);
  });
});

describe("audioMonitorSources", () => {
  it("builds Firefox-style monitor inputs with the default first", () => {
    const monitors = audioMonitorSources(parseAudioSinks(DUMP), {
      defaultTarget:
        "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo",
    });

    expect(monitors).toEqual([
      {
        id: buildMonitorSourceId(
          "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo",
        ),
        target:
          "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo",
        label: "Monitor of PCM2704 16-bit stereo audio DAC Analogue Stereo",
        description: "PCM2704 16-bit stereo audio DAC Analogue Stereo",
        isDefault: true,
      },
      {
        id: buildMonitorSourceId(
          "alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo",
        ),
        target:
          "alsa_output.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo",
        label: "Monitor of PCM2903B Audio CODEC Analogue Stereo",
        description: "PCM2903B Audio CODEC Analogue Stereo",
        isDefault: false,
      },
    ]);
  });

  it("works without a default sink", () => {
    const monitors = audioMonitorSources(parseAudioSinks(DUMP));
    expect(monitors.every((monitor) => monitor.isDefault === false)).toBe(true);
    expect(monitors.map((monitor) => monitor.label)).toEqual([
      "Monitor of PCM2704 16-bit stereo audio DAC Analogue Stereo",
      "Monitor of PCM2903B Audio CODEC Analogue Stereo",
    ]);
  });
});

function fakeExecFile(stdout, error = null) {
  return vi.fn((file, args, options, callback) => {
    callback(error, stdout, "");
  });
}

describe("listAudioMonitors", () => {
  it("lists sinks, prioritising the default one", async () => {
    const result = await listAudioMonitors({
      execFileImpl: fakeExecFile(DUMP),
      resolveDefaultSinkImpl: async () =>
        "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo",
      platform: "linux",
    });

    expect(result.available).toBe(true);
    expect(result.monitors).toHaveLength(2);
    expect(result.monitors[0].isDefault).toBe(true);
  });

  it("reports unavailable when pw-dump fails or there are no sinks", async () => {
    const failed = await listAudioMonitors({
      execFileImpl: fakeExecFile("", new Error("ENOENT")),
      platform: "linux",
    });
    expect(failed.available).toBe(false);
    expect(failed.monitors).toEqual([]);

    const empty = await listAudioMonitors({
      execFileImpl: fakeExecFile("[]"),
      platform: "linux",
    });
    expect(empty.available).toBe(false);
    expect(empty.monitors).toEqual([]);
  });

  it("is Linux only", async () => {
    const result = await listAudioMonitors({ platform: "darwin" });
    expect(result.available).toBe(false);
    expect(result.monitors).toEqual([]);
  });

  it("validates a target against the discovered sinks", async () => {
    const target =
      "alsa_output.usb-Vendor_strings_are_placed_here._ADAM_Audio_D3V-00.analog-stereo";
    expect(
      await isAudioMonitorTarget(target, {
        execFileImpl: fakeExecFile(DUMP),
        resolveDefaultSinkImpl: async () => null,
        platform: "linux",
      }),
    ).toBe(true);
    expect(
      await isAudioMonitorTarget("alsa_output.not-a-sink", {
        execFileImpl: fakeExecFile(DUMP),
        resolveDefaultSinkImpl: async () => null,
        platform: "linux",
      }),
    ).toBe(false);
    expect(await isAudioMonitorTarget("", { platform: "linux" })).toBe(false);
  });
});
