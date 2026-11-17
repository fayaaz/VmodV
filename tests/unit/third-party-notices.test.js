import fs from "fs";
import path from "path";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(".");
const NOTICES_PATH = path.join(ROOT, "THIRD_PARTY_NOTICES.md");
const notices = fs.readFileSync(NOTICES_PATH, "utf8");
const readme = fs.readFileSync(path.join(ROOT, "README.md"), "utf8");

const NC_SA = /non-?commercial|by-nc-sa/i;

// ————— Cargo.lock (Rust dependency inventory) —————

const CARGO_LOCK_PATH = path.join(ROOT, "crates/Cargo.lock");

/**
 * `[[package]]` entries from `crates/Cargo.lock`. Entries without a `source`
 * are the workspace's own crates (first-party), everything else comes from a
 * registry and therefore needs a notice row.
 */
function cargoLockPackages() {
  const lock = fs.readFileSync(CARGO_LOCK_PATH, "utf8");
  const packages = [];
  let current = null;
  for (const line of lock.split(/\r?\n/)) {
    if (line === "[[package]]") {
      current = {};
      continue;
    }
    if (!current) {
      continue;
    }
    if (line.startsWith("name = ")) {
      current.name = JSON.parse(line.slice(7).trim());
    } else if (line.startsWith("version = ")) {
      current.version = JSON.parse(line.slice(10).trim());
    } else if (line.startsWith("source = ")) {
      current.source = JSON.parse(line.slice(9).trim());
    } else if (line === "") {
      if (current.name) {
        packages.push(current);
      }
      current = null;
    }
  }
  if (current?.name) {
    packages.push(current);
  }
  return packages;
}

const cargoPackages = cargoLockPackages();
const workspaceCrates = cargoPackages
  .filter((p) => !p.source)
  .map((p) => p.name);
/** Registry packages, de-duplicated by name and sorted. */
const rustDependencies = [
  ...new Set(cargoPackages.filter((p) => p.source).map((p) => p.name)),
].sort();

/** The `## Rust engine dependencies` section of the notices. */
function noticesSection(heading) {
  const start = notices.indexOf(`## ${heading}`);
  expect(start, `notices must have a "## ${heading}" section`).toBeGreaterThan(
    -1,
  );
  const afterStart = start + heading.length;
  const next = notices.indexOf("\n## ", afterStart);
  return next === -1 ? notices.slice(start) : notices.slice(start, next);
}

const rustSection = noticesSection("Rust engine dependencies");

/**
 * One row per registry crate: `| `name` | SPDX licence | upstream link |`.
 */
