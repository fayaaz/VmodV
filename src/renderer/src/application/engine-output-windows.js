// Engine-mode Output Windows.
//
// The Rust engine owns the native windows (`crates/modv-app/src/windows.rs`):
// one winit event loop, one device/queue, and extra `winit::Window` +
// `wgpu::Surface` pairs that blit the primary composited frame. The renderer
// therefore never calls `window.open` in engine mode — it asks the engine to
// open or close a window, and the engine's `windows/ADD_WINDOW` /
// `windows/REMOVE_WINDOW` deltas keep the mirror store's `state.windows` list
// in step.
//
// Kept free of Electron/DOM imports so the payloads are unit-testable
// (`tests/unit/engine-output-windows.test.js`).

/** Vuex action that mints a window and queues the native creation. */
export const ENGINE_OUTPUT_WINDOW_CREATE = "windows/createWindow";

/** Vuex action that closes a window and emits `REMOVE_WINDOW`. */
export const ENGINE_OUTPUT_WINDOW_REMOVE = "windows/removeWindow";

/**
 * Ids of every Output Window the mirror knows about.
 *
 * @param {object} [state] the renderer's mirror state (`modV.store.state`)
 * @returns {string[]}
 */
export function engineOutputWindowIds(state) {
  const windows = state?.windows;
  if (!windows || typeof windows !== "object") {
    return [];
  }

  return Object.keys(windows).filter((id) => windows[id]);
}

/**
 * Ask the engine for one more native Output Window.
 *
 * The engine mints the id (the legacy action did the same) and returns it, so
 * a caller that wants to track the window can await this.
 *
 * @param {{dispatch: Function}} store the `modV.store` facade
 * @returns {Promise<string | null>}
 */
export function createEngineOutputWindow(store) {
  return Promise.resolve(store.dispatch(ENGINE_OUTPUT_WINDOW_CREATE)).then(
    (id) => (typeof id === "string" && id.length > 0 ? id : null),
  );
}

/**
 * Ask the engine to close an Output Window.
 *
 * Resolves to `true` when the request was sent. Unknown/blank ids are ignored
 * so a stale mirror entry never rejects.
 *
 * @param {{dispatch: Function}} store
 * @param {string} id
 * @returns {Promise<boolean>}
 */
export function closeEngineOutputWindow(store, id) {
  if (typeof id !== "string" || id.length === 0) {
    return Promise.resolve(false);
  }

  return Promise.resolve(
    store.dispatch(ENGINE_OUTPUT_WINDOW_REMOVE, { id }),
  ).then(() => true);
}

/**
 * Close every engine-owned Output Window.
 *
 * Used when the renderer unloads (reload or quit): the engine tears its
 * windows down with the process anyway, but removing them keeps the mirror from
 * listing ghost windows after a reload.
 *
 * @param {{dispatch: Function, state: object}} store
 * @returns {Promise<boolean[]>}
 */
export function closeAllEngineOutputWindows(store) {
  return Promise.all(
    engineOutputWindowIds(store?.state).map((id) =>
      closeEngineOutputWindow(store, id),
    ),
  );
}
