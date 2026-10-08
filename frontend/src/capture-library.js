import { captureId, normalizePinView } from "./capture-model.js";

const SETTINGS_KEY = "xchat.capture.settings.v1";
export const DEFAULT_CAPTURE_SETTINGS = { delay: 0, detect: true, magnifier: true, captureCursor: false, remember: true, limit: 12, theme: "light", preset: "xchat", presetW: 0, presetH: 0, group: "默认" };
let database;
let channel;
const contextId = globalThis[Symbol.for("xchat.capture.context")] ||= captureId();
function notifyCapture(change) {
  globalThis.dispatchEvent?.(new CustomEvent("xchat-capture-library", { detail: change }));
  if (globalThis.BroadcastChannel) {
    channel ||= new BroadcastChannel("xchat-capture-library");
    channel.postMessage({ ...change, source: contextId });
  }
}
export function watchCaptureLibrary(listener) {
  const local = event => listener(event.detail);
  globalThis.addEventListener("xchat-capture-library", local);
  const remote = globalThis.BroadcastChannel ? new BroadcastChannel("xchat-capture-library") : null;
  if (remote) remote.onmessage = event => { if (event.data?.source !== contextId) listener(event.data); };
  return () => { globalThis.removeEventListener("xchat-capture-library", local); remote?.close(); };
}
export function captureSettings() {
  try {
    const saved = JSON.parse(localStorage.getItem(SETTINGS_KEY) || "{}");
    return { ...DEFAULT_CAPTURE_SETTINGS, ...saved, captureCursor: saved.captureCursor === true, delay: [0, 3, 5].includes(saved.delay) ? saved.delay : 0, limit: [6, 12, 20].includes(saved.limit) ? saved.limit : 12 };
  } catch { return { ...DEFAULT_CAPTURE_SETTINGS }; }
}
export function saveCaptureSettings(patch) {
  const settings = { ...captureSettings(), ...patch };
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  notifyCapture({ kind: "settings" });
  return settings;
}
async function openLibrary() {
  if (!globalThis.indexedDB) throw new Error("当前环境不支持截图本地存储");
  if (!database) database = new Promise((resolve, reject) => {
    const request = indexedDB.open("xchat-captures", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("records", { keyPath: "id" });
    request.onerror = () => { database = null; reject(new Error("无法打开截图资料库，请检查浏览器存储权限")); };
    request.onsuccess = () => {
      request.result.onversionchange = () => { request.result.close(); database = null; };
      resolve(request.result);
    };
  });
  return database;
}
export async function captureRecords(kind) {
  const db = await openLibrary();
  return new Promise((resolve, reject) => {
    const request = db.transaction("records").objectStore("records").getAll();
    request.onsuccess = () => resolve(request.result.filter(record => !kind || record.kind === kind).sort((a, b) => b.updated - a.updated));
    request.onerror = () => reject(new Error("无法读取截图资料库"));
  });
}
export async function captureRecord(id) {
  const db = await openLibrary();
  return new Promise((resolve, reject) => {
    const request = db.transaction("records").objectStore("records").get(id);
    request.onsuccess = () => resolve(request.result || null);
    request.onerror = () => reject(new Error("无法读取截图记录"));
  });
}
export async function saveCaptureRecord(record) {
  const db = await openLibrary();
  const value = { ...record, id: record.id || captureId(), updated: Date.now() };
  if (value.kind === "pin") value.view = normalizePinView(value.view);
  return new Promise((resolve, reject) => {
    const tx = db.transaction("records", "readwrite"), store = tx.objectStore("records");
    store.put(value);
    if (value.kind === "history") {
      const request = store.getAll();
      request.onsuccess = () => request.result.filter(item => item.kind === "history").sort((a, b) => b.updated - a.updated).slice(captureSettings().limit).forEach(item => store.delete(item.id));
    }
    tx.oncomplete = () => { notifyCapture({ kind: value.kind, id: value.id }); resolve(value); };
    tx.onerror = () => reject(new Error("保存截图记录失败，可能是本地存储空间不足；当前编辑内容仍保留"));
    tx.onabort = () => reject(new Error("截图记录未保存，当前编辑内容仍保留"));
  });
}
export async function patchCaptureRecord(id, patch) {
  const db = await openLibrary();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("records", "readwrite"), store = tx.objectStore("records");
    let value = null;
    const request = store.get(id);
    request.onsuccess = () => {
      const previous = request.result;
      if (!previous) return;
      value = { ...previous, ...patch, id: previous.id, updated: Date.now() };
      if (value.kind === "pin") value.view = normalizePinView({ ...previous.view, ...patch.view });
      store.put(value);
    };
    tx.oncomplete = () => { if (value) notifyCapture({ kind: value.kind, id }); resolve(value); };
    tx.onerror = () => reject(new Error("更新截图记录失败，当前编辑内容仍保留"));
    tx.onabort = () => reject(new Error("截图记录更新已取消，当前编辑内容仍保留"));
  });
}
export async function removeCaptureRecord(id) {
  const db = await openLibrary();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("records", "readwrite");
    tx.objectStore("records").delete(id);
    tx.oncomplete = () => { notifyCapture({ kind: "delete", id }); resolve(); };
    tx.onerror = () => reject(new Error("无法删除截图记录"));
  });
}
export async function writeCaptureClipboard(dataUrl) {
  if (!navigator.clipboard?.write || !globalThis.ClipboardItem) throw new Error("当前浏览器不支持图片复制，请使用 HTTPS 或保存图片");
  const blob = await fetch(dataUrl).then(response => response.blob());
  await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
}
export function downloadCapture(dataUrl, fileName = `XChat-${Date.now()}.png`) {
  const link = document.createElement("a");
  link.href = dataUrl;
  link.download = fileName;
  link.click();
  return { downloaded: true };
}
