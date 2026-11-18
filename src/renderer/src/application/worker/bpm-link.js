// The BPM "audio feature" is a state Input Link on `beats.bpm` rather than a
// meyda getter: the tempo detector writes a real tempo (e.g. 174), not a
// normalised 0..1 audio band.
export const BPM_LINK_SOURCE = "bpm";
export const BPM_LINK_LOCATION = "beats.bpm";

/**
 * `inputs/createInputLink` payload for the BPM feature.
 *
 * `min`/`max` are deliberately omitted: the action defaults them to 0/1, which
 * maps the source onto itself. Setting the input's own 60..200 range here
 * would scale a 174 BPM tempo into the tens of thousands, because link bounds
 * remap the incoming 0..1 range rather than describing it.
 *
 * @param {String} inputId
 * @returns {Object}
 */
export function createBpmInputLink(inputId) {
  return {
    inputId,
    type: "state",
    location: BPM_LINK_LOCATION,
    source: BPM_LINK_SOURCE,
  };
}
