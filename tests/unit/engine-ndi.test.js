/**
 * WS-11 NDI mirror gate.
 *
 * The engine owns NDI in `crates/modv-app/src/ndi_runtime.rs` and emits the
 * same Vuex mutations the legacy `worker/store/modules/ndi.js` produced. This
 * test replays the engine's delta shapes into the real mirror store and asserts
 * the UI contract the panel relies on:
 *
 * - `state.ndi` settings,
 * - `state.ndi.receivers[id].receiver.source.name` (the receiver row label),
 * - `state.outputs.auxillary[id]` with group `NDI` (the texture picker lists
 *   the receiver as a canvas source),
 * - every mutation type the engine emits exists in the mirror store.
 *
 * libndi is not needed: these are the store-side records only.
 */

import { describe, expect, test } from "vitest";

/**
 * The worker store touches the worker global at module scope (`tweens.js`
 * shims anime.js DOM globals onto `self`), so recreate it before importing the
 * store. `document` stays undefined, so the persisted-state plugin is skipped.
 */
globalThis.self = globalThis;

const { default: store } =
  await import("../../src/renderer/src/application/worker/store/index.js");

const receiverRecord = (overrides = {}) => ({
  id: "receiver-1",
  outputId: "ndi:receiver-1",
  receiver: {
    source: {
      name: "STUDIO (Test Pattern)",
      urlAddress: "10.0.0.2:5961",
    },
  },
  enabled: true,
  state: "connected",
  source: "STUDIO (Test Pattern)",
  error: null,
  width: 1920,
  height: 1080,
  frames: 12,
  ...overrides,
});

describe("engine NDI deltas and the mirror store", () => {
  test("every emitted mutation type is implemented by the mirror store", () => {
    const types = [
      "ndi/SET_OUTPUT_ENABLED",
      "ndi/SET_OUTPUT_NAME",
      "ndi/SET_OUTPUT_SIZE",
      "ndi/SET_FOLLOW_MODV_OUTPUT_SIZE",
      "ndi/SET_FOLLOW_MODV_FPS",
      "ndi/SET_OUTPUT_TARGET_FPS",
      "ndi/SET_SOURCES",
      "ndi/SET_DISCOVERING",
      "ndi/SET_DISCOVERY_OPTIONS",
      "ndi/ADD_RECIEVER",
      "ndi/UPDATE_RECIEVER",
      "ndi/DELETE_RECIEVER",
      "outputs/ADD_AUXILLARY",
      "outputs/REMOVE_AUXILLARY",
    ];

    const unknown = types.filter((type) => !store._mutations[type]);
    expect(unknown).toEqual([]);
  });

  test("the sender settings land in state.ndi", () => {
    store.commit("ndi/SET_OUTPUT_ENABLED", true);
    store.commit("ndi/SET_OUTPUT_NAME", "Studio Output");
    store.commit("ndi/SET_OUTPUT_SIZE", { width: 1280 });
    store.commit("ndi/SET_OUTPUT_SIZE", { height: 720 });
    store.commit("ndi/SET_FOLLOW_MODV_OUTPUT_SIZE", false);
    store.commit("ndi/SET_FOLLOW_MODV_FPS", false);
    store.commit("ndi/SET_OUTPUT_TARGET_FPS", 30);

    expect(store.state.ndi.outputEnabled).toBe(true);
    expect(store.state.ndi.outputName).toBe("Studio Output");
    expect(store.state.ndi.outputWidth).toBe(1280);
    expect(store.state.ndi.outputHeight).toBe(720);
    expect(store.state.ndi.followModVOutputSize).toBe(false);
    expect(store.state.ndi.followModVFps).toBe(false);
    expect(store.state.ndi.outputTargetFps).toBe(30);
  });

  test("a receiver record feeds the receiver row and the canvas picker", () => {
    // The engine emits these in this order (aux output first, like
    // `outputs/getAuxillaryOutput` before `ADD_RECIEVER` in the legacy action).
    store.commit("outputs/ADD_AUXILLARY", {
      id: "ndi:receiver-1",
      name: "STUDIO (Test Pattern)",
      group: "NDI",
      reactToResize: false,
    });
    store.commit("ndi/ADD_RECIEVER", receiverRecord());

    const row = store.state.ndi.receivers["receiver-1"];
    expect(row.receiver.source.name).toBe("STUDIO (Test Pattern)");
    expect(row.enabled).toBe(true);
    expect(row.state).toBe("connected");
    expect(row.error).toBeNull();

    const aux = store.state.outputs.auxillary["ndi:receiver-1"];
    expect(aux).toMatchObject({
      id: "ndi:receiver-1",
      name: "STUDIO (Test Pattern)",
      group: "NDI",
      reactToResize: false,
    });

    // A source that disappears: the engine emits UPDATE with the error.
    store.commit(
      "ndi/UPDATE_RECIEVER",
      receiverRecord({
        enabled: false,
        state: "error",
        error: "the NDI source disappeared",
      }),
    );
    expect(store.state.ndi.receivers["receiver-1"].state).toBe("error");
    expect(store.state.ndi.receivers["receiver-1"].error).toBe(
      "the NDI source disappeared",
    );

    // Removal mirrors the legacy disable → drop aux → delete sequence.
    store.commit("outputs/REMOVE_AUXILLARY", "ndi:receiver-1");
    store.commit("ndi/DELETE_RECIEVER", receiverRecord());
    expect(store.state.ndi.receivers["receiver-1"]).toBeUndefined();
    expect(store.state.outputs.auxillary["ndi:receiver-1"]).toBeUndefined();
  });

  test("discovery records land in state.ndi", () => {
    store.commit("ndi/SET_DISCOVERING", true);
    expect(store.state.ndi.discovering).toBe(true);

    store.commit("ndi/SET_SOURCES", [
      { name: "STUDIO (Test Pattern)", urlAddress: "10.0.0.2:5961" },
    ]);
    expect(store.state.ndi.sources).toEqual([
      { name: "STUDIO (Test Pattern)", urlAddress: "10.0.0.2:5961" },
    ]);

    store.commit("ndi/SET_DISCOVERY_OPTIONS", { showLocalSources: false });
    expect(store.state.ndi.discoveryOptions.showLocalSources).toBe(false);

    store.commit("ndi/SET_DISCOVERING", false);
    expect(store.state.ndi.discovering).toBe(false);
  });
});
