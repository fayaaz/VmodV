#!/usr/bin/env node
// Regenerates `crates/modv-engine/data/tween-keyframe-reference.json`.
//
// The engine ports `buildFrames` from
// `src/renderer/src/application/worker/store/modules/tweens.js`, which hands
// each channel to anime.js as a property **array**. anime treats an array of
// three or more values as keyframes, so the fixture records what anime.js
// 3.2.2 actually does with three and five `data` rows: how it divides the
// duration between keyframes, how the easing applies (per segment, not over
// the whole run), and what the first normalised tween interpolates from.
//
// The script is a standalone copy of the worker's `buildFrames` (same
// `newDuration` division, same frame recorder, same `Object.assign` frame
// snaps) with `store.state.fps`/`store.state.beats.bpm` passed in. Each entry
// also carries the raw `animation.duration` and per-keyframe tween timings so
// the Rust port can be checked against the structure, not just the frames.
//
// Run through the Docker toolchain from the repo root:
//
//   docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp \
//     -v "$PWD":/app -v modv-node-modules:/app/node_modules -w /app \
//     modv-toolchain:local node tools/tween-keyframe-reference.mjs
//
// (or `make yarn ARGS="node tools/tween-keyframe-reference.mjs"` if the
// toolchain exposes node through yarn).

import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// The worker shims anime's browser globals before importing it
// (`tweens.js:6-12`); mirror that here so the generator can run in Node.
globalThis.NodeList = class NodeList {};
globalThis.HTMLCollection = class HTMLCollection {};
globalThis.SVGElement = class SVGElement {};
globalThis.window = { Promise };

const { default: anime } = await import("animejs");

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputPath = resolve(
  repoRoot,
  "crates/modv-engine/data/tween-keyframe-reference.json",
);
// `anime.version` still says 3.2.1 in the 3.2.2 package, so read the package
// metadata the dependency actually resolves to.
const animePackage = JSON.parse(
  readFileSync(createRequire(import.meta.url).resolve("animejs/package.json")),
);

/** `buildFrames` from `tweens.js`, with the store state passed in. */
function buildFrames(
  {
    loop,
    data,
    duration,
    direction,
    easing,
    bpmDivision,
    useBpm,
    durationAsTotalTime,
    steps,
  },
  { fps, bpm },
) {
  let newDuration = 1000;

  if (useBpm) {
    const tempo = bpm || 120;
    const calculatedBpm = ((60 * 60) / tempo) * bpmDivision;

    newDuration = durationAsTotalTime
      ? calculatedBpm
      : calculatedBpm / data.length;
  } else {
    newDuration = durationAsTotalTime ? duration : duration / data.length;
  }

  const objOut = {};
  const mapped = {};

  for (let i = 0, len = data.length; i < len; i++) {
    const datum = data[i];

    let index = 0;
    for (let j = 0, lenj = datum.length; j < lenj; j++) {
      const value = datum[j];

      if (!(index in objOut)) {
        objOut[index] = value;
        mapped[index] = [];
      }

      mapped[index].push(value);
      index++;
    }
  }

  const animation = anime({
    targets: objOut,
    ...mapped,
    duration: newDuration,
    easing: steps ? `steps(${steps})` : easing || "linear",
    direction,
    autoplay: false,
    loop,
  });

  const animationCache = [];
  let frame = 0;
  const framesRequired = Math.round(newDuration / (1000 / fps)) || 1;

  let frameRecordingCompleted = false;
  let frameRecordingDirection = 1;

  if (loop) {
    while (!frameRecordingCompleted) {
      animation.seek((frame / framesRequired) * newDuration);
      animationCache.push(Object.assign({}, objOut));

      if (frame === framesRequired) {
        frameRecordingDirection = -1;
      }

      frame += frameRecordingDirection;

      if (frame === 0) {
        frameRecordingCompleted = true;
      }
    }
  } else {
    while (frame < framesRequired) {
      animation.seek((frame / framesRequired) * newDuration);
      animationCache.push(Object.assign({}, objOut));
      frame++;
    }
  }

  return {
    frames: animationCache,
    // What anime actually built: the instance duration and, per channel, the
    // normalised tween list. A two-value property array is anime's `[from,
    // to]` shorthand and stays a single tween; three or more values become one
    // tween per keyframe, each `duration / valueCount` long.
    animation: {
      duration: animation.duration,
      channels: Object.keys(animation.animations)
        .map((channel) => animation.animations[channel])
        .filter(Boolean)
        .map((anim) => ({
          channel: anim.property,
          duration: anim.duration,
          tweens: anim.tweens.map((tween) => ({
            start: tween.start,
            end: tween.end,
            from: tween.from.original,
            to: tween.to.original,
          })),
        })),
    },
  };
}

