import { v4 as uuidv4 } from "uuid";

const state = {};

const actions = {
  createWindow({ commit }) {
    const win = {
      width: 300,
      height: 300,
      title: "VmodV Output",
      pixelRatio: 1,
      x: 0,
      y: 0,
      fullscreen: false,
      backgroundColor: "#000",
      outputId: "",
    };

    win.id = uuidv4();
    commit("ADD_WINDOW", win);

    return win.id;
  },
};

const mutations = {
  ADD_WINDOW(state, window) {
    state[window.id] = window;
  },

  /**
   * Replace the whole window list.
   *
   * Engine mode replays the engine's `state/sync` payload here: the engine owns
   * the native windows, so a late-joining mirror (or a mirror rebuilt after an
   * engine restart) must show exactly the windows the engine has open.
   */
  SET_WINDOWS(state, windows) {
    for (const id of Object.keys(state)) {
      delete state[id];
    }

    for (const window of windows || []) {
      if (window && typeof window.id === "string") {
        state[window.id] = window;
      }
    }
  },

  REMOVE_WINDOW(state, id) {
    delete state[id];
  },

  UPDATE_WINDOW(state, { id, key, value }) {
    state[id][key] = value;
  },
};

export default {
  namespaced: true,
  state,
  // getters,
  actions,
  mutations,
};
