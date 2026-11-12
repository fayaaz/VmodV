import { describe, expect, it } from "vitest";

import {
  buildMonitorSourceId,
  isMonitorSourceId,
  monitorSourceLabel,
  parseMonitorSourceId,
  MONITOR_LABEL_PREFIX,
  MONITOR_SOURCE_PREFIX,
} from "../../src/shared/audio-monitor-source.js";

describe("monitor source ids", () => {
  it("round-trip a PipeWire sink name", () => {
    const target = "alsa_output.usb-ADAM_Audio_D3V-00.analog-stereo";
    const id = buildMonitorSourceId(target);

    expect(id).toBe(`${MONITOR_SOURCE_PREFIX}${target}`);
    expect(parseMonitorSourceId(id)).toBe(target);
    expect(isMonitorSourceId(id)).toBe(true);
  });

  it("rejects the legacy loopback id and plain browser device ids", () => {
    expect(parseMonitorSourceId("modv:system-audio")).toBeNull();
    expect(parseMonitorSourceId("default")).toBeNull();
    expect(parseMonitorSourceId("")).toBeNull();
    expect(parseMonitorSourceId(null)).toBeNull();
    expect(parseMonitorSourceId(undefined)).toBeNull();
    expect(parseMonitorSourceId(123)).toBeNull();
    expect(isMonitorSourceId("modv:system-audio")).toBe(false);
  });

  it("does not treat an empty target as a monitor", () => {
    expect(parseMonitorSourceId(MONITOR_SOURCE_PREFIX)).toBeNull();
    expect(isMonitorSourceId(MONITOR_SOURCE_PREFIX)).toBe(false);
  });
});

describe("monitor source labels", () => {
  it("uses the Firefox wording with the sink description", () => {
    expect(
      monitorSourceLabel({
        description: "PCM2704 16-bit stereo audio DAC Analogue Stereo",
        nick: "ADAM Audio D3V",
        target: "alsa_output.usb",
      }),
    ).toBe(
      `${MONITOR_LABEL_PREFIX}PCM2704 16-bit stereo audio DAC Analogue Stereo`,
    );
  });

  it("falls back to the nick, target, then a generic label", () => {
    expect(monitorSourceLabel({ nick: "ADAM Audio D3V" })).toBe(
      "Monitor of ADAM Audio D3V",
    );
    expect(monitorSourceLabel({ target: "alsa_output.usb" })).toBe(
      "Monitor of alsa_output.usb",
    );
    expect(monitorSourceLabel({})).toBe("Monitor of system output");
    expect(monitorSourceLabel(null)).toBe("Monitor of system output");
  });
});