const configs = [
  {
    fps: 10,
    bpm: 120,
    tween: {
      id: "three-row-linear-10",
      loop: false,
      data: [[0], [0.5], [1]],
      duration: 3000,
      direction: "normal",
      easing: "linear",
      bpmDivision: 16,
      useBpm: false,
      durationAsTotalTime: false,
      steps: 0,
    },
  },
  {
    fps: 10,
    bpm: 120,
    tween: {
      id: "three-row-linear-loop-10",
      loop: true,
      data: [[0], [0.5], [1]],
      duration: 3000,
      direction: "normal",
      easing: "linear",
      bpmDivision: 16,
      useBpm: false,
      durationAsTotalTime: false,
      steps: 0,
    },
  },
  {
    fps: 10,
    bpm: 120,
    tween: {
      id: "three-row-linear-reverse-10",
      loop: false,
      data: [[0], [0.5], [1]],
      duration: 3000,
      direction: "reverse",
      easing: "linear",
      bpmDivision: 16,
      useBpm: false,
      durationAsTotalTime: false,
      steps: 0,
    },
  },
  {
    fps: 20,
    bpm: 120,
    tween: {
      id: "three-row-easeInOutQuad-loop-20",
      loop: true,
      data: [[0], [0.8], [0.2]],
      duration: 3000,
      direction: "alternate",
      easing: "easeInOutQuad",
      bpmDivision: 16,
      useBpm: false,
      durationAsTotalTime: false,
      steps: 0,
    },
  },
  {
    fps: 20,
    bpm: 120,
    tween: {
      id: "five-row-easeInOutQuad-20",
      loop: false,
      data: [[0], [0.2], [0.9], [0.4], [1]],
      duration: 5000,
      direction: "normal",
      easing: "easeInOutQuad",
      bpmDivision: 16,
      useBpm: false,
      durationAsTotalTime: false,
      steps: 0,
    },
  },
  {
    fps: 20,
    bpm: 120,
    tween: {
      id: "five-row-two-channel-cubic-loop-20",
      loop: true,
      data: [
        [0, 10],
        [0.25, 12],
        [0.6, 8],
        [0.4, 16],
        [1, 14],
      ],
      duration: 5000,
      direction: "normal",
      easing: "easeInOutCubic",
      bpmDivision: 16,
      useBpm: false,
      durationAsTotalTime: false,
      steps: 0,
    },
  },
  {
    fps: 10,
    bpm: 120,
    tween: {
      id: "three-row-steps4-10",
      loop: false,
      data: [[0], [0.5], [1]],
      duration: 3000,
      direction: "normal",
      easing: "linear",
      bpmDivision: 16,
      useBpm: false,
      durationAsTotalTime: false,
      steps: 4,
    },
  },
  {
    fps: 60,
    bpm: 128,
    tween: {
      id: "four-row-bpm-sine-loop-60",
      loop: true,
      data: [[0], [1], [0.25], [1]],
      duration: 1000,
      direction: "alternate",
      easing: "easeInOutSine",
      bpmDivision: 16,
      useBpm: true,
      durationAsTotalTime: false,
      steps: 0,
    },
  },
];

const entries = configs.map(({ fps, bpm, tween }) => {
  const { frames, animation } = buildFrames(tween, { fps, bpm });
  return {
    config: { fps, bpm, ...tween },
    frames,
    animation,
  };
});

const fixture = {
  source: "animejs",
  animeVersion: animePackage.version,
  generator: "tools/tween-keyframe-reference.mjs",
  notes: [
    "Generated by running the worker's buildFrames against the installed animejs 3.2.2.",
    "anime turns a property array of 3+ values into one tween per value; a two-value array is the [from, to] shorthand and stays a single tween.",
    "Every keyframe tween lasts newDuration / valueCount, and the tweens are chained back to back, so the animation lasts newDuration.",
    "The first keyframe tween interpolates the target's current value (data row 0) to keyframe 0 (also data row 0), so the first 1/N of the run holds row 0 before any movement.",
    "The easing (and steps quantisation) is evaluated per keyframe segment with local progress 0..1, not once over the whole run.",
    "direction reverse is unchanged from the two-row case: reset() installs the last keyframe, then seek() applies the reversed-time adjustment twice so frames 1.. sample normally.",
    "buildFrames already divides duration by data.length unless durationAsTotalTime is set, so a 3+ row wall-clock tween has segments of duration / N / N.",
  ],
  configs: entries,
};

writeFileSync(outputPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`Wrote ${outputPath}`);
console.log(
  entries
    .map(
      (entry) =>
        `${entry.config.id}: ${entry.frames.length} frames, anime duration ${entry.animation.duration}`,
    )
    .join("\n"),
);
