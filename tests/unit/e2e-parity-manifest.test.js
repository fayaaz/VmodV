import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// The Rust parity manifest is the inventory the port works from. This guard
// keeps it honest while the Electron suite still exists: every executed
// `test(...)` must appear once, every entry is either covered by a real Rust
// test or explicitly waived, and no unported `port-*` bucket may come back.
const ROOT = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
const MANIFEST = path.join(ROOT, "tests", "e2e-rust", "parity.json");
const STATUSES = new Set(["covered", "waived"]);
const EXPECTED_TOTAL = 147;
const EXPECTED_COVERED = 137;
const EXPECTED_WAIVED = 10;

function specFiles() {
  const files = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.name.endsWith(".spec.js")) {
        files.push(path.relative(ROOT, full).split(path.sep).join("/"));
      }
    }
  };
  walk(path.join(ROOT, "tests", "e2e"));
  return files.sort();
}

function stringAfterTest(source, start) {
  let index = start;
  while (" \t\n".includes(source[index])) {
    index += 1;
  }
  const quote = source[index];
  if (!['"', "'", "`"].includes(quote)) {
    throw new Error(`test() at ${start} does not start with a string`);
  }
  index += 1;
  let value = "";
  while (index < source.length) {
    const char = source[index];
    if (char === "\\") {
      value += source.slice(index, index + 2);
      index += 2;
      continue;
    }
    if (char === quote) {
      return value;
    }
    value += char;
    index += 1;
  }
  throw new Error(`unterminated test name at ${start}`);
}

function constantArrays(source) {
  const arrays = new Map();
  const pattern = /const\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*\[([\s\S]*?)\];/g;
  let match;
  while ((match = pattern.exec(source))) {
    const strings = [...match[2].matchAll(/"([^"]*)"/g)].map((m) => m[1]);
    const numbers = [...match[2].matchAll(/\b(\d+)\b/g)].map((m) => m[1]);
    const values = strings.length ? strings : numbers;
    if (values.length) {
      arrays.set(match[1], values);
    }
  }
  return arrays;
}

function expandTestNames(file, source) {
  const arrays = constantArrays(source);
  const names = [];
  const pattern = /^\s*test\(/gm;
  let match;
  while ((match = pattern.exec(source))) {
    const name = stringAfterTest(source, match.index + match[0].length);
    if (!name.includes("${")) {
      names.push(name);
      continue;
    }

    const before = source.slice(0, match.index);
    const forOf = [
      ...before.matchAll(
        /for\s*\((?:const|let)\s+(\w+)\s+of\s+([A-Za-z_][A-Za-z0-9_]*)\)/g,
      ),
    ];
    const lastForOf = forOf.at(-1);
    if (lastForOf && arrays.has(lastForOf[2])) {
      for (const value of arrays.get(lastForOf[2])) {
        names.push(name.replace(`\${${lastForOf[1]}}`, value));
      }
      continue;
    }

    const counted = [
      ...before.matchAll(
        /for\s*\(\s*let\s+(\w+)\s*=\s*0;\s*\1\s*<\s*([A-Za-z_][A-Za-z0-9_]*)\.length;\s*\1\+\+\s*\)/g,
      ),
    ];
    const lastCounted = counted.at(-1);
    if (lastCounted) {
      const arrayName = lastCounted[2];
      const body = source.slice(
        lastCounted.index + lastCounted[0].length,
        match.index,
      );
      const variable = body.match(
        new RegExp(`const\\s+(\\w+)\\s*=\\s*${arrayName}\\[`),
      );
      const placeholder = variable ? variable[1] : lastCounted[1];
      if (arrays.has(arrayName)) {
        for (const value of arrays.get(arrayName)) {
          names.push(name.replace(`\${${placeholder}}`, value));
        }
        continue;
      }
    }

    throw new Error(`${file}: cannot expand parameterised test ${name}`);
  }
  return names;
}

function rustTestExists(reference) {
  const colon = reference.lastIndexOf(":");
  if (colon < 1) {
    return false;
  }
  const file = reference.slice(0, colon);
  const name = reference.slice(colon + 1);
  const full = path.join(ROOT, file);
  if (!fs.existsSync(full)) {
    return false;
  }
  if (!file.endsWith(".rs")) {
    return true;
  }
  return fs.readFileSync(full, "utf8").includes(`fn ${name}(`);
}

function readManifest() {
  return JSON.parse(fs.readFileSync(MANIFEST, "utf8"));
}

describe("e2e parity manifest", () => {
  const manifest = readManifest();
  const manifestFiles = manifest.specs.map((spec) => spec.file);

  it("lists every spec file exactly once, sorted", () => {
    const expected = specFiles();
    expect(manifestFiles).toEqual(expected);
    expect(new Set(manifestFiles).size).toBe(manifestFiles.length);
    expect(manifestFiles).toEqual([...manifestFiles].sort());
  });

  it("lists every executed test exactly once and nothing stale", () => {
    const expected = [];
    for (const file of specFiles()) {
      const source = fs.readFileSync(path.join(ROOT, file), "utf8");
      for (const name of expandTestNames(file, source)) {
        expected.push(`${file}\u0000${name}`);
      }
    }

    const actual = [];
    for (const spec of manifest.specs) {
      for (const test of spec.tests) {
        actual.push(`${spec.file}\u0000${test.name}`);
      }
    }

    expect(actual.slice().sort()).toEqual(expected.slice().sort());
    expect(new Set(actual).size).toBe(actual.length);
    expect(actual.length).toBe(expected.length);
  });

  it("matches the final covered/waived totals", () => {
    const entries = manifest.specs.flatMap((spec) => spec.tests);
    const covered = entries.filter((test) => test.status === "covered");
    const waived = entries.filter((test) => test.status === "waived");

    expect(entries.length).toBe(EXPECTED_TOTAL);
    expect(covered.length).toBe(EXPECTED_COVERED);
    expect(waived.length).toBe(EXPECTED_WAIVED);
    expect(covered.length + waived.length).toBe(entries.length);
  });

  it("uses a known status and names real Rust coverage", () => {
    const problems = [];
    for (const spec of manifest.specs) {
      for (const test of spec.tests) {
        const label = `${spec.file}: ${test.name}`;
        if (!STATUSES.has(test.status)) {
          problems.push(`${label}: unknown status ${test.status}`);
        }
        if (test.status === "covered" && !(test.rust || []).length) {
          problems.push(`${label}: covered without a Rust test`);
        }
        if (test.status === "waived" && !String(test.notes || "").trim()) {
          problems.push(`${label}: waived without a concrete reason`);
        }
        for (const reference of test.rust || []) {
          if (!rustTestExists(reference)) {
            problems.push(`${label}: missing Rust reference ${reference}`);
          }
        }
      }
    }
    expect(problems).toEqual([]);
  });
});
