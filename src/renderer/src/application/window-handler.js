import {
  exitFullscreen,
  isFullscreen,
  isEditableTarget,
  isFullscreenExitEvent,
  isFullscreenToggleEvent,
  toggleFullscreen,
} from "./output-window-controls.js";
import {
  closeEngineOutputWindow,
  engineOutputWindowIds,
} from "./engine-output-windows.js";

export default function windowHandler() {
  const windows = {};
  const that = this;

  function createHideMouseTimerhandler(canvas) {
    let mouseTimer;

    function hideMouse() {
      canvas.ownerDocument.body.style.cursor = "none";
    }

    return function () {
      if (mouseTimer) {
        clearTimeout(mouseTimer);
      }

      canvas.ownerDocument.body.style.cursor = "default";
      mouseTimer = setTimeout(hideMouse, 200);
    };
  }

  /**
   * @param {Object} args
   * @param {Window} args.win
   * @param {HTMLCanvasElement} args.canvas
   * @param {string} args.backgroundColor
   */
  function configureWindow({ win, canvas, backgroundColor }) {
    win.document.body.appendChild(canvas);
    win.document.body.style.backgroundColor = backgroundColor;

    setSize.call(that, win);
  }

  function pollToConfigureWindow(args) {
    let poll;

    function checkIfDone(args) {
      if (args.win.document.readyState === "complete") {
        configureWindow(args);
      } else {
        pollToConfigureWindow(args)();
      }
    }

    return function () {
      if (poll) {
        clearTimeout(poll);
      }

      poll = setTimeout(() => checkIfDone(args), 100);
    };
  }

  function setSize(win) {
    const { innerWidth: width, innerHeight: height } = win;

    this.store.dispatch("size/setSize", {
      width,
      height,
    });
  }

  this._store.subscribe(async ({ type, payload }) => {
    if (type === "windows/ADD_WINDOW") {
      // Engine mode: the Rust engine owns the native window and created it
      // when `windows/createWindow` was dispatched (see
      // engine-output-windows.js). The ADD_WINDOW delta is its mirror record,
      // so there is nothing to open here and no canvas to transfer.
      if (that.engineMode) return;

      const { width, height, backgroundColor, title, id } = payload;

      const win = window.open(
        "./output-window.html",
        "modal",
        `width=${width}, height=${height}, location=no, menubar=no, left=0`,
      );
      if (win === null || typeof win === "undefined") {
        console.log(
          "Could not create Output Window",
          "VmodV couldn't open an Output Window. Please check you've allowed pop-ups, then reload",
        );

        return;
      }

      win.document.title = title;

      windows[id] = win;

      const canvas = document.createElement("canvas");
      const offscreen = canvas.transferControlToOffscreen();
      const { id: outputId } = await this.store.dispatch(
        "outputs/getAuxillaryOutput",
        {
          canvas: offscreen,
          name: `window-${Object.keys(windows).length}`,
          group: "window",
        },
        [offscreen],
      );

      this.store.commit("windows/UPDATE_WINDOW", {
        id,
        key: "outputId",
        value: outputId,
      });

      canvas.style.backgroundColor = "transparent";
      canvas.style.objectFit = "cover";
      canvas.style.width = "100%";

      // The canvas has been transferred to the worker, so it is a placeholder:
      // fullscreen the document root instead, and accept F/Escape like the
      // engine's native output window.
      const outputDocument = canvas.ownerDocument;

      canvas.addEventListener("dblclick", () => {
        toggleFullscreen(outputDocument);
      });

      outputDocument.addEventListener("keydown", (event) => {
        if (event.repeat || isEditableTarget(event.target)) {
          return;
        }

        if (isFullscreenToggleEvent(event)) {
          event.preventDefault();
          toggleFullscreen(outputDocument);
        } else if (
          isFullscreenExitEvent(event) &&
          isFullscreen(outputDocument)
        ) {
          event.preventDefault();
          exitFullscreen(outputDocument);
        }
      });

      // A popup opened from the main window does not always keep focus after a
      // click; without focus the keydown listener never sees F. Refocusing on
      // pointer input is enough and does not steal focus at open time.
      win.addEventListener("pointerdown", () => win.focus());

      canvas.addEventListener("mousemove", createHideMouseTimerhandler(canvas));

      pollToConfigureWindow({ win, canvas, title, backgroundColor })();

      let timer;

      win.addEventListener("resize", () => {
        if (timer) {
          cancelAnimationFrame(timer);
        }

        // Setup the new requestAnimationFrame()
        timer = requestAnimationFrame(() => {
          setSize.call(this, win);
        });
      });
    }
  });

  window.addEventListener("unload", () => {
    if (that.engineMode) {
      // The engine's native windows die with its process, but dispatching the
      // removals keeps the mirror (and a reloaded renderer) from listing ghost
      // windows.
      for (const id of engineOutputWindowIds(that._store.state)) {
        closeEngineOutputWindow(that.store, id);
      }

      return;
    }

    const windowIds = Object.keys(windows);
    for (let i = 0, len = windowIds.length; i < len; i++) {
      windows[windowIds[i]].close();
    }
  });
}
