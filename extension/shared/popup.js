const api = globalThis.browser ?? globalThis.chrome;
const status = document.querySelector("#status");

document.querySelector("#bootstrap").addEventListener("click", async () => {
  try {
    await api.runtime.sendMessage({ action: "bootstrap" });
    status.textContent = "Initial bookmark tree sent to the local daemon.";
  } catch (error) { status.textContent = `Could not bootstrap: ${error.message}`; }
});

document.querySelector("#replace").addEventListener("click", async () => {
  if (!confirm("Replace this browser's local bookmarks with the synchronized tree? This cannot be undone by Bookmark Sync.")) return;
  try {
    await api.runtime.sendMessage({ action: "replace" });
    status.textContent = "Local bookmarks replaced with the synchronized tree.";
  } catch (error) { status.textContent = `Could not replace bookmarks: ${error.message}`; }
});
