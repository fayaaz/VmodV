import init, {
  enable_microphone,
  resume_microphone,
  start,
  start_output,
} from "./pkg/modv_web.js";

const boot = document.getElementById("boot");
const status = document.getElementById("boot-status");
const error = document.getElementById("boot-error");
const mic = document.getElementById("mic");
const micDevice = document.getElementById("mic-device");
const toast = document.getElementById("toast");

let toastTimer = 0;
function showToast(message) {
  if (!toast) return;
  toast.textContent = message;
  toast.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.classList.add("hidden"), 4000);
}

// The output window plays a capture stream from a chrome-less renderer that
// runs on an offscreen canvas in this page. The renderer mirrors the main
// shell's exact state, audio and transition progress, so it cannot drift.
let outputWindow = null;
let outputEngine = null;

function attachOutputStream() {
  const canvas = document.getElementById("modv-output-canvas");
  if (!canvas) return;
  const stream = canvas.captureStream(30);
  window.modvCaptureStream = stream;
}

function ensureOutputEngine() {
  if (outputEngine) return outputEngine;
  outputEngine = (async () => {
    await start_output("modv-output-canvas");
    attachOutputStream();
  })().catch((reason) => {
    outputEngine = null;
    throw reason;
  });
  return outputEngine;
}

window.openModvOutput = () => {
  if (!outputWindow || outputWindow.closed) {
    outputWindow = window.open(
      "./output.html",
      "vmodv-output",
      "popup=yes,width=1280,height=720",
    );
    if (!outputWindow) {
      showToast("Allow pop-ups for this site to open the output window.");
      return false;
    }
  }
  window.__modvOutputActive = true;
  outputWindow.focus();
  ensureOutputEngine().catch((reason) => {
    console.error("could not start the output renderer", reason);
    showToast("Could not start the output renderer.");
  });
  return true;
};

window.closeModvOutput = () => {
  if (outputWindow && !outputWindow.closed) outputWindow.close();
  outputWindow = null;
  window.__modvOutputActive = false;
};

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
