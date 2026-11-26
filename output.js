import init, { enable_microphone, start_output } from "./pkg/modv_web.js";

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
      mic.textContent = "Enable microphone";
    }, 2500);
  }
});

try {
  await init();
  status.textContent = "Restoring the live stack…";
  await start_output("output-canvas");
  boot.remove();
} catch (reason) {
  console.error(reason);
  status.textContent = "The output view could not start.";
  error.textContent = String(reason?.stack ?? reason);
}
