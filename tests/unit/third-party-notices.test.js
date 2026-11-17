import fs from "fs";
import path from "path";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(".");
const NOTICES_PATH = path.join(ROOT, "THIRD_PARTY_NOTICES.md");
const notices = fs.readFileSync(NOTICES_PATH, "utf8");
const readme = fs.readFileSync(path.join(ROOT, "README.md"), "utf8");

const NC_SA = /non-?commercial|by-nc-sa/i;

function walk(dir, extension) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      return walk(full, extension);
    }
    return entry.name.endsWith(extension) ? [full] : [];
  });
}

function relative(file) {
  return path.relative(ROOT, file).split(path.sep).join("/");
}

function credit(source) {
  const match = source.match(/"CREDIT"\s*:\s*"((?:[^"\\]|\\.)*)"/);
  if (!match) {
    return null;
  }
  const raw = match[1].replace(/\\"/g, '"').trim();
  // Ports name the upstream author after "by", followed by the source URL.
  const port = raw.match(/^Port of .*?\bby\s+([^()]+?)\s*(?:\(|$)/i);
  if (port) {
    return port[1].trim();
  }
  return raw.replace(/^by\s+/i, "").trim();
}

/**
 * Every in-tree source file that carries an NC-SA marker must be named in the
 * notices. The list is derived from the files themselves, so a new port that
 * copies an NC-SA header is caught without editing this test.
 */
function ncSaSources() {
  const isf = walk(
    path.join(ROOT, "src/renderer/src/application/renderers/isf/modules"),
    ".fs",
  );
  const shader = walk(
    path.join(ROOT, "src/renderer/src/application/renderers/shader/modules"),
    ".frag",
  );
  const point10Js = walk(
    path.join(ROOT, "src/renderer/src/application/point10/scenes"),
    ".js",
  );
  const point10Rust = walk(
    path.join(ROOT, "crates/modv-renderers/src/point10/scenes"),
    ".rs",
  );

  return [...isf, ...shader, ...point10Js, ...point10Rust]
    .map((file) => ({ file, source: fs.readFileSync(file, "utf8") }))
    .filter(({ source }) => NC_SA.test(source))
    .map(({ file, source }) => ({
      path: relative(file),
      credit: credit(source),
    }));
}

const sources = ncSaSources();

describe("third-party notices cover NC-SA sources", () => {
  it("finds the known NC-SA modules in the tree", () => {
    // Guards the scanner itself: if the header wording changes, the
    // coverage assertions below would otherwise pass vacuously.
    expect(sources.length).toBeGreaterThanOrEqual(20);
    expect(sources.map((entry) => entry.path)).toContain(
      "src/renderer/src/application/renderers/isf/modules/isf/UltimateFlame.fs",
    );
    expect(sources.map((entry) => entry.path)).toContain(
      "src/renderer/src/application/point10/scenes/Boxy.js",
    );
    expect(sources.map((entry) => entry.path)).toContain(
      "crates/modv-renderers/src/point10/scenes/mirror_cube.rs",
    );
  });

  it.each(sources.map((entry) => [entry.path, entry]))(
    "%s is listed in THIRD_PARTY_NOTICES.md",
    (modulePath) => {
      expect(notices).toContain(modulePath);
    },
  );

  it.each(
    sources
      .filter((entry) => entry.credit)
      .map((entry) => [entry.path, entry.credit]),
  )("%s names its author (%s)", (modulePath, author) => {
    expect(notices, modulePath).toContain(author);
  });
});

describe("notices carry the corrected dependency attributions", () => {
  it.each([
    // st_Ms2SD1: the file names TDM and states NC-SA 3.0 explicitly.
    ["st_Ms2SD1.fs.fs", ["Alexander Alekseev", "TDM"]],
    // spherical-shader-tut: in-file "CC 3.0 Share Alike" with a Shadertoy
    // source cannot be less restrictive than the Shadertoy default.
    ["spherical-shader-tut.fs", ["variant not specified"]],
    // Perlin Transition: 4dS3Wd noise by Morgan McGuire.
    ["Perlin Transition.fs", ["Morgan McGuire", "4dS3Wd"]],
    // Helix Eye: iq raymarch/fog ports and the neilmendoza.com matrix.
    ["Helix Eye.fs", ["Inigo Quilez", "neilmendoza.com"]],
    // Traced Tunnel: camera/tick code retained from NuSan.
    ["Traced Tunnel.fs", ["NuSan"]],
  ])("%s row covers %j", (fileName, tokens) => {
    expect(notices).toContain(fileName);
    for (const token of tokens) {
      expect(notices).toContain(token);
    }
  });

  it("attributes the bundled PCB texture", () => {
    expect(notices).toContain("pcb.jpg");
    expect(notices).toContain("Miguel Alba");
  });

  it("attributes Around Plus despite its missing in-file license line", () => {
    expect(notices).toContain(
      "src/renderer/src/application/point10/scenes/AroundPlus.js",
    );
    expect(notices).toContain(
      "crates/modv-renderers/src/point10/scenes/around_plus.rs",
    );
    expect(notices).toContain("Around And Around");
  });

  it("does not let the README licence badge imply MIT for bundled assets", () => {
    expect(readme).toContain("THIRD_PARTY_NOTICES.md");
    expect(readme).not.toContain("img.shields.io/github/license");
  });
});
