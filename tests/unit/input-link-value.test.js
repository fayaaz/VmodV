import { describe, expect, it } from "vitest";
import { resolveInputLinkValue } from "../../src/renderer/src/application/worker/input-link-value.js";

describe("resolveInputLinkValue", () => {
  it("maps a value into the link range", () => {
    expect(resolveInputLinkValue(0, 1, 1.2)).toBeCloseTo(1);
    expect(resolveInputLinkValue(1, 1, 1.2)).toBeCloseTo(1.2);
    expect(resolveInputLinkValue(0.5, 1, 3)).toBeCloseTo(2);
  });

  it("keeps the raw value when the link has no range", () => {
    expect(resolveInputLinkValue(0.5)).toBe(0.5);
    expect(resolveInputLinkValue(0)).toBe(0);
  });

  it("does not turn a missing feature into NaN", () => {
    expect(resolveInputLinkValue(undefined, 1, 1.2)).toBeUndefined();
    expect(resolveInputLinkValue(null, 1, 1.2)).toBeUndefined();
  });

  it("rejects non-finite resolved values", () => {
    expect(resolveInputLinkValue(Number.NaN, 1, 1.2)).toBeUndefined();
    expect(resolveInputLinkValue(Infinity, 1, 1.2)).toBeUndefined();
  });

  it("waits for a real tempo before writing a BPM link", () => {
    // `beats.bpm` is 0 until the tempo detector has a confident estimate.
    // Writing it into a BPM input breaks shaders that divide by the tempo.
    expect(resolveInputLinkValue(0, 0, 1, "bpm")).toBeUndefined();
    expect(resolveInputLinkValue(-120, 0, 1, "bpm")).toBeUndefined();
    expect(resolveInputLinkValue(Number.NaN, 0, 1, "bpm")).toBeUndefined();
    expect(resolveInputLinkValue(174, 0, 1, "bpm")).toBe(174);
    // A BPM link is an absolute tempo: a stale ranged link must not scale it.
    expect(resolveInputLinkValue(174, 0.2, 0.8, "bpm")).toBe(174);
  });

  it("still writes zero for every other link source", () => {
    expect(resolveInputLinkValue(0, 0, 1, "meyda")).toBe(0);
    expect(resolveInputLinkValue(0, 1, 1.2, "tween")).toBe(1);
  });
});
