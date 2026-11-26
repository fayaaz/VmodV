import init, {
  enable_microphone,
  resume_microphone,
  start,
} from "./pkg/modv_web.js";

const boot = document.getElementById("boot");
const status = document.getElementById("boot-status");
const error = document.getElementById("boot-error");
const mic = document.getElementById("mic");
const micDevice = document.getElementById("mic-device");

// The output view is drawn by the wasm shell itself in this tab: `Open output`
// hides the UI and draws the compositor over the whole canvas at the viewport
// resolution, so there is no capture stream or pop-up window to encode and
// soften. The cog in the top-right corner returns to the UI.

async function refreshMicrophones() {
  if (!navigator.mediaDevices?.enumerateDevices) return;
  try {
    const devices = (await navigator.mediaDevices.enumerateDevices()).filter(
      (device) => device.kind === "audioinput",
    );
    const current = micDevice.value;
    micDevice.replaceChildren();
    const fallback = document.createElement("option");
    fallback.value = "";
    fallback.textContent = "Default microphone";
    micDevice.append(fallback);
    for (const device of devices) {
      const option = document.createElement("option");
      option.value = device.deviceId;
      option.textContent = device.label || "Microphone";
      micDevice.append(option);
    }
    if ([...micDevice.options].some((option) => option.value === current)) {
      micDevice.value = current;
    }
  } catch (reason) {
    console.warn("could not enumerate microphones", reason);
  }
}

navigator.mediaDevices?.addEventListener("devicechange", refreshMicrophones);


function resumeAudio() {
  resume_microphone().catch((reason) => {
    console.warn("could not resume the microphone context", reason);
  });
}

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) resumeAudio();
});
window.addEventListener("focus", resumeAudio);
window.addEventListener("pageshow", resumeAudio);
setInterval(resumeAudio, 10000);

mic.addEventListener("click", async () => {
  mic.disabled = true;
  mic.textContent = "Requesting microphone…";
  try {
    await enable_microphone(micDevice.value || undefined);
    await resumeAudio();
    mic.textContent = "Microphone live";
    await refreshMicrophones();
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
