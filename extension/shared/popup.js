const api = globalThis.browser ?? globalThis.chrome;
document.querySelector("#bootstrap").addEventListener("click", async () => {
  const status = document.querySelector("#status");
  try {
    await api.runtime.sendMessage({ action: "bootstrap" });
    status.textContent = "Initial bookmark tree sent to the local daemon.";
  } catch (error) { status.textContent = `Could not bootstrap: ${error.message}`; }
});
