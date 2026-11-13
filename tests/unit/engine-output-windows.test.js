/**
 * Engine-mode Output Windows — renderer side.
 *
 * The Rust engine owns the native windows; the renderer only dispatches the
 * two actions and reads the mirror list. These tests pin the action names and
 * payloads (so the engine's `windows/*` handlers are the only place that can
 * drift) and the mirror mutations that keep `state.windows` in step.
 */

import { describe, expect, test } from "vitest";

import {
  ENGINE_OUTPUT_WINDOW_CREATE,
  ENGINE_OUTPUT_WINDOW_REMOVE,
  closeAllEngineOutputWindows,
  closeEngineOutputWindow,
  createEngineOutputWindow,
  engineOutputWindowIds,
} from "../../src/renderer/src/application/engine-output-windows.js";
import windows from "../../src/renderer/src/application/worker/store/modules/windows.js";

/** A store facade that records dispatches and answers like the engine. */
function fakeStore({ state = {}, created = "window-new" } = {}) {
  const calls = [];
  return {
    state,
    calls,
    dispatch(type, payload) {
      calls.push({ type, payload });
      return type === ENGINE_OUTPUT_WINDOW_CREATE
        ? Promise.resolve(created)
        : Promise.resolve(null);
    },
  };
}

describe("engine output window actions", () => {
  test("create dispatches windows/createWindow and returns the engine id", async () => {
    const store = fakeStore({ created: "abc-123" });
    await expect(createEngineOutputWindow(store)).resolves.toBe("abc-123");
    expect(store.calls).toEqual([
      { type: "windows/createWindow", payload: undefined },
    ]);
  });

  test("create resolves null when the engine answers without an id", async () => {
    const store = fakeStore({ created: null });
    await expect(createEngineOutputWindow(store)).resolves.toBeNull();
  });

  test("close dispatches windows/removeWindow with the id", async () => {
    const store = fakeStore();
    await expect(closeEngineOutputWindow(store, "w1")).resolves.toBe(true);
    expect(store.calls).toEqual([
      { type: ENGINE_OUTPUT_WINDOW_REMOVE, payload: { id: "w1" } },
    ]);
  });

  test("close ignores blank or missing ids instead of dispatching", async () => {
    const store = fakeStore();
    await expect(closeEngineOutputWindow(store, "")).resolves.toBe(false);
    await expect(closeEngineOutputWindow(store, null)).resolves.toBe(false);
    expect(store.calls).toEqual([]);
  });

  test("lists only real ids from the mirror state", () => {
    expect(engineOutputWindowIds(undefined)).toEqual([]);
    expect(engineOutputWindowIds({ windows: null })).toEqual([]);
    expect(
      engineOutputWindowIds({
        windows: { a: { id: "a" }, b: null, c: { id: "c" } },
      }),
    ).toEqual(["a", "c"]);
  });

  test("closeAll closes every mirrored window", async () => {
    const store = fakeStore({
      state: { windows: { a: { id: "a" }, b: { id: "b" } } },
    });
    await expect(closeAllEngineOutputWindows(store)).resolves.toEqual([
      true,
      true,
    ]);
    expect(store.calls).toEqual([
      { type: ENGINE_OUTPUT_WINDOW_REMOVE, payload: { id: "a" } },
      { type: ENGINE_OUTPUT_WINDOW_REMOVE, payload: { id: "b" } },
    ]);
  });
});

describe("windows mirror mutations", () => {
  test("ADD_WINDOW keys the window record by id", () => {
    const state = {};
    windows.mutations.ADD_WINDOW(state, { id: "w1", title: "A", width: 300 });
    expect(state.w1).toMatchObject({ id: "w1", title: "A" });
  });

  test("REMOVE_WINDOW drops one entry and tolerates an unknown id", () => {
    const state = { w1: { id: "w1" }, w2: { id: "w2" } };
    windows.mutations.REMOVE_WINDOW(state, "w1");
    expect(Object.keys(state)).toEqual(["w2"]);
    expect(() =>
      windows.mutations.REMOVE_WINDOW(state, "missing"),
    ).not.toThrow();
  });

  test("SET_WINDOWS replaces the list (engine state/sync replay)", () => {
    const state = { stale: { id: "stale" } };
    windows.mutations.SET_WINDOWS(state, [
      { id: "w1", title: "A" },
      { id: "w2", title: "B" },
      { title: "no id" },
    ]);
    expect(Object.keys(state).sort()).toEqual(["w1", "w2"]);
    expect(state.w1.title).toBe("A");

    // A later sync with no windows clears the list (an engine restart opens
    // none; the mirror must not keep the old ones).
    windows.mutations.SET_WINDOWS(state, []);
    expect(Object.keys(state)).toEqual([]);
  });

  test("UPDATE_WINDOW writes one field on an existing entry", () => {
    const state = { w1: { id: "w1", fullscreen: false } };
    windows.mutations.UPDATE_WINDOW(state, {
      id: "w1",
      key: "fullscreen",
      value: true,
    });
    expect(state.w1.fullscreen).toBe(true);
  });
});
