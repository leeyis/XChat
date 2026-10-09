// Real WebView2/Edge integration through their task-owned debugging endpoints.
import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";

const [debugPort, apiPort, directory, mode, output] = process.argv.slice(2);
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
let target;
for (let attempt = 0; attempt < 60; attempt++) {
  try {
    const pages = await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json();
    target = pages.find((page) => page.type === "page" && page.url.startsWith("http://127.0.0.1:"));
    if (target) break;
  } catch {}
  await wait(500);
}
assert.ok(target, "task-owned browser page was not found");
const socket = new WebSocket(target.webSocketDebuggerUrl);
const pending = new Map();
const exceptions = [];
const requests = [];
let sequence = 0;
socket.onmessage = ({ data }) => {
  const message = JSON.parse(data);
  if (message.method === "Runtime.exceptionThrown") exceptions.push(message.params.exceptionDetails.text);
  if (message.method === "Network.requestWillBeSent") requests.push(message.params.request.url);
  if (pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error)));
    else resolve(message.result);
  }
};
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
function call(method, params = {}) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const result = await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  assert.equal(result.exceptionDetails, undefined, JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
async function until(expression, label) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (await evaluate(expression)) return;
    await wait(200);
  }
  throw new Error(label);
}
await call("Runtime.enable");
await call("Network.enable");
await until("document.querySelector('#root')?.textContent.length > 30", "application did not render");
await wait(700);
const native = mode === "tauri";
const first = native
  ? await evaluate("window.__TAURI__.core.invoke('sync_workspace', { cursor: null })")
  : await (await fetch(`http://127.0.0.1:${apiPort}/api/workspace/sync`)).json();
assert.equal(first.reset, true);
assert.ok(first.changes.self.id);
const idle = native
  ? await evaluate(`window.__TAURI__.core.invoke('sync_workspace', { cursor: ${JSON.stringify(first.cursor)} })`)
  : await (await fetch(`http://127.0.0.1:${apiPort}/api/workspace/sync?cursor=${encodeURIComponent(first.cursor)}`)).json();
assert.equal(idle.reset, false);
assert.deepEqual(idle.changes, {});

const database = new DatabaseSync(join(directory, "xchat.db"));
database.exec("PRAGMA busy_timeout = 5000");
const peer = "phase2-browser-peer";
const name = "Phase2 Incremental Device";
database.prepare("INSERT INTO users(id,name,addr,last_seen,is_offline) VALUES (?,?,?,1,1)").run(peer, name, "127.0.0.1:9");
if (native) await evaluate("window.__TAURI__.event.emit('workspace-changed', {})");
await until(`document.body.innerText.includes(${JSON.stringify(name)})`, "inserted device was not rendered through incremental synchronization");
const conversation = database.prepare("SELECT id FROM conversations WHERE peer_id=?").get(peer).id;
database.prepare("DELETE FROM conversation_members WHERE conversation_id=?").run(conversation);
database.prepare("DELETE FROM conversations WHERE id=?").run(conversation);
database.prepare("DELETE FROM users WHERE id=?").run(peer);
if (native) await evaluate("window.__TAURI__.event.emit('workspace-changed', {})");
await until(`!document.body.innerText.includes(${JSON.stringify(name)})`, "deleted device remained in the rendered workspace");
database.close();
assert.deepEqual(exceptions, [], "browser raised an uncaught exception");
if (!native) assert.ok(requests.some((url) => url.includes("/api/workspace/sync")), "frontend did not poll the sync endpoint");
const report = { mode, initial_render: true, native_command_permission: native, empty_delta: true,
  inserted_device_rendered: true, deleted_device_removed: true, uncaught_exceptions: exceptions,
  sync_http_requests: requests.filter((url) => url.includes("/api/workspace/sync")).length };
await writeFile(output, JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify(report));
socket.close();
