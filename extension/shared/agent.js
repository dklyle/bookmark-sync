/* Local-only WebExtension agent. It deliberately has no network permissions. */
const api = globalThis.browser ?? globalThis.chrome;
const ROOTS = {
  chrome: { "2": "root:toolbar", "3": "root:menu", "4": "root:mobile" },
  firefox: { toolbar_____: "root:toolbar", menu_____: "root:menu", unfiled_____: "root:menu", mobile_____: "root:mobile" },
};
const browserName = globalThis.BOOKMARK_SYNC_BROWSER;
const rootIds = ROOTS[browserName];
let port;
let applying = 0;
let mappings = {};
let remoteQueue = Promise.resolve();

function operation(kind, nodeId, fields = {}) {
  return { id: crypto.randomUUID(), nodeId, kind, ...fields };
}
function nativeId(canonicalId) {
  return Object.entries(mappings).find(([, value]) => value === canonicalId)?.[0];
}
function canonicalId(id) { return rootIds[id] ?? mappings[id]; }
async function saveMappings() { await api.storage.local.set({ mappings }); }
async function map(id, canonical) { mappings[id] = canonical; await saveMappings(); }
function send(message) { if (port) port.postMessage(message); }

function connect() {
  try {
    port = api.runtime.connectNative("io.bookmark-sync.host");
    port.onMessage.addListener((message) => {
      if (message.type === "operation") remoteQueue = remoteQueue.then(() => applyRemote(message.operation));
    });
    port.onDisconnect.addListener(() => { port = undefined; setTimeout(connect, 1000); });
    send({ type: "register", browser: browserName });
  } catch (_) { setTimeout(connect, 1000); }
}

async function localCreate(id, node) {
  if (applying || rootIds[id]) return;
  const parentId = canonicalId(node.parentId);
  if (!parentId) return;
  const canonical = crypto.randomUUID();
  await map(id, canonical);
  send({ type: "operation", operation: operation("create", canonical, {
    nodeType: node.url ? "bookmark" : "folder", title: node.title, url: node.url,
    parentId, index: node.index,
  }) });
}
async function localChange(id, changed) {
  if (applying) return;
  const nodeId = canonicalId(id);
  if (!nodeId) return;
  const fields = {};
  if (Object.hasOwn(changed, "title")) fields.title = changed.title;
  if (Object.hasOwn(changed, "url")) fields.url = changed.url;
  send({ type: "operation", operation: operation("change", nodeId, fields) });
}
async function localMove(id, move) {
  if (applying) return;
  const nodeId = canonicalId(id), parentId = canonicalId(move.parentId);
  if (!nodeId || !parentId) return;
  send({ type: "operation", operation: operation("move", nodeId, { parentId, index: move.index }) });
}
async function localRemove(id) {
  if (applying) return;
  const nodeId = canonicalId(id);
  if (!nodeId) return;
  send({ type: "operation", operation: operation("remove", nodeId) });
}

async function applyRemote(op) {
  if (op.kind === "create" && nativeId(op.nodeId)) return;
  applying += 1;
  try {
    if (op.kind === "create") {
      const parentId = nativeId(op.parentId) ?? Object.entries(rootIds).find(([, value]) => value === op.parentId)?.[0];
      if (!parentId) throw new Error(`unknown parent ${op.parentId}`);
      const details = { parentId, index: op.index, title: op.title ?? "" };
      if (op.nodeType === "bookmark") details.url = op.url ?? "";
      const children = await api.bookmarks.getChildren(parentId);
      const existing = children.find((node) => node.title === details.title &&
        (op.nodeType === "folder" ? !node.url : node.url === details.url));
      if (existing) { await map(existing.id, op.nodeId); return; }
      const node = await api.bookmarks.create(details);
      await map(node.id, op.nodeId);
      return;
    }
    const id = nativeId(op.nodeId);
    if (!id) return;
    if (op.kind === "change") await api.bookmarks.update(id, { ...(op.title !== undefined ? { title: op.title } : {}), ...(op.url !== undefined ? { url: op.url } : {}) });
    if (op.kind === "move") {
      const parentId = nativeId(op.parentId) ?? Object.entries(rootIds).find(([, value]) => value === op.parentId)?.[0];
      if (parentId) await api.bookmarks.move(id, { parentId, index: op.index });
    }
    if (op.kind === "remove") await api.bookmarks.removeTree(id);
  } catch (error) { console.error("bookmark-sync remote operation failed", op, error); }
  finally { applying -= 1; }
}

async function bootstrap() {
  const tree = await api.bookmarks.getTree();
  const operations = [];
  async function visit(node) {
    if (!rootIds[node.id]) {
      let id = canonicalId(node.id);
      if (!id) { id = crypto.randomUUID(); await map(node.id, id); }
      const parentId = canonicalId(node.parentId);
      if (parentId) operations.push(operation("create", id, {
        nodeType: node.url ? "bookmark" : "folder", title: node.title, url: node.url,
        parentId, index: node.index,
      }));
    }
    for (const child of node.children ?? []) await visit(child);
  }
  for (const root of tree) await visit(root);
  send({ type: "snapshot", operations });
  await api.storage.local.set({ bootstrapped: true });
}

async function start() {
  ({ mappings = {} } = await api.storage.local.get({ mappings: {} }));
  connect();
  api.bookmarks.onCreated.addListener((id, node) => void localCreate(id, node));
  api.bookmarks.onChanged.addListener((id, changed) => void localChange(id, changed));
  api.bookmarks.onMoved.addListener((id, move) => void localMove(id, move));
  api.bookmarks.onRemoved.addListener((id) => void localRemove(id));
  api.runtime.onMessage.addListener((message) => { if (message?.action === "bootstrap") return bootstrap(); });
}
void start();
