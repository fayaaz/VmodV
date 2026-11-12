// Enumerate the monitors of every PipeWire output device, so the Audio Input
// picker can offer them the way Firefox does ("Monitor of …").
//
// A PipeWire sink does not expose a separate monitor *node*: the monitor is the
// sink's own capture ports. `pw-record --target <sink node.name>` with
// `stream.capture.sink = true` records it (system-audio-pipewire.js). This
// module only discovers the sinks and gives each one a stable id/label.

import { execFile } from "node:child_process";

import {
  buildMonitorSourceId,
  monitorSourceLabel,
} from "../shared/audio-monitor-source.js";
import { resolveDefaultSink } from "./system-audio-pipewire.js";

/**
 * Parse `pw-dump` JSON into the audio sinks it contains.
 *
 * Pure so the discovery rules are testable without a PipeWire server. Props
 * can be missing or non-string for virtual/partially-initialised nodes, so
 * only `Audio/Sink` nodes with a string `node.name` are returned.
 *
 * @param {string} output `pw-dump` stdout
 * @returns {Array<{ target: string, description: string, nick: string, id: number | null }>}
 */
export function parseAudioSinks(output) {
  if (typeof output !== "string" || output.trim().length === 0) {
    return [];
  }

  let dump;

  try {
    dump = JSON.parse(output);
  } catch (error) {
    return [];
  }

  if (!Array.isArray(dump)) {
    return [];
  }

  const sinks = [];
  const seen = new Set();

  for (const object of dump) {
    if (object?.type !== "PipeWire:Interface:Node") {
      continue;
    }

    const props = object?.info?.props;

    if (!props || props["media.class"] !== "Audio/Sink") {
      continue;
    }

    const target =
      typeof props["node.name"] === "string" ? props["node.name"] : "";

    if (!target || seen.has(target)) {
      continue;
    }

    seen.add(target);

    sinks.push({
      target,
      description:
        typeof props["node.description"] === "string"
          ? props["node.description"]
          : "",
      nick: typeof props["node.nick"] === "string" ? props["node.nick"] : "",
      id: typeof object.id === "number" ? object.id : null,
    });
  }

  return sinks;
}

/**
 * Turn parsed sinks into selectable audio-input entries.
 *
 * The default sink leads, then the list is label-sorted (Firefox order is
 * discovery order; sorting keeps the picker stable across enumerations).
 *
 * @param {ReturnType<typeof parseAudioSinks>} sinks
 * @param {{ defaultTarget?: string | null }} [options]
 * @returns {Array<{ id: string, target: string, label: string, description: string, isDefault: boolean }>}
 */
export function audioMonitorSources(sinks, { defaultTarget = null } = {}) {
  const monitors = (Array.isArray(sinks) ? sinks : []).map((sink) => ({
    id: buildMonitorSourceId(sink.target),
    target: sink.target,
    label: monitorSourceLabel(sink),
    description: sink.description,
    isDefault: sink.target === defaultTarget,
  }));

  return monitors.sort(
    (a, b) =>
      Number(b.isDefault) - Number(a.isDefault) ||
      a.label.localeCompare(b.label),
  );
}

/**
 * Discover the monitors available on this host.
 *
 * Never throws: a missing `pw-dump`, a non-Linux host or an empty dump all
 * resolve to `{ available: false, monitors: [], reason }`, and the picker
 * keeps the single "System Audio (loopback)" fallback entry.
 *
 * @param {{
 *   execFileImpl?: typeof execFile,
 *   resolveDefaultSinkImpl?: () => Promise<string | null>,
 *   platform?: NodeJS.Platform,
 * }} [options]
 */
export async function listAudioMonitors({
  execFileImpl = execFile,
  resolveDefaultSinkImpl = resolveDefaultSink,
  platform = process.platform,
} = {}) {
  if (platform !== "linux") {
    return {
      available: false,
      monitors: [],
      reason: "PipeWire monitor inputs are Linux only.",
    };
  }

  return new Promise((resolve) => {
    execFileImpl(
      "pw-dump",
      [],
      { timeout: 4000, maxBuffer: 32 * 1024 * 1024 },
      async (error, stdout) => {
        if (error) {
          resolve({
            available: false,
            monitors: [],
            reason: `pw-dump failed: ${error.message}`,
          });
          return;
        }

        const sinks = parseAudioSinks(stdout);

        if (!sinks.length) {
          resolve({
            available: false,
            monitors: [],
            reason: "No PipeWire audio outputs were found.",
          });
          return;
        }

        let defaultTarget = null;

        try {
          defaultTarget = await resolveDefaultSinkImpl();
        } catch (e) {
          defaultTarget = null;
        }

        resolve({
          available: true,
          monitors: audioMonitorSources(sinks, { defaultTarget }),
        });
      },
    );
  });
}

/**
 * Whether `target` is one of the currently discovered sinks.
 *
 * Used by `system-audio:start` so a renderer cannot point `pw-record` at an
 * arbitrary node name.
 *
 * @param {string | null | undefined} target
 * @param {{ execFileImpl?: typeof execFile, platform?: NodeJS.Platform }} [options]
 * @returns {Promise<boolean>}
 */
export async function isAudioMonitorTarget(target, options = {}) {
  if (typeof target !== "string" || target.length === 0) {
    return false;
  }

  const { monitors } = await listAudioMonitors(options);

  return monitors.some((monitor) => monitor.target === target);
}
