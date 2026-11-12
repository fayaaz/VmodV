<template>
  <div
    v-if="!badModule"
    ref="galleryItem"
    v-contextMenu="() => GalleryItemContextMenu({ moduleName })"
    class="gallery-item"
    :class="{ grabbing }"
    @mouseover="focus"
    @mouseleave="blur"
    @dblclick="doubleClick"
    @mousedown="mouseDown"
  >
    <canvas ref="canvas" :class="{ 'engine-canvas': engineMode }"></canvas>
    <div class="title">{{ moduleName }}</div>
  </div>
</template>

<script>
import { GROUP_ENABLED, GROUP_DISABLED } from "../application/constants.js";
import { shouldRefresh } from "../application/engine-client.js";
import { GalleryItemContextMenu } from "../menus/context/galleryItemContextMenu";

// Engine mode: hovering a tile asks the engine for the module composited over
// the current output (the Rust counterpart of the worker's gallery auxillary,
// but with the live frame underneath). Tiles are blank until the pointer
// enters: 10 fps while hovered, no requests at all otherwise, so a Gallery with
// hundreds of tiles costs nothing until one is actually inspected.
const GALLERY_HOVER_FPS = 10;
// The engine may not implement `preview/module` yet (or may be mid-restart):
// back off so a Gallery full of tiles cannot flood the control socket.
const GALLERY_FAILURE_RETRY_MS = 15_000;
// Fallback request size when the tile has not been laid out yet.
const GALLERY_FALLBACK_WIDTH = 160;
const GALLERY_FALLBACK_HEIGHT = 90;

