const api = globalThis.browser ?? globalThis.chrome;
const status = document.querySelector("#status");

async function run(action) {
  // Firefox Manifest V2 has a persistent background page. Calling it directly avoids
  // a startup race in which the popup is the event that wakes that page.
  if (typeof api.runtime.getBackgroundPage === "function") {
    const background = await api.runtime.getBackgroundPage();
    if (background?.bookmarkSync?.[action]) return background.bookmarkSync[action]();
  }
  return api.runtime.sendMessage({ action });
}

document.querySelector("#bootstrap").addEventListener("click", async () => {
  try {
    await run("bootstrap");
    status.textContent = "Initial bookmark import started; you can close this popup.";
  } catch (error) { status.textContent = `Could not bootstrap: ${error.message}`; }
});

document.querySelector("#replace").addEventListener("click", async () => {
  if (!confirm("Replace this browser's local bookmarks with the synchronized tree? This cannot be undone by Bookmark Sync.")) return;
  try {
    await run("replace");
    status.textContent = "Replacement started; you can close this popup.";
  } catch (error) { status.textContent = `Could not replace bookmarks: ${error.message}`; }
});
