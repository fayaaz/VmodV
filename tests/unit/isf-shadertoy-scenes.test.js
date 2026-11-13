import fs from "fs";
import path from "path";
import { describe, expect, it } from "vitest";
import { getIsfGalleryCategory } from "../../src/renderer/src/application/module-category.js";

const ISF_DIR = path.resolve(
  "src/renderer/src/application/renderers/isf/modules/isf",
);

function readIsf(fileName) {
  const source = fs.readFileSync(path.join(ISF_DIR, fileName), "utf8");
  const match = source.match(/\/\*([\s\S]*?)\*\//);

  if (!match) {
    throw new Error(`No ISF metadata block in ${fileName}`);
  }

  return { metadata: JSON.parse(match[1]), source };
}

const shadertoyScenes = [
  ["Helix Eye.fs", "RagePly", "https://www.shadertoy.com/view/cdSGRw"],
  ["Traced Tunnel.fs", "Shane", "https://www.shadertoy.com/view/tdjfDR"],
  ["Hot Shower.fs", "Kali", "https://www.shadertoy.com/view/4lf3Rj"],
];

describe("Shadertoy NC-SA scene ISF modules", () => {
  it.each(shadertoyScenes)("%s exists and declares 3D scenes", (fileName) => {
    const { metadata } = readIsf(fileName);
    expect(getIsfGalleryCategory(metadata)).toBe("3d-scenes");
    expect(metadata.CATEGORIES).toEqual(["GENERATOR"]);
  });

  it.each(shadertoyScenes)(
    "%s keeps attribution, source URL and the NC-SA licence",
    (fileName, author, url) => {
      const { metadata, source } = readIsf(fileName);

      expect(metadata.CREDIT).toContain(author);
      expect(metadata.CREDIT).toContain(url);
      expect(source).toContain(url);
      expect(source).toMatch(/CC BY-NC-SA 3\.0/);
    },
  );

  it.each(shadertoyScenes)("%s only uses GLSL ES 1.00 builtins", (fileName) => {
    const { source } = readIsf(fileName);

    expect(source).not.toMatch(
      /\b(texture|tanh|round|trunc|textureLod|textureGrad)\s*\(/,
    );
    expect(source).not.toMatch(/\buint\b/);
  });

  it.each(shadertoyScenes)(
    "%s is listed in THIRD_PARTY_NOTICES.md",
    (fileName, author, url) => {
      const notice = fs.readFileSync(
        path.resolve("THIRD_PARTY_NOTICES.md"),
        "utf8",
      );

      expect(notice).toContain(url);
      expect(notice).toContain(author);
      expect(notice).toMatch(/CC BY-NC-SA 3\.0/);
    },
  );
});

describe("Helix Eye.fs", () => {
  it("declares the raymarch, lens and blur passes", () => {
    const { metadata } = readIsf("Helix Eye.fs");

    expect(Array.isArray(metadata.PASSES)).toBe(true);
    expect(metadata.PASSES).toHaveLength(3);
    expect(metadata.PASSES[0].TARGET).toBe("helixRender");
    expect(metadata.PASSES[1].TARGET).toBe("lensRender");
    expect(metadata.PASSES[2].TARGET).toBeUndefined();
    expect(metadata.INPUTS).toEqual([]);
  });

  it("renders opaque black where the ray misses", () => {
    const { source } = readIsf("Helix Eye.fs");

    expect(source).toContain("PASSINDEX");
    expect(source).toContain("helixRender");
    expect(source).toContain("lensRender");
  });
});

describe("Traced Tunnel.fs", () => {
  it("ports the single-pass image pass without texture inputs", () => {
    const { metadata, source } = readIsf("Traced Tunnel.fs");

    expect(metadata.PASSES).toBeUndefined();
    // the interactive-shader-format JS parser dereferences INPUTS without a
    // default, so generators must declare it explicitly
    expect(metadata.INPUTS).toEqual([]);
    // the procedural replacement must not reference any image inputs
    expect(source).not.toContain("iChannel0");
    expect(source).not.toContain("iChannel1");
    expect(source).not.toContain("IMG_NORM_PIXEL");
  });
});

describe("Hot Shower.fs", () => {
  it("ports the single-pass image pass without texture inputs", () => {
    const { metadata, source } = readIsf("Hot Shower.fs");

    expect(metadata.PASSES).toBeUndefined();
    // the interactive-shader-format JS parser dereferences INPUTS without a
    // default, so generators must declare it explicitly
    expect(metadata.INPUTS).toEqual([]);
    // the procedural replacement must not reference any image inputs
    expect(source).not.toContain("iChannel0");
    expect(source).not.toContain("iChannel1");
    expect(source).not.toContain("IMG_NORM_PIXEL");
  });
});