export default {
  props: {
    moduleName: { type: String },
    groupId: { type: String },
  },

  data() {
    return {
      id: "",
      outputId: "",
      activeModule: { meta: {} },
      badModule: false,
      grabbing: false,
      renderQuality: 1 * window.devicePixelRatio,
      GalleryItemContextMenu,
    };
  },

  computed: {
    engineMode() {
      return this.$modV?.engineMode === true;
    },
  },

  async mounted() {
    if (this.engineMode) {
      // No canvas transfer: the worker does not exist, and the tile's own 2D
      // canvas is a display copy of an engine-rendered frame.
      this.startEnginePreview();
      return;
    }

    const { canvas } = this.$refs;
    const { width, height } = canvas;
    const offscreen = canvas.transferControlToOffscreen();

    const outputContext = await this.$modV.store.dispatch(
      "outputs/getAuxillaryOutput",
      {
        name: `${this.moduleName}-gallery`,
        canvas: offscreen,
        type: "2d",
        group: "gallery",
        reactToResize: false,
        width: width * this.renderQuality,
        height: height * this.renderQuality,
      },
      [offscreen],
    );

    const module = await this.$modV.store.dispatch("modules/makeActiveModule", {
      moduleName: this.moduleName,
      moduleMeta: { enabled: false, isGallery: true },
      skipInit: true,
    });

    if (!module) {
      this.badModule = true;
      return;
    }

    this.activeModule = module;
    this.id = module.$id;
    this.outputId = outputContext.id;

    await this.$modV.store.commit("groups/ADD_MODULE_TO_GROUP", {
      moduleId: this.id,
      groupId: this.groupId,
    });

    await this.$modV.store.dispatch("modules/init", {
      moduleId: this.id,
      width: width * this.renderQuality,
      height: height * this.renderQuality,
    });

    await this.$modV.store.dispatch("modules/resize", {
      moduleId: this.id,
      width: width * this.renderQuality,
      height: height * this.renderQuality,
    });

    // Synthesise a resize event for smooth-dnd's buggy hit detection to work
    window.dispatchEvent(new Event("resize"));
  },

  async beforeUnmount() {
    if (this.engineMode) {
      // There is no gallery Module in the store and no auxillary output to
      // release in engine mode — just stop the polling and the drag listener.
      this.stopEnginePreview();
      this.mouseUp();
      return;
    }

    await this.$modV.store.dispatch("modules/removeActiveModule", {
      moduleId: this.id,
    });

    this.$modV.store.commit("outputs/REMOVE_AUXILLARY", this.outputId);
    this.$modV.store.commit("groups/REMOVE_MODULE_FROM_GROUP", {
      groupId: this.groupId,
      moduleId: this.id,
    });

    // ensure listener cleanup
    this.mouseUp();
  },

  methods: {
    /**
     * Engine mode: poll the engine for this module's hover preview.
     *
     * `requestModulePreview` resolves to `{ width, height, data }` (or null
     * while the engine has no preview for the module), painted into the tile's
     * own 2D canvas with `putImageData`. The engine composites the module over
     * the current live output, so the tile shows what adding it would look
     * like. Nothing is requested until the pointer enters the tile.
     */
    startEnginePreview() {
      const { canvas } = this.$refs;

      this._enginePreview = {
        ctx: canvas.getContext("2d"),
        timer: null,
        inFlight: false,
        disposed: false,
        hovered: false,
        failures: 0,
        // null → nothing pulled yet for this hover.
        lastMs: null,
      };
    },

    stopEnginePreview() {
      const state = this._enginePreview;
      if (!state) {
        return;
      }

      state.disposed = true;
      if (state.timer !== null) {
        clearTimeout(state.timer);
        state.timer = null;
      }
      this.clearEnginePreview(state);

      this._enginePreview = null;
    },

    /** Hovered tiles poll; there is no idle polling. */
    enginePreviewFps() {
      return GALLERY_HOVER_FPS;
    },

    /** Queue the next hover pull, honouring the rate and any failure backoff. */
    scheduleEnginePreview() {
      const state = this._enginePreview;
      if (!state || state.disposed || !state.hovered) {
        return;
      }
      if (state.inFlight || state.timer !== null) {
        return;
      }

      const now = performance.now();
      const fps = this.enginePreviewFps();
      let wait = 0;

      if (state.failures > 0) {
        wait = GALLERY_FAILURE_RETRY_MS;
      } else if (!shouldRefresh(now, state.lastMs, fps)) {
        wait = Math.max(0, state.lastMs + 1000 / fps - now);
      }

      state.timer = setTimeout(() => {
        state.timer = null;
        void this.pullEnginePreview();
      }, wait);
    },

    async pullEnginePreview() {
      const state = this._enginePreview;
      if (!state || state.disposed || state.inFlight) {
        return;
      }
      if (!state.hovered) {
        // The pointer left while the timer was queued.
        return;
      }

      const now = performance.now();
      if (!shouldRefresh(now, state.lastMs, this.enginePreviewFps())) {
        this.scheduleEnginePreview();
        return;
      }

      const { width, height } = this.enginePreviewSize();
      state.lastMs = now;
      state.inFlight = true;

      try {
        const frame = await this.$modV.requestModulePreview(
          this.moduleName,
          width,
          height,
          { withOutput: true },
        );

        if (
          !frame ||
          this._enginePreview !== state ||
          state.disposed ||
          !state.hovered
        ) {
          // A request issued before the pointer left must not repaint the
          // blanked tile with a stale live-output composite.
          if (this._enginePreview === state && state.hovered) {
            state.failures += 1;
          }
          return;
        }

        state.failures = 0;
        this.drawEnginePreviewFrame(state, frame);
      } finally {
        state.inFlight = false;
        this.scheduleEnginePreview();
      }
    },

    /** Backing-store size for the request, from the tile's laid-out size. */
    enginePreviewSize() {
      const rect = this.$refs.canvas?.getBoundingClientRect?.();
      const quality = this.renderQuality || 1;

      return {
        width: Math.max(
          1,
          Math.round((rect?.width || GALLERY_FALLBACK_WIDTH) * quality),
        ),
        height: Math.max(
          1,
          Math.round((rect?.height || GALLERY_FALLBACK_HEIGHT) * quality),
        ),
      };
    },

    drawEnginePreviewFrame(state, { width, height, data }) {
      const { canvas } = this.$refs;
      if (!canvas || data.length !== width * height * 4) {
        return;
      }

      // 1:1 backing store; the tile's CSS scales it to fit.
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }

      const ctx = state.ctx || canvas.getContext("2d");
      state.ctx = ctx;
      ctx.putImageData(new ImageData(data, width, height), 0, 0);
    },

    /** Hover changed: start pulling on enter, clear the tile on leave. */
    setEngineHover(hovered) {
      const state = this._enginePreview;
      if (!state || state.disposed) {
        return;
      }

      state.hovered = hovered;

      if (state.timer !== null) {
        clearTimeout(state.timer);
        state.timer = null;
      }

      if (!hovered) {
        this.clearEnginePreview(state);
        return;
      }

      state.lastMs = null;
      this.scheduleEnginePreview();
    },

    /** Blank the tile so an unhovered Gallery shows no stale previews. */
    clearEnginePreview(state) {
      const { canvas } = this.$refs;
      const ctx = state.ctx || canvas?.getContext?.("2d");

      if (!ctx || !canvas) {
        return;
      }

      state.ctx = ctx;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
    },

    async focus() {
      if (this.engineMode) {
        // No gallery Module to enable: the engine renders the hover preview
        // off-graph, composited over the live output.
        if (!this._enginePreview?.hovered) {
          this.setEngineHover(true);
        }
        return;
      }

      const {
        id,
        groupId,
        outputId,
        $refs: { galleryItem },
      } = this;

      const newWidth = galleryItem.clientWidth * this.renderQuality;
      const newHeight = galleryItem.clientHeight * this.renderQuality;

      this.$modV.store.commit("outputs/RESIZE_AUXILLARY", {
        id: outputId,
        width: newWidth,
        height: newHeight,
      });

      await this.$modV.store.dispatch("modules/resize", {
        moduleId: id,
        width: newWidth,
        height: newHeight,
      });

      this.$modV.store.commit("groups/UPDATE_GROUP", {
        groupId,
        data: {
          drawToCanvasId: outputId,
          enabled: GROUP_ENABLED,
        },
      });

      // Runs entirely in the worker's authoritative store: the renderer's
      // mirror store replays worker mutations asynchronously and can lag, so
      // reading the Group list from the mirror here (as before) silently
      // disabled the preview while it caught up.
      await this.$modV.store.dispatch("groups/focusGalleryItem", {
        groupId,
        moduleId: id,
        width: newWidth,
        height: newHeight,
      });
    },

    blur() {
      if (this.engineMode) {
        // Stop polling and clear the tile; there is no gallery Group or
        // Module to disable (the engine renders the preview off-graph).
        this.setEngineHover(false);
        return;
      }

      this.$modV.store.commit("modules/UPDATE_ACTIVE_MODULE_META", {
        id: this.id,
        metaKey: "enabled",
        data: false,
      });

      this.$modV.store.commit("groups/UPDATE_GROUP", {
        groupId: this.groupId,
        data: {
          enabled: GROUP_DISABLED,
        },
      });
    },

    async doubleClick() {
      const groupId = this.$store.state["uiGroups"].lastFocused;
      if (!groupId) {
        return;
      }

      const module = await this.$modV.store.dispatch(
        "modules/makeActiveModule",
        { moduleName: this.moduleName },
      );

      this.$modV.store.commit("groups/ADD_MODULE_TO_GROUP", {
        moduleId: module.$id,
        groupId,
        position: this.$modV.store.state.groups.groups.find(
          (group) => group.id === groupId,
        ).modules.length,
      });
    },

    mouseDown() {
      this.grabbing = true;
      window.addEventListener("mouseup", this.mouseUp);
    },

    mouseUp() {
      this.grabbing = false;
      window.removeEventListener("mouseup", this.mouseUp);
    },
  },
};
</script>

<style scoped>
canvas {
  max-width: 100%;
}

.gallery-item {
  position: relative;
  cursor: grab;
  display: flex;
  justify-content: center;
  align-items: center;
  height: 100%;
  width: 100%;
}

.gallery-item.grabbing {
  cursor: grabbing;
}

.gallery-item:hover canvas {
  opacity: 1;
}

.gallery-item:hover .title {
  opacity: 0.3;
}

.gallery-item canvas {
  opacity: 0.3;
  transition: 150ms opacity;
  width: 100%;
  height: 100%;
}

/*
  Engine mode draws an engine-rendered frame into the tile's own 2D canvas; the
  backdrop matches the black the worker's auxillary canvas used to show before
  the first frame arrives.
*/
.gallery-item canvas.engine-canvas {
  background: #000;
}

.gallery-item .title {
  transition: 150ms opacity;
  padding: 0.45em;
  text-overflow: ellipsis;
  width: 100%;
  text-align: center;
  overflow: hidden;
}

.title {
  position: absolute;
}
</style>
