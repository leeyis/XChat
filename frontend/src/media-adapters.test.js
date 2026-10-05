import test from "node:test";
import assert from "node:assert/strict";
import { HttpWsAdapter, TauriAdapter, fileKind, runtimeCapabilities } from "./xchat.js";

test("media sources use metadata endpoints and leave bytes to native players", async () => {
  const source = { url: "/api/media/27", mime_type: "video/mp4", file_name: "clip.mp4", file_size: 8_000_000_000 };
  const calls = [];
  const desktop = new TauriAdapter({ core: { invoke: async (...args) => { calls.push(args); return source; } } });
  const web = new HttpWsAdapter();
  web.request = async (...args) => { calls.push(args); return source; };
  assert.equal(await desktop.getMessageMediaSource(27), source);
  assert.equal(await web.getMessageMediaSource(27), source);
  assert.deepEqual(calls, [["get_workspace_media_source", { messageId: 27 }], ["/api/media-source/27"]]);
});

test("desktop GIF, WebP and APNG staging retains original data and filenames", async () => {
  const originalReader = globalThis.FileReader;
  const originalBitmap = globalThis.createImageBitmap;
  const calls = [];
  globalThis.FileReader = class {
    readAsDataURL(file) { this.result = file.data; queueMicrotask(() => this.onload()); }
  };
  globalThis.createImageBitmap = () => { throw new Error("Animated originals must not be rasterized"); };
  const desktop = new TauriAdapter({ core: { invoke: async (command, payload) => {
    calls.push({ command, payload });
    return { file_name: payload.fileName, file_path: "staged/original", file_size: 6 };
  } } });
  try {
    for (const [name, type] of [["motion.gif", "image/gif"], ["motion.webp", "image/webp"], ["motion.apng", "image/apng"], ["motion.png", "image/png"]]) {
      const data = `data:${type};base64,AQIDBAUG`;
      const result = await desktop.stageImage({ name, type, size: 6, data });
      assert.equal(result.file_name, name);
      assert.equal(result.preview_url, data);
      assert.deepEqual(calls.at(-1), { command: "stage_image_attachment", payload: { dataUrl: data, fileName: name } });
      assert.equal(fileKind({ file_name: name }), "image");
    }
    await desktop.stageImage({ name: "motion.apng", type: "", size: 6, data: "data:application/octet-stream;base64,AQIDBAUG" });
    assert.equal(calls.at(-1).payload.dataUrl, "data:image/apng;base64,AQIDBAUG");
  } finally {
    globalThis.FileReader = originalReader;
    globalThis.createImageBitmap = originalBitmap;
  }
});

test("Android retains system file opening while unavailable Save As stays hidden", () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "Android" } });
  try {
    const android = runtimeCapabilities("tauri", { nativeFilePicker: false, saveFileAs: true });
    assert.equal(android.nativeFileOpen, true);
    assert.equal(android.nativeFilePicker, false);
    assert.equal(android.saveFileAs, false);
    const web = runtimeCapabilities("web");
    assert.equal(web.nativeFileOpen, false);
    assert.equal(web.saveFileAs, false);
  } finally {
    if (descriptor) Object.defineProperty(globalThis, "navigator", descriptor);
    else delete globalThis.navigator;
  }
});

test("unsupported static image staging keeps the existing PNG conversion", async () => {
  const originalBitmap = globalThis.createImageBitmap;
  const originalDocument = globalThis.document;
  let closed = false, draw = false, invocation;
  globalThis.createImageBitmap = async () => ({ width: 20, height: 10, close() { closed = true; } });
  globalThis.document = { createElement: () => ({ getContext: () => ({ drawImage() { draw = true; } }), toDataURL: () => "data:image/png;base64,AQID" }) };
  const desktop = new TauriAdapter({ core: { invoke: async (command, payload) => { invocation = { command, payload }; return { file_name: payload.fileName }; } } });
  try {
    await desktop.stageImage({ name: "diagram.svg", type: "image/svg+xml", size: 3 });
    assert.equal(draw, true);
    assert.equal(closed, true);
    assert.deepEqual(invocation, { command: "stage_image_attachment", payload: { dataUrl: "data:image/png;base64,AQID", fileName: "diagram.png" } });
  } finally {
    globalThis.createImageBitmap = originalBitmap;
    globalThis.document = originalDocument;
  }
});
