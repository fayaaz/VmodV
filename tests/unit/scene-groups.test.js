import { describe, expect, it } from "vitest";
import { sceneModuleCount } from "../../src/renderer/src/application/scene-groups.js";

describe("sceneModuleCount", () => {
  const module = (id) => ({ id });

  it("sums the modules of every group", () => {
    const groups = [
      { name: "Background", modules: ["a", "b"] },
      { name: "Foreground", modules: ["c", "d", "e"] },
      { name: "Effects", modules: [] },
    ];

    expect(sceneModuleCount(groups)).toBe(5);
  });

  it("counts object-shaped module maps", () => {
    const groups = [
      { name: "Background", modules: { a: module("a"), b: module("b") } },
    ];

    expect(sceneModuleCount(groups)).toBe(2);
  });

  it("tolerates null holes left by a copied cell", () => {
    const groups = [
      { name: "Background", modules: ["a"] },
      null,
      {
        name: "Effects",
        modules: ["b", "c"],
      },
    ];

    expect(sceneModuleCount(groups)).toBe(3);
  });

  it("returns zero for a scene without groups", () => {
    expect(sceneModuleCount(undefined)).toBe(0);
    expect(sceneModuleCount(null)).toBe(0);
    expect(sceneModuleCount([])).toBe(0);
  });

  it("ignores groups without a modules array or map", () => {
    const groups = [
      { name: "Background" },
      { name: "Foreground", modules: 42 },
      null,
      { name: "Effects", modules: ["a"] },
    ];

    expect(sceneModuleCount(groups)).toBe(1);
  });
});
