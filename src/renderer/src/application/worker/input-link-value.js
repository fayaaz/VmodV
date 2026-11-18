import map from "../utils/map";
import { BPM_LINK_SOURCE } from "./bpm-link";

/**
 * Resolves an Input Link's raw source value into the value written to the
 * linked input. Audio features are undefined until the first audio frame
 * arrives (and stay undefined when no input is selected), so a ranged link
 * must not turn that into NaN and overwrite the input's saved value.
 *
 * BPM links additionally wait for a real tempo: `beats.bpm` is 0 until the
 * tempo detector has a confident estimate, and writing 0 into a tempo input
 * breaks shaders that divide by it. A BPM link also ignores `min`/`max`,
 * because a tempo is absolute and bounds would remap it as if it were a 0..1
 * audio feature.
 *
 * @param {Number|undefined|null} value
 * @param {Number} [min]
 * @param {Number} [max]
 * @param {String} [source]
 * @returns {Number|undefined}
 */
export function resolveInputLinkValue(value, min, max, source) {
  if (value === undefined || value === null) {
    return undefined;
  }

  if (source === BPM_LINK_SOURCE) {
    if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
      return undefined;
    }

    // A tempo is absolute: link bounds remap 0..1, so ignore a stale ranged
    // link's min/max instead of scaling 174 through 0.2..0.8.
    return value;
  }

  const resolved =
    min != null || max != null ? map(value, 0, 1, min, max) : value;

  if (typeof resolved === "number" && !Number.isFinite(resolved)) {
    return undefined;
  }

  return resolved;
}
