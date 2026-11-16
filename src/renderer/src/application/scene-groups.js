// Pure helpers for a saved Scene's `groups` array.
//
// The array can contain `null` holes: a cell copy into a Scene with fewer
// rows rebuilds `groups` with `null` for every undefined row so row indexes
// stay aligned (`scenes/copySceneCellToScene`). The apply loop skips the
// holes, and the UI helpers must not throw on them either.

/** Count the modules across every group, ignoring null holes. */
export function sceneModuleCount(groups) {
  if (!Array.isArray(groups)) {
    return 0;
  }

  return groups.reduce((count, group) => {
    if (!group || typeof group !== "object") {
      return count;
    }

    const modules = group.modules;

    if (Array.isArray(modules)) {
      return count + modules.length;
    }

    if (modules && typeof modules === "object") {
      return count + Object.keys(modules).length;
    }

    return count;
  }, 0);
}