function rustDependencyRows(section) {
  const rows = new Map();
  for (const match of section.matchAll(
    /^\| `([^`]+)` \| (.+?) \| (https?:\/\/[^\s|]+) \|$/gm,
  )) {
    rows.set(match[1], { licence: match[2].trim(), url: match[3] });
  }
  return rows;
}

const rustRows = rustDependencyRows(rustSection);

/**
 * SPDX identifiers used by the workspace's dependency graph (plus the common
 * permissive ids, so a new crate does not need a list edit unless it adds an
 * identifier nobody has used yet).
 */
const SPDX_IDENTIFIERS = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BSL-1.0",
  "CC-BY-3.0",
  "CC-BY-4.0",
  "CC0-1.0",
  "GPL-2.0-only",
  "ISC",
  "LGPL-2.1-or-later",
  "LLVM-exception",
  "MIT",
  "MPL-2.0",
  "NCSA",
  "OFL-1.1",
  "Ubuntu-font-1.0",
  "Unicode-DSL-1.0",
  "Unicode-3.0",
  "Unlicense",
  "Zlib",
]);

/**
 * Validate an SPDX licence expression: identifiers joined by `AND`/`OR`, with
 * optional parentheses and `WITH` exceptions. Anything else (free text,
 * shorthand like `MIT/Apache-2.0`) fails.
 */
function isSpdxExpression(expression) {
  const tokens = expression.match(/[()]|[^\s()]+/g) ?? [];
  let expectAtom = true;
  let depth = 0;
  for (const token of tokens) {
    if (token === "(") {
      // Opening a group is only valid where an atom is expected.
      if (!expectAtom) {
        return false;
      }
      depth += 1;
      continue;
    }
    if (token === ")") {
      if (expectAtom || depth === 0) {
        return false;
      }
      depth -= 1;
      continue;
    }
    if (token === "AND" || token === "OR") {
      if (expectAtom) {
        return false;
      }
      expectAtom = true;
      continue;
    }
    if (token === "WITH") {
      // `A WITH <exception>`: an exception identifier must follow.
      if (expectAtom) {
        return false;
      }
      expectAtom = true;
      continue;
    }
    if (!expectAtom || !SPDX_IDENTIFIERS.has(token)) {
      return false;
    }
    expectAtom = false;
  }
  return !expectAtom && depth === 0;
}

/** Hosts accepted as plausible upstream links. */
const KNOWN_FORGES =
  /(?:^|\.)(?:github\.com|gitlab\.com|freedesktop\.org|redox-os\.org|codeberg\.org|crates\.io|docs\.rs|bitbucket\.org|sourceforge\.net)\//;

function plausibleUpstream(name, url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  if (parsed.protocol !== "https:") {
    return false;
  }
  return KNOWN_FORGES.test(`${parsed.host}/`) || url.includes(name);
}

/**
 * Licence ids that carry obligations beyond "keep the notice": copyleft or
 * attribution terms that the notices must spell out.
 */
const OBLIGATION_LICENCE =
  /MPL-2\.0|CC-BY-\d(?:\.\d)?(?:-[^ ]+)?|NCSA|OFL-1\.1|Ubuntu-font-1\.0/g;

function obligationLicences(rows) {
  const found = new Set();
  for (const { licence } of rows.values()) {
    for (const match of licence.matchAll(OBLIGATION_LICENCE)) {
      found.add(match[0]);
    }
  }
  return [...found];
}

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

describe("third-party notices cover the Rust dependency graph", () => {
  it("the Cargo.lock scanner finds the registry packages and the workspace crates", () => {
    // Guards the scanner itself: a broken parse would make the coverage
    // assertions below pass vacuously.
    expect(rustDependencies.length).toBeGreaterThan(500);
    for (const crate of ["wgpu", "winit", "naga", "tokio"]) {
      expect(rustDependencies).toContain(crate);
    }
    // The workspace's own crates have no registry source and stay first-party.
    expect(workspaceCrates).toEqual([
      "modv-app",
      "modv-engine",
      "modv-ipc",
      "modv-media",
      "modv-render",
      "modv-renderers",
      "modv-ui",
    ]);
  });

  it.each(rustDependencies.map((name) => [name]))(
    "%s has a Rust dependency row",
    (name) => {
      expect(
        rustRows.has(name),
        `${name} is in crates/Cargo.lock but has no row in the "Rust engine dependencies" table`,
      ).toBe(true);
    },
  );

  it("no made-up rows: every Rust table entry resolves in Cargo.lock", () => {
    const stale = [...rustRows.keys()].filter(
      (name) => !rustDependencies.includes(name),
    );
    expect(
      stale,
      `rows without a Cargo.lock package (stale entries?): ${stale.join(", ")}`,
    ).toEqual([]);
    expect(rustRows.size).toBeGreaterThan(500);
  });

  it("the rows are sorted and one-per-crate", () => {
    const names = [...rustRows.keys()];
    expect(names.length, "duplicate rows").toBe(new Set(names).size);
    const sorted = [...names].sort((a, b) => a.localeCompare(b));
    expect(names).toEqual(sorted);
  });

  it("every row carries a valid SPDX licence expression", () => {
    for (const [name, row] of rustRows) {
      expect(
        isSpdxExpression(row.licence),
        `${name} licence "${row.licence}" is not a valid SPDX expression`,
      ).toBe(true);
    }
  });

  it("every row links to a plausible upstream", () => {
    for (const [name, row] of rustRows) {
      expect(
        plausibleUpstream(name, row.url),
        `${name} upstream "${row.url}" is neither a known forge nor a link containing the crate name`,
      ).toBe(true);
    }
  });

  it("notes the licence obligations of the copyleft and attribution crates", () => {
    const obligations = obligationLicences(rustRows);
    expect(
      obligations.length,
      "the obligation matcher found nothing — coloured (MPL-2.0) should be in the graph",
    ).toBeGreaterThan(0);

    // Bullets under the section carry the obligations, and stay tied to the
    // crates that actually carry the licence. Wrapped bullet text is joined
    // so a crate named on a continuation line still counts.
    const bullets = [];
    for (const line of rustSection.split(/\r?\n/)) {
      if (/^- /.test(line)) {
        bullets.push(line.trim());
      } else if (
        bullets.length &&
        line.trim() &&
        !/^[#|]/.test(line) &&
        !/^-\s/.test(line)
      ) {
        bullets[bullets.length - 1] += ` ${line.trim()}`;
      }
    }
    for (const id of obligations) {
      expect(
        bullets.some((bullet) => bullet.includes(id)),
        `${id} crates need an obligation note`,
      ).toBe(true);
    }
    for (const bullet of bullets) {
      const names = [...rustRows.keys()].filter((name) =>
        bullet.includes(`\`${name}\``),
      );
      expect(
        names.length,
        `obligation bullet "${bullet.slice(0, 60)}…" names no crate from the table`,
      ).toBeGreaterThan(0);
    }
  });
});
