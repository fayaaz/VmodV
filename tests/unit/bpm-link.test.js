import { describe, expect, it } from "vitest";
import {
  BPM_LINK_LOCATION,
  BPM_LINK_SOURCE,
  createBpmInputLink,
} from "../../src/renderer/src/application/worker/bpm-link.js";

describe("bpm input link", () => {
  it("links the input to the detected tempo as a state link", () => {
    expect(createBpmInputLink("input-1")).toEqual({
      inputId: "input-1",
      type: "state",
      location: BPM_LINK_LOCATION,
      source: BPM_LINK_SOURCE,
    });
    expect(BPM_LINK_LOCATION).toBe("beats.bpm");
    expect(BPM_LINK_SOURCE).toBe("bpm");
  });

  it("leaves min/max absent so the tempo passes through unchanged", () => {
    // `inputs/createInputLink` defaults min/max to 0/1, which is the identity
    // mapping; writing the input's own 60..200 range here would scale a 174
    // BPM tempo into the tens of thousands.
    const link = createBpmInputLink("input-1");

    expect(link).not.toHaveProperty("min");
    expect(link).not.toHaveProperty("max");
  });
});
