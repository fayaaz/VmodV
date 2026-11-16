import { existsSync, readFileSync, readdirSync } from "node:fs";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const SRC = join(ROOT, "src");
const TESTS = join(ROOT, "tests");
const THIS_FILE = fileURLToPath(import.meta.url);

const REMOVED_FILES = [
  "src/renderer/src/application/point10/scenes/SpectrumBars.js",
  "src/renderer/src/application/point10/scenes/Ferrofluid.js",
  "src/renderer/src/application/renderers/2d/modules/Point10SpectrumBars.js",
  "src/renderer/src/application/renderers/2d/modules/Point10Ferrofluid.js",
  "src/renderer/src/application/point10/scenes/GeometricTunnel.js",
  "src/renderer/src/application/renderers/2d/modules/Point10GeometricTunnel.js",
  // The Rust port and its exclusive SSAO pass are removed with it.
  "crates/modv-renderers/src/point10/scenes/geometric_tunnel.rs",
  "crates/modv-renderers/src/point10/postfx/ssao.rs",
  "crates/modv-renderers/src/point10/postfx/ssao.wgsl",
];

const FORBIDDEN = [
  "Point10 Spectrum Bars",
  "Point10 Ferrofluid",
  "Point10 Old Movie",
  "SpectrumBars",
  "scenes/Ferrofluid",
  "Point10Ferrofluid",
  // Geometric Tunnel was removed in DEFAULTS_VERSION 16.
  "Point10 Geometric Tunnel",
  "Point10GeometricTunnel",
  "GeometricTunnel",
  "geometric_tunnel",
];

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);

    return entry.isDirectory() ? walk(path) : [path];
  });
}

describe("removed Point10 effects and scenes", () => {
  it("removes the module and scene files", () => {
    for (const relativePath of REMOVED_FILES) {
      expect(existsSync(join(ROOT, relativePath)), relativePath).toBe(false);
    }
  });

  it("the seeded default Scenes do not reference it", () => {
    const contents = readFileSync(
      join(ROOT, "crates/modv-engine/data/default-scenes.json"),
      "utf8",
    );

    expect(contents).not.toContain("Point10 Geometric Tunnel");
    expect(contents).not.toContain("GeometricTunnel");
  });

  it("leaves no source or test reference behind", () => {
    const offenders = [];

    for (const path of [...walk(SRC), ...walk(TESTS)]) {
      if (path === THIS_FILE || !["", ".js", ".vue"].includes(extname(path))) {
        continue;
      }

      const contents = readFileSync(path, "utf8");

      for (const needle of FORBIDDEN) {
        if (contents.includes(needle)) {
          offenders.push(`${path.slice(ROOT.length + 1)}: ${needle}`);
        }
      }
    }

    expect(offenders).toEqual([]);
  });
});
