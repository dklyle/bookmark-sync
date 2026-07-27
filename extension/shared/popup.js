const api = globalThis.browser ?? globalThis.chrome;
const status = document.querySelector("#status");

async function run(action) {
  const response = await api.runtime.sendMessage({ action });
  if (response?.error) throw new Error(response.error);
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
