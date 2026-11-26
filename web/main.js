import init, { enable_microphone, start } from "./pkg/modv_web.js";

const boot = document.getElementById("boot");
const status = document.getElementById("boot-status");
const error = document.getElementById("boot-error");
const mic = document.getElementById("mic");

mic.addEventListener("click", async () => {
  mic.disabled = true;
  mic.textContent = "Requesting microphone…";
  try {
    await enable_microphone();
    mic.textContent = "Microphone live";
  } catch (reason) {
    console.error(reason);
    mic.textContent = "Microphone unavailable";
    setTimeout(() => {
      mic.disabled = false;
      mic.textContent = "Use microphone";
    }, 2500);
  }
});

function fail(reason) {
  console.error(reason);
  status.textContent = "The browser shell could not start.";
  error.textContent = String(reason?.stack ?? reason);
}

try {
  // eframe selects WebGPU when the browser exposes a working adapter and
  // falls back to wgpu's WebGL2 backend otherwise. Only a browser with
  // neither path will fail, and the thrown error is shown below.
  await init();
  status.textContent = "Initialising the engine…";
  await start("modv-canvas");
  boot.classList.add("hidden");
  setTimeout(() => boot.remove(), 300);
} catch (reason) {
  fail(reason);
}
