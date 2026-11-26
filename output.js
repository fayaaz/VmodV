const boot = document.getElementById("boot");
const status = document.getElementById("boot-status");
const error = document.getElementById("boot-error");
const video = document.getElementById("output-video");

try {
  if (window.opener) window.opener.__modvOutputActive = true;
} catch (reason) {
  console.warn("could not activate the output renderer", reason);
}
addEventListener("pagehide", () => {
  try {
    if (window.opener) window.opener.__modvOutputActive = false;
  } catch {
    // The opener may already be gone.
  }
});

async function waitForStream() {
  // The main tab starts the hidden renderer and exposes its capture stream on
  // `opener`; the renderer may take a few seconds to boot.
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const stream = window.opener?.modvCaptureStream;
    if (stream) return stream;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  return null;
}

try {
  const stream = await waitForStream();
  if (!stream) {
    status.textContent = "Open output from the VmodV browser tab.";
    error.textContent = "The output window needs the main tab to supply its stream.";
  } else {
    video.srcObject = stream;
    video.classList.add("visible");
    try {
      await video.play();
    } catch (reason) {
      console.warn("the output stream needs a click to start", reason);
      addEventListener("click", () => video.play().catch(() => {}), { once: true });
    }
    stream.getVideoTracks()[0]?.addEventListener("ended", () => {
      status.textContent = "The main VmodV window stopped the output.";
      boot.classList.remove("hidden");
    });
    boot.remove();
  }
} catch (reason) {
  console.error(reason);
  status.textContent = "The output view could not start.";
  error.textContent = String(reason?.stack ?? reason);
}
