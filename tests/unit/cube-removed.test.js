import { existsSync, readFileSync, readdirSync } from "node:fs";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const SRC = join(ROOT, "src");
const TESTS = join(ROOT, "tests");
const THIS_FILE = fileURLToPath(import.meta.url);

const REMOVED_FILES = [
  "src/renderer/src/application/renderers/three/index.js",
  "src/renderer/src/application/renderers/three/modules/Cube.js",
];

// The `three` renderer type is gone: these needles must not appear anywhere
// in src or tests again. They are specific to the Cube module and its wiring
// (Point10/Cube Floor/Mirror Cube/etc. are unrelated and unaffected).
const FORBIDDEN = [
  "renderers/three/",
  "renderers/three/modules",
  'name: "Cube"',
  'type: "three"',
];

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);

    return entry.isDirectory() ? walk(path) : [path];
  });
}

describe("the three renderer and Cube module are removed", () => {
  it("removes the module and renderer files", () => {
    for (const relativePath of REMOVED_FILES) {
      expect(existsSync(join(ROOT, relativePath)), relativePath).toBe(false);
    }
  });

  it("leaves no source or test reference behind", () => {
    const offenders = [];

    for (const path of [...walk(SRC), ...walk(TESTS)]) {
      if (
        path === THIS_FILE ||
        !["", ".js", ".vue", ".json"].includes(extname(path))
      ) {
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

  it("is not referenced by the seeded default Scenes", () => {
    const scenesPath = join(
      ROOT,
      "crates/modv-engine/data/default-scenes.json",
    );
    const contents = readFileSync(scenesPath, "utf8");

    expect(contents).not.toContain('"Cube"');
  });
});
