/**
 * Module-registry parity guard (2.0 hardening gate).
 *
 * The legacy worker builds its Gallery registry from the `import.meta.glob`
 * registrations in `application/worker/index.worker.js`: every `*.js` under
 * `renderers/2d/modules`, every `*.js` under `renderers/shader/modules`
 * (flattening array exports such as `Point10Effects.js`), and every `*.fs`
 * under `resources/isf` (named by file stem).
 *
 * `tests/fixtures/module-parity.json` is the committed snapshot of that
 * registry. This test recomputes the registry from the real files and fails
 * when the manifest, the JS glob list or the registered names diverge. The
 * Rust side reads the same manifest in
 * `crates/modv-app/src/module_registry.rs` (the count guard and the name-set
 * guard), so a module added/removed/renamed on either side trips one of the
 * two guards.
 *
 * Regenerate the manifest after an intentional registry change:
 *
 *     UPDATE_MODULE_PARITY=1 yarn test:unit tests/unit/module-parity.test.js
 *
 * Review the diff (it must only list the intended additions/removals).
 */

import fs from "fs";
import path from "path";
import { describe, expect, it } from "vitest";

const MANIFEST_PATH = path.resolve("tests/fixtures/module-parity.json");
const DEFAULT_SCENES_PATH = path.resolve(
  "crates/modv-engine/data/default-scenes.json",
);
const ISF_DIR = path.resolve(
  "resources/isf",
);

const twoDModuleFiles = import.meta.glob(
  "../../src/renderer/src/application/renderers/2d/modules/*.js",
  { eager: true },
);
const shaderModuleFiles = import.meta.glob(
  "../../src/renderer/src/application/renderers/shader/modules/*.js",
  { eager: true },
);

function fileStem(filePath) {
  return path.basename(filePath).replace(/\.js$/, "");
}

/** Rebuild the legacy registry exactly as `index.worker.js` does. */
function legacyRegistry() {
  const twoD = [];
  const point10 = [];

  for (const [filePath, module] of Object.entries(twoDModuleFiles)) {
    const definition = module.default;
    expect(definition, `${filePath} has a default export`).toBeTruthy();
    expect(definition.meta, `${filePath} has meta`).toBeTruthy();
    // The 27 Point10 wrappers live in the same folder and register through
    // `createPoint10Module`; the engine keeps them in its own registry.
    if (fileStem(filePath).startsWith("Point10")) {
      point10.push(definition.meta.name);
    } else {
      twoD.push(definition.meta.name);
    }
  }

  const shader = [];
  for (const [filePath, module] of Object.entries(shaderModuleFiles)) {
    const definitions = Array.isArray(module.default)
      ? module.default
      : [module.default];
    expect(
      definitions.length,
      `${filePath} registers something`,
    ).toBeGreaterThan(0);
    for (const definition of definitions) {
      expect(definition.meta, `${filePath} has meta`).toBeTruthy();
      shader.push(definition.meta.name);
    }
  }

  const isf = fs
    .readdirSync(ISF_DIR)
    .filter((fileName) => fileName.endsWith(".fs"))
    .map((fileName) => fileName.replace(/\.fs$/, ""));

  return {
    twoD: twoD.sort(),
    point10: point10.sort(),
    shader: shader.sort(),
    isf: isf.sort(),
  };
}

function manifestFor(groups) {
  const sorted = {
    twoD: [...groups.twoD].sort(),
    point10: [...groups.point10].sort(),
    shader: [...groups.shader].sort(),
    isf: [...groups.isf].sort(),
  };
  const total = Object.values(sorted).reduce(
    (sum, names) => sum + names.length,
    0,
  );

  return {
    $comment:
      "Generated from the legacy worker import.meta.glob registrations. " +
      "Consumed by tests/unit/module-parity.test.js and the Rust registry " +
      "guard in crates/modv-app/src/module_registry.rs.",
    counts: {
      twoD: sorted.twoD.length,
      point10: sorted.point10.length,
      shader: sorted.shader.length,
      isf: sorted.isf.length,
      total,
    },
    ...sorted,
  };
}

function defaultSceneModuleNames() {
  const scenes = JSON.parse(fs.readFileSync(DEFAULT_SCENES_PATH, "utf8"));
  const names = new Set();
  for (const scene of scenes) {
    for (const group of scene.groups || []) {
      for (const module of group.modules || []) {
        names.add(module.name);
      }
    }
  }
  return [...names].sort();
}

describe("legacy module registry parity", () => {
  const registry = legacyRegistry();

  it("has the audited 15 + 27 + 36 + 354 split", () => {
    expect(registry.twoD).toHaveLength(15);
    expect(registry.point10).toHaveLength(27);
    expect(registry.shader).toHaveLength(36);
    expect(registry.isf).toHaveLength(354);
    expect(
      registry.twoD.length +
        registry.point10.length +
        registry.shader.length +
        registry.isf.length,
    ).toBe(432);
  });

  it("matches the committed legacy manifest exactly", () => {
    const expected = manifestFor(registry);

    if (process.env.UPDATE_MODULE_PARITY === "1") {
      fs.writeFileSync(MANIFEST_PATH, `${JSON.stringify(expected, null, 2)}\n`);
    }

    const manifest = JSON.parse(fs.readFileSync(MANIFEST_PATH, "utf8"));

    // Friendly failure: report the symmetric difference, not a 400-line diff.
    for (const group of ["twoD", "point10", "shader", "isf"]) {
      const actual = new Set(registry[group]);
      const committed = new Set(manifest[group]);
      expect(
        {
          missingFromManifest: [...actual].filter(
            (name) => !committed.has(name),
          ),
          staleInManifest: [...committed].filter((name) => !actual.has(name)),
        },
        `${group} registry drifted from ${MANIFEST_PATH}`,
      ).toEqual({ missingFromManifest: [], staleInManifest: [] });
      expect(registry[group], `${group} order`).toEqual(manifest[group]);
    }

    expect(manifest.counts).toEqual(expected.counts);
  });

  it("registers each module name once across all four buckets", () => {
    const all = [
      ...registry.twoD,
      ...registry.point10,
      ...registry.shader,
      ...registry.isf,
    ];
    expect(new Set(all).size).toBe(all.length);
  });

  it("resolves every default-Scene module name", () => {
    const registered = new Set([
      ...registry.twoD,
      ...registry.point10,
      ...registry.shader,
      ...registry.isf,
    ]);
    const unresolved = defaultSceneModuleNames().filter(
      (name) => !registered.has(name),
    );

    expect(unresolved).toEqual([]);
  });
});
