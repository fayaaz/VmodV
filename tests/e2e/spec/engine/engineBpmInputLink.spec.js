// BPM Input Link parity against the Rust engine (MODV_ENGINE=1):
//  - the Audio panel's "bpm" feature creates a `state` link on `beats.bpm`;
//  - the engine leaves the linked prop at its saved value while the tempo
//    detector has no estimate (0 is the sentinel);
//  - a reported tempo drives the prop (rave.exe's `bpm` input), and the engine
//    keeps the last tempo when a zero is reported.
//
// Launches its own Electron instance (like engineParity.spec.js) so it runs
// with MODV_ENGINE=1 without touching the shared legacy page object.

import { test, expect } from "@playwright/test";
import { _electron as electron } from "playwright";
import {
  ipcRendererInvoke,
  parseElectronApp,
} from "electron-playwright-helpers";
import fs from "fs";
import os from "os";
import path from "path";
import { resolveLatestBuild } from "../../pageObjectModel/resolveLatestBuild";

async function waitForReady(page, timeout = 60_000) {
  const deadline = Date.now() + timeout;

  while (Date.now() < deadline) {
    if (page.isClosed()) return false;
    if (await ipcRendererInvoke(page, "is-modv-ready")) return true;
    await page.waitForTimeout(250);
  }

  return false;
}

test.describe("engine BPM Input Link (MODV_ENGINE=1)", () => {
  test.setTimeout(120_000);

  test("drives rave.exe's BPM input from the detected tempo", async () => {
    const latestBuild = resolveLatestBuild(path.resolve("./dist"));
    const appInfo = parseElectronApp(latestBuild.buildDir);
    const configDir = fs.mkdtempSync(path.join(os.tmpdir(), "vmodv-e2e-bpm-"));
    const cleanupDirs = [latestBuild.cleanupDir, configDir].filter(Boolean);

    const app = await electron.launch({
      args: ["--no-sandbox", appInfo.main],
      executablePath: appInfo.executable,
      env: {
        ...process.env,
        MODV_ENGINE: "1",
        ELECTRON_ENABLE_LOGGING: "1",
        XDG_CONFIG_HOME: configDir,
      },
    });

    try {
      const page = await app.firstWindow();
      expect(await waitForReady(page)).toBe(true);

      // The engine seeds the default Scenes on first run, including Rave.exe.
      await page.waitForFunction(
        () =>
          !!window.modV.store.state.scenes.scenes.find(
            (scene) => scene.name === "Rave.exe",
          ),
        undefined,
        { timeout: 20_000 },
      );

      const result = await page.evaluate(async () => {
        const store = window.modV.store;
        const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

        const waitFor = async (predicate, timeout = 15_000) => {
          const deadline = performance.now() + timeout;
          while (performance.now() < deadline) {
            if (predicate()) return true;
            await wait(50);
          }
          return false;
        };

        const scene = store.state.scenes.scenes.find(
          (item) => item.name === "Rave.exe",
        );
        if (!scene) return { error: "scene missing" };

        await store.dispatch("scenes/applyScene", { sceneId: scene.id });

        // The Scene's Foreground group owns the rave.exe ISF module. Its
        // `bpm` input is the one the Audio panel's BPM feature targets.
        const findModule = () =>
          Object.values(store.state.modules.active).find(
            (candidate) => candidate.$moduleName === "rave.exe",
          );
        if (!(await waitFor(() => !!findModule()))) {
          return { error: "rave.exe module missing" };
        }
        const module = findModule();

        const findInput = () =>
          Object.values(store.state.inputs.inputs).find(
            (candidate) =>
              candidate.data &&
              candidate.data.moduleId === module.$id &&
              candidate.data.prop === "bpm",
          );
        if (!(await waitFor(() => !!findInput()))) {
          return { error: "bpm input missing" };
        }
        const input = findInput();

        // This is exactly what `AudioFeatures.vue` dispatches for "bpm":
        // no min/max so the tempo passes through unchanged.
        await store.dispatch("inputs/createInputLink", {
          inputId: input.id,
          type: "state",
          location: "beats.bpm",
          source: "bpm",
        });

        // The engine has no analyser frame yet, so `beats.bpm` is 0 and the
        // link must keep the ISF input's saved default.
        await wait(400);
        const beforeDetector = module.props.bpm;

        // The renderer's tempo detector reports through `beats/SET_BPM`; the
        // engine stores it and echoes the delta into the mirror. Poll rather
        // than sleeping: a busy GPU can delay the engine frame that evaluates
        // the link.
        store.commit("beats/SET_BPM", { bpm: 174 });
        const reachedTempo = await waitFor(() => module.props.bpm === 174);
        const afterDetector = module.props.bpm;

        // A zero tempo is rejected/ignored by the engine, and the BPM link's
        // guard would skip it anyway; the last real tempo must survive.
        store.commit("beats/SET_BPM", { bpm: 0 });
        await wait(400);
        const afterReset = module.props.bpm;

        const link = store.state.inputs.inputLinks[input.id];

        return {
          beforeDetector,
          afterDetector,
          afterReset,
          reachedTempo,
          linkType: link && link.type,
          linkSource: link && link.source,
          linkMin: link && link.min,
          linkMax: link && link.max,
        };
      });

      expect(result.error).toBeUndefined();
      expect(result.linkType).toBe("state");
      expect(result.linkSource).toBe("bpm");
      expect(result.linkMin).toBe(0);
      expect(result.linkMax).toBe(1);
      expect(result.beforeDetector).toBe(140);
      expect(result.reachedTempo).toBe(true);
      expect(result.afterDetector).toBe(174);
      expect(result.afterReset).toBe(174);
    } finally {
      await app.close().catch(() => {});
      for (const dir of cleanupDirs) {
        fs.rmSync(dir, { recursive: true, force: true });
      }
    }
  });
});
