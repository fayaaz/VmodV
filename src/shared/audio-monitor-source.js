// PipeWire monitor sources, shared by the main process (which enumerates the
// sinks) and the renderer (which lists them as selectable audio inputs).
//
// Firefox lists every output device's monitor as an input ("Monitor of …").
// We do the same: each sink becomes an audio input whose id carries the sink's
// PipeWire `node.name`, and selecting it records that sink's monitor through
// `pw-record` (system-audio-pipewire.js).

/** Prefix for a per-output monitor source id. */
export const MONITOR_SOURCE_PREFIX = "modv:monitor:";

/** Prefix Firefox uses for its monitor inputs. */
export const MONITOR_LABEL_PREFIX = "Monitor of ";

/**
 * Build the selectable audio-input id for one sink's monitor.
 *
 * @param {string} target the sink's PipeWire `node.name`
 * @returns {string}
 */
export function buildMonitorSourceId(target) {
  return `${MONITOR_SOURCE_PREFIX}${String(target ?? "")}`;
}

/**
 * The sink `node.name` encoded in a monitor source id.
 *
 * @param {unknown} sourceId
 * @returns {string | null}
 */
export function parseMonitorSourceId(sourceId) {
  if (typeof sourceId !== "string") {
    return null;
  }

  if (!sourceId.startsWith(MONITOR_SOURCE_PREFIX)) {
    return null;
  }

  const target = sourceId.slice(MONITOR_SOURCE_PREFIX.length);

  return target || null;
}

/**
 * Whether an audio source id refers to a per-output monitor.
 *
 * @param {unknown} sourceId
 * @returns {boolean}
 */
export function isMonitorSourceId(sourceId) {
  return parseMonitorSourceId(sourceId) !== null;
}

/**
 * The input label for a monitor, matching Firefox's "Monitor of …" wording.
 *
 * @param {{ description?: string, nick?: string, target?: string } | null | undefined} sink
 * @returns {string}
 */
export function monitorSourceLabel(sink) {
  const description =
    typeof sink?.description === "string" ? sink.description.trim() : "";
  const nick = typeof sink?.nick === "string" ? sink.nick.trim() : "";
  const target = typeof sink?.target === "string" ? sink.target.trim() : "";

  return `${MONITOR_LABEL_PREFIX}${description || nick || target || "system output"}`;
}
